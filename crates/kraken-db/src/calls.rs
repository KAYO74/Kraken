//! Appels des MP et des groupes : qui peut appeler, qui faire sonner, et le message d'appel
//! posté dans la conversation (qui y est entré, quand il a pris fin).

use kraken_proto::*;

use crate::{Db, DbError, Result, message_sql};

impl Db {
    /// Puis-je appeler (ou entrer dans l'appel de) cette conversation ? Rend chez qui faire
    /// sonner : les autres participants, sauf ceux avec qui il y a un blocage (dans un sens ou
    /// dans l'autre). Refusé : pas une conversation dont je fais partie (`NotFound`), MP avec
    /// quelqu'un que j'ai bloqué (`Forbidden("call_blocked")`) ou qui m'a bloqué
    /// (`Forbidden("call_refused")`).
    pub async fn call_access(&self, cid: Id, me: Id) -> Result<(ChannelKind, Vec<Id>)> {
        let c = self.private_channel(cid, me).await?.ok_or(DbError::NotFound)?;
        let rows: Vec<(Id, bool, bool)> = sqlx::query_as(
            "select cr.user_id,
                    exists(select 1 from relationships where user_id = $2 and target_id = cr.user_id and kind = 'blocked'),
                    exists(select 1 from relationships where user_id = cr.user_id and target_id = $2 and kind = 'blocked')
             from channel_recipients cr where cr.channel_id = $1 and cr.user_id <> $2
             order by cr.created_at, cr.user_id",
        )
        .bind(cid)
        .bind(me)
        .fetch_all(&self.pool)
        .await?;
        if c.kind == ChannelKind::Dm {
            match rows.first() {
                Some((_, true, _)) => return Err(DbError::Forbidden("call_blocked")),
                Some((_, _, true)) => return Err(DbError::Forbidden("call_refused")),
                _ => {}
            }
        }
        Ok((c.kind, rows.into_iter().filter(|(_, mine, theirs)| !mine && !theirs).map(|(u, ..)| u).collect()))
    }

    /// Poste le message d'appel de `author` dans la conversation.
    pub async fn create_call_message(&self, cid: Id, author: Id) -> Result<Message> {
        let id: Id = sqlx::query_scalar(
            "with m as (insert into messages (channel_id, author_id, kind, call_participants)
                        values ($1, $2, 'call', array[$2]) returning id, channel_id)
             update channels c set last_message_id = greatest(c.last_message_id, m.id) from m
             where c.id = m.channel_id returning m.id",
        )
        .bind(cid)
        .bind(author)
        .fetch_one(&self.pool)
        .await?;
        self.message(id).await?.ok_or(DbError::NotFound)
    }

    /// Le message d'un appel qui n'a pas pu commencer.
    pub async fn discard_call_message(&self, mid: Id) -> Result<()> {
        sqlx::query("delete from messages where id = $1 and kind = 'call'").bind(mid).execute(&self.pool).await?;
        Ok(())
    }

    /// L'appel a pris fin : qui y est entré, et maintenant. `None` s'il était déjà terminé.
    pub async fn end_call_message(&self, mid: Id, participants: &[Id]) -> Result<Option<Message>> {
        let done = sqlx::query(
            "update messages set call_ended_at = now(), call_participants = $2
             where id = $1 and kind = 'call' and call_ended_at is null",
        )
        .bind(mid)
        .bind(participants)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if done == 0 {
            return Ok(None);
        }
        let r = sqlx::query(&message_sql("where m.id = $1")).bind(mid).fetch_optional(&self.pool).await?;
        Ok(r.as_ref().map(crate::message_from).transpose()?)
    }

    /// Au démarrage : les appels restés ouverts (serveur arrêté pendant l'appel) sont terminés.
    pub async fn end_open_calls(&self) -> Result<u64> {
        Ok(sqlx::query("update messages set call_ended_at = now() where kind = 'call' and call_ended_at is null")
            .execute(&self.pool)
            .await?
            .rows_affected())
    }
}
