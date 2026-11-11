//! Salons de bout en bout sur un vrai port et une vraie base : création, modification, ordre,
//! suppression, mode lent, salons privés et surcharges de droits ; refus pour qui n'a pas le droit ;
//! et ce que chaque membre reçoit en direct (un salon qui apparaît, change ou disparaît).

use futures_util::{SinkExt, StreamExt};
use kraken_core::perms;
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
        data_dir: std::env::temp_dir().join("kraken-test-data").to_string_lossy().into(),
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

    async fn guild(&self) -> GuildState {
        let (s, v) = self.call(Method::POST, "/guilds", Some(json!({ "name": "Abysse" }))).await;
        assert_eq!(s, 201);
        serde_json::from_value(v).unwrap()
    }

    /// Rejoint le serveur par invitation.
    async fn join(&self, g: &GuildState, inviter: &Who) -> (StatusCode, Value) {
        let cid = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
        let (s, v) = inviter.call(Method::POST, &format!("/channels/{cid}/invites"), Some(json!({}))).await;
        assert_eq!(s, 201);
        self.call(Method::POST, &format!("/invites/{}", v["code"].as_str().unwrap()), None).await
    }

    async fn state(&self, gid: Id) -> (StatusCode, Value) {
        self.call(Method::GET, &format!("/guilds/{gid}"), None).await
    }

    async fn role(&self, gid: Id, name: &str, perms: u64) -> Role {
        let body = json!({ "name": name, "permissions": perms, "color": 2, "hoist": true });
        let (s, v) = self.call(Method::POST, &format!("/guilds/{gid}/roles"), Some(body)).await;
        assert_eq!(s, 201, "{v}");
        serde_json::from_value(v).unwrap()
    }

    async fn say(&self, cid: Id, text: &str) -> u16 {
        self.call(Method::POST, &format!("/channels/{cid}/messages"), Some(json!({ "content": text }))).await.0.as_u16()
    }

    async fn give(&self, gid: Id, to: &Who, r: &Role) -> StatusCode {
        self.call(Method::PUT, &format!("/guilds/{gid}/members/{}/roles/{}", to.user.id, r.id), None).await.0
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

    /// Rien n'arrive (pendant un court instant).
    async fn quiet(&mut self) {
        if let Ok(Some(Ok(Ws::Text(t)))) =
            tokio::time::timeout(std::time::Duration::from_millis(400), self.0.next()).await
        {
            panic!("rien n'était attendu : {t}");
        }
    }
}

fn code(v: &Value) -> &str {
    v["code"].as_str().unwrap_or_default()
}

/// Un serveur à `own`, avec deux autres comptes dedans ; `mid` gère les salons (rôle « Bâtisseurs »).
async fn trio(base: &str) -> (Who, Who, Who, GuildState, Role) {
    let own = Who::new(base, "own").await;
    let mid = Who::new(base, "mid").await;
    let low = Who::new(base, "low").await;
    let g = own.guild().await;
    assert_eq!(mid.join(&g, &own).await.0, 200);
    assert_eq!(low.join(&g, &own).await.0, 200);
    let builders = own.role(g.guild.id, "Bâtisseurs", perms::MANAGE_CHANNELS).await;
    assert_eq!(own.give(g.guild.id, &mid, &builders).await, 200);
    (own, mid, low, g, builders)
}

fn general(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.name.as_deref() == Some("général")).unwrap().id
}

async fn make(who: &Who, gid: Id, name: &str, kind: &str, parent: Option<Id>) -> (StatusCode, Value) {
    who.call(
        Method::POST,
        &format!("/guilds/{gid}/channels"),
        Some(json!({ "name": name, "kind": kind, "parent_id": parent })),
    )
    .await
}

fn chan(v: Value) -> Channel {
    serde_json::from_value(v).unwrap()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn channels_are_created_edited_moved_and_deleted_live(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g, _) = trio(&base).await;
    let gid = g.guild.id;
    let texts = g.channels.iter().find(|c| c.name.as_deref() == Some("Salons textuels")).unwrap().id;
    let mut low_gw = low.gateway().await;
    let mut own_gw = own.gateway().await;

    // Un gestionnaire (pas propriétaire) crée un salon dans une catégorie : tout le monde le voit arriver.
    let (s, v) = make(&mid, gid, "Salon de Test", "text", Some(texts)).await;
    assert_eq!(s, 201, "{v}");
    let c = chan(v);
    assert_eq!(
        (c.name.as_deref(), c.parent_id, c.private, c.slowmode_s),
        (Some("salon-de-test"), Some(texts), false, 0)
    );
    for gw in [&mut low_gw, &mut own_gw] {
        assert_eq!(gw.next().await, Event::ChannelCreate(c.clone()));
    }
    // Les validations parlent français et ne créent rien.
    let (s, v) = make(&mid, gid, "   ", "text", None).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invalid"));
    assert!(v["message"].as_str().unwrap().contains("1 à 100 caractères"));
    assert_eq!(make(&mid, gid, "x", "text", Some(c.id)).await.0, 400, "le parent doit être une catégorie");
    assert_eq!(make(&mid, gid, "x", "dm", None).await.0, 400);
    let (s, cat) = make(&own, gid, "Archives", "category", None).await;
    assert_eq!(s, 201);
    low_gw.next().await;
    own_gw.next().await;

    // Modifier : nom, sujet, mode lent.
    let (s, v) = mid
        .call(
            Method::PATCH,
            &format!("/channels/{}", c.id),
            Some(json!({ "name": "Taverne", "topic": "Ici, on rigole.", "slowmode_s": 10 })),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let edited = chan(v);
    assert_eq!(
        (edited.name.as_deref(), edited.topic.as_deref(), edited.slowmode_s),
        (Some("taverne"), Some("Ici, on rigole."), 10)
    );
    assert_eq!(low_gw.next().await, Event::ChannelUpdate(edited.clone()));
    assert_eq!(mid.call(Method::PATCH, &format!("/channels/{}", c.id), Some(json!({ "slowmode_s": 7 }))).await.0, 400);
    assert_eq!(
        mid.call(Method::PATCH, &format!("/channels/{}", c.id), Some(json!({ "topic": "x".repeat(1025) }))).await.0,
        400
    );
    own_gw.next().await;

    // Ranger : dans une autre catégorie, tout en haut ; les salons déplacés sont annoncés.
    let archives = Id::parse_str(cat["id"].as_str().unwrap()).unwrap();
    let (s, v) = mid
        .call(Method::PUT, &format!("/channels/{}/position", c.id), Some(json!({ "parent_id": archives, "index": 0 })))
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(chan(v.as_array().unwrap()[0].clone()).parent_id, Some(archives));
    assert!(matches!(low_gw.next().await, Event::ChannelUpdate(u) if u.id == c.id && u.parent_id == Some(archives)));
    own_gw.next().await;
    assert_eq!(
        mid.call(Method::PUT, &format!("/channels/{}/position", c.id), Some(json!({ "parent_id": c.id, "index": 0 })))
            .await
            .0,
        400
    );

    // Supprimer : les messages partent avec lui, tout le monde l'apprend.
    assert_eq!(low.say(c.id, "salut").await, 201);
    low_gw.next().await;
    own_gw.next().await;
    assert_eq!(mid.call(Method::DELETE, &format!("/channels/{}", c.id), None).await.0, 204);
    assert_eq!(low_gw.next().await, Event::ChannelDelete { id: c.id });
    assert_eq!(own_gw.next().await, Event::ChannelDelete { id: c.id });
    assert_eq!(low.call(Method::GET, &format!("/channels/{}/messages", c.id), None).await.0, 404);
    assert_eq!(mid.call(Method::DELETE, &format!("/channels/{}", c.id), None).await.0, 404);

    // Le dernier salon d'écriture ne part pas, ni de l'API ni de l'interface.
    let (s, v) = own.call(Method::DELETE, &format!("/channels/{}", general(&g)), None).await;
    assert_eq!((s.as_u16(), code(&v)), (409, "last_text"));
    assert!(v["message"].as_str().unwrap().contains("dernier salon d'écriture"));
    assert_eq!(own.state(gid).await.1["channels"].as_array().unwrap().len(), 5);
    low_gw.quiet().await;
    // Une catégorie supprimée laisse ses salons, sortis de la catégorie : la mise à jour arrive.
    let (s, _) = make(&own, gid, "refuge", "text", Some(archives)).await;
    assert_eq!(s, 201);
    low_gw.next().await;
    assert_eq!(own.call(Method::DELETE, &format!("/channels/{archives}"), None).await.0, 204);
    let mut events = vec![low_gw.next().await, low_gw.next().await];
    events.sort_by_key(|e| matches!(e, Event::ChannelDelete { .. }));
    assert!(matches!(&events[0], Event::ChannelUpdate(c) if c.parent_id.is_none()), "{events:?}");
    assert_eq!(events[1], Event::ChannelDelete { id: archives });
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn without_the_permission_every_channel_route_refuses(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, _mid, low, g, builders) = trio(&base).await;
    let gid = g.guild.id;
    let general = general(&g);
    let stranger = Who::new(&base, "out").await;
    let spare = chan(make(&own, gid, "autre", "text", None).await.1);
    let (r, u) = (builders.id, low.user.id);

    // Un membre sans droit : 403 ; un étranger : 404 (le serveur et ses salons lui sont invisibles).
    let routes: Vec<(Method, String, Option<Value>)> = vec![
        (Method::POST, format!("/guilds/{gid}/channels"), Some(json!({ "name": "x", "kind": "text" }))),
        (Method::PATCH, format!("/channels/{}", spare.id), Some(json!({ "name": "x" }))),
        (Method::PUT, format!("/channels/{}/position", spare.id), Some(json!({ "parent_id": null, "index": 0 }))),
        (Method::GET, format!("/channels/{general}/permissions"), None),
        (
            Method::PUT,
            format!("/channels/{general}/permissions/{r}"),
            Some(json!({ "kind": "role", "allow": 0, "deny": 2 })),
        ),
        (
            Method::PUT,
            format!("/channels/{general}/permissions/{u}"),
            Some(json!({ "kind": "member", "allow": 0, "deny": 2 })),
        ),
        (Method::DELETE, format!("/channels/{general}/permissions/{r}"), None),
        (Method::DELETE, format!("/channels/{}", spare.id), None),
    ];
    for (method, path, body) in routes {
        let (s, v) = low.call(method.clone(), &path, body.clone()).await;
        assert_eq!((s.as_u16(), code(&v)), (403, "forbidden"), "{method} {path}");
        let (s, _) = stranger.call(method.clone(), &path, body).await;
        assert_eq!(s.as_u16(), 404, "{method} {path}");
    }
    // Sans session : 401. Rien n'a bougé.
    let anon = reqwest::Client::new()
        .patch(format!("http://{base}/api/v1/channels/{}", spare.id))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);
    let st = own.state(gid).await.1;
    let spare_now = st["channels"].as_array().unwrap().iter().find(|c| c["id"] == json!(spare.id)).unwrap();
    assert_eq!(spare_now["name"], "autre");
    assert_eq!(st["channels"].as_array().unwrap().len(), 5);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn slow_mode_is_enforced_on_send(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g, _) = trio(&base).await;
    let gid = g.guild.id;
    let general = general(&g);
    let (s, v) = mid.call(Method::PATCH, &format!("/channels/{general}"), Some(json!({ "slowmode_s": 60 }))).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(chan(v).slowmode_s, 60);
    assert_eq!(
        own.state(gid).await.1["channels"].as_array().unwrap().iter().find(|c| c["id"] == json!(general)).unwrap()["slowmode_s"],
        60
    );

    assert_eq!(low.say(general, "un").await, 201);
    let r = low
        .c
        .post(format!("http://{}/api/v1/channels/{general}/messages", low.base))
        .header("authorization", format!("Session {}", low.token))
        .json(&json!({ "content": "deux" }))
        .send()
        .await
        .unwrap();
    let wait = r.headers().get("retry-after").and_then(|h| h.to_str().ok()).and_then(|h| h.parse::<u32>().ok());
    let (s, v) = (r.status(), r.json::<Value>().await.unwrap());
    assert_eq!((s.as_u16(), code(&v)), (429, "slowmode"));
    assert!(v["message"].as_str().unwrap().starts_with("Mode lent"));
    // L'en-tête `Retry-After` donne le délai restant, en secondes entières.
    assert!(wait.is_some_and(|n| (1..=60).contains(&n)), "{wait:?}");
    // Ceux qui gèrent les salons (ou les messages) ne sont jamais retenus ; le propriétaire non plus.
    for who in [&mid, &own] {
        for i in 0..3 {
            assert_eq!(who.say(general, &format!("m{i}")).await, 201);
        }
    }
    let history = low.call(Method::GET, &format!("/channels/{general}/messages"), None).await.1;
    assert_eq!(history.as_array().unwrap().len(), 7, "le message refusé n'est pas écrit");
    // Désactivé, plus de délai.
    assert_eq!(mid.call(Method::PATCH, &format!("/channels/{general}"), Some(json!({ "slowmode_s": 0 }))).await.0, 200);
    assert_eq!(low.say(general, "trois").await, 201);
    assert_eq!(low.say(general, "quatre").await, 201);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn private_channels_appear_and_vanish_live(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g, builders) = trio(&base).await;
    let gid = g.guild.id;
    let secret = chan(make(&own, gid, "coulisses", "text", None).await.1);
    let mut mid_gw = mid.gateway().await;
    let mut low_gw = low.gateway().await;
    let mut own_gw = own.gateway().await;
    assert_eq!(own.say(secret.id, "avant").await, 201);
    for gw in [&mut mid_gw, &mut low_gw, &mut own_gw] {
        gw.next().await;
    }
    assert_eq!(low.call(Method::GET, &format!("/channels/{}/messages", secret.id), None).await.0, 200);

    // Privé : disparaît en direct pour qui ne peut pas le lire, reste (marqué) pour les autres.
    let (s, v) = own.call(Method::PATCH, &format!("/channels/{}", secret.id), Some(json!({ "private": true }))).await;
    assert_eq!(s, 200, "{v}");
    assert!(chan(v).private);
    assert_eq!(mid_gw.next().await, Event::ChannelDelete { id: secret.id });
    assert_eq!(low_gw.next().await, Event::ChannelDelete { id: secret.id });
    assert!(matches!(own_gw.next().await, Event::ChannelUpdate(c) if c.id == secret.id && c.private));
    for who in [&mid, &low] {
        assert_eq!(who.call(Method::GET, &format!("/channels/{}/messages", secret.id), None).await.0, 404);
        assert_eq!(who.say(secret.id, "intrus").await, 404);
        assert_eq!(
            who.call(Method::GET, &format!("/channels/{}/permissions", secret.id), None).await.0,
            404,
            "un salon caché n'existe pas"
        );
    }
    let st = low.state(gid).await.1;
    assert!(st["channels"].as_array().unwrap().iter().all(|c| c["id"] != json!(secret.id)));

    // Rouvrir à un rôle : ceux qui l'ont le voient apparaître (avec son drapeau), les autres non.
    let (s, v) = own
        .call(
            Method::PUT,
            &format!("/channels/{}/permissions/{}", secret.id, builders.id),
            Some(json!({ "kind": "role", "allow": 1, "deny": 0 })),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert!(matches!(mid_gw.next().await, Event::ChannelCreate(c) if c.id == secret.id && c.private));
    low_gw.quiet().await;
    assert_eq!(mid.call(Method::GET, &format!("/channels/{}/messages", secret.id), None).await.0, 200);
    // Il le voit et le gère (MANAGE_CHANNELS) : il lit ses réglages, et peut le rendre à tous.
    let (s, v) = mid.call(Method::GET, &format!("/channels/{}/permissions", secret.id), None).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v.as_array().unwrap().len(), 2);

    // Une personne précise.
    let (s, _) = own
        .call(
            Method::PUT,
            &format!("/channels/{}/permissions/{}", secret.id, low.user.id),
            Some(json!({ "kind": "member", "allow": 1 | 1 << 9, "deny": 0 })),
        )
        .await;
    assert_eq!(s, 200);
    assert!(matches!(low_gw.next().await, Event::ChannelCreate(c) if c.id == secret.id));
    assert_eq!(low.call(Method::GET, &format!("/channels/{}/messages", secret.id), None).await.0, 200);
    let (s, _) = own.call(Method::DELETE, &format!("/channels/{}/permissions/{}", secret.id, low.user.id), None).await;
    assert_eq!(s, 200);
    assert_eq!(low_gw.next().await, Event::ChannelDelete { id: secret.id });

    // Rendre public : tout le monde le revoit.
    let (s, _) = mid.call(Method::PATCH, &format!("/channels/{}", secret.id), Some(json!({ "private": false }))).await;
    assert_eq!(s, 200);
    assert!(matches!(low_gw.next().await, Event::ChannelCreate(c) if c.id == secret.id && !c.private));
    // Retirer ce qui n'existe pas : 404 ; des droits qui n'existent pas : 400.
    assert_eq!(
        own.call(Method::DELETE, &format!("/channels/{}/permissions/{}", secret.id, low.user.id), None).await.0,
        404
    );
    let (s, v) = own
        .call(
            Method::PUT,
            &format!("/channels/{}/permissions/{}", secret.id, builders.id),
            Some(json!({ "kind": "role", "allow": 1 << 30, "deny": 0 })),
        )
        .await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invalid"));
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn overwrites_respect_the_hierarchy_and_never_lock_you_out(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g, builders) = trio(&base).await;
    let gid = g.guild.id;
    let general = general(&g);
    let chiefs = own.role(gid, "Chefs", perms::MANAGE_CHANNELS).await;
    // « Chefs » a été créé après « Bâtisseurs » : plus bas. On le remonte au-dessus pour le test.
    let (s, _) =
        own.call(Method::PUT, &format!("/guilds/{gid}/roles"), Some(json!({ "ids": [chiefs.id, builders.id] }))).await;
    assert_eq!(s, 200);

    // Un rôle au-dessus du sien : refusé (hiérarchie) ; le propriétaire du serveur : refusé aussi.
    let (s, v) = mid
        .call(
            Method::PUT,
            &format!("/channels/{general}/permissions/{}", chiefs.id),
            Some(json!({ "kind": "role", "allow": 0, "deny": 2 })),
        )
        .await;
    assert_eq!((s.as_u16(), code(&v)), (403, "hierarchy"));
    let (s, v) = mid
        .call(
            Method::PUT,
            &format!("/channels/{general}/permissions/{}", own.user.id),
            Some(json!({ "kind": "member", "allow": 0, "deny": 2 })),
        )
        .await;
    assert_eq!((s.as_u16(), code(&v)), (403, "hierarchy"));
    // Un droit qu'il n'a pas (gérer les messages) : refusé. Un droit qu'il a : permis.
    let (s, v) = mid
        .call(
            Method::PUT,
            &format!("/channels/{general}/permissions/{gid}"),
            Some(json!({ "kind": "role", "allow": 1 << 8, "deny": 0 })),
        )
        .await;
    assert_eq!((s.as_u16(), code(&v)), (403, "forbidden"), "{v}");
    let (s, _) = mid
        .call(
            Method::PUT,
            &format!("/channels/{general}/permissions/{gid}"),
            Some(json!({ "kind": "role", "allow": 0, "deny": 1 << 6 })),
        )
        .await;
    assert_eq!(s, 200);
    // Se fermer la porte : refusé, et le salon reste ouvert.
    let (s, v) = mid.call(Method::PATCH, &format!("/channels/{general}"), Some(json!({ "private": true }))).await;
    assert_eq!((s.as_u16(), code(&v)), (403, "lockout"));
    assert_eq!(mid.call(Method::GET, &format!("/channels/{general}/messages"), None).await.0, 200);
    // Le salon est toujours visible de tous : rien n'a été rendu privé.
    assert_eq!(low.call(Method::GET, &format!("/channels/{general}/messages"), None).await.0, 200);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_private_category_going_away_or_a_channel_leaving_it_opens_nothing(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g, _) = trio(&base).await;
    let gid = g.guild.id;
    let cat = chan(make(&own, gid, "Staff", "category", None).await.1);
    let (kept, moved) = (
        chan(make(&own, gid, "salle", "text", Some(cat.id)).await.1),
        chan(make(&own, gid, "bureau", "text", Some(cat.id)).await.1),
    );
    let (s, v) = own.call(Method::PATCH, &format!("/channels/{}", cat.id), Some(json!({ "private": true }))).await;
    assert_eq!(s, 200, "{v}");
    for c in [&cat, &kept, &moved] {
        assert_eq!(low.call(Method::GET, &format!("/channels/{}/messages", c.id), None).await.0, 404);
    }
    let mut low_gw = low.gateway().await;
    let mut mid_gw = mid.gateway().await;
    let mut own_gw = own.gateway().await;

    // Un salon sort de la catégorie privée : il reste fermé, personne d'autre n'en entend parler.
    let (s, v) = own
        .call(Method::PUT, &format!("/channels/{}/position", moved.id), Some(json!({ "parent_id": null, "index": 0 })))
        .await;
    assert_eq!(s, 200, "{v}");
    assert!(
        matches!(own_gw.next().await, Event::ChannelUpdate(c) if c.id == moved.id && c.private && c.parent_id.is_none())
    );
    // Les places de la catégorie se resserrent : « salle » passe au rang 0 (déjà vu par le propriétaire).
    let st = own.state(gid).await.1;
    let find = |name: &str| st["channels"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap().clone();
    assert_eq!(find("salle")["position"], 0);
    while let Ok(ev) = tokio::time::timeout(std::time::Duration::from_millis(300), own_gw.next()).await {
        assert!(matches!(ev, Event::ChannelUpdate(c) if c.id == kept.id));
    }
    low_gw.quiet().await;
    mid_gw.quiet().await;
    assert_eq!(low.call(Method::GET, &format!("/channels/{}/messages", moved.id), None).await.0, 404);
    assert_eq!(low.say(moved.id, "intrus").await, 404);

    // La catégorie disparaît : son dernier salon reste fermé, et seuls ceux qui le voient reçoivent la mise à jour.
    assert_eq!(own.call(Method::DELETE, &format!("/channels/{}", cat.id), None).await.0, 204);
    let mut seen = [own_gw.next().await, own_gw.next().await];
    seen.sort_by_key(|e| matches!(e, Event::ChannelUpdate(_)));
    assert_eq!(seen[0], Event::ChannelDelete { id: cat.id });
    assert!(matches!(&seen[1], Event::ChannelUpdate(c) if c.id == kept.id && c.private && c.parent_id.is_none()));
    low_gw.quiet().await;
    mid_gw.quiet().await;
    for c in [&kept, &moved] {
        assert_eq!(low.call(Method::GET, &format!("/channels/{}/messages", c.id), None).await.0, 404);
        assert_eq!(mid.call(Method::GET, &format!("/channels/{}/messages", c.id), None).await.0, 404);
    }
    let st = low.state(gid).await.1;
    let names: Vec<&str> = st["channels"].as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(!names.contains(&"salle") && !names.contains(&"bureau"), "{names:?}");
}

/// Combien de surcharges ce salon a, vu par `who`.
async fn overwrite_count(who: &Who, cid: Id) -> usize {
    who.call(Method::GET, &format!("/channels/{cid}/permissions"), None).await.1.as_array().unwrap().len()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_wrong_kind_in_the_body_cannot_dodge_the_hierarchy_when_removing(pool: PgPool) {
    let base = spawn(pool.clone(), 1000).await;
    let (own, mid, boss) = (Who::new(&base, "own").await, Who::new(&base, "mid").await, Who::new(&base, "boss").await);
    let g = own.guild().await;
    let (gid, general) = (g.guild.id, general(&g));
    assert_eq!(mid.join(&g, &own).await.0, 200);
    assert_eq!(boss.join(&g, &own).await.0, 200);
    // « Haut » est créé avant « Bâtisseurs » : il est au-dessus.
    let top = own.role(gid, "Haut", perms::MANAGE_CHANNELS | perms::VIEW_CHANNEL).await;
    let builders = own.role(gid, "Bâtisseurs", perms::MANAGE_CHANNELS).await;
    let small = own.role(gid, "Petit", 0).await;
    assert_eq!(own.give(gid, &boss, &top).await, 200);
    assert_eq!(own.give(gid, &mid, &builders).await, 200);
    // Posé par le propriétaire : le rôle plus haut, son membre, et `@everyone` avec un droit que « Bâtisseurs » n'a pas.
    for (target, kind, allow, deny) in [
        (top.id, "role", 0, perms::SEND_MESSAGES),
        (boss.user.id, "member", 0, perms::EMBED_LINKS),
        (gid, "role", perms::MANAGE_MESSAGES, 0),
        (small.id, "role", 0, perms::EMBED_LINKS),
    ] {
        let (s, v) = own
            .call(
                Method::PUT,
                &format!("/channels/{general}/permissions/{target}"),
                Some(json!({ "kind": kind, "allow": allow, "deny": deny })),
            )
            .await;
        assert_eq!(s, 200, "{v}");
    }
    assert_eq!(overwrite_count(&own, general).await, 4);

    // Vider la surcharge avec le genre inverse : 403 comme avec le bon, et la ligne reste.
    for (target, kind) in [(top.id, "member"), (boss.user.id, "role"), (gid, "member"), (top.id, "role"), (gid, "role")]
    {
        let (s, v) = mid
            .call(
                Method::PUT,
                &format!("/channels/{general}/permissions/{target}"),
                Some(json!({ "kind": kind, "allow": 0, "deny": 0 })),
            )
            .await;
        assert_eq!(s, 403, "{target} {kind} : {v}");
        assert_eq!(overwrite_count(&own, general).await, 4, "la ligne reste : {target} {kind}");
    }
    for target in [top.id, boss.user.id, gid] {
        let (s, _) = mid.call(Method::DELETE, &format!("/channels/{general}/permissions/{target}"), None).await;
        assert_eq!(s, 403);
    }
    // Changer une surcharge en nommant un autre genre : 400, rien ne bouge.
    let (s, v) = mid
        .call(
            Method::PUT,
            &format!("/channels/{general}/permissions/{}", small.id),
            Some(json!({ "kind": "member", "allow": 0, "deny": perms::ADD_REACTIONS })),
        )
        .await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invalid"), "{v}");
    assert_eq!(overwrite_count(&own, general).await, 4);

    // Ce qu'il a le droit de toucher part, et une vraie orpheline aussi.
    assert_eq!(mid.call(Method::DELETE, &format!("/channels/{general}/permissions/{}", small.id), None).await.0, 200);
    let ghost = Id::from_u128(0xdead01);
    sqlx::query(
        "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny) values ($1, $2, 'role', $3, 0)",
    )
    .bind(general)
    .bind(ghost)
    .bind(perms::MANAGE_MESSAGES as i64)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(overwrite_count(&own, general).await, 4);
    let (s, v) = mid
        .call(
            Method::PUT,
            &format!("/channels/{general}/permissions/{ghost}"),
            Some(json!({ "kind": "member", "allow": 0, "deny": 0 })),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(overwrite_count(&own, general).await, 3);
}
