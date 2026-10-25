//! Micro, caméra et partage d'écran dans la fenêtre, pour les salons vocaux du client web.
//!
//! - Partout : seule l'instance (celle qu'on a rejointe, ou le serveur hébergé ici) y a droit ;
//!   toute autre page se les voit refuser.
//! - Le navigateur intégré ne les donne qu'à un « contexte sécurisé » : https, ou cet ordinateur
//!   (le serveur hébergé s'ouvre donc par localhost). Une instance en http sur le réseau local est
//!   traitée comme sécurisée sous Windows (argument de WebView2, au démarrage seulement : l'appli
//!   se relance) et sous Linux (réglage de WebKitGTK 2.42 et plus). macOS n'a pas de réglage
//!   public : on y écoute et regarde seulement, sans parler ni partager.
//! - Linux : WebRTC est désactivé par défaut dans WebKitGTK, et il faut une version compilée
//!   avec (sinon le client web dit que le vocal n'est pas disponible).
//! - macOS : textes des demandes d'accès dans `Info.plist`, droits du runtime renforcé dans
//!   `Entitlements.plist`.

use tauri::webview::{PermissionKind, PermissionResponse};
use tauri::{Manager, Url, Webview};
use url::Host;

/// L'origine d'une instance en http hors de cet ordinateur : sans réglage, le navigateur
/// intégré n'y donne ni micro ni partage d'écran.
pub fn insecure_origin(url: &Url) -> Option<String> {
    let local = match url.host() {
        Some(Host::Domain(d)) => d == "localhost" || d.ends_with(".localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => true,
    };
    (url.scheme() == "http" && !local).then(|| url.origin().ascii_serialization())
}

/// Une page demande le micro, la caméra ou l'écran : oui pour l'instance, non pour les autres.
/// Le reste (notifications…) suit le comportement du navigateur intégré.
pub fn permission(webview: Webview, kind: PermissionKind) -> PermissionResponse {
    if !matches!(kind, PermissionKind::Microphone | PermissionKind::Camera | PermissionKind::DisplayCapture) {
        return PermissionResponse::Default;
    }
    match webview.url() {
        Ok(page) if crate::is_instance(webview.app_handle(), &page) => PermissionResponse::Allow,
        _ => PermissionResponse::Deny,
    }
}

/// Arguments que wry donne à WebView2 par défaut : en ajouter demande de les redonner.
#[cfg(windows)]
const WEBVIEW2_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Windows : WebView2 traite `origin` comme sécurisée. Ces arguments ne changent qu'à la
/// création du navigateur intégré, et des arguments différents demandent un dossier de données
/// différent (sinon la création échoue tant que l'ancien navigateur n'est pas fermé) : un dossier
/// par instance ainsi traitée.
#[cfg(windows)]
pub fn treat_as_secure<'a, M: tauri::Manager<tauri::Wry>>(
    builder: tauri::WebviewWindowBuilder<'a, tauri::Wry, M>,
    data: &std::path::Path,
    origin: &str,
) -> tauri::WebviewWindowBuilder<'a, tauri::Wry, M> {
    let name: String = origin
        .split_once("://")
        .map_or(origin, |(_, rest)| rest)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' })
        .collect();
    builder
        .additional_browser_args(&format!("{WEBVIEW2_ARGS} --unsafely-treat-insecure-origin-as-secure={origin}"))
        .data_directory(data.join("webview").join(name))
}

/// Avant d'ouvrir une instance. Linux : WebRTC et capture activés dans WebKitGTK ; `over_http` :
/// micro et partage d'écran aussi pour une page en http (seule l'instance y a droit, voir
/// [`permission`]). Ailleurs, rien à faire ici.
pub fn prepare(window: &tauri::WebviewWindow, over_http: bool) {
    #[cfg(target_os = "linux")]
    let _ = window.with_webview(move |w| {
        use webkit2gtk::{SettingsExt, WebViewExt};
        let Some(settings) = w.inner().settings() else { return };
        settings.set_enable_media_stream(true);
        settings.set_enable_webrtc(true);
        capture_over_http(&settings, over_http);
    });
    #[cfg(not(target_os = "linux"))]
    let _ = (window, over_http);
}

/// La fonctionnalité `MediaCaptureRequiresSecureConnection` de WebKitGTK 2.42 et plus. Ses
/// fonctions sont cherchées au lancement : une version plus ancienne démarre quand même (sans
/// micro pour une instance en http).
#[cfg(target_os = "linux")]
fn capture_over_http(settings: &webkit2gtk::Settings, allow: bool) {
    use std::ffi::{CStr, c_char, c_int, c_void};

    use webkit2gtk::glib::object::ObjectType;

    type AllFeatures = unsafe extern "C" fn() -> *mut c_void;
    type Length = unsafe extern "C" fn(*mut c_void) -> usize;
    type Get = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
    type Identifier = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type SetEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int);
    type Unref = unsafe extern "C" fn(*mut c_void);

    // SAFETY : dlsym sur des noms terminés par un zéro.
    let find = |name: &CStr| unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    let symbols = [
        find(c"webkit_settings_get_all_features"),
        find(c"webkit_feature_list_get_length"),
        find(c"webkit_feature_list_get"),
        find(c"webkit_feature_get_identifier"),
        find(c"webkit_settings_set_feature_enabled"),
        find(c"webkit_feature_list_unref"),
    ];
    if symbols.iter().any(|s| s.is_null()) {
        return;
    }
    let [all, length, get, identifier, set_enabled, unref] = symbols;
    // SAFETY : signatures de WebKitFeature.h et WebKitSettings.h (WebKitGTK 2.42). La liste
    // (transfer full) est libérée à la fin ; ses éléments (transfer none) restent à elle.
    unsafe {
        let all = std::mem::transmute::<*mut c_void, AllFeatures>(all);
        let length = std::mem::transmute::<*mut c_void, Length>(length);
        let get = std::mem::transmute::<*mut c_void, Get>(get);
        let identifier = std::mem::transmute::<*mut c_void, Identifier>(identifier);
        let set_enabled = std::mem::transmute::<*mut c_void, SetEnabled>(set_enabled);
        let unref = std::mem::transmute::<*mut c_void, Unref>(unref);
        let list = all();
        if list.is_null() {
            return;
        }
        for i in 0..length(list) {
            let feature = get(list, i);
            let id = identifier(feature);
            if !id.is_null() && CStr::from_ptr(id).to_bytes() == b"MediaCaptureRequiresSecureConnection" {
                set_enabled(settings.as_ptr().cast(), feature, c_int::from(!allow));
            }
        }
        unref(list);
    }
}

#[cfg(test)]
mod tests {
    use tauri::Url;

    use super::insecure_origin;

    #[test]
    fn only_plain_http_away_from_this_computer() {
        let o = |s: &str| insecure_origin(&Url::parse(s).unwrap());
        assert_eq!(o("http://192.168.1.20:8080/invite/x").as_deref(), Some("http://192.168.1.20:8080"));
        assert_eq!(o("http://kraken.local/").as_deref(), Some("http://kraken.local"));
        assert_eq!(o("https://chat.exemple.org/"), None);
        assert_eq!(o("http://localhost:8080/"), None);
        assert_eq!(o("http://127.0.0.1:8080/"), None);
        assert_eq!(o("http://[::1]:8080/"), None);
    }
}
