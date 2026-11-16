//! Modèles de serveur, nom et icône d'un serveur, de bout en bout sur un vrai port et une vraie base :
//! `POST /guilds` avec un modèle, `PATCH /guilds/{id}`, `PUT` et `DELETE /guilds/{id}/icon`, droit « Gérer le
//! serveur » (403, 404 hors du serveur, 401 sans session), mêmes refus d'image que les photos de profil, lecture
//! publique `/guild-icons/{key}` avec le cache sans fin, événement `GUILD_UPDATE` en direct, journal d'audit,
//! limite de débit, fichiers qui partent avec l'icône remplacée ou le serveur.

use std::io::Cursor;

use futures_util::{SinkExt, StreamExt};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
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

/// Serveur et dossier de données propre à ce test.
async fn spawn(pool: PgPool) -> (String, std::path::PathBuf) {
    spawn_limited(pool, 1000).await
}

/// Comme [`spawn`], avec cette limite de connexions par minute (les écritures de serveur en ont au moins 60).
async fn spawn_limited(pool: PgPool, auth_per_min: u32) -> (String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kraken-guild-edit-{}", rand::random::<u64>()));
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), config(&dir, auth_per_min)).await.unwrap();
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

    /// Envoie une icône : le corps est l'image.
    async fn icon(&self, gid: Id, bytes: Vec<u8>) -> (StatusCode, Value) {
        let r = self
            .c
            .put(format!("http://{}/api/v1/guilds/{gid}/icon", self.base))
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

    async fn guild(&self, name: &str, template: Option<&str>) -> (StatusCode, Value) {
        let body = match template {
            Some(t) => json!({ "name": name, "template": t }),
            None => json!({ "name": name }),
        };
        self.call(Method::POST, "/guilds", Some(body)).await
    }

    async fn new_guild(&self) -> GuildState {
        let (s, v) = self.guild("Abysse", None).await;
        assert_eq!(s, 201, "{v}");
        serde_json::from_value(v).unwrap()
    }

    /// Rejoint ce serveur par une invitation de `inviter`.
    async fn join(&self, g: &GuildState, inviter: &Who) {
        let cid = general(g);
        let (s, v) = inviter.call(Method::POST, &format!("/channels/{cid}/invites"), Some(json!({}))).await;
        assert_eq!(s, 201);
        let code = v["code"].as_str().unwrap();
        assert_eq!(self.call(Method::POST, &format!("/invites/{code}"), None).await.0, 200);
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

    async fn gateway(&self) -> Gateway {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/gateway", self.base)).await.unwrap();
        Gateway::read(&mut ws).await;
        let op = ClientOp::Identify { token: Some(self.token.clone()) };
        ws.send(Ws::Text(serde_json::to_string(&op).unwrap().into())).await.unwrap();
        let ServerOp::Ready(_) = Gateway::read(&mut ws).await else { panic!("READY attendu") };
        Gateway { ws }
    }

    async fn audit(&self, gid: Id) -> Vec<Value> {
        let (s, v) = self.call(Method::GET, &format!("/guilds/{gid}/audit-log?category=serveur"), None).await;
        assert_eq!(s, 200, "{v}");
        v["entries"].as_array().unwrap().clone()
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

    /// Le prochain `GUILD_UPDATE`, en laissant passer les autres événements (arrivées, etc.).
    async fn next_update(&mut self) -> Guild {
        loop {
            if let ServerOp::Dispatch { event: Event::GuildUpdate(g), .. } = Self::read(&mut self.ws).await {
                return g;
            }
        }
    }

    /// Rien d'autre qu'un `GUILD_UPDATE` ne doit venir dans la seconde qui suit.
    async fn no_update(&mut self) {
        let wait = tokio::time::timeout(std::time::Duration::from_millis(400), async {
            loop {
                if let ServerOp::Dispatch { event: Event::GuildUpdate(g), .. } = Self::read(&mut self.ws).await {
                    return g;
                }
            }
        })
        .await;
        assert!(wait.is_err(), "aucun GUILD_UPDATE attendu");
    }
}

fn general(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id
}

fn picture(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 120]));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format).unwrap();
    out
}

/// Un GIF animé de `frames` images.
fn gif(w: u32, h: u32, frames: u32) -> Vec<u8> {
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

fn files_in(dir: &std::path::Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir.join("avatars")) else { return vec![] };
    let mut names: Vec<String> = rd.map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    names.sort();
    names
}

async fn get_icon(base: &str, key: &str) -> reqwest::Response {
    // Sans session : une balise <img> n'envoie pas d'en-tête d'autorisation.
    Client::new().get(format!("http://{base}/api/v1/guild-icons/{key}")).send().await.unwrap()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_template_builds_the_server_and_an_unknown_one_is_a_french_400(pool: PgPool) {
    let (base, _) = spawn(pool.clone()).await;
    let lou = Who::new(&base, "lou").await;
    let guilds =
        || async { sqlx::query_scalar::<_, i64>("select count(*) from guilds").fetch_one(&pool).await.unwrap() };

    let (s, v) = lou.guild("Révisions", Some("etudes")).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let g: GuildState = serde_json::from_value(v).unwrap();
    let names = |kind: ChannelKind| -> Vec<&str> {
        g.channels.iter().filter(|c| c.kind == kind).map(|c| c.name.as_deref().unwrap()).collect()
    };
    assert_eq!(names(ChannelKind::Text), ["général", "cours", "devoirs", "ressources"]);
    assert_eq!(names(ChannelKind::Voice), ["Salle de travail", "Révisions en groupe", "Pause"]);
    assert_eq!(g.roles.len(), 1, "pas de rôle en plus de @everyone");

    // Sans modèle, ou avec « vide » : la structure d'avant.
    for template in [None, Some("vide")] {
        let (s, v) = lou.guild("Abysse", template).await;
        assert_eq!(s, StatusCode::CREATED);
        let g: GuildState = serde_json::from_value(v).unwrap();
        assert_eq!(g.channels.iter().filter(|c| c.kind == ChannelKind::Category).count(), 2);
        assert_eq!(g.channels.len(), 4);
    }
    assert_eq!(guilds().await, 3);

    // Un modèle inconnu : 400 en français, rien n'est créé.
    for bad in ["createurs", "", "Etudes", "etudes\u{0}"] {
        let (s, v) = lou.guild("Autre", Some(bad)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad:?}");
        assert_eq!(v["message"], "Ce modèle de serveur n'existe pas.");
    }
    // Un champ `template` qui n'est pas du texte : refusé aussi.
    let (s, _) = lou.call(Method::POST, "/guilds", Some(json!({ "name": "Autre", "template": 3 }))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(guilds().await, 3, "un refus ne crée rien");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn an_icon_is_set_seen_live_served_replaced_and_removed(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let g = lou.new_guild().await;
    let gid = g.guild.id;
    sam.join(&g, &lou).await;
    let mut sam_gw = sam.gateway().await;

    // Un PNG large est recadré au centre en carré de 256 pixels.
    let (s, v) = lou.icon(gid, picture(500, 300, ImageFormat::Png)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let guild: Guild = serde_json::from_value(v).unwrap();
    let key = guild.icon.clone().expect("la clé de l'icône");
    assert_eq!((guild.id, guild.name.as_str()), (gid, "Abysse"));
    assert_eq!(sam_gw.next_update().await, guild);
    assert_eq!(files_in(&dir), [format!("{key}.png")]);

    // Lecture publique, cache sans fin, PNG carré.
    let r = get_icon(&base, &key).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert_eq!(r.headers()["cache-control"], "public, max-age=31536000, immutable");
    let img = image::load_from_memory(&r.bytes().await.unwrap()).unwrap();
    assert_eq!((img.width(), img.height()), (GUILD_ICON_SIZE, GUILD_ICON_SIZE));
    assert_eq!(get_icon(&base, "0123456789abcdef0123456789abcdef").await.status(), 404);
    assert_eq!(get_icon(&base, "../x").await.status(), 404);

    // L'état du serveur, vu d'un membre, porte l'icône.
    let (_, v) = sam.call(Method::GET, &format!("/guilds/{gid}"), None).await;
    assert_eq!(v["guild"]["icon"], key.as_str());

    // Remplacer : une nouvelle clé, l'ancien fichier part.
    let (s, v) = lou.icon(gid, picture(64, 64, ImageFormat::Jpeg)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let second: Guild = serde_json::from_value(v).unwrap();
    let key2 = second.icon.clone().unwrap();
    assert_ne!(key, key2);
    assert_eq!(sam_gw.next_update().await.icon.as_deref(), Some(key2.as_str()));
    assert_eq!(files_in(&dir), [format!("{key2}.png")]);
    assert_eq!(get_icon(&base, &key).await.status(), 404);

    // Un GIF animé reste animé, sous son propre format.
    let (s, v) = lou.icon(gid, gif(32, 32, 3)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let key3 = v["icon"].as_str().unwrap().to_string();
    assert_eq!(sam_gw.next_update().await.icon.as_deref(), Some(key3.as_str()));
    assert_eq!(files_in(&dir), [format!("{key3}.gif")]);
    assert_eq!(get_icon(&base, &key3).await.headers()["content-type"], "image/gif");

    // Retirer : l'icône et son fichier partent, tout le monde le sait. Le deuxième retrait ne dit rien de plus.
    let (s, v) = lou.call(Method::DELETE, &format!("/guilds/{gid}/icon"), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["icon"].is_null());
    assert_eq!(sam_gw.next_update().await.icon, None);
    assert!(files_in(&dir).is_empty());
    assert_eq!(get_icon(&base, &key3).await.status(), 404);

    // Le journal : trois changements, un retrait (du plus récent au plus ancien), jamais une clé de fichier.
    let entries = lou.audit(gid).await;
    let icons: Vec<_> =
        entries.iter().map(|e| (e["action"].as_str().unwrap(), e["details"]["icon"].as_str())).collect();
    assert_eq!(
        icons,
        [
            ("guild_update", Some("removed")),
            ("guild_update", Some("changed")),
            ("guild_update", Some("changed")),
            ("guild_update", Some("changed"))
        ]
    );
    let raw = serde_json::to_string(&entries).unwrap();
    assert!(![&key, &key2, &key3].iter().any(|k| raw.contains(k.as_str())));
    assert!(entries.iter().all(|e| e["actor"]["id"] == lou.user.id.to_string().as_str()));
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn renaming_is_live_logged_and_follows_the_name_rules(pool: PgPool) {
    let (base, _) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let sam = Who::new(&base, "sam").await;
    let g = lou.new_guild().await;
    let gid = g.guild.id;
    sam.join(&g, &lou).await;
    let mut sam_gw = sam.gateway().await;

    let (s, v) = lou.call(Method::PATCH, &format!("/guilds/{gid}"), Some(json!({ "name": "  Le Récif " }))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["id"].as_str(), v["name"].as_str()), (Some(gid.to_string().as_str()), Some("Le Récif")));
    let live = sam_gw.next_update().await;
    assert_eq!((live.id, live.name.as_str(), live.icon), (gid, "Le Récif", None));
    let (_, v) = sam.call(Method::GET, &format!("/guilds/{gid}"), None).await;
    assert_eq!(v["guild"]["name"], "Le Récif");

    // Journal : un avant / après sur le nom.
    let entries = lou.audit(gid).await;
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e["action"], "guild_update");
    assert_eq!(e["changes"], json!([{ "field": "name", "before": "Abysse", "after": "Le Récif" }]));

    // Mêmes règles qu'à la création.
    for bad in ["", "A", "Abys\u{202E}se", "\u{200B}\u{200B}", &"x".repeat(101)] {
        let (s, v) = lou.call(Method::PATCH, &format!("/guilds/{gid}"), Some(json!({ "name": bad }))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad:?}");
        assert!(v["message"].as_str().is_some_and(|m| !m.is_empty()));
    }
    assert_eq!(lou.audit(gid).await.len(), 1, "un refus n'écrit rien");
    sam_gw.no_update().await;
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_server_name_with_a_control_character_gets_a_clear_french_400(pool: PgPool) {
    let (base, _) = spawn(pool.clone()).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.new_guild().await;
    let gid = g.guild.id;
    let guilds =
        || async { sqlx::query_scalar::<_, i64>("select count(*) from guilds").fetch_one(&pool).await.unwrap() };
    let before = guilds().await;
    let said =
        "Le nom du serveur contient un caractère interdit (tabulation, saut de ligne ou autre caractère de contrôle).";
    for bad in ["Le\u{0}Récif", "Le\tRécif", "Le\nRécif", "Le\u{2028}Récif", "Le\u{2029}Récif", "Récif\r"] {
        let (s, v) = lou.guild(bad, None).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "création {bad:?}");
        assert_eq!((v["code"].as_str(), v["message"].as_str()), (Some("invalid_chars"), Some(said)), "{bad:?}");
        let (s, v) = lou.call(Method::PATCH, &format!("/guilds/{gid}"), Some(json!({ "name": bad }))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "renommage {bad:?}");
        assert_eq!((v["code"].as_str(), v["message"].as_str()), (Some("invalid_chars"), Some(said)), "{bad:?}");
    }
    assert_eq!(guilds().await, before, "un refus ne crée rien");
    assert!(lou.audit(gid).await.is_empty(), "un refus n'écrit rien au journal");
    let (_, v) = lou.call(Method::GET, &format!("/guilds/{gid}"), None).await;
    assert_eq!(v["guild"]["name"], "Abysse");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn only_those_who_manage_the_server_edit_it(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let (lou, sam, ada, noa) = (
        Who::new(&base, "lou").await,
        Who::new(&base, "sam").await,
        Who::new(&base, "ada").await,
        Who::new(&base, "noa").await,
    );
    let g = lou.new_guild().await;
    let gid = g.guild.id;
    sam.join(&g, &lou).await;
    ada.join(&g, &lou).await;
    let mut lou_gw = lou.gateway().await;

    // Un membre ordinaire : 403, même avec une image invalide (le droit passe avant l'image) ; un étranger : 404.
    let png = picture(40, 40, ImageFormat::Png);
    assert_eq!(sam.icon(gid, png.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(sam.icon(gid, b"pas une image".to_vec()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(sam.call(Method::DELETE, &format!("/guilds/{gid}/icon"), None).await.0, StatusCode::FORBIDDEN);
    let path = format!("/guilds/{gid}");
    assert_eq!(sam.call(Method::PATCH, &path, Some(json!({ "name": "Pirates" }))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(noa.icon(gid, png.clone()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(noa.call(Method::DELETE, &format!("/guilds/{gid}/icon"), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(noa.call(Method::PATCH, &path, Some(json!({ "name": "Pirates" }))).await.0, StatusCode::NOT_FOUND);
    assert_eq!(lou.icon(Id::nil(), png.clone()).await.0, StatusCode::NOT_FOUND);

    // Sans session : 401.
    let anon = Client::new();
    assert_eq!(
        anon.put(format!("http://{base}/api/v1/guilds/{gid}/icon")).body(png.clone()).send().await.unwrap().status(),
        401
    );
    assert_eq!(anon.delete(format!("http://{base}/api/v1/guilds/{gid}/icon")).send().await.unwrap().status(), 401);
    assert_eq!(
        anon.patch(format!("http://{base}/api/v1/guilds/{gid}"))
            .json(&json!({ "name": "Pirates" }))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

    // Rien n'a bougé : ni nom, ni icône, ni fichier, ni événement, ni journal.
    let (_, v) = lou.call(Method::GET, &path, None).await;
    assert_eq!((v["guild"]["name"].as_str(), v["guild"]["icon"].is_null()), (Some("Abysse"), true));
    assert!(files_in(&dir).is_empty());
    assert!(lou.audit(gid).await.is_empty());
    lou_gw.no_update().await;

    // Un rôle « Gérer le serveur » suffit, sans être propriétaire ni administrateur.
    lou.role(gid, &ada, perms::MANAGE_GUILD).await;
    assert_eq!(ada.icon(gid, png).await.0, StatusCode::OK);
    assert_eq!(ada.call(Method::PATCH, &path, Some(json!({ "name": "Chez Ada" }))).await.0, StatusCode::OK);
    assert!(lou_gw.next_update().await.icon.is_some());
    assert_eq!(lou_gw.next_update().await.name, "Chez Ada");
    let entries = lou.audit(gid).await;
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| e["actor"]["id"] == ada.user.id.to_string().as_str()));
    // …et le sait à l'état suivant : sam, lui, ne peut toujours rien.
    assert_eq!(sam.call(Method::PATCH, &path, Some(json!({ "name": "Pirates" }))).await.0, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn an_invalid_icon_is_refused_like_a_profile_picture_and_nothing_is_kept(pool: PgPool) {
    let (base, dir) = spawn(pool).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.new_guild().await;
    let gid = g.guild.id;
    let mut gw = lou.gateway().await;

    let refused: Vec<(&str, Vec<u8>, StatusCode, &str)> = vec![
        ("vide", vec![], StatusCode::BAD_REQUEST, "not_image"),
        ("texte", b"<html>pas un png</html>".to_vec(), StatusCode::BAD_REQUEST, "not_image"),
        ("svg", b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(), StatusCode::BAD_REQUEST, "not_image"),
        ("png coupé", picture(40, 40, ImageFormat::Png)[..30].to_vec(), StatusCode::BAD_REQUEST, "not_image"),
        ("trop petite", picture(7, 40, ImageFormat::Png), StatusCode::BAD_REQUEST, "image_too_small"),
        ("trop petite (carré)", picture(7, 7, ImageFormat::Png), StatusCode::BAD_REQUEST, "image_too_small"),
        ("trop grande", picture(4097, 8, ImageFormat::Png), StatusCode::BAD_REQUEST, "image_too_big"),
        ("trop lourde", vec![0x89; MAX_GUILD_ICON_BYTES + 1], StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
        ("animation trop grande", gif(1100, 1100, 2), StatusCode::BAD_REQUEST, "animation_too_big"),
    ];
    for (what, bytes, status, code) in refused {
        let (s, v) = lou.icon(gid, bytes).await;
        assert_eq!((s, v["code"].as_str()), (status, Some(code)), "{what} : {v}");
        assert!(v["message"].as_str().is_some_and(|m| m.chars().any(|c| c.is_alphabetic())), "{what}");
    }
    assert!(files_in(&dir).is_empty(), "un refus n'écrit aucun fichier");
    assert!(lou.audit(gid).await.is_empty());
    gw.no_update().await;
    // 8 pixels de côté : la limite passe.
    assert_eq!(lou.icon(gid, picture(8, 8, ImageFormat::Png)).await.0, StatusCode::OK);
    assert_eq!(files_in(&dir).len(), 1);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn the_write_routes_share_one_limiter_and_the_icon_goes_with_the_server(pool: PgPool) {
    let (base, dir) = spawn_limited(pool, 5).await;
    let lou = Who::new(&base, "lou").await;
    let g = lou.new_guild().await;
    let gid = g.guild.id;

    // Le fichier part avec le serveur supprimé.
    let (s, v) = lou.icon(gid, picture(40, 40, ImageFormat::Png)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let key = v["icon"].as_str().unwrap().to_string();
    assert_eq!(files_in(&dir).len(), 1);
    let (s, _) = lou.call(Method::DELETE, &format!("/guilds/{gid}"), Some(json!({ "name": "Abysse" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(files_in(&dir).is_empty());
    assert_eq!(get_icon(&base, &key).await.status(), 404);

    // 60 écritures par minute et par client, un seul compteur pour le nom et l'icône.
    let g = lou.new_guild().await;
    let path = format!("/guilds/{}", g.guild.id);
    let mut statuses = Vec::new();
    for _ in 0..70 {
        statuses.push(lou.call(Method::PATCH, &path, Some(json!({ "name": "Abysse" }))).await.0);
    }
    assert!(statuses[..55].iter().all(|s| *s == StatusCode::OK), "{statuses:?}");
    assert!(statuses.contains(&StatusCode::TOO_MANY_REQUESTS), "{statuses:?}");
    assert_eq!(lou.icon(g.guild.id, picture(40, 40, ImageFormat::Png)).await.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(lou.call(Method::DELETE, &format!("{path}/icon"), None).await.0, StatusCode::TOO_MANY_REQUESTS);
    // La lecture, elle, n'est pas comptée.
    assert_eq!(lou.call(Method::GET, &path, None).await.0, StatusCode::OK);
}
