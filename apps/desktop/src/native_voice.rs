//! Vocal natif (Linux, macOS, Windows) : le moteur de `kraken-voice-native` piloté par la page de l'instance.
//!
//! Le navigateur intégré ne sait pas toujours faire l'appel : celui de Linux (WebKitGTK de Debian, Ubuntu, Fedora)
//! n'a pas WebRTC, et celui de macOS (WKWebView) ou de Windows (WebView2) ne donne pas le micro à une instance en
//! http hors de cet ordinateur. La page web fait alors l'appel HTTP (`POST /channels/{id}/voice`, elle a la
//! session) et laisse ce module fabriquer l'offre, jouer le son et capter le micro (cpal : ALSA, CoreAudio ou
//! WASAPI ; la règle qui décide est dans le client, `voice::browser_can_call`). Les sept commandes enveloppent un seul
//! [`Controller`], partagé par toute l'appli (un appel à la fois) :
//!
//! | Commande | Arguments (`invoke`) | Réponse |
//! |---|---|---|
//! | `native_voice_available` | | `true` : le moteur est là (avec son pilote audio) |
//! | `native_voice_start` | `opts` : `StartOpts`, facultatif | `VoiceJoin` : le corps de `POST /channels/{id}/voice` |
//! | `native_voice_answer` | `sdp` : la réponse du serveur | rien |
//! | `native_voice_set` | `patch` : `StatePatch` | rien |
//! | `native_voice_poll` | | l'état (`Snapshot`), ou `null` sans appel |
//! | `native_voice_stop` | | rien |
//! | `native_voice_devices` | | `{ inputs, outputs }` |
//!
//! Les types et leurs champs : `kraken_voice_native::control`. Une erreur est une phrase en français
//! que la page peut afficher telle quelle.
//!
//! Même garde que `open_in_browser` ([`crate::media::instance_page`]) : seule la page de l'instance, dans la
//! fenêtre principale, en http ou https, y a droit (capacité ajoutée à son origine, `remote::allow_origin`,
//! et adresse revérifiée à chaque appel).
//!
//! Hors du fil principal : ouvrir un périphérique ou attendre le moteur peut prendre un moment. L'appel
//! ne survit pas à la page qui l'a lancé : un rechargement, un changement de page ([`page_changed`]) ou
//! la fermeture de l'appli ([`shutdown`]) l'arrêtent, micro compris.

use std::sync::OnceLock;

use kraken_proto::VoiceJoin;
use kraken_voice_native::Snapshot;
use kraken_voice_native::control::{Controller, Devices, StartOpts, StatePatch};
use tauri::Webview;

/// Commandes de ce module (build.rs, capacité de l'instance).
pub const COMMANDS: [&str; 7] = [
    "native_voice_available",
    "native_voice_start",
    "native_voice_answer",
    "native_voice_set",
    "native_voice_poll",
    "native_voice_stop",
    "native_voice_devices",
];

/// Le moteur de l'appli, créé au premier appel.
fn controller() -> &'static Controller {
    static CONTROLLER: OnceLock<Controller> = OnceLock::new();
    CONTROLLER.get_or_init(Controller::new)
}

/// La garde de toutes les commandes : la page est celle de l'instance (comme `open_in_browser`).
fn ready(webview: &Webview) -> Result<&'static Controller, String> {
    crate::media::instance_page(webview)?;
    Ok(controller())
}

/// Un appel qui peut durer (périphérique à ouvrir, fin du moteur à attendre), hors du fil principal et des
/// fils de l'exécuteur asynchrone.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| "Le vocal natif s'est arrêté brutalement.".to_string())?
}

/// Une nouvelle page se charge (rechargement, autre instance, accueil) : l'appel de la précédente n'a plus
/// de page pour le montrer ni le couper, et le micro resterait ouvert. On l'arrête.
pub fn page_changed() {
    drop(tauri::async_runtime::spawn_blocking(|| controller().stop()));
}

/// Fermeture de l'appli : le serveur voit le départ tout de suite.
pub fn shutdown() {
    controller().stop();
}

/// Vrai quand le moteur est là : la page l'utilise quand son navigateur intégré ne peut pas faire l'appel.
#[tauri::command(async)]
pub fn native_voice_available(webview: Webview) -> Result<bool, String> {
    crate::media::instance_page(&webview)?;
    Ok(controller().available())
}

/// Lance l'appel (le précédent est arrêté) ; rend le corps de `POST /channels/{id}/voice`.
#[tauri::command]
pub async fn native_voice_start(webview: Webview, opts: Option<StartOpts>) -> Result<VoiceJoin, String> {
    let controller = ready(&webview)?;
    blocking(move || controller.start(opts.unwrap_or_default())).await
}

/// La réponse du serveur à l'offre.
#[tauri::command(async)]
pub fn native_voice_answer(webview: Webview, sdp: String) -> Result<(), String> {
    ready(&webview)?.answer(&sdp)
}

/// Micro coupé, son coupé, gain du micro, volume de sortie : seuls les champs présents changent.
#[tauri::command(async)]
pub fn native_voice_set(webview: Webview, patch: StatePatch) -> Result<(), String> {
    ready(&webview)?.set(patch)
}

/// L'état de l'appel, `null` sans appel.
#[tauri::command(async)]
pub fn native_voice_poll(webview: Webview) -> Result<Option<Snapshot>, String> {
    Ok(ready(&webview)?.poll())
}

/// Quitte l'appel.
#[tauri::command]
pub async fn native_voice_stop(webview: Webview) -> Result<(), String> {
    let controller = ready(&webview)?;
    blocking(move || {
        controller.stop();
        Ok(())
    })
    .await
}

/// Micros et haut-parleurs.
#[tauri::command]
pub async fn native_voice_devices(webview: Webview) -> Result<Devices, String> {
    let controller = ready(&webview)?;
    blocking(move || controller.devices()).await
}
