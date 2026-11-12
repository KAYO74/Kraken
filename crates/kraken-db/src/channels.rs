//! Salons d'un serveur : créer, modifier, supprimer, ranger, et leurs surcharges de droits.
//! Comme pour les rôles, chaque opération vérifie ici les droits de celui qui agit, dans une
//! transaction qui verrouille le serveur : deux changements simultanés passent l'un après l'autre.

use std::collections::HashMap;

use kraken_core::perms;
use kraken_proto::*;
use serde_json::json;
use sqlx::{PgConnection, Row};

use crate::moderation::{Tx, actor, audit_changes, grantable, lock, standing};
use crate::perm::Ctx;
use crate::{CHANNEL_COLS, Db, DbError, Result, channel_from, insert_channel, kind_from, kind_str};

/// Salons où l'on écrit : seuls ceux-là ont un sujet et un mode lent.
fn writable(k: ChannelKind) -> bool {
    matches!(k, ChannelKind::Text | ChannelKind::Announcement)
}

fn has_topic(k: ChannelKind) -> bool {
    writable(k) || k == ChannelKind::Forum
}

/// Le nom d'un salon tel qu'il est gardé : un salon d'écriture est en minuscules, mots liés par
/// des tirets (`salon de test` devient `salon-de-test`) ; les autres gardent leur casse. Les
/// espaces en trop disparaissent.
pub fn clean_channel_name(kind: ChannelKind, raw: &str) -> Result<String> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let name = if has_topic(kind) { words.join("-").to_lowercase() } else { words.join(" ") };
    let n = name.chars().count();
    if n == 0 || n > MAX_CHANNEL_NAME || name.chars().any(char::is_control) {
        return Err(DbError::Invalid("channel_name"));
    }
    Ok(name)
}

/// Un salon de serveur, tel qu'il est en base.
struct Row_ {
    guild_id: Id,
    parent_id: Option<Id>,
    kind: ChannelKind,
    name: String,
}

/// Le serveur du salon ; `NotFound` pour un salon qui n'existe pas ou n'est pas d'un serveur.
async fn guild_of(conn: &mut PgConnection, cid: Id) -> Result<Id> {
    sqlx::query_scalar::<_, Option<Id>>("select guild_id from channels where id = $1")
        .bind(cid)
        .fetch_optional(&mut *conn)
        .await?
        .flatten()
        .ok_or(DbError::NotFound)
}

async fn row_of(conn: &mut PgConnection, gid: Id, cid: Id) -> Result<Row_> {
    let r =
        sqlx::query("select parent_id, kind, coalesce(name, '') as name from channels where id = $1 and guild_id = $2")
            .bind(cid)
            .bind(gid)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(DbError::NotFound)?;
    Ok(Row_ {
        guild_id: gid,
        parent_id: r.try_get("parent_id")?,
        kind: kind_from(r.try_get("kind")?),
        name: r.try_get("name")?,
    })
}

/// Mes droits dans ce salon ; `NotFound` si je ne le vois pas (on ne révèle rien), `Forbidden`
/// si je ne peux pas gérer les salons ici.
async fn manager(conn: &mut PgConnection, c: &Row_, cid: Id, by: Id) -> Result<u64> {
    let ctx = Ctx::load_on(conn, c.guild_id, Some(by)).await?.ok_or(DbError::NotFound)?;
    let p = ctx.channel(by, cid, c.parent_id);
    if p & perms::VIEW_CHANNEL == 0 {
        return Err(DbError::NotFound);
    }
    if p & perms::MANAGE_CHANNELS == 0 {
        return Err(DbError::Forbidden("perm"));
    }
    Ok(p)
}

/// Garde-fou après un changement de droits : celui qui agit voit encore le salon et peut encore
/// le gérer. Sans cela, on pourrait se fermer la porte au nez.
async fn keeps_access(conn: &mut PgConnection, c: &Row_, cid: Id, by: Id) -> Result<()> {
    let ctx = Ctx::load_on(conn, c.guild_id, Some(by)).await?.ok_or(DbError::NotFound)?;
    let p = ctx.channel(by, cid, c.parent_id);
    if p & (perms::VIEW_CHANNEL | perms::MANAGE_CHANNELS) != (perms::VIEW_CHANNEL | perms::MANAGE_CHANNELS) {
        return Err(DbError::Forbidden("lockout"));
    }
    Ok(())
}

async fn overwrite_of(conn: &mut PgConnection, cid: Id, target: Id) -> Result<Option<(u64, u64)>> {
    let r = sqlx::query("select allow, deny from permission_overwrites where channel_id = $1 and target_id = $2")
        .bind(cid)
        .bind(target)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(match r {
        Some(r) => Some((r.try_get::<i64, _>("allow")? as u64, r.try_get::<i64, _>("deny")? as u64)),
        None => None,
    })
}

/// La ligne telle qu'elle est en base : son genre (rôle ou membre) fait foi, jamais celui d'une requête.
async fn overwrite_row(conn: &mut PgConnection, cid: Id, target: Id) -> Result<Option<(OverwriteKind, u64, u64)>> {
    let r = sqlx::query(
        "select target_kind, allow, deny from permission_overwrites where channel_id = $1 and target_id = $2",
    )
    .bind(cid)
    .bind(target)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match r {
        Some(r) => Some((
            if r.try_get::<&str, _>("target_kind")? == "role" { OverwriteKind::Role } else { OverwriteKind::Member },
            r.try_get::<i64, _>("allow")? as u64,
            r.try_get::<i64, _>("deny")? as u64,
        )),
        None => None,
    })
}

/// Pose la surcharge ; `allow` et `deny` vides la retirent.
async fn write_overwrite(
    conn: &mut PgConnection,
    cid: Id,
    target: Id,
    kind: OverwriteKind,
    allow: u64,
    deny: u64,
) -> Result<()> {
    let kind = match kind {
        OverwriteKind::Role => "role",
        OverwriteKind::Member => "member",
    };
    if allow == 0 && deny == 0 {
        sqlx::query("delete from permission_overwrites where channel_id = $1 and target_id = $2 and target_kind = $3")
            .bind(cid)
            .bind(target)
            .bind(kind)
            .execute(&mut *conn)
            .await?;
        return Ok(());
    }
    sqlx::query(
        "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny) values ($1, $2, $3, $4, $5)
         on conflict (channel_id, target_id) do update set allow = $4, deny = $5, updated_at = now()
         where permission_overwrites.target_kind = $3",
    )
    .bind(cid)
    .bind(target)
    .bind(kind)
    .bind(allow as i64)
    .bind(deny as i64)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn overwrites_in(conn: &mut PgConnection, cid: Id) -> Result<Vec<PermissionOverwrite>> {
    sqlx::query("select target_id, target_kind, allow, deny from permission_overwrites where channel_id = $1 order by created_at, target_id")
        .bind(cid)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|r| {
            Ok(PermissionOverwrite {
                target_id: r.try_get("target_id")?,
                kind: if r.try_get::<&str, _>("target_kind")? == "role" {
                    OverwriteKind::Role
                } else {
                    OverwriteKind::Member
                },
                allow: r.try_get::<i64, _>("allow")? as u64,
                deny: r.try_get::<i64, _>("deny")? as u64,
            })
        })
        .collect::<sqlx::Result<_>>()
        .map_err(Into::into)
}

/// Un salon qui quitte sa catégorie (ou dont la catégorie disparaît) garde l'accès qu'elle lui
/// donnait : ses surcharges sont recopiées sur le salon, cible par cible. Pour une même cible,
/// la décision du salon passe après celle de la catégorie, bit par bit, comme dans le calcul des
/// droits. Si un cas tordu (une cible de la catégorie contre une autre du salon) élargissait
/// encore les droits de quelqu'un, une interdiction à son nom referme l'écart : un salon ne
/// devient jamais plus ouvert en changeant de place. Rend le nombre de surcharges recopiées.
async fn keep_rules(tx: &mut Tx<'_>, gid: Id, from: Id, kids: &[Id]) -> Result<u64> {
    let has: bool = sqlx::query_scalar("select exists(select 1 from permission_overwrites where channel_id = $1)")
        .bind(from)
        .fetch_one(&mut **tx)
        .await?;
    if !has || kids.is_empty() {
        return Ok(0);
    }
    let before = Ctx::load_on(tx, gid, None).await?.ok_or(DbError::NotFound)?;
    let mut copied = 0;
    for kid in kids {
        copied += sqlx::query(
            "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny)
             select $1, target_id, target_kind, allow, deny from permission_overwrites where channel_id = $2
             on conflict (channel_id, target_id) do update set
               allow = permission_overwrites.allow | (excluded.allow & ~permission_overwrites.deny),
               deny = permission_overwrites.deny | (excluded.deny & ~permission_overwrites.allow),
               updated_at = now()",
        )
        .bind(kid)
        .bind(from)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    }
    let after = Ctx::load_on(tx, gid, None).await?.ok_or(DbError::NotFound)?;
    for kid in kids {
        for u in before.member_ids() {
            let wider = after.channel(u, *kid, None) & !before.channel(u, *kid, Some(from)) & perms::CHANNEL_SCOPE;
            if wider != 0 {
                sqlx::query(
                    "insert into permission_overwrites (channel_id, target_id, target_kind, allow, deny)
                     values ($1, $2, 'member', 0, $3)
                     on conflict (channel_id, target_id) do update set
                       deny = permission_overwrites.deny | $3, allow = permission_overwrites.allow & ~$3,
                       updated_at = now()",
                )
                .bind(kid)
                .bind(u)
                .bind(wider as i64)
                .execute(&mut **tx)
                .await?;
            }
        }
    }
    Ok(copied)
}

/// Resserre les places d'un groupe (les enfants d'une catégorie, les catégories, ou les salons
/// sans catégorie) : 0, 1, 2… sans trou. Rend les salons dont la place a changé.
async fn compact(tx: &mut Tx<'_>, gid: Id, parent: Option<Id>, categories: bool) -> Result<Vec<Id>> {
    let rows: Vec<(Id, i32)> = sqlx::query_as(
        "select id, position from channels
         where guild_id = $1 and parent_id is not distinct from $2 and (kind = 'category') = $3
         order by position, id",
    )
    .bind(gid)
    .bind(parent)
    .bind(categories)
    .fetch_all(&mut **tx)
    .await?;
    let mut changed = Vec::new();
    for (rank, (id, position)) in rows.into_iter().enumerate() {
        if position != rank as i32 {
            sqlx::query("update channels set position = $2, updated_at = now() where id = $1")
                .bind(id)
                .bind(rank as i32)
                .execute(&mut **tx)
                .await?;
            changed.push(id);
        }
    }
    Ok(changed)
}

impl Db {
    // ───────── lecture ─────────

    /// Tous les salons du serveur (ceux que personne ne voit aussi), avec le drapeau « privé ».
    pub async fn guild_channels(&self, gid: Id) -> Result<Vec<Channel>> {
        // Aucun membre à charger : seuls les rôles et les surcharges comptent pour « privé ».
        let Some(ctx) = Ctx::load(&self.pool, gid, Some(Id::nil())).await? else { return Ok(vec![]) };
        sqlx::query(&format!("select {CHANNEL_COLS} from channels where guild_id = $1 order by position, id"))
            .bind(gid)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|r| {
                let mut c = channel_from(r)?;
                c.private = ctx.is_private(c.id, c.parent_id);
                Ok(c)
            })
            .collect::<sqlx::Result<_>>()
            .map_err(Into::into)
    }

    /// Un salon tel que les clients l'affichent : le drapeau « privé » pour ceux d'un serveur.
    pub async fn channel_view(&self, cid: Id) -> Result<Option<Channel>> {
        let Some(mut c) = self.channel(cid).await? else { return Ok(None) };
        if let Some(g) = c.guild_id
            && let Some(ctx) = Ctx::load(&self.pool, g, Some(Id::nil())).await?
        {
            c.private = ctx.is_private(c.id, c.parent_id);
        }
        Ok(Some(c))
    }

    /// Qui voit chaque salon du serveur : de quoi dire à chacun ce qui change quand des droits
    /// ou des places changent.
    pub async fn guild_audiences(&self, gid: Id) -> Result<HashMap<Id, Vec<Id>>> {
        let Some(ctx) = Ctx::load(&self.pool, gid, None).await? else { return Ok(HashMap::new()) };
        let rows: Vec<(Id, Option<Id>)> = sqlx::query_as("select id, parent_id from channels where guild_id = $1")
            .bind(gid)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(cid, parent)| {
                let mut who: Vec<Id> =
                    ctx.member_ids().filter(|u| ctx.channel(*u, cid, parent) & perms::VIEW_CHANNEL != 0).collect();
                who.sort_unstable();
                (cid, who)
            })
            .collect())
    }

    // ───────── créer, modifier, supprimer ─────────

    /// Crée un salon ou une catégorie, à la fin de son parent. Droit requis : gérer les salons.
    pub async fn create_channel(&self, gid: Id, by: Id, req: &CreateChannel) -> Result<Channel> {
        if matches!(req.kind, ChannelKind::Dm | ChannelKind::GroupDm | ChannelKind::Thread) {
            return Err(DbError::Invalid("kind"));
        }
        let name = clean_channel_name(req.kind, &req.name)?;
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        actor(&mut tx, gid, owner, by, perms::MANAGE_CHANNELS).await?;
        let count: i64 = sqlx::query_scalar("select count(*) from channels where guild_id = $1")
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?;
        if count as usize >= MAX_CHANNELS {
            return Err(DbError::Invalid("channels"));
        }
        if let Some(p) = req.parent_id {
            let ok: bool = sqlx::query_scalar(
                "select exists(select 1 from channels where id = $1 and guild_id = $2 and kind = 'category')",
            )
            .bind(p)
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?;
            let sees = match Ctx::load_on(&mut tx, gid, Some(by)).await? {
                Some(ctx) => ctx.channel(by, p, None) & perms::VIEW_CHANNEL != 0,
                None => false,
            };
            if !ok || !sees || req.kind == ChannelKind::Category {
                return Err(DbError::Invalid("parent_id"));
            }
        }
        let id = insert_channel(&mut tx, gid, req.parent_id, req.kind, &name).await?;
        audit_changes(
            &mut tx,
            gid,
            by,
            "channel_create",
            Some(id),
            json!({ "name": name, "kind": kind_str(req.kind) }),
        )
        .await?;
        tx.commit().await?;
        self.channel_view(id).await?.ok_or(DbError::NotFound)
    }

    /// Modifie le nom, le sujet, le mode lent ou le caractère privé d'un salon.
    pub async fn edit_channel(&self, cid: Id, by: Id, req: &EditChannel) -> Result<Channel> {
        let gid = guild_of(&mut *self.pool.acquire().await?, cid).await?;
        let mut tx = self.pool.begin().await?;
        lock(&mut tx, gid).await?;
        let c = row_of(&mut tx, gid, cid).await?;
        manager(&mut tx, &c, cid, by).await?;
        let name = req.name.as_deref().map(|n| clean_channel_name(c.kind, n)).transpose()?;
        let topic = match req.topic.as_deref().map(str::trim) {
            None => None,
            Some(t) if t.chars().count() > MAX_TOPIC => return Err(DbError::Invalid("topic")),
            Some(t) if !t.is_empty() && !has_topic(c.kind) => return Err(DbError::Invalid("topic")),
            Some(t) => Some(t.to_string()),
        };
        if let Some(n) = req.slowmode_s
            && (!SLOWMODE_STEPS.contains(&n) || (n > 0 && !writable(c.kind)))
        {
            return Err(DbError::Invalid("slowmode"));
        }
        // Ce qu'il y avait avant, pour le journal d'audit.
        let (old_topic, old_slowmode): (Option<String>, i32) =
            sqlx::query_as("select topic, slowmode_s from channels where id = $1")
                .bind(cid)
                .fetch_one(&mut *tx)
                .await?;
        let old_private =
            overwrite_of(&mut tx, cid, gid).await?.is_some_and(|(_, deny)| deny & perms::VIEW_CHANNEL != 0);
        sqlx::query(
            "update channels set name = coalesce($2, name),
               topic = case when $3::text is null then topic else nullif($3, '') end,
               slowmode_s = coalesce($4, slowmode_s), updated_at = now() where id = $1",
        )
        .bind(cid)
        .bind(&name)
        .bind(&topic)
        .bind(req.slowmode_s.map(|n| n as i32))
        .execute(&mut *tx)
        .await?;
        if let Some(private) = req.private {
            // Privé = `@everyone` ne voit pas le salon : sa surcharge refuse VIEW_CHANNEL.
            let (allow, deny) = overwrite_of(&mut tx, cid, gid).await?.unwrap_or((0, 0));
            let (allow, deny) = if private {
                (allow & !perms::VIEW_CHANNEL, deny | perms::VIEW_CHANNEL)
            } else {
                (allow, deny & !perms::VIEW_CHANNEL)
            };
            write_overwrite(&mut tx, cid, gid, OverwriteKind::Role, allow, deny).await?;
            keeps_access(&mut tx, &c, cid, by).await?;
        }
        if name.is_some() || topic.is_some() || req.slowmode_s.is_some() || req.private.is_some() {
            // Seulement ce qui change, avant et après ; le nom d'alors reste là si le salon disparaît.
            let mut diff = serde_json::Map::new();
            let mut note = |field: &str, old: serde_json::Value, new: serde_json::Value| {
                if old != new {
                    diff.insert(field.into(), json!({ "old": old, "new": new }));
                }
            };
            if let Some(n) = &name {
                note("name", json!(c.name), json!(n));
            }
            if let Some(t) = &topic {
                note("topic", json!(old_topic.clone().unwrap_or_default()), json!(t));
            }
            if let Some(n) = req.slowmode_s {
                note("slowmode_s", json!(old_slowmode), json!(n));
            }
            if let Some(p) = req.private {
                note("private", json!(old_private), json!(p));
            }
            diff.insert("target_name".into(), json!(name.as_deref().unwrap_or(&c.name)));
            audit_changes(&mut tx, gid, by, "channel_update", Some(cid), serde_json::Value::Object(diff)).await?;
        }
        tx.commit().await?;
        self.channel_view(cid).await?.ok_or(DbError::NotFound)
    }

    /// Supprime un salon avec ses messages et ses fichiers ; rend les chemins des fichiers à
    /// effacer du disque. Le dernier salon d'écriture d'un serveur reste. Les salons d'une
    /// catégorie supprimée sortent de la catégorie, à la fin des salons sans catégorie.
    pub async fn delete_channel(&self, cid: Id, by: Id) -> Result<Vec<String>> {
        let gid = guild_of(&mut *self.pool.acquire().await?, cid).await?;
        let mut tx = self.pool.begin().await?;
        lock(&mut tx, gid).await?;
        let c = row_of(&mut tx, gid, cid).await?;
        manager(&mut tx, &c, cid, by).await?;
        if writable(c.kind) {
            let writing: i64 = sqlx::query_scalar(
                "select count(*) from channels where guild_id = $1 and kind in ('text', 'announcement')",
            )
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?;
            if writing <= 1 {
                return Err(DbError::Conflict("last_text"));
            }
        }
        let mut files = Vec::new();
        for r in sqlx::query("select storage_key, preview from attachments where channel_id = $1")
            .bind(cid)
            .fetch_all(&mut *tx)
            .await?
        {
            files.extend(crate::attachment_paths(r.try_get("storage_key")?, r.try_get("preview")?));
        }
        let embeds: Vec<sqlx::types::Json<Vec<Embed>>> =
            sqlx::query_scalar("select embeds from messages where channel_id = $1 and embeds <> '[]'::jsonb")
                .bind(cid)
                .fetch_all(&mut *tx)
                .await?;
        for e in embeds {
            files.extend(crate::embed_paths(&e.0));
        }
        let mut kept = 0;
        if c.kind == ChannelKind::Category {
            let start: i32 = sqlx::query_scalar(
                "select coalesce(max(position) + 1, 0) from channels
                 where guild_id = $1 and parent_id is null and kind <> 'category'",
            )
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?;
            let kids: Vec<Id> =
                sqlx::query_scalar("select id from channels where parent_id = $1 order by position, id")
                    .bind(cid)
                    .fetch_all(&mut *tx)
                    .await?;
            // Les salons d'une catégorie privée restent privés : ils gardent ses règles d'accès.
            kept = keep_rules(&mut tx, gid, cid, &kids).await?;
            for (i, kid) in kids.into_iter().enumerate() {
                sqlx::query("update channels set parent_id = null, position = $2, updated_at = now() where id = $1")
                    .bind(kid)
                    .bind(start + i as i32)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        sqlx::query("delete from channels where id = $1").bind(cid).execute(&mut *tx).await?;
        compact(&mut tx, gid, c.parent_id, c.kind == ChannelKind::Category).await?;
        audit_changes(
            &mut tx,
            gid,
            by,
            "channel_delete",
            Some(cid),
            json!({ "name": c.name, "kind": kind_str(c.kind), "rules_kept": kept }),
        )
        .await?;
        tx.commit().await?;
        Ok(files)
    }

    /// Place un salon dans un parent, à un rang parmi les salons que `by` y voit. Les salons
    /// qu'il ne voit pas gardent leur ordre. Rend les salons dont la place a changé.
    pub async fn move_channel(&self, cid: Id, by: Id, req: &MoveChannel) -> Result<Vec<Id>> {
        let gid = guild_of(&mut *self.pool.acquire().await?, cid).await?;
        let mut tx = self.pool.begin().await?;
        lock(&mut tx, gid).await?;
        let c = row_of(&mut tx, gid, cid).await?;
        manager(&mut tx, &c, cid, by).await?;
        let category = c.kind == ChannelKind::Category;
        if category && req.parent_id.is_some() {
            return Err(DbError::Invalid("parent_id"));
        }
        let ctx = Ctx::load_on(&mut tx, gid, Some(by)).await?.ok_or(DbError::NotFound)?;
        if let Some(p) = req.parent_id {
            let ok: bool = sqlx::query_scalar(
                "select exists(select 1 from channels where id = $1 and guild_id = $2 and kind = 'category')",
            )
            .bind(p)
            .bind(gid)
            .fetch_one(&mut *tx)
            .await?;
            if !ok || ctx.channel(by, p, None) & perms::VIEW_CHANNEL == 0 {
                return Err(DbError::Invalid("parent_id"));
            }
        }
        // Le groupe d'arrivée : les enfants d'une catégorie, les catégories, ou les salons sans catégorie.
        let group: Vec<(Id, Option<Id>, i32)> = sqlx::query_as(
            "select id, parent_id, position from channels
             where guild_id = $1 and id <> $2 and parent_id is not distinct from $3
               and (kind = 'category') = $4 order by position, id",
        )
        .bind(gid)
        .bind(cid)
        .bind(req.parent_id)
        .bind(category)
        .fetch_all(&mut *tx)
        .await?;
        let seen = |(id, parent, _): &(Id, Option<Id>, i32)| ctx.channel(by, *id, *parent) & perms::VIEW_CHANNEL != 0;
        let visible: Vec<usize> = group.iter().enumerate().filter(|(_, g)| seen(g)).map(|(i, _)| i).collect();
        let at = match visible.get(req.index as usize) {
            Some(i) => *i,
            None => group.len(),
        };
        let mut order: Vec<Id> = group.iter().map(|g| g.0).collect();
        order.insert(at, cid);
        let was: HashMap<Id, (Option<Id>, i32)> = group.iter().map(|g| (g.0, (g.1, g.2))).collect();
        // Un salon qui change de catégorie garde l'accès que l'ancienne lui donnait.
        let left = c.parent_id.filter(|p| Some(*p) != req.parent_id);
        let kept = match left {
            Some(old) if !category => keep_rules(&mut tx, gid, old, &[cid]).await?,
            _ => 0,
        };
        let mut changed = Vec::new();
        for (rank, id) in order.iter().enumerate() {
            let rank = rank as i32;
            let same = if *id == cid {
                c.parent_id == req.parent_id && self_position(&mut tx, cid).await? == rank
            } else {
                was[id].1 == rank
            };
            if !same {
                sqlx::query("update channels set parent_id = $2, position = $3, updated_at = now() where id = $1")
                    .bind(id)
                    .bind(req.parent_id)
                    .bind(rank)
                    .execute(&mut *tx)
                    .await?;
                changed.push(*id);
            }
        }
        if c.parent_id != req.parent_id {
            // Plus de trou là où il était.
            for id in compact(&mut tx, gid, c.parent_id, false).await? {
                if !changed.contains(&id) {
                    changed.push(id);
                }
            }
        }
        if !changed.is_empty() {
            audit_changes(
                &mut tx,
                gid,
                by,
                "channel_move",
                Some(cid),
                json!({ "parent_id": req.parent_id, "index": req.index, "rules_kept": kept, "target_name": c.name }),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(changed)
    }

    // ───────── surcharges de droits ─────────

    /// Les surcharges de droits d'un salon, pour qui peut le gérer.
    pub async fn channel_overwrites(&self, cid: Id, by: Id) -> Result<Vec<PermissionOverwrite>> {
        let mut conn = self.pool.acquire().await?;
        let gid = guild_of(&mut conn, cid).await?;
        let c = row_of(&mut conn, gid, cid).await?;
        manager(&mut conn, &c, cid, by).await?;
        overwrites_in(&mut conn, cid).await
    }

    /// Remplace (ou, si `allow` et `deny` sont vides, retire) la surcharge d'un rôle ou d'un
    /// membre. On ne peut changer que des droits que l'on a soi-même, sur `@everyone`, un rôle
    /// plus bas que le sien ou un membre de rang inférieur ; et pas se retirer l'accès au salon.
    pub async fn set_overwrite(
        &self,
        cid: Id,
        by: Id,
        target: Id,
        req: &SetOverwrite,
    ) -> Result<Vec<PermissionOverwrite>> {
        if req.allow & req.deny != 0 {
            return Err(DbError::Invalid("overwrite"));
        }
        if (req.allow | req.deny) & !perms::CHANNEL_SCOPE != 0 {
            return Err(DbError::Invalid("permissions"));
        }
        let gid = guild_of(&mut *self.pool.acquire().await?, cid).await?;
        let mut tx = self.pool.begin().await?;
        let owner = lock(&mut tx, gid).await?;
        let c = row_of(&mut tx, gid, cid).await?;
        manager(&mut tx, &c, cid, by).await?;
        let me = standing(&mut tx, gid, owner, by).await?.ok_or(DbError::NotFound)?;
        let stored = overwrite_row(&mut tx, cid, target).await?;
        let removing = req.allow == 0 && req.deny == 0;
        // Le genre de la ligne en base fait foi : pour la retirer, on ne regarde pas celui de la
        // requête (sinon un « membre » à la place d'un « rôle » esquiverait la hiérarchie) ; pour la
        // modifier, les deux doivent concorder.
        let kind = match stored {
            Some((k, ..)) if removing => k,
            Some((k, ..)) if k != req.kind => return Err(DbError::Invalid("overwrite_kind")),
            _ => req.kind,
        };
        let before = stored.map(|(_, allow, deny)| (allow, deny));
        let (old_allow, old_deny) = before.unwrap_or((0, 0));
        let position: Option<i32> = sqlx::query_scalar("select position from roles where id = $1 and guild_id = $2")
            .bind(target)
            .bind(gid)
            .fetch_optional(&mut *tx)
            .await?;
        let member = standing(&mut tx, gid, owner, target).await?;
        // Une surcharge dont la cible a disparu (rôle supprimé, membre parti) doit pouvoir être
        // retirée : il suffit de gérer le salon, rien à comparer en rang ni en droits. Orpheline
        // seulement si la cible n'est ni un rôle de ce serveur ni un de ses membres.
        let orphan = removing && before.is_some() && position.is_none() && member.is_none();
        match kind {
            OverwriteKind::Role => match position {
                Some(position) => {
                    if target != gid {
                        me.over_role(position)?;
                    }
                }
                None if orphan => {}
                None => return Err(DbError::NotFound),
            },
            OverwriteKind::Member => match member {
                Some(who) => {
                    if target != by {
                        me.over(&who)?;
                    }
                }
                None if orphan => {}
                None => return Err(DbError::NotFound),
            },
        }
        if !orphan {
            grantable(&me, (req.allow ^ old_allow) | (req.deny ^ old_deny))?;
        }
        if before.is_none() && (req.allow | req.deny) != 0 {
            let n: i64 = sqlx::query_scalar("select count(*) from permission_overwrites where channel_id = $1")
                .bind(cid)
                .fetch_one(&mut *tx)
                .await?;
            if n as usize >= MAX_OVERWRITES {
                return Err(DbError::Invalid("overwrites"));
            }
        }
        write_overwrite(&mut tx, cid, target, kind, req.allow, req.deny).await?;
        keeps_access(&mut tx, &c, cid, by).await?;
        audit_changes(
            &mut tx,
            gid,
            by,
            "overwrite_update",
            Some(cid),
            json!({
                "target": target,
                "allow": req.allow,
                "deny": req.deny,
                "subject_kind": if kind == OverwriteKind::Role { "role" } else { "member" },
                "target_name": c.name,
            }),
        )
        .await?;
        let list = overwrites_in(&mut tx, cid).await?;
        tx.commit().await?;
        Ok(list)
    }

    /// Retire la surcharge d'un rôle ou d'un membre.
    pub async fn remove_overwrite(&self, cid: Id, by: Id, target: Id) -> Result<Vec<PermissionOverwrite>> {
        let kind = {
            let mut conn = self.pool.acquire().await?;
            let kind: Option<String> = sqlx::query_scalar(
                "select target_kind from permission_overwrites where channel_id = $1 and target_id = $2",
            )
            .bind(cid)
            .bind(target)
            .fetch_optional(&mut *conn)
            .await?;
            match kind.as_deref() {
                Some("role") => OverwriteKind::Role,
                Some(_) => OverwriteKind::Member,
                None => {
                    // Rien à retirer : même réponse que pour un salon qu'on ne peut pas gérer.
                    self.channel_overwrites(cid, by).await?;
                    return Err(DbError::NotFound);
                }
            }
        };
        self.set_overwrite(cid, by, target, &SetOverwrite { kind, allow: 0, deny: 0 }).await
    }
}

async fn self_position(tx: &mut Tx<'_>, cid: Id) -> Result<i32> {
    Ok(sqlx::query_scalar("select position from channels where id = $1").bind(cid).fetch_one(&mut **tx).await?)
}
