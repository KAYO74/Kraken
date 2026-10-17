//! Couche données de KRAKEN.
//!
//! Implémentation **en mémoire** pour la tranche verticale : mêmes signatures que la future
//! version sqlx/Postgres (`replica/schema.sql`), pour que `/replica-backend` la remplace sans
//! toucher au serveur ni au client. Les contraintes du schéma sont reproduites ici
//! (pseudo unique, `(author, nonce)` unique, `uses <= max_uses`, longueurs).

mod seed;

use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use kraken_core::perms::{self, Overwrite, Target};
use kraken_proto::*;
use rand::Rng;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    /// Violation d'unicité (nom du champ).
    Conflict(&'static str),
    NotFound,
    /// Violation d'une contrainte `check` (nom du champ).
    Invalid(&'static str),
}

pub type Result<T> = std::result::Result<T, DbError>;

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn new_id() -> Id {
    Id::now_v7()
}

pub fn valid_username(u: &str) -> bool {
    (2..=32).contains(&u.len())
        && u.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

struct UserRow {
    user: User,
    email: String,
    password_hash: String,
}

struct MemberRow {
    nick: Option<String>,
    roles: Vec<Id>,
    joined_at: i64,
}

struct InviteRow {
    guild_id: Id,
    channel_id: Option<Id>,
    max_uses: u32,
    uses: u32,
    expires_at: Option<i64>,
}

#[derive(Default)]
struct State {
    users: BTreeMap<Id, UserRow>,
    sessions: HashMap<String, Id>,
    guilds: BTreeMap<Id, Guild>,
    roles: BTreeMap<Id, (Id, Role)>,
    channels: BTreeMap<Id, Channel>,
    members: BTreeMap<(Id, Id), MemberRow>,
    /// Par salon, triés par id (donc par date).
    messages: HashMap<Id, Vec<Message>>,
    nonces: HashMap<(Id, String), (Id, Id)>,
    invites: HashMap<String, InviteRow>,
    relationships: HashMap<(Id, Id), RelationshipKind>,
    recipients: HashMap<Id, Vec<Id>>,
    overwrites: HashMap<Id, Vec<Overwrite>>,
}

#[derive(Default)]
pub struct Db {
    s: RwLock<State>,
}

impl State {
    fn user(&self, id: Id) -> Option<User> {
        self.users.get(&id).map(|r| r.user.clone())
    }

    fn guild_state(&self, gid: Id) -> Option<GuildState> {
        let guild = self.guilds.get(&gid)?.clone();
        let channels = self.channels.values().filter(|c| c.guild_id == Some(gid)).cloned().collect();
        let roles = self.roles.values().filter(|(g, _)| *g == gid).map(|(_, r)| r.clone()).collect();
        let members = self
            .members
            .range((gid, Id::nil())..=(gid, Id::max()))
            .filter_map(|((_, uid), m)| self.member(*uid, m))
            .collect();
        Some(GuildState { guild, channels, roles, members })
    }

    fn member(&self, uid: Id, m: &MemberRow) -> Option<Member> {
        Some(Member { user: self.user(uid)?, nick: m.nick.clone(), roles: m.roles.clone(), joined_at: m.joined_at })
    }

    fn guild_perms(&self, uid: Id, gid: Id) -> u64 {
        let (Some(g), Some(m)) = (self.guilds.get(&gid), self.members.get(&(gid, uid))) else { return 0 };
        let role = |id: &Id| self.roles.get(id).map_or(0, |(_, r)| r.permissions);
        perms::base(g.owner_id == uid, role(&gid), m.roles.iter().map(role))
    }

    fn channel_perms(&self, uid: Id, cid: Id) -> u64 {
        let Some(c) = self.channels.get(&cid) else { return 0 };
        let Some(gid) = c.guild_id else {
            let inside = self.recipients.get(&cid).is_some_and(|r| r.contains(&uid));
            return if inside {
                perms::VIEW_CHANNEL
                    | perms::SEND_MESSAGES
                    | perms::READ_HISTORY
                    | perms::ADD_REACTIONS
                    | perms::ATTACH_FILES
                    | perms::EMBED_LINKS
                    | perms::CONNECT
                    | perms::SPEAK
                    | perms::VIDEO
                    | perms::STREAM
            } else {
                0
            };
        };
        let base = self.guild_perms(uid, gid);
        if base == 0 {
            return 0;
        }
        let roles = self.members.get(&(gid, uid)).map(|m| m.roles.as_slice()).unwrap_or(&[]);
        let mut ow: Vec<Overwrite> = Vec::new();
        // Les surcharges de la catégorie s'appliquent d'abord (salon synchronisé).
        if let Some(p) = c.parent_id.and_then(|p| self.overwrites.get(&p)) {
            ow.extend(p);
        }
        ow.extend(self.overwrites.get(&cid).into_iter().flatten());
        perms::in_channel(base, gid, uid, roles, &ow)
    }

    fn invite(&self, code: &str) -> Option<Invite> {
        let r = self.invites.get(code)?;
        if r.expires_at.is_some_and(|e| e <= now_ms()) || (r.max_uses > 0 && r.uses >= r.max_uses) {
            return None;
        }
        let guild = self.guilds.get(&r.guild_id)?.clone();
        let member_count = self.members.range((guild.id, Id::nil())..=(guild.id, Id::max())).count() as u32;
        Some(Invite {
            code: code.to_owned(),
            guild,
            channel_id: r.channel_id,
            member_count,
            uses: r.uses,
            max_uses: r.max_uses,
            expires_at: r.expires_at,
        })
    }

    fn insert_channel(&mut self, gid: Option<Id>, parent: Option<Id>, kind: ChannelKind, name: &str) -> Channel {
        let position = self.channels.values().filter(|c| c.guild_id == gid && c.parent_id == parent).count() as i32;
        let c = Channel {
            id: new_id(),
            guild_id: gid,
            parent_id: parent,
            kind,
            name: Some(name.to_owned()),
            topic: None,
            position,
            recipients: vec![],
        };
        self.channels.insert(c.id, c.clone());
        c
    }

    fn insert_message(&mut self, cid: Id, author: Id, content: &str, nonce: Option<String>) -> Message {
        let m = Message {
            id: new_id(),
            channel_id: cid,
            author: self.user(author),
            content: content.to_owned(),
            nonce: nonce.clone(),
            created_at: now_ms(),
            edited_at: None,
        };
        if let Some(n) = nonce {
            self.nonces.insert((author, n), (cid, m.id));
        }
        self.messages.entry(cid).or_default().push(m.clone());
        m
    }
}

impl Db {
    pub fn new() -> Self {
        Self::default()
    }

    // ───────── comptes ─────────

    pub async fn create_user(&self, username: &str, email: &str, password_hash: String) -> Result<User> {
        if !valid_username(username) {
            return Err(DbError::Invalid("username"));
        }
        let mut s = self.s.write().unwrap();
        let email_l = email.to_lowercase();
        if s.users.values().any(|r| r.user.username == username) {
            return Err(DbError::Conflict("username"));
        }
        if s.users.values().any(|r| r.email == email_l) {
            return Err(DbError::Conflict("email"));
        }
        let user = User { id: new_id(), username: username.to_owned(), display_name: None, avatar: None };
        s.users.insert(user.id, UserRow { user: user.clone(), email: email_l, password_hash });
        Ok(user)
    }

    /// Utilisateur et hash du mot de passe, par pseudo ou e-mail.
    pub async fn user_credentials(&self, login: &str) -> Option<(User, String)> {
        let l = login.trim().to_lowercase();
        let s = self.s.read().unwrap();
        s.users
            .values()
            .find(|r| r.user.username == l || r.email == l)
            .map(|r| (r.user.clone(), r.password_hash.clone()))
    }

    pub async fn create_session(&self, user_id: Id, token_hash: String) {
        self.s.write().unwrap().sessions.insert(token_hash, user_id);
    }

    pub async fn session_user(&self, token_hash: &str) -> Option<User> {
        let s = self.s.read().unwrap();
        s.sessions.get(token_hash).and_then(|id| s.user(*id))
    }

    pub async fn delete_session(&self, token_hash: &str) {
        self.s.write().unwrap().sessions.remove(token_hash);
    }

    // ───────── serveurs ─────────

    /// Crée le serveur, son rôle `@everyone` (id = id du serveur), deux catégories, `#général`
    /// et un salon vocal, et fait du créateur le premier membre.
    pub async fn create_guild(&self, owner: Id, name: &str) -> Result<GuildState> {
        let name = name.trim();
        if !(2..=100).contains(&name.chars().count()) {
            return Err(DbError::Invalid("name"));
        }
        let mut s = self.s.write().unwrap();
        let gid = new_id();
        s.guilds.insert(gid, Guild { id: gid, name: name.to_owned(), icon: None, owner_id: owner });
        let everyone =
            Role { id: gid, name: "@everyone".into(), color: None, position: 0, permissions: perms::DEFAULT_EVERYONE };
        s.roles.insert(gid, (gid, everyone));
        let text = s.insert_channel(Some(gid), None, ChannelKind::Category, "Salons textuels");
        s.insert_channel(Some(gid), Some(text.id), ChannelKind::Text, "général");
        let voice = s.insert_channel(Some(gid), None, ChannelKind::Category, "Salons vocaux");
        s.insert_channel(Some(gid), Some(voice.id), ChannelKind::Voice, "Général");
        s.members.insert((gid, owner), MemberRow { nick: None, roles: vec![], joined_at: now_ms() });
        Ok(s.guild_state(gid).expect("just inserted"))
    }

    pub async fn guild_state(&self, gid: Id) -> Option<GuildState> {
        self.s.read().unwrap().guild_state(gid)
    }

    pub async fn user_guilds(&self, uid: Id) -> Vec<GuildState> {
        let s = self.s.read().unwrap();
        let ids: Vec<Id> = s.members.keys().filter(|(_, u)| *u == uid).map(|(g, _)| *g).collect();
        ids.into_iter().filter_map(|g| s.guild_state(g)).collect()
    }

    pub async fn guild_perms(&self, uid: Id, gid: Id) -> u64 {
        self.s.read().unwrap().guild_perms(uid, gid)
    }

    // ───────── salons ─────────

    pub async fn channel(&self, cid: Id) -> Option<Channel> {
        self.s.read().unwrap().channels.get(&cid).cloned()
    }

    pub async fn channel_perms(&self, uid: Id, cid: Id) -> u64 {
        self.s.read().unwrap().channel_perms(uid, cid)
    }

    /// Qui voit ce salon : destinataires des événements du gateway.
    pub async fn channel_audience(&self, cid: Id) -> Vec<Id> {
        let s = self.s.read().unwrap();
        let Some(c) = s.channels.get(&cid) else { return vec![] };
        let candidates: Vec<Id> = match c.guild_id {
            Some(g) => s.members.range((g, Id::nil())..=(g, Id::max())).map(|((_, u), _)| *u).collect(),
            None => s.recipients.get(&cid).cloned().unwrap_or_default(),
        };
        candidates.into_iter().filter(|u| s.channel_perms(*u, cid) & perms::VIEW_CHANNEL != 0).collect()
    }

    pub async fn create_channel(&self, gid: Id, req: &CreateChannel) -> Result<Channel> {
        let name = req.name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(DbError::Invalid("name"));
        }
        if matches!(req.kind, ChannelKind::Dm | ChannelKind::GroupDm | ChannelKind::Thread) {
            return Err(DbError::Invalid("kind"));
        }
        let mut s = self.s.write().unwrap();
        if !s.guilds.contains_key(&gid) {
            return Err(DbError::NotFound);
        }
        if let Some(p) = req.parent_id {
            let ok = s.channels.get(&p).is_some_and(|c| c.guild_id == Some(gid) && c.kind == ChannelKind::Category);
            if !ok || req.kind == ChannelKind::Category {
                return Err(DbError::Invalid("parent_id"));
            }
        }
        Ok(s.insert_channel(Some(gid), req.parent_id, req.kind, name))
    }

    pub async fn private_channels(&self, uid: Id) -> Vec<Channel> {
        let s = self.s.read().unwrap();
        s.recipients
            .iter()
            .filter(|(_, r)| r.contains(&uid))
            .filter_map(|(cid, r)| {
                let mut c = s.channels.get(cid)?.clone();
                c.recipients = r.iter().filter(|u| **u != uid).filter_map(|u| s.user(*u)).collect();
                Some(c)
            })
            .collect()
    }

    // ───────── messages ─────────

    /// Une page de messages, du plus ancien au plus récent, avant `before` si donné.
    pub async fn messages(&self, cid: Id, before: Option<Id>, limit: usize) -> Vec<Message> {
        let s = self.s.read().unwrap();
        let all = s.messages.get(&cid).map(Vec::as_slice).unwrap_or(&[]);
        let end = before.map_or(all.len(), |b| all.partition_point(|m| m.id < b));
        all[end.saturating_sub(limit)..end].to_vec()
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
        let content = content.trim();
        if content.is_empty() || content.chars().count() > MAX_MESSAGE_LEN {
            return Err(DbError::Invalid("content"));
        }
        let mut s = self.s.write().unwrap();
        if !s.channels.get(&cid).is_some_and(|c| c.kind.is_textual()) {
            return Err(DbError::NotFound);
        }
        if let Some(n) = &nonce
            && let Some((c, mid)) = s.nonces.get(&(author, n.clone()))
        {
            let m = s.messages.get(c).and_then(|v| v.iter().find(|m| m.id == *mid)).cloned();
            return m.map(|m| (m, false)).ok_or(DbError::Conflict("nonce"));
        }
        Ok((s.insert_message(cid, author, content, nonce), true))
    }

    // ───────── invitations ─────────

    pub async fn create_invite(
        &self,
        gid: Id,
        cid: Option<Id>,
        _inviter: Id,
        max_uses: u32,
        max_age_s: Option<u32>,
    ) -> Result<Invite> {
        let mut s = self.s.write().unwrap();
        if !s.guilds.contains_key(&gid) {
            return Err(DbError::NotFound);
        }
        let code: String = rand::rng().sample_iter(rand::distr::Alphanumeric).take(8).map(char::from).collect();
        let expires_at = max_age_s.filter(|a| *a > 0).map(|a| now_ms() + i64::from(a) * 1000);
        s.invites.insert(code.clone(), InviteRow { guild_id: gid, channel_id: cid, max_uses, uses: 0, expires_at });
        s.invite(&code).ok_or(DbError::NotFound)
    }

    /// Aperçu ; `None` si inconnue, expirée ou épuisée.
    pub async fn invite(&self, code: &str) -> Option<Invite> {
        self.s.read().unwrap().invite(code)
    }

    /// Rejoint le serveur. Le compteur d'usages ne bouge pas pour un membre déjà présent.
    /// Rend l'état du serveur et le nouveau membre (`None` s'il l'était déjà).
    pub async fn use_invite(&self, code: &str, uid: Id) -> Result<(GuildState, Option<Member>)> {
        let mut s = self.s.write().unwrap();
        let inv = s.invite(code).ok_or(DbError::NotFound)?;
        let gid = inv.guild.id;
        let mut new = None;
        if !s.members.contains_key(&(gid, uid)) {
            s.invites.get_mut(code).expect("checked").uses += 1;
            let row = MemberRow { nick: None, roles: vec![], joined_at: now_ms() };
            new = s.member(uid, &row);
            s.members.insert((gid, uid), row);
        }
        Ok((s.guild_state(gid).ok_or(DbError::NotFound)?, new))
    }

    // ───────── social ─────────

    pub async fn relationships(&self, uid: Id) -> Vec<Relationship> {
        let s = self.s.read().unwrap();
        s.relationships
            .iter()
            .filter(|((a, _), _)| *a == uid)
            .filter_map(|((_, b), k)| Some(Relationship { user: s.user(*b)?, kind: *k, status: Status::Offline }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block<F: std::future::Future>(f: F) -> F::Output {
        // Les méthodes ne suspendent jamais : un seul poll suffit.
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => unreachable!(),
        }
    }

    #[test]
    fn slice_rules() {
        let db = Db::new();
        let a = block(db.create_user("lou", "lou@example.org", "h".into())).unwrap();
        let b = block(db.create_user("sam", "sam@example.org", "h".into())).unwrap();
        assert_eq!(block(db.create_user("lou", "x@example.org", "h".into())), Err(DbError::Conflict("username")));
        assert_eq!(block(db.create_user("Lou!", "y@example.org", "h".into())), Err(DbError::Invalid("username")));

        let g = block(db.create_guild(a.id, "Abysse")).unwrap();
        let general = g.channels.iter().find(|c| c.kind == ChannelKind::Text).unwrap().id;
        assert_eq!(block(db.channel_perms(b.id, general)), 0);

        let inv = block(db.create_invite(g.guild.id, Some(general), a.id, 1, None)).unwrap();
        let (_, m) = block(db.use_invite(&inv.code, b.id)).unwrap();
        assert!(m.is_some());
        assert!(block(db.invite(&inv.code)).is_none(), "max_uses atteint");
        assert_ne!(block(db.channel_perms(b.id, general)) & perms::SEND_MESSAGES, 0);
        assert_eq!(block(db.channel_audience(general)).len(), 2);

        let (m1, new1) = block(db.create_message(general, b.id, "salut", Some("n1".into()))).unwrap();
        let (m2, new2) = block(db.create_message(general, b.id, "salut", Some("n1".into()))).unwrap();
        assert!(new1 && !new2);
        assert_eq!(m1.id, m2.id);
        assert_eq!(block(db.messages(general, None, 50)).len(), 1);
        assert!(block(db.create_message(general, b.id, &"x".repeat(4001), None)).is_err());
    }
}
