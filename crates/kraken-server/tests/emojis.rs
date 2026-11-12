//! Emojis perso d'un serveur, de bout en bout sur un vrai port et une vraie base : droit « Gérer les
//! émojis », 50 par serveur, formats reconnus aux premiers octets, 256 Kio, garde de dimensions,
//! noms uniques sans tenir compte de la casse, lecture de l'image avec cache d'un jour, événement en
//! direct, réactions, et fichiers qui partent avec l'emoji ou le serveur.

mod common;

use std::io::Cursor;

use futures_util::{SinkExt, StreamExt};
use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::{AnimationDecoder, Delay, Frame, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use kraken_core::perms;
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
        max_upload_bytes: 25 * 1024 * 1024,
        link_previews: true,
        preview_allow_private: false,
    }
}

/// Serveur et dossier de données propre à ce test.
async fn spawn(pool: PgPool) -> (String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kraken-emojis-{}", rand::random::<u64>()));
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

    /// Envoie une image comme emoji : le corps est l'image, le nom est dans l'adresse.
    async fn upload(&self, gid: Id, name: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
        let r = self
            .c
            .post(format!("http://{}/api/v1/guilds/{gid}/emojis?name={name}", self.base))
            .header("authorization", format!("Session {}", self.token))
            // Le type annoncé ne compte pas : seuls les premiers octets décident.
            .header("content-type", "text/plain")
            .body(bytes)
            .send()
            .await
            .unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn guild(&self, name: &str) -> GuildState {
        let (s, v) = self.call(Method::POST, "/guilds", Some(json!({ "name": name }))).await;
        assert_eq!(s, 201);
        serde_json::from_value(v).unwrap()
    }

    /// Rejoint ce serveur par une invitation de `inviter`.
    async fn join(&self, g: &GuildState, inviter: &Who) {
        let cid = general(g);
        let (s, v) = inviter.call(Method::POST, &format!("/channels/{cid}/invites"), Some(json!({}))).await;
        assert_eq!(s, 201);
        assert_eq!(self.call(Method::POST, &format!("/invites/{}", v["code"].as_str().unwrap()), None).await.0, 200);
    }

    async fn role(&self, gid: Id, give: &Who, permissions: u64) {
        let (s, v) = self
            .call(
                Method::POST,
                &format!("/guilds/{gid}/roles"),
                Some(json!({ "name": "Dessin", "permissions": permissions })),
            )
            .await;
        assert_eq!(s, 201, "{v}");
        let rid = v["id"].as_str().unwrap();
        assert_eq!(
            self.call(Method::PUT, &format!("/guilds/{gid}/members/{}/roles/{rid}", give.user.id), None).await.0,
            200
        );
    }

    async fn get_image(&self, eid: &str) -> reqwest::Response {
        self.c
            .get(format!("http://{}/api/v1/emojis/{eid}/image", self.base))
            .header("authorization", format!("Session {}", self.token))
            .send()
            .await
            .unwrap()
    }

    async fn gateway(&self) -> Gateway {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/gateway", self.base)).await.unwrap();
        Gateway::read(&mut ws).await;
        let op = ClientOp::Identify { token: Some(self.token.clone()) };
        ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
        let ServerOp::Ready(ready) = Gateway::read(&mut ws).await else { panic!("READY attendu") };
        Gateway { ws, ready: *ready }
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct Gateway {
    ws: Socket,
    ready: Ready,
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

    /// La prochaine liste d'emojis annoncée, en laissant passer les autres événements (arrivées, etc.).
    async fn next_emojis(&mut self) -> Vec<Emoji> {
        loop {
            if let Event::EmojisUpdate { emojis, .. } = self.next().await {
                return emojis;
            }
        }
    }

    async fn next(&mut self) -> Event {
        match Self::read(&mut self.ws).await {
            ServerOp::Dispatch { event, .. } => event,
            other => panic!("DISPATCH attendu : {other:?}"),
        }
    }
}

async fn put_reaction(who: &Who, cid: Id, mid: &str, emoji: &str) -> (StatusCode, Value) {
    who.call(Method::PUT, &format!("/channels/{cid}/messages/{mid}/reactions/{emoji}/@me"), None).await
}

fn general(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id
}

fn code(v: &Value) -> &str {
    v["code"].as_str().unwrap_or_default()
}

fn picture(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 120]));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format).unwrap();
    out
}

/// Un GIF de `frames` images.
fn gif(w: u32, h: u32, frames: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for i in 0..frames {
            let img = RgbaImage::from_fn(w, h, |x, y| Rgba([(x * 7 + i * 60) as u8, (y * 5) as u8, 90, 255]));
            enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(100, 1))).unwrap();
        }
    }
    out
}

/// Un GIF animé uni, qui tient dans 256 Kio malgré de grandes dimensions.
fn flat_gif(w: u32, h: u32, frames: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for i in 0..frames {
            let img = RgbaImage::from_pixel(w, h, Rgba([(i * 90) as u8, 40, 90, 255]));
            enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(100, 1))).unwrap();
        }
    }
    out
}

fn chunk(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut c = fourcc.to_vec();
    c.extend((payload.len() as u32).to_le_bytes());
    c.extend(payload);
    if payload.len() % 2 == 1 {
        c.push(0);
    }
    c
}

/// Un WebP animé de `frames` images (chacune une image sans perte encodée par `image`).
fn animated_webp(w: u32, h: u32, frames: u32) -> Vec<u8> {
    let le24 = |n: u32| n.to_le_bytes()[..3].to_vec();
    let mut vp8x = vec![0b0000_0010, 0, 0, 0];
    vp8x.extend(le24(w - 1));
    vp8x.extend(le24(h - 1));
    let mut body = b"WEBP".to_vec();
    body.extend(chunk(b"VP8X", &vp8x));
    body.extend(chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]));
    for i in 0..frames {
        let img = RgbaImage::from_fn(w, h, |x, y| Rgba([(x * 9 + i * 80) as u8, (y * 3) as u8, 40, 255]));
        let mut still = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut still)
            .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        let frame = still[12..].to_vec();
        let mut anmf = Vec::new();
        anmf.extend(le24(0));
        anmf.extend(le24(0));
        anmf.extend(le24(w - 1));
        anmf.extend(le24(h - 1));
        anmf.extend(le24(100));
        anmf.push(0);
        anmf.extend(frame);
        body.extend(chunk(b"ANMF", &anmf));
    }
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir.join("emojis")) else { return vec![] };
    let mut names: Vec<String> = rd.map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    names.sort();
    names
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn an_emoji_is_added_seen_live_served_renamed_and_removed(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let g = lou.guild("Abysse").await;
    let gid = g.guild.id;
    sam.join(&g, &lou).await;
    let mut sam_gw = sam.gateway().await;
    assert!(sam_gw.ready.guilds[0].emojis.is_empty());

    // Sans session : rien ne passe, et rien n'est écrit.
    let anon = Client::new();
    let r = anon.post(format!("http://{base}/api/v1/guilds/{gid}/emojis?name=poulpe")).body(picture(
        32,
        32,
        ImageFormat::Png,
    ));
    assert_eq!(r.send().await.unwrap().status(), 401);
    assert_eq!(anon.get(format!("http://{base}/api/v1/guilds/{gid}/emojis")).send().await.unwrap().status(), 401);
    assert!(files_in(&dir).is_empty());

    // Un PNG large est recadré au centre en carré, réduit à 128 pixels.
    let (s, v) = lou.upload(gid, "poulpe", picture(500, 300, ImageFormat::Png)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let e: Emoji = serde_json::from_value(v).unwrap();
    assert_eq!((e.name.as_str(), e.guild_id, e.animated), ("poulpe", gid, false));
    assert_eq!(e.creator, Some(EmojiCreator { id: lou.user.id, name: "lou".into() }));
    assert_eq!(sam_gw.next_emojis().await, vec![e.clone()]);
    assert_eq!(files_in(&dir).len(), 1);

    // L'image : une session suffit (même sans être dans ce serveur), cache d'un jour, PNG de 128 × 128.
    let noa = Who::new(&base, "noa").await;
    for who in [&lou, &sam, &noa] {
        let r = who.get_image(&e.id.to_string()).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "image/png");
        let cache = r.headers()["cache-control"].to_str().unwrap().to_string();
        assert_eq!(cache, "private, max-age=86400", "un jour, pas d'`immutable` : un emoji retiré ne reste pas un an");
        let img = image::load_from_memory(&r.bytes().await.unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (EMOJI_SIZE, EMOJI_SIZE));
    }
    assert_eq!(
        Client::new().get(format!("http://{base}/api/v1/emojis/{}/image", e.id)).send().await.unwrap().status(),
        401
    );
    assert_eq!(lou.get_image(&Id::nil().to_string()).await.status(), 404);
    assert_eq!(lou.get_image("pas-un-id").await.status(), 400);

    // Le serveur, la liste du serveur et la liste de tous mes emojis le disent.
    let (_, listed) = sam.call(Method::GET, &format!("/guilds/{gid}/emojis"), None).await;
    assert_eq!(serde_json::from_value::<Vec<Emoji>>(listed).unwrap(), std::slice::from_ref(&e));
    let (_, state) = sam.call(Method::GET, &format!("/guilds/{gid}"), None).await;
    assert_eq!(serde_json::from_value::<GuildState>(state).unwrap().emojis, std::slice::from_ref(&e));
    let (_, mine) = sam.call(Method::GET, "/users/@me/emojis", None).await;
    assert_eq!(serde_json::from_value::<Vec<Emoji>>(mine).unwrap(), std::slice::from_ref(&e));
    assert_eq!(noa.call(Method::GET, "/users/@me/emojis", None).await.1, json!([]));
    assert_eq!(noa.call(Method::GET, &format!("/guilds/{gid}/emojis"), None).await.0, 404);
    // Un nouveau membre reçoit les emojis avec le serveur, et à l'ouverture.
    noa.join(&g, &lou).await;
    assert_eq!(noa.gateway().await.ready.guilds[0].emojis, std::slice::from_ref(&e));

    // Renommer : l'id et l'image ne changent pas, les membres sont prévenus. Même nom : rien.
    let (s, v) =
        lou.call(Method::PATCH, &format!("/guilds/{gid}/emojis/{}", e.id), Some(json!({ "name": "Poulpe_2" }))).await;
    assert_eq!(s, 200, "{v}");
    let renamed: Emoji = serde_json::from_value(v).unwrap();
    assert_eq!((renamed.id, renamed.name.as_str()), (e.id, "Poulpe_2"));
    assert_eq!(sam_gw.next_emojis().await, vec![renamed.clone()]);
    assert_eq!(
        lou.call(Method::PATCH, &format!("/guilds/{gid}/emojis/{}", e.id), Some(json!({ "name": "Poulpe_2" }))).await.0,
        200
    );
    assert_eq!(lou.get_image(&e.id.to_string()).await.status(), 200);

    // Un JPEG devient un PNG, une petite image n'est pas agrandie ; GIF fixe : PNG aussi.
    let (s, v) = lou.upload(gid, "photo", picture(90, 60, ImageFormat::Jpeg)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let r = lou.get_image(v["id"].as_str().unwrap()).await;
    assert_eq!(r.headers()["content-type"], "image/png");
    let img = image::load_from_memory(&r.bytes().await.unwrap()).unwrap();
    assert_eq!((img.width(), img.height()), (60, 60));
    let (s, v) = lou.upload(gid, "fixe", gif(40, 40, 1)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["animated"], false);
    assert_eq!(lou.get_image(v["id"].as_str().unwrap()).await.headers()["content-type"], "image/png");
    assert_eq!(files_in(&dir).len(), 3);
    while sam_gw.next_emojis().await.len() != 3 {}

    // Retirer : le fichier part, l'image n'est plus servie, les membres sont prévenus.
    let (s, _) = lou.call(Method::DELETE, &format!("/guilds/{gid}/emojis/{}", e.id), None).await;
    assert_eq!(s, 204);
    let left = sam_gw.next_emojis().await;
    assert!(left.len() == 2 && left.iter().all(|x| x.id != e.id), "{left:?}");
    assert_eq!(lou.get_image(&e.id.to_string()).await.status(), 404);
    assert_eq!(files_in(&dir).len(), 2);
    assert_eq!(lou.call(Method::DELETE, &format!("/guilds/{gid}/emojis/{}", e.id), None).await.0, 404);
    assert_eq!(
        lou.call(Method::PATCH, &format!("/guilds/{gid}/emojis/{}", e.id), Some(json!({ "name": "autre" }))).await.0,
        404
    );
    let trace: Vec<String> = {
        let mut gw = lou.gateway().await;
        gw.ready.guilds.remove(0).emojis.into_iter().map(|e| e.name).collect()
    };
    assert_eq!(trace, ["photo", "fixe"]);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn managing_emojis_needs_the_permission_and_membership(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    let g = lou.guild("Abysse").await;
    let gid = g.guild.id;
    sam.join(&g, &lou).await;
    let (_, v) = lou.upload(gid, "poulpe", picture(32, 32, ImageFormat::Png)).await;
    let eid = v["id"].as_str().unwrap().to_string();

    // Membre sans le droit : peut lire, rien de plus. Étranger : le serveur n'existe pas pour lui.
    let png = picture(32, 32, ImageFormat::Png);
    assert_eq!(sam.upload(gid, "autre", png.clone()).await.0, 403);
    assert_eq!(
        sam.call(Method::PATCH, &format!("/guilds/{gid}/emojis/{eid}"), Some(json!({ "name": "autre" }))).await.0,
        403
    );
    assert_eq!(sam.call(Method::DELETE, &format!("/guilds/{gid}/emojis/{eid}"), None).await.0, 403);
    assert_eq!(sam.call(Method::GET, &format!("/guilds/{gid}/emojis"), None).await.0, 200);
    assert_eq!(noa.upload(gid, "autre", png.clone()).await.0, 404);
    assert_eq!(
        noa.call(Method::PATCH, &format!("/guilds/{gid}/emojis/{eid}"), Some(json!({ "name": "autre" }))).await.0,
        404
    );
    assert_eq!(noa.call(Method::DELETE, &format!("/guilds/{gid}/emojis/{eid}"), None).await.0, 404);
    assert_eq!(files_in(&dir).len(), 1, "un refus n'écrit rien");

    // « Gérer les émojis » suffit, sans rien d'autre ; pas « Gérer le serveur ».
    lou.role(gid, &sam, perms::MANAGE_GUILD | perms::MANAGE_CHANNELS).await;
    assert_eq!(sam.upload(gid, "autre", png.clone()).await.0, 403);
    lou.role(gid, &sam, perms::MANAGE_EMOJIS).await;
    let (s, v) = sam.upload(gid, "autre", png).await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["creator"]["name"], "sam");
    assert_eq!(
        sam.call(Method::PATCH, &format!("/guilds/{gid}/emojis/{eid}"), Some(json!({ "name": "renomme" }))).await.0,
        200
    );
    assert_eq!(sam.call(Method::DELETE, &format!("/guilds/{gid}/emojis/{eid}"), None).await.0, 204);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn names_formats_and_sizes_are_refused_with_a_clear_code(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let gid = lou.guild("Abysse").await.guild.id;
    let png = picture(40, 40, ImageFormat::Png);

    // Nom : 2 à 32 caractères parmi lettres, chiffres et _.
    for bad in ["", "a", "a-b", "%C3%A9t%C3%A9", "a%20b", &"x".repeat(33), "pieuvre!"] {
        let (s, v) = lou.upload(gid, bad, png.clone()).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "invalid_name"), "{bad}");
    }
    let (s, v) = lou.call(Method::POST, &format!("/guilds/{gid}/emojis"), None).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invalid_name"), "sans nom");
    // Pris, quelle que soit la casse.
    assert_eq!(lou.upload(gid, "Poulpe", png.clone()).await.0, 201);
    for taken in ["Poulpe", "poulpe", "POULPE"] {
        let (s, v) = lou.upload(gid, taken, png.clone()).await;
        assert_eq!((s.as_u16(), code(&v)), (409, "name_taken"), "{taken}");
    }
    let (_, other) = lou.upload(gid, "autre", png.clone()).await;
    let (s, v) = lou
        .call(
            Method::PATCH,
            &format!("/guilds/{gid}/emojis/{}", other["id"].as_str().unwrap()),
            Some(json!({ "name": "pOULPE" })),
        )
        .await;
    assert_eq!((s.as_u16(), code(&v)), (409, "name_taken"));
    let (s, v) = lou
        .call(
            Method::PATCH,
            &format!("/guilds/{gid}/emojis/{}", other["id"].as_str().unwrap()),
            Some(json!({ "name": "é" })),
        )
        .await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invalid_name"));

    // Formats : reconnus à leurs premiers octets. Texte, SVG, BMP, vide et fichier tronqué sont refusés.
    let mut bmp = b"BM".to_vec();
    bmp.resize(80, 0);
    let mut cut = png.clone();
    cut.truncate(40);
    for (what, bytes) in [
        ("svg", b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
        ("texte", b"pas une image".to_vec()),
        ("bmp", bmp),
        ("vide", vec![]),
        ("tronqué", cut),
    ] {
        let (s, v) = lou.upload(gid, "refuse", bytes).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "not_image"), "{what}");
    }

    // 256 Kio au plus : 413 explicite, sans lire le reste, et sans rien écrire.
    let before = files_in(&dir).len();
    let (s, v) = lou.upload(gid, "gros", vec![0u8; MAX_EMOJI_BYTES + 1]).await;
    assert_eq!((s.as_u16(), code(&v)), (413, "too_large"));
    assert!(v["message"].as_str().unwrap().contains("256 Ko"), "{v}");
    let mut fat = gif(16, 16, 2);
    fat.resize(MAX_EMOJI_BYTES + 1, 0);
    assert_eq!(lou.upload(gid, "gros", fat).await.0, 413);
    assert_eq!(files_in(&dir).len(), before);
    // Un GIF animé qui tient dans 256 Kio passe.
    let (s, v) = lou.upload(gid, "danse", gif(32, 32, 3)).await;
    assert_eq!(s, 201, "{v}");

    // Dimensions : même garde que les photos, jugée sur l'en-tête avant tout décodage.
    for (w, h) in [(4097, 2000), (6000, 6000), (4096, 4096)] {
        let (s, v) = lou.upload(gid, "enorme", common::flat(w, h, ImageFormat::Png)).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "image_too_big"), "{w} × {h}");
    }
    assert_eq!(lou.upload(gid, "limite", common::flat(4000, 4000, ImageFormat::Png)).await.0, 201);
    // Forme, jugée sur l'en-tête : 8 pixels au moins pour le petit côté, 8 pour 1 au plus. Une bande de
    // 4000 × 1 donnerait sinon un emoji d'un pixel. Même règle pour les animations.
    let before = files_in(&dir).len();
    for (what, bytes) in [
        ("bande fixe", common::flat(4000, 1, ImageFormat::Png)),
        ("bande JPEG", picture(900, 7, ImageFormat::Jpeg)),
        ("pixel", picture(1, 1, ImageFormat::Png)),
        ("petite", picture(7, 40, ImageFormat::Png)),
        ("petite animation", gif(7, 7, 2)),
        ("animation fine", flat_gif(600, 4, 2)),
    ] {
        let (s, v) = lou.upload(gid, "mince", bytes).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "image_too_small"), "{what}");
        assert!(v["message"].as_str().unwrap().contains("8 pixels"), "{what} : {v}");
    }
    for (what, bytes) in [
        ("fixe 9 pour 1", picture(72, 8, ImageFormat::Png)),
        ("fixe haute", common::flat(8, 100, ImageFormat::Png)),
        ("animation 9 pour 1", gif(72, 8, 2)),
    ] {
        let (s, v) = lou.upload(gid, "mince", bytes).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "bad_ratio"), "{what}");
        assert!(v["message"].as_str().unwrap().contains("8 fois"), "{what} : {v}");
    }
    assert_eq!(files_in(&dir).len(), before, "un refus n'écrit rien");
    // Les limites passent : 8 pixels, 8 pour 1 exactement (une fixe est recadrée en carré de 8 × 8).
    let (s, v) = lou.upload(gid, "bande", picture(64, 8, ImageFormat::Png)).await;
    assert_eq!(s, 201, "8 pour 1 exactement : {v}");
    let (_, list) = lou.call(Method::GET, &format!("/guilds/{gid}/emojis"), None).await;
    let band = list.as_array().unwrap().iter().find(|e| e["name"] == "bande").unwrap().clone();
    assert_eq!(
        lou.call(Method::DELETE, &format!("/guilds/{gid}/emojis/{}", band["id"].as_str().unwrap()), None).await.0,
        204
    );
    // Animation : 2 pour 1 au plus, et la garde des animations de profil.
    let (s, v) = lou.upload(gid, "longue", gif(40, 10, 2)).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "bad_ratio"));
    let (s, v) = lou.upload(gid, "haute", animated_webp(10, 40, 2)).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "bad_ratio"));
    let (s, v) = lou.upload(gid, "demi", gif(40, 20, 2)).await;
    assert_eq!(s, 201, "2 pour 1 exactement : {v}");
    let (s, v) = lou.upload(gid, "geante", flat_gif(MAX_ANIMATED_SIDE + 1, MAX_ANIMATED_SIDE + 1, 2)).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "animation_too_big"));
    // Aucun refus n'a laissé de fichier ni de ligne.
    let (_, list) = lou.call(Method::GET, &format!("/guilds/{gid}/emojis"), None).await;
    let names: Vec<_> = list.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["Poulpe", "autre", "danse", "limite", "demi"]);
    assert_eq!(files_in(&dir).len(), names.len());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn animations_stay_animated_and_stills_are_reencoded(pool: PgPool) {
    let (base, _dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let gid = lou.guild("Abysse").await.guild.id;

    let (s, v) = lou.upload(gid, "danse", gif(48, 48, 3)).await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["animated"], true);
    let r = lou.get_image(v["id"].as_str().unwrap()).await;
    assert_eq!(r.headers()["content-type"], "image/gif");
    let bytes = r.bytes().await.unwrap();
    let frames = GifDecoder::new(Cursor::new(bytes.to_vec())).unwrap().into_frames().collect_frames().unwrap();
    assert_eq!(frames.len(), 3, "l'animation est gardée telle quelle, sans recadrage");

    let (s, v) = lou.upload(gid, "vague", animated_webp(32, 32, 2)).await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["animated"], true);
    assert_eq!(lou.get_image(v["id"].as_str().unwrap()).await.headers()["content-type"], "image/webp");

    // WebP fixe : un PNG, et une grande image devient 128 × 128.
    let mut still = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut still)
        .encode(
            RgbaImage::from_pixel(300, 200, Rgba([9, 8, 7, 255])).as_raw(),
            300,
            200,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let (s, v) = lou.upload(gid, "statique", still).await;
    assert_eq!(s, 201, "{v}");
    assert_eq!(v["animated"], false);
    let r = lou.get_image(v["id"].as_str().unwrap()).await;
    assert_eq!(r.headers()["content-type"], "image/png");
    let img = image::load_from_memory(&r.bytes().await.unwrap()).unwrap();
    assert_eq!((img.width(), img.height()), (EMOJI_SIZE, EMOJI_SIZE));
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_server_holds_fifty_emojis(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.guild("Abysse").await;
    let gid = g.guild.id;
    let png = picture(16, 16, ImageFormat::Png);
    for i in 0..MAX_EMOJIS {
        let (s, v) = lou.upload(gid, &format!("e_{i:02}"), png.clone()).await;
        assert_eq!(s, 201, "{i} : {v}");
    }
    let (s, v) = lou.upload(gid, "de_trop", png.clone()).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "emoji_limit"));
    assert_eq!(files_in(&dir).len(), MAX_EMOJIS);
    // Une place se libère en retirant un emoji ; le décompte est par serveur.
    let (_, list) = lou.call(Method::GET, &format!("/guilds/{gid}/emojis"), None).await;
    let first = list[0]["id"].as_str().unwrap();
    assert_eq!(lou.call(Method::DELETE, &format!("/guilds/{gid}/emojis/{first}"), None).await.0, 204);
    assert_eq!(lou.upload(gid, "de_trop", png.clone()).await.0, 201);
    let other = lou.guild("Récif").await;
    assert_eq!(lou.upload(other.guild.id, "de_trop", png).await.0, 201);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn eight_big_uploads_never_decode_more_than_two_at_once(pool: PgPool) {
    let (base, _dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let gid = lou.guild("Abysse").await.guild.id;
    let big = common::flat(3000, 3000, ImageFormat::Png);
    assert!(big.len() < MAX_EMOJI_BYTES, "{} octets", big.len());
    let uploads = (0..8).map(|i| {
        let (lou, big, name) = (&lou, big.clone(), format!("grand_{i}"));
        async move { lou.upload(gid, &name, big).await }
    });
    for (s, v) in futures_util::future::join_all(uploads).await {
        assert_eq!(s, 201, "{v}");
    }
    let peak = kraken_server::peak_decodes();
    assert!((1..=2).contains(&peak), "{peak} décodages en même temps");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn reactions_take_unicode_or_an_emoji_of_a_server_the_reactor_is_in(pool: PgPool) {
    let (base, _dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    let a = lou.guild("Abysse").await;
    let b = noa.guild("Récif").await;
    sam.join(&a, &lou).await;
    sam.join(&b, &noa).await;
    let (_, ea) = lou.upload(a.guild.id, "poulpe", picture(32, 32, ImageFormat::Png)).await;
    let (_, eb) = noa.upload(b.guild.id, "corail", picture(32, 32, ImageFormat::Png)).await;
    let (ea, eb) = (ea["id"].as_str().unwrap().to_string(), eb["id"].as_str().unwrap().to_string());
    let cid = general(&a);
    let (_, m) = lou
        .call(Method::POST, &format!("/channels/{cid}/messages"), Some(json!({ "content": format!("<:poulpe:{ea}>") })))
        .await;
    let mid = m["id"].as_str().unwrap().to_string();
    let mut gw = lou.gateway().await;

    // Un emoji de son serveur, un emoji d'un autre de ses serveurs : permis, l'événement porte l'id.
    assert_eq!(put_reaction(&sam, cid, &mid, &ea).await.0, 204);
    assert_eq!(
        gw.next().await,
        Event::ReactionAdd {
            channel_id: cid,
            message_id: mid.parse().unwrap(),
            user_id: sam.user.id,
            emoji: ea.clone()
        }
    );
    assert_eq!(put_reaction(&sam, cid, &mid, &eb).await.0, 204, "emoji de Récif, dont sam est membre");
    assert!(matches!(gw.next().await, Event::ReactionAdd { emoji, .. } if emoji == eb));
    assert_eq!(put_reaction(&sam, cid, &mid, "👍").await.0, 204);

    // Lou n'est pas dans Récif : 400 `emoji`. Un id inconnu ou mal écrit aussi.
    for e in [eb.clone(), Id::nil().to_string(), ea.to_uppercase(), ea.replace('-', ""), "ok".into()] {
        let (s, v) = put_reaction(&lou, cid, &mid, &e).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "emoji"), "{e}");
    }
    // Un étranger au salon n'y réagit pas, quel que soit l'emoji.
    assert_eq!(put_reaction(&noa, cid, &mid, &ea).await.0, 404);

    // Les messages rendent la réaction avec l'id.
    let (_, msgs) = lou.call(Method::GET, &format!("/channels/{cid}/messages"), None).await;
    let emojis: Vec<_> =
        msgs[0]["reactions"].as_array().unwrap().iter().map(|r| r["emoji"].as_str().unwrap().to_string()).collect();
    assert_eq!(emojis, [ea.clone(), eb.clone(), "👍".to_string()]);

    // L'emoji supprimé : les réactions restent, mais plus personne n'en ajoute.
    assert_eq!(lou.call(Method::DELETE, &format!("/guilds/{}/emojis/{ea}", a.guild.id), None).await.0, 204);
    let (_, msgs) = lou.call(Method::GET, &format!("/channels/{cid}/messages"), None).await;
    assert!(msgs[0]["reactions"].as_array().unwrap().iter().any(|r| r["emoji"] == ea.as_str()));
    assert_eq!(put_reaction(&lou, cid, &mid, &ea).await.0, 400);
    let path = format!("/channels/{cid}/messages/{mid}/reactions/{ea}/@me");
    assert_eq!(sam.call(Method::DELETE, &path, None).await.0, 204, "on peut retirer la sienne");
}

/// Envoie un message (en réponse à `reply_to`, s'il y en a un) et rend le message créé.
async fn send(who: &Who, cid: Id, content: String, reply_to: Option<&str>) -> Value {
    let body = Some(json!({ "content": content, "reply_to": reply_to }));
    let (s, m) = who.call(Method::POST, &format!("/channels/{cid}/messages"), body).await;
    assert_eq!(s, 201, "{m}");
    m
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_reply_excerpt_never_shows_half_an_emoji_token(pool: PgPool) {
    let (base, _dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.guild("Abysse").await;
    let cid = general(&g);
    let (_, e) = lou.upload(g.guild.id, "pieuvre", picture(32, 32, ImageFormat::Png)).await;
    let tok = format!("<:pieuvre:{}>", e["id"].as_str().unwrap());
    let excerpt = |reply: Value| reply["reply"]["content"].as_str().unwrap().to_string();

    // Le jeton commence avant la 200e lettre et finit après : il est retiré en entier.
    let cut = send(&lou, cid, format!("{} {tok} suite", "a".repeat(190)), None).await;
    let r = send(&lou, cid, "ok".into(), Some(cut["id"].as_str().unwrap())).await;
    assert_eq!(excerpt(r), "a".repeat(190));
    // Un jeton qui tient dans les 200 caractères reste entier.
    let whole = send(&lou, cid, format!("{} {tok} suite", "a".repeat(100)), None).await;
    let r = send(&lou, cid, "ok".into(), Some(whole["id"].as_str().unwrap())).await;
    assert_eq!(excerpt(r), format!("{} {tok} suite", "a".repeat(100)));
    // Tout autre texte long est coupé à 200 caractères, sans plus.
    let long = send(&lou, cid, "é".repeat(300), None).await;
    let r = send(&lou, cid, "ok".into(), Some(long["id"].as_str().unwrap())).await;
    assert_eq!(excerpt(r), "é".repeat(REPLY_EXCERPT));
    // Même chose quand on relit le salon.
    let (_, msgs) = lou.call(Method::GET, &format!("/channels/{cid}/messages"), None).await;
    let first = msgs.as_array().unwrap().iter().find(|m| m["reply"]["id"] == cut["id"]).unwrap();
    assert_eq!(first["reply"]["content"], "a".repeat(190).as_str());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn deleting_the_server_removes_the_emoji_files_and_images(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.guild("Abysse").await;
    let gid = g.guild.id;
    let keep = lou.guild("Récif").await;
    let (_, a) = lou.upload(gid, "poulpe", picture(32, 32, ImageFormat::Png)).await;
    let (_, b) = lou.upload(gid, "danse", gif(32, 32, 2)).await;
    let (_, c) = lou.upload(keep.guild.id, "reste", picture(32, 32, ImageFormat::Png)).await;
    assert_eq!(files_in(&dir).len(), 3);

    let (s, _) = lou.call(Method::DELETE, &format!("/guilds/{gid}"), Some(json!({ "name": "Abysse" }))).await;
    assert_eq!(s, 204);
    assert_eq!(files_in(&dir).len(), 1, "seul l'emoji de l'autre serveur reste sur le disque");
    for e in [&a, &b] {
        assert_eq!(lou.get_image(e["id"].as_str().unwrap()).await.status(), 404);
    }
    assert_eq!(lou.get_image(c["id"].as_str().unwrap()).await.status(), 200);
    let (_, mine) = lou.call(Method::GET, "/users/@me/emojis", None).await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
}
