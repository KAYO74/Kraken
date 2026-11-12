//! Petits composants partagés : avatar, écrans à venir, page introuvable, dates.

use std::cell::RefCell;

use kraken_proto::{Status, User};
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::icons::{Icon, lu};
use crate::photo::Photo;
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

/// Ce qu'une pastille de statut montre : les statuts que les autres voient, et « invisible », que
/// seul son propriétaire voit sur lui-même.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Online,
    Idle,
    Dnd,
    Offline,
    Invisible,
}

impl From<Status> for Dot {
    fn from(s: Status) -> Self {
        match s {
            Status::Online => Self::Online,
            Status::Idle => Self::Idle,
            Status::Dnd => Self::Dnd,
            Status::Offline => Self::Offline,
        }
    }
}

impl Dot {
    /// Le mot qui le dit, pour les lecteurs d'écran et sous les noms.
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "en ligne",
            Self::Idle => "inactif",
            Self::Dnd => "ne pas déranger",
            Self::Offline => "hors ligne",
            Self::Invisible => "invisible",
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::Dnd => "dnd",
            Self::Offline => "offline",
            Self::Invisible => "invisible",
        }
    }
}

/// Seule la forme de la pastille (cercle plein, croissant, barre, anneau vide, anneau pointé)
/// distingue les états en plus de la couleur ; le mot est dans le nom accessible de l'avatar.
#[component]
pub fn StatusDot(#[prop(into)] status: Signal<Dot>) -> impl IntoView {
    view! { <i class=move || format!("k-presence k-presence--{}", status.get().class()) aria-hidden="true"></i> }
}

/// Une teinte stable par compte (1 à 8, somme des octets de l'id), pour reconnaître les gens d'un
/// coup d'œil : l'avatar sans photo, la bannière sans image.
pub fn tint(id: kraken_proto::Id) -> String {
    (id.as_bytes().iter().map(|&b| u32::from(b)).sum::<u32>() % 8 + 1).to_string()
}

#[component]
pub fn Avatar(
    user: User,
    #[prop(optional)] size: &'static str,
    #[prop(optional, into)] status: Option<Signal<Dot>>,
) -> impl IntoView {
    let class = if size.is_empty() { "k-avatar".to_string() } else { format!("k-avatar k-avatar--{size}") };
    let name = user.name().to_string();
    let label = move || match status {
        Some(s) => format!("{name}, {}", s.get().label()),
        None => name.clone(),
    };
    let presence = status.map(|s| view! { <StatusDot status=s/> });
    let tint = tint(user.id);
    // La photo remplace les initiales ; si elle ne charge pas, les initiales reviennent. Le
    // `<span>` porte déjà le nom (role="img"), l'image est donc décorative.
    let broken = RwSignal::new(false);
    let initials = initials(user.name());
    let content = {
        let photo = user.avatar.clone();
        move || match &photo {
            Some(key) if !broken.get() => view! {
                <Photo src=avatar_url(key) cap=256 on_error=move |()| broken.set(true)/>
            }
            .into_any(),
            _ => initials.clone().into_any(),
        }
    };
    view! { <span class=class data-tint=tint role="img" aria-label=label>{content}{presence}</span> }
}

/// Adresse d'une photo de profil, d'après la clé gardée dans `User::avatar`.
pub fn avatar_url(key: &str) -> String {
    format!("/api/v1/avatars/{key}")
}

/// Adresse d'une bannière, d'après la clé gardée dans `User::banner`.
pub fn banner_url(key: &str) -> String {
    format!("/api/v1/banners/{key}")
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

/// Avant d'ouvrir une fenêtre depuis une entrée d'un menu « Plus d'actions » : le focus passe au
/// bouton du menu, pour qu'il y revienne à la fermeture (l'entrée, elle, disparaît avec le menu).
pub fn focus_menu_trigger(e: &leptos::ev::MouseEvent) {
    let button = e
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.closest(".app-more").ok().flatten())
        .and_then(|more| more.query_selector("button").ok().flatten())
        .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(b) = button {
        let _ = b.focus();
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

/// L'heure locale de `ms`, au format choisi dans Réglages › Langue et heure : « 14:30 » en 24 h,
/// « 2:30 PM » en 12 h, et d'après la langue du navigateur en « Automatique ». C'est le seul endroit
/// qui écrit une heure : messages, « Exclu jusqu'à », sourdines, dates des invitations et des bans.
pub fn clock(ms: i64) -> String {
    let d = js_sys::Date::new(&(ms as f64).into());
    let (h, m) = (d.get_hours(), d.get_minutes());
    match crate::prefs::get(&crate::prefs::CLOCK) {
        "24" => format!("{h:02}:{m:02}"),
        "12" => format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" }),
        _ => automatic_clock(&d).unwrap_or_else(|| format!("{h:02}:{m:02}")),
    }
}

/// L'heure selon la langue du navigateur (« 14:30 » en France, « 2:30 PM » aux États-Unis).
fn automatic_clock(d: &js_sys::Date) -> Option<String> {
    let locale = web_sys::window()?.navigator().language()?;
    if locale.len() < 2 || !locale.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let options = js_sys::Object::new();
    for (key, value) in [("hour", "numeric"), ("minute", "2-digit")] {
        js_sys::Reflect::set(&options, &key.into(), &value.into()).ok()?;
    }
    Some(d.to_locale_string(&locale, &options).into())
}

/// L'heure ([`clock`]), précédée de la date si ce n'est pas aujourd'hui.
pub fn when(ms: i64) -> String {
    let d = js_sys::Date::new(&(ms as f64).into());
    let now = js_sys::Date::new_0();
    let hm = clock(ms);
    if d.to_date_string() == now.to_date_string() {
        hm
    } else {
        format!("{:02}/{:02}/{} {hm}", d.get_date(), d.get_month() + 1, d.get_full_year())
    }
}

/// Maintenant, en millisecondes Unix.
pub fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

/// Combien de temps il reste jusqu'à `until`, en une expression courte : « 2 j », « 3 h »,
/// « 12 min », « moins d'une minute ». Arrondi vers le haut, pour ne jamais annoncer « 0 ».
pub fn left(until: i64, now: i64) -> String {
    let secs = ((until - now).max(0) as u64).div_ceil(1000);
    let (mins, hours, days) = (secs.div_ceil(60), secs.div_ceil(3600), secs.div_ceil(86_400));
    if secs < 60 {
        "moins d'une minute".into()
    } else if mins < 60 {
        format!("{mins} min")
    } else if hours < 24 {
        format!("{hours} h")
    } else {
        format!("{days} j")
    }
}

/// `until` tant qu'il est dans le futur ; devient `None` à l'heure dite, sans que rien d'autre
/// ne change (un seul minuteur, pas de compte à rebours). Une durée plus longue que ce qu'un
/// minuteur du navigateur sait attendre est découpée en plusieurs attentes.
pub fn still(until: impl Fn() -> Option<i64> + Copy + Send + Sync + 'static) -> Signal<Option<i64>> {
    use gloo_timers::future::TimeoutFuture;
    let tick = RwSignal::new(0u32);
    Effect::new(move |_| {
        tick.track();
        if let Some(t) = until() {
            let left = t - now_ms();
            if left > 0 {
                let wait = left.min(i64::from(i32::MAX) - 1000) as u32;
                leptos::task::spawn_local(async move {
                    TimeoutFuture::new(wait + 50).await;
                    tick.try_update(|n| *n += 1);
                });
            }
        }
    });
    Signal::derive(move || {
        tick.track();
        until().filter(|t| *t > now_ms())
    })
}

/// Copie `text` dans le presse-papiers. L'API moderne si la page le permet, sinon (instance en
/// HTTP sur le réseau local, ou refus du navigateur) la sélection d'un champ caché. Vrai si c'est fait.
pub async fn copy_text(text: &str) -> bool {
    use wasm_bindgen_futures::JsFuture;
    let Some(w) = web_sys::window() else { return false };
    if w.is_secure_context() && JsFuture::from(w.navigator().clipboard().write_text(text)).await.is_ok() {
        return true;
    }
    let Some(doc) = w.document() else { return false };
    let Some(body) = doc.body() else { return false };
    let Ok(el) = doc.create_element("textarea") else { return false };
    let Ok(area) = el.dyn_into::<web_sys::HtmlTextAreaElement>() else { return false };
    area.set_value(text);
    let _ = area.set_attribute("readonly", "");
    let _ = area.set_attribute("aria-hidden", "true");
    let _ = area.set_attribute("tabindex", "-1");
    area.set_class_name("app-copy-buffer");
    let _ = body.append_child(&area);
    area.select();
    let done = doc.dyn_into::<web_sys::HtmlDocument>().ok().and_then(|d| d.exec_command("copy").ok()).unwrap_or(false);
    area.remove();
    done
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
