//! La langue des e-mails du serveur (confirmation d'adresse à l'inscription, nouveau mot de passe, nouvelle adresse) : le
//! client envoie `lang` avec la demande, le serveur le normalise (`fr`, `en`, `fa`) dans la tâche mise en file. Les
//! tâches ne sont pas exécutées ici (`app` ne lance pas les tâches de fond) : le test lit la file.

use kraken_proto::*;
use kraken_server::{AppState, Config};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

fn config(data_dir: &std::path::Path) -> Config {
    Config {
        addr: "127.0.0.1:0".into(),
        web_dir: "/nonexistent".into(),
        data_dir: data_dir.to_string_lossy().into(),
        database_url: String::new(),
        public_url: "http://127.0.0.1".into(),
        // Un SMTP qui n'existe pas : l'instance « envoie des e-mails » (les tâches se mettent en file), aucun ne part.
        smtp_url: Some("smtp://127.0.0.1:1".into()),
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
    let dir = std::env::temp_dir().join(format!("kraken-maillang-{}", rand::random::<u64>()));
    let state = AppState::with_db(kraken_db::Db::from_pool(pool), config(&dir)).await.unwrap();
    let app = kraken_server::app(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("{addr}")
}

const PASSWORD: &str = "motdepasse";

/// Les langues envoyées par le client et celle qui part dans la tâche : `None` est une demande sans `lang`.
fn cases() -> Vec<(Option<&'static str>, &'static str)> {
    vec![
        (Some("fa"), "fa"),
        (Some("en"), "en"),
        (Some("fa-IR"), "fa"),
        (Some("xx"), "fr"),
        (None, "fr"),
        (Some("fr"), "fr"),
        (Some("EN-gb"), "en"),
        (Some("fa_AF"), "fa"),
        (Some(""), "fr"),
    ]
}

/// Ajoute `lang` au corps d'une demande, quand il y en a une.
fn with_lang(mut body: Value, lang: Option<&str>) -> Value {
    if let Some(l) = lang {
        body["lang"] = json!(l);
    }
    body
}

/// Les tâches `kind` en file (leur charge utile), retirées ensuite : la suivante repart d'une file vide, et la clé d'idempotence
/// d'une demande déjà en file ne gêne pas.
async fn take_jobs(pool: &PgPool, kind: &str) -> Vec<Value> {
    let rows = sqlx::query_scalar::<_, Value>("select payload from jobs where kind = $1 order by id")
        .bind(kind)
        .fetch_all(pool)
        .await
        .unwrap();
    sqlx::query("delete from jobs where kind = $1").bind(kind).execute(pool).await.unwrap();
    rows
}

async fn register(c: &Client, base: &str, name: &str, lang: Option<&str>) -> AuthResp {
    let body = with_lang(json!({"username": name, "email": format!("{name}@example.org"), "password": PASSWORD}), lang);
    let r = c.post(format!("http://{base}/api/v1/auth/register")).json(&body).send().await.unwrap();
    assert_eq!(r.status(), 201, "{lang:?}");
    r.json().await.unwrap()
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn the_confirmation_mail_of_a_registration_is_queued_in_the_language_of_the_client(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let c = Client::new();
    for (i, (lang, want)) in cases().into_iter().enumerate() {
        let a = register(&c, &base, &format!("lea{i}"), lang).await;
        let jobs = take_jobs(&pool, "email_verify").await;
        assert_eq!(jobs.len(), 1, "{lang:?} : une tâche par inscription");
        assert_eq!(jobs[0]["lang"], want, "{lang:?} : {}", jobs[0]);
        assert_eq!(jobs[0]["user_id"], json!(a.user.id), "{lang:?}");
        // Rien d'autre ne traîne dans la tâche : ni adresse, ni jeton, ni mot de passe.
        let mut keys: Vec<&str> = jobs[0].as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["lang", "user_id"], "{lang:?}");
    }
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn the_new_password_mail_is_queued_in_the_language_of_the_client(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let c = Client::new();
    let a = register(&c, &base, "lea", Some("fa")).await;
    take_jobs(&pool, "email_verify").await;
    for (lang, want) in cases() {
        let body = with_lang(json!({"email": "lea@example.org"}), lang);
        let r = c.post(format!("http://{base}/api/v1/auth/reset")).json(&body).send().await.unwrap();
        assert_eq!(r.status(), 204, "{lang:?}");
        let jobs = take_jobs(&pool, "email_reset").await;
        assert_eq!(jobs.len(), 1, "{lang:?}");
        assert_eq!(jobs[0], json!({"user_id": a.user.id, "lang": want}), "{lang:?}");
    }
    // Un compte inconnu : même réponse, aucune tâche (le serveur ne dit pas quels comptes existent).
    let r = c
        .post(format!("http://{base}/api/v1/auth/reset"))
        .json(&json!({"email": "personne@example.org", "lang": "fa"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    assert!(take_jobs(&pool, "email_reset").await.is_empty());
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn the_idempotence_key_of_a_reset_is_unchanged_so_a_second_request_keeps_the_queued_job(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let c = Client::new();
    let a = register(&c, &base, "lea", None).await;
    for lang in ["fa", "en"] {
        let r = c
            .post(format!("http://{base}/api/v1/auth/reset"))
            .json(&json!({"email": "lea@example.org", "lang": lang}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 204);
    }
    let rows: Vec<(Option<String>, Value)> =
        sqlx::query_as("select dedupe_key, payload from jobs where kind = 'email_reset'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 1, "une seule tâche en file, comme avant");
    assert_eq!(rows[0].0.as_deref(), Some(format!("reset:{}", a.user.id).as_str()));
    assert_eq!(rows[0].1["lang"], "fa", "la première demande garde sa place");
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn the_confirmation_mail_of_a_new_address_is_queued_in_the_language_of_the_client(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let c = Client::new();
    let a = register(&c, &base, "lea", None).await;
    take_jobs(&pool, "email_verify").await;
    for (i, (lang, want)) in cases().into_iter().enumerate() {
        let body = with_lang(json!({"email": format!("nouvelle{i}@example.org"), "password": PASSWORD}), lang);
        let r = c
            .request(Method::PUT, format!("http://{base}/api/v1/users/@me/email"))
            .header("authorization", format!("Session {}", a.token))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "{lang:?} : {}", r.text().await.unwrap());
        let jobs = take_jobs(&pool, "email_change").await;
        assert_eq!(jobs.len(), 1, "{lang:?}");
        assert_eq!(jobs[0], json!({"user_id": a.user.id, "lang": want}), "{lang:?}");
    }
}

#[sqlx::test(migrator = "kraken_db::MIGRATOR")]
async fn a_lang_of_the_wrong_type_is_refused_like_the_one_of_a_new_server_and_the_field_is_optional(pool: PgPool) {
    let base = spawn(pool.clone()).await;
    let c = Client::new();
    let a = register(&c, &base, "lea", None).await;
    take_jobs(&pool, "email_verify").await;

    // Même réponse que `POST /guilds` : un `lang` qui n'est pas du texte n'est pas une demande valide.
    let guild = c
        .post(format!("http://{base}/api/v1/guilds"))
        .header("authorization", format!("Session {}", a.token))
        .json(&json!({"name": "Récif", "lang": 7}))
        .send()
        .await
        .unwrap()
        .status();
    assert!(guild.is_client_error(), "{guild}");
    let reg = c
        .post(format!("http://{base}/api/v1/auth/register"))
        .json(&json!({"username": "sam", "email": "sam@example.org", "password": PASSWORD, "lang": 7}))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(reg, guild, "même refus pour l'inscription");
    let reset = c
        .post(format!("http://{base}/api/v1/auth/reset"))
        .json(&json!({"email": "lea@example.org", "lang": ["fa"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(reset.status(), guild);
    assert!(take_jobs(&pool, "email_verify").await.is_empty(), "le compte refusé n'a pas été créé");
    assert!(take_jobs(&pool, "email_reset").await.is_empty());
    // Sans `lang`, comme avant : accepté, en français.
    let r = c
        .post(format!("http://{base}/api/v1/auth/register"))
        .json(&json!({"username": "sam", "email": "sam@example.org", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    assert_eq!(take_jobs(&pool, "email_verify").await[0]["lang"], "fr");
}

/// La requête du client (`kraken-proto`) : `lang` est facultatif dans les deux sens, comme `CreateGuild.lang`.
#[test]
fn the_requests_carry_an_optional_lang() {
    let r: RegisterReq =
        serde_json::from_value(json!({"username": "a", "email": "a@example.org", "password": "x"})).unwrap();
    assert_eq!(r.lang, None);
    let r: RegisterReq =
        serde_json::from_value(json!({"username": "a", "email": "a@example.org", "password": "x", "lang": "fa"}))
            .unwrap();
    assert_eq!(r.lang.as_deref(), Some("fa"));
    assert!(
        serde_json::to_value(RegisterReq {
            username: "a".into(),
            email: "a@example.org".into(),
            password: "x".into(),
            lang: None
        })
        .unwrap()
        .get("lang")
        .is_none()
    );
    assert_eq!(
        serde_json::to_value(EmailReq { email: "a@example.org".into(), lang: Some("en".into()) }).unwrap()["lang"],
        "en"
    );
    let e: ChangeEmail = serde_json::from_value(json!({"email": "a@example.org", "password": "x"})).unwrap();
    assert_eq!(e.lang, None);
}
