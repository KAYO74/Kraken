//! Photo de profil, fermeture d'un MP et comptes en cours de suppression, sur un vrai port et
//! une vraie base.

use std::io::Cursor;

use futures_util::{SinkExt, StreamExt};
use image::{ImageFormat, Rgb, RgbImage};
use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message as Ws;

fn config(data_dir: &std::path::Path) -> Config {
    Config {
        addr: "127.0.0.1:0".into(),
        web_dir: "/nonexistent".into(),
        data_dir: data_dir.to_string_lossy().into(),
        database_url: String::new(),
        public_url: "http://127.0.0.1".into(),
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
        ring_secs: 30,
    }
}

/// Serveur et dossier de données propre à ce test.
async fn spawn(pool: PgPool) -> (String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kraken-profile-{}", rand::random::<u64>()));
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), config(&dir)).await.unwrap();
    let app = kraken_server::app(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("{addr}"), dir)
}

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

    async fn put_avatar(&self, bytes: Vec<u8>) -> (StatusCode, Value) {
        let r = self
            .c
            .put(format!("http://{}/api/v1/users/@me/avatar", self.base))
            .header("authorization", format!("Session {}", self.token))
            .body(bytes)
            .send()
            .await
            .unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn befriend(&self, other: &Who) {
        let (s, _) =
            self.call(Method::POST, "/users/@me/relationships", Some(json!({"username": other.user.username}))).await;
        assert_eq!(s, 201);
        let path = format!("/users/@me/relationships/{}", self.user.id);
        assert_eq!(other.call(Method::PUT, &path, Some(json!({"kind": "friend"}))).await.0, 200);
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

fn picture(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 120]));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format).unwrap();
    out
}

fn code(v: &Value) -> &str {
    v["code"].as_str().unwrap_or_default()
}

async fn fetch(who: &Who, key: &str) -> reqwest::Response {
    Client::new().get(format!("http://{}/api/v1/avatars/{key}", who.base)).send().await.unwrap()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn avatar_upload_serve_replace_and_remove(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    lou.befriend(&sam).await;
    let mut sam_gw = sam.gateway().await;

    // Sans session : refusé, et rien n'est écrit.
    let anon = Client::new()
        .put(format!("http://{base}/api/v1/users/@me/avatar"))
        .body(picture(64, 64, ImageFormat::Png))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);
    assert_eq!(
        Client::new().delete(format!("http://{base}/api/v1/users/@me/avatar")).send().await.unwrap().status(),
        401
    );

    // Un PNG large : recadré au centre, rendu en 256 × 256.
    let (s, v) = lou.put_avatar(picture(300, 100, ImageFormat::Png)).await;
    assert_eq!(s, 200);
    let u: User = serde_json::from_value(v).unwrap();
    let key = u.avatar.clone().expect("clé d'avatar");
    assert!(
        matches!(sam_gw.next().await, Event::UserUpdate(seen) if seen.id == lou.user.id && seen.avatar.as_deref() == Some(key.as_str()))
    );

    // Un autre compte (et même un anonyme) la récupère, avec un cache sans fin.
    let r = fetch(&sam, &key).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert!(r.headers()["cache-control"].to_str().unwrap().contains("immutable"));
    let img = image::load_from_memory(&r.bytes().await.unwrap()).unwrap();
    assert_eq!((img.width(), img.height()), (AVATAR_SIZE, AVATAR_SIZE));
    assert_eq!(fetch(&sam, "../../etc/passwd").await.status(), 404);
    assert_eq!(fetch(&sam, &"0".repeat(32)).await.status(), 404);

    // Un JPEG remplace le PNG : l'ancien fichier disparaît.
    let (s, v) = lou.put_avatar(picture(120, 200, ImageFormat::Jpeg)).await;
    assert_eq!(s, 200);
    let second = v["avatar"].as_str().unwrap().to_string();
    assert_ne!(second, key);
    assert_eq!(fetch(&sam, &key).await.status(), 404);
    assert_eq!(fetch(&sam, &second).await.status(), 200);
    assert!(matches!(sam_gw.next().await, Event::UserUpdate(seen) if seen.avatar.as_deref() == Some(second.as_str())));

    // Retirer : retour aux initiales, fichier supprimé, les autres prévenus.
    let (s, v) = lou.call(Method::DELETE, "/users/@me/avatar", None).await;
    assert_eq!((s.as_u16(), v["avatar"].is_null()), (200, true));
    assert_eq!(fetch(&sam, &second).await.status(), 404);
    assert!(matches!(sam_gw.next().await, Event::UserUpdate(seen) if seen.avatar.is_none()));
    let left = std::fs::read_dir(dir.join("avatars")).unwrap().count();
    assert_eq!(left, 0);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn avatar_refuses_big_and_unreadable_files(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;

    let (s, v) = lou.put_avatar(vec![0u8; MAX_AVATAR_BYTES + 1]).await;
    assert_eq!((s.as_u16(), code(&v)), (413, "too_large"));
    let (s, v) = lou.put_avatar(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "not_image"));
    let (s, v) = lou.put_avatar(vec![]).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "not_image"));
    // Un PNG tronqué n'est pas une image non plus.
    let mut cut = picture(64, 64, ImageFormat::Png);
    cut.truncate(40);
    assert_eq!(lou.put_avatar(cut).await.0, 400);
    let me = lou.call(Method::GET, "/users/@me", None).await.1;
    assert!(me["avatar"].is_null(), "rien n'a été enregistré");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn closed_dm_comes_back_with_a_new_message(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    lou.befriend(&sam).await;
    lou.befriend(&noa).await;
    let (s, v) = lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id]}))).await;
    assert_eq!(s, 201);
    let dm: Channel = serde_json::from_value(v).unwrap();
    let list = |who: &Who| {
        let who_c = who.c.clone();
        let (base, token) = (who.base.clone(), who.token.clone());
        async move {
            let r = who_c
                .get(format!("http://{base}/api/v1/users/@me/channels"))
                .header("authorization", format!("Session {token}"))
                .send()
                .await
                .unwrap();
            r.json::<Vec<Channel>>().await.unwrap().into_iter().map(|c| c.id).collect::<Vec<_>>()
        }
    };

    let mut lou_gw = lou.gateway().await;
    let path = format!("/channels/{}", dm.id);
    // Étranger au MP, inconnu : introuvable. Fermer : pour moi seul.
    assert_eq!(noa.call(Method::DELETE, &path, None).await.0, 404);
    assert_eq!(lou.call(Method::DELETE, &path, None).await.0, 204);
    assert_eq!(lou_gw.next().await, Event::ChannelDelete { id: dm.id });
    assert!(list(&lou).await.is_empty());
    assert_eq!(list(&sam).await, [dm.id], "sam le garde");
    // L'historique reste lisible, et le prochain message le ramène avant lui.
    assert_eq!(lou.call(Method::GET, &format!("{path}/messages"), None).await.0, 200);
    assert_eq!(sam.call(Method::POST, &format!("{path}/messages"), Some(json!({"content": "coucou"}))).await.0, 201);
    assert!(matches!(lou_gw.next().await, Event::ChannelCreate(c) if c.id == dm.id));
    assert!(matches!(lou_gw.next().await, Event::MessageCreate(m) if m.content == "coucou"));
    assert_eq!(list(&lou).await, [dm.id]);

    // Fermé puis rouvert à la main : même salon, rendu à ma liste.
    assert_eq!(lou.call(Method::DELETE, &path, None).await.0, 204);
    assert_eq!(lou_gw.next().await, Event::ChannelDelete { id: dm.id });
    let (s, v) = lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id]}))).await;
    assert_eq!((s.as_u16(), v["id"].as_str().map(String::from)), (200, Some(dm.id.to_string())));
    assert!(matches!(lou_gw.next().await, Event::ChannelCreate(c) if c.id == dm.id));
    assert_eq!(list(&lou).await, [dm.id]);

    // Un groupe se quitte, il ne se ferme pas.
    let (_, g) =
        lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id, noa.user.id]}))).await;
    let gid = g["id"].as_str().unwrap();
    assert_eq!(lou.call(Method::DELETE, &format!("/channels/{gid}"), None).await.0, 400);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn account_pending_deletion_is_out_of_dms_groups_and_friends(pool: PgPool) {
    let (base, _) = spawn(pool.clone()).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    let ilan = Who::new(&base, "ilan").await;
    for f in [&sam, &noa, &ilan] {
        lou.befriend(f).await;
    }
    let open =
        |recipients: Vec<Id>| lou.call(Method::POST, "/users/@me/channels", Some(json!({ "recipients": recipients })));
    assert_eq!(open(vec![sam.user.id]).await.0, 201, "un MP existe déjà avec sam");
    let (_, v) = open(vec![noa.user.id, ilan.user.id]).await;
    let group = v["id"].as_str().unwrap().to_string();
    let mut lou_gw = lou.gateway().await;

    // Sam demande la suppression de son compte : ses liens d'amitié disparaissent, lou en est prévenu.
    let r = sam.call(Method::POST, "/users/@me/delete", Some(json!({"password": "motdepasse"}))).await;
    assert_eq!(r.0, 202);
    assert_eq!(lou_gw.next().await, Event::RelationshipRemove { user_id: sam.user.id });
    let rels: Vec<Relationship> =
        serde_json::from_value(lou.call(Method::GET, "/users/@me/relationships", None).await.1).unwrap();
    assert!(rels.iter().all(|r| r.user.id != sam.user.id));

    // Plus de MP avec lui, même celui qui existait. Pas de groupe non plus, même si une
    // amitié restait en base.
    assert_eq!(open(vec![sam.user.id]).await.0, 404);
    sqlx::query("insert into relationships (user_id, target_id, kind) values ($1, $2, 'friend')")
        .bind(lou.user.id)
        .bind(sam.user.id)
        .execute(&pool)
        .await
        .unwrap();
    let add = format!("/channels/{group}/recipients/{}", sam.user.id);
    assert_eq!(lou.call(Method::PUT, &add, None).await.0, 404);
    // Un compte actif, lui, s'ajoute toujours.
    let (_, v) = open(vec![noa.user.id]).await;
    assert!(v["id"].is_string());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn group_exposes_its_owner(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    lou.befriend(&sam).await;
    lou.befriend(&noa).await;
    let (_, v) =
        lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id, noa.user.id]}))).await;
    let g: Channel = serde_json::from_value(v).unwrap();
    assert_eq!(g.owner_id, Some(lou.user.id));
    let (_, v) = lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id]}))).await;
    assert_eq!(v["owner_id"], Value::Null, "pas de propriétaire affiché pour un MP");

    // Le propriétaire retire sam ; noa voit la mise à jour. Noa n'a pas ce droit.
    let mut noa_gw = noa.gateway().await;
    let member = |who: &Who| format!("/channels/{}/recipients/{}", g.id, who.user.id);
    assert_eq!(noa.call(Method::DELETE, &member(&sam), None).await.0, 403);
    assert_eq!(lou.call(Method::DELETE, &member(&sam), None).await.0, 204);
    assert!(
        matches!(noa_gw.next().await, Event::ChannelUpdate(c) if c.recipients.len() == 1 && c.owner_id == Some(lou.user.id))
    );
    // Il part : noa, seule restante, en hérite et son client l'apprend.
    assert_eq!(lou.call(Method::DELETE, &format!("/channels/{}/recipients/@me", g.id), None).await.0, 204);
    assert!(matches!(noa_gw.next().await, Event::ChannelUpdate(c) if c.owner_id == Some(noa.user.id)));
}
