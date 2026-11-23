//! Modèles de serveur : la structure (catégories, salons textuels et vocaux) qu'on choisit en créant un
//! serveur. Une liste fixe, partagée par le serveur (qui bâtit la structure) et le client (qui la montre
//! avant de créer). Aucun rôle en plus de `@everyone`.
//!
//! Le premier salon textuel du modèle est le salon système du serveur, et celui qui s'ouvre à la création.
//! Les noms de salons textuels suivent la règle des salons de serveur : minuscules, sans espace (mots liés
//! par des tirets) ; ceux des salons vocaux gardent leur casse et leurs espaces.
//!
//! Les noms des catégories et des salons existent en français et en anglais, côte à côte dans la même table
//! ([`Label`]), et en persan quand `.fa("…")` le donne (sinon l'anglais, comme pour le catalogue de l'interface) :
//! une seule source de vérité. Le serveur bâtit la structure dans la langue que le client lui
//! donne à la création ([`Lang`] ; une langue inconnue ou absente donne le français), et le client montre
//! dans l'aperçu exactement ces noms. Le nom et la description d'un modèle ne servent qu'à la carte du client :
//! ils restent du français que le catalogue de l'interface traduit.

use crate::i18n::Lang;

/// Le modèle choisi quand on n'en nomme aucun : la structure d'avant les modèles.
pub const DEFAULT_ID: &str = "vide";

/// Un nom dans chaque langue de l'interface. Le français et l'anglais sont toujours là ; le persan est facultatif :
/// sans lui, le nom anglais (la chaîne de repli de [`Lang::fallback`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    pub fr: &'static str,
    pub en: &'static str,
    pub fa: Option<&'static str>,
}

impl Label {
    /// Le nom dans cette langue.
    pub const fn get(self, lang: Lang) -> &'static str {
        match lang {
            Lang::Fr => self.fr,
            Lang::En => self.en,
            Lang::Fa => match self.fa {
                Some(name) => name,
                None => self.en,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Text,
    Voice,
}

/// Un salon du modèle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub kind: SlotKind,
    pub name: Label,
}

/// Une catégorie du modèle et ses salons, dans l'ordre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: Label,
    pub slots: &'static [Slot],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template {
    /// Identifiant stable, celui de `CreateGuild.template`.
    pub id: &'static str,
    /// Le nom de la carte, en français (le client le traduit avec son catalogue).
    pub name: &'static str,
    /// Une courte ligne pour le choix : à quoi sert ce modèle, en français (traduite de même).
    pub description: &'static str,
    pub groups: &'static [Group],
}

impl Slot {
    /// Le nom persan du salon (sinon l'anglais) : `text("général", "general").fa("عمومی")`. Un salon textuel suit la règle
    /// des autres : sans espace (mots liés par un tiret), les tests le vérifient dans chaque langue.
    pub const fn fa(self, name: &'static str) -> Slot {
        Slot { name: Label { fa: Some(name), ..self.name }, ..self }
    }
}

impl Group {
    /// Le nom persan de la catégorie (sinon l'anglais).
    pub const fn fa(self, name: &'static str) -> Group {
        Group { name: Label { fa: Some(name), ..self.name }, ..self }
    }
}

/// Un salon textuel : son nom en français, puis en anglais.
const fn text(fr: &'static str, en: &'static str) -> Slot {
    Slot { kind: SlotKind::Text, name: Label { fr, en, fa: None } }
}

/// Un salon vocal : son nom en français, puis en anglais.
const fn voice(fr: &'static str, en: &'static str) -> Slot {
    Slot { kind: SlotKind::Voice, name: Label { fr, en, fa: None } }
}

/// Une catégorie : son nom en français, puis en anglais, et ses salons.
const fn group(fr: &'static str, en: &'static str, slots: &'static [Slot]) -> Group {
    Group { name: Label { fr, en, fa: None }, slots }
}

/// Les modèles, dans l'ordre où on les propose ; le premier est celui par défaut.
pub const ALL: [Template; 5] = [
    Template {
        id: DEFAULT_ID,
        name: "Vide",
        description: "Un salon textuel et un vocal, à compléter.",
        groups: &[
            group("Salons textuels", "Text channels", &[text("général", "general")]),
            group("Salons vocaux", "Voice channels", &[voice("Général", "General")]),
        ],
    },
    Template {
        id: "communaute",
        name: "Communauté",
        description: "Annonces, règles et entraide, pour un club ou un projet.",
        groups: &[
            group("Accueil", "Welcome", &[text("annonces", "announcements"), text("règles", "rules")]),
            group("Discussions", "Discussions", &[text("général", "general"), text("aide", "help")]),
            group("Salons vocaux", "Voice channels", &[voice("Discussion libre", "Open chat")]),
        ],
    },
    Template {
        id: "jeux",
        name: "Jeux",
        description: "Trouver des partenaires, parler pendant la partie.",
        groups: &[
            group(
                "Salons textuels",
                "Text channels",
                &[
                    text("général", "general"),
                    text("recherche-de-joueurs", "looking-for-players"),
                    text("astuces", "tips"),
                ],
            ),
            group(
                "Salons vocaux",
                "Voice channels",
                &[voice("Partie 1", "Game 1"), voice("Partie 2", "Game 2"), voice("Pause", "Break")],
            ),
        ],
    },
    Template {
        id: "etudes",
        name: "Études",
        description: "Cours, devoirs et salles de travail vocales.",
        groups: &[
            group(
                "Travail",
                "Study",
                &[
                    text("général", "general"),
                    text("cours", "classes"),
                    text("devoirs", "homework"),
                    text("ressources", "resources"),
                ],
            ),
            group(
                "Salons vocaux",
                "Voice channels",
                &[
                    voice("Salle de travail", "Study room"),
                    voice("Révisions en groupe", "Group study"),
                    voice("Pause", "Break"),
                ],
            ),
        ],
    },
    Template {
        id: "amis",
        name: "Amis",
        description: "Discuter, partager ses photos, se retrouver en vocal.",
        groups: &[
            group(
                "Salons textuels",
                "Text channels",
                &[text("général", "general"), text("photos", "photos"), text("sorties", "outings")],
            ),
            group(
                "Salons vocaux",
                "Voice channels",
                &[voice("Le canapé", "The couch"), voice("Soirée film", "Movie night")],
            ),
        ],
    },
];

/// Le modèle de cet identifiant, s'il existe.
pub fn find(id: &str) -> Option<&'static Template> {
    ALL.iter().find(|t| t.id == id)
}

/// Le modèle par défaut.
pub fn default_template() -> &'static Template {
    &ALL[0]
}

impl Template {
    /// Tous les salons du modèle (sans les catégories), dans l'ordre d'affichage.
    pub fn slots(&self) -> impl Iterator<Item = &'static Slot> + use<> {
        let groups: &'static [Group] = self.groups;
        groups.iter().flat_map(|g| g.slots.iter())
    }

    /// Le premier salon textuel : salon système du serveur, et celui qu'on ouvre en arrivant.
    pub fn first_text(&self) -> Option<&'static Slot> {
        self.slots().find(|s| s.kind == SlotKind::Text)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::text::visible_name;

    /// `(catégorie, [(genre, nom)])` du modèle dans cette langue.
    fn shape(t: &Template, lang: Lang) -> Vec<(&'static str, Vec<(SlotKind, &'static str)>)> {
        t.groups
            .iter()
            .map(|g| (g.name.get(lang), g.slots.iter().map(|s| (s.kind, s.name.get(lang))).collect()))
            .collect()
    }

    #[test]
    fn ids_are_unique_and_lowercase_ascii() {
        let mut seen = HashSet::new();
        for t in &ALL {
            assert!(seen.insert(t.id), "{} en double", t.id);
            assert!(!t.id.is_empty() && t.id.bytes().all(|b| b.is_ascii_lowercase()), "{}", t.id);
            assert_eq!(find(t.id).map(|f| f.id), Some(t.id));
        }
        assert_eq!(find("n-importe-quoi"), None);
        assert_eq!(find(""), None);
        assert_eq!(find("Vide"), None, "l'identifiant ne tient pas compte du nom affiché");
    }

    #[test]
    fn the_default_is_the_first_and_matches_the_historical_structure() {
        assert_eq!(default_template().id, DEFAULT_ID);
        assert_eq!(find(DEFAULT_ID), Some(default_template()));
        let t = default_template();
        assert_eq!(
            shape(t, Lang::Fr),
            [
                ("Salons textuels", vec![(SlotKind::Text, "général")]),
                ("Salons vocaux", vec![(SlotKind::Voice, "Général")])
            ]
        );
        assert_eq!(
            shape(t, Lang::En),
            [
                ("Text channels", vec![(SlotKind::Text, "general")]),
                ("Voice channels", vec![(SlotKind::Voice, "General")])
            ]
        );
    }

    #[test]
    fn every_template_has_a_text_channel_a_description_and_valid_names_in_every_language() {
        for t in &ALL {
            assert!(visible_name(t.name), "{}", t.id);
            assert!(t.description.chars().count() >= 20 && t.description.chars().count() <= 60, "{}", t.id);
            assert!(t.first_text().is_some(), "{} n'a pas de salon textuel", t.id);
            assert!(!t.groups.is_empty());
            for lang in Lang::ALL {
                let mut texts = HashSet::new();
                for g in t.groups {
                    let name = g.name.get(lang);
                    assert!(!g.slots.is_empty(), "catégorie vide dans {}", t.id);
                    assert!(visible_name(name) && name.chars().count() <= 100, "{name}");
                    assert!(!name.chars().any(char::is_control), "{name}");
                    for s in g.slots {
                        let n = s.name.get(lang);
                        assert!(visible_name(n) && n.chars().count() <= 100 && !n.chars().any(char::is_control), "{n}");
                        if s.kind == SlotKind::Text {
                            // Même règle que les salons créés à la main : minuscules, mots liés par des tirets.
                            assert!(!n.chars().any(char::is_whitespace), "{n} : pas d'espace dans un salon textuel");
                            assert_eq!(n, n.to_lowercase(), "{n} : minuscules");
                            assert!(texts.insert(n), "{n} en double dans {} ({lang:?})", t.id);
                        } else {
                            assert_eq!(n, n.split_whitespace().collect::<Vec<_>>().join(" "), "{n} : espaces en trop");
                        }
                    }
                }
                // Une même catégorie n'apparaît pas deux fois, ni un même salon vocal dans la même catégorie.
                let names: HashSet<_> = t.groups.iter().map(|g| g.name.get(lang)).collect();
                assert_eq!(names.len(), t.groups.len(), "{} ({lang:?})", t.id);
                for g in t.groups {
                    let voices: HashSet<_> = g.slots.iter().map(|s| s.name.get(lang)).collect();
                    assert_eq!(voices.len(), g.slots.len(), "{} / {} ({lang:?})", t.id, g.name.get(lang));
                }
            }
        }
    }

    #[test]
    fn english_names_are_plain_ascii_and_really_differ_from_the_french_ones() {
        let mut same = Vec::new();
        for t in &ALL {
            for g in t.groups {
                for l in std::iter::once(g.name).chain(g.slots.iter().map(|s| s.name)) {
                    assert!(l.en.is_ascii(), "{} : de l'anglais sans accent", l.en);
                    assert!(!l.en.is_empty() && !l.fr.is_empty());
                    if l.fr == l.en {
                        same.push(l.en);
                    }
                }
            }
        }
        // Seuls quelques mots sont les mêmes dans les deux langues (« Discussions », « photos »).
        same.sort_unstable();
        same.dedup();
        assert_eq!(same, ["Discussions", "photos"]);
    }

    #[test]
    fn both_languages_have_the_same_structure() {
        for t in &ALL {
            let (fr, en) = (shape(t, Lang::Fr), shape(t, Lang::En));
            assert_eq!(fr.len(), en.len(), "{} : même nombre de catégories", t.id);
            for ((cat_fr, slots_fr), (cat_en, slots_en)) in fr.iter().zip(&en) {
                assert_eq!(slots_fr.len(), slots_en.len(), "{} / {cat_fr} / {cat_en} : même nombre de salons", t.id);
                let kinds = |v: &Vec<(SlotKind, &str)>| v.iter().map(|(k, _)| *k).collect::<Vec<_>>();
                assert_eq!(kinds(slots_fr), kinds(slots_en), "{} / {cat_en} : même type par position", t.id);
            }
            // Le premier salon textuel (le salon système) est à la même place dans les deux langues.
            let first = |lang| t.first_text().map(|s| s.name.get(lang));
            assert!(first(Lang::Fr).is_some() && first(Lang::En).is_some());
        }
    }

    #[test]
    fn the_first_text_channel_opens_the_server() {
        let first = |id: &str, lang| find(id).and_then(Template::first_text).map(|s| s.name.get(lang));
        assert_eq!(first("vide", Lang::Fr), Some("général"));
        assert_eq!(first("vide", Lang::En), Some("general"));
        assert_eq!(first("communaute", Lang::Fr), Some("annonces"));
        assert_eq!(first("communaute", Lang::En), Some("announcements"));
        assert_eq!(first("etudes", Lang::Fr), Some("général"));
        assert_eq!(first("etudes", Lang::En), Some("general"));
        for t in &ALL {
            assert_eq!(t.slots().count(), t.groups.iter().map(|g| g.slots.len()).sum::<usize>());
        }
    }

    #[test]
    fn the_community_template_has_its_four_text_channels() {
        let t = find("communaute").unwrap();
        let texts = |lang| t.slots().filter(|s| s.kind == SlotKind::Text).map(|s| s.name.get(lang)).collect::<Vec<_>>();
        assert_eq!(texts(Lang::Fr), ["annonces", "règles", "général", "aide"]);
        assert_eq!(texts(Lang::En), ["announcements", "rules", "general", "help"]);
    }

    #[test]
    fn a_label_reads_in_each_language() {
        let l = Label { fr: "général", en: "general", fa: None };
        assert_eq!((l.get(Lang::Fr), l.get(Lang::En)), ("général", "general"));
        assert_eq!(l.get(Lang::Fa), "general", "sans nom persan : l'anglais");
        let l = Label { fa: Some("عمومی"), ..l };
        assert_eq!((l.get(Lang::Fr), l.get(Lang::En), l.get(Lang::Fa)), ("général", "general", "عمومی"));
        for lang in Lang::ALL {
            assert_eq!(Lang::from_tag(lang.tag()), Some(lang));
        }
    }

    #[test]
    fn the_persian_builder_sets_only_the_persian_name() {
        const SLOT: Slot = text("général", "general").fa("عمومی");
        assert_eq!(
            (SLOT.kind, SLOT.name.fr, SLOT.name.en, SLOT.name.fa),
            (SlotKind::Text, "général", "general", Some("عمومی"))
        );
        const GROUP: Group = group("Accueil", "Welcome", &[SLOT]).fa("خوش\u{200c}آمد");
        assert_eq!(
            (GROUP.name.get(Lang::Fa), GROUP.name.get(Lang::En), GROUP.slots.len()),
            ("خوش\u{200c}آمد", "Welcome", 1)
        );
        assert_eq!(voice("Pause", "Break").name.get(Lang::Fa), "Break");
    }

    #[test]
    fn persian_names_follow_the_english_ones_until_they_are_given() {
        for t in &ALL {
            for g in t.groups {
                for l in std::iter::once(g.name).chain(g.slots.iter().map(|s| s.name)) {
                    assert_eq!(l.get(Lang::Fa), l.fa.unwrap_or(l.en), "{} / {}", t.id, l.fr);
                }
            }
        }
    }
}
