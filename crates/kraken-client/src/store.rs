//! État global réactif et connexion au gateway. Les écrans lisent ces signaux ; seuls le
//! gateway et les réponses REST les écrivent.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{Message as WsMsg, futures::WebSocket};
use gloo_timers::future::TimeoutFuture;
use kraken_proto::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

/// Simple indicateur « connecté » : la session elle-même est dans un cookie `HttpOnly`
/// que le code du client ne peut pas lire.
const SIGNED_IN_KEY: &str = "kraken.signed_in";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn saved_signed_in() -> bool {
    storage().and_then(|s| s.get_item(SIGNED_IN_KEY).ok().flatten()).is_some()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conn {
    Offline,
    Connecting,
    Ready,
}

/// Envoi optimiste, affiché grisé jusqu'au MESSAGE_CREATE portant le même nonce.
#[derive(Clone, PartialEq)]
pub struct Pending {
    pub nonce: String,
    pub content: String,
    pub error: Option<String>,
}

#[derive(Clone, Default, PartialEq)]
pub struct Log {
    pub loaded: bool,
    pub error: Option<String>,
    pub items: Vec<Message>,
    pub pending: Vec<Pending>,
}

impl Log {
    /// Ajoute un message à sa place dans l'ordre des ids, sans doublon : deux envois
    /// simultanés peuvent arriver dans le désordre.
    pub fn insert(&mut self, m: Message) {
        let at = self.items.partition_point(|x| x.id < m.id);
        if self.items.get(at).is_none_or(|x| x.id != m.id) {
            self.items.insert(at, m);
        }
    }
}

#[derive(Clone, Copy)]
pub struct Store {
    pub signed_in: RwSignal<bool>,
    pub me: RwSignal<Option<User>>,
    pub guilds: RwSignal<Vec<GuildState>>,
    pub dms: RwSignal<Vec<Channel>>,
    pub friends: RwSignal<Vec<Relationship>>,
    pub logs: RwSignal<HashMap<Id, Log>>,
    pub conn: RwSignal<Conn>,
    /// Tiroir de navigation sous 640 px.
    pub drawer: RwSignal<bool>,
}

pub fn store() -> Store {
    expect_context()
}

impl Store {
    pub fn new() -> Self {
        Self {
            signed_in: RwSignal::new(saved_signed_in()),
            me: RwSignal::new(None),
            guilds: RwSignal::new(vec![]),
            dms: RwSignal::new(vec![]),
            friends: RwSignal::new(vec![]),
            logs: RwSignal::new(HashMap::new()),
            conn: RwSignal::new(Conn::Offline),
            drawer: RwSignal::new(false),
        }
    }

    pub fn sign_in(&self, auth: AuthResp) {
        if let Some(s) = storage() {
            let _ = s.set_item(SIGNED_IN_KEY, "1");
        }
        self.me.set(Some(auth.user));
        self.signed_in.set(true);
    }

    pub fn sign_out(&self) {
        if let Some(s) = storage() {
            let _ = s.remove_item(SIGNED_IN_KEY);
        }
        self.signed_in.set(false);
        self.me.set(None);
        self.guilds.set(vec![]);
        self.dms.set(vec![]);
        self.friends.set(vec![]);
        self.logs.set(HashMap::new());
    }

    pub fn guild(&self, id: Id) -> Option<GuildState> {
        self.guilds.with(|gs| gs.iter().find(|g| g.guild.id == id).cloned())
    }

    pub fn channel(&self, id: Id) -> Option<Channel> {
        self.guilds
            .with(|gs| gs.iter().flat_map(|g| &g.channels).find(|c| c.id == id).cloned())
            .or_else(|| self.dms.with(|d| d.iter().find(|c| c.id == id).cloned()))
    }

    pub fn upsert_guild(&self, g: GuildState) {
        self.guilds.update(|gs| match gs.iter_mut().find(|x| x.guild.id == g.guild.id) {
            Some(x) => *x = g,
            None => gs.push(g),
        });
    }

    pub fn log(&self, channel: Id, f: impl FnOnce(&mut Log)) {
        self.logs.update(|l| f(l.entry(channel).or_default()));
    }

    /// Remplace partout la fiche d'un utilisateur (nom affiché changé).
    pub fn update_user(&self, u: User) {
        if self.me.with_untracked(|m| m.as_ref().is_some_and(|m| m.id == u.id)) {
            self.me.set(Some(u.clone()));
        }
        self.guilds.update(|gs| {
            gs.iter_mut().flat_map(|g| &mut g.members).filter(|m| m.user.id == u.id).for_each(|m| m.user = u.clone())
        });
        self.friends.update(|fs| fs.iter_mut().filter(|r| r.user.id == u.id).for_each(|r| r.user = u.clone()));
        self.dms.update(|ds| {
            ds.iter_mut().flat_map(|c| &mut c.recipients).filter(|r| r.id == u.id).for_each(|r| *r = u.clone())
        });
        self.logs.update(|ls| {
            ls.values_mut()
                .flat_map(|l| &mut l.items)
                .filter_map(|m| m.author.as_mut())
                .filter(|a| a.id == u.id)
                .for_each(|a| *a = u.clone())
        });
    }

    fn apply(&self, ev: Event) {
        match ev {
            Event::MessageCreate(m) => self.log(m.channel_id, |l| {
                l.pending.retain(|p| Some(&p.nonce) != m.nonce.as_ref());
                l.insert(m);
            }),
            Event::ChannelCreate(c) => {
                // Déjà là si l'événement a croisé le READY.
                let Some(gid) = c.guild_id else {
                    self.dms.update(|d| {
                        if !d.iter().any(|x| x.id == c.id) {
                            d.push(c)
                        }
                    });
                    return;
                };
                self.guilds.update(|gs| {
                    if let Some(g) = gs.iter_mut().find(|g| g.guild.id == gid)
                        && !g.channels.iter().any(|x| x.id == c.id)
                    {
                        g.channels.push(c);
                    }
                });
            }
            Event::GuildCreate(g) => self.upsert_guild(*g),
            Event::UserUpdate(u) => self.update_user(u),
            Event::MemberAdd { guild_id, member } => self.guilds.update(|gs| {
                if let Some(g) = gs.iter_mut().find(|g| g.guild.id == guild_id)
                    && !g.members.iter().any(|m| m.user.id == member.user.id)
                {
                    g.members.push(member);
                }
            }),
        }
    }
}

/// Garde une connexion au gateway tant qu'une session existe ; reconnecte après une coupure.
pub fn keep_connected(store: Store) {
    let running = Rc::new(Cell::new(false));
    Effect::new(move || {
        if !store.signed_in.get() || running.get() {
            return;
        }
        running.set(true);
        let running = running.clone();
        spawn_local(async move {
            while store.signed_in.get_untracked() {
                run(store).await;
                store.conn.set(Conn::Offline);
                TimeoutFuture::new(2_000).await;
            }
            running.set(false);
        });
    });
}

fn gateway_url() -> String {
    let loc = web_sys::window().unwrap().location();
    let scheme = if loc.protocol().as_deref() == Ok("https:") { "wss" } else { "ws" };
    format!("{scheme}://{}/gateway", loc.host().unwrap_or_default())
}

async fn run(store: Store) {
    store.conn.set(Conn::Connecting);
    let Ok(ws) = WebSocket::open(&gateway_url()) else { return };
    let (mut sink, mut stream) = ws.split();
    let mut next = async move || -> Option<ServerOp> {
        loop {
            match stream.next().await? {
                Ok(WsMsg::Text(t)) => return serde_json::from_str(&t).ok(),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    };
    let Some(ServerOp::Hello { heartbeat_ms }) = next().await else { return };
    let (out, mut out_rx) = futures::channel::mpsc::unbounded::<ClientOp>();
    spawn_local(async move {
        while let Some(op) = out_rx.next().await {
            let txt = serde_json::to_string(&op).unwrap();
            if sink.send(WsMsg::Text(txt)).await.is_err() {
                break;
            }
        }
    });
    let _ = out.unbounded_send(ClientOp::Identify { token: None });
    let seq = Rc::new(Cell::new(0u64));
    {
        let (out, seq) = (out.clone(), seq.clone());
        spawn_local(async move {
            loop {
                TimeoutFuture::new(heartbeat_ms).await;
                if out.unbounded_send(ClientOp::Heartbeat { seq: seq.get() }).is_err() {
                    break;
                }
            }
        });
    }
    while let Some(op) = next().await {
        match op {
            ServerOp::Ready(r) => {
                let r = *r;
                store.me.set(Some(r.user));
                store.guilds.set(r.guilds);
                store.dms.set(r.private_channels);
                store.friends.set(r.relationships);
                // Des messages ont pu passer pendant la coupure : on recharge à l'affichage.
                store.logs.update(|l| l.values_mut().for_each(|x| x.loaded = false));
                store.conn.set(Conn::Ready);
            }
            ServerOp::Dispatch { seq: s, event } => {
                seq.set(s);
                store.apply(event);
            }
            ServerOp::InvalidSession => {
                store.sign_out();
                break;
            }
            ServerOp::Hello { .. } | ServerOp::HeartbeatAck => {}
        }
    }
    out.close_channel();
}
