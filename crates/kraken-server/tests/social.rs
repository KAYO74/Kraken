//! Social de bout en bout (F02, F03, F11) sur un vrai port et une vraie base : demandes d'ami
//! par pseudo et leurs erreurs, acceptation reçue en direct, blocage (plus de MP ni de demande
//! pour le compte bloqué, un tiers n'est pas gêné), groupes privés et leurs droits, limite.

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

    async fn request(&self, username: &str) -> (StatusCode, Value) {
        self.call(Method::POST, "/users/@me/relationships", Some(json!({ "username": username }))).await
    }

    async fn set(&self, other: &Who, kind: &str) -> (StatusCode, Value) {
        let path = format!("/users/@me/relationships/{}", other.user.id);
        self.call(Method::PUT, &path, Some(json!({ "kind": kind }))).await
    }

    async fn unset(&self, other: &Who) -> StatusCode {
        self.call(Method::DELETE, &format!("/users/@me/relationships/{}", other.user.id), None).await.0
    }

    async fn open(&self, with: &[&Who], name: Option<&str>) -> (StatusCode, Value) {
        let ids: Vec<Id> = with.iter().map(|w| w.user.id).collect();
        self.call(Method::POST, "/users/@me/channels", Some(json!({ "recipients": ids, "name": name }))).await
    }

    async fn say(&self, cid: Id, text: &str) -> StatusCode {
        self.call(Method::POST, &format!("/channels/{cid}/messages"), Some(json!({ "content": text }))).await.0
    }

    async fn rels(&self) -> Vec<Relationship> {
        serde_json::from_value(self.call(Method::GET, "/users/@me/relationships", None).await.1).unwrap()
    }

    async fn befriend(&self, other: &Who) {
        assert_eq!(self.request(&other.user.username).await.0, 201);
        assert_eq!(other.set(self, "friend").await.0, 200);
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

fn code(v: &Value) -> &str {
    v["code"].as_str().unwrap_or_default()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn friend_requests_live_and_their_errors(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let mut sam_gw = sam.gateway().await;

    // Erreurs claires : inconnu, pseudo impossible, soi-même, vide.
    let (s, v) = lou.request("personne").await;
    assert_eq!((s.as_u16(), code(&v)), (404, "unknown_user"));
    assert!(v["message"].as_str().unwrap().contains("personne"));
    assert_eq!(code(&lou.request("Pas Un Pseudo!").await.1), "unknown_user");
    assert_eq!(code(&lou.request("@LOU").await.1), "self");
    assert_eq!(lou.request("   ").await.0, 400);

    // Demande (avec @ et majuscules) : sam la reçoit en direct.
    let (s, v) = lou.request("@Sam").await;
    assert_eq!(s, 201);
    assert_eq!(v["kind"], "pending_outgoing");
    let Event::RelationshipAdd(r) = sam_gw.next().await else { panic!("RELATIONSHIP_ADD attendu") };
    assert_eq!((r.user.id, r.kind), (lou.user.id, RelationshipKind::PendingIncoming));
    assert_eq!(r.status, Status::Offline, "pas de présence avant l'amitié");
    assert_eq!(code(&lou.request("sam").await.1), "already_sent");

    // Sam accepte : amis des deux côtés, lou connecté apparaît en ligne chez sam.
    let mut lou_gw = lou.gateway().await;
    let (s, v) = sam.set(&lou, "friend").await;
    assert_eq!((s.as_u16(), v["kind"].as_str()), (200, Some("friend")));
    assert_eq!(v["status"], "online");
    assert!(matches!(lou_gw.next().await, Event::RelationshipUpdate(r) if r.kind == RelationshipKind::Friend));
    assert!(matches!(sam_gw.next().await, Event::RelationshipUpdate(r) if r.user.id == lou.user.id));
    assert_eq!(code(&sam.request("lou").await.1), "already_friends");
    assert_eq!(lou.rels().await.iter().map(|r| r.kind).collect::<Vec<_>>(), [RelationshipKind::Friend]);
    assert_eq!(sam.set(&lou, "pending_incoming").await.0, 400);

    // Retirer l'ami : prévenu de chaque côté ; rien à retirer ensuite.
    assert_eq!(lou.unset(&sam).await, 204);
    assert_eq!(lou_gw.next().await, Event::RelationshipRemove { user_id: sam.user.id });
    assert_eq!(sam_gw.next().await, Event::RelationshipRemove { user_id: lou.user.id });
    assert_eq!(lou.unset(&sam).await, 404);

    // Refuser une demande, en annuler une autre.
    lou.request("sam").await;
    assert_eq!(sam.unset(&lou).await, 204, "refusée");
    assert!(lou.rels().await.is_empty());
    lou.request("sam").await;
    assert_eq!(lou.unset(&sam).await, 204, "annulée");
    assert!(sam.rels().await.is_empty());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_blocked_account_can_neither_write_nor_ask(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    lou.befriend(&sam).await;
    let (s, dm) = sam.open(&[&lou], None).await;
    assert_eq!(s, 201);
    let dm: Channel = serde_json::from_value(dm).unwrap();
    assert_eq!(dm.recipients.iter().map(|u| u.id).collect::<Vec<_>>(), [lou.user.id]);
    assert_eq!(sam.open(&[&lou], None).await.0, 200, "le même MP, rouvert");
    assert_eq!(sam.say(dm.id, "salut").await, 201);

    // Lou bloque sam : l'amitié disparaît chez sam, sans mention du blocage.
    let mut sam_gw = sam.gateway().await;
    let (s, v) = lou.set(&sam, "blocked").await;
    assert_eq!((s.as_u16(), v["kind"].as_str()), (200, Some("blocked")));
    assert_eq!(sam_gw.next().await, Event::RelationshipRemove { user_id: lou.user.id });
    assert!(sam.rels().await.is_empty());

    // Sam ne peut plus écrire dans le MP, en ouvrir un, ni redemander lou en ami.
    let (s, v) = sam.call(Method::POST, &format!("/channels/{}/messages", dm.id), Some(json!({"content": "hé"}))).await;
    assert_eq!((s.as_u16(), code(&v)), (403, "dm_refused"));
    assert_eq!(code(&sam.open(&[&lou], None).await.1), "dm_refused");
    assert_eq!(code(&sam.request("lou").await.1), "request_refused");
    assert_eq!(code(&sam.set(&lou, "friend").await.1), "request_refused");
    // Lou non plus, tant qu'il le bloque ; l'historique reste lisible des deux côtés.
    assert_eq!(code(&lou.request("sam").await.1), "blocked");
    let (s, v) = lou.call(Method::POST, &format!("/channels/{}/messages", dm.id), Some(json!({"content": "x"}))).await;
    assert_eq!((s.as_u16(), code(&v)), (403, "dm_blocked"));
    assert_eq!(sam.call(Method::GET, &format!("/channels/{}/messages", dm.id), None).await.0, 200);

    // Un tiers n'est pas gêné : noa demande lou en ami, puis ils s'écrivent.
    lou.befriend(&noa).await;
    let (s, v) = noa.open(&[&lou], None).await;
    assert_eq!(s, 201);
    let other: Channel = serde_json::from_value(v).unwrap();
    assert_eq!(noa.say(other.id, "coucou").await, 201);
    // Lou ne peut pas mettre sam dans un groupe : ils ne sont plus amis.
    assert_eq!(code(&lou.open(&[&noa, &sam], None).await.1), "not_friend");

    // Débloquer : sam peut de nouveau demander, mais l'amitié est à refaire.
    assert_eq!(lou.unset(&sam).await, 204);
    assert_eq!(sam.say(dm.id, "re").await, 201);
    assert_eq!(sam.request("lou").await.0, 201);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn groups_rename_add_and_leave(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    let ilan = Who::new(&base, "ilan").await;
    let outsider = Who::new(&base, "oscar").await;
    for f in [&sam, &noa, &ilan] {
        lou.befriend(f).await;
    }
    // Pas de MP avec un inconnu sans serveur commun ; pas de groupe avec lui.
    assert_eq!(code(&outsider.open(&[&lou], None).await.1), "dm_stranger");
    assert_eq!(code(&lou.open(&[&sam, &outsider], None).await.1), "not_friend");
    assert_eq!(lou.open(&[], None).await.0, 400);

    let mut sam_gw = sam.gateway().await;
    let (s, v) = lou.open(&[&sam, &noa], Some("La bande")).await;
    assert_eq!(s, 201);
    let g: Channel = serde_json::from_value(v).unwrap();
    assert_eq!((g.kind, g.name.as_deref(), g.recipients.len()), (ChannelKind::GroupDm, Some("La bande"), 2));
    // Sam le reçoit, vu de chez lui : lou et noa.
    let Event::ChannelCreate(seen) = sam_gw.next().await else { panic!("CHANNEL_CREATE attendu") };
    assert_eq!(seen.recipients.iter().map(|u| u.id).collect::<Vec<_>>(), [lou.user.id, noa.user.id]);
    assert_eq!(sam.say(g.id, "présent").await, 201);
    assert!(matches!(sam_gw.next().await, Event::MessageCreate(_)));

    // Renommer : tout membre ; un intrus ne voit pas le groupe.
    let (s, v) = sam.call(Method::PATCH, &format!("/channels/{}", g.id), Some(json!({"name": "Les abysses"}))).await;
    assert_eq!((s.as_u16(), v["name"].as_str()), (200, Some("Les abysses")));
    assert!(matches!(sam_gw.next().await, Event::ChannelUpdate(c) if c.name.as_deref() == Some("Les abysses")));
    let path = format!("/channels/{}", g.id);
    assert_eq!(outsider.call(Method::PATCH, &path, Some(json!({"name": "x"}))).await.0, 404);
    let long = "x".repeat(MAX_GROUP_NAME + 1);
    assert_eq!(sam.call(Method::PATCH, &path, Some(json!({ "name": long }))).await.0, 400);
    assert_eq!(outsider.call(Method::GET, &format!("/channels/{}/messages", g.id), None).await.0, 404);

    // Ajouter : un ami de qui ajoute ; sam (pas ami avec ilan) ne peut pas, lou oui.
    let add = |who: &Who| format!("/channels/{}/recipients/{}", g.id, who.user.id);
    assert_eq!(code(&sam.call(Method::PUT, &add(&ilan), None).await.1), "not_friend");
    assert_eq!(outsider.call(Method::PUT, &add(&outsider), None).await.0, 404);
    assert_eq!(lou.call(Method::PUT, &add(&ilan), None).await.0, 204);
    assert!(matches!(sam_gw.next().await, Event::ChannelUpdate(c) if c.recipients.len() == 3));
    assert_eq!(ilan.say(g.id, "me voilà").await, 201);
    assert!(matches!(sam_gw.next().await, Event::MessageCreate(m) if m.content == "me voilà"));

    // Retirer quelqu'un : le propriétaire seulement. Quitter : chacun.
    assert_eq!(sam.call(Method::DELETE, &add(&noa), None).await.0, 403);
    assert_eq!(sam.call(Method::DELETE, &format!("/channels/{}/recipients/@me", g.id), None).await.0, 204);
    assert_eq!(sam_gw.next().await, Event::ChannelDelete { id: g.id });
    assert_eq!(sam.say(g.id, "encore là ?").await, 404);
    assert_eq!(lou.call(Method::DELETE, &add(&ilan), None).await.0, 204);
    assert_eq!(ilan.call(Method::GET, &format!("/channels/{}/messages", g.id), None).await.0, 404);
    let mine: Vec<Channel> =
        serde_json::from_value(lou.call(Method::GET, "/users/@me/channels", None).await.1).unwrap();
    assert_eq!(mine.iter().find(|c| c.id == g.id).map(|c| c.recipients.len()), Some(1));
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn friend_requests_are_rate_limited(pool: PgPool) {
    // 3 par minute : l'inscription passe par un autre compteur.
    let base = spawn(pool, 3).await;
    let lou = Who::new(&base, "lou").await;
    let mut codes = vec![];
    for _ in 0..4 {
        codes.push(lou.request("personne").await.0.as_u16());
    }
    assert_eq!(codes, [404, 404, 404, 429]);
    // Lire sa liste n'est pas limité.
    assert_eq!(lou.call(Method::GET, "/users/@me/relationships", None).await.0, 200);
}
