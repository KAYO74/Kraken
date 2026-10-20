# Image unique de KRAKEN : serveur + client web, pour linux/amd64 et linux/arm64. Construite et
# publiée par la CI ; voir docker-compose.yml. La compilation tourne toujours sur la machine qui
# construit, sans émulation : le serveur est compilé en croisé quand l'architecture diffère.

# Client web (wasm) : le même pour toutes les architectures, donc construit une seule fois.
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS client
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version 0.2.129 --locked
WORKDIR /src
COPY . .
# target/ supprimé dans la même étape : couche (et cache de la CI) bien plus petite.
RUN ./scripts/build-client.sh release && rm -rf target

# Serveur, compilé pour l'architecture visée (TARGETARCH, fixée par docker buildx).
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS server
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
ARG TARGETARCH
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) arch=x86_64; cross="gcc-x86-64-linux-gnu libc6-dev-amd64-cross" ;; \
      arm64) arch=aarch64; cross="gcc-aarch64-linux-gnu libc6-dev-arm64-cross" ;; \
      *) echo "architecture non prise en charge : $TARGETARCH (amd64 ou arm64 seulement)" >&2; exit 1 ;; \
    esac; \
    if [ "$(uname -m)" != "$arch" ]; then \
      apt-get update && apt-get install -y --no-install-recommends $cross && rm -rf /var/lib/apt/lists/*; \
    fi; \
    rustup target add "$arch-unknown-linux-gnu"; \
    echo "$arch-unknown-linux-gnu" > /rust-target
WORKDIR /src
COPY . .
RUN target="$(cat /rust-target)" \
 && cargo build --release -p kraken-server --target "$target" \
 && cp "target/$target/release/kraken-server" /kraken-server \
 && rm -rf target

# Image finale : que des copies (aucune commande à exécuter pour l'architecture visée).
FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/KAYO74/Kraken" \
      org.opencontainers.image.description="KRAKEN, messagerie libre auto-hébergée" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"
COPY --from=server /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=server /kraken-server /usr/local/bin/kraken-server
COPY --from=client /src/crates/kraken-client/dist /srv/kraken/web
ENV KRAKEN_ADDR=0.0.0.0:8080 KRAKEN_WEB_DIR=/srv/kraken/web
# Utilisateur sans droits (pas de useradd : il faudrait exécuter une commande arm64)
USER 10001:10001
EXPOSE 8080
CMD ["kraken-server"]
