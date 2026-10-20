#!/bin/sh
# Installe ou met à jour une instance KRAKEN (Linux ou macOS) avec Docker :
#   curl -fsSL https://raw.githubusercontent.com/KAYO74/Kraken/main/install.sh | sh
# ou, depuis un clone du dépôt : sh install.sh. Aide : sh install.sh --help
# Relancer la même commande met à jour l'instance ; .env et les données ne sont jamais touchés.
set -eu

RAW_BASE=https://raw.githubusercontent.com/KAYO74/Kraken
DEFAULT_IMAGE=ghcr.io/kayo74/kraken:latest

say() { printf '%s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
warn() { printf '\nAttention : %s\n' "$*" >&2; }
die() {
	printf '\nErreur : %s\n' "$*" >&2
	exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
	cat <<'EOF'
Installe KRAKEN sur cette machine (Linux ou macOS), ou le met à jour s'il est déjà installé.

  curl -fsSL https://raw.githubusercontent.com/KAYO74/Kraken/main/install.sh | sh
  curl -fsSL https://raw.githubusercontent.com/KAYO74/Kraken/main/install.sh | sh -s -- --domain chat.exemple.org
  sh install.sh            (depuis un clone du dépôt)

Options, et la variable d'environnement qui fait pareil :
  -y, --yes        aucune question, choix par défaut               KRAKEN_YES=1
  --domain NOM     ton nom de domaine, pour le HTTPS               KRAKEN_DOMAIN=NOM
  --local          instance pour ton réseau local, en HTTP         KRAKEN_LOCAL=1
  --dir DOSSIER    dossier d'installation                          KRAKEN_DIR=DOSSIER
                   (défaut : /opt/kraken en root sur Linux, sinon ~/kraken ;
                   depuis un clone du dépôt : le clone lui-même)
  --port PORT      port HTTP en mode local (défaut 80, ou 8080 s'il est pris)
                                                                   KRAKEN_HTTP_PORT=PORT
  -h, --help       cette aide

Autres variables : KRAKEN_REF (branche ou tag des fichiers téléchargés, défaut main),
KRAKEN_IMAGE (image Docker, défaut ghcr.io/kayo74/kraken:latest), KRAKEN_HTTPS_PORT.

Sans nom de domaine : une machine qui a une adresse IP publique (un serveur loué) reçoit
une adresse en <ip>.sslip.io, avec un vrai HTTPS ; sinon (ordinateur derrière une box),
KRAKEN marche pour les personnes connectées au même réseau.
EOF
}

# --- Réseau --------------------------------------------------------------------------------

valid_ipv4() {
	case $1 in '' | *[!0-9.]* | .* | *. | *..*) return 1 ;; esac
	ip_old_ifs=$IFS
	IFS=.
	# shellcheck disable=SC2086 # découpage voulu sur les points
	set -- $1
	IFS=$ip_old_ifs
	[ "$#" -eq 4 ] || return 1
	for ip_part in "$@"; do
		if [ "${#ip_part}" -gt 3 ] || [ "$ip_part" -gt 255 ]; then return 1; fi
	done
	return 0
}

# Adresse joignable depuis Internet (pas privée, pas réservée).
is_public_ipv4() {
	valid_ipv4 "$1" || return 1
	case $1 in
	0.* | 10.* | 127.* | 169.254.* | 192.168.* | 255.*) return 1 ;;
	172.1[6-9].* | 172.2[0-9].* | 172.3[01].*) return 1 ;;
	100.6[4-9].* | 100.[7-9][0-9].* | 100.1[01][0-9].* | 100.12[0-7].*) return 1 ;;
	192.0.0.* | 192.0.2.* | 198.1[89].* | 198.51.100.* | 203.0.113.*) return 1 ;;
	22[4-9].* | 2[3-5][0-9].*) return 1 ;;
	esac
	return 0
}

# Toutes les adresses IPv4 de cette machine, une par ligne.
local_ipv4s() {
	if [ "$os" = mac ]; then
		ifconfig 2>/dev/null | awk '$1 == "inet" { print $2 }'
	elif have ip; then
		ip -4 -o addr show 2>/dev/null | awk '{ split($4, a, "/"); print a[1] }'
	elif [ -r /proc/net/fib_trie ]; then
		awk '/32 host LOCAL/ { print ip } { ip = $2 }' /proc/net/fib_trie | sort -u
	elif have hostname; then
		hostname -I 2>/dev/null | tr ' ' '\n'
	fi
}

public_ipv4() {
	for pub_candidate in $(local_ipv4s); do
		if is_public_ipv4 "$pub_candidate"; then
			printf '%s\n' "$pub_candidate"
			return 0
		fi
	done
	# Serveur loué chez AWS, Google, Azure, Oracle… : l'adresse publique passe par un NAT.
	if cloud_vm; then
		pub_ext=$(external_ipv4 || true)
		if is_public_ipv4 "$pub_ext"; then
			printf '%s\n' "$pub_ext"
			return 0
		fi
	fi
	return 1
}

# Machine virtuelle d'un grand hébergeur cloud (reconnue à ses infos DMI, comme le fait cloud-init).
cloud_vm() {
	[ "$os" = linux ] && [ -d /sys/class/dmi/id ] || return 1
	cv_info=$(cat /sys/class/dmi/id/sys_vendor /sys/class/dmi/id/product_name \
		/sys/class/dmi/id/chassis_asset_tag /sys/class/dmi/id/bios_version 2>/dev/null) || true
	case $cv_info in
	*"Amazon EC2"* | *amazon* | *"Google Compute Engine"* | *7783-7084-3265-9085-8269-3286-77* | \
		*OracleCloud.com* | *"Alibaba Cloud"* | *"Tencent Cloud"* | *Scaleway* | *HUAWEICLOUD*) return 0 ;;
	esac
	return 1
}

# Adresse de cette machine sur le réseau local (celle de la route vers Internet).
lan_ipv4() {
	lan_found=
	if [ "$os" = mac ]; then
		lan_iface=$(route -n get default 2>/dev/null | awk '$1 == "interface:" { print $2 }')
		if [ -n "$lan_iface" ]; then lan_found=$(ipconfig getifaddr "$lan_iface" 2>/dev/null || true); fi
	elif have ip; then
		lan_found=$(ip -4 route get 1.1.1.1 2>/dev/null |
			awk '{ for (i = 1; i < NF; i++) if ($i == "src") { print $(i + 1); exit } }')
	fi
	if ! valid_ipv4 "$lan_found"; then
		# Repli : la première adresse qui n'est ni locale ni un réseau de Docker.
		lan_found=
		for lan_candidate in $(local_ipv4s); do
			case $lan_candidate in
			127.* | 169.254.* | 172.1[6-9].* | 172.2[0-9].* | 172.3[01].*) ;;
			*) if valid_ipv4 "$lan_candidate"; then
				lan_found=$lan_candidate
				break
			fi ;;
			esac
		done
	fi
	if [ -z "$lan_found" ]; then return 1; fi
	printf '%s\n' "$lan_found"
}

# Adresses IPv4 vers lesquelles pointe un nom de domaine.
resolve_ipv4() {
	if have getent; then
		getent ahostsv4 "$1" 2>/dev/null | awk '{ print $1 }' | sort -u
	elif have dscacheutil; then
		dscacheutil -q host -a name "$1" 2>/dev/null | awk '$1 == "ip_address:" { print $2 }' | sort -u
	elif have dig; then
		dig +short A "$1" 2>/dev/null
	elif have host; then
		host -t A "$1" 2>/dev/null | awk '/has address/ { print $NF }'
	fi
}

http_text() {
	if have curl; then
		curl -fsS -4 -m 5 "$1"
	else
		wget -q -T 5 -O - "$1"
	fi
}

http_ok() {
	if have curl; then
		curl -fsS -m 5 -o /dev/null "$1" 2>/dev/null
	else
		wget -q -T 5 -O /dev/null "$1" 2>/dev/null
	fi
}

fetch() {
	if have curl; then
		curl -fsSL --retry 2 -o "$2" "$1"
	elif have wget; then
		wget -q -O "$2" "$1"
	else
		die "il faut curl ou wget pour télécharger les fichiers de KRAKEN."
	fi
}

# Adresse publique vue d'Internet (derrière une box, ce n'est pas celle de la machine).
external_ipv4() {
	for ext_url in https://api.ipify.org https://ipv4.icanhazip.com; do
		ext_ip=$(http_text "$ext_url" 2>/dev/null | tr -d ' \r\n' || true)
		if valid_ipv4 "$ext_ip"; then
			printf '%s\n' "$ext_ip"
			return 0
		fi
	done
	return 1
}

# Port déjà ouvert par un autre programme ? ($1 : port, $2 : tcp ou udp)
port_busy() {
	if [ "$os" = linux ] && [ -r /proc/net/tcp ]; then
		port_hex=$(printf '%04X' "$1")
		if [ "$2" = udp ]; then port_state=07; else port_state=0A; fi
		port_files=
		for port_file in "/proc/net/$2" "/proc/net/${2}6"; do
			if [ -r "$port_file" ]; then port_files="$port_files $port_file"; fi
		done
		# shellcheck disable=SC2086 # liste de fichiers sans espaces
		awk -v p=":$port_hex" -v st="$port_state" \
			'FNR > 1 && $4 == st && substr($2, length($2) - 4) == p { f = 1 } END { exit !f }' $port_files
	elif have netstat; then
		netstat -an -p "$2" 2>/dev/null | awk -v p="$1" -v proto="$2" '
			{ n = split($4, a, /[.:]/); if (a[n] == p && (proto == "udp" || $NF == "LISTEN")) f = 1 }
			END { exit !f }'
	else
		return 1
	fi
}

# Premier port libre à partir de $1.
free_port() {
	free_p=$1
	while port_busy "$free_p" tcp; do
		free_p=$((free_p + 1))
		if [ "$free_p" -ge $(($1 + 100)) ]; then die "aucun port libre trouvé à partir de $1."; fi
	done
	printf '%s\n' "$free_p"
}

valid_port() {
	case $1 in '' | *[!0-9]*) return 1 ;; esac
	[ "${#1}" -le 5 ] && [ "$1" -ge 1 ] && [ "$1" -le 65535 ]
}

# --- Domaine -------------------------------------------------------------------------------

# En minuscules, sans https:// ni / final (on accepte une adresse collée depuis le navigateur).
normalize_domain() {
	nd=$(printf '%s' "$1" | tr -d ' \t\r' | tr '[:upper:]' '[:lower:]')
	nd=${nd#http://}
	nd=${nd#https://}
	nd=${nd%%/*}
	nd=${nd%.}
	printf '%s' "$nd"
}

valid_domain() {
	case $1 in '' | *[!a-z0-9.-]* | .* | *. | *..*) return 1 ;; esac
	[ "${#1}" -le 253 ] || return 1
	case $1 in *.*) ;; *) return 1 ;; esac
	vd_old_ifs=$IFS
	IFS=.
	# shellcheck disable=SC2086 # découpage voulu sur les points
	set -- $1
	IFS=$vd_old_ifs
	for vd_label in "$@"; do
		case $vd_label in '' | -* | *-) return 1 ;; esac
		[ "${#vd_label}" -le 63 ] || return 1
		vd_tld=$vd_label
	done
	# Dernière partie : des lettres (com, org, fr…) ou un domaine international (xn--…).
	case $vd_tld in xn--*) return 0 ;; *[!a-z]* | ?) return 1 ;; esac
	return 0
}

sslip_name() { printf '%s.sslip.io' "$(printf '%s' "$1" | tr . -)"; }

# Prévient si le domaine ne pointe pas (encore) sur cette machine.
check_dns() {
	dns_ips=$(resolve_ipv4 "$domain" | tr '\n' ' ' | sed 's/ *$//')
	dns_ext=$(external_ipv4 || true)
	if [ -z "$dns_ips" ]; then
		warn "$domain ne pointe vers aucune adresse pour l'instant. Chez ton fournisseur de domaine, crée un enregistrement A vers l'adresse IP publique de cette machine${dns_ext:+ ($dns_ext)}. Le HTTPS se mettra en route tout seul dès que ce sera fait."
		return 0
	fi
	for dns_ip in $dns_ips; do
		if [ "$dns_ip" = "$dns_ext" ]; then return 0; fi
		for dns_local in $(local_ipv4s); do
			if [ "$dns_ip" = "$dns_local" ]; then return 0; fi
		done
	done
	if [ -n "$dns_ext" ]; then
		warn "$domain pointe vers $dns_ips, mais l'adresse publique de cette machine est $dns_ext. Corrige l'enregistrement A chez ton fournisseur de domaine : le HTTPS ne marchera qu'ensuite."
	else
		warn "$domain pointe vers $dns_ips ; je n'ai pas pu vérifier que c'est bien cette machine."
	fi
}

# --- Questions -----------------------------------------------------------------------------

# Lit la réponse sur le terminal (le script lui-même arrive souvent par l'entrée standard).
ask() {
	answer=
	if [ -n "$tty_ok" ]; then
		printf '%s' "$1" >/dev/tty
		IFS= read -r answer </dev/tty || answer=
	fi
}

# Oui par défaut, et sans terminal ou avec --yes.
confirm() {
	[ -n "$tty_ok" ] || return 0
	ask "$1 [O/n] "
	case $answer in [nN]*) return 1 ;; *) return 0 ;; esac
}

# --- Docker --------------------------------------------------------------------------------

# Docker répond-il, directement ou avec sudo (Linux seulement : Docker Desktop n'en a pas besoin) ?
docker_ready() {
	if docker info >/dev/null 2>&1; then
		docker_sudo=
		return 0
	fi
	if [ "$os" = linux ] && [ "$(id -u)" != 0 ] && have sudo && sudo docker info >/dev/null 2>&1; then
		docker_sudo=1
		return 0
	fi
	return 1
}

wait_docker() {
	wd_waited=0
	until docker_ready; do
		if [ "$wd_waited" -ge "$1" ]; then return 1; fi
		sleep 3
		wd_waited=$((wd_waited + 3))
	done
}

dk() {
	if [ -n "$docker_sudo" ]; then
		sudo env "KRAKEN_IMAGE=$image" docker "$@"
	else
		KRAKEN_IMAGE=$image docker "$@"
	fi
}

ensure_docker_linux() {
	if ! have docker; then
		step "Installation de Docker (script officiel get.docker.com)"
		fetch https://get.docker.com "$tmpdir/get-docker.sh" ||
			die "impossible de télécharger le script d'installation de Docker."
		as_root sh "$tmpdir/get-docker.sh" </dev/null ||
			die "l'installation de Docker a échoué. Installe Docker à la main (https://docs.docker.com/engine/install/), puis relance cette commande."
	fi
	if ! docker_ready; then
		if have systemctl; then
			as_root systemctl enable --now docker </dev/null >/dev/null 2>&1 || true
		elif have service; then
			as_root service docker start </dev/null >/dev/null 2>&1 || true
		fi
		wait_docker 90 || die "Docker est installé mais ne répond pas. Regarde ce qui bloque avec : sudo systemctl status docker"
	fi
	if ! dk compose version >/dev/null 2>&1; then
		step "Installation de docker compose"
		if have apt-get; then
			as_root apt-get update -qq </dev/null || true
			as_root apt-get install -y -qq docker-compose-plugin </dev/null ||
				as_root apt-get install -y -qq docker-compose-v2 </dev/null || true
		elif have dnf; then
			as_root dnf install -y docker-compose-plugin </dev/null || true
		elif have yum; then
			as_root yum install -y docker-compose-plugin </dev/null || true
		fi
		dk compose version >/dev/null 2>&1 ||
			die "docker compose est introuvable. Installe le paquet docker-compose-plugin (ou docker-compose-v2), ou remplace le Docker de ta distribution par celui de Docker (https://docs.docker.com/engine/install/), puis relance cette commande."
	fi
}

# Construire depuis le code source demande BuildKit, donc docker buildx.
ensure_buildx() {
	if dk buildx version >/dev/null 2>&1; then return 0; fi
	if [ "$os" = linux ]; then
		step "Installation de docker buildx (pour construire l'image)"
		if have apt-get; then
			as_root apt-get update -qq </dev/null || true
			as_root apt-get install -y -qq docker-buildx-plugin </dev/null ||
				as_root apt-get install -y -qq docker-buildx </dev/null || true
		elif have dnf; then
			as_root dnf install -y docker-buildx-plugin </dev/null || true
		elif have yum; then
			as_root yum install -y docker-buildx-plugin </dev/null || true
		fi
	fi
	dk buildx version >/dev/null 2>&1 ||
		die "pour construire KRAKEN depuis le code source, il faut docker buildx. Installe le paquet docker-buildx-plugin (ou docker-buildx), puis relance cette commande."
}

ensure_docker_mac() {
	PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:/Applications/Docker.app/Contents/Resources/bin"
	export PATH
	if ! have docker; then
		have brew || die "il faut Docker Desktop. Télécharge-le sur https://www.docker.com/products/docker-desktop/ , installe-le et ouvre-le une fois, puis relance cette commande."
		step "Installation de Docker Desktop (Homebrew)"
		brew install --cask docker-desktop </dev/null || brew install --cask docker </dev/null ||
			die "l'installation de Docker Desktop a échoué. Installe-le depuis https://www.docker.com/products/docker-desktop/ , puis relance cette commande."
	fi
	if ! docker_ready; then
		say "Je démarre Docker Desktop. Si une fenêtre s'ouvre, accepte ses conditions d'utilisation."
		open -a Docker >/dev/null 2>&1 || true
		wait_docker 300 || die "Docker Desktop ne répond pas. Ouvre-le (Applications > Docker), attends qu'il soit prêt, puis relance cette commande."
	fi
	dk compose version >/dev/null 2>&1 || die "docker compose est introuvable : mets Docker Desktop à jour, puis relance cette commande."
}

# --- Fichiers de l'instance ----------------------------------------------------------------

# Valeur d'une clé dans .env (le fichier n'est jamais exécuté).
env_get() {
	sed -n "s/^$1=//p" "$dir/.env" | tail -n 1 | tr -d '\r' | sed -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'\$/\1/"
}

# docker-compose.yml, deploy/Caddyfile et .env.example : du clone, sinon de GitHub.
install_files() {
	mkdir -p "$dir/deploy" "$dir/backups"
	chmod 700 "$dir/backups" 2>/dev/null || true
	if [ -n "$src" ] && [ "$(cd "$dir" && pwd -P)" = "$src" ]; then return 0; fi
	for if_name in docker-compose.yml deploy/Caddyfile .env.example; do
		if_new="$dir/$if_name.new"
		if [ -n "$src" ]; then
			cp "$src/$if_name" "$if_new"
		elif ! fetch "$RAW_BASE/$ref/$if_name" "$if_new"; then
			rm -f "$if_new"
			die "impossible de télécharger $if_name ($RAW_BASE/$ref/$if_name). Vérifie ta connexion à Internet, et que le dépôt KAYO74/Kraken est public."
		fi
		[ -s "$if_new" ] || die "$if_name téléchargé est vide."
		if [ -f "$dir/$if_name" ] && ! cmp -s "$if_new" "$dir/$if_name"; then
			cp "$dir/$if_name" "$dir/$if_name.old"
			say "$if_name mis à jour (l'ancienne version reste dans $if_name.old)."
		fi
		# Renommage, comme git pull : Caddy le verra au prochain redémarrage (voir start).
		mv "$if_new" "$dir/$if_name"
	done
}

# Mot de passe aléatoire de la base : 40 lettres et chiffres.
gen_password() {
	gp=$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom 2>/dev/null | head -c 40) || gp=
	if [ "${#gp}" -lt 40 ] && have openssl; then gp=$(openssl rand -hex 20); fi
	printf '%s' "$gp"
}

# Écrit .env à la première installation seulement (jamais d'écrasement).
write_env() {
	[ ! -e "$dir/.env" ] || die "$dir/.env existe déjà : je n'y touche pas."
	we_password=$(gen_password)
	[ "${#we_password}" -ge 32 ] || die "impossible de générer un mot de passe aléatoire."
	we_keys="KRAKEN_HTTP_PORT KRAKEN_HTTPS_PORT POSTGRES_PASSWORD"
	if [ "$mode" = local ]; then we_keys="$we_keys KRAKEN_PUBLIC_URL"; else we_keys="$we_keys KRAKEN_DOMAIN"; fi
	if [ -n "$image_explicit" ]; then we_keys="$we_keys KRAKEN_IMAGE"; fi
	we_tmpl="$dir/.env.example"
	[ -f "$we_tmpl" ] || we_tmpl=/dev/null
	(
		umask 077
		V_KRAKEN_DOMAIN=$domain V_KRAKEN_PUBLIC_URL=$public_url V_KRAKEN_HTTP_PORT=$http_port \
			V_KRAKEN_HTTPS_PORT=$https_port V_POSTGRES_PASSWORD=$we_password V_KRAKEN_IMAGE=$image \
			awk -v keys="$we_keys" -v today="$(date +%F)" '
			BEGIN {
				print "# Réglages de cette instance KRAKEN, écrits par install.sh le " today "."
				print "# À garder secret. Relancer l\047installation ne touche jamais ce fichier."
				print ""
				n = split(keys, k, " ")
				for (i = 1; i <= n; i++) val[k[i]] = ENVIRON["V_" k[i]]
			}
			{
				for (key in val) if (index($0, key "=") == 1) { print key "=" val[key]; done[key] = 1; next }
				print
			}
			END { for (key in val) if (!(key in done)) print key "=" val[key] }
		' "$we_tmpl" >"$dir/.env.tmp"
	)
	chmod 600 "$dir/.env.tmp"
	mv "$dir/.env.tmp" "$dir/.env"
}

# --- Étapes --------------------------------------------------------------------------------

parse_args() {
	while [ "$#" -gt 0 ]; do
		case $1 in
		-y | --yes) yes=1 ;;
		--local) local_mode=1 ;;
		--domain | --dir | --port)
			[ "$#" -ge 2 ] || die "il manque une valeur après $1 (voir --help)."
			case $1 in
			--domain) domain=$2 ;;
			--dir) dir=$2 ;;
			--port) http_port=$2 ;;
			esac
			shift
			;;
		--domain=*) domain=${1#*=} ;;
		--dir=*) dir=${1#*=} ;;
		--port=*) http_port=${1#*=} ;;
		-h | --help)
			usage
			exit 0
			;;
		*) die "option inconnue : $1 (voir --help)." ;;
		esac
		shift
	done
}

detect_platform() {
	case $(uname -s) in
	Linux) os=linux ;;
	Darwin) os=mac ;;
	*) die "système non pris en charge : $(uname -s). Sous Windows, utilise install.ps1 (voir le README)." ;;
	esac
	arch=$(uname -m)
	case $arch in
	x86_64 | amd64) arch=amd64 ;;
	aarch64 | arm64) arch=arm64 ;;
	armv* | arm) die "KRAKEN a besoin d'un système 64 bits, et celui-ci est en 32 bits ($arch). Sur un Raspberry Pi 3, 4 ou 5, installe « Raspberry Pi OS (64-bit) », puis relance cette commande." ;;
	i?86 | x86) die "KRAKEN a besoin d'un système 64 bits, et celui-ci est en 32 bits ($arch)." ;;
	*) die "processeur non pris en charge : $arch (il faut un processeur x86_64 ou ARM 64 bits)." ;;
	esac
	# Noyau 64 bits mais programmes 32 bits (fréquent sur Raspberry Pi) : pareil.
	if [ "$os" = linux ] && have getconf && [ "$(getconf LONG_BIT 2>/dev/null || true)" = 32 ]; then
		die "KRAKEN a besoin d'un système 64 bits, et celui-ci est en 32 bits. Sur un Raspberry Pi, installe « Raspberry Pi OS (64-bit) », puis relance cette commande."
	fi
}

# Lancé depuis un clone du dépôt ? On utilise alors ses fichiers, et son code si besoin.
find_checkout() {
	src=
	case $0 in
	*install.sh)
		fc_dir=$(cd "$(dirname "$0")" 2>/dev/null && pwd -P) || fc_dir=
		if [ -n "$fc_dir" ] && [ -f "$fc_dir/docker-compose.yml" ] && [ -f "$fc_dir/deploy/Caddyfile" ] &&
			[ -f "$fc_dir/Dockerfile" ] && [ -f "$fc_dir/Cargo.toml" ]; then
			src=$fc_dir
		fi
		;;
	esac
}

default_dir() {
	if [ -n "$src" ]; then
		printf '%s\n' "$src"
	elif [ "$os" = linux ] && { [ "$(id -u)" = 0 ] || [ -e /opt/kraken/.env ]; }; then
		# Sans root aussi, si KRAKEN y est déjà (installé avec sudo).
		printf '%s\n' /opt/kraken
	else
		[ -n "${HOME:-}" ] || die "HOME n'est pas défini : indique un dossier avec --dir."
		printf '%s\n' "$HOME/kraken"
	fi
}

# Première installation : la seule question (le domaine), puis le mode et les ports.
configure() {
	if [ -n "$local_mode" ] && [ -n "$domain" ]; then die "choisis --domain ou --local, pas les deux."; fi
	pub=
	if [ -z "$local_mode" ]; then pub=$(public_ipv4 || true); fi
	lan=$(lan_ipv4 || true)
	if [ -z "$local_mode" ] && [ -z "$domain" ] && [ -n "$tty_ok" ]; then
		say ""
		say "Si tu as un nom de domaine (ex. chat.exemple.org), KRAKEN l'utilisera avec le HTTPS."
		if [ -n "$pub" ]; then
			say "Sans domaine, KRAKEN sera à l'adresse https://$(sslip_name "$pub") (HTTPS compris)."
		else
			say "Sans domaine, KRAKEN marchera pour les personnes connectées au même réseau que"
			say "cette machine (Wi-Fi, box), à l'adresse http://${lan:-<adresse de cette machine>}"
		fi
		while :; do
			ask "Ton nom de domaine, ou Entrée pour t'en passer : "
			domain=$(normalize_domain "$answer")
			if [ -z "$domain" ] || valid_domain "$domain"; then break; fi
			say "« $answer » n'est pas un nom de domaine valable (exemple : chat.exemple.org). Réessaie."
		done
	fi
	if [ -n "$domain" ]; then
		domain=$(normalize_domain "$domain")
		valid_domain "$domain" || die "nom de domaine invalide : « $domain » (exemple : chat.exemple.org)."
		mode=domain
	elif [ -n "$pub" ]; then
		mode=sslip
		domain=$(sslip_name "$pub")
	else
		mode=local
	fi

	if [ -n "$http_port" ]; then valid_port "$http_port" || die "port HTTP invalide : $http_port"; fi
	if [ -n "$https_port" ]; then valid_port "$https_port" || die "port HTTPS invalide : $https_port"; fi
	if [ "$mode" = local ]; then
		if [ -z "$http_port" ]; then
			if port_busy 80 tcp; then
				http_port=$(free_port 8080)
				say "Le port 80 est déjà pris sur cette machine : KRAKEN utilisera le port $http_port."
			else
				http_port=80
			fi
		elif port_busy "$http_port" tcp; then
			die "le port $http_port est déjà pris par un autre programme. Choisis-en un autre avec --port."
		fi
		# Le HTTPS ne sert pas en mode local : on laisse le port 443 libre.
		[ -n "$https_port" ] || https_port=$(free_port 8443)
		public_url="http://${lan:-localhost}"
		if [ "$http_port" != 80 ]; then public_url="$public_url:$http_port"; fi
		if [ -z "$lan" ]; then warn "je n'ai pas trouvé l'adresse de cette machine sur ton réseau : KRAKEN ne sera ouvert qu'ici, sur $public_url."; fi
	else
		http_port=${http_port:-80}
		https_port=${https_port:-443}
		for cf_port in "$http_port" "$https_port"; do
			if port_busy "$cf_port" tcp; then
				die "le port $cf_port est déjà pris par un autre programme (un autre serveur web ?). Le HTTPS a besoin des ports 80 et 443 : arrête ce programme, ou installe KRAKEN pour ton réseau local seulement avec --local."
			fi
		done
		public_url="https://$domain"
		if [ "$mode" = domain ]; then check_dns; fi
	fi
}

# Mise à jour : les réglages viennent de .env, jamais des options.
read_settings() {
	rs_domain=$(env_get KRAKEN_DOMAIN)
	if { [ -n "$local_mode" ] && [ -n "$rs_domain" ]; } ||
		{ [ -n "$domain" ] && [ "$(normalize_domain "$domain")" != "$rs_domain" ]; }; then
		warn "KRAKEN est déjà installé dans $dir : je garde les réglages de son fichier .env (--domain et --local sont ignorés). Pour les changer, modifie $dir/.env puis relance."
	fi
	domain=$rs_domain
	public_url=$(env_get KRAKEN_PUBLIC_URL)
	http_port=$(env_get KRAKEN_HTTP_PORT)
	http_port=${http_port:-80}
	https_port=$(env_get KRAKEN_HTTPS_PORT)
	https_port=${https_port:-443}
	if [ -n "$domain" ]; then
		mode=domain
		public_url=${public_url:-https://$domain}
	else
		mode=local
		[ -n "$public_url" ] || die "$dir/.env ne contient ni KRAKEN_DOMAIN ni KRAKEN_PUBLIC_URL : remplis l'un des deux, puis relance."
		rs_host=${public_url#*://}
		rs_host=${rs_host%%[:/]*}
		rs_lan=$(lan_ipv4 || true)
		if [ -n "$rs_lan" ] && valid_ipv4 "$rs_host" && [ "$rs_host" != "$rs_lan" ]; then
			warn "l'adresse de cette machine sur ton réseau a changé ($rs_host, maintenant $rs_lan). Mets la nouvelle dans KRAKEN_PUBLIC_URL, dans $dir/.env, puis relance."
		fi
	fi
	rs_image=$(env_get KRAKEN_IMAGE)
	if [ -z "$image_explicit" ] && [ -n "$rs_image" ]; then
		image=$rs_image
		image_explicit=1
	fi
}

open_firewall() {
	[ "$os" = linux ] || return 0
	have ufw || return 0
	as_root env LC_ALL=C ufw status 2>/dev/null | grep -q "^Status: active" || return 0
	if [ "$mode" = local ]; then
		fw_ports="$http_port/tcp"
	else
		fw_ports="$http_port/tcp $https_port/tcp $https_port/udp"
	fi
	if confirm "Le pare-feu ufw est actif. J'ouvre les ports de KRAKEN ($fw_ports) ?"; then
		for fw_port in $fw_ports; do
			as_root ufw allow "$fw_port" >/dev/null
		done
		say "Ports ouverts dans ufw : $fw_ports"
	fi
}

# Une base existe déjà sous ce nom de projet Docker (nom du dossier) : celle d'une autre instance,
# ou une ancienne sans son .env (autre mot de passe : KRAKEN ne pourrait pas s'y connecter).
check_old_volume() {
	ov_project=$(basename "$dir" | tr '[:upper:]' '[:lower:]' | LC_ALL=C tr -cd 'a-z0-9_-' | sed 's/^[-_]*//')
	dk volume inspect "${ov_project}_db" >/dev/null 2>&1 || return 0
	ov_owner=$(dk ps -a --filter "label=com.docker.compose.project=$ov_project" \
		--format '{{.Label "com.docker.compose.project.working_dir"}}' 2>/dev/null | head -n 1) || ov_owner=
	if [ -n "$ov_owner" ]; then ov_owner=$(cd "$ov_owner" 2>/dev/null && pwd -P) || ov_owner=; fi
	ov_here=$(cd "$dir" 2>/dev/null && pwd -P) || ov_here=$dir
	if [ -n "$ov_owner" ] && [ "$ov_owner" != "$ov_here" ]; then
		die "KRAKEN est déjà installé sur cette machine, dans $ov_owner. Pour le mettre à jour, relance avec --dir $(quote "$ov_owner") (et sudo si tu l'as installé avec sudo). Pour une deuxième instance à côté, choisis un dossier d'un autre nom avec --dir."
	fi
	ov_elsewhere=
	if [ -z "$ov_owner" ]; then
		ov_elsewhere=" Si KRAKEN est installé dans un autre dossier nommé $(basename "$dir"), relance avec --dir et ce dossier."
	fi
	die "une base KRAKEN existe déjà sur cette machine (volume Docker ${ov_project}_db), mais il n'y a pas de .env dans $dir.$ov_elsewhere Si c'est une ancienne installation dont le .env a disparu, remets-le dans $dir ; sinon, en dernier recours, efface cette base (toutes ses données !) avec : $(sudo_word)docker volume rm ${ov_project}_db"
}

prepare_image() {
	step "Téléchargement de KRAKEN ($image)"
	if dk pull "$image" </dev/null; then return 0; fi
	if [ -n "$image_explicit" ] && dk image inspect "$image" >/dev/null 2>&1; then
		say "Téléchargement impossible : j'utilise l'image $image déjà présente sur cette machine."
		return 0
	fi
	if [ -n "$src" ]; then
		step "L'image n'est pas disponible en ligne : je la construis depuis le code source"
		ensure_buildx
		say "Ça prend de 10 à 30 minutes la première fois (bien plus sur un Raspberry Pi)."
		dk buildx build --load -t "$image" "$src" </dev/null ||
			die "la construction de l'image a échoué (voir les messages au-dessus)."
		return 0
	fi
	if dk image inspect "$image" >/dev/null 2>&1; then
		warn "impossible de télécharger la nouvelle version de KRAKEN : je garde celle déjà installée."
		return 0
	fi
	die "impossible de télécharger l'image de KRAKEN ($image). Soit elle n'est pas encore publiée, soit elle est privée. Si tu as accès au dépôt : $(sudo_word)docker login ghcr.io, puis relance cette commande. Sinon, installe depuis le code source : git clone https://github.com/KAYO74/Kraken && cd Kraken && sh install.sh"
}

# Un chemin prêt à copier-coller dans un terminal.
quote() {
	case $1 in
	*[!A-Za-z0-9_./:@%+=,-]*) printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")" ;;
	*) printf '%s' "$1" ;;
	esac
}

# « sudo » devant les commandes à retaper, quand il en faut (Docker par sudo, ou script lancé avec sudo).
sudo_word() { if [ -n "$docker_sudo" ] || [ -n "$by_sudo" ]; then printf 'sudo '; fi; }

compose_cmd() { printf 'cd %s && %sdocker compose' "$(quote "$dir")" "$(sudo_word)"; }

# La commande de mise à jour ($1 : sudo pour la forcer).
update_cmd() {
	uc_sudo=
	if [ -n "$by_sudo" ] || [ "${1:-}" = sudo ]; then uc_sudo='sudo '; fi
	if [ -n "$src" ]; then
		uc="cd $(quote "$src") && git pull && ${uc_sudo}sh install.sh"
	else
		uc="curl -fsSL $RAW_BASE/$ref/install.sh | ${uc_sudo}sh"
	fi
	if [ "$dir" != "$(default_dir)" ]; then
		if [ -n "$src" ]; then uc="$uc --dir"; else uc="$uc -s -- --dir"; fi
		uc="$uc $(quote "$dir")"
	fi
	printf '%s' "$uc"
}

start() {
	step "Démarrage de KRAKEN"
	(cd "$dir" && dk compose pull --quiet db caddy backup </dev/null) ||
		warn "certaines images n'ont pas pu être téléchargées ; j'utilise celles déjà présentes."
	(cd "$dir" && dk compose up -d --remove-orphans </dev/null) ||
		die "docker compose n'a pas pu démarrer KRAKEN. Pour voir pourquoi : $(compose_cmd) logs --tail 100"
	# Caddyfile changé (mise à jour, git pull) : Caddy ne le relit pas tout seul. S'il voit encore
	# l'ancien fichier, il faut le redémarrer ; sinon un rechargement suffit (sans coupure).
	if ! (cd "$dir" && dk compose exec -T caddy cat /etc/caddy/Caddyfile </dev/null 2>/dev/null) |
		cmp -s - "$dir/deploy/Caddyfile"; then
		(cd "$dir" && dk compose restart caddy </dev/null) || true
	elif [ -n "$update" ]; then
		(cd "$dir" && dk compose exec -T caddy caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile \
			</dev/null >/dev/null 2>&1) || warn "Caddy n'a pas pu recharger sa configuration. Pour voir pourquoi : $(compose_cmd) logs --tail 50 caddy"
	fi
}

wait_health() {
	step "Vérification (jusqu'à 4 minutes)"
	wh_waited=0
	until (cd "$dir" && dk compose exec -T caddy wget -q -T 3 -O /dev/null http://kraken:8080/api/v1/health) \
		</dev/null >/dev/null 2>&1; do
		if [ "$wh_waited" -ge 240 ]; then
			die "KRAKEN ne répond pas après 4 minutes. Pour voir ce qui se passe : $(compose_cmd) logs --tail 100 kraken db"
		fi
		sleep 3
		wh_waited=$((wh_waited + 3))
	done
	https_pending=
	if [ "$mode" = local ]; then
		wh_waited=0
		until http_ok "http://127.0.0.1:$http_port/api/v1/health"; do
			if [ "$wh_waited" -ge 60 ]; then
				die "KRAKEN tourne mais ne répond pas sur le port $http_port. Pour voir pourquoi : $(compose_cmd) logs --tail 100 caddy"
			fi
			sleep 3
			wh_waited=$((wh_waited + 3))
		done
	elif have curl; then
		say "Obtention du certificat HTTPS pour $domain…"
		wh_waited=0
		until curl -fsS -m 5 -o /dev/null --resolve "$domain:$https_port:127.0.0.1" \
			"https://$domain:$https_port/api/v1/health" 2>/dev/null; do
			if [ "$wh_waited" -ge 180 ]; then
				https_pending=1
				break
			fi
			sleep 5
			wh_waited=$((wh_waited + 5))
		done
	fi
}

summary() {
	say ""
	say "=================================================================="
	if [ -n "$https_pending" ]; then
		say "KRAKEN tourne, mais son certificat HTTPS n'est pas encore prêt."
		say "Vérifie que $domain pointe bien sur cette machine et que les ports 80 et 443"
		say "sont ouverts (chez ton hébergeur aussi). Caddy réessaie tout seul ; pour suivre :"
		say "  $(compose_cmd) logs -f caddy"
		say "Ensuite, KRAKEN sera ici : $public_url"
	else
		say "KRAKEN est prêt : $public_url"
		if [ "$mode" = local ]; then
			say "Les personnes connectées au même réseau (Wi-Fi, box) l'ouvrent avec cette adresse."
		fi
		say "Ouvre-la dans ton navigateur et crée ton compte."
	fi
	say ""
	say "Mettre à jour : relance la même commande (réglages et données sont gardés) :"
	say "  $(update_cmd)"
	say "Fichiers : $dir (réglages dans .env, à garder secret)"
	say "Sauvegardes : $dir/backups (une par jour, gardées 14 jours)"
	say "Journaux : $(compose_cmd) logs -f"
	say "Arrêter : $(compose_cmd) down    Relancer : $(compose_cmd) up -d"
	say "=================================================================="
}

main() {
	yes=${KRAKEN_YES:-}
	domain=${KRAKEN_DOMAIN:-}
	local_mode=${KRAKEN_LOCAL:-}
	dir=${KRAKEN_DIR:-}
	http_port=${KRAKEN_HTTP_PORT:-}
	https_port=${KRAKEN_HTTPS_PORT:-}
	ref=${KRAKEN_REF:-main}
	image=${KRAKEN_IMAGE:-}
	parse_args "$@"
	case $yes in 1 | true | yes | oui) yes=1 ;; *) yes= ;; esac
	case $local_mode in 1 | true | yes | oui) local_mode=1 ;; *) local_mode= ;; esac
	case $ref in '' | *[!A-Za-z0-9._/-]*) die "KRAKEN_REF invalide : $ref" ;; esac
	image_explicit=
	if [ -n "$image" ]; then
		case $image in *[!A-Za-z0-9._/:@-]*) die "KRAKEN_IMAGE invalide : $image" ;; esac
		image_explicit=1
	else
		image=$DEFAULT_IMAGE
	fi
	# .env est la seule source des réglages : rien de l'environnement ne doit passer devant.
	unset KRAKEN_DOMAIN KRAKEN_PUBLIC_URL KRAKEN_HTTP_PORT KRAKEN_HTTPS_PORT KRAKEN_IMAGE \
		POSTGRES_PASSWORD KRAKEN_SMTP_URL KRAKEN_SMTP_FROM KRAKEN_AUTH_PER_MIN KRAKEN_CONTACT \
		KRAKEN_PROCESSORS KRAKEN_SOURCE_URL KRAKEN_SEED RUST_LOG \
		COMPOSE_FILE COMPOSE_PROJECT_NAME COMPOSE_PROFILES COMPOSE_ENV_FILES

	tty_ok=
	if [ -z "$yes" ] && (exec </dev/tty) 2>/dev/null; then tty_ok=1; fi
	docker_sudo=
	by_sudo=
	if [ "$(id -u)" = 0 ] && [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then by_sudo=1; fi

	detect_platform
	if [ "$(id -u)" = 0 ]; then
		as_root() { "$@"; }
	elif have sudo; then
		as_root() { sudo "$@"; }
	else
		as_root() { die "il faut les droits administrateur (root ou sudo) pour : $*"; }
	fi
	find_checkout
	[ -n "$dir" ] || dir=$(default_dir)
	case $dir in /*) ;; *) dir="$(pwd -P)/$dir" ;; esac
	tmpdir=$(mktemp -d)
	trap 'rm -rf "$tmpdir"' EXIT
	trap 'exit 1' HUP INT TERM

	say "KRAKEN : installation dans $dir ($os, $arch)"
	update=
	if [ -f "$dir/.env" ]; then
		[ -r "$dir/.env" ] || die "KRAKEN est déjà installé dans $dir, mais tu n'as pas le droit de lire son fichier .env (installé avec sudo ?). Relance avec sudo : $(update_cmd sudo)"
		update=1
		say "Déjà installé : mise à jour, en gardant les réglages et les données."
		read_settings
	else
		configure
	fi

	if [ "$os" = mac ]; then ensure_docker_mac; else ensure_docker_linux; fi
	case $(dk info --format '{{.Architecture}}' 2>/dev/null || true) in
	x86_64 | amd64 | aarch64 | arm64 | '') ;;
	*) die "Docker tourne ici en 32 bits : KRAKEN a besoin d'un système 64 bits." ;;
	esac

	if [ -z "$update" ]; then check_old_volume; fi
	mkdir -p "$dir"
	install_files
	# L'image d'abord : si elle manque, rien n'est écrit et la prochaine fois repose la question.
	prepare_image
	if [ -z "$update" ]; then
		write_env
		open_firewall
	fi
	start
	wait_health
	summary
}

main "$@"
