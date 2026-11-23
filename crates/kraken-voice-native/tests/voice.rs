//! Le client vocal natif contre le vrai SFU, en mémoire : deux, puis trois `NativeVoice` (micro
//! synthétique, haut-parleurs qui gardent ce qu'ils reçoivent) sur 127.0.0.1, en UDP, sans carte
//! son. Les attentes sont toutes « jusqu'à ce que » avec de grands délais (15 s) : rien ne dépend
//! d'une durée précise.
//!
//! Le micro de l'un est une sinusoïde ; l'autre doit l'entendre, à cette fréquence (Goertzel).

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kraken_proto::{Id, VOICE_DATA_CHANNEL, VoiceClientMsg, VoiceMids, VoiceServerMsg, VoiceState};
use kraken_voice::{Join, Settings, Sfu};
use kraken_voice_native::audio::{Capture, Sink, SinkPlayback, ToneCapture};
use kraken_voice_native::{Config, ConnectionState, NativeVoice, Snapshot};
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Direction, Frequency, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};

// ───────────── analyse du son ─────────────

/// 0,5 s à 48 kHz : un nombre entier de périodes pour 440 Hz comme pour 880 Hz.
const WINDOW: usize = 24_000;

/// Part de l'énergie d'un signal à `freq` Hz (1 : sinusoïde pure), par Goertzel.
fn share(samples: &[f32], freq: f64) -> f64 {
    let total: f64 = samples.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    if total == 0.0 {
        return 0.0;
    }
    let w = std::f64::consts::TAU * freq / 48_000.0;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in samples {
        let s = f64::from(x) + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    2.0 * (s1 * s1 + s2 * s2 - coeff * s1 * s2) / samples.len() as f64 / total
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

/// La dernière demi-seconde de ce puits est (presque) une sinusoïde à `freq` Hz.
fn hears(sink: &Sink, freq: f64) -> bool {
    let w = sink.last(WINDOW);
    w.len() == WINDOW && share(&w, freq) > 0.6
}

/// La dernière fraction de seconde est muette.
fn silent(sink: &Sink, samples: usize) -> bool {
    let w = sink.last(samples);
    w.len() == samples && w.iter().all(|x| x.abs() < 1e-4)
}

/// Plus longue suite d'échantillons quasi nuls (un trou dans le son), en échantillons.
fn longest_gap(samples: &[f32]) -> usize {
    let (mut best, mut run) = (0, 0);
    for x in samples {
        if x.abs() < 1e-4 {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

// ───────────── le SFU et les clients ─────────────

fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}

/// Un salon : le vrai SFU, sur 127.0.0.1, port libre au choix du système.
struct Room {
    sfu: Sfu,
    guild: Id,
    room: Id,
    log: Arc<Mutex<Vec<VoiceState>>>,
}

impl Room {
    fn new() -> Self {
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        let settings = Settings { port: 0, ips: vec![Ipv4Addr::LOCALHOST.into()], ..Settings::default() };
        let sfu = Sfu::start(settings, move |s| seen.lock().unwrap().push(s)).unwrap();
        Self { sfu, guild: id(1), room: id(2), log }
    }

    fn join_request(&self, user: Id, sdp: String, mids: VoiceMids) -> Join {
        Join {
            guild_id: Some(self.guild),
            channel_id: self.room,
            user_id: user,
            offer: sdp,
            mids,
            self_mute: false,
            self_deaf: false,
            can_speak: true,
            can_stream: true,
            can_video: true,
        }
    }

    /// Un client natif entre dans le salon : micro = sinusoïde de `tone` Hz (ou aucun micro).
    fn join(&self, user: Id, tone: Option<f32>) -> Client {
        let (playback, sink) = SinkPlayback::new();
        let capture = tone.map(|f| Box::new(ToneCapture::new(f, 0.3)) as Box<dyn Capture>);
        let config =
            Config { capture, playback: Some(Box::new(playback)), ips: vec![Ipv4Addr::LOCALHOST], ..Config::default() };
        let (voice, offer) = NativeVoice::start(config).unwrap();
        let ports = udp_ports(&offer.sdp);
        let answer = self.sfu.join(self.join_request(user, offer.sdp, offer.mids)).unwrap();
        voice.accept_answer(&answer).unwrap();
        Client { voice, sink, user, ports }
    }

    fn state_of(&self, user: Id) -> Option<VoiceState> {
        self.sfu.state(user)
    }

    /// Le dernier état annoncé de `user` (même parti).
    fn last_state(&self, user: Id) -> Option<VoiceState> {
        self.log.lock().unwrap().iter().rev().find(|s| s.user_id == user).cloned()
    }
}

struct Client {
    voice: NativeVoice,
    sink: Sink,
    user: Id,
    /// Ports UDP locaux annoncés dans l'offre.
    ports: Vec<u16>,
}

impl Client {
    fn snap(&self) -> Snapshot {
        self.voice.snapshot()
    }

    fn connected(&self) -> bool {
        self.snap().state == ConnectionState::Connected
    }
}

/// Les ports UDP des candidats « host » d'une offre.
fn udp_ports(sdp: &str) -> Vec<u16> {
    sdp.lines()
        .filter(|l| l.starts_with("a=candidate:"))
        .filter_map(|l| l.split_whitespace().nth(5)?.parse().ok())
        .collect()
}

/// Attend que `done` soit vrai (15 s au plus), en regardant toutes les 10 ms.
fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "délai dépassé : {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn sorted(mut ids: Vec<Id>) -> Vec<Id> {
    ids.sort();
    ids
}

// ───────────── tests ─────────────

/// Le cas complet : deux personnes s'entendent, s'indiquent « parle », se coupent le micro et le son.
#[test]
fn two_people_hear_each_other_mute_and_deafen() {
    let room = Room::new();
    let (ua, ub) = (id(10), id(11));
    let a = room.join(ua, Some(440.0));
    let b = room.join(ub, Some(880.0));
    until("connexions", || a.connected() && b.connected());
    assert_eq!(room.sfu.states(room.guild).len(), 2);

    // B reçoit 440 Hz de A (et pas le sien) ; A reçoit 880 Hz de B (et pas le sien).
    until("B entend A", || hears(&b.sink, 440.0));
    until("A entend B", || hears(&a.sink, 880.0));
    assert!(share(&b.sink.last(WINDOW), 880.0) < 0.1, "B ne s'entend pas lui-même");
    assert!(share(&a.sink.last(WINDOW), 440.0) < 0.1, "A ne s'entend pas lui-même");
    until("indicateurs", || b.snap().speaking == [ua] && a.snap().speaking == [ub]);
    let (sa, sb) = (a.snap(), b.snap());
    assert_eq!(sa.participants, [ub]);
    assert_eq!(sb.participants, [ua]);
    assert!(sa.self_speaking && sb.self_speaking, "chacun se sait en train de parler");
    assert!(sa.mic_level > 0.5 && sb.mic_level > 0.5, "niveau du micro : {} et {}", sa.mic_level, sb.mic_level);
    assert!(sa.output_level > 0.5 && sb.output_level > 0.5, "niveau de sortie");
    assert!(sa.packets_sent > 10 && sa.packets_received > 10);

    // A coupe son micro : plus rien ne part, B n'entend plus rien et ne voit plus A « parler ».
    a.voice.set_state(true, false);
    until("serveur prévenu", || room.state_of(ua).is_some_and(|s| s.self_mute));
    pause(100);
    let sent = a.snap().packets_sent;
    until("silence chez B", || silent(&b.sink, 9_600));
    pause(500);
    assert_eq!(a.snap().packets_sent, sent, "micro coupé : aucun paquet envoyé");
    until("B ne voit plus A parler", || b.snap().speaking.is_empty());
    until("A ne se voit plus parler", || !a.snap().self_speaking);
    assert!(a.snap().self_mute && a.snap().mic_level > 0.5, "le niveau du micro reste visible micro coupé");
    // B, lui, continue d'être entendu par A (qui n'a coupé que son micro).
    assert!(hears(&a.sink, 880.0));
    assert_eq!(a.snap().speaking, [ub]);

    // A rouvre son micro : on le réentend.
    a.voice.set_state(false, false);
    until("serveur prévenu", || room.state_of(ua).is_some_and(|s| !s.self_mute));
    until("B réentend A", || hears(&b.sink, 440.0));
    until("B revoit A parler", || b.snap().speaking == [ua]);
    until("A envoie de nouveau", || a.snap().packets_sent > sent + 20);

    // B coupe le son : rien ne sort chez B, et son micro se coupe aussi.
    b.voice.set_state(false, true);
    until("serveur prévenu", || room.state_of(ub).is_some_and(|s| s.self_deaf && s.self_mute));
    pause(300);
    until("rien ne sort chez B", || b.sink.last(9_600).iter().all(|x| *x == 0.0) && b.sink.len() >= 9_600);
    let (received, sent) = (b.snap().packets_received, b.snap().packets_sent);
    pause(500);
    assert_eq!(b.snap().packets_received, received, "son coupé : le serveur n'envoie plus rien");
    assert_eq!(b.snap().packets_sent, sent, "son coupé : le micro est coupé aussi");
    assert!(b.snap().speaking.is_empty() && b.snap().self_deaf);
    assert!(b.sink.last(4_800).iter().all(|x| *x == 0.0), "toujours rien chez B");
    until("A n'entend plus B", || silent(&a.sink, 9_600));
    until("A ne voit plus B parler", || a.snap().speaking.is_empty());

    // B rétablit le son : il entend A de nouveau.
    b.voice.set_state(false, false);
    until("B réentend A", || hears(&b.sink, 440.0));
    until("A réentend B", || hears(&a.sink, 880.0));
    assert!(a.connected() && b.connected());
}

/// Gain du micro et volume de sortie.
#[test]
fn gain_and_volume_change_what_is_heard() {
    let room = Room::new();
    let (ua, ub) = (id(10), id(11));
    let a = room.join(ua, Some(440.0));
    let b = room.join(ub, None);
    until("B entend A", || hears(&b.sink, 440.0));
    // Une sinusoïde d'amplitude 0,3 a pour valeur efficace 0,3 / racine de 2.
    let loud = 0.3 / 2f32.sqrt();
    until("plein volume", || (rms(&b.sink.last(WINDOW)) - loud).abs() < loud * 0.15);

    // Volume de sortie de B divisé par quatre.
    b.voice.set_output_volume(0.25);
    until("volume baissé", || (rms(&b.sink.last(WINDOW)) - loud * 0.25).abs() < loud * 0.25 * 0.15);
    b.voice.set_output_volume(1.0);
    until("volume rétabli", || (rms(&b.sink.last(WINDOW)) - loud).abs() < loud * 0.15);

    // Gain du micro de A à zéro : B n'entend plus rien, et A ne se croit plus en train de parler.
    a.voice.set_mic_gain(0.0);
    until("silence", || silent(&b.sink, 9_600));
    until("plus de parole", || b.snap().speaking.is_empty() && !a.snap().self_speaking);
    a.voice.set_mic_gain(1.0);
    until("B réentend A", || hears(&b.sink, 440.0));

    // Valeurs absurdes : ignorées ou ramenées dans les limites, sans panique.
    a.voice.set_mic_gain(f32::NAN);
    a.voice.set_mic_gain(f32::INFINITY);
    b.voice.set_output_volume(-3.0);
    b.voice.set_output_volume(1.0);
    until("le son revient", || hears(&b.sink, 440.0));
}

/// Un troisième arrive (le serveur renégocie avec les deux premiers), puis repart : le son ne casse pas.
#[test]
fn a_third_person_arrives_and_leaves_without_breaking_the_sound() {
    let room = Room::new();
    let (ua, ub, uc) = (id(10), id(11), id(12));
    let a = room.join(ua, Some(440.0));
    let b = room.join(ub, None);
    until("B entend A", || hears(&b.sink, 440.0));
    assert_eq!(b.snap().participants, [ua]);

    // C arrive, avec sa propre voix. On suit le son de B pendant toute la renégociation.
    let from = b.sink.len();
    let c = room.join(uc, Some(880.0));
    until("C connecté", || c.connected());
    until("B voit C", || b.snap().participants == sorted(vec![ua, uc]));
    until("A voit C", || a.snap().participants == sorted(vec![ub, uc]));
    until("C voit A et B", || c.snap().participants == sorted(vec![ua, ub]));
    until("C entend A", || hears(&c.sink, 440.0));
    until("A entend C", || hears(&a.sink, 880.0));
    until("B entend A et C", || {
        let w = b.sink.last(WINDOW);
        w.len() == WINDOW && share(&w, 440.0) > 0.3 && share(&w, 880.0) > 0.3
    });
    until("B voit les deux parler", || b.snap().speaking == sorted(vec![ua, uc]));
    // Aucun trou dans ce que B a entendu pendant que C arrivait (hors le tout début, avant que A ne soit établi).
    let heard = b.sink.samples();
    let during = &heard[from.max(WINDOW)..];
    assert!(longest_gap(during) < 19_200, "trou de {} échantillons dans le son de B", longest_gap(during));
    assert!(a.connected() && b.connected() && c.connected());

    // C part : B n'entend plus que A, et l'état du serveur le dit.
    c.voice.stop();
    until("le serveur voit C partir", || room.state_of(uc).is_none());
    assert_eq!(room.last_state(uc).unwrap().channel_id, None);
    until("B ne voit plus C", || b.snap().participants == [ua]);
    until("A ne voit plus C", || a.snap().participants == [ub]);
    until("B n'entend plus que A", || hears(&b.sink, 440.0) && share(&b.sink.last(WINDOW), 880.0) < 0.05);
    until("B ne voit plus que A parler", || b.snap().speaking == [ua]);
    assert!(matches!(c.snap().state, ConnectionState::Lost(_)));
    assert!(a.connected() && b.connected());
}

/// `stop()`, et la destruction, libèrent le fil et les prises ; le serveur voit le départ.
#[test]
fn stop_frees_the_threads_and_the_sfu_sees_the_departure() {
    let room = Room::new();
    let (ua, ub, uc) = (id(10), id(11), id(12));

    // stop() après la connexion.
    let a = room.join(ua, Some(440.0));
    until("A connecté", || a.connected());
    assert!(!a.ports.is_empty());
    for p in &a.ports {
        assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, *p)).is_err(), "la prise {p} est en service");
    }
    a.voice.stop();
    assert_eq!(a.snap().state, ConnectionState::Lost("arrêté".into()));
    until("le serveur voit A partir", || room.state_of(ua).is_none());
    let last = room.last_state(ua).unwrap();
    assert_eq!((last.channel_id, last.self_mute, last.streaming), (None, false, false));
    for p in &a.ports {
        assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, *p)).is_ok(), "la prise {p} est libérée");
    }
    a.voice.stop();
    assert!(a.voice.accept_answer("v=0\r\n").is_err(), "plus rien à faire après stop()");
    a.voice.set_state(true, true);
    a.voice.set_mic_gain(2.0);
    let _ = a.snap();

    // La destruction suffit aussi.
    let b = room.join(ub, Some(440.0));
    until("B connecté", || b.connected());
    let (ports, user) = (b.ports.clone(), b.user);
    drop(b);
    until("le serveur voit B partir", || room.state_of(user).is_none());
    for p in &ports {
        assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, *p)).is_ok());
    }

    // Arrêté avant d'avoir reçu la réponse : rien ne reste.
    let (voice, offer) = NativeVoice::start(Config { ips: vec![Ipv4Addr::LOCALHOST], ..Config::default() }).unwrap();
    let ports = udp_ports(&offer.sdp);
    assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, ports[0])).is_err());
    voice.stop();
    assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, ports[0])).is_ok());
    drop(voice);

    // Une connexion qui se perd : l'état dit pourquoi, et le reste du salon continue.
    let c = room.join(uc, None);
    until("C connecté", || c.connected());
    assert!(room.sfu.leave(uc));
    c.voice.stop();
}

/// Une réponse inutilisable met fin à la connexion, avec un motif.
#[test]
fn a_refused_answer_is_reported_as_lost() {
    let room = Room::new();
    let (playback, _sink) = SinkPlayback::new();
    let config = Config { playback: Some(Box::new(playback)), ips: vec![Ipv4Addr::LOCALHOST], ..Config::default() };
    let (voice, offer) = NativeVoice::start(config).unwrap();
    let answer = room.sfu.join(room.join_request(id(10), offer.sdp, offer.mids)).unwrap();
    // La réponse sans l'empreinte DTLS : illisible pour WebRTC.
    let broken: String =
        answer.lines().filter(|l| !l.starts_with("a=fingerprint")).collect::<Vec<_>>().join("\r\n") + "\r\n";
    voice.accept_answer(&broken).unwrap();
    until("motif de la perte", || matches!(voice.snapshot().state, ConnectionState::Lost(_)));
    let ConnectionState::Lost(why) = voice.snapshot().state else { unreachable!() };
    assert!(why.contains("réponse du serveur refusée"), "{why}");
    assert!(voice.snapshot().participants.is_empty());
    voice.stop();
    room.sfu.leave(id(10));
}

// ───────────── un vrai « navigateur » : image reçue ignorée, Opus lisible ─────────────

/// Un pair str0m comme le navigateur de `kraken-voice/tests/roundtrip.rs` : mêmes quatre
/// transceivers, même canal, il répond aux offres du serveur ; il peut envoyer une caméra
/// (image VP8 factice) et garde ce qu'il reçoit.
struct Browser {
    rtc: Rtc,
    socket: UdpSocket,
    local: SocketAddr,
    mids: [Mid; 4],
    channel: Option<ChannelId>,
    connected: bool,
    send_camera: bool,
    last_frame: Instant,
    frames: u64,
    received: Vec<(String, MediaKind, Vec<u8>)>,
    offers: usize,
    next: Instant,
}

const KEYFRAME: [u8; 12] = [0x10, 0x02, 0x00, 0x9d, 0x01, 0x2a, 0x80, 0x02, 0xe0, 0x01, 7, 7];

impl Browser {
    fn join(room: &Room, user: Id) -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket.set_nonblocking(true).unwrap();
        let local = socket.local_addr().unwrap();
        let now = Instant::now();
        let mut rtc = Rtc::builder().set_crypto_provider(Arc::new(str0m::crypto::from_feature_flags())).build(now);
        rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
        let mut api = rtc.sdp_api();
        let mic = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let screen = api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
        let screen_audio = api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
        let camera = api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
        api.add_channel(VOICE_DATA_CHANNEL.into());
        let (offer, pending): (SdpOffer, SdpPendingOffer) = api.apply().unwrap();
        let mids = VoiceMids {
            mic: mic.to_string(),
            screen: screen.to_string(),
            screen_audio: screen_audio.to_string(),
            camera: camera.to_string(),
        };
        let answer = room.sfu.join(room.join_request(user, offer.to_sdp_string(), mids)).unwrap();
        rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap()).unwrap();
        Self {
            rtc,
            socket,
            local,
            mids: [mic, screen, screen_audio, camera],
            channel: None,
            connected: false,
            send_camera: false,
            last_frame: now,
            frames: 0,
            received: Vec::new(),
            offers: 0,
            next: now,
        }
    }

    fn ready(&self) -> bool {
        self.connected && self.channel.is_some()
    }

    fn send(&mut self, msg: &VoiceClientMsg) {
        let json = serde_json::to_string(msg).unwrap();
        let mut ch = self.rtc.channel(self.channel.expect("canal ouvert")).unwrap();
        ch.write(false, json.as_bytes()).unwrap();
    }

    fn poll(&mut self) {
        loop {
            match self.rtc.poll_output().unwrap() {
                Output::Timeout(t) => {
                    self.next = t;
                    return;
                }
                Output::Transmit(t) => {
                    let _ = self.socket.send_to(&t.contents, t.destination);
                }
                Output::Event(e) => self.event(e),
            }
        }
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Connected => self.connected = true,
            Event::ChannelOpen(id, label) if label == VOICE_DATA_CHANNEL => self.channel = Some(id),
            Event::ChannelData(d) => {
                if let Ok(VoiceServerMsg::Offer { sdp }) = serde_json::from_slice(&d.data) {
                    let answer = self.rtc.sdp_api().accept_offer(SdpOffer::from_sdp_string(&sdp).unwrap()).unwrap();
                    self.offers += 1;
                    self.send(&VoiceClientMsg::Answer { sdp: answer.to_sdp_string() });
                }
            }
            Event::MediaData(d) => {
                let m = self.rtc.media(d.mid).unwrap();
                self.received.push((m.stream_id().to_string(), m.kind(), d.data.to_vec()));
            }
            _ => {}
        }
    }

    /// Une image de caméra toutes les 20 ms, quand elle est allumée.
    fn send_media(&mut self) {
        if !self.connected || !self.send_camera || self.last_frame.elapsed() < Duration::from_millis(20) {
            return;
        }
        self.last_frame = Instant::now();
        self.frames += 1;
        let Some(w) = self.rtc.writer(self.mids[3]) else { return };
        let Some(pt) = w.payload_params().find(|p| p.spec().codec == Codec::Vp8).map(|p| p.pt()) else { return };
        let time = MediaTime::new(self.frames * 3000, Frequency::NINETY_KHZ);
        let _ = w.write(pt, Instant::now(), time, KEYFRAME.to_vec());
    }

    fn pump(&mut self) {
        self.poll();
        let mut buf = [0u8; 2000];
        while let Ok((n, from)) = self.socket.recv_from(&mut buf) {
            let rx = Receive::new(Protocol::Udp, from, self.local, &buf[..n]).unwrap();
            self.rtc.handle_input(Input::Receive(Instant::now(), rx)).unwrap();
            self.poll();
        }
        let now = Instant::now();
        if self.next <= now {
            self.rtc.handle_input(Input::Timeout(now)).unwrap();
        }
        self.send_media();
        self.poll();
    }

    fn got(&self, stream: &str, kind: MediaKind) -> usize {
        self.received.iter().filter(|(s, k, _)| s == stream && *k == kind).count()
    }
}

/// Comme [`until`], en faisant tourner le navigateur entre deux vérifications.
fn until_pumping(w: &mut Browser, what: &str, mut done: impl FnMut(&Browser) -> bool) {
    until(what, || {
        w.pump();
        done(w)
    });
}

/// Un navigateur allume sa caméra : le serveur ajoute une ligne vidéo chez le client natif, qui y
/// répond sans la toucher ; l'audio continue, et un vrai navigateur lit l'Opus du client natif.
#[test]
fn video_from_a_browser_is_ignored_and_a_browser_can_read_our_opus() {
    let room = Room::new();
    let (un, uw, um) = (id(10), id(11), id(12));
    let n = room.join(un, Some(440.0));
    let mut w = Browser::join(&room, uw);
    until_pumping(&mut w, "connexions", |w| w.ready() && n.connected());

    // Le navigateur reçoit l'Opus du client natif, tel qu'un décodeur Opus le lit : 440 Hz.
    let n_mic = format!("{un}.mic");
    until_pumping(&mut w, "audio relayé au navigateur", |w| w.got(&n_mic, MediaKind::Audio) >= 60);
    let mut decoder = opus::Decoder::new(48_000, opus::Channels::Mono).unwrap();
    let mut pcm = Vec::new();
    let mut out = [0f32; 5760];
    for (_, _, data) in w.received.iter().filter(|(s, k, _)| *s == n_mic && *k == MediaKind::Audio) {
        let got = decoder.decode_float(data, &mut out, false).unwrap();
        assert_eq!(got, 960, "des paquets de 20 ms");
        pcm.extend_from_slice(&out[..got]);
    }
    let tail = &pcm[pcm.len() - WINDOW.min(pcm.len())..];
    assert!(share(tail, 440.0) > 0.6, "le navigateur doit lire 440 Hz : {}", share(tail, 440.0));

    // La caméra du navigateur s'allume : une ligne vidéo apparaît chez le client natif.
    let offers = w.offers;
    w.send_camera = true;
    w.send(&VoiceClientMsg::State { self_mute: false, self_deaf: false, streaming: false, camera: true });
    until_pumping(&mut w, "caméra annoncée", |_| room.state_of(uw).is_some_and(|s| s.camera));
    pause(300);
    assert_eq!(n.snap().state, ConnectionState::Connected);

    // Un troisième arrive ensuite : le client natif ajoute encore des pistes audio, malgré la vidéo.
    let m = room.join(um, Some(880.0));
    until_pumping(&mut w, "M entendu de N", |_| hears(&n.sink, 880.0));
    assert!(w.offers > offers, "le navigateur a répondu à des offres");

    w.send_camera = false;
    w.send(&VoiceClientMsg::State { self_mute: false, self_deaf: false, streaming: false, camera: false });
    until_pumping(&mut w, "caméra éteinte", |_| room.state_of(uw).is_some_and(|s| !s.camera));
    let snap = n.snap();
    assert_eq!(snap.state, ConnectionState::Connected, "la vidéo n'a rien cassé");
    assert_eq!(snap.participants, sorted(vec![uw, um]), "seuls les micros comptent");
    assert!(snap.audio_error.is_none());
    assert!(hears(&n.sink, 880.0) || share(&n.sink.last(WINDOW), 880.0) > 0.3);
    drop(m);
}
