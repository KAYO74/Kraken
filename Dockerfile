# Image unique de KRAKEN : serveur + client web. Construite par la CI ; voir docker-compose.yml.
FROM rust:1-bookworm AS build
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version 0.2.129 --locked
WORKDIR /src
COPY . .
RUN ./scripts/build-client.sh release && cargo build --release -p kraken-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 kraken
COPY --from=build /src/target/release/kraken-server /usr/local/bin/kraken-server
COPY --from=build /src/crates/kraken-client/dist /srv/kraken/web
ENV KRAKEN_ADDR=0.0.0.0:8080 KRAKEN_WEB_DIR=/srv/kraken/web
USER kraken
EXPOSE 8080
CMD ["kraken-server"]
