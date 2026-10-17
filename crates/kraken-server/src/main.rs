use std::net::SocketAddr;

use kraken_server::{AppState, Config};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info,kraken_server=debug".into()))
        .init();
    let cfg = Config::from_env();
    let addr: SocketAddr = cfg.addr.parse().expect("KRAKEN_ADDR invalide");
    let state = AppState::new(&cfg).await;
    let app = kraken_server::app(state, &cfg);
    let listener = tokio::net::TcpListener::bind(addr).await.expect("port occupé");
    tracing::info!("KRAKEN écoute sur http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await
        .unwrap();
}
