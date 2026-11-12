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

/// Longueur, en caractères, du plus long jeton `<:nom:id>` (nom de 32 caractères).
pub const TOKEN_MAX_LEN: usize = 2 + 32 + 1 + 36 + 1;

/// Les `max` premiers caractères de `full`, pour un aperçu : un jeton `<:nom:id>` que la coupe tranche
/// en deux est retiré en entier (avec l'espace qui le précédait), pour que l'aperçu ne montre jamais un
/// bout de jeton (`… <:pieuvre`). Un jeton complet reste ; un `<:` qui n'est pas un jeton aussi. `full`
/// doit aller au moins [`TOKEN_MAX_LEN`] caractères au-delà de la coupe (ou jusqu'à la fin du message).
pub fn excerpt(full: &str, max: usize) -> String {
    let Some((end, _)) = full.char_indices().nth(max) else { return full.to_string() };
    // Le `:` juste après la coupe compte : elle peut tomber entre `<` et `:`.
    let probe = if full[end..].starts_with(':') { end + 1 } else { end };
    if let Some(at) = full[..probe].rfind("<:")
        && let Some((_, _, len)) = parse_token(&full[at..])
        && at + len > end
    {
        return full[..at].trim_end().to_string();
    }
    full[..end].to_string()
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

/// Un `:nom:` qui vient après `before` (le texte qui le précède) doit-il rester du texte ? Oui s'il est
/// protégé par une barre inverse (`\:nom:` ; `\\:nom:` ne protège pas, la barre s'échappe elle-même) ou
/// s'il est dans un lien : un mot (suite de caractères non blancs) qui contient `http://` ou `https://`.
fn literal(before: &str) -> bool {
    if before.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1 {
        return true;
    }
    let word = before.rsplit(char::is_whitespace).next().unwrap_or(before);
    word.contains("http://") || word.contains("https://")
}

/// Écrit `:nom:` partout où `find(nom)` donne le jeton d'un emoji (voir [`token`]) ; le code, les jetons
/// déjà écrits, les `:` collés à un mot (« 12:30:45 »), les liens (« https://…/:nom:/… ») et les `:nom:`
/// précédés d'une barre inverse (`\:nom:`, que l'affichage montre sans la barre) sont laissés tels quels.
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
                && !literal(&part[..i])
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
    fn excerpts_never_cut_a_token() {
        let id = Uuid::from_bytes([7; 16]);
        let tok = token("pieuvre", id);
        assert_eq!(tok.chars().count(), 2 + 7 + 1 + 36 + 1);
        assert_eq!(token(&"x".repeat(32), id).chars().count(), TOKEN_MAX_LEN);
        // Court, ou juste à la limite : tel quel.
        assert_eq!(excerpt("salut", 200), "salut");
        assert_eq!(excerpt("abcde", 5), "abcde");
        assert_eq!(excerpt("abcdef", 5), "abcde");
        assert_eq!(excerpt("é😀èà🙂suite", 5), "é😀èà🙂");
        // Un jeton complet reste, qu'il finisse à la limite ou avant.
        let text = format!("{}{tok}", "a".repeat(10));
        assert_eq!(excerpt(&text, text.chars().count()), text);
        assert_eq!(excerpt(&format!("{text} fin"), text.chars().count()), text);
        // Coupé en deux, où que tombe la coupe : retiré avec l'espace qui le précédait.
        let full = format!("{} {tok} la suite", "a".repeat(10));
        let start = 11;
        for cut in start + 1..start + tok.chars().count() {
            assert_eq!(excerpt(&full, cut), "a".repeat(10), "coupe à {cut}");
        }
        assert_eq!(excerpt(&full, start), format!("{} ", "a".repeat(10)), "la coupe juste avant le jeton");
        assert_eq!(excerpt(&full, start + tok.chars().count()), format!("{} {tok}", "a".repeat(10)));
        // Les caractères larges d'avant ne changent rien (les coupes se comptent en caractères).
        let wide = format!("😀é {tok} suite");
        assert_eq!(excerpt(&wide, 3 + 1 + 20), "😀é");
        assert_eq!(excerpt(&wide, 3 + tok.chars().count()), format!("😀é {tok}"));
        // Seul un vrai jeton est retiré : un `<:` ordinaire reste, comme un jeton invalide.
        assert_eq!(excerpt("a <:pieuvre suite", 12), "a <:pieuvre ");
        assert_eq!(excerpt("a <:p:0707 suite", 7), "a <:p:0");
        assert_eq!(excerpt("<:", 1), "<");
        // Deux jetons : seul le second, coupé, disparaît.
        let two = format!("{tok}{tok}!");
        assert_eq!(excerpt(&two, tok.chars().count() + 20), tok);
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
        // Une barre inverse protège le `:nom:` qui la suit (l'affichage montre `:nom:` sans la barre) ;
        // deux barres sont une barre, et le `:nom:` redevient un emoji.
        assert_eq!(expand_shortcodes(r"\:blob: et :blob:", find), format!(r"\:blob: et {tok}"));
        assert_eq!(expand_shortcodes(r"\\:blob:", find), format!(r"\\{tok}"));
        assert_eq!(expand_shortcodes(r"\\\:blob:", find), r"\\\:blob:");
        assert_eq!(expand_shortcodes(r"a\:blob:b \x:blob:", find), r"a\:blob:b \x:blob:");
        // Un lien reste intact, où que le `:nom:` s'y trouve ; le mot d'après n'est plus dans le lien.
        for same in [
            "https://x.org/a/:blob:/b",
            "http://x.org/?e=:blob:",
            "(https://x.org/:blob:)",
            "<https://x.org/:blob:>",
            "voir https://x.org/a:blob: ici",
            "https://x.org/:blob:/:blob:",
        ] {
            assert_eq!(expand_shortcodes(same, find), same, "{same}");
        }
        assert_eq!(expand_shortcodes("https://x.org/ :blob:", find), format!("https://x.org/ {tok}"));
        assert_eq!(expand_shortcodes("https://x.org/\n:blob:", find), format!("https://x.org/\n{tok}"));
        assert_eq!(expand_shortcodes("www.x.org/:blob:", find), format!("www.x.org/{tok}"));
        assert_eq!(expand_shortcodes(":blob: https://x.org/:blob:", find), format!("{tok} https://x.org/:blob:"));
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
