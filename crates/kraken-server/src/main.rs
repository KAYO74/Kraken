use std::net::SocketAddr;

use kraken_server::{AppState, Config};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info,kraken_server=debug".into()))
        .init();
    let cfg = Config::from_env();
    let addr: SocketAddr = cfg.addr.parse().expect("KRAKEN_ADDR invalide");
    let state = match AppState::new(cfg).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(1);
        }
    };
    if state.mailer.is_none() {
        tracing::warn!("KRAKEN_SMTP_URL absent : pas de vérification d'e-mail ni de mot de passe oublié");
    }
    kraken_server::spawn_workers(&state);
    let app = kraken_server::app(state);
    let listener = tokio::net::TcpListener::bind(addr).await.expect("port occupé");
    tracing::info!("KRAKEN écoute sur http://{addr}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await
        .unwrap();
}
