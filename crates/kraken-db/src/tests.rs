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
    assert_eq!(db.relationships(demo.id).await.unwrap().len(), 6, "4 amis, 1 demande, 1 bloqué");
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

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn edit_delete_reply_react_and_mention(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let outsider = db.create_user("noa", "noa@example.org", "h".into()).await.unwrap();
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), a.id, 0, None).await.unwrap();
    db.use_invite(&inv.code, b.id).await.unwrap();
    let other = db
        .create_channel(g.guild.id, &CreateChannel { name: "autre".into(), kind: ChannelKind::Text, parent_id: None })
        .await
        .unwrap();

    // Mentions : seulement ceux qui voient le salon ; @everyone seulement avec le droit.
    let draft = |content| Draft { content, ..Default::default() };
    let (m, _) = db.send_message(general, a.id, draft("salut @sam et @noa, @everyone `@lou`")).await.unwrap();
    assert_eq!(m.mentions.iter().map(|u| u.username.as_str()).collect::<Vec<_>>(), ["sam"]);
    assert!(!m.mention_everyone, "pas le droit de mentionner tout le monde");
    let (all, _) = db
        .send_message(
            general,
            a.id,
            Draft { content: "@here réunion", may_mention_everyone: true, ..Default::default() },
        )
        .await
        .unwrap();
    assert!(all.mention_everyone);

    // Réponse : même salon seulement ; l'aperçu suit l'original.
    let (r, _) = db.send_message(general, b.id, Draft { reply_to: Some(m.id), ..draft("oui !") }).await.unwrap();
    let reply = r.reply.clone().unwrap();
    assert_eq!((reply.id, reply.author.unwrap().id), (Some(m.id), a.id));
    assert!(reply.content.starts_with("salut @sam"));
    assert_eq!(
        db.send_message(other.id, b.id, Draft { reply_to: Some(m.id), ..draft("ailleurs") }).await.err(),
        Some(DbError::Invalid("reply_to"))
    );

    // Modifier : l'auteur seulement ; les mentions sont recalculées.
    assert_eq!(db.edit_message(general, m.id, b.id, "pirate", true).await.err(), Some(DbError::Forbidden("author")));
    assert_eq!(db.edit_message(other.id, m.id, a.id, "mauvais salon", false).await.err(), Some(DbError::NotFound));
    assert_eq!(db.edit_message(general, m.id, a.id, "   ", false).await.err(), Some(DbError::Invalid("content")));
    let e = db.edit_message(general, m.id, a.id, " corrigé, sans mention ", false).await.unwrap();
    assert_eq!(e.content, "corrigé, sans mention");
    assert!(e.edited_at.is_some() && e.mentions.is_empty());
    let back = db.messages(general, None, 10).await.unwrap();
    assert_eq!(back.iter().find(|x| x.id == r.id).unwrap().reply.as_ref().unwrap().content, "corrigé, sans mention");

    // Réactions : une par personne et par emoji, emojis seulement, 20 différents au plus.
    assert!(db.add_reaction(general, m.id, b.id, "🐙").await.unwrap());
    assert!(!db.add_reaction(general, m.id, b.id, "🐙").await.unwrap(), "déjà là");
    assert!(db.add_reaction(general, m.id, a.id, "🐙").await.unwrap());
    assert!(db.add_reaction(general, m.id, a.id, "👍").await.unwrap());
    assert_eq!(db.add_reaction(general, m.id, a.id, "ok").await.err(), Some(DbError::Invalid("emoji")));
    assert_eq!(db.add_reaction(other.id, m.id, a.id, "👍").await.err(), Some(DbError::NotFound));
    let seen = db.message(m.id).await.unwrap().unwrap().reactions;
    assert_eq!(
        seen,
        vec![
            Reaction { emoji: "🐙".into(), users: vec![b.id, a.id] },
            Reaction { emoji: "👍".into(), users: vec![a.id] }
        ]
    );
    for (e, _) in kraken_core::emoji::EMOJIS.iter().filter(|(e, _)| !["🐙", "👍"].contains(e)).take(18) {
        db.add_reaction(general, m.id, a.id, e).await.unwrap();
    }
    assert_eq!(db.add_reaction(general, m.id, b.id, "🦀").await.err(), Some(DbError::Invalid("reactions")));
    assert!(db.add_reaction(general, m.id, b.id, "👍").await.unwrap(), "un emoji déjà présent reste possible");
    assert!(db.remove_reaction(general, m.id, b.id, "🐙").await.unwrap());
    assert!(!db.remove_reaction(general, m.id, b.id, "🐙").await.unwrap());

    // Supprimer : l'auteur, ou qui gère les messages ; la réponse garde sa trace.
    assert_eq!(db.delete_message(general, m.id, b.id, false).await.err(), Some(DbError::Forbidden("author")));
    assert_eq!(db.delete_message(general, m.id, outsider.id, false).await.err(), Some(DbError::Forbidden("author")));
    db.delete_message(general, r.id, b.id, false).await.unwrap();
    let (r2, _) = db.send_message(general, b.id, Draft { reply_to: Some(m.id), ..draft("encore") }).await.unwrap();
    db.delete_message(general, m.id, b.id, true).await.unwrap();
    assert_eq!(db.delete_message(general, m.id, a.id, true).await.err(), Some(DbError::NotFound));
    let orphan = db.message(r2.id).await.unwrap().unwrap().reply.unwrap();
    assert_eq!((orphan.id, orphan.author), (None, None));
    let left: i64 = sqlx::query_scalar("select count(*) from reactions").fetch_one(&db.pool).await.unwrap();
    assert_eq!(left, 0, "les réactions partent avec le message");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn friends_requests_and_blocks(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let c = db.create_user("noa", "noa@example.org", "h".into()).await.unwrap();
    let kind = |r: &Relationship| r.kind;

    // Par pseudo, avec ou sans @, casse ignorée ; inconnu et soi-même refusés.
    assert_eq!(db.user_by_username(" @SAM ").await.unwrap().map(|u| u.id), Some(b.id));
    assert!(db.user_by_username("personne").await.unwrap().is_none());
    assert_eq!(db.befriend(a.id, a.id).await.err(), Some(DbError::Invalid("self")));
    assert_eq!(db.befriend(a.id, Id::nil()).await.err(), Some(DbError::NotFound));

    // Demande, doublon, puis acceptation par l'autre : chacun est prévenu.
    let (r, ev) = db.befriend(a.id, b.id).await.unwrap();
    assert_eq!((r.user.id, kind(&r)), (b.id, RelationshipKind::PendingOutgoing));
    assert_eq!(ev.iter().map(|(u, _)| *u).collect::<Vec<_>>(), [a.id, b.id]);
    assert_eq!(db.relationship(b.id, a.id).await.unwrap(), Some(RelationshipKind::PendingIncoming));
    assert_eq!(db.befriend(a.id, b.id).await.err(), Some(DbError::Conflict("already_sent")));
    let (r, _) = db.befriend(b.id, a.id).await.unwrap();
    assert_eq!(kind(&r), RelationshipKind::Friend);
    assert_eq!(db.relationship(a.id, b.id).await.unwrap(), Some(RelationshipKind::Friend));
    assert_eq!(db.befriend(a.id, b.id).await.err(), Some(DbError::Conflict("already_friends")));

    // Deux demandes croisées en même temps : une amitié, pas deux demandes.
    let (x, y) = tokio::join!(db.befriend(a.id, c.id), db.befriend(c.id, a.id));
    assert!(x.is_ok() && y.is_ok());
    assert_eq!(db.relationship(a.id, c.id).await.unwrap(), Some(RelationshipKind::Friend));
    assert_eq!(db.relationship(c.id, a.id).await.unwrap(), Some(RelationshipKind::Friend));

    // Retirer : les deux côtés ; recommencer : rien à retirer.
    let ev = db.remove_relationship(a.id, c.id).await.unwrap();
    assert_eq!(ev.len(), 2);
    assert_eq!(db.relationship(c.id, a.id).await.unwrap(), None);
    assert_eq!(db.remove_relationship(a.id, c.id).await.err(), Some(DbError::NotFound));

    // Bloquer un ami : l'amitié disparaît chez lui aussi ; il ne peut plus rien demander.
    let (r, ev) = db.block(a.id, b.id).await.unwrap();
    assert_eq!(kind(&r), RelationshipKind::Blocked);
    assert!(ev.contains(&(b.id, Event::RelationshipRemove { user_id: a.id })));
    assert_eq!(db.relationship(b.id, a.id).await.unwrap(), None);
    assert_eq!(db.blocks(a.id, b.id).await.unwrap(), (true, false));
    assert_eq!(db.befriend(b.id, a.id).await.err(), Some(DbError::Forbidden("request_refused")));
    assert_eq!(db.befriend(a.id, b.id).await.err(), Some(DbError::Forbidden("blocked")));
    assert_eq!(db.block(a.id, b.id).await.unwrap().1, vec![], "bloquer deux fois ne change rien");
    // Lui peut me bloquer aussi ; me débloquer ne touche pas son blocage.
    db.block(b.id, a.id).await.unwrap();
    assert_eq!(
        db.remove_relationship(a.id, b.id).await.unwrap(),
        vec![(a.id, Event::RelationshipRemove { user_id: b.id })]
    );
    assert_eq!(db.blocks(a.id, b.id).await.unwrap(), (false, true));
    db.remove_relationship(b.id, a.id).await.unwrap();
    assert!(db.relationships(a.id).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn direct_and_group_messages(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let mut friends = vec![b.clone()];
    for i in 0..9 {
        friends.push(db.create_user(&format!("ami{i}"), &format!("ami{i}@example.org"), "h".into()).await.unwrap());
    }
    let stranger = db.create_user("inconnu", "inconnu@example.org", "h".into()).await.unwrap();
    for f in &friends {
        db.befriend(a.id, f.id).await.unwrap();
        db.befriend(f.id, a.id).await.unwrap();
    }

    // MP : un seul par paire, rouvert ; pas avec un inconnu sans serveur commun.
    let (dm, created) = db.open_dm(a.id, b.id).await.unwrap();
    assert!(created);
    assert_eq!(db.open_dm(b.id, a.id).await.unwrap(), (dm, false));
    assert_eq!(db.open_dm(a.id, stranger.id).await.err(), Some(DbError::Forbidden("dm_stranger")));
    assert_eq!(db.open_dm(a.id, a.id).await.err(), Some(DbError::Invalid("self")));
    let seen = db.private_channel(dm, a.id).await.unwrap().unwrap();
    assert_eq!(seen.recipients.iter().map(|u| u.id).collect::<Vec<_>>(), [b.id]);
    assert!(db.private_channel(dm, stranger.id).await.unwrap().is_none());
    db.create_message(dm, b.id, "coucou", None).await.unwrap();

    // Bloqué : plus de message dans le MP, dans aucun sens, ni de nouveau MP.
    db.block(a.id, b.id).await.unwrap();
    assert_eq!(db.create_message(dm, b.id, "encore", None).await.err(), Some(DbError::Forbidden("dm_refused")));
    assert_eq!(db.create_message(dm, a.id, "non", None).await.err(), Some(DbError::Forbidden("dm_blocked")));
    assert_eq!(db.open_dm(b.id, a.id).await.err(), Some(DbError::Forbidden("dm_refused")));
    assert_eq!(db.messages(dm, None, 10).await.unwrap().len(), 1, "l'historique reste");
    db.remove_relationship(a.id, b.id).await.unwrap();
    db.create_message(dm, b.id, "de retour", None).await.unwrap();
    assert_eq!(db.relationship(a.id, b.id).await.unwrap(), None, "le blocage a défait l'amitié");
    db.befriend(a.id, b.id).await.unwrap();
    db.befriend(b.id, a.id).await.unwrap();

    // Groupe : entre amis seulement, 10 personnes au plus.
    let ids: Vec<Id> = friends.iter().skip(1).map(|u| u.id).collect();
    assert_eq!(db.create_group(a.id, &ids[..1], None).await.err(), Some(DbError::Invalid("recipients")));
    assert_eq!(db.create_group(a.id, &[ids[0], stranger.id], None).await.err(), Some(DbError::Forbidden("not_friend")));
    let mut all = ids.clone();
    all.push(stranger.id);
    assert_eq!(db.create_group(a.id, &all, None).await.err(), Some(DbError::Invalid("recipients")), "11 personnes");
    let g = db.create_group(a.id, &ids[..8], Some("  La bande  ")).await.unwrap();
    let seen = db.private_channel(g, ids[0]).await.unwrap().unwrap();
    assert_eq!((seen.kind, seen.name.as_deref(), seen.recipients.len()), (ChannelKind::GroupDm, Some("La bande"), 8));
    assert_ne!(db.channel_perms(ids[0], g).await.unwrap() & perms::SEND_MESSAGES, 0);

    // Renommer : tout membre ; trop long refusé ; vide efface le nom ; un intrus ne voit rien.
    db.rename_group(g, ids[1], Some("Les abysses")).await.unwrap();
    assert_eq!(db.rename_group(g, ids[1], Some(&"x".repeat(101))).await.err(), Some(DbError::Invalid("name")));
    assert_eq!(db.rename_group(g, stranger.id, Some("pirate")).await.err(), Some(DbError::NotFound));
    assert_eq!(db.rename_group(dm, a.id, Some("pas un groupe")).await.err(), Some(DbError::Invalid("kind")));
    db.rename_group(g, a.id, Some("   ")).await.unwrap();
    assert_eq!(db.channel(g).await.unwrap().unwrap().name, None);

    // Ajouter : un ami de qui ajoute, jusqu'à 10 ; déjà là, rien ne change.
    assert_eq!(db.add_recipient(g, ids[0], ids[8]).await.err(), Some(DbError::Forbidden("not_friend")));
    assert!(db.add_recipient(g, a.id, ids[8]).await.unwrap());
    assert!(!db.add_recipient(g, a.id, ids[8]).await.unwrap());
    assert_eq!(db.add_recipient(g, a.id, b.id).await.err(), Some(DbError::Invalid("group_full")));
    assert_eq!(db.add_recipient(g, stranger.id, b.id).await.err(), Some(DbError::NotFound));

    // Retirer : le propriétaire seulement ; quitter : chacun ; le propriétaire parti, un autre le devient.
    assert_eq!(db.remove_recipient(g, ids[0], ids[1]).await.err(), Some(DbError::Forbidden("owner")));
    assert_eq!(db.remove_recipient(g, a.id, ids[1]).await.unwrap().len(), 9);
    assert_eq!(db.channel_perms(ids[1], g).await.unwrap(), 0);
    let left = db.remove_recipient(g, a.id, a.id).await.unwrap();
    assert_eq!(left[0], ids[0]);
    assert_eq!(db.remove_recipient(g, ids[0], ids[2]).await.unwrap().len(), 7, "nouveau propriétaire");
    for u in db.channel_audience(g).await.unwrap() {
        db.remove_recipient(g, u, u).await.unwrap();
    }
    assert!(db.channel(g).await.unwrap().is_none(), "le dernier parti, le groupe disparaît");
}
