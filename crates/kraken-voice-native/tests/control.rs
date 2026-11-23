//! Le `Controller` (la surface de la page) contre le vrai SFU, en mémoire : deux contrôleurs
//! synthétiques (micros de 440 et 880 Hz), sur 127.0.0.1, sans carte son. Les attentes sont toutes
//! « jusqu'à ce que » avec de grands délais (15 s) : rien ne dépend d'une durée précise.

use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

use kraken_proto::{Id, VoiceJoin};
use kraken_voice::{Join, Settings, Sfu};
use kraken_voice_native::control::{Controller, STATS_WINDOW, StartOpts, StatePatch};
use kraken_voice_native::{ConnectionState, Snapshot};

fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}

/// Un contrôleur synthétique dont le micro joue `freq` Hz, sur la boucle locale.
fn controller(freq: f32) -> Controller {
    Controller::synthetic(freq).with_ips(vec![Ipv4Addr::LOCALHOST])
}

/// Un salon : le vrai SFU, sur 127.0.0.1, port libre au choix du système.
struct Room {
    sfu: Sfu,
    guild: Id,
    room: Id,
}

impl Room {
    fn new() -> Self {
        let settings = Settings { port: 0, ips: vec![Ipv4Addr::LOCALHOST.into()], ..Settings::default() };
        Self { sfu: Sfu::start(settings, |_| {}).unwrap(), guild: id(1), room: id(2) }
    }

    /// Ce que fait la page : poster l'offre du contrôleur au serveur, rendre la réponse.
    fn enter(&self, c: &Controller, user: Id, opts: StartOpts) -> VoiceJoin {
        let join = c.start(opts).unwrap();
        // Par le JSON, comme la page.
        let join: VoiceJoin = serde_json::from_value(serde_json::to_value(&join).unwrap()).unwrap();
        let answer = self
            .sfu
            .join(Join {
                guild_id: Some(self.guild),
                channel_id: self.room,
                user_id: user,
                offer: join.sdp.clone(),
                mids: join.mids.clone(),
                self_mute: join.self_mute,
                self_deaf: join.self_deaf,
                can_speak: true,
                can_stream: true,
                can_video: true,
            })
            .unwrap();
        c.answer(&answer).unwrap();
        join
    }
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "délai dépassé : {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn connected(c: &Controller) -> bool {
    c.poll().is_some_and(|s| s.state == ConnectionState::Connected)
}

/// Les ports UDP des candidats « host » d'une offre.
fn udp_ports(sdp: &str) -> Vec<u16> {
    sdp.lines()
        .filter(|l| l.starts_with("a=candidate:"))
        .filter_map(|l| l.split_whitespace().nth(5)?.parse().ok())
        .collect()
}

/// Ce que le puits de `c` entend, sur une fenêtre pleine : une sinusoïde à `freq` Hz.
fn hears(c: &Controller, freq: f32) -> bool {
    c.stats(&[freq]).is_some_and(|s| s.window == STATS_WINDOW && s.tones[0].share > 0.6)
}

/// Une fenêtre pleine de silence.
fn silent(c: &Controller) -> bool {
    c.stats(&[]).is_some_and(|s| s.window == STATS_WINDOW && s.rms < 1e-4)
}

/// Le cycle complet : démarrer, répondre, s'entendre, régler, quitter.
#[test]
fn a_page_can_run_a_whole_call_through_the_controller() {
    let room = Room::new();
    let (ua, ub) = (id(10), id(11));
    let (a, b) = (controller(440.0), controller(880.0));
    assert!(a.poll().is_none(), "pas d'appel : rien à relever");

    let join_a = room.enter(&a, ua, StartOpts::default());
    assert!(join_a.sdp.contains("m=application") && !join_a.mids.camera.is_empty());
    assert!(!join_a.self_mute && !join_a.self_deaf);
    // Juste après le démarrage : on se connecte, personne d'autre, aucun problème de son.
    let first = a.poll().unwrap();
    assert!(matches!(first.state, ConnectionState::Connecting | ConnectionState::Connected));
    assert_eq!((first.participants.len(), first.audio_error), (0, None));
    room.enter(&b, ub, StartOpts { mic_gain: 1.0, output_volume: 1.0, ..StartOpts::default() });
    until("connexions", || connected(&a) && connected(&b));

    // Chacun entend l'autre, à sa fréquence, et pas soi-même. La fenêtre peut contenir le silence du
    // début : on attend qu'elle soit pleine de son.
    until("B entend A", || hears(&b, 440.0));
    until("A entend B", || hears(&a, 880.0));
    // Une sinusoïde d'amplitude 0,3 : valeur efficace 0,3 / racine de 2.
    let loud = 0.3 / 2f32.sqrt();
    until("B entend A nettement", || {
        b.stats(&[440.0])
            .is_some_and(|s| s.window == STATS_WINDOW && s.tones[0].share > 0.97 && (s.rms - loud).abs() < loud * 0.1)
    });
    let sb = b.stats(&[440.0, 880.0, 1000.0]).unwrap();
    assert!(sb.tones[1].share < 0.05 && sb.tones[2].share < 0.05, "B ne s'entend pas lui-même : {sb:?}");
    assert!(sb.samples >= sb.window as u64);
    until("indicateurs", || {
        a.poll().is_some_and(|s| s.speaking == [ub]) && b.poll().is_some_and(|s| s.speaking == [ua])
    });
    let snap: Snapshot = a.poll().unwrap();
    assert_eq!(snap.participants, [ub]);
    assert!(snap.self_speaking && snap.mic_level > 0.5 && snap.output_level > 0.5);

    // Micro coupé de A : le serveur est prévenu, B n'entend plus rien, A le voit dans l'état.
    a.set(StatePatch { self_mute: Some(true), ..StatePatch::default() }).unwrap();
    let state = a.poll().unwrap();
    assert!(state.self_mute && !state.self_deaf, "effet immédiat, l'autre champ est resté tel quel");
    until("serveur prévenu", || room.sfu.state(ua).is_some_and(|s| s.self_mute));
    until("silence chez B", || silent(&b));
    until("B ne voit plus A parler", || b.poll().is_some_and(|s| s.speaking.is_empty()));
    // Son coupé : le micro de A l'est toujours, et le serveur sait les deux.
    a.set(StatePatch { self_deaf: Some(true), ..StatePatch::default() }).unwrap();
    until("serveur prévenu du son coupé", || room.sfu.state(ua).is_some_and(|s| s.self_deaf && s.self_mute));
    let state = a.poll().unwrap();
    assert!(state.self_mute && state.self_deaf);
    // On rouvre tout, d'un coup.
    a.set(StatePatch { self_mute: Some(false), self_deaf: Some(false), ..StatePatch::default() }).unwrap();
    until("B réentend A", || hears(&b, 440.0));

    // Volume de B divisé par quatre, gain hors limites sans dégâts, nombre invalide ignoré.
    b.set(StatePatch { output_volume: Some(0.25), ..StatePatch::default() }).unwrap();
    let quiet = loud * 0.25;
    until("volume baissé", || {
        b.stats(&[]).is_some_and(|s| s.window == STATS_WINDOW && (s.rms - quiet).abs() < quiet * 0.2)
    });
    b.set(StatePatch { output_volume: Some(f32::NAN), mic_gain: Some(100.0), ..StatePatch::default() }).unwrap();
    b.set(StatePatch::default()).unwrap();
    assert!(connected(&b));

    // Quitter : plus d'appel, le serveur voit le départ, les prises sont libres.
    let ports = udp_ports(&join_a.sdp);
    assert!(!ports.is_empty());
    for p in &ports {
        assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, *p)).is_err(), "la prise {p} est en service");
    }
    a.stop();
    assert!(a.poll().is_none() && a.stats(&[440.0]).is_none());
    until("le serveur voit A partir", || room.sfu.state(ua).is_none());
    for p in &ports {
        assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, *p)).is_ok(), "la prise {p} est libérée");
    }
    until("B ne voit plus A", || b.poll().is_some_and(|s| s.participants.is_empty()));
    assert_eq!(a.answer("v=0").unwrap_err(), "Aucun appel vocal en cours.");
    assert!(a.set(StatePatch::default()).is_err());
    a.stop(); // sans effet
    b.stop();
}

/// Un deuxième `start` arrête le premier appel (prises libérées, serveur prévenu) et en fait un neuf.
#[test]
fn a_second_start_replaces_the_first_call() {
    let room = Room::new();
    let (ua, ub) = (id(10), id(11));
    let (a, b) = (controller(440.0), controller(880.0));
    let first = room.enter(&a, ua, StartOpts::default());
    room.enter(&b, ub, StartOpts::default());
    until("connexions", || connected(&a) && connected(&b));
    until("B entend A", || hears(&b, 440.0));
    let first_ports = udp_ports(&first.sdp);

    // Sans avoir quitté : on relance, micro coupé cette fois.
    let second = room.enter(&a, ua, StartOpts { self_mute: true, ..StartOpts::default() });
    assert!(second.self_mute && !second.self_deaf, "le départ en micro coupé est dit au serveur");
    assert_ne!(second.sdp, first.sdp);
    for p in &first_ports {
        assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, *p)).is_ok(), "la prise {p} du premier appel est libérée");
    }
    until("connexion neuve", || connected(&a));
    assert!(room.sfu.state(ua).is_some_and(|s| s.self_mute), "le serveur voit le nouvel appel, micro coupé");
    assert!(a.poll().unwrap().self_mute);
    until("B ne voit plus parler A", || b.poll().is_some_and(|s| s.speaking.is_empty()));
    until("silence chez B", || silent(&b));
    // Le puits du nouvel appel repart de zéro.
    assert!(a.stats(&[]).unwrap().samples < 10 * 48_000);
    a.stop();
    b.stop();
    until("salon vide", || room.sfu.state(ua).is_none() && room.sfu.state(ub).is_none());
}

/// Une réponse illisible est refusée avec un message pour la page ; un appel perdu reste relevable.
#[test]
fn bad_answers_and_lost_calls_are_reported() {
    let room = Room::new();
    let (ua, ub) = (id(10), id(11));
    let a = controller(440.0);
    let join = a.start(StartOpts::default()).unwrap();
    let error = a.answer("ceci n'est pas du SDP").unwrap_err();
    assert_eq!(error, "La réponse du serveur est illisible.");
    let state = a.poll().unwrap();
    assert_eq!(state.state, ConnectionState::Connecting, "une réponse refusée tout de suite ne tue pas l'appel");

    // La bonne réponse, ensuite : ça marche.
    let answer = room
        .sfu
        .join(Join {
            guild_id: Some(room.guild),
            channel_id: room.room,
            user_id: ua,
            offer: join.sdp,
            mids: join.mids,
            self_mute: false,
            self_deaf: false,
            can_speak: true,
            can_stream: true,
            can_video: true,
        })
        .unwrap();
    a.answer(&answer).unwrap();
    until("connecté", || connected(&a));

    a.stop();

    // Une réponse que WebRTC refuse (sans l'empreinte DTLS) : l'appel est perdu, la page le voit avec
    // son motif, et il reste relevable jusqu'à `stop`.
    let b = controller(880.0);
    let join = b.start(StartOpts::default()).unwrap();
    let answer = room
        .sfu
        .join(Join {
            guild_id: Some(room.guild),
            channel_id: room.room,
            user_id: ub,
            offer: join.sdp,
            mids: join.mids,
            self_mute: false,
            self_deaf: false,
            can_speak: true,
            can_stream: true,
            can_video: true,
        })
        .unwrap();
    let broken: String =
        answer.lines().filter(|l| !l.starts_with("a=fingerprint")).collect::<Vec<_>>().join("\r\n") + "\r\n";
    b.answer(&broken).unwrap();
    until("perte vue", || b.poll().is_some_and(|s| matches!(s.state, ConnectionState::Lost(_))));
    let json = serde_json::to_value(b.poll().unwrap()).unwrap();
    assert_eq!(json["state"]["kind"], "lost");
    assert!(json["state"]["reason"].as_str().is_some_and(|r| r.contains("refusée")), "{json}");
    assert!(b.stats(&[440.0]).is_some(), "le puits reste lisible tant que l'appel n'est pas rangé");
    b.stop();
    assert!(b.poll().is_none());
    room.sfu.leave(ub);
}

/// Ce que la page reçoit et envoie, en JSON.
#[test]
fn the_json_the_page_sees() {
    let c = controller(440.0);
    let join = c.start(StartOpts { self_deaf: true, ..StartOpts::default() }).unwrap();
    let json = serde_json::to_value(&join).unwrap();
    let keys: Vec<&str> = {
        let mut k: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        k.sort_unstable();
        k
    };
    assert_eq!(keys, ["mids", "sdp", "self_deaf", "self_mute"]);
    assert_eq!(json["self_deaf"], true);
    let mids: Vec<&str> = {
        let mut k: Vec<&str> = json["mids"].as_object().unwrap().keys().map(String::as_str).collect();
        k.sort_unstable();
        k
    };
    assert_eq!(mids, ["camera", "mic", "screen", "screen_audio"]);

    let snap = serde_json::to_value(c.poll().unwrap()).unwrap();
    let mut keys: Vec<&str> = snap.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "audio_error",
            "mic_level",
            "output_level",
            "packets_received",
            "packets_sent",
            "participants",
            "self_deaf",
            "self_mute",
            "self_speaking",
            "speaking",
            "state"
        ]
    );
    assert_eq!(snap["self_deaf"], true);
    assert_eq!(snap["audio_error"], serde_json::Value::Null);

    let devices = serde_json::to_value(c.devices().unwrap()).unwrap();
    assert_eq!(devices["inputs"][0]["is_default"], true);
    assert!(devices["outputs"][0]["name"].is_string() && devices["outputs"][0]["id"].is_string());
    c.stop();
}

/// Plusieurs fils se servent du même contrôleur (comme les commandes Tauri) sans se gêner.
#[test]
fn threads_can_poll_while_another_starts_and_stops() {
    let c = std::sync::Arc::new(controller(440.0));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pollers: Vec<_> = (0..3)
        .map(|_| {
            let (c, stop) = (c.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = c.poll();
                    let _ = c.stats(&[440.0]);
                    let _ = c.set(StatePatch { mic_gain: Some(1.0), ..StatePatch::default() });
                }
            })
        })
        .collect();
    for _ in 0..4 {
        c.start(StartOpts::default()).unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    c.stop();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for p in pollers {
        p.join().unwrap();
    }
    assert!(c.poll().is_none());
}

/// Les vrais périphériques (cpal) : selon la machine il y a du son ou non, mais jamais de panique, et
/// ce qui échoue le dit en français.
#[cfg(feature = "device")]
#[test]
fn real_devices_never_panic_and_failures_are_readable() {
    let c = Controller::new().with_ips(vec![Ipv4Addr::LOCALHOST]);
    assert!(c.available());
    match c.devices() {
        Ok(d) => eprintln!("périphériques : {} micros, {} sorties", d.inputs.len(), d.outputs.len()),
        Err(e) => eprintln!("périphériques : {e}"),
    }
    match c.start(StartOpts { mic_device: Some("n'existe pas".into()), ..StartOpts::default() }) {
        Ok(join) => {
            // Au moins les haut-parleurs s'ouvrent : le micro introuvable donne un appel micro coupé, avec un motif.
            eprintln!("appel démarré, micro coupé : {}", join.self_mute);
            assert!(join.self_mute, "un micro introuvable : on entre micro coupé");
            assert!(c.poll().unwrap().audio_error.is_some(), "le motif est dit à la page");
            c.stop();
        }
        Err(e) => {
            eprintln!("démarrage refusé : {e}");
            assert!(e.ends_with('.') && e.len() < 100, "{e}");
        }
    }
    assert!(c.poll().is_none());
}
