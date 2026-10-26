//! Données de démonstration (`kraken-server --seed`) : personnages fictifs, serveurs, salons,
//! messages, amis et MP, pour que chaque écran ait quelque chose de réel à montrer.
//! Ne fait rien si la base contient déjà des comptes. Compte de démo : `demo`.

use kraken_core::perms;
use kraken_proto::{ChannelKind, Id};
use sqlx::{Postgres, Transaction};

use crate::{Db, Result, insert_channel};

type Tx<'a> = Transaction<'a, Postgres>;

async fn user(tx: &mut Tx<'_>, username: &str, display: Option<&str>, hash: &str) -> Result<Id> {
    Ok(sqlx::query_scalar(
        "insert into users (username, display_name, email, email_verified_at, password_hash)
         values ($1, $2, $1 || '@demo.kraken.invalid', now(), $3) returning id",
    )
    .bind(username)
    .bind(display)
    .bind(hash)
    .fetch_one(&mut **tx)
    .await?)
}

async fn say(tx: &mut Tx<'_>, channel: Id, author: Id, text: &str) -> Result<()> {
    sqlx::query("insert into messages (channel_id, author_id, content) values ($1, $2, $3)")
        .bind(channel)
        .bind(author)
        .bind(text)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn guild(tx: &mut Tx<'_>, owner: Id, name: &str) -> Result<Id> {
    let gid: Id = sqlx::query_scalar("insert into guilds (owner_id, name) values ($1, $2) returning id")
        .bind(owner)
        .bind(name)
        .fetch_one(&mut **tx)
        .await?;
    sqlx::query("insert into roles (id, guild_id, name, permissions) values ($1, $1, '@everyone', $2)")
        .bind(gid)
        .bind(perms::DEFAULT_EVERYONE as i64)
        .execute(&mut **tx)
        .await?;
    Ok(gid)
}

async fn join(tx: &mut Tx<'_>, gid: Id, uid: Id) -> Result<()> {
    sqlx::query("insert into members (guild_id, user_id) values ($1, $2)")
        .bind(gid)
        .bind(uid)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn dm(tx: &mut Tx<'_>, kind: &str, name: Option<&str>, users: &[Id]) -> Result<Id> {
    let key = (kind == "dm").then(|| {
        let mut k = users.iter().map(Id::to_string).collect::<Vec<_>>();
        k.sort();
        k.join(":")
    });
    let id: Id =
        sqlx::query_scalar("insert into channels (kind, name, dm_key, owner_id) values ($1, $2, $3, $4) returning id")
            .bind(kind)
            .bind(name)
            .bind(key)
            .bind(users[0])
            .fetch_one(&mut **tx)
            .await?;
    for u in users {
        sqlx::query("insert into channel_recipients (channel_id, user_id) values ($1, $2)")
            .bind(id)
            .bind(u)
            .execute(&mut **tx)
            .await?;
    }
    Ok(id)
}

impl Db {
    pub async fn seed(&self, password_hash: &str) -> Result<bool> {
        let n: i64 = sqlx::query_scalar("select count(*) from users").fetch_one(&self.pool).await?;
        if n > 0 {
            return Ok(false);
        }
        let mut tx = self.pool.begin().await?;
        let t = &mut tx;
        let h = password_hash;
        let demo = user(t, "demo", Some("Démo"), h).await?;
        let lou = user(t, "lou", Some("Lou"), h).await?;
        let sam = user(t, "sam", Some("Sam"), h).await?;
        let noa = user(t, "noa", None, h).await?;
        let ilan = user(t, "ilan", Some("Ilan"), h).await?;
        let long = user(t, "abyssal_navigator.42", Some("Navigatrice des fonds abyssaux"), h).await?;
        // Bloqué par demo : ses messages sont repliés dans Abysse.
        let zed = user(t, "zed", Some("Zed"), h).await?;

        // Abysse : le serveur principal.
        let g = guild(t, lou, "Abysse").await?;
        let modo: Id = sqlx::query_scalar(
            "insert into roles (guild_id, name, color, position, permissions) values ($1, 'Modération', $2, 1, $3) returning id",
        )
        .bind(g)
        .bind(0x2b7a78)
        .bind((perms::MANAGE_MESSAGES | perms::KICK | perms::TIMEOUT | perms::VIEW_AUDIT_LOG) as i64)
        .fetch_one(&mut **t)
        .await?;
        for u in [lou, demo, sam, noa, long, zed] {
            join(t, g, u).await?;
        }
        sqlx::query("insert into member_roles (guild_id, user_id, role_id) values ($1, $2, $3)")
            .bind(g)
            .bind(sam)
            .bind(modo)
            .execute(&mut **t)
            .await?;
        let news = insert_channel(t, g, None, ChannelKind::Announcement, "annonces").await?;
        let open = insert_channel(t, g, None, ChannelKind::Category, "Salons ouverts").await?;
        let general = insert_channel(t, g, Some(open), ChannelKind::Text, "général").await?;
        sqlx::query("update channels set topic = 'On parle de tout, gentiment.' where id = $1")
            .bind(general)
            .execute(&mut **t)
            .await?;
        let shots = insert_channel(t, g, Some(open), ChannelKind::Text, "captures").await?;
        insert_channel(
            t,
            g,
            Some(open),
            ChannelKind::Text,
            "un-salon-au-nom-vraiment-très-long-pour-tester-la-troncature",
        )
        .await?;
        insert_channel(t, g, Some(open), ChannelKind::Forum, "idées").await?;
        let vocal = insert_channel(t, g, None, ChannelKind::Category, "Vocal").await?;
        insert_channel(t, g, Some(vocal), ChannelKind::Voice, "Salle vocale").await?;
        let team = insert_channel(t, g, None, ChannelKind::Category, "Équipe").await?;
        let staff = insert_channel(t, g, Some(team), ChannelKind::Text, "staff").await?;
        for (target, allow, deny) in [(g, 0, perms::VIEW_CHANNEL), (modo, perms::VIEW_CHANNEL, 0)] {
            sqlx::query(
                "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny) values ($1, $2, 'role', $3, $4)",
            )
            .bind(team)
            .bind(target)
            .bind(allow as i64)
            .bind(deny as i64)
            .execute(&mut **t)
            .await?;
        }
        say(t, news, lou, "Bienvenue dans Abysse ! Les règles sont simples : respect et bonne humeur.").await?;
        for (a, text) in [
            (lou, "Quelqu'un a testé la nouvelle version ?"),
            (sam, "Oui, l'appel vidéo est super fluide."),
            (sam, "Et le partage d'écran en 1080p sans rien payer, c'est appréciable."),
            (noa, "Je vous rejoins ce soir dans la salle vocale."),
            (
                long,
                "Message d'un compte au nom très long, pour vérifier que l'en-tête ne déborde pas de sa colonne quand la fenêtre est étroite.",
            ),
            (lou, "Parfait, à ce soir alors."),
        ] {
            say(t, general, a, text).await?;
        }
        say(t, general, zed, "Qui veut des codes promo ? Écrivez-moi en privé, vite vite vite !").await?;
        say(t, shots, noa, "Ma plus belle capture de la semaine arrive bientôt.").await?;
        say(t, staff, sam, "Salon réservé à la modération.").await?;

        // Récif : un second serveur, presque vide.
        let r = guild(t, demo, "Récif").await?;
        join(t, r, demo).await?;
        insert_channel(t, r, None, ChannelKind::Text, "accueil").await?;

        // Social.
        for (u, mine, theirs) in [
            (lou, "friend", "friend"),
            (sam, "friend", "friend"),
            (noa, "friend", "friend"),
            (long, "friend", "friend"),
            (ilan, "pending_in", "pending_out"),
        ] {
            for (a, b, k) in [(demo, u, mine), (u, demo, theirs)] {
                sqlx::query("insert into relationships (user_id, target_id, kind) values ($1, $2, $3)")
                    .bind(a)
                    .bind(b)
                    .bind(k)
                    .execute(&mut **t)
                    .await?;
            }
        }
        sqlx::query("insert into relationships (user_id, target_id, kind) values ($1, $2, 'blocked')")
            .bind(demo)
            .bind(zed)
            .execute(&mut **t)
            .await?;
        let one = dm(t, "dm", None, &[demo, lou]).await?;
        say(t, one, lou, "Tu as vu l'invitation pour Abysse ?").await?;
        let group = dm(t, "group_dm", Some("Équipée du soir"), &[demo, sam, noa]).await?;
        say(t, group, sam, "On se retrouve à 21 h ?").await?;
        tx.commit().await?;
        Ok(true)
    }
}
