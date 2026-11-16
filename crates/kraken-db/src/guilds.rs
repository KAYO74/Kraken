//! Nom et icône d'un serveur : les modifier demande le droit « Gérer le serveur » (le propriétaire
//! l'a toujours). Chaque changement se fait dans une transaction qui verrouille le serveur et laisse sa
//! trace dans le journal d'audit (`guild_update`). L'image de l'icône est gardée sur le disque par
//! `kraken-server` ; ici, seule sa clé (`guilds.icon_key`) passe.

use kraken_core::perms;
use kraken_proto::*;
use serde_json::json;
use sqlx::Row;

use crate::moderation::{Tx, actor, audit_changes, lock};
use crate::{Db, DbError, Result};

/// Un caractère qui n'a pas sa place dans un nom de serveur : un caractère de contrôle (tabulation, saut de ligne,
/// NUL…), comme dans un nom de salon, et les séparateurs de ligne et de paragraphe U+2028 / U+2029, qui coupent une
/// ligne sans en être un pour `char::is_control`.
fn forbidden_in_guild_name(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')
}

/// Le nom d'un serveur : 2 à 100 caractères, rien d'invisible, aucun caractère de contrôle ni saut de ligne (jugés
/// avant de retirer les espaces autour : une tabulation en bout de nom est refusée comme une autre). Rend le nom
/// nettoyé de ses espaces autour.
pub(crate) fn clean_guild_name(name: &str) -> Result<&str> {
    if name.chars().any(forbidden_in_guild_name) {
        return Err(DbError::Invalid("guild_name_chars"));
    }
    let name = name.trim();
    if !(2..=100).contains(&name.chars().count()) {
        return Err(DbError::Invalid("guild_name"));
    }
    if !kraken_core::text::visible_name(name) {
        return Err(DbError::Invalid("hidden_chars"));
    }
    Ok(name)
}

async fn guild_in(tx: &mut Tx<'_>, gid: Id) -> Result<Guild> {
    let r = sqlx::query("select id, name, icon_key, owner_id from guilds where id = $1")
        .bind(gid)
        .fetch_one(&mut **tx)
        .await?;
    Ok(Guild {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        icon: r.try_get("icon_key")?,
        owner_id: r.try_get("owner_id")?,
    })
}

impl Db {
    /// Avant de travailler sur une image : cette personne peut-elle modifier ce serveur ? Mêmes refus que
    /// [`Self::set_guild_icon`] (non membre : introuvable ; droit manquant : interdit), sans rien écrire.
    pub async fn check_guild_edit(&self, gid: Id, by: Id) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let owner: Id = sqlx::query_scalar("select owner_id from guilds where id = $1")
            .bind(gid)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_GUILD).await?;
        Ok(())
    }

    /// Renomme le serveur (mêmes règles de nom qu'à la création). Un nom inchangé n'écrit rien au journal.
    pub async fn rename_guild(&self, gid: Id, by: Id, name: &str) -> Result<Guild> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_GUILD).await?;
        let name = clean_guild_name(name)?;
        let old: String =
            sqlx::query_scalar("select name from guilds where id = $1").bind(gid).fetch_one(&mut *tx).await?;
        if old != name {
            sqlx::query("update guilds set name = $2, updated_at = now() where id = $1")
                .bind(gid)
                .bind(name)
                .execute(&mut *tx)
                .await?;
            audit_changes(&mut tx, gid, by, "guild_update", None, json!({ "name": { "old": old, "new": name } }))
                .await?;
        }
        let guild = guild_in(&mut tx, gid).await?;
        tx.commit().await?;
        Ok(guild)
    }

    /// Pose (ou retire, avec `None`) la clé de l'icône ; rend le serveur à jour et l'ancienne clé, dont le
    /// fichier est à supprimer. Retirer une icône qui n'existe pas n'écrit rien au journal.
    pub async fn set_guild_icon(&self, gid: Id, by: Id, key: Option<&str>) -> Result<(Guild, Option<String>)> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_GUILD).await?;
        let old: Option<String> =
            sqlx::query_scalar("select icon_key from guilds where id = $1").bind(gid).fetch_one(&mut *tx).await?;
        if key.is_some() || old.is_some() {
            sqlx::query("update guilds set icon_key = $2, updated_at = now() where id = $1")
                .bind(gid)
                .bind(key)
                .execute(&mut *tx)
                .await?;
            let what = if key.is_some() { "changed" } else { "removed" };
            audit_changes(&mut tx, gid, by, "guild_update", None, json!({ "icon": what })).await?;
        }
        let guild = guild_in(&mut tx, gid).await?;
        tx.commit().await?;
        Ok((guild, old))
    }
}
