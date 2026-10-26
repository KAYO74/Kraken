//! Session vocale du navigateur : une seule pour toute l'appli, gardée quand on change de salon.
//! WebRTC vers le SFU du serveur (`kraken-voice`) : l'offre part par REST avec trois pistes
//! `sendonly` (micro, image de l'écran, son de l'écran) et le canal de données `kraken` ; ensuite
//! seul le serveur renégocie, par ce canal. Micro et partage d'écran démarrent et s'arrêtent avec
//! `replaceTrack`, sans renégocier.
//!
//! Les objets JavaScript (connexion, pistes) vivent dans `SESSION`, hors des signaux. Chaque
//! connexion a son numéro (`GEN`) : un rappel d'une connexion fermée ne touche plus à rien.
//!
//! Contrôle à distance : celui qui regarde un écran demande à le contrôler ; le serveur relaie
//! demande, réponse, actions et fin par le canal de données. Seule l'appli de bureau peut être
//! contrôlée : la page lui passe les actions (`remote_input`), et c'est elle qui demande l'accord,
//! dans une fenêtre native, avant de jouer quoi que ce soit. Dans un navigateur, la demande est
//! refusée d'office (« il faut l'appli de bureau »).

use std::cell::{Cell, RefCell};

use futures::StreamExt;
use futures::channel::mpsc;
use gloo_timers::future::TimeoutFuture;
use kraken_proto::{
    ControlRefusal, Id, RemoteInput, User, VOICE_DATA_CHANNEL, VoiceClientMsg, VoiceJoin, VoiceMids, VoiceServerMsg,
    VoiceState,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AnalyserNode, AudioContext, MediaDevices, MediaStream, MediaStreamAudioSourceNode, MediaStreamConstraints,
    MediaStreamTrack, MediaStreamTrackState, MessageEvent, RtcBundlePolicy, RtcConfiguration, RtcDataChannel,
    RtcDataChannelState, RtcPeerConnection, RtcPeerConnectionState, RtcRtpSender, RtcRtpTransceiverDirection,
    RtcRtpTransceiverInit, RtcSdpType, RtcSessionDescriptionInit, RtcTrackEvent,
};

use crate::store::Store;

/// Page en HTTP ailleurs que sur cet ordinateur : le navigateur ne donne ni micro ni écran.
pub const INSECURE: &str = "Connexion non sécurisée : tu peux écouter et regarder, mais parler et partager ton écran \
                            demandent l'instance en HTTPS (ou ouverte sur cet ordinateur, via localhost).";
const MIC_DENIED: &str =
    "Micro refusé ou introuvable : tu es en sourdine. Autorise le micro dans ton navigateur, puis réactive-le.";
const NO_WEBRTC: &str = "Ton navigateur ne permet pas les appels vocaux. Essaie avec un navigateur récent.";
const LOST: &str = "Connexion vocale perdue.";

/// Le salon vocal de la session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Room {
    pub guild: Id,
    pub channel: Id,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    /// Offre envoyée : ICE, DTLS puis canal de données.
    Connecting,
    Connected,
    /// Réseau coupé : on attend qu'il revienne (le serveur abandonne après 10 s).
    Unstable,
    /// Coupé, avec la raison ; « Reconnecter » relance le même salon.
    Lost(String),
}

/// Contrôle à distance, côté spectateur : l'écran que je demande à contrôler, ou que je contrôle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Off,
    Asking(Id),
    On(Id),
}

/// Ce que le spectateur apprend sur sa demande : refusée (et pourquoi), ou terminée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlNote {
    Refused(ControlRefusal),
    Ended,
}

/// Ce que celui qui partage doit savoir d'une demande qu'il n'a pas pu accepter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareNote {
    /// Demande refusée d'office : on est dans un navigateur.
    Desktop,
    /// Une fenêtre ou un onglet est partagé, pas un écran entier.
    Surface,
    /// Ce que l'appli de bureau a répondu (Wayland, autorisation macOS…).
    Message(String),
}

/// Un flux reçu : le micro de quelqu'un, ou son écran (image et son ensemble).
#[derive(Clone)]
pub struct Remote {
    pub user: Id,
    pub screen: bool,
    pub stream: MediaStream,
}

#[derive(Clone, Copy)]
pub struct Voice {
    pub room: RwSignal<Option<Room>>,
    pub status: RwSignal<Status>,
    /// Micro coupé par l'utilisateur ; le son coupé le coupe aussi, sans changer ce choix.
    pub mute: RwSignal<bool>,
    pub deaf: RwSignal<bool>,
    pub streaming: RwSignal<bool>,
    /// L'écran affiché en grand.
    pub watching: RwSignal<Option<Id>>,
    pub remote: RwSignal<Vec<Remote>, LocalStorage>,
    /// Mon écran partagé, pour l'aperçu.
    pub preview: RwSignal<Option<MediaStream>, LocalStorage>,
    /// Avertissement pendant l'appel (micro refusé, page en HTTP…).
    pub notice: RwSignal<Option<String>>,
    /// Échec en rejoignant ce salon (complet, vocal indisponible…).
    pub error: RwSignal<Option<(Id, String)>>,
    /// Qui parle en ce moment dans mon salon (moi compris).
    pub speaking: RwSignal<Vec<Id>>,
    /// Contrôle à distance de l'écran regardé.
    pub control: RwSignal<Control>,
    pub control_note: RwSignal<Option<(Id, ControlNote)>>,
    /// Contrôle à distance de mon écran : qui le demande, qui attend ma confirmation native,
    /// qui l'a.
    pub requests: RwSignal<Vec<Id>>,
    pub confirming: RwSignal<Option<Id>>,
    pub controller: RwSignal<Option<Id>>,
    pub share_note: RwSignal<Option<(Id, ShareNote)>>,
    /// Mon compte (celui du `Store`).
    me: RwSignal<Option<User>>,
}

type Handler = Closure<dyn FnMut(JsValue)>;

struct Session {
    pc: RtcPeerConnection,
    dc: RtcDataChannel,
    /// Micro, image et son de l'écran : les trois pistes de l'offre, dans cet ordre.
    senders: [RtcRtpSender; 3],
    mic: Option<MediaStream>,
    screen: Option<MediaStream>,
    /// Rappels JavaScript, détachés à la fermeture.
    handlers: Vec<Handler>,
    /// Le bouton « Arrêter le partage » du navigateur.
    screen_end: Option<Handler>,
}

/// Contrôle de mon écran accordé : à qui, et le jeton de l'appli de bureau.
struct Granted {
    user: Id,
    token: String,
}

impl Session {
    fn close(self) {
        self.pc.set_ontrack(None);
        self.pc.set_onconnectionstatechange(None);
        self.dc.set_onopen(None);
        self.dc.set_onclose(None);
        self.dc.set_onmessage(None);
        if let Some(s) = &self.screen {
            if let Some(v) = track(s, "video") {
                v.set_onended(None);
            }
            stop_all(s);
        }
        if let Some(m) = &self.mic {
            stop_all(m);
        }
        self.dc.close();
        self.pc.close();
        // Détachés plus haut : JavaScript ne peut plus les appeler.
        drop(self.handlers);
    }
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    /// Numéro de la connexion en cours ; change à chaque connexion et à chaque départ.
    static GEN: Cell<u64> = const { Cell::new(0) };
    static GRANTED: RefCell<Option<Granted>> = const { RefCell::new(None) };
    /// Actions reçues, en attente d'être passées à l'appli de bureau (dans l'ordre).
    static QUEUE: RefCell<Vec<RemoteInput>> = const { RefCell::new(Vec::new()) };
    static PUMPING: Cell<bool> = const { Cell::new(false) };
}

fn bump() -> u64 {
    GEN.with(|g| {
        g.set(g.get() + 1);
        g.get()
    })
}

fn current(n: u64) -> bool {
    GEN.with(Cell::get) == n
}

fn with_session<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.with_borrow_mut(|s| s.as_mut().map(f))
}

/// Le navigateur sait faire du WebRTC.
pub fn supported() -> bool {
    js_sys::Reflect::has(&window(), &"RTCPeerConnection".into()).unwrap_or(false)
}

/// Micro et écran : absents d'une page en HTTP hors de cet ordinateur.
pub fn devices() -> Option<MediaDevices> {
    let d = js_sys::Reflect::get(&window().navigator(), &"mediaDevices".into()).ok()?;
    (!d.is_undefined() && !d.is_null()).then(|| d.unchecked_into())
}

/// Le partage d'écran (absent des navigateurs mobiles).
pub fn can_share() -> bool {
    devices().is_some_and(|d| js_sys::Reflect::has(&d, &"getDisplayMedia".into()).unwrap_or(false))
}

// Attention : `MediaStream::clone` et `MediaStreamTrack::clone` sont les méthodes JavaScript, qui
// créent de nouvelles pistes ; pour une seconde référence au même objet, `Clone::clone(&x)`.

fn track(s: &MediaStream, kind: &str) -> Option<MediaStreamTrack> {
    let list = if kind == "video" { s.get_video_tracks() } else { s.get_audio_tracks() };
    list.get(0).dyn_into().ok()
}

fn stop_all(s: &MediaStream) {
    s.get_tracks().iter().filter_map(|t| t.dyn_into::<MediaStreamTrack>().ok()).for_each(|t| t.stop());
}

/// Un flux reçu qui a encore une piste en marche (le serveur retire celles qui s'arrêtent).
fn live(s: &MediaStream) -> bool {
    s.get_tracks()
        .iter()
        .filter_map(|t| t.dyn_into::<MediaStreamTrack>().ok())
        .any(|t| t.ready_state() == MediaStreamTrackState::Live)
}

/// `<id>.mic` ou `<id>.screen` : à qui est ce flux, et si c'est un écran.
fn parse_stream(id: &str) -> Option<(Id, bool)> {
    let (user, kind) = id.rsplit_once('.')?;
    let screen = match kind {
        "mic" => false,
        "screen" => true,
        _ => return None,
    };
    Some((user.parse().ok()?, screen))
}

async fn microphone(d: &MediaDevices) -> Result<MediaStream, JsValue> {
    let wanted: MediaStreamConstraints =
        js_sys::JSON::parse(r#"{"audio":{"echoCancellation":true,"noiseSuppression":true,"autoGainControl":true}}"#)?
            .unchecked_into();
    Ok(JsFuture::from(d.get_user_media_with_constraints(&wanted)?).await?.unchecked_into())
}

/// Refus ou annulation dans la fenêtre de choix du navigateur : rien à signaler.
fn cancelled(e: &JsValue) -> bool {
    let name = js_sys::Reflect::get(e, &"name".into()).ok().and_then(|n| n.as_string());
    matches!(name.as_deref(), Some("NotAllowedError" | "AbortError"))
}

fn send(msg: &VoiceClientMsg) {
    SESSION.with_borrow(|s| {
        if let Some(s) = s
            && s.dc.ready_state() == RtcDataChannelState::Open
        {
            let _ = s.dc.send_with_str(&serde_json::to_string(msg).unwrap_or_default());
        }
    });
}

/// L'appli de bureau KRAKEN : sa fonction `invoke`, absente d'un navigateur.
fn tauri() -> Option<(JsValue, js_sys::Function)> {
    let internals = js_sys::Reflect::get(&window(), &"__TAURI_INTERNALS__".into()).ok().filter(|v| v.is_object())?;
    let invoke = js_sys::Reflect::get(&internals, &"invoke".into()).ok()?.dyn_into().ok()?;
    Some((internals, invoke))
}

/// Dans l'appli de bureau : mon écran peut être contrôlé à distance.
pub fn desktop() -> bool {
    tauri().is_some()
}

/// Ce que l'appli de bureau répond quand elle refuse.
#[derive(serde::Deserialize)]
struct Refusal {
    reason: String,
    message: String,
}

/// Appelle une commande de l'appli de bureau.
async fn invoke(cmd: &str, args: serde_json::Value) -> Result<JsValue, Refusal> {
    let broken = || Refusal { reason: "unavailable".into(), message: "L'appli de bureau ne répond pas.".into() };
    let (internals, f) = tauri().ok_or_else(broken)?;
    let args = js_sys::JSON::parse(&args.to_string()).map_err(|_| broken())?;
    let promise = f.call2(&internals, &cmd.into(), &args).map_err(|_| broken())?;
    let promise: js_sys::Promise = promise.dyn_into().map_err(|_| broken())?;
    JsFuture::from(promise).await.map_err(|e| {
        js_sys::JSON::stringify(&e)
            .ok()
            .and_then(|s| s.as_string())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(broken)
    })
}

fn refusal(reason: &str) -> ControlRefusal {
    match reason {
        "declined" => ControlRefusal::Declined,
        "busy" => ControlRefusal::Busy,
        "surface" => ControlRefusal::Surface,
        _ => ControlRefusal::Unavailable,
    }
}

/// `width`, `height` et `displaySurface` de la piste partagée.
fn surface(t: &MediaStreamTrack) -> (u32, u32, Option<String>) {
    let s: JsValue = t.get_settings().into();
    let num = |k: &str| js_sys::Reflect::get(&s, &k.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    let kind = js_sys::Reflect::get(&s, &"displaySurface".into()).ok().and_then(|v| v.as_string());
    (num("width"), num("height"), kind)
}

/// Applique une offre du serveur et lui renvoie la réponse.
async fn answer(pc: &RtcPeerConnection, dc: &RtcDataChannel, sdp: &str) -> Result<(), JsValue> {
    let offer = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
    offer.set_sdp(sdp);
    JsFuture::from(pc.set_remote_description(&offer)).await?;
    let answer = JsFuture::from(pc.create_answer()).await?;
    JsFuture::from(pc.set_local_description(answer.unchecked_ref())).await?;
    let sdp = pc.local_description().map(|d| d.sdp()).unwrap_or_default();
    dc.send_with_str(&serde_json::to_string(&VoiceClientMsg::Answer { sdp }).unwrap_or_default())
}

impl Voice {
    pub fn new(me: RwSignal<Option<User>>) -> Self {
        Self {
            room: RwSignal::new(None),
            status: RwSignal::new(Status::Off),
            mute: RwSignal::new(false),
            deaf: RwSignal::new(false),
            streaming: RwSignal::new(false),
            watching: RwSignal::new(None),
            remote: RwSignal::new_local(Vec::new()),
            preview: RwSignal::new_local(None),
            notice: RwSignal::new(None),
            error: RwSignal::new(None),
            speaking: RwSignal::new(Vec::new()),
            control: RwSignal::new(Control::Off),
            control_note: RwSignal::new(None),
            requests: RwSignal::new(Vec::new()),
            confirming: RwSignal::new(None),
            controller: RwSignal::new(None),
            share_note: RwSignal::new(None),
            me,
        }
    }

    fn mic_off(&self) -> bool {
        self.mute.get_untracked() || self.deaf.get_untracked()
    }

    /// En vocal, ou en train d'y entrer : il y a une connexion à quitter.
    fn active(&self) -> bool {
        matches!(self.status.get_untracked(), Status::Connecting | Status::Connected | Status::Unstable)
    }

    /// Rejoint ce salon vocal ; quitte d'abord celui où l'on était.
    pub fn join(self, guild: Id, channel: Id) {
        self.teardown();
        let n = GEN.with(Cell::get);
        self.room.set(Some(Room { guild, channel }));
        self.status.set(Status::Connecting);
        self.notice.set(None);
        self.error.set(None);
        spawn_local(async move {
            if let Err(e) = self.connect(n, channel).await
                && current(n)
            {
                self.teardown();
                self.status.set(Status::Off);
                self.room.set(None);
                self.error.set(Some((channel, e)));
            }
        });
    }

    async fn connect(self, n: u64, channel: Id) -> Result<(), String> {
        if !supported() {
            return Err(NO_WEBRTC.into());
        }
        let mic = match devices() {
            None => {
                self.mute.set(true);
                self.notice.set(Some(INSECURE.into()));
                None
            }
            Some(d) => match microphone(&d).await {
                Ok(m) => Some(m),
                Err(_) => {
                    self.mute.set(true);
                    self.notice.set(Some(MIC_DENIED.into()));
                    None
                }
            },
        };
        if !current(n) {
            mic.iter().for_each(stop_all);
            return Ok(());
        }
        let cfg = RtcConfiguration::new();
        cfg.set_bundle_policy(RtcBundlePolicy::MaxBundle);
        let pc = RtcPeerConnection::new_with_configuration(&cfg).map_err(|_| NO_WEBRTC.to_string())?;
        let init = RtcRtpTransceiverInit::new();
        init.set_direction(RtcRtpTransceiverDirection::Sendonly);
        let mic_track = mic.as_ref().and_then(|m| track(m, "audio"));
        let t_mic = match &mic_track {
            Some(t) => {
                t.set_enabled(!self.mic_off());
                pc.add_transceiver_with_media_stream_track_and_init(t, &init)
            }
            None => pc.add_transceiver_with_str_and_init("audio", &init),
        };
        let t_screen = pc.add_transceiver_with_str_and_init("video", &init);
        let t_sound = pc.add_transceiver_with_str_and_init("audio", &init);
        let dc = pc.create_data_channel(VOICE_DATA_CHANNEL);
        let (tx, rx) = mpsc::unbounded::<String>();
        let handlers = self.wire(n, &pc, &dc, tx);
        SESSION.set(Some(Session {
            pc: pc.clone(),
            dc: dc.clone(),
            senders: [t_mic.sender(), t_screen.sender(), t_sound.sender()],
            mic,
            screen: None,
            handlers,
            screen_end: None,
        }));
        spawn_local(self.offers(n, pc.clone(), dc, rx));
        spawn_local(self.meter(n));

        let broken = |_| "Le navigateur n'a pas pu préparer l'appel.".to_string();
        let offer = JsFuture::from(pc.create_offer()).await.map_err(broken)?;
        JsFuture::from(pc.set_local_description(offer.unchecked_ref())).await.map_err(broken)?;
        if !current(n) {
            return Ok(());
        }
        let body = VoiceJoin {
            sdp: pc.local_description().map(|d| d.sdp()).unwrap_or_default(),
            mids: VoiceMids {
                mic: t_mic.mid().unwrap_or_default(),
                screen: t_screen.mid().unwrap_or_default(),
                screen_audio: t_sound.mid().unwrap_or_default(),
            },
            self_mute: self.mic_off(),
            self_deaf: self.deaf.get_untracked(),
        };
        let reply = crate::api::voice_join(channel, &body).await.map_err(|e| e.message)?;
        if !current(n) {
            // Parti pendant la demande : le serveur ne doit pas garder cette connexion.
            if self.room.get_untracked().is_none() {
                let _ = crate::api::voice_leave().await;
            }
            return Ok(());
        }
        let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        desc.set_sdp(&reply.sdp);
        JsFuture::from(pc.set_remote_description(&desc))
            .await
            .map_err(|_| "La réponse du serveur n'a pas convenu au navigateur.".to_string())?;
        TimeoutFuture::new(15_000).await;
        if current(n) && self.status.get_untracked() == Status::Connecting {
            self.lost("Connexion impossible : le port UDP du vocal est sans doute bloqué entre toi et le serveur.");
        }
        Ok(())
    }

    /// Les rappels de la connexion : pistes reçues, état du réseau, canal de données.
    fn wire(
        self,
        n: u64,
        pc: &RtcPeerConnection,
        dc: &RtcDataChannel,
        offers: mpsc::UnboundedSender<String>,
    ) -> Vec<Handler> {
        let on_track = Handler::new(move |e: JsValue| {
            let e: RtcTrackEvent = e.unchecked_into();
            let Ok(stream) = e.streams().get(0).dyn_into::<MediaStream>() else { return };
            let Some((user, screen)) = parse_stream(&stream.id()) else { return };
            if current(n) {
                self.remote.update(|r| {
                    r.retain(|x| x.user != user || x.screen != screen);
                    r.push(Remote { user, screen, stream });
                });
            }
        });
        let peer = pc.clone();
        let on_state = Handler::new(move |_| {
            if !current(n) {
                return;
            }
            match peer.connection_state() {
                RtcPeerConnectionState::Connected if self.status.get_untracked() == Status::Unstable => {
                    self.status.set(Status::Connected)
                }
                RtcPeerConnectionState::Disconnected if self.status.get_untracked() == Status::Connected => {
                    self.status.set(Status::Unstable);
                    spawn_local(async move {
                        TimeoutFuture::new(10_000).await;
                        if current(n) && self.status.get_untracked() == Status::Unstable {
                            self.lost(LOST);
                        }
                    });
                }
                RtcPeerConnectionState::Failed => self.lost(LOST),
                _ => {}
            }
        });
        let on_open = Handler::new(move |_| {
            if current(n) {
                self.status.set(Status::Connected);
                self.send_state();
            }
        });
        let on_close = Handler::new(move |_| {
            if current(n) {
                self.lost(LOST);
            }
        });
        let on_message = Handler::new(move |e: JsValue| {
            let Some(text) = e.unchecked_into::<MessageEvent>().data().as_string() else { return };
            match serde_json::from_str(&text) {
                Ok(VoiceServerMsg::Offer { sdp }) => {
                    let _ = offers.unbounded_send(sdp);
                }
                Ok(msg) if current(n) => self.on_control(msg),
                _ => {}
            }
        });
        pc.set_ontrack(Some(on_track.as_ref().unchecked_ref()));
        pc.set_onconnectionstatechange(Some(on_state.as_ref().unchecked_ref()));
        dc.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        dc.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        dc.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        vec![on_track, on_state, on_open, on_close, on_message]
    }

    /// Les offres du serveur, une à la fois (il n'en envoie pas d'autre avant la réponse).
    async fn offers(self, n: u64, pc: RtcPeerConnection, dc: RtcDataChannel, mut rx: mpsc::UnboundedReceiver<String>) {
        while let Some(sdp) = rx.next().await {
            if !current(n) {
                return;
            }
            if answer(&pc, &dc, &sdp).await.is_err() {
                if current(n) {
                    self.lost("Le navigateur a refusé une mise à jour de l'appel.");
                }
                return;
            }
            // Pistes arrêtées par le serveur (départ, fin de partage) : on les oublie.
            if current(n) && self.remote.with_untracked(|r| r.iter().any(|x| !live(&x.stream))) {
                self.remote.update(|r| r.retain(|x| live(&x.stream)));
            }
        }
    }

    /// Qui parle : le niveau de chaque micro (reçus et le mien), mesuré par WebAudio quelques
    /// fois par seconde, tant que la connexion `n` est la bonne.
    async fn meter(self, n: u64) {
        struct Tap {
            user: Id,
            stream: MediaStream,
            source: MediaStreamAudioSourceNode,
            level: AnalyserNode,
        }
        let Ok(ctx) = AudioContext::new() else { return };
        let me = self.me.with_untracked(|m| m.as_ref().map(|u| u.id));
        let mut taps: Vec<Tap> = Vec::new();
        let mut heard: Vec<(Id, f64)> = Vec::new();
        let mut buf = vec![0f32; 512];
        while current(n) {
            TimeoutFuture::new(150).await;
            if !current(n) {
                break;
            }
            let mut wanted: Vec<(Id, MediaStream)> = self.remote.with_untracked(|r| {
                r.iter().filter(|x| !x.screen).map(|x| (x.user, Clone::clone(&x.stream))).collect()
            });
            if let (Some(me), Some(mic)) = (me, SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.mic.clone()))) {
                wanted.push((me, mic));
            }
            taps.retain(|t| {
                let keep = wanted.iter().any(|(u, s)| *u == t.user && *s == t.stream);
                if !keep {
                    let _ = t.source.disconnect();
                }
                keep
            });
            for (user, stream) in wanted {
                if taps.iter().any(|t| t.user == user && t.stream == stream) {
                    continue;
                }
                if let (Ok(source), Ok(level)) = (ctx.create_media_stream_source(&stream), ctx.create_analyser()) {
                    level.set_fft_size(512);
                    let _ = source.connect_with_audio_node(&level);
                    taps.push(Tap { user, stream, source, level });
                }
            }
            let now = js_sys::Date::now();
            for t in &taps {
                t.level.get_float_time_domain_data(&mut buf);
                let rms = (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt();
                if rms > 0.02 {
                    heard.retain(|(u, _)| *u != t.user);
                    heard.push((t.user, now));
                }
            }
            // Un court silence entre deux mots ne fait pas clignoter le contour.
            heard.retain(|(_, at)| now - at < 400.0);
            let mut speaking: Vec<Id> = heard.iter().map(|(u, _)| *u).collect();
            speaking.sort();
            if self.speaking.with_untracked(|s| *s != speaking) {
                self.speaking.set(speaking);
            }
        }
        if let Ok(p) = ctx.close() {
            let _ = JsFuture::from(p).await;
        }
    }

    /// Ferme la connexion et oublie ce qu'elle recevait (sans prévenir le serveur).
    fn teardown(self) {
        bump();
        self.release_control(false);
        self.requests.set(Vec::new());
        self.confirming.set(None);
        self.control.set(Control::Off);
        if let Some(s) = SESSION.take() {
            s.close();
        }
        self.streaming.set(false);
        self.watching.set(None);
        self.remote.set(Vec::new());
        self.preview.set(None);
        self.speaking.set(Vec::new());
    }

    fn lost(self, why: &str) {
        self.teardown();
        self.status.set(Status::Lost(why.into()));
    }

    /// Quitte le salon vocal.
    pub fn leave(self) {
        let active = self.active();
        self.hang_up();
        if active {
            spawn_local(async {
                let _ = crate::api::voice_leave().await;
            });
        }
    }

    /// Raccroche sans rien demander au serveur (session fermée) : la connexion fermée suffit à
    /// l'en faire sortir.
    pub fn hang_up(self) {
        self.teardown();
        self.status.set(Status::Off);
        self.room.set(None);
        self.notice.set(None);
    }

    /// Mon état vu par le serveur. S'il me sort du salon où je me crois connecté, ma connexion a
    /// été coupée, ou reprise depuis un autre onglet ou appareil.
    pub fn check(self, s: &VoiceState) {
        let Some(room) = self.room.get_untracked() else { return };
        if s.guild_id == room.guild
            && s.channel_id != Some(room.channel)
            && matches!(self.status.get_untracked(), Status::Connected | Status::Unstable)
        {
            self.lost("Connexion vocale coupée par le serveur, ou reprise depuis un autre appareil.");
        }
    }

    pub fn send_state(&self) {
        send(&VoiceClientMsg::State {
            self_mute: self.mic_off(),
            self_deaf: self.deaf.get_untracked(),
            streaming: self.streaming.get_untracked(),
        });
    }

    /// Micro : le réactiver sous « son coupé » rend aussi le son.
    pub fn toggle_mute(self) {
        if self.deaf.get_untracked() {
            self.deaf.set(false);
            self.mute.set(false);
        } else {
            self.mute.update(|m| *m = !*m);
        }
        if !self.mic_off() {
            self.ensure_mic();
        }
    }

    pub fn toggle_deaf(self) {
        self.deaf.update(|d| *d = !*d);
        if !self.mic_off() {
            self.ensure_mic();
        }
    }

    /// Micro réactivé alors que la session n'en a pas (refusé au départ) : on le redemande.
    fn ensure_mic(self) {
        if with_session(|s| s.mic.is_none()) != Some(true) {
            return;
        }
        let Some(d) = devices() else {
            self.mute.set(true);
            self.notice.set(Some(INSECURE.into()));
            return;
        };
        let n = GEN.with(Cell::get);
        spawn_local(async move {
            match microphone(&d).await {
                Ok(m) if current(n) => {
                    let t = track(&m, "audio");
                    if let Some(t) = &t {
                        t.set_enabled(!self.mic_off());
                    }
                    if let Some(sender) = with_session(|s| {
                        s.mic = Some(Clone::clone(&m));
                        s.senders[0].clone()
                    }) {
                        let _ = JsFuture::from(sender.replace_track(t.as_ref())).await;
                    }
                    self.notice.set(None);
                }
                Ok(m) => stop_all(&m),
                Err(_) => {
                    self.mute.set(true);
                    self.notice.set(Some(MIC_DENIED.into()));
                }
            }
        });
    }

    /// Partage l'écran. À appeler dans le clic : le navigateur l'exige pour ouvrir son choix.
    pub fn share(self) {
        let Some(d) = devices().filter(|_| can_share()) else { return };
        let n = GEN.with(Cell::get);
        let Ok(wanted) = js_sys::JSON::parse(r#"{"video":{"frameRate":{"ideal":30,"max":30}},"audio":true}"#) else {
            return;
        };
        let Ok(asked) = d.get_display_media_with_constraints(wanted.unchecked_ref()) else { return };
        spawn_local(async move {
            let stream: MediaStream = match JsFuture::from(asked).await {
                Ok(s) => s.unchecked_into(),
                Err(e) => {
                    if !cancelled(&e) {
                        self.notice.set(Some("Le partage d'écran n'a pas pu démarrer.".into()));
                    }
                    return;
                }
            };
            let Some(video) = track(&stream, "video").filter(|_| current(n)) else {
                stop_all(&stream);
                return;
            };
            let audio = track(&stream, "audio");
            let end = Handler::new(move |_| {
                if current(n) {
                    self.stop_share();
                }
            });
            video.set_onended(Some(end.as_ref().unchecked_ref()));
            let senders = with_session(|s| {
                if let Some(old) = s.screen.replace(Clone::clone(&stream)) {
                    stop_all(&old);
                }
                s.screen_end = Some(end);
                [s.senders[1].clone(), s.senders[2].clone()]
            });
            let Some([image, sound]) = senders else {
                stop_all(&stream);
                return;
            };
            let _ = JsFuture::from(image.replace_track(Some(&video))).await;
            let _ = JsFuture::from(sound.replace_track(audio.as_ref())).await;
            if current(n) {
                self.preview.set(Some(stream));
                self.streaming.set(true);
            }
        });
    }

    pub fn stop_share(self) {
        let senders = with_session(|s| {
            if let Some(st) = s.screen.take() {
                if let Some(v) = track(&st, "video") {
                    v.set_onended(None);
                }
                stop_all(&st);
            }
            s.screen_end = None;
            [s.senders[1].clone(), s.senders[2].clone()]
        });
        for sender in senders.into_iter().flatten() {
            let _ = sender.replace_track(None);
        }
        self.preview.set(None);
        self.streaming.set(false);
        // Le serveur met fin au contrôle et aux demandes avec le partage ; l'appli de bureau aussi.
        self.release_control(false);
        self.requests.set(Vec::new());
    }

    /// Regarde l'écran de quelqu'un (un seul à la fois), ou plus aucun.
    pub fn watch(self, user: Option<Id>) {
        let before = self.watching.get_untracked();
        if before == user {
            return;
        }
        if let Some(u) = before {
            send(&VoiceClientMsg::Watch { user_id: u, on: false });
        }
        if let Some(u) = user {
            send(&VoiceClientMsg::Watch { user_id: u, on: true });
        }
        // Ne plus regarder met fin au contrôle (le serveur le fait aussi).
        if matches!(self.control.get_untracked(), Control::Asking(u) | Control::On(u) if Some(u) != user) {
            self.control.set(Control::Off);
        }
        self.watching.set(user);
    }

    // ───── contrôle à distance ─────

    /// Un message de contrôle à distance relayé par le serveur.
    fn on_control(self, msg: VoiceServerMsg) {
        match msg {
            VoiceServerMsg::Offer { .. } => {}
            // On regarde mon écran et voudrait le contrôler.
            VoiceServerMsg::ControlRequest { user_id } => {
                if !self.streaming.get_untracked() {
                    return;
                }
                if !desktop() {
                    send(&VoiceClientMsg::ControlAnswer {
                        user_id,
                        accept: false,
                        reason: Some(ControlRefusal::Desktop),
                    });
                    self.share_note.set(Some((user_id, ShareNote::Desktop)));
                    return;
                }
                if !self.requests.with_untracked(|r| r.contains(&user_id)) {
                    self.requests.update(|r| r.push(user_id));
                }
            }
            VoiceServerMsg::ControlAnswer { user_id, accept, reason } => {
                if self.control.get_untracked() != Control::Asking(user_id) {
                    return;
                }
                if accept {
                    self.control.set(Control::On(user_id));
                    self.control_note.set(None);
                } else {
                    self.control.set(Control::Off);
                    let why = reason.unwrap_or(ControlRefusal::Declined);
                    self.control_note.set(Some((user_id, ControlNote::Refused(why))));
                }
            }
            VoiceServerMsg::ControlInput { user_id, input } => {
                let mine = GRANTED.with_borrow(|g| g.as_ref().is_some_and(|g| g.user == user_id));
                if mine && input.valid() {
                    QUEUE.with_borrow_mut(|q| q.push(input));
                    self.pump();
                }
            }
            VoiceServerMsg::ControlEnd { user_id } => {
                // Il contrôlait mon écran, ou le demandait.
                if GRANTED.with_borrow(|g| g.as_ref().is_some_and(|g| g.user == user_id)) {
                    self.release_control(false);
                }
                if self.requests.with_untracked(|r| r.contains(&user_id)) {
                    self.requests.update(|r| r.retain(|u| *u != user_id));
                }
                // Je contrôlais son écran, ou le demandais.
                match self.control.get_untracked() {
                    Control::On(u) if u == user_id => {
                        self.control.set(Control::Off);
                        self.control_note.set(Some((user_id, ControlNote::Ended)));
                    }
                    Control::Asking(u) if u == user_id => self.control.set(Control::Off),
                    _ => {}
                }
            }
        }
    }

    /// Passe les actions reçues à l'appli de bureau, dans l'ordre, un lot à la fois.
    fn pump(self) {
        if PUMPING.get() {
            return;
        }
        PUMPING.set(true);
        spawn_local(async move {
            loop {
                let events = QUEUE.with_borrow_mut(std::mem::take);
                let token = GRANTED.with_borrow(|g| g.as_ref().map(|g| g.token.clone()));
                let (false, Some(token)) = (events.is_empty(), token) else { break };
                let args = serde_json::json!({ "token": token, "events": events });
                if invoke("remote_input", args).await.is_err() {
                    // Accord retiré dans l'appli (menu, page rechargée…) : fin pour tout le monde.
                    if GRANTED.with_borrow(|g| g.as_ref().is_some_and(|g| g.token == token)) {
                        self.release_control(true);
                    }
                    break;
                }
            }
            QUEUE.with_borrow_mut(Vec::clear);
            PUMPING.set(false);
        });
    }

    /// Je regarde l'écran de `user` et demande à le contrôler.
    pub fn request_control(self, user: Id) {
        if self.watching.get_untracked() != Some(user) || self.control.get_untracked() != Control::Off {
            return;
        }
        self.control_note.set(None);
        self.control.set(Control::Asking(user));
        send(&VoiceClientMsg::ControlRequest { user_id: user });
    }

    /// J'arrête de contrôler (ou de demander) ; je continue de regarder.
    pub fn stop_control(self) {
        if let Control::Asking(u) | Control::On(u) = self.control.get_untracked() {
            send(&VoiceClientMsg::ControlStop { user_id: u });
        }
        self.control.set(Control::Off);
    }

    /// Une action sur l'écran que je contrôle.
    pub fn control_input(self, input: RemoteInput) {
        if let Control::On(u) = self.control.get_untracked() {
            send(&VoiceClientMsg::ControlInput { user_id: u, input });
        }
    }

    /// J'accepte que `user` contrôle mon écran : l'appli de bureau me redemande, hors de la page.
    pub fn accept_control(self, user: Id, name: String) {
        if self.confirming.get_untracked().is_some() || !self.requests.with_untracked(|r| r.contains(&user)) {
            return;
        }
        let video = SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.screen.as_ref()).and_then(|m| track(m, "video")));
        let Some(video) = video else { return };
        let (width, height, kind) = surface(&video);
        if kind.as_deref().is_some_and(|k| k != "monitor") {
            self.answer_control(user, false, Some(ControlRefusal::Surface));
            self.share_note.set(Some((user, ShareNote::Surface)));
            return;
        }
        let n = GEN.with(Cell::get);
        self.confirming.set(Some(user));
        self.share_note.set(None);
        spawn_local(async move {
            let args = serde_json::json!({ "name": name, "width": width, "height": height, "surface": kind });
            let result = invoke("remote_begin", args).await;
            if !current(n) {
                if let Ok(token) = result
                    && let Some(token) = token.as_string()
                {
                    let _ = invoke("remote_end", serde_json::json!({ "token": token })).await;
                }
                return;
            }
            self.confirming.set(None);
            let still = self.streaming.get_untracked() && self.requests.with_untracked(|r| r.contains(&user));
            match result.map(|t| t.as_string()) {
                Ok(Some(token)) if still => {
                    GRANTED.set(Some(Granted { user, token }));
                    self.controller.set(Some(user));
                    self.answer_control(user, true, None);
                }
                // Demande retirée pendant la question : l'accord est rendu aussitôt.
                Ok(Some(token)) => {
                    let _ = invoke("remote_end", serde_json::json!({ "token": token })).await;
                }
                Ok(None) => self.answer_control(user, false, Some(ControlRefusal::Unavailable)),
                Err(r) => {
                    let why = refusal(&r.reason);
                    self.answer_control(user, false, Some(why));
                    if why != ControlRefusal::Declined {
                        self.share_note.set(Some((user, ShareNote::Message(r.message))));
                    }
                }
            }
        });
    }

    /// Je refuse que `user` contrôle mon écran.
    pub fn decline_control(self, user: Id) {
        self.answer_control(user, false, None);
    }

    fn answer_control(self, user: Id, accept: bool, reason: Option<ControlRefusal>) {
        self.requests.update(|r| r.retain(|u| *u != user));
        send(&VoiceClientMsg::ControlAnswer { user_id: user, accept, reason });
    }

    /// Reprend la main : plus personne ne contrôle mon écran. `tell_server` : le serveur ne le
    /// sait pas encore (arrêt depuis la page ou l'appli).
    pub fn release_control(self, tell_server: bool) {
        QUEUE.with_borrow_mut(Vec::clear);
        let Some(g) = GRANTED.take() else {
            self.controller.set(None);
            return;
        };
        self.controller.set(None);
        if tell_server {
            send(&VoiceClientMsg::ControlStop { user_id: g.user });
        }
        spawn_local(async move {
            let _ = invoke("remote_end", serde_json::json!({ "token": g.token })).await;
        });
    }
}

/// Ce qui suit la session : la piste du micro et l'état envoyé au serveur suivent micro, son et
/// partage ; l'écran regardé se ferme quand son auteur arrête ; se déconnecter raccroche.
pub fn init(store: Store) {
    let v = store.voice;
    Effect::new(move || {
        // Les trois sont lus à chaque fois : chacun doit relancer l'envoi.
        let (mute, deaf, _) = (v.mute.get(), v.deaf.get(), v.streaming.get());
        let off = mute || deaf;
        if let Some(t) =
            SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.mic.as_ref()).and_then(|m| track(m, "audio")))
        {
            t.set_enabled(!off);
        }
        v.send_state();
    });
    Effect::new(move || {
        let (Some(user), Some(room)) = (v.watching.get(), v.room.get()) else { return };
        let streams = store.guilds.with(|gs| {
            gs.iter().find(|g| g.guild.id == room.guild).is_some_and(|g| {
                g.voice_states.iter().any(|s| s.user_id == user && s.channel_id == Some(room.channel) && s.streaming)
            })
        });
        // Le serveur a déjà arrêté de l'envoyer : pas de « watch off » à faire.
        if !streams {
            v.watching.set(None);
        }
    });
    Effect::new(move || {
        if !store.signed_in.get() && v.room.get_untracked().is_some() {
            v.hang_up();
        }
    });
    // L'appli de bureau a retiré l'accord (menu « Arrêter le contrôle à distance »).
    let ended = Closure::<dyn FnMut()>::new(move || v.release_control(true));
    let _ = window().add_event_listener_with_callback("kraken-remote-ended", ended.as_ref().unchecked_ref());
    ended.forget();
}
