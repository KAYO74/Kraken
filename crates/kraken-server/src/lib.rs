//! Serveur KRAKEN : un binaire, REST sous `/api/v1`, gateway sur `/gateway`, tâches de fond,
//! client web à la racine. Données dans PostgreSQL (`kraken-db`).

mod api;
mod auth;
mod error;
mod gateway;
mod jobs;
mod limit;
mod voice;

use std::net::IpAddr;
use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::{delete, get, post};
use kraken_db::Db;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::{SetResponseHeader, SetResponseHeaderLayer};

pub use gateway::Hub;
pub use jobs::Mailer;
pub use kraken_voice::{self, Sfu};

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
    /// Pages Confidentialité et Conditions : contact et sous-traitants de l'exploitant, code source.
    pub instance: kraken_proto::Instance,
    /// Port UDP des salons vocaux, sur toutes les interfaces ; 0 : n'importe quel port libre.
    pub rtc_port: u16,
    /// Port pris : n'importe quel port libre plutôt que pas de vocal (appli de bureau).
    pub rtc_fallback: bool,
    /// Adresses annoncées aux navigateurs pour le vocal. Vide : celles de `public_url`
    /// (127.0.0.1 si elle est locale) et celles des interfaces réseau de la machine.
    pub rtc_ips: Vec<IpAddr>,
    /// Port UDP annoncé s'il diffère de `rtc_port` (redirection de port).
    pub rtc_public_port: Option<u16>,
}

impl Config {
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let addr = var("KRAKEN_ADDR").unwrap_or_else(|| "127.0.0.1:8080".into());
        let public_url = var("KRAKEN_PUBLIC_URL").unwrap_or_else(|| format!("http://{addr}"));
        let smtp_from = var("KRAKEN_SMTP_FROM").unwrap_or_else(|| format!("KRAKEN <no-reply@{}>", host(&public_url)));
        Self {
            public_url,
            addr,
            web_dir: var("KRAKEN_WEB_DIR").unwrap_or_else(|| "crates/kraken-client/dist".into()),
            database_url: var("DATABASE_URL").unwrap_or_else(|| "postgres://kraken:kraken@127.0.0.1/kraken".into()),
            smtp_url: var("KRAKEN_SMTP_URL"),
            smtp_from,
            seed: std::env::args().any(|a| a == "--seed") || var("KRAKEN_SEED").as_deref() == Some("1"),
            trust_proxy: var("KRAKEN_TRUST_PROXY").as_deref() == Some("1"),
            auth_per_min: var("KRAKEN_AUTH_PER_MIN").and_then(|v| v.parse().ok()).unwrap_or(20),
            instance: kraken_proto::Instance {
                contact: var("KRAKEN_CONTACT"),
                processors: var("KRAKEN_PROCESSORS"),
                source_url: var("KRAKEN_SOURCE_URL").unwrap_or_else(|| env!("CARGO_PKG_REPOSITORY").into()),
                public_url: String::new(),
            },
            rtc_port: var("KRAKEN_RTC_PORT").map_or(kraken_voice::DEFAULT_PORT, |v| {
                v.trim().parse().unwrap_or_else(|_| {
                    tracing::warn!("KRAKEN_RTC_PORT invalide ({v}) : {}", kraken_voice::DEFAULT_PORT);
                    kraken_voice::DEFAULT_PORT
                })
            }),
            rtc_fallback: false,
            rtc_ips: var("KRAKEN_RTC_IPS").map(|v| voice::parse_ips(&v)).unwrap_or_default(),
            rtc_public_port: var("KRAKEN_RTC_PUBLIC_PORT").and_then(|v| v.trim().parse().ok()).filter(|p| *p != 0),
        }
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

/// Le domaine d'une adresse, sans schéma, port ni chemin.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split('/').next().unwrap_or(rest);
    authority.rsplit_once(':').filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit())).map_or(authority, |(h, _)| h)
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub hub: Arc<Hub>,
    pub cfg: Arc<Config>,
    pub mailer: Option<Arc<Mailer>>,
    /// Salons vocaux ; `None` si le port UDP n'a pas pu être ouvert.
    pub voice: Option<Arc<Sfu>>,
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
        let hub = Arc::new(hub);
        let voice = voice::start(&cfg, &db, &hub).await;
        Ok(Self { db, hub, cfg: Arc::new(cfg), mailer, voice })
    }

    /// Ajoute à ce serveur ses états vocaux, pour les salons que le membre voit.
    pub fn with_voice(&self, mut g: kraken_proto::GuildState) -> kraken_proto::GuildState {
        if let Some(v) = &self.voice {
            let visible = |c: Option<kraken_proto::Id>| c.is_some_and(|c| g.channels.iter().any(|ch| ch.id == c));
            g.voice_states = v.states(g.guild.id).into_iter().filter(|s| visible(s.channel_id)).collect();
        }
        g
    }

    /// Coupe tout ce que cet utilisateur a d'ouvert : gateway et vocal.
    pub fn kick(&self, uid: kraken_proto::Id) {
        self.hub.kick(uid);
        if let Some(v) = &self.voice {
            v.leave(uid);
        }
    }
}

pub fn app(state: AppState) -> Router {
    let limited = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/verify-email", post(auth::verify_email))
        .route("/auth/reset", post(auth::reset))
        .route("/auth/reset/confirm", post(auth::reset_confirm))
        .route("/users/@me/delete", post(auth::delete_account))
        .route_layer(axum::middleware::from_fn_with_state(limit::Limiter::new(&state.cfg), limit::check));
    // Rejoindre un salon vocal coûte une clé DTLS : même limite, compteur à part.
    let voice_join = Router::new()
        .route("/channels/{id}/voice", post(voice::join))
        .route_layer(axum::middleware::from_fn_with_state(limit::Limiter::new(&state.cfg), limit::check));
    // Réactions : un clic chacune, diffusées à tout le salon ; compteur à part, plus large.
    let reactions = Router::new()
        .route(
            "/channels/{id}/messages/{mid}/reactions/{emoji}/@me",
            axum::routing::put(api::add_reaction).delete(api::remove_reaction),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            limit::Limiter::per_minute(&state.cfg, state.cfg.auth_per_min.max(api::REACTIONS_PER_MIN)),
            limit::check,
        ));
    let api = Router::new()
        .merge(limited)
        .merge(voice_join)
        .merge(reactions)
        .route("/voice", delete(voice::leave))
        .route("/health", get(api::health))
        .route("/instance", get(api::instance))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/logout-all", post(auth::logout_all))
        .route("/users/@me", get(auth::me).patch(auth::update_me))
        .route("/users/@me/relationships", get(api::relationships))
        .route("/users/@me/channels", get(api::private_channels))
        .route("/guilds", post(api::create_guild))
        .route("/guilds/{id}", get(api::guild))
        .route("/guilds/{id}/channels", get(api::guild_channels).post(api::create_channel))
        .route("/channels/{id}/messages", get(api::messages).post(api::create_message))
        .route("/channels/{id}/messages/{mid}", axum::routing::patch(api::edit_message).delete(api::delete_message))
        .route("/channels/{id}/invites", post(api::create_invite))
        .route("/invites/{code}", get(api::invite).post(api::join))
        .fallback(|| async { error::AppError::not_found() });
    let index = format!("{}/index.html", state.cfg.web_dir);
    Router::new()
        .nest("/api/v1", api)
        .route("/gateway", get(gateway::upgrade))
        // Noms de fichiers fixes d'une version à l'autre : le navigateur revalide toujours.
        .fallback_service(SetResponseHeader::if_not_present(
            ServeDir::new(&state.cfg.web_dir).fallback(ServeFile::new(index)),
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
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

#[cfg(test)]
mod tests {
    #[test]
    fn host_of_public_url() {
        assert_eq!(super::host("https://chat.example.org/"), "chat.example.org");
        assert_eq!(super::host("http://127.0.0.1:8080"), "127.0.0.1");
    }
}
