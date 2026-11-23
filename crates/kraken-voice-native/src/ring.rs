//! Tampon circulaire d'échantillons entre le rappel temps réel du périphérique (qui ne doit ni
//! attendre ni allouer) et le moteur : un producteur, un consommateur, sans verrou ni code
//! `unsafe` (les échantillons sont des `f32` rangés dans des `AtomicU32`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

struct Shared {
    buf: Box<[AtomicU32]>,
    /// Échantillons écrits depuis le début (compteur sans fin : l'indice est ce compteur modulo la taille).
    head: AtomicUsize,
    /// Échantillons lus depuis le début.
    tail: AtomicUsize,
}

impl Shared {
    fn len(&self) -> usize {
        self.head.load(Ordering::Acquire).wrapping_sub(self.tail.load(Ordering::Acquire))
    }
}

/// Côté producteur.
pub(crate) struct RingTx(Arc<Shared>);

/// Côté consommateur.
pub(crate) struct RingRx(Arc<Shared>);

/// Un tampon de `capacity` échantillons.
pub(crate) fn ring(capacity: usize) -> (RingTx, RingRx) {
    let capacity = capacity.max(1);
    let shared = Arc::new(Shared {
        buf: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (RingTx(shared.clone()), RingRx(shared))
}

impl RingTx {
    /// Écrit ce qui tient ; rend le nombre d'échantillons écrits (le reste est perdu).
    pub(crate) fn push(&mut self, data: &[f32]) -> usize {
        let s = &*self.0;
        let head = s.head.load(Ordering::Relaxed);
        let free = s.buf.len() - head.wrapping_sub(s.tail.load(Ordering::Acquire));
        let n = data.len().min(free);
        for (i, x) in data[..n].iter().enumerate() {
            s.buf[head.wrapping_add(i) % s.buf.len()].store(x.to_bits(), Ordering::Relaxed);
        }
        s.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }

    /// Échantillons écrits et pas encore lus.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl RingRx {
    /// Lit au plus `out.len()` échantillons ; rend le nombre lu.
    pub(crate) fn pop(&mut self, out: &mut [f32]) -> usize {
        let s = &*self.0;
        let tail = s.tail.load(Ordering::Relaxed);
        let n = out.len().min(s.head.load(Ordering::Acquire).wrapping_sub(tail));
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = f32::from_bits(s.buf[tail.wrapping_add(i) % s.buf.len()].load(Ordering::Relaxed));
        }
        s.tail.store(tail.wrapping_add(n), Ordering::Release);
        n
    }

    /// Jette les `n` plus anciens échantillons (au plus ce qu'il y a).
    pub(crate) fn discard(&mut self, n: usize) {
        let s = &*self.0;
        let tail = s.tail.load(Ordering::Relaxed);
        let n = n.min(s.head.load(Ordering::Acquire).wrapping_sub(tail));
        s.tail.store(tail.wrapping_add(n), Ordering::Release);
    }

    /// Échantillons écrits et pas encore lus.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_and_wraps_around() {
        let (mut tx, mut rx) = ring(8);
        assert_eq!(rx.len(), 0);
        let mut out = [0.0; 8];
        let mut next = 0.0f32;
        for round in 0..20 {
            let data: Vec<f32> = (0..5)
                .map(|_| {
                    next += 1.0;
                    next
                })
                .collect();
            assert_eq!(tx.push(&data), 5, "tour {round}");
            assert_eq!(rx.len(), 5);
            let n = rx.pop(&mut out);
            assert_eq!(&out[..n], &data[..]);
        }
    }

    #[test]
    fn drops_what_does_not_fit_and_discards_the_oldest_on_request() {
        let (mut tx, mut rx) = ring(4);
        assert_eq!(tx.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(tx.push(&[4.0, 5.0, 6.0]), 1, "un seul tient");
        assert_eq!(tx.len(), 4);
        rx.discard(2);
        assert_eq!(rx.len(), 2);
        let mut out = [0.0; 4];
        assert_eq!(rx.pop(&mut out), 2);
        assert_eq!(&out[..2], &[3.0, 4.0]);
        rx.discard(10);
        assert_eq!(rx.len(), 0);
        assert_eq!(rx.pop(&mut out), 0);
    }

    #[test]
    fn works_across_threads() {
        let (mut tx, mut rx) = ring(1024);
        let total = 200_000usize;
        let producer = std::thread::spawn(move || {
            let mut i = 0usize;
            while i < total {
                let end = (i + 100).min(total);
                let chunk: Vec<f32> = (i..end).map(|v| v as f32).collect();
                let n = tx.push(&chunk);
                i += n;
                if n == 0 {
                    std::thread::yield_now();
                }
            }
        });
        let mut expected = 0usize;
        let mut out = [0.0f32; 64];
        while expected < total {
            let n = rx.pop(&mut out);
            for v in &out[..n] {
                assert_eq!(*v, expected as f32);
                expected += 1;
            }
            if n == 0 {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
    }
}
