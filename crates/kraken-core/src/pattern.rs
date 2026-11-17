//! Reconnaître un message d'après son gabarit à `{variables}` : le contraire de [`crate::i18n::fill`].
//!
//! Le serveur écrit en français des messages où une valeur est glissée (« Ce salon a déjà 50 messages épinglés »).
//! Le catalogue en donne le gabarit (« Ce salon a déjà {n} messages épinglés ») : le client retrouve le gabarit d'un
//! message reçu, en tire les valeurs, puis remplit le gabarit de la langue affichée. Mêmes `{variables}` que
//! `fill` : un nom fait de lettres, de chiffres et de `_`.
//!
//! Un gabarit reconnaît un message quand ses morceaux fixes s'y retrouvent dans l'ordre, du premier au dernier :
//! le premier morceau en tête, le dernier en queue, et chaque valeur (jamais vide) tient entre deux morceaux, au
//! premier endroit où le morceau suivant apparaît. Deux variables côte à côte n'ont pas de frontière : un tel
//! gabarit ne reconnaît rien.

use crate::i18n::{Catalog, fill};

/// Le nom de la variable qui porte un libellé français connu du catalogue (« Nom affiché ») : sa valeur est traduite
/// à son tour. Toute autre valeur reste telle quelle (un nombre, un nom de champ technique, un pseudo).
pub const LABEL_VAR: &str = "champ";

/// Un morceau d'un gabarit.
enum Seg<'t> {
    Lit(&'t str),
    Var(&'t str),
}

/// Les morceaux d'un gabarit, selon la règle de `fill` : une accolade qui n'enferme pas un nom reste du texte.
fn segments(template: &str) -> Vec<Seg<'_>> {
    let mut out = Vec::new();
    let (mut lit_start, mut from) = (0, 0);
    while let Some(rel) = template[from..].find('{') {
        let open = from + rel;
        let after = &template[open + 1..];
        let name_end =
            after.find('}').filter(|&end| end > 0 && after[..end].chars().all(|c| c.is_alphanumeric() || c == '_'));
        match name_end {
            Some(end) => {
                if open > lit_start {
                    out.push(Seg::Lit(&template[lit_start..open]));
                }
                out.push(Seg::Var(&after[..end]));
                from = open + 1 + end + 1;
                lit_start = from;
            }
            None => from = open + 1,
        }
    }
    if lit_start < template.len() {
        out.push(Seg::Lit(&template[lit_start..]));
    }
    out
}

/// Le nombre de caractères fixes d'un gabarit (hors `{variables}`) : plus il est grand, plus le gabarit est précis.
/// Pour essayer les gabarits du plus précis au plus général.
pub fn literal_len(template: &str) -> usize {
    segments(template).iter().map(|s| if let Seg::Lit(l) = s { l.chars().count() } else { 0 }).sum()
}

/// Les valeurs que `message` donne aux `{variables}` de `template`, dans l'ordre du gabarit ; `None` si le message
/// n'a pas la forme du gabarit. Une variable répétée donne une valeur à chaque place.
pub fn extract<'t, 'm>(template: &'t str, message: &'m str) -> Option<Vec<(&'t str, &'m str)>> {
    let segs = segments(template);
    let mut found = Vec::new();
    let mut pos = 0;
    for (i, seg) in segs.iter().enumerate() {
        match seg {
            Seg::Lit(lit) => {
                if !message[pos..].starts_with(lit) {
                    return None;
                }
                pos += lit.len();
            }
            Seg::Var(name) => {
                let end = match segs.get(i + 1) {
                    None => message.len(),
                    Some(Seg::Var(_)) => return None,
                    // Le dernier morceau fixe est en queue du message ; les autres, à leur première occurrence.
                    Some(Seg::Lit(next)) if i + 2 == segs.len() => {
                        if !message.ends_with(next) {
                            return None;
                        }
                        message.len() - next.len()
                    }
                    Some(Seg::Lit(next)) => pos + message[pos..].find(next)?,
                };
                if end <= pos {
                    return None;
                }
                found.push((*name, &message[pos..end]));
                pos = end;
            }
        }
    }
    (pos == message.len()).then_some(found)
}

/// Les gabarits d'un catalogue (ses clés qui portent des `{variables}`), du plus précis au plus général : le premier
/// qui reconnaît un message est le bon.
pub fn templates_of(catalog: &Catalog) -> Vec<String> {
    let mut keys: Vec<String> = catalog.entries.keys().filter(|k| k.contains('{')).cloned().collect();
    keys.sort_by(|a, b| literal_len(b).cmp(&literal_len(a)).then_with(|| a.cmp(b)));
    keys
}

/// Un message du serveur où une valeur est glissée, dans la langue du catalogue : le premier gabarit de `templates`
/// qui le reconnaît donne les valeurs, et sa traduction (`lookup`, qui rend `None` pour un texte sans traduction) est
/// remplie avec elles. `None` si aucun gabarit ne le reconnaît, ou si le gabarit n'a pas de traduction.
pub fn translate(templates: &[String], message: &str, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    let (key, vars) = templates.iter().find_map(|key| extract(key, message).map(|vars| (key, vars)))?;
    let text = lookup(key)?;
    let vars: Vec<(&str, String)> = vars
        .into_iter()
        .map(|(name, value)| {
            let shown = if name == LABEL_VAR { lookup(value) } else { None };
            (name, shown.unwrap_or_else(|| value.to_string()))
        })
        .collect();
    let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
    Some(fill(&text, &vars))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::parse_catalog;

    fn get<'a>(vars: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
        vars.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }

    #[test]
    fn a_message_gives_back_its_values() {
        let t = "Ce salon a déjà {n} messages épinglés : désépingles-en un d'abord.";
        let m = "Ce salon a déjà 50 messages épinglés : désépingles-en un d'abord.";
        assert_eq!(extract(t, m), Some(vec![("n", "50")]));
        assert_eq!(extract("Déjà utilisé : {f}.", "Déjà utilisé : email."), Some(vec![("f", "email")]));
        // Début et fin de gabarit : une variable.
        assert_eq!(extract("{n} rôles au plus.", "250 rôles au plus."), Some(vec![("n", "250")]));
        assert_eq!(extract("Bonjour {nom}", "Bonjour Zoë et Lou"), Some(vec![("nom", "Zoë et Lou")]));
        assert_eq!(
            extract("{a} : {b} chars.", "Nom affiché : 32 chars."),
            Some(vec![("a", "Nom affiché"), ("b", "32")])
        );
        // Sans variable : le message doit être le texte lui-même.
        assert_eq!(extract("Introuvable.", "Introuvable."), Some(vec![]));
        assert_eq!(extract("Introuvable.", "Introuvable. "), None);
        assert_eq!(extract("", ""), Some(vec![]));
    }

    #[test]
    fn what_does_not_have_the_shape_is_refused() {
        let t = "Mode lent : encore {n} s avant ton prochain message.";
        for bad in [
            "",
            "Mode lent : encore  s avant ton prochain message.", // valeur vide
            "Mode lent : encore 5 s avant ton prochain message", // point final manquant
            "Mode lent : encore 5 s avant ton prochain message. ",
            "Mode lent: encore 5 s avant ton prochain message.",
            "xMode lent : encore 5 s avant ton prochain message.",
            "Mode lent : encore 5 min avant ton prochain message.",
        ] {
            assert_eq!(extract(t, bad), None, "{bad:?}");
        }
        // Deux variables côte à côte : aucune frontière, rien n'est reconnu.
        assert_eq!(extract("{a}{b}", "xy"), None);
        assert_eq!(extract("{a} {b}", "x y"), Some(vec![("a", "x"), ("b", "y")]));
    }

    #[test]
    fn the_last_fixed_part_is_the_tail_and_the_others_the_first_occurrence() {
        // Le morceau final est cherché en queue : la valeur peut le contenir.
        assert_eq!(extract("Valeur : {f}.", "Valeur : a.b."), Some(vec![("f", "a.b")]));
        // Un morceau du milieu : à sa première occurrence.
        assert_eq!(extract("{a} - {b} !", "x - y - z !"), Some(vec![("a", "x"), ("b", "y - z")]));
        // Des caractères accentués ne coupent rien au mauvais endroit.
        assert_eq!(extract("« {nom} » est vide.", "« Zoë » est vide."), Some(vec![("nom", "Zoë")]));
        assert_eq!(extract("Aucun compte « {x} ». Vérifie.", "Aucun compte « é ». Vérifie."), Some(vec![("x", "é")]));
    }

    #[test]
    fn braces_that_are_not_a_name_stay_text_like_in_fill() {
        assert_eq!(extract("un { seul {n}", "un { seul 3"), Some(vec![("n", "3")]));
        assert_eq!(extract("vide {} ici {n}", "vide {} ici 4"), Some(vec![("n", "4")]));
        assert_eq!(extract("{{n}}", "{3}"), Some(vec![("n", "3")]));
        assert_eq!(extract("{é} !", "ok !"), Some(vec![("é", "ok")]));
    }

    #[test]
    fn filling_the_template_with_what_was_extracted_gives_back_the_message() {
        for (t, m) in [
            (
                "Tu as déjà {n} invitations actives. Révoque-en une.",
                "Tu as déjà 50 invitations actives. Révoque-en une.",
            ),
            ("{champ} : {n} caractères au plus.", "Pronoms : 40 caractères au plus."),
            (
                "Mode lent : encore {n} s avant ton prochain message.",
                "Mode lent : encore 30 s avant ton prochain message.",
            ),
        ] {
            let vars = extract(t, m).unwrap();
            assert_eq!(fill(t, &vars), m);
        }
        let v = extract("{champ} : {n} caractères au plus.", "Pronoms : 40 caractères au plus.").unwrap();
        assert_eq!((get(&v, "champ"), get(&v, "n")), (Some("Pronoms"), Some("40")));
    }

    #[test]
    fn the_literal_length_ranks_templates_by_precision() {
        assert_eq!(
            literal_len("{n} rôles au plus, et l'ordre doit les citer tous."),
            " rôles au plus, et l'ordre doit les citer tous.".chars().count()
        );
        assert_eq!(literal_len("{a}"), 0);
        assert_eq!(literal_len("Introuvable."), 12);
        assert!(
            literal_len("{champ} : {n} caractères au plus, sans retour à la ligne.")
                > literal_len("{champ} : {n} caractères au plus.")
        );
    }

    #[test]
    fn a_message_is_translated_by_the_most_precise_template() {
        let c = parse_catalog(
            "{champ} : {n} caractères au plus.\t{champ}: {n} characters at most.\n\
             {champ} : {n} caractères au plus, sans retour à la ligne.\t{champ}: {n} characters at most, no line breaks.\n\
             Déjà utilisé : {f}.\tAlready in use: {f}.\n\
             Introuvable.\tNot found.\n\
             Pronoms\tPronouns\n",
        );
        let templates = templates_of(&c);
        // Du plus précis au plus général ; les textes sans variable n'en sont pas.
        assert_eq!(templates.len(), 3);
        assert!(templates[0].ends_with("sans retour à la ligne."));
        let lookup = |k: &str| c.get(k).map(str::to_string);
        let tr = |m: &str| translate(&templates, m, lookup);
        assert_eq!(tr("Pronoms : 40 caractères au plus.").as_deref(), Some("Pronouns: 40 characters at most."));
        assert_eq!(
            tr("Pronoms : 40 caractères au plus, sans retour à la ligne.").as_deref(),
            Some("Pronouns: 40 characters at most, no line breaks.")
        );
        // Un libellé inconnu du catalogue reste tel quel ; une valeur autre que `{champ}` n'est jamais traduite.
        assert_eq!(tr("Bio : 190 caractères au plus.").as_deref(), Some("Bio: 190 characters at most."));
        assert_eq!(tr("Déjà utilisé : Pronoms.").as_deref(), Some("Already in use: Pronoms."));
        // Aucun gabarit : `None` (le message reste tel quel chez l'appelant).
        assert_eq!(tr("Autre chose."), None);
        assert_eq!(tr("Introuvable."), None, "un texte sans variable se traduit par la recherche exacte");
        // Un gabarit sans traduction ne traduit rien.
        assert_eq!(translate(&["Rien : {n}.".to_string()], "Rien : 3.", |_| None), None);
    }
}
