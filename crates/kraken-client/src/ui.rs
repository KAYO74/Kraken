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

/// Glyphe KRAKEN (tête de kraken stylisée) en `currentColor`, pour le bouton d'accueil du rail.
/// Source et version carrée colorée : `replica/brand/logo.svg`.
#[component]
pub fn Logo() -> impl IntoView {
    view! { <svg class="app-logo" viewBox="88 72 336 384" aria-hidden="true" inner_html=LOGO_GLYPH></svg> }
}

const LOGO_GLYPH: &str = r##"<mask id="kraken-eyes"><rect width="512" height="512" fill="#fff"/><ellipse cx="206" cy="238" rx="22" ry="30" transform="rotate(-24 206 238)"/><ellipse cx="306" cy="238" rx="22" ry="30" transform="rotate(24 306 238)"/></mask><g fill="none" stroke="currentColor" stroke-width="30" stroke-linecap="round" stroke-linejoin="round"><path d="M176 318c-26 34-62 44-66 82-3 28 24 40 44 24"/><path d="M218 332c4 44 24 64 12 96-8 20-36 20-40 0"/><path d="M294 332c-4 44-24 64-12 96 8 20 36 20 40 0"/><path d="M336 318c26 34 62 44 66 82 3 28-24 40-44 24"/></g><path fill="currentColor" mask="url(#kraken-eyes)" d="M256 86c-82 0-136 58-136 138 0 52 24 92 60 112h152c36-20 60-60 60-112 0-80-54-138-136-138z"/>"##;
