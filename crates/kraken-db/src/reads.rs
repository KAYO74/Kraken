//! Non-lus et réglages de notification. Une ligne de `read_states` par personne et par salon :
//! le dernier message lu et le nombre de mentions arrivées depuis. Le compteur avance à chaque
//! message (règles de `kraken_core::notify`), revient à zéro quand on lit, et se recalcule quand
//! on lit seulement une partie ou qu'un message est supprimé.

use std::collections::{HashMap, HashSet};

use kraken_core::notify::{self, Level, Seen};
use kraken_proto::*;
use sqlx::Row;
use sqlx::postgres::PgRow;

use crate::{Db, DbError, Result};

/// Messages relus au plus pour recalculer un compteur de mentions.
const RECOUNT_LIMIT: i64 = 1000;

/// Plus longue sourdine à durée fixe : 30 jours.
const MAX_MUTE_MINUTES: u32 = 30 * 24 * 60;

const STATE_COLS: &str = "channel_id, last_read_message_id, mention_count";

fn state_from(r: &PgRow) -> sqlx::Result<ReadState> {
    Ok(ReadState {
        channel_id: r.try_get("channel_id")?,
        last_read: r.try_get("last_read_message_id")?,
        mention_count: r.try_get::<i32, _>("mention_count")?.max(0) as u32,
    })
}

const SETTING_COLS: &str = "guild_id, channel_id, level, suppress_everyone,
    case when muted_until = 'infinity' then 9223372036854775807
         else (extract(epoch from muted_until) * 1000)::int8 end as muted_ms";

fn level_from(s: &str) -> NotifyLevel {
    match s {
        "all" => NotifyLevel::All,
        "mentions" => NotifyLevel::Mentions,
        "none" => NotifyLevel::Nothing,
        _ => NotifyLevel::Default,
    }
}

fn level_str(l: NotifyLevel) -> &'static str {
    match l {
        NotifyLevel::Default => "default",
        NotifyLevel::All => "all",
        NotifyLevel::Mentions => "mentions",
        NotifyLevel::Nothing => "none",
    }
}

/// Le niveau du domaine ; `None` pour « par défaut ».
fn core_level(s: &str) -> Option<Level> {
    match s {
        "all" => Some(Level::All),
        "mentions" => Some(Level::Mentions),
        "none" => Some(Level::Nothing),
        _ => None,
    }
}

fn setting_from(r: &PgRow) -> sqlx::Result<NotifySetting> {
    Ok(NotifySetting {
        guild_id: r.try_get("guild_id")?,
        channel_id: r.try_get("channel_id")?,
        level: level_from(r.try_get("level")?),
        suppress_everyone: r.try_get("suppress_everyone")?,
        muted_until: r.try_get("muted_ms")?,
    })
}

/// Ce qui décide, pour une personne, si un message de ce salon compte.
#[derive(Default)]
struct Prefs {
    channel: Option<Level>,
    guild: Option<Level>,
    ignore_everyone: bool,
}

impl Db {
    /// Où j'en suis dans mes salons.
    pub async fn read_states(&self, uid: Id) -> Result<Vec<ReadState>> {
        let rows = sqlx::query(&format!("select {STATE_COLS} from read_states where user_id = $1"))
            .bind(uid)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(state_from).collect::<sqlx::Result<_>>()?)
    }

    pub async fn read_state(&self, uid: Id, cid: Id) -> Result<ReadState> {
        let r = sqlx::query(&format!("select {STATE_COLS} from read_states where user_id = $1 and channel_id = $2"))
            .bind(uid)
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(match r {
            Some(r) => state_from(&r)?,
            None => ReadState { channel_id: cid, ..Default::default() },
        })
    }

    /// Mes réglages de notification.
    pub async fn notify_settings(&self, uid: Id) -> Result<Vec<NotifySetting>> {
        let rows = sqlx::query(&format!(
            "select {SETTING_COLS} from notification_overrides where user_id = $1 order by created_at"
        ))
        .bind(uid)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(setting_from).collect::<sqlx::Result<_>>()?)
    }

    /// Réglages de ces personnes pour ce salon et son serveur.
    async fn prefs(&self, users: &[Id], cid: Id, gid: Option<Id>) -> Result<HashMap<Id, Prefs>> {
        let rows = sqlx::query(
            "select user_id, channel_id, level, suppress_everyone from notification_overrides
             where user_id = any($1) and (channel_id = $2 or (guild_id = $3 and channel_id is null))",
        )
        .bind(users)
        .bind(cid)
        .bind(gid)
        .fetch_all(&self.pool)
        .await?;
        let mut out: HashMap<Id, Prefs> = HashMap::new();
        for r in rows {
            let p = out.entry(r.try_get("user_id")?).or_default();
            let level = core_level(r.try_get("level")?);
            if r.try_get::<Option<Id>, _>("channel_id")?.is_some() {
                p.channel = level;
            } else {
                p.guild = level;
                p.ignore_everyone = r.try_get("suppress_everyone")?;
            }
        }
        Ok(out)
    }

    /// Un message vient d'être posté : son auteur a lu le salon jusque-là, et le compteur de
    /// mentions avance chez ceux qu'il mentionne (parmi `audience`, ceux qui voient le salon),
    /// sauf au niveau « rien » et chez qui a bloqué l'auteur.
    pub async fn record_message(&self, m: &Message, guild: Option<Id>, audience: &[Id]) -> Result<()> {
        let author = m.author.as_ref().map(|a| a.id);
        if let Some(a) = author {
            sqlx::query(
                "insert into read_states (user_id, channel_id, last_read_message_id) values ($1, $2, $3)
                 on conflict (user_id, channel_id) do update
                 set last_read_message_id = greatest(read_states.last_read_message_id, $3), mention_count = 0,
                     updated_at = now()",
            )
            .bind(a)
            .bind(m.channel_id)
            .bind(m.id)
            .execute(&self.pool)
            .await?;
        }
        let mentions: Vec<Id> = m.mentions.iter().map(|u| u.id).collect();
        let seen = Seen {
            author,
            mentions: &mentions,
            everyone: m.mention_everyone,
            reply_to: m.reply.as_ref().and_then(|r| r.author.as_ref()).map(|u| u.id),
            private: guild.is_none(),
        };
        // D'abord sans réglages (`@everyone` compris) : seuls ceux-là peuvent compter.
        let maybe: Vec<Id> = audience.iter().copied().filter(|u| notify::is_mention(*u, &seen, false)).collect();
        if maybe.is_empty() {
            return Ok(());
        }
        let prefs = self.prefs(&maybe, m.channel_id, guild).await?;
        let blocking: HashSet<Id> = match author {
            Some(a) => sqlx::query_scalar(
                "select user_id from relationships where kind = 'blocked' and target_id = $1 and user_id = any($2)",
            )
            .bind(a)
            .bind(&maybe)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .collect(),
            None => HashSet::new(),
        };
        let none = Prefs::default();
        let counted: Vec<Id> = maybe
            .into_iter()
            .filter(|u| {
                let p = prefs.get(u).unwrap_or(&none);
                let level = notify::effective_level(guild.is_none(), p.channel, p.guild);
                !blocking.contains(u) && notify::counts(level, notify::is_mention(*u, &seen, p.ignore_everyone))
            })
            .collect();
        sqlx::query(
            "insert into read_states (user_id, channel_id, mention_count) select unnest($1::uuid[]), $2, 1
             on conflict (user_id, channel_id) do update set mention_count = read_states.mention_count + 1,
                 updated_at = now()
             where read_states.last_read_message_id is null or read_states.last_read_message_id < $3",
        )
        .bind(&counted)
        .bind(m.channel_id)
        .bind(m.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mentions pour `uid` dans ce salon après `after` (tout le salon si `None`), d'après ses
    /// réglages actuels ; les [`RECOUNT_LIMIT`] derniers messages au plus.
    async fn recount(&self, uid: Id, cid: Id, gid: Option<Id>, after: Option<Id>) -> Result<u32> {
        let rows = sqlx::query(
            "select m.author_id, m.mention_everyone, r.author_id as reply_author,
                    exists(select 1 from message_mentions mm where mm.message_id = m.id and mm.user_id = $1) as named,
                    coalesce(exists(select 1 from relationships b where b.user_id = $1 and b.target_id = m.author_id
                                    and b.kind = 'blocked'), false) as blocked
             from messages m left join messages r on r.id = m.reply_to_id
             where m.channel_id = $2 and ($3::uuid is null or m.id > $3) and m.author_id is distinct from $1
             order by m.id desc limit $4",
        )
        .bind(uid)
        .bind(cid)
        .bind(after)
        .bind(RECOUNT_LIMIT)
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(0);
        }
        let p = self.prefs(&[uid], cid, gid).await?.remove(&uid).unwrap_or_default();
        let level = notify::effective_level(gid.is_none(), p.channel, p.guild);
        let mut n = 0;
        for r in rows {
            let named: bool = r.try_get("named")?;
            let seen = Seen {
                author: r.try_get("author_id")?,
                mentions: if named { std::slice::from_ref(&uid) } else { &[] },
                everyone: r.try_get("mention_everyone")?,
                reply_to: r.try_get("reply_author")?,
                private: gid.is_none(),
            };
            if !r.try_get::<bool, _>("blocked")?
                && notify::counts(level, notify::is_mention(uid, &seen, p.ignore_everyone))
            {
                n += 1;
            }
        }
        Ok(n)
    }

    /// J'ai lu ce salon jusqu'au message `mid` (qui doit en faire partie ; jamais en arrière).
    /// Le compteur garde les mentions plus récentes. Les droits sont vérifiés par l'appelant.
    pub async fn ack(&self, uid: Id, cid: Id, mid: Id) -> Result<ReadState> {
        let gid: Option<Option<Id>> = sqlx::query_scalar(
            "select c.guild_id from messages m join channels c on c.id = m.channel_id where m.id = $1 and m.channel_id = $2",
        )
        .bind(mid)
        .bind(cid)
        .fetch_optional(&self.pool)
        .await?;
        let gid = gid.ok_or(DbError::NotFound)?;
        let before = self.read_state(uid, cid).await?;
        let upto = before.last_read.map_or(mid, |l| l.max(mid));
        let count = self.recount(uid, cid, gid, Some(upto)).await?;
        let r = sqlx::query(&format!(
            "insert into read_states (user_id, channel_id, last_read_message_id, mention_count) values ($1, $2, $3, $4)
             on conflict (user_id, channel_id) do update
             set last_read_message_id = greatest(read_states.last_read_message_id, $3), mention_count = $4, updated_at = now()
             returning {STATE_COLS}"
        ))
        .bind(uid)
        .bind(cid)
        .bind(upto)
        .bind(count as i32)
        .fetch_one(&self.pool)
        .await?;
        Ok(state_from(&r)?)
    }

    /// Tout le serveur lu : chaque salon que je vois, jusqu'à son dernier message. Rend les
    /// salons qui ont changé (`None` : pas membre).
    pub async fn ack_guild(&self, uid: Id, gid: Id) -> Result<Option<Vec<ReadState>>> {
        let Some(g) = self.guild_state(gid, uid).await? else { return Ok(None) };
        let (ids, last): (Vec<Id>, Vec<Id>) =
            g.channels.iter().filter_map(|c| Some((c.id, c.last_message_id?))).unzip();
        let rows = sqlx::query(&format!(
            "insert into read_states (user_id, channel_id, last_read_message_id, mention_count)
             select $1, c, l, 0 from unnest($2::uuid[], $3::uuid[]) as t(c, l)
             on conflict (user_id, channel_id) do update
             set last_read_message_id = greatest(read_states.last_read_message_id, excluded.last_read_message_id),
                 mention_count = 0, updated_at = now()
             where read_states.last_read_message_id is distinct from excluded.last_read_message_id
                or read_states.mention_count <> 0
             returning {STATE_COLS}"
        ))
        .bind(uid)
        .bind(&ids)
        .bind(&last)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(rows.iter().map(state_from).collect::<sqlx::Result<_>>()?))
    }

    /// Après la suppression du message `mid` : recalcule le compteur de ceux qui ne l'avaient pas
    /// encore lu et avaient des mentions en attente. Rend leurs nouveaux états.
    pub async fn recount_after_delete(&self, cid: Id, mid: Id) -> Result<Vec<(Id, ReadState)>> {
        let Some(c) = self.channel(cid).await? else { return Ok(vec![]) };
        let rows: Vec<(Id, Option<Id>, i32)> = sqlx::query_as(
            "select user_id, last_read_message_id, mention_count from read_states
             where channel_id = $1 and mention_count > 0 and (last_read_message_id is null or last_read_message_id < $2)",
        )
        .bind(cid)
        .bind(mid)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for (uid, last, before) in rows {
            let n = self.recount(uid, cid, c.guild_id, last).await?;
            if n as i32 != before {
                sqlx::query("update read_states set mention_count = $3, updated_at = now() where user_id = $1 and channel_id = $2")
                    .bind(uid)
                    .bind(cid)
                    .bind(n as i32)
                    .execute(&self.pool)
                    .await?;
                out.push((uid, ReadState { channel_id: cid, last_read: last, mention_count: n }));
            }
        }
        Ok(out)
    }

    /// Change mon réglage pour un serveur ou un salon (l'un des deux ; l'appelant a vérifié que
    /// je le vois). Un salon revenu au réglage de son serveur, sans sourdine, perd sa ligne.
    pub async fn set_notify(&self, uid: Id, req: &SetNotify) -> Result<NotifySetting> {
        let (gid, cid) = match (req.guild_id, req.channel_id) {
            (Some(g), None) => (Some(g), None),
            (None, Some(c)) => (self.channel(c).await?.ok_or(DbError::NotFound)?.guild_id, Some(c)),
            _ => return Err(DbError::Invalid("target")),
        };
        let mute = match req.mute {
            None => "keep",
            Some(Mute::Off) => "off",
            Some(Mute::Forever) => "forever",
            Some(Mute::Minutes(m)) if (1..=MAX_MUTE_MINUTES).contains(&m) => "minutes",
            Some(Mute::Minutes(_)) => return Err(DbError::Invalid("mute")),
        };
        let minutes = match req.mute {
            Some(Mute::Minutes(m)) => m as i32,
            _ => 0,
        };
        let conflict = if cid.is_some() {
            "(user_id, channel_id) where channel_id is not null"
        } else {
            "(user_id, guild_id) where channel_id is null"
        };
        let until = |old: &str| {
            format!(
                "case $6::text when 'keep' then {old} when 'off' then null::timestamptz
                   when 'forever' then 'infinity'::timestamptz else now() + make_interval(mins => $7) end"
            )
        };
        let r = sqlx::query(&format!(
            "insert into notification_overrides as o (user_id, guild_id, channel_id, level, suppress_everyone, muted_until)
             values ($1, $2, $3, coalesce($4, 'default'), coalesce($5, false), {})
             on conflict {conflict} do update
             set level = coalesce($4, o.level), suppress_everyone = coalesce($5, o.suppress_everyone),
                 muted_until = {}, updated_at = now()
             returning {SETTING_COLS}",
            until("null::timestamptz"),
            until("o.muted_until"),
        ))
        .bind(uid)
        .bind(gid)
        .bind(cid)
        .bind(req.level.map(level_str))
        .bind(req.suppress_everyone)
        .bind(mute)
        .bind(minutes)
        .fetch_one(&self.pool)
        .await?;
        let s = setting_from(&r)?;
        if let Some(c) = cid
            && s.level == NotifyLevel::Default
            && s.muted_until.is_none()
            && !s.suppress_everyone
        {
            sqlx::query("delete from notification_overrides where user_id = $1 and channel_id = $2")
                .bind(uid)
                .bind(c)
                .execute(&self.pool)
                .await?;
        }
        Ok(s)
    }
}
