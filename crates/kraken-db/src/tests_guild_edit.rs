//! Modèles de serveur à la création, icône du serveur et renommage : structure bâtie, modèle inconnu refusé,
//! droit « Gérer le serveur », règles de nom, journal d'audit, fichiers qui partent avec le serveur.

use kraken_core::templates;
use kraken_proto::{AuditCategory, ChannelKind, CreateRole};
use sqlx::PgPool;

use super::*;

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

/// Un serveur à `owner` avec `others` dedans ; rend son id.
async fn guild_with(db: &Db, owner: &User, others: &[&User]) -> Id {
    let g = db.create_guild(owner.id, "Abysse", None).await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), owner.id, 0, None).await.unwrap();
    for u in others {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    g.guild.id
}

async fn audit_of(db: &Db, gid: Id, by: Id, action: &str) -> Vec<AuditEntry> {
    let q = AuditQuery { limit: 100, ..Default::default() };
    db.audit_log(gid, by, &q).await.unwrap().entries.into_iter().filter(|e| e.action == action).collect()
}

/// `(catégorie, [(genre, nom)])` de ce que `create_guild` a bâti, dans l'ordre.
fn shape(g: &GuildState) -> Vec<(String, Vec<(ChannelKind, String)>)> {
    let mut cats: Vec<&Channel> = g.channels.iter().filter(|c| c.kind == ChannelKind::Category).collect();
    cats.sort_by_key(|c| (c.position, c.id));
    cats.iter()
        .map(|cat| {
            let mut kids: Vec<&Channel> = g.channels.iter().filter(|c| c.parent_id == Some(cat.id)).collect();
            kids.sort_by_key(|c| (c.position, c.id));
            (cat.name.clone().unwrap(), kids.iter().map(|c| (c.kind, c.name.clone().unwrap())).collect())
        })
        .collect()
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn every_template_builds_its_structure_and_names_the_first_text_channel_as_system(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    for t in &templates::ALL {
        let g = db.create_guild(own.id, "Abysse", Some(t.id)).await.unwrap();
        let want: Vec<_> = t
            .groups
            .iter()
            .map(|grp| {
                (
                    grp.name.to_string(),
                    grp.slots
                        .iter()
                        .map(|s| {
                            let kind = match s.kind {
                                templates::SlotKind::Text => ChannelKind::Text,
                                templates::SlotKind::Voice => ChannelKind::Voice,
                            };
                            (kind, s.name.to_string())
                        })
                        .collect(),
                )
            })
            .collect();
        assert_eq!(shape(&g), want, "{}", t.id);
        // Les noms des salons textuels sont ceux que `clean_channel_name` garde tels quels.
        for c in g.channels.iter().filter(|c| c.kind == ChannelKind::Text) {
            let n = c.name.as_deref().unwrap();
            assert_eq!(clean_channel_name(ChannelKind::Text, n).as_deref(), Ok(n), "{}", t.id);
        }
        // Salon système : le premier salon textuel du modèle.
        let system: Option<Id> = sqlx::query_scalar("select system_channel_id from guilds where id = $1")
            .bind(g.guild.id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let first = g
            .channels
            .iter()
            .find(|c| c.name.as_deref() == t.first_text().map(|s| s.name) && c.kind == ChannelKind::Text);
        assert_eq!(system, first.map(|c| c.id), "{}", t.id);
        // Pas de rôle en plus de @everyone, un seul membre, et rien dans le journal.
        assert_eq!(g.roles.len(), 1, "{}", t.id);
        assert_eq!((g.members.len(), g.guild.icon.clone()), (1, None));
        assert!(
            db.audit_log(g.guild.id, own.id, &AuditQuery { limit: 10, ..Default::default() })
                .await
                .unwrap()
                .entries
                .is_empty()
        );
    }
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn without_a_template_the_server_is_the_empty_one(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let plain = db.create_guild(own.id, "Abysse", None).await.unwrap();
    let vide = db.create_guild(own.id, "Abysse", Some("vide")).await.unwrap();
    assert_eq!(shape(&plain), shape(&vide));
    assert_eq!(
        shape(&plain),
        [
            ("Salons textuels".to_string(), vec![(ChannelKind::Text, "général".to_string())]),
            ("Salons vocaux".to_string(), vec![(ChannelKind::Voice, "Général".to_string())]),
        ]
    );
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn an_unknown_template_is_refused_and_nothing_is_written(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    for bad in ["", "Vide", "createurs", "../etc", "etudes ", "ETUDES"] {
        assert_eq!(
            db.create_guild(own.id, "Abysse", Some(bad)).await.map(|_| ()),
            Err(DbError::Invalid("template")),
            "{bad:?}"
        );
    }
    let guilds: i64 = sqlx::query_scalar("select count(*) from guilds").fetch_one(&db.pool).await.unwrap();
    assert_eq!(guilds, 0);
    // Le nom est jugé aussi, avec un bon modèle.
    assert_eq!(db.create_guild(own.id, "A", Some("jeux")).await.map(|_| ()), Err(DbError::Invalid("guild_name")));
    assert_eq!(
        db.create_guild(own.id, &"x".repeat(101), Some("jeux")).await.map(|_| ()),
        Err(DbError::Invalid("guild_name"))
    );
    db.create_guild(own.id, &"x".repeat(100), Some("jeux")).await.unwrap();
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn the_icon_is_set_replaced_and_removed_with_its_old_key_returned(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let gid = guild_with(&db, &own, &[]).await;

    let (g, old) = db.set_guild_icon(gid, own.id, Some("aaaa")).await.unwrap();
    assert_eq!((g.icon.as_deref(), old), (Some("aaaa"), None));
    assert_eq!(db.guild_state(gid, own.id).await.unwrap().unwrap().guild.icon.as_deref(), Some("aaaa"));

    let (g, old) = db.set_guild_icon(gid, own.id, Some("bbbb")).await.unwrap();
    assert_eq!((g.icon.as_deref(), old.as_deref()), (Some("bbbb"), Some("aaaa")));

    let (g, old) = db.set_guild_icon(gid, own.id, None).await.unwrap();
    assert_eq!((g.icon, old.as_deref()), (None, Some("bbbb")));
    assert_eq!(db.guild_state(gid, own.id).await.unwrap().unwrap().guild.icon, None);

    // Retirer ce qui n'existe pas : rien ne change, rien n'est écrit au journal.
    let (g, old) = db.set_guild_icon(gid, own.id, None).await.unwrap();
    assert_eq!((g.icon, old), (None, None));
    let entries = audit_of(&db, gid, own.id, "guild_update").await;
    // Du plus récent au plus ancien : retiré, changé, changé.
    let icons: Vec<_> = entries.iter().map(|e| e.details.get("icon").map(String::as_str)).collect();
    assert_eq!(icons, [Some("removed"), Some("changed"), Some("changed")]);
    assert!(
        entries
            .iter()
            .all(|e| e.changes.is_empty() && e.target.is_none() && e.actor.as_ref().map(|a| a.id) == Some(own.id))
    );
    // Jamais la clé du fichier.
    assert!(entries.iter().all(|e| e.details.values().all(|v| !v.contains("aaaa") && !v.contains("bbbb"))));
    assert_eq!(AuditCategory::of("guild_update"), Some(AuditCategory::Guild));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn renaming_follows_the_creation_rules_and_is_logged_once(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let gid = guild_with(&db, &own, &[]).await;

    let g = db.rename_guild(gid, own.id, "  Le Récif  ").await.unwrap();
    assert_eq!(g.name, "Le Récif", "espaces autour retirés");
    assert_eq!(db.guild_state(gid, own.id).await.unwrap().unwrap().guild.name, "Le Récif");
    // Même nom : rien de plus au journal.
    db.rename_guild(gid, own.id, "Le Récif").await.unwrap();
    let entries = audit_of(&db, gid, own.id, "guild_update").await;
    assert_eq!(entries.len(), 1);
    let c = &entries[0].changes;
    assert_eq!(
        (c.len(), c[0].field.as_str(), c[0].before.as_str(), c[0].after.as_str()),
        (1, "name", "Abysse", "Le Récif")
    );
    assert!(entries[0].details.is_empty());

    // Mêmes règles qu'à la création : 2 à 100 caractères, rien d'invisible.
    for (bad, err) in [
        ("", "guild_name"),
        (" ", "guild_name"),
        ("A", "guild_name"),
        (" A ", "guild_name"),
        (&"x".repeat(101), "guild_name"),
        ("Abys\u{202E}se", "hidden_chars"),
        ("\u{200B}\u{200B}", "hidden_chars"),
    ] {
        assert_eq!(db.rename_guild(gid, own.id, bad).await.map(|_| ()), Err(DbError::Invalid(err)), "{bad:?}");
    }
    db.rename_guild(gid, own.id, &"é".repeat(100)).await.unwrap();
    assert_eq!(db.rename_guild(gid, own.id, &"é".repeat(101)).await.map(|_| ()), Err(DbError::Invalid("guild_name")));
    assert_eq!(audit_of(&db, gid, own.id, "guild_update").await.len(), 2);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn editing_the_server_needs_manage_guild_or_ownership(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, adm, mem, out) =
        (user(&db, "lou").await, user(&db, "ada").await, user(&db, "sam").await, user(&db, "noa").await);
    let gid = guild_with(&db, &own, &[&adm, &mem]).await;

    // Un membre ordinaire : interdit ; un étranger : introuvable (on ne révèle rien).
    assert_eq!(db.rename_guild(gid, mem.id, "Autre").await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(db.set_guild_icon(gid, mem.id, Some("k")).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(db.set_guild_icon(gid, mem.id, None).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(db.check_guild_edit(gid, mem.id).await, Err(DbError::Forbidden("perm")));
    assert_eq!(db.rename_guild(gid, out.id, "Autre").await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.set_guild_icon(gid, out.id, Some("k")).await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.check_guild_edit(gid, out.id).await, Err(DbError::NotFound));
    assert_eq!(db.check_guild_edit(Id::nil(), own.id).await, Err(DbError::NotFound));
    assert_eq!(db.rename_guild(Id::nil(), own.id, "Autre").await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(db.guild_state(gid, own.id).await.unwrap().unwrap().guild.name, "Abysse");
    assert!(audit_of(&db, gid, own.id, "guild_update").await.is_empty());
    db.check_guild_edit(gid, own.id).await.unwrap();

    // Le droit « Gérer le serveur » suffit, sans être propriétaire.
    let r = db
        .create_role(
            gid,
            own.id,
            &CreateRole { name: "Gérants".into(), color: None, hoist: false, permissions: perms::MANAGE_GUILD },
        )
        .await
        .unwrap();
    db.set_member_role(gid, own.id, adm.id, r.id, true).await.unwrap();
    db.check_guild_edit(gid, adm.id).await.unwrap();
    db.rename_guild(gid, adm.id, "Chez Ada").await.unwrap();
    db.set_guild_icon(gid, adm.id, Some("kkkk")).await.unwrap();
    let entries = audit_of(&db, gid, own.id, "guild_update").await;
    assert!(entries.iter().all(|e| e.actor.as_ref().map(|a| a.id) == Some(adm.id)));

    // Exclu temporairement : plus aucun droit de gérer.
    db.timeout_member(gid, own.id, adm.id, 3600, None).await.unwrap();
    assert_eq!(db.check_guild_edit(gid, adm.id).await, Err(DbError::Forbidden("perm")));
    assert_eq!(db.rename_guild(gid, adm.id, "Encore").await.map(|_| ()), Err(DbError::Forbidden("perm")));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn the_icon_file_goes_with_the_deleted_server(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let gid = guild_with(&db, &own, &[]).await;
    let key = "0123456789abcdef0123456789abcdef";
    db.set_guild_icon(gid, own.id, Some(key)).await.unwrap();
    let out = db.delete_guild(gid, own.id, "Abysse").await.unwrap();
    for ext in ["png", "gif", "webp"] {
        assert!(out.files.contains(&format!("avatars/{key}.{ext}")), "{ext}");
    }
    // Sans icône : aucun fichier de ce genre.
    let gid = guild_with(&db, &own, &[]).await;
    let out = db.delete_guild(gid, own.id, "Abysse").await.unwrap();
    assert!(out.files.iter().all(|f| !f.starts_with("avatars/")));
}
