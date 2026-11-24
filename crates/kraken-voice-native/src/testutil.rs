//! Aides des tests unitaires : analyse d'un signal.

use std::f64::consts::TAU;

pub(crate) use crate::analysis::tone_share_at;

/// [`tone_share_at`] à 48 kHz.
pub(crate) fn tone_share(samples: &[f32], freq: f64) -> f64 {
    tone_share_at(samples, freq, 48_000.0)
}

/// `count` échantillons d'une sinusoïde de `freq` Hz et d'amplitude `amplitude`, à `rate` Hz.
pub(crate) fn sine(freq: f64, amplitude: f32, count: usize, rate: f64) -> Vec<f32> {
    (0..count).map(|i| amplitude * (TAU * freq * i as f64 / rate).sin() as f32).collect()
}
