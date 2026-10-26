//! Contrôle à distance de cet ordinateur, par quelqu'un qui regarde l'écran partagé dans un
//! salon vocal. Le client web relaie ses actions (souris, molette, touches) ; ce module est le
//! seul à les jouer, et le seul juge :
//!
//! - Seule l'instance (celle qu'on a rejointe, ou le serveur hébergé ici) peut appeler ces trois
//!   commandes : une capacité ajoutée au lancement pour sa seule origine ([`allow_origin`]), et
//!   l'adresse de la page revérifiée à chaque appel.
//! - Rien n'est joué sans l'accord donné dans une fenêtre native, hors de la page, à chaque
//!   nouvelle prise de contrôle ([`remote_begin`]). L'accord vaut pour une session (un jeton
//!   aléatoire) et prend fin avec elle : arrêt dans la page, menu « Arrêter le contrôle à
//!   distance » (ou Ctrl+Alt+Maj+K, fenêtre au premier plan), rechargement ou changement de page,
//!   fermeture de l'appli.
//! - Un seul contrôle à la fois, une seule demande affichée à la fois, et un court délai après
//!   un refus (une page ne peut pas enchaîner les fenêtres pour faire cliquer « Autoriser »).
//!
//! Plateformes : X11 sous Linux (Wayland refusé : la saisie simulée n'y atteint pas les autres
//! applications), macOS avec l'autorisation « Accessibilité », Windows (les fenêtres lancées en
//! administrateur ne reçoivent pas la saisie simulée d'une appli qui ne l'est pas).
//!
//! L'écran contrôlé : le navigateur ne dit pas quel écran il partage. Il faut un écran entier
//! (`displaySurface` à « monitor ») ; l'écran choisi est celui dont la taille correspond à
//! l'image partagée, sinon le même rapport largeur/hauteur, sinon l'écran principal.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use enigo::{Axis, Button, Direction, Enigo, Keyboard, Mouse, Settings};
use kraken_proto::{MouseButton, RemoteInput};
use rand::RngCore;
use serde::Serialize;
use tauri::ipc::CapabilityBuilder;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Url, Webview, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// Commandes de ce module (build.rs, capacité de l'instance).
pub const COMMANDS: [&str; 3] = ["remote_begin", "remote_input", "remote_end"];
/// Délai après un refus avant d'afficher une nouvelle demande.
const COOLDOWN: Duration = Duration::from_secs(3);
/// Une réponse « Autoriser » plus rapide que ça n'a pas été lue (Entrée tapée d'avance, clic
/// destiné à la page) : elle compte comme un refus.
const MIN_READ: Duration = Duration::from_millis(800);
/// Actions au plus par appel de `remote_input`.
const MAX_BATCH: usize = 256;
/// Le raccourci du menu « Arrêter le contrôle à distance ».
pub const STOP_SHORTCUT: &str = "CmdOrCtrl+Alt+Shift+K";

/// Pourquoi le contrôle n'a pas commencé ou s'est arrêté : `reason` pour le client web (les
/// valeurs de `ControlRefusal`, plus `ended`), `message` pour la personne.
#[derive(Debug, Serialize)]
pub struct Refusal {
    reason: &'static str,
    message: String,
}

fn refuse(reason: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { reason, message: message.into() }
}

const NOT_ALLOWED: &str = "Cette page ne peut pas contrôler l'ordinateur.";
const ENDED: &str = "Le contrôle à distance a pris fin.";

/// Un écran, dans les coordonnées de la saisie simulée (pixels sous Linux et Windows, points
/// sous macOS).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Screen {
    /// Le pixel visé par une position relative de l'image partagée.
    fn point(&self, x: f32, y: f32) -> (i32, i32) {
        let px = (f64::from(x.clamp(0.0, 1.0)) * f64::from(self.w.saturating_sub(1))).round() as i32;
        let py = (f64::from(y.clamp(0.0, 1.0)) * f64::from(self.h.saturating_sub(1))).round() as i32;
        (self.x + px, self.y + py)
    }
}

/// Un écran de cet ordinateur : taille en pixels, position et taille pour la saisie simulée.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub screen: Screen,
}

/// L'écran partagé, d'après la taille de l'image (`exact` : sans `displaySurface`, la taille
/// doit correspondre exactement, sinon on ne sait pas ce qui est partagé).
pub fn pick_monitor(monitors: &[Monitor], width: u32, height: u32, exact: bool) -> Option<Screen> {
    let primary_first = |a: &&Monitor, b: &&Monitor| b.primary.cmp(&a.primary);
    let mut same: Vec<&Monitor> = monitors.iter().filter(|m| m.width == width && m.height == height).collect();
    same.sort_by(primary_first);
    if let Some(m) = same.first() {
        return Some(m.screen);
    }
    if exact || width == 0 || height == 0 {
        return None;
    }
    // Image réduite par le navigateur : même rapport largeur/hauteur (à 1 % près).
    let ratio = f64::from(width) / f64::from(height);
    let mut close: Vec<&Monitor> = monitors
        .iter()
        .filter(|m| m.height > 0 && ((f64::from(m.width) / f64::from(m.height)) / ratio - 1.0).abs() < 0.01)
        .collect();
    close.sort_by(primary_first);
    close.first().copied().or_else(|| monitors.iter().find(|m| m.primary)).or(monitors.first()).map(|m| m.screen)
}

/// Une touche à jouer : code brut (Linux : keycode X11 ; macOS : code virtuel ; Windows : code
/// de balayage) ou, sous Windows, touche virtuelle (touches « étendues » : flèches, Inser…).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Native {
    Raw(u16),
    #[allow(dead_code)] // Windows seulement
    Virtual(u32),
}

/// Colonne Windows de [`KEYS`].
#[derive(Clone, Copy)]
#[allow(dead_code)] // lue seulement sous Windows
enum Win {
    Scan(u16),
    Vk(u32),
    No,
}

/// Les touches physiques reconnues (`KeyboardEvent.code`) : keycode X11, Windows, code virtuel
/// macOS (`NO` : absente de cette plateforme).
const NO: u16 = u16::MAX;
#[rustfmt::skip]
const KEYS: &[(&str, u16, Win, u16)] = &[
    ("Escape", 9, Win::Scan(0x01), 0x35),
    ("Digit1", 10, Win::Scan(0x02), 0x12), ("Digit2", 11, Win::Scan(0x03), 0x13),
    ("Digit3", 12, Win::Scan(0x04), 0x14), ("Digit4", 13, Win::Scan(0x05), 0x15),
    ("Digit5", 14, Win::Scan(0x06), 0x17), ("Digit6", 15, Win::Scan(0x07), 0x16),
    ("Digit7", 16, Win::Scan(0x08), 0x1A), ("Digit8", 17, Win::Scan(0x09), 0x1C),
    ("Digit9", 18, Win::Scan(0x0A), 0x19), ("Digit0", 19, Win::Scan(0x0B), 0x1D),
    ("Minus", 20, Win::Scan(0x0C), 0x1B), ("Equal", 21, Win::Scan(0x0D), 0x18),
    ("Backspace", 22, Win::Scan(0x0E), 0x33), ("Tab", 23, Win::Scan(0x0F), 0x30),
    ("KeyQ", 24, Win::Scan(0x10), 0x0C), ("KeyW", 25, Win::Scan(0x11), 0x0D),
    ("KeyE", 26, Win::Scan(0x12), 0x0E), ("KeyR", 27, Win::Scan(0x13), 0x0F),
    ("KeyT", 28, Win::Scan(0x14), 0x11), ("KeyY", 29, Win::Scan(0x15), 0x10),
    ("KeyU", 30, Win::Scan(0x16), 0x20), ("KeyI", 31, Win::Scan(0x17), 0x22),
    ("KeyO", 32, Win::Scan(0x18), 0x1F), ("KeyP", 33, Win::Scan(0x19), 0x23),
    ("BracketLeft", 34, Win::Scan(0x1A), 0x21), ("BracketRight", 35, Win::Scan(0x1B), 0x1E),
    ("Enter", 36, Win::Scan(0x1C), 0x24), ("ControlLeft", 37, Win::Scan(0x1D), 0x3B),
    ("KeyA", 38, Win::Scan(0x1E), 0x00), ("KeyS", 39, Win::Scan(0x1F), 0x01),
    ("KeyD", 40, Win::Scan(0x20), 0x02), ("KeyF", 41, Win::Scan(0x21), 0x03),
    ("KeyG", 42, Win::Scan(0x22), 0x05), ("KeyH", 43, Win::Scan(0x23), 0x04),
    ("KeyJ", 44, Win::Scan(0x24), 0x26), ("KeyK", 45, Win::Scan(0x25), 0x28),
    ("KeyL", 46, Win::Scan(0x26), 0x25), ("Semicolon", 47, Win::Scan(0x27), 0x29),
    ("Quote", 48, Win::Scan(0x28), 0x27), ("Backquote", 49, Win::Scan(0x29), 0x32),
    ("ShiftLeft", 50, Win::Scan(0x2A), 0x38), ("Backslash", 51, Win::Scan(0x2B), 0x2A),
    ("KeyZ", 52, Win::Scan(0x2C), 0x06), ("KeyX", 53, Win::Scan(0x2D), 0x07),
    ("KeyC", 54, Win::Scan(0x2E), 0x08), ("KeyV", 55, Win::Scan(0x2F), 0x09),
    ("KeyB", 56, Win::Scan(0x30), 0x0B), ("KeyN", 57, Win::Scan(0x31), 0x2D),
    ("KeyM", 58, Win::Scan(0x32), 0x2E), ("Comma", 59, Win::Scan(0x33), 0x2B),
    ("Period", 60, Win::Scan(0x34), 0x2F), ("Slash", 61, Win::Scan(0x35), 0x2C),
    ("ShiftRight", 62, Win::Scan(0x36), 0x3C), ("NumpadMultiply", 63, Win::Scan(0x37), 0x43),
    ("AltLeft", 64, Win::Scan(0x38), 0x3A), ("Space", 65, Win::Scan(0x39), 0x31),
    ("CapsLock", 66, Win::Scan(0x3A), 0x39),
    ("F1", 67, Win::Scan(0x3B), 0x7A), ("F2", 68, Win::Scan(0x3C), 0x78), ("F3", 69, Win::Scan(0x3D), 0x63),
    ("F4", 70, Win::Scan(0x3E), 0x76), ("F5", 71, Win::Scan(0x3F), 0x60), ("F6", 72, Win::Scan(0x40), 0x61),
    ("F7", 73, Win::Scan(0x41), 0x62), ("F8", 74, Win::Scan(0x42), 0x64), ("F9", 75, Win::Scan(0x43), 0x65),
    ("F10", 76, Win::Scan(0x44), 0x6D), ("F11", 95, Win::Scan(0x57), 0x67), ("F12", 96, Win::Scan(0x58), 0x6F),
    ("NumLock", 77, Win::Vk(0x90), 0x47), ("ScrollLock", 78, Win::Scan(0x46), NO),
    ("Numpad7", 79, Win::Scan(0x47), 0x59), ("Numpad8", 80, Win::Scan(0x48), 0x5B),
    ("Numpad9", 81, Win::Scan(0x49), 0x5C), ("NumpadSubtract", 82, Win::Scan(0x4A), 0x4E),
    ("Numpad4", 83, Win::Scan(0x4B), 0x56), ("Numpad5", 84, Win::Scan(0x4C), 0x57),
    ("Numpad6", 85, Win::Scan(0x4D), 0x58), ("NumpadAdd", 86, Win::Scan(0x4E), 0x45),
    ("Numpad1", 87, Win::Scan(0x4F), 0x53), ("Numpad2", 88, Win::Scan(0x50), 0x54),
    ("Numpad3", 89, Win::Scan(0x51), 0x55), ("Numpad0", 90, Win::Scan(0x52), 0x52),
    ("NumpadDecimal", 91, Win::Scan(0x53), 0x41), ("IntlBackslash", 94, Win::Scan(0x56), 0x0A),
    ("IntlRo", 97, Win::Scan(0x73), 0x5E), ("IntlYen", 132, Win::Scan(0x7D), 0x5D),
    ("NumpadEnter", 104, Win::Vk(0x0D), 0x4C), ("ControlRight", 105, Win::Vk(0xA3), 0x3E),
    ("NumpadDivide", 106, Win::Vk(0x6F), 0x4B), ("PrintScreen", 107, Win::Vk(0x2C), NO),
    ("AltRight", 108, Win::Vk(0xA5), 0x3D), ("Home", 110, Win::Vk(0x24), 0x73),
    ("ArrowUp", 111, Win::Vk(0x26), 0x7E), ("PageUp", 112, Win::Vk(0x21), 0x74),
    ("ArrowLeft", 113, Win::Vk(0x25), 0x7B), ("ArrowRight", 114, Win::Vk(0x27), 0x7C),
    ("End", 115, Win::Vk(0x23), 0x77), ("ArrowDown", 116, Win::Vk(0x28), 0x7D),
    ("PageDown", 117, Win::Vk(0x22), 0x79), ("Insert", 118, Win::Vk(0x2D), 0x72),
    ("Delete", 119, Win::Vk(0x2E), 0x75), ("NumpadEqual", 125, Win::No, 0x51),
    ("Pause", 127, Win::Vk(0x13), NO),
    ("MetaLeft", 133, Win::Vk(0x5B), 0x37), ("MetaRight", 134, Win::Vk(0x5C), 0x36),
    ("OSLeft", 133, Win::Vk(0x5B), 0x37), ("OSRight", 134, Win::Vk(0x5C), 0x36),
    ("ContextMenu", 135, Win::Vk(0x5D), 0x6E),
];

/// La touche de cette plateforme pour un `KeyboardEvent.code` ; `None` : inconnue ici.
pub fn native_key(code: &str) -> Option<Native> {
    let &(_, linux, win, mac) = KEYS.iter().find(|(c, ..)| *c == code)?;
    for_platform(linux, win, mac)
}

#[cfg(target_os = "macos")]
fn for_platform(_: u16, _: Win, mac: u16) -> Option<Native> {
    (mac != NO).then_some(Native::Raw(mac))
}

#[cfg(windows)]
fn for_platform(_: u16, win: Win, _: u16) -> Option<Native> {
    match win {
        Win::Scan(s) => Some(Native::Raw(s)),
        Win::Vk(v) => Some(Native::Virtual(v)),
        Win::No => None,
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn for_platform(linux: u16, _: Win, _: u16) -> Option<Native> {
    (linux != NO).then_some(Native::Raw(linux))
}

/// Pourquoi la saisie simulée est impossible sur cet ordinateur, s'il y a une raison.
pub fn unavailable() -> Option<String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
            || (std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none());
        if wayland {
            return Some(
                "Le contrôle à distance n'est pas disponible sous Wayland. Ouvre une session X11 (Xorg) pour \
                 l'utiliser."
                    .into(),
            );
        }
        if std::env::var_os("DISPLAY").is_none() {
            return Some("Le contrôle à distance demande une session graphique X11.".into());
        }
    }
    #[cfg(target_os = "macos")]
    if !accessibility() {
        return Some(MAC_ACCESSIBILITY.into());
    }
    None
}

#[cfg(target_os = "macos")]
const MAC_ACCESSIBILITY: &str = "Pour être contrôlé à distance, KRAKEN doit en avoir l'autorisation : Réglages Système \
                                 > Confidentialité et sécurité > Accessibilité, active KRAKEN, puis réessaie.";

#[cfg(target_os = "macos")]
fn accessibility() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    // SAFETY : fonction sans argument de l'API Accessibilité.
    unsafe { AXIsProcessTrusted() }
}

/// Un nom affichable dans la fenêtre native : sans caractères de contrôle, 64 au plus.
fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(64)
        .collect();
    let name = name.trim();
    if name.is_empty() { "Quelqu'un".into() } else { name.to_string() }
}

enum Job {
    Input(RemoteInput),
}

struct Session {
    token: String,
    /// L'origine de la page qui a obtenu l'accord.
    origin: String,
    jobs: Sender<Job>,
    /// Mis à vrai à la fin : le fil de saisie jette ce qui reste dans son canal.
    stop: Arc<AtomicBool>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct State {
    /// Une fenêtre native de demande est ouverte.
    asking: bool,
    refused_at: Option<Instant>,
    session: Option<Session>,
    /// Change à chaque chargement de page : un accord donné pendant un changement de page ne
    /// vaut pas pour la nouvelle.
    page: u64,
}

/// L'état du contrôle à distance (géré par l'appli).
#[derive(Default)]
pub struct Remote {
    state: Mutex<State>,
    /// Origines déjà autorisées à appeler ces commandes.
    origins: Mutex<Vec<String>>,
}

impl Remote {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// L'élément de menu « Arrêter le contrôle à distance », actif seulement pendant un contrôle.
pub struct StopItem(pub MenuItem<Wry>);

/// Donne à la page de cette origine (l'instance) le droit d'appeler les trois commandes, et
/// rien d'autre. Sans effet si c'est déjà fait ; ignoré si l'origine ne fait pas un motif valide.
pub fn allow_origin(app: &AppHandle, url: &Url) {
    let origin = url.origin().ascii_serialization();
    if !matches!(url.scheme(), "http" | "https") || origin == "null" {
        return;
    }
    let remote = app.state::<Remote>();
    let mut origins = remote.origins.lock().unwrap_or_else(|e| e.into_inner());
    if origins.contains(&origin) {
        return;
    }
    // Une origine que Tauri ne saurait lire ferait échouer l'ajout : vérifiée avant.
    if origin.parse::<tauri::utils::acl::RemoteUrlPattern>().is_err() {
        return;
    }
    let mut capability = CapabilityBuilder::new(format!("remote-control-{}", origins.len()))
        .remote(origin.clone())
        .local(false)
        .window("main");
    for command in COMMANDS {
        capability = capability.permission(format!("allow-{}", command.replace('_', "-")));
    }
    if app.add_capability(capability).is_ok() {
        origins.push(origin);
    }
}

/// La page qui appelle est bien l'instance, dans la fenêtre principale.
fn check_page(webview: &Webview) -> Result<(), Refusal> {
    let ok =
        webview.label() == "main" && webview.url().is_ok_and(|page| crate::is_instance(webview.app_handle(), &page));
    if ok { Ok(()) } else { Err(refuse("unavailable", NOT_ALLOWED)) }
}

/// La fenêtre principale, sur la page de cette origine.
fn same_page(webview: &Webview, origin: &str) -> bool {
    webview.label() == "main" && webview.url().is_ok_and(|u| u.origin().ascii_serialization() == origin)
}

fn monitors(webview: &Webview) -> Vec<Monitor> {
    let window = webview.window();
    let primary = window.primary_monitor().ok().flatten();
    let same = |a: &tauri::Monitor, b: &tauri::Monitor| a.position() == b.position() && a.size() == b.size();
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (size, pos) = (m.size(), m.position());
            // macOS : la saisie simulée compte en points, pas en pixels.
            let scale = if cfg!(target_os = "macos") { m.scale_factor().max(1.0) } else { 1.0 };
            let screen = Screen {
                x: (f64::from(pos.x) / scale).round() as i32,
                y: (f64::from(pos.y) / scale).round() as i32,
                w: (f64::from(size.width) / scale).round() as u32,
                h: (f64::from(size.height) / scale).round() as u32,
            };
            Monitor {
                width: size.width,
                height: size.height,
                primary: primary.as_ref().is_some_and(|p| same(p, m)),
                screen,
            }
        })
        .collect()
}

/// Quelqu'un (`name`) demande à contrôler l'écran partagé (`width` × `height`, `surface` :
/// `displaySurface` de la piste). Demande l'accord dans une fenêtre native ; rend le jeton de la
/// session si la personne accepte.
#[tauri::command]
pub async fn remote_begin(
    app: AppHandle,
    webview: Webview,
    remote: tauri::State<'_, Remote>,
    name: String,
    width: u32,
    height: u32,
    surface: Option<String>,
) -> Result<String, Refusal> {
    check_page(&webview)?;
    if let Some(why) = unavailable() {
        return Err(refuse("unavailable", why));
    }
    let exact = match surface.as_deref() {
        Some("monitor") => false,
        None => true,
        Some(_) => return Err(refuse("surface", SURFACE)),
    };
    let screen = pick_monitor(&monitors(&webview), width, height, exact).ok_or_else(|| refuse("surface", SURFACE))?;
    let page = {
        let mut st = remote.lock();
        if st.session.is_some() {
            return Err(refuse("busy", "Quelqu'un contrôle déjà cet ordinateur."));
        }
        if st.asking {
            return Err(refuse("busy", "Une demande de contrôle est déjà affichée."));
        }
        if st.refused_at.is_some_and(|t| t.elapsed() < COOLDOWN) {
            return Err(refuse("declined", "Demande refusée."));
        }
        st.asking = true;
        st.page
    };
    let host = webview.url().ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let text = format!(
        "{} veut contrôler ton ordinateur : souris et clavier, depuis le serveur {host}.\n\n\
         N'accepte que si tu lui fais confiance. Tu pourras arrêter à tout moment : bouton « Arrêter le \
         contrôle » dans KRAKEN, ou menu KRAKEN > Arrêter le contrôle à distance (Ctrl+Alt+Maj+K).\n\n\
         Autoriser ?",
        clean_name(&name)
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .message(text)
        .title("KRAKEN : contrôle à distance")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Autoriser".into(), "Refuser".into()));
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.parent(&w);
    }
    let shown = Instant::now();
    dialog.show(move |ok| {
        let _ = tx.send(ok);
    });
    let accepted = rx.await.unwrap_or(false);
    {
        let mut st = remote.lock();
        st.asking = false;
        if !accepted {
            st.refused_at = Some(Instant::now());
            return Err(refuse("declined", "Demande refusée."));
        }
        if shown.elapsed() < MIN_READ {
            st.refused_at = Some(Instant::now());
            return Err(refuse(
                "unavailable",
                "Réponse trop rapide : par prudence, la demande a été ignorée. Redemande-la.",
            ));
        }
        // Pendant la question, la page a pu changer : l'accord ne vaut que pour celle qui l'a demandé.
        if st.page != page || st.session.is_some() {
            return Err(refuse("unavailable", ENDED));
        }
    }
    check_page(&webview)?;
    let origin = webview.url().map(|u| u.origin().ascii_serialization()).unwrap_or_default();
    // Le fil de saisie démarre hors du verrou : `remote_input` ne doit jamais l'attendre.
    let (jobs, rx) = mpsc::channel();
    let (ready_tx, ready) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::Builder::new()
        .name("kraken-controle".into())
        .spawn(move || inject(rx, screen, ready_tx, &stopped))
        .map_err(|e| refuse("unavailable", e.to_string()))?;
    let started = tokio::task::spawn_blocking(move || ready.recv()).await;
    if let Err(why) =
        started.ok().and_then(Result::ok).unwrap_or_else(|| Err("la saisie simulée n'a pas démarré".into()))
    {
        return Err(refuse("unavailable", why));
    }
    let mut st = remote.lock();
    if st.page != page || st.session.is_some() {
        // `stop` et `jobs` détruits ici : le fil s'arrête.
        return Err(refuse("unavailable", ENDED));
    }
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    st.session = Some(Session { token: token.clone(), origin, jobs, stop });
    drop(st);
    let _ = app.state::<StopItem>().0.set_enabled(true);
    Ok(token)
}

const SURFACE: &str =
    "Pour être contrôlé, partage ton écran entier (pas une fenêtre ni un onglet), puis accepte à nouveau.";

/// Les actions de celui qui contrôle, dans l'ordre, si la session `token` est toujours en cours.
#[tauri::command]
pub fn remote_input(
    webview: Webview,
    remote: tauri::State<'_, Remote>,
    token: String,
    events: Vec<RemoteInput>,
) -> Result<(), Refusal> {
    let mut st = remote.lock();
    let Some(session) = st.session.as_ref().filter(|s| s.token == token) else {
        return Err(refuse("ended", ENDED));
    };
    // La page qui a obtenu l'accord, et elle seule (sans relire la configuration à chaque lot).
    if !same_page(&webview, &session.origin) {
        return Err(refuse("unavailable", NOT_ALLOWED));
    }
    for e in events.into_iter().take(MAX_BATCH).filter(RemoteInput::valid) {
        if session.jobs.send(Job::Input(e)).is_err() {
            // Le fil de saisie s'est arrêté (erreur du système) : fin du contrôle.
            st.session = None;
            drop(st);
            let _ = webview.app_handle().state::<StopItem>().0.set_enabled(false);
            return Err(refuse("ended", ENDED));
        }
    }
    Ok(())
}

/// Fin de la session `token`, demandée par la page.
#[tauri::command]
pub fn remote_end(webview: Webview, remote: tauri::State<'_, Remote>, token: String) -> Result<(), Refusal> {
    let mine = remote.lock().session.as_ref().is_some_and(|s| s.token == token && same_page(&webview, &s.origin));
    if mine {
        end(webview.app_handle(), false);
    }
    Ok(())
}

/// Arrête le contrôle en cours, s'il y en a un. `tell_page` : prévenir la page (arrêt par le
/// menu), qui prévient alors le serveur.
pub fn end(app: &AppHandle, tell_page: bool) {
    let session = app.state::<Remote>().lock().session.take();
    let _ = app.state::<StopItem>().0.set_enabled(false);
    // Le fil de saisie s'arrête quand son canal se ferme, et relâche touches et boutons.
    drop(session);
    if tell_page && let Some(w) = app.get_webview_window("main") {
        let _ = w.eval("window.dispatchEvent(new Event('kraken-remote-ended'))");
    }
}

/// Une page commence à se charger dans la fenêtre : l'accord ne la suit pas.
pub fn page_changed(app: &AppHandle) {
    app.state::<Remote>().lock().page += 1;
    end(app, false);
}

/// Le fil de saisie d'une session : joue les actions jusqu'à la fin, puis relâche tout.
fn inject(jobs: Receiver<Job>, screen: Screen, ready: mpsc::SyncSender<Result<(), String>>, stop: &AtomicBool) {
    let settings = Settings { release_keys_when_dropped: true, ..Settings::default() };
    let mut enigo = match Enigo::new(&settings) {
        Ok(e) => e,
        Err(e) => {
            let why = match e {
                #[cfg(target_os = "macos")]
                enigo::NewConError::NoPermission => MAC_ACCESSIBILITY.to_string(),
                e => format!("La saisie simulée est indisponible sur cet ordinateur ({e})."),
            };
            let _ = ready.send(Err(why));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let mut held: Vec<Button> = Vec::new();
    while let Ok(Job::Input(input)) = jobs.recv() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        // Une action refusée par le système (coordonnées hors écran…) n'arrête pas la session.
        let _ = play(&mut enigo, screen, input, &mut held);
    }
    for b in held {
        let _ = enigo.button(b, Direction::Release);
    }
    // `enigo` détruit : les touches encore enfoncées sont relâchées.
}

fn play(enigo: &mut Enigo, screen: Screen, input: RemoteInput, held: &mut Vec<Button>) -> enigo::InputResult<()> {
    match input {
        RemoteInput::Move { x, y } => {
            let (x, y) = screen.point(x, y);
            move_to(enigo, x, y)
        }
        RemoteInput::Button { button, down } => {
            let b = match button {
                MouseButton::Left => Button::Left,
                MouseButton::Middle => Button::Middle,
                MouseButton::Right => Button::Right,
                MouseButton::Back => Button::Back,
                MouseButton::Forward => Button::Forward,
            };
            held.retain(|h| *h != b);
            if down {
                held.push(b);
            }
            enigo.button(b, if down { Direction::Press } else { Direction::Release })
        }
        RemoteInput::Wheel { dx, dy } => {
            if dy != 0 {
                enigo.scroll(dy, Axis::Vertical)?;
            }
            if dx != 0 {
                enigo.scroll(dx, Axis::Horizontal)?;
            }
            Ok(())
        }
        RemoteInput::Key { code, down } => {
            let direction = if down { Direction::Press } else { Direction::Release };
            match native_key(&code) {
                Some(Native::Raw(k)) => enigo.raw(k, direction),
                Some(Native::Virtual(vk)) => enigo.key(enigo::Key::Other(vk), direction),
                None => Ok(()),
            }
        }
    }
}

/// Windows : la position absolue d'enigo ne vaut que pour l'écran principal ; `SetCursorPos`
/// prend les coordonnées de tout le bureau.
#[cfg(windows)]
fn move_to(_: &mut Enigo, x: i32, y: i32) -> enigo::InputResult<()> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetCursorPos(x: i32, y: i32) -> i32;
    }
    // SAFETY : fonction de user32 sans pointeur.
    if unsafe { SetCursorPos(x, y) } != 0 {
        Ok(())
    } else {
        Err(enigo::InputError::Simulate("SetCursorPos a échoué"))
    }
}

#[cfg(not(windows))]
fn move_to(enigo: &mut Enigo, x: i32, y: i32) -> enigo::InputResult<()> {
    enigo.move_mouse(x, y, enigo::Coordinate::Abs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(width: u32, height: u32, x: i32, primary: bool) -> Monitor {
        Monitor { width, height, primary, screen: Screen { x, y: 0, w: width, h: height } }
    }

    #[test]
    fn monitor_by_size_then_ratio_then_primary() {
        let laptop = monitor(2560, 1600, 0, true);
        let wide = monitor(1920, 1080, 2560, false);
        let tall = monitor(1080, 1920, 4480, false);
        let all = [laptop, wide, tall];
        // Taille exacte.
        assert_eq!(pick_monitor(&all, 1920, 1080, false), Some(wide.screen));
        assert_eq!(pick_monitor(&all, 1080, 1920, true), Some(tall.screen));
        // Image réduite : même rapport.
        assert_eq!(pick_monitor(&all, 1280, 720, false), Some(wide.screen));
        assert_eq!(pick_monitor(&all, 1280, 800, false), Some(laptop.screen));
        // Rien de semblable : l'écran principal ; sans `displaySurface`, rien.
        assert_eq!(pick_monitor(&all, 1000, 1000, false), Some(laptop.screen));
        assert_eq!(pick_monitor(&all, 1280, 720, true), None);
        assert_eq!(pick_monitor(&all, 0, 0, false), None);
        assert_eq!(pick_monitor(&[], 1920, 1080, false), None);
        // Deux écrans identiques : le principal d'abord.
        let twin = monitor(1920, 1080, -1920, true);
        assert_eq!(pick_monitor(&[wide, twin], 1920, 1080, false), Some(twin.screen));
    }

    #[test]
    fn relative_points_land_on_the_screen() {
        let s = Screen { x: 2560, y: -200, w: 1920, h: 1080 };
        assert_eq!(s.point(0.0, 0.0), (2560, -200));
        assert_eq!(s.point(1.0, 1.0), (2560 + 1919, -200 + 1079));
        assert_eq!(s.point(0.5, 0.5), (2560 + 960, -200 + 540));
        assert_eq!(s.point(7.0, -1.0), (2560 + 1919, -200), "bornées à l'écran");
    }

    #[test]
    fn keys_by_physical_position() {
        let codes: Vec<&str> = KEYS.iter().map(|(c, ..)| *c).collect();
        let mut unique = codes.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), codes.len(), "codes en double");
        for code in codes {
            assert!(RemoteInput::Key { code: code.into(), down: true }.valid(), "{code}");
        }
        assert_eq!(native_key("Nope"), None);
        assert_eq!(native_key(""), None);
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert_eq!(native_key("KeyA"), Some(Native::Raw(38)));
            assert_eq!(native_key("KeyQ"), Some(Native::Raw(24)));
            assert_eq!(native_key("ArrowUp"), Some(Native::Raw(111)));
            assert_eq!(native_key("Escape"), Some(Native::Raw(9)));
            assert_eq!(native_key("F12"), Some(Native::Raw(96)));
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(native_key("KeyA"), Some(Native::Raw(0)));
            assert_eq!(native_key("ArrowUp"), Some(Native::Raw(0x7E)));
            assert_eq!(native_key("PrintScreen"), None);
        }
        #[cfg(windows)]
        {
            assert_eq!(native_key("KeyA"), Some(Native::Raw(0x1E)));
            assert_eq!(native_key("ArrowUp"), Some(Native::Virtual(0x26)));
            assert_eq!(native_key("NumpadEqual"), None);
        }
    }

    #[test]
    fn names_shown_in_the_native_question() {
        assert_eq!(clean_name("  Lou  "), "Lou");
        assert_eq!(clean_name("a\nb\u{202e}c"), "abc");
        assert_eq!(clean_name("\u{0007}"), "Quelqu'un");
        assert_eq!(clean_name(&"x".repeat(200)).len(), 64);
    }
}
