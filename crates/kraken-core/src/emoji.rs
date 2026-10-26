//! Emojis Unicode : la sélection proposée par le client (dessinés par la police du système,
//! aucune image) et la règle de ce qu'une réaction peut contenir.

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
}
