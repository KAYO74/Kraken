//! Le vocal de KRAKEN sans navigateur : un client natif pour le SFU de `kraken-voice`, pour
//! l'appli de bureau (Linux, macOS, Windows) quand son navigateur intégré ne peut pas faire l'appel : pas de WebRTC
//! (WebKitGTK de Linux), pas de micro pour une page en http (WKWebView de macOS).
//!
//! C'est un « navigateur » de plus pour le SFU : même offre (quatre transceivers `sendonly`, micro,
//! image de l'écran, son de l'écran et caméra, dans cet ordre, et le canal de données `kraken`),
//! même signalisation (`kraken-proto` : [`VoiceJoin`], `VoiceClientMsg`, `VoiceServerMsg`),
//! aucun changement du serveur. Voix seulement : les trois transceivers de l'écran et de la caméra
//! restent en place et ne servent jamais ; aucune piste d'image reçue n'est décodée ni touchée.
//!
//! # Déroulement
//!
//! 1. [`NativeVoice::start`] ouvre les prises UDP, prépare WebRTC et rend l'[`Offer`] ;
//! 2. la page (qui a la session) la poste à `POST /channels/{id}/voice` ([`Offer::into_join`] donne
//!    le corps de la requête) et reçoit la réponse du serveur ;
//! 3. [`NativeVoice::accept_answer`] la donne au moteur, qui se connecte ;
//! 4. la page relève [`NativeVoice::snapshot`] (connexion, qui est là, qui parle, niveaux) et règle
//!    [`NativeVoice::set_state`], [`NativeVoice::set_mic_gain`], [`NativeVoice::set_output_volume`] ;
//! 5. [`NativeVoice::stop`] (ou la destruction) quitte proprement ; la page prévient aussi le serveur
//!    (`DELETE /channels/{id}/voice`).
//!
//! Tout le travail est sur un fil dédié : l'API est `Send + Sync` et ne bloque jamais (sauf
//! [`NativeVoice::stop`], qui attend la fin du fil, une demi-seconde au plus).
//!
//! # Le son
//!
//! Le moteur ne voit que des trames de 20 ms (48 kHz, mono, `f32` : [`audio::FRAME`] échantillons)
//! par les traits [`Capture`] et [`Playback`]. Avec la fonction `device` (par défaut),
//! [`device::CpalCapture`] et [`device::CpalPlayback`] les branchent sur de vrais périphériques
//! (cpal : ALSA sous Linux, donc aussi PulseAudio et PipeWire, CoreAudio sous macOS, WASAPI sous Windows) ; sans carte son, [`audio::ToneCapture`] et
//! [`audio::SinkPlayback`] les remplacent, ce que font les tests.
//!
//! Émission : micro, gain, Opus (voix, 32 kbit/s, correction d'erreur intégrée), un paquet toutes les
//! 20 ms, rien du tout micro coupé. Réception : un décodeur et un tampon de gigue par participant
//! (60 ms d'attente, perte masquée), mélange sur la trame de sortie avec écrêtage doux, volume. La
//! lecture suit l'horloge du périphérique de sortie. « Parle » : une des 15 dernières trames (300 ms)
//! dépasse -40 dBFS.
//!
//! # La page
//!
//! [`control::Controller`] est la seule surface que la page web utilise (par les commandes Tauri de l'appli
//! de bureau) : mêmes appels, avec des types qui passent en JSON, des erreurs en français et un mode
//! synthétique pour les tests de bout en bout.
//!
//! Pas d'annulation d'écho (casque conseillé), pas d'image (écran, caméra), pas de chiffrement de
//! bout en bout (le serveur n'en a pas non plus).

mod analysis;
pub mod audio;
pub mod control;
#[cfg(feature = "device")]
pub mod device;
mod engine;
mod jitter;
mod mix;
pub mod resample;
#[cfg(feature = "device")]
mod ring;
#[cfg(test)]
mod testutil;

use std::fmt;
use std::io;
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Instant;

use kraken_proto::{Id, VOICE_DATA_CHANNEL, VoiceJoin, VoiceMids};
use serde::{Deserialize, Serialize};
use str0m::change::SdpAnswer;
use str0m::crypto::CryptoProvider;
use str0m::media::{Direction, MediaKind};
use str0m::{Candidate, Rtc, RtcConfig};

pub use audio::{Capture, Playback};
use engine::{Cmd, Init, Shared, Sock};

/// Débit de la voix envoyée, en bit/s.
pub const DEFAULT_BITRATE: u32 = 32_000;
/// Paquets attendus avant de commencer à jouer la voix de quelqu'un : 3, soit 60 ms.
pub const DEFAULT_JITTER_FRAMES: usize = 3;
/// Gain maximal du micro (quatre fois).
pub const MAX_MIC_GAIN: f32 = 4.0;
/// Volume de sortie maximal (deux fois).
pub const MAX_OUTPUT_VOLUME: f32 = 2.0;

/// Ce qui peut empêcher de démarrer, ou d'utiliser le moteur.
#[derive(Debug)]
pub enum Error {
    /// Prises UDP ou fil impossibles à ouvrir.
    Io(io::Error),
    /// WebRTC (str0m) a refusé quelque chose, au démarrage.
    Rtc(String),
    /// Réponse du serveur illisible.
    Sdp(String),
    /// Opus (encodeur) impossible.
    Codec(String),
    /// Micro ou haut-parleurs : introuvables ou impossibles à ouvrir.
    Device(String),
    /// Le moteur est arrêté (fin demandée, ou connexion perdue : voir [`Snapshot::state`]).
    Stopped,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "réseau ou fil impossible : {e}"),
            Self::Rtc(e) => write!(f, "WebRTC : {e}"),
            Self::Sdp(e) => write!(f, "réponse du serveur illisible : {e}"),
            Self::Codec(e) => write!(f, "Opus : {e}"),
            Self::Device(e) => write!(f, "périphérique audio : {e}"),
            Self::Stopped => f.write_str("vocal arrêté"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Réglages au démarrage.
pub struct Config {
    /// Le micro ; `None` : on n'envoie rien (écoute seule, ou micro introuvable).
    pub capture: Option<Box<dyn Capture>>,
    /// Les haut-parleurs ; `None` : on n'entend rien, mais les indicateurs « parle » marchent.
    pub playback: Option<Box<dyn Playback>>,
    /// Adresses IPv4 locales à utiliser (une prise UDP chacune, annoncées comme candidats « host »).
    /// Vide : celles des interfaces réseau (sans boucle locale ni lien local), ou 127.0.0.1 s'il n'y en a aucune.
    pub ips: Vec<Ipv4Addr>,
    /// Micro coupé au départ ; à dire aussi au serveur (voir [`Offer::into_join`]).
    pub self_mute: bool,
    /// Son coupé au départ (ce qui coupe aussi le micro) ; à dire aussi au serveur.
    pub self_deaf: bool,
    /// Gain du micro, de 0 à [`MAX_MIC_GAIN`] ; 1 : inchangé.
    pub mic_gain: f32,
    /// Volume de sortie, de 0 à [`MAX_OUTPUT_VOLUME`] ; 1 : inchangé.
    pub output_volume: f32,
    /// Paquets à avoir reçus d'une personne avant de la jouer (tampon de gigue), 1 au moins.
    pub jitter_frames: usize,
    /// Débit de la voix envoyée, en bit/s.
    pub bitrate: u32,
}

impl Default for Config {
    /// Ni micro ni haut-parleurs (à remplir), interfaces du réseau, rien de coupé, gain et volume à 1.
    fn default() -> Self {
        Self {
            capture: None,
            playback: None,
            ips: Vec::new(),
            self_mute: false,
            self_deaf: false,
            mic_gain: 1.0,
            output_volume: 1.0,
            jitter_frames: DEFAULT_JITTER_FRAMES,
            bitrate: DEFAULT_BITRATE,
        }
    }
}

/// L'offre à poster au serveur.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    /// Offre SDP : quatre transceivers `sendonly` (micro, écran, son de l'écran, caméra) et le canal de données.
    pub sdp: String,
    /// Les `mid` de ces quatre lignes.
    pub mids: VoiceMids,
}

impl Offer {
    /// Le corps de `POST /channels/{id}/voice`.
    pub fn into_join(self, self_mute: bool, self_deaf: bool) -> VoiceJoin {
        VoiceJoin { sdp: self.sdp, mids: self.mids, self_mute, self_deaf }
    }
}

/// Où en est la connexion.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum ConnectionState {
    /// Pas encore connecté (réponse du serveur attendue, ou ICE et DTLS en cours).
    #[default]
    Connecting,
    /// Connecté, canal de données ouvert : on entend et on est entendu.
    Connected,
    /// Fini, et pourquoi (arrêt demandé, connexion perdue, serveur parti…). Définitif.
    Lost(String),
}

/// L'état du vocal, relevé par [`NativeVoice::snapshot`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub state: ConnectionState,
    /// Les participants dont on reçoit le micro (triés).
    pub participants: Vec<Id>,
    /// Ceux d'entre eux qui parlent (triés).
    pub speaking: Vec<Id>,
    /// On parle (micro ouvert et au-dessus du seuil).
    pub self_speaking: bool,
    /// Micro coupé ([`NativeVoice::set_state`]).
    pub self_mute: bool,
    /// Son coupé ([`NativeVoice::set_state`]).
    pub self_deaf: bool,
    /// Niveau du micro, de 0 à 1 (échelle en dB, de -60 à 0 dBFS), après le gain, micro coupé ou non.
    pub mic_level: f32,
    /// Niveau de ce qui est joué, de 0 à 1 (même échelle), après le volume.
    pub output_level: f32,
    /// Micro ou haut-parleurs en panne (périphérique débranché…), sans que le vocal soit perdu.
    pub audio_error: Option<String>,
    /// Paquets de voix envoyés depuis le début (diagnostic ; n'augmente pas micro coupé).
    pub packets_sent: u64,
    /// Paquets de voix reçus des autres depuis le début (diagnostic).
    pub packets_received: u64,
}

impl Snapshot {
    /// La fin : plus personne, plus de niveau.
    pub(crate) fn lose(&mut self, reason: impl Into<String>) {
        self.state = ConnectionState::Lost(reason.into());
        self.participants.clear();
        self.speaking.clear();
        self.self_speaking = false;
        self.mic_level = 0.0;
        self.output_level = 0.0;
    }
}

/// Une connexion vocale native. Se ferme avec [`NativeVoice::stop`] ou en la détruisant.
pub struct NativeVoice {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Cmd>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl NativeVoice {
    /// Ouvre les prises UDP, prépare WebRTC (une clé DTLS : quelques millisecondes), lance le
    /// fil du moteur et rend l'offre à poster au serveur. Rien n'est envoyé avant
    /// [`NativeVoice::accept_answer`] ; sans réponse, la connexion est abandonnée au bout de 20 s.
    pub fn start(config: Config) -> Result<(Self, Offer), Error> {
        let sockets = open_sockets(&config.ips)?;
        let now = Instant::now();
        let mut rtc = rtc_config().build(now);
        for s in &sockets {
            let candidate = Candidate::host(s.addr, "udp").map_err(|e| Error::Rtc(e.to_string()))?;
            rtc.add_local_candidate(candidate);
        }
        let mut api = rtc.sdp_api();
        let mic = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let screen = api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
        let screen_audio = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let camera = api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
        api.add_channel(VOICE_DATA_CHANNEL.into());
        let (offer, pending) = api.apply().ok_or_else(|| Error::Rtc("offre vide".into()))?;
        let offer = Offer {
            sdp: offer.to_sdp_string(),
            mids: VoiceMids {
                mic: mic.to_string(),
                screen: screen.to_string(),
                screen_audio: screen_audio.to_string(),
                camera: camera.to_string(),
            },
        };

        let encoder = new_encoder(config.bitrate)?;
        let mic_gain = clamp(config.mic_gain, MAX_MIC_GAIN, 1.0);
        let output_volume = clamp(config.output_volume, MAX_OUTPUT_VOLUME, 1.0);
        let shared = Arc::new(Shared::new(mic_gain, output_volume, config.self_mute, config.self_deaf));
        let (commands, receiver) = mpsc::channel();
        let thread = engine::spawn(Init {
            rtc,
            pending,
            sockets,
            mic,
            encoder,
            capture: config.capture,
            playback: config.playback,
            commands: receiver,
            shared: shared.clone(),
            jitter_frames: config.jitter_frames.max(1),
            self_mute: config.self_mute,
            self_deaf: config.self_deaf,
        })?;
        Ok((Self { shared, commands, thread: Mutex::new(Some(thread)) }, offer))
    }

    /// Donne au moteur la réponse SDP du serveur ; il se connecte alors. Une réponse illisible est
    /// refusée tout de suite ; une réponse que WebRTC refuse met fin à la connexion (voir
    /// [`Snapshot::state`]). Ne bloque pas.
    pub fn accept_answer(&self, sdp: &str) -> Result<(), Error> {
        let answer = SdpAnswer::from_sdp_string(sdp).map_err(|e| Error::Sdp(e.to_string()))?;
        self.commands.send(Cmd::Answer(answer)).map_err(|_| Error::Stopped)
    }

    /// Coupe le micro et/ou le son. Le micro coupé n'envoie plus rien ; le son coupé n'en laisse rien
    /// sortir (et coupe aussi le micro). Effet immédiat ici ; le serveur est prévenu par un message
    /// `state` (regroupés si on change vite). Ne bloque pas.
    pub fn set_state(&self, self_mute: bool, self_deaf: bool) {
        self.shared.set_state(self_mute, self_deaf);
        let _ = self.commands.send(Cmd::State { self_mute, self_deaf });
    }

    /// Gain du micro, de 0 à [`MAX_MIC_GAIN`] (une valeur hors limites est ramenée dedans ; un nombre
    /// invalide est ignoré). Ne bloque pas.
    pub fn set_mic_gain(&self, gain: f32) {
        if gain.is_finite() {
            self.shared.set_mic_gain(gain.clamp(0.0, MAX_MIC_GAIN));
        }
    }

    /// Volume de sortie, de 0 à [`MAX_OUTPUT_VOLUME`] (mêmes règles). Ne bloque pas.
    pub fn set_output_volume(&self, volume: f32) {
        if volume.is_finite() {
            self.shared.set_output_volume(volume.clamp(0.0, MAX_OUTPUT_VOLUME));
        }
    }

    /// L'état du vocal, mis à jour toutes les 20 ms. Ne bloque pas.
    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot()
    }

    /// Quitte : fermeture propre de la connexion (le serveur voit le départ), arrêt du fil, libération des
    /// prises et des périphériques. Attend la fin du fil (une demi-seconde au plus). Sans effet si c'est déjà fait.
    pub fn stop(&self) {
        let mut thread = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        let Some(handle) = thread.take() else { return };
        let _ = self.commands.send(Cmd::Stop);
        if handle.join().is_err() {
            tracing::warn!("vocal natif : le fil du moteur s'est arrêté sur une panique");
        }
    }
}

impl Drop for NativeVoice {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Une valeur de réglage ramenée entre 0 et `max`, ou `default` si elle n'a pas de sens.
fn clamp(value: f32, max: f32, default: f32) -> f32 {
    if value.is_finite() { value.clamp(0.0, max) } else { default }
}

/// ICE complet (c'est nous qui faisons l'offre), Opus sans RED (comme le SFU), et VP8 : WebRTC refuse
/// une ligne vidéo sans codec, or l'offre garde les lignes de l'écran et de la caméra ; il ne sert jamais.
/// Le son n'est pas réordonné par str0m (c'est le tampon de gigue qui le fait), et pas de statistiques.
fn rtc_config() -> RtcConfig {
    static CRYPTO: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    let crypto = CRYPTO.get_or_init(|| Arc::new(str0m::crypto::from_feature_flags())).clone();
    Rtc::builder()
        .set_crypto_provider(crypto)
        .clear_codecs()
        .enable_opus(true, false)
        .enable_vp8(true)
        .set_reordering_size_audio(0)
        .set_stats_interval(None)
}

/// Opus pour la voix : mono, 48 kHz, débit fixé, correction d'erreur intégrée (5 % de perte attendue).
fn new_encoder(bitrate: u32) -> Result<opus::Encoder, Error> {
    let codec = |e: opus::Error| Error::Codec(e.to_string());
    let mut e = opus::Encoder::new(audio::SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip).map_err(codec)?;
    e.set_bitrate(opus::Bitrate::Bits(bitrate.clamp(6_000, 128_000) as i32)).map_err(codec)?;
    e.set_signal(opus::Signal::Voice).map_err(codec)?;
    e.set_inband_fec(true).map_err(codec)?;
    e.set_packet_loss_perc(5).map_err(codec)?;
    Ok(e)
}

/// Adresses IPv4 des interfaces, sans boucle locale ni lien local ; 127.0.0.1 faute d'autre.
fn interface_ips() -> Vec<Ipv4Addr> {
    let mut ips: Vec<Ipv4Addr> = Vec::new();
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        if let std::net::IpAddr::V4(ip) = i.ip()
            && !i.is_loopback()
            && !i.is_link_local()
            && !ip.is_unspecified()
            && !ips.contains(&ip)
        {
            ips.push(ip);
        }
    }
    if ips.is_empty() {
        ips.push(Ipv4Addr::LOCALHOST);
    }
    ips
}

/// Une prise UDP non bloquante par adresse (port libre au choix du système).
fn open_sockets(ips: &[Ipv4Addr]) -> Result<Vec<Sock>, Error> {
    let ips = if ips.is_empty() { interface_ips() } else { ips.to_vec() };
    let mut sockets: Vec<Sock> = Vec::new();
    let mut last_error = None;
    for ip in ips {
        let opened = UdpSocket::bind((ip, 0)).and_then(|socket| {
            socket.set_nonblocking(true)?;
            let addr = socket.local_addr()?;
            Ok(Sock { socket, addr })
        });
        match opened {
            Ok(s) => sockets.push(s),
            Err(e) => {
                tracing::debug!("vocal natif : prise UDP sur {ip} impossible ({e})");
                last_error = Some(e);
            }
        }
    }
    if sockets.is_empty() {
        let e = last_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "aucune adresse IPv4"));
        return Err(Error::Io(e));
    }
    Ok(sockets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NativeVoice>();
        assert_send_sync::<Snapshot>();
    }

    #[test]
    fn the_offer_has_the_four_transceivers_in_order_and_the_data_channel() {
        let (voice, offer) =
            NativeVoice::start(Config { ips: vec![Ipv4Addr::LOCALHOST], ..Config::default() }).unwrap();
        let m = &offer.mids;
        assert!(offer.sdp.contains("m=application"), "canal de données");
        let audio = offer.sdp.matches("m=audio").count();
        let video = offer.sdp.matches("m=video").count();
        assert_eq!((audio, video), (2, 2), "micro et son de l'écran, écran et caméra");
        let mids = [&m.mic, &m.screen, &m.screen_audio, &m.camera];
        for mid in mids {
            assert!(offer.sdp.contains(&format!("a=mid:{mid}")), "mid {mid}");
        }
        assert!(offer.sdp.contains("a=sendonly"));
        assert!(!offer.sdp.contains("a=sendrecv"));
        // Les lignes sont dans l'ordre : micro (audio), écran (vidéo), son de l'écran (audio), caméra (vidéo).
        let order: Vec<usize> = mids.iter().map(|mid| offer.sdp.find(&format!("a=mid:{mid}")).unwrap()).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]));
        assert!(offer.sdp.contains("opus"), "Opus est proposé");
        assert!(!offer.sdp.to_lowercase().contains("red/"), "pas de RED");
        let join = offer.clone().into_join(true, false);
        assert_eq!((join.sdp.as_str(), join.self_mute, join.self_deaf), (offer.sdp.as_str(), true, false));
        assert_eq!(voice.snapshot().state, ConnectionState::Connecting);
        voice.stop();
        assert!(matches!(voice.snapshot().state, ConnectionState::Lost(_)));
    }

    #[test]
    fn a_bad_answer_is_refused_at_once() {
        let (voice, _) = NativeVoice::start(Config { ips: vec![Ipv4Addr::LOCALHOST], ..Config::default() }).unwrap();
        assert!(matches!(voice.accept_answer("ceci n'est pas du SDP"), Err(Error::Sdp(_))));
        voice.stop();
        assert!(matches!(voice.accept_answer("v=0"), Err(Error::Sdp(_) | Error::Stopped)));
    }

    #[test]
    fn settings_are_kept_in_range() {
        assert_eq!(clamp(9.0, MAX_MIC_GAIN, 1.0), MAX_MIC_GAIN);
        assert_eq!(clamp(-1.0, MAX_MIC_GAIN, 1.0), 0.0);
        assert_eq!(clamp(f32::NAN, MAX_MIC_GAIN, 1.0), 1.0);
        assert_eq!(clamp(f32::INFINITY, MAX_MIC_GAIN, 1.0), 1.0);
        assert_eq!(clamp(0.5, MAX_MIC_GAIN, 1.0), 0.5);
    }

    #[test]
    fn unusable_addresses_are_an_error() {
        // 192.0.2.1 (documentation) n'est à personne : impossible de s'y lier.
        let r = NativeVoice::start(Config { ips: vec![Ipv4Addr::new(192, 0, 2, 1)], ..Config::default() });
        assert!(matches!(r, Err(Error::Io(_))));
    }

    #[test]
    fn snapshot_serializes_for_the_page() {
        let s = Snapshot { state: ConnectionState::Lost("fini".into()), ..Snapshot::default() };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["state"], serde_json::json!({"kind": "lost", "reason": "fini"}));
        assert_eq!(serde_json::to_value(ConnectionState::Connected).unwrap(), serde_json::json!({"kind": "connected"}));
        let back: Snapshot = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }
}
