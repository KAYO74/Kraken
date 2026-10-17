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
    let logout = move |_| {
        let navigate = navigate.clone();
        spawn_local(async move {
            let _ = crate::api::logout().await;
            s.sign_out();
            navigate("/login", Default::default());
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
        <div class="k-setting">
            <label for="logout">"Se déconnecter"</label>
            <button id="logout" class="k-btn k-btn--danger" on:click=logout>"Déconnexion"</button>
            <p>"Ferme la session sur cet appareil."</p>
        </div>
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
