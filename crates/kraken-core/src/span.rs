//! Durées et temps écoulé, écrits dans la langue de l'interface pour les phrases (« pendant 1 h »,
//! « il y a 5 min »). Aucune horloge ici : l'appelant donne la langue et les nombres.

use crate::i18n::{Lang, Plural};

/// Une durée en secondes, courte : « 45 s », « 1 min », « 1 h 30 », « 2 h », « 1 jour », « 7 jours »,
/// « 1 jour 6 h » (en anglais : « 1 h 30 min », « 1 day », « 7 days »). Zéro : « 0 s ».
pub fn duration(lang: Lang, secs: u64) -> String {
    let (days, hours, mins) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    let day = |n: u64| match (lang, lang.plural(n)) {
        (Lang::Fr, Plural::One) => format!("{n} jour"),
        (Lang::Fr, Plural::Other) => format!("{n} jours"),
        (Lang::En, Plural::One) => format!("{n} day"),
        (Lang::En, Plural::Other) => format!("{n} days"),
    };
    match (days, hours, mins) {
        (0, 0, 0) => format!("{secs} s"),
        (0, 0, m) if secs < 3600 && secs.is_multiple_of(60) => format!("{m} min"),
        (0, 0, _) => format!("{} min {} s", secs / 60, secs % 60),
        (0, h, 0) => format!("{h} h"),
        (0, h, m) if lang == Lang::En => format!("{h} h {m} min"),
        (0, h, m) => format!("{h} h {m:02}"),
        (d, 0, _) => day(d),
        (d, h, _) => format!("{} {h} h", day(d)),
    }
}

/// Le temps écoulé depuis un événement récent : « à l'instant » sous la minute, « il y a 5 min » sous
/// l'heure (« just now », « 5 min ago »). `None` au-delà (ou pour une date dans le futur d'une horloge un
/// peu en avance, qui compte comme « à l'instant ») : l'appelant affiche alors l'heure.
pub fn ago(lang: Lang, elapsed_ms: i64) -> Option<String> {
    let secs = elapsed_ms.max(0) / 1000;
    match (secs, lang) {
        (0..=59, Lang::Fr) => Some("à l'instant".into()),
        (0..=59, Lang::En) => Some("just now".into()),
        (60..=3599, Lang::Fr) => Some(format!("il y a {} min", secs / 60)),
        (60..=3599, Lang::En) => Some(format!("{} min ago", secs / 60)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        let d = |s| duration(Lang::Fr, s);
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
    fn durations_follow_the_language() {
        let d = |s| duration(Lang::En, s);
        assert_eq!([d(0), d(45), d(60), d(300), d(3599)], ["0 s", "45 s", "1 min", "5 min", "59 min 59 s"]);
        assert_eq!([d(3600), d(5400), d(7200), d(3660)], ["1 h", "1 h 30 min", "2 h", "1 h 1 min"]);
        assert_eq!([d(86_400), d(172_800), d(28 * 86_400)], ["1 day", "2 days", "28 days"]);
        assert_eq!(d(86_400 + 6 * 3600), "1 day 6 h");
    }

    #[test]
    fn recent_events_are_relative_and_old_ones_are_not() {
        let fr = |ms| ago(Lang::Fr, ms);
        assert_eq!(fr(-5000).as_deref(), Some("à l'instant"));
        assert_eq!(fr(59_999).as_deref(), Some("à l'instant"));
        assert_eq!(fr(60_000).as_deref(), Some("il y a 1 min"));
        assert_eq!(fr(5 * 60_000 + 10).as_deref(), Some("il y a 5 min"));
        assert_eq!(fr(3_599_999).as_deref(), Some("il y a 59 min"));
        assert_eq!(fr(3_600_000), None);
        let en = |ms| ago(Lang::En, ms);
        assert_eq!(en(-5000).as_deref(), Some("just now"));
        assert_eq!(en(60_000).as_deref(), Some("1 min ago"));
        assert_eq!(en(5 * 60_000 + 10).as_deref(), Some("5 min ago"));
        assert_eq!(en(3_600_000), None);
    }
}
