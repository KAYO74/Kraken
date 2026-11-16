//! Messages épinglés d'un salon : en épingler, en désépingler (au plus [`MAX_PINS`] par salon), les lister.
//! Le droit (« Gérer les messages » dans un serveur, tout participant en MP ou en groupe) est vérifié par
//! `kraken-server` ; ici, la limite de 50 tient même si deux épinglages arrivent ensemble (un verrou par
//! salon), et chaque changement dans un serveur laisse sa trace dans le journal d'audit (salon et message,
//! jamais le texte). Épingler un message déjà épinglé, ou désépingler un autre, ne change rien et n'écrit rien.
//! Dans un MP où l'un a bloqué l'autre, épingler et désépingler sont refusés comme écrire (`pin_blocked`, `pin_refused`) ; lire la liste reste permis.

use kraken_proto::*;
use serde_json::json;

use crate::moderation::audit_changes;
use crate::{Changed, Db, DbError, Result, message_from, message_sql};

impl Db {
    /// Épingle (`pin`) ou désépingle un message de ce salon, au nom de `by`. Rend le message tel qu'il est
    /// maintenant, et si cela a changé quelque chose. `NotFound` si le message n'est pas dans ce salon ;
    /// `Invalid("pin_limit")` au-delà de [`MAX_PINS`] ; `Invalid("pin_kind")` pour un message qui n'est ni
    /// un texte ni une réponse (appel).
    pub async fn set_pinned(&self, cid: Id, mid: Id, by: Id, pin: bool) -> Result<(Message, Changed)> {
        // Épingler est une écriture : un MP bloqué la refuse comme l'envoi d'un message (403), avec une phrase
        // qui parle d'épingles (`pin_blocked` : j'ai bloqué l'autre ; `pin_refused` : sans dire qui a bloqué).
        if self.channel(cid).await?.is_some_and(|c| c.kind == ChannelKind::Dm) {
            self.dm_writable(cid, by).await.map_err(|e| match e {
                DbError::Forbidden("dm_blocked") => DbError::Forbidden("pin_blocked"),
                DbError::Forbidden("dm_refused") => DbError::Forbidden("pin_refused"),
                e => e,
            })?;
        }
        let mut tx = self.pool.begin().await?;
        // Un verrou par salon : deux épinglages simultanés se comptent l'un après l'autre.
        sqlx::query("select pg_advisory_xact_lock(hashtextextended('pins:' || $1::text, 0))")
            .bind(cid)
            .execute(&mut *tx)
            .await?;
        let (author, pinned, kind): (Option<Id>, bool, String) = sqlx::query_as(
            "select author_id, pinned_at is not null, kind from messages where id = $1 and channel_id = $2 for update",
        )
        .bind(mid)
        .bind(cid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DbError::NotFound)?;
        let changed = pinned != pin;
        if changed {
            if pin {
                if !matches!(kind.as_str(), "default" | "reply") {
                    return Err(DbError::Invalid("pin_kind"));
                }
                let n: i64 =
                    sqlx::query_scalar("select count(*) from messages where channel_id = $1 and pinned_at is not null")
                        .bind(cid)
                        .fetch_one(&mut *tx)
                        .await?;
                if n as usize >= MAX_PINS {
                    return Err(DbError::Invalid("pin_limit"));
                }
            }
            sqlx::query("update messages set pinned_at = case when $2 then clock_timestamp() end where id = $1")
                .bind(mid)
                .bind(pin)
                .execute(&mut *tx)
                .await?;
            // Dans un serveur seulement : un MP n'a pas de journal. Jamais le texte du message.
            let (gid, name): (Option<Id>, Option<String>) =
                sqlx::query_as("select guild_id, name from channels where id = $1")
                    .bind(cid)
                    .fetch_one(&mut *tx)
                    .await?;
            if let Some(gid) = gid {
                audit_changes(
                    &mut tx,
                    gid,
                    by,
                    if pin { "message_pin" } else { "message_unpin" },
                    author,
                    json!({ "channel_id": cid, "channel": name.unwrap_or_default(), "message_id": mid }),
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok((self.message(mid).await?.ok_or(DbError::NotFound)?, changed))
    }

    /// Les messages épinglés du salon, le plus récemment épinglé d'abord, tels que l'historique les rend.
    pub async fn pinned_messages(&self, cid: Id) -> Result<Vec<Message>> {
        let rows = sqlx::query(&message_sql(
            "where m.channel_id = $1 and m.pinned_at is not null order by m.pinned_at desc, m.id desc limit $2",
        ))
        .bind(cid)
        .bind(MAX_PINS as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(message_from).collect::<sqlx::Result<_>>().map_err(Into::into)
    }
}
