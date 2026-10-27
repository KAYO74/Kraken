//! Serveurs, salons, messages, invitations (amis, MP et groupes : `social`). Chaque handler
//! vérifie les droits (moteur de `kraken-core`) avant de toucher aux données, puis prévient le
//! gateway.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use kraken_core::perms;
use kraken_proto::*;
use serde::Deserialize;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiResult, AppError};

async fn require_channel(state: &AppState, uid: Id, cid: Id, need: u64) -> ApiResult<Channel> {
    channel_perms(state, uid, cid, need).await?;
    state.db.channel(cid).await?.ok_or_else(AppError::not_found)
}

/// Mes droits dans ce salon, s'ils contiennent `need`.
async fn channel_perms(state: &AppState, uid: Id, cid: Id, need: u64) -> ApiResult<u64> {
    let p = state.db.channel_perms(uid, cid).await?;
    if p & perms::VIEW_CHANNEL == 0 {
        return Err(AppError::not_found()); // ne pas révéler l'existence du salon
    }
    if p & need != need {
        return Err(AppError::forbidden());
    }
    Ok(p)
}

/// Réactions ajoutées ou retirées par minute et par client.
pub const REACTIONS_PER_MIN: u32 = 300;

async fn require_guild(state: &AppState, uid: Id, gid: Id, need: u64) -> ApiResult<()> {
    let p = state.db.guild_perms(uid, gid).await?;
    if p == 0 {
        return Err(AppError::not_found());
    }
    if p & need != need {
        return Err(AppError::forbidden());
    }
    Ok(())
}

/// Pour les sondes de disponibilité : 200 si la base répond.
pub async fn health(State(state): State<AppState>) -> ApiResult<&'static str> {
    state.db.ping().await?;
    Ok("ok")
}

pub async fn instance(State(state): State<AppState>) -> Json<Instance> {
    let mut i = state.cfg.instance.clone();
    i.public_url = state.cfg.public_url.trim_end_matches('/').to_string();
    Json(i)
}

pub async fn create_guild(
    State(state): State<AppState>,
    auth: Auth,
    Json(req): Json<CreateGuild>,
) -> ApiResult<(StatusCode, Json<GuildState>)> {
    let g = state.db.create_guild(auth.user.id, &req.name).await?;
    state.hub.send(&[auth.user.id], Event::GuildCreate(Box::new(g.clone())));
    Ok((StatusCode::CREATED, Json(g)))
}

pub async fn guild(State(state): State<AppState>, auth: Auth, Path(gid): Path<Id>) -> ApiResult<Json<GuildState>> {
    let g = state.db.guild_state(gid, auth.user.id).await?.ok_or_else(AppError::not_found)?;
    Ok(Json(state.with_voice(g)))
}

pub async fn guild_channels(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
) -> ApiResult<Json<Vec<Channel>>> {
    let Json(g) = guild(State(state), auth, Path(gid)).await?;
    Ok(Json(g.channels))
}

pub async fn create_channel(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Json(req): Json<CreateChannel>,
) -> ApiResult<(StatusCode, Json<Channel>)> {
    require_guild(&state, auth.user.id, gid, perms::MANAGE_CHANNELS).await?;
    let c = state.db.create_channel(gid, &req).await?;
    let audience = state.db.channel_audience(c.id).await?;
    state.hub.send(&audience, Event::ChannelCreate(c.clone()));
    Ok((StatusCode::CREATED, Json(c)))
}

#[derive(Deserialize)]
pub struct Page {
    before: Option<Id>,
    limit: Option<i64>,
}

pub async fn messages(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Query(q): Query<Page>,
) -> ApiResult<Json<Vec<Message>>> {
    require_channel(&state, auth.user.id, cid, perms::READ_HISTORY).await?;
    Ok(Json(state.db.messages(cid, q.before, q.limit.unwrap_or(50).clamp(1, 100)).await?))
}

pub async fn create_message(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Json(req): Json<CreateMessage>,
) -> ApiResult<(StatusCode, Json<Message>)> {
    let p = channel_perms(&state, auth.user.id, cid, perms::SEND_MESSAGES).await?;
    if req.nonce.as_ref().is_some_and(|n| n.len() > 64) {
        return Err(AppError::invalid("nonce trop long"));
    }
    let draft = kraken_db::Draft {
        content: &req.content,
        nonce: req.nonce,
        reply_to: req.reply_to,
        may_mention_everyone: p & perms::MENTION_EVERYONE != 0,
    };
    let (m, created) = state.db.send_message(cid, auth.user.id, draft).await?;
    // Même pour un renvoi (même nonce) : si la diffusion avait échoué, elle a lieu cette fois ;
    // les clients ignorent un message qu'ils ont déjà.
    let audience = state.db.channel_audience(cid).await?;
    if created {
        crate::reads::record(&state, &m, &audience).await;
    }
    state.hub.send(&audience, Event::MessageCreate(Box::new(m.clone())));
    Ok((if created { StatusCode::CREATED } else { StatusCode::OK }, Json(m)))
}

/// Modifier son propre message.
pub async fn edit_message(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, mid)): Path<(Id, Id)>,
    Json(req): Json<EditMessage>,
) -> ApiResult<Json<Message>> {
    let p = channel_perms(&state, auth.user.id, cid, perms::READ_HISTORY).await?;
    let before = state.db.message(mid).await?;
    let m = state.db.edit_message(cid, mid, auth.user.id, &req.content, p & perms::MENTION_EVERYONE != 0).await?;
    if before.as_ref() != Some(&m) {
        let audience = state.db.channel_audience(cid).await?;
        state.hub.send(&audience, Event::MessageUpdate(Box::new(m.clone())));
    }
    Ok(Json(m))
}

/// Supprimer son message, ou celui d'un autre avec `MANAGE_MESSAGES`.
pub async fn delete_message(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, mid)): Path<(Id, Id)>,
) -> ApiResult<StatusCode> {
    let p = channel_perms(&state, auth.user.id, cid, perms::READ_HISTORY).await?;
    state.db.delete_message(cid, mid, auth.user.id, p & perms::MANAGE_MESSAGES != 0).await?;
    let audience = state.db.channel_audience(cid).await?;
    state.hub.send(&audience, Event::MessageDelete { channel_id: cid, id: mid });
    crate::reads::deleted(&state, cid, mid).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_reaction(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, mid, emoji)): Path<(Id, Id, String)>,
) -> ApiResult<StatusCode> {
    channel_perms(&state, auth.user.id, cid, perms::READ_HISTORY | perms::ADD_REACTIONS).await?;
    if state.db.add_reaction(cid, mid, auth.user.id, &emoji).await? {
        let audience = state.db.channel_audience(cid).await?;
        let ev = Event::ReactionAdd { channel_id: cid, message_id: mid, user_id: auth.user.id, emoji };
        state.hub.send(&audience, ev);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_reaction(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, mid, emoji)): Path<(Id, Id, String)>,
) -> ApiResult<StatusCode> {
    channel_perms(&state, auth.user.id, cid, perms::READ_HISTORY).await?;
    if state.db.remove_reaction(cid, mid, auth.user.id, &emoji).await? {
        let audience = state.db.channel_audience(cid).await?;
        let ev = Event::ReactionRemove { channel_id: cid, message_id: mid, user_id: auth.user.id, emoji };
        state.hub.send(&audience, ev);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_invite(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Json(req): Json<CreateInvite>,
) -> ApiResult<(StatusCode, Json<Invite>)> {
    let c = require_channel(&state, auth.user.id, cid, perms::CREATE_INVITE).await?;
    let gid = c.guild_id.ok_or_else(|| AppError::invalid("Pas d'invitation pour un MP."))?;
    if req.max_age.is_some_and(|a| a > 60 * 60 * 24 * 30) || req.max_uses.is_some_and(|u| u > 1000) {
        return Err(AppError::invalid("Durée de 30 jours et 1000 usages au plus."));
    }
    let inv = state.db.create_invite(gid, Some(cid), auth.user.id, req.max_uses.unwrap_or(0), req.max_age).await?;
    Ok((StatusCode::CREATED, Json(inv)))
}

/// Aperçu public, sans session.
pub async fn invite(State(state): State<AppState>, Path(code): Path<String>) -> ApiResult<Json<Invite>> {
    if code.len() > 16 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(AppError::not_found());
    }
    state.db.invite(&code).await?.map(Json).ok_or_else(AppError::not_found)
}

pub async fn join(State(state): State<AppState>, auth: Auth, Path(code): Path<String>) -> ApiResult<Json<GuildState>> {
    let (g, new) = state.db.use_invite(&code, auth.user.id).await?;
    let g = state.with_voice(g);
    if let Some(member) = new {
        let others: Vec<Id> = g.members.iter().map(|m| m.user.id).filter(|u| *u != auth.user.id).collect();
        state.hub.send(&others, Event::MemberAdd { guild_id: g.guild.id, member });
        state.hub.send(&[auth.user.id], Event::GuildCreate(Box::new(g.clone())));
    }
    Ok(Json(g))
}
