//! Non-lus et notifications : marquer un salon ou un serveur comme lu, régler ses
//! notifications. Chaque changement part vers mes autres onglets et appareils par le gateway.

use axum::Json;
use axum::extract::{Path, State};
use kraken_core::perms;
use kraken_proto::*;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiResult, AppError};

/// Salon que je vois, sinon 404 (ne pas révéler qu'il existe).
async fn visible(state: &AppState, uid: Id, cid: Id) -> ApiResult<Channel> {
    if state.db.channel_perms(uid, cid).await? & perms::VIEW_CHANNEL == 0 {
        return Err(AppError::not_found());
    }
    state.db.channel(cid).await?.ok_or_else(AppError::not_found)
}

/// `POST /channels/{id}/ack/{mid}` : j'ai lu ce salon jusqu'à ce message.
pub async fn ack(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, mid)): Path<(Id, Id)>,
) -> ApiResult<Json<ReadState>> {
    let me = auth.user.id;
    let c = visible(&state, me, cid).await?;
    if !c.kind.is_textual() {
        return Err(AppError::not_found());
    }
    let before = state.db.read_state(me, cid).await?;
    let after = state.db.ack(me, cid, mid).await?;
    if after != before {
        state.hub.send(&[me], Event::ReadStateUpdate(vec![after.clone()]));
    }
    Ok(Json(after))
}

/// `POST /guilds/{id}/ack` : tout le serveur lu.
pub async fn ack_guild(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
) -> ApiResult<Json<Vec<ReadState>>> {
    let me = auth.user.id;
    let states = state.db.ack_guild(me, gid).await?.ok_or_else(AppError::not_found)?;
    if !states.is_empty() {
        state.hub.send(&[me], Event::ReadStateUpdate(states.clone()));
    }
    Ok(Json(states))
}

/// `PUT /users/@me/notifications` : niveau, sourdine, `@everyone` ignoré, pour un serveur dont
/// je suis membre ou un salon que je vois.
pub async fn set_notify(
    State(state): State<AppState>,
    auth: Auth,
    Json(req): Json<SetNotify>,
) -> ApiResult<Json<NotifySetting>> {
    let me = auth.user.id;
    match (req.guild_id, req.channel_id) {
        (Some(g), None) => {
            if state.db.guild_perms(me, g).await? == 0 {
                return Err(AppError::not_found());
            }
        }
        (None, Some(c)) => {
            visible(&state, me, c).await?;
        }
        _ => return Err(AppError::invalid("Un serveur ou un salon, l'un des deux.")),
    }
    let s = state.db.set_notify(me, &req).await?;
    state.hub.send(&[me], Event::NotifySettingsUpdate(s.clone()));
    Ok(Json(s))
}

/// Après un nouveau message : l'auteur l'a lu, les mentionnés voient leur compteur avancer.
/// Un échec ici ne fait pas échouer l'envoi.
pub async fn record(state: &AppState, m: &Message, audience: &[Id]) {
    let guild = match state.db.channel(m.channel_id).await {
        Ok(Some(c)) => c.guild_id,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("non-lus de {} : {e:?}", m.channel_id);
            return;
        }
    };
    if let Err(e) = state.db.record_message(m, guild, audience).await {
        tracing::warn!("non-lus de {} : {e:?}", m.channel_id);
    }
}

/// Après une suppression : les compteurs qui la comptaient sont recalculés, et chacun prévenu.
pub async fn deleted(state: &AppState, cid: Id, mid: Id) {
    match state.db.recount_after_delete(cid, mid).await {
        Ok(changed) => {
            for (uid, s) in changed {
                state.hub.send(&[uid], Event::ReadStateUpdate(vec![s]));
            }
        }
        Err(e) => tracing::warn!("non-lus de {cid} : {e:?}"),
    }
}
