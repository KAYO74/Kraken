//! Journal d'audit de bout en bout sur un vrai port et une vraie base : `GET /guilds/{id}/audit-log`
//! (droit « Voir le journal d'audit », 403 sans lui, 404 hors du serveur), les actions faites par l'API qui
//! y laissent une trace lisible, l'ordre, la pagination par curseur, la limite, les filtres, les paramètres
//! mal formés (400) et ce qui ne doit jamais en sortir.

use kraken_core::perms;
use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

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

    async fn guild(&self) -> GuildState {
        let (s, v) = self.call(Method::POST, "/guilds", Some(json!({ "name": "Abysse" }))).await;
        assert_eq!(s, 201);
        serde_json::from_value(v).unwrap()
    }

    async fn join(&self, g: &GuildState, inviter: &Who) {
        let cid = general(g);
        let (s, v) = inviter.call(Method::POST, &format!("/channels/{cid}/invites"), Some(json!({}))).await;
        assert_eq!(s, 201);
        let (s, _) = self.call(Method::POST, &format!("/invites/{}", v["code"].as_str().unwrap()), None).await;
        assert_eq!(s, 200);
    }

    async fn role(&self, gid: Id, name: &str, perms: u64) -> Role {
        let body = json!({ "name": name, "permissions": perms, "color": 2, "hoist": true });
        let (s, v) = self.call(Method::POST, &format!("/guilds/{gid}/roles"), Some(body)).await;
        assert_eq!(s, 201, "{v}");
        serde_json::from_value(v).unwrap()
    }

    async fn give(&self, gid: Id, to: &Who, r: &Role) {
        let (s, _) =
            self.call(Method::PUT, &format!("/guilds/{gid}/members/{}/roles/{}", to.user.id, r.id), None).await;
        assert_eq!(s, 200);
    }

    /// Le journal, avec ces paramètres d'adresse.
    async fn log(&self, gid: Id, query: &str) -> (StatusCode, Value) {
        self.call(Method::GET, &format!("/guilds/{gid}/audit-log{query}"), None).await
    }

    async fn entries(&self, gid: Id, query: &str) -> Vec<Value> {
        let (s, v) = self.log(gid, query).await;
        assert_eq!(s, 200, "{v}");
        v["entries"].as_array().unwrap().clone()
    }
}

fn general(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id
}

fn actions(entries: &[Value]) -> Vec<&str> {
    entries.iter().map(|e| e["action"].as_str().unwrap()).collect()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn the_right_to_read_decides_who_gets_in(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, adm, aud, plain) = (
        Who::new(&base, "own").await,
        Who::new(&base, "adm").await,
        Who::new(&base, "aud").await,
        Who::new(&base, "pla").await,
    );
    let out = Who::new(&base, "out").await;
    let g = own.guild().await;
    let gid = g.guild.id;
    for w in [&adm, &aud, &plain] {
        w.join(&g, &own).await;
    }
    let admin = own.role(gid, "Admins", perms::ADMINISTRATOR).await;
    own.give(gid, &adm, &admin).await;
    let greffiers = own.role(gid, "Greffiers", perms::VIEW_AUDIT_LOG).await;
    own.give(gid, &aud, &greffiers).await;

    for w in [&own, &adm, &aud] {
        assert_eq!(w.log(gid, "").await.0, 200, "{}", w.user.username);
    }
    let (s, v) = plain.log(gid, "").await;
    assert_eq!((s.as_u16(), v["code"].as_str()), (403, Some("forbidden")));
    // Hors du serveur : 404, comme pour un serveur qui n'existe pas.
    let (s, v) = out.log(gid, "").await;
    assert_eq!((s.as_u16(), v["code"].as_str()), (404, Some("not_found")));
    assert_eq!(out.log(Id::nil(), "").await.0, 404);
    // Sans session : 401. Les autres verbes n'existent pas.
    let anon = Client::new().get(format!("http://{base}/api/v1/guilds/{gid}/audit-log")).send().await.unwrap();
    assert_eq!(anon.status(), 401);
    assert_eq!(own.call(Method::DELETE, &format!("/guilds/{gid}/audit-log"), None).await.0, 405);
    // Retirer le rôle retire l'accès.
    let (s, _) =
        own.call(Method::DELETE, &format!("/guilds/{gid}/members/{}/roles/{}", aud.user.id, greffiers.id), None).await;
    assert_eq!(s, 200);
    assert_eq!(aud.log(gid, "").await.0, 403);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn real_actions_read_back_as_named_entries_newest_first(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, sam, lea) = (Who::new(&base, "own").await, Who::new(&base, "sam").await, Who::new(&base, "lea").await);
    let g = own.guild().await;
    let gid = g.guild.id;
    sam.join(&g, &own).await;
    lea.join(&g, &own).await;

    let r = own.role(gid, "Modos", perms::KICK).await;
    own.give(gid, &sam, &r).await;
    let (s, _) = own
        .call(Method::POST, &format!("/guilds/{gid}/channels"), Some(json!({ "name": "annonces", "kind": "text" })))
        .await;
    assert_eq!(s, 201);
    let (s, _) = own
        .call(
            Method::PUT,
            &format!("/guilds/{gid}/bans/{}", lea.user.id),
            Some(json!({ "reason": "spam <b>gras</b>" })),
        )
        .await;
    assert_eq!(s, 200);

    let entries = own.entries(gid, "").await;
    // Les deux invitations qui ont fait entrer sam et lea sont là aussi.
    assert_eq!(
        actions(&entries),
        ["member_ban", "channel_create", "member_role_add", "role_create", "invite_create", "invite_create"]
    );
    let ban = &entries[0];
    assert_eq!(ban["actor"]["id"], json!(own.user.id));
    assert_eq!(ban["actor"]["name"], "own");
    assert_eq!(ban["target"], json!({ "kind": "user", "id": lea.user.id, "name": "lea" }));
    // Un texte reste un texte : le serveur ne le transforme pas, c'est au client de ne rien interpréter.
    assert_eq!(ban["reason"], "spam <b>gras</b>");
    assert!(ban["created_at"].as_i64().unwrap() > 1_700_000_000_000);
    let chan = &entries[1];
    assert_eq!(chan["target"]["kind"], "channel");
    assert_eq!(chan["target"]["name"], "annonces");
    assert_eq!(chan["details"]["kind"], "text");
    assert_eq!(entries[2]["details"]["role"], "Modos");
    assert_eq!(entries[2]["target"]["name"], "sam");
    // Rien d'autre que ces clés ne sort d'une entrée.
    for e in &entries {
        for k in e.as_object().unwrap().keys() {
            assert!(
                ["id", "created_at", "actor", "action", "target", "reason", "details", "changes"].contains(&k.as_str()),
                "{k}"
            );
        }
    }

    // Salon renommé puis supprimé : l'avant / après se lit, et le nom d'alors reste.
    let cid: Id = serde_json::from_value(chan["target"]["id"].clone()).unwrap();
    let (s, _) =
        own.call(Method::PATCH, &format!("/channels/{cid}"), Some(json!({ "name": "infos", "slowmode_s": 30 }))).await;
    assert_eq!(s, 200);
    assert_eq!(own.call(Method::DELETE, &format!("/channels/{cid}"), None).await.0, 204);
    let entries = own.entries(gid, "").await;
    assert_eq!(actions(&entries)[..2], ["channel_delete", "channel_update"]);
    assert_eq!(entries[0]["target"]["name"], "infos");
    assert_eq!(entries[1]["target"]["name"], "infos");
    assert_eq!(
        entries[1]["changes"],
        json!([
            { "field": "name", "before": "annonces", "after": "infos" },
            { "field": "slowmode_s", "before": "0", "after": "30" }
        ])
    );
    // Un rôle sans changement visible : pas d'avant / après, et la clé `changes` n'est pas là.
    assert!(entries[0].get("changes").is_none());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_moderator_deleting_a_message_is_logged_without_its_text_and_an_author_is_not(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, sam) = (Who::new(&base, "own").await, Who::new(&base, "sam").await);
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    sam.join(&g, &own).await;
    async fn say(w: &Who, cid: Id, t: &str) -> String {
        let (s, v) = w.call(Method::POST, &format!("/channels/{cid}/messages"), Some(json!({ "content": t }))).await;
        assert_eq!(s, 201);
        v["id"].as_str().unwrap().to_string()
    }
    let mine = say(&sam, cid, "secret du mot de passe").await;
    let theirs = say(&sam, cid, "autre secret").await;
    let theirs_too = say(&own, cid, "le mien").await;
    // L'auteur retire le sien, le propriétaire retire le sien : rien dans le journal.
    assert_eq!(sam.call(Method::DELETE, &format!("/channels/{cid}/messages/{mine}"), None).await.0, 204);
    assert_eq!(own.call(Method::DELETE, &format!("/channels/{cid}/messages/{theirs_too}"), None).await.0, 204);
    assert!(own.entries(gid, "?category=serveur").await.is_empty());
    // Le propriétaire retire celui de sam.
    assert_eq!(own.call(Method::DELETE, &format!("/channels/{cid}/messages/{theirs}"), None).await.0, 204);
    let (s, v) = own.log(gid, "?category=serveur").await;
    assert_eq!(s, 200);
    let e = &v["entries"][0];
    assert_eq!(
        (e["action"].as_str(), e["actor"]["name"].as_str(), e["target"]["name"].as_str()),
        (Some("message_delete"), Some("own"), Some("sam"))
    );
    assert_eq!(e["details"]["channel"], "général");
    assert!(!v.to_string().contains("secret"));
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn pages_limit_and_filters(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, sam) = (Who::new(&base, "own").await, Who::new(&base, "sam").await);
    let g = own.guild().await;
    let gid = g.guild.id;
    sam.join(&g, &own).await;
    let greffiers = own.role(gid, "Greffiers", perms::VIEW_AUDIT_LOG | perms::MANAGE_CHANNELS).await;
    own.give(gid, &sam, &greffiers).await;
    // 60 rôles du propriétaire, 12 salons de sam, l'invitation de sam et son rôle : 74 entrées en tout.
    for n in 0..59 {
        own.role(gid, &format!("r{n}"), 0).await;
    }
    for n in 0..12 {
        let (s, _) = sam
            .call(
                Method::POST,
                &format!("/guilds/{gid}/channels"),
                Some(json!({ "name": format!("c{n}"), "kind": "text" })),
            )
            .await;
        assert_eq!(s, 201);
    }

    // Par défaut 50, au plus 100, au moins 1.
    let (s, v) = own.log(gid, "").await;
    assert_eq!(s, 200);
    assert_eq!(v["entries"].as_array().unwrap().len(), AUDIT_PAGE as usize);
    assert!(v["next"].is_string());
    assert_eq!(own.entries(gid, "?limit=1000").await.len(), 74);
    assert_eq!(own.entries(gid, "?limit=0").await.len(), 1);
    assert_eq!(own.entries(gid, "?limit=-5").await.len(), 1);
    assert_eq!(own.entries(gid, "?limit=100").await.len(), 74);
    // Le tri est celui de l'id (UUID v7), du plus récent au plus ancien.
    let all = own.entries(gid, "?limit=100").await;
    let ids: Vec<&str> = all.iter().map(|e| e["id"].as_str().unwrap()).collect();
    let mut sorted = ids.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(ids, sorted);

    // Pages de 30 : les 74 entrées une seule fois, `next` nul à la fin, les « acteurs » seulement au début.
    let (mut seen, mut next, mut pages) = (vec![], None::<String>, 0);
    loop {
        let q = match &next {
            Some(n) => format!("?limit=30&before={n}"),
            None => "?limit=30".into(),
        };
        let (s, v) = own.log(gid, &q).await;
        assert_eq!(s, 200);
        pages += 1;
        assert_eq!(v.get("actors").is_some(), next.is_none(), "page {pages}");
        seen.extend(v["entries"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()));
        match v["next"].as_str() {
            Some(n) => next = Some(n.to_string()),
            None => break,
        }
    }
    assert_eq!((seen.len(), pages), (74, 3));
    assert_eq!(seen, ids.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let first: AuditPage = serde_json::from_value(own.log(gid, "?limit=2").await.1).unwrap();
    assert_eq!(first.actors.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["own", "sam"]);

    // Filtres.
    let channels = own.entries(gid, "?category=salons&limit=100").await;
    assert_eq!(channels.len(), 12);
    assert!(channels.iter().all(|e| e["action"] == "channel_create"));
    let roles = own.entries(gid, "?category=roles&limit=100").await;
    assert_eq!(roles.len(), 60);
    let by_sam = own.entries(gid, &format!("?actor={}&limit=100", sam.user.id)).await;
    assert_eq!(by_sam.len(), 12);
    assert!(by_sam.iter().all(|e| e["actor"]["id"] == json!(sam.user.id)));
    assert!(own.entries(gid, &format!("?actor={}&category=emojis", sam.user.id)).await.is_empty());
    assert_eq!(own.entries(gid, "?category=membres").await.len(), 1);
    assert_eq!(own.entries(gid, "?category=").await.len(), 50, "un filtre vide n'en est pas un");

    // Paramètres mal formés : 400 de l'API, avec son code.
    for bad in
        ["?before=pas-un-uuid", "?actor=12", "?limit=beaucoup", "?category=autre", "?category=Salons", "?limit=1.5"]
    {
        let (s, v) = own.log(gid, bad).await;
        assert_eq!((s.as_u16(), v["code"].as_str()), (400, Some("invalid")), "{bad}");
    }
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn nothing_sensitive_leaves_the_log(pool: PgPool) {
    let base = spawn(pool, 1000).await;
    let (own, sam) = (Who::new(&base, "own").await, Who::new(&base, "sam").await);
    let g = own.guild().await;
    let (gid, cid) = (g.guild.id, general(&g));
    sam.join(&g, &own).await;
    let (s, inv) = sam
        .call(Method::POST, &format!("/channels/{cid}/invites"), Some(json!({ "max_uses": 3, "max_age": 3600 })))
        .await;
    assert_eq!(s, 201);
    let code = inv["code"].as_str().unwrap().to_string();
    let (s, _) = own
        .call(
            Method::PUT,
            &format!("/guilds/{gid}/members/{}/timeout", sam.user.id),
            Some(json!({ "seconds": 3600, "reason": "pause" })),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(own.call(Method::DELETE, &format!("/invites/{code}"), None).await.0, 204);
    let (s, body) = own.log(gid, "?limit=100").await;
    assert_eq!(s, 200);
    let text = body.to_string();
    // Ni le code de l'invitation, ni une adresse e-mail, ni un jeton de session, ni un mot de passe.
    for secret in
        [code.as_str(), "example.org", sam.token.as_str(), own.token.as_str(), "motdepasse", "password", "hash"]
    {
        assert!(!text.contains(secret), "{secret}");
    }
    let entries = body["entries"].as_array().unwrap();
    let by_action = |a: &str| entries.iter().find(|e| e["action"] == a).unwrap().clone();
    let create = by_action("invite_create");
    assert_eq!(
        (
            create["details"]["max_uses"].as_str(),
            create["details"]["max_age_s"].as_str(),
            create["details"]["channel"].as_str()
        ),
        (Some("3"), Some("3600"), Some("général"))
    );
    assert!(create["target"].is_null());
    assert_eq!(by_action("invite_revoke")["details"]["inviter"], "sam");
    let to = by_action("member_timeout");
    assert_eq!(
        (to["details"]["seconds"].as_str(), to["reason"].as_str(), to["target"]["name"].as_str()),
        (Some("3600"), Some("pause"), Some("sam"))
    );
    // L'exclusion de sam lui a retiré le droit d'écrire, pas celui du propriétaire de lire.
    assert_eq!(own.log(gid, "").await.0, 200);
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn reads_are_rate_limited_per_client(pool: PgPool) {
    // 5 essais par minute pour les routes de connexion ; la lecture du journal a son compteur, plus large (120).
    let base = spawn(pool, 5).await;
    let own = Who::new(&base, "own").await;
    let gid = own.guild().await.guild.id;
    let mut ok = 0;
    let mut last = StatusCode::OK;
    for _ in 0..(AUDIT_READS_PER_MIN + 5) {
        last = own.log(gid, "?limit=1").await.0;
        if last == 200 {
            ok += 1;
        } else {
            break;
        }
    }
    assert_eq!((ok, last), (AUDIT_READS_PER_MIN, StatusCode::TOO_MANY_REQUESTS));
}
