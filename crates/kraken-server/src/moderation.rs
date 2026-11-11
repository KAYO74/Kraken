//! Rôles, exclusions, bannissements, départ et transfert de propriété. Les droits et la
//! hiérarchie sont vérifiés par `kraken-db` ; ici, on appelle puis on prévient le gateway.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use kraken_core::perms;
use kraken_proto::*;
use serde::Deserialize;

use crate::AppState;
use crate::auth::Auth;
use crate::error::ApiResult;

/// Tous les membres du serveur, à qui dire ce qui change.
async fn everyone(state: &AppState, gid: Id) -> ApiResult<Vec<Id>> {
    Ok(state.db.member_ids(gid).await?)
}

/// Quelqu'un n'est plus dans le serveur : les autres, et lui-même, le savent ; sa voix est coupée.
async fn gone(state: &AppState, gid: Id, uid: Id) -> ApiResult<()> {
    let mut audience = everyone(state, gid).await?;
    audience.push(uid);
    state.hub.send(&audience, Event::GuildMemberRemove { guild_id: gid, user_id: uid });
    if let Some(v) = &state.voice
        && v.state(uid).is_some_and(|s| s.guild_id == Some(gid))
    {
        v.leave(uid);
    }
    Ok(())
}

pub async fn create_role(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Json(req): Json<CreateRole>,
) -> ApiResult<(StatusCode, Json<Role>)> {
    let role = state.db.create_role(gid, auth.user.id, &req).await?;
    state.hub.send(&everyone(&state, gid).await?, Event::GuildRoleCreate { guild_id: gid, role: role.clone() });
    Ok((StatusCode::CREATED, Json(role)))
}

pub async fn edit_role(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, rid)): Path<(Id, Id)>,
    Json(req): Json<EditRole>,
) -> ApiResult<Json<Role>> {
    let role = state.db.edit_role(gid, rid, auth.user.id, &req).await?;
    state.hub.send(&everyone(&state, gid).await?, Event::GuildRoleUpdate { guild_id: gid, role: role.clone() });
    Ok(Json(role))
}

pub async fn delete_role(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, rid)): Path<(Id, Id)>,
) -> ApiResult<StatusCode> {
    state.db.delete_role(gid, rid, auth.user.id).await?;
    // Les rangs se resserrent : les clients recalculent les positions à partir de la liste.
    state.hub.send(&everyone(&state, gid).await?, Event::GuildRoleDelete { guild_id: gid, role_id: rid });
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder_roles(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Json(req): Json<ReorderRoles>,
) -> ApiResult<Json<Vec<Role>>> {
    let changed = state.db.reorder_roles(gid, auth.user.id, &req.ids).await?;
    let audience = everyone(&state, gid).await?;
    for role in &changed {
        state.hub.send(&audience, Event::GuildRoleUpdate { guild_id: gid, role: role.clone() });
    }
    Ok(Json(changed))
}

pub async fn add_member_role(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, uid, rid)): Path<(Id, Id, Id)>,
) -> ApiResult<Json<Member>> {
    member_role(state, auth, gid, uid, rid, true).await
}

pub async fn remove_member_role(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, uid, rid)): Path<(Id, Id, Id)>,
) -> ApiResult<Json<Member>> {
    member_role(state, auth, gid, uid, rid, false).await
}

async fn member_role(state: AppState, auth: Auth, gid: Id, uid: Id, rid: Id, on: bool) -> ApiResult<Json<Member>> {
    let member = state.db.set_member_role(gid, auth.user.id, uid, rid, on).await?;
    state.hub.send(&everyone(&state, gid).await?, Event::GuildMemberUpdate { guild_id: gid, member: member.clone() });
    Ok(Json(member))
}

#[derive(Deserialize)]
pub struct Reason {
    reason: Option<String>,
}

pub async fn kick(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, uid)): Path<(Id, Id)>,
    Query(q): Query<Reason>,
) -> ApiResult<StatusCode> {
    state.db.kick_member(gid, auth.user.id, uid, q.reason.as_deref()).await?;
    gone(&state, gid, uid).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn leave(State(state): State<AppState>, auth: Auth, Path(gid): Path<Id>) -> ApiResult<StatusCode> {
    state.db.leave_guild(gid, auth.user.id).await?;
    gone(&state, gid, auth.user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /guilds/{id}/members/{uid}/timeout` : exclure temporairement ; la personne est
/// déconnectée du vocal si elle y était, et tous les membres reçoivent le membre mis à jour.
pub async fn timeout(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, uid)): Path<(Id, Id)>,
    Json(req): Json<TimeoutMember>,
) -> ApiResult<Json<Member>> {
    let member = state.db.timeout_member(gid, auth.user.id, uid, req.seconds, req.reason.as_deref()).await?;
    if let Some(v) = &state.voice
        && v.state(uid).is_some_and(|s| s.guild_id == Some(gid))
    {
        v.leave(uid);
    }
    state.hub.send(&everyone(&state, gid).await?, Event::GuildMemberUpdate { guild_id: gid, member: member.clone() });
    Ok(Json(member))
}

/// `DELETE /guilds/{id}/members/{uid}/timeout` : lever l'exclusion.
pub async fn lift_timeout(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, uid)): Path<(Id, Id)>,
) -> ApiResult<Json<Member>> {
    let member = state.db.remove_timeout(gid, auth.user.id, uid).await?;
    state.hub.send(&everyone(&state, gid).await?, Event::GuildMemberUpdate { guild_id: gid, member: member.clone() });
    Ok(Json(member))
}

/// `DELETE /guilds/{id}` : supprimer le serveur (son propriétaire, qui retape le nom). Tous les
/// membres reçoivent `GUILD_DELETE` ; ceux qui étaient en vocal dans ce serveur en sortent.
pub async fn delete_guild(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    body: Option<Json<DeleteGuild>>,
) -> ApiResult<StatusCode> {
    let name = body.map(|b| b.0.name).unwrap_or_default();
    let out = state.db.delete_guild(gid, auth.user.id, &name).await?;
    tracing::info!("serveur {gid} supprimé par son propriétaire ({} membres)", out.members.len());
    if let Some(v) = &state.voice {
        for u in &out.members {
            if v.state(*u).is_some_and(|s| s.guild_id == Some(gid)) {
                v.leave(*u);
            }
        }
    }
    state.hub.send(&out.members, Event::GuildDelete { guild_id: gid });
    crate::attach::remove_files(&state, out.files).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn bans(State(state): State<AppState>, auth: Auth, Path(gid): Path<Id>) -> ApiResult<Json<Vec<Ban>>> {
    Ok(Json(state.db.bans(gid, auth.user.id).await?))
}

pub async fn ban(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, uid)): Path<(Id, Id)>,
    body: Option<Json<BanMember>>,
) -> ApiResult<Json<Ban>> {
    let req = body.map(|b| b.0).unwrap_or_default();
    let out = state.db.ban_member(gid, auth.user.id, uid, &req).await?;
    if out.was_member {
        gone(&state, gid, uid).await?;
    }
    for (cid, mid) in &out.purged {
        let audience = state.db.channel_audience(*cid).await?;
        state.hub.send(&audience, Event::MessageDelete { channel_id: *cid, id: *mid });
        crate::reads::deleted(&state, *cid, *mid).await;
    }
    crate::attach::remove_files(&state, out.files).await;
    let judges = state.db.members_with(gid, perms::BAN).await?;
    state.hub.send(&judges, Event::GuildBanAdd { guild_id: gid, ban: out.ban.clone() });
    Ok(Json(out.ban))
}

pub async fn unban(
    State(state): State<AppState>,
    auth: Auth,
    Path((gid, uid)): Path<(Id, Id)>,
) -> ApiResult<StatusCode> {
    state.db.unban(gid, auth.user.id, uid).await?;
    let judges = state.db.members_with(gid, perms::BAN).await?;
    state.hub.send(&judges, Event::GuildBanRemove { guild_id: gid, user_id: uid });
    Ok(StatusCode::NO_CONTENT)
}

pub async fn transfer(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Json(req): Json<TransferOwner>,
) -> ApiResult<StatusCode> {
    state.db.transfer_ownership(gid, auth.user.id, req.user_id).await?;
    state.hub.send(&everyone(&state, gid).await?, Event::GuildOwnerUpdate { guild_id: gid, owner_id: req.user_id });
    Ok(StatusCode::NO_CONTENT)
}
