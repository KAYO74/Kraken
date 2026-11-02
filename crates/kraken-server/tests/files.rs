//! Fichiers joints et aperçus de liens, sur un vrai port et une vraie base : limites d'envoi,
//! droits de lecture, en-têtes de service, suppression des fichiers, aperçus d'une page servie
//! par le test lui-même.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use image::{ImageFormat, Rgb, RgbImage};
use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message as Ws;

const MAX_UPLOAD: u64 = 100 * 1024;

fn config(data_dir: &Path, previews: bool, allow_private: bool) -> Config {
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
        auth_per_min: 10_000,
        instance: Default::default(),
        rtc_port: 0,
        rtc_fallback: false,
        rtc_ips: vec![std::net::Ipv4Addr::LOCALHOST.into()],
        rtc_public_port: None,
        ring_secs: 30,
        max_upload_bytes: MAX_UPLOAD,
        link_previews: previews,
        preview_allow_private: allow_private,
    }
}

async fn spawn(pool: PgPool, previews: bool, allow_private: bool) -> (String, PathBuf) {
    let dir = std::env::temp_dir().join(format!("kraken-files-{}", rand::random::<u64>()));
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), config(&dir, previews, allow_private)).await.unwrap();
    kraken_server::spawn_workers(&state);
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
        Self { c, base: base.into(), token: a.token }
    }

    fn req(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.c
            .request(method, format!("http://{}/api/v1{path}", self.base))
            .header("authorization", format!("Session {}", self.token))
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut rb = self.req(method, path);
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let r = rb.send().await.unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn upload(&self, channel: Id, name: &str, mime: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
        let r = self
            .req(Method::POST, &format!("/channels/{channel}/attachments?filename={}", name.replace(' ', "%20")))
            .header("content-type", mime)
            .body(bytes)
            .send()
            .await
            .unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn send(&self, channel: Id, content: &str, files: &[Value]) -> (StatusCode, Value) {
        let body = json!({"content": content, "attachments": files.iter().map(|f| json!({"id": f["id"]})).collect::<Vec<_>>()});
        self.call(Method::POST, &format!("/channels/{channel}/messages"), Some(body)).await
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.req(Method::GET, path).send().await.unwrap()
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
            let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("événement attendu");
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

    /// Le prochain MESSAGE_UPDATE.
    async fn update(&mut self) -> Message {
        loop {
            if let Event::MessageUpdate(m) = self.next().await {
                return *m;
            }
        }
    }

    /// Aucun MESSAGE_UPDATE pendant `ms` millisecondes.
    async fn quiet(&mut self, ms: u64) {
        let end = tokio::time::Instant::now() + Duration::from_millis(ms);
        while let Ok(Some(m)) = tokio::time::timeout_at(end, self.0.next()).await {
            if let Ok(Ws::Text(t)) = m
                && let Ok(ServerOp::Dispatch { event: Event::MessageUpdate(_), .. }) =
                    serde_json::from_str::<ServerOp>(&t)
            {
                panic!("MESSAGE_UPDATE inattendu");
            }
        }
    }
}

fn picture(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 120]));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format).unwrap();
    out
}

/// Un JPEG dont l'en-tête porte un bloc EXIF (de quoi y loger une position GPS).
fn jpeg_with_exif() -> Vec<u8> {
    let mut j = picture(64, 48, ImageFormat::Jpeg);
    let mut exif = b"Exif\0\0GPS-SECRET".to_vec();
    exif.extend_from_slice(&[0; 6]);
    let mut seg = vec![0xFF, 0xE1];
    seg.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    seg.extend(exif);
    j.splice(2..2, seg);
    j
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Un serveur, un serveur de discussion avec #général et un deuxième salon.
async fn world(base: &str) -> (Who, Who, Id, Id, Id) {
    let lou = Who::new(base, "lou").await;
    let sam = Who::new(base, "sam").await;
    let (s, g) = lou.call(Method::POST, "/guilds", Some(json!({"name": "Bord de mer"}))).await;
    assert_eq!(s, 201);
    let g: GuildState = serde_json::from_value(g).unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let (s, c2) = lou
        .call(
            Method::POST,
            &format!("/guilds/{}/channels", g.guild.id),
            Some(json!({"name": "autre", "kind": "text", "parent_id": null})),
        )
        .await;
    assert_eq!(s, 201);
    let other: Channel = serde_json::from_value(c2).unwrap();
    let (_, inv) = lou.call(Method::POST, &format!("/channels/{general}/invites"), Some(json!({}))).await;
    let code = inv["code"].as_str().unwrap().to_string();
    assert_eq!(sam.call(Method::POST, &format!("/invites/{code}"), None).await.0, StatusCode::OK);
    (lou, sam, g.guild.id, general, other.id)
}

fn files_in(dir: &Path, sub: &str) -> usize {
    std::fs::read_dir(dir.join(sub)).map(|d| d.count()).unwrap_or(0)
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn upload_limits_and_message_rules(pool: PgPool) {
    let (base, dir) = spawn(pool, false, false).await;
    let (lou, _sam, _g, general, _) = world(&base).await;

    // Trop lourd : refusé sans rien garder, même sans Content-Length fiable.
    let (s, e) = lou.upload(general, "gros.bin", "application/octet-stream", vec![7; MAX_UPLOAD as usize + 1]).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(e["code"], "too_large");
    let chunks: Vec<Result<Vec<u8>, std::io::Error>> = vec![Ok(vec![1; 60 * 1024]), Ok(vec![2; 60 * 1024])];
    let r = lou
        .req(Method::POST, &format!("/channels/{general}/attachments?filename=flux.bin"))
        .body(reqwest::Body::wrap_stream(futures_util::stream::iter(chunks)))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE, "le flux sans longueur annoncée est borné aussi");
    assert_eq!(files_in(&dir, "attachments"), 0);

    // Vide : refusé. Au plafond exact : accepté.
    assert_eq!(lou.upload(general, "vide.txt", "text/plain", vec![]).await.0, StatusCode::BAD_REQUEST);
    let (s, ok) = lou.upload(general, "plein.bin", "application/octet-stream", vec![9; MAX_UPLOAD as usize]).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(ok["size"], MAX_UPLOAD);

    // Le nom est nettoyé, le type annoncé ne sert qu'à l'affichage et reste borné.
    let (_, a) = lou.upload(general, "..%2F..%2Fsecret.txt", "text/plain; charset=utf-8", b"salut".to_vec()).await;
    assert!(a["filename"].as_str().unwrap().ends_with("secret.txt") && !a["filename"].as_str().unwrap().contains('/'));
    assert_eq!(a["mime"], "text/plain");
    let (_, weird) = lou.upload(general, "x.bin", "pas un type", b"x".to_vec()).await;
    assert_eq!(weird["mime"], "application/octet-stream");

    // Sans session : 401.
    let r = Client::new()
        .post(format!("http://{base}/api/v1/channels/{general}/attachments?filename=a.txt"))
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // Un message peut n'avoir que des fichiers ; un message vide sans fichier est refusé.
    assert_eq!(lou.send(general, "", &[]).await.0, StatusCode::BAD_REQUEST);
    let (s, m) = lou.send(general, "", std::slice::from_ref(&a)).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(m["attachments"][0]["filename"], a["filename"]);
    // Un fichier ne se rattache qu'une fois.
    assert_eq!(lou.send(general, "encore", std::slice::from_ref(&a)).await.0, StatusCode::BAD_REQUEST);

    // Dix fichiers au plus.
    let mut eleven = Vec::new();
    for i in 0..11 {
        eleven.push(lou.upload(general, &format!("f{i}.txt"), "text/plain", vec![b'a'; 3]).await.1);
    }
    assert_eq!(lou.send(general, "trop", &eleven).await.0, StatusCode::BAD_REQUEST);
    let (s, m) = lou.send(general, "dix", &eleven[..10]).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(m["attachments"].as_array().unwrap().len(), 10);
    // Le même fichier deux fois dans un message, ou celui d'un autre : refusés.
    let (_, one) = lou.upload(general, "un.txt", "text/plain", b"1".to_vec()).await;
    assert_eq!(lou.send(general, "doublon", &[one.clone(), one.clone()]).await.0, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn serving_rules_and_access(pool: PgPool) {
    let (base, dir) = spawn(pool, false, false).await;
    let (lou, sam, _g, general, other) = world(&base).await;
    let stranger = Who::new(&base, "noa").await;

    let (s, img) = lou.upload(general, "photo é.jpg", "image/jpeg", jpeg_with_exif()).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!((img["width"].as_u64(), img["height"].as_u64()), (Some(64), Some(48)));
    assert_eq!(img["mime"], "image/jpeg");
    let (_, html) = lou.upload(general, "page.html", "text/html", b"<script>alert(1)</script>".to_vec()).await;
    let (_, svg) = lou
        .upload(
            general,
            "dessin.svg",
            "image/svg+xml",
            b"<svg xmlns='http://www.w3.org/2000/svg'><script>1</script></svg>".to_vec(),
        )
        .await;
    // Une page déguisée en image n'est pas une image.
    let (_, fake) = lou.upload(general, "faux.png", "image/png", b"<html>pas un png</html>".to_vec()).await;
    assert!(fake["width"].is_null());
    let (_, m) = lou.send(general, "regardez", &[img.clone(), html.clone(), svg.clone(), fake.clone()]).await;
    assert!(m["id"].is_string());
    let path = |a: &Value| format!("/channels/{general}/attachments/{}", a["id"].as_str().unwrap());

    // Une image sûre s'affiche ; ni HTML ni SVG ni faux fichier ne s'affichent jamais.
    let r = sam.get(&path(&img)).await;
    assert_eq!(r.status(), 200);
    let h = r.headers().clone();
    assert_eq!(h["content-type"], "image/jpeg");
    assert!(h["content-disposition"].to_str().unwrap().starts_with("inline;"));
    assert!(h["content-disposition"].to_str().unwrap().contains("filename*=UTF-8''photo%20%C3%A9.jpg"));
    assert_eq!(h["x-content-type-options"], "nosniff");
    let original = r.bytes().await.unwrap();
    assert!(contains(&original, b"GPS-SECRET"), "l'original est gardé tel quel");
    for f in [&html, &svg, &fake] {
        let r = sam.get(&path(f)).await;
        assert_eq!(r.status(), 200);
        assert!(r.headers()["content-disposition"].to_str().unwrap().starts_with("attachment;"), "{f}");
        assert_eq!(r.headers()["content-type"], "application/octet-stream");
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
    }
    // Forcer le téléchargement d'une image.
    let r = sam.get(&format!("{}?download=1", path(&img))).await;
    assert!(r.headers()["content-disposition"].to_str().unwrap().starts_with("attachment;"));

    // L'aperçu est ré-encodé : lisible, et sans le bloc EXIF.
    let r = sam.get(&format!("{}/preview", path(&img))).await;
    assert_eq!(r.status(), 200);
    let preview = r.bytes().await.unwrap();
    assert!(image::load_from_memory(&preview).is_ok());
    assert!(!contains(&preview, b"GPS-SECRET") && !contains(&preview, b"Exif"));
    // Pas d'aperçu pour un fichier qui n'est pas une image.
    assert_eq!(sam.get(&format!("{}/preview", path(&html))).await.status(), 404);

    // Sans session : 401. Hors du serveur : 404. Mauvais salon : 404. Identifiant inventé : 404.
    let r = Client::new().get(format!("http://{base}/api/v1{}", path(&img))).send().await.unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(stranger.get(&path(&img)).await.status(), 404);
    assert_eq!(stranger.get(&format!("{}/preview", path(&img))).await.status(), 404);
    let wrong = format!("/channels/{other}/attachments/{}", img["id"].as_str().unwrap());
    assert_eq!(lou.get(&wrong).await.status(), 404);
    assert_eq!(lou.get(&format!("/channels/{general}/attachments/{}", Id::nil())).await.status(), 404);

    // Un envoi pas encore joint à un message n'est lisible que par celui qui l'a envoyé.
    let (_, pending) = lou.upload(general, "brouillon.txt", "text/plain", b"brouillon".to_vec()).await;
    assert_eq!(lou.get(&path(&pending)).await.status(), 200);
    assert_eq!(sam.get(&path(&pending)).await.status(), 404);
    // …qui peut le retirer ; plus personne ne le lit et le fichier part.
    let before = files_in(&dir, "attachments");
    assert_eq!(sam.call(Method::DELETE, &path(&pending), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(lou.call(Method::DELETE, &path(&pending), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(lou.get(&path(&pending)).await.status(), 404);
    assert_eq!(files_in(&dir, "attachments"), before - 1);
    // Un fichier déjà joint ne se retire pas par là.
    assert_eq!(lou.call(Method::DELETE, &path(&img), None).await.0, StatusCode::NOT_FOUND);

    // Sam ne peut pas joindre l'envoi de Lou.
    let (_, mine) = lou.upload(general, "a.txt", "text/plain", b"a".to_vec()).await;
    assert_eq!(sam.send(general, "volé", &[mine]).await.0, StatusCode::BAD_REQUEST);
    // Et un étranger ne peut pas envoyer dans ce salon.
    assert_eq!(stranger.upload(general, "z.txt", "text/plain", b"z".to_vec()).await.0, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn deleting_a_message_deletes_its_files(pool: PgPool) {
    let (base, dir) = spawn(pool, false, false).await;
    let (lou, sam, _g, general, _) = world(&base).await;
    let (_, img) = lou.upload(general, "p.png", "image/png", picture(40, 30, ImageFormat::Png)).await;
    let (_, txt) = lou.upload(general, "n.txt", "text/plain", b"notes".to_vec()).await;
    let (_, m) = lou.send(general, "", &[img.clone(), txt.clone()]).await;
    // Original + aperçu de l'image, et le fichier texte.
    assert_eq!(files_in(&dir, "attachments"), 3);
    let mid = m["id"].as_str().unwrap();
    // Un autre membre sans droit de modération ne supprime pas.
    assert_eq!(
        sam.call(Method::DELETE, &format!("/channels/{general}/messages/{mid}"), None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(files_in(&dir, "attachments"), 3);
    assert_eq!(
        lou.call(Method::DELETE, &format!("/channels/{general}/messages/{mid}"), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(files_in(&dir, "attachments"), 0);
    assert_eq!(
        lou.get(&format!("/channels/{general}/attachments/{}", img["id"].as_str().unwrap())).await.status(),
        404
    );
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn account_purge_deletes_uploaded_files(pool: PgPool) {
    let (base, dir) = spawn(pool.clone(), false, false).await;
    let (lou, sam, _g, general, _) = world(&base).await;
    let (_, a) = sam.upload(general, "sam.txt", "text/plain", b"sam".to_vec()).await;
    sam.send(general, "mon fichier", &[a]).await;
    let (_, b) = lou.upload(general, "lou.txt", "text/plain", b"lou".to_vec()).await;
    lou.send(general, "le sien", &[b]).await;
    assert_eq!(files_in(&dir, "attachments"), 2);
    // Sam ferme son compte ; le délai de grâce est écoulé, la tâche de purge passe.
    let (s, _) = sam.call(Method::POST, "/users/@me/delete", Some(json!({"password": "motdepasse"}))).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    sqlx::query("update users set deletion_requested_at = now() - interval '30 days' where username = 'sam'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("update jobs set run_at = now() where kind = 'account_purge'").execute(&pool).await.unwrap();
    for _ in 0..40 {
        if files_in(&dir, "attachments") == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(files_in(&dir, "attachments"), 1, "le fichier de Lou reste, celui de Sam est parti");
}

// ───────── aperçus de liens ─────────

/// Une page à servir : chemin → (type, corps).
async fn fixture(png: Vec<u8>) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { return };
            let png = png.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_string();
                let (status, ctype, body): (u16, &str, Vec<u8>) = match path.as_str() {
                    "/article" => (
                        200,
                        "text/html; charset=utf-8",
                        b"<html><head><title>Titre repli</title><meta property=\"og:title\" content=\"Le grand titre\">\
                          <meta property=\"og:description\" content=\"Un r&eacute;sum&eacute; de l'article\">\
                          <meta property=\"og:site_name\" content=\"Le Phare\"><meta property=\"og:image\" content=\"/une.png\">\
                          </head><body>Salut</body></html>"
                            .to_vec(),
                    ),
                    "/une.png" => (200, "image/png", png),
                    "/second" => (200, "text/html", b"<title>Seconde page</title>".to_vec()),
                    _ => (404, "text/plain", b"non".to_vec()),
                };
                let head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    port
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn link_previews_are_fetched_stored_updated_and_removable(pool: PgPool) {
    let (base, dir) = spawn(pool, true, true).await;
    let (lou, sam, _g, general, other) = world(&base).await;
    let port = fixture(picture(800, 450, ImageFormat::Png)).await;
    let mut gw = sam.gateway().await;

    let url = format!("http://127.0.0.1:{port}/article");
    let (s, m) = lou.send(general, &format!("à lire {url}"), &[]).await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(m["embeds"].is_null(), "l'aperçu arrive plus tard, par MESSAGE_UPDATE");
    let mid: Id = m["id"].as_str().unwrap().parse().unwrap();
    let updated = loop {
        let u = gw.update().await;
        if u.id == mid && !u.embeds.is_empty() {
            break u;
        }
    };
    let e = &updated.embeds[0];
    assert_eq!(e.url, url);
    assert_eq!(e.title.as_deref(), Some("Le grand titre"));
    assert_eq!(e.description.as_deref(), Some("Un résumé de l'article"));
    assert_eq!(e.site_name.as_deref(), Some("Le Phare"));
    let img = e.image.as_ref().expect("image gardée");
    assert_eq!((img.width, img.height), (640, 360), "réduite, jamais agrandie");
    assert!(img.key.ends_with(".jpg") && !img.key.contains("127.0.0.1"));
    assert_eq!(files_in(&dir, "embeds"), 1);

    // L'image est servie par KRAKEN, aux seuls membres du salon.
    let path = format!("/channels/{general}/embeds/{}", img.key);
    let r = sam.get(&path).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/jpeg");
    assert!(image::load_from_memory(&r.bytes().await.unwrap()).is_ok());
    let stranger = Who::new(&base, "noa").await;
    assert_eq!(stranger.get(&path).await.status(), 404);
    assert_eq!(lou.get(&format!("/channels/{other}/embeds/{}", img.key)).await.status(), 404);
    assert_eq!(Client::new().get(format!("http://{base}/api/v1{path}")).send().await.unwrap().status(), 401);

    // Seul l'auteur retire l'aperçu ; l'image part avec lui, il ne revient pas à une modification.
    let del = format!("/channels/{general}/messages/{mid}/embeds?url={}", url.replace(':', "%3A").replace('/', "%2F"));
    assert_eq!(sam.call(Method::DELETE, &del, None).await.0, StatusCode::FORBIDDEN);
    let (s, after) = lou.call(Method::DELETE, &del, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(after["embeds"].is_null());
    assert_eq!(gw.update().await.embeds.len(), 0);
    assert_eq!(files_in(&dir, "embeds"), 0);
    let edit = format!("/channels/{general}/messages/{mid}");
    lou.call(Method::PATCH, &edit, Some(json!({"content": format!("relu {url}")}))).await;
    // La modification diffuse un MESSAGE_UPDATE, puis la tâche conclut qu'il n'y a rien à refaire.
    assert!(gw.update().await.embeds.is_empty());
    gw.quiet(1500).await;

    // Un nouveau lien s'ajoute, l'ancien (retiré par l'auteur) reste caché.
    let second = format!("http://127.0.0.1:{port}/second");
    lou.call(Method::PATCH, &edit, Some(json!({"content": format!("relu {url} et {second}")}))).await;
    let _ = gw.update().await;
    let u = loop {
        let u = gw.update().await;
        if !u.embeds.is_empty() {
            break u;
        }
    };
    assert_eq!(u.embeds.len(), 1);
    assert_eq!(u.embeds[0].title.as_deref(), Some("Seconde page"));
    assert_eq!(u.embeds[0].site_name.as_deref(), Some("127.0.0.1"));

    // Retirer le lien du texte retire l'aperçu.
    lou.call(Method::PATCH, &edit, Some(json!({"content": "plus de lien"}))).await;
    let _ = gw.update().await;
    assert!(gw.update().await.embeds.is_empty());

    // Supprimer le message supprime ses images d'aperçu.
    let (_, m2) = lou.send(general, &url, &[]).await;
    let mid2: Id = m2["id"].as_str().unwrap().parse().unwrap();
    loop {
        let u = gw.update().await;
        if u.id == mid2 && !u.embeds.is_empty() {
            break;
        }
    }
    assert_eq!(files_in(&dir, "embeds"), 1);
    lou.call(Method::DELETE, &format!("/channels/{general}/messages/{mid2}"), None).await;
    assert_eq!(files_in(&dir, "embeds"), 0);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn previews_skip_suppressed_links_code_and_a_disabled_server(pool: PgPool) {
    let (base, _dir) = spawn(pool, true, true).await;
    let (lou, sam, _g, general, _) = world(&base).await;
    let port = fixture(picture(10, 10, ImageFormat::Png)).await;
    let mut gw = sam.gateway().await;
    let url = format!("http://127.0.0.1:{port}/article");
    // <url> : pas d'aperçu ; dans du code : non plus.
    lou.send(general, &format!("<{url}> et `{url}`"), &[]).await;
    gw.quiet(1500).await;
    // Plus de cinq liens : cinq aperçus au plus.
    let many: String = (0..8).map(|i| format!("http://127.0.0.1:{port}/second?n={i} ")).collect();
    lou.send(general, &many, &[]).await;
    let mut best = 0;
    for _ in 0..8 {
        match tokio::time::timeout(Duration::from_secs(3), gw.update()).await {
            Ok(u) => best = best.max(u.embeds.len()),
            Err(_) => break,
        }
    }
    assert_eq!(best, 5);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn disabled_previews_do_nothing(pool: PgPool) {
    let (base, _dir) = spawn(pool, false, true).await;
    let (lou, sam, _g, general, _) = world(&base).await;
    let port = fixture(picture(10, 10, ImageFormat::Png)).await;
    let mut gw = sam.gateway().await;
    lou.send(general, &format!("http://127.0.0.1:{port}/article"), &[]).await;
    gw.quiet(1500).await;
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn previews_never_reach_internal_addresses(pool: PgPool) {
    // Garde actif (réglage de test éteint) : le serveur local de la fixture est refusé.
    let (base, dir) = spawn(pool, true, false).await;
    let (lou, sam, _g, general, _) = world(&base).await;
    let port = fixture(picture(10, 10, ImageFormat::Png)).await;
    let mut gw = sam.gateway().await;
    lou.send(
        general,
        &format!("http://127.0.0.1:{port}/article http://169.254.169.254/latest/meta-data/ http://[::1]/"),
        &[],
    )
    .await;
    gw.quiet(2000).await;
    assert_eq!(files_in(&dir, "embeds"), 0);
}
