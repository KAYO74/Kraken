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
    /// Clé de la photo de profil (`/api/v1/avatars/{clé}`).
    pub avatar: Option<String>,
    /// Clé de la bannière (`/api/v1/banners/{clé}`). Les champs qui suivent sont facultatifs : un
    /// ancien client ou un ancien serveur qui ne les connaît pas reste compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    /// Présentation, [`MAX_BIO`] caractères au plus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// Pronoms, [`MAX_PRONOUNS`] caractères au plus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pronouns: Option<String>,
    /// Création du compte (ms Unix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
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

/// `PATCH /users/@me` : modifier son profil. Un champ absent reste inchangé ; vide, il est
/// effacé (`display_name` : retour au pseudo unique).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UpdateMe {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pronouns: Option<String>,
}

/// Longueur maximale du nom affiché, en caractères.
pub const MAX_DISPLAY_NAME: usize = 32;
/// Longueur maximale de la bio, en caractères (retours à la ligne compris).
pub const MAX_BIO: usize = 190;
/// Longueur maximale des pronoms, en caractères.
pub const MAX_PRONOUNS: usize = 40;

/// `PUT /users/@me/username` : changer son pseudo unique. Le mot de passe actuel est exigé.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangeUsername {
    pub username: String,
    pub password: String,
}

/// `PUT /users/@me/email` : changer son adresse e-mail. Le mot de passe actuel est exigé.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangeEmail {
    pub email: String,
    pub password: String,
}

/// `PUT /users/@me/password` : changer son mot de passe.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangePassword {
    pub current: String,
    pub new: String,
}

/// `GET /users/@me/account` : ce qui est privé dans mon compte (jamais envoyé aux autres).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub email: Option<String>,
    pub email_verified: bool,
    /// Nouvelle adresse choisie mais pas encore confirmée : l'ancienne reste valable d'ici là.
    pub pending_email: Option<String>,
    /// Cette instance envoie des e-mails : un changement d'adresse doit alors être confirmé.
    pub mail: bool,
}

/// Un pseudo peut changer 2 fois par heure.
pub const USERNAME_CHANGES_PER_WINDOW: u32 = 2;
/// La fenêtre de [`USERNAME_CHANGES_PER_WINDOW`], en secondes.
pub const USERNAME_WINDOW_S: u32 = 3600;

/// Ce que l'instance dit d'elle-même sur ses pages de confidentialité et de conditions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Instance {
    /// Comment joindre l'exploitant (e-mail ou page).
    pub contact: Option<String>,
    /// Hébergeur, fournisseur d'e-mails… tels que l'exploitant les déclare.
    pub processors: Option<String>,
    /// Code source de la version en service (AGPL : offert à chaque utilisateur).
    pub source_url: String,
    /// Adresse publique de l'instance, sans `/` final : celle des liens d'invitation, quelle que
    /// soit l'adresse par laquelle la page a été ouverte. Vide si le serveur ne la donne pas.
    #[serde(default)]
    pub public_url: String,
    /// Taille au plus d'un fichier joint, en octets (0 : réponse d'un ancien serveur).
    #[serde(default)]
    pub max_upload_bytes: u64,
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
    /// Le message le plus récent (non-lus : comparé à [`ReadState::last_read`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_id: Option<Id>,
    /// Groupes : son propriétaire, seul à pouvoir en retirer quelqu'un.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<Id>,
    /// Mode lent : secondes à attendre entre deux messages (0 : aucun). Sans effet sur ceux qui
    /// gèrent les messages ou les salons.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub slowmode_s: u32,
    /// Salon privé : `@everyone` ne le voit pas (lui-même, ou sa catégorie).
    #[serde(default, skip_serializing_if = "is_false")]
    pub private: bool,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn is_false(b: &bool) -> bool {
    !*b
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
    /// Numéro (1 à [`ROLE_COLORS`]) dans la palette fixe du thème, pas une valeur RGB : la
    /// couleur suit le thème clair ou sombre. `None` : pas de couleur.
    pub color: Option<u32>,
    /// Plus haut, plus puissant ; `@everyone` (id = id du serveur) est à 0.
    pub position: i32,
    pub permissions: u64,
    /// Ce rôle a sa propre rubrique dans la liste des membres.
    #[serde(default)]
    pub hoist: bool,
}

/// Nombre de couleurs de la palette des rôles.
pub const ROLE_COLORS: u32 = 8;
/// Au plus ce nombre de rôles (sans `@everyone`) par serveur.
pub const MAX_ROLES: usize = 100;
/// Raison d'une exclusion ou d'un bannissement : au plus ce nombre de caractères.
pub const MAX_REASON: usize = 512;
/// Un bannissement efface au plus les messages de cette durée (7 jours).
pub const MAX_PURGE_HOURS: u32 = 168;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRole {
    pub name: String,
    pub color: Option<u32>,
    #[serde(default)]
    pub hoist: bool,
    #[serde(default)]
    pub permissions: u64,
}

/// Champs absents : inchangés. `color` à 0 : retirer la couleur.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EditRole {
    pub name: Option<String>,
    pub color: Option<u32>,
    pub hoist: Option<bool>,
    pub permissions: Option<u64>,
}

/// Les rôles (sans `@everyone`) du plus haut au plus bas.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReorderRoles {
    pub ids: Vec<Id>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BanMember {
    pub reason: Option<String>,
    /// Efface aussi ses messages des dernières heures (0 à [`MAX_PURGE_HOURS`]).
    pub delete_hours: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferOwner {
    pub user_id: Id,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ban {
    pub user: User,
    pub reason: Option<String>,
    pub moderator_id: Option<Id>,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub user: User,
    pub nick: Option<String>,
    pub roles: Vec<Id>,
    pub joined_at: i64,
    /// Exclusion temporaire : jusqu'à quand (ms Unix) ; `None` s'il n'y en a pas ou si elle est finie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_until: Option<i64>,
}

/// Un serveur avec ce qu'il faut pour l'afficher (READY, GUILD_CREATE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuildState {
    pub guild: Guild,
    pub channels: Vec<Channel>,
    pub roles: Vec<Role>,
    pub members: Vec<Member>,
    /// Qui est dans les salons vocaux (ceux que ce membre voit).
    #[serde(default)]
    pub voice_states: Vec<VoiceState>,
    /// Les emojis perso du serveur, du plus ancien au plus récent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emojis: Vec<Emoji>,
}

/// Emplacements d'emojis perso au plus par serveur.
pub const MAX_EMOJIS: usize = 50;
/// Poids maximal d'un emoji envoyé, en octets (256 Kio).
pub const MAX_EMOJI_BYTES: usize = 256 * 1024;
/// Côté, en pixels, d'un emoji fixe tel que le serveur le garde (le client recadre en carré à cette
/// taille ; une image plus petite n'est pas agrandie).
pub const EMOJI_SIZE: u32 = 128;
/// Plus petit côté, en pixels, de l'image d'un emoji (fixe ou animée) : en dessous, il ne reste qu'un point.
pub const MIN_EMOJI_SIDE: u32 = 8;
/// Rapport grand côté / petit côté de l'image d'un emoji, fixe ou animée, au plus (8 pour 1) : une bande très
/// allongée n'a pas de sens dans un carré. Les animations sont encore plus serrées, voir [`MAX_EMOJI_RATIO`].
pub const MAX_EMOJI_IMAGE_RATIO: u32 = 8;
/// Une animation d'emoji tient dans un rectangle de ce rapport au plus (2 pour 1, dans un sens ou
/// dans l'autre) : elle n'est pas recadrée, l'emoji est affiché en entier dans un carré.
pub const MAX_EMOJI_RATIO: u32 = 2;

/// Qui a ajouté un emoji (« ajouté par … »).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmojiCreator {
    pub id: Id,
    /// Son nom affiché, ou son pseudo.
    pub name: String,
}

/// Un emoji perso. L'image se lit sous `/api/v1/emojis/{id}/image` ; elle ne change jamais (renommer
/// ne la touche pas), d'où un cache sans fin. Dans un message, il s'écrit `<:nom:id>` : l'id ne change
/// pas au renommage et survit à la suppression.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Emoji {
    pub id: Id,
    pub guild_id: Id,
    /// 2 à 32 caractères parmi les lettres, les chiffres et `_`, unique dans le serveur sans tenir
    /// compte de la casse.
    pub name: String,
    pub animated: bool,
    /// `None` : son compte n'existe plus.
    pub creator: Option<EmojiCreator>,
}

/// `PATCH /guilds/{id}/emojis/{eid}` : renommer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenameEmoji {
    pub name: String,
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

/// Au plus ce nombre de salons (catégories comprises) par serveur.
pub const MAX_CHANNELS: usize = 500;
/// Longueur maximale du nom d'un salon, en caractères.
pub const MAX_CHANNEL_NAME: usize = 100;
/// Longueur maximale du sujet d'un salon, en caractères.
pub const MAX_TOPIC: usize = 1024;
/// Délais possibles du mode lent, en secondes : aucun, 5 s, 10 s, 30 s, 1 min, 5 min, 15 min, 1 h.
pub const SLOWMODE_STEPS: [u32; 8] = [0, 5, 10, 30, 60, 300, 900, 3600];
/// Au plus ce nombre de surcharges de droits par salon.
pub const MAX_OVERWRITES: usize = 100;
/// Une invitation dure 7 jours au plus (ou n'expire jamais), en secondes.
pub const MAX_INVITE_AGE_S: u32 = 7 * 24 * 3600;
/// Une invitation sert 1000 fois au plus (0 : autant qu'on veut).
pub const MAX_INVITE_USES: u32 = 1000;
/// Jours pendant lesquels une invitation expirée ou épuisée reste dans la liste du serveur.
pub const INVITE_KEEP_DAYS: i32 = 7;
/// Au plus ce nombre d'invitations actives (ni expirées, ni épuisées) par personne et par serveur.
pub const MAX_INVITES_PER_MEMBER: usize = 50;
/// La liste des invitations d'un serveur ne rend jamais plus de lignes que cela (les actives d'abord).
pub const MAX_INVITE_ROWS: usize = 200;
/// Une exclusion temporaire dure de 60 secondes à 28 jours.
pub const MIN_TIMEOUT_S: u32 = 60;
pub const MAX_TIMEOUT_S: u32 = 28 * 24 * 3600;

/// `PUT /channels/{id}/position` : place le salon dans ce parent (`None` : hors catégorie ; une
/// catégorie n'a jamais de parent), à ce rang (0 : en premier) parmi les salons que l'on voit
/// dans ce parent, une fois le salon déplacé retiré de la liste.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveChannel {
    pub parent_id: Option<Id>,
    pub index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverwriteKind {
    Role,
    Member,
}

/// Ce qu'un rôle ou un membre peut (`allow`) ou ne peut pas (`deny`) faire dans un salon, en plus
/// de ses droits du serveur. Un droit absent des deux est hérité.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOverwrite {
    pub target_id: Id,
    pub kind: OverwriteKind,
    pub allow: u64,
    pub deny: u64,
}

/// `PUT /channels/{id}/permissions/{target}` : remplace la surcharge de ce rôle ou membre
/// (`allow` et `deny` vides : elle est retirée).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetOverwrite {
    pub kind: OverwriteKind,
    pub allow: u64,
    pub deny: u64,
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
    /// Le message auquel celui-ci répond.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Reply>,
    /// Dans l'ordre de la première réaction de chaque emoji.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<Reaction>,
    /// Personnes mentionnées par `@pseudo` qui voient le salon.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<User>,
    /// `@everyone` ou `@here` écrit par quelqu'un qui en a le droit.
    #[serde(default)]
    pub mention_everyone: bool,
    /// Message d'appel d'un MP ou d'un groupe (son auteur a lancé l'appel) ; le texte est vide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<CallInfo>,
    /// Fichiers joints, dans l'ordre d'envoi.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Aperçus des liens du texte, ajoutés par le serveur après l'envoi.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<Embed>,
}

/// Fichiers joints à un message au plus.
pub const MAX_ATTACHMENTS: usize = 10;
/// Taille par défaut d'un fichier joint, en octets (`KRAKEN_MAX_UPLOAD_MB` la change).
pub const DEFAULT_MAX_UPLOAD_BYTES: u64 = 25 * 1024 * 1024;
/// Longueur au plus du texte alternatif d'une image.
pub const MAX_ALT_TEXT: usize = 1024;
/// Liens d'un message qui reçoivent un aperçu, au plus.
pub const MAX_EMBEDS: usize = 5;

/// Un fichier joint. L'original se télécharge sous
/// `/channels/{salon}/attachments/{id}` ; une image a aussi un aperçu sans métadonnées sous
/// `…/preview`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: Id,
    pub filename: String,
    pub mime: String,
    pub size: u64,
    /// Dimensions d'une image lisible ; `None` pour tout autre fichier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

/// Un fichier déjà envoyé, à rattacher au message qui se crée.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

/// L'aperçu d'un lien : ce que la page dit d'elle-même.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Embed {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    /// Image de la page, gardée sur le serveur : `/channels/{salon}/embeds/{clé}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<EmbedImage>,
    /// Petite icône du site, même adresse que l'image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    /// Retiré par l'auteur : gardé côté serveur (pour ne pas revenir à une modification), jamais envoyé.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbedImage {
    pub key: String,
    pub width: u32,
    pub height: u32,
}

/// Ce qu'un message d'appel retient de l'appel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CallInfo {
    /// Ceux qui y sont entrés (l'auteur compris), connus à la fin de l'appel.
    #[serde(default)]
    pub participants: Vec<Id>,
    /// `None` : l'appel est en cours.
    pub ended_at: Option<i64>,
}

/// Longueur, en caractères, de l'extrait du message d'origine montré au-dessus d'une réponse.
pub const REPLY_EXCERPT: usize = 200;

/// Aperçu du message d'origine d'une réponse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    /// `None` : le message d'origine a été supprimé.
    pub id: Option<Id>,
    pub author: Option<User>,
    /// Ses [`REPLY_EXCERPT`] premiers caractères, sans jeton d'emoji perso coupé en deux.
    pub content: String,
}

/// Un emoji et ceux qui ont réagi avec, du premier au dernier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reaction {
    /// Un emoji Unicode, ou l'id (avec tirets) d'un emoji perso : il reste dans la réaction même
    /// si l'emoji est supprimé ensuite (le client l'affiche alors comme « emoji supprimé »).
    pub emoji: String,
    pub users: Vec<Id>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMessage {
    pub content: String,
    pub nonce: Option<String>,
    /// Répondre à ce message du même salon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Id>,
    /// Fichiers déjà envoyés (`POST /channels/{id}/attachments`), dans l'ordre voulu. Le texte
    /// peut alors rester vide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentRef>,
}

/// `PATCH /channels/{id}/messages/{mid}` : nouveau texte (auteur seulement).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditMessage {
    pub content: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CreateInvite {
    /// Secondes ; `None` : n'expire jamais.
    pub max_age: Option<u32>,
    /// 0 ou `None` : illimité.
    pub max_uses: Option<u32>,
    /// Sans fin et sans limite : reprendre celle que la personne a déjà dans ce salon plutôt
    /// que d'en créer une de plus (le bouton « Inviter » du rail, pour ne pas remplir le plafond).
    #[serde(default)]
    pub reuse: bool,
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

/// Où en est une invitation : encore valable, trop vieille ou déjà utilisée autant que permis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteState {
    Active,
    Expired,
    Exhausted,
}

/// Une ligne de la liste des invitations d'un serveur (`GET /guilds/{id}/invites`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuildInvite {
    pub code: String,
    pub channel_id: Option<Id>,
    pub channel_name: Option<String>,
    /// Qui l'a créée ; `None` si son compte n'existe plus.
    pub inviter: Option<User>,
    pub uses: u32,
    /// 0 : illimité.
    pub max_uses: u32,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub state: InviteState,
}

/// `DELETE /guilds/{id}` : le nom du serveur, retapé pour confirmer.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeleteGuild {
    pub name: String,
}

/// `PUT /guilds/{id}/members/{uid}/timeout`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimeoutMember {
    /// De [`MIN_TIMEOUT_S`] à [`MAX_TIMEOUT_S`].
    pub seconds: u32,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Friend,
    Blocked,
    PendingIncoming,
    PendingOutgoing,
}

/// Ce que les autres voient de quelqu'un.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Online,
    Idle,
    Dnd,
    Offline,
}

/// Le statut que l'on choisit. `Invisible` n'est jamais montré aux autres : ils voient la
/// personne hors ligne. « Hors ligne » n'est pas un choix (aucune connexion ouverte), et le
/// passage automatique à inactif ne remplace que `Online`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    #[default]
    Online,
    Idle,
    Dnd,
    Invisible,
}

impl Presence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::Dnd => "dnd",
            Self::Invisible => "invisible",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "online" => Self::Online,
            "idle" => Self::Idle,
            "dnd" => Self::Dnd,
            "invisible" => Self::Invisible,
            _ => return None,
        })
    }
}

/// `PUT /users/@me/presence` : choisir son statut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetPresence {
    pub presence: Presence,
}

/// Le statut d'une personne, tel que je le vois.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceEntry {
    pub user_id: Id,
    pub status: Status,
}

/// Sans activité du client pendant ce temps (5 minutes), il se signale inactif.
pub const IDLE_AFTER_SECS: u32 = 300;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub user: User,
    pub kind: RelationshipKind,
    pub status: Status,
}

/// `POST /users/@me/relationships` : demande d'ami par pseudo unique.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FriendRequest {
    pub username: String,
}

/// `PUT /users/@me/relationships/{id}` : `friend` accepte sa demande (ou en envoie une),
/// `blocked` bloque. Les autres valeurs sont refusées.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetRelationship {
    pub kind: RelationshipKind,
}

/// Personnes au plus dans un groupe privé, soi compris.
pub const MAX_GROUP_MEMBERS: usize = 10;

/// Poids maximal d'une photo de profil envoyée, en octets (8 Mio).
pub const MAX_AVATAR_BYTES: usize = 8 * 1024 * 1024;

/// Poids maximal d'une bannière envoyée, en octets : le même que pour la photo.
pub const MAX_BANNER_BYTES: usize = MAX_AVATAR_BYTES;

/// Côté, en pixels, de la photo de profil telle que le serveur la garde.
pub const AVATAR_SIZE: u32 = 256;

/// Dimensions, en pixels, d'une bannière fixe telle que le serveur la garde (3 pour 1).
pub const BANNER_WIDTH: u32 = 960;
pub const BANNER_HEIGHT: u32 = 320;

/// Une image animée (GIF, WebP) est gardée telle quelle : au plus ce côté, en pixels, et ce
/// nombre d'images (le produit des deux et de la surface est borné aussi).
pub const MAX_ANIMATED_SIDE: u32 = 1024;
pub const MAX_ANIMATED_FRAMES: usize = 600;

/// Longueur maximale du nom d'un groupe, en caractères.
pub const MAX_GROUP_NAME: usize = 100;

/// `POST /users/@me/channels` : une personne, le MP avec elle (rouvert s'il existe) ; de 2 à
/// [`MAX_GROUP_MEMBERS`] − 1 amis, un nouveau groupe.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OpenPrivate {
    pub recipients: Vec<Id>,
    /// Groupe seulement ; vide : les prénoms des membres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `PATCH /channels/{id}` : renommer un groupe (vide : retour aux noms des membres), ou modifier
/// un salon de serveur. Champs absents : inchangés. `topic` vide : plus de sujet. `private` :
/// `@everyone` ne voit plus le salon (ou le revoit).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EditChannel {
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slowmode_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
}

// ───────── non-lus et notifications ─────────

/// Où j'en suis dans un salon : le dernier message lu, et mes mentions plus récentes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadState {
    pub channel_id: Id,
    /// `None` : rien lu (tout message du salon est nouveau).
    pub last_read: Option<Id>,
    /// Mentions (`@moi`, `@everyone`, réponses à mes messages, tout message d'un MP) arrivées
    /// après `last_read`.
    pub mention_count: u32,
}

/// Messages acquittés par minute et par client (le client les regroupe).
pub const ACKS_PER_MIN: u32 = 120;

/// Ce qui prévient dans un serveur ou un salon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    /// Salon : comme son serveur ; serveur : le niveau par défaut (les mentions).
    Default,
    All,
    Mentions,
    #[serde(rename = "none")]
    Nothing,
}

/// Fin d'une sourdine sans fin (« jusqu'à ce que je la retire »).
pub const MUTED_FOREVER: i64 = i64::MAX;

/// Mon réglage de notification pour un serveur (`channel_id` absent) ou un salon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifySetting {
    pub guild_id: Option<Id>,
    pub channel_id: Option<Id>,
    pub level: NotifyLevel,
    /// Serveur seulement : `@everyone` et `@here` ne comptent pas comme des mentions.
    #[serde(default)]
    pub suppress_everyone: bool,
    /// Fin de la sourdine (ms) ; [`MUTED_FOREVER`] : jusqu'à ce que je la retire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted_until: Option<i64>,
}

/// Mettre en sourdine, ou l'enlever.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mute {
    Off,
    /// Pendant ce nombre de minutes (au plus 30 jours).
    Minutes(u32),
    Forever,
}

/// `PUT /users/@me/notifications` : un serveur ou un salon (l'un des deux) ; les champs absents
/// ne changent pas.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SetNotify {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<NotifyLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppress_everyone: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mute: Option<Mute>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    /// Avec le code `timed_out` : jusqu'à quand (ms Unix) la personne est exclue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_until: Option<i64>,
}

// ───────────── salons vocaux ─────────────

/// Participants au plus dans un salon vocal.
pub const MAX_VOICE_MEMBERS: usize = 25;

/// Libellé du canal de données WebRTC ouvert par le client.
pub const VOICE_DATA_CHANNEL: &str = "kraken";

/// Où en est quelqu'un dans les salons vocaux d'un serveur, ou dans l'appel d'un MP ou d'un
/// groupe. Une connexion vocale par utilisateur : rejoindre un autre salon quitte le précédent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceState {
    /// `None` : appel d'un MP ou d'un groupe (`channel_id` est alors la conversation).
    #[serde(default)]
    pub guild_id: Option<Id>,
    /// `None` : a quitté le vocal (ou se trouve dans un salon que tu ne vois pas).
    pub channel_id: Option<Id>,
    pub user_id: Id,
    /// Micro coupé par l'utilisateur lui-même.
    pub self_mute: bool,
    /// Son coupé : n'entend plus personne (et ne parle plus).
    pub self_deaf: bool,
    /// Partage son écran.
    pub streaming: bool,
}

/// Secondes pendant lesquelles un appel sonne chez ceux qui n'ont pas encore répondu.
pub const CALL_RING_SECONDS: u64 = 30;

/// Un appel en cours dans un MP ou un groupe : on y entre comme dans un salon vocal
/// (`POST /channels/{id}/voice`), le premier qui entre le lance et fait sonner les autres.
/// Qui y est : les états vocaux (`guild_id` à `None`). L'appel prend fin quand le dernier part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub channel_id: Id,
    /// Le message d'appel posté dans la conversation.
    pub message_id: Id,
    /// Qui l'a lancé.
    pub started_by: Id,
    /// Chez qui ça sonne encore (ni entré, ni refusé, ni plus de [`CALL_RING_SECONDS`]).
    pub ringing: Vec<Id>,
}

/// `POST /channels/{id}/voice` : l'offre WebRTC du navigateur. Elle contient, dans cet ordre,
/// trois transceivers `sendonly` (micro, image de l'écran, son de l'écran) et le canal de
/// données [`VOICE_DATA_CHANNEL`]. Le client ne renégocie jamais : il démarre et arrête micro et
/// partage avec `RTCRtpSender.replaceTrack` ; ensuite, seul le serveur fait des offres.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceJoin {
    pub sdp: String,
    pub mids: VoiceMids,
    /// État de départ (micro refusé, par exemple) ; ensuite, message `state`.
    #[serde(default)]
    pub self_mute: bool,
    #[serde(default)]
    pub self_deaf: bool,
}

/// Les `mid` des trois transceivers de l'offre.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceMids {
    pub mic: String,
    pub screen: String,
    pub screen_audio: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceAnswer {
    pub sdp: String,
}

/// Canal de données, client → serveur (JSON). Message invalide ou trop long : ignoré.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum VoiceClientMsg {
    /// Réponse à la dernière offre du serveur.
    Answer {
        sdp: String,
    },
    State {
        self_mute: bool,
        self_deaf: bool,
        streaming: bool,
    },
    /// Regarder (ou ne plus regarder) l'écran partagé de quelqu'un du même salon.
    Watch {
        user_id: Id,
        on: bool,
    },
    /// Contrôle à distance : demander à `user_id`, dont on regarde l'écran, de prendre la main.
    ControlRequest {
        user_id: Id,
    },
    /// Contrôle à distance : réponse de celui qui partage à la demande de `user_id`.
    ControlAnswer {
        user_id: Id,
        accept: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<ControlRefusal>,
    },
    /// Contrôle à distance : une action sur l'écran de `user_id`, qui l'a accepté.
    ControlInput {
        user_id: Id,
        input: RemoteInput,
    },
    /// Contrôle à distance : fin (ou demande retirée) entre moi et `user_id`, dans un sens ou
    /// dans l'autre.
    ControlStop {
        user_id: Id,
    },
}

/// Canal de données, serveur → client. Une seule offre à la fois : la suivante attend la réponse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum VoiceServerMsg {
    /// Pistes ajoutées ou retirées. Chaque piste reçue a pour flux `<user_id>.mic` ou
    /// `<user_id>.screen` (image et son de l'écran partagent ce flux).
    Offer { sdp: String },
    /// `user_id`, qui regarde mon écran, demande à le contrôler.
    ControlRequest { user_id: Id },
    /// Réponse de `user_id` à ma demande de contrôle.
    ControlAnswer {
        user_id: Id,
        accept: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<ControlRefusal>,
    },
    /// Une action de `user_id`, qui contrôle mon écran.
    ControlInput { user_id: Id, input: RemoteInput },
    /// Le contrôle (ou la demande) entre moi et `user_id` a pris fin : arrêté par l'autre, fin
    /// du partage, plus regardé, départ ou connexion perdue.
    ControlEnd { user_id: Id },
}

/// Pourquoi une demande de contrôle n'a pas abouti.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRefusal {
    /// Refusée par celui qui partage.
    Declined,
    /// Quelqu'un d'autre a déjà le contrôle.
    Busy,
    /// Celui qui partage est dans un navigateur : seule l'appli de bureau peut être contrôlée.
    Desktop,
    /// Il partage une fenêtre ou un onglet, pas un écran entier.
    Surface,
    /// Impossible sur son ordinateur (Wayland, autorisation macOS manquante…).
    Unavailable,
}

/// Boutons de souris transmis au contrôle à distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

impl MouseButton {
    /// `MouseEvent.button` du navigateur.
    pub fn from_dom(button: i16) -> Option<Self> {
        Some(match button {
            0 => Self::Left,
            1 => Self::Middle,
            2 => Self::Right,
            3 => Self::Back,
            4 => Self::Forward,
            _ => return None,
        })
    }
}

/// Une action du contrôle à distance. Les positions sont relatives à l'image partagée :
/// (0, 0) en haut à gauche, (1, 1) en bas à droite.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum RemoteInput {
    Move {
        x: f32,
        y: f32,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    /// Crans de molette (positif : vers le bas, ou vers la droite).
    Wheel {
        dx: i32,
        dy: i32,
    },
    /// Touche physique, nommée comme `KeyboardEvent.code` (« KeyA », « Enter »…).
    Key {
        code: String,
        down: bool,
    },
}

/// Crans de molette au plus par action.
pub const MAX_WHEEL_STEPS: i32 = 10;

impl RemoteInput {
    /// Une action plausible : position dans l'image, molette bornée, nom de touche court.
    pub fn valid(&self) -> bool {
        match self {
            Self::Move { x, y } => [x, y].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(*v)),
            Self::Button { .. } => true,
            Self::Wheel { dx, dy } => {
                (dx.unsigned_abs() <= MAX_WHEEL_STEPS as u32 && dy.unsigned_abs() <= MAX_WHEEL_STEPS as u32)
                    && (*dx != 0 || *dy != 0)
            }
            Self::Key { code, .. } => (1..=32).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric()),
        }
    }
}

/// Où tombe un point de l'élément vidéo (`px`, `py` depuis son coin, taille `w` × `h`) dans une
/// image `vw` × `vh` affichée en entier (`object-fit: contain`, bandes noires comprises).
/// `None` : dans les bandes, ou rien à afficher.
pub fn contain_point(px: f64, py: f64, w: f64, h: f64, vw: f64, vh: f64) -> Option<(f32, f32)> {
    if !(w > 0.0 && h > 0.0 && vw > 0.0 && vh > 0.0) {
        return None;
    }
    let scale = (w / vw).min(h / vh);
    let (iw, ih) = (vw * scale, vh * scale);
    let (x0, y0) = ((w - iw) / 2.0, (h - ih) / 2.0);
    let (x, y) = ((px - x0) / iw, (py - y0) / ih);
    ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then_some((x as f32, y as f32))
}

// ───────────── gateway ─────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ready {
    pub user: User,
    pub guilds: Vec<GuildState>,
    pub private_channels: Vec<Channel>,
    pub relationships: Vec<Relationship>,
    pub session_id: String,
    /// Appels en cours dans mes MP et groupes.
    #[serde(default)]
    pub calls: Vec<Call>,
    /// Qui est dans ces appels.
    #[serde(default)]
    pub call_voice_states: Vec<VoiceState>,
    /// Où j'en suis dans chaque salon déjà ouvert ou qui m'a mentionné.
    #[serde(default)]
    pub read_states: Vec<ReadState>,
    /// Mes réglages de notification par serveur et par salon.
    #[serde(default)]
    pub notify_settings: Vec<NotifySetting>,
    /// Mon statut choisi (il est resté le même d'une session à l'autre).
    #[serde(default)]
    pub presence: Presence,
    /// Qui est en ligne, inactif ou ne pas déranger parmi ceux qui me voient (amis, membres de mes
    /// serveurs, mes MP) ; les absents sont hors ligne.
    #[serde(default)]
    pub presences: Vec<PresenceEntry>,
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
    /// Un client qui comprend les statuts le dit en l'envoyant tout de suite après IDENTIFY
    /// (`idle` à faux) ; il reçoit alors [`Event::PresenceUpdate`] et [`Event::MyPresenceUpdate`],
    /// que les autres clients, qui ne les connaissent pas, ne reçoivent jamais. Ensuite il
    /// l'envoie à chaque changement : `idle` à vrai après [`IDLE_AFTER_SECS`] sans geste, à faux
    /// dès le suivant. La personne apparaît inactive quand tous ses clients le sont ; un choix
    /// manuel (ne pas déranger, invisible, inactif) n'est jamais remplacé.
    Activity {
        idle: bool,
    },
}

/// Serveur → client.
// Presque chaque trame est un `Dispatch` : l'enveloppe ne sert qu'un instant avant d'être écrite ou
// consommée. L'indirection n'économiserait rien, et casserait tous les motifs `Dispatch { event:
// Event::… }` des clients.
#[allow(clippy::large_enum_variant)]
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
    MessageCreate(Box<Message>),
    /// Texte modifié (ou mentions recalculées) : le message entier, à remplacer.
    MessageUpdate(Box<Message>),
    MessageDelete {
        channel_id: Id,
        id: Id,
    },
    ReactionAdd {
        channel_id: Id,
        message_id: Id,
        user_id: Id,
        emoji: String,
    },
    ReactionRemove {
        channel_id: Id,
        message_id: Id,
        user_id: Id,
        emoji: String,
    },
    ChannelCreate(Channel),
    /// Groupe renommé, membre arrivé ou parti : le salon entier, tel que tu le vois.
    ChannelUpdate(Channel),
    /// Tu as quitté ce groupe (ou en as été retiré).
    ChannelDelete {
        id: Id,
    },
    /// Nouvelle relation (demande reçue ou envoyée, compte bloqué).
    RelationshipAdd(Relationship),
    /// Relation changée (demande acceptée, ami bloqué).
    RelationshipUpdate(Relationship),
    /// Plus de relation avec cette personne (ami retiré, demande refusée ou annulée, débloqué).
    RelationshipRemove {
        user_id: Id,
    },
    GuildCreate(Box<GuildState>),
    /// Le serveur n'existe plus : son propriétaire l'a supprimé.
    GuildDelete {
        guild_id: Id,
    },
    MemberAdd {
        guild_id: Id,
        member: Member,
    },
    /// Rôles ou surnom d'un membre changés : le membre entier, à remplacer.
    GuildMemberUpdate {
        guild_id: Id,
        member: Member,
    },
    /// Parti, exclu ou banni. Reçu par la personne elle-même, c'est qu'elle n'a plus ce serveur.
    GuildMemberRemove {
        guild_id: Id,
        user_id: Id,
    },
    GuildRoleCreate {
        guild_id: Id,
        role: Role,
    },
    /// Rôle modifié ou déplacé : le rôle entier, à sa nouvelle place.
    GuildRoleUpdate {
        guild_id: Id,
        role: Role,
    },
    GuildRoleDelete {
        guild_id: Id,
        role_id: Id,
    },
    /// À ceux qui peuvent bannir seulement.
    GuildBanAdd {
        guild_id: Id,
        ban: Ban,
    },
    GuildBanRemove {
        guild_id: Id,
        user_id: Id,
    },
    /// Propriété transférée.
    GuildOwnerUpdate {
        guild_id: Id,
        owner_id: Id,
    },
    /// Un emoji perso ajouté, renommé ou retiré : la liste entière du serveur, à sa place.
    EmojisUpdate {
        guild_id: Id,
        emojis: Vec<Emoji>,
    },
    /// Profil changé (nom affiché, photo, bannière, bio, pronoms, pseudo) : envoyé à l'intéressé et à
    /// ceux qui le voient.
    UserUpdate(User),
    /// Statut de ces personnes, tel que je le vois : un changement en direct, ou un lot à
    /// l'arrivée dans un serveur. Hors ligne : plus aucune connexion, ou invisible.
    PresenceUpdate {
        presences: Vec<PresenceEntry>,
    },
    /// Mon statut choisi a changé, sur cet appareil ou un autre ; à moi seulement.
    MyPresenceUpdate {
        presence: Presence,
    },
    /// Arrivée, départ ou changement d'état dans un salon vocal (à ceux qui le voient) ou dans
    /// l'appel d'un MP ou d'un groupe (à ses participants).
    VoiceStateUpdate(VoiceState),
    /// Appel lancé, ou changement de qui il fait sonner ; aux participants de la conversation.
    CallUpdate(Call),
    /// Appel terminé : le dernier est parti.
    CallDelete {
        channel_id: Id,
    },
    /// Salons lus (ici ou sur un autre appareil), ou compteur de mentions recalculé ; à moi
    /// seulement.
    ReadStateUpdate(Vec<ReadState>),
    /// Réglage de notification changé (ici ou sur un autre appareil) ; à moi seulement.
    NotifySettingsUpdate(NotifySetting),
}

// ───────── journal d'audit ─────────

/// Lignes par page du journal d'audit quand `limit` n'est pas donné.
pub const AUDIT_PAGE: u32 = 50;
/// Lignes par page du journal d'audit, au plus.
pub const MAX_AUDIT_PAGE: u32 = 100;
/// Lectures du journal d'audit par minute et par client.
pub const AUDIT_READS_PER_MIN: u32 = 120;
/// Au plus ce nombre de personnes dans la liste « qui a agi » d'une première page.
pub const MAX_AUDIT_ACTORS: usize = 200;

/// Les familles d'actions du journal, pour le filtre `?category=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditCategory {
    #[serde(rename = "membres")]
    Members,
    #[serde(rename = "roles")]
    Roles,
    #[serde(rename = "salons")]
    Channels,
    #[serde(rename = "invitations")]
    Invites,
    #[serde(rename = "emojis")]
    Emojis,
    #[serde(rename = "serveur")]
    Guild,
}

impl AuditCategory {
    pub const ALL: [Self; 6] = [Self::Members, Self::Roles, Self::Channels, Self::Invites, Self::Emojis, Self::Guild];

    /// Le mot de l'adresse (`?category=membres`).
    pub fn slug(self) -> &'static str {
        match self {
            Self::Members => "membres",
            Self::Roles => "roles",
            Self::Channels => "salons",
            Self::Invites => "invitations",
            Self::Emojis => "emojis",
            Self::Guild => "serveur",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.slug() == s)
    }

    /// Les actions de cette famille, telles qu'elles sont écrites dans `audit_log.action`.
    pub fn actions(self) -> &'static [&'static str] {
        match self {
            Self::Members => &[
                "member_role_add",
                "member_role_remove",
                "member_kick",
                "member_ban",
                "member_unban",
                "member_timeout",
                "member_timeout_remove",
                "owner_transfer",
            ],
            Self::Roles => &["role_create", "role_update", "role_delete", "role_reorder"],
            Self::Channels => {
                &["channel_create", "channel_update", "channel_delete", "channel_move", "overwrite_update"]
            }
            Self::Invites => &["invite_create", "invite_revoke"],
            Self::Emojis => &["emoji_create", "emoji_update", "emoji_delete"],
            Self::Guild => &["guild_update", "message_delete"],
        }
    }

    /// La famille d'une action ; `None` pour une action que cette version ne connaît pas.
    pub fn of(action: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.actions().contains(&action))
    }
}

/// Qui a agi : son nom affiché (ou son pseudo) et sa photo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditTargetKind {
    User,
    Role,
    Channel,
    Emoji,
}

/// Ce sur quoi l'action a porté. `name` est lu au moment de la requête, ou à défaut le nom gardé à
/// l'écriture ; `None` : la cible n'existe plus et son nom n'a pas été gardé (affiché « supprimé »).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditTarget {
    pub kind: AuditTargetKind,
    pub id: Id,
    pub name: Option<String>,
}

/// Un avant / après lisible. `field` : `name`, `topic`, `slowmode_s`, `private`, `color`, `hoist`
/// ou `permissions` (un champ de bits écrit en décimal). Une valeur vide : rien (pas de sujet, pas
/// de couleur).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

/// Une ligne du journal. Rien n'y passe que la liste blanche de chaque action : `details` porte les faits
/// de la phrase (`role`, `seconds`, `kind`, `channel`, `max_uses`, `max_age_s`, `animated`, `inviter`,
/// `allow`, `deny`, `subject`, `subject_kind`), `changes` les avant / après.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Id,
    /// Quand (ms Unix).
    pub created_at: i64,
    /// `None` : son compte n'existe plus.
    pub actor: Option<AuditActor>,
    pub action: String,
    pub target: Option<AuditTarget>,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub details: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<AuditChange>,
}

/// `GET /guilds/{id}/audit-log` : du plus récent au plus ancien. `next` : à passer en `before` pour la
/// page suivante, `None` à la fin. `actors` (première page seulement) : tous ceux qui ont agi dans ce
/// serveur, par nom, quel que soit le filtre, pour remplir la liste « Membre ».
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    pub next: Option<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<AuditActor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le format du canal de données vocal, tel que le client web l'écrit.
    #[test]
    fn voice_data_channel_messages() {
        let u = Id::from_bytes([7; 16]);
        let watch: VoiceClientMsg =
            serde_json::from_str(&format!(r#"{{"t":"watch","user_id":"{u}","on":true}}"#)).unwrap();
        assert_eq!(watch, VoiceClientMsg::Watch { user_id: u, on: true });
        let state: VoiceClientMsg =
            serde_json::from_str(r#"{"t":"state","self_mute":true,"self_deaf":false,"streaming":false}"#).unwrap();
        assert_eq!(state, VoiceClientMsg::State { self_mute: true, self_deaf: false, streaming: false });
        let offer = serde_json::to_value(VoiceServerMsg::Offer { sdp: "v=0".into() }).unwrap();
        assert_eq!(offer, serde_json::json!({"t": "offer", "sdp": "v=0"}));
        // Les identifiants s'écrivent avec tirets : c'est le début des flux `<id>.mic`.
        assert_eq!(serde_json::to_value(u).unwrap(), "07070707-0707-0707-0707-070707070707");
        // Anciennes réponses sans les champs vocaux : toujours lisibles.
        let g: Instance = serde_json::from_str(r#"{"contact":null,"processors":null,"source_url":"x"}"#).unwrap();
        assert!(g.public_url.is_empty());
    }

    /// Le contrôle à distance, tel que le client web l'écrit.
    #[test]
    fn remote_control_messages() {
        let u = Id::from_bytes([7; 16]);
        let key: VoiceClientMsg = serde_json::from_str(&format!(
            r#"{{"t":"control_input","user_id":"{u}","input":{{"k":"key","code":"KeyA","down":true}}}}"#
        ))
        .unwrap();
        assert_eq!(
            key,
            VoiceClientMsg::ControlInput { user_id: u, input: RemoteInput::Key { code: "KeyA".into(), down: true } }
        );
        let answer: VoiceClientMsg =
            serde_json::from_str(&format!(r#"{{"t":"control_answer","user_id":"{u}","accept":true}}"#)).unwrap();
        assert_eq!(answer, VoiceClientMsg::ControlAnswer { user_id: u, accept: true, reason: None });
        let refused =
            VoiceServerMsg::ControlAnswer { user_id: u, accept: false, reason: Some(ControlRefusal::Desktop) };
        assert_eq!(
            serde_json::to_value(refused).unwrap(),
            serde_json::json!({"t": "control_answer", "user_id": u, "accept": false, "reason": "desktop"})
        );
        let click = serde_json::to_value(RemoteInput::Button { button: MouseButton::Right, down: false }).unwrap();
        assert_eq!(click, serde_json::json!({"k": "button", "button": "right", "down": false}));
        assert_eq!(MouseButton::from_dom(2), Some(MouseButton::Right));
        assert_eq!(MouseButton::from_dom(7), None);
    }

    /// Réglages de notification, tels que le client les écrit.
    #[test]
    fn notify_settings_format() {
        let req: SetNotify = serde_json::from_str(r#"{"guild_id":null,"channel_id":"07070707-0707-0707-0707-070707070707","level":"none","mute":{"minutes":15}}"#).unwrap();
        assert_eq!(req.level, Some(NotifyLevel::Nothing));
        assert_eq!(req.mute, Some(Mute::Minutes(15)));
        assert_eq!(req.suppress_everyone, None);
        let forever: SetNotify =
            serde_json::from_str(r#"{"guild_id":"07070707-0707-0707-0707-070707070707","mute":"forever"}"#).unwrap();
        assert_eq!(forever.mute, Some(Mute::Forever));
        assert_eq!(serde_json::to_value(Mute::Off).unwrap(), "off");
        assert_eq!(serde_json::to_value(NotifyLevel::Default).unwrap(), "default");
        // Un salon sans dernier message l'omet ; un ancien client sans le champ reste lisible.
        let c: Channel = serde_json::from_str(
            r#"{"id":"07070707-0707-0707-0707-070707070707","guild_id":null,"parent_id":null,"kind":"dm","name":null,"topic":null,"position":0}"#,
        )
        .unwrap();
        assert_eq!(c.last_message_id, None);
        assert!(!serde_json::to_string(&c).unwrap().contains("last_message_id"));
    }

    #[test]
    fn remote_inputs_are_checked() {
        let ok = |i: RemoteInput| i.valid();
        assert!(ok(RemoteInput::Move { x: 0.0, y: 1.0 }));
        assert!(ok(RemoteInput::Move { x: 0.5, y: 0.25 }));
        assert!(!ok(RemoteInput::Move { x: -0.01, y: 0.5 }));
        assert!(!ok(RemoteInput::Move { x: 0.5, y: 1.5 }));
        assert!(!ok(RemoteInput::Move { x: f32::NAN, y: 0.5 }));
        assert!(!ok(RemoteInput::Move { x: f32::INFINITY, y: 0.5 }));
        assert!(ok(RemoteInput::Wheel { dx: 0, dy: -3 }));
        assert!(!ok(RemoteInput::Wheel { dx: 0, dy: 0 }));
        assert!(!ok(RemoteInput::Wheel { dx: 11, dy: 0 }));
        assert!(!ok(RemoteInput::Wheel { dx: 0, dy: i32::MIN }));
        assert!(ok(RemoteInput::Key { code: "ArrowLeft".into(), down: true }));
        assert!(!ok(RemoteInput::Key { code: String::new(), down: true }));
        assert!(!ok(RemoteInput::Key { code: "Key A".into(), down: true }));
        assert!(!ok(RemoteInput::Key { code: "K".repeat(33), down: true }));
        assert!(!ok(RemoteInput::Key { code: "Clé".into(), down: true }));
    }

    #[test]
    fn audit_categories_cover_each_action_once() {
        let mut seen = std::collections::HashSet::new();
        for c in AuditCategory::ALL {
            assert_eq!(AuditCategory::parse(c.slug()), Some(c), "{c:?}");
            assert_eq!(serde_json::to_string(&c).unwrap(), format!("\"{}\"", c.slug()));
            for a in c.actions() {
                assert!(seen.insert(*a), "{a} est dans deux familles");
                assert_eq!(AuditCategory::of(a), Some(c));
            }
        }
        assert_eq!(AuditCategory::parse(""), None);
        assert_eq!(AuditCategory::parse("Roles"), None);
        assert_eq!(AuditCategory::of("action_de_demain"), None);
        assert_eq!(AuditCategory::of("message_delete"), Some(AuditCategory::Guild));
    }

    #[test]
    fn points_in_a_contained_video() {
        // Image 16:9 dans un élément 16:9 : coins et milieu.
        assert_eq!(contain_point(0.0, 0.0, 1600.0, 900.0, 1920.0, 1080.0), Some((0.0, 0.0)));
        assert_eq!(contain_point(800.0, 450.0, 1600.0, 900.0, 1920.0, 1080.0), Some((0.5, 0.5)));
        assert_eq!(contain_point(1600.0, 900.0, 1600.0, 900.0, 1920.0, 1080.0), Some((1.0, 1.0)));
        // Image 4:3 dans un élément 16:9 : bandes à gauche et à droite (1200 px d'image sur 1600).
        assert_eq!(contain_point(100.0, 450.0, 1600.0, 900.0, 1024.0, 768.0), None);
        assert_eq!(contain_point(200.0, 0.0, 1600.0, 900.0, 1024.0, 768.0), Some((0.0, 0.0)));
        assert_eq!(contain_point(1400.0, 900.0, 1600.0, 900.0, 1024.0, 768.0), Some((1.0, 1.0)));
        // Image plus large : bandes en haut et en bas.
        let (x, y) = contain_point(800.0, 500.0, 1600.0, 1000.0, 3200.0, 900.0).unwrap();
        assert!((x - 0.5).abs() < 1e-6 && (y - 0.5).abs() < 1e-6);
        assert_eq!(contain_point(800.0, 10.0, 1600.0, 1000.0, 3200.0, 900.0), None);
        // Rien à afficher.
        assert_eq!(contain_point(1.0, 1.0, 100.0, 100.0, 0.0, 0.0), None);
        assert_eq!(contain_point(1.0, 1.0, 0.0, 100.0, 10.0, 10.0), None);
    }
}
