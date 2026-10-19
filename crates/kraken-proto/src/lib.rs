//! Types échangés entre `kraken-server` et `kraken-client` (REST `/api/v1` et gateway).
//! Les dates sont en millisecondes Unix UTC ; le client les affiche en heure locale.

use serde::{Deserialize, Serialize};
pub use uuid::Uuid as Id;

pub const MAX_MESSAGE_LEN: usize = 4000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: Id,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
}

impl User {
    pub fn name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.username)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginReq {
    /// Pseudo ou e-mail.
    pub login: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailReq {
    pub email: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenReq {
    pub token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResetConfirmReq {
    pub token: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PasswordReq {
    pub password: String,
}

/// Modifier son compte. `display_name` vide ou absent : on revient au pseudo unique.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UpdateMe {
    pub display_name: Option<String>,
}

/// Longueur maximale du nom affiché, en caractères.
pub const MAX_DISPLAY_NAME: usize = 32;

/// Ce que l'instance dit d'elle-même sur ses pages de confidentialité et de conditions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Instance {
    /// Comment joindre l'exploitant (e-mail ou page).
    pub contact: Option<String>,
    /// Hébergeur, fournisseur d'e-mails… tels que l'exploitant les déclare.
    pub processors: Option<String>,
    /// Code source de la version en service (AGPL : offert à chaque utilisateur).
    pub source_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthResp {
    pub token: String,
    pub user: User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Text,
    Voice,
    Announcement,
    Category,
    Forum,
    Stage,
    Dm,
    GroupDm,
    Thread,
}

impl ChannelKind {
    /// Salons où l'on écrit des messages.
    pub fn is_textual(self) -> bool {
        matches!(self, Self::Text | Self::Announcement | Self::Dm | Self::GroupDm | Self::Thread)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: Id,
    pub guild_id: Option<Id>,
    pub parent_id: Option<Id>,
    pub kind: ChannelKind,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub position: i32,
    /// MP et groupes : les autres participants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipients: Vec<User>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guild {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub owner_id: Id,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Role {
    pub id: Id,
    pub name: String,
    pub color: Option<u32>,
    pub position: i32,
    pub permissions: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub user: User,
    pub nick: Option<String>,
    pub roles: Vec<Id>,
    pub joined_at: i64,
}

/// Un serveur avec ce qu'il faut pour l'afficher (READY, GUILD_CREATE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuildState {
    pub guild: Guild,
    pub channels: Vec<Channel>,
    pub roles: Vec<Role>,
    pub members: Vec<Member>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateGuild {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateChannel {
    pub name: String,
    pub kind: ChannelKind,
    pub parent_id: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    /// `None` : compte supprimé.
    pub author: Option<User>,
    pub content: String,
    pub nonce: Option<String>,
    pub created_at: i64,
    pub edited_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMessage {
    pub content: String,
    pub nonce: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CreateInvite {
    /// Secondes ; `None` : n'expire jamais.
    pub max_age: Option<u32>,
    /// 0 ou `None` : illimité.
    pub max_uses: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    pub code: String,
    pub guild: Guild,
    pub channel_id: Option<Id>,
    pub member_count: u32,
    pub uses: u32,
    pub max_uses: u32,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Friend,
    Blocked,
    PendingIncoming,
    PendingOutgoing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Online,
    Idle,
    Dnd,
    Offline,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub user: User,
    pub kind: RelationshipKind,
    pub status: Status,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

// ───────────── gateway ─────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ready {
    pub user: User,
    pub guilds: Vec<GuildState>,
    pub private_channels: Vec<Channel>,
    pub relationships: Vec<Relationship>,
    pub session_id: String,
}

/// Client → serveur.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientOp {
    /// `token` : jeton de session pour les clients natifs et les bots ; le client web
    /// s'en passe, le cookie de session suffit.
    Identify {
        token: Option<String>,
    },
    Heartbeat {
        seq: u64,
    },
}

/// Serveur → client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerOp {
    Hello { heartbeat_ms: u32 },
    Ready(Box<Ready>),
    Dispatch { seq: u64, event: Event },
    HeartbeatAck,
    InvalidSession,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    MessageCreate(Message),
    ChannelCreate(Channel),
    GuildCreate(Box<GuildState>),
    MemberAdd {
        guild_id: Id,
        member: Member,
    },
    /// Nom affiché ou avatar changé : envoyé à l'intéressé et à ceux qui le voient.
    UserUpdate(User),
}
