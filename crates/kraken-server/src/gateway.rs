//! Gateway WebSocket : HELLO → IDENTIFY → READY, puis DISPATCH numérotés et battements.
//! Chaque connexion a une file bornée ; un client trop lent est déconnecté (il se reconnecte).

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message as WsMsg, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use kraken_db::Db;
use kraken_proto::*;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};

use crate::AppState;
use crate::auth::token_hash;

const HEARTBEAT_MS: u32 = 30_000;
const QUEUE: usize = 256;

/// Une connexion ouverte : son numéro, la session qui l'a ouverte (empreinte), sa file d'envoi.
struct Conn {
    id: u64,
    session: Vec<u8>,
    tx: mpsc::Sender<Event>,
    /// Ce client s'est déclaré inactif (voir [`ClientOp::Activity`]).
    idle: bool,
    /// Il comprend les événements de statut : il a envoyé [`ClientOp::Activity`].
    presence_aware: bool,
}

#[derive(Default)]
pub struct Hub {
    conns: RwLock<HashMap<Id, Vec<Conn>>>,
    next: AtomicU64,
    /// Présence fixe des comptes de démo (ils ne se connectent jamais).
    demo_presence: RwLock<HashMap<Id, Status>>,
    /// Le statut choisi par chaque personne connectée (la base garde le choix des absents).
    choices: RwLock<HashMap<Id, Presence>>,
    /// Le dernier statut annoncé aux autres, par personne ; absent : hors ligne. Tenu pendant
    /// toute une annonce pour que deux annonces ne se croisent pas (voir `presence::refresh`).
    pub(crate) announced: tokio::sync::Mutex<HashMap<Id, Status>>,
}

impl Hub {
    /// Envoie l'événement à toutes les connexions de ces utilisateurs.
    pub fn send(&self, users: &[Id], ev: Event) {
        let mut conns = self.conns.write().unwrap();
        for u in users {
            if let Some(list) = conns.get_mut(u) {
                // File pleine ou connexion fermée : on lâche l'émetteur, la connexion se termine.
                list.retain(|c| c.tx.try_send(ev.clone()).is_ok());
            }
        }
    }

    /// Comme [`Self::send`], aux seuls clients qui comprennent les statuts : les autres ne
    /// connaissent pas ces événements et ne les liraient pas.
    pub fn send_presence(&self, users: &[Id], ev: Event) {
        let mut conns = self.conns.write().unwrap();
        for u in users {
            if let Some(list) = conns.get_mut(u) {
                list.retain(|c| !c.presence_aware || c.tx.try_send(ev.clone()).is_ok());
            }
        }
    }

    /// Ferme toutes les connexions de cet utilisateur (déconnexion partout, suppression).
    pub fn kick(&self, uid: Id) {
        self.conns.write().unwrap().remove(&uid);
        self.choices.write().unwrap().remove(&uid);
    }

    /// Ferme les connexions ouvertes avec cette session (déconnexion de cet appareil).
    pub fn kick_session(&self, uid: Id, session: &[u8]) {
        self.kick_where(uid, |c| c.session == session);
    }

    /// Ferme les connexions de toutes les sessions sauf celle-ci (changement de mot de passe).
    pub fn kick_others(&self, uid: Id, keep: &[u8]) {
        self.kick_where(uid, |c| c.session != keep);
    }

    fn kick_where(&self, uid: Id, closed: impl Fn(&Conn) -> bool) {
        let mut conns = self.conns.write().unwrap();
        if let Some(list) = conns.get_mut(&uid) {
            list.retain(|c| !closed(c));
            if list.is_empty() {
                conns.remove(&uid);
                self.choices.write().unwrap().remove(&uid);
            }
        }
    }

    fn register(&self, uid: Id, session: Vec<u8>, choice: Presence) -> (u64, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.conns.write().unwrap().entry(uid).or_default().push(Conn {
            id,
            session,
            tx,
            idle: false,
            presence_aware: false,
        });
        self.choices.write().unwrap().insert(uid, choice);
        (id, rx)
    }

    fn unregister(&self, uid: Id, conn: u64) {
        let mut conns = self.conns.write().unwrap();
        if let Some(list) = conns.get_mut(&uid) {
            list.retain(|c| c.id != conn);
            if list.is_empty() {
                conns.remove(&uid);
                self.choices.write().unwrap().remove(&uid);
            }
        }
    }

    /// Ce client est inactif, ou de nouveau actif. Vrai si c'est la première fois qu'il parle de
    /// statut : il vient de se déclarer au courant.
    fn set_activity(&self, uid: Id, conn: u64, idle: bool) -> bool {
        let mut conns = self.conns.write().unwrap();
        let Some(c) = conns.get_mut(&uid).and_then(|l| l.iter_mut().find(|c| c.id == conn)) else { return false };
        c.idle = idle;
        !std::mem::replace(&mut c.presence_aware, true)
    }

    /// Le statut choisi par cette personne, si elle est connectée.
    pub fn choice(&self, uid: Id) -> Option<Presence> {
        self.choices.read().unwrap().get(&uid).copied()
    }

    /// Retient le nouveau choix d'une personne connectée.
    pub fn set_choice(&self, uid: Id, presence: Presence) {
        if let Some(c) = self.choices.write().unwrap().get_mut(&uid) {
            *c = presence;
        }
    }

    /// Le statut de cette personne tel que les autres le voient : hors ligne sans connexion ou en
    /// mode invisible ; sinon son choix, et « inactif » à la place de « en ligne » quand tous ses
    /// clients le sont.
    pub fn status(&self, uid: Id) -> Status {
        let conns = self.conns.read().unwrap();
        let Some(list) = conns.get(&uid) else {
            drop(conns);
            return self.demo_presence.read().unwrap().get(&uid).copied().unwrap_or(Status::Offline);
        };
        match self.choice(uid).unwrap_or_default() {
            Presence::Invisible => Status::Offline,
            Presence::Dnd => Status::Dnd,
            Presence::Idle => Status::Idle,
            Presence::Online if list.iter().all(|c| c.idle) => Status::Idle,
            Presence::Online => Status::Online,
        }
    }

    pub fn with_presence(&self, mut rels: Vec<Relationship>) -> Vec<Relationship> {
        // Seuls les amis voient ta présence : pas une demande en attente, pas un compte bloqué.
        for r in &mut rels {
            r.status = if r.kind == RelationshipKind::Friend { self.status(r.user.id) } else { Status::Offline };
        }
        rels
    }

    pub async fn seed_presence(&self, db: &Db) {
        let demo = [
            ("lou", Status::Online),
            ("sam", Status::Idle),
            ("noa", Status::Dnd),
            ("abyssal_navigator.42", Status::Online),
        ];
        let mut found = Vec::new();
        for (login, status) in demo {
            if let Ok(Some((u, _))) = db.user_credentials(login).await {
                found.push((u.id, status));
            }
        }
        self.demo_presence.write().unwrap().extend(found);
    }
}

pub async fn upgrade(State(state): State<AppState>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    // Le cookie n'est accepté que depuis notre propre origine (détournement de WebSocket).
    let cookie = crate::auth::presented_token(&headers)
        .filter(|(_, from_cookie)| !from_cookie || crate::auth::same_origin(&headers, &state.cfg.public_url))
        .map(|(t, _)| t);
    ws.max_message_size(64 * 1024).on_upgrade(move |socket| session(state, socket, cookie))
}

async fn send(ws: &mut WebSocket, op: &ServerOp) -> bool {
    let txt = serde_json::to_string(op).expect("serializable");
    ws.send(WsMsg::Text(txt.into())).await.is_ok()
}

fn parse(msg: Option<Result<WsMsg, axum::Error>>) -> Option<Option<ClientOp>> {
    match msg? {
        Ok(WsMsg::Text(t)) => Some(serde_json::from_str(&t).ok()),
        Ok(WsMsg::Close(_)) | Err(_) => None,
        Ok(_) => Some(None),
    }
}

async fn session(state: AppState, mut ws: WebSocket, cookie: Option<String>) {
    if !send(&mut ws, &ServerOp::Hello { heartbeat_ms: HEARTBEAT_MS }).await {
        return;
    }
    let session = match timeout(Duration::from_secs(10), ws.recv()).await {
        Ok(m) => match parse(m) {
            Some(Some(ClientOp::Identify { token })) => token.or(cookie).map(|t| token_hash(&t)),
            _ => None,
        },
        Err(_) => None,
    };
    let user = match &session {
        Some(h) => match state.db.session_user(h).await {
            Ok(u) => u,
            // Panne passagère de la base : on coupe sans INVALID_SESSION, le client se reconnecte
            // au lieu de se déconnecter.
            Err(e) => {
                tracing::error!("IDENTIFY : {e:?}");
                return;
            }
        },
        None => None,
    };
    let (Some(user), Some(session)) = (user, session) else {
        send(&mut ws, &ServerOp::InvalidSession).await;
        return;
    };
    let uid = user.id;
    let choice = match state.db.presence(uid).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("IDENTIFY : {e:?}");
            return;
        }
    };
    let (conn, mut rx) = state.hub.register(uid, session, choice);

    let loaded = async {
        let private_channels = state.db.private_channels(uid).await?;
        let mine: Vec<Id> = private_channels.iter().map(|c| c.id).collect();
        let call_voice_states = match &state.voice {
            Some(v) => {
                v.private_states().into_iter().filter(|s| s.channel_id.is_some_and(|c| mine.contains(&c))).collect()
            }
            None => vec![],
        };
        Ok::<_, kraken_db::DbError>(Ready {
            user,
            guilds: state.db.user_guilds(uid).await?.into_iter().map(|g| state.with_voice(g)).collect(),
            calls: state.calls.of(&mine),
            call_voice_states,
            private_channels,
            relationships: state.hub.with_presence(state.db.relationships(uid).await?),
            read_states: state.db.read_states(uid).await?,
            notify_settings: state.db.notify_settings(uid).await?,
            session_id: format!("{conn:x}"),
            presence: choice,
            presences: crate::presence::snapshot(&state, uid, None).await?,
        })
    };
    let ready = match loaded.await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("READY : {e:?}");
            state.hub.unregister(uid, conn);
            return;
        }
    };
    if send(&mut ws, &ServerOp::Ready(Box::new(ready))).await {
        // Les autres apprennent que je suis là (si ce n'était pas déjà le cas).
        tokio::spawn(crate::presence::refresh(state.clone(), uid));
        let mut seq = 0u64;
        let grace = Duration::from_millis(u64::from(HEARTBEAT_MS) * 2);
        let mut deadline = Instant::now() + grace;
        loop {
            tokio::select! {
                ev = rx.recv() => {
                    let Some(event) = ev else { break };
                    seq += 1;
                    if !send(&mut ws, &ServerOp::Dispatch { seq, event }).await { break }
                }
                m = ws.recv() => match parse(m) {
                    None => break,
                    Some(Some(ClientOp::Heartbeat { .. })) => {
                        deadline = Instant::now() + grace;
                        if !send(&mut ws, &ServerOp::HeartbeatAck).await { break }
                    }
                    Some(Some(ClientOp::Activity { idle })) => {
                        let first = state.hub.set_activity(uid, conn, idle);
                        tokio::spawn(crate::presence::refresh(state.clone(), uid));
                        if first {
                            // Ce qui a été annoncé avant qu'il se déclare ne lui est pas parvenu.
                            tokio::spawn(crate::presence::resync(state.clone(), uid));
                        }
                    }
                    Some(_) => {}
                },
                _ = sleep_until(deadline) => break,
            }
        }
    }
    state.hub.unregister(uid, conn);
    // Le dernier appareil parti : les autres me voient hors ligne.
    tokio::spawn(crate::presence::refresh(state, uid));
}
