#!/usr/bin/env sh
# Construit le client web dans crates/kraken-client/dist (servi par kraken-server).
# Prérequis : rustup target add wasm32-unknown-unknown ; cargo install wasm-bindgen-cli --version 0.2.129
set -eu
cd "$(dirname "$0")/.."
profile="${1:-release}"
flag=""; [ "$profile" = "release" ] && flag="--release"
cargo build -p kraken-client --target wasm32-unknown-unknown $flag
out=crates/kraken-client/dist
rm -rf "$out" && mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" "target/wasm32-unknown-unknown/$profile/kraken-client.wasm"
cp crates/kraken-client/index.html crates/kraken-client/boot.js crates/kraken-client/style/app.css "$out/"
cp replica/design/tokens.css replica/design/components.css replica/brand/logo.svg "$out/"
echo "client prêt dans $out"
