//! Sélecteur d'emojis : la sélection Unicode de `kraken_core::emoji`, dessinée par la police
//! du système (aucune image). Recherche par nom, Échap ou clic à côté pour fermer.

use kraken_core::emoji::EMOJIS;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::ui::modal;

#[component]
pub fn EmojiPicker(
    /// Nom accessible du sélecteur.
    title: &'static str,
    on_pick: Callback<String>,
    on_close: Callback<()>,
) -> impl IntoView {
    let root = NodeRef::<html::Div>::new();
    modal(root, move || on_close.run(()));
    let outside = window_event_listener(leptos::ev::pointerdown, move |e| {
        let Some(r) = root.get_untracked() else { return };
        let inside = e.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok()).is_some_and(|t| r.contains(Some(&t)));
        if !inside {
            on_close.run(());
        }
    });
    on_cleanup(move || outside.remove());
    let q = RwSignal::new(String::new());
    let found = move || {
        let q = q.get().trim().to_lowercase();
        EMOJIS.iter().filter(|(e, n)| q.is_empty() || n.contains(&q) || *e == q).copied().collect::<Vec<_>>()
    };
    view! {
        <div class="k-menu app-picker" role="dialog" aria-label=title node_ref=root>
            <input class="k-input" type="search" placeholder="Chercher un emoji" aria-label="Chercher un emoji" autofocus bind:value=q
                on:keydown=move |e| if e.key() == "Enter" {
                    e.prevent_default();
                    if let Some((emoji, _)) = found().first() { on_pick.run(emoji.to_string()) }
                }/>
            <div class="app-picker-grid" role="group" aria-label="Emojis">
                {move || found().into_iter().map(|(emoji, name)| view! {
                    <button type="button" class="app-picker-emoji" aria-label=name title=name on:click=move |_| on_pick.run(emoji.to_string())>{emoji}</button>
                }).collect_view()}
            </div>
            <Show when=move || found().is_empty()><p class="app-hint">"Aucun emoji de la sélection ne correspond."</p></Show>
        </div>
    }
}
