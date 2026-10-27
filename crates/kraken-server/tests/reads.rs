//! Non-lus et notifications de bout en bout sur un vrai port et une vraie base : compteurs de
//! mentions dans `READY`, lecture synchronisée entre deux connexions d'un même compte, salons
//! invisibles refusés, réglages de notification et leurs droits, limite.

use futures_util::{SinkExt, StreamExt};
use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message as Ws;

fn config(auth_per_min: u32) -> Config {
    Config {
        addr: "127.0.0.1:0".into(),
        web_dir: "/nonexistent".into(),
        database_url: String::new(),
        public_url: "http://127.0.0.1".into(),
        smtp_url: None,
        smtp_from: "KRAKEN <no-reply@localhost>".into(),
        seed: false,
        trust_proxy: false,
        auth_per_min,
        instance: Default::default(),
        rtc_port: 0,
        rtc_fallback: false,
        rtc_ips: vec![std::net::Ipv4Addr::LOCALHOST.into()],
        rtc_public_port: None,
        ring_secs: 30,
    }
}

async fn spawn(pool: PgPool, per_min: u32) -> String {
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), config(per_min)).await.unwrap();
    let app = kraken_server::app(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("{addr}")
}

/// Un compte et de quoi appeler l'API en son nom.
struct Who {
    c: Client,
    base: String,
    token: String,
    user: User,
}

impl Who {
    async fn new(base: &str, name: &str) -> Self {
        let c = Client::new();
        let r = c
            .post(format!("http://{base}/api/v1/auth/register"))
            .json(&json!({"username": name, "email": format!("{name}@example.org"), "password": "motdepasse"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 201);
        let a: AuthResp = r.json().await.unwrap();
        Self { c, base: base.into(), token: a.token, user: a.user }
    }

    /// Statut et corps JSON (`null` si vide).
    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut rb = self
            .c
            .request(method, format!("http://{}/api/v1{path}", self.base))
            .header("authorization", format!("Session {}", self.token));
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let r = rb.send().await.unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn gateway(&self) -> Gateway {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/gateway", self.base)).await.unwrap();
        Gateway::read(&mut ws).await;
        let op = ClientOp::Identify { token: Some(self.token.clone()) };
        ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
        assert!(matches!(Gateway::read(&mut ws).await, ServerOp::Ready(_)));
        Gateway(ws)
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct Gateway(Socket);

impl Gateway {
    async fn read(ws: &mut Socket) -> ServerOp {
        loop {
            let m =
                tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await.expect("événement attendu");
            if let Ws::Text(t) = m.unwrap().unwrap() {
                return serde_json::from_str(&t).unwrap();
            }
        }
    }

    async fn next(&mut self) -> Event {
        match Self::read(&mut self.0).await {
            ServerOp::Dispatch { event, .. } => event,
            other => panic!("DISPATCH attendu : {other:?}"),
        }
    }
}

impl Gateway {
    /// Le prochain événement qui satisfait `f`, en passant les autres.
    async fn until(&mut self, f: impl Fn(&Event) -> bool) -> Event {
        loop {
            let e = self.next().await;
            if f(&e) {
                return e;
            }
        }
    }
}

fn acked(e: &Event) -> bool {
    matches!(e, Event::ReadStateUpdate(_))
}

impl Who {
    async fn ready(&self) -> Ready {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/gateway", self.base)).await.unwrap();
        Gateway::read(&mut ws).await;
        let op = ClientOp::Identify { token: Some(self.token.clone()) };
        ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
        match Gateway::read(&mut ws).await {
            ServerOp::Ready(r) => *r,
            other => panic!("READY attendu : {other:?}"),
        }
    }

    async fn post(&self, cid: Id, text: &str) -> Message {
        let (s, v) =
            self.call(Method::POST, &format!("/channels/{cid}/messages"), Some(json!({ "content": text }))).await;
        assert_eq!(s, 201, "{v}");
        serde_json::from_value(v).unwrap()
    }

    async fn ack(&self, cid: Id, mid: Id) -> (StatusCode, Value) {
        self.call(Method::POST, &format!("/channels/{cid}/ack/{mid}"), None).await
    }

    async fn notify(&self, body: Value) -> (StatusCode, Value) {
        self.call(Method::PUT, "/users/@me/notifications", Some(body)).await
    }
}

fn mentions(r: &Ready, cid: Id) -> u32 {
    r.read_states.iter().find(|s| s.channel_id == cid).map_or(0, |s| s.mention_count)
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn unread_mentions_sync_between_tabs(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    let (_, g) = lou.call(Method::POST, "/guilds", Some(json!({"name": "Abysse"}))).await;
    let g: GuildState = serde_json::from_value(g).unwrap();
    let gid = g.guild.id;
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let voice = g.channels.iter().find(|c| c.kind == ChannelKind::Voice).unwrap().id;
    let (_, inv) = lou.call(Method::POST, &format!("/channels/{general}/invites"), Some(json!({}))).await;
    let code = inv["code"].as_str().unwrap();
    assert_eq!(sam.call(Method::POST, &format!("/invites/{code}"), None).await.0, 200);

    // Deux onglets de sam ; lou le mentionne, puis répond à son message.
    let mut tab1 = sam.gateway().await;
    let mut tab2 = sam.gateway().await;
    lou.post(general, "bonjour à tous").await;
    let hey = lou.post(general, "@sam tu viens ?").await;
    let mine = sam.post(general, "j'arrive").await;
    let path = format!("/channels/{general}/messages");
    assert_eq!(lou.call(Method::POST, &path, Some(json!({ "content": "super", "reply_to": mine.id }))).await.0, 201);
    let last = lou.post(general, "@everyone sans le droit ? non, lou est propriétaire").await;
    let r = sam.ready().await;
    assert_eq!(mentions(&r, general), 2, "réponse + @everyone du propriétaire, après son propre message");
    let c = r.guilds[0].channels.iter().find(|c| c.id == general).unwrap();
    assert_eq!(c.last_message_id, Some(last.id));
    assert_eq!(r.read_states.iter().find(|s| s.channel_id == general).unwrap().last_read, Some(mine.id));
    assert_eq!(mentions(&lou.ready().await, general), 0, "ses propres messages");

    // sam lit tout : ses deux onglets le savent, et le compte suit au prochain READY.
    let (s, v) = sam.ack(general, last.id).await;
    assert_eq!((s.as_u16(), v["mention_count"].as_u64()), (200, Some(0)));
    for tab in [&mut tab1, &mut tab2] {
        let Event::ReadStateUpdate(states) = tab.until(acked).await else { unreachable!() };
        assert_eq!(states, vec![ReadState { channel_id: general, last_read: Some(last.id), mention_count: 0 }]);
    }
    assert_eq!(mentions(&sam.ready().await, general), 0);
    // Jamais en arrière : relire un ancien message ne change rien.
    let (_, v) = sam.ack(general, hey.id).await;
    assert_eq!(v["last_read"], json!(last.id));

    // Un salon que je ne vois pas, ou pas textuel, ou un message d'ailleurs : 404, sans rien révéler.
    assert_eq!(noa.ack(general, last.id).await.0, 404, "pas membre du serveur");
    let (_, dm) = lou.call(Method::POST, "/users/@me/channels", Some(json!({ "recipients": [sam.user.id] }))).await;
    let dm: Id = serde_json::from_value(dm["id"].clone()).unwrap();
    let in_dm = lou.post(dm, "en privé").await;
    assert_eq!(noa.ack(dm, in_dm.id).await.0, 404, "MP des autres");
    assert_eq!(sam.ack(general, in_dm.id).await.0, 404, "message d'un autre salon");
    assert_eq!(sam.ack(voice, last.id).await.0, 404, "salon vocal");
    assert_eq!(noa.call(Method::POST, &format!("/guilds/{gid}/ack"), None).await.0, 404);
    assert_eq!(lou.call(Method::POST, &format!("/channels/{dm}/ack/{}", in_dm.id), None).await.0, 200);
    let unauth = Client::new().post(format!("http://{base}/api/v1/channels/{dm}/ack/{}", in_dm.id)).send().await;
    assert_eq!(unauth.unwrap().status(), 401);

    // MP : tout message compte ; un message supprimé avant lecture ne compte plus.
    let gone = lou.post(dm, "oups").await;
    assert_eq!(mentions(&sam.ready().await, dm), 2);
    assert_eq!(lou.call(Method::DELETE, &format!("/channels/{dm}/messages/{}", gone.id), None).await.0, 204);
    let Event::ReadStateUpdate(states) =
        tab1.until(|e| acked(e) && matches!(e, Event::ReadStateUpdate(s) if s[0].channel_id == dm)).await
    else {
        unreachable!()
    };
    assert_eq!(states[0].mention_count, 1);

    // Tout le serveur lu d'un coup.
    lou.post(general, "@sam encore toi").await;
    let (s, v) = sam.call(Method::POST, &format!("/guilds/{gid}/ack"), None).await;
    assert_eq!(s, 200);
    assert_eq!(v.as_array().unwrap().len(), 1, "seul le salon qui avait changé");
    assert_eq!(mentions(&sam.ready().await, general), 0);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn notification_settings_rights_and_sync(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let (_, g) = lou.call(Method::POST, "/guilds", Some(json!({"name": "Abysse"}))).await;
    let g: GuildState = serde_json::from_value(g).unwrap();
    let (gid, general) = (g.guild.id, g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id);

    // Pas membre : 404, pour le serveur comme pour son salon ; requête mal formée : 400.
    assert_eq!(sam.notify(json!({ "guild_id": gid, "mute": "forever" })).await.0, 404);
    assert_eq!(sam.notify(json!({ "channel_id": general, "level": "none" })).await.0, 404);
    assert_eq!(lou.notify(json!({ "guild_id": gid, "channel_id": general })).await.0, 400);
    assert_eq!(lou.notify(json!({})).await.0, 400);
    assert_eq!(lou.notify(json!({ "guild_id": gid, "mute": { "minutes": 999999 } })).await.0, 400);

    // Mis en sourdine sur un appareil : l'autre le sait tout de suite, et au prochain READY.
    let mut other = lou.gateway().await;
    let (s, v) = lou.notify(json!({ "guild_id": gid, "mute": { "minutes": 60 }, "suppress_everyone": true })).await;
    assert_eq!(s, 200, "{v}");
    let Event::NotifySettingsUpdate(n) = other.until(|e| matches!(e, Event::NotifySettingsUpdate(_))).await else {
        unreachable!()
    };
    assert_eq!((n.guild_id, n.channel_id, n.level, n.suppress_everyone), (Some(gid), None, NotifyLevel::Default, true));
    assert!(n.muted_until.is_some());
    let (_, v) = lou.notify(json!({ "channel_id": general, "level": "all" })).await;
    assert_eq!((v["level"].as_str(), v["guild_id"].clone()), (Some("all"), json!(gid)));
    let r = lou.ready().await;
    assert_eq!(r.notify_settings.len(), 2);
    assert!(sam.ready().await.notify_settings.is_empty());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn notification_settings_are_rate_limited(pool: PgPool) {
    let base = spawn(pool, 3).await;
    let lou = Who::new(&base, "lou").await;
    let (_, g) = lou.call(Method::POST, "/guilds", Some(json!({"name": "Abysse"}))).await;
    let gid = g["guild"]["id"].clone();
    let mut codes = vec![];
    for _ in 0..4 {
        codes.push(lou.notify(json!({ "guild_id": gid, "mute": "off" })).await.0.as_u16());
    }
    assert_eq!(codes, [200, 200, 200, 429]);
}
