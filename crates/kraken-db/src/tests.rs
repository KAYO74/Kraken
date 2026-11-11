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
        .create_channel(gid, a.id, &CreateChannel { name: "staff".into(), kind: ChannelKind::Text, parent_id: None })
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
    assert_eq!(db.purge_deleted_accounts(14).await.unwrap().0, 0, "délai de grâce");
    assert_eq!(db.purge_deleted_accounts(-1).await.unwrap().0, 2, "lou et sam");
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
        .create_channel(
            g.guild.id,
            a.id,
            &CreateChannel { name: "staff".into(), kind: ChannelKind::Text, parent_id: None },
        )
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
        .create_channel(
            g.guild.id,
            a.id,
            &CreateChannel { name: "autre".into(), kind: ChannelKind::Text, parent_id: None },
        )
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

/// Envoie un message comme le serveur : enregistré, puis compté chez ceux qu'il mentionne.
async fn post(db: &Db, cid: Id, author: Id, d: Draft<'_>) -> Message {
    let (m, _) = db.send_message(cid, author, d).await.unwrap();
    let guild = db.channel(cid).await.unwrap().unwrap().guild_id;
    let audience = db.channel_audience(cid).await.unwrap();
    db.record_message(&m, guild, &audience).await.unwrap();
    m
}

async fn state(db: &Db, uid: Id, cid: Id) -> (Option<Id>, u32) {
    let s = db.read_state(uid, cid).await.unwrap();
    (s.last_read, s.mention_count)
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn unread_and_mention_counts(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let c = db.create_user("noa", "noa@example.org", "h".into()).await.unwrap();
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), a.id, 0, None).await.unwrap();
    db.use_invite(&inv.code, b.id).await.unwrap();
    db.use_invite(&inv.code, c.id).await.unwrap();
    let text = |content| Draft { content, ..Default::default() };

    // Un message ordinaire : non lu chez les autres, sans mention ; lu chez son auteur.
    let m1 = post(&db, general, a.id, text("bonjour")).await;
    assert_eq!(state(&db, a.id, general).await, (Some(m1.id), 0));
    assert_eq!(state(&db, b.id, general).await, (None, 0));
    let ch = db.channel(general).await.unwrap().unwrap();
    assert_eq!(ch.last_message_id, Some(m1.id));

    // @pseudo, réponse à mon message, @everyone (avec le droit) : chacun compte.
    post(&db, general, a.id, text("@sam tu es là ?")).await;
    let mine = post(&db, general, b.id, text("oui")).await;
    assert_eq!(state(&db, b.id, general).await, (Some(mine.id), 0), "écrire vaut lecture");
    post(&db, general, a.id, Draft { reply_to: Some(mine.id), ..text("parfait") }).await;
    post(&db, general, a.id, Draft { may_mention_everyone: true, ..text("@everyone réunion") }).await;
    post(&db, general, a.id, text("@everyone sans le droit")).await;
    assert_eq!(state(&db, b.id, general).await.1, 2, "réponse + @everyone");
    assert_eq!(state(&db, c.id, general).await.1, 1, "seul @everyone compte pour noa");

    // Lire jusqu'au bout remet à zéro ; lire une partie garde les mentions plus récentes.
    let last = db.channel(general).await.unwrap().unwrap().last_message_id.unwrap();
    let r = db.ack(c.id, general, mine.id).await.unwrap();
    assert_eq!((r.last_read, r.mention_count), (Some(mine.id), 1));
    let r = db.ack(c.id, general, last).await.unwrap();
    assert_eq!((r.last_read, r.mention_count), (Some(last), 0));
    let r = db.ack(c.id, general, m1.id).await.unwrap();
    assert_eq!(r.last_read, Some(last), "jamais en arrière");
    assert_eq!(db.ack(c.id, general, Id::nil()).await.err(), Some(DbError::NotFound));

    // Ignorer @everyone dans ce serveur : il ne compte plus.
    let set = |guild_id, channel_id, level, suppress, mute| SetNotify {
        guild_id,
        channel_id,
        level,
        suppress_everyone: suppress,
        mute,
    };
    db.set_notify(c.id, &set(Some(g.guild.id), None, None, Some(true), None)).await.unwrap();
    post(&db, general, a.id, Draft { may_mention_everyone: true, ..text("@here encore") }).await;
    assert_eq!(state(&db, c.id, general).await.1, 0, "@everyone ignoré");
    post(&db, general, a.id, Draft { may_mention_everyone: true, ..text("@noa toi oui") }).await;
    assert_eq!(state(&db, c.id, general).await.1, 1);
    // La sourdine n'arrête pas le compteur.
    db.set_notify(c.id, &set(None, Some(general), None, None, Some(Mute::Forever))).await.unwrap();
    let m = post(&db, general, a.id, text("@noa en sourdine")).await;
    assert_eq!(state(&db, c.id, general).await.1, 2);
    // Supprimer un message pas encore lu le retire du compteur.
    db.delete_message(general, m.id, a.id, false).await.unwrap();
    assert_eq!(
        db.recount_after_delete(general, m.id).await.unwrap(),
        vec![(c.id, ReadState { channel_id: general, last_read: Some(last), mention_count: 1 })]
    );
    assert_ne!(db.channel(general).await.unwrap().unwrap().last_message_id, Some(m.id));
    // Niveau « rien » sur le salon : plus de compteur.
    let s = db.set_notify(c.id, &set(None, Some(general), Some(NotifyLevel::Nothing), None, None)).await.unwrap();
    assert_eq!((s.guild_id, s.level), (Some(g.guild.id), NotifyLevel::Nothing));
    post(&db, general, a.id, text("@noa et maintenant ?")).await;
    assert_eq!(state(&db, c.id, general).await.1, 1, "niveau « rien »");

    // Un compte qui a bloqué l'auteur n'est pas compté.
    db.block(b.id, a.id).await.unwrap();
    let before = state(&db, b.id, general).await.1;
    post(&db, general, a.id, text("@sam bloqué")).await;
    assert_eq!(state(&db, b.id, general).await.1, before);

    // Tout le serveur lu d'un coup.
    let states = db.ack_guild(c.id, g.guild.id).await.unwrap().unwrap();
    assert!(states.iter().all(|s| s.mention_count == 0));
    assert_eq!(state(&db, c.id, general).await.1, 0);
    assert_eq!(db.ack_guild(Id::nil(), g.guild.id).await.unwrap(), None, "pas membre");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn private_messages_all_count_and_settings_persist(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), a.id, 0, None).await.unwrap();
    db.use_invite(&inv.code, b.id).await.unwrap();
    let (dm, _) = db.open_dm(a.id, b.id).await.unwrap();
    let text = |content| Draft { content, ..Default::default() };
    post(&db, dm, a.id, text("coucou")).await;
    post(&db, dm, a.id, text("tu dors ?")).await;
    assert_eq!(state(&db, b.id, dm).await.1, 2, "tout message d'un MP compte");
    assert_eq!(state(&db, a.id, dm).await.1, 0);

    // Réglages : un par serveur, un par salon, mis à jour en place, gardés entre appareils.
    let mute = |m| SetNotify { guild_id: Some(g.guild.id), mute: Some(m), ..Default::default() };
    let s = db.set_notify(b.id, &mute(Mute::Minutes(60))).await.unwrap();
    assert_eq!(s.level, NotifyLevel::Default);
    assert!(s.muted_until.is_some_and(|u| u > 0 && u < MUTED_FOREVER));
    let s = db.set_notify(b.id, &mute(Mute::Forever)).await.unwrap();
    assert_eq!(s.muted_until, Some(MUTED_FOREVER));
    let s = db
        .set_notify(
            b.id,
            &SetNotify { guild_id: Some(g.guild.id), level: Some(NotifyLevel::All), ..Default::default() },
        )
        .await
        .unwrap();
    assert_eq!((s.level, s.muted_until), (NotifyLevel::All, Some(MUTED_FOREVER)), "la sourdine reste");
    let s = db.set_notify(b.id, &mute(Mute::Off)).await.unwrap();
    assert_eq!(s.muted_until, None);
    assert_eq!(db.set_notify(b.id, &mute(Mute::Minutes(0))).await.err(), Some(DbError::Invalid("mute")));
    assert_eq!(db.set_notify(b.id, &SetNotify::default()).await.err(), Some(DbError::Invalid("target")));
    let both = SetNotify { guild_id: Some(g.guild.id), channel_id: Some(general), ..Default::default() };
    assert_eq!(db.set_notify(b.id, &both).await.err(), Some(DbError::Invalid("target")));
    db.set_notify(b.id, &SetNotify { channel_id: Some(dm), mute: Some(Mute::Minutes(15)), ..Default::default() })
        .await
        .unwrap();
    let all = db.notify_settings(b.id).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].channel_id, Some(dm));
    assert_eq!(all[1].guild_id, None);
    // Un salon revenu au réglage de son serveur, sans sourdine, n'a plus de ligne.
    db.set_notify(b.id, &SetNotify { channel_id: Some(dm), mute: Some(Mute::Off), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(db.notify_settings(b.id).await.unwrap().len(), 1);
    assert!(db.notify_settings(a.id).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn attachments_belong_to_their_author_and_channel_and_leave_with_the_message(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let other = db
        .create_channel(
            g.guild.id,
            a.id,
            &CreateChannel { name: "autre".into(), kind: ChannelKind::Text, parent_id: None },
        )
        .await
        .unwrap()
        .id;
    let new = |channel_id, uploader_id, key: &'static str, dims| NewAttachment {
        channel_id,
        uploader_id,
        filename: "f.png",
        mime: "image/png",
        size: 10,
        storage_key: key,
        dims,
        preview: dims.is_some(),
    };
    let img = db.create_attachment(new(general, a.id, "k1", Some((4, 3)))).await.unwrap();
    let txt = db.create_attachment(new(general, a.id, "k2", None)).await.unwrap();
    let elsewhere = db.create_attachment(new(other, a.id, "k3", None)).await.unwrap();
    let theirs = db.create_attachment(new(general, b.id, "k4", None)).await.unwrap();
    let refs = |ids: &[Id]| ids.iter().map(|id| AttachmentRef { id: *id, alt: None }).collect::<Vec<_>>();

    // Ni le fichier d'un autre, ni celui d'un autre salon, ni 11 fichiers, ni un doublon.
    for bad in [vec![theirs], vec![elsewhere], vec![img, img]] {
        let r = refs(&bad);
        let d = Draft { content: "x", attachments: &r, ..Default::default() };
        assert_eq!(db.send_message(general, a.id, d).await.err(), Some(DbError::Invalid("attachments")));
    }
    let many = refs(&[img; 11]);
    assert_eq!(
        db.send_message(general, a.id, Draft { content: "x", attachments: &many, ..Default::default() }).await.err(),
        Some(DbError::Invalid("attachments"))
    );
    // Sans texte ni fichier : refusé ; avec fichiers seuls : accepté, texte alternatif gardé.
    assert!(db.send_message(general, a.id, Draft { content: " ", ..Default::default() }).await.is_err());
    let r = vec![AttachmentRef { id: img, alt: Some(" un dessin ".into()) }, AttachmentRef { id: txt, alt: None }];
    let (m, _) = db
        .send_message(
            general,
            a.id,
            Draft { content: "", nonce: Some("n".into()), attachments: &r, ..Default::default() },
        )
        .await
        .unwrap();
    assert_eq!(m.attachments.len(), 2);
    assert_eq!((m.attachments[0].width, m.attachments[0].alt.as_deref()), (Some(4), Some("un dessin")));
    // Renvoi du même nonce : le même message, sans erreur.
    let (again, created) = db
        .send_message(
            general,
            a.id,
            Draft { content: "", nonce: Some("n".into()), attachments: &r, ..Default::default() },
        )
        .await
        .unwrap();
    assert!(!created && again.id == m.id);
    // Les envois non joints se comptent, puis partent avec le ménage.
    assert_eq!(db.pending_attachments(a.id).await.unwrap(), 1);
    assert!(db.purge_orphan_attachments(1).await.unwrap().is_empty(), "trop récents");
    sqlx::query("update attachments set created_at = now() - interval '2 hours' where message_id is null")
        .execute(&db.pool)
        .await
        .unwrap();
    let mut gone = db.purge_orphan_attachments(1).await.unwrap();
    gone.sort();
    assert_eq!(gone, vec!["attachments/k3".to_string(), "attachments/k4".to_string()]);
    // Supprimer le message rend les chemins de ses fichiers (l'original et l'aperçu de l'image).
    let mut files = db.delete_message(general, m.id, a.id, false).await.unwrap();
    files.sort();
    assert_eq!(files, vec!["attachments/k1", "attachments/k1.p", "attachments/k2"]);
    let left: i64 = sqlx::query_scalar("select count(*) from attachments").fetch_one(&db.pool).await.unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn two_sends_of_the_same_file_leave_one_message(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, _) = two_users(&db).await;
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    for round in 0..10 {
        let key: &'static str = Box::leak(format!("race{round}").into_boxed_str());
        let f = db
            .create_attachment(NewAttachment {
                channel_id: general,
                uploader_id: a.id,
                filename: "f.txt",
                mime: "text/plain",
                size: 1,
                storage_key: key,
                dims: None,
                preview: false,
            })
            .await
            .unwrap();
        let r = vec![AttachmentRef { id: f, alt: None }];
        // Deux nonces différents à chaque tour : ce ne sont pas des renvois.
        let send = |n: &str| {
            let d = Draft { content: "", nonce: Some(format!("{n}{round}")), attachments: &r, ..Default::default() };
            db.send_message(general, a.id, d)
        };
        let (x, y) = tokio::join!(send("x"), send("y"));
        assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1, "un seul envoi gagne");
    }
    // Aucun message vide : chacun porte son fichier.
    let empty: i64 = sqlx::query_scalar(
        "select count(*) from messages m where content = '' and not exists (select 1 from attachments a where a.message_id = m.id)",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(empty, 0);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn account_purge_returns_uploaded_files(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let g = db.create_guild(a.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    for (who, key) in [(a.id, "ka"), (b.id, "kb")] {
        db.create_attachment(NewAttachment {
            channel_id: general,
            uploader_id: who,
            filename: "f",
            mime: "text/plain",
            size: 1,
            storage_key: key,
            dims: None,
            preview: false,
        })
        .await
        .unwrap();
    }
    db.request_deletion(b.id).await.unwrap();
    sqlx::query("update users set deletion_requested_at = now() - interval '30 days' where id = $1")
        .bind(b.id)
        .execute(&db.pool)
        .await
        .unwrap();
    let (n, _, files) = db.purge_deleted_accounts(14).await.unwrap();
    assert_eq!((n, files), (1, vec!["attachments/kb".to_string()]));
    let left: i64 = sqlx::query_scalar("select count(*) from attachments").fetch_one(&db.pool).await.unwrap();
    assert_eq!(left, 1);
}

// ───────── modération : rôles, hiérarchie, exclusion, bannissement, départ ─────────

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

/// Un serveur à `owner`, avec `others` dedans.
async fn guild_with(db: &Db, owner: &User, others: &[&User]) -> Id {
    let g = db.create_guild(owner.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), owner.id, 0, None).await.unwrap();
    for u in others {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    g.guild.id
}

fn role(name: &str, permissions: u64) -> CreateRole {
    CreateRole { name: name.into(), color: None, hoist: false, permissions }
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn role_crud_and_order(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (a, b) = two_users(&db).await;
    let gid = guild_with(&db, &a, &[&b]).await;

    let r1 = db
        .create_role(gid, a.id, &CreateRole { color: Some(3), hoist: true, ..role("Modos", perms::KICK) })
        .await
        .unwrap();
    let r2 = db.create_role(gid, a.id, &role("Vétérans", 0)).await.unwrap();
    let r3 = db.create_role(gid, a.id, &role("Invités", 0)).await.unwrap();
    // Un nouveau rôle se place juste au-dessus de @everyone : le dernier créé est le plus bas.
    let order = |roles: Vec<Role>| roles.into_iter().filter(|r| r.id != gid).map(|r| r.name).rev().collect::<Vec<_>>();
    assert_eq!(order(db.roles(gid).await.unwrap()), ["Modos", "Vétérans", "Invités"]);
    assert_eq!((r1.color, r1.hoist, r1.position), (Some(3), true, 1));

    // Renommer, recolorer, retirer la couleur ; entrées invalides refusées.
    let e = db
        .edit_role(gid, r2.id, a.id, &EditRole { name: Some(" Anciens ".into()), color: Some(5), ..Default::default() })
        .await
        .unwrap();
    assert_eq!((e.name.as_str(), e.color), ("Anciens", Some(5)));
    let e = db.edit_role(gid, r2.id, a.id, &EditRole { color: Some(0), ..Default::default() }).await.unwrap();
    assert_eq!(e.color, None);
    let edit = |r: &Role, e: EditRole| {
        let (db, rid) = (db.clone(), r.id);
        async move { db.edit_role(gid, rid, a.id, &e).await }
    };
    assert_eq!(
        edit(&r2, EditRole { color: Some(ROLE_COLORS + 1), ..Default::default() }).await,
        Err(DbError::Invalid("color"))
    );
    assert_eq!(
        edit(&r2, EditRole { name: Some("  ".into()), ..Default::default() }).await,
        Err(DbError::Invalid("role_name"))
    );
    assert_eq!(
        edit(&r2, EditRole { permissions: Some(1 << 60), ..Default::default() }).await,
        Err(DbError::Invalid("permissions"))
    );
    // @everyone : ses droits seulement.
    let ev = db
        .edit_role(gid, gid, a.id, &EditRole { permissions: Some(perms::VIEW_CHANNEL), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(ev.permissions, perms::VIEW_CHANNEL);
    assert_eq!(
        db.edit_role(gid, gid, a.id, &EditRole { name: Some("x".into()), ..Default::default() }).await,
        Err(DbError::Invalid("everyone"))
    );

    // Réordonner : le plus haut d'abord ; seuls les rôles déplacés sont rendus.
    let changed = db.reorder_roles(gid, a.id, &[r3.id, r2.id, r1.id]).await.unwrap();
    assert_eq!(changed.len(), 2, "le rôle du milieu ne bouge pas");
    assert_eq!(order(db.roles(gid).await.unwrap()), ["Invités", "Anciens", "Modos"]);
    assert_eq!(db.reorder_roles(gid, a.id, &[r3.id, r2.id]).await, Err(DbError::Invalid("roles")), "il en manque un");
    assert_eq!(db.reorder_roles(gid, a.id, &[r3.id, r3.id, r1.id]).await, Err(DbError::Invalid("roles")));

    // Suppression : les positions se resserrent, ceux qui l'avaient le perdent.
    db.set_member_role(gid, a.id, b.id, r3.id, true).await.unwrap();
    db.delete_role(gid, r3.id, a.id).await.unwrap();
    let roles = db.roles(gid).await.unwrap();
    assert_eq!(roles.iter().map(|r| r.position).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(db.members(gid).await.unwrap().iter().all(|m| m.roles.is_empty()));
    assert_eq!(db.delete_role(gid, gid, a.id).await, Err(DbError::Invalid("everyone")));
    assert_eq!(db.delete_role(gid, r3.id, a.id).await, Err(DbError::NotFound));
    let audited: i64 = sqlx::query_scalar("select count(*) from audit_log where guild_id = $1")
        .bind(gid)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(audited >= 8);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn role_hierarchy_binds_everyone_but_the_owner(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mod_) = two_users(&db).await;
    let (adm, low) = (user(&db, "adm").await, user(&db, "low").await);
    let gid = guild_with(&db, &own, &[&mod_, &adm, &low]).await;
    let all = perms::MANAGE_ROLES | perms::KICK | perms::BAN;
    let high = db.create_role(gid, own.id, &role("Haut", all)).await.unwrap();
    let mid = db.create_role(gid, own.id, &role("Moyen", all)).await.unwrap();
    let small = db.create_role(gid, own.id, &role("Petit", 0)).await.unwrap();
    // Ordre voulu : Haut > Moyen > Petit.
    db.reorder_roles(gid, own.id, &[high.id, mid.id, small.id]).await.unwrap();
    let adm_role = db.create_role(gid, own.id, &role("Admin", perms::ADMINISTRATOR)).await.unwrap();
    db.reorder_roles(gid, own.id, &[high.id, mid.id, small.id, adm_role.id]).await.unwrap();
    db.set_member_role(gid, own.id, mod_.id, mid.id, true).await.unwrap();
    db.set_member_role(gid, own.id, adm.id, adm_role.id, true).await.unwrap();

    // `mod_` (Moyen) : peut donner Petit, pas Moyen (égal) ni Haut (supérieur).
    assert!(db.set_member_role(gid, mod_.id, low.id, small.id, true).await.is_ok());
    for r in [mid.id, high.id] {
        assert_eq!(
            db.set_member_role(gid, mod_.id, low.id, r, true).await.map(|_| ()),
            Err(DbError::Forbidden("hierarchy"))
        );
    }
    // Il ne peut pas toucher à un rang égal ou supérieur, ni au propriétaire, ni à lui-même.
    let owner_hit = db.kick_member(gid, mod_.id, own.id, None).await;
    assert_eq!(owner_hit, Err(DbError::Forbidden("hierarchy")));
    db.set_member_role(gid, own.id, low.id, high.id, true).await.unwrap();
    assert_eq!(db.kick_member(gid, mod_.id, low.id, None).await, Err(DbError::Forbidden("hierarchy")), "Haut > Moyen");
    assert_eq!(
        db.ban_member(gid, mod_.id, low.id, &Default::default()).await.map(|_| ()),
        Err(DbError::Forbidden("hierarchy"))
    );
    assert_eq!(db.kick_member(gid, mod_.id, mod_.id, None).await, Err(DbError::Invalid("self_target")));
    // Rôle ADMINISTRATOR (au rang le plus bas) : tous les droits, mais pas de passe-droit hiérarchique.
    assert_eq!(db.kick_member(gid, adm.id, low.id, None).await, Err(DbError::Forbidden("hierarchy")));
    assert_eq!(db.kick_member(gid, adm.id, mod_.id, None).await, Err(DbError::Forbidden("hierarchy")));
    // Ni créer un rôle avec plus de droits que soi, ni modifier un rôle plus haut.
    assert_eq!(db.create_role(gid, mod_.id, &role("X", perms::MANAGE_GUILD)).await, Err(DbError::Forbidden("perm")));
    assert_eq!(
        db.edit_role(gid, high.id, mod_.id, &EditRole { name: Some("Pirate".into()), ..Default::default() }).await,
        Err(DbError::Forbidden("hierarchy"))
    );
    assert_eq!(db.delete_role(gid, mid.id, mod_.id).await, Err(DbError::Forbidden("hierarchy")), "rôle de même rang");
    // Réordonner : le haut de la liste, jusqu'à son propre rang, est figé.
    let swap = db.reorder_roles(gid, mod_.id, &[mid.id, high.id, small.id, adm_role.id]).await;
    assert_eq!(swap, Err(DbError::Forbidden("hierarchy")));
    let below = db.reorder_roles(gid, mod_.id, &[high.id, mid.id, adm_role.id, small.id]).await;
    assert_eq!(below.map(|c| c.len()), Ok(2), "permuter deux rôles plus bas que soi est permis");

    // Le propriétaire n'est lié par rien, même pas à lui-même.
    assert!(db.set_member_role(gid, own.id, own.id, high.id, true).await.is_ok());
    assert!(db.kick_member(gid, own.id, low.id, Some("test")).await.is_ok());
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn permission_denials(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, plain) = two_users(&db).await;
    let out = user(&db, "out").await;
    let gid = guild_with(&db, &own, &[&plain]).await;
    let r = db.create_role(gid, own.id, &role("R", 0)).await.unwrap();
    // Un simple membre n'a aucun de ces droits (403) ; un étranger ne sait même pas que le serveur existe (404).
    let denied = Err::<(), _>(DbError::Forbidden("perm"));
    assert_eq!(db.create_role(gid, plain.id, &role("X", 0)).await.map(|_| ()), denied);
    assert_eq!(db.edit_role(gid, r.id, plain.id, &EditRole::default()).await.map(|_| ()), denied);
    assert_eq!(db.delete_role(gid, r.id, plain.id).await, denied);
    assert_eq!(db.reorder_roles(gid, plain.id, &[r.id]).await.map(|_| ()), denied);
    assert_eq!(db.set_member_role(gid, plain.id, plain.id, r.id, true).await.map(|_| ()), denied);
    assert_eq!(db.kick_member(gid, plain.id, own.id, None).await, denied);
    assert_eq!(db.ban_member(gid, plain.id, own.id, &Default::default()).await.map(|_| ()), denied);
    assert_eq!(db.unban(gid, plain.id, own.id).await, denied);
    assert_eq!(db.bans(gid, plain.id).await.map(|_| ()), denied);
    assert_eq!(db.transfer_ownership(gid, plain.id, plain.id).await, Err(DbError::Forbidden("owner_only")));
    assert_eq!(db.create_role(gid, out.id, &role("X", 0)).await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.kick_member(gid, out.id, plain.id, None).await, Err(DbError::NotFound));
    assert_eq!(db.bans(gid, out.id).await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.leave_guild(gid, out.id).await, Err(DbError::NotFound));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn kick_ban_unban_and_rejoin(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, a) = two_users(&db).await;
    let b = user(&db, "bob").await;
    let g = db.create_guild(own.id, "Abysse").await.unwrap();
    let gid = g.guild.id;
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(gid, Some(general), own.id, 0, None).await.unwrap();
    for u in [&a, &b] {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }

    // Exclusion : retiré, mais il peut revenir par invitation.
    db.kick_member(gid, own.id, a.id, Some("  bruyant ")).await.unwrap();
    assert!(db.guild_state(gid, a.id).await.unwrap().is_none());
    assert!(db.use_invite(&inv.code, a.id).await.unwrap().1.is_some());
    assert_eq!(
        db.kick_member(gid, own.id, a.id, Some(&"x".repeat(MAX_REASON + 1))).await,
        Err(DbError::Invalid("reason"))
    );
    let reason: Option<String> = sqlx::query_scalar("select reason from audit_log where action = 'member_kick'")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(reason.as_deref(), Some("bruyant"));

    // Bannissement avec effacement des messages récents (les anciens restent).
    let (old, _) = db.create_message(general, b.id, "ancien", None).await.unwrap();
    sqlx::query("update messages set created_at = now() - interval '3 days' where id = $1")
        .bind(old.id)
        .execute(&db.pool)
        .await
        .unwrap();
    db.create_message(general, b.id, "récent", None).await.unwrap();
    db.create_message(general, own.id, "du chef", None).await.unwrap();
    let out = db
        .ban_member(gid, own.id, b.id, &BanMember { reason: Some("spam".into()), delete_hours: Some(24) })
        .await
        .unwrap();
    assert!(out.was_member);
    assert_eq!((out.ban.user.id, out.ban.reason.as_deref(), out.purged.len()), (b.id, Some("spam"), 1));
    let left: Vec<String> = db.messages(general, None, 10).await.unwrap().into_iter().map(|m| m.content).collect();
    assert_eq!(left.len(), 2);
    assert!(left.contains(&"ancien".to_string()) && left.contains(&"du chef".to_string()));
    assert!(db.guild_state(gid, b.id).await.unwrap().is_none());
    assert_eq!(db.use_invite(&inv.code, b.id).await.map(|_| ()), Err(DbError::Forbidden("banned")));
    assert_eq!(db.bans(gid, own.id).await.unwrap().iter().map(|b| b.user.id).collect::<Vec<_>>(), [b.id]);
    assert_eq!(
        db.ban_member(gid, own.id, b.id, &BanMember { delete_hours: Some(MAX_PURGE_HOURS + 1), ..Default::default() })
            .await
            .map(|_| ()),
        Err(DbError::Invalid("hours"))
    );

    // Bannir quelqu'un qui n'est pas (ou plus) là marche aussi ; jamais soi-même ni le propriétaire.
    let ghost = user(&db, "ghost").await;
    assert!(!db.ban_member(gid, own.id, ghost.id, &Default::default()).await.unwrap().was_member);
    assert_eq!(
        db.ban_member(gid, own.id, own.id, &Default::default()).await.map(|_| ()),
        Err(DbError::Invalid("self_target"))
    );
    assert_eq!(db.ban_member(gid, own.id, Id::nil(), &Default::default()).await.map(|_| ()), Err(DbError::NotFound));

    // Débannir : il peut rejoindre de nouveau ; une seconde fois, il n'y a plus rien.
    db.unban(gid, own.id, b.id).await.unwrap();
    assert_eq!(db.unban(gid, own.id, b.id).await, Err(DbError::NotFound));
    assert!(db.use_invite(&inv.code, b.id).await.unwrap().1.is_some());
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn leaving_and_ownership_transfer(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, a) = two_users(&db).await;
    let gid = guild_with(&db, &own, &[&a]).await;
    let r = db.create_role(gid, own.id, &role("R", 0)).await.unwrap();
    db.set_member_role(gid, own.id, a.id, r.id, true).await.unwrap();

    // Le propriétaire ne part pas sans transférer ; un autre peut partir, et revenir sans ses rôles.
    assert_eq!(db.leave_guild(gid, own.id).await, Err(DbError::Forbidden("owner_leave")));
    db.leave_guild(gid, a.id).await.unwrap();
    assert_eq!(db.leave_guild(gid, a.id).await, Err(DbError::NotFound));
    assert!(db.guild_state(gid, a.id).await.unwrap().is_none());
    let general = db
        .guild_state(gid, own.id)
        .await
        .unwrap()
        .unwrap()
        .channels
        .iter()
        .find(|c| c.kind == ChannelKind::Text)
        .unwrap()
        .id;
    let inv = db.create_invite(gid, Some(general), own.id, 0, None).await.unwrap();
    let (back, _) = db.use_invite(&inv.code, a.id).await.unwrap();
    assert!(back.members.iter().find(|m| m.user.id == a.id).unwrap().roles.is_empty());

    // Transfert : à un membre seulement, jamais à soi-même, réservé au propriétaire.
    let out = user(&db, "out").await;
    assert_eq!(db.transfer_ownership(gid, own.id, own.id).await, Err(DbError::Invalid("self_target")));
    assert_eq!(db.transfer_ownership(gid, own.id, out.id).await, Err(DbError::NotFound));
    assert_eq!(db.transfer_ownership(gid, a.id, a.id).await, Err(DbError::Forbidden("owner_only")));
    db.transfer_ownership(gid, own.id, a.id).await.unwrap();
    assert_eq!(db.guild_state(gid, a.id).await.unwrap().unwrap().guild.owner_id, a.id);
    assert_eq!(db.guild_perms(a.id, gid).await.unwrap(), perms::ALL);
    // L'ancien propriétaire est un membre comme les autres : il peut partir, et n'a plus la main.
    assert_eq!(db.kick_member(gid, own.id, a.id, None).await, Err(DbError::Forbidden("perm")));
    db.leave_guild(gid, own.id).await.unwrap();
}
