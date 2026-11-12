//! Emojis Unicode : la sélection proposée par le client (dessinés par la police du système,
//! aucune image) et la règle de ce qu'une réaction peut contenir. Emojis perso d'un serveur : la
//! règle de leur nom et le jeton `<:nom:id>` qui les désigne dans le texte d'un message.

use uuid::Uuid;

/// Réactions différentes au plus sur un même message.
pub const MAX_REACTIONS: usize = 20;

/// La sélection du sélecteur : caractère et nom français pour la recherche.
pub const EMOJIS: &[(&str, &str)] = &[
    ("👍", "pouce levé, d'accord"),
    ("👎", "pouce baissé, pas d'accord"),
    ("❤️", "cœur rouge"),
    ("😂", "pleurer de rire"),
    ("😊", "sourire timide"),
    ("😄", "grand sourire"),
    ("😉", "clin d'œil"),
    ("😍", "yeux en cœur"),
    ("🥰", "visage aux cœurs"),
    ("😎", "lunettes de soleil, cool"),
    ("🤔", "réfléchir, hmm"),
    ("😮", "bouche bée, surpris"),
    ("😢", "larme, triste"),
    ("😭", "sanglots"),
    ("😡", "en colère"),
    ("😴", "dormir, sommeil"),
    ("🤯", "tête qui explose"),
    ("🥳", "fête, cotillons"),
    ("😅", "sourire gêné, sueur"),
    ("🙃", "tête à l'envers"),
    ("😇", "auréole, ange"),
    ("🤗", "câlin"),
    ("🤝", "poignée de main"),
    ("👏", "applaudir, bravo"),
    ("🙌", "mains levées, hourra"),
    ("🙏", "mains jointes, merci"),
    ("👋", "coucou, salut"),
    ("💪", "biceps, force"),
    ("👀", "yeux, regarder"),
    ("🫡", "salut militaire"),
    ("✅", "coche, fait"),
    ("❌", "croix, non"),
    ("❓", "question"),
    ("❗", "exclamation"),
    ("💯", "cent, parfait"),
    ("🔥", "feu, génial"),
    ("✨", "étincelles"),
    ("⭐", "étoile"),
    ("🎉", "confettis, bravo"),
    ("🎂", "gâteau d'anniversaire"),
    ("🎁", "cadeau"),
    ("🏆", "trophée"),
    ("🚀", "fusée, lancement"),
    ("💡", "ampoule, idée"),
    ("📌", "punaise, épingle"),
    ("📎", "trombone"),
    ("🔔", "cloche"),
    ("⏰", "réveil, heure"),
    ("💬", "bulle, discussion"),
    ("💤", "zzz, sommeil"),
    ("💔", "cœur brisé"),
    ("💙", "cœur bleu"),
    ("💚", "cœur vert"),
    ("💛", "cœur jaune"),
    ("💜", "cœur violet"),
    ("🖤", "cœur noir"),
    ("🐙", "pieuvre, kraken"),
    ("🦑", "calmar"),
    ("🐠", "poisson tropical"),
    ("🐟", "poisson"),
    ("🐬", "dauphin"),
    ("🐳", "baleine"),
    ("🦈", "requin"),
    ("🐢", "tortue"),
    ("🦀", "crabe"),
    ("🐚", "coquillage"),
    ("🌊", "vague, mer"),
    ("⚓", "ancre"),
    ("🐱", "chat"),
    ("🐶", "chien"),
    ("🦊", "renard"),
    ("🌱", "pousse, plante"),
    ("🌈", "arc-en-ciel"),
    ("☀️", "soleil"),
    ("🌙", "lune, nuit"),
    ("☕", "café"),
    ("🍕", "pizza"),
    ("🍰", "part de gâteau"),
    ("🍿", "pop-corn"),
    ("🎮", "manette, jeu"),
    ("🎵", "note de musique"),
    ("🎧", "casque audio"),
    ("📷", "appareil photo"),
    ("💻", "ordinateur"),
    ("🐛", "bogue, insecte"),
    ("🔧", "clé à molette, réparer"),
];

/// Une réaction : un emoji Unicode court, pas de texte. Les lettres et chiffres ASCII
/// (sauf ceux des touches `1️⃣`), espaces et caractères de contrôle sont refusés.
pub fn valid(e: &str) -> bool {
    let n = e.chars().count();
    (1..=10).contains(&n)
        && e.len() <= 32
        && !e.is_ascii()
        && e.chars().all(|c| {
            !c.is_control() && !c.is_whitespace() && (!c.is_ascii() || c.is_ascii_digit() || c == '#' || c == '*')
        })
}

/// Longueur du nom d'un emoji perso, en caractères.
pub const NAME_LEN: std::ops::RangeInclusive<usize> = 2..=32;

/// Le nom d'un emoji perso : 2 à 32 caractères parmi les lettres ASCII, les chiffres et `_`.
pub fn valid_name(n: &str) -> bool {
    NAME_LEN.contains(&n.len()) && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Un nom proposé pour un fichier : le nom du fichier sans son extension, les caractères qui ne sont
/// ni lettre ASCII, ni chiffre, ni `_` remplacés par `_` (les `_` du début et de la fin, et les
/// répétitions, retirés), coupé à 32 caractères. Moins de 2 caractères : `emoji`.
pub fn name_from_file(file: &str) -> String {
    let stem = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out: String = out.trim_matches('_').chars().take(32).collect();
    let out = out.trim_end_matches('_').to_string();
    if out.len() >= 2 { out } else { "emoji".into() }
}

/// Le jeton qui désigne l'emoji dans le texte d'un message : `<:nom:id>`. Le nom n'est là que pour
/// le lire et pour l'afficher si l'emoji n'existe plus ; c'est l'id qui compte.
pub fn token(name: &str, id: Uuid) -> String {
    format!("<:{name}:{id}>")
}

/// Lit un jeton `<:nom:id>` au tout début de `s` : son nom, son id et sa longueur en octets.
pub fn parse_token(s: &str) -> Option<(&str, Uuid, usize)> {
    let rest = s.strip_prefix("<:")?;
    let colon = rest.find(':')?;
    let name = &rest[..colon];
    let after = &rest[colon + 1..];
    // 36 caractères avec tirets, puis `>`.
    let id = after.get(..36).filter(|_| after.as_bytes().get(36) == Some(&b'>'))?;
    if !valid_name(name) {
        return None;
    }
    let id = Uuid::parse_str(id).ok()?;
    Some((name, id, 2 + colon + 1 + 36 + 1))
}

/// Une réaction faite avec un emoji perso : l'id, écrit avec tirets et en minuscules (une seule
/// écriture par emoji, pour qu'il n'y ait pas deux réactions « différentes » du même).
pub fn custom_key(e: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(e).ok()?;
    (e.len() == 36 && id.to_string() == e).then_some(id)
}

/// Coupe un texte en morceaux de code (entre des suites d'accents graves de même longueur, ou jusqu'à la
/// fin du texte si elle ne se referme pas, ce qui n'est alors pas du code) et en texte ordinaire.
fn split_code(text: &str) -> Vec<(bool, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut plain_from, mut i) = (0, 0);
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let after = i + run;
        // La suite de même longueur qui la referme.
        let mut j = after;
        let mut close = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let n = bytes[j..].iter().take_while(|b| **b == b'`').count();
                if n == run {
                    close = Some(j + n);
                    break;
                }
                j += n;
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                if plain_from < i {
                    out.push((false, &text[plain_from..i]));
                }
                out.push((true, &text[i..end]));
                plain_from = end;
                i = end;
            }
            None => i = after,
        }
    }
    if plain_from < text.len() {
        out.push((false, &text[plain_from..]));
    }
    out
}

/// Écrit `:nom:` partout où `find(nom)` donne le jeton d'un emoji (voir [`token`]) ; le code, les jetons
/// déjà écrits et les `:` collés à un mot (« 12:30:45 », « http://… ») sont laissés tels quels.
pub fn expand_shortcodes(text: &str, find: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    for (code, part) in split_code(text) {
        if code {
            out.push_str(part);
            continue;
        }
        let b = part.as_bytes();
        let (mut done, mut i) = (0, 0);
        while i < b.len() {
            if b[i] != b':' {
                i += 1;
                continue;
            }
            let before = part[..i].chars().next_back();
            let name_len = b[i + 1..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'_').count();
            let closed = b.get(i + 1 + name_len) == Some(&b':');
            let name = &part[i + 1..i + 1 + name_len];
            // Deux emojis collés (`:a::b:`) : le `:` d'avant est celui qu'on vient de refermer.
            let lone =
                (i > 0 && done == i) || before.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '<' | ':')));
            if lone
                && closed
                && valid_name(name)
                && let Some(t) = find(name)
            {
                out.push_str(&part[done..i]);
                out.push_str(&t);
                i += name_len + 2;
                done = i;
            } else {
                i += 1;
            }
        }
        out.push_str(&part[done..]);
    }
    out
}

/// L'inverse, pour modifier un message : chaque jeton `<:nom:id>` que `keeps(nom, id)` accepte (c'est-à-dire
/// que `:nom:` retrouvera, à l'envoi, le même emoji) redevient `:nom:`. Le code est laissé tel quel.
pub fn collapse_tokens(text: &str, keeps: impl Fn(&str, Uuid) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    for (code, part) in split_code(text) {
        if code {
            out.push_str(part);
            continue;
        }
        let mut rest = part;
        while let Some(at) = rest.find("<:") {
            out.push_str(&rest[..at]);
            let tail = &rest[at..];
            match parse_token(tail) {
                Some((name, id, len)) if keeps(name, id) => {
                    out.push(':');
                    out.push_str(name);
                    out.push(':');
                    rest = &tail[len..];
                }
                _ => {
                    out.push_str("<:");
                    rest = &tail[2..];
                }
            }
        }
        out.push_str(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactions_are_emojis() {
        for (e, _) in EMOJIS {
            assert!(valid(e), "{e}");
        }
        assert!(valid("1️⃣"));
        assert!(valid("👩‍💻"));
        for bad in ["", "a", "ok", "👍 ", "👍\n", "<b>", "👍👍👍👍👍👍👍👍👍👍👍", "1"] {
            assert!(!valid(bad), "{bad:?}");
        }
        let mut seen = std::collections::HashSet::new();
        assert!(EMOJIS.iter().all(|(e, _)| seen.insert(*e)), "pas de doublon");
    }

    #[test]
    fn custom_names_and_tokens() {
        for ok in ["ab", "Blob_Fish", "x".repeat(32).as_str(), "a1_B2"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "a", "é!", "a b", "a-b", "x".repeat(33).as_str(), "éé", "ab:"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert_eq!(name_from_file("Mon Super-Chat (2).PNG"), "Mon_Super_Chat_2");
        assert_eq!(name_from_file("C:\\dessins\\a.gif"), "emoji");
        assert_eq!(name_from_file("é.png"), "emoji");
        assert_eq!(name_from_file("__x__.webp"), "emoji");
        assert_eq!(name_from_file(&format!("{}.png", "a".repeat(50))), "a".repeat(32));
        assert_eq!(name_from_file(&format!("{}_b.png", "a".repeat(31))), "a".repeat(31));
        assert!(valid_name(&name_from_file("image (1).jpeg")));

        let id = Uuid::from_bytes([7; 16]);
        let t = token("blob", id);
        assert_eq!(t, "<:blob:07070707-0707-0707-0707-070707070707>");
        assert_eq!(parse_token(&t), Some(("blob", id, t.len())));
        assert_eq!(parse_token(&format!("{t} suite")), Some(("blob", id, t.len())));
        for bad in [
            "<:b:07070707-0707-0707-0707-070707070707>",
            "<:blob:0707>",
            "<:blob:07070707-0707-0707-0707-07070707070z>",
            "<:blob:07070707-0707-0707-0707-070707070707",
            "<:bl ob:07070707-0707-0707-0707-070707070707>",
            "<blob:07070707-0707-0707-0707-070707070707>",
            "<:blob:07070707070707070707070707070707>",
            "<:blob:é7070707-0707-0707-0707-07070707070>",
        ] {
            assert_eq!(parse_token(bad), None, "{bad}");
        }
        assert_eq!(custom_key("07070707-0707-0707-0707-070707070707"), Some(id));
        for bad in [
            "07070707070707070707070707070707",
            "07070707-0707-0707-0707-07070707070A",
            "👍",
            "",
            "{07070707-0707-0707-0707-070707070707}",
        ] {
            assert_eq!(custom_key(bad), None, "{bad}");
        }
        // Un id n'est jamais un emoji Unicode.
        assert!(!valid("07070707-0707-0707-0707-070707070707"));
    }

    #[test]
    fn shortcodes_become_tokens_and_back() {
        let id = Uuid::from_bytes([7; 16]);
        let tok = token("blob", id);
        let find = |n: &str| (n == "blob").then(|| tok.clone());
        assert_eq!(expand_shortcodes(":blob:", find), tok);
        assert_eq!(expand_shortcodes("a :blob:, :blob::blob: (:blob:)", find), format!("a {tok}, {tok}{tok} ({tok})"));
        for same in [
            ":autre:",
            "12:blob:45",
            "x:blob:",
            "http://blob:blob:",
            ":blob",
            "blob:",
            "::blob:",
            &tok,
            "`:blob:`",
            "``a :blob: ` b``",
            "```\n:blob:\n```",
            ":b:",
            ": blob :",
        ] {
            assert_eq!(expand_shortcodes(same, find), same, "{same}");
        }
        // Un accent grave jamais refermé n'est pas du code.
        assert_eq!(expand_shortcodes("`:blob:", find), format!("`{tok}"));
        assert_eq!(expand_shortcodes("`a` :blob: `b`", find), format!("`a` {tok} `b`"));

        let keeps = |n: &str, i: Uuid| n == "blob" && i == id;
        assert_eq!(collapse_tokens(&format!("{tok} et {tok}"), keeps), ":blob: et :blob:");
        let other = token("blob", Uuid::from_bytes([8; 16]));
        assert_eq!(collapse_tokens(&other, keeps), other, "un autre emoji du même nom reste un jeton");
        assert_eq!(collapse_tokens(&format!("`{tok}`"), keeps), format!("`{tok}`"));
        assert_eq!(collapse_tokens("<:pas un jeton <:", keeps), "<:pas un jeton <:");
        assert_eq!(expand_shortcodes(&collapse_tokens(&format!("a {tok} b"), keeps), find), format!("a {tok} b"));
    }
}
