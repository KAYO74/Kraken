//! Limite de débit par IP sur les routes de connexion, d'inscription et d'e-mail.

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::Config;
use crate::error::AppError;

#[derive(Clone)]
pub struct Limiter {
    inner: Arc<DefaultKeyedRateLimiter<String>>,
    trust_proxy: bool,
}

impl Limiter {
    pub fn new(cfg: &Config) -> Self {
        let n = NonZeroU32::new(cfg.auth_per_min.max(1)).unwrap();
        Self { inner: Arc::new(RateLimiter::keyed(Quota::per_minute(n))), trust_proxy: cfg.trust_proxy }
    }
}

pub async fn check(State(l): State<Limiter>, req: Request, next: Next) -> Response {
    let forwarded = l
        .trust_proxy
        .then(|| req.headers().get("x-forwarded-for")?.to_str().ok()?.split(',').next().map(|s| s.trim().to_string()))
        .flatten();
    let ip = forwarded
        .or_else(|| req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string()))
        .unwrap_or_default();
    if l.inner.check_key(&ip).is_err() {
        return AppError(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Trop d'essais. Attends une minute.".into())
            .into_response();
    }
    next.run(req).await
}
