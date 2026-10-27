//! Petits composants partagés : avatar, écrans à venir, page introuvable, dates.

use std::cell::RefCell;

use kraken_proto::{Status, User};
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::icons::{Icon, lu};
use crate::store::store;

thread_local! {
    /// Boîtes de dialogue ouvertes, la plus haute en dernier : ce qui avait le focus avant chacune.
    static MODALS: RefCell<Vec<Option<web_sys::Element>>> = const { RefCell::new(Vec::new()) };
}

/// Comportement d'une boîte de dialogue : le focus va sur son élément `autofocus` à
/// l'ouverture (l'attribut ne joue qu'une fois par page), Échap ferme la plus haute
/// seulement, et le focus revient à son déclencheur à la fermeture.
pub fn modal(root: NodeRef<html::Div>, close: impl Fn() + 'static) {
    let depth = MODALS.with_borrow_mut(|m| {
        m.push(document().active_element());
        m.len()
    });
    Effect::new(move || {
        let target = root.get().and_then(|el| el.query_selector("[autofocus]").ok().flatten());
        if let Some(h) = target.and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = h.focus();
        }
    });
    let esc = window_event_listener(leptos::ev::keydown, move |e| {
        if e.key() == "Escape" && MODALS.with_borrow(Vec::len) == depth {
            close();
        }
    });
    on_cleanup(move || {
        esc.remove();
        let before = MODALS.with_borrow_mut(|m| {
            let b = m.get_mut(depth - 1).and_then(Option::take);
            m.truncate(depth - 1);
            b
        });
        if let Some(h) = before.and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = h.focus();
        }
    });
}

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
    // Une teinte stable par compte (bits aléatoires de l'uuidv7), pour reconnaître les gens d'un coup d'œil.
    let tint = (user.id.as_u128() % 8 + 1).to_string();
    view! { <span class=class data-tint=tint role="img" aria-label=label>{initials(user.name())}{presence}</span> }
}

/// « Plus d'actions » : un bouton ⋯ qui déplie les actions secondaires (retirer, bloquer…), pour ne
/// pas les mettre au rang des actions courantes. Choisir une action, Échap, un clic ailleurs ou le
/// focus qui sort le referment. Les enfants sont des `<button class="k-menuitem">`.
#[component]
pub fn MoreMenu(label: String, children: Children) -> impl IntoView {
    let open = RwSignal::new(false);
    let wrap = NodeRef::<html::Div>::new();
    let button = NodeRef::<html::Button>::new();
    let menu = NodeRef::<html::Div>::new();
    let inside = move |t: Option<web_sys::EventTarget>| {
        t.and_then(|t| t.dyn_into::<web_sys::Node>().ok())
            .zip(wrap.get_untracked())
            .is_some_and(|(t, w)| w.contains(Some(&t)))
    };
    Effect::new(move || {
        if open.get()
            && let Some(first) = menu.get().and_then(|m| m.query_selector("button").ok().flatten())
            && let Ok(h) = first.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = h.focus();
        }
    });
    let outside = window_event_listener(leptos::ev::pointerdown, move |e| {
        if open.get_untracked() && !inside(e.target()) {
            open.set(false);
        }
    });
    on_cleanup(move || outside.remove());
    let on_key = move |e: leptos::ev::KeyboardEvent| {
        if e.key() == "Escape" && open.get_untracked() {
            e.stop_propagation();
            open.set(false);
            if let Some(b) = button.get_untracked() {
                let _ = b.focus();
            }
        }
    };
    let on_focusout = move |e: leptos::ev::FocusEvent| {
        if e.related_target().is_some() && !inside(e.related_target()) {
            open.set(false);
        }
    };
    view! {
        <div class="app-more" node_ref=wrap on:keydown=on_key on:focusout=on_focusout>
            <button class="k-btn k-btn--icon" node_ref=button aria-label=label title="Plus"
                aria-expanded=move || open.get().to_string() on:click=move |_| open.update(|o| *o = !*o)>
                <Icon icon=lu::LuEllipsis/>
            </button>
            <div class="k-menu app-more-menu" node_ref=menu hidden=move || !open.get() on:click=move |_| open.set(false)>
                {children()}
            </div>
        </div>
    }
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

/// Logo KRAKEN pour le bouton d'accueil du rail : l'icône carrée, arrondie par le bouton.
/// Source et tailles : `replica/brand/` (`python3 replica/brand/icons.py`).
#[component]
pub fn Logo() -> impl IntoView {
    view! { <img class="app-logo" src="/icon-128.png" alt="" width="128" height="128"/> }
}
