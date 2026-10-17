//! Gateway WebSocket : HELLO → IDENTIFY → READY, puis DISPATCH numérotés et battements.
//! Chaque connexion a une file bornée ; un client trop lent est déconnecté (il se reconnecte).

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message as WsMsg, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use kraken_db::Db;
use kraken_proto::*;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};

use crate::AppState;
use crate::auth::token_hash;

const HEARTBEAT_MS: u32 = 30_000;
const QUEUE: usize = 256;

/// Connexions ouvertes d'un utilisateur : (numéro de connexion, file d'envoi).
type Conns = Vec<(u64, mpsc::Sender<Event>)>;

#[derive(Default)]
pub struct Hub {
    conns: RwLock<HashMap<Id, Conns>>,
    next: AtomicU64,
    /// Présence fixe des comptes de démo (ils ne se connectent jamais).
    demo_presence: RwLock<HashMap<Id, Status>>,
}

impl Hub {
    /// Envoie l'événement à toutes les connexions de ces utilisateurs.
    pub fn send(&self, users: &[Id], ev: Event) {
        let mut conns = self.conns.write().unwrap();
        for u in users {
            if let Some(list) = conns.get_mut(u) {
                // File pleine ou connexion fermée : on lâche l'émetteur, la connexion se termine.
                list.retain(|(_, tx)| tx.try_send(ev.clone()).is_ok());
            }
        }
    }

    fn register(&self, uid: Id) -> (u64, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.conns.write().unwrap().entry(uid).or_default().push((id, tx));
        (id, rx)
    }

    fn unregister(&self, uid: Id, conn: u64) {
        let mut conns = self.conns.write().unwrap();
        if let Some(list) = conns.get_mut(&uid) {
            list.retain(|(id, _)| *id != conn);
            if list.is_empty() {
                conns.remove(&uid);
            }
        }
    }

    pub fn status(&self, uid: Id) -> Status {
        if self.conns.read().unwrap().contains_key(&uid) {
            return Status::Online;
        }
        self.demo_presence.read().unwrap().get(&uid).copied().unwrap_or(Status::Offline)
    }

    pub fn with_presence(&self, mut rels: Vec<Relationship>) -> Vec<Relationship> {
        for r in &mut rels {
            r.status = self.status(r.user.id);
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
            if let Some((u, _)) = db.user_credentials(login).await {
                found.push((u.id, status));
            }
        }
        self.demo_presence.write().unwrap().extend(found);
    }
}

pub async fn upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(64 * 1024).on_upgrade(move |socket| session(state, socket))
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

async fn session(state: AppState, mut ws: WebSocket) {
    if !send(&mut ws, &ServerOp::Hello { heartbeat_ms: HEARTBEAT_MS }).await {
        return;
    }
    let user = match timeout(Duration::from_secs(10), ws.recv()).await {
        Ok(m) => match parse(m) {
            Some(Some(ClientOp::Identify { token })) => state.db.session_user(&token_hash(&token)).await,
            _ => None,
        },
        Err(_) => None,
    };
    let Some(user) = user else {
        send(&mut ws, &ServerOp::InvalidSession).await;
        return;
    };
    let uid = user.id;
    let (conn, mut rx) = state.hub.register(uid);

    let mut guilds = Vec::new();
    for g in state.db.user_guilds(uid).await {
        guilds.push(crate::api::visible_state(&state, uid, g).await);
    }
    let ready = Ready {
        user,
        guilds,
        private_channels: state.db.private_channels(uid).await,
        relationships: state.hub.with_presence(state.db.relationships(uid).await),
        session_id: format!("{conn:x}"),
    };
    if send(&mut ws, &ServerOp::Ready(Box::new(ready))).await {
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
                    Some(_) => {}
                },
                _ = sleep_until(deadline) => break,
            }
        }
    }
    state.hub.unregister(uid, conn);
}
