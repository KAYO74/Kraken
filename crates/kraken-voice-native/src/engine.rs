//! Le moteur : un fil qui fait tourner le `Rtc` de str0m (prises UDP, ICE, DTLS, SRTP, canal de
//! données), lit le micro, encode et envoie, reçoit, décode, mélange et joue.
//!
//! Tout y est sur un seul fil, sans async : à chaque tour (toutes les 2 ms) il lit les prises,
//! donne au `Rtc` ses échéances, reprend les trames du micro, fournit celles que les haut-parleurs
//! demandent, envoie les messages de contrôle et publie l'état lu par [`crate::NativeVoice::snapshot`].
//! Les ordres de l'API lui arrivent par un canal ; les réglages simples (gain, volume, micro et
//! son coupés) par des atomiques, qu'il relit à chaque trame.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kraken_proto::{Id, VOICE_DATA_CHANNEL, VoiceClientMsg, VoiceServerMsg};
use opus::{Channels, Decoder};
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Frequency, MediaAdded, MediaChanged, MediaData, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive, Transmit};
use str0m::{Event, IceConnectionState, Input, Output, Rtc};

use crate::audio::{Capture, FRAME, FRAME_DURATION, Frame, Playback};
use crate::jitter::{JitterBuffer, Pull};
use crate::mix::{self, SpeechDetector};
use crate::{ConnectionState, Snapshot};

/// Pause entre deux tours : la prise est lue et les trames du micro reprises au moins à ce rythme.
const TICK: Duration = Duration::from_millis(2);
/// La connexion (ICE, DTLS, canal de données) doit être établie dans ce délai après le départ.
const SETUP: Duration = Duration::from_secs(20);
/// ICE coupé plus longtemps : la connexion est perdue.
const LOST: Duration = Duration::from_secs(10);
/// Deux messages `state` au moins séparés de ce délai (le serveur en limite le débit) ; un
/// changement plus rapide est regroupé : seul le dernier état part.
const STATE_GAP: Duration = Duration::from_millis(300);
/// Au plus ce temps pour que `close()` ait fini d'émettre ses paquets d'adieu.
const CLOSE_GRACE: Duration = Duration::from_millis(400);
/// Message du canal de données plus long : ignoré.
const MAX_MESSAGE: usize = 256 * 1024;
/// Participants dont on décode le micro, au plus (le serveur n'en met pas plus de 25).
const MAX_PEERS: usize = 64;
/// Trames reprises ou fournies au plus par tour : un fil bloqué un moment ne rattrape pas d'un bloc.
const MAX_FRAMES_PER_STEP: usize = 8;
/// Fréquence RTP du son Opus.
const OPUS_RATE: u32 = 48_000;
/// Plus grande trame Opus décodable : 120 ms.
const MAX_DECODED: usize = 5760;
/// Premier horodatage RTP envoyé.
const FIRST_RTP_TIME: u64 = FRAME as u64;

/// Ordres de l'API au moteur.
pub(crate) enum Cmd {
    /// La réponse du serveur à l'offre.
    Answer(SdpAnswer),
    /// Micro et son coupés ou non : à annoncer au serveur.
    State {
        self_mute: bool,
        self_deaf: bool,
    },
    Stop,
}

/// Partagé entre l'API et le moteur.
pub(crate) struct Shared {
    snapshot: Mutex<Snapshot>,
    mic_gain: AtomicU32,
    output_volume: AtomicU32,
    self_mute: AtomicBool,
    self_deaf: AtomicBool,
}

impl Shared {
    pub(crate) fn new(mic_gain: f32, output_volume: f32, self_mute: bool, self_deaf: bool) -> Self {
        Self {
            snapshot: Mutex::new(Snapshot { self_mute, self_deaf, ..Snapshot::default() }),
            mic_gain: AtomicU32::new(mic_gain.to_bits()),
            output_volume: AtomicU32::new(output_volume.to_bits()),
            self_mute: AtomicBool::new(self_mute),
            self_deaf: AtomicBool::new(self_deaf),
        }
    }

    fn snapshot_lock(&self) -> MutexGuard<'_, Snapshot> {
        self.snapshot.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.snapshot_lock().clone()
    }

    pub(crate) fn set_mic_gain(&self, gain: f32) {
        self.mic_gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn set_output_volume(&self, volume: f32) {
        self.output_volume.store(volume.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn set_state(&self, self_mute: bool, self_deaf: bool) {
        self.self_mute.store(self_mute, Ordering::Relaxed);
        self.self_deaf.store(self_deaf, Ordering::Relaxed);
        let mut s = self.snapshot_lock();
        s.self_mute = self_mute;
        s.self_deaf = self_deaf;
    }

    fn mic_gain(&self) -> f32 {
        f32::from_bits(self.mic_gain.load(Ordering::Relaxed))
    }

    fn output_volume(&self) -> f32 {
        f32::from_bits(self.output_volume.load(Ordering::Relaxed))
    }

    fn muted(&self) -> bool {
        self.self_mute.load(Ordering::Relaxed) || self.self_deaf.load(Ordering::Relaxed)
    }

    fn deaf(&self) -> bool {
        self.self_deaf.load(Ordering::Relaxed)
    }
}

/// Si le fil s'arrête (même par une panique), l'état le dit.
struct Guard(Arc<Shared>);

impl Drop for Guard {
    fn drop(&mut self) {
        let mut s = self.0.snapshot_lock();
        if !matches!(s.state, ConnectionState::Lost(_)) {
            s.lose("moteur arrêté");
        }
    }
}

/// Une prise UDP et l'adresse annoncée pour elle.
pub(crate) struct Sock {
    pub(crate) socket: UdpSocket,
    pub(crate) addr: SocketAddr,
}

/// Tout ce qu'il faut au moteur pour partir.
pub(crate) struct Init {
    pub(crate) rtc: Rtc,
    pub(crate) pending: SdpPendingOffer,
    pub(crate) sockets: Vec<Sock>,
    pub(crate) mic: Mid,
    pub(crate) encoder: opus::Encoder,
    pub(crate) capture: Option<Box<dyn Capture>>,
    pub(crate) playback: Option<Box<dyn Playback>>,
    pub(crate) commands: Receiver<Cmd>,
    pub(crate) shared: Arc<Shared>,
    pub(crate) jitter_frames: usize,
    pub(crate) self_mute: bool,
    pub(crate) self_deaf: bool,
}

pub(crate) fn spawn(init: Init) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new().name("kraken-vocal-natif".into()).spawn(move || {
        let shared = init.shared.clone();
        let _guard = Guard(shared);
        Engine::new(init).run();
    })
}

/// Un participant dont on reçoit le micro.
struct Peer {
    decoder: Decoder,
    jitter: JitterBuffer,
    speech: SpeechDetector,
    /// Sortie du décodeur : assez grande pour la plus longue trame Opus.
    scratch: Vec<f32>,
}

impl Peer {
    fn new(decoder: Decoder, jitter_frames: usize) -> Self {
        Self {
            decoder,
            jitter: JitterBuffer::new(jitter_frames),
            speech: SpeechDetector::default(),
            scratch: vec![0.0; MAX_DECODED],
        }
    }

    /// Les 20 ms suivantes de ce participant dans `out` ; faux s'il n'y a rien à jouer.
    fn pull(&mut self, out: &mut Frame) -> bool {
        match self.jitter.pull() {
            Pull::Silence => false,
            Pull::Packet(data) => self.decode(&data, false, out),
            // Un paquet manque : le suivant (s'il suit directement) peut en contenir une version réduite.
            Pull::Lost { fec: Some(next) } => self.decode(&next, true, out),
            Pull::Lost { fec: None } => self.decode(&[], false, out),
        }
    }

    /// Décode `data` (vide : masque une perte) ; `fec` : ne récupère que le paquet d'avant.
    fn decode(&mut self, data: &[u8], fec: bool, out: &mut Frame) -> bool {
        let room = if data.is_empty() || fec { &mut self.scratch[..FRAME] } else { &mut self.scratch[..] };
        match self.decoder.decode_float(data, room, fec) {
            Ok(n) => {
                let n = n.min(FRAME);
                out[..n].copy_from_slice(&self.scratch[..n]);
                out[n..].fill(0.0);
                true
            }
            Err(e) => {
                tracing::debug!("vocal natif : paquet Opus illisible ({e})");
                false
            }
        }
    }
}

struct Engine {
    rtc: Rtc,
    pending: Option<SdpPendingOffer>,
    sockets: Vec<Sock>,
    /// La ligne qui envoie le micro.
    mic: Mid,
    channel: Option<ChannelId>,
    connected: bool,
    ice_down_since: Option<Instant>,
    started: Instant,
    /// Prochaine échéance du `Rtc`.
    next_timeout: Instant,
    commands: Receiver<Cmd>,
    shared: Arc<Shared>,
    capture: Option<Box<dyn Capture>>,
    playback: Option<Box<dyn Playback>>,
    /// Sans haut-parleurs : l'horloge qui cadence quand même la lecture (pour les indicateurs).
    next_play: Instant,
    encoder: opus::Encoder,
    /// Horodatage RTP du dernier paquet envoyé ou sauté (micro coupé).
    rtp_time: u64,
    peers: HashMap<Id, Peer>,
    /// Quelle ligne apporte le micro de qui.
    owners: HashMap<Mid, Id>,
    jitter_frames: usize,
    self_speech: SpeechDetector,
    self_speaking: bool,
    mic_level: f32,
    output_level: f32,
    packets_sent: u64,
    packets_received: u64,
    /// État annoncé au serveur, et celui qu'on voudrait lui annoncer.
    sent_state: (bool, bool),
    wanted_state: (bool, bool),
    state_sent_at: Instant,
    last_publish: Instant,
    /// Fin demandée par l'API.
    stopping: bool,
    /// Pourquoi la connexion est finie, si elle l'est.
    lost: Option<String>,
    recv_buf: Vec<u8>,
}

impl Engine {
    fn new(init: Init) -> Self {
        let now = Instant::now();
        Self {
            rtc: init.rtc,
            pending: Some(init.pending),
            sockets: init.sockets,
            mic: init.mic,
            channel: None,
            connected: false,
            ice_down_since: None,
            started: now,
            next_timeout: now,
            commands: init.commands,
            shared: init.shared,
            capture: init.capture,
            playback: init.playback,
            next_play: now,
            encoder: init.encoder,
            rtp_time: FIRST_RTP_TIME,
            peers: HashMap::new(),
            owners: HashMap::new(),
            jitter_frames: init.jitter_frames,
            self_speech: SpeechDetector::default(),
            self_speaking: false,
            mic_level: 0.0,
            output_level: 0.0,
            packets_sent: 0,
            packets_received: 0,
            sent_state: (init.self_mute, init.self_deaf),
            wanted_state: (init.self_mute, init.self_deaf),
            state_sent_at: now.checked_sub(STATE_GAP).unwrap_or(now),
            last_publish: now.checked_sub(FRAME_DURATION).unwrap_or(now),
            stopping: false,
            lost: None,
            recv_buf: vec![0; 2048],
        }
    }

    fn run(mut self) {
        while self.lost.is_none() && !self.stopping {
            self.step();
            std::thread::sleep(TICK);
        }
        self.finish();
    }

    /// Un tour : ordres, réseau, échéances, audio, contrôle, état publié.
    fn step(&mut self) {
        self.commands();
        if self.stopping || self.lost.is_some() {
            return;
        }
        self.receive();
        let now = Instant::now();
        if self.next_timeout <= now {
            self.input_timeout(now);
        }
        self.poll();
        self.capture_audio();
        self.playout(now);
        self.control(now);
        self.poll();
        self.publish(now);
    }

    fn lose(&mut self, why: impl Into<String>) {
        if self.lost.is_none() {
            let why = why.into();
            tracing::debug!("vocal natif : fin ({why})");
            self.lost = Some(why);
        }
    }

    // ───────────── ordres de l'API ─────────────

    fn commands(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(Cmd::Answer(answer)) => self.accept_answer(answer),
                Ok(Cmd::State { self_mute, self_deaf }) => self.wanted_state = (self_mute, self_deaf),
                Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => {
                    self.stopping = true;
                    return;
                }
                Err(TryRecvError::Empty) => return,
            }
        }
    }

    fn accept_answer(&mut self, answer: SdpAnswer) {
        let Some(pending) = self.pending.take() else {
            tracing::debug!("vocal natif : réponse déjà reçue, celle-ci est ignorée");
            return;
        };
        if let Err(e) = self.rtc.sdp_api().accept_answer(pending, answer) {
            self.lose(format!("réponse du serveur refusée : {e}"));
        }
    }

    // ───────────── réseau ─────────────

    /// Tout ce qui est arrivé sur les prises.
    fn receive(&mut self) {
        for i in 0..self.sockets.len() {
            // Borné : une prise qui déborde ne monopolise pas le tour.
            for _ in 0..64 {
                if self.lost.is_some() {
                    return;
                }
                let (n, from) = match self.sockets[i].socket.recv_from(&mut self.recv_buf) {
                    Ok(r) => r,
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    // Windows signale ainsi un ancien envoi refusé (ICMP) ; ailleurs, rien de durable.
                    Err(e) => {
                        tracing::trace!("vocal natif : lecture UDP ({e})");
                        break;
                    }
                };
                let destination = self.sockets[i].addr;
                let Ok(rx) = Receive::new(Protocol::Udp, from, destination, &self.recv_buf[..n]) else { continue };
                if !self.rtc.is_alive() {
                    return;
                }
                if let Err(e) = self.rtc.handle_input(Input::Receive(Instant::now(), rx)) {
                    self.rtc.disconnect();
                    self.lose(format!("paquet refusé : {e}"));
                    return;
                }
                self.poll();
            }
        }
    }

    fn input_timeout(&mut self, now: Instant) {
        if !self.rtc.is_alive() {
            return;
        }
        if let Err(e) = self.rtc.handle_input(Input::Timeout(now)) {
            self.rtc.disconnect();
            self.lose(format!("échéance refusée : {e}"));
        }
    }

    /// Vide la sortie du `Rtc` : paquets envoyés, événements traités, prochaine échéance notée.
    fn poll(&mut self) {
        while self.lost.is_none() {
            if !self.rtc.is_alive() {
                self.lose("connexion fermée");
                return;
            }
            match self.rtc.poll_output() {
                Ok(Output::Timeout(t)) => {
                    self.next_timeout = t;
                    return;
                }
                Ok(Output::Transmit(t)) => self.transmit(&t),
                Ok(Output::Event(e)) => self.event(e),
                Err(e) => {
                    self.rtc.disconnect();
                    self.lose(format!("erreur WebRTC : {e}"));
                }
            }
        }
    }

    fn transmit(&self, t: &Transmit) {
        // La prise dont l'adresse est la source demandée par ICE ; à défaut, la première.
        let Some(s) = self.sockets.iter().find(|s| s.addr == t.source).or(self.sockets.first()) else { return };
        // Une erreur d'envoi (réseau coupé…) se voit ensuite par ICE.
        let _ = s.socket.send_to(&t.contents, t.destination);
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Connected => self.connected = true,
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                self.ice_down_since.get_or_insert_with(Instant::now);
            }
            Event::IceConnectionStateChange(IceConnectionState::Connected | IceConnectionState::Completed) => {
                self.ice_down_since = None;
            }
            Event::ChannelOpen(id, label) if label == VOICE_DATA_CHANNEL && self.channel.is_none() => {
                self.channel = Some(id);
            }
            Event::ChannelClose(id) if Some(id) == self.channel => self.lose("canal de données fermé"),
            Event::ChannelData(d) if Some(d.id) == self.channel => self.message(&d.data),
            Event::MediaAdded(m) => self.media_added(&m),
            Event::MediaChanged(m) => self.media_changed(&m),
            Event::MediaData(d) => self.media_data(&d),
            Event::Closed => self.lose("fermée par le serveur"),
            _ => {}
        }
    }

    /// Un message du serveur par le canal de données.
    fn message(&mut self, data: &[u8]) {
        if data.len() > MAX_MESSAGE {
            return;
        }
        let Ok(msg) = serde_json::from_slice::<VoiceServerMsg>(data) else { return };
        // Le contrôle à distance ne concerne que celui qui partage son écran : ce client n'a rien à en faire.
        if let VoiceServerMsg::Offer { sdp } = msg {
            self.answer_offer(&sdp);
        }
    }

    /// Renégociation : des pistes de micro arrivent ou partent. On répond toujours, sans rien
    /// demander au serveur : une offre sans réponse bloque toutes les suivantes.
    fn answer_offer(&mut self, sdp: &str) {
        let offer = match SdpOffer::from_sdp_string(sdp) {
            Ok(o) => o,
            Err(e) => return self.lose(format!("offre du serveur illisible : {e}")),
        };
        match self.rtc.sdp_api().accept_offer(offer) {
            Ok(answer) => {
                self.send(&VoiceClientMsg::Answer { sdp: answer.to_sdp_string() });
            }
            Err(e) => self.lose(format!("offre du serveur refusée : {e}")),
        }
    }

    /// Un message au serveur par le canal de données ; faux (et connexion finie) en cas d'échec.
    fn send(&mut self, msg: &VoiceClientMsg) -> bool {
        let Some(channel) = self.channel else { return false };
        let Ok(json) = serde_json::to_string(msg) else { return false };
        let sent = matches!(self.rtc.channel(channel).map(|mut c| c.write(false, json.as_bytes())), Some(Ok(_)));
        if !sent {
            self.lose("message non envoyé");
        }
        sent
    }

    // ───────────── pistes reçues ─────────────

    /// Une piste apparaît. Seul le micro d'un participant (flux `<id>.mic`) est gardé ; les images
    /// (écran, caméra) et le son d'un écran partagé ne sont jamais décodés : on ne les touche pas.
    fn media_added(&mut self, m: &MediaAdded) {
        if m.kind != MediaKind::Audio || !m.direction.is_receiving() {
            return;
        }
        let Some(id) = self.rtc.media(m.mid).and_then(|media| mic_owner(media.stream_id())) else { return };
        if !self.peers.contains_key(&id) && self.peers.len() >= MAX_PEERS {
            return;
        }
        let decoder = match Decoder::new(OPUS_RATE, Channels::Mono) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("vocal natif : décodeur Opus impossible ({e})");
                return;
            }
        };
        // Un participant n'a qu'une ligne de micro : l'ancienne, s'il en reste une, n'est plus à lui.
        self.owners.retain(|_, owner| *owner != id);
        self.owners.insert(m.mid, id);
        let jitter_frames = self.jitter_frames;
        self.peers.entry(id).or_insert_with(|| Peer::new(decoder, jitter_frames));
    }

    /// Une piste s'arrête (ligne à `m=audio 0`) : son participant n'est plus entendu.
    fn media_changed(&mut self, m: &MediaChanged) {
        if m.direction.is_receiving() {
            return;
        }
        if let Some(id) = self.owners.remove(&m.mid) {
            self.peers.remove(&id);
        }
    }

    fn media_data(&mut self, d: &MediaData) {
        let Some(peer) = self.owners.get(&d.mid).and_then(|id| self.peers.get_mut(id)) else { return };
        if d.time.denom() != OPUS_RATE {
            return;
        }
        peer.jitter.push(d.time.numer(), &d.data);
        self.packets_received += 1;
    }

    // ───────────── audio ─────────────

    /// Reprend les trames du micro : gain, niveau, parole, puis envoi.
    fn capture_audio(&mut self) {
        let mut frame = mix::silence();
        for _ in 0..MAX_FRAMES_PER_STEP {
            if !self.capture.as_mut().is_some_and(|c| c.read_frame(&mut frame)) {
                return;
            }
            mix::apply_gain(&mut frame, self.shared.mic_gain());
            let rms = mix::rms(&frame);
            self.mic_level = mix::smooth(self.mic_level, mix::level(rms));
            let muted = self.shared.muted();
            self.self_speaking = if muted {
                self.self_speech.reset();
                false
            } else {
                self.self_speech.update(rms)
            };
            // Le temps RTP avance même micro coupé : à la reprise, le destinataire voit un silence.
            self.rtp_time += FRAME as u64;
            if !muted && self.ready() {
                self.send_frame(&frame);
            }
        }
    }

    /// Connexion établie et canal de données ouvert.
    fn ready(&self) -> bool {
        self.connected && self.channel.is_some() && self.pending.is_none()
    }

    fn send_frame(&mut self, frame: &Frame) {
        let mut packet = [0u8; 1500];
        let n = match self.encoder.encode_float(&frame[..], &mut packet) {
            Ok(n) => n,
            Err(e) => {
                tracing::debug!("vocal natif : encodage impossible ({e})");
                return;
            }
        };
        let Some(w) = self.rtc.writer(self.mic) else { return };
        let Some(pt) = w.payload_params().find(|p| p.spec().codec == Codec::Opus).map(|p| p.pt()) else { return };
        let time = MediaTime::new(self.rtp_time, Frequency::FORTY_EIGHT_KHZ);
        match w.write(pt, Instant::now(), time, packet[..n].to_vec()) {
            Ok(()) => self.packets_sent += 1,
            Err(e) => tracing::debug!("vocal natif : trame non envoyée ({e})"),
        }
    }

    /// Fournit aux haut-parleurs les trames qu'ils demandent (ou, sans haut-parleurs, une toutes
    /// les 20 ms : les indicateurs « parle » restent à jour).
    fn playout(&mut self, now: Instant) {
        let deaf = self.shared.deaf();
        let volume = if deaf { 0.0 } else { self.shared.output_volume() };
        for _ in 0..MAX_FRAMES_PER_STEP {
            let wanted = match self.playback.as_mut() {
                Some(p) => p.needs_frame(),
                None => now >= self.next_play,
            };
            if !wanted {
                return;
            }
            if self.playback.is_none() {
                let late = now.duration_since(self.next_play) > Duration::from_millis(100);
                self.next_play = if late { now + FRAME_DURATION } else { self.next_play + FRAME_DURATION };
            }
            let frame = self.mix_frame(deaf, volume);
            if let Some(p) = self.playback.as_mut() {
                p.write_frame(&frame);
            }
        }
    }

    /// Les 20 ms suivantes de tout le monde, mélangées.
    fn mix_frame(&mut self, deaf: bool, volume: f32) -> Frame {
        let mut out = mix::silence();
        if deaf {
            // Son coupé : rien ne sort, rien ne s'accumule, personne n'est « en train de parler ».
            for p in self.peers.values_mut() {
                p.jitter.clear();
                p.speech.reset();
            }
        } else {
            let mut decoded = mix::silence();
            for p in self.peers.values_mut() {
                if p.pull(&mut decoded) {
                    p.speech.update(mix::rms(&decoded));
                    mix::add(&mut out, &decoded);
                } else {
                    p.speech.update(0.0);
                }
            }
        }
        mix::finish(&mut out, volume);
        self.output_level = mix::smooth(self.output_level, mix::level(mix::rms(&out)));
        out
    }

    // ───────────── contrôle et état ─────────────

    fn control(&mut self, now: Instant) {
        if !(self.connected && self.channel.is_some()) && now.duration_since(self.started) > SETUP {
            return self.lose("connexion jamais établie");
        }
        if self.ice_down_since.is_some_and(|t| now.duration_since(t) > LOST) {
            return self.lose("connexion perdue");
        }
        if self.channel.is_some()
            && self.wanted_state != self.sent_state
            && now.duration_since(self.state_sent_at) >= STATE_GAP
        {
            let (self_mute, self_deaf) = self.wanted_state;
            if self.send(&VoiceClientMsg::State { self_mute, self_deaf, streaming: false, camera: false }) {
                self.sent_state = (self_mute, self_deaf);
                self.state_sent_at = now;
            }
        }
    }

    /// Met à jour l'état que lit l'API, au plus toutes les 20 ms.
    fn publish(&mut self, now: Instant) {
        if now.duration_since(self.last_publish) < FRAME_DURATION {
            return;
        }
        self.last_publish = now;
        let mut participants: Vec<Id> = self.peers.keys().copied().collect();
        participants.sort();
        let mut speaking: Vec<Id> = self.peers.iter().filter(|(_, p)| p.speech.speaking()).map(|(id, _)| *id).collect();
        speaking.sort();
        let audio_error = self
            .capture
            .as_ref()
            .and_then(|c| c.failure())
            .or_else(|| self.playback.as_ref().and_then(|p| p.failure()));
        let state = if self.ready() { ConnectionState::Connected } else { ConnectionState::Connecting };
        let mut s = self.shared.snapshot_lock();
        // `lose` l'a peut-être déjà dit entre-temps : jamais de retour en arrière.
        if matches!(s.state, ConnectionState::Lost(_)) {
            return;
        }
        s.state = state;
        s.participants = participants;
        s.speaking = speaking;
        s.self_speaking = self.self_speaking;
        s.mic_level = self.mic_level;
        s.output_level = self.output_level;
        s.audio_error = audio_error;
        s.packets_sent = self.packets_sent;
        s.packets_received = self.packets_received;
    }

    /// Fin du fil : adieux propres si c'est nous qui partons, puis l'état dit pourquoi c'est fini.
    fn finish(&mut self) {
        let reason = match self.lost.take() {
            Some(why) => why,
            None => {
                self.say_goodbye();
                "arrêté".to_string()
            }
        };
        self.shared.snapshot_lock().lose(reason);
    }

    /// Ferme proprement (RTCP BYE, fermeture SCTP, `close_notify` DTLS) pour que le serveur voie le
    /// départ tout de suite plutôt qu'au bout de la coupure de la liaison.
    fn say_goodbye(&mut self) {
        if !self.connected || !self.rtc.is_alive() || self.rtc.close().is_err() {
            return;
        }
        let end = Instant::now() + CLOSE_GRACE;
        while self.rtc.is_alive() && Instant::now() < end {
            for _ in 0..100 {
                match self.rtc.poll_output() {
                    Ok(Output::Transmit(t)) => self.transmit(&t),
                    Ok(Output::Event(_)) => {}
                    Ok(Output::Timeout(t)) => {
                        let now = Instant::now();
                        if t > now || self.rtc.handle_input(Input::Timeout(now)).is_err() {
                            break;
                        }
                    }
                    Err(_) => return,
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// À qui est ce flux, si c'est un micro : `<id>.mic`.
fn mic_owner(stream_id: &str) -> Option<Id> {
    Id::parse_str(stream_id.strip_suffix(".mic")?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_microphone_streams_have_an_owner() {
        let id = Id::from_bytes([7; 16]);
        assert_eq!(mic_owner(&format!("{id}.mic")), Some(id));
        assert_eq!(mic_owner(&format!("{id}.screen")), None, "le son de l'écran n'est pas un micro");
        assert_eq!(mic_owner(&format!("{id}.camera")), None);
        assert_eq!(mic_owner("pas-un-id.mic"), None);
        assert_eq!(mic_owner(".mic"), None);
        assert_eq!(mic_owner(""), None);
    }

    #[test]
    fn a_peer_decodes_what_the_encoder_makes_and_conceals_what_is_lost() {
        use crate::audio::ToneCapture;
        let mut enc = opus::Encoder::new(OPUS_RATE, Channels::Mono, opus::Application::Voip).unwrap();
        enc.set_inband_fec(true).unwrap();
        enc.set_packet_loss_perc(10).unwrap();
        let mut tone = ToneCapture::unpaced(500.0, 0.5);
        let mut frame = mix::silence();
        let mut packets = Vec::new();
        for _ in 0..30 {
            tone.fill(&mut frame);
            let mut out = [0u8; 1500];
            let n = enc.encode_float(&frame[..], &mut out).unwrap();
            packets.push(out[..n].to_vec());
        }
        let mut peer = Peer::new(Decoder::new(OPUS_RATE, Channels::Mono).unwrap(), 3);
        let mut played = Vec::new();
        let mut decoded = mix::silence();
        for (i, p) in packets.iter().enumerate() {
            // Le paquet 12 est perdu.
            if i != 12 {
                peer.jitter.push(1000 + i as u64 * 960, p);
            }
            if peer.pull(&mut decoded) {
                played.extend_from_slice(&decoded);
            } else {
                played.extend_from_slice(&mix::silence());
            }
        }
        // Une fois le tampon rempli, tout est joué, y compris au rang de la perte.
        assert!(played.len() >= 28 * FRAME);
        let body = &played[10 * FRAME..26 * FRAME];
        let share = crate::testutil::tone_share(body, 500.0);
        assert!(share > 0.9, "le son décodé doit rester une sinusoïde à 500 Hz malgré la perte : {share}");
        assert!(mix::rms(body) > 0.25, "et rester audible : {}", mix::rms(body));
    }

    #[test]
    fn a_garbage_packet_is_silence_not_a_panic() {
        let mut peer = Peer::new(Decoder::new(OPUS_RATE, Channels::Mono).unwrap(), 1);
        let mut out = mix::silence();
        for i in 0..5u64 {
            peer.jitter.push(1000 + i * 960, &[0xff; 7]);
        }
        for _ in 0..10 {
            let _ = peer.pull(&mut out);
        }
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn sample_rate_constant_matches_the_codec() {
        assert_eq!(crate::audio::SAMPLE_RATE, OPUS_RATE);
    }
}
