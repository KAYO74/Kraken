//! Domaine sans I/O : moteur de permissions, markdown des messages, emojis, non-lus et
//! notifications, reconnaissance des images de profil, texte de profil, durées, recherche du Quick Switcher, nom d'une touche pressée selon la disposition,
//! modèles de serveur, langues de l'interface et catalogues de traduction, reconnaissance d'un message d'après son
//! gabarit à `{variables}`.

pub mod emoji;
pub mod grid;
pub mod i18n;
pub mod image;
pub mod keys;
pub mod markdown;
pub mod notify;
pub mod pattern;
pub mod perms;
pub mod search;
pub mod span;
pub mod templates;
pub mod text;
