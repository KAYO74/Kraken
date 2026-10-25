//! Le SFU pour de vrai : des « navigateurs » str0m sur 127.0.0.1, en UDP, rejoignent un salon.
//! Le micro de l'un arrive chez l'autre ; l'écran partagé n'arrive qu'à qui le regarde (avec une
//! image clé demandée à celui qui partage) ; chaque piste reçue porte le flux `<id>.mic` ou
//! `<id>.screen` ; regarder, arrêter de regarder et partir renégocient par le canal de données.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kraken_proto::{Id, VOICE_DATA_CHANNEL, VoiceClientMsg, VoiceMids, VoiceServerMsg, VoiceState};
use kraken_voice::{Join, JoinError, Settings, Sfu};
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Direction, Frequency, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};

/// Trame Opus factice (le SFU ne décode rien) et image clé VP8 factice (bit 0 à zéro).
const MIC: [u8; 4] = [0xa0, 1, 2, 3];
const KEYFRAME: [u8; 12] = [0x10, 0x02, 0x00, 0x9d, 0x01, 0x2a, 0x80, 0x02, 0xe0, 0x01, 7, 7];

/// Un navigateur réduit à l'essentiel : trois pistes envoyées, le canal `kraken`, et il répond
/// à chaque offre du serveur comme le client web.
struct Browser {
    rtc: Rtc,
    socket: UdpSocket,
    local: SocketAddr,
    /// Micro, image et son de l'écran.
    mids: [Mid; 3],
    channel: Option<ChannelId>,
    connected: bool,
    send_mic: bool,
    send_screen: bool,
    last_frame: Instant,
    frames: u64,
    /// Offres reçues du serveur.
    offers: Vec<String>,
    /// Trames reçues : flux (msid) de la piste, nature, contenu.
    received: Vec<(String, MediaKind, Vec<u8>)>,
    keyframe_requests: usize,
    next: Instant,
}

impl Browser {
    fn new() -> (Self, String, VoiceMids, SdpPendingOffer) {
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
        api.add_channel(VOICE_DATA_CHANNEL.into());
        let (offer, pending) = api.apply().unwrap();
        let mids =
            VoiceMids { mic: mic.to_string(), screen: screen.to_string(), screen_audio: screen_audio.to_string() };
        let b = Self {
            rtc,
            socket,
            local,
            mids: [mic, screen, screen_audio],
            channel: None,
            connected: false,
            send_mic: false,
            send_screen: false,
            last_frame: now,
            frames: 0,
            offers: Vec::new(),
            received: Vec::new(),
            keyframe_requests: 0,
            next: now,
        };
        (b, offer.to_sdp_string(), mids, pending)
    }

    /// Rejoint `room` (serveur `guild`) avec tous les droits.
    fn join(sfu: &Sfu, guild: Id, room: Id, user: Id) -> Self {
        let (mut b, offer, mids, pending) = Self::new();
        let answer = sfu.join(join(guild, room, user, offer, mids)).unwrap();
        assert!(answer.contains("a=ice-lite"), "le serveur est ICE-lite");
        b.rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap()).unwrap();
        b
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
                let VoiceServerMsg::Offer { sdp } = serde_json::from_slice(&d.data).unwrap();
                let answer = self.rtc.sdp_api().accept_offer(SdpOffer::from_sdp_string(&sdp).unwrap()).unwrap();
                self.offers.push(sdp);
                self.send(&VoiceClientMsg::Answer { sdp: answer.to_sdp_string() });
            }
            Event::MediaData(d) => {
                let m = self.rtc.media(d.mid).unwrap();
                self.received.push((m.stream_id().to_string(), m.kind(), d.data.to_vec()));
            }
            Event::KeyframeRequest(r) if r.mid == self.mids[1] => self.keyframe_requests += 1,
            _ => {}
        }
    }

    /// Une trame toutes les 20 ms sur chaque piste active.
    fn send_media(&mut self) {
        if !self.connected || self.last_frame.elapsed() < Duration::from_millis(20) {
            return;
        }
        self.last_frame = Instant::now();
        self.frames += 1;
        let audio = MediaTime::new(self.frames * 960, Frequency::FORTY_EIGHT_KHZ);
        if self.send_mic {
            self.write(0, Codec::Opus, audio, &MIC);
        }
        if self.send_screen {
            self.write(1, Codec::Vp8, MediaTime::new(self.frames * 3000, Frequency::NINETY_KHZ), &KEYFRAME);
            self.write(2, Codec::Opus, audio, &MIC);
        }
    }

    fn write(&mut self, i: usize, codec: Codec, time: MediaTime, data: &[u8]) {
        let Some(w) = self.rtc.writer(self.mids[i]) else { return };
        let Some(pt) = w.payload_params().find(|p| p.spec().codec == codec).map(|p| p.pt()) else { return };
        let _ = w.write(pt, Instant::now(), time, data.to_vec());
    }

    /// Tout ce qui est arrivé sur la prise, les échéances, les envois.
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

fn join(guild: Id, room: Id, user: Id, offer: String, mids: VoiceMids) -> Join {
    Join {
        guild_id: guild,
        channel_id: room,
        user_id: user,
        offer,
        mids,
        self_mute: false,
        self_deaf: false,
        can_speak: true,
        can_stream: true,
    }
}

fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}

fn sfu(max_members: usize) -> (Sfu, Arc<Mutex<Vec<VoiceState>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = log.clone();
    let settings = Settings { port: 0, ips: vec![Ipv4Addr::LOCALHOST.into()], max_members, ..Settings::default() };
    let sfu = Sfu::start(settings, move |s| seen.lock().unwrap().push(s)).unwrap();
    (sfu, log)
}

/// Fait tourner les navigateurs jusqu'à ce que `done` soit vrai (10 s au plus).
fn until(bs: &mut [&mut Browser], what: &str, done: impl Fn(&[&mut Browser]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(bs) {
        assert!(Instant::now() < deadline, "délai dépassé : {what}");
        for b in bs.iter_mut() {
            b.pump();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn voice_and_screen_round_trip() {
    let (sfu, log) = sfu(25);
    assert_ne!(sfu.port(), 0);
    let (guild, room) = (id(1), id(2));
    let (ua, ub) = (id(10), id(11));
    let mut a = Browser::join(&sfu, guild, room, ua);
    let mut b = Browser::join(&sfu, guild, room, ub);
    let states = sfu.states(guild);
    assert_eq!(
        states.iter().map(|s| (s.user_id, s.channel_id)).collect::<Vec<_>>(),
        [(ua, Some(room)), (ub, Some(room))]
    );
    assert_eq!(log.lock().unwrap().len(), 2, "une arrivée annoncée par participant");

    // Chacun reçoit le micro de l'autre.
    a.send_mic = true;
    b.send_mic = true;
    let (a_mic, b_mic) = (format!("{ua}.mic"), format!("{ub}.mic"));
    until(&mut [&mut a, &mut b], "micros relayés", |bs| {
        bs[0].got(&b_mic, MediaKind::Audio) > 0 && bs[1].got(&a_mic, MediaKind::Audio) > 0
    });
    assert!(b.received.iter().any(|(s, _, d)| *s == a_mic && d[..] == MIC), "trame relayée telle quelle");
    assert!(a.ready() && b.ready());

    // A envoie son écran, mais ne l'a pas annoncé : rien ne part.
    a.send_screen = true;
    let a_screen = format!("{ua}.screen");
    b.send(&VoiceClientMsg::Watch { user_id: ua, on: true });
    let t = Instant::now();
    until(&mut [&mut a, &mut b], "attente", |_| t.elapsed() > Duration::from_millis(300));
    assert_eq!(b.got(&a_screen, MediaKind::Video), 0, "pas de partage annoncé : rien à regarder");

    // A annonce le partage ; B regarde : image (après une image clé demandée à A) et son.
    a.send(&VoiceClientMsg::State { self_mute: false, self_deaf: false, streaming: true });
    until(&mut [&mut a, &mut b], "partage annoncé", |_| sfu.state(ua).is_some_and(|s| s.streaming));
    assert_eq!(b.got(&a_screen, MediaKind::Video), 0, "personne ne regarde sans le demander");
    b.send(&VoiceClientMsg::Watch { user_id: ua, on: true });
    until(&mut [&mut a, &mut b], "écran relayé", |bs| {
        bs[1].got(&a_screen, MediaKind::Video) > 0 && bs[1].got(&a_screen, MediaKind::Audio) > 0
    });
    assert!(a.keyframe_requests > 0, "image clé demandée à celui qui partage");
    assert_eq!(a.got(&format!("{ub}.screen"), MediaKind::Video), 0);

    // B arrête de regarder : les deux lignes sont arrêtées, plus rien n'arrive.
    let offers = b.offers.len();
    b.send(&VoiceClientMsg::Watch { user_id: ua, on: false });
    until(&mut [&mut a, &mut b], "arrêt de l'écran", |bs| bs[1].offers.len() > offers);
    let stop = b.offers.last().unwrap();
    assert!(stop.contains("m=video 0 "), "ligne vidéo arrêtée (port 0) :\n{stop}");
    let t = Instant::now();
    until(&mut [&mut a, &mut b], "fin du flux", |_| t.elapsed() > Duration::from_millis(200));
    let before = b.got(&a_screen, MediaKind::Video);
    let t = Instant::now();
    until(&mut [&mut a, &mut b], "attente", |_| t.elapsed() > Duration::from_millis(300));
    assert_eq!(b.got(&a_screen, MediaKind::Video), before, "écran plus relayé");

    // B coupe son micro mais l'envoie quand même : plus rien ne part.
    b.send(&VoiceClientMsg::State { self_mute: true, self_deaf: false, streaming: false });
    until(&mut [&mut a, &mut b], "micro coupé", |_| sfu.state(ub).is_some_and(|s| s.self_mute));
    let t = Instant::now();
    until(&mut [&mut a, &mut b], "fin du flux", |_| t.elapsed() > Duration::from_millis(200));
    let before = a.got(&b_mic, MediaKind::Audio);
    let t = Instant::now();
    until(&mut [&mut a, &mut b], "attente", |_| t.elapsed() > Duration::from_millis(300));
    assert_eq!(a.got(&b_mic, MediaKind::Audio), before, "micro coupé : plus relayé");

    // Messages invalides : ignorés, la connexion reste.
    let ch = b.channel.unwrap();
    b.rtc.channel(ch).unwrap().write(false, b"{\"t\":\"nimporte\"}").unwrap();
    b.rtc.channel(ch).unwrap().write(true, &[0xff; 64]).unwrap();

    // A part : B le voit partir et sa piste est arrêtée.
    let offers = b.offers.len();
    assert!(sfu.leave(ua));
    assert!(!sfu.leave(ua), "déjà parti");
    until(&mut [&mut a, &mut b], "départ renégocié", |bs| bs[1].offers.len() > offers);
    assert!(b.offers.last().unwrap().contains("m=audio 0 "));
    assert_eq!(sfu.states(guild).iter().map(|s| s.user_id).collect::<Vec<_>>(), [ub]);
    let last = log.lock().unwrap().iter().rev().find(|s| s.user_id == ua).cloned().unwrap();
    assert_eq!(last.channel_id, None);
}

#[test]
fn full_room_moves_and_bad_offers() {
    let (sfu, log) = sfu(2);
    let (g1, g2, room, other) = (id(1), id(2), id(3), id(4));
    let offer = |user| {
        let (_, sdp, mids, _) = Browser::new();
        join(g1, room, user, sdp, mids)
    };
    sfu.join(offer(id(10))).unwrap();
    sfu.join(offer(id(11))).unwrap();
    assert!(matches!(sfu.join(offer(id(12))), Err(JoinError::Full)));
    // Une nouvelle connexion du même utilisateur remplace l'ancienne : pas comptée deux fois.
    sfu.join(offer(id(11))).unwrap();
    assert_eq!(sfu.states(g1).len(), 2);

    // Changer de serveur : l'ancien apprend le départ, le nouveau l'arrivée.
    sfu.join(Join { guild_id: g2, channel_id: other, ..offer(id(11)) }).unwrap();
    let tail: Vec<(Id, Option<Id>)> =
        log.lock().unwrap().iter().rev().take(2).map(|s| (s.guild_id, s.channel_id)).collect();
    assert_eq!(tail, [(g2, Some(other)), (g1, None)]);
    assert_eq!(sfu.states(g1).len(), 1);
    sfu.join(offer(id(12))).unwrap();

    // Offres inutilisables (dans un salon vide : le salon plein se voit avant l'offre).
    let bad = |f: &dyn Fn(&mut Join)| {
        let mut j = Join { channel_id: id(5), ..offer(id(20)) };
        f(&mut j);
        matches!(sfu.join(j), Err(JoinError::BadOffer(_)))
    };
    assert!(bad(&|j| j.offer = "pas du SDP".into()));
    assert!(bad(&|j| j.mids.mic = "99".into()), "mid absent");
    assert!(bad(&|j| j.mids.mic = j.mids.screen.clone()), "vidéo à la place du micro");
    assert!(bad(&|j| j.mids.screen_audio = j.mids.mic.clone()), "mid en double");
    assert!(bad(&|j| j.mids.screen = "a b".into()), "mid invalide");
    assert!(sfu.state(id(20)).is_none());

    // Sans connexion établie en 15 s, la place se libère (ici : départ demandé).
    assert!(sfu.leave(id(10)));
    assert!(sfu.state(id(10)).is_none());
}

#[test]
fn port_taken() {
    let busy = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    let port = busy.local_addr().unwrap().port();
    let settings = |fallback| Settings { port, fallback, ips: vec![Ipv4Addr::LOCALHOST.into()], ..Settings::default() };
    assert!(Sfu::start(settings(false), |_| {}).is_err(), "port pris : pas de vocal");
    let sfu = Sfu::start(settings(true), |_| {}).unwrap();
    assert_ne!(sfu.port(), port, "repli sur un port libre");
    assert_eq!(sfu.candidates(), [SocketAddr::from((Ipv4Addr::LOCALHOST, sfu.port()))]);
    let public = Settings { port: 0, public_port: Some(40000), ..settings(false) };
    assert_eq!(Sfu::start(public, |_| {}).unwrap().candidates()[0].port(), 40000);
    let none = Settings { port: 0, ips: vec![], ..Settings::default() };
    assert!(Sfu::start(none, |_| {}).is_err(), "rien à annoncer");
}
