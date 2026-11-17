//! Messages épinglés : épingler ou désépingler (bouton de la barre d'outils d'un message, bouton du panneau) et le
//! panneau « Messages épinglés » d'un salon. Une colonne à côté du fil (comme les membres) ; sous 640 px, une feuille
//! pleine page montée dans un `Portal`, avec sa fermeture visible. Le panneau charge la liste à l'ouverture
//! (`GET /channels/{id}/pins`) puis la suit par le gateway : un message désépinglé, supprimé ou modifié y est mis à jour.

use gloo_timers::future::TimeoutFuture;
use kraken_proto::{Id, MAX_PINS, Message};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::i18n::{t, tf};
use crate::icons::{Icon, lu};
use crate::store::{Conn, PinList, Store, store};
use crate::ui::{iso, trap_tab, when};

/// Combien de temps la confirmation reste dans la zone `status` de la coquille.
const SAID_MS: u32 = 6_000;

/// Dit une chose dans la zone `status` de la coquille, puis la retire si personne ne l'a remplacée.
fn say(s: Store, text: String) {
    s.notice.set(Some(text.clone()));
    spawn_local(async move {
        TimeoutFuture::new(SAID_MS).await;
        if s.notice.try_get_untracked().flatten().as_deref() == Some(text.as_str()) {
            s.notice.try_set(None);
        }
    });
}

/// Épingle (`on`) ou désépingle ce message : le fil et la liste du panneau suivent, la zone `status` le dit. En cas
/// d'échec (droit retiré, 50 messages déjà épinglés…), rend le message d'erreur du serveur. Le `Store` est pris par
/// l'appelant : une tâche asynchrone n'a plus de contexte réactif.
pub async fn set_pinned(s: Store, channel: Id, id: Id, on: bool) -> Result<(), String> {
    match crate::api::set_pin(channel, id, on).await {
        Ok(m) => {
            s.log_if_open(channel, |l| l.update(m.clone()));
            s.pin_changed(&m);
            say(s, if on { t("Message épinglé.") } else { t("Message désépinglé.") });
            Ok(())
        }
        Err(e) => Err(e.message),
    }
}

/// Le texte d'un message épinglé, en une ligne de texte simple.
fn excerpt(m: &Message) -> String {
    let text = kraken_core::markdown::summary(&m.content);
    if text.trim().is_empty() { t("Fichier joint") } else { text }
}

fn focus(el: Option<web_sys::Element>) {
    if let Some(h) = el.and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
        let _ = h.focus();
    }
}

/// Le panneau des messages épinglés de ce salon. `sheet` : feuille pleine page (petit écran), qui est une boîte
/// de dialogue ; sinon une colonne du fil. `can_manage` : désépingler. `close` ferme le panneau (et rend le focus à
/// son bouton) ; `goto` mène au message (et ferme la feuille).
#[component]
pub fn PinsPanel(
    channel: Id,
    sheet: bool,
    #[prop(into)] can_manage: Signal<bool>,
    close: Callback<()>,
    goto: Callback<Id>,
) -> impl IntoView {
    let s = store();
    // Chaque ouverture repart d'une liste à relire.
    s.pins.update(|p| {
        p.insert(channel, PinList::default());
    });
    let list = Memo::new(move |_| s.pins.with(|p| p.get(&channel).cloned().unwrap_or_default()));
    let need = Memo::new(move |_| s.conn.get() == Conn::Ready && !list.with(|l| l.loaded));
    Effect::new(move || {
        if !need.get() {
            return;
        }
        spawn_local(async move {
            let r = crate::api::pins(channel).await;
            s.pins.update(|p| {
                let l = p.entry(channel).or_default();
                l.loaded = true;
                match r {
                    Ok(items) => (l.items, l.error) = (items, None),
                    Err(e) => l.error = Some(e.message),
                }
            });
        });
    });
    // Des booléens dérivés, pas la liste : le bloc des états ne se refait que quand l'état change, pas à chaque
    // message épinglé ou retiré. Seul le `For` suit les lignes, et les autres gardent leur focus.
    let loading = Memo::new(move |_| list.with(|l| !l.loaded));
    let failure = Memo::new(move |_| list.with(|l| l.error.clone()));
    let empty = Memo::new(move |_| list.with(|l| l.items.is_empty()));
    let count = Memo::new(move |_| list.with(|l| l.items.len()));

    let root = NodeRef::<html::Div>::new();
    let title = NodeRef::<html::H2>::new();
    // Le focus arrive sur le titre à l'ouverture.
    Effect::new(move || {
        if let Some(h) = title.get() {
            let _ = h.focus();
        }
    });
    let on_key = move |e: leptos::ev::KeyboardEvent| {
        if !crate::shortcuts::is_key(&e) {
            return;
        }
        if e.key() == "Escape" {
            e.prevent_default();
            e.stop_propagation();
            close.run(());
        } else if sheet {
            trap_tab(&e, root);
        }
    };

    // Le focus était sur une ligne qui disparaît (désépinglée, supprimée) : il passe à la suivante, ou au titre.
    let on_row = StoredValue::new(None::<Id>);
    let row_at = StoredValue::new(0usize);
    Effect::new(move |prev: Option<Vec<Id>>| {
        let now: Vec<Id> = list.with(|l| l.items.iter().map(|m| m.id).collect());
        if let (Some(prev), Some(was)) = (prev.as_ref(), on_row.get_value())
            && prev.contains(&was)
            && !now.contains(&was)
        {
            on_row.set_value(None);
            let next = now.get(row_at.get_value()).or(now.last()).copied();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                let Some(panel) = root.try_get_untracked().flatten() else { return };
                // Le focus est déjà ailleurs dans le panneau : on n'y touche pas.
                if document().active_element().as_ref().is_some_and(|a| panel.contains(Some(a))) {
                    return;
                }
                let row =
                    next.and_then(|id| panel.query_selector(&format!("[data-pin=\"{id}\"] button")).ok().flatten());
                match row {
                    Some(b) => focus(Some(b)),
                    None => focus(title.try_get_untracked().flatten().map(|h| h.unchecked_into::<web_sys::Element>())),
                }
            });
        }
        now
    });

    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let unpin = move |id: Id| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            if let Err(e) = set_pinned(s, channel, id, false).await {
                error.try_set(Some(e));
            }
            busy.try_set(false);
        });
    };
    let retry = move |_| {
        s.pins.update(|p| {
            p.insert(channel, PinList::default());
        });
        let _ = title.get_untracked().map(|h| h.focus());
    };

    let body = move || {
        view! {
            <header class="app-pins-head">
                <h2 id="pins-title" tabindex="-1" node_ref=title>{t("Messages épinglés")}</h2>
                <Show when=move || !loading.get() && failure.with(Option::is_none)>
                    <span class="k-badge k-badge--neutral">{move || format!("{} / {MAX_PINS}", count.get())}</span>
                </Show>
                <button type="button" class="k-btn k-btn--icon app-pins-close" aria-label=t("Fermer les messages épinglés") title=t("Fermer")
                    on:click=move |_| close.run(())><Icon icon=lu::LuX/></button>
            </header>
            <div class="app-pins-body" aria-busy=move || loading.get().to_string()>
                // Toujours là, de hauteur réservée : le texte vient sans rien décaler.
                <p class="k-hint app-pins-status" role="status">{move || {
                    if loading.get() { t("Chargement des messages épinglés…") }
                    else if failure.with(Option::is_none) && !empty.get() { t("Du plus récemment épinglé au plus ancien.") }
                    else { String::new() }
                }}</p>
                <Show when=move || error.get().is_some()>
                    <p class="k-error" role="alert">{move || error.get()}</p>
                </Show>
                {move || {
                    if loading.get() {
                        return view! {
                            <ul class="app-pins-list" aria-hidden="true">
                                {(0..3).map(|_| view! { <li><div class="k-skeleton app-pin-skel"></div></li> }).collect_view()}
                            </ul>
                        }.into_any();
                    }
                    if let Some(e) = failure.get() {
                        return view! {
                            <div class="k-empty" role="alert">
                                <b>{t("Impossible de charger les messages épinglés")}</b>
                                <span>{e}</span>
                                <button type="button" class="k-btn" on:click=retry>{t("Réessayer")}</button>
                            </div>
                        }.into_any();
                    }
                    if empty.get() {
                        return view! {
                            <div class="k-empty">
                                <Icon icon=lu::LuPin/>
                                <b>{t("Aucun message épinglé")}</b>
                                <span>{move || if can_manage.get() {
                                    t("Épingle un message depuis ses actions pour le retrouver ici.")
                                } else {
                                    t("Les modérateurs peuvent en épingler.")
                                }}</span>
                            </div>
                        }.into_any();
                    }
                    view! {
                        <ul class="app-pins-list">
                            <For each=move || list.with(|l| l.items.clone())
                                key=|m| (m.id, m.content.clone(), m.edited_at, m.author.as_ref().map(|a| a.name().to_string()))
                                children=move |m| {
                                    let id = m.id;
                                    let who = m.author.as_ref().map(|a| a.name().to_string()).unwrap_or_else(|| t("Compte supprimé"));
                                    let (go_label, unpin_label) = (
                                        tf("Aller au message de {nom}", &[("nom", &who)]),
                                        tf("Désépingler le message de {nom}", &[("nom", &who)]),
                                    );
                                    let text = excerpt(&m);
                                    // Un auteur bloqué est replié comme dans le fil : même ligne, même bouton, dépliable au clic.
                                    let author_id = m.author.as_ref().map(|a| a.id);
                                    let blocked = Memo::new(move |_| author_id.is_some_and(|a| s.is_blocked(a)));
                                    let shown = RwSignal::new(false);
                                    view! {
                                        <li class="app-pin" data-pin=id.to_string()
                                            on:focusin=move |_| {
                                                on_row.set_value(Some(id));
                                                row_at.set_value(list.with_untracked(|l| l.items.iter().position(|x| x.id == id)).unwrap_or(0));
                                            }>
                                            <Show when=move || blocked.get()>
                                                <div class="app-blocked">
                                                    <Icon icon=lu::LuEyeOff small=true/>
                                                    <span>{t("Message d'un compte que tu as bloqué.")}</span>
                                                    <button type="button" class="k-btn k-btn--link" aria-expanded=move || shown.get().to_string()
                                                        aria-controls=format!("pin-body-{id}")
                                                        on:click=move |_| shown.update(|v| *v = !*v)>
                                                        {move || if shown.get() { t("Masquer") } else { t("Afficher") }}
                                                    </button>
                                                </div>
                                            </Show>
                                            <div id=format!("pin-body-{id}") class="app-pin-body" hidden=move || blocked.get() && !shown.get()>
                                                <div class="app-pin-head">
                                                    <b class="app-ellipsis"><bdi>{who}</bdi></b>
                                                    <time datetime=iso(m.created_at)>{when(m.created_at)}</time>
                                                </div>
                                                <p class="app-pin-text" dir="auto">{text}</p>
                                                <div class="app-pin-actions">
                                                    <button type="button" class="k-btn k-btn--sm" aria-label=go_label
                                                        on:click=move |_| goto.run(id)>{t("Aller au message")}</button>
                                                    <Show when=move || can_manage.get()>
                                                        <button type="button" class="k-btn k-btn--sm" aria-label=unpin_label.clone()
                                                            on:click=move |_| unpin(id)>{t("Désépingler")}</button>
                                                    </Show>
                                                </div>
                                            </div>
                                        </li>
                                    }
                                }/>
                        </ul>
                    }.into_any()
                }}
            </div>
        }
    };

    if sheet {
        view! {
            <div id="pins-panel" class="app-sheet" role="dialog" aria-modal="true" aria-labelledby="pins-title"
                node_ref=root on:keydown=on_key>{body()}</div>
        }
        .into_any()
    } else {
        view! {
            <aside id="pins-panel" class="app-aside app-aside--pins" aria-labelledby="pins-title" on:keydown=on_key>
                <div class="app-pins" node_ref=root>{body()}</div>
            </aside>
        }
        .into_any()
    }
}
