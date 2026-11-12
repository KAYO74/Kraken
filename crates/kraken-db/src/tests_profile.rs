//! Profil, compte et présence sur une vraie base : champs facultatifs, changement de pseudo,
//! d'adresse e-mail et de mot de passe, statut choisi et qui le voit.

use kraken_proto::{ChannelKind, Presence, USERNAME_CHANGES_PER_WINDOW, USERNAME_WINDOW_S};
use sqlx::PgPool;

use super::*;

async fn user(db: &Db, name: &str) -> User {
    db.create_user(name, &format!("{name}@example.org"), "h".into()).await.unwrap()
}

async fn friends(db: &Db, a: &User, b: &User) {
    db.befriend(a.id, b.id).await.unwrap();
    db.befriend(b.id, a.id).await.unwrap();
}

fn patch<'a>(
    display_name: Option<Option<&'a str>>,
    bio: Option<Option<&'a str>>,
    pronouns: Option<Option<&'a str>>,
) -> ProfilePatch<'a> {
    ProfilePatch { display_name, bio, pronouns }
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn profile_fields_are_patched_one_by_one(pool: PgPool) {
    let db = Db::from_pool(pool);
    let lou = user(&db, "lou").await;
    assert_eq!((lou.bio.as_deref(), lou.pronouns.as_deref(), lou.banner.as_deref()), (None, None, None));
    assert!(lou.created_at.is_some_and(|t| t > 0), "la date de création du compte est donnée");

    let u = db
        .set_profile(lou.id, patch(Some(Some("Lou")), Some(Some("Plongeuse.\nLa nuit.")), Some(Some("elle/elles"))))
        .await
        .unwrap();
    assert_eq!(
        (u.display_name.as_deref(), u.bio.as_deref(), u.pronouns.as_deref()),
        (Some("Lou"), Some("Plongeuse.\nLa nuit."), Some("elle/elles"))
    );
    // Un champ absent reste tel quel : changer les pronoms ne touche ni le nom ni la bio.
    let u = db.set_profile(lou.id, patch(None, None, Some(Some("iel")))).await.unwrap();
    assert_eq!(
        (u.display_name.as_deref(), u.bio.as_deref(), u.pronouns.as_deref()),
        (Some("Lou"), Some("Plongeuse.\nLa nuit."), Some("iel"))
    );
    // Vider un champ l'efface, les autres restent.
    let u = db.set_profile(lou.id, patch(None, Some(None), None)).await.unwrap();
    assert_eq!((u.display_name.as_deref(), u.bio, u.pronouns.as_deref()), (Some("Lou"), None, Some("iel")));
    // Les autres voient les mêmes champs partout où le compte apparaît.
    let sam = user(&db, "sam").await;
    friends(&db, &lou, &sam).await;
    let seen = db.relationships(sam.id).await.unwrap();
    assert_eq!(seen[0].user.pronouns.as_deref(), Some("iel"));
    assert_eq!(db.set_profile(Id::from_u128(7), patch(None, None, None)).await, Err(DbError::NotFound));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn banner_key_is_kept_replaced_and_purged_with_the_account(pool: PgPool) {
    let db = Db::from_pool(pool);
    let lou = user(&db, "lou").await;
    let (u, old) = db.set_banner(lou.id, Some("b1")).await.unwrap();
    assert_eq!((u.banner.as_deref(), old), (Some("b1"), None));
    let (u, old) = db.set_banner(lou.id, Some("b2")).await.unwrap();
    assert_eq!((u.banner.as_deref(), old.as_deref()), (Some("b2"), Some("b1")), "l'ancien fichier est à supprimer");
    // L'avatar et la bannière sont indépendants.
    let (u, old) = db.set_avatar(lou.id, Some("a1")).await.unwrap();
    assert_eq!((u.avatar.as_deref(), u.banner.as_deref(), old), (Some("a1"), Some("b2"), None));
    let (u, old) = db.set_banner(lou.id, None).await.unwrap();
    assert_eq!((u.banner, old.as_deref()), (None, Some("b2")));
    db.set_banner(lou.id, Some("b3")).await.unwrap();

    db.request_deletion(lou.id).await.unwrap();
    let (n, files, _) = db.purge_deleted_accounts(-1).await.unwrap();
    let mut files = files;
    files.sort();
    assert_eq!((n, files), (1, vec!["a1".to_string(), "b3".to_string()]), "photo et bannière sont supprimées");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn username_change_is_unique_limited_and_frees_the_old_name(pool: PgPool) {
    let db = Db::from_pool(pool);
    let lou = user(&db, "lou").await;
    let sam = user(&db, "sam").await;

    assert_eq!(db.change_username(lou.id, "sam").await, Err(DbError::Conflict("username")));
    assert_eq!(db.change_username(lou.id, "Lou!").await, Err(DbError::Invalid("username")));
    assert_eq!(db.change_username(lou.id, "x").await, Err(DbError::Invalid("username")));
    // Les essais refusés ne comptent pas dans la limite.
    let first = db.change_username(lou.id, "lou_2").await.unwrap();
    assert_eq!(first.username, "lou_2");
    // L'ancien pseudo est libre aussitôt : un autre compte le prend.
    assert!(db.user_credentials("lou").await.unwrap().is_none());
    let taker = db.change_username(sam.id, "lou").await.unwrap();
    assert_eq!(taker.username, "lou");
    db.change_username(lou.id, "lou_3").await.unwrap();

    assert_eq!(USERNAME_CHANGES_PER_WINDOW, 2);
    match db.change_username(lou.id, "lou_4").await {
        Err(DbError::Cooldown(secs)) => assert!((1..=USERNAME_WINDOW_S).contains(&secs), "{secs} s à attendre"),
        other => panic!("limite attendue : {other:?}"),
    }
    assert_eq!(db.user_credentials("lou_3").await.unwrap().map(|(u, _)| u.id), Some(lou.id));

    // Un changement vieux de plus d'une heure ne compte plus.
    sqlx::query("update username_changes set changed_at = now() - interval '61 minutes' where user_id = $1")
        .bind(lou.id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(db.change_username(lou.id, "lou_4").await.unwrap().username, "lou_4");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn email_change_waits_for_confirmation_and_keeps_the_old_address(pool: PgPool) {
    let db = Db::from_pool(pool);
    let lou = user(&db, "lou").await;
    let sam = user(&db, "sam").await;
    db.mark_email_verified(lou.id).await.unwrap();
    let a = db.account(lou.id).await.unwrap();
    assert_eq!(
        (a.email.as_deref(), a.email_verified, a.pending_email.as_deref()),
        (Some("lou@example.org"), true, None)
    );

    // Une adresse déjà prise (par un autre compte) est refusée, quelle que soit la casse.
    assert_eq!(db.set_pending_email(lou.id, "SAM@example.org").await, Err(DbError::Conflict("email")));

    db.set_pending_email(lou.id, "lou@exemple.fr").await.unwrap();
    db.create_email_change_token(lou.id, "lou@exemple.fr", b"tok-1", 60).await.unwrap();
    // D'ici la confirmation, l'ancienne adresse reste valable et la nouvelle ne connecte pas.
    let a = db.account(lou.id).await.unwrap();
    assert_eq!(
        (a.email.as_deref(), a.email_verified, a.pending_email.as_deref()),
        (Some("lou@example.org"), true, Some("lou@exemple.fr"))
    );
    assert!(db.user_credentials("lou@example.org").await.unwrap().is_some());
    assert!(db.user_credentials("lou@exemple.fr").await.unwrap().is_none());
    assert_eq!(db.pending_email(lou.id).await.unwrap().map(|(e, _)| e).as_deref(), Some("lou@exemple.fr"));

    // Un jeton inconnu ne change rien ; le bon applique l'adresse, confirmée, une seule fois.
    assert_eq!(db.confirm_email_change(b"nope").await.unwrap(), None);
    assert_eq!(db.confirm_email_change(b"tok-1").await.unwrap(), Some(lou.id));
    let a = db.account(lou.id).await.unwrap();
    assert_eq!((a.email.as_deref(), a.email_verified, a.pending_email), (Some("lou@exemple.fr"), true, None));
    assert!(db.user_credentials("lou@exemple.fr").await.unwrap().is_some());
    assert!(db.user_credentials("lou@example.org").await.unwrap().is_none());
    assert_eq!(db.confirm_email_change(b"tok-1").await.unwrap(), None, "usage unique");

    // Une demande plus récente remplace l'ancienne : le premier lien ne sert plus.
    db.set_pending_email(lou.id, "un@exemple.fr").await.unwrap();
    db.create_email_change_token(lou.id, "un@exemple.fr", b"tok-2", 60).await.unwrap();
    db.set_pending_email(lou.id, "deux@exemple.fr").await.unwrap();
    db.create_email_change_token(lou.id, "deux@exemple.fr", b"tok-3", 60).await.unwrap();
    assert_eq!(db.confirm_email_change(b"tok-2").await.unwrap(), None);
    assert_eq!(db.confirm_email_change(b"tok-3").await.unwrap(), Some(lou.id));
    assert_eq!(db.account(lou.id).await.unwrap().email.as_deref(), Some("deux@exemple.fr"));

    // Un lien expiré ne sert pas non plus.
    db.set_pending_email(lou.id, "trois@exemple.fr").await.unwrap();
    db.create_email_change_token(lou.id, "trois@exemple.fr", b"tok-4", 60).await.unwrap();
    sqlx::query("update auth_tokens set expires_at = now() - interval '1 minute' where kind = 'change_email'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(db.confirm_email_change(b"tok-4").await.unwrap(), None);
    assert_eq!(db.account(lou.id).await.unwrap().email.as_deref(), Some("deux@exemple.fr"));
    // Sans e-mail sur l'instance, la nouvelle adresse remplace l'ancienne tout de suite, non confirmée.
    db.set_email_now(sam.id, "sam@exemple.fr").await.unwrap();
    let a = db.account(sam.id).await.unwrap();
    assert_eq!((a.email.as_deref(), a.email_verified, a.pending_email), (Some("sam@exemple.fr"), false, None));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn changing_the_password_closes_the_other_sessions_only(pool: PgPool) {
    let db = Db::from_pool(pool);
    let lou = user(&db, "lou").await;
    let sam = user(&db, "sam").await;
    for (who, token) in [(&lou, b"here"), (&lou, b"else"), (&sam, b"sams")] {
        db.create_session(who.id, token, "Navigateur", "web").await.unwrap();
    }
    assert_eq!(db.revoke_other_sessions(lou.id, b"here").await.unwrap(), 1);
    assert!(db.session_user(b"here").await.unwrap().is_some(), "celle-ci reste ouverte");
    assert!(db.session_user(b"else").await.unwrap().is_none());
    assert!(db.session_user(b"sams").await.unwrap().is_some(), "les sessions des autres comptes ne bougent pas");
    assert_eq!(db.revoke_other_sessions(lou.id, b"here").await.unwrap(), 0, "rien de plus à fermer");
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn chosen_presence_is_kept(pool: PgPool) {
    let db = Db::from_pool(pool);
    let lou = user(&db, "lou").await;
    assert_eq!(db.presence(lou.id).await.unwrap(), Presence::Online, "en ligne par défaut");
    for p in [Presence::Dnd, Presence::Invisible, Presence::Idle, Presence::Online] {
        db.set_presence(lou.id, p).await.unwrap();
        assert_eq!(db.presence(lou.id).await.unwrap(), p);
    }
    assert_eq!(db.set_presence(Id::from_u128(7), Presence::Dnd).await, Err(DbError::NotFound));
}

#[sqlx::test(migrator = "crate::MIGRATOR")]
async fn presence_is_seen_by_friends_and_members_only_and_never_across_a_block(pool: PgPool) {
    let db = Db::from_pool(pool);
    let (lou, ami, collegue, mp, inconnu, bloque) = (
        user(&db, "lou").await,
        user(&db, "ami").await,
        user(&db, "collegue").await,
        user(&db, "mp").await,
        user(&db, "inconnu").await,
        user(&db, "bloque").await,
    );
    friends(&db, &lou, &ami).await;
    // Un serveur partagé, un MP (ami d'un ami : on s'écrit) et un compte bloqué qui partage pourtant le serveur.
    let g = db.create_guild(lou.id, "Abysse").await.unwrap();
    let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
    let inv = db.create_invite(g.guild.id, Some(general), lou.id, 0, None).await.unwrap();
    for u in [&collegue, &bloque] {
        db.use_invite(&inv.code, u.id).await.unwrap();
    }
    friends(&db, &lou, &mp).await;
    db.open_dm(lou.id, mp.id).await.unwrap();
    db.remove_relationship(lou.id, mp.id).await.unwrap();

    let mut want = vec![lou.id, ami.id, collegue.id, mp.id, bloque.id];
    want.sort_unstable();
    assert_eq!(db.presence_peers(lou.id).await.unwrap(), want);
    assert!(!db.presence_peers(lou.id).await.unwrap().contains(&inconnu.id));
    assert!(!db.presence_peers(inconnu.id).await.unwrap().contains(&lou.id), "la vue est réciproque");

    // Un blocage coupe la vue dans les deux sens, même avec un serveur en commun.
    db.block(lou.id, bloque.id).await.unwrap();
    assert!(!db.presence_peers(lou.id).await.unwrap().contains(&bloque.id));
    assert!(!db.presence_peers(bloque.id).await.unwrap().contains(&lou.id));
    // Débloquer rend la vue.
    db.remove_relationship(lou.id, bloque.id).await.unwrap();
    assert!(db.presence_peers(lou.id).await.unwrap().contains(&bloque.id));
    // Un compte en cours de suppression ne voit plus personne et n'est plus vu.
    db.request_deletion(ami.id).await.unwrap();
    assert!(!db.presence_peers(lou.id).await.unwrap().contains(&ami.id));
}
