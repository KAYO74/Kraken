//! Quelle touche a été pressée, quelle que soit la disposition du clavier, sans I/O : le client donne
//! `KeyboardEvent.key` et `.code`, ce module rend le nom à comparer à celui d'un raccourci.
//!
//! Sur un clavier russe ou grec, Ctrl+K arrive avec `key` = « л » ou « κ » mais `code` = `KeyK` : on lit alors
//! la touche physique. Tant que `key` est de l'ASCII (QWERTY, AZERTY, Dvorak, `?` et `/` qui demandent Maj),
//! c'est `key` qui décide, comme avant.

/// Le nom, en minuscules, d'une touche pressée : `key` s'il est de l'ASCII (ou un nom de touche comme
/// `ArrowUp`), sinon la lettre, le chiffre ou le `/` de la touche physique `code`. `shift` : Maj enfoncé
/// (le `/` physique donne `?`).
pub fn name(key: &str, code: &str, shift: bool) -> String {
    if key.is_ascii() && !matches!(key, "Dead" | "Unidentified") {
        return key.to_lowercase();
    }
    from_code(code, shift).unwrap_or_else(|| key.to_lowercase())
}

/// La touche d'un `KeyboardEvent.code` : `KeyK` → `k`, `Digit7` → `7`, `Slash` → `/` (ou `?` avec Maj).
fn from_code(code: &str, shift: bool) -> Option<String> {
    let single = |rest: &str, ok: fn(&char) -> bool| {
        let mut chars = rest.chars();
        chars.next().filter(|c| ok(c) && chars.next().is_none())
    };
    if let Some(letter) = code.strip_prefix("Key").and_then(|r| single(r, char::is_ascii_alphabetic)) {
        return Some(letter.to_ascii_lowercase().to_string());
    }
    if let Some(digit) = code.strip_prefix("Digit").and_then(|r| single(r, char::is_ascii_digit)) {
        return Some(digit.to_string());
    }
    (code == "Slash").then(|| if shift { "?" } else { "/" }.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_letters_use_the_key() {
        assert_eq!(name("k", "KeyK", false), "k");
        assert_eq!(name("K", "KeyK", true), "k");
        // Dvorak : la touche physique V donne « k », et c'est ce que l'utilisateur lit.
        assert_eq!(name("k", "KeyV", false), "k");
    }

    #[test]
    fn other_alphabets_use_the_physical_key() {
        assert_eq!(name("л", "KeyK", false), "k");
        assert_eq!(name("Л", "KeyK", true), "k");
        assert_eq!(name("κ", "KeyK", false), "k");
        assert_eq!(name("ь", "KeyM", true), "m");
        assert_eq!(name("в", "KeyD", true), "d");
    }

    #[test]
    fn azerty_symbols_keep_the_key() {
        // AZERTY : « / » est sur Maj+; (code Period), « ? » sur Maj+, (code KeyM) : la touche décide.
        assert_eq!(name("/", "Period", true), "/");
        assert_eq!(name("?", "KeyM", true), "?");
        assert_eq!(name("?", "Slash", true), "?");
    }

    #[test]
    fn slash_and_digits_fall_back_to_the_code() {
        assert_eq!(name("ю", "Slash", false), "/");
        assert_eq!(name("ю", "Slash", true), "?");
        assert_eq!(name("é", "Digit2", false), "2");
    }

    #[test]
    fn named_keys_pass_through() {
        assert_eq!(name("ArrowUp", "ArrowUp", false), "arrowup");
        assert_eq!(name("Escape", "Escape", false), "escape");
        assert_eq!(name("Enter", "Enter", true), "enter");
    }

    #[test]
    fn unknown_keys_stay_unmatched() {
        assert_eq!(name("Dead", "KeyK", false), "k");
        assert_eq!(name("Unidentified", "", false), "unidentified");
        assert_eq!(name("Л", "", false), "л");
        assert_eq!(name("ж", "Semicolon", false), "ж");
    }
}
