//! Vocal natif de l'appli de bureau Linux, vu de la page.
//!
//! Le navigateur intégré de Linux (WebKitGTK de la distribution) n'a pas WebRTC. L'appli de bureau
//! y fait alors l'appel à la place de la page : un moteur Rust (`kraken-voice-native`, voir
//! `replica/native-voice.md`) capte le micro, parle au SFU du serveur et joue le son. La page, elle,
//! garde la session : elle poste l'offre à `POST /channels/{id}/voice` (comme avec WebRTC), rend la
//! réponse au moteur, puis le questionne quelques fois par seconde et lui passe ce que la personne
//! règle. Les sept commandes Tauri `native_voice_*` sont la seule surface (`apps/desktop/src/native_voice.rs`) ;
//! l'exemple `bridge` du moteur les sert en HTTP pour les tests de bout en bout.
//!
//! Déroulement d'un appel (`Voice::connect_native`) : `native_voice_start` (options : micro et sortie
//! choisis, gain, volume, micro et son coupés d'après la page) rend le `VoiceJoin` à poster tel quel ;
//! la réponse du serveur part dans `native_voice_answer` ; `follow` relève ensuite `native_voice_poll`
//! (état, qui parle, niveau du micro, panne audio) ; `push` règle micro coupé, son coupé, gain et volume
//! par `native_voice_set` ; `stop` quitte. Appuyer pour parler ferme le micro par un gain nul : pour
//! le serveur, on n'est pas coupé (comme avec WebRTC, où seule la piste se ferme).
//!
//! `start` et `stop` passent par une file à un seul lecteur : le moteur ne tient qu'un appel, et deux
//! ordres partis à la suite (quitter puis rejoindre) doivent arriver dans cet ordre. Les rappels d'un
//! appel abandonné sont écartés par le numéro de connexion (`voice::current`), comme avec WebRTC.
//!
//! Ce que le moteur ne fait pas, et que la page dit en toutes lettres : caméra et partage d'écran (ni
//! envoi ni réception), annulation d'écho et réduction du bruit.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gloo_timers::future::TimeoutFuture;
use kraken_proto::{Id, VoiceJoin};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::i18n::t;
use crate::tk;
use crate::voice::{self, Voice, current};
use crate::voice_settings as vs;

/// Le micro choisi (identifiant du moteur ; vide ou absent : celui du système), la sortie, le gain du micro
/// et le volume de sortie. Les clés du vocal du navigateur (`kraken.mic`…) ne servent pas ici : leurs
/// identifiants viennent de `enumerateDevices`, ceux-ci du moteur.
pub const MIC: &str = "kraken.native.mic";
pub const SPEAKER: &str = "kraken.native.speaker";
pub const GAIN: &str = "kraken.native.gain";
pub const VOLUME: &str = "kraken.native.volume";
/// Les bornes du moteur (`MAX_MIC_GAIN`, `MAX_OUTPUT_VOLUME`) ; 1 : inchangé.
pub const MAX_GAIN: f32 = 4.0;
pub const MAX_VOLUME: f32 = 2.0;
/// Pas des curseurs de réglage.
pub const LEVEL_STEP: f32 = 0.05;

/// Pause entre deux relevés de l'état de l'appel.
const POLL_MS: u32 = 120;
/// Relevés qui échouent à la suite avant de tenir l'appel pour perdu.
const POLL_FAILURES: u32 = 5;

// Les textes ci-dessous sont des clés de traduction : `t(…)` à l'affichage.
/// Ce que la page dit de la caméra et du partage d'écran, partout où ils manquent.
pub const NO_VIDEO: &str = tk!(
    "Pas de caméra ni de partage d'écran dans l'appli Linux pour l'instant : ouvre cette page dans ton navigateur."
);
/// Quelqu'un partage son écran ou allume sa caméra : l'image n'arrive pas jusqu'ici.
pub const CANT_SHOW: &str = tk!(
    "Écran partagé ou caméra allumée chez {noms} : l'image ne s'affiche pas dans l'appli Linux pour l'instant. Ouvre cette page dans ton navigateur pour la voir."
);
pub const NO_ECHO_CANCEL: &str =
    tk!("Pas d'annulation d'écho ni de réduction de bruit dans l'appli Linux pour l'instant : un casque évite l'écho.");
pub const BY_DEFAULT: &str = tk!("{nom} (par défaut)");
pub const PERCENT: &str = tk!("{n} %");
/// Le micro manque et la personne veut le rouvrir : il faut un nouvel appel (le moteur ne le change pas à chaud).
pub const NO_MIC: &str =
    tk!("Aucun micro n'est utilisable : tu es en sourdine. Branche-en un, puis quitte le salon et rejoins-le.");
const AUDIO_FAILED: &str = tk!(
    "L'audio ne répond plus : le micro ou les haut-parleurs ont été débranchés ou repris par une autre application."
);
const NO_ANSWER: &str = tk!("Le vocal natif ne répond pas.");
/// La connexion n'a jamais abouti, ou le serveur l'a fermée : les mêmes mots que pour WebRTC.
const SETUP_FAILED: &str =
    tk!("Connexion impossible : le port UDP du vocal est sans doute bloqué entre toi et le serveur.");
const CLOSED_BY_SERVER: &str = tk!("Connexion vocale coupée par le serveur, ou reprise depuis un autre appareil.");
const LOST: &str = tk!("Connexion vocale perdue.");

/// Les phrases que le moteur écrit lui-même (`kraken-voice-native/src/control.rs`, `apps/desktop/src/native_voice.rs`) :
/// elles sont traduites par leur texte, qui est donc la clé. Les écrire ici les met sous le contrôle de
/// `scripts/i18n-check.py` ; un texte du moteur qui change sans que celui-ci suive reste en français.
const ENGINE_TEXTS: &[&str] = &[
    tk!("Impossible d'ouvrir une connexion réseau pour le vocal."),
    tk!("Le vocal natif n'a pas pu démarrer."),
    tk!("La réponse du serveur est illisible."),
    tk!("Le micro ou les haut-parleurs sont inutilisables."),
    tk!("Le vocal est arrêté."),
    tk!("Aucun appel vocal en cours."),
    tk!("Micro indisponible : tu entres micro coupé."),
    tk!("Haut-parleurs indisponibles : tu n'entendras personne."),
    tk!("Ni micro ni haut-parleurs utilisables : vérifie le son du système."),
    tk!("Cette version de KRAKEN n'a pas de pilote audio."),
    tk!("Le vocal natif n'existe que sous Linux."),
    tk!("Le vocal natif s'est arrêté brutalement."),
];

thread_local! {
    /// Le moteur natif est là (réponse de `native_voice_available`). `ArcRwSignal` : il n'appartient à aucun
    /// composant, donc rien ne le libère avec une vue.
    static AVAILABLE: ArcRwSignal<bool> = ArcRwSignal::new(false);
    /// Un appel natif est demandé (de `start` à `stop`) : le quitter arrête le moteur.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// La réponse du serveur est donnée : l'état peut se régler.
    static READY: Cell<bool> = const { Cell::new(false) };
    /// Le moteur est entré sans micro : le rouvrir demande un nouvel appel.
    static NO_MIC_NOW: Cell<bool> = const { Cell::new(false) };
    /// Un `set` est en route ; un autre réglage a pu arriver depuis (il part à la suite, avec les valeurs du moment).
    static PUSHING: Cell<bool> = const { Cell::new(false) };
    static DIRTY: Cell<bool> = const { Cell::new(false) };
    static JOBS: RefCell<Option<mpsc::UnboundedSender<Job>>> = const { RefCell::new(None) };
}

type Job = Pin<Box<dyn Future<Output = ()>>>;

// ───── ce que la page sait du moteur ─────

/// Le moteur natif est là. Suit le signal : à lire dans une vue ou un effet.
pub fn available() -> bool {
    AVAILABLE.with(|a| a.get())
}

/// Le moteur natif est là, sans suivre le signal (code asynchrone et gestionnaires).
pub fn available_now() -> bool {
    AVAILABLE.with(|a| a.get_untracked())
}

/// Au démarrage de la page : dans l'appli de bureau sans WebRTC, demande si le moteur natif est utilisable
/// (Linux avec un pilote audio). Un navigateur, Windows et macOS n'appellent rien.
pub fn init() {
    if !voice::desktop() || voice::browser_webrtc() {
        return;
    }
    spawn_local(async {
        let yes = invoke("native_voice_available", json!({})).await.ok().and_then(|v| v.as_bool());
        if yes == Some(true) {
            AVAILABLE.with(|a| a.set(true));
        }
    });
}

/// Le micro manque à l'appel en cours (il est entré micro coupé).
pub fn no_mic() -> bool {
    NO_MIC_NOW.get()
}

// ───── les commandes ─────

/// Appelle une commande `native_voice_*`. Une commande qui échoue rend une phrase française, que la page affiche
/// (traduite quand c'est une de celles du moteur) ; sans réponse de l'appli, une phrase de la page.
async fn invoke(cmd: &str, args: serde_json::Value) -> Result<JsValue, String> {
    let broken = || t(NO_ANSWER);
    let (internals, f) = voice::tauri().ok_or_else(broken)?;
    let args = js_sys::JSON::parse(&args.to_string()).map_err(|_| broken())?;
    let promise = f.call2(&internals, &cmd.into(), &args).map_err(|_| broken())?;
    let promise: js_sys::Promise = promise.dyn_into().map_err(|_| broken())?;
    JsFuture::from(promise).await.map_err(|e| match e.as_string() {
        Some(message) => t(&message),
        None => broken(),
    })
}

/// La réponse d'une commande lue comme `T` (`null` : `None` pour un `Option`).
fn read<T: DeserializeOwned>(value: &JsValue) -> Option<T> {
    let text = js_sys::JSON::stringify(value).ok()?.as_string()?;
    serde_json::from_str(&text).ok()
}

/// Met `f` à la queue : les travaux s'exécutent un par un, dans l'ordre où ils ont été demandés. Le travail
/// est posé dans la file tout de suite (sans attendre qu'on lise le résultat) : on peut ne pas attendre.
fn enqueue<T: 'static>(f: impl Future<Output = T> + 'static) -> impl Future<Output = Option<T>> {
    let (tx, rx) = oneshot::channel();
    let job: Job = Box::pin(async move {
        let _ = tx.send(f.await);
    });
    JOBS.with_borrow_mut(|jobs| {
        let queue = jobs.get_or_insert_with(|| {
            let (queue, mut jobs) = mpsc::unbounded::<Job>();
            spawn_local(async move {
                while let Some(job) = jobs.next().await {
                    job.await;
                }
            });
            queue
        });
        let _ = queue.unbounded_send(job);
    });
    async move { rx.await.ok() }
}

/// Lance l'appel natif (le précédent s'arrête) ; rend le corps de `POST /channels/{id}/voice`.
async fn start(opts: serde_json::Value) -> Result<VoiceJoin, String> {
    ACTIVE.set(true);
    READY.set(false);
    let started = enqueue(async move { invoke("native_voice_start", json!({ "opts": opts })).await }).await;
    let value = started.ok_or_else(|| t(NO_ANSWER))??;
    read::<VoiceJoin>(&value).ok_or_else(|| t(NO_ANSWER))
}

/// Donne au moteur la réponse du serveur, sauf si l'appel `n` est déjà abandonné (un autre est peut-être parti
/// depuis : la réponse ne serait plus pour lui).
async fn answer(n: u64, sdp: String) -> Result<(), String> {
    let done = enqueue(async move {
        if !current(n) {
            return Ok(());
        }
        invoke("native_voice_answer", json!({ "sdp": sdp })).await.map(|_| ())
    })
    .await;
    done.unwrap_or_else(|| Err(t(NO_ANSWER)))
}

/// Quitte l'appel natif, s'il y en a un (sans attendre la fin). La file garde l'ordre avec un `start` qui suivrait.
pub fn stop() {
    if !ACTIVE.replace(false) {
        return;
    }
    READY.set(false);
    NO_MIC_NOW.set(false);
    drop(enqueue(async { invoke("native_voice_stop", json!({})).await.map(|_| ()) }));
}

/// L'état de l'appel natif, `None` sans appel.
async fn poll() -> Result<Option<Snapshot>, String> {
    invoke("native_voice_poll", json!({})).await.map(|v| read::<Snapshot>(&v))
}

// ───── ce que le moteur rend ─────

/// Où en est la connexion (`{kind: "connecting" | "connected" | "lost", reason?}`).
#[derive(Deserialize, Default)]
struct State {
    kind: String,
    reason: Option<String>,
}

/// Ce que `native_voice_poll` rend, sans le diagnostic.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Snapshot {
    state: State,
    speaking: Vec<Id>,
    self_speaking: bool,
    mic_level: f32,
    audio_error: Option<String>,
}

/// Un périphérique audio que le moteur sait ouvrir.
#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct Device {
    pub name: String,
    pub id: String,
    #[serde(default)]
    pub is_default: bool,
}

/// Micros et haut-parleurs, le défaut du système en premier.
#[derive(Deserialize, Default, Clone, PartialEq, Debug)]
pub struct Devices {
    pub inputs: Vec<Device>,
    pub outputs: Vec<Device>,
}

/// Les micros et les haut-parleurs du système, pour Réglages › Voix et vidéo.
pub async fn devices() -> Result<Devices, String> {
    let list = invoke("native_voice_devices", json!({})).await?;
    read::<Devices>(&list).ok_or_else(|| t(NO_ANSWER))
}

// ───── réglages gardés sur l'appareil ─────

/// Un niveau gardé (`kraken.native.gain`…), de 0 à `max` ; 1 sans choix valable.
pub fn stored_level(key: &str, max: f32) -> f32 {
    vs::stored(key).and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(1.0, |v| v.clamp(0.0, max))
}

/// Garde un niveau ; 1 (inchangé) n'a pas besoin d'être gardé.
pub fn remember_level(key: &str, level: f32) {
    let changed = (level - 1.0).abs() > f32::EPSILON;
    vs::remember(key, changed.then(|| level.to_string()).as_deref());
}

/// Un niveau sur 100 : « 100 % » pour 1.
pub fn percent(level: f32) -> String {
    crate::i18n::tf(PERCENT, &[("n", &((level * 100.0).round() as i64).to_string())])
}

// ───── l'appel, côté page ─────

/// Les phrases que le moteur écrit sur le micro ou les haut-parleurs, ou une phrase générale pour tout le reste
/// (le détail d'une panne de pilote est en anglais et technique : il va dans la console).
fn audio_text(error: &str) -> String {
    if ENGINE_TEXTS.contains(&error) {
        return t(error);
    }
    web_sys::console::warn_1(&format!("vocal natif : {error}").into());
    t(AUDIO_FAILED)
}

/// Pourquoi l'appel est perdu, d'après ce que dit le moteur : un texte de la page, jamais le motif brut (il va dans la console).
fn lost_reason(engine: &str) -> &'static str {
    web_sys::console::warn_1(&format!("vocal natif : appel perdu ({engine})").into());
    match engine {
        "connexion jamais établie" => SETUP_FAILED,
        "fermée par le serveur" => CLOSED_BY_SERVER,
        _ => LOST,
    }
}

impl Voice {
    /// Les options de `native_voice_start` : micro et sortie choisis dans les réglages (un choix qui n'existe
    /// plus retombe sur celui du système, sans rien dire, comme avec le navigateur), gain et volume, micro et
    /// son coupés comme la page les tient.
    async fn native_options(self) -> serde_json::Value {
        let (mut mic, mut speaker) = (vs::stored(MIC), vs::stored(SPEAKER));
        if (mic.is_some() || speaker.is_some())
            && let Ok(list) = devices().await
        {
            mic = mic.filter(|m| list.inputs.iter().any(|d| d.id == *m));
            speaker = speaker.filter(|s| list.outputs.iter().any(|d| d.id == *s));
        }
        json!({
            "mic_device": mic,
            "speaker_device": speaker,
            "mic_gain": self.native_mic_gain(),
            "output_volume": self.native_volume.get_untracked(),
            "self_mute": self.mic_off(),
            "self_deaf": self.deaf.get_untracked(),
        })
    }

    /// Le gain à passer au moteur : celui qu'on a réglé, nul tant qu'appuyer pour parler garde le micro fermé.
    fn native_mic_gain(&self) -> f32 {
        if self.ptt_closed() { 0.0 } else { self.native_gain.get_untracked() }
    }

    /// Entre dans le salon (ou l'appel) par le moteur natif : `start`, l'offre au serveur, sa réponse au moteur,
    /// puis `follow`. Une erreur est une phrase à montrer sous « Rejoindre ».
    pub(crate) async fn connect_native(self, n: u64, channel: Id) -> Result<(), String> {
        let opts = self.native_options().await;
        if !current(n) {
            return Ok(());
        }
        let muted_before = self.mic_off();
        let join = start(opts).await?;
        if !current(n) {
            // `teardown` a mis l'arrêt dans la file, derrière ce démarrage.
            return Ok(());
        }
        // Sans micro, le moteur entre micro coupé : la page le montre aussi (l'avis vient du premier relevé).
        NO_MIC_NOW.set(join.self_mute && !muted_before);
        if no_mic() {
            self.mute.set(true);
        }
        let reply = crate::api::voice_join(channel, &join).await.map_err(|e| e.message)?;
        if !current(n) {
            // Parti pendant la demande : le serveur ne doit pas garder cette connexion.
            if self.room.get_untracked().is_none() {
                let _ = crate::api::voice_leave().await;
            }
            return Ok(());
        }
        answer(n, reply.sdp).await?;
        if !current(n) {
            return Ok(());
        }
        READY.set(true);
        // Ce qui a bougé pendant la connexion (micro coupé, gain…) n'a pas pu partir : tout est redit une fois.
        self.push_native();
        spawn_local(self.follow(n));
        Ok(())
    }

    /// Relève l'état de l'appel tant que la connexion `n` est la bonne.
    async fn follow(self, n: u64) {
        let (mut failures, mut said) = (0, None::<String>);
        while current(n) {
            TimeoutFuture::new(POLL_MS).await;
            if !current(n) {
                break;
            }
            let snapshot = match poll().await {
                Ok(Some(s)) => s,
                // Le moteur n'a plus d'appel (l'appli de bureau l'a arrêté) : la page n'a plus rien à suivre.
                Ok(None) => {
                    if current(n) {
                        self.lost(LOST);
                    }
                    break;
                }
                Err(_) => {
                    failures += 1;
                    if failures >= POLL_FAILURES && current(n) {
                        self.lost(LOST);
                        break;
                    }
                    continue;
                }
            };
            failures = 0;
            if !current(n) {
                break;
            }
            if !self.apply_snapshot(snapshot, &mut said) {
                break;
            }
        }
    }

    /// Pose l'état relevé dans les signaux de la page. Faux : l'appel est perdu, on arrête de relever.
    fn apply_snapshot(self, s: Snapshot, said: &mut Option<String>) -> bool {
        match s.state.kind.as_str() {
            "lost" => {
                self.lost(lost_reason(s.state.reason.as_deref().unwrap_or_default()));
                return false;
            }
            "connected" if self.status.get_untracked() == voice::Status::Connecting => {
                self.status.set(voice::Status::Connected)
            }
            _ => {}
        }
        // Qui parle : ceux que le moteur entend, et moi quand mon micro est ouvert et au-dessus du seuil.
        let mut speaking = s.speaking;
        let me = self.me.with_untracked(|m| m.as_ref().map(|u| u.id));
        speaking.extend(me.filter(|_| s.self_speaking));
        speaking.sort();
        speaking.dedup();
        if self.speaking.with_untracked(|now| *now != speaking) {
            self.speaking.set(speaking);
        }
        // Le niveau du micro : un changement visible seulement, pas un rendu par relevé.
        if (self.mic_level.get_untracked() - s.mic_level).abs() > 0.02 {
            self.mic_level.set(s.mic_level);
        }
        // Micro ou haut-parleurs en panne : un avis, une fois par panne.
        if s.audio_error != *said {
            if let Some(error) = &s.audio_error {
                self.notice.set(Some(audio_text(error)));
            }
            *said = s.audio_error;
        }
        true
    }

    /// Passe au moteur ce que la personne règle : micro coupé, son coupé, gain (nul quand appuyer pour parler garde
    /// le micro fermé) et volume. Les réglages qui se suivent ne font pas un appel chacun : celui en route se
    /// termine, puis un seul repart avec les valeurs du moment.
    pub(crate) fn push_native(self) {
        if !READY.get() {
            return;
        }
        if PUSHING.replace(true) {
            DIRTY.set(true);
            return;
        }
        spawn_local(async move {
            while READY.get() {
                DIRTY.set(false);
                let patch = json!({
                    "self_mute": self.mic_off(),
                    "self_deaf": self.deaf.get_untracked(),
                    "mic_gain": self.native_mic_gain(),
                    "output_volume": self.native_volume.get_untracked(),
                });
                if let Err(e) = invoke("native_voice_set", json!({ "patch": patch })).await {
                    web_sys::console::warn_1(&format!("vocal natif : réglage refusé ({e})").into());
                }
                if !DIRTY.get() {
                    break;
                }
            }
            PUSHING.set(false);
        });
    }
}
