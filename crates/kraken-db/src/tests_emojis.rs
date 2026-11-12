//! Emojis perso d'un serveur : droit « Gérer les émojis », 50 par serveur, noms (2 à 32 caractères,
//! uniques sans tenir compte de la casse), journal d'audit, réactions, suppression du serveur.

use kraken_proto::{ChannelKind, CreateRole, MAX_EMOJIS};
use sqlx::PgPool;

use super::*;

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

/// Un serveur à `owner` avec `others` dedans ; rend son id et celui de #général.
async fn guild_with(db: &Db, name: &str, owner: &User, others: &[&User]) -> (Id, Id) {
    let g = db.create_guild(owner.id, name).await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), owner.id, 0, None).await.unwrap();
    for u in others {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    (g.guild.id, general)
}

async fn audit(db: &Db, action: &str) -> i64 {
    sqlx::query_scalar("select count(*) from audit_log where action = $1")
        .bind(action)
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn managing_emojis_needs_the_permission_and_leaves_a_trace(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let mem = user(&db, "sam").await;
    let out = user(&db, "noa").await;
    let (gid, _) = guild_with(&db, "Abysse", &own, &[&mem]).await;

    // Un membre sans le droit ne peut rien ; un étranger ne sait même pas que le serveur existe.
    assert_eq!(db.add_emoji(gid, mem.id, "poulpe", false, "k.png").await, Err(DbError::Forbidden("perm")));
    assert_eq!(db.add_emoji(gid, out.id, "poulpe", false, "k.png").await, Err(DbError::NotFound));
    assert_eq!(db.check_new_emoji(gid, mem.id, "poulpe").await, Err(DbError::Forbidden("perm")));
    assert_eq!(db.check_new_emoji(gid, out.id, "poulpe").await, Err(DbError::NotFound));
    assert_eq!(db.check_new_emoji(Id::nil(), own.id, "poulpe").await, Err(DbError::NotFound));
    assert!(db.guild_emojis(gid).await.unwrap().is_empty());

    // Le propriétaire ajoute ; l'emoji dit qui l'a ajouté, avec son nom affiché.
    db.set_display_name(own.id, Some("Lou la pieuvre")).await.unwrap();
    db.check_new_emoji(gid, own.id, "poulpe").await.unwrap();
    let e = db.add_emoji(gid, own.id, "poulpe", false, "aaa.png").await.unwrap();
    assert_eq!((e.name.as_str(), e.guild_id, e.animated), ("poulpe", gid, false));
    assert_eq!(e.creator, Some(EmojiCreator { id: own.id, name: "Lou la pieuvre".into() }));
    assert_eq!(audit(&db, "emoji_create").await, 1);
    assert_eq!(db.emoji_image(e.id).await.unwrap().as_deref(), Some("aaa.png"));
    assert_eq!(db.emoji_image(Id::nil()).await.unwrap(), None);

    // Un rôle qui porte « Gérer les émojis » suffit, sans être propriétaire ni administrateur.
    let r = db
        .create_role(
            gid,
            own.id,
            &CreateRole { name: "Dessinateur".into(), color: None, hoist: false, permissions: perms::MANAGE_EMOJIS },
        )
        .await
        .unwrap();
    db.set_member_role(gid, own.id, mem.id, r.id, true).await.unwrap();
    let anim = db.add_emoji(gid, mem.id, "danse", true, "bbb.gif").await.unwrap();
    assert!(anim.animated);
    assert_eq!(anim.creator.as_ref().map(|c| c.name.as_str()), Some("sam"), "à défaut, le pseudo");
    // Du plus ancien au plus récent.
    let names: Vec<_> = db.guild_emojis(gid).await.unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["poulpe", "danse"]);
    // Le serveur, vu d'un membre, porte ses emojis.
    assert_eq!(db.guild_state(gid, mem.id).await.unwrap().unwrap().emojis.len(), 2);

    // Renommer : l'id et l'image restent ; rien n'est écrit au journal si le nom ne change pas.
    let renamed = db.rename_emoji(gid, e.id, mem.id, "Poulpe_2").await.unwrap();
    assert_eq!((renamed.id, renamed.name.as_str()), (e.id, "Poulpe_2"));
    assert_eq!(db.emoji_image(e.id).await.unwrap().as_deref(), Some("aaa.png"));
    assert_eq!(audit(&db, "emoji_update").await, 1);
    db.rename_emoji(gid, e.id, mem.id, "Poulpe_2").await.unwrap();
    assert_eq!(audit(&db, "emoji_update").await, 1);
    // Changer seulement la casse de son propre nom est permis.
    assert_eq!(db.rename_emoji(gid, e.id, mem.id, "poulpe_2").await.unwrap().name, "poulpe_2");
    assert_eq!(db.rename_emoji(gid, Id::nil(), mem.id, "autre").await, Err(DbError::NotFound));
    db.set_member_role(gid, own.id, mem.id, r.id, false).await.unwrap();
    assert_eq!(db.rename_emoji(gid, e.id, mem.id, "autre").await, Err(DbError::Forbidden("perm")));
    assert_eq!(db.delete_emoji(gid, e.id, mem.id).await, Err(DbError::Forbidden("perm")));

    // Retirer rend la clé du fichier à effacer, et laisse une trace.
    assert_eq!(db.delete_emoji(gid, e.id, own.id).await.unwrap(), "aaa.png");
    assert_eq!(db.delete_emoji(gid, e.id, own.id).await, Err(DbError::NotFound), "déjà retiré");
    assert_eq!(db.emoji_image(e.id).await.unwrap(), None);
    assert_eq!(audit(&db, "emoji_delete").await, 1);
    assert_eq!(db.guild_emojis(gid).await.unwrap().len(), 1);

    // Si le compte du créateur disparaît, l'emoji reste, sans nom.
    sqlx::query("delete from users where id = $1").bind(mem.id).execute(&db.pool).await.unwrap();
    let left = db.guild_emojis(gid).await.unwrap();
    assert_eq!((left.len(), left[0].name.as_str(), left[0].creator.clone()), (1, "danse", None));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn names_are_checked_and_unique_ignoring_case(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let (gid, _) = guild_with(&db, "Abysse", &own, &[]).await;
    let (other, _) = guild_with(&db, "Récif", &own, &[]).await;

    for bad in ["", "a", "é", "a b", "a-b", "pieuvre!", "éléphant", &"x".repeat(33), "a\u{200b}b", "ab:"] {
        assert_eq!(
            db.add_emoji(gid, own.id, bad, false, "k.png").await,
            Err(DbError::Invalid("emoji_name")),
            "{bad:?}"
        );
        assert_eq!(db.check_new_emoji(gid, own.id, bad).await, Err(DbError::Invalid("emoji_name")), "{bad:?}");
    }
    for ok in ["ab", "A_1", &"x".repeat(32)] {
        db.add_emoji(gid, own.id, ok, false, "k.png").await.unwrap();
    }
    // Pris, quelle que soit la casse ; libre dans un autre serveur.
    for taken in ["ab", "AB", "aB", "a_1"] {
        assert_eq!(
            db.add_emoji(gid, own.id, taken, false, "k.png").await,
            Err(DbError::Conflict("emoji_name")),
            "{taken}"
        );
        assert_eq!(db.check_new_emoji(gid, own.id, taken).await, Err(DbError::Conflict("emoji_name")), "{taken}");
    }
    db.add_emoji(other, own.id, "ab", false, "k.png").await.unwrap();
    // Renommer vers un nom pris : refusé ; vers le sien avec une autre casse : permis.
    let [first, second, _] = db.guild_emojis(gid).await.unwrap().try_into().unwrap();
    assert_eq!(db.rename_emoji(gid, second.id, own.id, "AB").await, Err(DbError::Conflict("emoji_name")));
    assert_eq!(db.rename_emoji(gid, second.id, own.id, "é").await, Err(DbError::Invalid("emoji_name")));
    assert_eq!(db.rename_emoji(gid, first.id, own.id, "AB").await.unwrap().name, "AB");
    // Un emoji d'un autre serveur ne se renomme pas par ce serveur-ci.
    let foreign = db.guild_emojis(other).await.unwrap().remove(0);
    assert_eq!(db.rename_emoji(gid, foreign.id, own.id, "zz").await, Err(DbError::NotFound));
    assert_eq!(db.delete_emoji(gid, foreign.id, own.id).await, Err(DbError::NotFound));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn a_server_holds_fifty_emojis_and_a_slot_frees_when_one_goes(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let (gid, _) = guild_with(&db, "Abysse", &own, &[]).await;
    let (other, _) = guild_with(&db, "Récif", &own, &[]).await;
    for i in 0..MAX_EMOJIS {
        db.add_emoji(gid, own.id, &format!("e_{i:02}"), false, &format!("k{i}.png")).await.unwrap();
    }
    assert_eq!(db.add_emoji(gid, own.id, "de_trop", false, "k.png").await, Err(DbError::Invalid("emoji_limit")));
    assert_eq!(db.check_new_emoji(gid, own.id, "de_trop").await, Err(DbError::Invalid("emoji_limit")));
    // Le décompte est par serveur.
    db.add_emoji(other, own.id, "de_trop", false, "k.png").await.unwrap();
    let first = db.guild_emojis(gid).await.unwrap().remove(0);
    db.delete_emoji(gid, first.id, own.id).await.unwrap();
    db.add_emoji(gid, own.id, "de_trop", false, "k.png").await.unwrap();
    assert_eq!(db.guild_emojis(gid).await.unwrap().len(), MAX_EMOJIS);

    // Deux ajouts simultanés pour la dernière place : un seul passe.
    let e = db.guild_emojis(gid).await.unwrap().remove(0);
    db.delete_emoji(gid, e.id, own.id).await.unwrap();
    let (a, b) = tokio::join!(
        db.add_emoji(gid, own.id, "course_a", false, "ka.png"),
        db.add_emoji(gid, own.id, "course_b", false, "kb.png")
    );
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(), 1);
    assert_eq!(db.guild_emojis(gid).await.unwrap().len(), MAX_EMOJIS);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn people_use_the_emojis_of_the_servers_they_are_in(pool: PgPool) {
    let db = Db::from_pool(pool);
    let lou = user(&db, "lou").await;
    let sam = user(&db, "sam").await;
    let noa = user(&db, "noa").await;
    let (a, a_general) = guild_with(&db, "Abysse", &lou, &[&sam]).await;
    let (b, _) = guild_with(&db, "Récif", &sam, &[]).await;
    let ea = db.add_emoji(a, lou.id, "poulpe", false, "a.png").await.unwrap();
    let eb = db.add_emoji(b, sam.id, "corail", true, "b.gif").await.unwrap();

    // Sam est dans les deux serveurs, lou dans un seul, noa dans aucun.
    let mine = |u: &User| {
        let db = db.clone();
        let id = u.id;
        async move { db.user_emojis(id).await.unwrap().into_iter().map(|e| e.name).collect::<Vec<_>>() }
    };
    assert_eq!(mine(&sam).await, ["poulpe", "corail"]);
    assert_eq!(mine(&lou).await, ["poulpe"]);
    assert!(mine(&noa).await.is_empty());
    assert_eq!(db.member_guild_emojis(b, sam.id).await.unwrap().len(), 1);
    assert_eq!(db.member_guild_emojis(b, lou.id).await, Err(DbError::NotFound));

    // Réactions : un caractère Unicode, ou un emoji perso d'un serveur dont on est membre.
    let m = db.create_message(a_general, lou.id, "salut", None).await.unwrap().0;
    let (ea_key, eb_key) = (ea.id.to_string(), eb.id.to_string());
    assert!(db.add_reaction(a_general, m.id, lou.id, &ea_key).await.unwrap());
    assert!(db.add_reaction(a_general, m.id, sam.id, &eb_key).await.unwrap(), "emoji d'un autre de ses serveurs");
    assert_eq!(
        db.add_reaction(a_general, m.id, lou.id, &eb_key).await,
        Err(DbError::Invalid("emoji")),
        "pas membre de Récif"
    );
    assert_eq!(
        db.add_reaction(a_general, m.id, lou.id, &Id::from_bytes([9; 16]).to_string()).await,
        Err(DbError::Invalid("emoji")),
        "inconnu"
    );
    for bad in [ea_key.to_uppercase(), ea_key.replace('-', ""), format!("{{{ea_key}}}"), format!("<:poulpe:{ea_key}>")]
    {
        assert_eq!(db.add_reaction(a_general, m.id, lou.id, &bad).await, Err(DbError::Invalid("emoji")), "{bad}");
    }
    assert!(!db.add_reaction(a_general, m.id, lou.id, &ea_key).await.unwrap(), "déjà là");
    let shown = db.message(m.id).await.unwrap().unwrap();
    assert_eq!(
        shown.reactions,
        [
            Reaction { emoji: ea_key.clone(), users: vec![lou.id] },
            Reaction { emoji: eb_key.clone(), users: vec![sam.id] }
        ]
    );
    // Elles comptent dans les 20 sortes d'un message, comme les autres.
    for (e, _) in kraken_core::emoji::EMOJIS.iter().take(18) {
        db.add_reaction(a_general, m.id, lou.id, e).await.unwrap();
    }
    assert_eq!(db.add_reaction(a_general, m.id, lou.id, "🐙").await, Err(DbError::Invalid("reactions")));
    assert!(db.add_reaction(a_general, m.id, sam.id, &ea_key).await.unwrap(), "une sorte déjà présente reste possible");

    // Retirer l'emoji laisse les réactions en base : le client dit « emoji supprimé ».
    db.delete_emoji(a, ea.id, lou.id).await.unwrap();
    let shown = db.message(m.id).await.unwrap().unwrap();
    assert!(shown.reactions.iter().any(|r| r.emoji == ea_key && r.users.len() == 2));
    // On peut encore retirer sa propre réaction, mais plus en ajouter.
    assert_eq!(db.add_reaction(a_general, m.id, noa.id, &ea_key).await, Err(DbError::Invalid("emoji")));
    assert!(db.remove_reaction(a_general, m.id, lou.id, &ea_key).await.unwrap());

    // Quitter un serveur retire ses emojis de ceux qu'on peut utiliser.
    db.leave_guild(a, sam.id).await.unwrap();
    assert_eq!(mine(&sam).await, ["corail"]);
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn deleting_a_server_takes_its_emojis_and_lists_their_files(pool: PgPool) {
    let db = Db::from_pool(pool);
    let own = user(&db, "lou").await;
    let (gid, general) = guild_with(&db, "Abysse", &own, &[]).await;
    let (other, _) = guild_with(&db, "Récif", &own, &[]).await;
    let e = db.add_emoji(gid, own.id, "poulpe", false, "aaa.png").await.unwrap();
    db.add_emoji(gid, own.id, "danse", true, "bbb.gif").await.unwrap();
    db.add_emoji(other, own.id, "reste", false, "ccc.png").await.unwrap();
    let m = db.create_message(general, own.id, &format!("<:poulpe:{}>", e.id), None).await.unwrap().0;
    db.add_reaction(general, m.id, own.id, &e.id.to_string()).await.unwrap();

    let done = db.delete_guild(gid, own.id, "Abysse").await.unwrap();
    assert_eq!(done.files, ["emojis/aaa.png", "emojis/bbb.gif"]);
    let left: i64 = sqlx::query_scalar("select count(*) from emojis where guild_id = $1")
        .bind(gid)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert_eq!(db.emoji_image(e.id).await.unwrap(), None);
    assert_eq!(db.guild_emojis(other).await.unwrap().len(), 1, "l'autre serveur ne bouge pas");
}
