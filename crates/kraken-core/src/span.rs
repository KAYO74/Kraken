//! Durées et temps écoulé, écrits dans la langue de l'interface pour les phrases (« pendant 1 h »,
//! « il y a 5 min »). Aucune horloge ici : l'appelant donne la langue et les nombres. Les chiffres sont latins
//! dans toutes les langues, le persan compris (« 5 دقیقه پیش ») : c'est le choix de toute l'interface (`-u-nu-latn`).
//! L'allemand abrège « Min. » et « Std. » (« vor 5 Min. », « 1 Std. 30 Min. ») et accorde « Tag » / « Tage ».

use crate::i18n::{Lang, Plural};

/// Une durée en secondes, courte : « 45 s », « 1 min », « 1 h 30 », « 2 h », « 1 jour », « 7 jours »,
/// « 1 jour 6 h » (en anglais : « 1 h 30 min », « 1 day », « 7 days » ; en persan : « 45 ثانیه », « 1 ساعت و 30 دقیقه »,
/// « 2 روز », « 1 روز و 6 ساعت » ; en allemand : « 45 s », « 1 Min. », « 1 Std. 30 Min. », « 1 Tag », « 2 Tage »,
/// « 1 Tag 6 Std. »). Zéro : « 0 s » (« 0 ثانیه »). Le persan garde le nom au singulier après un nombre.
pub fn duration(lang: Lang, secs: u64) -> String {
    let (days, hours, mins) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    let day = |n: u64| match (lang, lang.plural(n)) {
        (Lang::Fr, Plural::One) => format!("{n} jour"),
        (Lang::Fr, Plural::Other) => format!("{n} jours"),
        (Lang::En, Plural::One) => format!("{n} day"),
        (Lang::En, Plural::Other) => format!("{n} days"),
        (Lang::Fa, _) => format!("{n} روز"),
        (Lang::De, Plural::One) => format!("{n} Tag"),
        (Lang::De, Plural::Other) => format!("{n} Tage"),
    };
    // Le persan écrit « 1 دقیقه و 30 ثانیه » : les parties se lient par « و », le nom reste au singulier.
    let fa = lang == Lang::Fa;
    let de = lang == Lang::De;
    match (days, hours, mins) {
        (0, 0, 0) if fa => format!("{secs} ثانیه"),
        (0, 0, 0) => format!("{secs} s"),
        (0, 0, m) if secs < 3600 && secs.is_multiple_of(60) => {
            if fa {
                format!("{m} دقیقه")
            } else if de {
                format!("{m} Min.")
            } else {
                format!("{m} min")
            }
        }
        (0, 0, _) if fa => format!("{} دقیقه و {} ثانیه", secs / 60, secs % 60),
        (0, 0, _) if de => format!("{} Min. {} s", secs / 60, secs % 60),
        (0, 0, _) => format!("{} min {} s", secs / 60, secs % 60),
        (0, h, 0) if fa => format!("{h} ساعت"),
        (0, h, 0) if de => format!("{h} Std."),
        (0, h, 0) => format!("{h} h"),
        (0, h, m) if fa => format!("{h} ساعت و {m} دقیقه"),
        (0, h, m) if de => format!("{h} Std. {m} Min."),
        (0, h, m) if lang == Lang::En => format!("{h} h {m} min"),
        (0, h, m) => format!("{h} h {m:02}"),
        (d, 0, _) => day(d),
        (d, h, _) if fa => format!("{} و {h} ساعت", day(d)),
        (d, h, _) if de => format!("{} {h} Std.", day(d)),
        (d, h, _) => format!("{} {h} h", day(d)),
    }
}

/// Le temps écoulé depuis un événement récent : « à l'instant » sous la minute, « il y a 5 min » sous
/// l'heure (« just now », « 5 min ago » ; « الان », « 5 دقیقه پیش » ; « gerade eben », « vor 5 Min. »). `None` au-delà (ou pour
/// une date dans le futur d'une horloge un peu en avance, qui compte comme « à l'instant ») : l'appelant affiche alors l'heure.
pub fn ago(lang: Lang, elapsed_ms: i64) -> Option<String> {
    let secs = elapsed_ms.max(0) / 1000;
    match (secs, lang) {
        (0..=59, Lang::Fr) => Some("à l'instant".into()),
        (0..=59, Lang::En) => Some("just now".into()),
        (0..=59, Lang::Fa) => Some("الان".into()),
        (0..=59, Lang::De) => Some("gerade eben".into()),
        (60..=3599, Lang::Fr) => Some(format!("il y a {} min", secs / 60)),
        (60..=3599, Lang::En) => Some(format!("{} min ago", secs / 60)),
        (60..=3599, Lang::Fa) => Some(format!("{} دقیقه پیش", secs / 60)),
        (60..=3599, Lang::De) => Some(format!("vor {} Min.", secs / 60)),
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
    fn persian_durations_use_latin_digits_and_the_conjunction() {
        let d = |s| duration(Lang::Fa, s);
        assert_eq!(
            [d(0), d(5), d(45), d(60), d(300), d(3599)],
            ["0 ثانیه", "5 ثانیه", "45 ثانیه", "1 دقیقه", "5 دقیقه", "59 دقیقه و 59 ثانیه"]
        );
        assert_eq!([d(3600), d(5400), d(7200), d(3660)], ["1 ساعت", "1 ساعت و 30 دقیقه", "2 ساعت", "1 ساعت و 1 دقیقه"]);
        assert_eq!([d(86_400), d(172_800), d(604_800), d(28 * 86_400)], ["1 روز", "2 روز", "7 روز", "28 روز"]);
        assert_eq!(d(86_400 + 6 * 3600), "1 روز و 6 ساعت");
        assert_eq!(d(90), "1 دقیقه و 30 ثانیه");
        // Jamais de chiffres persans ou arabo-indiens, jamais d'espace en bord ni de demi-espace en tête ou en queue.
        for secs in [0, 1, 59, 60, 61, 3599, 3600, 3661, 86_399, 86_400, 90_061, 604_800, u32::MAX as u64] {
            let text = d(secs);
            assert!(!text.chars().any(|c| ('۰'..='۹').contains(&c) || ('٠'..='٩').contains(&c)), "{text}");
            assert!(text.chars().any(|c| c.is_ascii_digit()), "{text}");
            assert_eq!(text, text.trim(), "{text}");
            assert!(!text.starts_with('\u{200c}') && !text.ends_with('\u{200c}'), "{text}");
        }
    }

    #[test]
    fn german_durations_abbreviate_and_agree_the_day() {
        let d = |s| duration(Lang::De, s);
        assert_eq!(
            [d(0), d(5), d(45), d(60), d(300), d(3599)],
            ["0 s", "5 s", "45 s", "1 Min.", "5 Min.", "59 Min. 59 s"]
        );
        assert_eq!([d(3600), d(5400), d(7200), d(3660)], ["1 Std.", "1 Std. 30 Min.", "2 Std.", "1 Std. 1 Min."]);
        assert_eq!([d(86_400), d(172_800), d(604_800), d(28 * 86_400)], ["1 Tag", "2 Tage", "7 Tage", "28 Tage"]);
        assert_eq!(d(86_400 + 6 * 3600), "1 Tag 6 Std.");
        assert_eq!(d(2 * 86_400 + 3600), "2 Tage 1 Std.");
        assert_eq!(d(90), "1 Min. 30 s");
        // Chiffres latins, jamais d'espace en bord ni d'espace insécable : le texte se coupe là où on l'attend.
        for secs in [0, 1, 59, 60, 61, 3599, 3600, 3661, 86_399, 86_400, 90_061, 604_800, u32::MAX as u64] {
            let text = d(secs);
            assert!(text.is_ascii(), "{text}");
            assert_eq!(text, text.trim(), "{text}");
        }
    }

    #[test]
    fn every_language_has_the_same_cases() {
        // Mêmes seuils dans toutes les langues : un texte distinct par plage, jamais vide.
        for secs in [0, 5, 45, 60, 90, 300, 3599, 3600, 3660, 5400, 7200, 86_400, 90_000, 172_800, 28 * 86_400] {
            let all: Vec<String> = Lang::ALL.into_iter().map(|l| duration(l, secs)).collect();
            assert!(all.iter().all(|t| !t.is_empty() && t.chars().any(|c| c.is_ascii_digit())), "{secs} : {all:?}");
        }
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
        let fa = |ms| ago(Lang::Fa, ms);
        assert_eq!(fa(-5000).as_deref(), Some("الان"));
        assert_eq!(fa(59_999).as_deref(), Some("الان"));
        assert_eq!(fa(60_000).as_deref(), Some("1 دقیقه پیش"));
        assert_eq!(fa(5 * 60_000 + 10).as_deref(), Some("5 دقیقه پیش"));
        assert_eq!(fa(3_599_999).as_deref(), Some("59 دقیقه پیش"));
        assert_eq!(fa(3_600_000), None);
        let de = |ms| ago(Lang::De, ms);
        assert_eq!(de(-5000).as_deref(), Some("gerade eben"));
        assert_eq!(de(0).as_deref(), Some("gerade eben"));
        assert_eq!(de(59_999).as_deref(), Some("gerade eben"));
        assert_eq!(de(60_000).as_deref(), Some("vor 1 Min."));
        assert_eq!(de(5 * 60_000 + 10).as_deref(), Some("vor 5 Min."));
        assert_eq!(de(3_599_999).as_deref(), Some("vor 59 Min."));
        assert_eq!(de(3_600_000), None);
    }

    #[test]
    fn recent_events_have_a_text_in_every_language_below_one_hour_and_none_above() {
        for l in Lang::ALL {
            for ms in [-1, 0, 30_000, 60_000, 600_000, 3_599_999] {
                assert!(ago(l, ms).is_some_and(|t| !t.is_empty()), "{l:?} {ms}");
            }
            assert_eq!(ago(l, 3_600_000), None, "{l:?}");
        }
    }
}
