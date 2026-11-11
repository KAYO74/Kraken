//! Salons d'un serveur : créer, modifier, supprimer, ranger, et leurs surcharges de droits. Les
//! droits et la hiérarchie sont vérifiés par `kraken-db` ; ici, on appelle, puis on dit à chaque
//! membre ce que ce changement fait de son point de vue : un salon qui apparaît, qui change, ou
//! qui disparaît (supprimé, ou devenu privé pour lui).

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use kraken_proto::*;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiResult, AppError};

/// Les salons d'un serveur et qui les voit, à un instant.
struct Snapshot {
    channels: HashMap<Id, Channel>,
    audiences: HashMap<Id, Vec<Id>>,
}

async fn snapshot(state: &AppState, gid: Id) -> ApiResult<Snapshot> {
    Ok(Snapshot {
        channels: state.db.guild_channels(gid).await?.into_iter().map(|c| (c.id, c)).collect(),
        audiences: state.db.guild_audiences(gid).await?,
    })
}

/// Compare avec l'état d'avant : chacun reçoit `CHANNEL_CREATE` pour un salon qu'il voit
/// maintenant, `CHANNEL_DELETE` pour un salon qu'il ne voit plus (supprimé, ou rendu privé), et
/// `CHANNEL_UPDATE` pour un salon qu'il voyait et qui a changé. Ceux qui perdent un salon vocal
/// en sont retirés.
async fn announce(state: &AppState, gid: Id, before: Snapshot) -> ApiResult<()> {
    let after = snapshot(state, gid).await?;
    let none: Vec<Id> = vec![];
    let who = |s: &Snapshot, id: Id| s.audiences.get(&id).unwrap_or(&none).clone();
    let gone = |state: &AppState, id: Id, users: &[Id]| {
        if users.is_empty() {
            return;
        }
        state.hub.send(users, Event::ChannelDelete { id });
        if let Some(v) = &state.voice {
            for u in users {
                if v.state(*u).is_some_and(|s| s.channel_id == Some(id)) {
                    v.leave(*u);
                }
            }
        }
    };
    for id in before.channels.keys().filter(|id| !after.channels.contains_key(id)) {
        gone(state, *id, &who(&before, *id));
    }
    for (id, c) in &after.channels {
        let (was, now) = (who(&before, *id), who(&after, *id));
        let lost: Vec<Id> = was.iter().filter(|u| !now.contains(u)).copied().collect();
        let gained: Vec<Id> = now.iter().filter(|u| !was.contains(u)).copied().collect();
        let stayed: Vec<Id> = now.iter().filter(|u| was.contains(u)).copied().collect();
        gone(state, *id, &lost);
        state.hub.send(&gained, Event::ChannelCreate(c.clone()));
        if before.channels.get(id).is_some_and(|old| old != c) {
            state.hub.send(&stayed, Event::ChannelUpdate(c.clone()));
        }
    }
    Ok(())
}

/// Le serveur d'un salon, ou 404 pour un MP, un groupe ou un salon inconnu.
async fn guild_of(state: &AppState, cid: Id) -> ApiResult<Id> {
    state.db.channel(cid).await?.and_then(|c| c.guild_id).ok_or_else(AppError::not_found)
}

/// `POST /guilds/{id}/channels` : un salon ou une catégorie, pour qui gère les salons.
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Json(req): Json<CreateChannel>,
) -> ApiResult<(StatusCode, Json<Channel>)> {
    let before = snapshot(&state, gid).await?;
    let c = state.db.create_channel(gid, auth.user.id, &req).await?;
    announce(&state, gid, before).await?;
    Ok((StatusCode::CREATED, Json(c)))
}

/// `PATCH /channels/{id}` : modifier un salon de serveur ; pour un groupe, le renommer.
pub async fn edit(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Json(req): Json<EditChannel>,
) -> ApiResult<Json<Channel>> {
    let Ok(gid) = guild_of(&state, cid).await else {
        return crate::social::edit_channel(State(state), auth, Path(cid), Json(req)).await;
    };
    let before = snapshot(&state, gid).await?;
    let c = state.db.edit_channel(cid, auth.user.id, &req).await?;
    announce(&state, gid, before).await?;
    Ok(Json(c))
}

/// `DELETE /channels/{id}` : supprimer un salon de serveur avec ses messages ; pour un MP, le
/// fermer chez soi.
pub async fn delete(State(state): State<AppState>, auth: Auth, Path(cid): Path<Id>) -> ApiResult<StatusCode> {
    let Ok(gid) = guild_of(&state, cid).await else {
        return crate::social::close_channel(State(state), auth, Path(cid)).await;
    };
    let before = snapshot(&state, gid).await?;
    let files = state.db.delete_channel(cid, auth.user.id).await?;
    announce(&state, gid, before).await?;
    crate::attach::remove_files(&state, files).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /channels/{id}/position` : rend les salons dont la place a changé.
pub async fn position(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Json(req): Json<MoveChannel>,
) -> ApiResult<Json<Vec<Channel>>> {
    let gid = guild_of(&state, cid).await?;
    let before = snapshot(&state, gid).await?;
    let changed = state.db.move_channel(cid, auth.user.id, &req).await?;
    announce(&state, gid, before).await?;
    let all = state.db.guild_channels(gid).await?;
    Ok(Json(all.into_iter().filter(|c| changed.contains(&c.id)).collect()))
}

/// `GET /channels/{id}/permissions` : les surcharges de droits, pour qui gère le salon.
pub async fn overwrites(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
) -> ApiResult<Json<Vec<PermissionOverwrite>>> {
    Ok(Json(state.db.channel_overwrites(cid, auth.user.id).await?))
}

/// `PUT /channels/{id}/permissions/{target}` : poser la surcharge d'un rôle ou d'un membre.
pub async fn set_overwrite(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, target)): Path<(Id, Id)>,
    Json(req): Json<SetOverwrite>,
) -> ApiResult<Json<Vec<PermissionOverwrite>>> {
    let gid = guild_of(&state, cid).await?;
    let before = snapshot(&state, gid).await?;
    let list = state.db.set_overwrite(cid, auth.user.id, target, &req).await?;
    announce(&state, gid, before).await?;
    Ok(Json(list))
}

/// `DELETE /channels/{id}/permissions/{target}` : retirer la surcharge d'un rôle ou d'un membre.
pub async fn remove_overwrite(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, target)): Path<(Id, Id)>,
) -> ApiResult<Json<Vec<PermissionOverwrite>>> {
    let gid = guild_of(&state, cid).await?;
    let before = snapshot(&state, gid).await?;
    let list = state.db.remove_overwrite(cid, auth.user.id, target).await?;
    announce(&state, gid, before).await?;
    Ok(Json(list))
}
