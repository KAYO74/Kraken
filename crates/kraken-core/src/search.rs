//! Recherche du Quick Switcher (Ctrl+K) : comparer une requête à des noms sans tenir compte de la
//! casse ni des accents, classer les résultats, dire quelles lettres surligner. Sans I/O, sans
//! dépendance : les accents du français (et des langues voisines) sont repliés à la main.
//!
//! Classement, du meilleur au moins bon : le texte entier (`Exact`), son début (`Prefix`), le début d'un
//! mot (`Word`), un passage quelconque (`Contains`), puis les lettres dans l'ordre mais séparées
//! (`Subsequence`, « gnr » trouve « général »). À rang égal, le passage le plus tôt et le texte le plus
//! court passent devant ; à score égal, l'ordre d'origine est gardé.

/// Longueur maximale, en caractères, de la requête : le reste est ignoré.
pub const MAX_QUERY: usize = 64;
/// Longueur maximale, en caractères, du texte comparé : le reste est ignoré (un nom en fait 100 au plus).
pub const MAX_TEXT: usize = 200;

/// Ce que la requête cherche, d'après son premier caractère : `#` les salons, `@` les personnes (et les
/// MP et groupes, par le nom de leurs membres), `*` les serveurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Channels,
    People,
    Servers,
}

/// Sépare l'opérateur du reste : `#gén` donne `(Channels, "gén")`. Sans opérateur, toute la requête est
/// gardée, sans ses espaces de bord.
pub fn scope_of(raw: &str) -> (Scope, &str) {
    let t = raw.trim_start();
    let scope = match t.chars().next() {
        Some('#') => Scope::Channels,
        Some('@') => Scope::People,
        Some('*') => Scope::Servers,
        _ => return (Scope::All, raw.trim()),
    };
    (scope, t[1..].trim())
}

/// Rang d'une correspondance : plus grand, meilleur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Subsequence = 1,
    Contains,
    Word,
    Prefix,
    Exact,
}

/// Une correspondance : le score (plus grand, meilleur) et les passages du texte à surligner, en
/// caractères (début inclus, fin exclue), triés et sans chevauchement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub tier: Tier,
    pub score: u32,
    pub ranges: Vec<(usize, usize)>,
}

/// Un texte replié : ses caractères sans accent ni majuscule, et pour chacun l'indice du caractère
/// d'origine (« œ » donne deux caractères, tous deux rattachés au même).
struct Folded {
    chars: Vec<char>,
    origin: Vec<usize>,
}

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

fn fold(text: &str, max: usize) -> Folded {
    let mut f = Folded { chars: Vec::new(), origin: Vec::new() };
    for (i, c) in text.chars().take(max).enumerate() {
        for lower in c.to_lowercase() {
            let plain = match lower {
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
                'ç' | 'ć' | 'č' => "c",
                'ď' | 'đ' => "d",
                'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
                'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
                'ł' => "l",
                'ñ' | 'ń' | 'ň' => "n",
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
                'ř' => "r",
                'ś' | 'š' | 'ş' => "s",
                'ť' | 'ţ' => "t",
                'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
                'ý' | 'ÿ' => "y",
                'ź' | 'ż' | 'ž' => "z",
                'œ' => "oe",
                'æ' => "ae",
                'ß' => "ss",
                '\u{2019}' | '\u{2018}' => "'",
                '\u{a0}' | '\u{202f}' => " ",
                // Les accents d'un texte décomposé (NFD) ne comptent pas.
                '\u{300}'..='\u{36f}' => "",
                // Persan et arabe : les habitudes de frappe ne doivent pas cacher un salon. Le demi-espace (ZWNJ) se tape
                // souvent comme une espace ; « ك » et « ي » arabes sont les touches d'un clavier arabe, « ک » et « ی »
                // celles du persan ; les chiffres persans et arabo-indiens valent les latins (le modèle de serveur écrit
                // « بازی 1 ») ; voyelles brèves (tashkeel), alef suscrit et tatweel ne comptent pas.
                '\u{200c}' => " ",
                '\u{643}' => "\u{6a9}",
                '\u{64a}' | '\u{649}' => "\u{6cc}",
                '\u{660}'..='\u{669}' => DIGITS[usize::from(lower as u16 - 0x660)],
                '\u{6f0}'..='\u{6f9}' => DIGITS[usize::from(lower as u16 - 0x6f0)],
                '\u{64b}'..='\u{65f}' | '\u{670}' | '\u{640}' => "",
                other => {
                    f.chars.push(other);
                    f.origin.push(i);
                    continue;
                }
            };
            for p in plain.chars() {
                f.chars.push(p);
                f.origin.push(i);
            }
        }
    }
    f
}

/// Un texte replié comme pour la recherche (minuscules, sans accents, habitudes de frappe du persan et de l'arabe :
/// demi-espace, ك et ي, chiffres), pour comparer des débuts de mots hors du commutateur (complétion de `@` et de `#`).
pub fn fold_key(text: &str) -> String {
    fold(text, usize::MAX).chars.into_iter().collect()
}

/// La requête repliée, espaces de bord retirés et suites d'espaces réduites à une.
fn query_of(raw: &str) -> Vec<char> {
    // On replie d'abord : un demi-espace persan en bord de requête devient une espace, qu'il faut aussi retirer.
    let folded: String = fold(raw, MAX_QUERY).chars.into_iter().collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect()
}

/// Les passages (indices dans le texte replié) en indices du texte d'origine, fusionnés.
fn ranges_of(folded: &Folded, spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &(from, to) in spans {
        if from >= to || to > folded.origin.len() {
            continue;
        }
        let (a, b) = (folded.origin[from], folded.origin[to - 1] + 1);
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn find_at(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

fn scored(tier: Tier, pos: usize, extra: usize) -> u32 {
    // Jusqu'à 400 points pour un passage proche du début, 200 pour un texte pas plus long que la requête.
    tier as u32 * 1000 + 400 - pos.min(200) as u32 + 200 - extra.min(200) as u32
}

/// Compare `query` à `text`. `None` si elle n'y est même pas en lettres dispersées, ou si elle est vide.
pub fn find(query: &str, text: &str) -> Option<Match> {
    let q = query_of(query);
    if q.is_empty() {
        return None;
    }
    let t = fold(text, MAX_TEXT);
    let extra = t.chars.len().saturating_sub(q.len());
    let hit = |tier: Tier, pos: usize| {
        Some(Match { tier, score: scored(tier, pos, extra), ranges: ranges_of(&t, &[(pos, pos + q.len())]) })
    };
    if t.chars == q {
        return hit(Tier::Exact, 0);
    }
    if t.chars.starts_with(&q) {
        return hit(Tier::Prefix, 0);
    }
    let mut from = 1;
    while let Some(i) = find_at(&t.chars, &q, from) {
        if !t.chars[i - 1].is_alphanumeric() {
            return hit(Tier::Word, i);
        }
        from = i + 1;
    }
    if let Some(i) = find_at(&t.chars, &q, 0) {
        return hit(Tier::Contains, i);
    }
    // Les lettres dispersées seulement à partir de trois : en dessous, elles surlignent n'importe quoi.
    if q.len() < 3 {
        return None;
    }
    // Les lettres dans l'ordre, au plus tôt : chacune est son propre passage, ceux qui se touchent fusionnent.
    let mut spans = Vec::with_capacity(q.len());
    let mut at = 0;
    for &c in &q {
        let i = at + t.chars[at..].iter().position(|&x| x == c)?;
        spans.push((i, i + 1));
        at = i + 1;
    }
    let spread = spans.last().map_or(0, |s| s.1) - spans[0].0 - q.len();
    Some(Match {
        tier: Tier::Subsequence,
        score: Tier::Subsequence as u32 * 1000 + 500 - spread.min(400) as u32,
        ranges: ranges_of(&t, &spans),
    })
}

/// Un résultat classé : l'élément, son score, le texte qui l'a fait trouver (indice dans ses textes ; 0
/// est son nom) et les passages à surligner dans ce texte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked<T> {
    pub item: T,
    pub score: u32,
    pub via: usize,
    pub ranges: Vec<(usize, usize)>,
}

/// Classe `items`, chacun avec ses textes (le nom d'abord, puis ce qui le décrit : serveur, pseudo,
/// membres). Un texte autre que le nom compte pour moitié : trouver par le nom passe devant. Les éléments
/// sans correspondance sont écartés ; on garde `limit` résultats au plus, les meilleurs d'abord, à score
/// égal dans l'ordre d'origine.
pub fn rank<T>(query: &str, items: impl IntoIterator<Item = (T, Vec<String>)>, limit: usize) -> Vec<Ranked<T>> {
    let mut out: Vec<Ranked<T>> = items
        .into_iter()
        .filter_map(|(item, texts)| {
            texts
                .iter()
                .enumerate()
                .filter_map(|(via, t)| {
                    find(query, t).map(|m| (via, Match { score: if via == 0 { m.score } else { m.score / 2 }, ..m }))
                })
                .reduce(|best, next| if next.1.score > best.1.score { next } else { best })
                .map(|(via, m)| Ranked { item, score: m.score, via, ranges: m.ranges })
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.score));
    out.truncate(limit);
    out
}

/// Le texte en morceaux, chacun avec « surligné ou non » : de quoi écrire `<mark>` autour des passages.
pub fn segments(text: &str, ranges: &[(usize, usize)]) -> Vec<(String, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    for &(from, to) in ranges {
        let (from, to) = (from.min(chars.len()), to.min(chars.len()));
        if from < at || from >= to {
            continue;
        }
        if from > at {
            out.push((chars[at..from].iter().collect(), false));
        }
        out.push((chars[from..to].iter().collect(), true));
        at = to;
    }
    if at < chars.len() {
        out.push((chars[at..].iter().collect(), false));
    }
    out
}

/// Nombre d'endroits gardés en mémoire pour « Récents ».
pub const MAX_RECENTS: usize = 20;

/// Met `id` en tête de `list` (sans doublon), la liste gardant `MAX_RECENTS` éléments au plus.
pub fn remember(list: &mut Vec<String>, id: &str) {
    list.retain(|x| x != id);
    list.insert(0, id.to_string());
    list.truncate(MAX_RECENTS);
}

/// La liste telle qu'elle est écrite dans le stockage : des identifiants séparés par des virgules.
/// Tout ce qui n'a pas la forme d'un identifiant (lettres, chiffres, tirets) est ignoré.
pub fn parse_recents(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in raw.split(',').map(str::trim) {
        let valid = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if valid && !out.iter().any(|x| x == id) && out.len() < MAX_RECENTS {
            out.push(id.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(q: &str, t: &str) -> Option<Tier> {
        find(q, t).map(|m| m.tier)
    }

    #[test]
    fn fold_key_gives_the_same_key_as_the_search() {
        assert_eq!(fold_key("علی رضایی"), fold_key("علي رضايي"));
        assert!(fold_key("رضا‌ها").starts_with(&fold_key("رضا ")));
        assert_eq!(fold_key("Ève ۱"), "eve 1");
        assert_eq!(fold_key(""), "");
    }

    #[test]
    fn persian_typing_habits_do_not_hide_a_match() {
        // Le demi-espace (ZWNJ) du nom, une espace ou rien : tous trouvent « هم‌تیمی ».
        for q in ["هم‌تیمی", "هم تیمی", "هم\u{200c}", "نکته ها"] {
            assert!(find(q, "یافتن-هم‌تیمی").is_some() || find(q, "نکته‌ها").is_some(), "{q:?}");
        }
        assert_eq!(tier("هم تیمی", "هم‌تیمی"), Some(Tier::Exact));
        assert_eq!(tier("نکته ها", "نکته‌ها"), Some(Tier::Exact));
        assert_eq!(tier("هم‌تیمی", "هم تیمی"), Some(Tier::Exact));
        // Une requête qui finit par un demi-espace garde le préfixe.
        assert_eq!(tier("نکته\u{200c}", "نکته‌ها"), Some(Tier::Prefix));
        // Chiffres persans (clavier fa) et arabo-indiens contre les latins que le modèle écrit.
        assert_eq!(tier("بازی ۱", "بازی 1"), Some(Tier::Exact));
        assert_eq!(tier("بازی ١", "بازی 1"), Some(Tier::Exact));
        assert_eq!(tier("بازی 2", "بازی ۲"), Some(Tier::Exact));
        // « ك » et « ي » arabes (clavier arabe) contre « ک » et « ی » persans.
        assert!(find("نكته", "نکته‌ها").is_some());
        assert!(find("يافتن", "یافتن-هم‌تیمی").is_some());
        assert_eq!(tier("نکته", "نكته"), Some(Tier::Exact));
        // Voyelles brèves, alef suscrit et tatweel ne comptent pas.
        assert_eq!(tier("كَتَبَ", "کتب"), Some(Tier::Exact));
        assert_eq!(tier("عـربي", "عربی"), Some(Tier::Exact));
        // Le latin ne bouge pas, ni les chiffres latins entre eux.
        assert_eq!(tier("game 1", "Game 1"), Some(Tier::Exact));
        assert!(find("نکته", "general").is_none());
    }

    #[test]
    fn case_and_accents_do_not_matter() {
        assert_eq!(tier("GENERAL", "général"), Some(Tier::Exact));
        assert_eq!(tier("ecole", "École des Loisirs"), Some(Tier::Prefix));
        assert_eq!(tier("été", "ete"), Some(Tier::Exact));
        assert_eq!(tier("coeur", "Cœur de ville"), Some(Tier::Prefix));
        assert_eq!(tier("francais", "Le français"), Some(Tier::Word));
        assert_eq!(tier("cafe", "Café".replace('é', "e\u{301}").as_str()), Some(Tier::Exact));
        assert_eq!(tier("l'ete", "L\u{2019}été"), Some(Tier::Exact));
    }

    #[test]
    fn tiers_rank_exact_prefix_word_contains_subsequence() {
        let texts = ["xx général yy", "salon général", "général", "généralités", "grand nord de l'ère", "autre"];
        let ranked = rank("général", texts.iter().enumerate().map(|(i, t)| (i, vec![t.to_string()])), 10);
        let order: Vec<usize> = ranked.iter().map(|r| r.item).collect();
        // Exact, préfixe, puis les deux débuts de mot (le plus tôt d'abord, sinon l'ordre d'origine).
        assert_eq!(order, vec![2, 3, 0, 1]);
        assert_eq!(tier("gnr", "général"), Some(Tier::Subsequence));
        assert_eq!(tier("énér", "général"), Some(Tier::Contains));
        assert_eq!(tier("zzz", "général"), None);
        let all = ["grande nuit", "xx gen", "gen", "regen", "gen suite"];
        let ranked = rank("gen", all.iter().map(|t| (*t, vec![t.to_string()])), 10);
        let order: Vec<&str> = ranked.iter().map(|r| r.item).collect();
        assert_eq!(order, vec!["gen", "gen suite", "xx gen", "regen", "grande nuit"]);
    }

    #[test]
    fn ties_keep_the_original_order_and_limit_applies() {
        let items = (0..30).map(|i| (i, vec![format!("salon {i:02}")]));
        let ranked = rank("salon", items, 12);
        assert_eq!(ranked.len(), 12);
        assert_eq!(ranked.iter().map(|r| r.item).collect::<Vec<_>>(), (0..12).collect::<Vec<_>>());
    }

    #[test]
    fn a_name_beats_its_description() {
        let items = vec![
            ("a", vec!["Atelier".to_string(), "Océan".to_string()]),
            ("b", vec!["Océan".to_string(), "x".to_string()]),
        ];
        let ranked = rank("ocean", items, 5);
        assert_eq!(ranked.iter().map(|r| (r.item, r.via)).collect::<Vec<_>>(), vec![("b", 0), ("a", 1)]);
        // Le surlignage porte sur le texte qui a fait trouver.
        assert_eq!(ranked[1].ranges, vec![(0, 5)]);
    }

    #[test]
    fn ranges_point_at_the_original_letters() {
        let m = find("gen", "Salon Général").unwrap();
        assert_eq!(m.tier, Tier::Word);
        assert_eq!(m.ranges, vec![(6, 9)]);
        assert_eq!(
            segments("Salon Général", &m.ranges),
            vec![("Salon ".into(), false), ("Gén".into(), true), ("éral".into(), false)]
        );
        let sub = find("sgl", "Salon Général").unwrap();
        assert_eq!(sub.tier, Tier::Subsequence);
        assert_eq!(sub.ranges, vec![(0, 1), (6, 7), (12, 13)]);
        // « œ » compte pour une lettre du texte d'origine.
        assert_eq!(find("oe", "cœur").unwrap().ranges, vec![(1, 2)]);
        assert_eq!(segments("abc", &[]), vec![("abc".to_string(), false)]);
    }

    #[test]
    fn empty_and_blank_queries_match_nothing() {
        assert_eq!(find("", "général"), None);
        assert_eq!(find("   ", "général"), None);
        assert!(rank("", vec![(1, vec!["a".to_string()])], 5).is_empty());
        assert_eq!(find("a", ""), None);
        // Deux lettres ne se cherchent pas dispersées : « gé » ne surligne pas un « g » et un « é » éloignés.
        assert_eq!(find("gé", "très-long-pour-tester"), None);
    }

    #[test]
    fn spaces_are_collapsed() {
        assert_eq!(tier("  jean   dupont ", "Jean Dupont"), Some(Tier::Exact));
        assert_eq!(tier("jean dup", "Jean Dupont"), Some(Tier::Prefix));
    }

    #[test]
    fn long_input_is_capped() {
        let long = "é".repeat(5_000);
        // La requête est coupée à MAX_QUERY caractères, le texte à MAX_TEXT : on trouve encore, sans tout parcourir.
        let m = find(&long, &long).unwrap();
        assert_eq!(m.tier, Tier::Prefix);
        assert_eq!(m.ranges, vec![(0, MAX_QUERY)]);
        // Au-delà de MAX_TEXT, rien n'est comparé.
        let text = format!("{}fin", "x".repeat(MAX_TEXT));
        assert_eq!(find("fin", &text), None);
        assert_eq!(segments(&text, &[(0, 10_000)]).len(), 1);
    }

    #[test]
    fn operators_pick_the_scope() {
        assert_eq!(scope_of("#gén"), (Scope::Channels, "gén"));
        assert_eq!(scope_of("  @ alix "), (Scope::People, "alix"));
        assert_eq!(scope_of("*océan"), (Scope::Servers, "océan"));
        assert_eq!(scope_of("  texte "), (Scope::All, "texte"));
        assert_eq!(scope_of("#"), (Scope::Channels, ""));
        assert_eq!(scope_of(""), (Scope::All, ""));
        // Seul le premier caractère compte.
        assert_eq!(scope_of("a#b"), (Scope::All, "a#b"));
        assert_eq!(scope_of("##x"), (Scope::Channels, "#x"));
    }

    #[test]
    fn recents_are_deduplicated_capped_and_validated() {
        let mut list = vec![];
        for i in 0..25 {
            remember(&mut list, &format!("id-{i}"));
        }
        assert_eq!(list.len(), MAX_RECENTS);
        assert_eq!(list[0], "id-24");
        remember(&mut list, "id-10");
        assert_eq!(list[0], "id-10");
        assert_eq!(list.iter().filter(|x| *x == "id-10").count(), 1);
        assert_eq!(parse_recents("a1-b2, c3,,a1-b2,<script>,d4"), vec!["a1-b2", "c3", "d4"]);
        assert_eq!(parse_recents(""), Vec::<String>::new());
        assert_eq!(parse_recents(&"x,".repeat(100)), vec!["x"]);
        assert_eq!(parse_recents(&"y".repeat(65)), Vec::<String>::new());
    }
}
