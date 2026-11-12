//! Journal d'audit : ce qui est écrit par chaque action (rôles, membres, salons, droits, invitations,
//! emojis, messages retirés par un modérateur), puis lu par pages avec filtres, noms résolus à la lecture
//! (ou gardés à l'écriture quand la cible a disparu), acteur supprimé, droit de lecture et liste blanche.

use kraken_proto::{
    AuditCategory, AuditTargetKind, BanMember, ChannelKind, CreateChannel, CreateRole, EditChannel, EditRole,
    MAX_AUDIT_PAGE, OverwriteKind, SetOverwrite,
};
use serde_json::json;
use sqlx::PgPool;

use super::*;

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

/// Un serveur à `owner` avec `others` dedans ; rend son id et celui de #général.
async fn guild_with(db: &Db, owner: &User, others: &[&User]) -> (Id, Id) {
    let g = db.create_guild(owner.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), owner.id, 0, None).await.unwrap();
    for u in others {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    (g.guild.id, general)
}

fn role(name: &str, permissions: u64) -> CreateRole {
    CreateRole { name: name.into(), color: None, hoist: false, permissions }
}

fn query(limit: u32) -> AuditQuery {
    AuditQuery { limit, ..Default::default() }
}

async fn all(db: &Db, gid: Id, by: Id) -> Vec<AuditEntry> {
    db.audit_log(gid, by, &query(MAX_AUDIT_PAGE)).await.unwrap().entries
}

fn of<'a>(entries: &'a [AuditEntry], action: &str) -> Vec<&'a AuditEntry> {
    entries.iter().filter(|e| e.action == action).collect()
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn only_members_with_the_permission_read_the_log(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, adm, aud, plain, out) = (
        user(&db, "lou").await,
        user(&db, "ada").await,
        user(&db, "val").await,
        user(&db, "sam").await,
        user(&db, "noa").await,
    );
    let (gid, _) = guild_with(&db, &own, &[&adm, &aud, &plain]).await;
    let admin = db.create_role(gid, own.id, &role("Admins", perms::ADMINISTRATOR)).await.unwrap();
    db.set_member_role(gid, own.id, adm.id, admin.id, true).await.unwrap();
    let auditors = db.create_role(gid, own.id, &role("Greffiers", perms::VIEW_AUDIT_LOG)).await.unwrap();
    db.set_member_role(gid, own.id, aud.id, auditors.id, true).await.unwrap();

    // Propriétaire, administrateur et porteur du droit lisent ; un simple membre est refusé ; un étranger
    // ne sait même pas que le serveur existe.
    for who in [&own, &adm, &aud] {
        assert!(db.audit_log(gid, who.id, &query(10)).await.is_ok(), "{}", who.username);
    }
    assert_eq!(db.audit_log(gid, plain.id, &query(10)).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(db.audit_log(gid, out.id, &query(10)).await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.audit_log(Id::nil(), own.id, &query(10)).await.map(|_| ()), Err(DbError::NotFound));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn every_action_reads_back_with_its_names(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam, lea, gus) =
        (user(&db, "lou").await, user(&db, "sam").await, user(&db, "lea").await, user(&db, "gus").await);
    db.set_display_name(own.id, Some("Lou la pieuvre")).await.unwrap();
    let (gid, general) = guild_with(&db, &own, &[&sam, &lea, &gus]).await;

    // Rôles : création, modification (avant / après), attribution, retrait.
    let r = db.create_role(gid, own.id, &role("Modos", perms::KICK)).await.unwrap();
    db.edit_role(
        gid,
        r.id,
        own.id,
        &EditRole {
            name: Some("Modérateurs".into()),
            color: Some(3),
            hoist: Some(true),
            permissions: Some(perms::KICK | perms::BAN),
        },
    )
    .await
    .unwrap();
    db.set_member_role(gid, own.id, sam.id, r.id, true).await.unwrap();
    db.set_member_role(gid, own.id, sam.id, r.id, false).await.unwrap();
    // Modération.
    db.timeout_member(gid, own.id, lea.id, 3600, Some("trop de bruit")).await.unwrap();
    db.remove_timeout(gid, own.id, lea.id).await.unwrap();
    db.kick_member(gid, own.id, gus.id, Some("hors sujet")).await.unwrap();
    db.ban_member(gid, own.id, gus.id, &BanMember { reason: Some("récidive".into()), delete_hours: None })
        .await
        .unwrap();
    db.unban(gid, own.id, gus.id).await.unwrap();
    db.transfer_ownership(gid, own.id, sam.id).await.unwrap();
    db.transfer_ownership(gid, sam.id, own.id).await.unwrap();
    // Salons.
    let c = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "Annonces".into(), kind: ChannelKind::Text, parent_id: None },
        )
        .await
        .unwrap();
    db.edit_channel(
        c.id,
        own.id,
        &EditChannel {
            name: Some("infos".into()),
            topic: Some("Les nouvelles".into()),
            slowmode_s: Some(5),
            private: Some(true),
        },
    )
    .await
    .unwrap();
    db.set_overwrite(
        c.id,
        own.id,
        r.id,
        &SetOverwrite { kind: OverwriteKind::Role, allow: perms::SEND_MESSAGES, deny: perms::ADD_REACTIONS },
    )
    .await
    .unwrap();
    db.set_overwrite(
        c.id,
        own.id,
        lea.id,
        &SetOverwrite { kind: OverwriteKind::Member, allow: perms::ATTACH_FILES, deny: 0 },
    )
    .await
    .unwrap();
    // Invitations.
    let inv = db.create_invite(gid, Some(general), sam.id, 5, Some(3600)).await.unwrap();
    db.revoke_invite(&inv.code, own.id).await.unwrap();
    // Emojis.
    let e = db.add_emoji(gid, own.id, "poulpe", true, "k.gif").await.unwrap();
    db.rename_emoji(gid, e.id, own.id, "pieuvre").await.unwrap();

    let entries = all(&db, gid, own.id).await;
    // Du plus récent au plus ancien.
    let ids: Vec<_> = entries.iter().map(|e| e.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(ids, sorted);
    assert!(entries.iter().all(|e| e.created_at > 1_700_000_000_000));
    let actor = entries[0].actor.clone().unwrap();
    assert_eq!((actor.id, actor.name.as_str()), (own.id, "Lou la pieuvre"));

    let name = |e: &AuditEntry| e.target.as_ref().and_then(|t| t.name.clone());
    let detail = |e: &AuditEntry, k: &str| e.details.get(k).cloned();

    // Rôles.
    assert_eq!(name(of(&entries, "role_create")[0]).as_deref(), Some("Modérateurs"), "nom lu maintenant");
    let upd = of(&entries, "role_update")[0];
    let changes: Vec<_> = upd.changes.iter().map(|c| (c.field.as_str(), c.before.as_str(), c.after.as_str())).collect();
    let (kick, kick_ban) = (perms::KICK.to_string(), (perms::KICK | perms::BAN).to_string());
    assert_eq!(
        changes,
        [
            ("name", "Modos", "Modérateurs"),
            ("color", "", "3"),
            ("hoist", "false", "true"),
            ("permissions", kick.as_str(), kick_ban.as_str())
        ]
    );
    assert_eq!(of(&entries, "member_role_add").len(), 1);
    let add = of(&entries, "member_role_add")[0];
    assert_eq!((name(add).as_deref(), detail(add, "role").as_deref()), (Some("sam"), Some("Modérateurs")));
    assert_eq!(add.target.as_ref().unwrap().kind, AuditTargetKind::User);
    assert_eq!(detail(of(&entries, "member_role_remove")[0], "role").as_deref(), Some("Modérateurs"));
    // Modération.
    let t = of(&entries, "member_timeout")[0];
    assert_eq!(
        (name(t).as_deref(), detail(t, "seconds").as_deref(), t.reason.as_deref()),
        (Some("lea"), Some("3600"), Some("trop de bruit"))
    );
    assert_eq!(name(of(&entries, "member_timeout_remove")[0]).as_deref(), Some("lea"));
    let kick = of(&entries, "member_kick")[0];
    assert_eq!((name(kick).as_deref(), kick.reason.as_deref()), (Some("gus"), Some("hors sujet")));
    assert_eq!(of(&entries, "member_ban")[0].reason.as_deref(), Some("récidive"));
    assert_eq!(name(of(&entries, "member_unban")[0]).as_deref(), Some("gus"));
    let owners = of(&entries, "owner_transfer");
    assert_eq!(owners.len(), 2);
    assert_eq!(name(owners[0]).as_deref(), Some("Lou la pieuvre"), "le plus récent : rendu à lou");
    assert_eq!(name(owners[1]).as_deref(), Some("sam"));
    // Salons et droits.
    let created = of(&entries, "channel_create").into_iter().find(|e| name(e).as_deref() == Some("infos")).unwrap();
    assert_eq!(detail(created, "kind").as_deref(), Some("text"));
    assert_eq!(created.target.as_ref().unwrap().id, c.id);
    let upd = of(&entries, "channel_update")[0];
    let changes: Vec<_> = upd.changes.iter().map(|c| (c.field.as_str(), c.before.as_str(), c.after.as_str())).collect();
    assert_eq!(
        changes,
        [
            ("name", "annonces", "infos"),
            ("topic", "", "Les nouvelles"),
            ("slowmode_s", "0", "5"),
            ("private", "false", "true")
        ]
    );
    let ow = of(&entries, "overwrite_update");
    assert_eq!(ow.len(), 2);
    assert_eq!(name(ow[1]).as_deref(), Some("infos"));
    assert_eq!(
        (detail(ow[1], "subject_kind").as_deref(), detail(ow[1], "subject").as_deref()),
        (Some("role"), Some("Modérateurs"))
    );
    assert_eq!(detail(ow[1], "allow"), Some(perms::SEND_MESSAGES.to_string()));
    assert_eq!(detail(ow[1], "deny"), Some(perms::ADD_REACTIONS.to_string()));
    assert_eq!(
        (detail(ow[0], "subject_kind").as_deref(), detail(ow[0], "subject").as_deref()),
        (Some("member"), Some("lea"))
    );
    // Invitations : jamais le code, mais ce qu'il faut pour la phrase.
    let inv_create = of(&entries, "invite_create").into_iter().find(|e| e.details.contains_key("max_uses")).unwrap();
    assert_eq!(detail(inv_create, "max_uses").as_deref(), Some("5"));
    assert_eq!(detail(inv_create, "max_age_s").as_deref(), Some("3600"));
    assert_eq!(detail(inv_create, "channel").as_deref(), Some("général"));
    assert!(inv_create.target.is_none());
    assert_eq!(detail(of(&entries, "invite_revoke")[0], "inviter").as_deref(), Some("sam"));
    let all_json = serde_json::to_string(&entries).unwrap();
    assert!(!all_json.contains(&inv.code), "le code d'une invitation ne sort pas du journal");
    // Emojis.
    assert_eq!(detail(of(&entries, "emoji_create")[0], "animated").as_deref(), Some("true"));
    let ren = of(&entries, "emoji_update")[0];
    assert_eq!(
        (name(ren).as_deref(), ren.changes[0].before.as_str(), ren.changes[0].after.as_str()),
        (Some("pieuvre"), "poulpe", "pieuvre")
    );
    assert_eq!(of(&entries, "emoji_create")[0].target.as_ref().unwrap().kind, AuditTargetKind::Emoji);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn removed_targets_fall_back_to_the_name_kept_at_write(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (gid, _) = guild_with(&db, &own, &[&sam]).await;
    let r = db.create_role(gid, own.id, &role("Éphémère", 0)).await.unwrap();
    let c = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "brouillon".into(), kind: ChannelKind::Voice, parent_id: None },
        )
        .await
        .unwrap();
    db.edit_channel(
        c.id,
        own.id,
        &EditChannel { topic: None, slowmode_s: None, private: Some(true), name: Some("secret".into()) },
    )
    .await
    .unwrap();
    db.set_overwrite(c.id, own.id, r.id, &SetOverwrite { kind: OverwriteKind::Role, allow: perms::CONNECT, deny: 0 })
        .await
        .unwrap();
    let e = db.add_emoji(gid, own.id, "vague", false, "k.png").await.unwrap();
    db.edit_role(gid, r.id, own.id, &EditRole { name: Some("Passager".into()), ..Default::default() }).await.unwrap();
    db.set_member_role(gid, own.id, sam.id, r.id, true).await.unwrap();

    db.delete_channel(c.id, own.id).await.unwrap();
    db.delete_role(gid, r.id, own.id).await.unwrap();
    db.delete_emoji(gid, e.id, own.id).await.unwrap();
    db.kick_member(gid, own.id, sam.id, None).await.unwrap();
    // Le compte de la personne renvoyée disparaît : plus de nom du tout, on n'en garde pas la trace.
    sqlx::query("delete from users where id = $1").bind(sam.id).execute(&db.pool).await.unwrap();

    let entries = all(&db, gid, own.id).await;
    let name = |action: &str, nth: usize| of(&entries, action)[nth].target.as_ref().unwrap().name.clone();
    // Le salon, le rôle et l'emoji n'existent plus : le nom gardé à l'écriture (le dernier connu).
    assert_eq!(name("channel_delete", 0).as_deref(), Some("secret"));
    assert_eq!(name("channel_update", 0).as_deref(), Some("secret"));
    assert_eq!(name("channel_create", 0).as_deref(), Some("brouillon"));
    assert_eq!(name("overwrite_update", 0).as_deref(), Some("secret"));
    assert_eq!(name("role_delete", 0).as_deref(), Some("Passager"));
    assert_eq!(name("role_update", 0).as_deref(), Some("Passager"));
    assert_eq!(name("role_create", 0).as_deref(), Some("Éphémère"));
    assert_eq!(name("emoji_create", 0).as_deref(), Some("vague"));
    assert_eq!(name("emoji_delete", 0).as_deref(), Some("vague"));
    // Le rôle cité par une attribution garde son nom d'alors, le sujet d'une surcharge n'a plus de nom.
    assert_eq!(of(&entries, "member_role_add")[0].details.get("role").map(String::as_str), Some("Passager"));
    assert_eq!(of(&entries, "overwrite_update")[0].details.get("subject"), None);
    assert_eq!(of(&entries, "overwrite_update")[0].details.get("subject_kind").map(String::as_str), Some("role"));
    // Une personne dont le compte a disparu : cible sans nom (l'id reste).
    let kick = of(&entries, "member_kick")[0];
    assert_eq!((kick.target.as_ref().unwrap().id, kick.target.as_ref().unwrap().name.clone()), (sam.id, None));
    // Les noms de lecture sont des noms vivants : le salon renommé s'appelait « brouillon » à sa création.
    let before_rename = of(&entries, "channel_update")[0].changes[0].clone();
    assert_eq!((before_rename.before.as_str(), before_rename.after.as_str()), ("brouillon", "secret"));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_deleted_actor_reads_as_none_and_old_rows_still_read(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mem) = (user(&db, "lou").await, user(&db, "sam").await);
    let (gid, _) = guild_with(&db, &own, &[&mem]).await;
    let r =
        db.create_role(gid, own.id, &role("Greffiers", perms::VIEW_AUDIT_LOG | perms::MANAGE_EMOJIS)).await.unwrap();
    db.set_member_role(gid, own.id, mem.id, r.id, true).await.unwrap();
    db.add_emoji(gid, mem.id, "vague", false, "k.png").await.unwrap();
    // Une ligne écrite avant que le journal garde les noms : action inconnue de cette version, et un
    // `changes` qui n'est pas ce que l'on attend.
    sqlx::query("insert into audit_log (guild_id, actor_id, action, target_id, changes) values ($1, $2, 'futur_inconnu', $3, $4)")
        .bind(gid)
        .bind(mem.id)
        .bind(own.id)
        .bind(sqlx::types::Json(json!({ "secret": "x", "name": { "old": 1 } })))
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("insert into audit_log (guild_id, actor_id, action, changes) values ($1, $2, 'channel_update', $3)")
        .bind(gid)
        .bind(own.id)
        .bind(sqlx::types::Json(json!({ "name": "ancien", "topic": null, "slowmode_s": null, "private": true })))
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("delete from users where id = $1").bind(mem.id).execute(&db.pool).await.unwrap();

    let page = db.audit_log(gid, own.id, &query(50)).await.unwrap();
    let unknown = of(&page.entries, "futur_inconnu")[0];
    assert!(unknown.actor.is_none(), "compte supprimé");
    assert!(unknown.target.is_none(), "action inconnue : pas de cible");
    assert!(unknown.details.is_empty() && unknown.changes.is_empty(), "rien de la ligne brute ne sort");
    assert!(of(&page.entries, "emoji_create")[0].actor.is_none());
    // Ancienne écriture d'un salon modifié : lue sans avant / après, sans planter.
    let old = of(&page.entries, "channel_update")[0];
    assert!(old.changes.is_empty() && old.target.is_none());
    // Ceux qui ont agi : plus le compte supprimé.
    assert_eq!(page.actors.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["lou"]);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn pages_follow_the_cursor_and_filters_narrow_them(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (gid, _) = guild_with(&db, &own, &[&sam]).await;
    let r =
        db.create_role(gid, own.id, &role("Greffiers", perms::MANAGE_EMOJIS | perms::VIEW_AUDIT_LOG)).await.unwrap();
    db.set_member_role(gid, own.id, sam.id, r.id, true).await.unwrap();
    for n in 0..7 {
        db.add_emoji(gid, sam.id, &format!("e{n}"), false, &format!("k{n}.png")).await.unwrap();
    }
    for n in 0..4 {
        db.create_channel(
            gid,
            own.id,
            &CreateChannel { name: format!("salon {n}"), kind: ChannelKind::Text, parent_id: None },
        )
        .await
        .unwrap();
    }
    let total = all(&db, gid, own.id).await.len();
    assert!(total >= 13, "{total}");

    // Pages de 5 : chaque entrée une seule fois, dans l'ordre, la dernière page sans suite.
    let mut seen = vec![];
    let mut before = None;
    let mut pages = 0;
    loop {
        let page = db.audit_log(gid, own.id, &AuditQuery { before, limit: 5, ..Default::default() }).await.unwrap();
        pages += 1;
        // La liste de ceux qui ont agi n'accompagne que la première page.
        assert_eq!(page.actors.is_empty(), before.is_some());
        assert!(page.entries.len() <= 5);
        seen.extend(page.entries.iter().map(|e| e.id));
        match page.next {
            Some(n) => {
                assert_eq!(Some(n), page.entries.last().map(|e| e.id));
                before = Some(n);
            }
            None => break,
        }
    }
    assert_eq!(seen.len(), total);
    assert_eq!(pages, total.div_ceil(5));
    let mut sorted = seen.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    sorted.dedup();
    assert_eq!(seen, sorted);

    // La limite est bornée à [1, 100].
    assert_eq!(db.audit_log(gid, own.id, &query(0)).await.unwrap().entries.len(), 1);
    assert_eq!(db.audit_log(gid, own.id, &query(u32::MAX)).await.unwrap().entries.len(), total);
    // Une page pile : pas de suite.
    let exact = db.audit_log(gid, own.id, &query(total as u32)).await.unwrap();
    assert_eq!((exact.entries.len(), exact.next), (total, None));
    // Un curseur inconnu mais bien formé : les entrées plus anciennes que lui, ici toutes ou aucune.
    assert!(
        db.audit_log(gid, own.id, &AuditQuery { before: Some(Id::nil()), limit: 10, ..Default::default() })
            .await
            .unwrap()
            .entries
            .is_empty()
    );

    // Filtre par famille.
    let cat = |c| AuditQuery { limit: 100, category: Some(c), ..Default::default() };
    let emojis = db.audit_log(gid, own.id, &cat(AuditCategory::Emojis)).await.unwrap().entries;
    assert_eq!(emojis.len(), 7);
    assert!(emojis.iter().all(|e| e.action == "emoji_create"));
    let channels = db.audit_log(gid, own.id, &cat(AuditCategory::Channels)).await.unwrap().entries;
    assert!(channels.len() >= 4 && channels.iter().all(|e| e.action.starts_with("channel_")));
    let members = db.audit_log(gid, own.id, &cat(AuditCategory::Members)).await.unwrap().entries;
    assert_eq!(members.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), ["member_role_add"]);
    assert!(!db.audit_log(gid, own.id, &cat(AuditCategory::Invites)).await.unwrap().entries.is_empty());
    assert!(db.audit_log(gid, own.id, &cat(AuditCategory::Guild)).await.unwrap().entries.is_empty());
    // Filtre par personne : elle seule, et la page compte ce qu'elle a fait.
    let by_sam =
        db.audit_log(gid, own.id, &AuditQuery { limit: 100, actor: Some(sam.id), ..Default::default() }).await.unwrap();
    assert_eq!(by_sam.entries.len(), 7);
    assert!(by_sam.entries.iter().all(|e| e.actor.as_ref().map(|a| a.id) == Some(sam.id)));
    // Les deux ensemble, et la liste de ceux qui ont agi ne rétrécit pas avec le filtre.
    let both = db
        .audit_log(
            gid,
            own.id,
            &AuditQuery {
                limit: 100,
                actor: Some(sam.id),
                category: Some(AuditCategory::Channels),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(both.entries.is_empty());
    assert_eq!(both.actors.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["lou", "sam"]);
    // Quelqu'un qui n'a rien fait, ou que l'on ne connaît pas : une page vide, pas une erreur.
    let nobody = db
        .audit_log(
            gid,
            own.id,
            &AuditQuery { limit: 10, actor: Some(Id::from_u128(0xdead_beef)), ..Default::default() },
        )
        .await
        .unwrap();
    assert!(nobody.entries.is_empty() && nobody.next.is_none());
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_moderator_removing_someone_elses_message_leaves_a_trace_without_its_text(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, sam) = (user(&db, "lou").await, user(&db, "sam").await);
    let (gid, general) = guild_with(&db, &own, &[&sam]).await;
    let draft = |content| Draft { content, ..Default::default() };
    let (mine, _) = db.send_message(general, own.id, draft("mon message")).await.unwrap();
    let (theirs, _) = db.send_message(general, sam.id, draft("un secret très secret")).await.unwrap();
    let (own_too, _) = db.send_message(general, sam.id, draft("encore moi")).await.unwrap();

    // Retirer le sien : rien dans le journal. Un autre sans le droit : refusé, rien non plus.
    db.delete_message(general, own_too.id, sam.id, false).await.unwrap();
    assert_eq!(db.delete_message(general, mine.id, sam.id, false).await, Err(DbError::Forbidden("author")));
    db.delete_message(general, mine.id, own.id, true).await.unwrap();
    assert!(of(&all(&db, gid, own.id).await, "message_delete").is_empty());

    // Un modérateur retire celui d'un autre : cible = l'auteur, salon en détail, jamais le texte.
    db.delete_message(general, theirs.id, own.id, true).await.unwrap();
    let entries = all(&db, gid, own.id).await;
    let e = of(&entries, "message_delete");
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].actor.as_ref().map(|a| a.id), Some(own.id));
    let t = e[0].target.as_ref().unwrap();
    assert_eq!((t.kind, t.id, t.name.as_deref()), (AuditTargetKind::User, sam.id, Some("sam")));
    assert_eq!(e[0].details.get("channel").map(String::as_str), Some("général"));
    let json = serde_json::to_string(&entries).unwrap();
    assert!(!json.contains("secret"), "le texte du message ne sort pas");
    let stored: serde_json::Value = sqlx::query_scalar("select changes from audit_log where action = 'message_delete'")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(!stored.to_string().contains("secret"), "et il n'est pas écrit non plus");
    // Rangé avec « Serveur ».
    let guild = db
        .audit_log(gid, own.id, &AuditQuery { limit: 10, category: Some(AuditCategory::Guild), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(guild.entries.len(), 1);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_hundred_entries_read_in_one_page_and_the_log_goes_with_the_guild(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let (gid, _) = guild_with(&db, &own, &[]).await;
    sqlx::query(
        "insert into audit_log (guild_id, actor_id, action, target_id, changes)
         select $1, $2, 'role_create', gen_random_uuid(), jsonb_build_object('name', 'r' || g) from generate_series(1, 250) g",
    )
    .bind(gid)
    .bind(own.id)
    .execute(&db.pool)
    .await
    .unwrap();
    let page = db.audit_log(gid, own.id, &query(MAX_AUDIT_PAGE)).await.unwrap();
    assert_eq!(page.entries.len(), 100);
    assert!(page.next.is_some());
    // Chaque rôle supprimé garde le nom de l'écriture.
    assert!(
        page.entries
            .iter()
            .all(|e| e.target.as_ref().is_some_and(|t| t.name.as_deref().is_some_and(|n| n.starts_with('r'))))
    );
    // Un autre serveur ne voit rien du premier.
    let other = user(&db, "noa").await;
    let (g2, _) = guild_with(&db, &other, &[]).await;
    assert!(
        all(&db, g2, other.id).await.iter().all(|e| e.target.as_ref().is_none_or(|t| t.name.as_deref() != Some("r1")))
    );
    assert_eq!(db.audit_log(gid, other.id, &query(10)).await.map(|_| ()), Err(DbError::NotFound));
}
