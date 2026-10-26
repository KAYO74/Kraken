//! Salons vocaux côté HTTP : rejoindre (offre WebRTC → réponse), quitter, et la diffusion des
//! états vocaux par le gateway. Le son et l'image passent ailleurs, en UDP (`kraken-voice`).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use kraken_core::perms;
use kraken_db::Db;
use kraken_proto::*;
use kraken_voice::{JoinError, Settings, Sfu};
use tokio::sync::Notify;

use crate::auth::Auth;
use crate::error::{ApiResult, AppError};
use crate::{AppState, Config, Hub};

fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "voice_unavailable",
        "Le vocal n'est pas disponible sur cette instance.".into(),
    )
}

/// `POST /channels/{id}/voice` : rejoindre ce salon vocal (et quitter celui où l'on était).
pub async fn join(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Json(req): Json<VoiceJoin>,
) -> ApiResult<Json<VoiceAnswer>> {
    let uid = auth.user.id;
    let p = state.db.channel_perms(uid, cid).await?;
    if p & perms::VIEW_CHANNEL == 0 {
        return Err(AppError::not_found()); // ne pas révéler l'existence du salon
    }
    let c = state.db.channel(cid).await?.ok_or_else(AppError::not_found)?;
    let gid = c.guild_id.filter(|_| matches!(c.kind, ChannelKind::Voice | ChannelKind::Stage));
    let Some(gid) = gid else { return Err(AppError::invalid("Ce salon n'est pas un salon vocal.")) };
    if p & perms::CONNECT == 0 {
        return Err(AppError::forbidden());
    }
    let Some(sfu) = state.voice.clone() else { return Err(unavailable()) };
    if req.sdp.len() > 64 * 1024 {
        return Err(AppError::invalid("Offre WebRTC trop longue."));
    }
    let j = kraken_voice::Join {
        guild_id: gid,
        channel_id: cid,
        user_id: uid,
        offer: req.sdp,
        mids: req.mids,
        self_mute: req.self_mute,
        self_deaf: req.self_deaf,
        can_speak: p & perms::SPEAK != 0,
        can_stream: p & perms::STREAM != 0,
    };
    // Une clé DTLS par connexion : hors des fils du runtime.
    let joined = tokio::task::spawn_blocking(move || sfu.join(j)).await.map_err(|_| unavailable())?;
    match joined {
        Ok(sdp) => Ok(Json(VoiceAnswer { sdp })),
        Err(JoinError::Full) => Err(AppError(
            StatusCode::CONFLICT,
            "voice_full",
            format!("Ce salon vocal est complet ({MAX_VOICE_MEMBERS} personnes au plus)."),
        )),
        Err(JoinError::BadOffer(e)) => {
            tracing::debug!("vocal {uid} : {e}");
            Err(AppError::invalid("Connexion vocale refusée : offre WebRTC inattendue."))
        }
        Err(JoinError::Stopped) => Err(unavailable()),
    }
}

/// `DELETE /voice` : quitter le vocal. Sans effet si l'on n'y était pas.
pub async fn leave(State(state): State<AppState>, auth: Auth) -> StatusCode {
    if let Some(v) = &state.voice {
        v.leave(auth.user.id);
    }
    StatusCode::NO_CONTENT
}

/// `KRAKEN_RTC_IPS` : adresses séparées par des virgules ; une entrée illisible est ignorée.
pub fn parse_ips(v: &str) -> Vec<IpAddr> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let ip = s.parse().ok();
            if ip.is_none() {
                tracing::warn!("KRAKEN_RTC_IPS : adresse ignorée ({s})");
            }
            ip
        })
        .collect()
}

/// Ouvre le port UDP et lance le SFU. En cas d'échec : un avertissement, et pas de vocal (le
/// reste de l'instance marche).
pub async fn start(cfg: &Config, db: &Db, hub: &Arc<Hub>) -> Option<Arc<Sfu>> {
    let settings = Settings {
        port: cfg.rtc_port,
        fallback: cfg.rtc_fallback,
        ips: announced_ips(cfg).await,
        public_port: cfg.rtc_public_port,
        max_members: MAX_VOICE_MEMBERS,
    };
    match Sfu::start(settings, notifier(db.clone(), hub.clone())) {
        Ok(sfu) => {
            let list: Vec<String> = sfu.candidates().iter().map(|a| a.to_string()).collect();
            tracing::info!("salons vocaux : port UDP {} ; annoncé aux navigateurs : {}", sfu.port(), list.join(", "));
            Some(Arc::new(sfu))
        }
        Err(e) => {
            tracing::warn!("salons vocaux désactivés : port UDP {} indisponible ({e})", cfg.rtc_port);
            None
        }
    }
}

/// Les adresses à annoncer : `KRAKEN_RTC_IPS` s'il est donné ; sinon celles de l'adresse
/// publique (127.0.0.1 si elle est locale), puis celles des interfaces de la machine.
async fn announced_ips(cfg: &Config) -> Vec<IpAddr> {
    if !cfg.rtc_ips.is_empty() {
        return cfg.rtc_ips.clone();
    }
    let host = crate::host(&cfg.public_url).trim_start_matches('[').trim_end_matches(']');
    let resolved: Vec<IpAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) if host.eq_ignore_ascii_case("localhost") => vec![Ipv4Addr::LOCALHOST.into()],
        Err(_) => match tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((host, 0))).await {
            Ok(Ok(addrs)) => addrs.map(|a| a.ip()).collect(),
            _ => {
                tracing::warn!("vocal : {host} ne se résout pas ; seules les adresses locales sont annoncées");
                vec![]
            }
        },
    };
    let mut ips: Vec<IpAddr> = Vec::new();
    for ip in resolved {
        let ip = if ip.is_loopback() { Ipv4Addr::LOCALHOST.into() } else { ip };
        if ip.is_ipv4() && !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    // Les adresses des interfaces seulement pour une instance locale (ou si rien ne se résout) :
    // sinon elles dévoileraient le réseau interne du serveur.
    let public = ips.iter().any(|ip| match ip {
        IpAddr::V4(v4) => !(v4.is_private() || v4.is_loopback() || v4.is_link_local()),
        IpAddr::V6(_) => false,
    });
    for ip in kraken_voice::interface_ips().into_iter().filter(|_| !public) {
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    ips
}

/// Le rappel du SFU : seul le dernier état de chaque personne attend d'être diffusé (une
/// rafale de changements n'en fait qu'un), par une tâche qui les envoie un à un.
fn notifier(db: Db, hub: Arc<Hub>) -> impl Fn(VoiceState) + Send + Sync + 'static {
    let waiting: Arc<Mutex<HashMap<Id, VoiceState>>> = Arc::default();
    let wake = Arc::new(Notify::new());
    let (queue, signal) = (waiting.clone(), wake.clone());
    tokio::spawn(async move {
        // Le dernier salon diffusé de chacun, pour prévenir ceux qui le voyaient quand il le quitte.
        let mut last: HashMap<Id, Id> = HashMap::new();
        loop {
            signal.notified().await;
            let batch: Vec<VoiceState> = queue.lock().unwrap().drain().map(|(_, s)| s).collect();
            for s in batch {
                let before = match s.channel_id {
                    Some(c) => last.insert(s.user_id, c),
                    None => last.remove(&s.user_id),
                };
                if let Err(e) = broadcast(&db, &hub, s, before).await {
                    tracing::warn!("VOICE_STATE_UPDATE : {e:?}");
                }
            }
        }
    });
    move |s| {
        waiting.lock().unwrap().insert(s.user_id, s);
        wake.notify_one();
    }
}

/// À ceux qui voient le salon. Ceux qui voyaient le salon d'avant, mais pas celui-ci, apprennent
/// seulement que la personne l'a quitté ; les autres ne reçoivent rien.
async fn broadcast(db: &Db, hub: &Hub, s: VoiceState, before: Option<Id>) -> Result<(), kraken_db::DbError> {
    let viewers = match s.channel_id {
        Some(c) => db.channel_audience(c).await?,
        None => vec![],
    };
    let left = match before.filter(|b| Some(*b) != s.channel_id) {
        Some(b) => db.channel_audience(b).await?.into_iter().filter(|u| !viewers.contains(u)).collect(),
        None => vec![],
    };
    let hidden = VoiceState { channel_id: None, self_mute: false, self_deaf: false, streaming: false, ..s.clone() };
    hub.send(&viewers, Event::VoiceStateUpdate(s));
    hub.send(&left, Event::VoiceStateUpdate(hidden));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rtc_ips() {
        let ips = super::parse_ips(" 203.0.113.7, ,192.168.1.20,nimporte quoi,2001:db8::1");
        assert_eq!(
            ips.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["203.0.113.7", "192.168.1.20", "2001:db8::1"]
        );
    }
}
