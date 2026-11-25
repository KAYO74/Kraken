//! Le son vu du moteur : des trames de 20 ms (48 kHz, mono, `f32` entre -1 et 1, soit 960
//! échantillons), données par une [`Capture`] et reçues par un [`Playback`].
//!
//! Ces deux traits cachent le matériel : [`crate::device`] les implémente avec cpal (vrais
//! micro et haut-parleurs) ; ici, des versions synthétiques sans carte son, pour les tests
//! et pour essayer le moteur : [`ToneCapture`] (une sinusoïde) et [`SinkPlayback`] (un puits qui
//! garde ce qu'il reçoit, lisible par [`Sink`]).
//!
//! Le moteur ne règle pas la cadence : c'est le périphérique qui la fixe. Il demande une trame
//! tant que [`Capture::read_frame`] en a une, et il en fournit tant que
//! [`Playback::needs_frame`] le demande. Aucune des deux méthodes ne doit bloquer.

use std::f64::consts::TAU;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Fréquence d'échantillonnage du moteur, en Hz (celle d'Opus pour la voix large bande).
pub const SAMPLE_RATE: u32 = 48_000;
/// Échantillons d'une trame : 20 ms.
pub const FRAME: usize = 960;
/// Durée d'une trame.
pub const FRAME_DURATION: Duration = Duration::from_millis(20);

/// Une trame de 20 ms.
pub type Frame = [f32; FRAME];

/// Un périphérique audio que l'on peut choisir (micro ou haut-parleurs), tel que la page l'affiche.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Son nom, tel qu'on l'affiche (à donner à `CpalCapture::open` et `CpalPlayback::open`).
    pub name: String,
    /// Son identifiant (cpal : `alsa:default` sous Linux, un identifiant CoreAudio ou WASAPI ailleurs), stable et sans ambiguïté : `open` le prend aussi.
    pub id: String,
    /// C'est le périphérique par défaut du système.
    pub is_default: bool,
}

/// Le micro : fournit des trames, dans l'ordre, au rythme du périphérique.
pub trait Capture: Send {
    /// Écrit les 20 ms suivantes dans `frame` et rend vrai ; faux (sans rien écrire) s'il n'y a
    /// pas encore une trame entière. Ne bloque pas.
    fn read_frame(&mut self, frame: &mut Frame) -> bool;

    /// Pourquoi le micro ne marche plus (périphérique débranché…), s'il ne marche plus.
    fn failure(&self) -> Option<String> {
        None
    }
}

/// Les haut-parleurs : reçoivent des trames, dans l'ordre.
pub trait Playback: Send {
    /// Vrai si le périphérique a besoin d'une trame de plus pour ne pas manquer de son. Le
    /// moteur en fournit alors une, puis redemande : c'est ainsi que l'horloge du périphérique,
    /// et non la nôtre, règle la lecture. Ne bloque pas.
    fn needs_frame(&mut self) -> bool;

    /// Donne la trame au périphérique. Ne bloque pas ; ce qui ne tient pas est perdu.
    fn write_frame(&mut self, frame: &Frame);

    /// Pourquoi la lecture ne marche plus (périphérique débranché…), si elle ne marche plus.
    fn failure(&self) -> Option<String> {
        None
    }
}

/// Une sinusoïde comme micro. Elle suit l'horloge murale, comme un vrai périphérique : une
/// trame de 20 ms est prête toutes les 20 ms (avec [`ToneCapture::unpaced`], toujours).
#[derive(Debug)]
pub struct ToneCapture {
    frequency: f64,
    amplitude: f32,
    /// Phase, en tours (entre 0 et 1) : continue d'une trame à l'autre.
    phase: f64,
    /// Heure de la prochaine trame ; `None` : pas de cadence.
    next: Option<Instant>,
    paced: bool,
}

impl ToneCapture {
    /// Une sinusoïde de `frequency` Hz, d'amplitude crête `amplitude` (0 à 1), à la cadence
    /// de l'horloge.
    pub fn new(frequency: f32, amplitude: f32) -> Self {
        Self { frequency: f64::from(frequency), amplitude, phase: 0.0, next: None, paced: true }
    }

    /// La même, mais une trame est toujours prête (tests unitaires).
    pub fn unpaced(frequency: f32, amplitude: f32) -> Self {
        Self { paced: false, ..Self::new(frequency, amplitude) }
    }

    /// Écrit la trame suivante, dans la continuité de la précédente.
    pub fn fill(&mut self, frame: &mut Frame) {
        let step = self.frequency / f64::from(SAMPLE_RATE);
        for s in frame.iter_mut() {
            *s = self.amplitude * (self.phase * TAU).sin() as f32;
            self.phase = (self.phase + step).fract();
        }
    }
}

impl ToneCapture {
    /// [`Capture::read_frame`] à l'instant `now` (les tests font avancer une heure fabriquée, sans dormir : la vraie
    /// horloge d'une machine chargée n'a pas la précision qu'ils demandent).
    fn read_frame_at(&mut self, frame: &mut Frame, now: Instant) -> bool {
        if self.paced {
            let due = *self.next.get_or_insert(now);
            if now < due {
                return false;
            }
            // Plus de 100 ms de retard (fil bloqué) : on repart d'ici, comme un périphérique qui
            // perd des trames, plutôt que de rattraper en rafale.
            let late = now.duration_since(due) > Duration::from_millis(100);
            self.next = Some(if late { now + FRAME_DURATION } else { due + FRAME_DURATION });
        }
        self.fill(frame);
        true
    }
}

impl Capture for ToneCapture {
    fn read_frame(&mut self, frame: &mut Frame) -> bool {
        self.read_frame_at(frame, Instant::now())
    }
}

/// Ce que le puits a reçu, partagé avec [`SinkPlayback`].
#[derive(Debug)]
struct SinkData {
    samples: Vec<f32>,
    /// Échantillons reçus depuis le début, ceux qu'on a oubliés compris.
    total: u64,
    /// Nombre d'échantillons à garder au moins ; au-delà du double, les plus anciens sont oubliés.
    keep: usize,
}

impl Default for SinkData {
    fn default() -> Self {
        Self { samples: Vec::new(), total: 0, keep: usize::MAX }
    }
}

/// Le côté « lecture des résultats » d'un [`SinkPlayback`] : on le garde dans le test pendant
/// que le puits est dans le moteur. Se clone ; toutes les copies voient les mêmes échantillons.
#[derive(Clone, Debug, Default)]
pub struct Sink(Arc<Mutex<SinkData>>);

impl Sink {
    fn lock(&self) -> MutexGuard<'_, SinkData> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Tous les échantillons reçus depuis le début (ou depuis [`Sink::clear`]).
    pub fn samples(&self) -> Vec<f32> {
        self.lock().samples.clone()
    }

    /// Nombre d'échantillons gardés (tous ceux reçus, sauf avec [`SinkPlayback::bounded`]).
    pub fn len(&self) -> usize {
        self.lock().samples.len()
    }

    /// Nombre d'échantillons reçus depuis le début, y compris ceux que [`SinkPlayback::bounded`] a
    /// oubliés ou que [`Sink::clear`] a effacés.
    pub fn total(&self) -> u64 {
        self.lock().total
    }

    /// Rien reçu pour l'instant.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Les `count` derniers échantillons reçus (moins s'il y en a moins).
    pub fn last(&self, count: usize) -> Vec<f32> {
        let d = self.lock();
        d.samples[d.samples.len().saturating_sub(count)..].to_vec()
    }

    /// Oublie ce qui a été reçu.
    pub fn clear(&self) {
        self.lock().samples.clear();
    }
}

/// Un haut-parleur qui n'existe pas : garde ce qu'il reçoit (voir [`Sink`]) et se comporte
/// comme un périphérique qui joue en temps réel, avec une file de deux trames.
#[derive(Debug)]
pub struct SinkPlayback {
    sink: Sink,
    /// Échantillons donnés et pas encore « joués », à l'instant `at`.
    queued: f64,
    at: Instant,
}

impl SinkPlayback {
    /// File visée : deux trames, soit 40 ms de retard.
    const TARGET: f64 = (2 * FRAME) as f64;

    /// Le puits, et son [`Sink`] pour lire ce qu'il reçoit.
    pub fn new() -> (Self, Sink) {
        let sink = Sink::default();
        (Self { sink: sink.clone(), queued: 0.0, at: Instant::now() }, sink)
    }

    /// Un puits qui n'engloutit pas la mémoire : il garde au moins les `keep` derniers échantillons
    /// (jusqu'au double), et [`Sink::total`] compte tout ce qu'il a reçu. Pour ce qui tourne longtemps
    /// (le mode synthétique de [`crate::control::Controller`]).
    pub fn bounded(keep: usize) -> (Self, Sink) {
        let (playback, sink) = Self::new();
        sink.lock().keep = keep.max(FRAME);
        (playback, sink)
    }

    /// Le périphérique a joué ce que le temps écoulé lui a laissé jouer, à l'instant `now`.
    fn advance(&mut self, now: Instant) {
        let played = now.saturating_duration_since(self.at).as_secs_f64() * f64::from(SAMPLE_RATE);
        self.queued = (self.queued - played).max(0.0);
        self.at = self.at.max(now);
    }

    /// [`Playback::needs_frame`] à l'instant `now` (les tests font avancer une heure fabriquée, sans dormir).
    fn needs_frame_at(&mut self, now: Instant) -> bool {
        self.advance(now);
        self.queued < Self::TARGET
    }

    /// [`Playback::write_frame`] à l'instant `now`.
    fn write_frame_at(&mut self, frame: &Frame, now: Instant) {
        self.advance(now);
        self.queued += FRAME as f64;
        let mut data = self.sink.lock();
        data.samples.extend_from_slice(frame);
        data.total += FRAME as u64;
        if data.samples.len() > data.keep.saturating_mul(2) {
            let forget = data.samples.len() - data.keep;
            data.samples.drain(..forget);
        }
    }
}

impl Playback for SinkPlayback {
    fn needs_frame(&mut self) -> bool {
        self.needs_frame_at(Instant::now())
    }

    fn write_frame(&mut self, frame: &Frame) {
        self.write_frame_at(frame, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tone_share;

    #[test]
    fn tone_has_the_right_frequency_and_amplitude() {
        let mut tone = ToneCapture::unpaced(440.0, 0.5);
        let mut samples = Vec::new();
        let mut frame = [0.0; FRAME];
        for _ in 0..25 {
            assert!(tone.read_frame(&mut frame));
            samples.extend_from_slice(&frame);
        }
        // 24 000 échantillons : 220 périodes entières, la fréquence tombe pile sur un point de la TFD.
        let window = &samples[..24_000];
        assert!(tone_share(window, 440.0) > 0.999, "{}", tone_share(window, 440.0));
        assert!(tone_share(window, 880.0) < 0.001);
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak - 0.5).abs() < 0.01, "crête {peak}");
        // Une seule fréquence : le carré moyen d'une sinusoïde d'amplitude a vaut a²/2.
        let mean_square = window.iter().map(|x| x * x).sum::<f32>() / window.len() as f32;
        assert!((mean_square - 0.125).abs() < 0.002, "{mean_square}");
    }

    #[test]
    fn tone_is_continuous_from_frame_to_frame() {
        // Sans saut de phase entre deux trames : le pas le plus grand reste celui de la sinusoïde.
        let mut tone = ToneCapture::unpaced(1000.0, 1.0);
        let mut frame = [0.0; FRAME];
        let mut last = 0.0f32;
        let mut biggest = 0.0f32;
        for i in 0..10 {
            tone.read_frame(&mut frame);
            if i > 0 {
                biggest = biggest.max((frame[0] - last).abs());
            }
            for pair in frame.windows(2) {
                biggest = biggest.max((pair[1] - pair[0]).abs());
            }
            last = frame[FRAME - 1];
        }
        let max_step = (TAU * 1000.0 / f64::from(SAMPLE_RATE)) as f32;
        assert!(biggest <= max_step * 1.01, "saut de {biggest} (maximum {max_step})");
    }

    #[test]
    fn paced_tone_follows_the_clock() {
        let mut tone = ToneCapture::new(440.0, 0.5);
        let mut frame = [0.0; FRAME];
        let t0 = Instant::now();
        assert!(tone.read_frame_at(&mut frame, t0), "la première trame est prête tout de suite");
        assert!(!tone.read_frame_at(&mut frame, t0), "la suivante attend 20 ms");
        assert!(!tone.read_frame_at(&mut frame, t0 + Duration::from_millis(19)));
        let later = t0 + Duration::from_millis(45);
        assert!(tone.read_frame_at(&mut frame, later));
        assert!(tone.read_frame_at(&mut frame, later), "le retard se rattrape trame par trame");
        assert!(!tone.read_frame_at(&mut frame, later));
    }

    #[test]
    fn paced_tone_restarts_after_a_long_stall() {
        let mut tone = ToneCapture::new(440.0, 0.5);
        let mut frame = [0.0; FRAME];
        let t0 = Instant::now();
        assert!(tone.read_frame_at(&mut frame, t0));
        // Plus de 100 ms de retard : le périphérique a perdu des trames, il repart d'ici, sans rafale.
        let stalled = t0 + Duration::from_millis(500);
        assert!(tone.read_frame_at(&mut frame, stalled));
        assert!(!tone.read_frame_at(&mut frame, stalled), "pas de rattrapage en rafale");
        assert!(tone.read_frame_at(&mut frame, stalled + FRAME_DURATION));
    }

    #[test]
    fn sink_keeps_samples_and_asks_for_two_frames_ahead() {
        let (mut playback, sink) = SinkPlayback::new();
        assert!(sink.is_empty());
        let frame = [0.25; FRAME];
        // Une heure fabriquée, figée : la file visée est de deux trames, exactement.
        let t0 = Instant::now();
        let mut written = 0;
        while playback.needs_frame_at(t0) {
            playback.write_frame_at(&frame, t0);
            written += 1;
            assert!(written <= 2, "la file visée est de deux trames");
        }
        assert_eq!(written, 2);
        assert_eq!(sink.len(), written * FRAME);
        // Le temps passe : le périphérique a joué une trame, il en demande une autre.
        assert!(playback.needs_frame_at(t0 + Duration::from_millis(25)));
        assert_eq!(sink.last(3), [0.25; 3]);
        sink.clear();
        assert!(sink.is_empty());
        assert_eq!(sink.total(), (written * FRAME) as u64, "le total survit à clear()");
    }

    #[test]
    fn bounded_sink_forgets_the_oldest_but_counts_everything() {
        let (mut playback, sink) = SinkPlayback::bounded(2 * FRAME);
        for n in 0..10 {
            playback.write_frame(&[n as f32; FRAME]);
        }
        assert_eq!(sink.total(), 10 * FRAME as u64);
        assert!(sink.len() >= 2 * FRAME && sink.len() <= 4 * FRAME + FRAME, "{} échantillons gardés", sink.len());
        assert_eq!(sink.last(1), [9.0], "le plus récent est toujours là");
        assert_eq!(sink.last(2 * FRAME)[0], 8.0, "au moins les deux dernières trames");
    }
}
