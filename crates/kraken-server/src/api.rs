//! Serveurs, salons, messages, invitations, social. Chaque handler vérifie les droits
//! (moteur de `kraken-core`) avant de toucher aux données, puis prévient le gateway.

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
    let p = state.db.channel_perms(uid, cid).await?;
    if p & perms::VIEW_CHANNEL == 0 {
        return Err(AppError::not_found()); // ne pas révéler l'existence du salon
    }
    if p & need != need {
        return Err(AppError::forbidden());
    }
    state.db.channel(cid).await?.ok_or_else(AppError::not_found)
}

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
    Json(state.cfg.instance.clone())
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
    state.db.guild_state(gid, auth.user.id).await?.map(Json).ok_or_else(AppError::not_found)
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
    require_channel(&state, auth.user.id, cid, perms::SEND_MESSAGES).await?;
    if req.nonce.as_ref().is_some_and(|n| n.len() > 64) {
        return Err(AppError::invalid("nonce trop long"));
    }
    let (m, created) = state.db.create_message(cid, auth.user.id, &req.content, req.nonce).await?;
    // Même pour un renvoi (même nonce) : si la diffusion avait échoué, elle a lieu cette fois ;
    // les clients ignorent un message qu'ils ont déjà.
    let audience = state.db.channel_audience(cid).await?;
    state.hub.send(&audience, Event::MessageCreate(m.clone()));
    Ok((if created { StatusCode::CREATED } else { StatusCode::OK }, Json(m)))
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
    if let Some(member) = new {
        let others: Vec<Id> = g.members.iter().map(|m| m.user.id).filter(|u| *u != auth.user.id).collect();
        state.hub.send(&others, Event::MemberAdd { guild_id: g.guild.id, member });
        state.hub.send(&[auth.user.id], Event::GuildCreate(Box::new(g.clone())));
    }
    Ok(Json(g))
}

pub async fn relationships(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<Vec<Relationship>>> {
    Ok(Json(state.hub.with_presence(state.db.relationships(auth.user.id).await?)))
}

pub async fn private_channels(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<Vec<Channel>>> {
    Ok(Json(state.db.private_channels(auth.user.id).await?))
}
