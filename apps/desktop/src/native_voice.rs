//! Vocal natif (Linux seulement) : le moteur de `kraken-voice-native` piloté par la page de l'instance.
//!
//! Le navigateur intégré de Linux (WebKitGTK de Debian, Ubuntu, Fedora) n'a pas WebRTC : la page
//! web fait donc l'appel HTTP (`POST /channels/{id}/voice`, elle a la session) et laisse ce module
//! fabriquer l'offre, jouer le son et capter le micro. Les sept commandes enveloppent un seul
//! [`Controller`](engine::Controller), partagé par toute l'appli (un appel à la fois) :
//!
//! | Commande | Arguments (`invoke`) | Réponse |
//! |---|---|---|
//! | `native_voice_available` | | `true` sous Linux, `false` ailleurs |
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
//! et adresse revérifiée à chaque appel). Windows et macOS ont WebRTC dans leur webview : les commandes
//! existent (la page n'a pas à deviner) mais `native_voice_available` y rend `false` et les autres refusent.
//!
//! Hors du fil principal : ouvrir un périphérique ou attendre le moteur peut prendre un moment. L'appel
//! ne survit pas à la page qui l'a lancé : un rechargement, un changement de page ([`page_changed`]) ou
//! la fermeture de l'appli ([`shutdown`]) l'arrêtent, micro compris.

use std::sync::OnceLock;

use tauri::Webview;

use engine::{Controller, Devices, Snapshot, StartOpts, StatePatch, VoiceJoin};

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

/// Ce que dit la page, hors de Linux, à qui appelle une commande autre que `native_voice_available`.
const ONLY_LINUX: &str = "Le vocal natif n'existe que sous Linux.";

/// Le moteur de cette plateforme : le vrai sous Linux.
#[cfg(target_os = "linux")]
mod engine {
    pub use kraken_proto::VoiceJoin;
    pub use kraken_voice_native::Snapshot;
    pub use kraken_voice_native::control::{Controller, Devices, StartOpts, StatePatch};
}

/// Ailleurs : mêmes noms, rien dedans (les commandes refusent avant, voir [`ready`]).
#[cfg(not(target_os = "linux"))]
mod engine {
    use serde_json::Value;

    pub type VoiceJoin = Value;
    pub type Snapshot = Value;
    pub type StartOpts = Value;
    pub type StatePatch = Value;
    pub type Devices = Value;

    pub struct Controller;

    impl Controller {
        pub fn new() -> Self {
            Self
        }
        pub fn available(&self) -> bool {
            false
        }
        pub fn start(&self, _: StartOpts) -> Result<VoiceJoin, String> {
            Err(super::ONLY_LINUX.into())
        }
        pub fn answer(&self, _: &str) -> Result<(), String> {
            Err(super::ONLY_LINUX.into())
        }
        pub fn set(&self, _: StatePatch) -> Result<(), String> {
            Err(super::ONLY_LINUX.into())
        }
        pub fn poll(&self) -> Option<Snapshot> {
            None
        }
        pub fn stop(&self) {}
        pub fn devices(&self) -> Result<Devices, String> {
            Err(super::ONLY_LINUX.into())
        }
    }
}

/// Le moteur de l'appli, créé au premier appel.
fn controller() -> &'static Controller {
    static CONTROLLER: OnceLock<Controller> = OnceLock::new();
    CONTROLLER.get_or_init(Controller::new)
}

/// La garde de toutes les commandes : la page est celle de l'instance (comme `open_in_browser`), et ce
/// système a un vocal natif.
fn ready(webview: &Webview) -> Result<&'static Controller, String> {
    crate::media::instance_page(webview)?;
    if cfg!(target_os = "linux") { Ok(controller()) } else { Err(ONLY_LINUX.into()) }
}

/// Un appel qui peut durer (périphérique à ouvrir, fin du moteur à attendre), hors du fil principal et des
/// fils de l'exécuteur asynchrone.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| "Le vocal natif s'est arrêté brutalement.".to_string())?
}

/// Une nouvelle page se charge (rechargement, autre instance, accueil) : l'appel de la précédente n'a plus
/// de page pour le montrer ni le couper, et le micro resterait ouvert. On l'arrête.
pub fn page_changed() {
    #[cfg(target_os = "linux")]
    drop(tauri::async_runtime::spawn_blocking(|| controller().stop()));
}

/// Fermeture de l'appli : le serveur voit le départ tout de suite.
pub fn shutdown() {
    #[cfg(target_os = "linux")]
    controller().stop();
}

/// Vrai sous Linux : la page choisit le vocal natif quand son navigateur n'a pas WebRTC.
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
