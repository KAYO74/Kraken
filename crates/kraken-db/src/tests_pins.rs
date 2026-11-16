//! Messages épinglés : épingler, désépingler, sans effet quand rien ne change, 50 au plus par salon, du plus
//! récemment épinglé au plus ancien, avec réactions et pièces jointes comme l'historique, retirés avec le message,
//! gardés quand on le modifie ; journal d'audit dans un serveur seulement, sans le texte et sans doublon.

use kraken_proto::{AuditCategory, ChannelKind, CreateChannel, MAX_AUDIT_PAGE, MAX_PINS};
use sqlx::PgPool;

use super::*;

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

/// Un serveur à `owner` avec `others` dedans ; rend son id et celui de #général.
async fn guild_with(db: &Db, owner: &User, others: &[&User]) -> (Id, Id) {
    let g = db.create_guild(owner.id, "Abysse", None).await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), owner.id, 0, None).await.unwrap();
    for u in others {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    (g.guild.id, general)
}

async fn say(db: &Db, cid: Id, who: Id, text: &str) -> Message {
    db.create_message(cid, who, text, None).await.unwrap().0
}

async fn audit_rows(db: &Db, action: &str) -> i64 {
    sqlx::query_scalar("select count(*) from audit_log where action = $1")
        .bind(action)
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

fn ids(v: &[Message]) -> Vec<Id> {
    v.iter().map(|m| m.id).collect()
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn pinning_is_idempotent_and_lists_the_latest_pin_first(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (_, general) = guild_with(&db, &lou, &[&sam]).await;
    let (a, b, c) = (
        say(&db, general, sam.id, "premier").await,
        say(&db, general, lou.id, "deuxième").await,
        say(&db, general, sam.id, "troisième").await,
    );
    assert!(db.pinned_messages(general).await.unwrap().is_empty());
    assert!(!a.pinned, "un message neuf n'est pas épinglé");

    // Épingler : le message le dit, et la liste le rend.
    let (m, changed) = db.set_pinned(general, a.id, lou.id, true).await.unwrap();
    assert!(changed && m.pinned);
    assert!(db.message(a.id).await.unwrap().unwrap().pinned);
    // Déjà épinglé : rien ne change.
    let (m, changed) = db.set_pinned(general, a.id, lou.id, true).await.unwrap();
    assert!(!changed && m.pinned);

    // Le plus récemment épinglé d'abord, quel que soit l'ordre d'envoi.
    db.set_pinned(general, c.id, sam.id, true).await.unwrap();
    db.set_pinned(general, b.id, lou.id, true).await.unwrap();
    assert_eq!(ids(&db.pinned_messages(general).await.unwrap()), [b.id, c.id, a.id]);

    // L'historique, lui, garde son ordre et porte l'épingle.
    let log = db.messages(general, None, 50).await.unwrap();
    assert_eq!(ids(&log), [a.id, b.id, c.id]);
    assert!(log.iter().all(|m| m.pinned));

    // Désépingler : il sort de la liste ; recommencer ne change rien.
    let (m, changed) = db.set_pinned(general, c.id, lou.id, false).await.unwrap();
    assert!(changed && !m.pinned);
    let (m, changed) = db.set_pinned(general, c.id, lou.id, false).await.unwrap();
    assert!(!changed && !m.pinned);
    assert_eq!(ids(&db.pinned_messages(general).await.unwrap()), [b.id, a.id]);
    // Épingler de nouveau : il revient en tête.
    db.set_pinned(general, c.id, lou.id, true).await.unwrap();
    assert_eq!(ids(&db.pinned_messages(general).await.unwrap()), [c.id, b.id, a.id]);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_pinned_message_keeps_its_pin_when_edited_and_loses_it_when_deleted(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (_, general) = guild_with(&db, &lou, &[&sam]).await;
    let (a, b) = (say(&db, general, sam.id, "à garder").await, say(&db, general, sam.id, "à retirer").await);
    db.set_pinned(general, a.id, lou.id, true).await.unwrap();
    db.set_pinned(general, b.id, lou.id, true).await.unwrap();

    let edited = db.edit_message(general, a.id, sam.id, "à garder, corrigé", false).await.unwrap();
    assert!(edited.pinned && edited.edited_at.is_some());
    assert_eq!(db.pinned_messages(general).await.unwrap()[1].content, "à garder, corrigé");

    // Un message épinglé qu'on supprime quitte la liste (la ligne de la base s'en va avec lui).
    db.delete_message(general, b.id, sam.id, false).await.unwrap();
    assert_eq!(ids(&db.pinned_messages(general).await.unwrap()), [a.id]);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn pinned_messages_come_back_in_the_history_format(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (_, general) = guild_with(&db, &lou, &[&sam]).await;
    let origin = say(&db, general, lou.id, "d'origine").await;
    let (reply, _) = db
        .send_message(
            general,
            sam.id,
            Draft { content: "bonjour @lou", reply_to: Some(origin.id), ..Default::default() },
        )
        .await
        .unwrap();
    db.add_reaction(general, reply.id, lou.id, "👍").await.unwrap();
    db.set_pinned(general, reply.id, lou.id, true).await.unwrap();

    let pinned = db.pinned_messages(general).await.unwrap();
    assert_eq!(pinned.len(), 1);
    let m = &pinned[0];
    assert_eq!(Some(m), db.messages(general, None, 50).await.unwrap().last());
    assert_eq!(m.reply.as_ref().map(|r| r.content.as_str()), Some("d'origine"));
    assert_eq!((m.reactions.len(), m.mentions.len()), (1, 1));
    assert!(m.pinned);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_message_of_another_channel_or_a_call_cannot_be_pinned(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (gid, general) = guild_with(&db, &lou, &[&sam]).await;
    let other = db
        .create_channel(gid, lou.id, &CreateChannel { name: "autre".into(), kind: ChannelKind::Text, parent_id: None })
        .await
        .unwrap();
    let here = say(&db, general, sam.id, "ici").await;
    let there = say(&db, other.id, sam.id, "là").await;

    // L'id d'un message d'un autre salon, ou d'un message qui n'existe pas : introuvable, et rien n'a changé.
    assert_eq!(db.set_pinned(general, there.id, lou.id, true).await.err(), Some(DbError::NotFound));
    assert_eq!(db.set_pinned(other.id, here.id, lou.id, true).await.err(), Some(DbError::NotFound));
    assert_eq!(db.set_pinned(general, Id::nil(), lou.id, false).await.err(), Some(DbError::NotFound));
    assert!(!db.message(there.id).await.unwrap().unwrap().pinned);
    assert_eq!(audit_rows(&db, "message_pin").await, 0);

    // Un message d'appel n'est pas du texte à épingler.
    let call = say(&db, general, sam.id, "appel").await;
    sqlx::query("update messages set kind = 'call' where id = $1").bind(call.id).execute(&db.pool).await.unwrap();
    assert_eq!(db.set_pinned(general, call.id, lou.id, true).await.err(), Some(DbError::Invalid("pin_kind")));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_channel_holds_fifty_pins_and_unpinning_makes_room(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (gid, general) = guild_with(&db, &lou, &[&sam]).await;
    let other = db
        .create_channel(gid, lou.id, &CreateChannel { name: "autre".into(), kind: ChannelKind::Text, parent_id: None })
        .await
        .unwrap();
    let mut all = Vec::new();
    for i in 0..=MAX_PINS {
        all.push(say(&db, general, sam.id, &format!("message {i}")).await);
    }
    for m in &all[..MAX_PINS] {
        db.set_pinned(general, m.id, lou.id, true).await.unwrap();
    }
    assert_eq!(db.pinned_messages(general).await.unwrap().len(), MAX_PINS);

    // Le 51e est refusé, et rien n'est écrit ; un message déjà épinglé reste un succès sans effet.
    let last = &all[MAX_PINS];
    assert_eq!(db.set_pinned(general, last.id, lou.id, true).await.err(), Some(DbError::Invalid("pin_limit")));
    assert!(!db.message(last.id).await.unwrap().unwrap().pinned);
    assert_eq!(audit_rows(&db, "message_pin").await, MAX_PINS as i64);
    assert!(!db.set_pinned(general, all[0].id, lou.id, true).await.unwrap().1);

    // La limite est par salon.
    let elsewhere = say(&db, other.id, sam.id, "ailleurs").await;
    db.set_pinned(other.id, elsewhere.id, lou.id, true).await.unwrap();

    // Désépingler en libère un ; supprimer un message épinglé aussi.
    db.set_pinned(general, all[10].id, lou.id, false).await.unwrap();
    db.set_pinned(general, last.id, lou.id, true).await.unwrap();
    assert_eq!(db.set_pinned(general, all[10].id, lou.id, true).await.err(), Some(DbError::Invalid("pin_limit")));
    db.delete_message(general, all[11].id, sam.id, false).await.unwrap();
    db.set_pinned(general, all[10].id, lou.id, true).await.unwrap();
    assert_eq!(db.pinned_messages(general).await.unwrap().len(), MAX_PINS);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn two_pins_at_once_never_exceed_the_limit(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (_, general) = guild_with(&db, &lou, &[&sam]).await;
    let mut all = Vec::new();
    for i in 0..MAX_PINS + 3 {
        all.push(say(&db, general, sam.id, &format!("message {i}")).await);
    }
    for m in &all[..MAX_PINS - 3] {
        db.set_pinned(general, m.id, lou.id, true).await.unwrap();
    }
    // Six demandes simultanées pour trois places.
    let tasks: Vec<_> = all[MAX_PINS - 3..]
        .iter()
        .map(|m| {
            let (db, id) = (db.clone(), m.id);
            tokio::spawn(async move { db.set_pinned(general, id, lou.id, true).await })
        })
        .collect();
    let mut results = Vec::new();
    for t in tasks {
        results.push(t.await.unwrap());
    }
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 3);
    assert_eq!(results.iter().filter(|r| matches!(r, Err(DbError::Invalid("pin_limit")))).count(), 3);
    assert_eq!(db.pinned_messages(general).await.unwrap().len(), MAX_PINS);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn pins_in_a_server_leave_one_trace_each_and_never_the_text(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (gid, general) = guild_with(&db, &lou, &[&sam]).await;
    let m = say(&db, general, sam.id, "un secret très secret").await;

    db.set_pinned(general, m.id, lou.id, true).await.unwrap();
    db.set_pinned(general, m.id, lou.id, true).await.unwrap(); // sans effet : pas de doublon
    assert_eq!((audit_rows(&db, "message_pin").await, audit_rows(&db, "message_unpin").await), (1, 0));
    db.set_pinned(general, m.id, lou.id, false).await.unwrap();
    db.set_pinned(general, m.id, lou.id, false).await.unwrap();
    assert_eq!((audit_rows(&db, "message_pin").await, audit_rows(&db, "message_unpin").await), (1, 1));

    let page = db.audit_log(gid, lou.id, &AuditQuery { limit: MAX_AUDIT_PAGE, ..Default::default() }).await.unwrap();
    let pins: Vec<_> = page.entries.iter().filter(|e| e.action.starts_with("message_")).collect();
    assert_eq!(pins.len(), 2);
    for e in &pins {
        assert_eq!(e.actor.as_ref().map(|a| a.id), Some(lou.id));
        let t = e.target.as_ref().unwrap();
        assert_eq!((t.kind, t.id, t.name.as_deref()), (AuditTargetKind::User, sam.id, Some("sam")));
        assert_eq!(e.details.get("channel").map(String::as_str), Some("général"));
    }
    assert_eq!(pins[0].action, "message_unpin", "du plus récent au plus ancien");
    let json = serde_json::to_string(&page.entries).unwrap();
    assert!(!json.contains("secret"), "le texte du message ne sort pas");
    let stored: Vec<serde_json::Value> =
        sqlx::query_scalar("select changes from audit_log where action like 'message\\_%pin'")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert!(stored.iter().all(|c| !c.to_string().contains("secret")), "et il n'est pas écrit non plus");
    assert!(
        stored
            .iter()
            .all(|c| c["message_id"] == serde_json::json!(m.id) && c["channel_id"] == serde_json::json!(general))
    );
    // Rangé avec « Serveur ».
    let guild = db
        .audit_log(gid, lou.id, &AuditQuery { limit: 10, category: Some(AuditCategory::Guild), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(guild.entries.len(), 2);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn pins_in_a_private_conversation_work_for_both_and_leave_no_audit(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    guild_with(&db, &lou, &[&sam]).await;
    let (dm, _) = db.open_dm(lou.id, sam.id).await.unwrap();
    let (a, b) = (say(&db, dm, lou.id, "de lou").await, say(&db, dm, sam.id, "de sam").await);

    // Chacun épingle et désépingle, y compris le message de l'autre.
    db.set_pinned(dm, a.id, sam.id, true).await.unwrap();
    db.set_pinned(dm, b.id, lou.id, true).await.unwrap();
    assert_eq!(ids(&db.pinned_messages(dm).await.unwrap()), [b.id, a.id]);
    db.set_pinned(dm, a.id, lou.id, false).await.unwrap();
    assert_eq!(ids(&db.pinned_messages(dm).await.unwrap()), [b.id]);
    assert_eq!(
        audit_rows(&db, "message_pin").await + audit_rows(&db, "message_unpin").await,
        0,
        "pas de journal en MP"
    );
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_blocked_private_conversation_refuses_pins_like_messages(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    guild_with(&db, &lou, &[&sam]).await;
    let (dm, _) = db.open_dm(lou.id, sam.id).await.unwrap();
    let (a, b) = (say(&db, dm, lou.id, "de lou").await, say(&db, dm, sam.id, "de sam").await);
    db.set_pinned(dm, a.id, sam.id, true).await.unwrap();

    // Lou bloque sam : mêmes refus que l'envoi, dans les deux sens, et rien ne change ; la liste reste lisible.
    db.block(lou.id, sam.id).await.unwrap();
    for (who, refusal, pin_refusal) in [(sam.id, "dm_refused", "pin_refused"), (lou.id, "dm_blocked", "pin_blocked")] {
        assert_eq!(db.create_message(dm, who, "x", None).await.err(), Some(DbError::Forbidden(refusal)));
        assert_eq!(db.set_pinned(dm, b.id, who, true).await.err(), Some(DbError::Forbidden(pin_refusal)));
        assert_eq!(db.set_pinned(dm, a.id, who, false).await.err(), Some(DbError::Forbidden(pin_refusal)));
    }
    assert_eq!(ids(&db.pinned_messages(dm).await.unwrap()), [a.id]);

    // Le blocage levé, le MP se rouvre : on épingle de nouveau.
    db.remove_relationship(lou.id, sam.id).await.unwrap();
    db.set_pinned(dm, b.id, sam.id, true).await.unwrap();
    assert_eq!(ids(&db.pinned_messages(dm).await.unwrap()), [b.id, a.id]);
}
