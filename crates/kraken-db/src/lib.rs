//! Couche données de KRAKEN : PostgreSQL (`migrations/`, 0001 = `replica/schema.sql`).
//!
//! Toutes les lectures et écritures passent par ici. Les droits se calculent avec
//! `kraken-core` à partir de ce que renvoie la base (pas de RLS : le serveur est le seul
//! client de la base). Les dates sortent en millisecondes Unix.

mod jobs;
mod perm;
mod seed;
mod social;

use kraken_core::perms;
use kraken_proto::*;
use rand::Rng;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::types::Json;
use sqlx::{Postgres, Row, Transaction};

pub use jobs::Job;
pub use social::Notices;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    /// Violation d'unicité (nom du champ).
    Conflict(&'static str),
    NotFound,
    /// Violation d'une contrainte `check` (nom du champ).
    Invalid(&'static str),
    /// Interdit par une règle de données (bannissement…).
    Forbidden(&'static str),
    Internal(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        if let Some(db) = e.as_database_error() {
            let c = db.constraint().unwrap_or_default();
            match db.code().as_deref() {
                Some("23505") => {
                    return Self::Conflict(if c.contains("username") {
                        "username"
                    } else if c.contains("email") || c == "users_lower_idx" {
                        "email"
                    } else if c.contains("nonce") {
                        "nonce"
                    } else {
                        "unique"
                    });
                }
                // Contrainte, texte trop long, caractère nul ou inencodable.
                Some("23514" | "22001" | "22021" | "22P05") => return Self::Invalid("value"),
                _ => {}
            }
        }
        match e {
            sqlx::Error::RowNotFound => Self::NotFound,
            e => Self::Internal(e.to_string()),
        }
    }
}

pub fn valid_username(u: &str) -> bool {
    (2..=32).contains(&u.len())
        && u.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

/// Colonnes d'un utilisateur affichable, préfixées par `p`.
fn user_cols(p: &str) -> String {
    format!(
        "{p}.id as {p}_id, {p}.username as {p}_username, {p}.display_name as {p}_display, {p}.avatar_key as {p}_avatar"
    )
}

fn user_from(r: &PgRow, p: &str) -> sqlx::Result<Option<User>> {
    let id: Option<Id> = r.try_get(format!("{p}_id").as_str())?;
    let Some(id) = id else { return Ok(None) };
    Ok(Some(User {
        id,
        username: r.try_get(format!("{p}_username").as_str())?,
        display_name: r.try_get(format!("{p}_display").as_str())?,
        avatar: r.try_get(format!("{p}_avatar").as_str())?,
    }))
}

fn kind_from(s: &str) -> ChannelKind {
    match s {
        "voice" => ChannelKind::Voice,
        "announcement" => ChannelKind::Announcement,
        "category" => ChannelKind::Category,
        "forum" => ChannelKind::Forum,
        "stage" => ChannelKind::Stage,
        "dm" => ChannelKind::Dm,
        "group_dm" => ChannelKind::GroupDm,
        "thread" => ChannelKind::Thread,
        _ => ChannelKind::Text,
    }
}

pub(crate) fn kind_str(k: ChannelKind) -> &'static str {
    match k {
        ChannelKind::Text => "text",
        ChannelKind::Voice => "voice",
        ChannelKind::Announcement => "announcement",
        ChannelKind::Category => "category",
        ChannelKind::Forum => "forum",
        ChannelKind::Stage => "stage",
        ChannelKind::Dm => "dm",
        ChannelKind::GroupDm => "group_dm",
        ChannelKind::Thread => "thread",
    }
}

const CHANNEL_COLS: &str = "id, guild_id, parent_id, kind, name, topic, position";

fn channel_from(r: &PgRow) -> sqlx::Result<Channel> {
    Ok(Channel {
        id: r.try_get("id")?,
        guild_id: r.try_get("guild_id")?,
        parent_id: r.try_get("parent_id")?,
        kind: kind_from(r.try_get("kind")?),
        name: r.try_get("name")?,
        topic: r.try_get("topic")?,
        position: r.try_get("position")?,
        recipients: vec![],
    })
}

fn message_sql(filter: &str) -> String {
    format!(
        "select m.id, m.channel_id, m.content, m.nonce, m.kind, m.reply_to_id, m.mention_everyone,
                (extract(epoch from m.created_at) * 1000)::int8 as created_ms,
                (extract(epoch from m.edited_at) * 1000)::int8 as edited_ms, {}, {},
                left(r.content, {REPLY_EXCERPT}) as reply_content,
                coalesce((select json_agg(json_build_object('emoji', x.emoji_key, 'users', x.users) order by x.first)
                          from (select emoji_key, array_agg(user_id order by created_at, user_id) as users,
                                       min(created_at) as first
                                from reactions where message_id = m.id group by emoji_key) x), '[]') as reactions,
                coalesce((select json_agg(json_build_object('id', u.id, 'username', u.username,
                                  'display_name', u.display_name, 'avatar', u.avatar_key) order by u.username)
                          from message_mentions mm join users u on u.id = mm.user_id
                          where mm.message_id = m.id), '[]') as mentions
         from messages m left join users a on a.id = m.author_id
         left join messages r on r.id = m.reply_to_id left join users ra on ra.id = r.author_id {filter}",
        user_cols("a"),
        user_cols("ra"),
    )
}

fn message_from(r: &PgRow) -> sqlx::Result<Message> {
    let reply = match r.try_get::<&str, _>("kind")? {
        "reply" => Some(Reply {
            id: r.try_get("reply_to_id")?,
            author: user_from(r, "ra")?,
            content: r.try_get::<Option<String>, _>("reply_content")?.unwrap_or_default(),
        }),
        _ => None,
    };
    Ok(Message {
        id: r.try_get("id")?,
        channel_id: r.try_get("channel_id")?,
        author: user_from(r, "a")?,
        content: r.try_get("content")?,
        nonce: r.try_get("nonce")?,
        created_at: r.try_get("created_ms")?,
        edited_at: r.try_get("edited_ms")?,
        reply,
        reactions: r.try_get::<Json<Vec<Reaction>>, _>("reactions")?.0,
        mentions: r.try_get::<Json<Vec<User>>, _>("mentions")?.0,
        mention_everyone: r.try_get("mention_everyone")?,
    })
}

/// Un message à envoyer.
#[derive(Clone, Debug, Default)]
pub struct Draft<'a> {
    pub content: &'a str,
    pub nonce: Option<String>,
    pub reply_to: Option<Id>,
    /// L'auteur a le droit de mentionner `@everyone` et `@here` dans ce salon.
    pub may_mention_everyone: bool,
}

/// Une réaction ajoutée ou retirée a-t-elle changé quelque chose ?
pub type Changed = bool;

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

impl Db {
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new().max_connections(20).connect(url).await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn ping(&self) -> Result<()> {
        sqlx::query("select 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn migrate(&self) -> Result<()> {
        MIGRATOR.run(&self.pool).await.map_err(|e| DbError::Internal(e.to_string()))
    }

    // ───────── comptes ─────────

    pub async fn create_user(&self, username: &str, email: &str, password_hash: String) -> Result<User> {
        if !valid_username(username) {
            return Err(DbError::Invalid("username"));
        }
        let r = sqlx::query(&format!(
            "insert into users as u (username, email, password_hash) values ($1, $2, $3) returning {}",
            user_cols("u")
        ))
        .bind(username)
        .bind(email.trim())
        .bind(password_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(user_from(&r, "u")?.expect("returning"))
    }

    pub async fn user(&self, id: Id) -> Result<Option<User>> {
        let r =
            sqlx::query(&format!("select {} from users u where u.id = $1 and u.deleted_at is null", user_cols("u")))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(match r {
            Some(r) => user_from(&r, "u")?,
            None => None,
        })
    }

    pub async fn set_display_name(&self, id: Id, name: Option<&str>) -> Result<User> {
        let r = sqlx::query(&format!(
            "update users as u set display_name = $2 where u.id = $1 and u.deleted_at is null returning {}",
            user_cols("u")
        ))
        .bind(id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        match r {
            Some(r) => user_from(&r, "u")?.ok_or(DbError::NotFound),
            None => Err(DbError::NotFound),
        }
    }

    /// Ceux qui voient cet utilisateur : membres de ses serveurs, ses MP, ses relations.
    pub async fn related_users(&self, id: Id) -> Result<Vec<Id>> {
        Ok(sqlx::query_scalar(
            "select m2.user_id from members m1 join members m2 on m2.guild_id = m1.guild_id where m1.user_id = $1
             union select r2.user_id from channel_recipients r1
               join channel_recipients r2 on r2.channel_id = r1.channel_id where r1.user_id = $1
             union select user_id from relationships where target_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Utilisateur et hash du mot de passe, par pseudo ou e-mail. Exclut les comptes
    /// supprimés ou en cours de suppression.
    pub async fn user_credentials(&self, login: &str) -> Result<Option<(User, String)>> {
        let r = sqlx::query(&format!(
            "select {}, u.password_hash from users u
             where (u.username = lower($1) or lower(u.email) = lower($1))
               and u.deleted_at is null and u.deletion_requested_at is null and u.password_hash is not null",
            user_cols("u")
        ))
        .bind(login.trim())
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = r else { return Ok(None) };
        Ok(Some((user_from(&r, "u")?.expect("id"), r.try_get("password_hash")?)))
    }

    pub async fn password_hash(&self, uid: Id) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("select password_hash from users where id = $1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await?
            .flatten())
    }

    /// Id et pseudo d'un compte actif par e-mail (mot de passe oublié).
    pub async fn user_by_email(&self, email: &str) -> Result<Option<(Id, String)>> {
        let r = sqlx::query(
            "select id, username from users where lower(email) = lower($1) and deleted_at is null and deletion_requested_at is null",
        )
        .bind(email.trim())
        .fetch_optional(&self.pool)
        .await?;
        Ok(match r {
            Some(r) => Some((r.try_get("id")?, r.try_get("username")?)),
            None => None,
        })
    }

    /// E-mail, pseudo et vérification d'un compte actif.
    pub async fn user_email(&self, uid: Id) -> Result<Option<(String, String, bool)>> {
        let r = sqlx::query(
            "select email, username, email_verified_at is not null as verified from users
             where id = $1 and email is not null and deleted_at is null and deletion_requested_at is null",
        )
        .bind(uid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(match r {
            Some(r) => Some((r.try_get("email")?, r.try_get("username")?, r.try_get("verified")?)),
            None => None,
        })
    }

    pub async fn set_password(&self, uid: Id, password_hash: &str) -> Result<()> {
        sqlx::query("update users set password_hash = $2 where id = $1")
            .bind(uid)
            .bind(password_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_email_verified(&self, uid: Id) -> Result<()> {
        sqlx::query("update users set email_verified_at = coalesce(email_verified_at, now()) where id = $1")
            .bind(uid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_session(&self, user_id: Id, token_hash: &[u8], name: &str, platform: &str) -> Result<()> {
        sqlx::query("insert into sessions (user_id, token_hash, name, platform) values ($1, $2, $3, $4)")
            .bind(user_id)
            .bind(token_hash)
            .bind(name)
            .bind(platform)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn session_user(&self, token_hash: &[u8]) -> Result<Option<User>> {
        let r = sqlx::query(&format!(
            "select {} from sessions s join users u on u.id = s.user_id
             where s.token_hash = $1 and s.revoked_at is null and u.deleted_at is null and u.deletion_requested_at is null",
            user_cols("u")
        ))
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(match r {
            Some(r) => user_from(&r, "u")?,
            None => None,
        })
    }

    pub async fn revoke_session(&self, token_hash: &[u8]) -> Result<()> {
        sqlx::query("update sessions set revoked_at = now() where token_hash = $1 and revoked_at is null")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn revoke_all_sessions(&self, uid: Id) -> Result<u64> {
        let r = sqlx::query("update sessions set revoked_at = now() where user_id = $1 and revoked_at is null")
            .bind(uid)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }

    /// Jeton à usage unique (vérif e-mail, reset), stocké haché.
    pub async fn create_auth_token(&self, uid: Id, kind: &str, token_hash: &[u8], ttl_minutes: i32) -> Result<()> {
        sqlx::query(
            "insert into auth_tokens (user_id, kind, token_hash, expires_at)
             values ($1, $2, $3, now() + make_interval(mins => $4))",
        )
        .bind(uid)
        .bind(kind)
        .bind(token_hash)
        .bind(ttl_minutes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Consomme le jeton s'il est valide ; rend l'utilisateur concerné.
    pub async fn use_auth_token(&self, kind: &str, token_hash: &[u8]) -> Result<Option<Id>> {
        Ok(sqlx::query_scalar(
            "update auth_tokens set used_at = now()
             where kind = $1 and token_hash = $2 and used_at is null and expires_at > now() returning user_id",
        )
        .bind(kind)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Demande de suppression : le compte est fermé tout de suite (sessions révoquées,
    /// connexion refusée) et anonymisé par la tâche `account_purge` après le délai de grâce.
    /// Ses serveurs sans autre membre disparaissent ; les autres passent au plus ancien membre.
    pub async fn request_deletion(&self, uid: Id) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        // La ligne verrouillée d'abord : un `create_guild` concurrent attend, puis voit le compte fermé.
        sqlx::query("update users set deletion_requested_at = now() where id = $1").bind(uid).execute(&mut *tx).await?;
        let heir = "from members m join users u on u.id = m.user_id
                    where m.guild_id = g.id and m.user_id <> $1 and u.deletion_requested_at is null";
        sqlx::query(&format!("delete from guilds g where owner_id = $1 and not exists(select 1 {heir})"))
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&format!(
            "update guilds g set owner_id = (select m.user_id {heir} order by m.joined_at, m.user_id limit 1)
             where owner_id = $1"
        ))
        .bind(uid)
        .execute(&mut *tx)
        .await?;
        sqlx::query("update sessions set revoked_at = now() where user_id = $1 and revoked_at is null")
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Anonymise les comptes dont la demande de suppression a plus de `grace_days` jours :
    /// identité, e-mail, mot de passe, profil et liens effacés ; les messages restent sous
    /// « compte supprimé ».
    pub async fn purge_deleted_accounts(&self, grace_days: i32) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let ids: Vec<Id> = sqlx::query_scalar(
            "select id from users where deletion_requested_at < now() - make_interval(days => $1) and deleted_at is null for update",
        )
        .bind(grace_days)
        .fetch_all(&mut *tx)
        .await?;
        for table in ["sessions", "auth_tokens", "relationships", "connections", "user_settings", "members"] {
            sqlx::query(&format!("delete from {table} where user_id = any($1)")).bind(&ids).execute(&mut *tx).await?;
        }
        sqlx::query("delete from relationships where target_id = any($1)").bind(&ids).execute(&mut *tx).await?;
        sqlx::query(
            "update users set username = 'deleted_' || substr(replace(id::text, '-', ''), 21), display_name = null,
               email = null, phone = null, password_hash = null, mfa_secret = null, mfa_recovery_hashes = '{}',
               avatar_key = null, banner_key = null, avatar_decoration = null, bio = null, pronouns = null,
               accent_color = null, profile_theme = null, name_style = null, deleted_at = now()
             where id = any($1)",
        )
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ids.len() as u64)
    }

    // ───────── serveurs ─────────

    /// Crée le serveur, son rôle `@everyone` (id = id du serveur), deux catégories, `#général`
    /// et un salon vocal, et fait du créateur le premier membre.
    pub async fn create_guild(&self, owner: Id, name: &str) -> Result<GuildState> {
        let name = name.trim();
        if !(2..=100).contains(&name.chars().count()) {
            return Err(DbError::Invalid("name"));
        }
        let mut tx = self.pool.begin().await?;
        // FOR SHARE : sérialisé avec `request_deletion`, un compte fermé ne crée plus rien.
        sqlx::query("select 1 from users where id = $1 and deletion_requested_at is null for share")
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        let gid: Id = sqlx::query_scalar("insert into guilds (owner_id, name) values ($1, $2) returning id")
            .bind(owner)
            .bind(name)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("insert into roles (id, guild_id, name, permissions) values ($1, $1, '@everyone', $2)")
            .bind(gid)
            .bind(perms::DEFAULT_EVERYONE as i64)
            .execute(&mut *tx)
            .await?;
        let text = insert_channel(&mut tx, gid, None, ChannelKind::Category, "Salons textuels").await?;
        let general = insert_channel(&mut tx, gid, Some(text), ChannelKind::Text, "général").await?;
        let voice = insert_channel(&mut tx, gid, None, ChannelKind::Category, "Salons vocaux").await?;
        insert_channel(&mut tx, gid, Some(voice), ChannelKind::Voice, "Général").await?;
        sqlx::query("update guilds set system_channel_id = $2 where id = $1")
            .bind(gid)
            .bind(general)
            .execute(&mut *tx)
            .await?;
        sqlx::query("insert into members (guild_id, user_id) values ($1, $2)")
            .bind(gid)
            .bind(owner)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.guild_state(gid, owner).await?.ok_or(DbError::NotFound)
    }

    /// Le serveur tel que ce membre le voit (sans les salons qui lui sont cachés) ;
    /// `None` s'il n'en est pas membre.
    pub async fn guild_state(&self, gid: Id, uid: Id) -> Result<Option<GuildState>> {
        let ctx = perm::Ctx::load(&self.pool, gid, Some(uid)).await?;
        let Some(ctx) = ctx.filter(|c| c.is_member(uid)) else { return Ok(None) };
        let g = sqlx::query("select id, name, icon_key, owner_id from guilds where id = $1")
            .bind(gid)
            .fetch_one(&self.pool)
            .await?;
        let guild = Guild {
            id: g.try_get("id")?,
            name: g.try_get("name")?,
            icon: g.try_get("icon_key")?,
            owner_id: g.try_get("owner_id")?,
        };
        let channels =
            sqlx::query(&format!("select {CHANNEL_COLS} from channels where guild_id = $1 order by position, id"))
                .bind(gid)
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(channel_from)
                .collect::<sqlx::Result<Vec<_>>>()?
                .into_iter()
                .filter(|c| ctx.channel(uid, c.id, c.parent_id) & perms::VIEW_CHANNEL != 0)
                .collect();
        let roles = sqlx::query(
            "select id, name, color, position, permissions from roles where guild_id = $1 order by position",
        )
        .bind(gid)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(|r| {
            Ok(Role {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                color: r.try_get::<Option<i32>, _>("color")?.map(|c| c as u32),
                position: r.try_get("position")?,
                permissions: r.try_get::<i64, _>("permissions")? as u64,
            })
        })
        .collect::<sqlx::Result<Vec<_>>>()?;
        // États vocaux : en mémoire dans le serveur, ajoutés par lui.
        Ok(Some(GuildState { guild, channels, roles, members: self.members(gid).await?, voice_states: vec![] }))
    }

    async fn members(&self, gid: Id) -> Result<Vec<Member>> {
        let rows = sqlx::query(&format!(
            "select {}, m.nick, (extract(epoch from m.joined_at) * 1000)::int8 as joined_ms,
                    coalesce(array_agg(mr.role_id) filter (where mr.role_id is not null), '{{}}') as roles
             from members m join users u on u.id = m.user_id
             left join member_roles mr on mr.guild_id = m.guild_id and mr.user_id = m.user_id
             where m.guild_id = $1 group by u.id, m.nick, m.joined_at order by m.joined_at",
            user_cols("u")
        ))
        .bind(gid)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Member {
                    user: user_from(r, "u")?.expect("join"),
                    nick: r.try_get("nick")?,
                    roles: r.try_get("roles")?,
                    joined_at: r.try_get("joined_ms")?,
                })
            })
            .collect::<sqlx::Result<_>>()
            .map_err(Into::into)
    }

    pub async fn user_guilds(&self, uid: Id) -> Result<Vec<GuildState>> {
        let ids: Vec<Id> = sqlx::query_scalar("select guild_id from members where user_id = $1 order by joined_at")
            .bind(uid)
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::with_capacity(ids.len());
        for g in ids {
            out.extend(self.guild_state(g, uid).await?);
        }
        Ok(out)
    }

    pub async fn guild_perms(&self, uid: Id, gid: Id) -> Result<u64> {
        Ok(perm::Ctx::load(&self.pool, gid, Some(uid)).await?.map_or(0, |c| c.base(uid)))
    }

    /// Membres du serveur (pour prévenir les autres d'une arrivée).
    pub async fn member_ids(&self, gid: Id) -> Result<Vec<Id>> {
        Ok(sqlx::query_scalar("select user_id from members where guild_id = $1")
            .bind(gid)
            .fetch_all(&self.pool)
            .await?)
    }

    // ───────── salons ─────────

    pub async fn channel(&self, cid: Id) -> Result<Option<Channel>> {
        let r = sqlx::query(&format!("select {CHANNEL_COLS} from channels where id = $1"))
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(r.as_ref().map(channel_from).transpose()?)
    }

    pub async fn channel_perms(&self, uid: Id, cid: Id) -> Result<u64> {
        let Some(c) = self.channel(cid).await? else { return Ok(0) };
        let Some(gid) = c.guild_id else {
            let inside: bool = sqlx::query_scalar(
                "select exists(select 1 from channel_recipients where channel_id = $1 and user_id = $2)",
            )
            .bind(cid)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?;
            return Ok(if inside { perm::DM } else { 0 });
        };
        Ok(perm::Ctx::load(&self.pool, gid, Some(uid)).await?.map_or(0, |ctx| ctx.channel(uid, cid, c.parent_id)))
    }

    /// Qui voit ce salon : destinataires des événements du gateway.
    pub async fn channel_audience(&self, cid: Id) -> Result<Vec<Id>> {
        let Some(c) = self.channel(cid).await? else { return Ok(vec![]) };
        let Some(gid) = c.guild_id else {
            return Ok(sqlx::query_scalar("select user_id from channel_recipients where channel_id = $1")
                .bind(cid)
                .fetch_all(&self.pool)
                .await?);
        };
        let Some(ctx) = perm::Ctx::load(&self.pool, gid, None).await? else { return Ok(vec![]) };
        Ok(ctx.member_ids().filter(|u| ctx.channel(*u, cid, c.parent_id) & perms::VIEW_CHANNEL != 0).collect())
    }

    pub async fn create_channel(&self, gid: Id, req: &CreateChannel) -> Result<Channel> {
        let name = req.name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(DbError::Invalid("name"));
        }
        if matches!(req.kind, ChannelKind::Dm | ChannelKind::GroupDm | ChannelKind::Thread) {
            return Err(DbError::Invalid("kind"));
        }
        if let Some(p) = req.parent_id {
            let ok: bool = sqlx::query_scalar(
                "select exists(select 1 from channels where id = $1 and guild_id = $2 and kind = 'category')",
            )
            .bind(p)
            .bind(gid)
            .fetch_one(&self.pool)
            .await?;
            if !ok || req.kind == ChannelKind::Category {
                return Err(DbError::Invalid("parent_id"));
            }
        }
        let mut tx = self.pool.begin().await?;
        let id = insert_channel(&mut tx, gid, req.parent_id, req.kind, name).await?;
        tx.commit().await?;
        self.channel(id).await?.ok_or(DbError::NotFound)
    }

    // ───────── messages ─────────

    /// Une page de messages, du plus ancien au plus récent, avant `before` si donné.
    pub async fn messages(&self, cid: Id, before: Option<Id>, limit: i64) -> Result<Vec<Message>> {
        let rows = sqlx::query(&message_sql(
            "where m.channel_id = $1 and ($2::uuid is null or m.id < $2) order by m.id desc limit $3",
        ))
        .bind(cid)
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut v = rows.iter().map(message_from).collect::<sqlx::Result<Vec<_>>>()?;
        v.reverse();
        Ok(v)
    }

    /// Envoie un message. Idempotent sur `(auteur, nonce)` : un renvoi rend l'original
    /// avec `false`.
    pub async fn create_message(
        &self,
        cid: Id,
        author: Id,
        content: &str,
        nonce: Option<String>,
    ) -> Result<(Message, bool)> {
        self.send_message(cid, author, Draft { content, nonce, ..Default::default() }).await
    }

    /// Envoie un message, éventuellement en réponse à un autre du même salon. Les mentions
    /// `@pseudo` ne retiennent que ceux qui voient le salon.
    pub async fn send_message(&self, cid: Id, author: Id, d: Draft<'_>) -> Result<(Message, bool)> {
        let content = d.content.trim();
        if content.is_empty() || content.chars().count() > MAX_MESSAGE_LEN {
            return Err(DbError::Invalid("content"));
        }
        let Some(channel) = self.channel(cid).await?.filter(|c| c.kind.is_textual()) else {
            return Err(DbError::NotFound);
        };
        if channel.kind == ChannelKind::Dm {
            // Un blocage, dans un sens ou dans l'autre, ferme le MP (l'historique reste lisible).
            let blocked: Option<bool> = sqlx::query_scalar(
                "select bool_or(r.user_id = $2) from channel_recipients cr join relationships r
                   on r.kind = 'blocked' and ((r.user_id = $2 and r.target_id = cr.user_id)
                                           or (r.user_id = cr.user_id and r.target_id = $2))
                 where cr.channel_id = $1 and cr.user_id <> $2",
            )
            .bind(cid)
            .bind(author)
            .fetch_one(&self.pool)
            .await?;
            match blocked {
                Some(true) => return Err(DbError::Forbidden("dm_blocked")),
                Some(false) => return Err(DbError::Forbidden("dm_refused")),
                None => {}
            }
        }
        if let Some(r) = d.reply_to {
            let here: bool =
                sqlx::query_scalar("select exists(select 1 from messages where id = $1 and channel_id = $2)")
                    .bind(r)
                    .bind(cid)
                    .fetch_one(&self.pool)
                    .await?;
            if !here {
                return Err(DbError::Invalid("reply_to"));
            }
        }
        let (mentioned, everyone) = self.mentioned(cid, content).await?;
        let everyone = everyone && d.may_mention_everyone;
        let id: Option<Id> = sqlx::query_scalar(
            "with m as (insert into messages (channel_id, author_id, content, nonce, kind, reply_to_id, mention_everyone)
                        values ($1, $2, $3, $4, case when $5::uuid is null then 'default' else 'reply' end, $5, $6)
                        on conflict (author_id, channel_id, nonce) where nonce is not null do nothing
                        returning id, channel_id)
             update channels c set last_message_id = greatest(c.last_message_id, m.id) from m
             where c.id = m.channel_id returning m.id",
        )
        .bind(cid)
        .bind(author)
        .bind(content)
        .bind(&d.nonce)
        .bind(d.reply_to)
        .bind(everyone)
        .fetch_optional(&self.pool)
        .await?;
        let r = match id {
            Some(id) => {
                self.set_mentions(id, &mentioned).await?;
                self.message(id).await?.ok_or(DbError::NotFound)?
            }
            None => {
                let r = sqlx::query(&message_sql("where m.author_id = $1 and m.nonce = $2 and m.channel_id = $3"))
                    .bind(author)
                    .bind(&d.nonce)
                    .bind(cid)
                    .fetch_one(&self.pool)
                    .await?;
                message_from(&r)?
            }
        };
        Ok((r, id.is_some()))
    }

    pub async fn message(&self, mid: Id) -> Result<Option<Message>> {
        let r = sqlx::query(&message_sql("where m.id = $1")).bind(mid).fetch_optional(&self.pool).await?;
        Ok(r.as_ref().map(message_from).transpose()?)
    }

    /// Les personnes mentionnées qui voient le salon, et si `@everyone`/`@here` est écrit.
    async fn mentioned(&self, cid: Id, content: &str) -> Result<(Vec<Id>, bool)> {
        let (names, everyone) = kraken_core::markdown::mentions(content);
        if names.is_empty() {
            return Ok((vec![], everyone));
        }
        let ids: Vec<Id> = sqlx::query_scalar("select id from users where username = any($1) and deleted_at is null")
            .bind(&names)
            .fetch_all(&self.pool)
            .await?;
        let audience = self.channel_audience(cid).await?;
        Ok((ids.into_iter().filter(|u| audience.contains(u)).collect(), everyone))
    }

    async fn set_mentions(&self, mid: Id, users: &[Id]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("delete from message_mentions where message_id = $1 and user_id <> all($2)")
            .bind(mid)
            .bind(users)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "insert into message_mentions (message_id, user_id) select $1, unnest($2::uuid[]) on conflict do nothing",
        )
        .bind(mid)
        .bind(users)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Modifie le texte d'un message de ce salon. Seul son auteur le peut (`Forbidden` sinon).
    pub async fn edit_message(
        &self,
        cid: Id,
        mid: Id,
        editor: Id,
        content: &str,
        may_mention_everyone: bool,
    ) -> Result<Message> {
        let content = content.trim();
        if content.is_empty() || content.chars().count() > MAX_MESSAGE_LEN {
            return Err(DbError::Invalid("content"));
        }
        let author: Option<Option<Id>> =
            sqlx::query_scalar("select author_id from messages where id = $1 and channel_id = $2")
                .bind(mid)
                .bind(cid)
                .fetch_optional(&self.pool)
                .await?;
        match author {
            None => return Err(DbError::NotFound),
            Some(a) if a != Some(editor) => return Err(DbError::Forbidden("author")),
            Some(_) => {}
        }
        let (mentioned, everyone) = self.mentioned(cid, content).await?;
        let done = sqlx::query(
            "update messages set content = $3, edited_at = now(), mention_everyone = $4
             where id = $1 and author_id = $2 and kind in ('default', 'reply') and content <> $3",
        )
        .bind(mid)
        .bind(editor)
        .bind(content)
        .bind(everyone && may_mention_everyone)
        .execute(&self.pool)
        .await?;
        if done.rows_affected() > 0 {
            self.set_mentions(mid, &mentioned).await?;
        }
        self.message(mid).await?.ok_or(DbError::NotFound)
    }

    /// Supprime un message de ce salon : son auteur, ou quelqu'un qui gère les messages.
    pub async fn delete_message(&self, cid: Id, mid: Id, uid: Id, can_manage: bool) -> Result<()> {
        let author: Option<Option<Id>> =
            sqlx::query_scalar("select author_id from messages where id = $1 and channel_id = $2")
                .bind(mid)
                .bind(cid)
                .fetch_optional(&self.pool)
                .await?;
        match author {
            None => return Err(DbError::NotFound),
            Some(a) if a != Some(uid) && !can_manage => return Err(DbError::Forbidden("author")),
            Some(_) => {}
        }
        sqlx::query("delete from messages where id = $1 and channel_id = $2")
            .bind(mid)
            .bind(cid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Ajoute ma réaction ; `false` si elle y était déjà. Au plus
    /// [`kraken_core::emoji::MAX_REACTIONS`] emojis différents par message.
    pub async fn add_reaction(&self, cid: Id, mid: Id, uid: Id, emoji: &str) -> Result<Changed> {
        if !kraken_core::emoji::valid(emoji) {
            return Err(DbError::Invalid("emoji"));
        }
        let mut tx = self.pool.begin().await?;
        // Verrou sur le message : deux nouvelles réactions simultanées ne dépassent pas la limite.
        sqlx::query("select 1 from messages where id = $1 and channel_id = $2 for update")
            .bind(mid)
            .bind(cid)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        let (kinds, known): (i64, bool) = sqlx::query_as(
            "select count(distinct emoji_key), coalesce(bool_or(emoji_key = $2), false) from reactions where message_id = $1",
        )
        .bind(mid)
        .bind(emoji)
        .fetch_one(&mut *tx)
        .await?;
        if !known && kinds as usize >= kraken_core::emoji::MAX_REACTIONS {
            return Err(DbError::Invalid("reactions"));
        }
        let added = sqlx::query(
            "insert into reactions (message_id, user_id, emoji_key) values ($1, $2, $3) on conflict do nothing",
        )
        .bind(mid)
        .bind(uid)
        .bind(emoji)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        tx.commit().await?;
        Ok(added)
    }

    /// Retire ma réaction ; `false` s'il n'y en avait pas.
    pub async fn remove_reaction(&self, cid: Id, mid: Id, uid: Id, emoji: &str) -> Result<Changed> {
        let here: bool = sqlx::query_scalar("select exists(select 1 from messages where id = $1 and channel_id = $2)")
            .bind(mid)
            .bind(cid)
            .fetch_one(&self.pool)
            .await?;
        if !here {
            return Err(DbError::NotFound);
        }
        Ok(sqlx::query("delete from reactions where message_id = $1 and user_id = $2 and emoji_key = $3 and not burst")
            .bind(mid)
            .bind(uid)
            .bind(emoji)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    // ───────── invitations ─────────

    pub async fn create_invite(
        &self,
        gid: Id,
        cid: Option<Id>,
        inviter: Id,
        max_uses: u32,
        max_age_s: Option<u32>,
    ) -> Result<Invite> {
        let code: String = rand::rng().sample_iter(rand::distr::Alphanumeric).take(8).map(char::from).collect();
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
        .execute(&self.pool)
        .await?;
        self.invite(&code).await?.ok_or(DbError::NotFound)
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

    /// Rejoint le serveur. Le compteur d'usages ne bouge pas pour un membre déjà présent ;
    /// l'incrément est conditionnel, donc deux adhésions simultanées ne dépassent pas
    /// `max_uses`. Rend l'état du serveur et le nouveau membre (`None` s'il l'était déjà).
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
            sqlx::query(
                "update invites set uses = uses + 1 where code = $1
                   and (max_uses = 0 or uses < max_uses) and (expires_at is null or expires_at > now())
                 returning guild_id",
            )
            .bind(code)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(DbError::NotFound)?;
        }
        tx.commit().await?;
        let state = self.guild_state(gid, uid).await?.ok_or(DbError::NotFound)?;
        let new = if already { None } else { state.members.iter().find(|m| m.user.id == uid).cloned() };
        Ok((state, new))
    }

    // ───────── social ─────────

    pub async fn relationships(&self, uid: Id) -> Result<Vec<Relationship>> {
        let rows = sqlx::query(&format!(
            "select {}, r.kind from relationships r join users u on u.id = r.target_id
             where r.user_id = $1 and r.kind <> 'ignored' order by r.created_at",
            user_cols("u")
        ))
        .bind(uid)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                let kind = match r.try_get::<&str, _>("kind")? {
                    "friend" => RelationshipKind::Friend,
                    "blocked" => RelationshipKind::Blocked,
                    "pending_in" => RelationshipKind::PendingIncoming,
                    _ => RelationshipKind::PendingOutgoing,
                };
                Ok(Relationship { user: user_from(r, "u")?.expect("join"), kind, status: Status::Offline })
            })
            .collect::<sqlx::Result<_>>()
            .map_err(Into::into)
    }
}

pub(crate) async fn insert_channel(
    tx: &mut Transaction<'_, Postgres>,
    gid: Id,
    parent: Option<Id>,
    kind: ChannelKind,
    name: &str,
) -> Result<Id> {
    Ok(sqlx::query_scalar(
        "insert into channels (guild_id, parent_id, kind, name, position)
         values ($1, $2, $3, $4, (select count(*) from channels where guild_id = $1 and parent_id is not distinct from $2))
         returning id",
    )
    .bind(gid)
    .bind(parent)
    .bind(kind_str(kind))
    .bind(name)
    .fetch_one(&mut **tx)
    .await?)
}

#[cfg(test)]
mod tests;
