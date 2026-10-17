//! Serveur KRAKEN : un binaire, REST sous `/api/v1`, gateway sur `/gateway`, client web à la racine.

mod api;
mod auth;
mod error;
mod gateway;

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::{get, post};
use kraken_db::Db;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

pub use gateway::Hub;

/// Mot de passe des comptes de démonstration créés par `--seed`.
pub const DEMO_PASSWORD: &str = "kraken-demo";

pub struct Config {
    pub addr: String,
    pub web_dir: String,
    pub seed: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
        Self {
            addr: var("KRAKEN_ADDR", "127.0.0.1:8080"),
            web_dir: var("KRAKEN_WEB_DIR", "crates/kraken-client/dist"),
            seed: std::env::args().any(|a| a == "--seed") || var("KRAKEN_SEED", "0") == "1",
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub hub: Arc<Hub>,
}

impl AppState {
    pub async fn new(cfg: &Config) -> Self {
        let db = Db::new();
        let hub = Hub::default();
        if cfg.seed {
            db.seed(&auth::hash_password(DEMO_PASSWORD.into()).await.expect("argon2"));
            hub.seed_presence(&db).await;
            tracing::info!("données de démo chargées : compte `demo`, mot de passe `{DEMO_PASSWORD}`");
        }
        Self { db: Arc::new(db), hub: Arc::new(hub) }
    }
}

pub fn app(state: AppState, cfg: &Config) -> Router {
    let api = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/users/@me", get(auth::me))
        .route("/users/@me/relationships", get(api::relationships))
        .route("/users/@me/channels", get(api::private_channels))
        .route("/guilds", post(api::create_guild))
        .route("/guilds/{id}", get(api::guild))
        .route("/guilds/{id}/channels", get(api::guild_channels).post(api::create_channel))
        .route("/channels/{id}/messages", get(api::messages).post(api::create_message))
        .route("/channels/{id}/invites", post(api::create_invite))
        .route("/invites/{code}", get(api::invite).post(api::join));
    let index = format!("{}/index.html", cfg.web_dir);
    Router::new()
        .nest("/api/v1", api)
        .route("/gateway", get(gateway::upgrade))
        .fallback_service(ServeDir::new(&cfg.web_dir).fallback(ServeFile::new(index)))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
        .with_state(state)
}
