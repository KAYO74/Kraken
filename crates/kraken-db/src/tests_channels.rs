//! Salons : droits de création, de modification et de suppression, mode lent, ordre, salons
//! privés et surcharges de droits (hiérarchie, droits que l'on n'a pas, verrouillage de soi).

use kraken_proto::{
    ChannelKind, CreateChannel, CreateRole, EditChannel, MAX_OVERWRITES, MAX_TOPIC, MoveChannel, OverwriteKind,
    SetOverwrite,
};
use sqlx::PgPool;

use super::*;

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

/// Un serveur à `owner` avec `others` dedans ; rend son état.
async fn guild_with(db: &Db, owner: &User, others: &[&User]) -> GuildState {
    let g = db.create_guild(owner.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), owner.id, 0, None).await.unwrap();
    for u in others {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    g
}

fn text(name: &str, parent: Option<Id>) -> CreateChannel {
    CreateChannel { name: name.into(), kind: ChannelKind::Text, parent_id: parent }
}

fn role(name: &str, permissions: u64) -> CreateRole {
    CreateRole { name: name.into(), color: None, hoist: false, permissions }
}

fn general(g: &GuildState) -> Id {
    g.channels.iter().find(|c| c.name.as_deref() == Some("général")).unwrap().id
}

fn category(g: &GuildState, name: &str) -> Id {
    g.channels.iter().find(|c| c.kind == ChannelKind::Category && c.name.as_deref() == Some(name)).unwrap().id
}

/// Noms des enfants de `parent` (ou des salons sans catégorie) vus par `uid`, dans l'ordre.
async fn names(db: &Db, gid: Id, uid: Id, parent: Option<Id>, categories: bool) -> Vec<String> {
    let mut v = db.guild_state(gid, uid).await.unwrap().unwrap().channels;
    v.retain(|c| c.parent_id == parent && (c.kind == ChannelKind::Category) == categories);
    v.sort_by_key(|c| (c.position, c.id));
    v.into_iter().map(|c| c.name.unwrap()).collect()
}

async fn audit(db: &Db, action: &str) -> i64 {
    sqlx::query_scalar("select count(*) from audit_log where action = $1")
        .bind(action)
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn only_channel_managers_create_and_names_are_cleaned(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, plain, out) = (user(&db, "lou").await, user(&db, "sam").await, user(&db, "noa").await);
    let g = guild_with(&db, &own, &[&plain]).await;
    let gid = g.guild.id;
    let texts = category(&g, "Salons textuels");

    // Un simple membre : 403 ; quelqu'un d'étranger : 404 (le serveur n'existe pas pour lui).
    assert_eq!(db.create_channel(gid, plain.id, &text("x", None)).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(db.create_channel(gid, out.id, &text("x", None)).await.map(|_| ()), Err(DbError::NotFound));
    // Le droit « Gérer les salons » suffit, sans être propriétaire.
    let r = db.create_role(gid, own.id, &role("Bâtisseurs", perms::MANAGE_CHANNELS)).await.unwrap();
    db.set_member_role(gid, own.id, plain.id, r.id, true).await.unwrap();

    // Un salon d'écriture : minuscules, mots liés par des tirets ; vocal et catégorie gardent la casse.
    let c = db.create_channel(gid, plain.id, &text("  Salon   de TEST ", Some(texts))).await.unwrap();
    assert_eq!((c.name.as_deref(), c.parent_id, c.position), (Some("salon-de-test"), Some(texts), 1));
    let v = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "Salle  des Cartes".into(), kind: ChannelKind::Voice, parent_id: None },
        )
        .await
        .unwrap();
    assert_eq!(v.name.as_deref(), Some("Salle des Cartes"));
    let cat = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "Archives".into(), kind: ChannelKind::Category, parent_id: None },
        )
        .await
        .unwrap();
    assert_eq!(cat.kind, ChannelKind::Category);

    // Noms et parents invalides.
    for bad in ["", "   ", "a\u{7}b", &"x".repeat(101)] {
        assert_eq!(
            db.create_channel(gid, own.id, &text(bad, None)).await.map(|_| ()),
            Err(DbError::Invalid("channel_name")),
            "{bad:?}"
        );
    }
    assert_eq!(
        db.create_channel(gid, own.id, &text("ok", Some(c.id))).await.map(|_| ()),
        Err(DbError::Invalid("parent_id")),
        "pas une catégorie"
    );
    assert_eq!(
        db.create_channel(gid, own.id, &text("ok", Some(Id::nil()))).await.map(|_| ()),
        Err(DbError::Invalid("parent_id"))
    );
    let nested = CreateChannel { name: "Sous".into(), kind: ChannelKind::Category, parent_id: Some(texts) };
    assert_eq!(db.create_channel(gid, own.id, &nested).await.map(|_| ()), Err(DbError::Invalid("parent_id")));
    let dm = CreateChannel { name: "mp".into(), kind: ChannelKind::Dm, parent_id: None };
    assert_eq!(db.create_channel(gid, own.id, &dm).await.map(|_| ()), Err(DbError::Invalid("kind")));
    assert_eq!(audit(&db, "channel_create").await, 3);

    // Plafond de salons par serveur.
    sqlx::query(
        "insert into channels (guild_id, kind, name, position) select $1, 'text', 'n' || g, g from generate_series(1, $2) g",
    )
    .bind(gid)
    .bind(MAX_CHANNELS as i32)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        db.create_channel(gid, own.id, &text("de-trop", None)).await.map(|_| ()),
        Err(DbError::Invalid("channels"))
    );
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn editing_a_channel_validates_and_needs_the_right(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, plain, out) = (user(&db, "lou").await, user(&db, "sam").await, user(&db, "noa").await);
    let g = guild_with(&db, &own, &[&plain]).await;
    let general = general(&g);
    let voice = g.channels.iter().find(|c| c.kind == ChannelKind::Voice).unwrap().id;
    let edit = |name: Option<&str>, topic: Option<&str>, slow: Option<u32>| EditChannel {
        name: name.map(Into::into),
        topic: topic.map(Into::into),
        slowmode_s: slow,
        private: None,
    };

    let c = db
        .edit_channel(general, own.id, &edit(Some("Café Du Port"), Some("  Bienvenue à bord  "), Some(30)))
        .await
        .unwrap();
    assert_eq!(
        (c.name.as_deref(), c.topic.as_deref(), c.slowmode_s),
        (Some("café-du-port"), Some("Bienvenue à bord"), 30)
    );
    // Champs absents : inchangés. Sujet vide : plus de sujet.
    let c = db.edit_channel(general, own.id, &edit(None, Some(""), None)).await.unwrap();
    assert_eq!((c.name.as_deref(), c.topic.as_deref(), c.slowmode_s), (Some("café-du-port"), None, 30));
    let c = db.edit_channel(general, own.id, &edit(None, None, Some(0))).await.unwrap();
    assert_eq!(c.slowmode_s, 0);

    // Valeurs refusées : délai hors liste, sujet trop long, sujet ou mode lent sur un salon vocal.
    assert_eq!(
        db.edit_channel(general, own.id, &edit(None, None, Some(7))).await.map(|_| ()),
        Err(DbError::Invalid("slowmode"))
    );
    assert_eq!(
        db.edit_channel(general, own.id, &edit(None, Some(&"x".repeat(MAX_TOPIC + 1)), None)).await.map(|_| ()),
        Err(DbError::Invalid("topic"))
    );
    assert_eq!(
        db.edit_channel(general, own.id, &edit(Some(" "), None, None)).await.map(|_| ()),
        Err(DbError::Invalid("channel_name"))
    );
    assert_eq!(
        db.edit_channel(voice, own.id, &edit(None, Some("sujet"), None)).await.map(|_| ()),
        Err(DbError::Invalid("topic"))
    );
    assert_eq!(
        db.edit_channel(voice, own.id, &edit(None, None, Some(5))).await.map(|_| ()),
        Err(DbError::Invalid("slowmode"))
    );
    assert_eq!(
        db.edit_channel(voice, own.id, &edit(Some("Cale Sèche"), None, None)).await.unwrap().name.as_deref(),
        Some("Cale Sèche")
    );

    // Sans le droit : 403. Étranger au serveur, ou salon inconnu, ou MP : 404.
    assert_eq!(
        db.edit_channel(general, plain.id, &edit(Some("x"), None, None)).await.map(|_| ()),
        Err(DbError::Forbidden("perm"))
    );
    assert_eq!(
        db.edit_channel(general, out.id, &edit(Some("x"), None, None)).await.map(|_| ()),
        Err(DbError::NotFound)
    );
    assert_eq!(
        db.edit_channel(Id::nil(), own.id, &edit(Some("x"), None, None)).await.map(|_| ()),
        Err(DbError::NotFound)
    );
    assert_eq!(audit(&db, "channel_update").await, 4);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn deleting_takes_messages_and_files_but_never_the_last_text_channel(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, plain) = (user(&db, "lou").await, user(&db, "sam").await);
    let g = guild_with(&db, &own, &[&plain]).await;
    let (gid, general) = (g.guild.id, general(&g));
    let texts = category(&g, "Salons textuels");
    let other = db.create_channel(gid, own.id, &text("autre", Some(texts))).await.unwrap();
    let (m, _) = db.create_message(other.id, own.id, "bonjour", None).await.unwrap();
    db.create_attachment(NewAttachment {
        channel_id: other.id,
        uploader_id: own.id,
        filename: "f.png",
        mime: "image/png",
        size: 3,
        storage_key: "clefdisque",
        dims: Some((1, 1)),
        preview: true,
    })
    .await
    .unwrap();

    assert_eq!(db.delete_channel(other.id, plain.id).await, Err(DbError::Forbidden("perm")));
    assert!(db.channel(other.id).await.unwrap().is_some());
    let files = db.delete_channel(other.id, own.id).await.unwrap();
    assert_eq!(files, ["attachments/clefdisque", "attachments/clefdisque.p"]);
    assert!(db.channel(other.id).await.unwrap().is_none());
    assert!(db.message(m.id).await.unwrap().is_none(), "les messages partent avec le salon");
    assert_eq!(audit(&db, "channel_delete").await, 1);
    assert_eq!(db.delete_channel(other.id, own.id).await, Err(DbError::NotFound));

    // Le dernier salon d'écriture reste ; un salon vocal ou une catégorie, non.
    assert_eq!(db.delete_channel(general, own.id).await, Err(DbError::Conflict("last_text")));
    let announce = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "annonces".into(), kind: ChannelKind::Announcement, parent_id: None },
        )
        .await
        .unwrap();
    db.delete_channel(general, own.id).await.unwrap();
    assert_eq!(db.delete_channel(announce.id, own.id).await, Err(DbError::Conflict("last_text")));

    // Supprimer une catégorie : ses salons restent, sans catégorie, à la fin.
    let a = db.create_channel(gid, own.id, &text("a", Some(texts))).await.unwrap();
    let b = db.create_channel(gid, own.id, &text("b", Some(texts))).await.unwrap();
    db.delete_channel(texts, own.id).await.unwrap();
    let loose = names(&db, gid, own.id, None, false).await;
    assert_eq!(loose, ["annonces", "a", "b"]);
    assert!(db.channel(a.id).await.unwrap().is_some_and(|c| c.parent_id.is_none()));
    let _ = b;
    // Un salon vocal sans cousin peut partir.
    let voice = g.channels.iter().find(|c| c.kind == ChannelKind::Voice).unwrap().id;
    db.delete_channel(voice, own.id).await.unwrap();
}

async fn say_in(
    db: &Db,
    cid: Id,
    who: &User,
    text: &str,
    nonce: Option<&str>,
    exempt: bool,
) -> Result<(Message, bool)> {
    let nonce = nonce.map(String::from);
    db.send_message(cid, who.id, Draft { content: text, nonce, slowmode_exempt: exempt, ..Default::default() }).await
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn slow_mode_holds_back_writers_but_not_managers(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, a, b, boss) =
        (user(&db, "lou").await, user(&db, "sam").await, user(&db, "eve").await, user(&db, "max").await);
    let g = guild_with(&db, &own, &[&a, &b, &boss]).await;
    let (gid, general) = (g.guild.id, general(&g));
    db.edit_channel(general, own.id, &EditChannel { slowmode_s: Some(30), ..Default::default() }).await.unwrap();
    assert!(say_in(&db, general, &a, "un", Some("n1"), false).await.unwrap().1);
    // Le deuxième tombe dans le délai : refusé, et rien n'est écrit.
    let wait = match say_in(&db, general, &a, "deux", None, false).await {
        Err(DbError::Slowmode(n)) => n,
        other => panic!("attendu le mode lent : {other:?}"),
    };
    assert!((1..=30).contains(&wait));
    assert_eq!(db.messages(general, None, 10).await.unwrap().len(), 1);
    // Un renvoi du même message (même nonce) n'est pas un nouvel envoi.
    assert!(!say_in(&db, general, &a, "un", Some("n1"), false).await.unwrap().1);
    // Les autres ne sont pas retenus par ma lenteur ; ceux qui gèrent les messages, jamais.
    assert!(say_in(&db, general, &b, "salut", None, false).await.is_ok());
    for i in 0..3 {
        assert!(say_in(&db, general, &boss, &format!("m{i}"), None, true).await.is_ok());
    }
    // Le délai écoulé, je peux de nouveau écrire.
    sqlx::query("update slowmode_marks set last_at = now() - interval '31 seconds' where user_id = $1")
        .bind(a.id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(say_in(&db, general, &a, "trois", None, false).await.unwrap().1);
    // Deux envois simultanés de la même personne : un seul passe.
    sqlx::query("update slowmode_marks set last_at = now() - interval '31 seconds' where user_id = $1")
        .bind(b.id)
        .execute(&db.pool)
        .await
        .unwrap();
    let (r1, r2) = tokio::join!(say_in(&db, general, &b, "x", None, false), say_in(&db, general, &b, "y", None, false));
    assert_eq!([r1.is_ok(), r2.is_ok()].iter().filter(|ok| **ok).count(), 1);
    // Sans mode lent, rien ne retient personne.
    db.edit_channel(general, own.id, &EditChannel { slowmode_s: Some(0), ..Default::default() }).await.unwrap();
    assert!(say_in(&db, general, &a, "quatre", None, false).await.is_ok());
    assert!(say_in(&db, general, &a, "cinq", None, false).await.is_ok());
    let _ = gid;
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn moving_channels_keeps_hidden_ones_in_place(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, plain) = (user(&db, "lou").await, user(&db, "max").await, user(&db, "sam").await);
    let g = guild_with(&db, &own, &[&mgr, &plain]).await;
    let gid = g.guild.id;
    let (texts, voices) = (category(&g, "Salons textuels"), category(&g, "Salons vocaux"));
    let r = db.create_role(gid, own.id, &role("Bâtisseurs", perms::MANAGE_CHANNELS)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, r.id, true).await.unwrap();
    let a = db.create_channel(gid, own.id, &text("a", Some(texts))).await.unwrap();
    let b = db.create_channel(gid, own.id, &text("b", Some(texts))).await.unwrap();
    assert_eq!(names(&db, gid, own.id, Some(texts), false).await, ["général", "a", "b"]);

    // Descendre « général » d'un rang, puis monter « b » tout en haut.
    db.move_channel(general(&g), own.id, &MoveChannel { parent_id: Some(texts), index: 1 }).await.unwrap();
    assert_eq!(names(&db, gid, own.id, Some(texts), false).await, ["a", "général", "b"]);
    let changed = db.move_channel(b.id, own.id, &MoveChannel { parent_id: Some(texts), index: 0 }).await.unwrap();
    assert_eq!(names(&db, gid, own.id, Some(texts), false).await, ["b", "a", "général"]);
    assert_eq!(changed.len(), 3);
    // Sans changement : rien n'est écrit.
    assert!(db.move_channel(b.id, own.id, &MoveChannel { parent_id: Some(texts), index: 0 }).await.unwrap().is_empty());

    // Changer de catégorie ; un rang trop grand met à la fin ; hors catégorie.
    db.move_channel(a.id, own.id, &MoveChannel { parent_id: Some(voices), index: 99 }).await.unwrap();
    assert_eq!(names(&db, gid, own.id, Some(voices), false).await, ["Général", "a"]);
    assert_eq!(names(&db, gid, own.id, Some(texts), false).await, ["b", "général"]);
    db.move_channel(a.id, own.id, &MoveChannel { parent_id: None, index: 0 }).await.unwrap();
    assert_eq!(names(&db, gid, own.id, None, false).await, ["a"]);
    // Les catégories se rangent entre elles, et ne se logent jamais dans une autre.
    db.move_channel(voices, own.id, &MoveChannel { parent_id: None, index: 0 }).await.unwrap();
    assert_eq!(names(&db, gid, own.id, None, true).await, ["Salons vocaux", "Salons textuels"]);
    assert_eq!(
        db.move_channel(voices, own.id, &MoveChannel { parent_id: Some(texts), index: 0 }).await,
        Err(DbError::Invalid("parent_id"))
    );
    assert_eq!(
        db.move_channel(b.id, own.id, &MoveChannel { parent_id: Some(a.id), index: 0 }).await,
        Err(DbError::Invalid("parent_id"))
    );
    assert_eq!(audit(&db, "channel_move").await, 5);

    // Sans le droit : 403. Un salon caché au gestionnaire garde sa place quand il range les autres.
    assert_eq!(
        db.move_channel(b.id, plain.id, &MoveChannel { parent_id: None, index: 0 }).await,
        Err(DbError::Forbidden("perm"))
    );
    let secret = db.create_channel(gid, own.id, &text("secret", Some(texts))).await.unwrap();
    db.edit_channel(secret.id, own.id, &EditChannel { private: Some(true), ..Default::default() }).await.unwrap();
    assert_eq!(names(&db, gid, own.id, Some(texts), false).await, ["b", "général", "secret"]);
    assert_eq!(names(&db, gid, mgr.id, Some(texts), false).await, ["b", "général"]);
    db.move_channel(general(&g), mgr.id, &MoveChannel { parent_id: Some(texts), index: 0 }).await.unwrap();
    assert_eq!(names(&db, gid, own.id, Some(texts), false).await, ["général", "b", "secret"]);
    assert_eq!(
        db.move_channel(secret.id, mgr.id, &MoveChannel { parent_id: None, index: 0 }).await,
        Err(DbError::NotFound),
        "caché : 404"
    );
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn private_channels_vanish_for_those_who_cannot_read_them(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, plain, vip) =
        (user(&db, "lou").await, user(&db, "max").await, user(&db, "sam").await, user(&db, "eve").await);
    let g = guild_with(&db, &own, &[&mgr, &plain, &vip]).await;
    let gid = g.guild.id;
    let r = db.create_role(gid, own.id, &role("Bâtisseurs", perms::MANAGE_CHANNELS)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, r.id, true).await.unwrap();
    let c = db.create_channel(gid, mgr.id, &text("coulisses", None)).await.unwrap();
    assert!(!c.private);
    let sees = |uid: Id| {
        let db = db.clone();
        async move { db.channel_audience(c.id).await.unwrap().contains(&uid) }
    };
    assert!(sees(plain.id).await);

    // Privé : seuls ceux à qui un rôle ou une surcharge le rouvre le voient.
    let c = db.edit_channel(c.id, own.id, &EditChannel { private: Some(true), ..Default::default() }).await.unwrap();
    assert!(c.private);
    assert!(!sees(plain.id).await && !sees(vip.id).await && !sees(mgr.id).await);
    assert!(sees(own.id).await, "le propriétaire voit tout");
    assert!(db.guild_state(gid, plain.id).await.unwrap().unwrap().channels.iter().all(|x| x.id != c.id));
    assert!(db.guild_state(gid, own.id).await.unwrap().unwrap().channels.iter().any(|x| x.id == c.id && x.private));
    assert_eq!(db.channel_perms(plain.id, c.id).await.unwrap(), 0);
    assert_eq!(
        db.channel_overwrites(c.id, plain.id).await,
        Err(DbError::NotFound),
        "un salon qu'on ne voit pas n'existe pas"
    );
    // Rouvrir à une personne précise.
    let list = db
        .set_overwrite(
            c.id,
            own.id,
            vip.id,
            &SetOverwrite { kind: OverwriteKind::Member, allow: perms::VIEW_CHANNEL, deny: 0 },
        )
        .await
        .unwrap();
    assert_eq!(list.len(), 2);
    assert!(sees(vip.id).await && !sees(plain.id).await);
    assert_eq!(db.channel_overwrites(c.id, plain.id).await.map(|_| ()), Err(DbError::NotFound));
    // Redevenir public.
    let c = db.edit_channel(c.id, own.id, &EditChannel { private: Some(false), ..Default::default() }).await.unwrap();
    assert!(!c.private && sees(plain.id).await);

    // Une catégorie privée cache ses salons ; un salon peut la contredire pour un rôle.
    let cat = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "Staff".into(), kind: ChannelKind::Category, parent_id: None },
        )
        .await
        .unwrap();
    let inner = db.create_channel(gid, own.id, &text("réunion", Some(cat.id))).await.unwrap();
    db.edit_channel(cat.id, own.id, &EditChannel { private: Some(true), ..Default::default() }).await.unwrap();
    assert!(
        db.guild_state(gid, plain.id)
            .await
            .unwrap()
            .unwrap()
            .channels
            .iter()
            .all(|x| x.id != cat.id && x.id != inner.id)
    );
    let seen = db.guild_state(gid, own.id).await.unwrap().unwrap();
    assert!(seen.channels.iter().find(|x| x.id == inner.id).unwrap().private, "hérité de la catégorie");
    let audiences = db.guild_audiences(gid).await.unwrap();
    assert!(!audiences[&inner.id].contains(&plain.id) && audiences[&inner.id].contains(&own.id));
    assert_eq!(db.guild_channels(gid).await.unwrap().len(), seen.channels.len(), "guild_channels les montre tous");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn overwrites_follow_the_role_hierarchy_and_cannot_grant_what_you_lack(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, other, plain) =
        (user(&db, "lou").await, user(&db, "max").await, user(&db, "eve").await, user(&db, "sam").await);
    let g = guild_with(&db, &own, &[&mgr, &other, &plain]).await;
    let (gid, general) = (g.guild.id, general(&g));
    // Haut > Gestionnaires > Petit. « Gestionnaires » gère les salons mais ne peut ni écrire ni gérer les messages.
    let high = db.create_role(gid, own.id, &role("Haut", perms::ALL & !perms::ADMINISTRATOR)).await.unwrap();
    let managers = db
        .create_role(
            gid,
            own.id,
            &role("Gestionnaires", perms::MANAGE_CHANNELS | perms::VIEW_CHANNEL | perms::SEND_MESSAGES),
        )
        .await
        .unwrap();
    let small = db.create_role(gid, own.id, &role("Petit", 0)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, managers.id, true).await.unwrap();
    db.set_member_role(gid, own.id, other.id, small.id, true).await.unwrap();
    let ov = |kind, allow, deny| SetOverwrite { kind, allow, deny };
    let role_ov = |allow, deny| ov(OverwriteKind::Role, allow, deny);

    // Rôle plus bas que soi : permis. Rôle plus haut : 403 « hierarchy ». @everyone : permis.
    let list = db.set_overwrite(general, mgr.id, small.id, &role_ov(0, perms::SEND_MESSAGES)).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        db.set_overwrite(general, mgr.id, high.id, &role_ov(0, perms::SEND_MESSAGES)).await,
        Err(DbError::Forbidden("hierarchy"))
    );
    assert!(db.set_overwrite(general, mgr.id, gid, &role_ov(0, perms::ADD_REACTIONS)).await.is_ok());
    assert_eq!(db.channel_perms(other.id, general).await.unwrap() & perms::SEND_MESSAGES, 0, "le refus s'applique");
    // Pas de droit qu'on n'a pas : MANAGE_MESSAGES manque au gestionnaire (autoriser ou refuser).
    assert_eq!(
        db.set_overwrite(general, mgr.id, small.id, &role_ov(perms::MANAGE_MESSAGES, 0)).await,
        Err(DbError::Forbidden("perm"))
    );
    assert_eq!(
        db.set_overwrite(general, mgr.id, small.id, &role_ov(0, perms::SEND_MESSAGES | perms::MANAGE_MESSAGES)).await,
        Err(DbError::Forbidden("perm"))
    );
    // Un droit déjà posé par un autre reste intact quand on en change un autre.
    db.set_overwrite(general, own.id, small.id, &role_ov(0, perms::SEND_MESSAGES | perms::MANAGE_MESSAGES))
        .await
        .unwrap();
    assert!(
        db.set_overwrite(
            general,
            mgr.id,
            small.id,
            &role_ov(0, perms::SEND_MESSAGES | perms::MANAGE_MESSAGES | perms::ADD_REACTIONS)
        )
        .await
        .is_ok()
    );
    // Membres : un membre de rang inférieur seulement, jamais le propriétaire ; soi-même, si.
    let member = |allow, deny| ov(OverwriteKind::Member, allow, deny);
    assert!(db.set_overwrite(general, mgr.id, other.id, &member(perms::EMBED_LINKS, 0)).await.is_ok());
    assert_eq!(
        db.set_overwrite(general, mgr.id, own.id, &member(0, perms::SEND_MESSAGES)).await,
        Err(DbError::Forbidden("hierarchy"))
    );
    assert!(db.set_overwrite(general, mgr.id, mgr.id, &member(0, perms::ADD_REACTIONS)).await.is_ok());
    let peer = user(&db, "pair").await;
    let inv = db.create_invite(gid, Some(general), own.id, 0, None).await.unwrap();
    db.use_invite(&inv.code, peer.id).await.unwrap();
    db.set_member_role(gid, own.id, peer.id, managers.id, true).await.unwrap();
    assert_eq!(
        db.set_overwrite(general, mgr.id, peer.id, &member(0, perms::SEND_MESSAGES)).await,
        Err(DbError::Forbidden("hierarchy")),
        "même rang"
    );

    // Entrées invalides : droit à la fois autorisé et refusé, droit du serveur entier, cible inconnue.
    assert_eq!(
        db.set_overwrite(general, own.id, small.id, &role_ov(perms::SEND_MESSAGES, perms::SEND_MESSAGES)).await,
        Err(DbError::Invalid("overwrite"))
    );
    for bit in [perms::ADMINISTRATOR, perms::BAN, perms::MANAGE_ROLES] {
        assert_eq!(
            db.set_overwrite(general, own.id, small.id, &role_ov(bit, 0)).await,
            Err(DbError::Invalid("permissions"))
        );
    }
    assert_eq!(
        db.set_overwrite(general, own.id, Id::nil(), &role_ov(perms::EMBED_LINKS, 0)).await,
        Err(DbError::NotFound)
    );
    assert_eq!(
        db.set_overwrite(general, own.id, Id::nil(), &member(perms::EMBED_LINKS, 0)).await,
        Err(DbError::NotFound)
    );
    // Sans droit : 403 ; salon inconnu ou MP : 404.
    assert_eq!(
        db.set_overwrite(general, plain.id, small.id, &role_ov(0, perms::EMBED_LINKS)).await,
        Err(DbError::Forbidden("perm"))
    );
    assert_eq!(db.channel_overwrites(general, plain.id).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    assert_eq!(db.channel_overwrites(Id::nil(), own.id).await.map(|_| ()), Err(DbError::NotFound));

    // Retirer : la surcharge disparaît (vider `allow` et `deny` fait pareil) ; il n'y a rien à retirer deux fois.
    let before = db.channel_overwrites(general, own.id).await.unwrap();
    assert!(before.iter().any(|o| o.target_id == small.id));
    let after = db.remove_overwrite(general, own.id, small.id).await.unwrap();
    assert!(after.iter().all(|o| o.target_id != small.id));
    assert_eq!(db.remove_overwrite(general, own.id, small.id).await, Err(DbError::NotFound));
    assert!(audit(&db, "overwrite_update").await >= 5);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn nobody_can_lock_themselves_out(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, plain) = (user(&db, "lou").await, user(&db, "max").await, user(&db, "sam").await);
    let g = guild_with(&db, &own, &[&mgr, &plain]).await;
    let (gid, general) = (g.guild.id, general(&g));
    let managers = db.create_role(gid, own.id, &role("Gestionnaires", perms::MANAGE_CHANNELS)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, managers.id, true).await.unwrap();
    let c = db.create_channel(gid, mgr.id, &text("coulisses", None)).await.unwrap();

    // Rendre le salon privé alors que rien ne le rouvre au gestionnaire : refusé, et rien ne change.
    assert_eq!(
        db.edit_channel(c.id, mgr.id, &EditChannel { private: Some(true), ..Default::default() }).await.map(|_| ()),
        Err(DbError::Forbidden("lockout"))
    );
    assert!(!db.channel_view(c.id).await.unwrap().unwrap().private);
    // Se refuser à soi-même, ou refuser le droit de gérer, de même.
    let me = |allow, deny| SetOverwrite { kind: OverwriteKind::Member, allow, deny };
    assert_eq!(
        db.set_overwrite(c.id, mgr.id, mgr.id, &me(0, perms::VIEW_CHANNEL)).await.map(|_| ()),
        Err(DbError::Forbidden("lockout"))
    );
    let everyone_cannot_manage = SetOverwrite { kind: OverwriteKind::Role, allow: 0, deny: perms::MANAGE_CHANNELS };
    assert_eq!(
        db.set_overwrite(c.id, mgr.id, gid, &everyone_cannot_manage).await.map(|_| ()),
        Err(DbError::Forbidden("lockout"))
    );
    assert!(db.channel_overwrites(c.id, mgr.id).await.unwrap().is_empty());
    // Avec une ouverture pour son propre rôle, c'est permis.
    db.set_overwrite(
        c.id,
        own.id,
        managers.id,
        &SetOverwrite { kind: OverwriteKind::Role, allow: perms::VIEW_CHANNEL, deny: 0 },
    )
    .await
    .unwrap();
    assert!(
        db.edit_channel(c.id, mgr.id, &EditChannel { private: Some(true), ..Default::default() })
            .await
            .unwrap()
            .private
    );
    // Le propriétaire n'est jamais verrouillé dehors.
    assert!(
        db.edit_channel(general, own.id, &EditChannel { private: Some(true), ..Default::default() })
            .await
            .unwrap()
            .private
    );
    let _ = (MAX_OVERWRITES, plain);
}

fn view(kind: OverwriteKind) -> SetOverwrite {
    SetOverwrite { kind, allow: perms::VIEW_CHANNEL, deny: 0 }
}

/// Un salon fermé à `@everyone`, qu'un rôle rouvre : le modèle d'une catégorie « Staff ».
async fn staff_category(db: &Db, g: &GuildState, own: &User, staff: &Role) -> Channel {
    let gid = g.guild.id;
    let cat = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "Staff".into(), kind: ChannelKind::Category, parent_id: None },
        )
        .await
        .unwrap();
    db.edit_channel(cat.id, own.id, &EditChannel { private: Some(true), ..Default::default() }).await.unwrap();
    db.set_overwrite(cat.id, own.id, staff.id, &view(OverwriteKind::Role)).await.unwrap();
    cat
}

async fn sees(db: &Db, cid: Id, uid: Id) -> bool {
    db.channel_audience(cid).await.unwrap().contains(&uid)
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn leaving_roles_and_members_take_their_overwrites_with_them(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, plain) = (user(&db, "lou").await, user(&db, "max").await, user(&db, "sam").await);
    let (gone, kicked, banned) = (user(&db, "eve").await, user(&db, "kim").await, user(&db, "zoe").await);
    let g = guild_with(&db, &own, &[&mgr, &plain, &gone, &kicked, &banned]).await;
    let (gid, general) = (g.guild.id, general(&g));
    let builders = db.create_role(gid, own.id, &role("Bâtisseurs", perms::MANAGE_CHANNELS)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, builders.id, true).await.unwrap();
    let temp = db.create_role(gid, own.id, &role("Passager", 0)).await.unwrap();
    let write = SetOverwrite { kind: OverwriteKind::Member, allow: perms::ADD_REACTIONS, deny: 0 };
    db.set_overwrite(general, own.id, temp.id, &view(OverwriteKind::Role)).await.unwrap();
    for u in [&gone, &kicked, &banned, &plain] {
        db.set_overwrite(general, own.id, u.id, &write).await.unwrap();
    }
    let count = || {
        let db = db.clone();
        async move { db.channel_overwrites(general, own.id).await.unwrap().len() }
    };
    assert_eq!(count().await, 5);

    // Le rôle supprimé emporte sa surcharge ; les autres surcharges restent.
    db.delete_role(gid, temp.id, own.id).await.unwrap();
    assert_eq!(count().await, 4);
    // Partir, être exclu ou être banni : la surcharge du membre part aussi.
    db.leave_guild(gid, gone.id).await.unwrap();
    assert_eq!(count().await, 3);
    db.kick_member(gid, own.id, kicked.id, None).await.unwrap();
    assert_eq!(count().await, 2);
    db.ban_member(gid, own.id, banned.id, &BanMember::default()).await.unwrap();
    assert_eq!(count().await, 1);
    let left = db.channel_overwrites(general, own.id).await.unwrap();
    assert_eq!(left.iter().map(|o| o.target_id).collect::<Vec<_>>(), [plain.id], "celle d'un membre présent reste");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn an_orphan_overwrite_can_still_be_removed_by_a_manager(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, plain) = (user(&db, "lou").await, user(&db, "max").await, user(&db, "sam").await);
    let g = guild_with(&db, &own, &[&mgr, &plain]).await;
    let (gid, general) = (g.guild.id, general(&g));
    let builders = db.create_role(gid, own.id, &role("Bâtisseurs", perms::MANAGE_CHANNELS)).await.unwrap();
    db.set_member_role(gid, own.id, mgr.id, builders.id, true).await.unwrap();
    // Des lignes laissées par une version qui ne nettoyait pas : un rôle et un membre qui n'existent plus.
    // Elles portent un droit que le gestionnaire n'a pas (gérer les messages).
    let (dead_role, dead_member) = (Id::from_u128(0xdead01), Id::from_u128(0xdead02));
    for (target, kind) in [(dead_role, "role"), (dead_member, "member")] {
        sqlx::query(
            "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny) values ($1, $2, $3, $4, 0)",
        )
        .bind(general)
        .bind(target)
        .bind(kind)
        .bind(perms::MANAGE_MESSAGES as i64)
        .execute(&db.pool)
        .await
        .unwrap();
    }
    assert_eq!(db.channel_overwrites(general, mgr.id).await.unwrap().len(), 2);

    // Un simple membre ne touche à rien ; poser une surcharge à un inconnu reste refusé.
    assert_eq!(db.remove_overwrite(general, plain.id, dead_role).await.map(|_| ()), Err(DbError::Forbidden("perm")));
    let set = SetOverwrite { kind: OverwriteKind::Role, allow: perms::ADD_REACTIONS, deny: 0 };
    assert_eq!(
        db.set_overwrite(general, own.id, Id::from_u128(0xdead03), &set).await.map(|_| ()),
        Err(DbError::NotFound)
    );
    // Le gestionnaire les retire, par DELETE ou en vidant la surcharge, sans en avoir les droits.
    let list = db.remove_overwrite(general, mgr.id, dead_role).await.unwrap();
    assert_eq!(list.len(), 1);
    let empty = SetOverwrite { kind: OverwriteKind::Member, allow: 0, deny: 0 };
    let list = db.set_overwrite(general, mgr.id, dead_member, &empty).await.unwrap();
    assert!(list.is_empty());
    // Plus rien à retirer : 404, comme avant.
    assert_eq!(db.remove_overwrite(general, mgr.id, dead_role).await.map(|_| ()), Err(DbError::NotFound));
    assert_eq!(audit(&db, "overwrite_update").await, 2);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn deleting_a_private_category_keeps_its_channels_private(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, plain, vip) = (user(&db, "lou").await, user(&db, "sam").await, user(&db, "eve").await);
    let g = guild_with(&db, &own, &[&plain, &vip]).await;
    let gid = g.guild.id;
    let staff = db.create_role(gid, own.id, &role("Staff", 0)).await.unwrap();
    db.set_member_role(gid, own.id, vip.id, staff.id, true).await.unwrap();
    let cat = staff_category(&db, &g, &own, &staff).await;
    let bare = db.create_channel(gid, own.id, &text("réunion", Some(cat.id))).await.unwrap();
    // Un salon qui a déjà une surcharge de `@everyone` (ici : personne n'y écrit) : la sienne
    // s'ajoute à celle de la catégorie, elle ne la remplace pas.
    let calm = db.create_channel(gid, own.id, &text("calme", Some(cat.id))).await.unwrap();
    let mute = SetOverwrite { kind: OverwriteKind::Role, allow: 0, deny: perms::SEND_MESSAGES };
    db.set_overwrite(calm.id, own.id, gid, &mute).await.unwrap();
    let after_cat = db.create_channel(gid, own.id, &text("dernier", Some(cat.id))).await.unwrap();
    for c in [&bare, &calm, &after_cat] {
        assert!(!sees(&db, c.id, plain.id).await && sees(&db, c.id, vip.id).await && sees(&db, c.id, own.id).await);
    }
    assert_eq!(db.channel_perms(plain.id, bare.id).await.unwrap(), 0);

    db.delete_channel(cat.id, own.id).await.unwrap();
    for c in [&bare, &calm, &after_cat] {
        let now = db.channel_view(c.id).await.unwrap().unwrap();
        assert!(now.parent_id.is_none() && now.private, "{:?} reste privé", c.name);
        assert!(!sees(&db, c.id, plain.id).await, "{:?} ne doit pas s'ouvrir à tous", c.name);
        assert_eq!(db.channel_perms(plain.id, c.id).await.unwrap(), 0);
        assert!(sees(&db, c.id, vip.id).await && sees(&db, c.id, own.id).await);
        assert!(db.messages(c.id, None, 10).await.is_ok());
    }
    // Ce que chacun peut y faire n'a pas bougé non plus : la consigne du salon s'ajoute à la catégorie.
    let calm_rules = db.channel_overwrites(calm.id, own.id).await.unwrap();
    let everyone = calm_rules.iter().find(|o| o.target_id == gid).unwrap();
    assert_eq!((everyone.allow, everyone.deny), (0, perms::VIEW_CHANNEL | perms::SEND_MESSAGES));
    assert!(calm_rules.iter().any(|o| o.target_id == staff.id && o.allow == perms::VIEW_CHANNEL));
    assert_eq!(db.channel_perms(vip.id, calm.id).await.unwrap() & perms::SEND_MESSAGES, 0);
    assert_ne!(db.channel_perms(vip.id, bare.id).await.unwrap() & perms::SEND_MESSAGES, 0);
    // Les places des catégories se resserrent.
    assert_eq!(names(&db, gid, own.id, None, true).await, ["Salons textuels", "Salons vocaux"]);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn moving_a_channel_out_of_a_private_category_keeps_it_private(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, plain, vip) = (user(&db, "lou").await, user(&db, "sam").await, user(&db, "eve").await);
    let g = guild_with(&db, &own, &[&plain, &vip]).await;
    let (gid, voices) = (g.guild.id, category(&g, "Salons vocaux"));
    let staff = db.create_role(gid, own.id, &role("Staff", 0)).await.unwrap();
    db.set_member_role(gid, own.id, vip.id, staff.id, true).await.unwrap();
    let cat = staff_category(&db, &g, &own, &staff).await;
    let (a, b, c) = (
        db.create_channel(gid, own.id, &text("a", Some(cat.id))).await.unwrap(),
        db.create_channel(gid, own.id, &text("b", Some(cat.id))).await.unwrap(),
        db.create_channel(gid, own.id, &text("c", Some(cat.id))).await.unwrap(),
    );
    // Sortir de la catégorie, puis passer dans une autre, publique : l'accès ne change pas.
    db.move_channel(a.id, own.id, &MoveChannel { parent_id: None, index: 0 }).await.unwrap();
    let changed = db.move_channel(b.id, own.id, &MoveChannel { parent_id: Some(voices), index: 0 }).await.unwrap();
    for x in [&a, &b] {
        assert!(db.channel_view(x.id).await.unwrap().unwrap().private);
        assert!(!sees(&db, x.id, plain.id).await, "{:?} ne doit pas s'ouvrir à tous", x.name);
        assert!(sees(&db, x.id, vip.id).await && sees(&db, x.id, own.id).await);
    }
    // Ceux qui restent dans la catégorie sont toujours cachés, et leurs places n'ont plus de trou.
    assert!(!sees(&db, c.id, plain.id).await);
    assert!(changed.contains(&c.id), "c a changé de place : {changed:?}");
    assert_eq!(db.channel(c.id).await.unwrap().unwrap().position, 0);
    assert_eq!(db.channel(b.id).await.unwrap().unwrap().position, 0);
    assert_eq!(audit(&db, "channel_move").await, 2);

    // Bouger dans la même catégorie ne recopie rien.
    db.move_channel(c.id, own.id, &MoveChannel { parent_id: Some(cat.id), index: 0 }).await.unwrap();
    assert_eq!(db.channel_overwrites(c.id, own.id).await.unwrap().len(), 0);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_channel_never_becomes_more_open_by_leaving_its_category(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, plain, vip) = (user(&db, "lou").await, user(&db, "sam").await, user(&db, "eve").await);
    let g = guild_with(&db, &own, &[&plain, &vip]).await;
    let gid = g.guild.id;
    let staff = db.create_role(gid, own.id, &role("Staff", 0)).await.unwrap();
    db.set_member_role(gid, own.id, vip.id, staff.id, true).await.unwrap();
    // La catégorie rouvre le salon à « Staff » ; le salon, lui, se ferme à tous. Pris dans l'ordre
    // (catégorie, puis salon), « Staff » ne le voit pas ; fondues en une seule couche, les deux
    // règles l'ouvriraient : une interdiction à son nom referme l'écart.
    let cat = db
        .create_channel(
            gid,
            own.id,
            &CreateChannel { name: "Mixte".into(), kind: ChannelKind::Category, parent_id: None },
        )
        .await
        .unwrap();
    db.set_overwrite(cat.id, own.id, staff.id, &view(OverwriteKind::Role)).await.unwrap();
    let c = db.create_channel(gid, own.id, &text("fermé", Some(cat.id))).await.unwrap();
    db.edit_channel(c.id, own.id, &EditChannel { private: Some(true), ..Default::default() }).await.unwrap();
    assert!(!sees(&db, c.id, vip.id).await && !sees(&db, c.id, plain.id).await);
    db.delete_channel(cat.id, own.id).await.unwrap();
    assert!(!sees(&db, c.id, vip.id).await, "le rôle de la catégorie n'ouvre pas ce que le salon fermait");
    assert!(!sees(&db, c.id, plain.id).await);
    assert!(sees(&db, c.id, own.id).await);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn removing_an_overwrite_with_the_wrong_kind_never_skips_the_hierarchy(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (own, mgr, peer, boss, small) = (
        user(&db, "lou").await,
        user(&db, "max").await,
        user(&db, "pair").await,
        user(&db, "chef").await,
        user(&db, "eve").await,
    );
    let g = guild_with(&db, &own, &[&mgr, &peer, &boss, &small]).await;
    let (gid, general) = (g.guild.id, general(&g));
    // Haut > Gestionnaires > Petit ; « Gestionnaires » gère les salons mais pas les messages.
    let top = db.create_role(gid, own.id, &role("Haut", perms::ALL & !perms::ADMINISTRATOR)).await.unwrap();
    let managers = db
        .create_role(
            gid,
            own.id,
            &role("Gestionnaires", perms::MANAGE_CHANNELS | perms::VIEW_CHANNEL | perms::SEND_MESSAGES),
        )
        .await
        .unwrap();
    let low = db.create_role(gid, own.id, &role("Petit", 0)).await.unwrap();
    db.set_member_role(gid, own.id, boss.id, top.id, true).await.unwrap();
    for u in [&mgr, &peer] {
        db.set_member_role(gid, own.id, u.id, managers.id, true).await.unwrap();
    }
    let (role_ov, member_ov) = (
        |deny| SetOverwrite { kind: OverwriteKind::Role, allow: 0, deny },
        |deny| SetOverwrite { kind: OverwriteKind::Member, allow: 0, deny },
    );
    // Posé par le propriétaire : un rôle plus haut, un membre plus haut, un pair, `@everyone` qui
    // porte un droit que le gestionnaire n'a pas, et un rôle plus bas (que lui peut toucher).
    db.set_overwrite(general, own.id, top.id, &role_ov(perms::SEND_MESSAGES)).await.unwrap();
    db.set_overwrite(general, own.id, boss.id, &member_ov(perms::EMBED_LINKS)).await.unwrap();
    db.set_overwrite(general, own.id, peer.id, &member_ov(perms::ADD_REACTIONS)).await.unwrap();
    let everyone = SetOverwrite { kind: OverwriteKind::Role, allow: perms::MANAGE_MESSAGES, deny: 0 };
    db.set_overwrite(general, own.id, gid, &everyone).await.unwrap();
    db.set_overwrite(general, own.id, low.id, &role_ov(perms::EMBED_LINKS)).await.unwrap();
    let targets = || {
        let db = db.clone();
        async move {
            let mut t: Vec<Id> =
                db.channel_overwrites(general, own.id).await.unwrap().iter().map(|o| o.target_id).collect();
            t.sort_unstable();
            t
        }
    };
    let all = targets().await;
    assert_eq!(all.len(), 5);

    // Vider la surcharge en nommant le mauvais genre : même refus qu'avec le bon, la ligne reste.
    let (as_member, as_role) = (member_ov(0), role_ov(0));
    let cases = [
        (top.id, as_member, DbError::Forbidden("hierarchy"), "rôle plus haut, donné pour un membre"),
        (boss.id, as_role, DbError::Forbidden("hierarchy"), "membre plus haut, donné pour un rôle"),
        (peer.id, as_role, DbError::Forbidden("hierarchy"), "pair, donné pour un rôle"),
        (gid, as_member, DbError::Forbidden("perm"), "@everyone avec un droit qu'on n'a pas, donné pour un membre"),
        (top.id, as_role, DbError::Forbidden("hierarchy"), "le bon genre"),
        (gid, as_role, DbError::Forbidden("perm"), "le bon genre"),
    ];
    for (target, req, err, what) in cases {
        assert_eq!(db.set_overwrite(general, mgr.id, target, &req).await.map(|_| ()), Err(err), "{what}");
        assert_eq!(targets().await, all, "la ligne reste : {what}");
    }
    assert_eq!(db.remove_overwrite(general, mgr.id, top.id).await.map(|_| ()), Err(DbError::Forbidden("hierarchy")));
    assert_eq!(db.remove_overwrite(general, mgr.id, boss.id).await.map(|_| ()), Err(DbError::Forbidden("hierarchy")));
    assert_eq!(targets().await, all);

    // Modifier (et non vider) avec le mauvais genre : refusé, rien ne change.
    let wrong = SetOverwrite { kind: OverwriteKind::Member, allow: 0, deny: perms::ADD_REACTIONS };
    assert_eq!(
        db.set_overwrite(general, mgr.id, low.id, &wrong).await.map(|_| ()),
        Err(DbError::Invalid("overwrite_kind"))
    );
    assert_eq!(
        db.set_overwrite(general, own.id, top.id, &wrong).await.map(|_| ()),
        Err(DbError::Invalid("overwrite_kind"))
    );
    let rows = db.channel_overwrites(general, own.id).await.unwrap();
    let kept = rows.iter().find(|o| o.target_id == low.id).unwrap();
    assert_eq!((kept.kind, kept.deny), (OverwriteKind::Role, perms::EMBED_LINKS));
    assert_eq!(rows.iter().find(|o| o.target_id == top.id).unwrap().kind, OverwriteKind::Role);

    // Ce qu'il peut toucher se vide, même avec le mauvais genre (c'est celui de la ligne qui compte).
    let list = db.set_overwrite(general, mgr.id, low.id, &as_member).await.unwrap();
    assert!(list.iter().all(|o| o.target_id != low.id));
    assert_eq!(targets().await.len(), 4);

    // Une vraie orpheline (ni rôle ni membre) part toujours, avec n'importe quel genre.
    let (dead_role, dead_member) = (Id::from_u128(0xdead01), Id::from_u128(0xdead02));
    for (target, kind) in [(dead_role, "role"), (dead_member, "member")] {
        sqlx::query(
            "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny) values ($1, $2, $3, $4, 0)",
        )
        .bind(general)
        .bind(target)
        .bind(kind)
        .bind(perms::MANAGE_MESSAGES as i64)
        .execute(&db.pool)
        .await
        .unwrap();
    }
    assert_eq!(targets().await.len(), 6);
    db.set_overwrite(general, mgr.id, dead_role, &as_member).await.unwrap();
    db.set_overwrite(general, mgr.id, dead_member, &as_role).await.unwrap();
    assert_eq!(targets().await.len(), 4);
    assert!(targets().await.iter().all(|t| *t != dead_role && *t != dead_member));
}
