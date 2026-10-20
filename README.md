# KRAKEN

Messagerie libre et auto-hébergeable, écrite en Rust : serveurs, salons, MP, et à terme vocal, vidéo et partage d'écran. Tout est gratuit, y compris ce que d'autres font payer.

État : tranche verticale (comptes, serveurs, invitations, messages en direct) sur PostgreSQL. Voir `replica/build-log.md` et `replica/backend.md`.

![Un salon de KRAKEN](docs/screens/salon.png)

| Accueil | Réglages | Sur téléphone |
| --- | --- | --- |
| ![Accueil et amis](docs/screens/accueil.png) | ![Réglages, apparence](docs/screens/reglages.png) | ![Un salon sur téléphone](docs/screens/mobile-salon.png) |

## Télécharger

Appli de bureau pour Windows, macOS et Linux : [dernière version](https://github.com/KAYO74/Kraken/releases/latest). Au premier lancement, deux choix :

- **Rejoindre un serveur** : colle son adresse ou le lien d'invitation qu'on t'a envoyé.
- **Héberger sur cet ordinateur** : un clic, rien d'autre à installer. L'appli lance son propre serveur KRAKEN et affiche l'adresse à donner aux personnes du même Wi-Fi ou du même réseau. Pour les inviter : dans KRAKEN, crée ton serveur avec « + », puis « Inviter ». Ton serveur tourne tant que l'appli est ouverte ; n'héberge que sur un réseau de confiance (chez toi, au travail), jamais sur un Wi-Fi public.

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
| `crates/kraken-client` | client Leptos (WASM) |
| `apps/desktop` | appli de bureau (Tauri) : rejoindre ou héberger, construite par `.github/workflows/desktop.yml` |
| `install.sh`, `install.ps1` | installation en une commande (Linux et macOS, Windows) |
| `deploy/` | configuration de Caddy pour `docker compose` |
| `replica/` | recon, architecture, design, journal de construction |

Licence : [AGPL-3.0-or-later](LICENSE).
