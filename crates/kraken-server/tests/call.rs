//! Appels des MP et des groupes, sur un vrai port et une vraie base : qui peut appeler (et pas
//! un inconnu ni un compte bloqué), chez qui ça sonne, refuser, sonnerie qui s'arrête seule,
//! fin de l'appel au départ du dernier, et le message d'appel qui en garde la trace.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use kraken_proto::*;
use kraken_server::{AppState, Config};
use serde_json::json;
use sqlx::PgPool;
use str0m::media::{Direction, MediaKind};
use str0m::{Candidate, Rtc};
use tokio_tungstenite::tungstenite::Message as Ws;

type Gateway = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn config(ring_secs: u64) -> Config {
    Config {
        addr: "127.0.0.1:0".into(),
        web_dir: "/nonexistent".into(),
        database_url: String::new(),
        public_url: "http://127.0.0.1:8080/".into(),
        smtp_url: None,
        smtp_from: "KRAKEN <no-reply@localhost>".into(),
        seed: false,
        trust_proxy: false,
        auth_per_min: 1000,
        instance: Default::default(),
        rtc_port: 0,
        rtc_fallback: false,
        rtc_ips: vec![std::net::Ipv4Addr::LOCALHOST.into()],
        rtc_public_port: None,
        ring_secs,
    }
}

struct Api {
    c: reqwest::Client,
    base: String,
}

/// Un compte : son id et son en-tête d'authentification.
struct Who {
    id: Id,
    auth: String,
}

impl Api {
    async fn spawn(pool: PgPool, ring_secs: u64) -> Self {
        let state = AppState::with_db(kraken_db::Db::from_pool(pool), config(ring_secs)).await.unwrap();
        let app = kraken_server::app(state);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        Self { c: reqwest::Client::new(), base }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}/api/v1{path}", self.base)
    }

    async fn register(&self, name: &str) -> Who {
        let r = self
            .c
            .post(self.url("/auth/register"))
            .json(&json!({"username": name, "email": format!("{name}@example.org"), "password": "motdepasse"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 201);
        let a: AuthResp = r.json().await.unwrap();
        Who { id: a.user.id, auth: format!("Session {}", a.token) }
    }

    async fn befriend(&self, a: &Who, b: &Who, b_name: &str) {
        let r = self.c.post(self.url("/users/@me/relationships")).header("authorization", &a.auth);
        assert_eq!(r.json(&json!({"username": b_name})).send().await.unwrap().status(), 201);
        let r = self.c.put(self.url(&format!("/users/@me/relationships/{}", a.id))).header("authorization", &b.auth);
        assert_eq!(r.json(&json!({"kind": "friend"})).send().await.unwrap().status(), 200);
    }

    async fn open(&self, me: &Who, with: &[&Who]) -> Channel {
        let ids: Vec<Id> = with.iter().map(|w| w.id).collect();
        let r = self.c.post(self.url("/users/@me/channels")).header("authorization", &me.auth);
        let r = r.json(&json!({"recipients": ids})).send().await.unwrap();
        assert!(r.status().is_success(), "{}", r.status());
        r.json().await.unwrap()
    }

    async fn join(&self, me: &Who, cid: Id) -> reqwest::Response {
        let r = self.c.post(self.url(&format!("/channels/{cid}/voice"))).header("authorization", &me.auth);
        r.json(&offer()).send().await.unwrap()
    }

    async fn leave(&self, me: &Who) {
        let r = self.c.delete(self.url("/voice")).header("authorization", &me.auth).send().await.unwrap();
        assert_eq!(r.status(), 204);
    }

    async fn decline(&self, me: &Who, cid: Id) -> reqwest::StatusCode {
        let r = self.c.post(self.url(&format!("/channels/{cid}/call/decline"))).header("authorization", &me.auth);
        r.send().await.unwrap().status()
    }

    async fn block(&self, me: &Who, other: &Who) {
        let r =
            self.c.put(self.url(&format!("/users/@me/relationships/{}", other.id))).header("authorization", &me.auth);
        assert_eq!(r.json(&json!({"kind": "blocked"})).send().await.unwrap().status(), 200);
    }

    async fn messages(&self, me: &Who, cid: Id) -> Vec<Message> {
        let r = self.c.get(self.url(&format!("/channels/{cid}/messages"))).header("authorization", &me.auth);
        r.send().await.unwrap().json().await.unwrap()
    }

    async fn feed(&self, me: &Who) -> Feed {
        Feed { ws: self.gateway(me).await.0, kept: vec![] }
    }

    async fn gateway(&self, me: &Who) -> (Gateway, Box<Ready>) {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/gateway", self.base)).await.unwrap();
        assert!(matches!(read(&mut ws).await, ServerOp::Hello { .. }));
        let token = me.auth.trim_start_matches("Session ").to_string();
        let id = ClientOp::Identify { token: Some(token) };
        ws.send(Ws::Text(serde_json::to_string(&id).unwrap().into())).await.unwrap();
        let ServerOp::Ready(ready) = read(&mut ws).await else { panic!("READY attendu") };
        (ws, ready)
    }
}

/// L'offre d'un navigateur : micro, image et son de l'écran, puis le canal de données.
fn offer() -> VoiceJoin {
    let crypto = Arc::new(str0m::crypto::from_feature_flags());
    let mut rtc = Rtc::builder().set_crypto_provider(crypto).build(Instant::now());
    rtc.add_local_candidate(Candidate::host("127.0.0.1:9".parse().unwrap(), "udp").unwrap());
    let mut api = rtc.sdp_api();
    let mic = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
    let screen = api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
    let screen_audio = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
    api.add_channel(VOICE_DATA_CHANNEL.into());
    let (sdp, _) = api.apply().unwrap();
    VoiceJoin {
        sdp: sdp.to_sdp_string(),
        mids: VoiceMids { mic: mic.to_string(), screen: screen.to_string(), screen_audio: screen_audio.to_string() },
        self_mute: false,
        self_deaf: false,
    }
}

async fn read(ws: &mut Gateway) -> ServerOp {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(8), ws.next()).await.expect("événement attendu");
        if let Ws::Text(t) = m.unwrap().unwrap() {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

/// Les événements d'une connexion au gateway ; ceux qu'on n'attend pas encore restent de côté
/// (l'ordre entre états vocaux et appels n'est pas garanti).
struct Feed {
    ws: Gateway,
    kept: Vec<Event>,
}

impl Feed {
    /// Le premier événement (gardé ou à venir) que `pick` retient.
    async fn next<T>(&mut self, pick: impl Fn(&Event) -> Option<T>) -> T {
        if let Some(i) = self.kept.iter().position(|e| pick(e).is_some()) {
            return pick(&self.kept.remove(i)).unwrap();
        }
        loop {
            if let ServerOp::Dispatch { event, .. } = read(&mut self.ws).await {
                if let Some(x) = pick(&event) {
                    return x;
                }
                self.kept.push(event);
            }
        }
    }

    async fn call(&mut self) -> Call {
        self.next(|e| if let Event::CallUpdate(c) = e { Some(c.clone()) } else { None }).await
    }

    async fn ended(&mut self) -> Id {
        self.next(|e| if let Event::CallDelete { channel_id } = e { Some(*channel_id) } else { None }).await
    }

    async fn updated(&mut self) -> Message {
        self.next(|e| if let Event::MessageUpdate(m) = e { Some((**m).clone()) } else { None }).await
    }

    async fn created(&mut self) -> Message {
        self.next(|e| if let Event::MessageCreate(m) = e { Some((**m).clone()) } else { None }).await
    }

    async fn voice(&mut self, user: Id) -> VoiceState {
        self.next(|e| match e {
            Event::VoiceStateUpdate(s) if s.user_id == user => Some(s.clone()),
            _ => None,
        })
        .await
    }
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn one_to_one_call(pool: PgPool) {
    let api = Api::spawn(pool, 30).await;
    let lou = api.register("lou").await;
    let sam = api.register("sam").await;
    let noa = api.register("noa").await;
    api.befriend(&lou, &sam, "sam").await;
    let dm = api.open(&lou, &[&sam]).await.id;

    // Hors de la conversation : elle n'existe pas. Sans session : 401.
    assert_eq!(api.join(&noa, dm).await.status(), 404);
    assert_eq!(api.decline(&noa, dm).await, 404);
    let anonymous = api.c.post(api.url(&format!("/channels/{dm}/voice"))).json(&offer()).send().await.unwrap();
    assert_eq!(anonymous.status(), 401);

    // Lou appelle : le message d'appel, puis ça sonne chez Sam, qui voit Lou dans l'appel.
    let mut ws = api.feed(&sam).await;
    let (mut ws_noa, _) = api.gateway(&noa).await;
    assert_eq!(api.join(&lou, dm).await.status(), 200);
    let m = ws.created().await;
    assert_eq!((m.channel_id, m.author.as_ref().map(|u| u.id)), (dm, Some(lou.id)));
    assert_eq!(m.call, Some(CallInfo { participants: vec![lou.id], ended_at: None }));
    let call = ws.call().await;
    assert_eq!(call, Call { channel_id: dm, message_id: m.id, started_by: lou.id, ringing: vec![sam.id] });
    let here = ws.voice(lou.id).await;
    assert_eq!((here.guild_id, here.channel_id, here.user_id), (None, Some(dm), lou.id));

    // Qui se connecte maintenant le voit aussi (READY).
    let (_, ready) = api.gateway(&sam).await;
    assert_eq!(ready.calls, std::slice::from_ref(&call));
    assert_eq!(ready.call_voice_states, std::slice::from_ref(&here));
    let (_, ready_noa) = api.gateway(&noa).await;
    assert!(ready_noa.calls.is_empty() && ready_noa.call_voice_states.is_empty());

    // Sam décroche : ça ne sonne plus.
    assert_eq!(api.join(&sam, dm).await.status(), 200);
    assert_eq!(ws.call().await.ringing, Vec::<Id>::new());
    // Lou part, revient (l'appel continue tant que quelqu'un y est), puis les deux partent.
    api.leave(&lou).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(api.join(&lou, dm).await.status(), 200);
    api.leave(&lou).await;
    api.leave(&sam).await;
    let done = ws.updated().await;
    assert_eq!(ws.ended().await, dm);
    assert_eq!(done.id, m.id);
    let info = done.call.clone().unwrap();
    assert_eq!(info.participants, [lou.id, sam.id]);
    assert!(info.ended_at.is_some());
    // Un seul message d'appel : revenir dans un appel en cours n'en crée pas d'autre.
    let calls: Vec<Message> = api.messages(&sam, dm).await.into_iter().filter(|m| m.call.is_some()).collect();
    assert_eq!(calls, [done]);
    // Noa n'a rien reçu de cet appel.
    let quiet = tokio::time::timeout(Duration::from_millis(300), ws_noa.next()).await;
    assert!(quiet.is_err(), "rien pour qui n'est pas de la conversation : {quiet:?}");

    // Un blocage, dans un sens comme dans l'autre, empêche d'appeler.
    api.block(&sam, &lou).await;
    let r = api.join(&lou, dm).await;
    assert_eq!(r.status(), 403);
    assert_eq!(r.json::<ApiError>().await.unwrap().code, "call_refused");
    let r = api.join(&sam, dm).await;
    assert_eq!(r.status(), 403);
    assert_eq!(r.json::<ApiError>().await.unwrap().code, "call_blocked");
    let after: Vec<Message> = api.messages(&lou, dm).await.into_iter().filter(|m| m.call.is_some()).collect();
    assert_eq!(after.len(), 1, "un appel refusé ne laisse pas de message");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn group_call_decline_timeout_and_blocks(pool: PgPool) {
    let api = Api::spawn(pool, 2).await;
    let lou = api.register("lou").await;
    let sam = api.register("sam").await;
    let noa = api.register("noa").await;
    let zed = api.register("zed").await;
    api.befriend(&lou, &sam, "sam").await;
    api.befriend(&lou, &noa, "noa").await;
    api.befriend(&lou, &zed, "zed").await;
    let group = api.open(&lou, &[&sam, &noa, &zed]).await.id;
    // Zed a bloqué Lou : ça ne sonnera pas chez lui, mais il reste du groupe.
    api.block(&zed, &lou).await;

    let mut ws = api.feed(&sam).await;
    assert_eq!(api.join(&lou, group).await.status(), 200);
    let call = ws.call().await;
    let mut ringing = call.ringing.clone();
    ringing.sort();
    let mut expected = vec![sam.id, noa.id];
    expected.sort();
    assert_eq!(ringing, expected);

    // Noa refuse ; refuser encore ne change rien.
    assert_eq!(api.decline(&noa, group).await, 204);
    assert_eq!(ws.call().await.ringing, [sam.id]);
    assert_eq!(api.decline(&noa, group).await, 204);
    // Personne d'autre ne répond : la sonnerie s'arrête seule.
    let started = Instant::now();
    assert_eq!(ws.call().await.ringing, Vec::<Id>::new());
    assert!(started.elapsed() < Duration::from_secs(4));

    // Zed peut quand même entrer dans l'appel du groupe.
    assert_eq!(api.join(&zed, group).await.status(), 200);
    // Retiré du groupe pendant l'appel : il en sort.
    let r =
        api.c.delete(api.url(&format!("/channels/{group}/recipients/{}", zed.id))).header("authorization", &lou.auth);
    assert_eq!(r.send().await.unwrap().status(), 204);
    assert_eq!(ws.voice(zed.id).await.channel_id, Some(group));
    let gone = ws.voice(zed.id).await;
    assert_eq!((gone.guild_id, gone.channel_id), (None, None));

    // Lou raccroche, seul : fin de l'appel ; Sam et Noa l'ont manqué.
    api.leave(&lou).await;
    let done = ws.updated().await.call.unwrap();
    assert_eq!(ws.ended().await, group);
    let mut joined = done.participants.clone();
    joined.sort();
    let mut expected = vec![lou.id, zed.id];
    expected.sort();
    assert_eq!(joined, expected);
    assert!(!done.participants.contains(&sam.id) && !done.participants.contains(&noa.id));

    // Plus d'appel : un nouveau commence au prochain qui entre.
    assert_eq!(api.join(&sam, group).await.status(), 200);
    let again = ws.call().await;
    assert_ne!(again.message_id, call.message_id);
    let mut ringing = again.ringing.clone();
    ringing.sort();
    let mut expected = vec![lou.id, noa.id];
    expected.sort();
    assert_eq!(ringing, expected);
    api.leave(&sam).await;
    assert_eq!(ws.ended().await, group);
}
