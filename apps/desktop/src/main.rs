//! KRAKEN de bureau. Au premier lancement, `ui/index.html` propose deux choix :
//! - rejoindre un serveur : la fenêtre ouvre l'instance, qui sert elle-même le client web ;
//! - héberger sur cet ordinateur : l'appli fait tourner son propre serveur (`host.rs`).
//!
//! Le choix est gardé dans le dossier de configuration de l'appli ; le menu KRAKEN permet d'en
//! changer. Micro et partage d'écran (salons vocaux) : `media.rs`. `--self-test` vérifie le mode
//! « héberger », vocal compris, sans ouvrir de fenêtre (CI).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod host;
mod media;
mod selftest;

use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use host::Hosting;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder, SubmenuBuilder};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, RunEvent, State, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};
use url::Host;

/// Une page locale, selon la plateforme (schéma `tauri://` ou `http://tauri.localhost`).
fn local_page(page: &str) -> Url {
    let base = if cfg!(any(windows, target_os = "android")) { "http://tauri.localhost/" } else { "tauri://localhost/" };
    Url::parse(base).expect("adresse locale").join(page).expect("adresse locale")
}

fn main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main").ok_or_else(|| "fenêtre absente".into())
}

/// Le choix retenu au lancement précédent.
enum Saved {
    Welcome,
    Join(Url),
    Host,
}

fn config_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok()
}

fn saved(app: &AppHandle) -> Saved {
    let Some(dir) = config_dir(app) else { return Saved::Welcome };
    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap_or_default().trim().to_string();
    if read("mode.txt") == "host" {
        return Saved::Host;
    }
    // Sans `mode.txt` (version 0.1) : l'instance retenue.
    match Url::parse(&read("instance.txt")) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => Saved::Join(url),
        _ => Saved::Welcome,
    }
}

fn save(app: &AppHandle, mode: &str, instance: Option<&Url>) {
    let Some(dir) = config_dir(app) else { return };
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("mode.txt"), mode);
    if let Some(url) = instance {
        let _ = fs::write(dir.join("instance.txt"), url.as_str());
    }
}

/// Le lien à ouvrir au prochain lancement, quand l'appli se relance pour le vocal (Windows).
#[cfg(windows)]
fn save_link(app: &AppHandle, link: &Url) {
    if let Some(dir) = config_dir(app) {
        let _ = fs::write(dir.join("open.txt"), link.as_str());
    }
}

/// Le lien laissé par [`save_link`], une seule fois.
fn take_link(app: &AppHandle) -> Option<Url> {
    let path = config_dir(app)?.join("open.txt");
    let link = fs::read_to_string(&path).ok();
    let _ = fs::remove_file(&path);
    Url::parse(link?.trim()).ok()
}

/// Vrai si `page` est sur l'instance : celle qu'on a rejointe, ou le serveur hébergé ici.
fn is_instance(app: &AppHandle, page: &Url) -> bool {
    let origin = page.origin();
    let hosted = app.state::<Hosting>().local_url().and_then(|u| Url::parse(&u).ok());
    hosted.is_some_and(|u| u.origin() == origin) || matches!(saved(app), Saved::Join(url) if url.origin() == origin)
}

/// Windows : l'instance en http que WebView2 traite comme sécurisée depuis le lancement.
#[cfg(windows)]
struct SecureOrigin(Option<String>);

/// Dossier des données du serveur hébergé, et client web livré avec l'appli.
fn host_dirs(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let data = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("server");
    let web = app.path().resolve("client", BaseDirectory::Resource).map_err(|e| e.to_string())?;
    Ok((data, web))
}

/// L'élément « Adresse à partager… », actif seulement quand l'appli héberge.
struct ShareItem(MenuItem<Wry>);

fn share_enabled(app: &AppHandle, enabled: bool) {
    let _ = app.state::<ShareItem>().0.set_enabled(enabled);
}

/// Ce que la personne a collé : l'adresse d'une instance ou un lien d'invitation.
fn parse_address(input: &str) -> Result<Url, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Colle l'adresse du serveur ou le lien d'invitation.".into());
    }
    let with_scheme = if input.contains("://") {
        input.to_string()
    } else {
        // Sans schéma : http pour une adresse du réseau local, https sinon.
        let host = input.split(['/', ':', '?', '#']).next().unwrap_or_default();
        let local = host.parse::<IpAddr>().is_ok() || host == "localhost" || host.ends_with(".local");
        format!("{}://{input}", if local { "http" } else { "https" })
    };
    let mut url = Url::parse(&with_scheme).map_err(|_| "Adresse invalide.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Adresse invalide.".into());
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Ok(url)
}

const UNREACHABLE: &str =
    "Ce serveur ne répond pas. Vérifie l'adresse, ou que l'ordinateur qui l'héberge a KRAKEN ouvert.";

/// Pour une adresse du réseau local seulement : vrai si quelque chose y répond. Les autres ne
/// sont pas vérifiées : derrière un proxy d'entreprise, la fenêtre peut les ouvrir alors
/// qu'une connexion directe ne passe pas.
async fn reachable(url: &Url, wait: Duration) -> bool {
    let local = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        Some(Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
        Some(Host::Domain(d)) => d == "localhost" || d.ends_with(".local"),
        None => false,
    };
    let (true, Some(host), Some(port)) = (local, url.host_str(), url.port_or_known_default()) else { return true };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let connect = async {
        for addr in tokio::net::lookup_host((host, port)).await.ok()? {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return Some(());
            }
        }
        None
    };
    matches!(tokio::time::timeout(wait, connect).await, Ok(Some(())))
}

/// Appelée par la page d'accueil : retient l'instance (son origine) et ouvre le lien tel quel,
/// pour qu'un lien d'invitation affiche sa page. Arrête le serveur hébergé s'il tourne, sauf si
/// c'est lui qu'on rejoint (ouvert alors par localhost).
#[tauri::command]
async fn join(app: AppHandle, hosting: State<'_, Hosting>, address: String) -> Result<(), String> {
    let link = parse_address(&address)?;
    if let Some(own) = hosting.own_link(&link) {
        return main_window(&app)?.navigate(own).map_err(|e| e.to_string());
    }
    if !reachable(&link, Duration::from_secs(4)).await {
        return Err(UNREACHABLE.into());
    }
    let origin = Url::parse(&link.origin().ascii_serialization()).map_err(|e| e.to_string())?;
    save(&app, "join", Some(&origin));
    hosting.stop();
    share_enabled(&app, false);
    // WebView2 ne change d'origine traitée comme sécurisée (et de dossier de données) qu'à son
    // démarrage : l'appli se relance sur le lien.
    #[cfg(windows)]
    if media::insecure_origin(&origin) != app.state::<SecureOrigin>().0 {
        save_link(&app, &link);
        app.request_restart();
        return Ok(());
    }
    let window = main_window(&app)?;
    media::prepare(&window, media::insecure_origin(&origin).is_some());
    window.navigate(link).map_err(|e| e.to_string())
}

/// Appelée par la page d'accueil : passe en mode « héberger » et démarre le serveur.
#[tauri::command]
fn host_start(app: AppHandle, hosting: State<'_, Hosting>) -> Result<(), String> {
    let (data, web) = host_dirs(&app)?;
    save(&app, "host", None);
    hosting.start(data, web);
    share_enabled(&app, true);
    Ok(())
}

#[tauri::command]
fn host_status(hosting: State<'_, Hosting>) -> host::Status {
    hosting.status()
}

/// Ouvre le serveur hébergé dans la fenêtre, par localhost (micro et partage d'écran permis) ;
/// les liens d'invitation prennent son adresse sur le réseau (`public_url`).
#[tauri::command]
fn host_open(app: AppHandle, hosting: State<'_, Hosting>) -> Result<(), String> {
    let url = hosting.open_url().ok_or("Ton serveur n'est pas encore prêt.")?;
    let url = Url::parse(&url).map_err(|e| e.to_string())?;
    main_window(&app)?.navigate(url).map_err(|e| e.to_string())
}

fn main() {
    let context = tauri::generate_context!();
    if std::env::args().any(|a| a == "--self-test") {
        // Avant toute fenêtre (et GTK) : tourne en CI, sans écran.
        let web = tauri::utils::platform::resource_dir(context.package_info(), &tauri::Env::default())
            .map(|d| d.join("client"))
            .unwrap_or_default();
        std::process::exit(selftest::run(&web));
    }
    let app = tauri::Builder::default()
        .manage(Hosting::default())
        .invoke_handler(tauri::generate_handler![join, host_start, host_status, host_open])
        .setup(|app| {
            let saved = saved(app.handle());
            let hosting = matches!(saved, Saved::Host);
            let change = MenuItemBuilder::with_id("change", "Changer de serveur…").build(app)?;
            let share = MenuItemBuilder::with_id("share", "Adresse à partager…").enabled(hosting).build(app)?;
            let kraken = SubmenuBuilder::new(app, "KRAKEN").item(&change).item(&share).separator();
            // L'élément « Quitter » natif n'existe pas sous Linux (GTK) : un élément à nous,
            // sauf sur macOS où le natif est attendu.
            let kraken = if cfg!(target_os = "macos") {
                kraken.quit_with_text("Quitter KRAKEN")
            } else {
                kraken.item(&MenuItemBuilder::with_id("quit", "Quitter").accelerator("CmdOrCtrl+Q").build(app)?)
            };
            let menu = MenuBuilder::new(app)
                .item(&kraken.build()?)
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
            app.manage(ShareItem(share));
            app.on_menu_event(|app, event| {
                let page = match event.id().as_ref() {
                    "change" => "index.html",
                    "share" => "host.html#partager",
                    "quit" => return app.exit(0),
                    _ => return,
                };
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.navigate(local_page(page));
                }
            });
            // Une instance en http sur le réseau local : micro et partage d'écran quand même.
            let insecure = match &saved {
                Saved::Join(url) => media::insecure_origin(url),
                _ => None,
            };
            let link = take_link(app.handle());
            let start = match saved {
                Saved::Host => match host_dirs(app.handle()) {
                    Ok((data, web)) => {
                        app.state::<Hosting>().start(data, web);
                        WebviewUrl::App("host.html".into())
                    }
                    Err(_) => WebviewUrl::App("index.html".into()),
                },
                // Un serveur du réseau local éteint : l'accueil, avec l'adresse prête à réessayer.
                Saved::Join(url) if !tauri::async_runtime::block_on(reachable(&url, Duration::from_secs(2))) => {
                    let address: String = url::form_urlencoded::byte_serialize(url.as_str().as_bytes()).collect();
                    WebviewUrl::App(format!("index.html#injoignable={address}").into())
                }
                Saved::Join(url) => WebviewUrl::External(link.filter(|l| l.origin() == url.origin()).unwrap_or(url)),
                Saved::Welcome => WebviewUrl::App("index.html".into()),
            };
            let builder = WebviewWindowBuilder::new(app, "main", start)
                .title("KRAKEN")
                .inner_size(1280.0, 800.0)
                .min_inner_size(400.0, 500.0)
                .on_permission_request(media::permission);
            #[cfg(windows)]
            let (builder, insecure) = match (insecure, app.path().app_local_data_dir()) {
                (Some(origin), Ok(data)) => (media::treat_as_secure(builder, &data, &origin), Some(origin)),
                _ => (builder, None),
            };
            #[cfg(windows)]
            app.manage(SecureOrigin(insecure.clone()));
            media::prepare(&builder.build()?, insecure.is_some());
            Ok(())
        })
        .build(context)
        .expect("KRAKEN n'a pas pu démarrer");
    app.run(|app, event| {
        // PostgreSQL s'arrête proprement avec l'appli.
        if let RunEvent::Exit = event {
            app.state::<Hosting>().stop_and_wait();
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tauri::Url;

    use super::{parse_address, reachable};

    #[tokio::test]
    async fn only_local_addresses_are_checked() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let open = Url::parse(&format!("http://127.0.0.1:{port}/invite/x")).unwrap();
        assert!(reachable(&open, Duration::from_secs(2)).await);
        drop(listener);
        assert!(!reachable(&open, Duration::from_secs(2)).await);
        assert!(!reachable(&Url::parse(&format!("http://localhost:{port}")).unwrap(), Duration::from_secs(2)).await);
        // Une adresse publique n'est pas vérifiée (un proxy peut être le seul chemin).
        assert!(reachable(&Url::parse("https://chat.exemple.org").unwrap(), Duration::from_millis(1)).await);
    }

    #[test]
    fn addresses_and_invite_links() {
        let ok = |s: &str| parse_address(s).map(|u| u.to_string());
        assert_eq!(ok("chat.exemple.org").unwrap(), "https://chat.exemple.org/");
        assert_eq!(ok(" https://chat.exemple.org/invite/AbC123 ").unwrap(), "https://chat.exemple.org/invite/AbC123");
        assert_eq!(ok("192.168.1.20:8080/invite/x").unwrap(), "http://192.168.1.20:8080/invite/x");
        assert_eq!(ok("http://a:b@chat.exemple.org").unwrap(), "http://chat.exemple.org/");
        assert!(parse_address("").is_err());
        assert!(parse_address("ftp://chat.exemple.org").is_err());
        assert!(parse_address("file:///etc/passwd").is_err());
        assert!(parse_address("https://").is_err());
    }
}
