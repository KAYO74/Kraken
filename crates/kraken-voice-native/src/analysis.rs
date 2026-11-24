//! Analyse d'un signal échantillonné : puissance à une fréquence (Goertzel), valeur efficace.
//! Sert aux statistiques du mode synthétique ([`crate::control::Controller::stats`]) et aux tests.

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
    // Négatif de quelques ulp près sur un signal sans rien à cette fréquence : ramené à 0.
    (2.0 * goertzel(samples, freq, rate) / samples.len() as f64 / total).max(0.0)
}

/// Valeur efficace du signal (0 s'il est vide).
pub(crate) fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}
