//! Tranche verticale de bout en bout sur un vrai port et une vraie base : inscription,
//! serveur, invitation, message reçu en direct par le second compte via le gateway ; puis
//! sessions (cookie, anti-CSRF, déconnexion partout), suppression de compte, limites.

use futures_util::{SinkExt, StreamExt};
use kraken_proto::*;
use kraken_server::{AppState, Config};
use serde_json::json;
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

async fn spawn_with(pool: PgPool, cfg: Config) -> String {
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), cfg).await.unwrap();
    let app = kraken_server::app(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("{addr}")
}

async fn spawn(pool: PgPool) -> String {
    spawn_with(pool, config(1000)).await
}

async fn register(c: &reqwest::Client, base: &str, name: &str) -> AuthResp {
    let r = c
        .post(format!("http://{base}/api/v1/auth/register"))
        .json(&json!({"username": name, "email": format!("{name}@example.org"), "password": "motdepasse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    r.json().await.unwrap()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn register_create_invite_and_chat_live(pool: PgPool) {
    let base = spawn(pool).await;
    let c = reqwest::Client::new();
    let a = register(&c, &base, "lou").await;
    let b = register(&c, &base, "sam").await;
    let auth = |t: &str| format!("Session {t}");

    // Pseudo pris, mauvais mot de passe.
    let dup = c
        .post(format!("http://{base}/api/v1/auth/register"))
        .json(&json!({"username": "Lou", "email": "x@example.org", "password": "motdepasse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(dup.status(), 409);
    let bad = c
        .post(format!("http://{base}/api/v1/auth/login"))
        .json(&json!({"login": "lou", "password": "faux-faux"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 401);

    let g: GuildState = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", auth(&a.token))
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let general = g.channels.iter().find(|ch| ch.kind == ChannelKind::Text).unwrap().id;

    // Sam ne voit pas le salon avant d'avoir rejoint.
    let hidden = c
        .get(format!("http://{base}/api/v1/channels/{general}/messages"))
        .header("authorization", auth(&b.token))
        .send()
        .await
        .unwrap();
    assert_eq!(hidden.status(), 404);

    let inv: Invite = c
        .post(format!("http://{base}/api/v1/channels/{general}/invites"))
        .header("authorization", auth(&a.token))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let preview: Invite =
        c.get(format!("http://{base}/api/v1/invites/{}", inv.code)).send().await.unwrap().json().await.unwrap();
    assert_eq!((preview.guild.name.as_str(), preview.member_count), ("Abysse", 1));
    let joined = c
        .post(format!("http://{base}/api/v1/invites/{}", inv.code))
        .header("authorization", auth(&b.token))
        .send()
        .await
        .unwrap();
    assert_eq!(joined.status(), 200);

    // Sam ouvre le gateway.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{base}/gateway")).await.unwrap();
    assert!(matches!(read(&mut ws).await, ServerOp::Hello { .. }));
    ws.send(Ws::Text(serde_json::to_string(&ClientOp::Identify { token: Some(b.token.clone()) }).unwrap().into()))
        .await
        .unwrap();
    let ServerOp::Ready(ready) = read(&mut ws).await else { panic!("READY attendu") };
    assert_eq!(ready.guilds.len(), 1);

    // Lou écrit, deux fois avec le même nonce : un seul message, un seul événement.
    for _ in 0..2 {
        let r = c
            .post(format!("http://{base}/api/v1/channels/{general}/messages"))
            .header("authorization", auth(&a.token))
            .json(&json!({"content": "Bienvenue à bord !", "nonce": "n-1"}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }
    let ServerOp::Dispatch { seq: 1, event: Event::MessageCreate(m) } = read(&mut ws).await else {
        panic!("MESSAGE_CREATE attendu")
    };
    assert_eq!(m.content, "Bienvenue à bord !");
    assert_eq!(m.author.unwrap().username, "lou");

    let hist: Vec<Message> = c
        .get(format!("http://{base}/api/v1/channels/{general}/messages"))
        .header("authorization", auth(&b.token))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(hist.len(), 1);
}

async fn read(ws: &mut (impl StreamExt<Item = Result<Ws, tokio_tungstenite::tungstenite::Error>> + Unpin)) -> ServerOp {
    loop {
        if let Ws::Text(t) = ws.next().await.unwrap().unwrap() {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn cookie_sessions_csrf_and_logout_everywhere(pool: PgPool) {
    let base = spawn(pool).await;
    let jar = reqwest::Client::builder().cookie_store(true).build().unwrap();
    let r = jar
        .post(format!("http://{base}/api/v1/auth/register"))
        .json(&json!({"username": "lou", "email": "lou@example.org", "password": "motdepasse"}))
        .send()
        .await
        .unwrap();
    let set = r.headers().get("set-cookie").unwrap().to_str().unwrap().to_string();
    assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax"), "{set}");

    // Lecture par cookie : oui. Écriture par cookie sans Origin : refusée (CSRF).
    assert_eq!(jar.get(format!("http://{base}/api/v1/users/@me")).send().await.unwrap().status(), 200);
    let csrf = jar.post(format!("http://{base}/api/v1/guilds")).json(&json!({"name": "Abysse"})).send().await.unwrap();
    assert_eq!(csrf.status(), 403);
    let foreign = jar
        .post(format!("http://{base}/api/v1/guilds"))
        .header("origin", "https://evil.example")
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);
    let ok = jar
        .post(format!("http://{base}/api/v1/guilds"))
        .header("origin", format!("http://{base}"))
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 201);

    // Une seconde session (autre appareil) tombe aussi avec « déconnexion partout ».
    let other: AuthResp = reqwest::Client::new()
        .post(format!("http://{base}/api/v1/auth/login"))
        .json(&json!({"login": "lou", "password": "motdepasse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let out = jar
        .post(format!("http://{base}/api/v1/auth/logout-all"))
        .header("origin", format!("http://{base}"))
        .send()
        .await
        .unwrap();
    assert_eq!(out.status(), 204);
    let me = reqwest::Client::new()
        .get(format!("http://{base}/api/v1/users/@me"))
        .header("authorization", format!("Session {}", other.token))
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 401);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn health_and_unknown_api_route(pool: PgPool) {
    let base = spawn(pool).await;
    let r = reqwest::get(format!("http://{base}/api/v1/pas-de-route")).await.unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(r.json::<serde_json::Value>().await.unwrap()["code"], "not_found");
    let health = reqwest::get(format!("http://{base}/api/v1/health")).await.unwrap();
    assert_eq!(health.status(), 200);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn delete_account_reset_without_smtp_and_rate_limit(pool: PgPool) {
    let base = spawn_with(pool, config(6)).await;
    let c = reqwest::Client::new();
    let a = register(&c, &base, "lou").await;
    let auth = format!("Session {}", a.token);
    // Propriétaire d'un serveur : il peut quand même partir.
    let g = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", &auth)
        .json(&json!({"name": "Récif"}))
        .send()
        .await
        .unwrap();
    assert_eq!(g.status(), 201);

    let wrong = c
        .post(format!("http://{base}/api/v1/users/@me/delete"))
        .header("authorization", &auth)
        .json(&json!({"password": "pas-le-bon"}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 403);
    let gone = c
        .post(format!("http://{base}/api/v1/users/@me/delete"))
        .header("authorization", &auth)
        .json(&json!({"password": "motdepasse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), 202);
    let me = c.get(format!("http://{base}/api/v1/users/@me")).header("authorization", &auth).send().await.unwrap();
    assert_eq!(me.status(), 401);
    let login = c
        .post(format!("http://{base}/api/v1/auth/login"))
        .json(&json!({"login": "lou", "password": "motdepasse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 401);

    // Sans SMTP, l'instance le dit au lieu de faire semblant.
    let reset = c
        .post(format!("http://{base}/api/v1/auth/reset"))
        .json(&json!({"email": "lou@example.org"}))
        .send()
        .await
        .unwrap();
    assert_eq!(reset.status(), 501);

    // 6 essais par minute : register + 2 suppressions + login + reset = 5, il en reste 1.
    let mut codes = vec![];
    for _ in 0..4 {
        let r = c
            .post(format!("http://{base}/api/v1/auth/login"))
            .json(&json!({"login": "x", "password": "y"}))
            .send()
            .await
            .unwrap();
        codes.push(r.status().as_u16());
    }
    assert_eq!(codes, [401, 429, 429, 429]);
}

/// BUG-002 : la déconnexion d'un appareil ferme son gateway (et seulement le sien).
#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn logout_closes_that_sessions_gateway(pool: PgPool) {
    let base = spawn(pool).await;
    let c = reqwest::Client::new();
    let first = register(&c, &base, "lou").await;
    let second: AuthResp = c
        .post(format!("http://{base}/api/v1/auth/login"))
        .json(&json!({"login": "lou", "password": "motdepasse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let open = |token: String| {
        let base = base.clone();
        async move {
            let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{base}/gateway")).await.unwrap();
            read(&mut ws).await;
            let op = ClientOp::Identify { token: Some(token) };
            ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
            assert!(matches!(read(&mut ws).await, ServerOp::Ready(_)));
            ws
        }
    };
    let mut gone = open(first.token.clone()).await;
    let mut kept = open(second.token.clone()).await;

    let out = c
        .post(format!("http://{base}/api/v1/auth/logout"))
        .header("authorization", format!("Session {}", first.token))
        .send()
        .await
        .unwrap();
    assert_eq!(out.status(), 204);
    let end = tokio::time::timeout(std::time::Duration::from_secs(2), gone.next()).await.expect("gateway fermé");
    assert!(!matches!(end, Some(Ok(Ws::Text(_)))), "plus aucun événement : {end:?}");

    let made = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", format!("Session {}", second.token))
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(made.status(), 201);
    assert!(matches!(read(&mut kept).await, ServerOp::Dispatch { event: Event::GuildCreate(_), .. }));
}

/// Nom affiché : modifiable, validé, diffusé en direct à ceux qui partagent un serveur.
#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn display_name_changes_live(pool: PgPool) {
    let base = spawn(pool).await;
    let c = reqwest::Client::new();
    let lou = register(&c, &base, "lou").await;
    let sam = register(&c, &base, "sam").await;
    let auth = |t: &str| format!("Session {t}");
    let g: GuildState = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", auth(&lou.token))
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let inv: Invite = c
        .post(format!(
            "http://{base}/api/v1/channels/{}/invites",
            g.channels.iter().find(|c| c.kind.is_textual()).unwrap().id
        ))
        .header("authorization", auth(&lou.token))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let joined = c
        .post(format!("http://{base}/api/v1/invites/{}", inv.code))
        .header("authorization", auth(&sam.token))
        .send()
        .await;
    assert_eq!(joined.unwrap().status(), 200);

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{base}/gateway")).await.unwrap();
    read(&mut ws).await;
    let op = ClientOp::Identify { token: Some(sam.token.clone()) };
    ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
    assert!(matches!(read(&mut ws).await, ServerOp::Ready(_)));

    let patch = |name: serde_json::Value| {
        c.patch(format!("http://{base}/api/v1/users/@me"))
            .header("authorization", auth(&lou.token))
            .json(&json!({ "display_name": name }))
            .send()
    };
    assert_eq!(patch(json!("x".repeat(33))).await.unwrap().status(), 400);
    assert_eq!(patch(json!("deux\nlignes")).await.unwrap().status(), 400);
    let u: User = patch(json!("  Lou des abysses 🐙 ")).await.unwrap().json().await.unwrap();
    assert_eq!((u.username.as_str(), u.name()), ("lou", "Lou des abysses 🐙"));
    let ServerOp::Dispatch { event: Event::UserUpdate(seen), .. } = read(&mut ws).await else {
        panic!("USER_UPDATE attendu")
    };
    assert_eq!(seen.name(), "Lou des abysses 🐙");
    // Vide : on revient au pseudo unique.
    let u: User = patch(json!("   ")).await.unwrap().json().await.unwrap();
    assert_eq!(u.display_name, None);
    assert_eq!(u.name(), "lou");
}

/// Modifier, supprimer, répondre, réagir : droits vérifiés par le serveur pour un second
/// compte et un inconnu, événements reçus en direct.
#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn edit_delete_reply_and_react(pool: PgPool) {
    let base = spawn(pool).await;
    let c = reqwest::Client::new();
    let lou = register(&c, &base, "lou").await;
    let sam = register(&c, &base, "sam").await;
    let noa = register(&c, &base, "noa").await;
    let auth = |t: &str| format!("Session {t}");
    let api = |p: String| format!("http://{base}/api/v1{p}");
    let g: GuildState = c
        .post(api("/guilds".into()))
        .header("authorization", auth(&lou.token))
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let cid = g.channels.iter().find(|c| c.kind.is_textual()).unwrap().id;
    let inv: Invite = c
        .post(api(format!("/channels/{cid}/invites")))
        .header("authorization", auth(&lou.token))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let joined = c.post(api(format!("/invites/{}", inv.code))).header("authorization", auth(&sam.token)).send().await;
    assert_eq!(joined.unwrap().status(), 200);

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{base}/gateway")).await.unwrap();
    read(&mut ws).await;
    let op = ClientOp::Identify { token: Some(sam.token.clone()) };
    ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
    assert!(matches!(read(&mut ws).await, ServerOp::Ready(_)));
    let mut next = async || match read(&mut ws).await {
        ServerOp::Dispatch { event, .. } => event,
        other => panic!("DISPATCH attendu : {other:?}"),
    };

    // Lou (propriétaire) mentionne sam et tout le monde : il en a le droit.
    let m: Message = c
        .post(api(format!("/channels/{cid}/messages")))
        .header("authorization", auth(&lou.token))
        .json(&json!({"content": "**Bienvenue** @sam et @everyone"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(m.mention_everyone && m.mentions.len() == 1);
    assert!(matches!(next().await, Event::MessageCreate(x) if x.id == m.id));
    let msg = format!("/channels/{cid}/messages/{}", m.id);

    // Sam ne peut ni modifier ni supprimer le message de lou ; noa ne voit rien.
    let r = c.patch(api(msg.clone())).header("authorization", auth(&sam.token)).json(&json!({"content": "x"})).send();
    assert_eq!(r.await.unwrap().status(), 403);
    assert_eq!(
        c.delete(api(msg.clone())).header("authorization", auth(&sam.token)).send().await.unwrap().status(),
        403
    );
    let r = c.patch(api(msg.clone())).header("authorization", auth(&noa.token)).json(&json!({"content": "x"})).send();
    assert_eq!(r.await.unwrap().status(), 404);
    assert_eq!(
        c.delete(api(msg.clone())).header("authorization", auth(&noa.token)).send().await.unwrap().status(),
        404
    );
    let octo = "%F0%9F%90%99"; // 🐙
    let react = |who: &str, emoji: &str| {
        c.put(api(format!("{msg}/reactions/{emoji}/@me"))).header("authorization", auth(who)).send()
    };
    assert_eq!(react(&noa.token, octo).await.unwrap().status(), 404);
    assert_eq!(react(&sam.token, "ok").await.unwrap().status(), 400);

    // Sam réagit (deux fois : un seul événement), puis répond ; sam, lui, ne mentionne pas tout le monde.
    for _ in 0..2 {
        assert_eq!(react(&sam.token, octo).await.unwrap().status(), 204);
    }
    let ev = next().await;
    assert_eq!(ev, Event::ReactionAdd { channel_id: cid, message_id: m.id, user_id: sam.user.id, emoji: "🐙".into() });
    let reply: Message = c
        .post(api(format!("/channels/{cid}/messages")))
        .header("authorization", auth(&sam.token))
        .json(&json!({"content": "Merci @lou ! @everyone", "reply_to": m.id}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reply.reply.as_ref().and_then(|r| r.id), Some(m.id));
    assert!(!reply.mention_everyone, "@everyone réservé à qui en a le droit");
    assert!(matches!(next().await, Event::MessageCreate(_)));

    // Lou modifie son message : MESSAGE_UPDATE.
    let r = c.patch(api(msg.clone())).header("authorization", auth(&lou.token)).json(&json!({"content": "Bienvenue"}));
    let edited: Message = r.send().await.unwrap().json().await.unwrap();
    assert!(edited.edited_at.is_some() && edited.mentions.is_empty() && !edited.mention_everyone);
    assert_eq!(edited.reactions.len(), 1);
    assert!(matches!(next().await, Event::MessageUpdate(x) if x.content == "Bienvenue"));

    // Sam retire sa réaction ; lou, propriétaire, supprime la réponse de sam.
    let r = c.delete(api(format!("{msg}/reactions/{octo}/@me"))).header("authorization", auth(&sam.token));
    assert_eq!(r.send().await.unwrap().status(), 204);
    assert!(matches!(next().await, Event::ReactionRemove { .. }));
    let theirs = format!("/channels/{cid}/messages/{}", reply.id);
    assert_eq!(
        c.delete(api(theirs.clone())).header("authorization", auth(&lou.token)).send().await.unwrap().status(),
        204
    );
    assert_eq!(next().await, Event::MessageDelete { channel_id: cid, id: reply.id });
    assert_eq!(c.delete(api(theirs)).header("authorization", auth(&lou.token)).send().await.unwrap().status(), 404);
    assert_eq!(c.delete(api(msg)).header("authorization", auth(&lou.token)).send().await.unwrap().status(), 204);
}
