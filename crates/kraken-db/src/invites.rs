//! Invitations : créer, aperçu, rejoindre, lister celles d'un serveur et les révoquer. Une
//! invitation révoquée est supprimée (le journal d'audit garde la trace) ; une invitation périmée
//! ou épuisée reste en base quelques jours pour apparaître, grisée, dans la liste du serveur.

use kraken_core::perms;
use kraken_proto::*;
use rand::Rng;
use serde_json::json;
use sqlx::Row;

use crate::moderation::audit_changes;
use crate::perm::Ctx;
use crate::{Db, DbError, Result, user_cols, user_from};

impl Db {
    /// Crée une invitation. `max_age_s` : `None` (ou 0) pour qu'elle n'expire jamais, sinon
    /// 7 jours au plus ; `max_uses` : 0 pour autant d'usages que l'on veut, sinon 1000 au plus.
    /// Au plus [`MAX_INVITES_PER_MEMBER`] invitations actives par personne et par serveur
    /// (`Invalid("invite_limit")` au-delà). Les invitations expirées ou épuisées depuis plus de
    /// [`INVITE_KEEP_DAYS`] jours partent.
    pub async fn create_invite(
        &self,
        gid: Id,
        cid: Option<Id>,
        inviter: Id,
        max_uses: u32,
        max_age_s: Option<u32>,
    ) -> Result<Invite> {
        if max_age_s.is_some_and(|a| a > MAX_INVITE_AGE_S) {
            return Err(DbError::Invalid("invite_age"));
        }
        if max_uses > MAX_INVITE_USES {
            return Err(DbError::Invalid("invite_uses"));
        }
        let code: String = rand::rng().sample_iter(rand::distr::Alphanumeric).take(8).map(char::from).collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "delete from invites where guild_id = $1 and (
               (expires_at is not null and expires_at < now() - make_interval(days => $2))
               or (max_uses > 0 and uses >= max_uses and updated_at < now() - make_interval(days => $2)))",
        )
        .bind(gid)
        .bind(INVITE_KEEP_DAYS)
        .execute(&mut *tx)
        .await?;
        // Plafond : MAX_INVITES_PER_MEMBER invitations actives par personne et par serveur. Le
        // verrou (levé à la fin de la transaction) fait passer deux demandes simultanées l'une
        // après l'autre, sinon elles compteraient toutes deux avant que l'autre n'écrive.
        sqlx::query("select pg_advisory_xact_lock(hashtextextended('invites:' || $1::text || ':' || $2::text, 0))")
            .bind(gid)
            .bind(inviter)
            .execute(&mut *tx)
            .await?;
        let active: i64 = sqlx::query_scalar(
            "select count(*) from invites where guild_id = $1 and inviter_id = $2
               and (expires_at is null or expires_at > now()) and (max_uses = 0 or uses < max_uses)",
        )
        .bind(gid)
        .bind(inviter)
        .fetch_one(&mut *tx)
        .await?;
        if active >= MAX_INVITES_PER_MEMBER as i64 {
            return Err(DbError::Invalid("invite_limit"));
        }
        sqlx::query(
            "insert into invites (code, guild_id, channel_id, inviter_id, max_uses, expires_at)
             values ($1, $2, $3, $4, $5, case when $6::int > 0 then now() + make_interval(secs => $6) end)",
        )
        .bind(&code)
        .bind(gid)
        .bind(cid)
        .bind(inviter)
        .bind(max_uses as i32)
        .bind(max_age_s.map(|a| a as i32))
        .execute(&mut *tx)
        .await?;
        let channel: Option<String> = match cid {
            Some(cid) => sqlx::query_scalar("select name from channels where id = $1")
                .bind(cid)
                .fetch_optional(&mut *tx)
                .await?
                .flatten(),
            None => None,
        };
        audit_changes(
            &mut tx,
            gid,
            inviter,
            "invite_create",
            None,
            json!({ "code": code, "max_uses": max_uses, "max_age_s": max_age_s, "channel": channel }),
        )
        .await?;
        tx.commit().await?;
        self.invite(&code).await?.ok_or(DbError::NotFound)
    }

    /// L'invitation sans fin ni limite que `inviter` a déjà dans ce salon, la plus récente.
    pub async fn reusable_invite(&self, gid: Id, cid: Id, inviter: Id) -> Result<Option<Invite>> {
        let code: Option<String> = sqlx::query_scalar(
            "select code from invites where guild_id = $1 and channel_id = $2 and inviter_id = $3
               and expires_at is null and max_uses = 0 order by created_at desc limit 1",
        )
        .bind(gid)
        .bind(cid)
        .bind(inviter)
        .fetch_optional(&self.pool)
        .await?;
        match code {
            Some(c) => self.invite(&c).await,
            None => Ok(None),
        }
    }

    /// Aperçu ; `None` si inconnue, expirée ou épuisée.
    pub async fn invite(&self, code: &str) -> Result<Option<Invite>> {
        let r = sqlx::query(
            "select i.code, i.channel_id, i.uses, i.max_uses, (extract(epoch from i.expires_at) * 1000)::int8 as exp_ms,
                    g.id, g.name, g.icon_key, g.owner_id,
                    (select count(*) from members m where m.guild_id = g.id)::int4 as member_count
             from invites i join guilds g on g.id = i.guild_id
             where i.code = $1 and (i.expires_at is null or i.expires_at > now()) and (i.max_uses = 0 or i.uses < i.max_uses)",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = r else { return Ok(None) };
        Ok(Some(Invite {
            code: r.try_get("code")?,
            guild: Guild {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                icon: r.try_get("icon_key")?,
                owner_id: r.try_get("owner_id")?,
            },
            channel_id: r.try_get("channel_id")?,
            member_count: r.try_get::<i32, _>("member_count")? as u32,
            uses: r.try_get::<i32, _>("uses")? as u32,
            max_uses: r.try_get::<i32, _>("max_uses")? as u32,
            expires_at: r.try_get("exp_ms")?,
        }))
    }

    /// Comme [`Self::invite`], mais dit pourquoi elle ne sert plus : `NotFound` si elle n'existe
    /// pas (ou a été révoquée), `Gone("expired")` ou `Gone("exhausted")` sinon.
    pub async fn invite_checked(&self, code: &str) -> Result<Invite> {
        if let Some(i) = self.invite(code).await? {
            return Ok(i);
        }
        Err(self.why_dead(code).await?)
    }

    async fn why_dead(&self, code: &str) -> Result<DbError> {
        let expired: Option<bool> =
            sqlx::query_scalar("select expires_at is not null and expires_at <= now() from invites where code = $1")
                .bind(code)
                .fetch_optional(&self.pool)
                .await?;
        Ok(match expired {
            None => DbError::NotFound,
            Some(true) => DbError::Gone("expired"),
            Some(false) => DbError::Gone("exhausted"),
        })
    }

    /// Rejoint le serveur. Le compteur d'usages ne bouge pas pour un membre déjà présent ;
    /// l'incrément est conditionnel, donc deux adhésions simultanées ne dépassent pas
    /// `max_uses`. Rend l'état du serveur et le nouveau membre (`None` s'il l'était déjà).
    /// Une invitation inconnue ou révoquée : `NotFound` ; périmée ou épuisée : `Gone`.
    pub async fn use_invite(&self, code: &str, uid: Id) -> Result<(GuildState, Option<Member>)> {
        let mut tx = self.pool.begin().await?;
        let gid: Id = sqlx::query_scalar("select guild_id from invites where code = $1")
            .bind(code)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        let banned: bool = sqlx::query_scalar("select exists(select 1 from bans where guild_id = $1 and user_id = $2)")
            .bind(gid)
            .bind(uid)
            .fetch_one(&mut *tx)
            .await?;
        if banned {
            return Err(DbError::Forbidden("banned"));
        }
        // Adhésion d'abord : une seconde en parallèle attend la première, puis ne fait rien.
        let already = sqlx::query("insert into members (guild_id, user_id) values ($1, $2) on conflict do nothing")
            .bind(gid)
            .bind(uid)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            == 0;
        if !already {
            let counted = sqlx::query(
                "update invites set uses = uses + 1, updated_at = now() where code = $1
                   and (max_uses = 0 or uses < max_uses) and (expires_at is null or expires_at > now())
                 returning guild_id",
            )
            .bind(code)
            .fetch_optional(&mut *tx)
            .await?;
            if counted.is_none() {
                // Rien n'est écrit : la transaction est abandonnée, l'adhésion avec elle.
                drop(tx);
                return Err(self.why_dead(code).await?);
            }
            // Parti pendant une exclusion : on la retrouve, à la même échéance (ou plus rien
            // si elle a fini entre-temps). La ligne gardée est consommée dans tous les cas.
            sqlx::query(
                "update members m set timeout_until = t.until, updated_at = now()
                 from member_timeouts t
                 where m.guild_id = $1 and m.user_id = $2 and t.guild_id = m.guild_id and t.user_id = m.user_id
                   and t.until > now()",
            )
            .bind(gid)
            .bind(uid)
            .execute(&mut *tx)
            .await?;
            sqlx::query("delete from member_timeouts where guild_id = $1 and user_id = $2")
                .bind(gid)
                .bind(uid)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        let state = self.guild_state(gid, uid).await?.ok_or(DbError::NotFound)?;
        let new = if already { None } else { state.members.iter().find(|m| m.user.id == uid).cloned() };
        Ok((state, new))
    }

    /// Les invitations du serveur (au plus [`MAX_INVITE_ROWS`]) : celles qui servent encore, puis
    /// celles qui ont cessé de servir depuis moins de [`INVITE_KEEP_DAYS`] jours. Avec « Gérer le
    /// serveur », toutes ; avec seulement « Créer une invitation », les siennes. Sinon 403 ; 404
    /// hors du serveur. Un membre exclu temporairement n'a plus aucun de ces droits.
    pub async fn guild_invites(&self, gid: Id, by: Id) -> Result<Vec<GuildInvite>> {
        let ctx = Ctx::load(&self.pool, gid, Some(by)).await?.filter(|c| c.is_member(by)).ok_or(DbError::NotFound)?;
        let base = ctx.base(by);
        let all = base & perms::MANAGE_GUILD != 0;
        if !all && base & perms::CREATE_INVITE == 0 {
            return Err(DbError::Forbidden("perm"));
        }
        let rows = sqlx::query(&format!(
            "select i.code, i.channel_id, c.name as channel_name, i.uses, i.max_uses,
                    (extract(epoch from i.created_at) * 1000)::int8 as created_ms,
                    (extract(epoch from i.expires_at) * 1000)::int8 as exp_ms,
                    case when i.expires_at is not null and i.expires_at <= now() then 'expired'
                         when i.max_uses > 0 and i.uses >= i.max_uses then 'exhausted'
                         else 'active' end as state,
                    {}
             from invites i
             left join channels c on c.id = i.channel_id
             left join users u on u.id = i.inviter_id
             where i.guild_id = $1 and ($2 or i.inviter_id = $3)
               and (  (i.expires_at is null or i.expires_at > now()) and (i.max_uses = 0 or i.uses < i.max_uses)
                   or (i.expires_at is not null and i.expires_at <= now()
                       and i.expires_at > now() - make_interval(days => $4))
                   or ((i.expires_at is null or i.expires_at > now()) and i.max_uses > 0 and i.uses >= i.max_uses
                       and i.updated_at > now() - make_interval(days => $4)))
             order by (case when i.expires_at is not null and i.expires_at <= now() then 1
                            when i.max_uses > 0 and i.uses >= i.max_uses then 1 else 0 end),
                      i.created_at desc, i.code
             limit $5",
            user_cols("u")
        ))
        .bind(gid)
        .bind(all)
        .bind(by)
        .bind(INVITE_KEEP_DAYS)
        .bind(MAX_INVITE_ROWS as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(GuildInvite {
                    code: r.try_get("code")?,
                    channel_id: r.try_get("channel_id")?,
                    channel_name: r.try_get("channel_name")?,
                    inviter: user_from(r, "u")?,
                    uses: r.try_get::<i32, _>("uses")? as u32,
                    max_uses: r.try_get::<i32, _>("max_uses")? as u32,
                    created_at: r.try_get("created_ms")?,
                    expires_at: r.try_get("exp_ms")?,
                    state: match r.try_get::<&str, _>("state")? {
                        "expired" => InviteState::Expired,
                        "exhausted" => InviteState::Exhausted,
                        _ => InviteState::Active,
                    },
                })
            })
            .collect::<sqlx::Result<_>>()
            .map_err(Into::into)
    }

    /// Révoque une invitation : « Gérer le serveur », ou celui qui l'a créée (et qui est
    /// encore dans le serveur). Inconnue, ou d'un serveur où l'on n'est pas : 404.
    pub async fn revoke_invite(&self, code: &str, by: Id) -> Result<Id> {
        let mut tx = self.pool.begin().await?;
        let row: Option<(Id, Option<Id>)> =
            sqlx::query_as("select guild_id, inviter_id from invites where code = $1 for update")
                .bind(code)
                .fetch_optional(&mut *tx)
                .await?;
        let (gid, inviter) = row.ok_or(DbError::NotFound)?;
        let ctx = Ctx::load_on(&mut tx, gid, Some(by)).await?.filter(|c| c.is_member(by)).ok_or(DbError::NotFound)?;
        if inviter != Some(by) && ctx.base(by) & perms::MANAGE_GUILD == 0 {
            return Err(DbError::Forbidden("perm"));
        }
        sqlx::query("delete from invites where code = $1").bind(code).execute(&mut *tx).await?;
        audit_changes(&mut tx, gid, by, "invite_revoke", None, json!({ "code": code, "inviter": inviter })).await?;
        tx.commit().await?;
        Ok(gid)
    }
}
