//! KRAKEN de bureau : une fenêtre sur l'instance choisie, qui sert elle-même le client web.
//! Au premier lancement, `ui/index.html` demande l'adresse de l'instance ; elle est gardée
//! dans le dossier de configuration de l'appli. Le menu KRAKEN permet d'en changer.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::path::PathBuf;

use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

/// La page locale, selon la plateforme (schéma `tauri://` ou `http://tauri.localhost`).
fn local_page() -> Url {
    let base = if cfg!(any(windows, target_os = "android")) { "http://tauri.localhost/" } else { "tauri://localhost/" };
    Url::parse(base).expect("adresse locale").join("index.html").expect("adresse locale")
}

fn saved_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("instance.txt"))
}

fn saved(app: &AppHandle) -> Option<Url> {
    let text = fs::read_to_string(saved_path(app)?).ok()?;
    Url::parse(text.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https"))
}

/// Appelée par la page locale : retient l'adresse et y va.
#[tauri::command]
fn open_instance(app: AppHandle, address: String) -> Result<(), String> {
    let address = address.trim();
    let with_scheme = if address.contains("://") { address.to_string() } else { format!("https://{address}") };
    let url = Url::parse(&with_scheme).map_err(|_| "Adresse invalide.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Adresse invalide.".into());
    }
    // Seule l'origine compte : l'instance sert le client à sa racine.
    let url = Url::parse(&url.origin().ascii_serialization()).map_err(|e| e.to_string())?;
    if let Some(path) = saved_path(&app) {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(path, url.as_str());
    }
    let window = app.get_webview_window("main").ok_or("fenêtre absente")?;
    window.navigate(url).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![open_instance])
        .setup(|app| {
            let change = MenuItemBuilder::with_id("change", "Changer d'instance…").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&SubmenuBuilder::new(app, "KRAKEN").item(&change).separator().quit().build()?)
                .item(
                    &SubmenuBuilder::new(app, "Édition")
                        .undo()
                        .redo()
                        .separator()
                        .cut()
                        .copy()
                        .paste()
                        .select_all()
                        .build()?,
                )
                .build()?;
            app.set_menu(menu)?;
            app.on_menu_event(|app, event| {
                if event.id() == "change"
                    && let Some(w) = app.get_webview_window("main")
                {
                    let _ = w.navigate(local_page());
                }
            });
            let start = match saved(app.handle()) {
                Some(url) => WebviewUrl::External(url),
                None => WebviewUrl::App("index.html".into()),
            };
            WebviewWindowBuilder::new(app, "main", start)
                .title("KRAKEN")
                .inner_size(1280.0, 800.0)
                .min_inner_size(400.0, 500.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("KRAKEN n'a pas pu démarrer");
}
