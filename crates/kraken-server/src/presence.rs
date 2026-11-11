//! Statut en ligne, inactif, ne pas déranger, invisible. Rien n'est écrit en base sauf le choix de
//! la personne ; « hors ligne » et « inactif automatique » viennent des connexions ouvertes
//! (voir `gateway::Hub::status`). Les statuts sont envoyés en direct à ceux qui voient la
//! personne (`Db::presence_peers` : membres de ses serveurs, participants de ses MP, amis, sans
//! les comptes bloqués) et seulement aux clients qui ont déclaré les comprendre.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use kraken_proto::*;

use crate::AppState;
use crate::auth::Auth;
use crate::error::ApiResult;

fn entry(state: &AppState, uid: Id) -> PresenceEntry {
    PresenceEntry { user_id: uid, status: state.hub.status(uid) }
}

/// Les statuts de ceux que `viewer` voit (ou de ceux de `among` seulement), sans les hors ligne.
pub async fn snapshot(state: &AppState, viewer: Id, among: Option<&[Id]>) -> kraken_db::Result<Vec<PresenceEntry>> {
    let among: Option<HashSet<Id>> = among.map(|a| a.iter().copied().collect());
    Ok(state
        .db
        .presence_peers(viewer)
        .await?
        .into_iter()
        .filter(|p| among.as_ref().is_none_or(|a| a.contains(p)))
        .map(|p| entry(state, p))
        .filter(|e| e.status != Status::Offline)
        .collect())
}

/// Annonce le statut de cette personne à ceux qui la voient, s'il a changé depuis la dernière
/// annonce. À appeler après tout ce qui peut le changer : connexion, déconnexion, activité,
/// nouveau choix.
pub async fn refresh(state: AppState, uid: Id) {
    // Tenu jusqu'à l'envoi : deux annonces ne se croisent pas, la dernière dit toujours vrai.
    let mut announced = state.hub.announced.lock().await;
    let now = state.hub.status(uid);
    let before = announced.get(&uid).copied().unwrap_or(Status::Offline);
    if now == before {
        return;
    }
    match now {
        Status::Offline => announced.remove(&uid),
        s => announced.insert(uid, s),
    };
    match state.db.presence_peers(uid).await {
        Ok(peers) => state.hub.send_presence(
            &peers,
            Event::PresenceUpdate { presences: vec![PresenceEntry { user_id: uid, status: now }] },
        ),
        Err(e) => {
            // Rien n'est parti : on garde l'ancienne annonce pour réessayer au prochain changement.
            tracing::warn!("statut : {e:?}");
            match before {
                Status::Offline => announced.remove(&uid),
                s => announced.insert(uid, s),
            };
        }
    }
}

/// Redonne à une personne qui vient de se déclarer au courant ses statuts et son choix.
pub async fn resync(state: AppState, uid: Id) {
    match snapshot(&state, uid, None).await {
        Ok(presences) => {
            state.hub.send_presence(&[uid], Event::PresenceUpdate { presences });
            if let Some(presence) = state.hub.choice(uid) {
                state.hub.send_presence(&[uid], Event::MyPresenceUpdate { presence });
            }
        }
        Err(e) => tracing::warn!("statut : {e:?}"),
    }
}

/// `PUT /users/@me/presence` : choisir son statut. Gardé pour la prochaine fois, appliqué sur
/// tous mes appareils, annoncé aux autres.
pub async fn set(
    State(state): State<AppState>,
    auth: Auth,
    Json(req): Json<SetPresence>,
) -> ApiResult<Json<SetPresence>> {
    let uid = auth.user.id;
    state.db.set_presence(uid, req.presence).await?;
    state.hub.set_choice(uid, req.presence);
    state.hub.send_presence(&[uid], Event::MyPresenceUpdate { presence: req.presence });
    refresh(state, uid).await;
    Ok(Json(req))
}

/// Quelqu'un vient de rejoindre un serveur dont `members` sont les membres : il découvre leurs
/// statuts, et eux le sien.
pub async fn joined(state: &AppState, uid: Id, members: &[Id]) {
    let peers = match state.db.presence_peers(uid).await {
        Ok(p) => p,
        Err(e) => return tracing::warn!("statut : {e:?}"),
    };
    let here: HashSet<Id> = members.iter().copied().collect();
    let seen: Vec<PresenceEntry> = peers
        .iter()
        .filter(|p| **p != uid && here.contains(*p))
        .map(|p| entry(state, *p))
        .filter(|e| e.status != Status::Offline)
        .collect();
    if !seen.is_empty() {
        state.hub.send_presence(&[uid], Event::PresenceUpdate { presences: seen });
    }
    let mine = entry(state, uid);
    if mine.status != Status::Offline {
        let others: Vec<Id> = peers.into_iter().filter(|p| *p != uid && here.contains(p)).collect();
        state.hub.send_presence(&others, Event::PresenceUpdate { presences: vec![mine] });
    }
}

/// Un blocage coupe la vue des statuts dans les deux sens.
pub fn hide(state: &AppState, a: Id, b: Id) {
    for (viewer, subject) in [(a, b), (b, a)] {
        let gone = PresenceEntry { user_id: subject, status: Status::Offline };
        state.hub.send_presence(&[viewer], Event::PresenceUpdate { presences: vec![gone] });
    }
}

/// Une relation retirée (déblocage, ami retiré) : si ces deux-là se voient encore, chacun
/// retrouve le statut de l'autre.
pub async fn reveal(state: &AppState, a: Id, b: Id) {
    let Ok(peers) = state.db.presence_peers(a).await else { return };
    if !peers.contains(&b) {
        return;
    }
    for (viewer, subject) in [(a, b), (b, a)] {
        let e = entry(state, subject);
        if e.status != Status::Offline {
            state.hub.send_presence(&[viewer], Event::PresenceUpdate { presences: vec![e] });
        }
    }
}
