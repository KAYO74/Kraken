<p align="center">
  <img src="docs/images/banniere.png" alt="KRAKEN, messagerie libre, gratuite et auto-hébergeable" width="100%">
</p>

<p align="center">
  <a href="https://github.com/KAYO74/Kraken/releases/latest"><img src="https://img.shields.io/badge/version-0.4.0-2b7a78" alt="Version 0.4.0"></a>
  <a href="https://github.com/KAYO74/Kraken/actions/workflows/ci.yml"><img src="https://github.com/KAYO74/Kraken/actions/workflows/ci.yml/badge.svg" alt="Tests"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/licence-AGPL--3.0-7B61FF" alt="Licence AGPL-3.0"></a>
  <img src="https://img.shields.io/badge/Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-5cc2bd" alt="Windows, macOS, Linux">
  <img src="https://img.shields.io/badge/%C3%A9crit%20en-Rust-CE422B" alt="Écrit en Rust">
</p>

<p align="center">
  <b>KRAKEN</b> est une messagerie libre pour ta communauté : serveurs, salons, messages privés, vocal et partage d'écran.<br>
  Tu l'héberges toi-même, sans pièce d'identité ni numéro de téléphone, et tout est gratuit, y compris ce que d'autres font payer.
</p>

# KRAKEN

![Un salon de KRAKEN](docs/screens/salon.png)

| Accueil | Réglages | Sur téléphone |
| --- | --- | --- |
| ![Accueil et amis](docs/screens/accueil.png) | ![Réglages, apparence](docs/screens/reglages.png) | ![Un salon sur téléphone](docs/screens/mobile-salon.png) |

## Nouveautés

**Salons vocaux et partage d'écran (0.3.0).** Rejoins un salon vocal en un clic, parle, coupe ton micro ou le son, et partage ton écran (avec le son) à tout le salon. Chacun choisit quel écran regarder, en grand ou en plein écran.

![Regarder un écran partagé dans un salon vocal](docs/screens/appel.png)

**Contrôle à distance (0.4.0).** Celui qui regarde ton écran peut demander à le piloter, comme avec un logiciel d'assistance : tu acceptes dans KRAKEN, puis ton ordinateur te redemande l'accord. Sa souris et son clavier agissent alors chez toi. Échap, « Arrêter le contrôle » ou la fin du partage rendent la main à tout moment. Disponible dans l'appli de bureau (sous Linux en session X11 ; sous macOS, autorise KRAKEN dans Accessibilité).

| La demande arrive | Pendant le contrôle |
| --- | --- |
| ![Une demande de contrôle de l'écran](docs/screens/controle-demande.png) | ![Piloter l'écran partagé](docs/screens/controle.png) |

**Une apparence à elle (0.4.0).** Nouvelles tuiles de serveurs, carte « Mon compte », panneau d'appel, textes réécrits, et un logo aux coins arrondis.

> [!TIP]
> **Fait avec l'IA.** KRAKEN est un projet personnel construit avec l'intelligence artificielle : le code, la documentation et les images de cette page ont été écrits par Claude (Anthropic) à partir des demandes de son créateur, pour offrir à tous une messagerie libre et gratuite. Rien n'est repris de logiciels propriétaires : le code, les textes et l'apparence sont originaux, les icônes viennent de [Lucide](https://lucide.dev) (licence ISC) et les polices sont Inter et JetBrains Mono (licence SIL OFL).

## Télécharger

Appli de bureau pour Windows, macOS et Linux : [dernière version](https://github.com/KAYO74/Kraken/releases/latest). Au premier lancement, deux choix :

- **Rejoindre un serveur** : colle son adresse ou le lien d'invitation qu'on t'a envoyé.
- **Héberger sur cet ordinateur** : un clic, rien d'autre à installer. L'appli lance son propre serveur KRAKEN et affiche l'adresse à donner aux personnes du même Wi-Fi ou du même réseau. Pour les inviter : dans KRAKEN, crée ton serveur avec « + », puis « Inviter ». Ton serveur tourne tant que l'appli est ouverte ; n'héberge que sur un réseau de confiance (chez toi, au travail), jamais sur un Wi-Fi public. Pour inviter quelqu'un qui n'est pas chez toi : [Inviter à distance](docs/inviter-a-distance.md) (avec Tailscale, sans rien ouvrir sur ta box).

Le menu KRAKEN permet de changer de serveur et de revoir l'adresse à partager. Sur téléphone, ouvre l'instance dans le navigateur.

## Lancer en local

Il faut Rust, PostgreSQL 16 ou plus, et une base vide.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
./scripts/build-client.sh                     # client web dans crates/kraken-client/dist
export DATABASE_URL=postgres://kraken:kraken@127.0.0.1/kraken
cargo run -p kraken-server -- --seed          # http://127.0.0.1:8080, compte demo / kraken-demo
```

Les migrations s'appliquent au démarrage. Toutes les variables sont dans `.env.example`.

## Héberger son instance

Une seule commande, sur un serveur loué (VPS), un PC, un Mac ou un Raspberry Pi (système 64 bits) :

```sh
curl -fsSL https://raw.githubusercontent.com/KAYO74/Kraken/main/install.sh | sh         # Linux, macOS
```

```powershell
irm https://raw.githubusercontent.com/KAYO74/Kraken/main/install.ps1 | iex              # Windows (PowerShell)
```

Le script installe Docker s'il manque, pose une seule question (ton nom de domaine, facultatif), crée les mots de passe et démarre KRAKEN : PostgreSQL 18, Caddy pour le HTTPS automatique, sauvegarde quotidienne. Sans nom de domaine, un serveur loué reçoit une adresse `<ip>.sslip.io` avec un vrai HTTPS (ouvre aussi les ports 80 et 443 dans le pare-feu de l'hébergeur) ; un ordinateur à la maison sert ton réseau local. `sh install.sh --help` liste les options (`--domain`, `--local`, `--dir`, `--yes`).

Salons vocaux (voix et partage d'écran) : le son et l'image passent en direct par le port UDP 3478 (`KRAKEN_RTC_PORT`), qui doit être joignable par les navigateurs : le script l'ouvre dans ufw, à toi de l'ouvrir chez l'hébergeur ou de le rediriger sur ta box. Pas de serveur relais (TURN) pour l'instant. Parler et partager son écran demandent une instance en HTTPS ; en HTTP simple sur le réseau local, on écoute et on regarde.

Mise à jour : relance la même commande (avec `sudo` si tu as installé avec `sudo`) ; tes réglages et tes données sont gardés. Sauvegardes dans `backups/` du dossier d'installation (14 jours) ; restauration : `docker compose exec -T db pg_restore -U kraken -d kraken --clean < backups/kraken-AAAA-MM-JJ.dump`. Sonde de disponibilité : `https://<domaine>/api/v1/health` répond `ok`.

À la main : `cp .env.example .env`, remplis-le, puis `docker compose up -d`. Détails : `replica/deploy.md`.

## Tests

```sh
cargo test --workspace --exclude kraken-client   # DATABASE_URL vers un Postgres où l'on peut créer des bases
cd e2e && npm ci && npx playwright install chromium
npx playwright test                       # serveur lancé avec KRAKEN_AUTH_PER_MIN=10000
```

## Dépôt

| chemin | rôle |
| --- | --- |
| `crates/kraken-proto` | types REST et gateway partagés |
| `crates/kraken-core` | moteur de permissions |
| `crates/kraken-db` | PostgreSQL : migrations, requêtes, file de tâches |
| `crates/kraken-server` | REST `/api/v1`, gateway `/gateway`, client web |
| `crates/kraken-voice` | salons vocaux : SFU WebRTC (str0m) sur un port UDP |
| `crates/kraken-client` | client Leptos (WASM) |
| `apps/desktop` | appli de bureau (Tauri) : rejoindre ou héberger, construite par `.github/workflows/desktop.yml` |
| `install.sh`, `install.ps1` | installation en une commande (Linux et macOS, Windows) |
| `deploy/` | configuration de Caddy pour `docker compose` |
| `replica/` | recon, architecture, design, journal de construction |

Licence : [AGPL-3.0-or-later](LICENSE).
