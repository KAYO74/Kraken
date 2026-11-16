//! Nom et icône d'un serveur : `PATCH /guilds/{id}` (`{ "name" }`), `PUT /guilds/{id}/icon` (le corps est
//! l'image elle-même, comme pour une photo de profil) et `DELETE /guilds/{id}/icon`. Réservé à qui a « Gérer
//! le serveur », propriétaire compris : le droit est jugé par `kraken-db` avant de lire ou de décoder quoi
//! que ce soit, et la trace est écrite au journal d'audit dans la même transaction.
//!
//! L'image suit la voie des photos de profil (`avatar`) : reconnue à ses premiers octets, recadrée en carré de
//! 256 pixels, ré-encodée en PNG (sauf un GIF ou un WebP animé, gardé animé), gardée sous une clé aléatoire
//! dans `KRAKEN_DATA_DIR/avatars`. Lecture : `GET /guild-icons/{key}`, publique, avec le cache sans fin des
//! photos. Chaque changement est diffusé à tous les membres (`GUILD_UPDATE`).

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kraken_proto::*;

use crate::AppState;
use crate::auth::Auth;
use crate::avatar::{self, Kind};
use crate::error::ApiResult;

/// Renommages, envois et retraits d'icône par minute et par client (un envoi coûte un décodage d'image).
pub const WRITES_PER_MIN: u32 = 60;

/// Dit aux membres du serveur comment il s'appelle et quelle icône il porte.
async fn announce(state: &AppState, guild: &Guild) -> ApiResult<()> {
    let audience = state.db.member_ids(guild.id).await?;
    state.hub.send(&audience, Event::GuildUpdate(guild.clone()));
    Ok(())
}

/// `PATCH /guilds/{id}` : renommer.
pub async fn rename(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Json(req): Json<RenameGuild>,
) -> ApiResult<Json<Guild>> {
    let guild = state.db.rename_guild(gid, auth.user.id, &req.name).await?;
    announce(&state, &guild).await?;
    Ok(Json(guild))
}

/// `PUT /guilds/{id}/icon` : le corps est l'image.
pub async fn set_icon(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Json<Guild>> {
    // Le droit d'abord : sans lui, on ne lit ni ne décode rien.
    state.db.check_guild_edit(gid, auth.user.id).await?;
    let (key, file) = avatar::receive(&state, &headers, body, Kind::GuildIcon).await?;
    let (guild, old) = match state.db.set_guild_icon(gid, auth.user.id, Some(&key)).await {
        Ok(r) => r,
        Err(e) => {
            tokio::fs::remove_file(&file).await.ok();
            return Err(e.into());
        }
    };
    avatar::forget(&state, old).await;
    announce(&state, &guild).await?;
    Ok(Json(guild))
}

/// `DELETE /guilds/{id}/icon` : retour aux initiales.
pub async fn remove_icon(State(state): State<AppState>, auth: Auth, Path(gid): Path<Id>) -> ApiResult<Json<Guild>> {
    let (guild, old) = state.db.set_guild_icon(gid, auth.user.id, None).await?;
    avatar::forget(&state, old).await;
    announce(&state, &guild).await?;
    Ok(Json(guild))
}
