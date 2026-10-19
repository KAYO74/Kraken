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
async fn unknown_api_route_is_json_404(pool: PgPool) {
    let base = spawn(pool).await;
    let r = reqwest::get(format!("http://{base}/api/v1/pas-de-route")).await.unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(r.json::<serde_json::Value>().await.unwrap()["code"], "not_found");
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
