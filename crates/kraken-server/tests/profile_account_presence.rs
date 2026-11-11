//! Profil (bio, pronoms, bannière, animations), compte (pseudo, e-mail, mot de passe) et présence
//! (statut choisi, inactif, invisible), sur un vrai port et une vraie base.

use std::io::Cursor;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::{AnimationDecoder, Delay, Frame, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
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

async fn spawn_with(pool: PgPool, tweak: impl FnOnce(&mut Config)) -> (String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kraken-pap-{}", rand::random::<u64>()));
    let mut cfg = config(&dir);
    tweak(&mut cfg);
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), cfg).await.unwrap();
    let app = kraken_server::app(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("{addr}"), dir)
}

async fn spawn(pool: PgPool) -> (String, std::path::PathBuf) {
    spawn_with(pool, |_| {}).await
}

const PASSWORD: &str = "motdepasse";

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
            .json(&json!({"username": name, "email": format!("{name}@example.org"), "password": PASSWORD}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 201);
        let a: AuthResp = r.json().await.unwrap();
        Self { c, base: base.into(), token: a.token, user: a.user }
    }

    /// Une seconde session du même compte (un autre appareil).
    async fn login_again(&self, login: &str, password: &str) -> (StatusCode, Value) {
        let r = Client::new()
            .post(format!("http://{}/api/v1/auth/login", self.base))
            .json(&json!({"login": login, "password": password}))
            .send()
            .await
            .unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call_as(&self.token, method, path, body).await
    }

    async fn call_as(&self, token: &str, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut rb = self
            .c
            .request(method, format!("http://{}/api/v1{path}", self.base))
            .header("authorization", format!("Session {token}"));
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let r = rb.send().await.unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn put_image(&self, which: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
        let r = self
            .c
            .put(format!("http://{}/api/v1/users/@me/{which}", self.base))
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

    /// Connecté à l'ancienne : ne comprend pas les statuts (n'envoie jamais `Activity`).
    async fn gateway(&self) -> Gateway {
        self.gateway_with(&self.token).await.0
    }

    async fn gateway_with(&self, token: &str) -> (Gateway, Ready) {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/gateway", self.base)).await.unwrap();
        Gateway::read(&mut ws).await;
        let op = ClientOp::Identify { token: Some(token.to_string()) };
        ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
        let ServerOp::Ready(ready) = Gateway::read(&mut ws).await else { panic!("READY attendu") };
        (Gateway(ws), *ready)
    }

    /// Connecté et déclaré au courant des statuts : reçoit ce qu'il voit des autres (et de
    /// lui-même, qu'on met de côté), puis son propre choix.
    async fn aware_gateway(&self) -> (Gateway, Vec<PresenceEntry>, Presence) {
        let mut gw = self.gateway().await;
        gw.send(&ClientOp::Activity { idle: false }).await;
        let mut seen = Vec::new();
        loop {
            match gw.next().await {
                Event::PresenceUpdate { presences } => {
                    seen.extend(presences.into_iter().filter(|p| p.user_id != self.user.id))
                }
                Event::MyPresenceUpdate { presence } => return (gw, seen, presence),
                other => panic!("statuts attendus : {other:?}"),
            }
        }
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct Gateway(Socket);

impl Gateway {
    async fn read(ws: &mut Socket) -> ServerOp {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("événement attendu");
            if let Ws::Text(t) = m.unwrap().unwrap() {
                return serde_json::from_str(&t).unwrap();
            }
        }
    }

    async fn send(&mut self, op: &ClientOp) {
        self.0.send(Ws::Text(serde_json::to_string(op).unwrap().into())).await.unwrap();
    }

    async fn next(&mut self) -> Event {
        match Self::read(&mut self.0).await {
            ServerOp::Dispatch { event, .. } => event,
            other => panic!("DISPATCH attendu : {other:?}"),
        }
    }

    /// Le prochain statut annoncé pour cette personne. Les autres événements, et les statuts des
    /// autres comptes (dont le sien, que ses autres appareils reçoivent aussi), passent sans compter.
    async fn status_of(&mut self, who: &Who) -> Status {
        loop {
            if let Event::PresenceUpdate { presences } = self.next().await
                && let Some(p) = presences.iter().find(|p| p.user_id == who.user.id)
            {
                return p.status;
            }
        }
    }

    /// Aucun statut annoncé pour cette personne pendant un instant.
    async fn no_news_of(&mut self, who: &Who) {
        let end = tokio::time::Instant::now() + Duration::from_millis(500);
        while let Ok(m) = tokio::time::timeout_at(end, self.0.next()).await {
            let Some(Ok(Ws::Text(t))) = m else { continue };
            if let Ok(ServerOp::Dispatch { event: Event::PresenceUpdate { presences }, .. }) = serde_json::from_str(&t)
            {
                assert!(presences.iter().all(|p| p.user_id != who.user.id), "statut inattendu : {presences:?}");
            }
        }
    }

    /// Plus aucun statut du tout.
    async fn no_presence(&mut self) {
        let end = tokio::time::Instant::now() + Duration::from_millis(500);
        while let Ok(m) = tokio::time::timeout_at(end, self.0.next()).await {
            let Some(Ok(Ws::Text(t))) = m else { continue };
            if let Ok(ServerOp::Dispatch {
                event: e @ (Event::PresenceUpdate { .. } | Event::MyPresenceUpdate { .. }),
                ..
            }) = serde_json::from_str(&t)
            {
                panic!("statut inattendu : {e:?}");
            }
        }
    }

    /// La connexion est coupée par le serveur.
    async fn closed(&mut self) -> bool {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match self.0.next().await {
                    None | Some(Err(_)) | Some(Ok(Ws::Close(_))) => return true,
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
        .unwrap_or(false)
    }
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
        // Le fichier fixe est « RIFF taille WEBP » puis le chunk d'image : on garde ce chunk.
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

async fn fetch(base: &str, kind: &str, key: &str) -> reqwest::Response {
    Client::new().get(format!("http://{base}/api/v1/{kind}/{key}")).send().await.unwrap()
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir.join("avatars")) else { return vec![] };
    let mut names: Vec<String> = rd.map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    names.sort();
    names
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn bio_and_pronouns_are_optional_validated_and_announced(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    lou.befriend(&sam).await;
    let mut sam_gw = sam.gateway().await;

    // Un compte neuf n'a ni bio ni pronoms, mais une date de création.
    let (_, me) = lou.call(Method::GET, "/users/@me", None).await;
    assert!(me["bio"].is_null() && me["pronouns"].is_null() && me["banner"].is_null());
    assert!(me["created_at"].as_i64().is_some_and(|t| t > 0));

    let body = json!({"bio": "  Plongeuse de nuit.\nSans lampe.  ", "pronouns": " elle/elles "});
    let (s, v) = lou.call(Method::PATCH, "/users/@me", Some(body)).await;
    assert_eq!(s, 200);
    assert_eq!(
        (v["bio"].as_str(), v["pronouns"].as_str()),
        (Some("Plongeuse de nuit.\nSans lampe."), Some("elle/elles"))
    );
    assert!(
        matches!(sam_gw.next().await, Event::UserUpdate(u) if u.id == lou.user.id && u.bio.as_deref() == Some("Plongeuse de nuit.\nSans lampe."))
    );

    // Un champ absent reste tel quel (le nom affiché aussi : seul « display_name » change ici).
    let (_, v) = lou.call(Method::PATCH, "/users/@me", Some(json!({"display_name": "Lou des abysses"}))).await;
    assert_eq!(v["display_name"], "Lou des abysses");
    assert_eq!(
        (v["bio"].as_str(), v["pronouns"].as_str()),
        (Some("Plongeuse de nuit.\nSans lampe."), Some("elle/elles"))
    );
    // Un champ vide est effacé.
    let (_, v) = lou.call(Method::PATCH, "/users/@me", Some(json!({"bio": "", "display_name": ""}))).await;
    assert!(v["bio"].is_null() && v["display_name"].is_null());
    assert_eq!(v["pronouns"], "elle/elles");

    // Les bornes : 190 caractères pour la bio, 40 pour les pronoms, comptés en caractères.
    let max_bio = "é".repeat(MAX_BIO);
    let (s, v) = lou.call(Method::PATCH, "/users/@me", Some(json!({"bio": max_bio}))).await;
    assert_eq!((s.as_u16(), v["bio"].as_str().map(|b| b.chars().count())), (200, Some(MAX_BIO)));
    for refused in [
        json!({"bio": "é".repeat(MAX_BIO + 1)}),
        json!({"pronouns": "p".repeat(MAX_PRONOUNS + 1)}),
        json!({"pronouns": "elle\nelles"}),
        json!({"bio": "bip\u{7}"}),
        json!({"display_name": "Lou\nLou"}),
    ] {
        let (s, v) = lou.call(Method::PATCH, "/users/@me", Some(refused.clone())).await;
        assert_eq!((s.as_u16(), code(&v)), (400, "invalid"), "{refused}");
    }
    // Rien de ce qui a été refusé n'a été enregistré.
    let (_, me) = lou.call(Method::GET, "/users/@me", None).await;
    assert_eq!(me["bio"].as_str().map(|b| b.chars().count()), Some(MAX_BIO));
    assert_eq!(me["pronouns"], "elle/elles");

    // Sans session : refusé. Et un ancien serveur ou client sans ces champs reste lisible.
    let anon =
        Client::new().patch(format!("http://{base}/api/v1/users/@me")).json(&json!({"bio": "x"})).send().await.unwrap();
    assert_eq!(anon.status(), 401);
    let old: User =
        serde_json::from_value(json!({"id": lou.user.id, "username": "lou", "display_name": null, "avatar": null}))
            .unwrap();
    assert_eq!((old.bio, old.pronouns, old.banner, old.created_at), (None, None, None, None));
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn banner_is_validated_cropped_served_replaced_and_removed(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    lou.befriend(&sam).await;
    let mut sam_gw = sam.gateway().await;

    let anon = Client::new()
        .put(format!("http://{base}/api/v1/users/@me/banner"))
        .body(picture(64, 64, ImageFormat::Png))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);

    // Une photo carrée : recadrée au centre en 3 pour 1, rendue en 960 × 320.
    let (s, v) = lou.put_image("banner", picture(400, 400, ImageFormat::Png)).await;
    assert_eq!(s, 200);
    let key = v["banner"].as_str().expect("clé de bannière").to_string();
    assert!(v["avatar"].is_null(), "la photo n'a pas bougé");
    assert!(matches!(sam_gw.next().await, Event::UserUpdate(u) if u.banner.as_deref() == Some(key.as_str())));
    let r = fetch(&base, "banners", &key).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert!(r.headers()["cache-control"].to_str().unwrap().contains("immutable"));
    let img = image::load_from_memory(&r.bytes().await.unwrap()).unwrap();
    assert_eq!((img.width(), img.height()), (BANNER_WIDTH, BANNER_HEIGHT));
    assert_eq!(fetch(&base, "banners", "../../etc/passwd").await.status(), 404);
    assert_eq!(fetch(&base, "banners", &"0".repeat(32)).await.status(), 404);

    // Un JPEG remplace le PNG ; une photo et une bannière vivent côte à côte, l'ancien fichier part.
    let (_, v) = lou.put_image("avatar", picture(300, 300, ImageFormat::Png)).await;
    let avatar = v["avatar"].as_str().unwrap().to_string();
    let (s, v) = lou.put_image("banner", picture(900, 200, ImageFormat::Jpeg)).await;
    assert_eq!(s, 200);
    let second = v["banner"].as_str().unwrap().to_string();
    assert_ne!(second, key);
    assert_eq!(fetch(&base, "banners", &key).await.status(), 404);
    assert_eq!(fetch(&base, "banners", &second).await.status(), 200);
    assert_eq!(files_in(&dir).len(), 2, "une photo et une bannière");

    // Pas une image, trop lourde : refusé, rien n'est enregistré.
    let (s, v) = lou.put_image("banner", b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "not_image"));
    let (s, v) = lou.put_image("banner", vec![0u8; MAX_BANNER_BYTES + 1]).await;
    assert_eq!((s.as_u16(), code(&v)), (413, "too_large"));
    let (_, me) = lou.call(Method::GET, "/users/@me", None).await;
    assert_eq!(me["banner"], second.as_str());

    // Retirer : fichier supprimé, les autres prévenus, la photo reste.
    let (s, v) = lou.call(Method::DELETE, "/users/@me/banner", None).await;
    assert_eq!((s.as_u16(), v["banner"].is_null(), v["avatar"].as_str()), (200, true, Some(avatar.as_str())));
    while !matches!(sam_gw.next().await, Event::UserUpdate(u) if u.banner.is_none()) {}
    assert_eq!(fetch(&base, "banners", &second).await.status(), 404);
    assert_eq!(files_in(&dir).len(), 1);
    assert_eq!(lou.call(Method::DELETE, "/users/@me/banner", None).await.0, 200, "idempotent");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn animated_gif_and_webp_stay_animated_for_avatar_and_banner(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;

    // GIF animé en photo : gardé en GIF, toujours animé (3 images), non recadré.
    let (s, v) = lou.put_image("avatar", gif(60, 40, 3)).await;
    assert_eq!(s, 200, "{v}");
    let key = v["avatar"].as_str().unwrap().to_string();
    let r = fetch(&base, "avatars", &key).await;
    assert_eq!((r.status().as_u16(), r.headers()["content-type"].to_str().unwrap()), (200, "image/gif"));
    let bytes = r.bytes().await.unwrap();
    let frames = GifDecoder::new(Cursor::new(bytes.to_vec())).unwrap().into_frames().collect_frames().unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!((frames[0].buffer().width(), frames[0].buffer().height()), (60, 40));

    // WebP animé en bannière.
    let (s, v) = lou.put_image("banner", animated_webp(48, 16, 2)).await;
    assert_eq!(s, 200, "{v}");
    let banner = v["banner"].as_str().unwrap().to_string();
    let r = fetch(&base, "banners", &banner).await;
    assert_eq!((r.status().as_u16(), r.headers()["content-type"].to_str().unwrap()), (200, "image/webp"));
    let bytes = r.bytes().await.unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert!(bytes.windows(4).any(|w| w == b"ANMF"), "les images de l'animation sont gardées");

    // Un GIF d'une seule image n'est pas une animation : il est recadré et passe en PNG.
    let (s, v) = lou.put_image("avatar", gif(100, 50, 1)).await;
    assert_eq!(s, 200);
    let still = v["avatar"].as_str().unwrap().to_string();
    let r = fetch(&base, "avatars", &still).await;
    assert_eq!(r.headers()["content-type"], "image/png");
    let img = image::load_from_memory(&r.bytes().await.unwrap()).unwrap();
    assert_eq!((img.width(), img.height()), (AVATAR_SIZE, AVATAR_SIZE));
    // L'ancien GIF est supprimé quelle que soit son extension.
    assert_eq!(fetch(&base, "avatars", &key).await.status(), 404);
    assert_eq!(files_in(&dir).len(), 2, "la photo fixe et la bannière animée");

    // Une animation trop grande est refusée avec son propre code ; un fichier qui n'est qu'un début de GIF aussi.
    let (s, v) = lou.put_image("banner", gif(MAX_ANIMATED_SIDE + 1, 4, 2)).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "animation_too_big"));
    let mut head = gif(60, 40, 3);
    head.truncate(24);
    let (s, v) = lou.put_image("avatar", head).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "not_image"));
    // Une animation coupée en route n'est jamais gardée telle quelle : seule sa première image, lisible, devient une photo fixe.
    let mut cut = gif(60, 40, 3);
    cut.truncate(cut.len() / 2);
    let (s, v) = lou.put_image("avatar", cut).await;
    assert_eq!(s, 200);
    let cut_key = v["avatar"].as_str().unwrap().to_string();
    assert_eq!(fetch(&base, "avatars", &cut_key).await.headers()["content-type"], "image/png");
    let (_, me) = lou.call(Method::GET, "/users/@me", None).await;
    assert_eq!((me["avatar"].as_str(), me["banner"].as_str()), (Some(cut_key.as_str()), Some(banner.as_str())));

    // Supprimer le compte efface aussi les fichiers du disque une fois le délai passé : ici, retirer suffit.
    assert_eq!(lou.call(Method::DELETE, "/users/@me/banner", None).await.0, 200);
    assert_eq!(files_in(&dir).len(), 1);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn username_change_needs_the_password_follows_the_rules_and_is_limited(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    lou.befriend(&sam).await;
    let mut sam_gw = sam.gateway().await;
    let change = |name: &'static str, password: &'static str| {
        lou.call(Method::PUT, "/users/@me/username", Some(json!({"username": name, "password": password})))
    };

    let anon = Client::new()
        .put(format!("http://{base}/api/v1/users/@me/username"))
        .json(&json!({"username": "x1", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);
    // Mauvais mot de passe : refusé, sans rien changer.
    let (s, v) = change("lou_deux", "pas-le-bon").await;
    assert_eq!((s.as_u16(), code(&v)), (403, "bad_password"));
    // Les mêmes règles qu'à l'inscription.
    for (name, expected) in [("Lou!", "invalid"), ("x", "invalid"), ("sam", "conflict"), ("lou", "unchanged")] {
        let (s, v) = change(name, PASSWORD).await;
        assert!(s.is_client_error(), "{name}");
        assert_eq!(code(&v), expected, "{name} : {v}");
    }
    assert_eq!(lou.call(Method::GET, "/users/@me", None).await.1["username"], "lou");

    // Un changement valable : mis en minuscules, @ retiré, les amis sont prévenus, l'ancien pseudo ne connecte plus.
    let (s, v) = change("@Lou_Deux", PASSWORD).await;
    assert_eq!((s.as_u16(), v["username"].as_str()), (200, Some("lou_deux")));
    assert!(matches!(sam_gw.next().await, Event::UserUpdate(u) if u.id == lou.user.id && u.username == "lou_deux"));
    assert_eq!(lou.login_again("lou", PASSWORD).await.0, 401);
    assert_eq!(lou.login_again("lou_deux", PASSWORD).await.0, 200);
    // L'ancien pseudo est libre pour un autre compte.
    let (s, _) =
        sam.call(Method::PUT, "/users/@me/username", Some(json!({"username": "lou", "password": PASSWORD}))).await;
    assert_eq!(s, 200);

    // Deux changements par heure : le troisième attend, et dit combien de temps.
    let (s, _) = change("lou_trois", PASSWORD).await;
    assert_eq!(s, 200);
    let r = lou
        .c
        .put(format!("http://{base}/api/v1/users/@me/username"))
        .header("authorization", format!("Session {}", lou.token))
        .json(&json!({"username": "lou_quatre", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    let wait: u32 = r.headers()["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((1..=USERNAME_WINDOW_S).contains(&wait));
    let v: Value = r.json().await.unwrap();
    assert_eq!(code(&v), "username_cooldown");
    assert!(v["message"].as_str().unwrap().contains("Réessaie dans"), "{v}");
    assert!(!v["message"].as_str().unwrap().contains("pas-le-bon"));
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn email_change_needs_the_password_and_waits_for_confirmation_when_mail_is_on(pool: PgPool) {
    // Sans e-mail sur l'instance : la nouvelle adresse remplace l'ancienne tout de suite.
    let (base, _) = spawn(pool.clone()).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let (s, v) = lou.call(Method::GET, "/users/@me/account", None).await;
    assert_eq!((s.as_u16(), v["email"].as_str(), v["mail"].as_bool()), (200, Some("lou@example.org"), Some(false)));
    async fn put(who: &Who, email: &str, password: &str) -> (StatusCode, Value) {
        who.call(Method::PUT, "/users/@me/email", Some(json!({"email": email, "password": password}))).await
    }
    let (s, v) = put(&lou, "lou@exemple.fr", "pas-le-bon").await;
    assert_eq!((s.as_u16(), code(&v)), (403, "bad_password"));
    for (email, expected) in [
        ("pas-une-adresse", "invalid"),
        ("a b@exemple.fr", "invalid"),
        ("sam@example.org", "conflict"),
        ("LOU@example.org", "unchanged"),
    ] {
        let (s, v) = put(&lou, email, PASSWORD).await;
        assert!(s.is_client_error(), "{email}");
        assert_eq!(code(&v), expected, "{email} : {v}");
    }
    let (s, v) = put(&lou, "lou@exemple.fr", PASSWORD).await;
    assert_eq!((s.as_u16(), v["email"].as_str(), v["pending_email"].is_null()), (200, Some("lou@exemple.fr"), true));
    assert_eq!(lou.login_again("lou@exemple.fr", PASSWORD).await.0, 200);
    assert_eq!(lou.login_again("lou@example.org", PASSWORD).await.0, 401);

    // Avec e-mail : la nouvelle adresse attend son lien, l'ancienne reste valable d'ici là.
    let (base, _) = spawn_with(pool.clone(), |c| c.smtp_url = Some("smtp://127.0.0.1:1".into())).await;
    let sam2 = Who { c: Client::new(), base: base.clone(), token: sam.token.clone(), user: sam.user.clone() };
    let (s, v) = put(&sam2, "sam@exemple.fr", PASSWORD).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        (v["email"].as_str(), v["pending_email"].as_str(), v["mail"].as_bool()),
        (Some("sam@example.org"), Some("sam@exemple.fr"), Some(true))
    );
    assert_eq!(sam2.login_again("sam@example.org", PASSWORD).await.0, 200, "l'ancienne adresse connecte encore");
    assert_eq!(sam2.login_again("sam@exemple.fr", PASSWORD).await.0, 401, "la nouvelle pas encore");

    // Le lien reçu à la nouvelle adresse la confirme (la même page que l'inscription).
    let db = kraken_db::Db::from_pool(pool);
    let raw = "jeton-de-test-pour-le-changement-d-adresse";
    let hash = Sha256::digest(raw.as_bytes()).to_vec();
    db.create_email_change_token(sam.user.id, "sam@exemple.fr", &hash, 60).await.unwrap();
    let verify = |token: &str| {
        let c = Client::new();
        let url = format!("http://{base}/api/v1/auth/verify-email");
        let token = token.to_string();
        async move { c.post(url).json(&json!({"token": token})).send().await.unwrap().status() }
    };
    assert_eq!(verify("n-importe-quoi").await, 400);
    assert_eq!(verify(raw).await, 204);
    assert_eq!(verify(raw).await, 400, "un lien ne sert qu'une fois");
    let (_, v) = sam2.call(Method::GET, "/users/@me/account", None).await;
    assert_eq!(
        (v["email"].as_str(), v["email_verified"].as_bool(), v["pending_email"].is_null()),
        (Some("sam@exemple.fr"), Some(true), true)
    );
    assert_eq!(sam2.login_again("sam@exemple.fr", PASSWORD).await.0, 200);
    assert_eq!(sam2.login_again("sam@example.org", PASSWORD).await.0, 401);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn password_change_closes_the_other_sessions_and_keeps_this_one(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let (_, other) = lou.login_again("lou", PASSWORD).await;
    let other_token = other["token"].as_str().unwrap().to_string();
    let (mut other_gw, _) = lou.gateway_with(&other_token).await;
    let put = |current: &str, new: &str| {
        lou.call(Method::PUT, "/users/@me/password", Some(json!({"current": current, "new": new})))
    };

    let anon = Client::new()
        .put(format!("http://{base}/api/v1/users/@me/password"))
        .json(&json!({"current": PASSWORD, "new": "un-autre-mot-de-passe"}))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);
    let (s, v) = put("pas-le-bon", "un-autre-mot-de-passe").await;
    assert_eq!((s.as_u16(), code(&v)), (403, "bad_password"));
    let (s, v) = put(PASSWORD, "court").await;
    assert_eq!((s.as_u16(), code(&v)), (400, "invalid"), "même minimum qu'à l'inscription");
    let (s, v) = put(PASSWORD, PASSWORD).await;
    assert_eq!((s.as_u16(), code(&v)), (400, "same_password"));
    // Rien n'a bougé : l'autre session marche encore.
    assert_eq!(lou.call_as(&other_token, Method::GET, "/users/@me", None).await.0, 200);

    let (s, _) = put(PASSWORD, "un-autre-mot-de-passe").await;
    assert_eq!(s, 204);
    // Cette session reste ouverte, l'autre est fermée et sa connexion coupée.
    assert_eq!(lou.call(Method::GET, "/users/@me", None).await.0, 200);
    assert_eq!(lou.call_as(&other_token, Method::GET, "/users/@me", None).await.0, 401);
    assert!(other_gw.closed().await, "la connexion de l'autre appareil est coupée");
    // L'ancien mot de passe ne connecte plus, le nouveau si.
    assert_eq!(lou.login_again("lou", PASSWORD).await.0, 401);
    assert_eq!(lou.login_again("lou", "un-autre-mot-de-passe").await.0, 200);
}

/// Un ami, un compte à l'ancienne, un inconnu : qui reçoit quoi.
#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn presence_is_chosen_private_when_invisible_automatic_when_idle_and_scoped(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let noa = Who::new(&base, "noa").await;
    let ilan = Who::new(&base, "ilan").await;
    lou.befriend(&sam).await;
    lou.befriend(&ilan).await;

    // Sam se déclare au courant des statuts : lou n'est pas là, personne n'est annoncé.
    let (mut sam_gw, seen, mine) = sam.aware_gateway().await;
    assert_eq!((seen, mine), (vec![], Presence::Online));
    let (mut noa_gw, _, _) = noa.aware_gateway().await;
    // Ilan, à l'ancienne, ne comprend pas les statuts.
    let mut ilan_gw = ilan.gateway().await;
    // Lou arrive : son ami sam le voit en ligne, et lui voit sam.
    let mut lou_gw = lou.gateway().await;
    assert_eq!(sam_gw.status_of(&lou).await, Status::Online);
    lou_gw.send(&ClientOp::Activity { idle: false }).await;
    assert_eq!(lou_gw.status_of(&sam).await, Status::Online);
    while !matches!(lou_gw.next().await, Event::MyPresenceUpdate { presence: Presence::Online }) {}

    // Ne pas déranger : annoncé aux amis, et à mes autres appareils.
    let (s, v) = lou.call(Method::PUT, "/users/@me/presence", Some(json!({"presence": "dnd"}))).await;
    assert_eq!((s.as_u16(), v["presence"].as_str()), (200, Some("dnd")));
    assert_eq!(sam_gw.status_of(&lou).await, Status::Dnd);
    while !matches!(lou_gw.next().await, Event::MyPresenceUpdate { presence: Presence::Dnd }) {}
    // Le même choix une deuxième fois n'annonce rien de plus aux autres.
    lou.call(Method::PUT, "/users/@me/presence", Some(json!({"presence": "dnd"}))).await;
    sam_gw.no_news_of(&lou).await;

    // Inactif automatique : un appareil qui se dit inactif passe « en ligne » à « inactif »,
    // et revient en ligne au premier geste. Un choix manuel (ici ne pas déranger) n'est jamais remplacé.
    lou.call(Method::PUT, "/users/@me/presence", Some(json!({"presence": "online"}))).await;
    assert_eq!(sam_gw.status_of(&lou).await, Status::Online);
    lou_gw.send(&ClientOp::Activity { idle: true }).await;
    assert_eq!(sam_gw.status_of(&lou).await, Status::Idle);
    lou_gw.send(&ClientOp::Activity { idle: false }).await;
    assert_eq!(sam_gw.status_of(&lou).await, Status::Online);
    lou.call(Method::PUT, "/users/@me/presence", Some(json!({"presence": "dnd"}))).await;
    assert_eq!(sam_gw.status_of(&lou).await, Status::Dnd);
    lou_gw.send(&ClientOp::Activity { idle: true }).await;
    sam_gw.no_news_of(&lou).await;
    let status_seen_by_sam = || async {
        let rels: Vec<Relationship> =
            serde_json::from_value(sam.call(Method::GET, "/users/@me/relationships", None).await.1).unwrap();
        rels.iter().find(|r| r.user.id == lou.user.id).map(|r| r.status)
    };
    assert_eq!(status_seen_by_sam().await, Some(Status::Dnd));
    lou_gw.send(&ClientOp::Activity { idle: false }).await;

    // Invisible : les autres le voient hors ligne, lui garde son choix.
    lou.call(Method::PUT, "/users/@me/presence", Some(json!({"presence": "invisible"}))).await;
    assert_eq!(sam_gw.status_of(&lou).await, Status::Offline);
    while !matches!(lou_gw.next().await, Event::MyPresenceUpdate { presence: Presence::Invisible }) {}
    assert_eq!(status_seen_by_sam().await, Some(Status::Offline));
    let rels: Vec<Relationship> =
        serde_json::from_value(ilan.call(Method::GET, "/users/@me/relationships", None).await.1).unwrap();
    assert_eq!(rels.iter().find(|r| r.user.id == lou.user.id).map(|r| r.status), Some(Status::Offline));

    // Le choix est gardé : une nouvelle connexion de lou le retrouve, et sam ne le voit toujours pas.
    drop(lou_gw);
    let (mut lou_gw, ready) = lou.gateway_with(&lou.token).await;
    assert_eq!(ready.presence, Presence::Invisible);
    sam_gw.no_news_of(&lou).await;
    lou_gw.send(&ClientOp::Activity { idle: false }).await;

    // Un choix inconnu est refusé, sans session aussi.
    let (s, _) = lou.call(Method::PUT, "/users/@me/presence", Some(json!({"presence": "absent"}))).await;
    assert!(s.is_client_error());
    let anon = Client::new()
        .put(format!("http://{base}/api/v1/users/@me/presence"))
        .json(&json!({"presence": "dnd"}))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);

    // Ni l'inconnu (aucun serveur, aucun ami en commun) ni le client à l'ancienne ne reçoivent le statut de lou.
    noa_gw.no_news_of(&lou).await;
    ilan_gw.no_presence().await;
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_block_hides_presence_both_ways_and_unblocking_restores_it(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    lou.befriend(&sam).await;
    // Un MP les relie : il reste après un blocage, et rend la vue au déblocage.
    let (s, _) = lou.call(Method::POST, "/users/@me/channels", Some(json!({"recipients": [sam.user.id]}))).await;
    assert_eq!(s, 201);
    let (mut sam_gw, _, _) = sam.aware_gateway().await;
    let (mut lou_gw, seen, _) = lou.aware_gateway().await;
    assert_eq!(seen, vec![PresenceEntry { user_id: sam.user.id, status: Status::Online }]);
    assert_eq!(sam_gw.status_of(&lou).await, Status::Online);

    let path = format!("/users/@me/relationships/{}", sam.user.id);
    assert_eq!(lou.call(Method::PUT, &path, Some(json!({"kind": "blocked"}))).await.0, 200);
    assert_eq!(sam_gw.status_of(&lou).await, Status::Offline);
    assert_eq!(lou_gw.status_of(&sam).await, Status::Offline);
    // Lou change de statut : sam, bloqué, n'en entend plus parler.
    lou.call(Method::PUT, "/users/@me/presence", Some(json!({"presence": "dnd"}))).await;
    while !matches!(lou_gw.next().await, Event::MyPresenceUpdate { presence: Presence::Dnd }) {}
    sam_gw.no_news_of(&lou).await;

    // Débloquer rend la vue, dans les deux sens.
    assert_eq!(lou.call(Method::DELETE, &path, None).await.0, 204);
    assert_eq!(sam_gw.status_of(&lou).await, Status::Dnd);
    assert_eq!(lou_gw.status_of(&sam).await, Status::Online);
}
