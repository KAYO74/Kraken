//! Serveur KRAKEN : un binaire, REST sous `/api/v1`, gateway sur `/gateway`, tâches de fond,
//! client web à la racine. Données dans PostgreSQL (`kraken-db`).

mod api;
mod auth;
mod error;
mod gateway;
mod jobs;
mod limit;

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::{get, post};
use kraken_db::Db;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

pub use gateway::Hub;
pub use jobs::Mailer;

/// Mot de passe des comptes de démonstration créés par `--seed`.
pub const DEMO_PASSWORD: &str = "kraken-demo";

/// Réglages lus dans l'environnement (voir `.env.example`).
#[derive(Clone)]
pub struct Config {
    pub addr: String,
    pub web_dir: String,
    pub database_url: String,
    /// Adresse publique de l'instance, pour les liens des e-mails ; `https://` active les
    /// cookies `Secure`.
    pub public_url: String,
    /// `smtps://user:pass@hôte:465` ou `smtp://…?tls=required` ; absent : pas d'e-mail.
    pub smtp_url: Option<String>,
    pub smtp_from: String,
    pub seed: bool,
    /// Derrière un proxy : l'IP du client vient de `X-Forwarded-For`.
    pub trust_proxy: bool,
    /// Essais par minute et par IP sur les routes de connexion.
    pub auth_per_min: u32,
}

impl Config {
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let addr = var("KRAKEN_ADDR").unwrap_or_else(|| "127.0.0.1:8080".into());
        Self {
            public_url: var("KRAKEN_PUBLIC_URL").unwrap_or_else(|| format!("http://{addr}")),
            addr,
            web_dir: var("KRAKEN_WEB_DIR").unwrap_or_else(|| "crates/kraken-client/dist".into()),
            database_url: var("DATABASE_URL").unwrap_or_else(|| "postgres://kraken:kraken@127.0.0.1/kraken".into()),
            smtp_url: var("KRAKEN_SMTP_URL"),
            smtp_from: var("KRAKEN_SMTP_FROM").unwrap_or_else(|| "KRAKEN <no-reply@localhost>".into()),
            seed: std::env::args().any(|a| a == "--seed") || var("KRAKEN_SEED").as_deref() == Some("1"),
            trust_proxy: var("KRAKEN_TRUST_PROXY").as_deref() == Some("1"),
            auth_per_min: var("KRAKEN_AUTH_PER_MIN").and_then(|v| v.parse().ok()).unwrap_or(20),
        }
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub hub: Arc<Hub>,
    pub cfg: Arc<Config>,
    pub mailer: Option<Arc<Mailer>>,
}

impl AppState {
    /// Connexion, migrations, données de démo si demandées.
    pub async fn new(cfg: Config) -> Result<Self, String> {
        let db = Db::connect(&cfg.database_url).await.map_err(|e| format!("base injoignable : {e:?}"))?;
        db.migrate().await.map_err(|e| format!("migrations : {e:?}"))?;
        Self::with_db(db, cfg).await
    }

    pub async fn with_db(db: Db, cfg: Config) -> Result<Self, String> {
        let mailer = match &cfg.smtp_url {
            Some(url) => Some(Arc::new(Mailer::new(url, &cfg.smtp_from)?)),
            None => None,
        };
        let hub = Hub::default();
        if cfg.seed {
            let hash = auth::hash_password(DEMO_PASSWORD.into()).await.ok_or("argon2")?;
            if db.seed(&hash).await.map_err(|e| format!("seed : {e:?}"))? {
                tracing::info!("données de démo chargées : compte `demo`, mot de passe `{DEMO_PASSWORD}`");
            }
            hub.seed_presence(&db).await;
        }
        Ok(Self { db, hub: Arc::new(hub), cfg: Arc::new(cfg), mailer })
    }
}

pub fn app(state: AppState) -> Router {
    let limited = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/verify-email", post(auth::verify_email))
        .route("/auth/reset", post(auth::reset))
        .route("/auth/reset/confirm", post(auth::reset_confirm))
        .route_layer(axum::middleware::from_fn_with_state(limit::Limiter::new(&state.cfg), limit::check));
    let api = Router::new()
        .merge(limited)
        .route("/auth/logout", post(auth::logout))
        .route("/auth/logout-all", post(auth::logout_all))
        .route("/users/@me", get(auth::me).patch(auth::update_me))
        .route("/users/@me/delete", post(auth::delete_account))
        .route("/users/@me/relationships", get(api::relationships))
        .route("/users/@me/channels", get(api::private_channels))
        .route("/guilds", post(api::create_guild))
        .route("/guilds/{id}", get(api::guild))
        .route("/guilds/{id}/channels", get(api::guild_channels).post(api::create_channel))
        .route("/channels/{id}/messages", get(api::messages).post(api::create_message))
        .route("/channels/{id}/invites", post(api::create_invite))
        .route("/invites/{code}", get(api::invite).post(api::join));
    let index = format!("{}/index.html", state.cfg.web_dir);
    Router::new()
        .nest("/api/v1", api)
        .route("/gateway", get(gateway::upgrade))
        .fallback_service(ServeDir::new(&state.cfg.web_dir).fallback(ServeFile::new(index)))
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

/// Lance les tâches de fond (e-mails, purge des comptes supprimés).
pub fn spawn_workers(state: &AppState) {
    tokio::spawn(jobs::run(state.clone()));
}
