//! Amis, demandes d'ami, blocages ; ouverture des MP et des groupes privés.
//!
//! Une ligne de `relationships` par sens : une amitié, ce sont deux lignes `friend` ; une
//! demande, `pending_out` chez qui l'envoie et `pending_in` chez qui la reçoit ; un blocage,
//! une seule ligne `blocked` chez qui bloque (l'autre n'en sait rien : la relation disparaît
//! simplement de sa liste). Chaque changement verrouille la paire, donc deux demandes croisées
//! finissent en une amitié et pas en deux demandes.

use kraken_proto::*;
use sqlx::{Postgres, Row, Transaction};

use crate::{CHANNEL_COLS, Db, DbError, Result, channel_from, user_cols, user_from};

type Tx<'a> = Transaction<'a, Postgres>;

/// Qui prévenir, et de quoi.
pub type Notices = Vec<(Id, Event)>;

fn rel(user: User, kind: RelationshipKind) -> Relationship {
    Relationship { user, kind, status: Status::Offline }
}

fn kind_of(s: &str) -> Option<RelationshipKind> {
    Some(match s {
        "friend" => RelationshipKind::Friend,
        "blocked" => RelationshipKind::Blocked,
        "pending_in" => RelationshipKind::PendingIncoming,
        "pending_out" => RelationshipKind::PendingOutgoing,
        _ => return None,
    })
}

/// Verrou de transaction sur la paire, dans les deux sens.
async fn lock_pair(tx: &mut Tx<'_>, a: Id, b: Id) -> Result<()> {
    let key = if a < b { format!("rel:{a}:{b}") } else { format!("rel:{b}:{a}") };
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))").bind(key).execute(&mut **tx).await?;
    Ok(())
}

/// Un compte actif (ni supprimé ni en cours de suppression).
async fn active_user(tx: &mut Tx<'_>, id: Id) -> Result<User> {
    let r = sqlx::query(&format!(
        "select {} from users u where u.id = $1 and u.deleted_at is null and u.deletion_requested_at is null",
        user_cols("u")
    ))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(DbError::NotFound)?;
    user_from(&r, "u")?.ok_or(DbError::NotFound)
}

/// La relation de `a` envers `b`, telle qu'écrite en base.
async fn row(tx: &mut Tx<'_>, a: Id, b: Id) -> Result<Option<String>> {
    Ok(sqlx::query_scalar("select kind from relationships where user_id = $1 and target_id = $2")
        .bind(a)
        .bind(b)
        .fetch_optional(&mut **tx)
        .await?)
}

async fn set(tx: &mut Tx<'_>, a: Id, b: Id, kind: &str) -> Result<()> {
    sqlx::query(
        "insert into relationships (user_id, target_id, kind) values ($1, $2, $3)
         on conflict (user_id, target_id) do update set kind = excluded.kind, updated_at = now()",
    )
    .bind(a)
    .bind(b)
    .bind(kind)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn unset(tx: &mut Tx<'_>, a: Id, b: Id) -> Result<()> {
    sqlx::query("delete from relationships where user_id = $1 and target_id = $2")
        .bind(a)
        .bind(b)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Amitié ou demande (pas un blocage).
fn social(kind: Option<&str>) -> bool {
    matches!(kind, Some("friend" | "pending_in" | "pending_out"))
}

impl Db {
    /// Un compte actif par son pseudo unique (sans `@`, casse ignorée).
    pub async fn user_by_username(&self, username: &str) -> Result<Option<User>> {
        let name = username.trim().trim_start_matches('@').to_lowercase();
        let r = sqlx::query(&format!(
            "select {} from users u where u.username = $1 and u.deleted_at is null and u.deletion_requested_at is null",
            user_cols("u")
        ))
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(match r {
            Some(r) => user_from(&r, "u")?,
            None => None,
        })
    }

    /// Ma relation avec cette personne, s'il y en a une.
    pub async fn relationship(&self, me: Id, other: Id) -> Result<Option<RelationshipKind>> {
        let k: Option<String> =
            sqlx::query_scalar("select kind from relationships where user_id = $1 and target_id = $2")
                .bind(me)
                .bind(other)
                .fetch_optional(&self.pool)
                .await?;
        Ok(k.as_deref().and_then(kind_of))
    }

    /// Demander `target` en ami, ou accepter sa demande s'il m'en a envoyé une. Refusé : moi-même
    /// (`Invalid("self")`), compte inconnu (`NotFound`), déjà amis ou déjà demandé (`Conflict`),
    /// compte que j'ai bloqué (`Forbidden("blocked")`), compte qui m'a bloqué
    /// (`Forbidden("request_refused")`, sans dire pourquoi).
    pub async fn befriend(&self, me: Id, target: Id) -> Result<(Relationship, Notices)> {
        if me == target {
            return Err(DbError::Invalid("self"));
        }
        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, me, target).await?;
        let them = active_user(&mut tx, target).await?;
        let me_u = active_user(&mut tx, me).await?;
        let mine = row(&mut tx, me, target).await?;
        let theirs = row(&mut tx, target, me).await?;
        let (mine_now, events) = match mine.as_deref() {
            Some("blocked") => return Err(DbError::Forbidden("blocked")),
            _ if theirs.as_deref() == Some("blocked") => return Err(DbError::Forbidden("request_refused")),
            Some("friend") => return Err(DbError::Conflict("already_friends")),
            Some("pending_out") => return Err(DbError::Conflict("already_sent")),
            Some("pending_in") => {
                set(&mut tx, me, target, "friend").await?;
                set(&mut tx, target, me, "friend").await?;
                let r = rel(them, RelationshipKind::Friend);
                let ev = vec![
                    (me, Event::RelationshipUpdate(r.clone())),
                    (target, Event::RelationshipUpdate(rel(me_u, RelationshipKind::Friend))),
                ];
                (r, ev)
            }
            _ => {
                set(&mut tx, me, target, "pending_out").await?;
                set(&mut tx, target, me, "pending_in").await?;
                let r = rel(them, RelationshipKind::PendingOutgoing);
                let theirs_ev = if theirs.is_some() { Event::RelationshipUpdate } else { Event::RelationshipAdd };
                let ev = vec![
                    (me, Event::RelationshipAdd(r.clone())),
                    (target, theirs_ev(rel(me_u, RelationshipKind::PendingIncoming))),
                ];
                (r, ev)
            }
        };
        tx.commit().await?;
        Ok((mine_now, events))
    }

    /// Retirer un ami, refuser ou annuler une demande, débloquer. `NotFound` sans relation.
    pub async fn remove_relationship(&self, me: Id, target: Id) -> Result<Notices> {
        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, me, target).await?;
        let mine = row(&mut tx, me, target).await?;
        if mine.is_none() || mine.as_deref() == Some("ignored") {
            return Err(DbError::NotFound);
        }
        unset(&mut tx, me, target).await?;
        let mut events = vec![(me, Event::RelationshipRemove { user_id: target })];
        // Un blocage ne touche que ma ligne ; une amitié ou une demande, les deux.
        if social(mine.as_deref()) && social(row(&mut tx, target, me).await?.as_deref()) {
            unset(&mut tx, target, me).await?;
            events.push((target, Event::RelationshipRemove { user_id: me }));
        }
        tx.commit().await?;
        Ok(events)
    }

    /// Bloquer : l'amitié ou les demandes entre nous disparaissent, des deux côtés. L'autre
    /// n'est pas prévenu du blocage lui-même. Bloquer deux fois ne change rien.
    pub async fn block(&self, me: Id, target: Id) -> Result<(Relationship, Notices)> {
        if me == target {
            return Err(DbError::Invalid("self"));
        }
        let mut tx = self.pool.begin().await?;
        lock_pair(&mut tx, me, target).await?;
        let them = active_user(&mut tx, target).await?;
        let mine = row(&mut tx, me, target).await?;
        let r = rel(them, RelationshipKind::Blocked);
        if mine.as_deref() == Some("blocked") {
            return Ok((r, vec![]));
        }
        let mut events = Vec::new();
        if social(row(&mut tx, target, me).await?.as_deref()) {
            unset(&mut tx, target, me).await?;
            events.push((target, Event::RelationshipRemove { user_id: me }));
        }
        set(&mut tx, me, target, "blocked").await?;
        let mine_ev = if mine.is_some() { Event::RelationshipUpdate } else { Event::RelationshipAdd };
        events.push((me, mine_ev(r.clone())));
        tx.commit().await?;
        Ok((r, events))
    }

    /// `(je l'ai bloqué, il m'a bloqué)`.
    pub async fn blocks(&self, me: Id, other: Id) -> Result<(bool, bool)> {
        let (mine, theirs): (bool, bool) = sqlx::query_as(
            "select exists(select 1 from relationships where user_id = $1 and target_id = $2 and kind = 'blocked'),
                    exists(select 1 from relationships where user_id = $2 and target_id = $1 and kind = 'blocked')",
        )
        .bind(me)
        .bind(other)
        .fetch_one(&self.pool)
        .await?;
        Ok((mine, theirs))
    }

    // ───────── MP et groupes ─────────

    /// Ajoute à chaque salon privé ses participants, sauf `viewer`.
    async fn with_recipients(&self, mut channels: Vec<Channel>, viewer: Id) -> Result<Vec<Channel>> {
        let ids: Vec<Id> = channels.iter().map(|c| c.id).collect();
        let rows = sqlx::query(&format!(
            "select cr.channel_id, {} from channel_recipients cr join users u on u.id = cr.user_id
             where cr.channel_id = any($1) and cr.user_id <> $2 order by cr.created_at, u.id",
            user_cols("u")
        ))
        .bind(&ids)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await?;
        for r in &rows {
            let cid: Id = r.try_get("channel_id")?;
            if let (Some(c), Some(u)) = (channels.iter_mut().find(|c| c.id == cid), user_from(r, "u")?) {
                c.recipients.push(u);
            }
        }
        Ok(channels)
    }

    /// Mes MP et groupes ouverts, le plus récemment actif en premier.
    pub async fn private_channels(&self, uid: Id) -> Result<Vec<Channel>> {
        let rows = sqlx::query(&format!(
            "select {CHANNEL_COLS} from channels where id in
               (select channel_id from channel_recipients where user_id = $1 and not closed)
             order by coalesce(last_message_id, id) desc"
        ))
        .bind(uid)
        .fetch_all(&self.pool)
        .await?;
        let channels = rows.iter().map(channel_from).collect::<sqlx::Result<Vec<_>>>()?;
        self.with_recipients(channels, uid).await
    }

    /// Un MP ou un groupe tel que `viewer` le voit ; `None` s'il n'en fait pas partie.
    pub async fn private_channel(&self, cid: Id, viewer: Id) -> Result<Option<Channel>> {
        let r = sqlx::query(&format!(
            "select {CHANNEL_COLS} from channels c where id = $1 and guild_id is null
               and exists(select 1 from channel_recipients where channel_id = c.id and user_id = $2)"
        ))
        .bind(cid)
        .bind(viewer)
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = r else { return Ok(None) };
        Ok(self.with_recipients(vec![channel_from(&r)?], viewer).await?.pop())
    }

    /// Le MP avec `other`, créé s'il n'existe pas (rend aussi `true` dans ce cas). Il faut ne
    /// pas s'être bloqués, et être amis ou membres d'un même serveur pour en créer un.
    pub async fn open_dm(&self, me: Id, other: Id) -> Result<(Id, bool)> {
        if me == other {
            return Err(DbError::Invalid("self"));
        }
        if self.user(other).await?.is_none() {
            return Err(DbError::NotFound);
        }
        match self.blocks(me, other).await? {
            (true, _) => return Err(DbError::Forbidden("dm_blocked")),
            (_, true) => return Err(DbError::Forbidden("dm_refused")),
            _ => {}
        }
        let key = if me < other { format!("{me}:{other}") } else { format!("{other}:{me}") };
        let mut tx = self.pool.begin().await?;
        let existing: Option<Id> =
            sqlx::query_scalar("select id from channels where dm_key = $1").bind(&key).fetch_optional(&mut *tx).await?;
        let (cid, created) = match existing {
            Some(id) => (id, false),
            None => {
                let known: bool = sqlx::query_scalar(
                    "select exists(select 1 from relationships where user_id = $1 and target_id = $2 and kind = 'friend')
                        or exists(select 1 from members a join members b on b.guild_id = a.guild_id
                                  where a.user_id = $1 and b.user_id = $2)",
                )
                .bind(me)
                .bind(other)
                .fetch_one(&mut *tx)
                .await?;
                if !known {
                    return Err(DbError::Forbidden("dm_stranger"));
                }
                let id: Option<Id> = sqlx::query_scalar(
                    "insert into channels (kind, dm_key, owner_id) values ('dm', $1, $2)
                     on conflict (dm_key) do nothing returning id",
                )
                .bind(&key)
                .bind(me)
                .fetch_optional(&mut *tx)
                .await?;
                match id {
                    Some(id) => (id, true),
                    // Ouvert au même instant par l'autre.
                    None => (
                        sqlx::query_scalar("select id from channels where dm_key = $1")
                            .bind(&key)
                            .fetch_one(&mut *tx)
                            .await?,
                        false,
                    ),
                }
            }
        };
        sqlx::query(
            "insert into channel_recipients (channel_id, user_id) select $1, unnest($2::uuid[])
             on conflict (channel_id, user_id) do update set closed = false",
        )
        .bind(cid)
        .bind(vec![me, other])
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((cid, created))
    }

    /// Nouveau groupe avec ces amis (de 2 à [`MAX_GROUP_MEMBERS`] − 1), dont je suis le
    /// propriétaire.
    pub async fn create_group(&self, me: Id, others: &[Id], name: Option<&str>) -> Result<Id> {
        let mut others: Vec<Id> = others.iter().copied().filter(|u| *u != me).collect();
        others.sort();
        others.dedup();
        if others.len() < 2 || others.len() + 1 > MAX_GROUP_MEMBERS {
            return Err(DbError::Invalid("recipients"));
        }
        let name = group_name(name)?;
        let mut tx = self.pool.begin().await?;
        let friends: i64 = sqlx::query_scalar(
            "select count(*) from relationships r join users u on u.id = r.target_id
             where r.user_id = $1 and r.target_id = any($2) and r.kind = 'friend' and u.deleted_at is null
               and u.deletion_requested_at is null",
        )
        .bind(me)
        .bind(&others)
        .fetch_one(&mut *tx)
        .await?;
        if friends as usize != others.len() {
            return Err(DbError::Forbidden("not_friend"));
        }
        let id: Id =
            sqlx::query_scalar("insert into channels (kind, name, owner_id) values ('group_dm', $1, $2) returning id")
                .bind(name)
                .bind(me)
                .fetch_one(&mut *tx)
                .await?;
        // Moi d'abord : la liste des membres suit l'ordre d'arrivée.
        for u in std::iter::once(me).chain(others) {
            sqlx::query("insert into channel_recipients (channel_id, user_id) values ($1, $2)")
                .bind(id)
                .bind(u)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Verrouille un groupe dont `me` fait partie ; rend son propriétaire. `NotFound` sinon,
    /// `Invalid("kind")` pour un autre salon que je vois.
    async fn lock_group(tx: &mut Tx<'_>, cid: Id, me: Id) -> Result<Option<Id>> {
        let r = sqlx::query(
            "select c.kind, c.owner_id, exists(select 1 from channel_recipients where channel_id = c.id and user_id = $2) as inside
             from channels c where c.id = $1 and c.guild_id is null for update",
        )
        .bind(cid)
        .bind(me)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(DbError::NotFound)?;
        if !r.try_get::<bool, _>("inside")? {
            return Err(DbError::NotFound);
        }
        if r.try_get::<&str, _>("kind")? != "group_dm" {
            return Err(DbError::Invalid("kind"));
        }
        Ok(r.try_get("owner_id")?)
    }

    /// Renommer un groupe (tout membre). Vide : plus de nom.
    pub async fn rename_group(&self, cid: Id, me: Id, name: Option<&str>) -> Result<()> {
        let name = group_name(name)?;
        let mut tx = self.pool.begin().await?;
        Self::lock_group(&mut tx, cid, me).await?;
        sqlx::query("update channels set name = $2, updated_at = now() where id = $1")
            .bind(cid)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Ajouter un de mes amis à un groupe dont je fais partie ; `false` s'il y était déjà.
    pub async fn add_recipient(&self, cid: Id, me: Id, target: Id) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        Self::lock_group(&mut tx, cid, me).await?;
        let (inside, count): (bool, i64) = sqlx::query_as(
            "select coalesce(bool_or(user_id = $2), false), count(*) from channel_recipients where channel_id = $1",
        )
        .bind(cid)
        .bind(target)
        .fetch_one(&mut *tx)
        .await?;
        if inside {
            return Ok(false);
        }
        if row(&mut tx, me, target).await?.as_deref() != Some("friend") {
            return Err(DbError::Forbidden("not_friend"));
        }
        if count as usize >= MAX_GROUP_MEMBERS {
            return Err(DbError::Invalid("group_full"));
        }
        sqlx::query("insert into channel_recipients (channel_id, user_id) values ($1, $2)")
            .bind(cid)
            .bind(target)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Quitter un groupe (`target == me`) ou en retirer quelqu'un (propriétaire seulement).
    /// Rend ceux qui restent ; le dernier parti, le groupe disparaît. Si le propriétaire
    /// s'en va, le plus ancien membre le remplace.
    pub async fn remove_recipient(&self, cid: Id, me: Id, target: Id) -> Result<Vec<Id>> {
        let mut tx = self.pool.begin().await?;
        let owner = Self::lock_group(&mut tx, cid, me).await?;
        if target != me && owner != Some(me) {
            return Err(DbError::Forbidden("owner"));
        }
        let gone = sqlx::query("delete from channel_recipients where channel_id = $1 and user_id = $2")
            .bind(cid)
            .bind(target)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if gone == 0 {
            return Err(DbError::NotFound);
        }
        let left: Vec<Id> = sqlx::query_scalar(
            "select user_id from channel_recipients where channel_id = $1 order by created_at, user_id",
        )
        .bind(cid)
        .fetch_all(&mut *tx)
        .await?;
        match left.first() {
            None => {
                sqlx::query("delete from channels where id = $1").bind(cid).execute(&mut *tx).await?;
            }
            Some(heir) if owner == Some(target) => {
                sqlx::query("update channels set owner_id = $2 where id = $1")
                    .bind(cid)
                    .bind(heir)
                    .execute(&mut *tx)
                    .await?;
            }
            Some(_) => {}
        }
        tx.commit().await?;
        Ok(left)
    }
}

/// Nom de groupe nettoyé : `None` s'il est vide, refusé s'il est trop long.
fn group_name(name: Option<&str>) -> Result<Option<String>> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    if name.is_some_and(|n| n.chars().count() > MAX_GROUP_NAME) {
        return Err(DbError::Invalid("name"));
    }
    Ok(name.map(String::from))
}
