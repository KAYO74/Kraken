# KRAKEN

Messagerie libre et auto-hébergeable, écrite en Rust : serveurs, salons, MP, et à terme vocal, vidéo et partage d'écran. Tout est gratuit, y compris ce que d'autres font payer.

État : tranche verticale (comptes, serveurs, invitations, messages en direct) sur PostgreSQL. Voir `replica/build-log.md` et `replica/backend.md`.

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

```sh
cp .env.example .env    # mettre au moins POSTGRES_PASSWORD et KRAKEN_PUBLIC_URL
docker compose up -d    # Postgres 18 + KRAKEN sur le port 8080
```

## Tests

```sh
cargo test --workspace --exclude kraken-client   # DATABASE_URL vers un Postgres où l'on peut créer des bases
cd e2e && npm ci && npx playwright test   # serveur lancé avec KRAKEN_AUTH_PER_MIN=10000
```

## Dépôt

| chemin | rôle |
| --- | --- |
| `crates/kraken-proto` | types REST et gateway partagés |
| `crates/kraken-core` | moteur de permissions |
| `crates/kraken-db` | PostgreSQL : migrations, requêtes, file de tâches |
| `crates/kraken-server` | REST `/api/v1`, gateway `/gateway`, client web |
| `crates/kraken-client` | client Leptos (WASM) |
| `replica/` | recon, architecture, design, journal de construction |

Licence proposée : AGPL-3.0-or-later.
