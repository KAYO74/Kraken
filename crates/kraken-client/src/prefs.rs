//! Réglages d'affichage de cet appareil : thème, taille du texte, zoom, densité, mouvement,
//! contraste, liens soulignés, format de l'heure (Réglages › Apparence, Accessibilité, Langue et
//! heure). Ils restent dans le `localStorage` de l'appareil (jamais envoyés au serveur), s'appliquent
//! tout de suite sans bouton « Enregistrer », et fonctionnent sans stockage : le choix tient alors
//! jusqu'à la fermeture de l'onglet.
//!
//! Les valeurs sont posées sur `<html>` (`data-theme`, `data-text`, `data-zoom`, `data-density`,
//! `data-motion`, `data-contrast`, `data-underline-links`) et les feuilles de style font le reste
//! (`replica/design/tokens.css`). `boot.js` pose les mêmes attributs avant le premier rendu ; les deux
//! suivent les mêmes clés et les mêmes valeurs.
//!
//! Le zoom est plafonné à la largeur de la fenêtre : les points de rupture des feuilles de style (639 et
//! 1023 px) lisent la largeur réelle de la fenêtre, pas celle de la page zoomée, donc un zoom qui
//! réduirait la page sous 1024 px (ou 640 px) de large ferait garder la mise en page large dans une
//! colonne écrasée. Le zoom réellement appliqué (`--zoom` en ligne sur `<html>` quand il diffère du
//! choix) garde donc la largeur de la page au moins égale à celle du point de rupture où l'on se trouve,
//! et vaut 1 sous 640 px. Il est recalculé quand la fenêtre change de taille (un seul écouteur).

use std::cell::RefCell;
use std::collections::HashMap;

use leptos::prelude::{ArcRwSignal, Get, GetUntracked, Set};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

/// Largeur de la fenêtre (px) sous laquelle la mise en page est celle du téléphone, et où le zoom est coupé.
const PHONE_PX: f64 = 640.0;
/// Largeur de la fenêtre (px) à partir de laquelle la liste des membres s'affiche.
const DESKTOP_PX: f64 = 1024.0;

/// Un réglage : sa clé de stockage, ses valeurs permises et sa valeur par défaut.
pub struct Pref {
    key: &'static str,
    allowed: &'static [&'static str],
    default: &'static str,
}

pub const THEME: Pref = Pref { key: "kraken.theme", allowed: &["auto", "dark", "light", "black"], default: "dark" };
pub const TEXT: Pref = Pref { key: "kraken.text", allowed: &["14", "16", "18", "20"], default: "16" };
pub const ZOOM: Pref = Pref { key: "kraken.zoom", allowed: &["90", "100", "110", "125", "150"], default: "100" };
pub const DENSITY: Pref = Pref { key: "kraken.density", allowed: &["comfortable", "compact"], default: "comfortable" };
pub const MOTION: Pref = Pref { key: "kraken.motion", allowed: &["auto", "reduce", "normal"], default: "auto" };
pub const CONTRAST: Pref = Pref { key: "kraken.contrast", allowed: &["normal", "high", "auto"], default: "normal" };
pub const UNDERLINE: Pref = Pref { key: "kraken.underline", allowed: &["off", "on"], default: "off" };
pub const CLOCK: Pref = Pref { key: "kraken.clock", allowed: &["auto", "12", "24"], default: "auto" };

const ALL: [&Pref; 8] = [&THEME, &TEXT, &ZOOM, &DENSITY, &MOTION, &CONTRAST, &UNDERLINE, &CLOCK];

thread_local! {
    /// La valeur courante de chaque réglage, lue une fois dans le stockage.
    static VALUES: RefCell<HashMap<&'static str, &'static str>> = RefCell::new(HashMap::new());
    /// La fenêtre est trop étroite pour zoomer (sous 640 px) : le choix du zoom est alors gelé.
    static ZOOM_LOCKED: ArcRwSignal<bool> = ArcRwSignal::new(window_width() < PHONE_PX);
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

impl Pref {
    /// La valeur permise qui s'écrit `value`, s'il y en a une.
    fn find(&self, value: &str) -> Option<&'static str> {
        self.allowed.iter().copied().find(|a| *a == value)
    }

    fn load(&self) -> &'static str {
        storage().and_then(|s| s.get_item(self.key).ok().flatten()).and_then(|v| self.find(&v)).unwrap_or(self.default)
    }
}

/// La valeur courante d'un réglage.
pub fn get(pref: &Pref) -> &'static str {
    if let Some(v) = VALUES.with_borrow(|m| m.get(pref.key).copied()) {
        return v;
    }
    let v = pref.load();
    VALUES.with_borrow_mut(|m| m.insert(pref.key, v));
    v
}

/// Change un réglage : il s'applique tout de suite et est gardé sur l'appareil si le stockage le permet.
pub fn set(pref: &Pref, value: &str) {
    let Some(value) = pref.find(value) else { return };
    VALUES.with_borrow_mut(|m| m.insert(pref.key, value));
    if let Some(s) = storage() {
        let _ = s.set_item(pref.key, value);
    }
    apply();
}

fn media(query: &str) -> bool {
    web_sys::window().and_then(|w| w.match_media(query).ok().flatten()).is_some_and(|m| m.matches())
}

/// Le mouvement est réduit : choisi ainsi, ou laissé au système (`prefers-reduced-motion`).
pub fn reduced_motion() -> bool {
    match get(&MOTION) {
        "reduce" => true,
        "auto" => media("(prefers-reduced-motion: reduce)"),
        _ => false,
    }
}

/// Le contraste est élevé : choisi ainsi, ou laissé au système (`prefers-contrast: more`).
pub fn high_contrast() -> bool {
    match get(&CONTRAST) {
        "high" => true,
        "auto" => media("(prefers-contrast: more)"),
        _ => false,
    }
}

/// Le zoom ne peut pas être changé : la fenêtre est trop étroite (se suit dans une vue).
pub fn zoom_locked() -> bool {
    ZOOM_LOCKED.with(|locked| locked.get())
}

/// La largeur de la fenêtre, en pixels (sans limite si on ne peut pas la lire).
fn window_width() -> f64 {
    web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64()).unwrap_or(f64::INFINITY)
}

/// Le zoom à appliquer : le choix, plafonné pour que la page garde au moins la largeur du point de rupture
/// où se trouve la fenêtre (1024 px, puis 640 px), et 1 sous 640 px. Même calcul que `boot.js`.
fn effective_zoom(choice: f64, width: f64) -> f64 {
    if width < PHONE_PX {
        return 1.0;
    }
    let layout = if width >= DESKTOP_PX { DESKTOP_PX } else { PHONE_PX };
    choice.min(width / layout)
}

/// Pose le zoom réellement appliqué sur `<html>` (`--zoom` en ligne, seulement s'il diffère du choix que
/// les feuilles de style connaissent déjà par `data-zoom`) et met à jour l'état « zoom gelé ».
fn apply_zoom() {
    let width = window_width();
    let choice = get(&ZOOM).parse::<f64>().map_or(1.0, |percent| percent / 100.0);
    let zoom = effective_zoom(choice, width);
    let root = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(root) = root {
        let style = root.style();
        let _ = if (zoom - choice).abs() < 1e-6 {
            style.remove_property("--zoom").map(|_| ())
        } else {
            style.set_property("--zoom", &format!("{}", (zoom * 1000.0).round() / 1000.0))
        };
    }
    let locked = width < PHONE_PX;
    ZOOM_LOCKED.with(|signal| {
        if signal.get_untracked() != locked {
            signal.set(locked);
        }
    });
}

fn attribute(name: &str, value: Option<&str>) {
    let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) else { return };
    let _ = match value {
        Some(v) => root.set_attribute(name, v),
        None => root.remove_attribute(name),
    };
}

/// Pose tous les réglages sur `<html>`.
fn apply() {
    attribute("data-theme", Some(get(&THEME)));
    let text = get(&TEXT);
    attribute("data-text", (text != TEXT.default).then_some(text));
    let zoom = get(&ZOOM);
    attribute("data-zoom", (zoom != ZOOM.default).then_some(zoom));
    apply_zoom();
    attribute("data-density", (get(&DENSITY) == "compact").then_some("compact"));
    attribute("data-motion", Some(if reduced_motion() { "reduce" } else { "normal" }));
    attribute("data-contrast", high_contrast().then_some("high"));
    attribute("data-underline-links", (get(&UNDERLINE) == "on").then_some(""));
}

/// Au démarrage : lit les réglages, les pose, suit les changements du système (mouvement, contraste)
/// pour les réglages laissés sur « Automatique », et la taille de la fenêtre pour le zoom.
pub fn init() {
    for pref in ALL {
        get(pref);
    }
    apply();
    for query in ["(prefers-reduced-motion: reduce)", "(prefers-contrast: more)"] {
        let Some(list) = web_sys::window().and_then(|w| w.match_media(query).ok().flatten()) else { continue };
        let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(|_| apply());
        let _ = list.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
        on_change.forget();
    }
    if let Some(window) = web_sys::window() {
        let on_resize = Closure::<dyn FnMut()>::new(apply_zoom);
        let _ = window.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
        on_resize.forget();
    }
}
