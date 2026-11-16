//! Modèles de serveur : la structure (catégories, salons textuels et vocaux) qu'on choisit en créant un
//! serveur. Une liste fixe, partagée par le serveur (qui bâtit la structure) et le client (qui la montre
//! avant de créer). Aucun rôle en plus de `@everyone`.
//!
//! Le premier salon textuel du modèle est le salon système du serveur, et celui qui s'ouvre à la création.
//! Les noms de salons textuels suivent la règle des salons de serveur : minuscules, sans espace (mots liés
//! par des tirets) ; ceux des salons vocaux gardent leur casse et leurs espaces.

/// Le modèle choisi quand on n'en nomme aucun : la structure d'avant les modèles.
pub const DEFAULT_ID: &str = "vide";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Text,
    Voice,
}

/// Un salon du modèle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub kind: SlotKind,
    pub name: &'static str,
}

/// Une catégorie du modèle et ses salons, dans l'ordre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: &'static str,
    pub slots: &'static [Slot],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template {
    /// Identifiant stable, celui de `CreateGuild.template`.
    pub id: &'static str,
    pub name: &'static str,
    /// Une courte ligne pour le choix : à quoi sert ce modèle.
    pub description: &'static str,
    pub groups: &'static [Group],
}

const fn text(name: &'static str) -> Slot {
    Slot { kind: SlotKind::Text, name }
}

const fn voice(name: &'static str) -> Slot {
    Slot { kind: SlotKind::Voice, name }
}

const fn group(name: &'static str, slots: &'static [Slot]) -> Group {
    Group { name, slots }
}

/// Les modèles, dans l'ordre où on les propose ; le premier est celui par défaut.
pub const ALL: [Template; 5] = [
    Template {
        id: DEFAULT_ID,
        name: "Vide",
        description: "Un salon textuel et un vocal, à compléter.",
        groups: &[group("Salons textuels", &[text("général")]), group("Salons vocaux", &[voice("Général")])],
    },
    Template {
        id: "communaute",
        name: "Communauté",
        description: "Annonces, règles et entraide, pour un club ou un projet.",
        groups: &[
            group("Accueil", &[text("annonces"), text("règles")]),
            group("Discussions", &[text("général"), text("aide")]),
            group("Salons vocaux", &[voice("Discussion libre")]),
        ],
    },
    Template {
        id: "jeux",
        name: "Jeux",
        description: "Trouver des partenaires, parler pendant la partie.",
        groups: &[
            group("Salons textuels", &[text("général"), text("recherche-de-joueurs"), text("astuces")]),
            group("Salons vocaux", &[voice("Partie 1"), voice("Partie 2"), voice("Pause")]),
        ],
    },
    Template {
        id: "etudes",
        name: "Études",
        description: "Cours, devoirs et salles de travail vocales.",
        groups: &[
            group("Travail", &[text("général"), text("cours"), text("devoirs"), text("ressources")]),
            group("Salons vocaux", &[voice("Salle de travail"), voice("Révisions en groupe"), voice("Pause")]),
        ],
    },
    Template {
        id: "amis",
        name: "Amis",
        description: "Discuter, partager ses photos, se retrouver en vocal.",
        groups: &[
            group("Salons textuels", &[text("général"), text("photos"), text("sorties")]),
            group("Salons vocaux", &[voice("Le canapé"), voice("Soirée film")]),
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
        let got: Vec<_> =
            t.groups.iter().map(|g| (g.name, g.slots.iter().map(|s| (s.kind, s.name)).collect::<Vec<_>>())).collect();
        assert_eq!(
            got,
            [
                ("Salons textuels", vec![(SlotKind::Text, "général")]),
                ("Salons vocaux", vec![(SlotKind::Voice, "Général")])
            ]
        );
    }

    #[test]
    fn every_template_has_a_text_channel_a_description_and_valid_names() {
        for t in &ALL {
            assert!(visible_name(t.name), "{}", t.id);
            assert!(t.description.chars().count() >= 20 && t.description.chars().count() <= 60, "{}", t.id);
            assert!(t.first_text().is_some(), "{} n'a pas de salon textuel", t.id);
            assert!(!t.groups.is_empty());
            let mut texts = HashSet::new();
            for g in t.groups {
                assert!(!g.slots.is_empty(), "catégorie vide dans {}", t.id);
                assert!(visible_name(g.name) && g.name.chars().count() <= 100, "{}", g.name);
                for s in g.slots {
                    let n = s.name;
                    assert!(visible_name(n) && n.chars().count() <= 100 && !n.chars().any(char::is_control), "{n}");
                    if s.kind == SlotKind::Text {
                        // Même règle que les salons créés à la main : minuscules, mots liés par des tirets.
                        assert!(!n.chars().any(char::is_whitespace), "{n} : pas d'espace dans un salon textuel");
                        assert_eq!(n, n.to_lowercase(), "{n} : minuscules");
                        assert!(texts.insert(n), "{n} en double dans {}", t.id);
                    } else {
                        assert_eq!(n, n.split_whitespace().collect::<Vec<_>>().join(" "), "{n} : espaces en trop");
                    }
                }
            }
            // Une même catégorie n'apparaît pas deux fois.
            let names: HashSet<_> = t.groups.iter().map(|g| g.name).collect();
            assert_eq!(names.len(), t.groups.len(), "{}", t.id);
        }
    }

    #[test]
    fn the_first_text_channel_opens_the_server() {
        let first = |id: &str| find(id).and_then(Template::first_text).map(|s| s.name);
        assert_eq!(first("vide"), Some("général"));
        assert_eq!(first("communaute"), Some("annonces"));
        assert_eq!(first("etudes"), Some("général"));
        for t in &ALL {
            assert_eq!(t.slots().count(), t.groups.iter().map(|g| g.slots.len()).sum::<usize>());
        }
    }

    #[test]
    fn the_community_template_has_its_four_text_channels() {
        let t = find("communaute").unwrap();
        let texts: Vec<_> = t.slots().filter(|s| s.kind == SlotKind::Text).map(|s| s.name).collect();
        assert_eq!(texts, ["annonces", "règles", "général", "aide"]);
    }
}
