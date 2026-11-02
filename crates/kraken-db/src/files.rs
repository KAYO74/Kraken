//! Fichiers joints et aperçus de liens : lignes de `attachments`, colonne `messages.embeds`.
//! Les fichiers eux-mêmes sont sur disque (voir `kraken-server`) ; ici, seuls leurs chemins,
//! relatifs au dossier de données, sortent de la base pour être supprimés avec leur ligne.

use kraken_proto::*;
use sqlx::Row;

use crate::{Db, DbError, Result};

/// Ce que le serveur doit savoir d'un fichier joint pour le servir.
#[derive(Debug, Clone)]
pub struct StoredAttachment {
    pub id: Id,
    pub channel_id: Id,
    pub message_id: Option<Id>,
    pub uploader_id: Option<Id>,
    pub filename: String,
    pub mime: String,
    pub size: u64,
    pub storage_key: String,
    pub preview: bool,
}

/// Un envoi tout juste reçu.
pub struct NewAttachment<'a> {
    pub channel_id: Id,
    pub uploader_id: Id,
    pub filename: &'a str,
    pub mime: &'a str,
    pub size: u64,
    pub storage_key: &'a str,
    pub dims: Option<(u32, u32)>,
    pub preview: bool,
}

/// Chemins (sous le dossier de données) des fichiers d'un envoi.
pub fn attachment_paths(key: &str, preview: bool) -> Vec<String> {
    let mut v = vec![format!("attachments/{key}")];
    if preview {
        v.push(format!("attachments/{key}.p"));
    }
    v
}

/// Chemins des images gardées par des aperçus de liens.
pub fn embed_paths(embeds: &[Embed]) -> Vec<String> {
    embeds
        .iter()
        .flat_map(|e| e.image.iter().map(|i| i.key.as_str()).chain(e.favicon.as_deref()))
        .map(|k| format!("embeds/{k}"))
        .collect()
}

impl Db {
    pub async fn create_attachment(&self, a: NewAttachment<'_>) -> Result<Id> {
        let (w, h) = a.dims.map_or((None, None), |(w, h)| (Some(w as i32), Some(h as i32)));
        let id = sqlx::query_scalar(
            "insert into attachments (channel_id, uploader_id, filename, mime, size_bytes, storage_key, width, height, preview)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9) returning id",
        )
        .bind(a.channel_id)
        .bind(a.uploader_id)
        .bind(a.filename)
        .bind(a.mime)
        .bind(a.size as i64)
        .bind(a.storage_key)
        .bind(w)
        .bind(h)
        .bind(a.preview)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn attachment(&self, id: Id) -> Result<Option<StoredAttachment>> {
        let r = sqlx::query(
            "select id, channel_id, message_id, uploader_id, filename, mime, size_bytes, storage_key, preview
             from attachments where id = $1 and channel_id is not null",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = r else { return Ok(None) };
        Ok(Some(StoredAttachment {
            id: r.try_get("id")?,
            channel_id: r.try_get("channel_id")?,
            message_id: r.try_get("message_id")?,
            uploader_id: r.try_get("uploader_id")?,
            filename: r.try_get("filename")?,
            mime: r.try_get("mime")?,
            size: r.try_get::<i64, _>("size_bytes")? as u64,
            storage_key: r.try_get("storage_key")?,
            preview: r.try_get("preview")?,
        }))
    }

    /// Envois jamais rattachés à un message, plus vieux que `hours` heures : supprimés, leurs
    /// chemins rendus.
    pub async fn purge_orphan_attachments(&self, hours: i32) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "delete from attachments where message_id is null and created_at < now() - make_interval(hours => $1)
             returning storage_key, preview",
        )
        .bind(hours)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for r in rows {
            out.extend(attachment_paths(r.try_get("storage_key")?, r.try_get("preview")?));
        }
        Ok(out)
    }

    /// Les embeds d'un message (cachés compris), son texte et son salon.
    pub async fn message_embeds(&self, mid: Id) -> Result<Option<(Id, String, Vec<Embed>)>> {
        let r = sqlx::query("select channel_id, content, embeds from messages where id = $1")
            .bind(mid)
            .fetch_optional(&self.pool)
            .await?;
        let Some(r) = r else { return Ok(None) };
        let embeds: sqlx::types::Json<Vec<Embed>> = r.try_get("embeds")?;
        Ok(Some((r.try_get("channel_id")?, r.try_get("content")?, embeds.0)))
    }

    /// Remplace les embeds d'un message encore là ; rend le message à jour, ou `None` s'il a
    /// disparu entre-temps. L'heure de modification ne bouge pas.
    pub async fn set_embeds(&self, mid: Id, embeds: &[Embed]) -> Result<Option<Message>> {
        let done = sqlx::query("update messages set embeds = $2 where id = $1")
            .bind(mid)
            .bind(sqlx::types::Json(embeds))
            .execute(&self.pool)
            .await?;
        if done.rows_affected() == 0 {
            return Ok(None);
        }
        self.message(mid).await
    }

    /// L'auteur retire l'aperçu de ce lien : seule l'adresse reste en base, cachée, pour que
    /// l'aperçu ne revienne pas à une modification. Rend le message à jour et les chemins des
    /// images devenues inutiles.
    pub async fn hide_embed(&self, cid: Id, mid: Id, uid: Id, url: &str) -> Result<(Message, Vec<String>)> {
        let mut tx = self.pool.begin().await?;
        let r = sqlx::query("select author_id, embeds from messages where id = $1 and channel_id = $2 for update")
            .bind(mid)
            .bind(cid)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        if r.try_get::<Option<Id>, _>("author_id")? != Some(uid) {
            return Err(DbError::Forbidden("author"));
        }
        let mut embeds = r.try_get::<sqlx::types::Json<Vec<Embed>>, _>("embeds")?.0;
        let e = embeds.iter_mut().find(|e| e.url == url && !e.hidden).ok_or(DbError::NotFound)?;
        let files = embed_paths(std::slice::from_ref(e));
        *e = Embed { url: e.url.clone(), hidden: true, ..Default::default() };
        sqlx::query("update messages set embeds = $2 where id = $1")
            .bind(mid)
            .bind(sqlx::types::Json(&embeds))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok((self.message(mid).await?.ok_or(DbError::NotFound)?, files))
    }

    /// Retire un envoi pas encore rattaché à un message (le fichier retiré de la saisie) ;
    /// rend les chemins à supprimer, `None` s'il n'existe pas ou n'est pas à cette personne.
    pub async fn delete_pending_attachment(&self, id: Id, uid: Id) -> Result<Option<Vec<String>>> {
        let r = sqlx::query(
            "delete from attachments where id = $1 and uploader_id = $2 and message_id is null
             returning storage_key, preview",
        )
        .bind(id)
        .bind(uid)
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = r else { return Ok(None) };
        Ok(Some(attachment_paths(r.try_get("storage_key")?, r.try_get("preview")?)))
    }

    /// Envois en attente (pas encore rattachés à un message) de cet utilisateur.
    pub async fn pending_attachments(&self, uid: Id) -> Result<i64> {
        Ok(sqlx::query_scalar("select count(*) from attachments where uploader_id = $1 and message_id is null")
            .bind(uid)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Cette image d'aperçu appartient-elle à un message visible de ce salon ?
    pub async fn embed_file_in_channel(&self, cid: Id, key: &str) -> Result<bool> {
        let image = serde_json::json!([{ "image": { "key": key } }]);
        let icon = serde_json::json!([{ "favicon": key }]);
        let found: bool = sqlx::query_scalar(
            "select exists(select 1 from messages where channel_id = $1
                           and not embeds = '[]'::jsonb and (embeds @> $2 or embeds @> $3))",
        )
        .bind(cid)
        .bind(image)
        .bind(icon)
        .fetch_one(&self.pool)
        .await?;
        Ok(found)
    }
}
