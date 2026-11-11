//! Invitations (validation, liste, révocation, périmées et épuisées), suppression d'un serveur
//! et exclusion temporaire.

use kraken_proto::{ChannelKind, CreateRole, EditRole, InviteState, MAX_INVITE_AGE_S, MAX_INVITE_USES};
use sqlx::PgPool;

use super::*;

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

/// Un serveur à `owner` avec `others` dedans ; rend son état et l'id de #général.
async fn guild_with(db: &Db, owner: &User, others: &[&User]) -> (GuildState, Id) {
    let g = db.create_guild(owner.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), owner.id, 0, None).await.unwrap();
    for u in others {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    (g, general)
}

fn role(name: &str, permissions: u64) -> CreateRole {
    CreateRole { name: name.into(), color: None, hoist: false, permissions }
}

async fn audit(db: &Db, action: &str) -> i64 {
    sqlx::query_scalar("select count(*) from audit_log where action = $1")
        .bind(action)
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

async fn count(db: &Db, table: &str, gid: Id) -> i64 {
    let by_channel = matches!(table, "messages" | "attachments" | "permission_overwrites");
    let sql = if by_channel {
        format!("select count(*) from {table} t join channels c on c.id = t.channel_id where c.guild_id = $1")
    } else {
        format!("select count(*) from {table} where guild_id = $1")
    };
    sqlx::query_scalar(&sql).bind(gid).fetch_one(&db.pool).await.unwrap()
}

// ───────── invitations ─────────

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn invites_are_validated_and_audited(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let (g, general) = guild_with(&db, &own, &[]).await;
    let gid = g.guild.id;

    assert_eq!(
        db.create_invite(gid, Some(general), own.id, 0, Some(MAX_INVITE_AGE_S + 1)).await.map(|_| ()),
        Err(DbError::Invalid("invite_age"))
    );
    assert_eq!(
        db.create_invite(gid, Some(general), own.id, MAX_INVITE_USES + 1, None).await.map(|_| ()),
        Err(DbError::Invalid("invite_uses"))
    );
    let week = db.create_invite(gid, Some(general), own.id, 0, Some(MAX_INVITE_AGE_S)).await.unwrap();
    assert!(week.expires_at.is_some() && week.max_uses == 0);
    let never = db.create_invite(gid, Some(general), own.id, 5, None).await.unwrap();
    assert!(never.expires_at.is_none() && never.max_uses == 5);
    // 0 seconde : comme « jamais ».
    assert!(db.create_invite(gid, None, own.id, 0, Some(0)).await.unwrap().expires_at.is_none());
    // Les trois qui ont réussi (et celle de `guild_with`) sont au journal, pas les refusées.
    assert_eq!(audit(&db, "invite_create").await, 4);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn the_invite_list_shows_live_ones_then_recent_dead_ones_to_who_may_see_them(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, plain, out) =
        (user(&db, "lou").await, user(&db, "max").await, user(&db, "sam").await, user(&db, "noa").await);
    let (g, general) = guild_with(&db, &own, &[&mgr, &plain]).await;
    let gid = g.guild.id;
    let managers = db.create_role(gid, own.id, &role("Gérants", perms::MANAGE_GUILD)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, managers.id, true).await.unwrap();

    let live = db.create_invite(gid, Some(general), plain.id, 10, Some(3600)).await.unwrap();
    let mine = db.create_invite(gid, Some(general), mgr.id, 0, None).await.unwrap();
    let expired = db.create_invite(gid, Some(general), own.id, 0, Some(3600)).await.unwrap();
    let spent = db.create_invite(gid, Some(general), own.id, 1, None).await.unwrap();
    let ancient = db.create_invite(gid, Some(general), own.id, 0, Some(3600)).await.unwrap();
    sqlx::query("update invites set expires_at = now() - interval '2 hours' where code = $1")
        .bind(&expired.code)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("update invites set expires_at = now() - interval '20 days' where code = $1")
        .bind(&ancient.code)
        .execute(&db.pool)
        .await
        .unwrap();
    db.use_invite(&spent.code, out.id).await.unwrap();

    // Le gérant du serveur voit tout, sauf ce qui est mort depuis plus d'une semaine.
    let all = db.guild_invites(gid, mgr.id).await.unwrap();
    let states: Vec<(&str, InviteState)> = all.iter().map(|i| (i.code.as_str(), i.state)).collect();
    assert!(states.contains(&(live.code.as_str(), InviteState::Active)), "{states:?}");
    assert!(states.contains(&(expired.code.as_str(), InviteState::Expired)));
    assert!(states.contains(&(spent.code.as_str(), InviteState::Exhausted)));
    assert!(!states.iter().any(|(c, _)| *c == ancient.code));
    let first_dead = all.iter().position(|i| i.state != InviteState::Active).unwrap();
    assert!(all[first_dead..].iter().all(|i| i.state != InviteState::Active), "les vivantes d'abord");
    let row = all.iter().find(|i| i.code == live.code).unwrap();
    assert_eq!((row.uses, row.max_uses, row.inviter.as_ref().map(|u| u.id)), (0, 10, Some(plain.id)));
    assert_eq!(row.channel_name.as_deref(), Some("général"));
    assert!(row.expires_at.is_some());

    // Un membre qui peut seulement inviter ne voit que les siennes.
    let own_list = db.guild_invites(gid, plain.id).await.unwrap();
    assert_eq!(own_list.iter().map(|i| i.code.as_str()).collect::<Vec<_>>(), [live.code.as_str()]);
    // Sans « Créer une invitation » : 403. Étranger au serveur : 404.
    let everyone = perms::DEFAULT_EVERYONE & !perms::CREATE_INVITE;
    db.edit_role(gid, gid, own.id, &EditRole { permissions: Some(everyone), ..Default::default() }).await.unwrap();
    assert_eq!(db.guild_invites(gid, plain.id).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(
        db.guild_invites(gid, out.id).await.map(|_| ()),
        Err(DbError::Forbidden("perm")),
        "un simple membre n'a plus le droit"
    );
    let stranger = user(&db, "eve").await;
    assert_eq!(db.guild_invites(gid, stranger.id).await.map(|_| ()), Err(DbError::NotFound));

    // La création nettoie ce qui est mort depuis plus d'une semaine.
    db.create_invite(gid, Some(general), own.id, 0, None).await.unwrap();
    let gone: i64 = sqlx::query_scalar("select count(*) from invites where code = $1")
        .bind(&ancient.code)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(gone, 0);
    let _ = mine;
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn dead_invites_say_why_and_join_nobody(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, newcomer) = (user(&db, "lou").await, user(&db, "sam").await);
    let (g, general) = guild_with(&db, &own, &[]).await;
    let gid = g.guild.id;
    let members = |db: &Db| {
        let db = db.clone();
        async move { count(&db, "members", gid).await }
    };

    assert_eq!(db.invite_checked("introuvable").await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.use_invite("introuvable", newcomer.id).await.map(|_| ()), Err(DbError::NotFound));

    let timed = db.create_invite(gid, Some(general), own.id, 0, Some(3600)).await.unwrap();
    assert!(db.invite_checked(&timed.code).await.is_ok());
    sqlx::query("update invites set expires_at = now() - interval '1 minute' where code = $1")
        .bind(&timed.code)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(db.invite_checked(&timed.code).await.map(|_| ()), Err(DbError::Gone("expired")));
    assert_eq!(db.use_invite(&timed.code, newcomer.id).await.map(|_| ()), Err(DbError::Gone("expired")));
    assert_eq!(members(&db).await, 1, "personne n'est entré");

    let once = db.create_invite(gid, Some(general), own.id, 1, None).await.unwrap();
    let first = user(&db, "eve").await;
    db.use_invite(&once.code, first.id).await.unwrap();
    assert_eq!(db.invite_checked(&once.code).await.map(|_| ()), Err(DbError::Gone("exhausted")));
    assert_eq!(db.use_invite(&once.code, newcomer.id).await.map(|_| ()), Err(DbError::Gone("exhausted")));
    assert_eq!(members(&db).await, 2);
    let used: i32 = sqlx::query_scalar("select uses from invites where code = $1")
        .bind(&once.code)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(used, 1, "le compteur n'a pas bougé");
    // Déjà membre : le lien périmé ouvre quand même le serveur, sans rien compter.
    assert_eq!(db.use_invite(&once.code, first.id).await.map(|(_, m)| m), Ok(None));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn revoking_an_invite_is_for_its_creator_or_guild_managers(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, maker, other, out) = (
        user(&db, "lou").await,
        user(&db, "max").await,
        user(&db, "sam").await,
        user(&db, "eve").await,
        user(&db, "noa").await,
    );
    let (g, general) = guild_with(&db, &own, &[&mgr, &maker, &other]).await;
    let gid = g.guild.id;
    let managers = db.create_role(gid, own.id, &role("Gérants", perms::MANAGE_GUILD)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, managers.id, true).await.unwrap();
    let a = db.create_invite(gid, Some(general), maker.id, 0, None).await.unwrap();
    let b = db.create_invite(gid, Some(general), maker.id, 0, None).await.unwrap();
    let c = db.create_invite(gid, Some(general), maker.id, 0, None).await.unwrap();

    assert_eq!(db.revoke_invite(&a.code, other.id).await, Err(DbError::Forbidden("perm")), "ni gérant ni créateur");
    assert_eq!(db.revoke_invite(&a.code, out.id).await, Err(DbError::NotFound), "étranger au serveur");
    assert_eq!(db.revoke_invite("introuvable", own.id).await, Err(DbError::NotFound));
    assert!(db.invite(&a.code).await.unwrap().is_some());
    assert_eq!(db.revoke_invite(&a.code, maker.id).await, Ok(gid), "son créateur");
    assert_eq!(db.revoke_invite(&b.code, mgr.id).await, Ok(gid), "« Gérer le serveur »");
    assert_eq!(db.revoke_invite(&c.code, own.id).await, Ok(gid), "le propriétaire");
    assert_eq!(db.revoke_invite(&a.code, maker.id).await, Err(DbError::NotFound), "déjà révoquée");
    // Révoquée : comme si elle n'avait jamais existé, y compris pour rejoindre.
    assert_eq!(db.invite_checked(&b.code).await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.use_invite(&b.code, out.id).await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(audit(&db, "invite_revoke").await, 3);

    // Un créateur qui a quitté le serveur n'a plus la main sur ses invitations.
    let d = db.create_invite(gid, Some(general), other.id, 0, None).await.unwrap();
    db.leave_guild(gid, other.id).await.unwrap();
    assert_eq!(db.revoke_invite(&d.code, other.id).await, Err(DbError::NotFound));
    assert_eq!(db.revoke_invite(&d.code, mgr.id).await, Ok(gid));
}

// ───────── suppression du serveur ─────────

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn deleting_a_guild_is_for_its_owner_who_retypes_the_name_and_takes_everything(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mem, out) = (user(&db, "lou").await, user(&db, "sam").await, user(&db, "noa").await);
    let (g, general) = guild_with(&db, &own, &[&mem]).await;
    let gid = g.guild.id;
    let (other, other_general) = {
        let og = db.create_guild(own.id, "Récif").await.unwrap();
        let c = og.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
        db.create_message(c, own.id, "ailleurs", None).await.unwrap();
        (og.guild.id, c)
    };

    // De quoi remplir : messages, pièce jointe, rôle donné, surcharge, ban, invitation.
    let (m, _) = db.create_message(general, mem.id, "bonjour", None).await.unwrap();
    db.create_attachment(NewAttachment {
        channel_id: general,
        uploader_id: mem.id,
        filename: "f.png",
        mime: "image/png",
        size: 3,
        storage_key: "clefdisque",
        dims: Some((1, 1)),
        preview: true,
    })
    .await
    .unwrap();
    let r = db.create_role(gid, own.id, &role("Haut", perms::MANAGE_MESSAGES)).await.unwrap();
    db.set_member_role(gid, own.id, mem.id, r.id, true).await.unwrap();
    db.ban_member(gid, own.id, out.id, &BanMember::default()).await.unwrap();
    db.set_overwrite(
        general,
        own.id,
        r.id,
        &SetOverwrite { kind: OverwriteKind::Role, allow: perms::EMBED_LINKS, deny: 0 },
    )
    .await
    .unwrap();
    let _ = m;
    for t in ["channels", "messages", "attachments", "roles", "members", "invites", "bans", "audit_log"] {
        assert!(count(&db, t, gid).await > 0, "{t} doit avoir des lignes avant");
    }

    // Seul le propriétaire, avec le bon nom : sinon rien ne bouge.
    assert_eq!(db.delete_guild(gid, mem.id, "Abysse").await, Err(DbError::Forbidden("owner_only")));
    assert_eq!(db.delete_guild(gid, out.id, "Abysse").await, Err(DbError::NotFound), "étranger : 404");
    assert_eq!(db.delete_guild(gid, own.id, "abysse").await, Err(DbError::Invalid("name_mismatch")), "la casse compte");
    assert_eq!(db.delete_guild(gid, own.id, "").await, Err(DbError::Invalid("name_mismatch")));
    assert_eq!(db.delete_guild(Id::nil(), own.id, "Abysse").await, Err(DbError::NotFound));
    assert!(db.guild_state(gid, own.id).await.unwrap().is_some());

    let done = db.delete_guild(gid, own.id, "  Abysse ").await.unwrap();
    assert_eq!(done.members.len(), 2);
    assert!(done.members.contains(&own.id) && done.members.contains(&mem.id));
    assert_eq!(done.files, ["attachments/clefdisque", "attachments/clefdisque.p"]);
    assert!(db.guild_state(gid, own.id).await.unwrap().is_none());
    assert!(db.channel(general).await.unwrap().is_none());
    for t in [
        "channels",
        "messages",
        "attachments",
        "roles",
        "members",
        "invites",
        "bans",
        "audit_log",
        "permission_overwrites",
    ] {
        assert_eq!(count(&db, t, gid).await, 0, "{t} doit être vide après");
    }
    assert_eq!(db.delete_guild(gid, own.id, "Abysse").await, Err(DbError::NotFound), "déjà supprimé");
    // Les comptes restent, et l'autre serveur n'a pas bougé.
    assert!(db.user(mem.id).await.unwrap().is_some());
    assert_eq!(db.messages(other_general, None, 10).await.unwrap().len(), 1);
    assert!(db.guild_state(other, own.id).await.unwrap().is_some());
}

// ───────── exclusion temporaire ─────────

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_timed_out_member_reads_but_does_nothing_else_until_it_ends(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mem) = (user(&db, "lou").await, user(&db, "sam").await);
    let (g, general) = guild_with(&db, &own, &[&mem]).await;
    let gid = g.guild.id;
    let voice = g.channels.iter().find(|c| c.kind == ChannelKind::Voice).unwrap().id;
    let mods = db.create_role(gid, own.id, &role("Modos", perms::TIMEOUT)).await.unwrap();
    let modo = user(&db, "max").await;
    let inv = db.create_invite(gid, Some(general), own.id, 0, None).await.unwrap();
    db.use_invite(&inv.code, modo.id).await.unwrap();
    db.set_member_role(gid, own.id, modo.id, mods.id, true).await.unwrap();

    let before = db.channel_perms(mem.id, general).await.unwrap();
    for bit in [perms::SEND_MESSAGES, perms::ADD_REACTIONS, perms::ATTACH_FILES] {
        assert_ne!(before & bit, 0);
    }
    assert_ne!(db.channel_perms(mem.id, voice).await.unwrap() & (perms::CONNECT | perms::SPEAK), 0);
    assert_eq!(db.channel_timeout(mem.id, general).await, Ok(None));

    let m = db.timeout_member(gid, modo.id, mem.id, 300, Some("trop de bruit")).await.unwrap();
    let until = m.timeout_until.unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    assert!((now + 290_000..=now + 310_000).contains(&until), "{until} vs {now}");
    // Il voit, il lit ; rien d'autre, dans un salon écrit comme dans un vocal.
    assert_eq!(db.channel_perms(mem.id, general).await.unwrap(), perms::VIEW_CHANNEL | perms::READ_HISTORY);
    assert_eq!(db.channel_perms(mem.id, voice).await.unwrap(), perms::VIEW_CHANNEL | perms::READ_HISTORY);
    assert_eq!(db.channel_timeout(mem.id, general).await, Ok(Some(until)));
    assert!(db.guild_state(gid, own.id).await.unwrap().unwrap().members.iter().any(|x| x.timeout_until == Some(until)));
    assert_eq!(audit(&db, "member_timeout").await, 1);
    let reason: Option<String> = sqlx::query_scalar("select reason from audit_log where action = 'member_timeout'")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(reason.as_deref(), Some("trop de bruit"));
    // Les autres ne sont pas touchés.
    assert_ne!(db.channel_perms(modo.id, general).await.unwrap() & perms::SEND_MESSAGES, 0);

    // La fin se lit sur l'horloge de la base : sans rien faire d'autre, tout revient.
    sqlx::query("update members set timeout_until = now() - interval '1 second' where user_id = $1")
        .bind(mem.id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(db.channel_perms(mem.id, general).await.unwrap(), before);
    assert_eq!(db.channel_timeout(mem.id, general).await, Ok(None));
    assert!(db.guild_state(gid, own.id).await.unwrap().unwrap().members.iter().all(|x| x.timeout_until.is_none()));

    // Levée à la main.
    db.timeout_member(gid, modo.id, mem.id, 3600, None).await.unwrap();
    let lifted = db.remove_timeout(gid, modo.id, mem.id).await.unwrap();
    assert_eq!(lifted.timeout_until, None);
    assert_eq!(db.channel_perms(mem.id, general).await.unwrap(), before);
    assert_eq!(audit(&db, "member_timeout_remove").await, 1);
    // Rien à lever : pas de nouvelle ligne de journal.
    db.remove_timeout(gid, modo.id, mem.id).await.unwrap();
    assert_eq!(audit(&db, "member_timeout_remove").await, 1);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn timing_out_follows_the_rights_and_the_hierarchy(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, modo, peer, low, plain, admin) = (
        user(&db, "lou").await,
        user(&db, "max").await,
        user(&db, "pair").await,
        user(&db, "eve").await,
        user(&db, "sam").await,
        user(&db, "ada").await,
    );
    let (g, general) = guild_with(&db, &own, &[&modo, &peer, &low, &plain, &admin]).await;
    let gid = g.guild.id;
    // Haut (Modos, avec TIMEOUT) > Admins (ADMINISTRATOR) > Petit ; « Pair » a les mêmes droits que Modos.
    let mods = db.create_role(gid, own.id, &role("Modos", perms::TIMEOUT)).await.unwrap();
    let admins = db.create_role(gid, own.id, &role("Admins", perms::ADMINISTRATOR)).await.unwrap();
    let small = db.create_role(gid, own.id, &role("Petit", 0)).await.unwrap();
    for (u, r) in [(&modo, &mods), (&peer, &mods), (&admin, &admins), (&low, &small)] {
        db.set_member_role(gid, own.id, u.id, r.id, true).await.unwrap();
    }

    let t = |by: &User, target: &User, secs: u32| {
        let db = db.clone();
        let (by, target) = (by.id, target.id);
        async move { db.timeout_member(gid, by, target, secs, None).await.map(|m| m.timeout_until.is_some()) }
    };
    assert_eq!(t(&modo, &low, 60).await, Ok(true), "rang plus bas");
    assert_eq!(t(&modo, &plain, 28 * 24 * 3600).await, Ok(true), "sans rôle, 28 jours");
    assert_eq!(t(&plain, &low, 60).await, Err(DbError::Forbidden("perm")), "sans le droit");
    assert_eq!(t(&modo, &modo, 60).await, Err(DbError::Invalid("self_target")));
    assert_eq!(t(&modo, &peer, 60).await, Err(DbError::Forbidden("hierarchy")), "même rang");
    assert_eq!(t(&modo, &own, 60).await, Err(DbError::Forbidden("hierarchy")), "le propriétaire");
    assert_eq!(t(&modo, &admin, 60).await, Err(DbError::Forbidden("admin_target")), "un administrateur");
    assert_eq!(t(&own, &admin, 60).await, Err(DbError::Forbidden("admin_target")), "même pour le propriétaire");
    assert_eq!(t(&own, &own, 60).await, Err(DbError::Invalid("self_target")));
    // Durées : de 60 s à 28 jours.
    assert_eq!(t(&modo, &low, 59).await, Err(DbError::Invalid("timeout_seconds")));
    assert_eq!(t(&modo, &low, 28 * 24 * 3600 + 1).await, Err(DbError::Invalid("timeout_seconds")));
    let stranger = user(&db, "zoe").await;
    assert_eq!(t(&modo, &stranger, 60).await, Err(DbError::NotFound), "pas membre");
    assert_eq!(t(&stranger, &low, 60).await, Err(DbError::NotFound), "étranger au serveur");
    assert_eq!(
        db.timeout_member(gid, modo.id, low.id, 60, Some(&"x".repeat(600))).await.map(|_| ()),
        Err(DbError::Invalid("reason"))
    );

    // Lever : mêmes droits, même hiérarchie.
    assert_eq!(db.remove_timeout(gid, plain.id, low.id).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(db.remove_timeout(gid, modo.id, peer.id).await.map(|_| ()), Err(DbError::Forbidden("hierarchy")));
    assert_eq!(db.remove_timeout(gid, modo.id, modo.id).await.map(|_| ()), Err(DbError::Invalid("self_target")));
    assert!(db.remove_timeout(gid, own.id, low.id).await.is_ok());

    // Un administrateur ou le propriétaire gardent tous leurs droits, même avec une ligne en base.
    sqlx::query("update members set timeout_until = now() + interval '1 hour' where user_id = any($1)")
        .bind([admin.id, own.id])
        .execute(&db.pool)
        .await
        .unwrap();
    for u in [&admin, &own] {
        assert_ne!(db.channel_perms(u.id, general).await.unwrap() & perms::SEND_MESSAGES, 0);
    }
}
