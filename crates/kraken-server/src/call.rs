//! Appels des MP et des groupes, sans serveur : la conversation sert de salon vocal (même SFU,
//! mêmes routes `POST /channels/{id}/voice` et `DELETE /voice`). Le premier qui entre lance
//! l'appel : un message d'appel est posté, et ça sonne chez les autres participants (sauf
//! blocage) pendant `ring_secs`. Entrer, refuser ou attendre trop longtemps arrête la sonnerie ;
//! l'appel prend fin quand le dernier part. Seuls les participants de la conversation en sont
//! prévenus (CALL_UPDATE, CALL_DELETE).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use kraken_proto::*;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiResult, AppError};

/// Toutes les combien on regarde qui est dans l'appel (sonnerie, fin).
const TICK: Duration = Duration::from_millis(500);

struct Entry {
    caller: Id,
    /// `None` le temps que le message d'appel soit écrit.
    message_id: Option<Id>,
    ringing: Vec<Id>,
    /// Ceux qui y sont entrés, pour le message d'appel.
    joined: Vec<Id>,
}

/// Les appels en cours, par conversation.
#[derive(Default)]
pub struct Calls {
    inner: Mutex<HashMap<Id, Entry>>,
}

impl Calls {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Id, Entry>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Réserve l'appel de cette conversation ; vrai s'il n'y en avait pas (c'est à moi de le lancer).
    fn begin(&self, cid: Id, caller: Id) -> bool {
        let mut calls = self.lock();
        if calls.contains_key(&cid) {
            return false;
        }
        calls.insert(cid, Entry { caller, message_id: None, ringing: vec![], joined: vec![caller] });
        true
    }

    fn abort(&self, cid: Id) {
        self.lock().remove(&cid);
    }

    fn snapshot(cid: Id, e: &Entry) -> Option<Call> {
        Some(Call { channel_id: cid, message_id: e.message_id?, started_by: e.caller, ringing: e.ringing.clone() })
    }

    /// Les appels en cours de ces conversations.
    pub fn of(&self, channels: &[Id]) -> Vec<Call> {
        let calls = self.lock();
        channels.iter().filter_map(|c| calls.get(c).and_then(|e| Self::snapshot(*c, e))).collect()
    }

    /// `user` est entré, a refusé ou n'est plus là : ça ne sonne plus chez lui. Rend l'appel
    /// s'il a changé.
    fn stop_ringing(&self, cid: Id, user: Id, entered: bool) -> Option<Call> {
        let mut calls = self.lock();
        let e = calls.get_mut(&cid)?;
        if entered && !e.joined.contains(&user) {
            e.joined.push(user);
        }
        let before = e.ringing.len();
        e.ringing.retain(|u| *u != user);
        (e.ringing.len() != before).then(|| Self::snapshot(cid, e)).flatten()
    }
}

/// À tous les participants de la conversation.
async fn announce(state: &AppState, cid: Id, ev: Event) {
    match state.db.channel_audience(cid).await {
        Ok(audience) => state.hub.send(&audience, ev),
        Err(e) => tracing::warn!("appel {cid} : {e:?}"),
    }
}

/// Avant d'entrer dans l'appel de `cid` : rend l'id du message d'appel si c'est à moi de le
/// lancer (pas encore d'appel en cours), `None` sinon.
pub async fn prepare(state: &AppState, cid: Id, me: Id) -> ApiResult<Option<Message>> {
    if !state.calls.begin(cid, me) {
        return Ok(None);
    }
    match state.db.create_call_message(cid, me).await {
        Ok(m) => Ok(Some(m)),
        Err(e) => {
            state.calls.abort(cid);
            Err(e.into())
        }
    }
}

/// L'entrée a échoué : l'appel réservé par [`prepare`] n'a jamais commencé.
pub async fn cancel(state: &AppState, cid: Id, message: Option<Message>) {
    let Some(m) = message else { return };
    state.calls.abort(cid);
    if let Err(e) = state.db.discard_call_message(m.id).await {
        tracing::warn!("appel {cid} : {e:?}");
    }
}

/// Je suis entré dans l'appel de `cid`. `message` : je le lance, et ça sonne chez `targets`.
pub async fn entered(state: &AppState, cid: Id, me: Id, message: Option<Message>, targets: Vec<Id>) {
    let Some(m) = message else {
        if let Some(call) = state.calls.stop_ringing(cid, me, true) {
            announce(state, cid, Event::CallUpdate(call)).await;
        }
        return;
    };
    let present: Vec<Id> =
        state.voice.as_ref().map(|v| v.room_states(cid).iter().map(|s| s.user_id).collect()).unwrap_or_default();
    let call = {
        let mut calls = state.calls.lock();
        calls.get_mut(&cid).and_then(|e| {
            e.message_id = Some(m.id);
            e.ringing = targets.into_iter().filter(|u| !present.contains(u)).collect();
            Calls::snapshot(cid, e)
        })
    };
    match state.db.channel_audience(cid).await {
        Ok(audience) => {
            crate::reads::record(state, &m, &audience).await;
            state.hub.send(&audience, Event::MessageCreate(Box::new(m)));
        }
        Err(e) => tracing::warn!("appel {cid} : {e:?}"),
    }
    if let Some(call) = call {
        announce(state, cid, Event::CallUpdate(call)).await;
    }
    tokio::spawn(watch(state.clone(), cid));
}

/// Suit l'appel : arrête la sonnerie de ceux qui sont entrés ou qui ont trop attendu, et y met
/// fin quand il n'y a plus personne.
async fn watch(state: AppState, cid: Id) {
    let ring_until = Instant::now() + Duration::from_secs(state.cfg.ring_secs);
    loop {
        tokio::time::sleep(TICK).await;
        let present: Vec<Id> =
            state.voice.as_ref().map(|v| v.room_states(cid).iter().map(|s| s.user_id).collect()).unwrap_or_default();
        let (ended, changed) = {
            let mut calls = state.calls.lock();
            let Some(e) = calls.get_mut(&cid) else { return };
            for u in &present {
                if !e.joined.contains(u) {
                    e.joined.push(*u);
                }
            }
            let before = e.ringing.len();
            e.ringing.retain(|u| !present.contains(u));
            if Instant::now() >= ring_until {
                e.ringing.clear();
            }
            let changed = (e.ringing.len() != before).then(|| Calls::snapshot(cid, e)).flatten();
            let ended = if present.is_empty() { calls.remove(&cid) } else { None };
            (ended, changed)
        };
        if let Some(e) = ended {
            finish(&state, cid, e).await;
            return;
        }
        if let Some(call) = changed {
            announce(&state, cid, Event::CallUpdate(call)).await;
        }
    }
}

/// Le dernier est parti : le message d'appel dit qui y était et combien de temps il a duré.
async fn finish(state: &AppState, cid: Id, e: Entry) {
    if let Some(mid) = e.message_id {
        match state.db.end_call_message(mid, &e.joined).await {
            Ok(Some(m)) => announce(state, cid, Event::MessageUpdate(Box::new(m))).await,
            Ok(None) => {}
            Err(err) => tracing::warn!("fin de l'appel {cid} : {err:?}"),
        }
    }
    announce(state, cid, Event::CallDelete { channel_id: cid }).await;
}

/// `POST /channels/{id}/call/decline` : ne plus faire sonner cet appel chez moi.
pub async fn decline(State(state): State<AppState>, auth: Auth, Path(cid): Path<Id>) -> ApiResult<StatusCode> {
    let me = auth.user.id;
    state.db.private_channel(cid, me).await?.ok_or_else(AppError::not_found)?;
    if let Some(call) = state.calls.stop_ringing(cid, me, false) {
        announce(&state, cid, Event::CallUpdate(call)).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Retiré d'un groupe (ou parti) : il quitte son appel, et ça ne sonne plus chez lui.
pub async fn removed(state: &AppState, cid: Id, user: Id) {
    if let Some(v) = &state.voice
        && v.state(user).is_some_and(|s| s.channel_id == Some(cid))
    {
        v.leave(user);
    }
    if let Some(call) = state.calls.stop_ringing(cid, user, false) {
        announce(state, cid, Event::CallUpdate(call)).await;
    }
}
