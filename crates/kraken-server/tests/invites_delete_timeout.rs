//! Invitations (liste, révocation, codes de refus), suppression d'un serveur et exclusion
//! temporaire, de bout en bout sur un vrai port et une vraie base : les refus pour qui n'a pas le
//! droit, ce que chaque membre reçoit en direct, et ce qui disparaît avec le serveur.

use std::sync::Arc;
use std::time::Instant;

use futures_util::{SinkExt, StreamExt};
use kraken_core::perms;
use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use str0m::media::{Direction, MediaKind};
use str0m::{Candidate, Rtc};
use tokio_tungstenite::tungstenite::Message as Ws;

fn config() -> Config {
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
        auth_per_min: 1000,
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
    spawn_with_state(pool).await.0
}

async fn spawn_with_state(pool: PgPool) -> (String, AppState) {
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), config()).await.unwrap();
    let app = kraken_server::app(state.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("{addr}"), state)
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

    /// Une invitation, avec ces réglages.
    async fn invite(&self, cid: Id, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, &format!("/channels/{cid}/invites"), Some(body)).await
    }

    async fn use_code(&self, code: &str) -> (StatusCode, Value) {
        self.call(Method::POST, &format!("/invites/{code}"), None).await
    }

    /// Rejoint le serveur par une invitation toute neuve de `inviter`.
    async fn join(&self, g: &GuildState, inviter: &Who) {
        let (s, v) = inviter.invite(general(g), json!({})).await;
        assert_eq!(s, 201);
        assert_eq!(self.use_code(v["code"].as_str().unwrap()).await.0, 200);
    }

    async fn say(&self, cid: Id, text: &str) -> (StatusCode, Value) {
        self.call(Method::POST, &format!("/channels/{cid}/messages"), Some(json!({ "content": text }))).await
    }

    async fn role(&self, gid: Id, name: &str, perms: u64) -> Role {
        let body = json!({ "name": name, "permissions": perms, "color": 2, "hoist": true });
        let (s, v) = self.call(Method::POST, &format!("/guilds/{gid}/roles"), Some(body)).await;
        assert_eq!(s, 201, "{v}");
        serde_json::from_value(v).unwrap()
    }

    async fn give(&self, gid: Id, to: &Who, r: &Role) {
        let path = format!("/guilds/{gid}/members/{}/roles/{}", to.user.id, r.id);
        assert_eq!(self.call(Method::PUT, &path, None).await.0, 200);
    }

    async fn timeout(&self, gid: Id, to: &Who, seconds: u32) -> (StatusCode, Value) {
        let path = format!("/guilds/{gid}/members/{}/timeout", to.user.id);
        self.call(Method::PUT, &path, Some(json!({ "seconds": seconds, "reason": "calme" }))).await
    }

    async fn join_voice(&self, cid: Id) -> (StatusCode, Value) {
        self.call(Method::POST, &format!("/channels/{cid}/voice"), Some(serde_json::to_value(offer()).unwrap())).await
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

    /// Le premier événement qui satisfait `pick` (les autres sont ignorés).
    async fn until<T>(&mut self, pick: impl Fn(&Event) -> Option<T>) -> T {
        loop {
            if let ServerOp::Dispatch { event, .. } = Self::read(&mut self.0).await
                && let Some(t) = pick(&event)
            {
                return t;
            }
        }
    }
}

fn code(v: &Value) -> &str {
    v["code"].as_str().unwrap_or_default()
}

fn general(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.name.as_deref() == Some("général")).unwrap().id
}

fn voice_room(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.kind == ChannelKind::Voice).unwrap().id
}

fn rows(v: &Value) -> Vec<GuildInvite> {
    serde_json::from_value(v.clone()).unwrap()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn invites_are_validated_listed_revoked_and_say_why_they_are_dead(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let own = Who::new(&base, "own").await;
    let mid = Who::new(&base, "mid").await;
    let low = Who::new(&base, "low").await;
    let eve = Who::new(&base, "eve").await;
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    mid.join(&g, &own).await;
    low.join(&g, &own).await;

    // La durée : 7 jours au plus ; jamais (rien) est permis ; le nombre d'usages est borné.
    let (s, v) = own.invite(cid, json!({ "max_age": MAX_INVITE_AGE_S + 1 })).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invalid"));
    assert!(v["message"].as_str().unwrap().contains("7 jours"));
    assert_eq!(own.invite(cid, json!({ "max_uses": MAX_INVITE_USES + 1 })).await.0, 400);
    let (s, week) = own.invite(cid, json!({ "max_age": MAX_INVITE_AGE_S, "max_uses": 0 })).await;
    assert_eq!(s, 201, "{week}");
    assert!(week["expires_at"].as_i64().is_some() && week["max_uses"] == 0);
    let (s, forever) = own.invite(cid, json!({ "max_age": null, "max_uses": 5 })).await;
    assert_eq!(s, 201);
    assert!(forever["expires_at"].is_null() && forever["max_uses"] == 5);

    // Le bouton « Inviter » du rail reprend l'invitation sans fin déjà faite (200, même code).
    let (s, first) = own.invite(cid, json!({ "reuse": true })).await;
    assert!(s == StatusCode::OK || s == StatusCode::CREATED, "{first}");
    let (s, again) = own.invite(cid, json!({ "reuse": true })).await;
    assert_eq!(s, 200, "{again}");
    assert_eq!(first["code"], again["code"]);
    let (s, plain) = own.invite(cid, json!({})).await;
    assert_eq!(s, 201);
    assert_ne!(plain["code"], first["code"]);

    // Chacun crée la sienne ; le gérant voit tout, le simple membre seulement les siennes.
    let (_, mine) = mid.invite(cid, json!({ "max_uses": 10 })).await;
    let (s, all) = own.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await;
    assert_eq!(s, 200, "{all}");
    let all = rows(&all);
    assert!(all.len() >= 4 && all.iter().any(|i| i.code == code(&mine)));
    assert!(all.iter().all(|i| i.state == InviteState::Active));
    let (s, own_list) = mid.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await;
    assert_eq!(s, 200);
    let own_list = rows(&own_list);
    assert!(own_list.iter().all(|i| i.inviter.as_ref().map(|u| u.id) == Some(mid.user.id)));
    assert!(own_list.iter().any(|i| i.code == code(&mine) && i.max_uses == 10 && i.uses == 0));
    // Un étranger au serveur n'en sait rien ; sans « Créer une invitation », 403.
    assert_eq!(eve.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await.0, 404);
    let everyone = perms::DEFAULT_EVERYONE & !perms::CREATE_INVITE;
    let (s, _) =
        own.call(Method::PATCH, &format!("/guilds/{gid}/roles/{gid}"), Some(json!({ "permissions": everyone }))).await;
    assert_eq!(s, 200);
    assert_eq!(low.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await.0, 403);
    assert_eq!(low.invite(cid, json!({})).await.0, 403);
    own.call(
        Method::PATCH,
        &format!("/guilds/{gid}/roles/{gid}"),
        Some(json!({ "permissions": perms::DEFAULT_EVERYONE })),
    )
    .await;

    // Épuisée : un seul usage, le suivant reçoit un code qui le dit (et l'invitation reste listée).
    let (_, once) = own.invite(cid, json!({ "max_uses": 1 })).await;
    assert_eq!(eve.use_code(code(&once)).await.0, 200);
    let late = Who::new(&base, "late").await;
    let (s, v) = late.use_code(code(&once)).await;
    assert_eq!((s.as_u16(), code(&v)), (404, "invite_exhausted"));
    assert!(v["message"].as_str().unwrap().contains("autant de fois"));
    let (s, v) = late.call(Method::GET, &format!("/invites/{}", code(&once)), None).await;
    assert_eq!((s.as_u16(), code(&v)), (404, "invite_exhausted"));
    let (_, all) = own.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await;
    let spent = rows(&all).into_iter().find(|i| i.code == code(&once)).unwrap();
    assert_eq!((spent.state, spent.uses, spent.max_uses), (InviteState::Exhausted, 1, 1));

    // Expirée : même chose, avec son propre code ; elle ressort après les vivantes.
    let (_, old) = own.invite(cid, json!({ "max_age": 3600 })).await;
    sqlx::query("update invites set expires_at = now() - interval '1 hour' where code = $1")
        .bind(code(&old))
        .execute(&pool)
        .await
        .unwrap();
    let (s, v) = late.use_code(code(&old)).await;
    assert_eq!((s.as_u16(), code(&v)), (404, "invite_expired"));
    let (_, all) = own.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await;
    let all = rows(&all);
    assert_eq!(all.iter().find(|i| i.code == code(&old)).unwrap().state, InviteState::Expired);
    let first_dead = all.iter().position(|i| i.state != InviteState::Active).unwrap();
    assert!(all[first_dead..].iter().all(|i| i.state != InviteState::Active), "les vivantes d'abord");
    // Un code qui n'a jamais existé : ni expirée ni épuisée, simplement inconnue.
    let (s, v) = late.use_code("zzzzzzzz").await;
    assert_eq!((s.as_u16(), code(&v)), (404, "invite_unknown"));

    // Révoquer : le créateur ou un gérant du serveur ; pas un autre simple membre.
    let (_, theirs) = mid.invite(cid, json!({})).await;
    assert_eq!(low.call(Method::DELETE, &format!("/invites/{}", code(&theirs)), None).await.0, 403);
    assert_eq!(eve.call(Method::DELETE, &format!("/invites/{}", code(&theirs)), None).await.0, 403);
    let (s, _) = mid.call(Method::DELETE, &format!("/invites/{}", code(&theirs)), None).await;
    assert_eq!(s, 204);
    let (s, v) = late.use_code(code(&theirs)).await;
    assert_eq!((s.as_u16(), code(&v)), (404, "invite_unknown"), "révoquée = disparue");
    assert_eq!(mid.call(Method::DELETE, &format!("/invites/{}", code(&theirs)), None).await.0, 404);
    let (s, _) = own.call(Method::DELETE, &format!("/invites/{}", code(&mine)), None).await;
    assert_eq!(s, 204, "un gérant révoque celle d'un autre");
    let (_, all) = own.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await;
    assert!(!rows(&all).iter().any(|i| i.code == code(&mine) || i.code == code(&theirs)));
    let trail: i64 = sqlx::query_scalar("select count(*) from audit_log where action = 'invite_revoke'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(trail, 2, "chaque révocation laisse sa trace");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn only_the_owner_deletes_a_server_by_retyping_its_name_and_everything_goes(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let own = Who::new(&base, "own").await;
    let mid = Who::new(&base, "mid").await;
    let low = Who::new(&base, "low").await;
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    mid.join(&g, &own).await;
    low.join(&g, &own).await;
    // Un gestionnaire de serveur n'est pas pour autant propriétaire.
    let boss = own.role(gid, "Direction", perms::ADMINISTRATOR | perms::MANAGE_GUILD).await;
    own.give(gid, &mid, &boss).await;
    assert_eq!(low.say(cid, "bonjour").await.0, 201);
    let (_, code_live) = own.invite(cid, json!({})).await;

    let path = format!("/guilds/{gid}");
    let right = json!({ "name": "Abysse" });
    assert_eq!(mid.call(Method::DELETE, &path, Some(right.clone())).await.0, 403, "même administrateur");
    assert_eq!(low.call(Method::DELETE, &path, Some(right.clone())).await.0, 403);
    // Le nom doit être retapé : mauvais, vide ou absent, rien ne bouge.
    for body in [Some(json!({ "name": "abyss" })), Some(json!({ "name": "" })), None] {
        let (s, v) = own.call(Method::DELETE, &path, body).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "name_mismatch"));
    }
    assert_eq!(own.call(Method::GET, &path, None).await.0, 200);

    let mut low_gw = low.gateway().await;
    let mut own_gw = own.gateway().await;
    // Les espaces autour du nom retapé ne comptent pas.
    let (s, _) = own.call(Method::DELETE, &path, Some(json!({ "name": "  Abysse " }))).await;
    assert_eq!(s, 204);
    for gw in [&mut low_gw, &mut own_gw] {
        let deleted = gw.until(|e| if let Event::GuildDelete { guild_id } = e { Some(*guild_id) } else { None }).await;
        assert_eq!(deleted, gid);
    }

    // Plus rien : ni le serveur, ni ses salons, ni ses messages, ni ses invitations, ni ses membres.
    for who in [&own, &mid, &low] {
        assert_eq!(who.call(Method::GET, &path, None).await.0, 404);
        assert_eq!(who.call(Method::GET, &format!("/channels/{cid}/messages"), None).await.0, 404);
    }
    let (s, v) = low.use_code(code(&code_live)).await;
    assert_eq!((s.as_u16(), code(&v)), (404, "invite_unknown"));
    assert_eq!(own.call(Method::DELETE, &path, Some(right)).await.0, 404, "déjà supprimé");
    for table in ["channels", "messages", "roles", "invites", "members"] {
        let n: i64 = sqlx::query_scalar(&format!("select count(*) from {table}")).fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0, "{table}");
    }
    // Les comptes, eux, restent.
    assert_eq!(own.call(Method::GET, "/users/@me", None).await.0, 200);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn deleting_a_server_disconnects_the_people_talking_in_it(pool: PgPool) {
    let (base, state) = spawn_with_state(pool).await;
    let own = Who::new(&base, "own").await;
    let low = Who::new(&base, "low").await;
    let g = own.guild().await;
    low.join(&g, &own).await;
    let room = voice_room(&g);
    let mut low_gw = low.gateway().await;
    assert_eq!(low.join_voice(room).await.0, 200);
    let sfu = state.voice.clone().expect("le vocal tourne dans ce test");
    assert_eq!(sfu.state(low.user.id).and_then(|s| s.channel_id), Some(room));

    let (s, _) = own.call(Method::DELETE, &format!("/guilds/{}", g.guild.id), Some(json!({ "name": "Abysse" }))).await;
    assert_eq!(s, 204);
    let deleted = low_gw.until(|e| if let Event::GuildDelete { guild_id } = e { Some(*guild_id) } else { None }).await;
    assert_eq!(deleted, g.guild.id);
    assert!(sfu.state(low.user.id).is_none(), "sorti du vocal avec le serveur");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_timed_out_member_reads_but_cannot_speak_react_upload_or_join_voice(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let own = Who::new(&base, "own").await;
    let mid = Who::new(&base, "mid").await;
    let low = Who::new(&base, "low").await;
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    mid.join(&g, &own).await;
    low.join(&g, &own).await;
    let mods = own.role(gid, "Modération", perms::TIMEOUT).await;
    own.give(gid, &mid, &mods).await;
    let (s, hello) = own.say(cid, "bienvenue").await;
    assert_eq!(s, 201);
    let mut own_gw = own.gateway().await;

    // Il parle, puis il est mis de côté : tout le monde reçoit le membre mis à jour avec la fin.
    assert_eq!(low.say(cid, "avant").await.0, 201);
    let before = low.call(Method::GET, &format!("/guilds/{gid}"), None).await.1;
    let before: GuildState = serde_json::from_value(before).unwrap();
    assert!(before.members.iter().all(|m| m.timeout_until.is_none()));
    let (s, v) = mid.timeout(gid, &low, 600).await;
    assert_eq!(s, 200, "{v}");
    let member: Member = serde_json::from_value(v).unwrap();
    let until = member.timeout_until.expect("la fin de l'exclusion");
    let now_ms = chrono_ms();
    assert!(until > now_ms + 590_000 && until <= now_ms + 610_000, "{until} vs {now_ms}");
    let update = own_gw
        .until(|e| match e {
            Event::GuildMemberUpdate { member, .. } if member.user.id == low.user.id => Some(member.clone()),
            _ => None,
        })
        .await;
    assert_eq!(update.timeout_until, Some(until));
    let state: GuildState =
        serde_json::from_value(low.call(Method::GET, &format!("/guilds/{gid}"), None).await.1).unwrap();
    assert_eq!(state.members.iter().find(|m| m.user.id == low.user.id).unwrap().timeout_until, Some(until));

    // Écrire, réagir, joindre un fichier, entrer en vocal : 403 `timed_out`, avec la fin.
    let denied = |(s, v): (StatusCode, Value)| {
        assert_eq!((s.as_u16(), code(&v)), (403, "timed_out"), "{v}");
        assert_eq!(v["timeout_until"].as_i64(), Some(until));
        assert!(v["message"].as_str().unwrap().contains("exclu"), "{v}");
    };
    denied(low.say(cid, "encore ?").await);
    let mid_msg = hello["id"].as_str().unwrap();
    denied(
        low.call(Method::PUT, &format!("/channels/{cid}/messages/{mid_msg}/reactions/%F0%9F%91%8D/@me"), None).await,
    );
    let up = low
        .c
        .post(format!("http://{base}/api/v1/channels/{cid}/attachments?filename=a.txt"))
        .header("authorization", format!("Session {}", low.token))
        .header("content-type", "text/plain")
        .body("salut")
        .send()
        .await
        .unwrap();
    assert_eq!(up.status(), 403);
    let up: Value = up.json().await.unwrap();
    assert_eq!(code(&up), "timed_out");
    denied(low.join_voice(voice_room(&g)).await);
    // Lire reste possible : historique, serveur, salons, profil.
    let (s, history) = low.call(Method::GET, &format!("/channels/{cid}/messages"), None).await;
    assert_eq!(s, 200);
    assert!(history.as_array().unwrap().len() >= 2);
    assert_eq!(low.call(Method::GET, &format!("/guilds/{gid}"), None).await.0, 200);
    // Un autre membre n'est pas touché.
    assert_eq!(own.say(cid, "tout va bien").await.0, 201);

    // La hiérarchie, comme pour expulser : pas soi-même, pas le propriétaire, pas un rang égal
    // ou supérieur, pas un administrateur ; et il faut le droit.
    assert_eq!(mid.timeout(gid, &mid, 60).await.0, 400);
    assert_eq!(mid.timeout(gid, &own, 60).await.0, 403);
    assert_eq!(low.timeout(gid, &mid, 60).await.0, 403, "sans le droit");
    assert_eq!(own.timeout(gid, &own, 60).await.0, 400);
    assert_eq!(mid.timeout(gid, &low, 59).await.0, 400, "au moins une minute");
    assert_eq!(mid.timeout(gid, &low, MAX_TIMEOUT_S + 1).await.0, 400, "28 jours au plus");
    let admins = own.role(gid, "Admins", perms::ADMINISTRATOR).await;
    let adm = Who::new(&base, "adm").await;
    adm.join(&g, &own).await;
    own.give(gid, &adm, &admins).await;
    // `mid` est sous les admins dans la hiérarchie : refusé ; le propriétaire, lui, essaie en vain.
    assert_eq!(mid.timeout(gid, &adm, 60).await.0, 403);
    let (s, v) = own.timeout(gid, &adm, 60).await;
    assert_eq!((s.as_u16(), v["message"].as_str().is_some_and(|m| m.contains("administrateur"))), (403, true));
    assert_eq!(adm.say(cid, "je parle toujours").await.0, 201);

    // La lever : il reparle, et le membre mis à jour part sans date.
    let path = format!("/guilds/{gid}/members/{}/timeout", low.user.id);
    assert_eq!(low.call(Method::DELETE, &path, None).await.0, 403, "il ne se lève pas lui-même");
    let (s, v) = mid.call(Method::DELETE, &path, None).await;
    assert_eq!(s, 200);
    assert_eq!(serde_json::from_value::<Member>(v).unwrap().timeout_until, None);
    let lifted = own_gw
        .until(|e| match e {
            Event::GuildMemberUpdate { member, .. } if member.user.id == low.user.id => Some(member.clone()),
            _ => None,
        })
        .await;
    assert_eq!(lifted.timeout_until, None);
    assert_eq!(low.say(cid, "me revoilà").await.0, 201);

    // Sans tâche de fond : l'exclusion se termine toute seule, par comparaison à l'heure.
    assert_eq!(mid.timeout(gid, &low, 60).await.0, 200);
    assert_eq!(low.say(cid, "non").await.0, 403);
    sqlx::query("update members set timeout_until = now() - interval '1 second' where user_id = $1")
        .bind(low.user.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(low.say(cid, "terminé").await.0, 201);
    let state: GuildState =
        serde_json::from_value(low.call(Method::GET, &format!("/guilds/{gid}"), None).await.1).unwrap();
    assert_eq!(state.members.iter().find(|m| m.user.id == low.user.id).unwrap().timeout_until, None);

    let trail: Vec<(String,)> =
        sqlx::query_as("select action from audit_log where action like 'member_timeout%' order by id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let trail: Vec<&str> = trail.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(trail, ["member_timeout", "member_timeout_remove", "member_timeout"]);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn timing_out_someone_in_voice_disconnects_them(pool: PgPool) {
    let base = spawn(pool).await;
    let own = Who::new(&base, "own").await;
    let low = Who::new(&base, "low").await;
    let g = own.guild().await;
    low.join(&g, &own).await;
    let room = voice_room(&g);
    let mut own_gw = own.gateway().await;
    assert_eq!(low.join_voice(room).await.0, 200);
    let here = own_gw.until(|e| if let Event::VoiceStateUpdate(s) = e { Some(s.clone()) } else { None }).await;
    assert_eq!(here.channel_id, Some(room));

    assert_eq!(own.timeout(g.guild.id, &low, 300).await.0, 200);
    let gone = own_gw.until(|e| if let Event::VoiceStateUpdate(s) = e { Some(s.clone()) } else { None }).await;
    assert_eq!((gone.user_id, gone.channel_id), (low.user.id, None));
    assert_eq!(low.join_voice(room).await.0, 403, "et ne peut pas revenir");
}

/// Maintenant, en millisecondes Unix.
fn chrono_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_timed_out_moderator_has_no_power_left_until_it_ends(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let own = Who::new(&base, "own").await;
    let mb = Who::new(&base, "mb").await;
    let low = Who::new(&base, "low").await;
    let adm = Who::new(&base, "adm").await;
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    for w in [&mb, &low, &adm] {
        w.join(&g, &own).await;
    }
    let every_power = perms::TIMEOUT
        | perms::KICK
        | perms::BAN
        | perms::MANAGE_ROLES
        | perms::MANAGE_CHANNELS
        | perms::MANAGE_GUILD
        | perms::CREATE_INVITE
        | perms::MANAGE_MESSAGES;
    let mods = own.role(gid, "Modération", every_power).await;
    let admins = own.role(gid, "Admins", perms::ADMINISTRATOR).await;
    own.give(gid, &mb, &mods).await;
    own.give(gid, &adm, &admins).await;
    let (s, old) = mb.say(cid, "mon ancien message").await;
    assert_eq!(s, 201);
    let (s, other) = mb.say(cid, "un autre à moi").await;
    assert_eq!(s, 201);
    let (old, other) = (old["id"].as_str().unwrap().to_string(), other["id"].as_str().unwrap().to_string());
    let (s, theirs) = own.invite(cid, json!({})).await;
    assert_eq!(s, 201);

    // Avant : il modère.
    let (s, v) = mb.timeout(gid, &low, 60).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(mb.call(Method::DELETE, &format!("/guilds/{gid}/members/{}/timeout", low.user.id), None).await.0, 200);

    let (s, v) = own.timeout(gid, &mb, 600).await;
    assert_eq!(s, 200, "{v}");
    let until = serde_json::from_value::<Member>(v).unwrap().timeout_until.unwrap();

    // Exclu : 403 partout, personne n'est touché.
    let forbidden = |(s, v): (StatusCode, Value)| assert_eq!((s.as_u16(), code(&v)), (403, "forbidden"), "{v}");
    let guild = |path: &str| format!("/guilds/{gid}{path}");
    forbidden(mb.call(Method::DELETE, &guild(&format!("/members/{}", low.user.id)), None).await);
    forbidden(mb.call(Method::PUT, &guild(&format!("/bans/{}", low.user.id)), Some(json!({}))).await);
    forbidden(mb.timeout(gid, &low, 60).await);
    forbidden(mb.call(Method::DELETE, &guild(&format!("/members/{}/timeout", mb.user.id)), None).await);
    forbidden(mb.call(Method::POST, &guild("/roles"), Some(json!({ "name": "Neuf", "permissions": 0 }))).await);
    forbidden(mb.call(Method::PUT, &guild(&format!("/members/{}/roles/{}", low.user.id, mods.id)), None).await);
    forbidden(mb.call(Method::GET, &guild("/bans"), None).await);
    forbidden(mb.call(Method::GET, &guild("/invites"), None).await);
    forbidden(mb.call(Method::DELETE, &format!("/invites/{}", code(&theirs)), None).await);
    forbidden(mb.call(Method::POST, &guild("/channels"), Some(json!({ "name": "neuf", "kind": "text" }))).await);
    forbidden(mb.call(Method::DELETE, &format!("/channels/{cid}"), None).await);
    // Inviter : l'exclusion est dite, avec sa fin.
    let (s, v) = mb.invite(cid, json!({})).await;
    assert_eq!((s.as_u16(), code(&v)), (403, "timed_out"), "{v}");
    assert_eq!(v["timeout_until"].as_i64(), Some(until));
    let state: GuildState = serde_json::from_value(own.call(Method::GET, &guild(""), None).await.1).unwrap();
    assert_eq!(state.members.len(), 4);
    assert_eq!(own.call(Method::GET, &guild("/bans"), None).await.1.as_array().unwrap().len(), 0);
    // Trois invitations pour les arrivées, plus `theirs` : celle du modérateur exclu n'a pas été créée.
    assert_eq!(own.call(Method::GET, &guild("/invites"), None).await.1.as_array().unwrap().len(), 4);

    // Modifier ses anciens messages : 403 `timed_out` ; supprimer les siens reste permis.
    let (s, v) =
        mb.call(Method::PATCH, &format!("/channels/{cid}/messages/{old}"), Some(json!({ "content": "réécrit" }))).await;
    assert_eq!((s.as_u16(), code(&v)), (403, "timed_out"), "{v}");
    assert_eq!(v["timeout_until"].as_i64(), Some(until));
    let history = mb.call(Method::GET, &format!("/channels/{cid}/messages"), None).await.1;
    let text = |id: &str| {
        history.as_array().unwrap().iter().find(|m| m["id"] == id).map(|m| m["content"].as_str().unwrap().to_string())
    };
    assert_eq!(text(&old).as_deref(), Some("mon ancien message"), "le texte n'a pas changé");
    assert_eq!(
        mb.call(Method::DELETE, &format!("/channels/{cid}/messages/{other}"), None).await.0.as_u16(),
        204,
        "retirer son propre message reste permis"
    );

    // Le propriétaire et un administrateur gardent tout, même avec une exclusion en base.
    sqlx::query("update members set timeout_until = now() + interval '1 hour' where user_id = any($1)")
        .bind([adm.user.id, own.user.id])
        .execute(&pool)
        .await
        .unwrap();
    let (s, mine) = adm.say(cid, "à moi").await;
    assert_eq!(s, 201);
    let (s, v) = adm
        .call(
            Method::PATCH,
            &format!("/channels/{cid}/messages/{}", mine["id"].as_str().unwrap()),
            Some(json!({ "content": "à moi, corrigé" })),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        own.call(Method::PATCH, &format!("/channels/{cid}/messages/{old}"), Some(json!({ "content": "x" }))).await.1["code"],
        "forbidden",
        "pas le sien : refusé, mais pas pour une exclusion"
    );
    assert_eq!(adm.timeout(gid, &low, 60).await.0, 200);

    // Fini : il retrouve ses pouvoirs et peut expulser.
    sqlx::query("update members set timeout_until = now() - interval '1 second' where user_id = $1")
        .bind(mb.user.id)
        .execute(&pool)
        .await
        .unwrap();
    let (s, v) =
        mb.call(Method::PATCH, &format!("/channels/{cid}/messages/{old}"), Some(json!({ "content": "réécrit" }))).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(mb.call(Method::DELETE, &guild(&format!("/members/{}", low.user.id)), None).await.0.as_u16(), 204);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn leaving_a_server_does_not_lift_the_exclusion(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let own = Who::new(&base, "own").await;
    let low = Who::new(&base, "low").await;
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    low.join(&g, &own).await;
    let (s, v) = own.timeout(gid, &low, 600).await;
    assert_eq!(s, 200, "{v}");
    let until = serde_json::from_value::<Member>(v).unwrap().timeout_until.unwrap();

    // Il part, il revient par une invitation toute neuve : même exclusion, même fin.
    assert_eq!(low.call(Method::DELETE, &format!("/guilds/{gid}/members/@me"), None).await.0.as_u16(), 204);
    let mut own_gw = own.gateway().await;
    let (s, inv) = own.invite(cid, json!({})).await;
    assert_eq!(s, 201);
    let (s, back) = low.use_code(code(&inv)).await;
    assert_eq!(s, 200, "{back}");
    let back: GuildState = serde_json::from_value(back).unwrap();
    assert_eq!(back.members.iter().find(|m| m.user.id == low.user.id).unwrap().timeout_until, Some(until));
    let added = own_gw
        .until(|e| match e {
            Event::MemberAdd { member, .. } if member.user.id == low.user.id => Some(member.clone()),
            _ => None,
        })
        .await;
    assert_eq!(added.timeout_until, Some(until), "les autres le voient exclu");
    let (s, v) = low.say(cid, "pas si vite").await;
    assert_eq!((s.as_u16(), code(&v)), (403, "timed_out"), "{v}");
    assert_eq!(v["timeout_until"].as_i64(), Some(until));
    assert_eq!(low.call(Method::GET, &format!("/channels/{cid}/messages"), None).await.0, 200, "lire reste permis");

    // Une fois l'exclusion finie, partir puis revenir ne rend rien d'autre.
    sqlx::query("update members set timeout_until = now() - interval '1 second' where user_id = $1")
        .bind(low.user.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(low.say(cid, "terminé").await.0, 201);
    assert_eq!(low.call(Method::DELETE, &format!("/guilds/{gid}/members/@me"), None).await.0.as_u16(), 204);
    low.join(&g, &own).await;
    assert_eq!(low.say(cid, "libre").await.0, 201);

    // Renvoyé : c'est une sanction qui remplace l'exclusion.
    assert_eq!(own.timeout(gid, &low, 600).await.0, 200);
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/members/{}", low.user.id), None).await.0.as_u16(), 204);
    low.join(&g, &own).await;
    assert_eq!(low.say(cid, "revenu").await.0, 201);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn sending_while_a_channel_is_being_deleted_is_a_404_not_a_500(pool: PgPool) {
    let (base, state) = spawn_with_state(pool.clone()).await;
    let own = Who::new(&base, "own").await;
    let low = Who::new(&base, "low").await;
    let g = own.guild().await;
    let gid = g.guild.id;
    low.join(&g, &own).await;
    let (s, doomed) = own
        .call(Method::POST, &format!("/guilds/{gid}/channels"), Some(json!({ "name": "bientot", "kind": "text" })))
        .await;
    assert_eq!(s, 201, "{doomed}");
    let doomed: Id = doomed["id"].as_str().unwrap().parse().unwrap();

    // La suppression est en cours (pas validée) : l'envoi passe les vérifications, puis attend.
    let mut tx = state.db.pool.begin().await.unwrap();
    sqlx::query("delete from channels where id = $1").bind(doomed).execute(&mut *tx).await.unwrap();
    let sender = tokio::spawn(async move { low.say(doomed, "trop tard").await });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!sender.is_finished(), "l'envoi doit attendre la suppression");
    tx.commit().await.unwrap();
    let (s, v) = sender.await.unwrap();
    assert_eq!((s.as_u16(), code(&v)), (404, "not_found"), "{v}");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn fifty_active_invitations_per_person_and_a_list_of_two_hundred_at_most(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let own = Who::new(&base, "own").await;
    let sam = Who::new(&base, "sam").await;
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    sam.join(&g, &own).await;

    let mut codes = Vec::new();
    for _ in 0..MAX_INVITES_PER_MEMBER {
        let (s, v) = sam.invite(cid, json!({})).await;
        assert_eq!(s, 201, "{v}");
        codes.push(code(&v).to_string());
    }
    let (s, v) = sam.invite(cid, json!({})).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invite_limit"), "{v}");
    assert_eq!(v["message"], "Tu as déjà 50 invitations actives. Révoque-en une.");
    // Pas pour les autres, et la liste ne bouge pas.
    assert_eq!(own.invite(cid, json!({})).await.0, 201);
    let (s, list) = sam.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await;
    assert_eq!(s, 200);
    assert_eq!(rows(&list).len(), MAX_INVITES_PER_MEMBER);
    // En révoquer une en libère une.
    assert_eq!(sam.call(Method::DELETE, &format!("/invites/{}", codes[0]), None).await.0.as_u16(), 204);
    assert_eq!(sam.invite(cid, json!({})).await.0, 201);
    assert_eq!(sam.invite(cid, json!({})).await.0, 400);

    // La liste du gestionnaire s'arrête à 200 lignes, les vivantes d'abord.
    sqlx::query(
        "insert into invites (code, guild_id, channel_id, inviter_id)
         select 'extra' || lpad(n::text, 4, '0'), $1, $2, $3 from generate_series(1, 260) n",
    )
    .bind(gid)
    .bind(cid)
    .bind(own.user.id)
    .execute(&pool)
    .await
    .unwrap();
    let (s, list) = own.call(Method::GET, &format!("/guilds/{gid}/invites"), None).await;
    assert_eq!(s, 200);
    let list = rows(&list);
    assert_eq!(list.len(), MAX_INVITE_ROWS);
    assert!(list.iter().all(|i| i.state == InviteState::Active));
}
