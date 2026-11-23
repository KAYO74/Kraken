//! Aides des tests unitaires : analyse d'un signal.

use std::f64::consts::TAU;

/// Puissance (carré du module) de la composante à `freq` Hz d'un signal échantillonné à `rate`
/// Hz, par l'algorithme de Goertzel.
pub(crate) fn goertzel(samples: &[f32], freq: f64, rate: f64) -> f64 {
    let w = TAU * freq / rate;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in samples {
        let s = f64::from(x) + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    s1 * s1 + s2 * s2 - coeff * s1 * s2
}

/// Part de l'énergie du signal qui est à `freq` Hz (1 : une sinusoïde pure ; plus le point de
/// la TFD tombe juste, plus c'est exact).
pub(crate) fn tone_share_at(samples: &[f32], freq: f64, rate: f64) -> f64 {
    let total: f64 = samples.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    if total == 0.0 {
        return 0.0;
    }
    2.0 * goertzel(samples, freq, rate) / samples.len() as f64 / total
}

/// [`tone_share_at`] à 48 kHz.
pub(crate) fn tone_share(samples: &[f32], freq: f64) -> f64 {
    tone_share_at(samples, freq, 48_000.0)
}

/// `count` échantillons d'une sinusoïde de `freq` Hz et d'amplitude `amplitude`, à `rate` Hz.
pub(crate) fn sine(freq: f64, amplitude: f32, count: usize, rate: f64) -> Vec<f32> {
    (0..count).map(|i| amplitude * (TAU * freq * i as f64 / rate).sin() as f32).collect()
}
