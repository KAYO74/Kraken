//! Amis, demandes, blocages (F02, F11) ; ouvrir un MP ou un groupe, le renommer, y ajouter
//! quelqu'un, le quitter (F03). Les règles sont dans `kraken-db` ; ici, on prévient chacun par
//! le gateway, avec le salon tel que lui le voit.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use kraken_db::Notices;
use kraken_proto::*;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiResult, AppError};

/// Une relation avec la présence de l'autre, s'il est un ami (pas pour une demande ni un blocage).
fn presence(state: &AppState, mut r: Relationship) -> Relationship {
    r.status = if r.kind == RelationshipKind::Friend { state.hub.status(r.user.id) } else { Status::Offline };
    r
}

fn notify(state: &AppState, notices: Notices) {
    for (uid, ev) in notices {
        let ev = match ev {
            Event::RelationshipAdd(r) => Event::RelationshipAdd(presence(state, r)),
            Event::RelationshipUpdate(r) => Event::RelationshipUpdate(presence(state, r)),
            ev => ev,
        };
        state.hub.send(&[uid], ev);
    }
}

pub async fn relationships(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<Vec<Relationship>>> {
    Ok(Json(state.hub.with_presence(state.db.relationships(auth.user.id).await?)))
}

/// Demande d'ami par pseudo (ou acceptation, s'il m'en a envoyé une).
pub async fn send_request(
    State(state): State<AppState>,
    auth: Auth,
    Json(req): Json<FriendRequest>,
) -> ApiResult<(StatusCode, Json<Relationship>)> {
    let name = req.username.trim().trim_start_matches('@');
    if name.is_empty() {
        return Err(AppError::invalid("Écris le pseudo de la personne à ajouter."));
    }
    let target = match state.db.user_by_username(name).await? {
        Some(u) => u,
        None if kraken_db::valid_username(&name.to_lowercase()) => {
            return Err(AppError(
                StatusCode::NOT_FOUND,
                "unknown_user",
                format!("Aucun compte ne porte le pseudo « {} ». Vérifie son orthographe.", name.to_lowercase()),
            ));
        }
        None => {
            return Err(AppError(
                StatusCode::NOT_FOUND,
                "unknown_user",
                "Aucun compte ne porte ce pseudo : il ne contient que des minuscules, chiffres, points et tirets bas."
                    .into(),
            ));
        }
    };
    let (r, notices) = state.db.befriend(auth.user.id, target.id).await?;
    notify(&state, notices);
    Ok((StatusCode::CREATED, Json(presence(&state, r))))
}

/// Accepter une demande (`friend`) ou bloquer (`blocked`).
pub async fn set_relationship(
    State(state): State<AppState>,
    auth: Auth,
    Path(uid): Path<Id>,
    Json(req): Json<SetRelationship>,
) -> ApiResult<Json<Relationship>> {
    let (r, notices) = match req.kind {
        RelationshipKind::Friend => state.db.befriend(auth.user.id, uid).await?,
        RelationshipKind::Blocked => state.db.block(auth.user.id, uid).await?,
        _ => return Err(AppError::invalid("Seuls « friend » et « blocked » se choisissent.")),
    };
    notify(&state, notices);
    Ok(Json(presence(&state, r)))
}

/// Retirer un ami, refuser ou annuler une demande, débloquer.
pub async fn remove_relationship(
    State(state): State<AppState>,
    auth: Auth,
    Path(uid): Path<Id>,
) -> ApiResult<StatusCode> {
    let notices = state.db.remove_relationship(auth.user.id, uid).await?;
    notify(&state, notices);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn private_channels(State(state): State<AppState>, auth: Auth) -> ApiResult<Json<Vec<Channel>>> {
    Ok(Json(state.db.private_channels(auth.user.id).await?))
}

/// Envoie à chacun le salon privé tel qu'il le voit (ses participants, sans lui-même).
async fn announce(state: &AppState, cid: Id, users: &[Id], ev: fn(Channel) -> Event) -> ApiResult<()> {
    for &u in users {
        if let Some(c) = state.db.private_channel(cid, u).await? {
            state.hub.send(&[u], ev(c));
        }
    }
    Ok(())
}

async fn mine(state: &AppState, cid: Id, me: Id) -> ApiResult<Channel> {
    state.db.private_channel(cid, me).await?.ok_or_else(AppError::not_found)
}

/// Ouvrir le MP avec une personne, ou créer un groupe avec plusieurs amis.
pub async fn open_private(
    State(state): State<AppState>,
    auth: Auth,
    Json(req): Json<OpenPrivate>,
) -> ApiResult<(StatusCode, Json<Channel>)> {
    let me = auth.user.id;
    let (cid, created) = match req.recipients.as_slice() {
        [] => return Err(AppError::invalid("Choisis au moins une personne.")),
        [one] => state.db.open_dm(me, *one).await?,
        many => (state.db.create_group(me, many, req.name.as_deref()).await?, true),
    };
    if created {
        let audience = state.db.channel_audience(cid).await?;
        announce(&state, cid, &audience, Event::ChannelCreate).await?;
    }
    let status = if created { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(mine(&state, cid, me).await?)))
}

/// Renommer un groupe.
pub async fn edit_channel(
    State(state): State<AppState>,
    auth: Auth,
    Path(cid): Path<Id>,
    Json(req): Json<EditChannel>,
) -> ApiResult<Json<Channel>> {
    state.db.rename_group(cid, auth.user.id, req.name.as_deref()).await?;
    let audience = state.db.channel_audience(cid).await?;
    announce(&state, cid, &audience, Event::ChannelUpdate).await?;
    Ok(Json(mine(&state, cid, auth.user.id).await?))
}

/// Ajouter un ami à un groupe.
pub async fn add_recipient(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, uid)): Path<(Id, Id)>,
) -> ApiResult<StatusCode> {
    if state.db.add_recipient(cid, auth.user.id, uid).await? {
        let others: Vec<Id> = state.db.channel_audience(cid).await?.into_iter().filter(|u| *u != uid).collect();
        announce(&state, cid, &[uid], Event::ChannelCreate).await?;
        announce(&state, cid, &others, Event::ChannelUpdate).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Quitter un groupe (`@me`) ou en retirer quelqu'un (propriétaire).
pub async fn remove_recipient(
    State(state): State<AppState>,
    auth: Auth,
    Path((cid, who)): Path<(Id, String)>,
) -> ApiResult<StatusCode> {
    let target = if who == "@me" { auth.user.id } else { who.parse().map_err(|_| AppError::not_found())? };
    let left = state.db.remove_recipient(cid, auth.user.id, target).await?;
    state.hub.send(&[target], Event::ChannelDelete { id: cid });
    announce(&state, cid, &left, Event::ChannelUpdate).await?;
    Ok(StatusCode::NO_CONTENT)
}
