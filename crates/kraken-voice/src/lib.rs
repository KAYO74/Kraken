//! Salons vocaux : un SFU WebRTC ([str0m]) sur un seul port UDP. Chacun envoie son micro et,
//! s'il partage son écran, l'image et le son de l'écran ; le serveur renvoie à chacun les micros
//! des autres, et l'écran de ceux qu'il regarde. Rien n'est décodé : les trames sont relayées
//! telles quelles.
//!
//! Le serveur HTTP crée la connexion ([`Sfu::join`] : offre du navigateur → réponse) ; une
//! boucle sans I/O asynchrone, sur son propre fil, fait le reste (ICE-lite, DTLS, SRTP, canal de
//! données). Après le premier échange, seul le serveur fait des offres, par le canal de données :
//! jamais deux offres croisées. Pas de TURN ni d'ICE-TCP : le port UDP doit être joignable.
//!
//! Les états vocaux (qui est où, micro coupé, partage…) sont tenus ici, en mémoire ; chaque
//! changement est signalé par le rappel donné à [`Sfu::start`].

mod run;

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc};
use std::thread::JoinHandle;
use std::time::Instant;

use kraken_proto::{Id, MAX_VOICE_MEMBERS, VoiceMids, VoiceState};
use str0m::change::SdpOffer;
use str0m::crypto::CryptoProvider;
use str0m::media::{MediaKind, Mid};
use str0m::{Candidate, Rtc, RtcConfig};

/// Port UDP par défaut (`KRAKEN_RTC_PORT`).
pub const DEFAULT_PORT: u16 = 3478;

/// Réglages du SFU.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Port UDP local, sur toutes les interfaces ; 0 : n'importe quel port libre.
    pub port: u16,
    /// Port pris : n'importe quel port libre plutôt qu'un échec (appli de bureau).
    pub fallback: bool,
    /// Adresses annoncées aux navigateurs (candidats « host »). Seules les IPv4 servent : la
    /// prise n'écoute qu'en IPv4.
    pub ips: Vec<IpAddr>,
    /// Port annoncé s'il diffère du port local (redirection de port devant le serveur).
    pub public_port: Option<u16>,
    /// Participants au plus par salon.
    pub max_members: usize,
}

impl Default for Settings {
    /// Port 3478, sans repli, rien à annoncer (à remplir), 25 participants par salon.
    fn default() -> Self {
        Self { port: DEFAULT_PORT, fallback: false, ips: Vec::new(), public_port: None, max_members: MAX_VOICE_MEMBERS }
    }
}

/// Une demande de connexion, droits déjà vérifiés par l'appelant.
#[derive(Clone, Debug)]
pub struct Join {
    pub guild_id: Id,
    pub channel_id: Id,
    pub user_id: Id,
    /// Offre SDP du navigateur.
    pub offer: String,
    pub mids: VoiceMids,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Droit SPEAK : son micro est relayé.
    pub can_speak: bool,
    /// Droit STREAM : son écran peut être regardé.
    pub can_stream: bool,
}

#[derive(Debug)]
pub enum JoinError {
    /// Le salon a déjà son maximum de participants.
    Full,
    /// Offre illisible, ou sans les pistes attendues (détail pour le journal).
    BadOffer(String),
    /// Le SFU est arrêté.
    Stopped,
}

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("salon plein"),
            Self::BadOffer(e) => write!(f, "offre refusée : {e}"),
            Self::Stopped => f.write_str("SFU arrêté"),
        }
    }
}

impl std::error::Error for JoinError {}

/// Ce que chaque navigateur envoie, dans l'ordre de l'offre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Mic,
    Screen,
    ScreenAudio,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Mic, Kind::Screen, Kind::ScreenAudio];

    fn media(self) -> MediaKind {
        match self {
            Kind::Screen => MediaKind::Video,
            Kind::Mic | Kind::ScreenAudio => MediaKind::Audio,
        }
    }

    fn is_audio(self) -> bool {
        self.media() == MediaKind::Audio
    }

    /// Flux (msid) des pistes relayées : le navigateur s'en sert pour savoir à qui est quoi.
    fn stream_id(self, user: Id) -> String {
        match self {
            Kind::Mic => format!("{user}.mic"),
            Kind::Screen | Kind::ScreenAudio => format!("{user}.screen"),
        }
    }

    fn track_id(self, user: Id) -> String {
        match self {
            Kind::Mic => format!("{user}.mic"),
            Kind::Screen => format!("{user}.screen-video"),
            Kind::ScreenAudio => format!("{user}.screen-audio"),
        }
    }
}

/// Une connexion vocale : son numéro (une nouvelle connexion du même utilisateur remplace
/// l'ancienne) et l'état diffusé.
struct Entry {
    session: u64,
    state: VoiceState,
}

type Notify = Box<dyn Fn(VoiceState) + Send + Sync>;

/// Partagé entre l'API et la boucle UDP : les états vocaux, source de vérité, et le rappel. Le
/// rappel est appelé sous le verrou : les changements sortent dans l'ordre où ils ont lieu.
struct Shared {
    users: Mutex<HashMap<Id, Entry>>,
    notify: Notify,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, HashMap<Id, Entry>> {
        self.users.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Retire cet utilisateur si `session` est toujours la sienne (ou quelle qu'elle soit) et
    /// prévient de son départ.
    fn release(&self, user: Id, session: Option<u64>) -> Option<u64> {
        let mut users = self.lock();
        if session.is_some_and(|s| users.get(&user).is_none_or(|e| e.session != s)) {
            return None;
        }
        let e = users.remove(&user)?;
        (self.notify)(left(&e.state));
        Some(e.session)
    }

    /// Change l'état de cette connexion (si elle est toujours la sienne) ; prévient si besoin.
    fn update(&self, user: Id, session: u64, f: impl FnOnce(&mut VoiceState)) {
        let mut users = self.lock();
        let Some(e) = users.get_mut(&user).filter(|e| e.session == session) else { return };
        let before = e.state.clone();
        f(&mut e.state);
        if e.state != before {
            (self.notify)(e.state.clone());
        }
    }
}

fn left(s: &VoiceState) -> VoiceState {
    VoiceState {
        guild_id: s.guild_id,
        channel_id: None,
        user_id: s.user_id,
        self_mute: false,
        self_deaf: false,
        streaming: false,
    }
}

/// Ordres de l'API à la boucle UDP.
enum Cmd {
    Join(Box<run::Client>),
    Leave { session: u64 },
}

/// Le SFU en marche. L'arrêter : le détruire (la boucle s'arrête et libère le port).
pub struct Sfu {
    shared: Arc<Shared>,
    tx: Option<mpsc::Sender<Cmd>>,
    thread: Option<JoinHandle<()>>,
    port: u16,
    candidates: Vec<SocketAddr>,
    max_members: usize,
    next_session: AtomicU64,
}

impl Sfu {
    /// Ouvre le port UDP et lance la boucle. `notify` reçoit chaque changement d'état vocal (un
    /// départ : `channel_id` à `None`) ; il est appelé depuis n'importe quel fil et ne doit pas
    /// bloquer.
    pub fn start(settings: Settings, notify: impl Fn(VoiceState) + Send + Sync + 'static) -> io::Result<Self> {
        let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, settings.port)) {
            Err(e) if settings.fallback && settings.port != 0 => {
                tracing::debug!("port UDP {} pris ({e}) : un autre port", settings.port);
                UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?
            }
            r => r?,
        };
        let port = socket.local_addr()?.port();
        let announced = settings.public_port.unwrap_or(port);
        let mut candidates: Vec<SocketAddr> = Vec::new();
        for ip in &settings.ips {
            let addr = SocketAddr::new(*ip, announced);
            if ip.is_ipv4() && !ip.is_unspecified() && !candidates.contains(&addr) {
                candidates.push(addr);
            }
        }
        if candidates.is_empty() {
            return Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "aucune adresse IPv4 à annoncer"));
        }
        let shared = Arc::new(Shared { users: Mutex::default(), notify: Box::new(notify) });
        let (tx, rx) = mpsc::channel();
        // Les paquets reçus sont tous présentés comme arrivés sur le premier candidat : la prise
        // écoute sur toutes les interfaces et ne sait pas laquelle a servi.
        let local = candidates[0];
        let loop_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("kraken-vocal".into())
            .spawn(move || run::run(socket, local, rx, loop_shared))?;
        Ok(Self {
            shared,
            tx: Some(tx),
            thread: Some(thread),
            port,
            candidates,
            max_members: settings.max_members,
            next_session: AtomicU64::new(1),
        })
    }

    /// Port UDP local.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Adresses annoncées aux navigateurs.
    pub fn candidates(&self) -> &[SocketAddr] {
        &self.candidates
    }

    /// Accepte l'offre et rend la réponse SDP. Une connexion précédente du même utilisateur est
    /// remplacée (même dans un autre salon). Coûte une clé DTLS : à appeler hors d'une boucle
    /// asynchrone si possible.
    pub fn join(&self, j: Join) -> Result<String, JoinError> {
        let tx = self.tx.as_ref().ok_or(JoinError::Stopped)?;
        // Salon plein : inutile de fabriquer la connexion (revérifié plus bas, sous le verrou).
        if self.others(&self.shared.lock(), &j) >= self.max_members {
            return Err(JoinError::Full);
        }
        let offer = SdpOffer::from_sdp_string(&j.offer).map_err(|e| JoinError::BadOffer(e.to_string()))?;
        let now = Instant::now();
        let mut rtc = rtc_config().build(now);
        for addr in &self.candidates {
            let c = Candidate::host(*addr, "udp").map_err(|e| JoinError::BadOffer(e.to_string()))?;
            rtc.add_local_candidate(c);
        }
        let answer = rtc.sdp_api().accept_offer(offer).map_err(|e| JoinError::BadOffer(e.to_string()))?;
        let sources = sources(&rtc, &j.mids)?;
        let session = self.next_session.fetch_add(1, Ordering::Relaxed);
        let state = VoiceState {
            guild_id: j.guild_id,
            channel_id: Some(j.channel_id),
            user_id: j.user_id,
            self_mute: j.self_mute || j.self_deaf,
            self_deaf: j.self_deaf,
            streaming: false,
        };
        let client = run::Client::new(rtc, now, session, &j, sources);
        let mut users = self.shared.lock();
        if self.others(&users, &j) >= self.max_members {
            return Err(JoinError::Full);
        }
        // Sous le verrou : la boucle reçoit les ordres dans l'ordre des états.
        tx.send(Cmd::Join(Box::new(client))).map_err(|_| JoinError::Stopped)?;
        let previous = users.insert(j.user_id, Entry { session, state: state.clone() });
        // Passé d'un serveur à un autre : l'ancien doit apprendre le départ.
        if let Some(p) = previous.filter(|p| p.state.guild_id != j.guild_id) {
            (self.shared.notify)(left(&p.state));
        }
        (self.shared.notify)(state);
        Ok(answer.to_sdp_string())
    }

    /// Les autres participants du salon que `j` veut rejoindre.
    fn others(&self, users: &HashMap<Id, Entry>, j: &Join) -> usize {
        users.values().filter(|e| e.state.channel_id == Some(j.channel_id) && e.state.user_id != j.user_id).count()
    }

    /// Quitte le vocal ; faux si l'utilisateur n'y était pas.
    pub fn leave(&self, user: Id) -> bool {
        let Some(session) = self.shared.release(user, None) else { return false };
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Leave { session });
        }
        true
    }

    /// Les états vocaux d'un serveur.
    pub fn states(&self, guild: Id) -> Vec<VoiceState> {
        let mut out: Vec<VoiceState> =
            self.shared.lock().values().filter(|e| e.state.guild_id == guild).map(|e| e.state.clone()).collect();
        out.sort_by_key(|s| s.user_id);
        out
    }

    /// L'état vocal d'un utilisateur, s'il est connecté.
    pub fn state(&self, user: Id) -> Option<VoiceState> {
        self.shared.lock().get(&user).map(|e| e.state.clone())
    }
}

impl Drop for Sfu {
    fn drop(&mut self) {
        // Sans émetteur, la boucle s'arrête à son prochain tour (au plus quelques dizaines de ms).
        self.tx.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Les trois pistes de l'offre, vérifiées : elles existent, sont de la bonne nature, et le
/// navigateur les envoie.
fn sources(rtc: &Rtc, mids: &VoiceMids) -> Result<[Mid; 3], JoinError> {
    let wanted = [&mids.mic, &mids.screen, &mids.screen_audio];
    let mut out = [Mid::new(); 3];
    for (i, (kind, raw)) in Kind::ALL.iter().zip(wanted).enumerate() {
        let bad = |why: &str| JoinError::BadOffer(format!("mid {raw:?} ({kind:?}) : {why}"));
        if raw.is_empty() || raw.len() > 16 || !raw.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(bad("invalide"));
        }
        let mid = Mid::from(raw.as_str());
        let media = rtc.media(mid).ok_or_else(|| bad("absent de l'offre"))?;
        if media.kind() != kind.media() || !media.direction().is_receiving() {
            return Err(bad("nature ou direction inattendue"));
        }
        if out[..i].contains(&mid) {
            return Err(bad("en double"));
        }
        out[i] = mid;
    }
    Ok(out)
}

/// ICE-lite (le navigateur fait les vérifications), et des codecs que tous les navigateurs
/// décodent : ce que l'un envoie, les autres doivent pouvoir le lire tel quel.
fn rtc_config() -> RtcConfig {
    static CRYPTO: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    let crypto = CRYPTO.get_or_init(|| Arc::new(str0m::crypto::from_feature_flags())).clone();
    Rtc::builder()
        .set_ice_lite(true)
        .set_crypto_provider(crypto)
        .clear_codecs()
        .enable_opus(true, false)
        .enable_vp8(true)
        .enable_h264(true)
}

/// Adresses IPv4 de cet ordinateur sur ses réseaux, sans boucle locale ni lien local.
pub fn interface_ips() -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = Vec::new();
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        let ip = i.ip();
        if !i.is_loopback() && !i.is_link_local() && ip.is_ipv4() && !ip.is_unspecified() && !ips.contains(&ip) {
            ips.push(ip);
        }
    }
    ips
}
