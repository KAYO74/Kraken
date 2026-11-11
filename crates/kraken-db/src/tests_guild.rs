//! Invitations (validation, liste, révocation, périmées et épuisées), suppression d'un serveur
//! et exclusion temporaire.

use kraken_proto::{
    ChannelKind, CreateRole, EditRole, InviteState, MAX_INVITE_AGE_S, MAX_INVITE_ROWS, MAX_INVITE_USES,
    MAX_INVITES_PER_MEMBER,
};
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

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_timed_out_moderator_loses_every_server_power_until_it_ends(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mb, adm, low, other) = (
        user(&db, "lou").await,
        user(&db, "max").await,
        user(&db, "ada").await,
        user(&db, "eve").await,
        user(&db, "sam").await,
    );
    let (g, general) = guild_with(&db, &own, &[&mb, &adm, &low, &other]).await;
    let gid = g.guild.id;
    // Un modérateur bien équipé, plus un administrateur ; tous deux sous le propriétaire.
    let everything = perms::TIMEOUT
        | perms::KICK
        | perms::BAN
        | perms::MANAGE_ROLES
        | perms::MANAGE_CHANNELS
        | perms::MANAGE_GUILD
        | perms::CREATE_INVITE
        | perms::VIEW_AUDIT_LOG;
    let mods = db.create_role(gid, own.id, &role("Modos", everything)).await.unwrap();
    let admins = db.create_role(gid, own.id, &role("Admins", perms::ADMINISTRATOR)).await.unwrap();
    db.set_member_role(gid, own.id, mb.id, mods.id, true).await.unwrap();
    db.set_member_role(gid, own.id, adm.id, admins.id, true).await.unwrap();
    let theirs = db.create_invite(gid, Some(general), other.id, 0, None).await.unwrap();

    let free = db.guild_perms(mb.id, gid).await.unwrap();
    assert_eq!(free & everything, everything);

    // Exclu : voir et lire, pas un droit de plus au niveau du serveur.
    db.timeout_member(gid, own.id, mb.id, 3600, None).await.unwrap();
    assert_eq!(db.guild_perms(mb.id, gid).await.unwrap(), perms::VIEW_CHANNEL | perms::READ_HISTORY);
    macro_rules! denied {
        ($call:expr) => {
            assert_eq!($call.map(|_| ()), Err(DbError::Forbidden("perm")), "{}", stringify!($call))
        };
    }
    denied!(db.kick_member(gid, mb.id, low.id, None).await);
    denied!(db.ban_member(gid, mb.id, low.id, &BanMember { reason: None, delete_hours: None }).await);
    denied!(db.timeout_member(gid, mb.id, low.id, 600, None).await);
    denied!(db.remove_timeout(gid, mb.id, mb.id).await);
    denied!(db.create_role(gid, mb.id, &role("Neuf", 0)).await);
    denied!(db.edit_role(gid, mods.id, mb.id, &EditRole { name: Some("X".into()), ..Default::default() }).await);
    denied!(db.delete_role(gid, mods.id, mb.id).await);
    denied!(db.reorder_roles(gid, mb.id, &[mods.id]).await);
    denied!(db.set_member_role(gid, mb.id, low.id, mods.id, true).await);
    denied!(db.unban(gid, mb.id, low.id).await);
    denied!(db.bans(gid, mb.id).await);
    denied!(db.guild_invites(gid, mb.id).await);
    denied!(db.revoke_invite(&theirs.code, mb.id).await);
    let new_channel = CreateChannel { name: "neuf".into(), kind: ChannelKind::Text, parent_id: None };
    denied!(db.create_channel(gid, mb.id, &new_channel).await);
    denied!(db.delete_channel(general, mb.id).await);
    assert!(!db.members_with(gid, perms::BAN).await.unwrap().contains(&mb.id));
    // Personne n'a été touché, rien n'a été journalisé.
    assert_eq!(count(&db, "bans", gid).await, 0);
    assert_eq!(audit(&db, "member_kick").await, 0);
    assert_eq!(db.guild_state(gid, own.id).await.unwrap().unwrap().members.len(), 5);

    // Le propriétaire et un administrateur ne sont jamais bridés, même avec une ligne en base.
    sqlx::query("update members set timeout_until = now() + interval '1 hour' where user_id = any($1)")
        .bind([adm.id, own.id])
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(db.guild_perms(own.id, gid).await.unwrap(), perms::ALL);
    assert_eq!(db.guild_perms(adm.id, gid).await.unwrap(), perms::ALL);
    db.timeout_member(gid, adm.id, low.id, 600, None).await.unwrap();
    db.kick_member(gid, own.id, low.id, None).await.unwrap();

    // La fin se lit sur l'horloge de la base : tout revient sans rien faire d'autre.
    sqlx::query("update members set timeout_until = now() - interval '1 second' where user_id = $1")
        .bind(mb.id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(db.guild_perms(mb.id, gid).await.unwrap(), free);
    db.kick_member(gid, mb.id, other.id, None).await.unwrap();
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn leaving_and_coming_back_keeps_the_exclusion_until_it_ends(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, modo, sam, eve, zoe) = (
        user(&db, "lou").await,
        user(&db, "max").await,
        user(&db, "sam").await,
        user(&db, "eve").await,
        user(&db, "zoe").await,
    );
    let (g, general) = guild_with(&db, &own, &[&modo, &sam, &eve, &zoe]).await;
    let gid = g.guild.id;
    let mods = db.create_role(gid, own.id, &role("Modos", perms::TIMEOUT | perms::KICK | perms::BAN)).await.unwrap();
    db.set_member_role(gid, own.id, modo.id, mods.id, true).await.unwrap();
    let inv = db.create_invite(gid, Some(general), own.id, 0, None).await.unwrap();
    let deadline = |uid: Id| {
        let db = db.clone();
        async move {
            let s = db.guild_state(gid, own.id).await.unwrap().unwrap();
            s.members.iter().find(|m| m.user.id == uid).map(|m| m.timeout_until)
        }
    };

    // Départ volontaire pendant l'exclusion, puis retour par invitation : toujours exclu, à la même échéance.
    let until = db.timeout_member(gid, modo.id, sam.id, 3600, None).await.unwrap().timeout_until.unwrap();
    db.leave_guild(gid, sam.id).await.unwrap();
    assert_eq!(deadline(sam.id).await, None, "parti");
    let (state, new) = db.use_invite(&inv.code, sam.id).await.unwrap();
    assert_eq!(new.unwrap().timeout_until, Some(until));
    assert!(state.members.iter().any(|m| m.user.id == sam.id && m.timeout_until == Some(until)));
    assert_eq!(db.channel_perms(sam.id, general).await.unwrap(), perms::VIEW_CHANNEL | perms::READ_HISTORY);
    assert!(db.channel_timeout(sam.id, general).await.unwrap().is_some());
    // La ligne gardée est consommée : un second départ la réécrit, rien ne s'accumule.
    assert_eq!(count(&db, "member_timeouts", gid).await, 0);
    db.leave_guild(gid, sam.id).await.unwrap();
    assert_eq!(count(&db, "member_timeouts", gid).await, 1);
    db.leave_guild(gid, sam.id).await.unwrap_err();
    assert_eq!(count(&db, "member_timeouts", gid).await, 1);

    // Exclusion terminée entre-temps : retour libre, plus rien de gardé.
    sqlx::query("update member_timeouts set until = now() - interval '1 second' where user_id = $1")
        .bind(sam.id)
        .execute(&db.pool)
        .await
        .unwrap();
    let (_, new) = db.use_invite(&inv.code, sam.id).await.unwrap();
    assert_eq!(new.unwrap().timeout_until, None);
    assert_eq!(count(&db, "member_timeouts", gid).await, 0);
    assert_ne!(db.channel_perms(sam.id, general).await.unwrap() & perms::SEND_MESSAGES, 0);

    // Un départ sans exclusion ne laisse rien.
    db.leave_guild(gid, sam.id).await.unwrap();
    assert_eq!(count(&db, "member_timeouts", gid).await, 0);
    db.use_invite(&inv.code, sam.id).await.unwrap();

    // Un renvoi ou un bannissement est une sanction : ils lèvent l'exclusion gardée.
    db.timeout_member(gid, modo.id, eve.id, 3600, None).await.unwrap();
    db.kick_member(gid, modo.id, eve.id, None).await.unwrap();
    let (_, new) = db.use_invite(&inv.code, eve.id).await.unwrap();
    assert_eq!(new.unwrap().timeout_until, None, "renvoyé puis revenu : libre");
    db.timeout_member(gid, modo.id, zoe.id, 3600, None).await.unwrap();
    db.leave_guild(gid, zoe.id).await.unwrap();
    assert_eq!(count(&db, "member_timeouts", gid).await, 1);
    db.ban_member(gid, modo.id, zoe.id, &BanMember { reason: None, delete_hours: None }).await.unwrap();
    assert_eq!(count(&db, "member_timeouts", gid).await, 0, "le bannissement efface ce qui était gardé");
    db.unban(gid, modo.id, zoe.id).await.unwrap();
    let (_, new) = db.use_invite(&inv.code, zoe.id).await.unwrap();
    assert_eq!(new.unwrap().timeout_until, None);

    // Une invitation morte ne rend rien et ne consomme rien : l'exclusion gardée reste.
    let dead = db.create_invite(gid, Some(general), own.id, 1, None).await.unwrap();
    db.use_invite(&dead.code, user(&db, "ned").await.id).await.unwrap();
    db.timeout_member(gid, modo.id, sam.id, 3600, None).await.unwrap();
    db.leave_guild(gid, sam.id).await.unwrap();
    assert!(db.use_invite(&dead.code, sam.id).await.is_err());
    assert_eq!(count(&db, "member_timeouts", gid).await, 1);
    // La suppression du serveur emporte aussi ce qui était gardé.
    db.delete_guild(gid, own.id, "Abysse").await.unwrap();
    let left: i64 = sqlx::query_scalar("select count(*) from member_timeouts").fetch_one(&db.pool).await.unwrap();
    assert_eq!(left, 0);
}

// ───────── cible disparue : 404, pas 500 ─────────

/// L'erreur de base que donne `sql` (une violation de clé étrangère, ici), passée par la
/// conversion qu'utilise tout le reste du code.
async fn db_error(db: &Db, sql: &str, binds: &[Id]) -> DbError {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    DbError::from(q.execute(&db.pool).await.unwrap_err())
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_write_aimed_at_a_vanished_channel_or_guild_is_not_found_but_other_keys_stay_errors(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (g, general) = guild_with(&db, &own, &[&sam]).await;
    let (gid, gone) = (g.guild.id, Id::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef));
    // Le salon ou le serveur visé n'existe plus (supprimé entre la vérification et l'écriture).
    let vanished = [
        ("insert into messages (channel_id, author_id, content) values ($1, $2, 'x')", vec![gone, sam.id]),
        ("insert into read_states (user_id, channel_id) values ($2, $1)", vec![gone, sam.id]),
        ("insert into slowmode_marks (channel_id, user_id) values ($1, $2)", vec![gone, sam.id]),
        (
            "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny) values ($1, $2, 'member', 0, 0)",
            vec![gone, sam.id],
        ),
        ("insert into invites (code, guild_id, inviter_id) select 'abcdefgh', $1, $2", vec![gone, sam.id]),
        ("insert into members (guild_id, user_id) values ($1, $2)", vec![gone, sam.id]),
        ("insert into roles (guild_id, name, position, permissions) values ($1, 'r', 1, 0)", vec![gone]),
    ];
    for (sql, binds) in &vanished {
        assert_eq!(db_error(&db, sql, binds).await, DbError::NotFound, "{sql}");
    }
    // Les autres clés étrangères ne sont pas masquées : ce sont des défauts, pas une cible disparue.
    let broken = [
        ("insert into messages (channel_id, author_id, content) values ($1, $2, 'x')", vec![general, gone]),
        (
            "insert into messages (channel_id, author_id, content, reply_to_id) values ($1, $2, 'x', $3)",
            vec![general, sam.id, gone],
        ),
        ("insert into read_states (user_id, channel_id) values ($1, $2)", vec![gone, general]),
        ("insert into members (guild_id, user_id) values ($1, $2)", vec![gid, gone]),
        ("update guilds set system_channel_id = $2 where id = $1", vec![gid, gone]),
        ("update channels set parent_id = $2 where id = $1", vec![general, gone]),
    ];
    for (sql, binds) in &broken {
        assert!(matches!(db_error(&db, sql, binds).await, DbError::Internal(_)), "{sql}");
    }
    // Une unicité reste une unicité.
    assert_eq!(
        db_error(&db, "insert into members (guild_id, user_id) values ($1, $2)", &[gid, sam.id]).await,
        DbError::Conflict("unique")
    );
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn sending_while_the_channel_is_deleted_says_not_found(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (g, _) = guild_with(&db, &own, &[&sam]).await;
    let doomed = db
        .create_channel(
            g.guild.id,
            own.id,
            &CreateChannel { name: "bientot".into(), kind: ChannelKind::Text, parent_id: None },
        )
        .await
        .unwrap()
        .id;
    // La suppression a eu lieu mais n'est pas encore validée : l'envoi passe la vérification du
    // salon, puis attend la clé étrangère... et la trouve supprimée.
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("delete from channels where id = $1").bind(doomed).execute(&mut *tx).await.unwrap();
    let sender = {
        let db = db.clone();
        tokio::spawn(async move { db.create_message(doomed, sam.id, "trop tard", None).await.map(|_| ()) })
    };
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(!sender.is_finished(), "l'envoi doit attendre la suppression");
    tx.commit().await.unwrap();
    assert_eq!(sender.await.unwrap(), Err(DbError::NotFound));
    // Pareil pour l'accusé de lecture d'un salon qui vient de disparaître.
    let m = db
        .create_message(g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id, sam.id, "ok", None)
        .await
        .unwrap()
        .0;
    let mut gone = m.clone();
    gone.channel_id = doomed;
    assert_eq!(db.record_message(&gone, Some(g.guild.id), &[own.id]).await, Err(DbError::NotFound));
}

// ───────── plafond d'invitations ─────────

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_person_holds_fifty_active_invitations_per_server_at_most(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (g, general) = guild_with(&db, &own, &[&sam]).await;
    let gid = g.guild.id;
    let other = db.create_guild(sam.id, "Ailleurs").await.unwrap();
    let elsewhere = other.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let make = |who: Id| {
        let db = db.clone();
        async move { db.create_invite(gid, Some(general), who, 0, None).await.map(|i| i.code) }
    };

    // « guild_with » a déjà donné une invitation au propriétaire ; sam monte jusqu'au plafond.
    let mut codes = Vec::new();
    for _ in 0..MAX_INVITES_PER_MEMBER {
        codes.push(make(sam.id).await.unwrap());
    }
    assert_eq!(make(sam.id).await, Err(DbError::Invalid("invite_limit")));
    assert_eq!(
        audit(&db, "invite_create").await as usize,
        1 + MAX_INVITES_PER_MEMBER,
        "la refusée n'est pas journalisée"
    );
    // Le plafond est par personne et par serveur.
    make(own.id).await.unwrap();
    db.create_invite(other.guild.id, Some(elsewhere), sam.id, 0, None).await.unwrap();

    // Révoquer en libère une ; une périmée ou une épuisée ne compte plus.
    db.revoke_invite(&codes[0], sam.id).await.unwrap();
    codes.push(make(sam.id).await.unwrap());
    assert_eq!(make(sam.id).await, Err(DbError::Invalid("invite_limit")));
    sqlx::query("update invites set expires_at = now() - interval '1 minute' where code = $1")
        .bind(&codes[1])
        .execute(&db.pool)
        .await
        .unwrap();
    codes.push(make(sam.id).await.unwrap());
    assert_eq!(make(sam.id).await, Err(DbError::Invalid("invite_limit")));
    sqlx::query("update invites set max_uses = 1, uses = 1 where code = $1")
        .bind(&codes[2])
        .execute(&db.pool)
        .await
        .unwrap();
    codes.push(make(sam.id).await.unwrap());
    assert_eq!(make(sam.id).await, Err(DbError::Invalid("invite_limit")));
    // Une invitation limitée dans le temps ou en usages compte tant qu'elle sert.
    db.revoke_invite(&codes[3], sam.id).await.unwrap();
    db.create_invite(gid, Some(general), sam.id, 5, Some(3600)).await.unwrap();
    assert_eq!(make(sam.id).await, Err(DbError::Invalid("invite_limit")));

    // Deux demandes à la fois au pied du plafond : une seule passe.
    db.revoke_invite(&codes[4], sam.id).await.unwrap();
    db.revoke_invite(&codes[5], sam.id).await.unwrap();
    let racers: Vec<_> = (0..6).map(|_| tokio::spawn(make(sam.id))).collect();
    let mut won = 0;
    for r in racers {
        match r.await.unwrap() {
            Ok(_) => won += 1,
            Err(e) => assert_eq!(e, DbError::Invalid("invite_limit")),
        }
    }
    assert_eq!(won, 2, "il restait deux places");
    let active: i64 = sqlx::query_scalar(
        "select count(*) from invites where guild_id = $1 and inviter_id = $2
           and (expires_at is null or expires_at > now()) and (max_uses = 0 or uses < max_uses)",
    )
    .bind(gid)
    .bind(sam.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(active as usize, MAX_INVITES_PER_MEMBER);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn the_invite_list_never_holds_more_than_two_hundred_rows_active_ones_first(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let (g, general) = guild_with(&db, &own, &[]).await;
    let gid = g.guild.id;
    // 150 mortes (créées en premier, donc plus anciennes) puis 120 vivantes, posées directement.
    sqlx::query(
        "insert into invites (code, guild_id, channel_id, inviter_id, expires_at, created_at)
         select 'dead' || lpad(n::text, 4, '0'), $1, $2, $3, now() - interval '1 hour', now() - interval '1 day' - make_interval(secs => n)
         from generate_series(1, 150) n",
    )
    .bind(gid)
    .bind(general)
    .bind(own.id)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "insert into invites (code, guild_id, channel_id, inviter_id, created_at)
         select 'live' || lpad(n::text, 4, '0'), $1, $2, $3, now() - make_interval(secs => n) from generate_series(1, 120) n",
    )
    .bind(gid)
    .bind(general)
    .bind(own.id)
    .execute(&db.pool)
    .await
    .unwrap();
    let list = db.guild_invites(gid, own.id).await.unwrap();
    assert_eq!(list.len(), MAX_INVITE_ROWS);
    // 121 vivantes (celle de guild_with comprise), puis les mortes les plus récentes.
    let active = list.iter().filter(|i| i.state == InviteState::Active).count();
    assert_eq!(active, 121);
    assert!(list[..active].iter().all(|i| i.state == InviteState::Active));
    assert!(list[active..].iter().all(|i| i.state != InviteState::Active));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn the_rail_invite_reuses_the_endless_one_instead_of_filling_the_cap(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (g, general) = guild_with(&db, &own, &[&sam]).await;
    let gid = g.guild.id;
    assert!(db.reusable_invite(gid, general, sam.id).await.unwrap().is_none());
    let first = db.create_invite(gid, Some(general), sam.id, 0, None).await.unwrap();
    assert_eq!(db.reusable_invite(gid, general, sam.id).await.unwrap().unwrap().code, first.code);
    // Ni celle d'une autre personne, ni une invitation à durée ou à usages limités.
    let eve = user(&db, "eve").await;
    db.create_invite(gid, Some(general), eve.id, 5, None).await.unwrap();
    db.create_invite(gid, Some(general), eve.id, 0, Some(3600)).await.unwrap();
    assert!(db.reusable_invite(gid, general, eve.id).await.unwrap().is_none());
}
