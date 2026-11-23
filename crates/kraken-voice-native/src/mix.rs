//! Le travail sur les trames : niveaux, mélange des participants, écrêtage doux, détection de parole.

use crate::audio::{FRAME, Frame};

/// Valeur efficace d'un signal (0 : silence ; 1 : un carré plein de -1 à 1).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Valeur efficace en niveau d'affichage, de 0 à 1 : -60 dBFS (ou moins) donne 0, 0 dBFS donne 1.
pub fn level(rms: f32) -> f32 {
    if rms <= 1e-6 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Un niveau qui monte tout de suite et redescend doucement (environ 150 ms), pour un indicateur
/// lisible à raison d'une valeur par trame.
pub fn smooth(previous: f32, current: f32) -> f32 {
    current.max(previous * 0.85)
}

/// Multiplie la trame par `gain` ; le résultat reste entre -1 et 1 (micro).
pub fn apply_gain(frame: &mut Frame, gain: f32) {
    for s in frame.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

/// Écrêtage doux : identique jusqu'à 0,8, puis une courbe qui tend vers ±1 sans jamais l'atteindre
/// ni faire de marche (plusieurs personnes qui parlent en même temps ne doivent pas saturer).
pub fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    let shaped = KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh();
    shaped.copysign(x)
}

/// Mélange : ajoute `input` à `out` (qui part de silence).
pub fn add(out: &mut Frame, input: &Frame) {
    for (o, i) in out.iter_mut().zip(input) {
        *o += i;
    }
}

/// Fin du mélange : volume de sortie, puis écrêtage doux.
pub fn finish(out: &mut Frame, volume: f32) {
    for s in out.iter_mut() {
        *s = soft_clip(*s * volume);
    }
}

/// Seuil de la parole : valeur efficace d'une trame, environ -40 dBFS.
pub const SPEECH_THRESHOLD: f32 = 0.01;
/// Durée de maintien : 15 trames, soit 300 ms.
const SPEECH_HOLD: u32 = 15;

/// Détecteur de parole par l'énergie : « parle » si une des 15 dernières trames (300 ms) dépasse
/// le seuil. Sert pour soi et pour chaque participant.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpeechDetector {
    left: u32,
}

impl SpeechDetector {
    /// Une trame de plus, de valeur efficace `rms` ; rend si la personne parle.
    pub fn update(&mut self, rms: f32) -> bool {
        self.left = if rms >= SPEECH_THRESHOLD { SPEECH_HOLD } else { self.left.saturating_sub(1) };
        self.speaking()
    }

    pub fn speaking(&self) -> bool {
        self.left > 0
    }

    pub fn reset(&mut self) {
        self.left = 0;
    }
}

/// Une trame de silence.
pub const fn silence() -> Frame {
    [0.0; FRAME]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tone_share;

    fn tone(freq: f64, amplitude: f32) -> Frame {
        let mut f = silence();
        let v = crate::testutil::sine(freq, amplitude, FRAME, 48_000.0);
        f.copy_from_slice(&v);
        f
    }

    #[test]
    fn rms_of_known_signals() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&silence()), 0.0);
        assert!((rms(&[1.0, -1.0, 1.0, -1.0]) - 1.0).abs() < 1e-6);
        assert!((rms(&tone(500.0, 0.5)) - 0.5 / 2f32.sqrt()).abs() < 1e-3);
    }

    #[test]
    fn level_is_a_decibel_scale() {
        assert_eq!(level(0.0), 0.0);
        assert_eq!(level(0.0001), 0.0, "-80 dBFS");
        assert!((level(0.001) - 0.0).abs() < 1e-6, "-60 dBFS : le plancher");
        assert!((level(0.01) - 1.0 / 3.0).abs() < 1e-5, "-40 dBFS");
        assert!((level(0.1) - 2.0 / 3.0).abs() < 1e-5, "-20 dBFS");
        assert_eq!(level(1.0), 1.0);
        assert_eq!(level(5.0), 1.0, "au-delà : plafonné");
        // Monotone.
        assert!(level(0.02) < level(0.03));
    }

    #[test]
    fn smoothing_rises_fast_and_falls_slowly() {
        assert_eq!(smooth(0.2, 0.8), 0.8);
        let mut v = 0.8;
        for _ in 0..10 {
            v = smooth(v, 0.0);
        }
        assert!(v > 0.1 && v < 0.3, "{v}");
    }

    #[test]
    fn gain_clamps() {
        let mut f = tone(440.0, 0.5);
        apply_gain(&mut f, 4.0);
        assert!(f.iter().all(|x| x.abs() <= 1.0));
        assert!(f.iter().any(|x| (x.abs() - 1.0).abs() < 1e-6), "écrêté à pleine échelle");
        let mut g = tone(440.0, 0.5);
        apply_gain(&mut g, 0.0);
        assert_eq!(g, silence());
    }

    #[test]
    fn soft_clip_is_identity_below_the_knee_and_bounded_above() {
        for x in [-0.8f32, -0.3, 0.0, 0.5, 0.8] {
            assert_eq!(soft_clip(x), x);
        }
        let mut last = soft_clip(0.8);
        for i in 1..=200 {
            let y = soft_clip(0.8 + i as f32 * 0.1);
            assert!(y >= last, "monotone");
            assert!(y < 1.0 + 1e-6, "borné par 1 : {y}");
            last = y;
        }
        assert!((soft_clip(-3.0) + soft_clip(3.0)).abs() < 1e-6, "symétrique");
        assert!(soft_clip(0.81) - soft_clip(0.8) < 0.011, "pas de marche au genou");
    }

    #[test]
    fn mixing_adds_then_limits() {
        // Des fréquences à nombre entier de périodes par trame (multiples de 50 Hz).
        let a = tone(500.0, 0.3);
        let b = tone(1000.0, 0.3);
        let mut out = silence();
        add(&mut out, &a);
        add(&mut out, &b);
        finish(&mut out, 1.0);
        // Les deux fréquences sont là, et le mélange est la somme (sous le genou de l'écrêtage).
        let expected: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        for (o, e) in out.iter().zip(&expected) {
            assert!((o - e).abs() < 0.01, "{o} contre {e}");
        }
        let shown: Vec<f32> = [out.as_slice(), out.as_slice(), out.as_slice()].concat();
        assert!(tone_share(&shown, 500.0) > 0.45 && tone_share(&shown, 1000.0) > 0.45);
    }

    #[test]
    fn mixing_loud_voices_never_exceeds_full_scale() {
        let loud = tone(300.0, 1.0);
        let mut out = silence();
        for _ in 0..8 {
            add(&mut out, &loud);
        }
        finish(&mut out, 2.0);
        assert!(out.iter().all(|x| x.abs() <= 1.0));
    }

    #[test]
    fn volume_zero_is_silence() {
        let mut out = tone(440.0, 0.9);
        finish(&mut out, 0.0);
        assert_eq!(out, silence());
    }

    #[test]
    fn speech_detector_holds_for_300_ms() {
        let mut d = SpeechDetector::default();
        assert!(!d.update(0.0));
        assert!(!d.update(SPEECH_THRESHOLD * 0.9), "sous le seuil");
        assert!(d.update(SPEECH_THRESHOLD), "au seuil");
        // 14 trames de silence de plus : encore « parle » (15 trames avec la bruyante).
        for i in 0..14 {
            assert!(d.update(0.0), "trame de silence {}", i + 1);
        }
        assert!(!d.update(0.0), "300 ms après la dernière trame bruyante");
        // Une trame bruyante relance tout.
        assert!(d.update(0.2));
        d.reset();
        assert!(!d.speaking());
    }

    #[test]
    fn speech_detector_sees_a_real_voice_level_but_not_the_noise_floor() {
        let mut d = SpeechDetector::default();
        let noise = tone(200.0, 0.002);
        for _ in 0..50 {
            assert!(!d.update(rms(&noise)), "bruit de fond à -60 dBFS");
        }
        let voice = tone(200.0, 0.2);
        assert!(d.update(rms(&voice)));
    }
}
