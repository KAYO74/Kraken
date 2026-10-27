//! Limite de débit par client sur les routes de connexion, d'inscription, d'e-mail, de
//! suppression de compte, sur les réactions et sur le social (demandes d'ami, blocages,
//! nouveaux MP et groupes).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::Config;
use crate::error::AppError;

#[derive(Clone)]
pub struct Limiter {
    inner: Arc<DefaultKeyedRateLimiter<IpAddr>>,
    trust_proxy: bool,
}

impl Limiter {
    pub fn new(cfg: &Config) -> Self {
        Self::per_minute(cfg, cfg.auth_per_min)
    }

    /// `per_min` essais par minute et par client.
    pub fn per_minute(cfg: &Config, per_min: u32) -> Self {
        let n = NonZeroU32::new(per_min.max(1)).unwrap();
        let inner = Arc::new(RateLimiter::keyed(Quota::per_minute(n)));
        // Les compteurs revenus au plein sont oubliés : la table ne grossit pas sans fin.
        let weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tick.tick().await;
                let Some(l) = weak.upgrade() else { break };
                l.retain_recent();
                l.shrink_to_fit();
            }
        });
        Self { inner, trust_proxy: cfg.trust_proxy }
    }
}

/// Le client : l'IP de la connexion ou, derrière un proxy de confiance, la dernière entrée de
/// `X-Forwarded-For` (celle que le proxy ajoute ; les précédentes viennent du client). En IPv6,
/// un /64 entier compte pour un seul client.
fn client(headers: &HeaderMap, peer: Option<IpAddr>, trust_proxy: bool) -> IpAddr {
    let forwarded = trust_proxy
        .then(|| {
            headers
                .get_all("x-forwarded-for")
                .iter()
                .next_back()?
                .to_str()
                .ok()?
                .rsplit(',')
                .next()?
                .trim()
                .parse()
                .ok()
        })
        .flatten();
    match forwarded.or(peer) {
        Some(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(Ipv6Addr::from_bits(v6.to_bits() & !(u64::MAX as u128))),
        },
        Some(v4) => v4,
        None => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    }
}

pub async fn check(State(l): State<Limiter>, req: Request, next: Next) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    if l.inner.check_key(&client(req.headers(), peer, l.trust_proxy)).is_err() {
        return AppError(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Trop d'essais. Attends une minute.".into())
            .into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xff(lines: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for l in lines {
            h.append("x-forwarded-for", l.parse().unwrap());
        }
        h
    }

    #[test]
    fn client_is_what_the_proxy_saw() {
        let peer = Some("10.0.0.2".parse().unwrap());
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        // L'entrée de gauche est choisie par le client : seule la dernière compte.
        assert_eq!(client(&xff(&["6.6.6.6, 203.0.113.7"]), peer, true), ip("203.0.113.7"));
        assert_eq!(client(&xff(&["6.6.6.6", "203.0.113.7"]), peer, true), ip("203.0.113.7"));
        assert_eq!(client(&xff(&["n'importe quoi"]), peer, true), ip("10.0.0.2"));
        assert_eq!(client(&xff(&["203.0.113.7"]), peer, false), ip("10.0.0.2"), "proxy non déclaré");
        // Tout un /64 IPv6 partage un compteur.
        assert_eq!(client(&xff(&["2001:db8:1:2:aaaa::1"]), peer, true), ip("2001:db8:1:2::"));
        assert_eq!(client(&HeaderMap::new(), Some(ip("::ffff:192.0.2.1")), false), ip("192.0.2.1"));
    }
}
