//! Durées et temps écoulé, écrits en français pour les phrases de l'interface (« pendant 1 h »,
//! « il y a 5 min »). Aucune horloge ici : l'appelant donne les nombres.

/// Une durée en secondes, courte : « 45 s », « 1 min », « 1 h 30 », « 2 h », « 1 jour », « 7 jours »,
/// « 1 jour 6 h ». Zéro : « 0 s ».
pub fn duration(secs: u64) -> String {
    let (days, hours, mins) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    let day = |n: u64| format!("{n} jour{}", if n > 1 { "s" } else { "" });
    match (days, hours, mins) {
        (0, 0, 0) => format!("{secs} s"),
        (0, 0, m) if secs < 3600 && secs.is_multiple_of(60) => format!("{m} min"),
        (0, 0, _) => format!("{} min {} s", secs / 60, secs % 60),
        (0, h, 0) => format!("{h} h"),
        (0, h, m) => format!("{h} h {m:02}"),
        (d, 0, _) => day(d),
        (d, h, _) => format!("{} {h} h", day(d)),
    }
}

/// Le temps écoulé depuis un événement récent : « à l'instant » sous la minute, « il y a 5 min » sous
/// l'heure. `None` au-delà (ou pour une date dans le futur d'une horloge un peu en avance, qui compte
/// comme « à l'instant ») : l'appelant affiche alors l'heure.
pub fn ago(elapsed_ms: i64) -> Option<String> {
    let secs = elapsed_ms.max(0) / 1000;
    match secs {
        0..=59 => Some("à l'instant".into()),
        60..=3599 => Some(format!("il y a {} min", secs / 60)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        let d = duration;
        assert_eq!(
            [d(0), d(5), d(45), d(60), d(300), d(3599)],
            ["0 s", "5 s", "45 s", "1 min", "5 min", "59 min 59 s"]
        );
        assert_eq!([d(3600), d(5400), d(7200), d(3660)], ["1 h", "1 h 30", "2 h", "1 h 01"]);
        assert_eq!([d(86_400), d(172_800), d(604_800), d(28 * 86_400)], ["1 jour", "2 jours", "7 jours", "28 jours"]);
        assert_eq!(d(86_400 + 6 * 3600), "1 jour 6 h");
        assert_eq!(d(90), "1 min 30 s");
    }

    #[test]
    fn recent_events_are_relative_and_old_ones_are_not() {
        assert_eq!(ago(-5000).as_deref(), Some("à l'instant"));
        assert_eq!(ago(59_999).as_deref(), Some("à l'instant"));
        assert_eq!(ago(60_000).as_deref(), Some("il y a 1 min"));
        assert_eq!(ago(5 * 60_000 + 10).as_deref(), Some("il y a 5 min"));
        assert_eq!(ago(3_599_999).as_deref(), Some("il y a 59 min"));
        assert_eq!(ago(3_600_000), None);
    }
}
