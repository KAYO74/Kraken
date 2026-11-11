//! Profil (bio, pronoms), compte (pseudo, e-mail, sessions) et statut choisi. Le reste du profil
//! (nom affiché, photo, bannière) est dans `lib.rs`, avec les autres accès aux comptes.

use kraken_proto::*;
use sqlx::Row;

use crate::{Db, DbError, Result, user_cols, user_from, valid_username};

/// Champs du profil à changer : `None` laisse tel quel, `Some(None)` efface.
#[derive(Default, Clone, Copy)]
pub struct ProfilePatch<'a> {
    pub display_name: Option<Option<&'a str>>,
    pub bio: Option<Option<&'a str>>,
    pub pronouns: Option<Option<&'a str>>,
}

impl Db {
    /// Change d'un coup les champs du profil donnés ; rend l'utilisateur à jour.
    pub async fn set_profile(&self, id: Id, patch: ProfilePatch<'_>) -> Result<User> {
        let r = sqlx::query(&format!(
            "update users as u set
               display_name = case when $2 then $3 else u.display_name end,
               bio = case when $4 then $5 else u.bio end,
               pronouns = case when $6 then $7 else u.pronouns end
             where u.id = $1 and u.deleted_at is null returning {}",
            user_cols("u")
        ))
        .bind(id)
        .bind(patch.display_name.is_some())
        .bind(patch.display_name.flatten())
        .bind(patch.bio.is_some())
        .bind(patch.bio.flatten())
        .bind(patch.pronouns.is_some())
        .bind(patch.pronouns.flatten())
        .fetch_optional(&self.pool)
        .await?;
        match r {
            Some(r) => user_from(&r, "u")?.ok_or(DbError::NotFound),
            None => Err(DbError::NotFound),
        }
    }

    // ───────── compte ─────────

    /// Mon adresse, si elle est confirmée, et celle qui attend sa confirmation. `mail` est
    /// laissé à faux : c'est le serveur qui sait s'il envoie des e-mails.
    pub async fn account(&self, uid: Id) -> Result<Account> {
        let r = sqlx::query(
            "select email, email_verified_at is not null as verified, pending_email from users
             where id = $1 and deleted_at is null",
        )
        .bind(uid)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        Ok(Account {
            email: r.try_get("email")?,
            email_verified: r.try_get("verified")?,
            pending_email: r.try_get("pending_email")?,
            mail: false,
        })
    }

    /// Change le pseudo, au plus [`USERNAME_CHANGES_PER_WINDOW`] fois par [`USERNAME_WINDOW_S`]
    /// secondes. L'ancien pseudo est libre aussitôt. `Cooldown(n)` : encore `n` secondes à attendre.
    pub async fn change_username(&self, uid: Id, username: &str) -> Result<User> {
        if !valid_username(username) {
            return Err(DbError::Invalid("username"));
        }
        let mut tx = self.pool.begin().await?;
        // La ligne verrouillée d'abord : deux changements simultanés se suivent au lieu de se croiser.
        sqlx::query_scalar::<_, Id>("select id from users where id = $1 and deleted_at is null for update")
            .bind(uid)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        let recent: Vec<f64> = sqlx::query_scalar(
            "select extract(epoch from changed_at + make_interval(secs => $2) - now())::float8 from username_changes
             where user_id = $1 and changed_at > now() - make_interval(secs => $2) order by changed_at",
        )
        .bind(uid)
        .bind(USERNAME_WINDOW_S as i32)
        .fetch_all(&mut *tx)
        .await?;
        let allowed = USERNAME_CHANGES_PER_WINDOW as usize;
        if recent.len() >= allowed {
            // La place qui se libère en premier est celle du plus ancien changement qui compte.
            let wait = recent[recent.len() - allowed];
            return Err(DbError::Cooldown(wait.ceil().max(1.0) as u32));
        }
        let r =
            sqlx::query(&format!("update users as u set username = $2 where u.id = $1 returning {}", user_cols("u")))
                .bind(uid)
                .bind(username)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("insert into username_changes (user_id) values ($1)").bind(uid).execute(&mut *tx).await?;
        sqlx::query("delete from username_changes where user_id = $1 and changed_at < now() - interval '1 day'")
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        user_from(&r, "u")?.ok_or(DbError::NotFound)
    }

    /// Une autre personne a déjà cette adresse (ou l'attend, confirmée) ?
    async fn email_taken(&self, uid: Id, email: &str) -> Result<bool> {
        Ok(sqlx::query_scalar("select exists(select 1 from users where lower(email) = lower($1) and id <> $2)")
            .bind(email)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Instance sans e-mail : la nouvelle adresse remplace l'ancienne tout de suite (non confirmée,
    /// comme à l'inscription).
    pub async fn set_email_now(&self, uid: Id, email: &str) -> Result<()> {
        let done = sqlx::query(
            "update users set email = $2, email_verified_at = null, pending_email = null
             where id = $1 and deleted_at is null",
        )
        .bind(uid)
        .bind(email.trim())
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 { Err(DbError::NotFound) } else { Ok(()) }
    }

    /// Instance avec e-mail : la nouvelle adresse attend sa confirmation, l'ancienne reste valable.
    pub async fn set_pending_email(&self, uid: Id, email: &str) -> Result<()> {
        let email = email.trim();
        if self.email_taken(uid, email).await? {
            return Err(DbError::Conflict("email"));
        }
        let done = sqlx::query("update users set pending_email = $2 where id = $1 and deleted_at is null")
            .bind(uid)
            .bind(email)
            .execute(&self.pool)
            .await?;
        if done.rows_affected() == 0 { Err(DbError::NotFound) } else { Ok(()) }
    }

    /// L'adresse en attente de confirmation, si on lui doit encore un e-mail.
    pub async fn pending_email(&self, uid: Id) -> Result<Option<(String, String)>> {
        let r = sqlx::query(
            "select pending_email, username from users
             where id = $1 and pending_email is not null and deleted_at is null and deletion_requested_at is null",
        )
        .bind(uid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(match r {
            Some(r) => Some((r.try_get("pending_email")?, r.try_get("username")?)),
            None => None,
        })
    }

    /// Jeton qui confirme `email` pour ce compte ; les jetons de changement encore valables
    /// d'une demande précédente ne servent plus.
    pub async fn create_email_change_token(&self, uid: Id, email: &str, hash: &[u8], ttl_minutes: i32) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("delete from auth_tokens where user_id = $1 and kind = 'change_email' and used_at is null")
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "insert into auth_tokens (user_id, kind, token_hash, expires_at, email)
             values ($1, 'change_email', $2, now() + make_interval(mins => $3), $4)",
        )
        .bind(uid)
        .bind(hash)
        .bind(ttl_minutes)
        .bind(email)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Consomme un jeton de changement d'adresse : la nouvelle adresse remplace l'ancienne, confirmée.
    /// `None` : jeton inconnu, expiré, déjà servi, ou adresse remplacée depuis par une autre demande.
    /// `Conflict("email")` : quelqu'un d'autre a pris l'adresse entre-temps.
    pub async fn confirm_email_change(&self, hash: &[u8]) -> Result<Option<Id>> {
        let mut tx = self.pool.begin().await?;
        let t = sqlx::query(
            "update auth_tokens set used_at = now()
             where kind = 'change_email' and token_hash = $1 and used_at is null and expires_at > now() and email is not null
             returning user_id, email",
        )
        .bind(hash)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(t) = t else { return Ok(None) };
        let (uid, email): (Id, String) = (t.try_get("user_id")?, t.try_get("email")?);
        let done = sqlx::query(
            "update users set email = pending_email, email_verified_at = now(), pending_email = null
             where id = $1 and lower(pending_email) = lower($2) and deleted_at is null and deletion_requested_at is null",
        )
        .bind(uid)
        .bind(&email)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(None);
        }
        tx.commit().await?;
        Ok(Some(uid))
    }

    /// Ferme toutes les sessions sauf celle-ci ; rend leur nombre.
    pub async fn revoke_other_sessions(&self, uid: Id, keep: &[u8]) -> Result<u64> {
        let r = sqlx::query(
            "update sessions set revoked_at = now() where user_id = $1 and token_hash <> $2 and revoked_at is null",
        )
        .bind(uid)
        .bind(keep)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected())
    }

    // ───────── présence ─────────

    /// Le statut que cette personne a choisi.
    pub async fn presence(&self, uid: Id) -> Result<Presence> {
        let s: Option<String> =
            sqlx::query_scalar("select presence from users where id = $1").bind(uid).fetch_optional(&self.pool).await?;
        Ok(s.as_deref().and_then(Presence::parse).unwrap_or_default())
    }

    pub async fn set_presence(&self, uid: Id, presence: Presence) -> Result<()> {
        let done = sqlx::query("update users set presence = $2 where id = $1 and deleted_at is null")
            .bind(uid)
            .bind(presence.as_str())
            .execute(&self.pool)
            .await?;
        if done.rows_affected() == 0 { Err(DbError::NotFound) } else { Ok(()) }
    }

    /// Ceux qui voient le statut de cette personne, et dont elle voit le statut : membres de ses
    /// serveurs, participants de ses MP et groupes, amis ; elle-même en fait partie. Un blocage,
    /// dans un sens ou dans l'autre, coupe la vue.
    pub async fn presence_peers(&self, uid: Id) -> Result<Vec<Id>> {
        let mut peers: Vec<Id> = sqlx::query_scalar(
            "select p from (
               select m2.user_id as p from members m1 join members m2 on m2.guild_id = m1.guild_id where m1.user_id = $1
               union select r2.user_id from channel_recipients r1
                 join channel_recipients r2 on r2.channel_id = r1.channel_id where r1.user_id = $1
               union select target_id from relationships where user_id = $1 and kind = 'friend'
             ) peers
             where p <> $1 and not exists (
               select 1 from relationships b where b.kind = 'blocked'
                 and ((b.user_id = $1 and b.target_id = peers.p) or (b.user_id = peers.p and b.target_id = $1)))
               and exists (select 1 from users u where u.id = peers.p and u.deleted_at is null and u.deletion_requested_at is null)",
        )
        .bind(uid)
        .fetch_all(&self.pool)
        .await?;
        peers.push(uid);
        peers.sort_unstable();
        peers.dedup();
        Ok(peers)
    }
}
