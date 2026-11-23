//! Rééchantillonnage en continu d'un signal mono (micro et haut-parleurs ont rarement 48 kHz).
//!
//! Un sinus cardinal fenêtré (Blackman), évalué dans une table avec interpolation linéaire :
//! 32 points de part et d'autre à la fréquence la plus basse, filtre passe-bas intégré quand on
//! réduit la fréquence (rien ne se replie), gain exactement 1 en continu (le poids de chaque
//! sortie est normalisé). Le résultat ne dépend pas de la taille des blocs fournis. Pas d'allocation
//! une fois les tampons dimensionnés.

use std::f64::consts::PI;

/// Zéros du sinus cardinal de chaque côté, à la fréquence la plus basse des deux.
const HALF: usize = 16;
/// Points de la table par zéro.
const RES: usize = 256;

/// Un rééchantillonneur mono de `from` Hz vers `to` Hz.
#[derive(Clone, Debug)]
pub struct Resampler {
    /// Fréquences d'entrée et de sortie : la position avance de `from / to` échantillons d'entrée
    /// par échantillon de sortie, en nombres entiers (aucune dérive, résultat indépendant des blocs).
    from: u64,
    to: u64,
    /// 1 en augmentant la fréquence ; `to / from` en la réduisant (fréquence de coupure relative).
    cutoff: f64,
    /// Demi-largeur du filtre, en échantillons d'entrée.
    reach: usize,
    /// Sinus cardinal fenêtré, de 0 à `HALF` zéros.
    table: Vec<f32>,
    /// L'entrée gardée : de quoi remplir le filtre, puis ce qui n'est pas encore consommé.
    buf: Vec<f32>,
    /// Position de la prochaine sortie dans `buf`, en échantillons d'entrée : `whole + rem / to`.
    whole: usize,
    rem: u64,
    same: bool,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let (from, to) = (from.max(1), to.max(1));
        let cutoff = (f64::from(to) / f64::from(from)).min(1.0);
        let reach = (HALF as f64 / cutoff).ceil() as usize;
        let table = (0..=HALF * RES + 1)
            .map(|i| {
                let x = i as f64 / RES as f64;
                let sinc = if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
                let u = x / HALF as f64;
                let blackman = 0.42 + 0.5 * (PI * u).cos() + 0.08 * (2.0 * PI * u).cos();
                (sinc * blackman.max(0.0)) as f32
            })
            .collect();
        let mut r = Self {
            from: u64::from(from),
            to: u64::from(to),
            cutoff,
            reach,
            table,
            buf: Vec::new(),
            whole: 0,
            rem: 0,
            same: from == to,
        };
        r.reset();
        r
    }

    /// Reprend à zéro (comme si aucun échantillon n'avait été donné).
    pub fn reset(&mut self) {
        self.buf.clear();
        self.buf.resize(self.reach, 0.0);
        self.whole = self.reach;
        self.rem = 0;
    }

    /// Valeur du filtre à `t` échantillons d'entrée du centre.
    fn kernel(&self, t: f64) -> f64 {
        let at = t.abs() * self.cutoff * RES as f64;
        let i = at as usize;
        if i >= HALF * RES {
            return 0.0;
        }
        let frac = at - i as f64;
        f64::from(self.table[i]) * (1.0 - frac) + f64::from(self.table[i + 1]) * frac
    }

    /// Ajoute à `out` les échantillons de sortie que `input` permet de calculer. Au total, la
    /// sortie compte `to / from` fois l'entrée, à un demi-filtre près.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.same {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        let reach = self.reach;
        // Il faut `reach` échantillons après la position, et `reach - 1` avant (gardés).
        while self.whole + reach < self.buf.len() {
            let i0 = self.whole;
            let frac = self.rem as f64 / self.to as f64;
            let (mut sum, mut weight) = (0.0f64, 0.0f64);
            for (k, x) in self.buf[i0 + 1 - reach..=i0 + reach].iter().enumerate() {
                let t = (i0 + 1 - reach + k) as f64 - i0 as f64 - frac;
                let w = self.kernel(t);
                sum += f64::from(*x) * w;
                weight += w;
            }
            out.push(if weight.abs() > 1e-9 { (sum / weight) as f32 } else { 0.0 });
            self.whole += (self.from / self.to) as usize;
            self.rem += self.from % self.to;
            if self.rem >= self.to {
                self.rem -= self.to;
                self.whole += 1;
            }
        }
        let keep_from = self.whole.saturating_sub(reach);
        if keep_from > 0 {
            self.buf.drain(..keep_from);
            self.whole -= keep_from;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sine, tone_share_at};

    fn run(from: u32, to: u32, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut r = Resampler::new(from, to);
        let mut out = Vec::new();
        for c in input.chunks(chunk) {
            r.process(c, &mut out);
        }
        out
    }

    #[test]
    fn same_rate_is_a_copy() {
        let input = sine(440.0, 0.5, 1000, 48_000.0);
        assert_eq!(run(48_000, 48_000, &input, 333), input);
    }

    #[test]
    fn length_follows_the_ratio() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (48_000, 16_000), (16_000, 48_000), (96_000, 48_000)] {
            let input = sine(300.0, 0.5, from as usize, from as f64);
            let out = run(from, to, &input, 480);
            // Le filtre a besoin de `reach` échantillons d'avance : autant de sorties de moins au bout.
            let lag = (Resampler::new(from, to).reach as f64 * f64::from(to) / f64::from(from)).ceil() as i64;
            assert!((out.len() as i64 - to as i64).abs() <= lag + 2, "{from} -> {to} : {} échantillons", out.len());
        }
    }

    #[test]
    fn a_tone_keeps_its_frequency_and_amplitude() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (48_000, 16_000), (16_000, 48_000), (96_000, 48_000)] {
            let input = sine(1000.0, 0.5, from as usize / 2, from as f64);
            let out = run(from, to, &input, 441);
            // Sans le début (le filtre se remplit) ni la fin.
            let body = &out[out.len() / 4..out.len() * 3 / 4];
            let share = tone_share_at(body, 1000.0, f64::from(to));
            assert!(share > 0.999, "{from} -> {to} : {share} de l'énergie à 1 kHz");
            let peak = body.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((peak - 0.5).abs() < 0.01, "{from} -> {to} : crête {peak}");
        }
    }

    #[test]
    fn block_size_does_not_matter() {
        let input = sine(700.0, 0.4, 44_100, 44_100.0);
        let whole = run(44_100, 48_000, &input, input.len());
        for chunk in [1, 7, 441, 480, 4410] {
            assert_eq!(run(44_100, 48_000, &input, chunk), whole, "blocs de {chunk}");
        }
    }

    #[test]
    fn nothing_folds_back_when_reducing() {
        // 20 kHz à 48 kHz, ramené à 16 kHz : au-dessus de la moitié de 16 kHz, doit être éteint
        // (sans filtre il se replierait à 4 kHz).
        let input = sine(20_000.0, 0.8, 48_000, 48_000.0);
        let out = run(48_000, 16_000, &input, 480);
        let body = &out[out.len() / 4..out.len() * 3 / 4];
        let rms = (body.iter().map(|x| x * x).sum::<f32>() / body.len() as f32).sqrt();
        assert!(rms < 0.01, "il reste {rms} (au départ 0,57)");
    }

    #[test]
    fn direct_current_passes_with_unit_gain() {
        let input = vec![0.5f32; 10_000];
        let out = run(44_100, 48_000, &input, 441);
        for x in &out[100..] {
            assert!((x - 0.5).abs() < 1e-4, "{x}");
        }
    }

    #[test]
    fn reset_forgets_the_past() {
        let input = sine(500.0, 0.5, 4800, 48_000.0);
        let mut r = Resampler::new(48_000, 44_100);
        let mut first = Vec::new();
        r.process(&input, &mut first);
        r.reset();
        let mut second = Vec::new();
        r.process(&input, &mut second);
        assert_eq!(first, second);
    }
}
