//! Messages épinglés de bout en bout sur un vrai port et une vraie base : `PUT` et `DELETE
//! /channels/{id}/pins/{mid}`, `GET /channels/{id}/pins`, droit « Gérer les messages » dans un serveur (403), 404
//! hors du salon, MP et groupes ouverts à tous leurs participants, 50 au plus par salon (400 en français),
//! sans effet quand rien ne change (ni événement ni journal), événement `MESSAGE_UPDATE` en direct, journal
//! d'audit dans un serveur seulement, MP bloqué refusé comme l'envoi (403), limite de débit.

use futures_util::{SinkExt, StreamExt};
use kraken_core::perms;
use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message as Ws;

fn config(data_dir: &std::path::Path, auth_per_min: u32) -> Config {
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
        auth_per_min,
        instance: Default::default(),
        rtc_port: 0,
        rtc_fallback: false,
        rtc_ips: vec![std::net::Ipv4Addr::LOCALHOST.into()],
        rtc_public_port: None,
        ring_secs: 30,
        max_upload_bytes: 25 * 1024 * 1024,
        link_previews: true,
        preview_allow_private: false,
    }
}

async fn spawn(pool: PgPool) -> String {
    spawn_limited(pool, 1000).await
}

async fn spawn_limited(pool: PgPool, auth_per_min: u32) -> String {
    let dir = std::env::temp_dir().join(format!("kraken-pins-{}", rand::random::<u64>()));
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), config(&dir, auth_per_min)).await.unwrap();
    let app = kraken_server::app(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("{addr}")
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

    async fn new_guild(&self) -> GuildState {
        let (s, v) = self.call(Method::POST, "/guilds", Some(json!({ "name": "Abysse" }))).await;
        assert_eq!(s, 201, "{v}");
        serde_json::from_value(v).unwrap()
    }

    async fn join(&self, g: &GuildState, inviter: &Who) {
        let cid = general(g);
        let (s, v) = inviter.call(Method::POST, &format!("/channels/{cid}/invites"), Some(json!({}))).await;
        assert_eq!(s, 201);
        let code = v["code"].as_str().unwrap();
        assert_eq!(self.call(Method::POST, &format!("/invites/{code}"), None).await.0, 200);
    }

    async fn befriend(&self, other: &Who) {
        let (s, _) =
            self.call(Method::POST, "/users/@me/relationships", Some(json!({"username": other.user.username}))).await;
        assert_eq!(s, 201);
        let path = format!("/users/@me/relationships/{}", self.user.id);
        assert_eq!(other.call(Method::PUT, &path, Some(json!({"kind": "friend"}))).await.0, 200);
    }

    /// Écrit un message et rend son id.
    async fn say(&self, cid: Id, text: &str) -> Id {
        let (s, v) =
            self.call(Method::POST, &format!("/channels/{cid}/messages"), Some(json!({ "content": text }))).await;
        assert_eq!(s, 201, "{v}");
        serde_json::from_value(v["id"].clone()).unwrap()
    }

    async fn pin(&self, cid: Id, mid: Id) -> (StatusCode, Value) {
        self.call(Method::PUT, &format!("/channels/{cid}/pins/{mid}"), None).await
    }

    async fn unpin(&self, cid: Id, mid: Id) -> (StatusCode, Value) {
        self.call(Method::DELETE, &format!("/channels/{cid}/pins/{mid}"), None).await
    }

    /// Les messages épinglés, par id.
    async fn pins(&self, cid: Id) -> Vec<Id> {
        let (s, v) = self.call(Method::GET, &format!("/channels/{cid}/pins"), None).await;
        assert_eq!(s, 200, "{v}");
        serde_json::from_value::<Vec<Message>>(v).unwrap().iter().map(|m| m.id).collect()
    }

    async fn role(&self, gid: Id, give: &Who, permissions: u64) {
        let (s, v) = self
            .call(
                Method::POST,
                &format!("/guilds/{gid}/roles"),
                Some(json!({ "name": "Gérants", "permissions": permissions })),
            )
            .await;
        assert_eq!(s, 201, "{v}");
        let rid = v["id"].as_str().unwrap();
        let path = format!("/guilds/{gid}/members/{}/roles/{rid}", give.user.id);
        assert_eq!(self.call(Method::PUT, &path, None).await.0, 200);
    }

    async fn audit(&self, gid: Id) -> Vec<Value> {
        let (s, v) = self.call(Method::GET, &format!("/guilds/{gid}/audit-log?category=serveur"), None).await;
        assert_eq!(s, 200, "{v}");
        v["entries"].as_array().unwrap().clone()
    }

    async fn gateway(&self) -> Gateway {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/gateway", self.base)).await.unwrap();
        Gateway::read(&mut ws).await;
        let op = ClientOp::Identify { token: Some(self.token.clone()) };
        ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
        let ServerOp::Ready(_) = Gateway::read(&mut ws).await else { panic!("READY attendu") };
        Gateway { ws }
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct Gateway {
    ws: Socket,
}

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

    /// Le prochain `MESSAGE_UPDATE`, en laissant passer les autres événements.
    async fn next_update(&mut self) -> Message {
        loop {
            if let ServerOp::Dispatch { event: Event::MessageUpdate(m), .. } = Self::read(&mut self.ws).await {
                return *m;
            }
        }
    }

    /// Aucun `MESSAGE_UPDATE` ne doit venir dans les instants qui suivent.
    async fn no_update(&mut self) {
        let wait = tokio::time::timeout(std::time::Duration::from_millis(400), self.next_update()).await;
        assert!(wait.is_err(), "aucun MESSAGE_UPDATE attendu");
    }
}

fn general(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn pinning_in_a_server_needs_manage_messages_and_listing_needs_only_to_read(pool: PgPool) {
    let base = spawn(pool).await;
    let (lou, sam, noa) = (Who::new(&base, "lou").await, Who::new(&base, "sam").await, Who::new(&base, "noa").await);
    let g = lou.new_guild().await;
    sam.join(&g, &lou).await;
    let (gid, cid) = (g.guild.id, general(&g));
    let mid = sam.say(cid, "à retenir").await;

    // Sans le droit, même pour son propre message : 403, et rien n'est épinglé.
    assert_eq!(sam.pin(cid, mid).await.0, StatusCode::FORBIDDEN);
    assert_eq!(sam.unpin(cid, mid).await.0, StatusCode::FORBIDDEN);
    assert!(sam.pins(cid).await.is_empty(), "la liste se lit sans le droit");
    // Un étranger au serveur ne sait pas que le salon existe ; sans session : 401.
    assert_eq!(noa.pin(cid, mid).await.0, StatusCode::NOT_FOUND);
    assert_eq!(noa.call(Method::GET, &format!("/channels/{cid}/pins"), None).await.0, StatusCode::NOT_FOUND);
    let anon = Client::new().put(format!("http://{base}/api/v1/channels/{cid}/pins/{mid}")).send().await.unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);

    // Le propriétaire épingle : le message revient avec `pinned`, comme dans la liste.
    let (s, v) = lou.pin(cid, mid).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let m: Message = serde_json::from_value(v).unwrap();
    assert!(m.pinned && m.id == mid && m.content == "à retenir");
    assert_eq!(sam.pins(cid).await, [mid]);
    let (_, hist) = sam.call(Method::GET, &format!("/channels/{cid}/messages"), None).await;
    assert_eq!(hist[0]["pinned"], true);
    // Un message qui n'est pas épinglé ne porte pas le champ du tout.
    let plain = sam.say(cid, "banal").await;
    let (_, hist) = sam.call(Method::GET, &format!("/channels/{cid}/messages"), None).await;
    assert!(hist.as_array().unwrap().iter().find(|m| m["id"] == json!(plain)).unwrap().get("pinned").is_none());

    // Un rôle « Gérer les messages » suffit.
    lou.role(gid, &sam, perms::MANAGE_MESSAGES).await;
    assert_eq!(sam.pin(cid, plain).await.0, StatusCode::OK);
    assert_eq!(sam.unpin(cid, mid).await.0, StatusCode::OK);
    assert_eq!(lou.pins(cid).await, [plain]);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_message_of_another_channel_is_not_found(pool: PgPool) {
    let base = spawn(pool).await;
    let (lou, sam) = (Who::new(&base, "lou").await, Who::new(&base, "sam").await);
    let g = lou.new_guild().await;
    sam.join(&g, &lou).await;
    let (s, v) = lou
        .call(
            Method::POST,
            &format!("/guilds/{}/channels", g.guild.id),
            Some(json!({ "name": "autre", "kind": "text" })),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let other: Id = serde_json::from_value(v["id"].clone()).unwrap();
    let cid = general(&g);
    let here = sam.say(cid, "ici").await;
    let there = sam.say(other, "là").await;

    assert_eq!(lou.pin(cid, there).await.0, StatusCode::NOT_FOUND);
    assert_eq!(lou.pin(other, here).await.0, StatusCode::NOT_FOUND);
    assert_eq!(lou.unpin(cid, there).await.0, StatusCode::NOT_FOUND);
    assert_eq!(lou.pin(cid, Id::nil()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(lou.pin(Id::nil(), here).await.0, StatusCode::NOT_FOUND);
    assert!(lou.pins(cid).await.is_empty() && lou.pins(other).await.is_empty());
    // Même un message qu'on peut lire ailleurs n'est pas épinglé en passant par l'autre salon.
    assert_eq!(lou.pin(other, there).await.0, StatusCode::OK);
    assert_eq!(lou.pins(other).await, [there]);
    assert!(lou.pins(cid).await.is_empty());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn both_people_of_a_dm_and_every_member_of_a_group_can_pin_without_audit(pool: PgPool) {
    let base = spawn(pool).await;
    let (lou, sam, noa, kim) = (
        Who::new(&base, "lou").await,
        Who::new(&base, "sam").await,
        Who::new(&base, "noa").await,
        Who::new(&base, "kim").await,
    );
    for other in [&sam, &noa] {
        lou.befriend(other).await;
    }
    let (s, v) = lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id]}))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let dm: Channel = serde_json::from_value(v).unwrap();
    let (a, b) = (lou.say(dm.id, "de lou").await, sam.say(dm.id, "de sam").await);

    // Chacun épingle le message de l'autre comme le sien ; un tiers ne voit pas le MP.
    assert_eq!(sam.pin(dm.id, a).await.0, StatusCode::OK);
    assert_eq!(lou.pin(dm.id, b).await.0, StatusCode::OK);
    assert_eq!(lou.pins(dm.id).await, [b, a]);
    assert_eq!(sam.unpin(dm.id, b).await.0, StatusCode::OK);
    assert_eq!(sam.pins(dm.id).await, [a]);
    assert_eq!(kim.pin(dm.id, a).await.0, StatusCode::NOT_FOUND);
    assert_eq!(kim.call(Method::GET, &format!("/channels/{}/pins", dm.id), None).await.0, StatusCode::NOT_FOUND);

    // Un groupe : tous ses participants.
    let (s, v) =
        lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id, noa.user.id]}))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let group: Channel = serde_json::from_value(v).unwrap();
    let m = noa.say(group.id, "dans le groupe").await;
    assert_eq!(sam.pin(group.id, m).await.0, StatusCode::OK);
    assert_eq!(noa.unpin(group.id, m).await.0, StatusCode::OK);
    assert_eq!(kim.pin(group.id, m).await.0, StatusCode::NOT_FOUND);
    assert!(lou.pins(group.id).await.is_empty());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_blocked_dm_refuses_pinning_like_writing_but_still_lists(pool: PgPool) {
    let base = spawn(pool).await;
    let (lou, sam) = (Who::new(&base, "lou").await, Who::new(&base, "sam").await);
    lou.befriend(&sam).await;
    let (s, v) = lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id]}))).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let dm: Channel = serde_json::from_value(v).unwrap();
    let (a, b) = (lou.say(dm.id, "de lou").await, sam.say(dm.id, "de sam").await);
    assert_eq!(sam.pin(dm.id, a).await.0, StatusCode::OK);
    let mut gw = sam.gateway().await;

    // Lou bloque sam : l'envoi est refusé (403), et épingler ou désépingler de même, pour chacun des deux.
    let path = format!("/users/@me/relationships/{}", sam.user.id);
    assert_eq!(lou.call(Method::PUT, &path, Some(json!({"kind": "blocked"}))).await.0, StatusCode::OK);
    let code = |v: &Value| v["code"].as_str().unwrap_or_default().to_string();
    let send = json!({"content": "hé"});
    let (s, v) = sam.call(Method::POST, &format!("/channels/{}/messages", dm.id), Some(send.clone())).await;
    assert_eq!((s, code(&v).as_str()), (StatusCode::FORBIDDEN, "dm_refused"));
    let (s, v) = sam.pin(dm.id, b).await;
    assert_eq!((s, code(&v).as_str()), (StatusCode::FORBIDDEN, "dm_refused"));
    let (s, v) = sam.unpin(dm.id, a).await;
    assert_eq!((s, code(&v).as_str()), (StatusCode::FORBIDDEN, "dm_refused"));
    let (s, v) = lou.call(Method::POST, &format!("/channels/{}/messages", dm.id), Some(send)).await;
    assert_eq!((s, code(&v).as_str()), (StatusCode::FORBIDDEN, "dm_blocked"));
    let (s, v) = lou.pin(dm.id, b).await;
    assert_eq!((s, code(&v).as_str()), (StatusCode::FORBIDDEN, "dm_blocked"));
    let (s, v) = lou.unpin(dm.id, a).await;
    assert_eq!((s, code(&v).as_str()), (StatusCode::FORBIDDEN, "dm_blocked"));
    // Rien n'a changé, et personne n'en est averti.
    gw.no_update().await;
    // La lecture reste permise des deux côtés.
    assert_eq!(sam.pins(dm.id).await, [a]);
    assert_eq!(lou.pins(dm.id).await, [a]);

    // Débloqué, le MP se rouvre : on épingle de nouveau.
    assert!(lou.call(Method::DELETE, &path, None).await.0.is_success());
    assert_eq!(sam.pin(dm.id, b).await.0, StatusCode::OK);
    assert_eq!(lou.unpin(dm.id, a).await.0, StatusCode::OK);
    assert_eq!(lou.pins(dm.id).await, [b]);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn the_fifty_first_pin_is_a_french_400_and_unpinning_makes_room(pool: PgPool) {
    let base = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.new_guild().await;
    let cid = general(&g);
    let mut ids = Vec::new();
    for i in 0..=MAX_PINS {
        ids.push(lou.say(cid, &format!("message {i}")).await);
    }
    for id in &ids[..MAX_PINS] {
        assert_eq!(lou.pin(cid, *id).await.0, StatusCode::OK);
    }
    let (s, v) = lou.pin(cid, ids[MAX_PINS]).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["code"], "pin_limit");
    assert_eq!(v["message"], "Ce salon a déjà 50 messages épinglés : désépingles-en un d'abord.");
    assert_eq!(lou.pins(cid).await.len(), MAX_PINS);
    // Un message déjà épinglé reste un succès, même au plafond.
    assert_eq!(lou.pin(cid, ids[0]).await.0, StatusCode::OK);
    // Désépingler en libère une.
    assert_eq!(lou.unpin(cid, ids[7]).await.0, StatusCode::OK);
    assert_eq!(lou.pin(cid, ids[MAX_PINS]).await.0, StatusCode::OK);
    assert_eq!(lou.pins(cid).await[0], ids[MAX_PINS], "le dernier épinglé vient en premier");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn every_change_reaches_the_channel_and_a_repeat_says_nothing(pool: PgPool) {
    let base = spawn(pool).await;
    let (lou, sam, noa) = (Who::new(&base, "lou").await, Who::new(&base, "sam").await, Who::new(&base, "noa").await);
    let g = lou.new_guild().await;
    sam.join(&g, &lou).await;
    noa.join(&g, &lou).await;
    let (gid, cid) = (g.guild.id, general(&g));
    let mid = sam.say(cid, "un secret très secret").await;
    let (mut sam_gw, mut noa_gw) = (sam.gateway().await, noa.gateway().await);

    // Tout le salon l'apprend, auteur compris.
    assert_eq!(lou.pin(cid, mid).await.0, StatusCode::OK);
    for gw in [&mut sam_gw, &mut noa_gw] {
        let m = gw.next_update().await;
        assert!(m.pinned && m.id == mid && m.channel_id == cid);
    }
    // Épingler de nouveau : succès, ni événement ni ligne de journal.
    assert_eq!(lou.pin(cid, mid).await.0, StatusCode::OK);
    sam_gw.no_update().await;
    let pins = |entries: &[Value], action: &str| entries.iter().filter(|e| e["action"] == action).count();
    let entries = lou.audit(gid).await;
    assert_eq!((pins(&entries, "message_pin"), pins(&entries, "message_unpin")), (1, 0));

    // Modifier le message le garde épinglé, et l'événement le dit.
    let (s, _) = sam
        .call(Method::PATCH, &format!("/channels/{cid}/messages/{mid}"), Some(json!({ "content": "corrigé" })))
        .await;
    assert_eq!(s, StatusCode::OK);
    let m = noa_gw.next_update().await;
    assert!(m.pinned && m.content == "corrigé");
    assert_eq!(noa.pins(cid).await, [mid]);

    // Désépingler : événement avec `pinned` retiré ; recommencer ne dit rien.
    assert_eq!(lou.unpin(cid, mid).await.0, StatusCode::OK);
    let m = noa_gw.next_update().await;
    assert!(!m.pinned && m.id == mid);
    assert_eq!(lou.unpin(cid, mid).await.0, StatusCode::OK);
    noa_gw.no_update().await;
    let (s, v) = lou.pin(cid, mid).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // Le journal : qui, quel salon ; pas le texte, ni le message lui-même.
    let entries = lou.audit(gid).await;
    assert_eq!((pins(&entries, "message_pin"), pins(&entries, "message_unpin")), (2, 1));
    let e = entries.iter().find(|e| e["action"] == "message_unpin").unwrap();
    assert_eq!((e["actor"]["name"].as_str(), e["target"]["name"].as_str()), (Some("lou"), Some("sam")));
    assert_eq!(e["details"]["channel"], "général");
    assert!(!serde_json::to_string(&entries).unwrap().contains("secret"));

    // Supprimer un message épinglé le retire de la liste, et chacun le sait.
    assert_eq!(
        lou.call(Method::DELETE, &format!("/channels/{cid}/messages/{mid}"), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert!(noa.pins(cid).await.is_empty());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn pinning_is_rate_limited_separately_from_reads(pool: PgPool) {
    let base = spawn_limited(pool, 1).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.new_guild().await;
    let cid = general(&g);
    let mid = lou.say(cid, "encore").await;

    // 60 écritures par minute et par client : la suite est refusée.
    let mut statuses = Vec::new();
    for _ in 0..70 {
        statuses.push(lou.pin(cid, mid).await.0);
    }
    assert!(statuses[..55].iter().all(|s| *s == StatusCode::OK), "{statuses:?}");
    assert!(statuses.contains(&StatusCode::TOO_MANY_REQUESTS), "{statuses:?}");
    assert_eq!(lou.unpin(cid, mid).await.0, StatusCode::TOO_MANY_REQUESTS);
    // La lecture n'est pas comptée.
    assert_eq!(lou.call(Method::GET, &format!("/channels/{cid}/pins"), None).await.0, StatusCode::OK);
}
