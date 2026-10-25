//! Salons vocaux côté API, sur un vrai port et une vraie base : droits pour rejoindre, salon
//! plein, vocal indisponible, et les états vocaux diffusés (gateway, READY, serveur). L'offre
//! WebRTC vient d'un vrai pair (str0m) ; le son lui-même est testé dans `kraken-voice`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use kraken_core::perms;
use kraken_proto::*;
use kraken_server::{AppState, Config};
use serde_json::json;
use sqlx::PgPool;
use str0m::media::{Direction, MediaKind};
use str0m::{Candidate, Rtc};
use tokio_tungstenite::tungstenite::Message as Ws;

type Gateway = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn config(rtc_port: u16) -> Config {
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
        rtc_port,
        rtc_fallback: false,
        rtc_ips: vec![std::net::Ipv4Addr::LOCALHOST.into()],
        rtc_public_port: None,
    }
}

async fn spawn(pool: PgPool, cfg: Config) -> String {
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), cfg).await.unwrap();
    let app = kraken_server::app(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("{addr}")
}

/// Un compte, et son en-tête d'authentification.
async fn register(c: &reqwest::Client, base: &str, name: &str) -> (Id, String) {
    let r = c
        .post(format!("http://{base}/api/v1/auth/register"))
        .json(&json!({"username": name, "email": format!("{name}@example.org"), "password": "motdepasse"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let a: AuthResp = r.json().await.unwrap();
    (a.user.id, format!("Session {}", a.token))
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

async fn join_voice(c: &reqwest::Client, base: &str, auth: &str, cid: Id) -> reqwest::Response {
    c.post(format!("http://{base}/api/v1/channels/{cid}/voice"))
        .header("authorization", auth)
        .json(&offer())
        .send()
        .await
        .unwrap()
}

async fn gateway(base: &str, auth: &str) -> (Gateway, Box<Ready>) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{base}/gateway")).await.unwrap();
    assert!(matches!(read(&mut ws).await, ServerOp::Hello { .. }));
    let token = auth.trim_start_matches("Session ").to_string();
    ws.send(Ws::Text(serde_json::to_string(&ClientOp::Identify { token: Some(token) }).unwrap().into())).await.unwrap();
    let ServerOp::Ready(ready) = read(&mut ws).await else { panic!("READY attendu") };
    (ws, ready)
}

async fn read(ws: &mut Gateway) -> ServerOp {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("événement attendu");
        if let Ws::Text(t) = m.unwrap().unwrap() {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

async fn next_voice(ws: &mut Gateway) -> VoiceState {
    loop {
        if let ServerOp::Dispatch { event: Event::VoiceStateUpdate(s), .. } = read(ws).await {
            return s;
        }
    }
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn join_rights_and_voice_states(pool: PgPool) {
    let base = spawn(pool.clone(), config(0)).await;
    let c = reqwest::Client::new();
    let (lou, a) = register(&c, &base, "lou").await;
    let (sam, b) = register(&c, &base, "sam").await;
    let g: GuildState = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", &a)
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let gid = g.guild.id;
    let text = g.channels.iter().find(|ch| ch.kind == ChannelKind::Text).unwrap().id;
    let voice = g.channels.iter().find(|ch| ch.kind == ChannelKind::Voice).unwrap().id;
    assert!(g.voice_states.is_empty());

    // Pas membre : le salon n'existe pas pour lui. Sans session : 401.
    assert_eq!(join_voice(&c, &base, &b, voice).await.status(), 404);
    let anonymous = c.post(format!("http://{base}/api/v1/channels/{voice}/voice")).json(&offer()).send().await.unwrap();
    assert_eq!(anonymous.status(), 401);
    // Un salon textuel n'est pas un salon vocal.
    assert_eq!(join_voice(&c, &base, &a, text).await.status(), 400);

    let inv: Invite = c
        .post(format!("http://{base}/api/v1/channels/{text}/invites"))
        .header("authorization", &a)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let joined = c.post(format!("http://{base}/api/v1/invites/{}", inv.code)).header("authorization", &b).send().await;
    assert_eq!(joined.unwrap().status(), 200);

    // Membre, mais CONNECT retiré sur ce salon : 403.
    sqlx::query(
        "insert into permission_overwrites (channel_id, target_id, target_kind, deny) values ($1, $2, 'member', $3)",
    )
    .bind(voice)
    .bind(sam)
    .bind(perms::CONNECT as i64)
    .execute(&pool)
    .await
    .unwrap();
    let denied = join_voice(&c, &base, &b, voice).await;
    assert_eq!(denied.status(), 403);
    sqlx::query("delete from permission_overwrites").execute(&pool).await.unwrap();

    // Offre inutilisable : 400, et personne n'est annoncé.
    let mut bad = offer();
    bad.mids.mic = bad.mids.screen.clone();
    let r = c.post(format!("http://{base}/api/v1/channels/{voice}/voice")).header("authorization", &a).json(&bad);
    assert_eq!(r.send().await.unwrap().status(), 400);

    // Lou rejoint : réponse ICE-lite qui annonce 127.0.0.1 ; Sam le voit arriver.
    let (mut ws, _) = gateway(&base, &b).await;
    let r = join_voice(&c, &base, &a, voice).await;
    assert_eq!(r.status(), 200);
    let answer: VoiceAnswer = r.json().await.unwrap();
    assert!(answer.sdp.contains("a=ice-lite"));
    assert!(answer.sdp.contains(" 127.0.0.1 "), "candidat local annoncé :\n{}", answer.sdp);
    let s = next_voice(&mut ws).await;
    let here = VoiceState {
        guild_id: gid,
        channel_id: Some(voice),
        user_id: lou,
        self_mute: false,
        self_deaf: false,
        streaming: false,
    };
    assert_eq!(s, here);

    // État vocal dans le serveur, au READY et dans GUILD_CREATE de qui arrive ensuite.
    let g: GuildState = c
        .get(format!("http://{base}/api/v1/guilds/{gid}"))
        .header("authorization", &b)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(g.voice_states, std::slice::from_ref(&here));
    let (_, ready) = gateway(&base, &b).await;
    assert_eq!(ready.guilds[0].voice_states, std::slice::from_ref(&here));
    let (_, noa) = register(&c, &base, "noa").await;
    let (mut ws_noa, _) = gateway(&base, &noa).await;
    c.post(format!("http://{base}/api/v1/invites/{}", inv.code)).header("authorization", &noa).send().await.unwrap();
    let created = loop {
        if let ServerOp::Dispatch { event: Event::GuildCreate(g), .. } = read(&mut ws_noa).await {
            break g;
        }
    };
    assert_eq!(created.voice_states, std::slice::from_ref(&here));

    // Salon caché à Sam : il apprend seulement que Lou n'est dans aucun salon qu'il voit.
    let secret: Channel = c
        .post(format!("http://{base}/api/v1/guilds/{gid}/channels"))
        .header("authorization", &a)
        .json(&json!({"name": "QG", "kind": "voice", "parent_id": null}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    sqlx::query(
        "insert into permission_overwrites (channel_id, target_id, target_kind, deny) values ($1, $2, 'member', $3)",
    )
    .bind(secret.id)
    .bind(sam)
    .bind(perms::VIEW_CHANNEL as i64)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(join_voice(&c, &base, &a, secret.id).await.status(), 200);
    assert_eq!(next_voice(&mut ws).await, VoiceState { channel_id: None, ..here.clone() });
    assert_eq!(next_voice(&mut ws_noa).await.channel_id, Some(secret.id));
    let g: GuildState = c
        .get(format!("http://{base}/api/v1/guilds/{gid}"))
        .header("authorization", &b)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(g.voice_states.is_empty(), "rien sur un salon caché");

    // Lou quitte : tout le monde le voit partir ; quitter deux fois ne fait rien.
    for _ in 0..2 {
        let r = c.delete(format!("http://{base}/api/v1/voice")).header("authorization", &a).send().await.unwrap();
        assert_eq!(r.status(), 204);
    }
    assert_eq!(next_voice(&mut ws_noa).await, VoiceState { channel_id: None, ..here.clone() });
    let g: GuildState = c
        .get(format!("http://{base}/api/v1/guilds/{gid}"))
        .header("authorization", &a)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(g.voice_states.is_empty());

    // L'adresse publique, pour les liens d'invitation.
    let i: Instance = c.get(format!("http://{base}/api/v1/instance")).send().await.unwrap().json().await.unwrap();
    assert_eq!(i.public_url, "http://127.0.0.1:8080");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn room_full(pool: PgPool) {
    let base = spawn(pool, config(0)).await;
    let c = reqwest::Client::new();
    let (_, owner) = register(&c, &base, "lou").await;
    let g: GuildState = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", &owner)
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let text = g.channels.iter().find(|ch| ch.kind == ChannelKind::Text).unwrap().id;
    let voice = g.channels.iter().find(|ch| ch.kind == ChannelKind::Voice).unwrap().id;
    let inv: Invite = c
        .post(format!("http://{base}/api/v1/channels/{text}/invites"))
        .header("authorization", &owner)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut users = vec![owner];
    for i in 1..=MAX_VOICE_MEMBERS {
        let (_, auth) = register(&c, &base, &format!("membre{i}")).await;
        c.post(format!("http://{base}/api/v1/invites/{}", inv.code))
            .header("authorization", &auth)
            .send()
            .await
            .unwrap();
        users.push(auth);
    }
    for auth in &users[..MAX_VOICE_MEMBERS] {
        assert_eq!(join_voice(&c, &base, auth, voice).await.status(), 200);
    }
    let last = &users[MAX_VOICE_MEMBERS];
    let full = join_voice(&c, &base, last, voice).await;
    assert_eq!(full.status(), 409);
    assert_eq!(full.json::<ApiError>().await.unwrap().code, "voice_full");
    // Rejoindre à nouveau quand on y est déjà : pas compté deux fois.
    assert_eq!(join_voice(&c, &base, &users[0], voice).await.status(), 200);
    // Une place se libère.
    c.delete(format!("http://{base}/api/v1/voice")).header("authorization", &users[0]).send().await.unwrap();
    assert_eq!(join_voice(&c, &base, last, voice).await.status(), 200);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn voice_unavailable_when_the_port_is_taken(pool: PgPool) {
    let busy = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    let base = spawn(pool, config(busy.local_addr().unwrap().port())).await;
    let c = reqwest::Client::new();
    assert_eq!(c.get(format!("http://{base}/api/v1/health")).send().await.unwrap().status(), 200);
    let (_, a) = register(&c, &base, "lou").await;
    let g: GuildState = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", &a)
        .json(&json!({"name": "Abysse"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let voice = g.channels.iter().find(|ch| ch.kind == ChannelKind::Voice).unwrap().id;
    let r = join_voice(&c, &base, &a, voice).await;
    assert_eq!(r.status(), 503);
    assert_eq!(r.json::<ApiError>().await.unwrap().code, "voice_unavailable");
}
