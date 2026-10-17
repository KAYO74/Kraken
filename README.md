# KRAKEN

Messagerie libre et auto-hébergeable, écrite en Rust : serveurs, salons, MP, et à terme vocal, vidéo et partage d'écran. Tout est gratuit, y compris ce que d'autres font payer.

État : tranche verticale (comptes, serveurs, invitations, messages en direct). Voir `replica/build-log.md`.

## Lancer en local

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
./scripts/build-client.sh             # client web dans crates/kraken-client/dist
cargo run -p kraken-server -- --seed  # http://127.0.0.1:8080, compte demo / kraken-demo
```

Variables : `KRAKEN_ADDR` (défaut `127.0.0.1:8080`), `KRAKEN_WEB_DIR`, `KRAKEN_SEED=1`.

## Tests

```sh
cargo test --workspace --exclude kraken-client
NODE_PATH=$(npm root -g) node e2e/slice.cjs   # serveur lancé, Playwright installé
```

## Dépôt

| chemin | rôle |
| --- | --- |
| `crates/kraken-proto` | types REST et gateway partagés |
| `crates/kraken-core` | moteur de permissions |
| `crates/kraken-db` | couche données |
| `crates/kraken-server` | REST `/api/v1`, gateway `/gateway`, client web |
| `crates/kraken-client` | client Leptos (WASM) |
| `replica/` | recon, architecture, design, journal de construction |

Licence proposée : AGPL-3.0-or-later.
