//! Emojis perso d'un serveur : ajouter, renommer, retirer (droit « Gérer les émojis »), les lister,
//! et savoir lesquels une personne peut utiliser (ceux des serveurs dont elle est membre). L'image
//! est gardée sur le disque par `kraken-server` ; ici, seule sa clé (`image_key`) passe. Chaque
//! écriture se fait dans une transaction qui verrouille le serveur (le décompte de 50 tient même si
//! deux ajouts arrivent ensemble) et laisse sa trace dans le journal d'audit.

use kraken_core::perms;
use kraken_proto::*;
use serde_json::json;
use sqlx::Row;
use sqlx::postgres::PgRow;

use crate::moderation::{actor, audit_changes, lock};
use crate::{Db, DbError, Result};

const COLS: &str =
    "e.id, e.guild_id, e.name, e.animated, e.creator_id, coalesce(u.display_name, u.username) as creator_name";
const FROM: &str = "emojis e left join users u on u.id = e.creator_id";

fn emoji_from(r: &PgRow) -> sqlx::Result<Emoji> {
    let creator_id: Option<Id> = r.try_get("creator_id")?;
    Ok(Emoji {
        id: r.try_get("id")?,
        guild_id: r.try_get("guild_id")?,
        name: r.try_get("name")?,
        animated: r.try_get("animated")?,
        creator: match (creator_id, r.try_get::<Option<String>, _>("creator_name")?) {
            (Some(id), Some(name)) => Some(EmojiCreator { id, name }),
            _ => None,
        },
    })
}

fn clean_name(name: &str) -> Result<&str> {
    if kraken_core::emoji::valid_name(name) { Ok(name) } else { Err(DbError::Invalid("emoji_name")) }
}

/// Les emojis d'un serveur, du plus ancien au plus récent.
async fn list_in(db: &Db, gid: Id) -> Result<Vec<Emoji>> {
    let rows = sqlx::query(&format!("select {COLS} from {FROM} where e.guild_id = $1 order by e.id"))
        .bind(gid)
        .fetch_all(&db.pool)
        .await?;
    rows.iter().map(emoji_from).collect::<sqlx::Result<_>>().map_err(Into::into)
}

impl Db {
    /// Les emojis perso du serveur, du plus ancien au plus récent.
    pub async fn guild_emojis(&self, gid: Id) -> Result<Vec<Emoji>> {
        list_in(self, gid).await
    }

    /// Ceux de ce serveur, pour un membre ; `NotFound` pour qui n'en est pas (on ne révèle rien).
    pub async fn member_guild_emojis(&self, gid: Id, uid: Id) -> Result<Vec<Emoji>> {
        let member: bool =
            sqlx::query_scalar("select exists(select 1 from members where guild_id = $1 and user_id = $2)")
                .bind(gid)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?;
        if !member {
            return Err(DbError::NotFound);
        }
        list_in(self, gid).await
    }

    /// Tous ceux que cette personne peut utiliser : les emojis des serveurs dont elle est membre,
    /// serveur par serveur (dans l'ordre où elle les a rejoints), puis du plus ancien au plus récent.
    pub async fn user_emojis(&self, uid: Id) -> Result<Vec<Emoji>> {
        let rows = sqlx::query(&format!(
            "select {COLS} from {FROM} join members m on m.guild_id = e.guild_id
             where m.user_id = $1 order by m.joined_at, e.guild_id, e.id"
        ))
        .bind(uid)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(emoji_from).collect::<sqlx::Result<_>>().map_err(Into::into)
    }

    /// Avant de travailler sur une image : cette personne peut-elle ajouter cet emoji ? Mêmes
    /// refus que [`Self::add_emoji`] (membre introuvable, droit manquant, nom invalide ou pris,
    /// serveur plein), sans rien écrire.
    pub async fn check_new_emoji(&self, gid: Id, by: Id, name: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let owner: Id = sqlx::query_scalar("select owner_id from guilds where id = $1")
            .bind(gid)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_EMOJIS).await?;
        let name = clean_name(name)?;
        let (count, taken): (i64, bool) = sqlx::query_as(
            "select count(*), coalesce(bool_or(lower(name) = lower($2)), false) from emojis where guild_id = $1",
        )
        .bind(gid)
        .bind(name)
        .fetch_one(&mut *tx)
        .await?;
        if count as usize >= MAX_EMOJIS {
            return Err(DbError::Invalid("emoji_limit"));
        }
        if taken {
            return Err(DbError::Conflict("emoji_name"));
        }
        Ok(())
    }

    /// Ajoute un emoji ; `image_key` est le nom du fichier déjà écrit (`clé.png`, `clé.gif`…).
    pub async fn add_emoji(&self, gid: Id, by: Id, name: &str, animated: bool, image_key: &str) -> Result<Emoji> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_EMOJIS).await?;
        let name = clean_name(name)?;
        let count: i64 =
            sqlx::query_scalar("select count(*) from emojis where guild_id = $1").bind(gid).fetch_one(&mut *tx).await?;
        if count as usize >= MAX_EMOJIS {
            return Err(DbError::Invalid("emoji_limit"));
        }
        let eid: Id = sqlx::query_scalar(
            "insert into emojis (guild_id, creator_id, name, animated, image_key) values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(gid)
        .bind(by)
        .bind(name)
        .bind(animated)
        .bind(image_key)
        .fetch_one(&mut *tx)
        .await?;
        audit_changes(&mut tx, gid, by, "emoji_create", Some(eid), json!({ "name": name, "animated": animated }))
            .await?;
        let r =
            sqlx::query(&format!("select {COLS} from {FROM} where e.id = $1")).bind(eid).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(emoji_from(&r)?)
    }

    /// Renomme un emoji ; l'image, l'id et donc les messages qui le citent ne changent pas.
    pub async fn rename_emoji(&self, gid: Id, eid: Id, by: Id, name: &str) -> Result<Emoji> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_EMOJIS).await?;
        let name = clean_name(name)?;
        let old: String = sqlx::query_scalar("select name from emojis where id = $1 and guild_id = $2 for update")
            .bind(eid)
            .bind(gid)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        if old != name {
            sqlx::query("update emojis set name = $2, updated_at = now() where id = $1")
                .bind(eid)
                .bind(name)
                .execute(&mut *tx)
                .await?;
            audit_changes(&mut tx, gid, by, "emoji_update", Some(eid), json!({ "name": { "old": old, "new": name } }))
                .await?;
        }
        let r =
            sqlx::query(&format!("select {COLS} from {FROM} where e.id = $1")).bind(eid).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(emoji_from(&r)?)
    }

    /// Retire un emoji. Rend la clé de son image, que l'appelant efface du disque. Les réactions
    /// faites avec lui restent (elles disent l'id), les messages qui le citent aussi.
    pub async fn delete_emoji(&self, gid: Id, eid: Id, by: Id) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_EMOJIS).await?;
        let (name, key): (String, String) =
            sqlx::query_as("delete from emojis where id = $1 and guild_id = $2 returning name, image_key")
                .bind(eid)
                .bind(gid)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(DbError::NotFound)?;
        audit_changes(&mut tx, gid, by, "emoji_delete", Some(eid), json!({ "name": name })).await?;
        tx.commit().await?;
        Ok(key)
    }

    /// La clé de l'image d'un emoji, s'il existe.
    pub async fn emoji_image(&self, eid: Id) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("select image_key from emojis where id = $1")
            .bind(eid)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Cette personne peut-elle réagir avec cet emoji ? Oui s'il existe et vient d'un serveur dont
    /// elle est membre.
    pub(crate) async fn can_use_emoji(&self, eid: Id, uid: Id) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "select exists(select 1 from emojis e join members m on m.guild_id = e.guild_id
                           where e.id = $1 and m.user_id = $2)",
        )
        .bind(eid)
        .bind(uid)
        .fetch_one(&self.pool)
        .await?)
    }
}
