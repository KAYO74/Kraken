//! Modération de bout en bout sur un vrai port et une vraie base : rôles (création, couleur,
//! ordre, droits, attribution), exclusion, bannissement et débannissement, départ, transfert de
//! propriété, hiérarchie, et refus pour qui n'a pas le droit. Les clients les reçoivent en direct.

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
}

fn code(v: &Value) -> &str {
    v["code"].as_str().unwrap_or_default()
}

const MODS: u64 = perms::MANAGE_ROLES | perms::KICK | perms::BAN;

/// Un serveur à `own`, avec deux autres comptes dedans.
async fn trio(base: &str) -> (Who, Who, Who, GuildState) {
    let own = Who::new(base, "own").await;
    let mid = Who::new(base, "mid").await;
    let low = Who::new(base, "low").await;
    let g = own.guild().await;
    assert_eq!(mid.join(&g, &own).await.0, 200);
    assert_eq!(low.join(&g, &own).await.0, 200);
    (own, mid, low, g)
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn roles_are_created_assigned_and_seen_live(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g) = trio(&base).await;
    let gid = g.guild.id;
    let mut mid_gw = mid.gateway().await;
    let mut own_gw = own.gateway().await;

    let r = own.role(gid, "Modos", perms::KICK).await;
    assert_eq!((r.color, r.hoist, r.position), (Some(2), true, 1));
    for gw in [&mut mid_gw, &mut own_gw] {
        assert_eq!(gw.next().await, Event::GuildRoleCreate { guild_id: gid, role: r.clone() });
    }

    // Attribuer : le membre à jour arrive en direct, et l'état du serveur le dit.
    assert_eq!(own.give(gid, &low, &r).await, 200);
    let Event::GuildMemberUpdate { member, .. } = mid_gw.next().await else { panic!("GUILD_MEMBER_UPDATE attendu") };
    assert_eq!((member.user.id, member.roles.clone()), (low.user.id, vec![r.id]));
    let (_, v) = low.state(gid).await;
    let st: GuildState = serde_json::from_value(v).unwrap();
    assert_eq!(st.members.iter().find(|m| m.user.id == low.user.id).unwrap().roles, [r.id]);
    assert!(st.roles.iter().any(|x| x.id == r.id && x.hoist && x.color == Some(2)));

    // Renommer, recolorer, droits : événement et réponse concordent.
    let (s, v) = own
        .call(
            Method::PATCH,
            &format!("/guilds/{gid}/roles/{}", r.id),
            Some(json!({ "name": "Modération", "color": 7, "permissions": perms::KICK | perms::BAN })),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let Event::GuildRoleUpdate { role, .. } = mid_gw.next().await else { panic!("GUILD_ROLE_UPDATE attendu") };
    assert_eq!((role.name.as_str(), role.color, role.permissions), ("Modération", Some(7), perms::KICK | perms::BAN));
    assert_eq!(
        own.call(Method::PATCH, &format!("/guilds/{gid}/roles/{}", r.id), Some(json!({ "color": 99 }))).await.0,
        400
    );

    // Réordonner (deux rôles) : un événement par rôle déplacé.
    let r2 = own.role(gid, "Anciens", 0).await;
    mid_gw.next().await;
    own_gw.next().await;
    own_gw.next().await;
    own_gw.next().await;
    let (s, v) = own.call(Method::PUT, &format!("/guilds/{gid}/roles"), Some(json!({ "ids": [r2.id, r.id] }))).await;
    assert_eq!(s, 200, "{v}");
    let moved: Vec<Role> = serde_json::from_value(v).unwrap();
    assert_eq!(moved.len(), 2);
    assert!(matches!(mid_gw.next().await, Event::GuildRoleUpdate { .. }));
    assert!(matches!(mid_gw.next().await, Event::GuildRoleUpdate { .. }));
    assert_eq!(own.call(Method::PUT, &format!("/guilds/{gid}/roles"), Some(json!({ "ids": [r.id] }))).await.0, 400);

    // Retirer le rôle, puis le supprimer.
    let (s, _) = own.call(Method::DELETE, &format!("/guilds/{gid}/members/{}/roles/{}", low.user.id, r.id), None).await;
    assert_eq!(s, 200);
    assert!(matches!(mid_gw.next().await, Event::GuildMemberUpdate { member, .. } if member.roles.is_empty()));
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/roles/{}", r.id), None).await.0, 204);
    assert_eq!(mid_gw.next().await, Event::GuildRoleDelete { guild_id: gid, role_id: r.id });
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/roles/{}", r.id), None).await.0, 404);
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/roles/{gid}"), None).await.0, 400, "@everyone");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn without_the_permission_every_route_refuses(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g) = trio(&base).await;
    let gid = g.guild.id;
    let stranger = Who::new(&base, "out").await;
    let r = own.role(gid, "R", 0).await;
    let uid = own.user.id;
    let (m, l) = (mid.user.id, low.user.id);

    // Un membre sans droits : 403 partout ; un étranger : 404 (le serveur lui est invisible).
    let routes: Vec<(Method, String, Option<Value>)> = vec![
        (Method::POST, format!("/guilds/{gid}/roles"), Some(json!({ "name": "X" }))),
        (Method::PATCH, format!("/guilds/{gid}/roles/{}", r.id), Some(json!({ "name": "X" }))),
        (Method::DELETE, format!("/guilds/{gid}/roles/{}", r.id), None),
        (Method::PUT, format!("/guilds/{gid}/roles"), Some(json!({ "ids": [r.id] }))),
        (Method::PUT, format!("/guilds/{gid}/members/{l}/roles/{}", r.id), None),
        (Method::DELETE, format!("/guilds/{gid}/members/{l}/roles/{}", r.id), None),
        (Method::DELETE, format!("/guilds/{gid}/members/{uid}"), None),
        (Method::GET, format!("/guilds/{gid}/bans"), None),
        (Method::PUT, format!("/guilds/{gid}/bans/{uid}"), Some(json!({}))),
        (Method::DELETE, format!("/guilds/{gid}/bans/{uid}"), None),
        (Method::POST, format!("/guilds/{gid}/owner"), Some(json!({ "user_id": m }))),
    ];
    for (method, path, body) in routes {
        let (s, v) = mid.call(method.clone(), &path, body.clone()).await;
        assert_eq!((s.as_u16(), code(&v)), (403, "forbidden"), "{method} {path}");
        let (s, _) = stranger.call(method.clone(), &path, body).await;
        assert_eq!(s.as_u16(), 404, "{method} {path}");
    }
    // Rien n'a bougé.
    assert_eq!(own.state(gid).await.1["members"].as_array().unwrap().len(), 3);
    // Quitter un serveur dont on n'est pas membre : 404.
    assert_eq!(stranger.call(Method::DELETE, &format!("/guilds/{gid}/members/@me"), None).await.0, 404);
    // Sans session : 401.
    let anon =
        reqwest::Client::new().delete(format!("http://{base}/api/v1/guilds/{gid}/members/{l}")).send().await.unwrap();
    assert_eq!(anon.status(), 401);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn hierarchy_over_http(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g) = trio(&base).await;
    let gid = g.guild.id;
    let high = own.role(gid, "Haut", MODS).await;
    let middle = own.role(gid, "Moyen", MODS).await;
    own.call(Method::PUT, &format!("/guilds/{gid}/roles"), Some(json!({ "ids": [high.id, middle.id] }))).await;
    assert_eq!(own.give(gid, &mid, &middle).await, 200);
    assert_eq!(own.give(gid, &low, &high).await, 200);

    // `mid` ne peut agir ni sur `low` (rôle plus haut) ni sur le propriétaire, ni se donner un rôle égal.
    for (method, path, body) in [
        (Method::DELETE, format!("/guilds/{gid}/members/{}", low.user.id), None),
        (Method::PUT, format!("/guilds/{gid}/bans/{}", low.user.id), Some(json!({}))),
        (Method::PUT, format!("/guilds/{gid}/bans/{}", own.user.id), Some(json!({}))),
        (Method::DELETE, format!("/guilds/{gid}/members/{}", own.user.id), None),
        (Method::PUT, format!("/guilds/{gid}/members/{}/roles/{}", mid.user.id, high.id), None),
        (Method::PATCH, format!("/guilds/{gid}/roles/{}", high.id), Some(json!({ "name": "Pirate" }))),
        (Method::DELETE, format!("/guilds/{gid}/roles/{}", middle.id), None),
    ] {
        let (s, v) = mid.call(method.clone(), &path, body).await;
        assert_eq!((s.as_u16(), code(&v)), (403, "hierarchy"), "{method} {path}");
    }
    // Le propriétaire ne peut pas non plus être « exclu » par son propre compte : il se transfère ou supprime.
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/members/{}", own.user.id), None).await.0, 400);
    assert_eq!(code(&own.call(Method::DELETE, &format!("/guilds/{gid}/members/@me"), None).await.1), "owner_leave");
    // `low` (rôle Haut) peut exclure `mid` (Moyen).
    assert_eq!(low.call(Method::DELETE, &format!("/guilds/{gid}/members/{}", mid.user.id), None).await.0, 204);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn kick_and_leave_are_live_and_rejoinable(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g) = trio(&base).await;
    let gid = g.guild.id;
    let mut own_gw = own.gateway().await;
    let mut mid_gw = mid.gateway().await;
    let mut low_gw = low.gateway().await;

    assert_eq!(
        own.call(Method::DELETE, &format!("/guilds/{gid}/members/{}?reason=bruit", mid.user.id), None).await.0,
        204
    );
    // Les autres, et l'exclu lui-même, l'apprennent ; il ne voit plus le serveur.
    let gone = Event::GuildMemberRemove { guild_id: gid, user_id: mid.user.id };
    for gw in [&mut own_gw, &mut mid_gw, &mut low_gw] {
        assert_eq!(gw.next().await, gone);
    }
    assert_eq!(mid.state(gid).await.0, 404);
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/members/{}", mid.user.id), None).await.0, 404);

    // Il revient par invitation, puis part de lui-même.
    assert_eq!(mid.join(&g, &own).await.0, 200);
    assert!(matches!(own_gw.next().await, Event::MemberAdd { .. }));
    assert_eq!(mid.call(Method::DELETE, &format!("/guilds/{gid}/members/@me"), None).await.0, 204);
    assert_eq!(own_gw.next().await, gone);
    assert_eq!(mid.state(gid).await.0, 404);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn ban_refuses_rejoin_until_unbanned(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g) = trio(&base).await;
    let gid = g.guild.id;
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    assert_eq!(low.say(general, "pub").await, 201);
    let mut own_gw = own.gateway().await;
    let mut mid_gw = mid.gateway().await;
    let mut low_gw = low.gateway().await;

    // Bannir `low` en effaçant ses dernières heures de messages.
    let (s, v) = own
        .call(
            Method::PUT,
            &format!("/guilds/{gid}/bans/{}", low.user.id),
            Some(json!({ "reason": "pub", "delete_hours": 1 })),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["user"]["id"].as_str(), v["reason"].as_str()), (Some(low.user.id.to_string().as_str()), Some("pub")));
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(own_gw.next().await);
    }
    assert!(seen.contains(&Event::GuildMemberRemove { guild_id: gid, user_id: low.user.id }));
    assert!(seen.iter().any(|e| matches!(e, Event::MessageDelete { channel_id, .. } if *channel_id == general)));
    assert!(seen.iter().any(|e| matches!(e, Event::GuildBanAdd { ban, .. } if ban.user.id == low.user.id)));
    assert_eq!(low_gw.next().await, Event::GuildMemberRemove { guild_id: gid, user_id: low.user.id });
    // `mid` (sans droit de bannir) ne reçoit pas l'événement des bannis, seulement le départ.
    assert!(matches!(mid_gw.next().await, Event::MessageDelete { .. } | Event::GuildMemberRemove { .. }));
    let msgs = own.call(Method::GET, &format!("/channels/{general}/messages"), None).await.1;
    assert!(msgs.as_array().unwrap().is_empty());

    // Plus de retour par invitation (403 « banned »), même avec une invitation neuve.
    let (s, v) = low.join(&g, &own).await;
    assert_eq!((s.as_u16(), code(&v)), (403, "banned"));
    let (_, v) = own.call(Method::GET, &format!("/guilds/{gid}/bans"), None).await;
    let list: Vec<Ban> = serde_json::from_value(v).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].user.id, list[0].moderator_id), (low.user.id, Some(own.user.id)));

    // Débannir : prévenu, et il peut revenir.
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/bans/{}", low.user.id), None).await.0, 204);
    loop {
        if own_gw.next().await == (Event::GuildBanRemove { guild_id: gid, user_id: low.user.id }) {
            break;
        }
    }
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/bans/{}", low.user.id), None).await.0, 404);
    assert_eq!(low.join(&g, &own).await.0, 200);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn ownership_transfer_then_the_old_owner_can_leave(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, mid, low, g) = trio(&base).await;
    let gid = g.guild.id;
    let mut low_gw = low.gateway().await;
    let stranger = Who::new(&base, "out").await;

    let to = |w: &Who| Some(json!({ "user_id": w.user.id }));
    let path = format!("/guilds/{gid}/owner");
    assert_eq!(mid.call(Method::POST, &path, to(&mid)).await.0, 403, "pas le propriétaire");
    assert_eq!(own.call(Method::POST, &path, to(&own)).await.0, 400, "soi-même");
    assert_eq!(own.call(Method::POST, &path, to(&stranger)).await.0, 404, "pas un membre");
    assert_eq!(own.call(Method::POST, &path, to(&mid)).await.0, 204);
    assert_eq!(low_gw.next().await, Event::GuildOwnerUpdate { guild_id: gid, owner_id: mid.user.id });
    assert_eq!(own.state(gid).await.1["guild"]["owner_id"], json!(mid.user.id));
    // L'ancien propriétaire n'a plus la main, peut partir ; le nouveau, lui, ne le peut plus.
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/members/{}", low.user.id), None).await.0, 403);
    assert_eq!(code(&mid.call(Method::DELETE, &format!("/guilds/{gid}/members/@me"), None).await.1), "owner_leave");
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/members/@me"), None).await.0, 204);
    assert_eq!(own.state(gid).await.0, 404);
}
