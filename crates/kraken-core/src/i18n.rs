//! Langues de l'interface : la logique pure (sans navigateur) que le client et ses tests partagent.
//!
//! Le **français est la clé** : le texte français écrit dans le code sert de clé au catalogue de chaque
//! autre langue (`français<TAB>traduction`), et reste le texte affiché quand une traduction manque. Ici :
//! les langues et leur étiquette BCP-47, le choix (« Automatique », « Français », « English »), la règle de
//! pluriel, la lecture d'un catalogue TSV et le remplacement des `{variables}`. `kraken-client/src/i18n.rs`
//! y ajoute le navigateur (stockage, `navigator.languages`, `<html lang>`).

use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// Une langue de l'interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    Fr,
    En,
}

/// Le sens d'écriture. Toutes les langues prises en charge s'écrivent de gauche à droite pour l'instant ;
/// le type existe pour que rien ne suppose qu'il en sera toujours ainsi (`<html dir>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Ltr,
    Rtl,
}

impl Dir {
    /// La valeur de l'attribut `dir`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }
}

/// La forme du pluriel que choisit une langue pour un nombre : un seul gabarit pour « un », l'autre pour le reste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plural {
    One,
    Other,
}

impl Lang {
    /// Les langues prises en charge, dans l'ordre où le sélecteur les propose.
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];
    /// La langue quand rien n'est choisi et que le navigateur n'en annonce aucune de prise en charge : celle
    /// des textes écrits dans le code.
    pub const DEFAULT: Lang = Lang::Fr;

    /// Le code court : `fr`, `en` (stockage, nom de fichier, attribut `lang`).
    pub fn code(self) -> &'static str {
        match self {
            Self::Fr => "fr",
            Self::En => "en",
        }
    }

    /// L'étiquette BCP-47 donnée à `Intl` (dates, segmentation). Sans région : le client y ajoute celle du
    /// navigateur quand elle va avec la langue (`en-GB`), sinon `Intl` choisit la plus courante.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Fr => "fr",
            Self::En => "en",
        }
    }

    /// Le nom de la langue, écrit dans cette langue (le sélecteur ne le traduit jamais).
    pub fn autonym(self) -> &'static str {
        match self {
            Self::Fr => "Français",
            Self::En => "English",
        }
    }

    /// Le sens d'écriture de la langue.
    pub fn dir(self) -> Dir {
        match self {
            Self::Fr | Self::En => Dir::Ltr,
        }
    }

    /// La langue d'un code court exact (`fr`, `en`).
    pub fn from_code(code: &str) -> Option<Lang> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }

    /// La langue d'une étiquette BCP-47 : `fr`, `fr-FR`, `fr_CA`, `en-GB`… (sans tenir compte de la casse ;
    /// seule la langue compte, pas la région). Une autre langue, ou rien qui y ressemble : `None`.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let primary = tag.trim().split(['-', '_']).next()?;
        Self::ALL.into_iter().find(|l| primary.eq_ignore_ascii_case(l.code()))
    }

    /// La forme du pluriel pour `n` : en français, 0 et 1 sont au singulier ; en anglais, seul 1 l'est.
    pub fn plural(self, n: u64) -> Plural {
        let one = match self {
            Self::Fr => n < 2,
            Self::En => n == 1,
        };
        if one { Plural::One } else { Plural::Other }
    }
}

/// Le choix de l'utilisateur : suivre l'appareil, ou une langue précise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangPref {
    Auto,
    Fixed(Lang),
}

impl LangPref {
    /// Ce qui est gardé dans le stockage : `auto`, `fr`, `en`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fixed(l) => l.code(),
        }
    }

    /// Le choix qu'un code gardé désigne ; un code inconnu : `None` (le client retombe sur « Automatique »).
    pub fn from_code(code: &str) -> Option<LangPref> {
        if code == "auto" { Some(Self::Auto) } else { Lang::from_code(code).map(Self::Fixed) }
    }

    /// La langue à afficher : la langue choisie, ou, en « Automatique », la première des langues du
    /// navigateur (par ordre de préférence) que l'interface prend en charge, à défaut le français.
    pub fn resolve(self, preferred: &[&str]) -> Lang {
        match self {
            Self::Fixed(l) => l,
            Self::Auto => preferred.iter().find_map(|t| Lang::from_tag(t)).unwrap_or(Lang::DEFAULT),
        }
    }
}

/// Le catalogue d'une langue : texte français → traduction.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub entries: HashMap<String, String>,
    /// Lignes ignorées parce qu'elles n'ont pas la forme `français<TAB>traduction` (une seule tabulation,
    /// deux textes non vides, échappements connus).
    pub malformed: usize,
    /// Lignes dont le français était déjà dans le catalogue : la première entrée est gardée.
    pub duplicates: usize,
}

impl Catalog {
    /// Lit un fichier TSV de plus et l'ajoute. Une ligne : `français<TAB>traduction`. Lignes vides et lignes
    /// qui commencent par `#` ignorées ; échappements `\n`, `\t`, `\\` et `\#` (pour un texte qui commence par
    /// `#`) ; fin de ligne `\r\n` acceptée. Une ligne mal formée est comptée et ignorée, jamais fatale.
    pub fn add(&mut self, src: &str) {
        for line in src.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cells = line.split('\t');
            let (Some(key), Some(value), None) = (cells.next(), cells.next(), cells.next()) else {
                self.malformed += 1;
                continue;
            };
            let (Some(key), Some(value)) = (unescape(key), unescape(value)) else {
                self.malformed += 1;
                continue;
            };
            if key.is_empty() || value.is_empty() {
                self.malformed += 1;
            } else {
                match self.entries.entry(key) {
                    Entry::Occupied(_) => self.duplicates += 1,
                    Entry::Vacant(slot) => {
                        slot.insert(value);
                    }
                }
            }
        }
    }

    /// La traduction d'un texte français, s'il y en a une.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

/// Lit un catalogue TSV (voir [`Catalog::add`]).
pub fn parse_catalog(src: &str) -> Catalog {
    let mut c = Catalog::default();
    c.add(src);
    c
}

/// Défait les échappements d'une cellule ; `None` si une barre oblique inverse n'en commence aucun de connu.
fn unescape(cell: &str) -> Option<String> {
    let mut out = String::with_capacity(cell.len());
    let mut chars = cell.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next()? {
            'n' => '\n',
            't' => '\t',
            '\\' => '\\',
            '#' => '#',
            _ => return None,
        });
    }
    Some(out)
}

/// Remplace les `{nom}` d'un gabarit par leur valeur, en un seul passage (une valeur qui contient `{x}` n'est
/// pas remplacée à son tour). Un `{nom}` sans valeur, ou une accolade qui n'enferme pas un nom
/// (lettres, chiffres, `_`), reste tel quel.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    fill_with(template, vars, false)
}

/// Isolat de premier fort (U+2068) et sa fin (U+2069) : le texte entre les deux garde son propre sens d'écriture et
/// n'influence pas celui des mots autour.
const FSI: char = '\u{2068}';
const PDI: char = '\u{2069}';

/// [`fill`] pour une interface de droite à gauche : chaque valeur qui contient une lettre (un nom, un pseudo, une
/// adresse…) est isolée entre FSI et PDI, pour que `« {nom} »` ou `{auteur} a renommé…` ne mêle pas le sens d'un nom
/// latin à celui d'une phrase arabe, et inversement. Un `@` ou un `#` collé devant la valeur (`@{pseudo}`) entre dans
/// l'isolat, sinon `@nom` se lirait `nom@`. Une valeur sans lettre (un nombre, une heure) reste telle quelle.
pub fn fill_isolated(template: &str, vars: &[(&str, &str)]) -> String {
    fill_with(template, vars, true)
}

fn fill_with(template: &str, vars: &[(&str, &str)], isolate: bool) -> String {
    if !template.contains('{') {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let named = after
            .find('}')
            .filter(|&end| end > 0 && after[..end].chars().all(|c| c.is_alphanumeric() || c == '_'))
            .and_then(|end| vars.iter().find(|(k, _)| *k == &after[..end]).map(|(_, v)| (end, *v)));
        match named {
            Some((end, value)) => {
                if isolate && value.chars().any(char::is_alphabetic) {
                    let sigil = if out.ends_with(['@', '#']) { out.pop() } else { None };
                    out.push(FSI);
                    out.extend(sigil);
                    out.push_str(value);
                    out.push(PDI);
                } else {
                    out.push_str(value);
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_fill_wraps_names_and_keeps_numbers() {
        let vars = [("nom", "Marie"), ("n", "12"), ("pseudo", "marie_k")];
        // Sans isolat, `fill` ne change rien.
        assert_eq!(fill("Quitter « {nom} » ?", &vars), "Quitter « Marie » ?");
        assert_eq!(fill_isolated("Quitter « {nom} » ?", &vars), "Quitter « \u{2068}Marie\u{2069} » ?");
        // Un nombre n'est pas isolé ; une valeur absente laisse l'accolade.
        assert_eq!(fill_isolated("{n} membres, {inconnu}", &vars), "12 membres, {inconnu}");
        // `@` ou `#` collé devant la valeur entre dans l'isolat.
        assert_eq!(fill_isolated("Ton pseudo est @{pseudo}.", &vars), "Ton pseudo est \u{2068}@marie_k\u{2069}.");
        assert_eq!(fill_isolated("#{nom}", &vars), "\u{2068}#Marie\u{2069}");
        // Pas d'accolade : le gabarit tel quel.
        assert_eq!(fill_isolated("Bonjour", &vars), "Bonjour");
    }

    #[test]
    fn tags_map_to_languages() {
        for (tag, lang) in [
            ("fr", Some(Lang::Fr)),
            ("fr-FR", Some(Lang::Fr)),
            ("FR-ca", Some(Lang::Fr)),
            ("fr_BE", Some(Lang::Fr)),
            ("en", Some(Lang::En)),
            ("en-US", Some(Lang::En)),
            ("en-GB", Some(Lang::En)),
            (" en-AU ", Some(Lang::En)),
            ("de", None),
            ("de-CH", None),
            ("fra", None),
            ("english", None),
            ("f", None),
            ("", None),
            ("-fr", None),
        ] {
            assert_eq!(Lang::from_tag(tag), lang, "{tag:?}");
        }
        assert_eq!(Lang::from_code("fr"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("en"), Some(Lang::En));
        assert_eq!(Lang::from_code("fr-FR"), None);
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
            assert_eq!(Lang::from_tag(l.tag()), Some(l));
            assert_eq!(l.dir(), Dir::Ltr);
        }
        assert_eq!(Dir::Ltr.as_str(), "ltr");
        assert_eq!(Dir::Rtl.as_str(), "rtl");
        assert_eq!((Lang::Fr.autonym(), Lang::En.autonym()), ("Français", "English"));
    }

    #[test]
    fn the_choice_resolves_to_a_supported_language() {
        let auto = LangPref::Auto;
        assert_eq!(auto.resolve(&["en-US", "fr"]), Lang::En);
        assert_eq!(auto.resolve(&["de-DE", "fr-CA", "en"]), Lang::Fr, "la première langue prise en charge");
        assert_eq!(auto.resolve(&["de", "ja"]), Lang::Fr, "repli sur le français");
        assert_eq!(auto.resolve(&[]), Lang::Fr);
        assert_eq!(LangPref::Fixed(Lang::En).resolve(&["fr-FR"]), Lang::En, "le choix passe avant le navigateur");
        assert_eq!(LangPref::Fixed(Lang::Fr).resolve(&["en-US"]), Lang::Fr);
        for p in [LangPref::Auto, LangPref::Fixed(Lang::Fr), LangPref::Fixed(Lang::En)] {
            assert_eq!(LangPref::from_code(p.code()), Some(p));
        }
        assert_eq!(LangPref::from_code("de"), None);
        assert_eq!(LangPref::from_code(""), None);
        assert_eq!(LangPref::from_code("AUTO"), None);
    }

    #[test]
    fn plural_rules_follow_the_language() {
        let one = |l: Lang, n: u64| l.plural(n) == Plural::One;
        // Français : 0 et 1 au singulier.
        assert!(one(Lang::Fr, 0) && one(Lang::Fr, 1));
        assert!(!one(Lang::Fr, 2) && !one(Lang::Fr, 100));
        // Anglais : seul 1 au singulier.
        assert!(!one(Lang::En, 0) && one(Lang::En, 1));
        assert!(!one(Lang::En, 2) && !one(Lang::En, 21));
    }

    #[test]
    fn catalogs_read_escapes_and_skip_comments() {
        let c = parse_catalog(
            "# un commentaire\n\nSalut\tHello\nLigne 1\\nLigne 2\tLine 1\\nLine 2\nA\\tB\tC\\tD\nChemin\\\\x\tPath\\\\x\n\\#salon\t#channel\n",
        );
        assert_eq!((c.malformed, c.duplicates), (0, 0));
        assert_eq!(c.entries.len(), 5);
        assert_eq!(c.get("Salut"), Some("Hello"));
        assert_eq!(c.get("Ligne 1\nLigne 2"), Some("Line 1\nLine 2"));
        assert_eq!(c.get("A\tB"), Some("C\tD"));
        assert_eq!(c.get("Chemin\\x"), Some("Path\\x"));
        assert_eq!(c.get("#salon"), Some("#channel"));
        assert_eq!(c.get("Absent"), None);
    }

    #[test]
    fn catalogs_count_duplicates_and_malformed_lines() {
        let c = parse_catalog(
            "Un\tOne\nUn\tAgain\nDeux\n\tVide\nTrois\t\nQuatre\ta\tb\nCinq\tbad \\q escape\nSix\tfin\\\nSept\tSeven\r\nHuit\tEight\r\n",
        );
        assert_eq!(c.get("Un"), Some("One"), "la première entrée gagne");
        assert_eq!(c.duplicates, 1);
        // Sans tabulation, clé vide, valeur vide, trois cellules, échappement inconnu, barre finale.
        assert_eq!(c.malformed, 6);
        assert_eq!((c.get("Sept"), c.get("Huit")), (Some("Seven"), Some("Eight")), "fin de ligne Windows");
        assert_eq!(c.entries.len(), 3);
        // Plusieurs fichiers : un doublon entre eux est compté de la même façon.
        let mut all = Catalog::default();
        all.add("Salut\tHello\n");
        all.add("Salut\tHi\nMerci\tThanks\n");
        assert_eq!((all.entries.len(), all.duplicates), (2, 1));
        assert_eq!(all.get("Salut"), Some("Hello"));
        assert_eq!(parse_catalog(""), Catalog::default());
    }

    #[test]
    fn templates_fill_their_variables_once() {
        let f = fill;
        assert_eq!(f("{n} membres", &[("n", "3")]), "3 membres");
        assert_eq!(f("{a} et {b}, {a}", &[("a", "x"), ("b", "y")]), "x et y, x");
        assert_eq!(f("Aucune variable", &[("n", "3")]), "Aucune variable");
        assert_eq!(f("{inconnue} reste", &[("n", "3")]), "{inconnue} reste");
        assert_eq!(f("{a}", &[("a", "{b}"), ("b", "non")]), "{b}", "pas de second passage");
        assert_eq!(f("un { seul", &[("n", "3")]), "un { seul");
        assert_eq!(f("vide {} ici {n", &[("n", "3")]), "vide {} ici {n");
        assert_eq!(f("{{n}}", &[("n", "3")]), "{3}");
        assert_eq!(f("Prénom : {nom}.", &[("nom", "Zoë")]), "Prénom : Zoë.");
        assert_eq!(f("{é}", &[("é", "ok")]), "ok");
    }
}
