//! Rendu du markdown des messages (`kraken_core::markdown`) en nœuds DOM : le texte de
//! l'utilisateur n'est jamais interprété comme du HTML.

use std::rc::Rc;

use kraken_core::markdown::{self, Block, Inline};
use kraken_proto::{Id, User};
use leptos::prelude::*;

use crate::store::store;

/// Ce qu'il faut pour dessiner mentions et salons.
struct Lookup {
    mentions: Vec<User>,
    everyone: bool,
    me: Option<Id>,
    /// Salons textuels du serveur : nom, serveur, salon.
    channels: Vec<(String, Id, Id)>,
}

/// Le corps d'un message.
#[component]
pub fn Body(content: String, mentions: Vec<User>, everyone: bool, guild: Option<Id>) -> impl IntoView {
    let s = store();
    let channels = guild
        .and_then(|g| s.guilds.with_untracked(|gs| gs.iter().find(|x| x.guild.id == g).cloned()))
        .map(|g| {
            g.channels
                .iter()
                .filter(|c| c.kind.is_textual())
                .filter_map(|c| Some((c.name.clone()?, g.guild.id, c.id)))
                .collect()
        })
        .unwrap_or_default();
    let me = s.me.with_untracked(|m| m.as_ref().map(|m| m.id));
    let lk = Rc::new(Lookup { mentions, everyone, me, channels });
    let blocks = markdown::parse(&content)
        .into_iter()
        .map(|b| match b {
            Block::Text(v) => view! { <p>{inline(v, &lk)}</p> }.into_any(),
            Block::Quote(v) => view! { <blockquote class="app-quote">{inline(v, &lk)}</blockquote> }.into_any(),
            Block::Code { lang, text } => {
                view! { <pre class="app-code" data-lang=lang><code>{text}</code></pre> }.into_any()
            }
        })
        .collect_view();
    view! { <div class="app-md">{blocks}</div> }
}

fn inline(v: Vec<Inline>, lk: &Rc<Lookup>) -> Vec<AnyView> {
    v.into_iter().map(|i| one(i, lk)).collect()
}

fn one(i: Inline, lk: &Rc<Lookup>) -> AnyView {
    match i {
        Inline::Text(t) => t.into_any(),
        Inline::Bold(v) => view! { <strong>{inline(v, lk)}</strong> }.into_any(),
        Inline::Italic(v) => view! { <em>{inline(v, lk)}</em> }.into_any(),
        Inline::Underline(v) => view! { <u>{inline(v, lk)}</u> }.into_any(),
        Inline::Strike(v) => view! { <s>{inline(v, lk)}</s> }.into_any(),
        Inline::Code(t) => view! { <code class="app-code-inline">{t}</code> }.into_any(),
        Inline::Link(url) => {
            let href = url.clone();
            view! { <a href=href target="_blank" rel="noopener noreferrer nofollow ugc">{url}</a> }.into_any()
        }
        Inline::Spoiler(v) => view! { <Spoiler inner=v lk=lk.clone()/> }.into_any(),
        Inline::User(name) => match lk.mentions.iter().find(|u| u.username == name) {
            Some(u) => {
                let me = (Some(u.id) == lk.me).then_some("");
                let title = format!("@{}", u.username);
                view! { <span class="app-mention" data-me=me title=title>"@"{u.name().to_string()}</span> }.into_any()
            }
            None => format!("@{name}").into_any(),
        },
        Inline::Everyone | Inline::Here => {
            let word = if i == Inline::Everyone { "@everyone" } else { "@here" };
            if lk.everyone {
                view! { <span class="app-mention" data-me="">{word}</span> }.into_any()
            } else {
                word.into_any()
            }
        }
        Inline::Channel(name) => match lk.channels.iter().find(|(n, _, _)| *n == name) {
            Some((_, g, c)) => {
                view! { <a class="app-mention" href=format!("/channels/{g}/{c}")>"#"{name}</a> }.into_any()
            }
            None => format!("#{name}").into_any(),
        },
    }
}

/// Texte masqué jusqu'au clic (ou Entrée, Espace). Masqué, il n'est ni lu ni cliquable.
#[component]
fn Spoiler(inner: Vec<Inline>, lk: Rc<Lookup>) -> impl IntoView {
    let shown = RwSignal::new(false);
    let hidden = markdown::plain(&inner);
    let lk = StoredValue::new_local(lk);
    let inner = StoredValue::new(inner);
    move || {
        if shown.get() {
            view! { <span class="app-spoiler" data-shown="">{inline(inner.get_value(), &lk.get_value())}</span> }
                .into_any()
        } else {
            let hidden = hidden.clone();
            view! {
                <span class="app-spoiler" role="button" tabindex="0" aria-label="Afficher le contenu masqué" title="Contenu masqué"
                    on:click=move |_| shown.set(true)
                    on:keydown=move |e| if e.key() == "Enter" || e.key() == " " { e.prevent_default(); shown.set(true) }>
                    <span aria-hidden="true">{hidden}</span>
                </span>
            }
            .into_any()
        }
    }
}
