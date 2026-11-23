//! La seule surface que la page web utilise : un [`Controller`] qui porte au plus un appel vocal
//! natif, et des types qui passent en JSON tels quels. L'appli de bureau en fait des commandes Tauri
//! (`native_voice_*`, une ligne chacune) ; l'exemple `bridge` les sert en HTTP pour les tests de bout
//! en bout, sans Tauri ni carte son.
//!
//! # Déroulement vu de la page
//!
//! 1. [`Controller::start`] (avec [`StartOpts`]) : arrête l'appel précédent, ouvre le micro et les
//!    haut-parleurs, prépare WebRTC et rend un `VoiceJoin` (`sdp`, `mids`, `self_mute`, `self_deaf`), le
//!    corps de `POST /channels/{id}/voice` que la page poste telle quelle ;
//! 2. la réponse du serveur est rendue à [`Controller::answer`] ;
//! 3. [`Controller::poll`] (quelques fois par seconde) relève le [`Snapshot`] : connexion, qui est là,
//!    qui parle, niveaux ; [`Controller::set`] règle micro coupé, son coupé, gain, volume ;
//! 4. [`Controller::stop`] quitte (la page prévient aussi le serveur par `DELETE /channels/{id}/voice`) ;
//!    [`Controller::devices`] liste les périphériques pour les réglages.
//!
//! Changer de micro ou de haut-parleurs en cours d'appel demande de relancer ([`Controller::start`]
//! arrête l'appel en cours) : le moteur ne les change pas à chaud.
//!
//! Toutes les méthodes rendent des erreurs en français, courtes et sans détail technique (le détail
//! est dans `tracing`, au niveau `warn`) : la page peut les afficher telles quelles. Elles ne bloquent
//! que le temps d'ouvrir un périphérique ([`Controller::start`], [`Controller::devices`]) ou d'attendre
//! la fin du moteur, une demi-seconde au plus ([`Controller::stop`], [`Controller::start`]) : l'appli
//! de bureau les lance hors du fil principal.
//!
//! # Mode synthétique
//!
//! [`Controller::synthetic`] remplace le micro par une sinusoïde et les haut-parleurs par un puits qui
//! garde ce qu'il reçoit : aucun matériel, donc utilisable en CI. [`Controller::stats`] dit alors ce
//! que le puits a entendu (nombre d'échantillons, niveau, énergie à des fréquences données) : un test
//! qui fait parler deux appels de fréquences différentes vérifie ainsi que chacun entend l'autre.

use std::net::Ipv4Addr;
use std::sync::{Mutex, MutexGuard};

use kraken_proto::VoiceJoin;
use serde::{Deserialize, Serialize};

use crate::analysis::{rms, tone_share_at};
use crate::audio::{Capture, DeviceInfo, Playback, SAMPLE_RATE, Sink, SinkPlayback, ToneCapture};
use crate::{Config, Error, NativeVoice, Snapshot};

/// Amplitude crête du micro synthétique.
const SYNTHETIC_AMPLITUDE: f32 = 0.3;
/// Ce que garde le puits synthétique : 10 s de son (le total, lui, est compté depuis le début).
const SINK_KEEP: usize = 10 * SAMPLE_RATE as usize;
/// Fenêtre de [`Controller::stats`] : la dernière demi-seconde (24 000 échantillons : un nombre entier
/// de périodes pour 440 comme pour 880 Hz, donc des mesures nettes).
pub const STATS_WINDOW: usize = SAMPLE_RATE as usize / 2;

const NO_CALL: &str = "Aucun appel vocal en cours.";
#[cfg(not(feature = "device"))]
const NO_DRIVER: &str = "Cette version de KRAKEN n'a pas de pilote audio.";

/// Ce que la page demande en lançant un appel ([`Controller::start`]). Tout est facultatif en JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StartOpts {
    /// Le micro (nom ou identifiant, voir [`Controller::devices`]) ; `None` : celui du système.
    pub mic_device: Option<String>,
    /// Les haut-parleurs (de même) ; `None` : ceux du système.
    pub speaker_device: Option<String>,
    /// Gain du micro, de 0 à [`crate::MAX_MIC_GAIN`] ; 1 : inchangé.
    pub mic_gain: f32,
    /// Volume de sortie, de 0 à [`crate::MAX_OUTPUT_VOLUME`] ; 1 : inchangé.
    pub output_volume: f32,
    /// Micro coupé au départ.
    pub self_mute: bool,
    /// Son coupé au départ (ce qui coupe aussi le micro).
    pub self_deaf: bool,
}

impl Default for StartOpts {
    fn default() -> Self {
        Self {
            mic_device: None,
            speaker_device: None,
            mic_gain: 1.0,
            output_volume: 1.0,
            self_mute: false,
            self_deaf: false,
        }
    }
}

/// Un réglage en cours d'appel ([`Controller::set`]) : seuls les champs présents changent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatePatch {
    pub self_mute: Option<bool>,
    pub self_deaf: Option<bool>,
    pub mic_gain: Option<f32>,
    pub output_volume: Option<f32>,
}

/// Les périphériques audio ([`Controller::devices`]), le défaut du système en premier.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Devices {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

/// L'énergie d'une fréquence dans ce qu'a entendu le puits synthétique.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToneShare {
    /// La fréquence demandée, en Hz.
    pub hz: f32,
    /// Part de l'énergie du signal qui est à cette fréquence, de 0 à 1 (Goertzel) : près de 1 pour
    /// une sinusoïde pure à `hz`, près de 0 pour tout autre son.
    pub share: f64,
}

/// Ce que le puits du mode synthétique a entendu ([`Controller::stats`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Échantillons (48 kHz, mono) reçus depuis le début de l'appel.
    pub samples: u64,
    /// Échantillons analysés : les [`STATS_WINDOW`] derniers, ou moins au début de l'appel (les mesures
    /// ne sont nettes qu'avec la fenêtre pleine).
    pub window: usize,
    /// Valeur efficace de la fenêtre (0 : silence ; une sinusoïde d'amplitude `a` donne `a / √2`).
    pub rms: f32,
    /// Une entrée par fréquence demandée, dans l'ordre.
    pub tones: Vec<ToneShare>,
}

enum Mode {
    /// Vrais micro et haut-parleurs (cpal).
    Device,
    /// Sinusoïde de cette fréquence (Hz) en guise de micro, puits en guise de haut-parleurs.
    Synthetic(f32),
}

/// L'appel en cours.
struct Call {
    voice: NativeVoice,
    /// Mode synthétique : ce que le puits a reçu.
    sink: Option<Sink>,
    /// Ce qui a manqué à l'ouverture (micro absent…), rendu par [`Controller::poll`] tant que le moteur
    /// n'a pas de panne audio à signaler lui-même.
    warning: Option<String>,
}

/// Le micro et les haut-parleurs ouverts pour un appel.
struct Audio {
    capture: Option<Box<dyn Capture>>,
    playback: Option<Box<dyn Playback>>,
    sink: Option<Sink>,
    warning: Option<String>,
}

/// Porte au plus un appel vocal natif ; voir le module. `Send + Sync` : un seul, partagé, suffit à
/// l'appli (`OnceLock`).
pub struct Controller {
    mode: Mode,
    ips: Vec<Ipv4Addr>,
    /// Sérialise [`Controller::start`] et [`Controller::stop`], qui durent : `poll`, `set` et `answer`
    /// n'attendent jamais un démarrage (ils voient « pas d'appel » jusqu'à ce qu'il soit prêt).
    starting: Mutex<()>,
    call: Mutex<Option<Call>>,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    /// Vrais micro et haut-parleurs (sans la fonction `device`, [`Controller::start`] refuse).
    pub fn new() -> Self {
        Self::with_mode(Mode::Device)
    }

    /// Mode synthétique : une sinusoïde de `freq_hz` Hz en guise de micro, un puits en guise de
    /// haut-parleurs ([`Controller::stats`]). Aucun matériel.
    pub fn synthetic(freq_hz: f32) -> Self {
        Self::with_mode(Mode::Synthetic(freq_hz))
    }

    fn with_mode(mode: Mode) -> Self {
        Self { mode, ips: Vec::new(), starting: Mutex::new(()), call: Mutex::new(None) }
    }

    /// Adresses IPv4 locales à utiliser (une prise UDP chacune) au lieu de celles des interfaces
    /// réseau : `127.0.0.1` pour un serveur qui tourne sur la même machine (tests).
    pub fn with_ips(mut self, ips: Vec<Ipv4Addr>) -> Self {
        self.ips = ips;
        self
    }

    /// Le vocal natif est utilisable : il y a un pilote audio, ou le mode est synthétique.
    pub fn available(&self) -> bool {
        matches!(self.mode, Mode::Synthetic(_)) || cfg!(feature = "device")
    }

    /// Lance un appel (le précédent est d'abord arrêté) et rend ce que la page poste à
    /// `POST /channels/{id}/voice`. Un micro ou des haut-parleurs introuvables n'empêchent pas d'entrer
    /// (micro coupé, ou sans son) : [`Snapshot::audio_error`] le dit ; les deux à la fois, si.
    pub fn start(&self, opts: StartOpts) -> Result<VoiceJoin, String> {
        let _starting = lock(&self.starting);
        self.stop_call();
        let audio = self.open_audio(&opts)?;
        // Sans micro, on entre micro coupé (comme le client web quand le navigateur le refuse).
        let self_mute = opts.self_mute || audio.capture.is_none();
        let config = Config {
            capture: audio.capture,
            playback: audio.playback,
            ips: self.ips.clone(),
            self_mute,
            self_deaf: opts.self_deaf,
            mic_gain: opts.mic_gain,
            output_volume: opts.output_volume,
            ..Config::default()
        };
        let (voice, offer) = NativeVoice::start(config).map_err(failure)?;
        let join = offer.into_join(self_mute, opts.self_deaf);
        *lock(&self.call) = Some(Call { voice, sink: audio.sink, warning: audio.warning });
        Ok(join)
    }

    /// Donne au moteur la réponse SDP du serveur (le corps de la réponse à `POST /channels/{id}/voice`) ;
    /// il se connecte alors, [`Controller::poll`] le dit.
    pub fn answer(&self, sdp: &str) -> Result<(), String> {
        let call = lock(&self.call);
        call.as_ref().ok_or(NO_CALL)?.voice.accept_answer(sdp).map_err(failure)
    }

    /// Règle l'appel en cours : seuls les champs présents de `patch` changent. Les valeurs hors limites
    /// sont ramenées dedans, les nombres invalides ignorés.
    pub fn set(&self, patch: StatePatch) -> Result<(), String> {
        let call = lock(&self.call);
        let voice = &call.as_ref().ok_or(NO_CALL)?.voice;
        if patch.self_mute.is_some() || patch.self_deaf.is_some() {
            let now = voice.snapshot();
            voice.set_state(patch.self_mute.unwrap_or(now.self_mute), patch.self_deaf.unwrap_or(now.self_deaf));
        }
        if let Some(gain) = patch.mic_gain {
            voice.set_mic_gain(gain);
        }
        if let Some(volume) = patch.output_volume {
            voice.set_output_volume(volume);
        }
        Ok(())
    }

    /// L'état de l'appel, ou `None` s'il n'y en a pas. Un appel perdu reste là (état « perdu » et son
    /// motif) jusqu'à [`Controller::stop`] ou un nouveau [`Controller::start`].
    pub fn poll(&self) -> Option<Snapshot> {
        let call = lock(&self.call);
        let call = call.as_ref()?;
        let mut snapshot = call.voice.snapshot();
        if snapshot.audio_error.is_none() {
            snapshot.audio_error.clone_from(&call.warning);
        }
        Some(snapshot)
    }

    /// Quitte l'appel en cours (le serveur voit le départ) et libère le son. Sans effet s'il n'y en a pas.
    pub fn stop(&self) {
        let _starting = lock(&self.starting);
        self.stop_call();
    }

    /// Micros et haut-parleurs disponibles. Le mode synthétique en annonce un de chaque.
    pub fn devices(&self) -> Result<Devices, String> {
        match self.mode {
            Mode::Synthetic(_) => {
                let fake = |name: &str, id: &str| DeviceInfo { name: name.into(), id: id.into(), is_default: true };
                Ok(Devices {
                    inputs: vec![fake("Micro synthétique", "synthetic:mic")],
                    outputs: vec![fake("Haut-parleurs synthétiques", "synthetic:speaker")],
                })
            }
            Mode::Device => list_devices(),
        }
    }

    /// Mode synthétique : ce que le puits a entendu, avec la part d'énergie à chacune des fréquences
    /// `freqs` (Hz). `None` hors du mode synthétique, ou sans appel.
    pub fn stats(&self, freqs: &[f32]) -> Option<Stats> {
        let sink = lock(&self.call).as_ref()?.sink.clone()?;
        let window = sink.last(STATS_WINDOW);
        let rate = f64::from(SAMPLE_RATE);
        Some(Stats {
            samples: sink.total(),
            window: window.len(),
            rms: rms(&window),
            tones: freqs
                .iter()
                .map(|&hz| ToneShare { hz, share: tone_share_at(&window, f64::from(hz), rate) })
                .collect(),
        })
    }

    /// Arrête l'appel en cours, hors du verrou de `call` (l'arrêt attend le fil du moteur).
    fn stop_call(&self) {
        let old = lock(&self.call).take();
        if let Some(call) = old {
            call.voice.stop();
        }
    }

    fn open_audio(&self, opts: &StartOpts) -> Result<Audio, String> {
        match self.mode {
            Mode::Synthetic(freq) => {
                let (playback, sink) = SinkPlayback::bounded(SINK_KEEP);
                Ok(Audio {
                    capture: Some(Box::new(ToneCapture::new(freq, SYNTHETIC_AMPLITUDE))),
                    playback: Some(Box::new(playback)),
                    sink: Some(sink),
                    warning: None,
                })
            }
            Mode::Device => open_devices(opts),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Une erreur du moteur pour la page : un message court, le détail dans `tracing`.
fn failure(e: Error) -> String {
    tracing::warn!("vocal natif : {e}");
    match e {
        Error::Io(_) => "Impossible d'ouvrir une connexion réseau pour le vocal.",
        Error::Rtc(_) | Error::Codec(_) => "Le vocal natif n'a pas pu démarrer.",
        Error::Sdp(_) => "La réponse du serveur est illisible.",
        Error::Device(_) => "Le micro ou les haut-parleurs sont inutilisables.",
        Error::Stopped => "Le vocal est arrêté.",
    }
    .into()
}

#[cfg(feature = "device")]
fn open_devices(opts: &StartOpts) -> Result<Audio, String> {
    use crate::device::{CpalCapture, CpalPlayback};

    let capture = CpalCapture::open(opts.mic_device.as_deref())
        .map(|c| Box::new(c) as Box<dyn Capture>)
        .map_err(|e| tracing::warn!("vocal natif : micro impossible à ouvrir ({e})"))
        .ok();
    let playback = CpalPlayback::open(opts.speaker_device.as_deref())
        .map(|p| Box::new(p) as Box<dyn Playback>)
        .map_err(|e| tracing::warn!("vocal natif : haut-parleurs impossibles à ouvrir ({e})"))
        .ok();
    let warning = match (capture.is_some(), playback.is_some()) {
        (true, true) => None,
        (false, true) => Some("Micro indisponible : tu entres micro coupé."),
        (true, false) => Some("Haut-parleurs indisponibles : tu n'entendras personne."),
        (false, false) => return Err("Ni micro ni haut-parleurs utilisables : vérifie le son du système.".into()),
    };
    Ok(Audio { capture, playback, sink: None, warning: warning.map(Into::into) })
}

#[cfg(not(feature = "device"))]
fn open_devices(_: &StartOpts) -> Result<Audio, String> {
    Err(NO_DRIVER.into())
}

#[cfg(feature = "device")]
fn list_devices() -> Result<Devices, String> {
    use crate::device::{input_devices, output_devices};
    Ok(Devices { inputs: input_devices().map_err(failure)?, outputs: output_devices().map_err(failure)? })
}

#[cfg(not(feature = "device"))]
fn list_devices() -> Result<Devices, String> {
    Err(NO_DRIVER.into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_controller_can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Controller>();
    }

    #[test]
    fn start_options_are_all_optional_in_json() {
        let opts: StartOpts = serde_json::from_value(json!({})).unwrap();
        assert_eq!(opts, StartOpts::default());
        assert_eq!((opts.mic_gain, opts.output_volume), (1.0, 1.0), "gain et volume : inchangés par défaut");
        let opts: StartOpts = serde_json::from_value(json!({
            "mic_device": "Casque", "speaker_device": null, "mic_gain": 2.5, "self_mute": true, "inconnu": 1
        }))
        .unwrap();
        assert_eq!(opts.mic_device.as_deref(), Some("Casque"));
        assert_eq!((opts.speaker_device, opts.mic_gain, opts.output_volume), (None, 2.5, 1.0));
        assert!(opts.self_mute && !opts.self_deaf);
        let patch: StatePatch = serde_json::from_value(json!({"self_deaf": true, "output_volume": 0.5})).unwrap();
        assert_eq!(patch, StatePatch { self_deaf: Some(true), output_volume: Some(0.5), ..StatePatch::default() });
    }

    #[test]
    fn without_a_call_everything_is_quiet_and_says_so() {
        let c = Controller::synthetic(440.0);
        assert!(c.poll().is_none());
        assert!(c.stats(&[440.0]).is_none());
        assert_eq!(c.answer("v=0").unwrap_err(), NO_CALL);
        assert_eq!(c.set(StatePatch::default()).unwrap_err(), NO_CALL);
        c.stop();
        assert!(c.poll().is_none());
    }

    #[test]
    fn availability_and_devices() {
        let c = Controller::synthetic(440.0);
        assert!(c.available());
        let d = c.devices().unwrap();
        assert_eq!((d.inputs.len(), d.outputs.len()), (1, 1));
        assert!(d.inputs[0].is_default && d.outputs[0].is_default);
        assert_eq!(Controller::new().available(), cfg!(feature = "device"));
    }

    #[cfg(not(feature = "device"))]
    #[test]
    fn without_the_audio_driver_the_real_mode_refuses_politely() {
        let c = Controller::new();
        assert_eq!(c.start(StartOpts::default()).unwrap_err(), NO_DRIVER);
        assert_eq!(c.devices().unwrap_err(), NO_DRIVER);
        assert!(c.poll().is_none());
    }

    #[test]
    fn errors_are_short_french_sentences_without_raw_detail() {
        let all = [
            failure(Error::Io(std::io::Error::other("os error 98 EADDRINUSE"))),
            failure(Error::Rtc("ice agent: Foo(1)".into())),
            failure(Error::Sdp("parse error at line 3".into())),
            failure(Error::Codec("opus: -3".into())),
            failure(Error::Device("alsa: snd_pcm_open".into())),
            failure(Error::Stopped),
        ];
        for message in all {
            assert!(message.ends_with('.') && message.len() < 80, "{message}");
            for raw in ["os error", "EADDR", "ice agent", "parse error", "opus:", "alsa", "snd_pcm", "Foo"] {
                assert!(!message.contains(raw), "{message}");
            }
        }
    }
}
