//! Texte que les gens saisissent (nom affiché, pronoms, bio) : les caractères qui ne se voient
//! pas. Ils servent à tromper la lecture (un nom qui s'affiche à l'envers, un nom entièrement
//! invisible, deux noms qui semblent identiques) et n'ont rien à faire dans un profil.

/// Le liant d'emoji : sans lui, la famille, le drapeau arc-en-ciel et les autres emojis composés
/// ne s'écrivent pas. Il reste permis.
pub const ZWJ: char = '\u{200D}';
/// Le sélecteur de variante d'emoji (`❤️`) : il reste permis lui aussi.
pub const VS16: char = '\u{FE0F}';
/// Le non-liant sans chasse (ZWNJ, « demi-espace ») : il sépare deux lettres sans espace visible, et la
/// langue persane ne s'écrit pas correctement sans lui (« می‌خواهم »). Permis seulement entre deux lettres
/// d'écriture arabe (voir [`has_hidden`]) ; partout ailleurs il reste invisible, donc refusé.
pub const ZWNJ: char = '\u{200C}';

/// Les points de code « ignorables par défaut » d'Unicode (propriété `Default_Ignorable_Code_Point`,
/// Unicode 15.1) : trait d'union conditionnel, espaces de largeur nulle, marques et isolats de
/// sens d'écriture, joint de mots, sélecteurs de variante, remplisseurs hangul, étiquettes…
const DEFAULT_IGNORABLE: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x034F, 0x034F),
    (0x061C, 0x061C),
    (0x115F, 0x1160),
    (0x17B4, 0x17B5),
    (0x180B, 0x180F),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x206F),
    (0x3164, 0x3164),
    (0xFE00, 0xFE0F),
    (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0),
    (0xFFF0, 0xFFF8),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0000, 0xE0FFF),
];

/// Un caractère qui ne se voit pas et qu'on refuse dans un texte de profil : tous les points de
/// code ignorables par défaut d'Unicode, plus le braille vide (U+2800), **sauf** le liant d'emoji
/// et le sélecteur de variante d'emoji.
pub fn is_hidden(c: char) -> bool {
    if c == ZWJ || c == VS16 {
        return false;
    }
    let n = u32::from(c);
    n == 0x2800 || DEFAULT_IGNORABLE.iter().any(|&(lo, hi)| (lo..=hi).contains(&n))
}

/// Le texte montre-t-il quelque chose ? Au moins un caractère qui n'est ni une espace, ni un
/// caractère de contrôle, ni invisible (le liant et le sélecteur d'emoji ne se voient pas seuls).
pub fn has_visible(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace() && !c.is_control() && !is_hidden(c) && c != ZWJ && c != VS16)
}

/// Une lettre de l'écriture arabe (arabe, persan, ourdou, kurde…) : la seule où le ZWNJ a un sens (« می‌خواهم »).
/// Entre deux lettres d'une autre écriture il ferait deux noms qui se lisent pareil sans être égaux
/// (« мир » et « м‌ир », « ab » et « a‌b »).
fn is_arabic_letter(c: char) -> bool {
    c.is_alphabetic()
        && matches!(u32::from(c), 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Le ZWNJ entre `prev` et `next` joint-il deux lettres arabes ? Il en faut une de chaque côté (donc ni en tête,
/// ni en queue, ni deux de suite), toutes deux d'écriture arabe.
fn joins_letters(prev: Option<char>, next: Option<char>) -> bool {
    let (Some(a), Some(b)) = (prev, next) else { return false };
    is_arabic_letter(a) && is_arabic_letter(b)
}

/// Le texte contient-il un caractère invisible refusé ? Tous ceux d'[`is_hidden`], sauf le ZWNJ placé entre deux
/// lettres d'écriture arabe (un mot persan). Le ZWNJ en tête ou en queue, doublé, ou entre deux lettres d'une
/// autre écriture (latin, cyrillique, grec, CJK…), reste refusé.
pub fn has_hidden(s: &str) -> bool {
    let mut prev = None;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if is_hidden(c) && !(c == ZWNJ && joins_letters(prev, chars.peek().copied())) {
            return true;
        }
        prev = Some(c);
    }
    false
}

/// Un caractère qui s'attache à celui qui précède dans un même « caractère » à l'écran : marques combinantes
/// (accents, voyelles de l'arabe et de l'hébreu, signes indiens et thaï), liants et non-liants, sélecteurs de
/// variante, teintes de peau, étiquettes d'emoji.
fn extends_cluster(c: char) -> bool {
    matches!(u32::from(c),
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670
        | 0x06D6..=0x06ED | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0951..=0x0957 | 0x0E31 | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C | 0x200D | 0x20D0..=0x20FF
        | 0x3099..=0x309A | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F
        | 0xE0100..=0xE01EF)
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Les « caractères » que l'œil voit dans un texte (approximation des graphèmes d'Unicode, sans tables) : une
/// lettre avec ses accents, un emoji avec sa teinte ou lié à d'autres (👨‍👩‍👧), un drapeau (deux indicateurs
/// régionaux). Le repli de `Intl.Segmenter`, qui reste la référence quand le navigateur l'a.
pub fn clusters(s: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut start = 0;
    let (mut prev, mut regional) = (None::<char>, 0usize);
    for (i, c) in s.char_indices() {
        let attaches = prev.is_some_and(|p| {
            extends_cluster(c)
                || p == ZWJ
                || (is_regional_indicator(c) && is_regional_indicator(p) && regional % 2 == 1)
        });
        if !attaches && prev.is_some() {
            out.push(&s[start..i]);
            start = i;
        }
        regional = if is_regional_indicator(c) {
            if attaches || prev.is_none_or(|p| !is_regional_indicator(p)) { regional + 1 } else { 1 }
        } else {
            0
        };
        prev = Some(c);
    }
    if prev.is_some() {
        out.push(&s[start..]);
    }
    out
}

/// Une marque de sens d'écriture (LRM, RLM, embedding, override, isolat) : invisible, jamais une lettre.
fn is_bidi_control(c: char) -> bool {
    matches!(u32::from(c), 0x200E | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
}

/// Les initiales d'un nom pour un avatar sans photo : le premier « caractère » de chacun des deux premiers
/// mots, sinon les deux premiers « caractères » du mot, « ? » pour un nom vide. Mises en capitales pour les
/// écritures qui ont une casse (les autres n'en changent pas). `split(mot, n)` rend les `n` premiers
/// « caractères » d'un mot : `Intl.Segmenter` dans le navigateur, [`clusters`] à défaut.
pub fn initials(name: &str, split: impl Fn(&str, usize) -> Vec<String>) -> String {
    // Un nom venu d'un texte isolé pour le sens d'écriture (U+2066 à U+2069) ne doit pas compter ces marques comme lettres.
    let name: String = name.chars().filter(|c| !is_bidi_control(*c)).collect();
    let words: Vec<&str> = name.split_whitespace().collect();
    let letters: Vec<String> = match words.as_slice() {
        [a, b, ..] => split(a, 1).into_iter().chain(split(b, 1)).collect(),
        [a] => split(a, 2),
        [] => vec!["?".into()],
    };
    letters.concat().to_uppercase()
}

/// [`initials`] avec le repli sans navigateur ([`clusters`]).
pub fn initials_of(name: &str) -> String {
    initials(name, |word, n| clusters(word).into_iter().take(n).map(str::to_string).collect())
}

/// Un nom (rôle, salon, serveur) : rien d'invisible dedans (espace de largeur nulle, changement de
/// sens d'écriture, remplisseur ; le ZWNJ entre deux lettres arabes passe, voir [`has_hidden`]) et au moins un
/// caractère visible.
pub fn visible_name(s: &str) -> bool {
    !has_hidden(s) && has_visible(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_refuse_invisible_and_bidi_characters() {
        assert!(visible_name("Modérateurs"));
        assert!(visible_name("annonces 🎉"));
        assert!(visible_name("👨\u{200D}👩"));
        assert!(!visible_name("evil\u{202E}tset"), "changement de sens d'écriture");
        assert!(!visible_name("\u{200B}"), "rien que de la largeur nulle");
        assert!(!visible_name("a\u{200B}b"));
    }

    #[test]
    fn the_half_space_joins_letters_of_a_script_that_needs_it() {
        // Persan : « می‌خواهم » (je veux), « کتاب‌خانه » (bibliothèque), « نیم‌فاصله ».
        for word in ["می\u{200C}خواهم", "کتاب\u{200C}خانه", "نیم\u{200C}فاصله", "Zoë کتاب\u{200C}خانه"]
        {
            assert!(!has_hidden(word), "{word:?}");
            assert!(visible_name(word), "{word:?}");
        }
        // Deux demi-espaces dans des mots différents.
        assert!(visible_name("می\u{200C}خواهم کتاب\u{200C}خانه"));
        // En tête, en queue, doublé, seul, collé à une espace, un chiffre ou un signe : refusé.
        for bad in [
            "\u{200C}کتاب",
            "کتاب\u{200C}",
            "کتاب\u{200C}\u{200C}خانه",
            "\u{200C}",
            "کتاب \u{200C}خانه",
            "کتاب\u{200C} خانه",
            "کتاب\u{200C}1",
            "1\u{200C}کتاب",
            "کتاب\u{200C}-خانه",
        ] {
            assert!(has_hidden(bad), "{bad:?}");
            assert!(!visible_name(bad), "{bad:?}");
        }
        // Hors de l'écriture arabe il ne sert à rien et ferait des sosies : « ab » et « a‌b », « мир » et « м‌ир »,
        // « 山田 » et « 山‌田 » se liraient pareil. Un mélange latin et arabe est refusé aussi.
        for bad in [
            "a\u{200C}b",
            "Zoë\u{200C}Lou",
            "é\u{200C}è",
            "ǆ\u{200C}a",
            "м\u{200C}ир",
            "при\u{200C}вет",
            "山\u{200C}田",
            "α\u{200C}β",
            "ש\u{200C}ל",
            "a\u{200C}ک",
            "ک\u{200C}a",
        ] {
            assert!(has_hidden(bad), "{bad:?}");
            assert!(!visible_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_half_space_opens_no_door_to_other_invisible_characters() {
        // Autour d'un mot persan correct, tout le reste est toujours refusé.
        for n in [
            0x200Bu32, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2060, 0x2066, 0x2067, 0x2068, 0x2069,
            0xFEFF, 0x061C, 0x00AD, 0x2800,
        ] {
            let c = char::from_u32(n).unwrap();
            for text in [
                format!("می{c}خواهم"),
                format!("می{ZWNJ}{c}خواهم"),
                format!("می{c}{ZWNJ}خواهم"),
                format!("{c}می"),
                format!("می{c}"),
            ] {
                assert!(has_hidden(&text), "U+{n:04X} dans {text:?}");
                assert!(!visible_name(&text), "U+{n:04X} dans {text:?}");
            }
        }
        // Le liant d'emoji et le sélecteur de variante gardent leur place à côté d'un demi-espace.
        assert!(visible_name("می\u{200C}خواهم 👨\u{200D}👩 ❤\u{FE0F}"));
    }

    #[test]
    fn clusters_keep_what_the_eye_sees_together() {
        assert_eq!(clusters("abc"), ["a", "b", "c"]);
        assert_eq!(clusters("e\u{0301}a"), ["e\u{0301}", "a"], "lettre et accent combinant");
        assert_eq!(clusters("👨\u{200D}👩\u{200D}👧x"), ["👨\u{200D}👩\u{200D}👧", "x"], "famille liée");
        assert_eq!(clusters("👍🏽!"), ["👍🏽", "!"], "teinte de peau");
        assert_eq!(clusters("❤\u{FE0F}a"), ["❤\u{FE0F}", "a"], "sélecteur d'emoji");
        assert_eq!(clusters("🇫🇷🇯🇵🇺"), ["🇫🇷", "🇯🇵", "🇺"], "drapeaux : deux indicateurs chacun");
        assert_eq!(clusters("می\u{200C}خ"), ["م", "ی\u{200C}", "خ"], "demi-espace attaché à la lettre qui précède");
        assert_eq!(clusters("كَتَبَ"), ["كَ", "تَ", "بَ"], "voyelles arabes");
        assert_eq!(clusters("ก\u{0E34}"), ["ก\u{0E34}"], "thaï");
        assert_eq!(clusters(""), Vec::<&str>::new());
    }

    #[test]
    fn initials_come_from_whole_characters_not_bytes() {
        for (name, want) in [
            ("Lou Marin", "LM"),
            ("lou", "LO"),
            ("  ", "?"),
            ("", "?"),
            ("Zoë", "ZO"),
            ("élodie dupont", "ÉD"),
            ("山田 太郎", "山太"),
            ("山田太郎", "山田"),
            ("علی رضایی", "عر"),
            ("می\u{200C}خواهم کتاب", "مک"),
            ("👨\u{200D}👩\u{200D}👧 Famille", "👨\u{200D}👩\u{200D}👧F"),
            ("🇫🇷 France", "🇫🇷F"),
            ("e\u{0301}lodie", "E\u{0301}L"),
            ("ß", "SS"),
        ] {
            assert_eq!(initials_of(name), want, "{name:?}");
        }
        // Le découpage vient de l'appelant : ici, le premier char, comme le faisait l'ancien code.
        let naive = |w: &str, n: usize| w.chars().take(n).map(String::from).collect::<Vec<_>>();
        assert_eq!(initials("Ada Lovelace", naive), "AL");
        // Un nom isolé pour le sens d'écriture (FSI … PDI) donne les mêmes initiales que le nom seul.
        assert_eq!(initials("\u{2068}Ada\u{2069} Lovelace", naive), "AL");
        assert_eq!(initials("سرور \u{2068}رضا\u{2069}", naive), initials("سرور رضا", naive));
    }

    #[test]
    fn invisible_code_points_are_hidden() {
        // De quoi ne rien laisser passer : chaque famille, ses deux bords, et des points isolés.
        let hidden: Vec<u32> = [
            0x00AD, 0x034F, 0x061C, 0x115F, 0x1160, 0x17B4, 0x17B5, 0x180B, 0x180C, 0x180D, 0x180E, 0x180F, 0x200B,
            0x200C, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2060, 0x2061, 0x2062, 0x2063, 0x2064,
            0x2065, 0x2066, 0x2067, 0x2068, 0x2069, 0x206A, 0x206F, 0x2800, 0x3164, 0xFE00, 0xFE01, 0xFE0E, 0xFEFF,
            0xFFA0, 0xFFF0, 0xFFF8, 0x1BCA0, 0x1BCA3, 0x1D173, 0x1D17A, 0xE0000, 0xE0001, 0xE0020, 0xE007F, 0xE0100,
            0xE01EF, 0xE0FFF,
        ]
        .to_vec();
        for n in hidden {
            let c = char::from_u32(n).unwrap();
            assert!(is_hidden(c), "U+{n:04X} aurait dû être refusé");
            assert!(!has_visible(&c.to_string()), "U+{n:04X} ne montre rien");
        }
        // Chaque plage entière, bords compris, à l'exception des deux caractères d'emoji.
        for &(lo, hi) in DEFAULT_IGNORABLE {
            for n in lo..=hi {
                let c = char::from_u32(n).unwrap();
                assert_eq!(is_hidden(c), c != ZWJ && c != VS16, "U+{n:04X}");
            }
        }
    }

    #[test]
    fn emoji_joiners_and_ordinary_text_pass() {
        assert!(!is_hidden(ZWJ) && !is_hidden(VS16));
        for text in [
            "👩\u{200D}👩\u{200D}👧",
            "🏳\u{FE0F}\u{200D}🌈",
            "❤\u{FE0F}",
            "Zoë",
            "太郎",
            "Lou — la plongeuse",
            "a\u{00A0}b",
        ] {
            assert!(!text.chars().any(is_hidden), "{text:?}");
            assert!(has_visible(text), "{text:?}");
        }
        // Ce qui borde les plages n'est pas touché.
        for n in [
            0x00AC, 0x00AE, 0x034E, 0x0350, 0x061B, 0x061D, 0x115E, 0x1161, 0x2029, 0x2070, 0x27FF, 0x2801, 0x3163,
            0x3165, 0xFDFF, 0xFE10, 0xFEFE, 0xFF00, 0xFFA1, 0xFFEF, 0xFFF9, 0xE1000,
        ] {
            let c = char::from_u32(n).unwrap();
            assert!(!is_hidden(c), "U+{n:04X} n'est pas invisible");
        }
    }

    #[test]
    fn a_text_needs_something_to_show() {
        for text in [
            "",
            " ",
            "\t \n",
            "\u{200D}",
            "\u{FE0F}",
            "\u{FE0F}\u{200D}\u{FE0F}",
            " \u{200D} ",
            "\u{3000}\u{200D}",
            "\u{2800}",
        ] {
            assert!(!has_visible(text), "{text:?}");
        }
        for text in ["a", " a ", "\u{200D}a", "🙂", "\u{FE0F}🙂"] {
            assert!(has_visible(text), "{text:?}");
        }
    }
}
