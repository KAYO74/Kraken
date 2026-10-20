# Déploiement : KRAKEN

Date : 2026-10-19  Commit : voir la PR #4  Feu vert de Fred : pas encore

Hébergement retenu : un serveur Linux (VPS) avec `docker compose` : Postgres 18, KRAKEN, Caddy pour le HTTPS automatique, sauvegarde quotidienne. Vercel et équivalents ne conviennent pas : binaire Rust, WebSocket permanent, base PostgreSQL.

## Préalables (tous obligatoires)

- [x] e2e verts : 50 / 50 (Playwright, Chromium), plus 18 tests Rust
- [x] aucun bug S1 ou S2 ouvert (`replica/bugs.md` : tous les S1 et S2 corrigés)
- [ ] **parité : 2 fonctions indispensables sur 40, score 14,2 / 100** : bloquant. Manquent entre autres le vocal, la modération, les notifications, la recherche, les emojis (`replica/parity.md`).
- [x] rebranding propre : `sweep.py . --config replica/brand.json` sort 0 (nom, domaines et couleurs de l'original absents du code)
- [ ] fiche des magasins d'applications : sans objet tant qu'il n'y a pas d'appli mobile
- [x] build de production : image Docker construite par la CI, client en `release`
- [x] Confidentialité (`/privacy`) et Conditions (`/terms`) en ligne ; contact et sous-traitants renseignés par l'exploitant (`KRAKEN_CONTACT`, `KRAKEN_PROCESSORS`)
- [x] suppression de compte : marche, y compris pour un propriétaire de serveur
- [x] favicon, titre, image de partage, icônes : le logo KRAKEN de Fred

Verdict : **pas encore publiable comme service ouvert à tous** (parité). Une instance de test privée, sur invitation, est possible dès maintenant si Fred le décide.

## Production

- [x] base séparée : volume `db` de l'instance, jamais la base de développement
- [x] sauvegardes : service `backup`, `pg_dump` chaque jour dans `backups/`, 14 jours gardés
- [x] migrations au démarrage du serveur, pas à la main
- [x] variables dans `.env` sur le serveur : écrites par `install.sh` (noms dans `.env.example`)
- [x] paiements : aucun (tout est gratuit), pas de Stripe
- [x] OAuth : aucun pour l'instant
- [ ] domaine d'envoi des e-mails vérifié chez le fournisseur SMTP de Fred (sinon pas de mot de passe oublié)

## Domaine (une fois acheté par Fred)

| enregistrement | nom | valeur |
| --- | --- | --- |
| A | `@` ou le sous-domaine choisi (ex. `chat`) | IPv4 du serveur |
| AAAA | idem | IPv6 du serveur, s'il en a une |
| TXT | `@` | SPF donné par le fournisseur d'e-mails |
| TXT ou CNAME | selon le fournisseur | DKIM donné par le fournisseur d'e-mails |
| TXT | `_dmarc` | `v=DMARC1; p=none; rua=mailto:<adresse de Fred>` puis `p=quarantine` |

- [ ] hôte canonique : `KRAKEN_DOMAIN` seul ; un `www` éventuel se redirige dans `deploy/Caddyfile`
- [ ] HTTPS valide : Caddy obtient le certificat au premier démarrage si le DNS pointe déjà sur le serveur

## Mise en route (Fred, sur le serveur)

```sh
curl -fsSL https://raw.githubusercontent.com/KAYO74/Kraken/main/install.sh | sh     # ou | sudo sh
```

Le script (`install.sh`, ou `install.ps1` sous Windows) installe Docker, demande le domaine, écrit `.env` (mode 600, mot de passe aléatoire, jamais réécrit ensuite) dans `/opt/kraken` (root) ou `~/kraken`, télécharge l'image et démarre. Sans domaine : `<ip>.sslip.io` si la machine a une IP publique, y compris derrière le NAT d'AWS, Google Cloud, Azure ou Oracle (reconnus par leurs infos DMI ; l'IP publique est alors demandée à api.ipify.org) ; sinon mode local en HTTP. Relancer la commande = mettre à jour. Deux instances sur une machine : deux dossiers de noms différents (le nom du projet Docker vient du dossier).

Pare-feu : ouvrir 80 et 443 (TCP) et 443 (UDP) seulement, sur la machine (le script le propose avec ufw) et chez l'hébergeur. Le port 8080 n'est pas publié.

Image : la CI publie `ghcr.io/kayo74/kraken:latest` et `:<commit>` (amd64 et arm64) sur `main`, une fois les tests rust, e2e et installateur passés. **Tant que le dépôt est privé, le paquet GHCR l'est aussi** : rends-le public (GitHub, Packages, kraken, Package settings, Change visibility) ou rends le dépôt public, sinon l'installation par `curl | sh` ne peut pas le télécharger. Depuis un clone (`sh install.sh`), le script construit l'image lui-même si besoin (il lui faut `docker buildx`, qu'il installe quand il peut).

## Appli de bureau

- « Héberger sur cet ordinateur » lance `kraken-server` dans l'appli avec un PostgreSQL 18.4 embarqué (binaires zonky de Maven Central, SHA-256 vérifié à la construction ; `KRAKEN_PG_ARCHIVE` pour construire hors ligne). Données dans `org.kraken.desktop/server` du dossier de données local de l'appli ; PostgreSQL n'écoute que sur 127.0.0.1, KRAKEN sur le réseau local en HTTP (port 8080, ou un autre s'il est pris, retenu ensuite).
- Construire : `scripts/build-client.sh release` d'abord (le client web est embarqué), puis `cd apps/desktop && tauri build`. Sous Windows, il faut les outils C++ de Visual Studio (leurs DLL d'exécution sont livrées avec PostgreSQL) ou `KRAKEN_VC_RUNTIME_DIR`.
- `kraken-desktop --self-test` démarre PostgreSQL et KRAKEN dans un dossier temporaire, vérifie l'API, le client et l'inscription, puis s'arrête (code 0 = OK) ; la CI le lance sur les trois systèmes. PostgreSQL refuse de tourner en root.
- Une version de `apps/desktop/Cargo.toml` qui n'a pas encore de Release est publiée à la fusion dans `main`.

## Surveillance

- [ ] disponibilité : une sonde (ex. UptimeRobot, gratuit) sur `https://<domaine>/api/v1/health`, qui répond `ok` quand serveur et base vont bien, alerte par e-mail à Fred
- [x] journaux : `docker compose logs -f kraken`
- [x] mesure d'audience : aucune, par choix (pas de bandeau cookies)
- [ ] parcours principal fait sur le site en ligne, ordinateur et téléphone

## Première semaine

Les erreurs `base :` dans les journaux, la taille de `backups/`, les refus d'envoi SMTP (`refus SMTP` dans les journaux), et les 429 sur la connexion si une communauté entière arrive derrière une même adresse IP (relever `KRAKEN_AUTH_PER_MIN`).
