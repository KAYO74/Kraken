//! Rôles, exclusions, bannissements, départ et transfert de propriété. Chaque opération vérifie
//! ici les droits de celui qui agit et la hiérarchie des rôles (jamais sur un rang égal ou
//! supérieur, jamais sur le propriétaire), dans une transaction qui verrouille le serveur : deux
//! actions de modération simultanées passent l'une après l'autre.

use kraken_core::perms;
use kraken_proto::*;
use sqlx::{Postgres, Row, Transaction};

use crate::{Db, DbError, ROLE_COLS, Result, role_from, user_cols, user_from};

pub(crate) type Tx<'a> = Transaction<'a, Postgres>;

/// Où en est quelqu'un dans un serveur : propriétaire ou non, droits, rang.
pub(crate) struct Standing {
    pub is_owner: bool,
    pub base: u64,
    pub top: i32,
}

impl Standing {
    pub(crate) fn need(&self, bit: u64) -> Result<()> {
        if self.base & bit == bit { Ok(()) } else { Err(DbError::Forbidden("perm")) }
    }

    /// Peut-il agir sur ce rôle (le modifier, le donner, le retirer) ?
    pub(crate) fn over_role(&self, role_position: i32) -> Result<()> {
        if self.is_owner || self.top > role_position { Ok(()) } else { Err(DbError::Forbidden("hierarchy")) }
    }

    pub(crate) fn over(&self, target: &Standing) -> Result<()> {
        if perms::outranks(self.is_owner, self.top, target.is_owner, target.top) {
            Ok(())
        } else {
            Err(DbError::Forbidden("hierarchy"))
        }
    }
}

/// Verrouille le serveur et lit qui est son propriétaire ; `NotFound` s'il n'existe pas.
pub(crate) async fn lock(tx: &mut Tx<'_>, gid: Id) -> Result<Id> {
    sqlx::query_scalar("select owner_id from guilds where id = $1 for update")
        .bind(gid)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DbError::NotFound)
}

/// `None` si ce n'est pas un membre.
pub(crate) async fn standing(tx: &mut Tx<'_>, gid: Id, owner: Id, uid: Id) -> Result<Option<Standing>> {
    let member: bool = sqlx::query_scalar("select exists(select 1 from members where guild_id = $1 and user_id = $2)")
        .bind(gid)
        .bind(uid)
        .fetch_one(&mut **tx)
        .await?;
    if !member {
        return Ok(None);
    }
    let rows = sqlx::query(
        "select r.id, r.position, r.permissions from roles r where r.guild_id = $1
           and (r.id = $1 or r.id in (select role_id from member_roles where guild_id = $1 and user_id = $2))",
    )
    .bind(gid)
    .bind(uid)
    .fetch_all(&mut **tx)
    .await?;
    let (mut everyone, mut held, mut positions) = (0u64, Vec::new(), Vec::new());
    for r in &rows {
        let p = r.try_get::<i64, _>("permissions")? as u64;
        if r.try_get::<Id, _>("id")? == gid {
            everyone = p;
        } else {
            held.push(p);
            positions.push(r.try_get::<i32, _>("position")?);
        }
    }
    let is_owner = owner == uid;
    Ok(Some(Standing {
        is_owner,
        base: perms::base(is_owner, everyone, held),
        top: perms::top_position(is_owner, positions),
    }))
}

/// Celui qui agit : membre du serveur (sinon `NotFound`, on ne révèle rien) qui a ce droit.
pub(crate) async fn actor(tx: &mut Tx<'_>, gid: Id, owner: Id, uid: Id, need: u64) -> Result<Standing> {
    let s = standing(tx, gid, owner, uid).await?.ok_or(DbError::NotFound)?;
    s.need(need)?;
    Ok(s)
}

pub(crate) async fn audit(
    tx: &mut Tx<'_>,
    gid: Id,
    actor: Id,
    action: &str,
    target: Option<Id>,
    reason: Option<&str>,
) -> Result<()> {
    sqlx::query("insert into audit_log (guild_id, actor_id, action, target_id, reason) values ($1, $2, $3, $4, $5)")
        .bind(gid)
        .bind(actor)
        .bind(action)
        .bind(target)
        .bind(reason)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Une ligne du journal avec le détail de ce qui a changé (nom du salon, par exemple).
pub(crate) async fn audit_changes(
    tx: &mut Tx<'_>,
    gid: Id,
    actor: Id,
    action: &str,
    target: Option<Id>,
    changes: serde_json::Value,
) -> Result<()> {
    sqlx::query("insert into audit_log (guild_id, actor_id, action, target_id, changes) values ($1, $2, $3, $4, $5)")
        .bind(gid)
        .bind(actor)
        .bind(action)
        .bind(target)
        .bind(sqlx::types::Json(changes))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn clean_reason(r: Option<&str>) -> Result<Option<String>> {
    let r = r.map(str::trim).filter(|r| !r.is_empty());
    if r.is_some_and(|r| r.chars().count() > MAX_REASON) {
        return Err(DbError::Invalid("reason"));
    }
    Ok(r.map(str::to_string))
}

fn clean_role_name(n: &str) -> Result<&str> {
    let n = n.trim();
    if (1..=100).contains(&n.chars().count()) { Ok(n) } else { Err(DbError::Invalid("role_name")) }
}

fn clean_color(c: u32) -> Result<Option<i32>> {
    match c {
        0 => Ok(None),
        1..=ROLE_COLORS => Ok(Some(c as i32)),
        _ => Err(DbError::Invalid("color")),
    }
}

/// Droits que l'on peut accorder : valides, et que l'on a soi-même.
pub(crate) fn grantable(who: &Standing, changed: u64) -> Result<()> {
    if changed & !perms::ALL != 0 {
        return Err(DbError::Invalid("permissions"));
    }
    if changed & !who.base != 0 {
        return Err(DbError::Forbidden("perm"));
    }
    Ok(())
}

async fn role_in(tx: &mut Tx<'_>, gid: Id, rid: Id) -> Result<Role> {
    let r = sqlx::query(&format!("select {ROLE_COLS} from roles where id = $1 and guild_id = $2"))
        .bind(rid)
        .bind(gid)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(role_from(&r)?)
}

/// Une exclusion ou un bannissement et ce qu'il a effacé.
pub struct BanOutcome {
    pub ban: Ban,
    pub was_member: bool,
    /// Messages effacés (salon, message).
    pub purged: Vec<(Id, Id)>,
    /// Fichiers de ces messages, à supprimer du disque.
    pub files: Vec<String>,
}

impl Db {
    // ───────── rôles ─────────

    /// Nouveau rôle, juste au-dessus de `@everyone` ; il ne peut pas avoir plus de droits que
    /// son créateur.
    pub async fn create_role(&self, gid: Id, by: Id, req: &CreateRole) -> Result<Role> {
        let name = clean_role_name(&req.name)?;
        let color = req.color.map_or(Ok(None), clean_color)?;
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = actor(&mut tx, gid, owner, by, perms::MANAGE_ROLES).await?;
        grantable(&me, req.permissions)?;
        let count: i64 = sqlx::query_scalar("select count(*) from roles where guild_id = $1 and id <> $1")
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?;
        if count as usize >= MAX_ROLES {
            return Err(DbError::Invalid("roles"));
        }
        sqlx::query("update roles set position = position + 1 where guild_id = $1 and id <> $1")
            .bind(gid)
            .execute(&mut *tx)
            .await?;
        let rid: Id = sqlx::query_scalar(
            "insert into roles (guild_id, name, color, position, permissions, hoist) values ($1, $2, $3, 1, $4, $5) returning id",
        )
        .bind(gid)
        .bind(name)
        .bind(color)
        .bind(req.permissions as i64)
        .bind(req.hoist)
        .fetch_one(&mut *tx)
        .await?;
        audit(&mut tx, gid, by, "role_create", Some(rid), None).await?;
        let role = role_in(&mut tx, gid, rid).await?;
        tx.commit().await?;
        Ok(role)
    }

    /// Modifie un rôle plus bas que soi. `@everyone` : ses droits seulement.
    pub async fn edit_role(&self, gid: Id, rid: Id, by: Id, req: &EditRole) -> Result<Role> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = actor(&mut tx, gid, owner, by, perms::MANAGE_ROLES).await?;
        let role = role_in(&mut tx, gid, rid).await?;
        let everyone = rid == gid;
        if everyone {
            if req.name.is_some() || req.color.is_some() || req.hoist.is_some() {
                return Err(DbError::Invalid("everyone"));
            }
        } else {
            me.over_role(role.position)?;
        }
        let name = req.name.as_deref().map(clean_role_name).transpose()?.unwrap_or(&role.name).to_string();
        let color = match req.color {
            Some(c) => clean_color(c)?,
            None => role.color.map(|c| c as i32),
        };
        let permissions = req.permissions.unwrap_or(role.permissions);
        grantable(&me, permissions ^ role.permissions)?;
        sqlx::query(
            "update roles set name = $3, color = $4, hoist = $5, permissions = $6, updated_at = now()
             where id = $1 and guild_id = $2",
        )
        .bind(rid)
        .bind(gid)
        .bind(&name)
        .bind(color)
        .bind(req.hoist.unwrap_or(role.hoist))
        .bind(permissions as i64)
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, gid, by, "role_update", Some(rid), None).await?;
        let role = role_in(&mut tx, gid, rid).await?;
        tx.commit().await?;
        Ok(role)
    }

    /// Supprime un rôle plus bas que soi ; ceux qui l'avaient le perdent, les rangs se resserrent.
    pub async fn delete_role(&self, gid: Id, rid: Id, by: Id) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = actor(&mut tx, gid, owner, by, perms::MANAGE_ROLES).await?;
        if rid == gid {
            return Err(DbError::Invalid("everyone"));
        }
        let role = role_in(&mut tx, gid, rid).await?;
        me.over_role(role.position)?;
        sqlx::query("delete from roles where id = $1").bind(rid).execute(&mut *tx).await?;
        sqlx::query("update roles set position = position - 1 where guild_id = $1 and id <> $1 and position > $2")
            .bind(gid)
            .bind(role.position)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, gid, by, "role_delete", Some(rid), None).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Nouvel ordre des rôles (sans `@everyone`), du plus haut au plus bas. Sauf pour le
    /// propriétaire, les rôles au niveau du sien ou au-dessus ne bougent pas. Rend les rôles
    /// dont la position a changé.
    pub async fn reorder_roles(&self, gid: Id, by: Id, ids: &[Id]) -> Result<Vec<Role>> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = actor(&mut tx, gid, owner, by, perms::MANAGE_ROLES).await?;
        let mut current: Vec<Role> = sqlx::query(&format!(
            "select {ROLE_COLS} from roles where guild_id = $1 and id <> $1 order by position desc, id"
        ))
        .bind(gid)
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(role_from)
        .collect::<sqlx::Result<_>>()?;
        let mut sorted: Vec<Id> = ids.to_vec();
        let mut known: Vec<Id> = current.iter().map(|r| r.id).collect();
        sorted.sort();
        known.sort();
        if sorted != known {
            return Err(DbError::Invalid("roles"));
        }
        for (i, r) in current.iter().enumerate() {
            if ids[i] != r.id {
                me.over_role(r.position)?;
                let moved = current.iter().find(|x| x.id == ids[i]).expect("permutation");
                me.over_role(moved.position)?;
            }
        }
        let n = current.len();
        let mut changed = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let position = (n - i) as i32;
            let r = current.iter_mut().find(|r| r.id == *id).expect("permutation");
            if r.position != position {
                sqlx::query("update roles set position = $2, updated_at = now() where id = $1")
                    .bind(id)
                    .bind(position)
                    .execute(&mut *tx)
                    .await?;
                r.position = position;
                changed.push(r.clone());
            }
        }
        if !changed.is_empty() {
            audit(&mut tx, gid, by, "role_reorder", None, None).await?;
        }
        tx.commit().await?;
        Ok(changed)
    }

    /// Donne (`on`) ou retire un rôle à un membre. Le rôle doit être plus bas que soi, et le
    /// membre aussi (sauf pour le propriétaire). Rend le membre à jour.
    pub async fn set_member_role(&self, gid: Id, by: Id, uid: Id, rid: Id, on: bool) -> Result<Member> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = actor(&mut tx, gid, owner, by, perms::MANAGE_ROLES).await?;
        let target = standing(&mut tx, gid, owner, uid).await?.ok_or(DbError::NotFound)?;
        if rid == gid {
            return Err(DbError::Invalid("everyone"));
        }
        let role = role_in(&mut tx, gid, rid).await?;
        me.over_role(role.position)?;
        me.over(&target)?;
        let done = if on {
            sqlx::query(
                "insert into member_roles (guild_id, user_id, role_id) values ($1, $2, $3) on conflict do nothing",
            )
        } else {
            sqlx::query("delete from member_roles where guild_id = $1 and user_id = $2 and role_id = $3")
        }
        .bind(gid)
        .bind(uid)
        .bind(rid)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if done > 0 {
            audit(&mut tx, gid, by, if on { "member_role_add" } else { "member_role_remove" }, Some(uid), None).await?;
        }
        tx.commit().await?;
        self.members_of(gid, Some(uid)).await?.pop().ok_or(DbError::NotFound)
    }

    /// Les membres qui ont ce droit (à qui adresser un événement réservé).
    pub async fn members_with(&self, gid: Id, bit: u64) -> Result<Vec<Id>> {
        let Some(ctx) = crate::perm::Ctx::load(&self.pool, gid, None).await? else { return Ok(vec![]) };
        Ok(ctx.member_ids().filter(|u| ctx.base(*u) & bit == bit).collect())
    }

    // ───────── exclusion, bannissement, départ ─────────

    pub async fn kick_member(&self, gid: Id, by: Id, uid: Id, reason: Option<&str>) -> Result<()> {
        let reason = clean_reason(reason)?;
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = actor(&mut tx, gid, owner, by, perms::KICK).await?;
        if by == uid {
            return Err(DbError::Invalid("self_target"));
        }
        let target = standing(&mut tx, gid, owner, uid).await?.ok_or(DbError::NotFound)?;
        me.over(&target)?;
        sqlx::query("delete from members where guild_id = $1 and user_id = $2")
            .bind(gid)
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, gid, by, "member_kick", Some(uid), reason.as_deref()).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Bannit un membre, ou quelqu'un qui est déjà parti. Il ne peut plus rejoindre par
    /// invitation. `delete_hours` : efface aussi ses messages des dernières heures.
    pub async fn ban_member(&self, gid: Id, by: Id, uid: Id, req: &BanMember) -> Result<BanOutcome> {
        let reason = clean_reason(req.reason.as_deref())?;
        let hours = req.delete_hours.unwrap_or(0);
        if hours > MAX_PURGE_HOURS {
            return Err(DbError::Invalid("hours"));
        }
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = actor(&mut tx, gid, owner, by, perms::BAN).await?;
        if by == uid {
            return Err(DbError::Invalid("self_target"));
        }
        let exists: bool = sqlx::query_scalar("select exists(select 1 from users where id = $1)")
            .bind(uid)
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            return Err(DbError::NotFound);
        }
        let target = standing(&mut tx, gid, owner, uid).await?;
        // Le propriétaire n'est jamais banni, même s'il n'était plus « membre ».
        if uid == owner {
            return Err(DbError::Forbidden("hierarchy"));
        }
        if let Some(t) = &target {
            me.over(t)?;
        }
        sqlx::query("delete from members where guild_id = $1 and user_id = $2")
            .bind(gid)
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "insert into bans (guild_id, user_id, moderator_id, reason) values ($1, $2, $3, $4)
             on conflict (guild_id, user_id) do update set moderator_id = $3, reason = $4, created_at = now()",
        )
        .bind(gid)
        .bind(uid)
        .bind(by)
        .bind(&reason)
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, gid, by, "member_ban", Some(uid), reason.as_deref()).await?;
        tx.commit().await?;

        let (mut purged, mut files) = (Vec::new(), Vec::new());
        if hours > 0 {
            let rows: Vec<(Id, Id)> = sqlx::query_as(
                "select m.channel_id, m.id from messages m join channels c on c.id = m.channel_id
                 where c.guild_id = $1 and m.author_id = $2 and m.created_at > now() - make_interval(hours => $3)
                 order by m.id limit 5000",
            )
            .bind(gid)
            .bind(uid)
            .bind(hours as i32)
            .fetch_all(&self.pool)
            .await?;
            for (cid, mid) in rows {
                match self.delete_message(cid, mid, uid, true).await {
                    Ok(f) => {
                        files.extend(f);
                        purged.push((cid, mid));
                    }
                    Err(DbError::NotFound) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let ban = self.ban_of(gid, uid).await?;
        Ok(BanOutcome { ban, was_member: target.is_some(), purged, files })
    }

    async fn ban_of(&self, gid: Id, uid: Id) -> Result<Ban> {
        self.bans_where(gid, Some(uid)).await?.pop().ok_or(DbError::NotFound)
    }

    async fn bans_where(&self, gid: Id, only: Option<Id>) -> Result<Vec<Ban>> {
        let rows = sqlx::query(&format!(
            "select {}, b.moderator_id, b.reason, (extract(epoch from b.created_at) * 1000)::int8 as at_ms
             from bans b join users u on u.id = b.user_id
             where b.guild_id = $1 and ($2::uuid is null or b.user_id = $2) order by b.created_at desc, b.user_id",
            user_cols("u")
        ))
        .bind(gid)
        .bind(only)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Ban {
                    user: user_from(r, "u")?.expect("join"),
                    reason: r.try_get("reason")?,
                    moderator_id: r.try_get("moderator_id")?,
                    created_at: r.try_get("at_ms")?,
                })
            })
            .collect::<sqlx::Result<_>>()
            .map_err(Into::into)
    }

    /// Les bannis du serveur, pour qui peut bannir.
    pub async fn bans(&self, gid: Id, by: Id) -> Result<Vec<Ban>> {
        let p = self.guild_perms(by, gid).await?;
        if p == 0 {
            return Err(DbError::NotFound);
        }
        if p & perms::BAN == 0 {
            return Err(DbError::Forbidden("perm"));
        }
        self.bans_where(gid, None).await
    }

    pub async fn unban(&self, gid: Id, by: Id, uid: Id) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        actor(&mut tx, gid, owner, by, perms::BAN).await?;
        let done = sqlx::query("delete from bans where guild_id = $1 and user_id = $2")
            .bind(gid)
            .bind(uid)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if done == 0 {
            return Err(DbError::NotFound);
        }
        audit(&mut tx, gid, by, "member_unban", Some(uid), None).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Quitte le serveur. Le propriétaire doit d'abord transférer la propriété.
    pub async fn leave_guild(&self, gid: Id, uid: Id) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        if owner == uid {
            return Err(DbError::Forbidden("owner_leave"));
        }
        let done = sqlx::query("delete from members where guild_id = $1 and user_id = $2")
            .bind(gid)
            .bind(uid)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if done == 0 {
            return Err(DbError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    /// Le propriétaire donne le serveur à un autre membre.
    pub async fn transfer_ownership(&self, gid: Id, by: Id, to: Id) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let me = standing(&mut tx, gid, owner, by).await?.ok_or(DbError::NotFound)?;
        if !me.is_owner {
            return Err(DbError::Forbidden("owner_only"));
        }
        if by == to {
            return Err(DbError::Invalid("self_target"));
        }
        standing(&mut tx, gid, owner, to).await?.ok_or(DbError::NotFound)?;
        sqlx::query("update guilds set owner_id = $2, updated_at = now() where id = $1")
            .bind(gid)
            .bind(to)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, gid, by, "owner_transfer", Some(to), None).await?;
        tx.commit().await?;
        Ok(())
    }
}
