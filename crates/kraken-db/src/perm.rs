//! Chargement de ce qu'il faut pour calculer des droits dans un serveur, puis calcul en
//! mémoire avec `kraken-core` (une requête par table, pas une par salon).

use std::collections::{HashMap, HashSet};

use kraken_core::perms::{self, Overwrite, Target};
use kraken_proto::Id;
use sqlx::{PgConnection, PgPool, Row};

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
    /// Parmi eux, ceux qui sont exclus temporairement en ce moment.
    timed_out: HashSet<Id>,
    overwrites: HashMap<Id, Vec<Overwrite>>,
}

impl Ctx {
    /// `only` : ne charger que ce membre. `None` si le serveur n'existe pas.
    pub async fn load(pool: &PgPool, gid: Id, only: Option<Id>) -> Result<Option<Self>> {
        let mut conn = pool.acquire().await?;
        Self::load_on(&mut conn, gid, only).await
    }

    /// Comme [`Self::load`], sur une connexion (dans une transaction : on y voit ses changements).
    pub async fn load_on(conn: &mut PgConnection, gid: Id, only: Option<Id>) -> Result<Option<Self>> {
        let Some(owner) = sqlx::query_scalar::<_, Id>("select owner_id from guilds where id = $1")
            .bind(gid)
            .fetch_optional(&mut *conn)
            .await?
        else {
            return Ok(None);
        };
        let role_perms = sqlx::query("select id, permissions from roles where guild_id = $1")
            .bind(gid)
            .fetch_all(&mut *conn)
            .await?
            .iter()
            .map(|r| Ok((r.try_get("id")?, r.try_get::<i64, _>("permissions")? as u64)))
            .collect::<sqlx::Result<_>>()?;
        let rows = sqlx::query(
            "select m.user_id, coalesce(array_agg(mr.role_id) filter (where mr.role_id is not null), '{}') as roles,
                    coalesce(bool_or(m.timeout_until > now()), false) as timed_out
             from members m left join member_roles mr on mr.guild_id = m.guild_id and mr.user_id = m.user_id
             where m.guild_id = $1 and ($2::uuid is null or m.user_id = $2) group by m.user_id",
        )
        .bind(gid)
        .bind(only)
        .fetch_all(&mut *conn)
        .await?;
        let mut members = HashMap::new();
        let mut timed_out = HashSet::new();
        for r in &rows {
            let uid: Id = r.try_get("user_id")?;
            members.insert(uid, r.try_get("roles")?);
            if r.try_get::<bool, _>("timed_out")? {
                timed_out.insert(uid);
            }
        }
        let mut overwrites: HashMap<Id, Vec<Overwrite>> = HashMap::new();
        for r in sqlx::query(
            "select po.channel_id, po.target_id, po.target_kind, po.allow, po.deny
             from permission_overwrites po join channels c on c.id = po.channel_id where c.guild_id = $1",
        )
        .bind(gid)
        .fetch_all(&mut *conn)
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
        Ok(Some(Self { gid, owner, role_perms, members, timed_out, overwrites }))
    }

    /// Un membre qui n'a que `@everyone` ne voit pas ce salon (le sien ou celui de sa catégorie).
    pub fn is_private(&self, cid: Id, parent: Option<Id>) -> bool {
        let everyone = self.role_perms.get(&self.gid).copied().unwrap_or(0);
        let (above, own) = self.layers(cid, parent);
        perms::in_layers(everyone, self.gid, Id::nil(), &[], &[above, own]) & perms::VIEW_CHANNEL == 0
    }

    /// Surcharges de la catégorie, puis celles du salon.
    fn layers(&self, cid: Id, parent: Option<Id>) -> (&[Overwrite], &[Overwrite]) {
        let get = |id: Id| self.overwrites.get(&id).map_or(&[][..], Vec::as_slice);
        (parent.map_or(&[][..], get), get(cid))
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
        let (above, own) = self.layers(cid, parent);
        let roles = self.members.get(&uid).map(Vec::as_slice).unwrap_or(&[]);
        let p = perms::in_layers(base, self.gid, uid, roles, &[above, own]);
        // Exclu temporairement : il voit et lit, rien de plus (ni écrire, ni réagir, ni envoyer
        // de fichier, ni parler). Le propriétaire et les administrateurs ne le sont jamais.
        if self.timed_out.contains(&uid) && base & perms::ADMINISTRATOR == 0 {
            p & (perms::VIEW_CHANNEL | perms::READ_HISTORY)
        } else {
            p
        }
    }
}
