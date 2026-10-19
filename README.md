# KRAKEN

Messagerie libre et auto-hébergeable, écrite en Rust : serveurs, salons, MP, et à terme vocal, vidéo et partage d'écran. Tout est gratuit, y compris ce que d'autres font payer.

État : tranche verticale (comptes, serveurs, invitations, messages en direct) sur PostgreSQL. Voir `replica/build-log.md` et `replica/backend.md`.

![Un salon de KRAKEN](docs/screens/salon.png)

| Accueil | Réglages | Sur téléphone |
| --- | --- | --- |
| ![Accueil et amis](docs/screens/accueil.png) | ![Réglages, apparence](docs/screens/reglages.png) | ![Un salon sur téléphone](docs/screens/mobile-salon.png) |

## Télécharger

Appli de bureau pour Windows, macOS et Linux : [dernière version](https://github.com/KAYO74/Kraken/releases/latest). Au premier lancement, elle demande l'adresse de ton instance. Sur téléphone, ouvre l'instance dans le navigateur.

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

Il faut un serveur Linux avec Docker, ports 80 et 443 ouverts, et un nom de domaine dont l'enregistrement A pointe sur lui.

```sh
git clone https://github.com/KAYO74/Kraken && cd Kraken
cp .env.example .env    # KRAKEN_DOMAIN, POSTGRES_PASSWORD, KRAKEN_CONTACT, KRAKEN_PROCESSORS
docker compose up -d    # Postgres 18, KRAKEN, Caddy (HTTPS automatique), sauvegarde quotidienne
```

Mise à jour : `git pull && docker compose up -d --build`. Sauvegardes dans `backups/` (14 jours) ; restauration : `docker compose exec -T db pg_restore -U kraken -d kraken --clean < backups/kraken-AAAA-MM-JJ.dump`. Sonde de disponibilité : `https://<domaine>/api/v1/health` répond `ok`. Détails : `replica/deploy.md`.

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
| `apps/desktop` | appli de bureau (Tauri), construite par `.github/workflows/desktop.yml` |
| `deploy/` | configuration de Caddy pour `docker compose` |
| `replica/` | recon, architecture, design, journal de construction |

Licence proposée : AGPL-3.0-or-later.
