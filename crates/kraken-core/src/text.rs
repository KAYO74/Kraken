//! Texte que les gens saisissent (nom affiché, pronoms, bio) : les caractères qui ne se voient
//! pas. Ils servent à tromper la lecture (un nom qui s'affiche à l'envers, un nom entièrement
//! invisible, deux noms qui semblent identiques) et n'ont rien à faire dans un profil.

/// Le liant d'emoji : sans lui, la famille, le drapeau arc-en-ciel et les autres emojis composés
/// ne s'écrivent pas. Il reste permis.
pub const ZWJ: char = '\u{200D}';
/// Le sélecteur de variante d'emoji (`❤️`) : il reste permis lui aussi.
pub const VS16: char = '\u{FE0F}';

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

#[cfg(test)]
mod tests {
    use super::*;

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
