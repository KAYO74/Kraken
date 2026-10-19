//! Chargement de ce qu'il faut pour calculer des droits dans un serveur, puis calcul en
//! mémoire avec `kraken-core` (une requête par table, pas une par salon).

use std::collections::HashMap;

use kraken_core::perms::{self, Overwrite, Target};
use kraken_proto::Id;
use sqlx::{PgPool, Row};

use crate::Result;

/// Droits d'un participant dans un MP ou un groupe.
pub(crate) const DM: u64 = perms::VIEW_CHANNEL
    | perms::SEND_MESSAGES
    | perms::READ_HISTORY
    | perms::ADD_REACTIONS
    | perms::ATTACH_FILES
    | perms::EMBED_LINKS
    | perms::CONNECT
    | perms::SPEAK
    | perms::VIDEO
    | perms::STREAM;

pub(crate) struct Ctx {
    gid: Id,
    owner: Id,
    role_perms: HashMap<Id, u64>,
    /// Membres chargés (un seul ou tous) et leurs rôles.
    members: HashMap<Id, Vec<Id>>,
    overwrites: HashMap<Id, Vec<Overwrite>>,
}

impl Ctx {
    /// `only` : ne charger que ce membre. `None` si le serveur n'existe pas.
    pub async fn load(pool: &PgPool, gid: Id, only: Option<Id>) -> Result<Option<Self>> {
        let Some(owner) = sqlx::query_scalar::<_, Id>("select owner_id from guilds where id = $1")
            .bind(gid)
            .fetch_optional(pool)
            .await?
        else {
            return Ok(None);
        };
        let role_perms = sqlx::query("select id, permissions from roles where guild_id = $1")
            .bind(gid)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| Ok((r.try_get("id")?, r.try_get::<i64, _>("permissions")? as u64)))
            .collect::<sqlx::Result<_>>()?;
        let members = sqlx::query(
            "select m.user_id, coalesce(array_agg(mr.role_id) filter (where mr.role_id is not null), '{}') as roles
             from members m left join member_roles mr on mr.guild_id = m.guild_id and mr.user_id = m.user_id
             where m.guild_id = $1 and ($2::uuid is null or m.user_id = $2) group by m.user_id",
        )
        .bind(gid)
        .bind(only)
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| Ok((r.try_get("user_id")?, r.try_get("roles")?)))
        .collect::<sqlx::Result<_>>()?;
        let mut overwrites: HashMap<Id, Vec<Overwrite>> = HashMap::new();
        for r in sqlx::query(
            "select po.channel_id, po.target_id, po.target_kind, po.allow, po.deny
             from permission_overwrites po join channels c on c.id = po.channel_id where c.guild_id = $1",
        )
        .bind(gid)
        .fetch_all(pool)
        .await?
        {
            let target: Id = r.try_get("target_id")?;
            let target = if r.try_get::<&str, _>("target_kind")? == "role" {
                Target::Role(target)
            } else {
                Target::Member(target)
            };
            overwrites.entry(r.try_get("channel_id")?).or_default().push(Overwrite {
                target,
                allow: r.try_get::<i64, _>("allow")? as u64,
                deny: r.try_get::<i64, _>("deny")? as u64,
            });
        }
        Ok(Some(Self { gid, owner, role_perms, members, overwrites }))
    }

    pub fn is_member(&self, uid: Id) -> bool {
        self.members.contains_key(&uid)
    }

    pub fn member_ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.members.keys().copied()
    }

    /// Droits au niveau du serveur ; 0 pour un non-membre.
    pub fn base(&self, uid: Id) -> u64 {
        let Some(roles) = self.members.get(&uid) else { return 0 };
        let role = |id: &Id| self.role_perms.get(id).copied().unwrap_or(0);
        perms::base(self.owner == uid, role(&self.gid), roles.iter().map(role))
    }

    /// Droits dans un salon : surcharges de la catégorie, puis du salon.
    pub fn channel(&self, uid: Id, cid: Id, parent: Option<Id>) -> u64 {
        let base = self.base(uid);
        if base == 0 {
            return 0;
        }
        let mut ow = Vec::new();
        if let Some(p) = parent.and_then(|p| self.overwrites.get(&p)) {
            ow.extend(p);
        }
        ow.extend(self.overwrites.get(&cid).into_iter().flatten());
        let roles = self.members.get(&uid).map(Vec::as_slice).unwrap_or(&[]);
        perms::in_channel(base, self.gid, uid, roles, &ow)
    }
}
