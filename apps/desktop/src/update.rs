//! Mises à jour de l'appli. La page de l'instance (et elle seule, comme pour le contrôle à
//! distance : capacité ajoutée à l'origine, adresse revérifiée à chaque appel) demande
//! `update_status`, puis `update_install` si une version plus récente existe.
//!
//! Deux voies :
//! - signée : `latest.json` de la dernière Release, vérifié avec la clé publique de
//!   `tauri.conf.json` ; la mise à jour est téléchargée, installée, puis l'appli redémarre ;
//! - de repli, sans `latest.json` (Release publiée sans clé de signature), signature refusée,
//!   AppImage absente sous Linux (.deb, .rpm) : la dernière Release de l'API GitHub est comparée à la
//!   version de l'appli, et `update_install` ouvre le bon téléchargement dans le navigateur.
//!
//! La progression n'est pas poussée vers la page (aucun événement à autoriser) : elle la lit en
//! rappelant `update_status` pendant l'installation (`phase`, `downloaded`, `total`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Webview};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Commandes de ce module (build.rs, capacité de l'instance).
pub const COMMANDS: [&str; 2] = ["update_status", "update_install"];

/// Délai entre deux vérifications automatiques.
pub const EVERY: Duration = Duration::from_secs(6 * 3600);
/// Un résultat plus récent que ça est réutilisé (la page rappelle souvent pendant l'installation).
const FRESH: Duration = Duration::from_secs(300);
const API: &str = "https://api.github.com/repos/KAYO74/Kraken/releases/latest";
const DOWNLOADS: &str = "https://github.com/KAYO74/Kraken/releases/latest/download";
const RELEASES: &str = "https://github.com/KAYO74/Kraken/releases";
const NOT_ALLOWED: &str = "Commande réservée à la page de ton instance.";

/// Nom fixe du téléchargement pour ce système (les noms des Releases, `desktop.yml`) ; `None`
/// quand aucune version n'est construite pour lui (la page des Releases s'ouvre alors).
fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", _) => Some("KRAKEN-Windows.exe"),
        ("macos", "aarch64") => Some("KRAKEN-macOS-Apple.dmg"),
        ("macos", _) => Some("KRAKEN-macOS-Intel.dmg"),
        ("linux", "x86_64") => Some("KRAKEN-Linux.AppImage"),
        _ => None,
    }
}

fn download_url(os: &str, arch: &str) -> String {
    match asset_name(os, arch) {
        Some(name) => format!("{DOWNLOADS}/{name}"),
        None => RELEASES.to_string(),
    }
}

/// Vrai si `tag` (« v0.9.0 » ou « 0.9.0 ») est une version strictement plus récente que `current`.
/// Une étiquette illisible n'est jamais plus récente.
fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| Version::parse(s.trim().trim_start_matches('v')).ok();
    matches!((parse(tag), parse(current)), (Some(t), Some(c)) if t > c)
}

/// Ce que la page voit.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status {
    pub current: String,
    /// La version proposée, s'il y en a une.
    pub available: Option<String>,
    pub notes: Option<String>,
    /// Page des nouveautés de cette version.
    pub notes_url: Option<String>,
    /// `install` : installée dans l'appli ; `download` : le téléchargement s'ouvre dans le navigateur.
    pub mode: Option<&'static str>,
    /// `idle`, `downloading`, `installing` ou `restarting`.
    pub phase: &'static str,
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Downloading,
    Installing,
    Restarting,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Downloading => "downloading",
            Phase::Installing => "installing",
            Phase::Restarting => "restarting",
        }
    }
}

#[derive(Clone)]
struct Found {
    version: String,
    notes: Option<String>,
    notes_url: String,
    /// La mise à jour signée ; sans elle, c'est le repli (téléchargement dans le navigateur).
    signed: Option<Update>,
}

struct Inner {
    checked: Option<Instant>,
    found: Option<Found>,
    phase: Phase,
    downloaded: u64,
    total: Option<u64>,
}

pub struct Updates(Mutex<Inner>);

impl Default for Updates {
    fn default() -> Self {
        Updates(Mutex::new(Inner { checked: None, found: None, phase: Phase::Idle, downloaded: 0, total: None }))
    }
}

impl Updates {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn status(app: &AppHandle) -> Status {
    let inner = app.state::<Updates>();
    let inner = inner.lock();
    let found = inner.found.as_ref();
    Status {
        current: app.package_info().version.to_string(),
        available: found.map(|f| f.version.clone()),
        notes: found.and_then(|f| f.notes.clone()),
        notes_url: found.map(|f| f.notes_url.clone()),
        mode: found.map(|f| if f.signed.is_some() { "install" } else { "download" }),
        phase: inner.phase.name(),
        downloaded: inner.downloaded,
        total: inner.total,
    }
}

fn notes_url(version: &str) -> String {
    format!("{RELEASES}/tag/v{version}")
}

fn short(notes: Option<String>) -> Option<String> {
    notes.map(|n| n.trim().chars().take(4000).collect::<String>()).filter(|n| !n.is_empty())
}

/// Le repli : la dernière Release publiée sur GitHub, comparée à la version de l'appli.
async fn from_github(current: &str) -> Result<Option<Found>, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        body: Option<String>,
        html_url: Option<String>,
    }
    let client = reqwest::Client::builder()
        .user_agent(format!("KRAKEN/{current}"))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let release: Release = client
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if !is_newer(&release.tag_name, current) {
        return Ok(None);
    }
    let version = release.tag_name.trim().trim_start_matches('v').to_string();
    let notes_url =
        release.html_url.filter(|u| u.starts_with("https://github.com/")).unwrap_or_else(|| notes_url(&version));
    Ok(Some(Found { version, notes: short(release.body), notes_url, signed: None }))
}

/// Cherche une version plus récente : voie signée, sinon repli. `Err` : rien n'a pu être lu.
async fn find(app: &AppHandle) -> Result<Option<Found>, String> {
    // Sous Linux, l'installation sur place n'existe que pour l'AppImage.
    let in_place = !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some();
    if in_place && let Ok(updater) = app.updater() {
        match updater.check().await {
            Ok(Some(update)) => {
                let version = update.version.clone();
                return Ok(Some(Found {
                    notes_url: notes_url(&version),
                    notes: short(update.body.clone()),
                    version,
                    signed: Some(update),
                }));
            }
            Ok(None) => return Ok(None),
            // Pas de `latest.json` (Release sans signature) ou illisible : le repli.
            Err(_) => {}
        }
    }
    from_github(&app.package_info().version.to_string()).await
}

/// Revérifie (sauf résultat récent, sans `force`, ou installation en cours) et rend l'état.
pub async fn refresh(app: &AppHandle, force: bool) -> Status {
    {
        let updates = app.state::<Updates>();
        let inner = updates.lock();
        if inner.phase != Phase::Idle || (!force && inner.checked.is_some_and(|t| t.elapsed() < FRESH)) {
            drop(inner);
            return status(app);
        }
    }
    let result = find(app).await;
    {
        let updates = app.state::<Updates>();
        let mut inner = updates.lock();
        // Hors ligne : on garde ce qu'on savait.
        if let Ok(found) = result {
            inner.found = found;
        }
        inner.checked = Some(Instant::now());
    }
    status(app)
}

/// Seule la page de l'instance, dans la fenêtre principale.
fn check_page(webview: &Webview) -> Result<(), String> {
    let ok =
        webview.label() == "main" && webview.url().is_ok_and(|page| crate::is_instance(webview.app_handle(), &page));
    if ok { Ok(()) } else { Err(NOT_ALLOWED.into()) }
}

#[tauri::command]
pub async fn update_status(webview: Webview) -> Result<Status, String> {
    check_page(&webview)?;
    Ok(refresh(webview.app_handle(), false).await)
}

/// Ouvre une adresse https de GitHub (construite ici, jamais fournie par la page) dans le navigateur.
fn open_external(url: &str) -> Result<(), String> {
    use std::process::Command;
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    command.spawn().map(|_| ()).map_err(|_| "Impossible d'ouvrir le navigateur.".to_string())
}

/// Installe la mise à jour signée puis relance l'appli ; en repli, ouvre le téléchargement.
#[tauri::command]
pub async fn update_install(webview: Webview) -> Result<(), String> {
    check_page(&webview)?;
    let app = webview.app_handle().clone();
    let found = {
        let updates = app.state::<Updates>();
        let mut inner = updates.lock();
        if inner.phase != Phase::Idle {
            return Err("Une mise à jour est déjà en cours.".into());
        }
        let Some(found) = inner.found.clone() else { return Err("Aucune mise à jour disponible.".into()) };
        if found.signed.is_some() {
            inner.phase = Phase::Downloading;
            inner.downloaded = 0;
            inner.total = None;
        }
        found
    };
    let Some(update) = found.signed else {
        return open_external(&download_url(std::env::consts::OS, std::env::consts::ARCH));
    };
    let progress = app.clone();
    let finished = app.clone();
    let result = update
        .download_and_install(
            move |chunk, total| {
                let updates = progress.state::<Updates>();
                let mut inner = updates.lock();
                inner.downloaded += chunk as u64;
                inner.total = total;
            },
            move || finished.state::<Updates>().lock().phase = Phase::Installing,
        )
        .await;
    let updates = app.state::<Updates>();
    match result {
        Ok(()) => {
            updates.lock().phase = Phase::Restarting;
            // Arrête aussi PostgreSQL proprement (`RunEvent::Exit`).
            app.request_restart();
            Ok(())
        }
        Err(e) => {
            let mut inner = updates.lock();
            inner.phase = Phase::Idle;
            // Signature refusée, ou installation impossible ici : désormais, le téléchargement à la main.
            if let Some(f) = inner.found.as_mut() {
                f.signed = None;
            }
            Err(format!("La mise à jour n'a pas pu s'installer ({e}). Tu peux la télécharger à la main."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{asset_name, download_url, is_newer};

    #[test]
    fn newer_versions_by_semver() {
        assert!(is_newer("v0.9.0", "0.8.0"));
        assert!(is_newer("0.8.1", "0.8.0"));
        assert!(is_newer("v0.10.0", "0.9.9"), "comparaison numérique, pas de texte");
        assert!(is_newer("v1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer("v0.8.0", "0.8.0"));
        assert!(!is_newer("v0.7.9", "0.8.0"));
        assert!(!is_newer("v1.0.0-rc.1", "1.0.0"));
        assert!(!is_newer("nightly", "0.8.0"));
        assert!(!is_newer("", "0.8.0"));
    }

    #[test]
    fn download_by_os_and_arch() {
        assert_eq!(asset_name("windows", "x86_64"), Some("KRAKEN-Windows.exe"));
        assert_eq!(asset_name("windows", "aarch64"), Some("KRAKEN-Windows.exe"));
        assert_eq!(asset_name("macos", "aarch64"), Some("KRAKEN-macOS-Apple.dmg"));
        assert_eq!(asset_name("macos", "x86_64"), Some("KRAKEN-macOS-Intel.dmg"));
        assert_eq!(asset_name("linux", "x86_64"), Some("KRAKEN-Linux.AppImage"));
        assert_eq!(asset_name("linux", "aarch64"), None);
        assert_eq!(
            download_url("macos", "aarch64"),
            "https://github.com/KAYO74/Kraken/releases/latest/download/KRAKEN-macOS-Apple.dmg"
        );
        assert_eq!(download_url("freebsd", "x86_64"), "https://github.com/KAYO74/Kraken/releases");
    }
}
