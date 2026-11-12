//! Emojis. Le sélecteur propose d'abord les emojis perso des serveurs dont on est membre (celui du salon
//! en tête), puis la sélection Unicode de `kraken_core::emoji`, dessinée par la police du système.
//! Recherche par nom, Échap ou clic à côté pour fermer.
//!
//! Un emoji perso est une image (`EmojiImg`), servie à toute session ouverte : un message qui le cite doit
//! s'afficher chez tous ceux qui le lisent, membres du serveur ou non. S'il n'existe plus, l'image ne se
//! charge pas et on montre son nom, `:nom:`. Une animation reste fixe dans les listes et s'anime au survol
//! ou au focus de sa ligne, comme les photos de profil (voir `photo.rs`).

use kraken_core::emoji::EMOJIS;
use kraken_proto::{Emoji, Guild, Id};
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::photo::Photo;
use crate::store::store;
use crate::ui::{initials, modal};

/// Ce que l'on a choisi dans le sélecteur.
#[derive(Clone, PartialEq)]
pub enum Picked {
    Unicode(String),
    Custom(Emoji),
}

/// `:nom:` : la façon de désigner un emoji dans la saisie.
pub fn shortcode(name: &str) -> String {
    format!(":{name}:")
}

/// Ce qu'on écrit dans la saisie pour cet emoji, quand on écrit dans `current` : `:nom:` si ce nom y
/// désigne bien lui (c'est ce que l'envoi remplacera), sinon le jeton complet.
pub fn written(e: &Emoji, current: Option<Id>) -> String {
    match store().emoji_named(&e.name, current) {
        Some(found) if found.id == e.id => shortcode(&e.name),
        _ => kraken_core::emoji::token(&e.name, e.id),
    }
}

/// À l'envoi : chaque `:nom:` qui désigne un emoji de mes serveurs (celui du salon d'abord) devient son
/// jeton `<:nom:id>`. Le reste du texte n'est pas touché.
pub fn expand(text: &str, current: Option<Id>) -> String {
    let s = store();
    kraken_core::emoji::expand_shortcodes(text, |name| {
        s.emoji_named(name, current).map(|e| kraken_core::emoji::token(&e.name, e.id))
    })
}

/// Pour modifier un message : les jetons que `:nom:` retrouvera redeviennent lisibles.
pub fn collapse(text: &str, current: Option<Id>) -> String {
    let s = store();
    kraken_core::emoji::collapse_tokens(text, |name, id| s.emoji_named(name, current).is_some_and(|e| e.id == id))
}

/// Un emoji perso en image, à la taille du texte (`--size-emoji`, voir `app.css`). `name` : son nom, lu
/// à la place de l'image et montré si elle manque.
#[component]
pub fn EmojiImg(id: Id, #[prop(into)] name: String) -> impl IntoView {
    let broken = RwSignal::new(false);
    let alt = shortcode(&name);
    let text = alt.clone();
    view! {
        <span class="app-emoji" data-broken=move || broken.get().then_some("")>
            <Show when=move || !broken.get() fallback=move || text.clone()>
                <Photo class="app-emoji-img" src=crate::api::emoji_url(id) alt=alt.clone() cap=128 on_error=move |()| broken.set(true)/>
            </Show>
        </span>
    }
}

/// Une réaction dite à voix haute ou en infobulle : l'emoji lui-même, `:nom:` pour un emoji perso d'un de
/// mes serveurs, « emoji personnalisé » pour celui d'un serveur que je ne connais pas.
pub fn reaction_name(emoji: &str) -> String {
    match kraken_core::emoji::custom_key(emoji) {
        None => emoji.to_string(),
        Some(id) => store().emoji(id).map_or_else(|| "emoji personnalisé".into(), |e| shortcode(&e.name)),
    }
}

/// Ce que montre une réaction : l'emoji Unicode tel quel, ou l'image de l'emoji perso. Un emoji retiré du
/// serveur garde ses réactions ; son image ne se charge plus (`gone` passe à vrai) et le texte « emoji
/// supprimé » la remplace.
#[component]
pub fn ReactionFace(emoji: String, gone: RwSignal<bool>) -> impl IntoView {
    let Some(id) = kraken_core::emoji::custom_key(&emoji) else { return emoji.into_any() };
    let alt = reaction_name(&emoji);
    view! {
        <span class="app-emoji" data-broken=move || gone.get().then_some("")>
            <Show when=move || !gone.get() fallback=|| view! { <span class="app-emoji-gone">"emoji supprimé"</span> }>
                <Photo class="app-emoji-img" src=crate::api::emoji_url(id) alt=alt.clone() cap=128 on_error=move |()| gone.set(true)/>
            </Show>
        </span>
    }
    .into_any()
}

#[component]
pub fn EmojiPicker(
    /// Nom accessible du sélecteur.
    title: &'static str,
    /// Le serveur du salon : ses emojis passent en premier.
    guild: Option<Id>,
    on_pick: Callback<Picked>,
    on_close: Callback<()>,
) -> impl IntoView {
    let s = store();
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
    let query = move || q.get().trim().to_lowercase();
    let found = move || {
        let q = query();
        EMOJIS.iter().filter(|(e, n)| q.is_empty() || n.contains(&q) || *e == q).copied().collect::<Vec<_>>()
    };
    // Les emojis perso dont le nom contient la recherche (écrite avec ou sans les deux-points).
    let custom = move || -> Vec<(Guild, Vec<Emoji>)> {
        let q = query();
        let q = q.trim_matches(':');
        s.emoji_sets(guild)
            .into_iter()
            .map(|(g, es)| {
                (g, es.into_iter().filter(|e| q.is_empty() || e.name.to_lowercase().contains(q)).collect::<Vec<_>>())
            })
            .filter(|(_, es)| !es.is_empty())
            .collect()
    };
    let empty = move || found().is_empty() && custom().is_empty();
    let first = move || {
        custom()
            .into_iter()
            .next()
            .and_then(|(_, es)| es.into_iter().next())
            .map(Picked::Custom)
            .or_else(|| found().first().map(|(e, _)| Picked::Unicode(e.to_string())))
    };
    view! {
        <div class="k-menu app-picker" role="dialog" aria-label=title node_ref=root>
            <input class="k-input" type="search" placeholder="Chercher un emoji" aria-label="Chercher un emoji" autofocus bind:value=q
                on:keydown=move |e| if e.key() == "Enter" {
                    e.prevent_default();
                    if let Some(p) = first() { on_pick.run(p) }
                }/>
            <div class="app-picker-scroll">
                {move || custom().into_iter().map(|(g, es)| {
                    let heading = format!("emojis-{}", g.id);
                    let (labelled, own) = (heading.clone(), heading);
                    view! {
                        <section class="app-picker-sec" aria-labelledby=labelled>
                            <h3 class="app-picker-title" id=own>
                                <span class="k-avatar k-avatar--sm" aria-hidden="true">{initials(&g.name)}</span>
                                <span class="app-ellipsis">{g.name.clone()}</span>
                            </h3>
                            <div class="app-picker-grid" role="group" aria-label=format!("Emojis de {}", g.name)>
                                {es.into_iter().map(|e| {
                                    let label = shortcode(&e.name);
                                    let (id, name) = (e.id, e.name.clone());
                                    view! {
                                        <button type="button" class="app-picker-emoji app-picker-emoji--custom" aria-label=label.clone() title=label
                                            on:click=move |_| on_pick.run(Picked::Custom(e.clone()))>
                                            <EmojiImg id=id name=name/>
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        </section>
                    }
                }).collect_view()}
                <Show when=move || !found().is_empty()>
                    <section class="app-picker-sec" aria-labelledby="emojis-unicode">
                        <h3 class="app-picker-title" id="emojis-unicode" data-only=move || custom().is_empty().then_some("")>"Emojis standards"</h3>
                        <div class="app-picker-grid" role="group" aria-label="Emojis">
                            {move || found().into_iter().map(|(emoji, name)| view! {
                                <button type="button" class="app-picker-emoji" aria-label=name title=name
                                    on:click=move |_| on_pick.run(Picked::Unicode(emoji.to_string()))>{emoji}</button>
                            }).collect_view()}
                        </div>
                    </section>
                </Show>
            </div>
            <Show when=empty><p class="app-hint">"Aucun emoji de la sélection ne correspond."</p></Show>
        </div>
    }
}
