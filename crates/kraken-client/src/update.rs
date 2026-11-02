//! Mises à jour de l'appli de bureau. Dans le navigateur, rien : sans `__TAURI_INTERNALS__`, ce
//! module ne fait aucun appel et ne montre rien.
//!
//! Dans l'appli, `update_status` est demandé au lancement puis toutes les 6 h. Quand une version
//! plus récente existe, une tuile apparaît au bas du rail, au-dessus de « Mon compte » (et une
//! entrée dans la carte « Mon compte ») : elle ouvre une carte, jamais une fenêtre qui s'ouvre
//! seule. « Installer et redémarrer » lance `update_install` ; pendant ce temps la progression se
//! lit en rappelant `update_status`. Sans mise à jour signée (`mode` « download »), le bouton
//! devient « Télécharger » et la page de téléchargement s'ouvre dans le navigateur.

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::icons::{Icon, lu};
use crate::store::store;
use crate::voice::{self, Status};

/// Entre deux demandes de l'état, hors installation.
const EVERY_MS: u32 = 6 * 3600 * 1000;
/// Entre deux lectures de la progression pendant l'installation.
const POLL_MS: u32 = 400;

/// Ce que répond `update_status`.
#[derive(serde::Deserialize, Clone, PartialEq, Default)]
pub struct Info {
    available: Option<String>,
    notes_url: Option<String>,
    /// `install` ou `download`.
    mode: Option<String>,
    phase: String,
    downloaded: u64,
    total: Option<u64>,
}

impl Info {
    /// Avancement du téléchargement en pour cent, s'il est connu.
    fn percent(&self) -> Option<u64> {
        self.total.filter(|t| *t > 0).map(|t| (self.downloaded * 100 / t).min(100))
    }
}

#[derive(Clone, Copy)]
pub struct Updater {
    info: RwSignal<Option<Info>>,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    restarting: RwSignal<bool>,
    /// En appel : le premier clic demande confirmation.
    confirm: RwSignal<bool>,
    error: RwSignal<Option<String>>,
}

/// Appelle une commande de l'appli de bureau ; l'erreur est le texte qu'elle renvoie.
async fn call(cmd: &str) -> Result<wasm_bindgen::JsValue, String> {
    let broken = || "L'appli de bureau ne répond pas.".to_string();
    let (internals, f) = voice::tauri().ok_or_else(broken)?;
    let promise = f.call2(&internals, &cmd.into(), &js_sys::Object::new()).map_err(|_| broken())?;
    let promise: js_sys::Promise = promise.dyn_into().map_err(|_| broken())?;
    JsFuture::from(promise).await.map_err(|e| e.as_string().unwrap_or_else(broken))
}

async fn fetch() -> Option<Info> {
    let value = call("update_status").await.ok()?;
    let json = js_sys::JSON::stringify(&value).ok()?.as_string()?;
    serde_json::from_str(&json).ok()
}

impl Updater {
    /// Démarre la veille (appli de bureau seulement) ; sans elle, l'état reste vide.
    pub fn start() -> Self {
        let u = Updater {
            info: RwSignal::new(None),
            open: RwSignal::new(false),
            busy: RwSignal::new(false),
            restarting: RwSignal::new(false),
            confirm: RwSignal::new(false),
            error: RwSignal::new(None),
        };
        if voice::desktop() {
            spawn_local(async move {
                loop {
                    if let Some(info) = fetch().await
                        && u.info.try_set(Some(info)).is_some()
                    {
                        break;
                    }
                    TimeoutFuture::new(EVERY_MS).await;
                }
            });
        }
        u
    }

    fn available(self) -> Option<String> {
        self.info.with(|i| i.as_ref().and_then(|i| i.available.clone()))
    }

    fn download(self) -> bool {
        self.info.with(|i| i.as_ref().is_some_and(|i| i.mode.as_deref() == Some("download")))
    }

    /// Lance l'installation (ou ouvre le téléchargement) et suit la progression.
    fn install(self) {
        self.error.set(None);
        self.confirm.set(false);
        self.busy.set(true);
        spawn_local(async move {
            let installing = !self.download();
            if installing {
                spawn_local(async move {
                    while self.busy.get_untracked() {
                        TimeoutFuture::new(POLL_MS).await;
                        if let Some(info) = fetch().await {
                            self.info.try_set(Some(info));
                        }
                    }
                });
            }
            match call("update_install").await {
                // L'appli redémarre : la page est remplacée.
                Ok(_) if installing => self.restarting.set(true),
                Ok(_) => {
                    self.busy.set(false);
                    self.open.set(false);
                }
                Err(e) => {
                    self.busy.set(false);
                    self.error.set(Some(e));
                    // Le mode a pu changer (signature refusée : téléchargement à la main).
                    if let Some(info) = fetch().await {
                        self.info.set(Some(info));
                    }
                }
            }
        });
    }
}

/// La tuile du rail et sa carte, au-dessus de « Mon compte ».
#[component]
pub fn UpdateTile() -> impl IntoView {
    let u = expect_context::<Updater>();
    let s = store();
    let button = NodeRef::<leptos::html::Button>::new();
    let wrap = NodeRef::<leptos::html::Div>::new();
    let card = NodeRef::<leptos::html::Div>::new();
    let close = move |refocus: bool| {
        u.open.set(false);
        u.confirm.set(false);
        if refocus && let Some(b) = button.get_untracked() {
            let _ = b.focus();
        }
    };
    // À l'ouverture, le focus va sur le premier lien ou bouton de la carte.
    Effect::new(move || {
        if u.open.get()
            && let Some(first) = card.get().and_then(|c| c.query_selector("a, button:not([disabled])").ok().flatten())
            && let Ok(h) = first.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = h.focus();
        }
    });
    let esc = window_event_listener(leptos::ev::keydown, move |e| {
        if e.key() == "Escape" && u.open.get_untracked() {
            e.stop_propagation();
            close(true);
        }
    });
    let outside = window_event_listener(leptos::ev::pointerdown, move |e| {
        if !u.open.get_untracked() {
            return;
        }
        let inside = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
            .zip(wrap.get_untracked())
            .is_some_and(|(t, w)| w.contains(Some(&t)));
        if !inside {
            close(false);
        }
    });
    on_cleanup(move || {
        esc.remove();
        outside.remove();
    });
    // Le focus qui quitte la carte (Tab après le dernier bouton) la referme.
    let on_focusout = move |e: leptos::ev::FocusEvent| {
        let next = e.related_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
        let stays = next.clone().zip(wrap.get_untracked()).is_some_and(|(n, w)| w.contains(Some(&n)));
        if next.is_some() && !stays {
            close(false);
        }
    };
    let in_call = move || !matches!(s.voice.status.get(), Status::Off);
    // Le redémarrage ne coupe l'appel que si l'on installe dans l'appli.
    let warn = move || in_call() && !u.download();
    let label = move || u.available().map(|v| format!("Mise à jour v{v} disponible")).unwrap_or_default();
    let phase = move || {
        if u.restarting.get() {
            return "restarting".to_string();
        }
        match u.info.with(|i| i.as_ref().map(|i| i.phase.clone())) {
            Some(p) if p != "idle" => p,
            // Lancé, avant la première lecture de la progression.
            _ => "downloading".to_string(),
        }
    };
    let percent = move || match phase().as_str() {
        "downloading" => u.info.with(|i| i.as_ref().and_then(Info::percent)),
        _ => Some(100),
    };
    let progress_text = move || match phase().as_str() {
        "downloading" => match percent() {
            Some(p) => format!("Téléchargement… {p} %"),
            None => "Téléchargement…".to_string(),
        },
        "installing" => "Installation…".to_string(),
        _ => "Redémarrage…".to_string(),
    };
    let primary = move || {
        if u.download() {
            "Télécharger"
        } else if u.confirm.get() {
            "Couper l'appel et installer"
        } else if u.error.with(|e| e.is_some()) {
            "Réessayer"
        } else {
            "Installer et redémarrer"
        }
    };
    let go = move |_| {
        if warn() && !u.confirm.get_untracked() {
            u.confirm.set(true);
        } else {
            if warn() {
                s.voice.hang_up();
            }
            u.install();
        }
    };
    view! {
        <Show when=move || u.available().is_some()>
            <div class="app-update" node_ref=wrap on:focusout=on_focusout>
                <button type="button" class="k-server app-update-tile" node_ref=button aria-label=label title=label
                    aria-expanded=move || u.open.get().to_string() aria-controls=move || u.open.get().then_some("update-card")
                    on:click=move |_| u.open.update(|o| *o = !*o)>
                    <Icon icon=lu::LuDownload/>
                </button>
                <Show when=move || u.open.get()>
                    <div class="k-mecard app-update-card" id="update-card" role="group" aria-label="Mise à jour" node_ref=card>
                        <p class="app-update-title">{move || format!("KRAKEN v{} est disponible", u.available().unwrap_or_default())}</p>
                        {move || u.info.with(|i| i.as_ref().and_then(|i| i.notes_url.clone())).map(|url| view! {
                            <a class="k-btn k-btn--link app-update-link" href=url target="_blank" rel="noopener noreferrer">"Nouveautés"</a>
                        })}
                        <Show when=warn>
                            <p class="app-update-warn" role="alert">"Tu es en appel : le redémarrage va le couper."</p>
                        </Show>
                        <Show when=move || u.busy.get() && !u.download()>
                            <div class="app-update-progress">
                                <div class="app-update-bar" role="progressbar" aria-label="Avancement de la mise à jour" aria-valuemin="0"
                                    aria-valuemax="100" aria-valuenow=move || percent().unwrap_or(0).to_string()>
                                    <span style=move || format!("width: {}%", percent().unwrap_or(0))></span>
                                </div>
                                <p role="status">{progress_text}</p>
                            </div>
                        </Show>
                        {move || u.error.get().map(|e| view! { <p class="k-error app-update-note" role="alert">{e}</p> })}
                        <div class="app-update-actions">
                            <button type="button" class="k-btn k-btn--ghost" disabled=move || u.busy.get() on:click=move |_| close(true)>"Plus tard"</button>
                            <button type="button" class="k-btn k-btn--primary" disabled=move || u.busy.get() on:click=go>{primary}</button>
                        </div>
                    </div>
                </Show>
            </div>
        </Show>
    }
}

/// L'entrée « Mettre à jour vers vX.Y.Z » de la carte « Mon compte » ; `me_open` est l'ouverture de cette carte, refermée au clic.
#[component]
pub fn UpdateEntry(me_open: RwSignal<bool>) -> impl IntoView {
    let u = use_context::<Updater>();
    view! {
        {move || u.and_then(|u| u.available().map(|v| view! {
            <button type="button" class="k-mecard-item" on:click=move |_| { me_open.set(false); u.open.set(true); }>
                <Icon icon=lu::LuDownload/>{format!("Mettre à jour vers v{v}")}
            </button>
        }))}
    }
}
