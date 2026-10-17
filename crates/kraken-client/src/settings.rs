//! Réglages utilisateur (S06 à S15) dans la modale `k-settings`. Compte et Apparence sont
//! branchés ; les autres sections ont leur route et leur place dans la navigation.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::icons::{Icon, lu};
use crate::store::store;
use crate::ui::Avatar;

const SECTIONS: &[(&str, &str, &str, &str)] = &[
    ("Utilisateur", "account", "Mon compte", "S06"),
    ("Utilisateur", "profiles", "Profils", "S20"),
    ("Utilisateur", "privacy", "Données et confidentialité", "S07"),
    ("Utilisateur", "messaging", "Messagerie et contacts", "S08"),
    ("Utilisateur", "apps", "Applications connectées", "S15"),
    ("Appli", "notifications", "Notifications", "S09"),
    ("Appli", "voice", "Voix et vidéo", "S10"),
    ("Appli", "appearance", "Apparence", "S11"),
    ("Appli", "accessibility", "Accessibilité", "S12"),
    ("Appli", "keybinds", "Raccourcis clavier", "S13"),
    ("Appli", "language", "Langue et heure", "S14"),
];

const THEME_KEY: &str = "kraken.theme";
const THEMES: &[(&str, &str, &str)] = &[
    ("dark", "Sombre", "Le thème par défaut, doux pour les yeux le soir."),
    ("light", "Clair", "Fond blanc, pour les pièces lumineuses."),
    ("black", "Noir", "Noir profond, idéal sur les écrans OLED."),
    ("auto", "Automatique", "Suit le réglage clair ou sombre de ton système."),
];

fn set_theme(t: &str) {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = el.set_attribute("data-theme", t);
    }
}

fn saved_theme() -> String {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(THEME_KEY).ok().flatten())
        .unwrap_or_else(|| "dark".into())
}

pub fn apply_saved_theme() {
    set_theme(&saved_theme());
}

#[component]
pub fn Settings() -> impl IntoView {
    let params = use_params_map();
    let section = move || params.read().get("section").unwrap_or_default();
    let navigate = use_navigate();
    let close = move || navigate("/channels/@me", Default::default());
    let close2 = close.clone();
    let mut group = "";
    let nav = SECTIONS.iter().map(|(g, slug, label, _)| {
        let head = (*g != group).then(|| view! { <h3>{*g}</h3> });
        group = g;
        view! {
            {head}
            <a class="k-navitem" href=format!("/settings/{slug}") aria-current=move || (section() == *slug).then_some("page")>{*label}</a>
        }
    }).collect_view();
    view! {
        <div class="k-scrim app-settings" on:keydown=move |e| if e.key() == "Escape" { close() }>
            <div class="k-settings" role="dialog" aria-modal="true" aria-label="Réglages">
                <nav aria-label="Sections des réglages">{nav}</nav>
                <main>
                    <div class="app-settings-top">
                        <h2>{move || SECTIONS.iter().find(|s| s.1 == section()).map(|s| s.2).unwrap_or("Réglages")}</h2>
                        <button class="k-btn k-btn--icon" aria-label="Fermer les réglages (Échap)" on:click=move |_| close2() autofocus>
                            <Icon icon=lu::LuX/>
                        </button>
                    </div>
                    {move || match section().as_str() {
                        "account" => view! { <Account/> }.into_any(),
                        "appearance" => view! { <Appearance/> }.into_any(),
                        other => {
                            let id = SECTIONS.iter().find(|s| s.1 == other).map(|s| s.3).unwrap_or("—");
                            view! { <div class="k-empty"><b>"Bientôt ici"</b>{format!("Section {id} prévue, pas encore construite.")}</div> }.into_any()
                        }
                    }}
                </main>
            </div>
        </div>
    }
}

#[component]
fn Account() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let deleting = RwSignal::new(false);
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let leave = move |all: bool| {
        let navigate = navigate.clone();
        spawn_local(async move {
            let _ = if all { crate::api::logout_all().await } else { crate::api::logout().await };
            s.sign_out();
            navigate("/login", Default::default());
        });
    };
    let leave2 = leave.clone();
    let display = RwSignal::new(String::new());
    // Suit le compte chargé (le READY peut arriver après l'ouverture des réglages).
    Effect::new(move || {
        display.set(s.me.with(|m| m.as_ref().and_then(|u| u.display_name.clone())).unwrap_or_default())
    });
    let saved = RwSignal::new(false);
    let name_error = RwSignal::new(None::<String>);
    let save_name = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        saved.set(false);
        name_error.set(None);
        spawn_local(async move {
            match crate::api::update_me(&display.get_untracked()).await {
                Ok(u) => {
                    display.set(u.display_name.clone().unwrap_or_default());
                    s.update_user(u);
                    saved.set(true);
                }
                Err(e) => name_error.set(Some(e.message)),
            }
        });
    };
    let username = move || s.me.get().map(|u| u.username).unwrap_or_default();
    let delete = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        spawn_local(async move {
            match crate::api::delete_account(&password.get_untracked()).await {
                Ok(()) => {
                    s.sign_out();
                    let _ = web_sys::window().unwrap().location().set_href("/login");
                }
                Err(e) => error.set(Some(e.message)),
            }
        });
    };
    view! {
        {move || s.me.get().map(|u| view! {
            <div class="k-card app-account">
                <header>
                    <Avatar user=u.clone() size="lg"/>
                    <div><b class="text-strong">{u.name().to_string()}</b><div class="type-sm text-muted">{format!("@{}", u.username)}</div></div>
                </header>
            </div>
        })}
        <form class="k-setting app-name-form" on:submit=save_name>
            <label for="display-name">"Nom affiché"</label>
            <p id="display-name-h">{move || format!("Ce que les autres voient. Ton pseudo unique @{} ne change pas : il reste écrit en dessous.", username())}</p>
            <button class="k-btn k-btn--primary" type="submit">"Enregistrer"</button>
            <input id="display-name" class="k-input" maxlength=kraken_proto::MAX_DISPLAY_NAME.to_string() placeholder=username bind:value=display
                aria-describedby="display-name-h" aria-invalid=move || name_error.get().is_some().then_some("true")/>
            <Show when=move || name_error.get().is_some()><p class="k-error" role="alert">{move || name_error.get()}</p></Show>
            <Show when=move || saved.get()><p role="status">"Nom affiché enregistré."</p></Show>
        </form>
        <div class="k-setting">
            <label for="logout">"Se déconnecter"</label>
            <button id="logout" class="k-btn" on:click=move |_| leave(false)>"Déconnexion"</button>
            <p>"Ferme la session sur cet appareil."</p>
        </div>
        <div class="k-setting">
            <label for="logout-all">"Se déconnecter partout"</label>
            <button id="logout-all" class="k-btn" on:click=move |_| leave2(true)>"Tous les appareils"</button>
            <p>"Ferme toutes tes sessions, y compris celle-ci. Utile après la perte d'un appareil."</p>
        </div>
        <div class="k-setting">
            <label for="delete">"Supprimer le compte"</label>
            <button id="delete" class="k-btn k-btn--danger" on:click=move |_| deleting.set(true)>"Supprimer"</button>
            <p>"Ton compte est fermé tout de suite et effacé au bout de 14 jours. Tes messages restent, signés « compte supprimé »."</p>
        </div>
        <Show when=move || deleting.get()>
            <div class="k-scrim" on:keydown=move |e| if e.key() == "Escape" { deleting.set(false) }>
                <div class="k-dialog" role="alertdialog" aria-modal="true" aria-labelledby="del-t">
                    <header><h2 id="del-t">"Supprimer ton compte ?"</h2></header>
                    <form on:submit=delete>
                        <div>
                            <p>"Tu ne pourras plus te connecter. Confirme avec ton mot de passe."</p>
                            <label class="k-field"><span class="k-label">"Mot de passe"</span>
                                <input class="k-input" type="password" autocomplete="current-password" required bind:value=password/></label>
                            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                        </div>
                        <footer>
                            <button type="button" class="k-btn k-btn--ghost" on:click=move |_| deleting.set(false) autofocus>"Annuler"</button>
                            <button type="submit" class="k-btn k-btn--danger">"Supprimer définitivement"</button>
                        </footer>
                    </form>
                </div>
            </div>
        </Show>
    }
}

#[component]
fn Appearance() -> impl IntoView {
    let current = RwSignal::new(saved_theme());
    let pick = move |t: &'static str| {
        current.set(t.into());
        set_theme(t);
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(THEME_KEY, t);
        }
    };
    view! {
        <fieldset class="k-radios">
            <legend class="k-label app-legend">"Thème"</legend>
            {THEMES.iter().map(|(t, label, sub)| view! {
                <label class="k-radio">
                    <input type="radio" name="theme" prop:checked=move || current.get() == *t on:change=move |_| pick(t)/>
                    <div><b>{*label}</b><span>{*sub}</span></div>
                </label>
            }).collect_view()}
        </fieldset>
    }
}
