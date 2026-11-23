//! Le tampon de gigue d'un participant : il reçoit les paquets Opus dans le désordre et à des
//! instants irréguliers, et en rend un toutes les 20 ms, à la cadence de la lecture.
//!
//! Il attend quelques paquets avant de commencer à rendre (`target`, 60 ms par défaut) pour
//! absorber les irrégularités, remet les paquets dans l'ordre de leur horodatage RTP, jette ceux
//! qui arrivent après leur tour, signale un paquet perdu (le décodeur masque alors la perte, avec
//! la correction d'erreur du paquet suivant s'il en a une) et redevient silencieux quand le flux
//! s'arrête, pour repartir de zéro à la prochaine prise de parole.
//!
//! Il suppose des paquets de 20 ms, ce que font les navigateurs et ce client.

use std::collections::BTreeMap;

/// Durée d'un paquet, en échantillons à 48 kHz.
const PACKET: u64 = 960;
/// Un paquet qui tombe à moins d'une demi-durée de l'attendu est « l'attendu ».
const TOLERANCE: u64 = PACKET / 2;
/// Au-delà de ce nombre de paquets d'écart, ce n'est plus une perte mais une nouvelle prise de
/// parole : on repart sur le paquet reçu, sans rien masquer.
const MAX_GAP: u64 = 10;
/// Pertes de suite masquées quand plus rien n'arrive, avant de conclure que le flux est fini.
const MAX_CONCEAL: usize = 5;
/// Paquets gardés au plus, en plus de `target` ; au-delà, les plus anciens sont jetés.
const MAX_EXTRA: usize = 8;
/// Taille maximale d'un paquet accepté (un paquet Opus de 120 ms en fait au plus un peu plus de 1 Ko).
const MAX_PACKET_BYTES: usize = 4000;

/// Ce que la lecture doit jouer pour les 20 ms qui viennent.
#[derive(Debug, PartialEq, Eq)]
pub enum Pull {
    /// Rien : personne ne parle (ou le tampon se remplit encore).
    Silence,
    /// Le paquet à décoder.
    Packet(Vec<u8>),
    /// Un paquet manque : à masquer. `fec` est le paquet suivant, s'il vient juste après : il peut
    /// contenir de quoi retrouver celui-ci.
    Lost { fec: Option<Vec<u8>> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Pas de flux.
    Idle,
    /// Des paquets arrivent, la lecture n'a pas commencé.
    Buffering,
    /// La lecture suit le flux.
    Playing,
}

#[derive(Debug)]
pub struct JitterBuffer {
    /// Paquets en attente, par horodatage RTP (échantillons à 48 kHz).
    packets: BTreeMap<u64, Vec<u8>>,
    state: State,
    /// Horodatage du prochain paquet à rendre.
    next: u64,
    /// Paquets à avoir avant de commencer.
    target: usize,
    /// Tours de lecture passés à attendre, au début.
    waited: usize,
    /// Pertes masquées de suite.
    concealed: usize,
    /// Paquets arrivés trop tard, jetés.
    late: u64,
}

impl JitterBuffer {
    pub fn new(target: usize) -> Self {
        Self {
            packets: BTreeMap::new(),
            state: State::Idle,
            next: 0,
            target: target.max(1),
            waited: 0,
            concealed: 0,
            late: 0,
        }
    }

    /// Paquets arrivés trop tard pour être joués, depuis le début.
    #[cfg(test)]
    pub fn late(&self) -> u64 {
        self.late
    }

    /// Paquets en attente.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.packets.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// La lecture suit un flux (ou s'apprête à le faire).
    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.state != State::Idle
    }

    /// Jette tout et attend une nouvelle prise de parole.
    pub fn clear(&mut self) {
        self.packets.clear();
        self.state = State::Idle;
        self.waited = 0;
        self.concealed = 0;
    }

    /// Un paquet reçu, d'horodatage RTP `ts` (échantillons à 48 kHz).
    pub fn push(&mut self, ts: u64, data: &[u8]) {
        if data.is_empty() || data.len() > MAX_PACKET_BYTES {
            return;
        }
        match self.state {
            State::Idle => {
                self.state = State::Buffering;
                self.waited = 0;
            }
            State::Buffering => {}
            State::Playing => {
                if ts + TOLERANCE <= self.next {
                    self.late += 1;
                    return;
                }
            }
        }
        self.packets.insert(ts, data.to_vec());
        let mut trimmed = false;
        while self.packets.len() > self.target + MAX_EXTRA {
            self.packets.pop_first();
            trimmed = true;
        }
        // Trop de retard accumulé (l'émetteur va plus vite que la lecture) : on saute au plus ancien gardé.
        if trimmed
            && self.state == State::Playing
            && let Some((&first, _)) = self.packets.first_key_value()
        {
            self.next = first;
        }
    }

    /// À appeler une fois par trame de 20 ms jouée.
    pub fn pull(&mut self) -> Pull {
        match self.state {
            State::Idle => return Pull::Silence,
            State::Buffering => {
                self.waited += 1;
                if self.packets.len() < self.target && self.waited <= self.target {
                    return Pull::Silence;
                }
                let Some((&first, _)) = self.packets.first_key_value() else {
                    self.state = State::Idle;
                    return Pull::Silence;
                };
                self.state = State::Playing;
                self.next = first;
                self.concealed = 0;
            }
            State::Playing => {}
        }
        // Arrivés trop tard (leur tour a été masqué).
        while let Some((&k, _)) = self.packets.first_key_value() {
            if k + TOLERANCE > self.next {
                break;
            }
            self.packets.pop_first();
            self.late += 1;
        }
        let Some((&k, _)) = self.packets.first_key_value() else {
            // Plus rien : on masque un moment, puis le flux est fini.
            if self.concealed < MAX_CONCEAL {
                self.concealed += 1;
                self.next += PACKET;
                return Pull::Lost { fec: None };
            }
            self.clear();
            return Pull::Silence;
        };
        if k <= self.next + TOLERANCE {
            return self.take_next();
        }
        if k - self.next <= MAX_GAP * PACKET {
            // Un trou : le paquet attendu n'est pas venu, un suivant oui.
            let fec = (k <= self.next + PACKET + TOLERANCE).then(|| self.packets.get(&k).cloned()).flatten();
            self.next += PACKET;
            self.concealed += 1;
            return Pull::Lost { fec };
        }
        // Très loin devant : une nouvelle prise de parole, sans rien à masquer.
        self.next = k;
        self.take_next()
    }

    /// Rend le plus ancien paquet, qui est l'attendu.
    fn take_next(&mut self) -> Pull {
        match self.packets.pop_first() {
            Some((k, data)) => {
                self.next = k + PACKET;
                self.concealed = 0;
                Pull::Packet(data)
            }
            None => Pull::Silence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un paquet reconnaissable, au `n`-ième rang.
    fn p(n: u64) -> Vec<u8> {
        vec![n as u8, 1, 2]
    }

    fn feed(j: &mut JitterBuffer, ranks: &[u64]) {
        for &n in ranks {
            j.push(1_000_000 + n * PACKET, &p(n));
        }
    }

    #[test]
    fn plays_in_order_after_the_prefill() {
        let mut j = JitterBuffer::new(3);
        assert_eq!(j.pull(), Pull::Silence, "rien reçu");
        feed(&mut j, &[0, 1]);
        assert!(j.is_active());
        assert_eq!(j.pull(), Pull::Silence, "deux paquets sur trois : on attend");
        feed(&mut j, &[2]);
        assert_eq!(j.pull(), Pull::Packet(p(0)));
        assert_eq!(j.pull(), Pull::Packet(p(1)));
        feed(&mut j, &[3]);
        assert_eq!(j.pull(), Pull::Packet(p(2)));
        assert_eq!(j.pull(), Pull::Packet(p(3)));
        assert_eq!(j.late(), 0);
    }

    #[test]
    fn starts_anyway_when_the_stream_is_short() {
        let mut j = JitterBuffer::new(3);
        feed(&mut j, &[0]);
        for _ in 0..3 {
            assert_eq!(j.pull(), Pull::Silence);
        }
        assert_eq!(j.pull(), Pull::Packet(p(0)), "il n'en viendra pas d'autre : on joue le seul qu'on a");
    }

    #[test]
    fn puts_packets_back_in_order_and_ignores_duplicates() {
        let mut j = JitterBuffer::new(3);
        feed(&mut j, &[2, 0, 1, 1, 0]);
        assert_eq!(j.len(), 3);
        assert_eq!((j.pull(), j.pull(), j.pull()), (Pull::Packet(p(0)), Pull::Packet(p(1)), Pull::Packet(p(2))));
    }

    #[test]
    fn reports_a_lost_packet_with_the_next_one_for_fec() {
        let mut j = JitterBuffer::new(3);
        feed(&mut j, &[0, 2, 3]);
        assert_eq!(j.pull(), Pull::Packet(p(0)));
        assert_eq!(j.pull(), Pull::Lost { fec: Some(p(2)) });
        assert_eq!(j.pull(), Pull::Packet(p(2)));
        assert_eq!(j.pull(), Pull::Packet(p(3)));
    }

    #[test]
    fn longer_holes_are_concealed_packet_by_packet() {
        let mut j = JitterBuffer::new(2);
        feed(&mut j, &[0, 3]);
        assert_eq!(j.pull(), Pull::Packet(p(0)));
        assert_eq!(j.pull(), Pull::Lost { fec: None }, "le paquet 1, et le 3 n'est pas son suivant");
        assert_eq!(j.pull(), Pull::Lost { fec: Some(p(3)) }, "le paquet 2 : le 3 le suit");
        assert_eq!(j.pull(), Pull::Packet(p(3)));
    }

    #[test]
    fn a_packet_after_its_turn_is_dropped() {
        let mut j = JitterBuffer::new(2);
        feed(&mut j, &[0, 2]);
        assert_eq!(j.pull(), Pull::Packet(p(0)));
        assert_eq!(j.pull(), Pull::Lost { fec: Some(p(2)) }, "le 1 est masqué");
        feed(&mut j, &[1]);
        assert_eq!(j.len(), 1, "le 1, en retard, n'est pas gardé");
        assert_eq!(j.late(), 1);
        assert_eq!(j.pull(), Pull::Packet(p(2)));
    }

    #[test]
    fn the_stream_ends_after_a_few_concealed_packets_and_restarts_cleanly() {
        let mut j = JitterBuffer::new(2);
        feed(&mut j, &[0, 1]);
        assert_eq!(j.pull(), Pull::Packet(p(0)));
        assert_eq!(j.pull(), Pull::Packet(p(1)));
        for _ in 0..MAX_CONCEAL {
            assert_eq!(j.pull(), Pull::Lost { fec: None });
        }
        assert_eq!(j.pull(), Pull::Silence, "flux terminé");
        assert!(!j.is_active());
        assert_eq!(j.pull(), Pull::Silence);
        // Il reparle, bien plus tard : on attend le tampon, sans rien masquer.
        feed(&mut j, &[100, 101]);
        assert!(j.is_active());
        assert_eq!(j.pull(), Pull::Packet(p(100)));
        assert_eq!(j.pull(), Pull::Packet(p(101)));
    }

    #[test]
    fn a_far_jump_is_a_new_talk_spurt_not_a_loss() {
        let mut j = JitterBuffer::new(2);
        feed(&mut j, &[0, 1]);
        assert_eq!(j.pull(), Pull::Packet(p(0)));
        assert_eq!(j.pull(), Pull::Packet(p(1)));
        feed(&mut j, &[200]);
        assert_eq!(j.pull(), Pull::Packet(p(200)), "pas de masquage sur 200 paquets");
    }

    #[test]
    fn piles_up_no_more_than_its_limit_and_catches_up() {
        let mut j = JitterBuffer::new(3);
        let ranks: Vec<u64> = (0..40).collect();
        feed(&mut j, &ranks);
        assert_eq!(j.len(), 3 + MAX_EXTRA);
        // Le plus ancien gardé est le 29 (40 - 11).
        assert_eq!(j.pull(), Pull::Packet(p(29)));
        // Même chose en cours de lecture : l'émetteur va plus vite, on saute en avant.
        feed(&mut j, &(40..80).collect::<Vec<_>>());
        assert_eq!(j.len(), 3 + MAX_EXTRA);
        assert_eq!(j.pull(), Pull::Packet(p(69)));
    }

    #[test]
    fn ignores_empty_and_huge_packets() {
        let mut j = JitterBuffer::new(2);
        j.push(0, &[]);
        j.push(960, &vec![0; MAX_PACKET_BYTES + 1]);
        assert!(j.is_empty() && !j.is_active());
    }

    #[test]
    fn clear_goes_back_to_silence() {
        let mut j = JitterBuffer::new(2);
        feed(&mut j, &[0, 1, 2]);
        j.clear();
        assert!(j.is_empty() && !j.is_active());
        assert_eq!(j.pull(), Pull::Silence);
    }
}
