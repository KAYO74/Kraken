//! Petits composants partagés : avatar, écrans à venir, page introuvable, dates.

use kraken_proto::{Status, User};
use leptos::prelude::*;

use crate::icons::{Icon, lu};
use crate::store::store;

/// Deux lettres pour un nom : initiales des deux premiers mots, sinon deux premières lettres.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let s: String = match words.as_slice() {
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
        [a] => a.chars().take(2).collect(),
        [] => "?".into(),
    };
    s.to_uppercase()
}

pub fn status_label(s: Status) -> &'static str {
    match s {
        Status::Online => "en ligne",
        Status::Idle => "absent",
        Status::Dnd => "ne pas déranger",
        Status::Offline => "hors ligne",
    }
}

#[component]
pub fn Avatar(
    user: User,
    #[prop(optional)] size: &'static str,
    #[prop(optional)] status: Option<Status>,
) -> impl IntoView {
    let class = if size.is_empty() { "k-avatar".to_string() } else { format!("k-avatar k-avatar--{size}") };
    let label = match status {
        Some(s) => format!("{}, {}", user.name(), status_label(s)),
        None => user.name().to_string(),
    };
    let presence = status.map(|s| {
        let k = match s {
            Status::Online => "online",
            Status::Idle => "idle",
            Status::Dnd => "dnd",
            Status::Offline => "offline",
        };
        view! { <i class=format!("k-presence k-presence--{k}")></i> }
    });
    view! { <span class=class role="img" aria-label=label>{initials(user.name())}{presence}</span> }
}

/// En-tête de la zone principale, avec le bouton du tiroir de navigation en mobile.
#[component]
pub fn Header(children: Children) -> impl IntoView {
    let s = store();
    view! {
        <header class="app-header">
            <button class="k-btn k-btn--icon app-menu" aria-label="Ouvrir la navigation" on:click=move |_| s.drawer.set(true)>
                <Icon icon=lu::LuMenu/>
            </button>
            {children()}
        </header>
    }
}

/// Écran de l'inventaire pas encore construit : la route existe, le contenu viendra.
#[component]
pub fn Stub(id: &'static str, title: &'static str) -> impl IntoView {
    view! {
        <Header><h1 class="app-title">{title}</h1></Header>
        <div class="k-empty app-fill">
            <b>{title}</b>
            <span>{format!("Écran {id} prévu, pas encore construit.")}</span>
            <a class="k-btn" href="/channels/@me">"Retour à l'accueil"</a>
        </div>
    }
}

#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <main class="app-center">
            <div class="k-empty">
                <b>"Cette page n'existe pas"</b>
                <span>"Le lien est peut-être ancien ou mal copié."</span>
                <a class="k-btn k-btn--primary" href="/channels/@me">"Retour à l'accueil"</a>
            </div>
        </main>
    }
}

/// Heure locale `HH:MM`, précédée de la date si ce n'est pas aujourd'hui.
pub fn when(ms: i64) -> String {
    let d = js_sys::Date::new(&(ms as f64).into());
    let now = js_sys::Date::new_0();
    let hm = format!("{:02}:{:02}", d.get_hours(), d.get_minutes());
    if d.to_date_string() == now.to_date_string() {
        hm
    } else {
        format!("{:02}/{:02}/{} {hm}", d.get_date(), d.get_month() + 1, d.get_full_year())
    }
}

pub fn iso(ms: i64) -> String {
    js_sys::Date::new(&(ms as f64).into()).to_iso_string().into()
}
