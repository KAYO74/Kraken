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
    /// Adresse publique de l'instance, sans `/` final : celle des liens d'invitation, quelle que
    /// soit l'adresse par laquelle la page a été ouverte. Vide si le serveur ne la donne pas.
    #[serde(default)]
    pub public_url: String,
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
    /// Qui est dans les salons vocaux (ceux que ce membre voit).
    #[serde(default)]
    pub voice_states: Vec<VoiceState>,
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

// ───────────── salons vocaux ─────────────

/// Participants au plus dans un salon vocal.
pub const MAX_VOICE_MEMBERS: usize = 25;

/// Libellé du canal de données WebRTC ouvert par le client.
pub const VOICE_DATA_CHANNEL: &str = "kraken";

/// Où en est quelqu'un dans les salons vocaux d'un serveur. Une connexion vocale par
/// utilisateur : rejoindre un autre salon quitte le précédent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceState {
    pub guild_id: Id,
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
    /// Arrivée, départ ou changement d'état dans un salon vocal ; envoyé à tout le serveur.
    VoiceStateUpdate(VoiceState),
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
