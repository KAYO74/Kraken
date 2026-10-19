//! Règles de la couche données sur une vraie base (une base neuve par test, via
//! `#[sqlx::test]` et `DATABASE_URL`).

use kraken_proto::{ChannelKind, CreateChannel};
use sqlx::PgPool;

use super::*;

async fn two_users(db: &Db) -> (User, User) {
    let a = db.create_user("lou", "lou@example.org", "h".into()).await.unwrap();
    let b = db.create_user("sam", "sam@example.org", "h".into()).await.unwrap();
    (a, b)
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn accounts_are_unique_and_valid(pool: PgPool) {
    let db = Db::from_pool(pool);
    two_users(&db).await;
    assert_eq!(db.create_user("lou", "x@example.org", "h".into()).await, Err(DbError::Conflict("username")));
    assert_eq!(db.create_user("lou2", "LOU@example.org", "h".into()).await, Err(DbError::Conflict("email")));
    assert_eq!(db.create_user("Lou!", "y@example.org", "h".into()).await, Err(DbError::Invalid("username")));
    assert!(db.user_credentials("LOU@EXAMPLE.ORG").await.unwrap().is_some());
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn outsiders_see_nothing(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    db.create_message(general, a.id, "secret", None).await.unwrap();

    assert_eq!(db.channel_perms(b.id, general).await.unwrap(), 0);
    assert_eq!(db.guild_perms(b.id, g.guild.id).await.unwrap(), 0);
    assert!(db.guild_state(g.guild.id, b.id).await.unwrap().is_none());
    assert!(db.user_guilds(b.id).await.unwrap().is_empty());
    assert_eq!(db.channel_audience(general).await.unwrap(), vec![a.id]);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn invites_messages_and_private_channels(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let gid = g.guild.id;
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;

    let inv = db.create_invite(gid, Some(general), a.id, 1, None).await.unwrap();
    let (_, m) = db.use_invite(&inv.code, b.id).await.unwrap();
    assert!(m.is_some());
    assert!(db.invite(&inv.code).await.unwrap().is_none(), "max_uses atteint");
    // Déjà membre : l'invitation épuisée ne compte plus, mais rien ne casse côté serveur.
    assert_eq!(db.use_invite(&inv.code, b.id).await.map(|(_, m)| m), Ok(None));
    assert_ne!(db.channel_perms(b.id, general).await.unwrap() & perms::SEND_MESSAGES, 0);

    // Idempotence de l'envoi.
    let (m1, new1) = db.create_message(general, b.id, "salut", Some("n1".into())).await.unwrap();
    let (m2, new2) = db.create_message(general, b.id, "salut", Some("n1".into())).await.unwrap();
    assert!(new1 && !new2);
    assert_eq!(m1.id, m2.id);
    for i in 0..5 {
        db.create_message(general, a.id, &format!("m{i}"), None).await.unwrap();
    }
    let page = db.messages(general, None, 3).await.unwrap();
    assert_eq!(page.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), ["m2", "m3", "m4"]);
    let older = db.messages(general, Some(page[0].id), 10).await.unwrap();
    assert_eq!(older.len(), 3);
    assert!(db.create_message(general, b.id, &"x".repeat(4001), None).await.is_err());

    // Salon privé : caché à @everyone.
    let staff = db
        .create_channel(gid, &CreateChannel { name: "staff".into(), kind: ChannelKind::Text, parent_id: None })
        .await
        .unwrap();
    sqlx::query(
        "insert into permission_overwrites (channel_id, target_id, target_kind, deny) values ($1, $2, 'role', $3)",
    )
    .bind(staff.id)
    .bind(gid)
    .bind(perms::VIEW_CHANNEL as i64)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(db.channel_perms(b.id, staff.id).await.unwrap(), 0);
    assert_ne!(db.channel_perms(a.id, staff.id).await.unwrap(), 0, "le propriétaire voit tout");
    let seen = db.guild_state(gid, b.id).await.unwrap().unwrap();
    assert!(!seen.channels.iter().any(|c| c.id == staff.id));
    assert_eq!(db.channel_audience(staff.id).await.unwrap(), vec![a.id]);

    // Banni : l'invitation ne marche plus.
    let inv2 = db.create_invite(gid, None, a.id, 0, Some(3600)).await.unwrap();
    let c = db.create_user("noa", "noa@example.org", "h".into()).await.unwrap();
    sqlx::query("insert into bans (guild_id, user_id) values ($1, $2)")
        .bind(gid)
        .bind(c.id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(db.use_invite(&inv2.code, c.id).await.unwrap_err(), DbError::Forbidden("banned"));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn sessions_tokens_and_deletion(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    db.create_session(a.id, b"t1", "web", "web").await.unwrap();
    db.create_session(a.id, b"t2", "web", "web").await.unwrap();
    assert!(db.session_user(b"t1").await.unwrap().is_some());
    db.revoke_session(b"t1").await.unwrap();
    assert!(db.session_user(b"t1").await.unwrap().is_none());
    assert_eq!(db.revoke_all_sessions(a.id).await.unwrap(), 1);

    db.create_auth_token(a.id, "reset_password", b"r1", 30).await.unwrap();
    assert_eq!(db.use_auth_token("reset_password", b"r1").await.unwrap(), Some(a.id));
    assert_eq!(db.use_auth_token("reset_password", b"r1").await.unwrap(), None, "usage unique");
    assert_eq!(db.use_auth_token("verify_email", b"r1").await.unwrap(), None, "mauvais type");

    // Un propriétaire peut partir : son serveur vide disparaît, l'autre passe au plus ancien membre.
    let c = db.create_user("noa", "noa@example.org", "h".into()).await.unwrap();
    let solo = db.create_guild(b.id, "Récif").await.unwrap().guild.id;
    let shared = db.create_guild(b.id, "Lagon").await.unwrap().guild.id;
    let inv = db.create_invite(shared, None, b.id, 0, None).await.unwrap();
    db.use_invite(&inv.code, a.id).await.unwrap();
    db.use_invite(&inv.code, c.id).await.unwrap();
    db.request_deletion(b.id).await.unwrap();
    let owners: Vec<(Id, Id)> = sqlx::query_as("select id, owner_id from guilds where id = any($1)")
        .bind(vec![solo, shared])
        .fetch_all(&db.pool)
        .await
        .unwrap();
    assert_eq!(owners, vec![(shared, a.id)]);
    assert_eq!(db.create_guild(b.id, "Trop tard").await.err(), Some(DbError::NotFound), "compte fermé");

    db.create_session(a.id, b"t3", "web", "web").await.unwrap();
    db.request_deletion(a.id).await.unwrap();
    assert!(db.session_user(b"t3").await.unwrap().is_none());
    assert!(db.user_credentials("lou").await.unwrap().is_none());
    assert_eq!(db.purge_deleted_accounts(14).await.unwrap(), 0, "délai de grâce");
    assert_eq!(db.purge_deleted_accounts(-1).await.unwrap(), 2, "lou et sam");
    let left: Option<String> =
        sqlx::query_scalar("select email from users where id = $1").bind(a.id).fetch_one(&db.pool).await.unwrap();
    assert!(left.is_none());
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn jobs_retry_then_dead_letter(pool: PgPool) {
    let db = Db::from_pool(pool);
    db.enqueue("email", serde_json::json!({"to": "x"}), Some("k1"), 0).await.unwrap();
    db.enqueue("email", serde_json::json!({"to": "x"}), Some("k1"), 0).await.unwrap();
    let jobs = db.claim_jobs(10).await.unwrap();
    assert_eq!(jobs.len(), 1, "dédoublonné");
    assert!(db.claim_jobs(10).await.unwrap().is_empty(), "verrouillée");
    db.job_failed(&jobs[0], "smtp down", 1).await.unwrap();
    let dead: bool = sqlx::query_scalar("select failed_at is not null from jobs where id = $1")
        .bind(jobs[0].id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(dead);
    // La lettre morte ne bloque pas la même tâche plus tard (mot de passe oublié, purge).
    db.enqueue("email", serde_json::json!({"to": "x"}), Some("k1"), 0).await.unwrap();
    assert_eq!(db.claim_jobs(10).await.unwrap().len(), 1, "remise en file");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn seed_is_idempotent(pool: PgPool) {
    let db = Db::from_pool(pool);
    assert!(db.seed("h").await.unwrap());
    assert!(!db.seed("h").await.unwrap());
    let (demo, _) = db.user_credentials("demo").await.unwrap().unwrap();
    let guilds = db.user_guilds(demo.id).await.unwrap();
    assert_eq!(guilds.len(), 2);
    assert!(!guilds[0].channels.iter().any(|c| c.name.as_deref() == Some("staff")), "staff caché à demo");
    assert_eq!(db.private_channels(demo.id).await.unwrap().len(), 2);
    assert_eq!(db.relationships(demo.id).await.unwrap().len(), 5);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn odd_inputs_and_races(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    // Un caractère nul est une valeur refusée, pas une erreur du serveur.
    assert_eq!(db.create_message(general, a.id, "a\0b", None).await.err(), Some(DbError::Invalid("value")));
    // Le même nonce dans un autre salon est un autre message.
    let other = db
        .create_channel(g.guild.id, &CreateChannel { name: "staff".into(), kind: ChannelKind::Text, parent_id: None })
        .await
        .unwrap();
    db.create_message(general, a.id, "un", Some("n".into())).await.unwrap();
    let (m, new) = db.create_message(other.id, a.id, "deux", Some("n".into())).await.unwrap();
    assert!(new && m.channel_id == other.id && m.content == "deux");
    // Deux adhésions simultanées (double clic, deux onglets) réussissent ; une seule compte.
    let inv = db.create_invite(g.guild.id, Some(general), a.id, 1, None).await.unwrap();
    let (r1, r2) = tokio::join!(db.use_invite(&inv.code, b.id), db.use_invite(&inv.code, b.id));
    let joined = [r1.unwrap().1, r2.unwrap().1];
    assert_eq!(joined.iter().filter(|m| m.is_some()).count(), 1);
}
