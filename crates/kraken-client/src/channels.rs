//! Gérer les salons d'un serveur : créer (salon, vocal, catégorie), les ranger au clavier, régler
//! un salon (nom, sujet, mode lent, privé, suppression) et ses droits par rôle et par membre. Le
//! serveur reste seul juge des droits et de la hiérarchie : l'interface désactive ce qui sera
//! refusé, et montre son message quand il refuse quand même.

use std::collections::HashMap;

use gloo_timers::future::TimeoutFuture;
use kraken_core::perms;
use kraken_proto::{
    Channel, ChannelKind, CreateChannel, EditChannel, Id, MAX_CHANNEL_NAME, MAX_TOPIC, MoveChannel, OverwriteKind,
    PermissionOverwrite, SLOWMODE_STEPS, SetOverwrite,
};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use wasm_bindgen::JsCast;

use crate::icons::{Icon, lu};
use crate::shell::ordered;
use crate::social::{Confirm, ConfirmDialog};
use crate::store::{Conn, store};
use crate::ui::modal;

/// Le délai du mode lent, en toutes lettres.
pub fn slow_label(seconds: u32) -> String {
    match seconds {
        0 => "Désactivé".into(),
        1 => "1 seconde".into(),
        s if s < 60 => format!("{s} secondes"),
        60 => "1 minute".into(),
        s if s < 3600 => format!("{} minutes", s / 60),
        3600 => "1 heure".into(),
        s => format!("{} heures", s / 3600),
    }
}

fn is_writable(kind: ChannelKind) -> bool {
    matches!(kind, ChannelKind::Text | ChannelKind::Announcement)
}

fn focus_by_selector(selector: &str) {
    if let Some(h) =
        document().query_selector(selector).ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = h.focus();
    }
}

fn empty(title: &'static str, text: &'static str) -> impl IntoView {
    view! { <div class="k-empty"><b>{title}</b><span>{text}</span></div> }
}

// ───────────────────────── créer ─────────────────────────

const KINDS: [(ChannelKind, &str, &str); 3] = [
    (ChannelKind::Text, "Salon textuel", "Pour écrire des messages, partager des images et des fichiers."),
    (ChannelKind::Voice, "Salon vocal", "Pour parler et se voir en direct."),
    (ChannelKind::Category, "Catégorie", "Pour ranger plusieurs salons sous un même titre."),
];

/// Fenêtre « Créer un salon » (ou une catégorie). `go` : aller dans le salon créé.
#[component]
pub fn CreateChannelDialog(
    guild: Id,
    kind: ChannelKind,
    parent: Option<Id>,
    go: bool,
    close: Callback<()>,
    /// Où remettre le focus si la fenêtre ouvrante a été redessinée entre-temps (sélecteur CSS).
    #[prop(optional)]
    restore: &'static str,
) -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let root = NodeRef::<html::Div>::new();
    modal(root, move || {
        close.try_run(());
    });
    let kind = RwSignal::new(kind);
    let parent = RwSignal::new(parent.map(|p| p.to_string()).unwrap_or_default());
    let name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let input = NodeRef::<html::Input>::new();
    let category = move || kind.get() == ChannelKind::Category;
    let categories = Memo::new(move |_| {
        s.guild(guild)
            .map(|g| {
                let mut c: Vec<Channel> = g.channels.into_iter().filter(|c| c.kind == ChannelKind::Category).collect();
                c.sort_by_key(|c| c.position);
                c
            })
            .unwrap_or_default()
    });
    let refocus_name = move || {
        if let Some(i) = input.get_untracked() {
            let _ = i.focus();
        }
    };
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        let n = name.get_untracked();
        let k = kind.get_untracked();
        if n.trim().is_empty() {
            let what = if k == ChannelKind::Category { "à la catégorie" } else { "au salon" };
            error.set(Some(format!("Donne un nom {what}.")));
            refocus_name();
            return;
        }
        let parent_id = if k == ChannelKind::Category { None } else { parent.get_untracked().parse::<Id>().ok() };
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        spawn_local(async move {
            match crate::api::create_channel(guild, &CreateChannel { name: n, kind: k, parent_id }).await {
                Ok(c) => {
                    s.upsert_channel(guild, c.clone());
                    close.try_run(());
                    if go && c.kind != ChannelKind::Category {
                        // En mobile, le tiroir de navigation se referme sur le salon créé.
                        s.drawer.set(false);
                        navigate(&format!("/channels/{guild}/{}", c.id), Default::default());
                        if c.kind.is_textual() {
                            TimeoutFuture::new(150).await;
                            focus_by_selector(".k-composer textarea");
                        }
                    } else if !restore.is_empty() {
                        // La barre latérale se redessine avec le nouveau salon : le bouton qui avait le
                        // focus n'existe plus.
                        TimeoutFuture::new(150).await;
                        if document().active_element().is_none_or(|a| a.tag_name() == "BODY") {
                            focus_by_selector(restore);
                        }
                    }
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                    busy.try_set(false);
                    refocus_name();
                }
            }
        });
    };
    let title = move || if category() { "Créer une catégorie" } else { "Créer un salon" };
    let label = move || if category() { "Nom de la catégorie" } else { "Nom du salon" };
    let help = move || match kind.get() {
        ChannelKind::Category => "Elle range des salons sous un même titre, que l'on peut replier.",
        ChannelKind::Voice => "Le nom peut contenir des espaces et des majuscules.",
        _ => "Les majuscules et les espaces sont remplacés : « Salon de test » donne #salon-de-test.",
    };
    let placeholder = move || match kind.get() {
        ChannelKind::Category => "Nouvelle catégorie",
        ChannelKind::Voice => "Salon vocal",
        _ => "nouveau-salon",
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog app-dialog-wide" role="dialog" aria-modal="true" aria-labelledby="cc-t">
                <header><h2 id="cc-t">{title}</h2></header>
                <form on:submit=submit novalidate>
                    <div>
                        <fieldset class="k-radios">
                            <legend class="k-label app-legend">"Type"</legend>
                            {KINDS.iter().map(|(k, label, sub)| view! {
                                <label class="k-radio">
                                    <input type="radio" name="cc-kind" prop:checked=move || kind.get() == *k on:change=move |_| kind.set(*k)/>
                                    <div><b>{*label}</b><span>{*sub}</span></div>
                                </label>
                            }).collect_view()}
                        </fieldset>
                        <div class="k-field">
                            <label class="k-label" for="cc-name">{label}</label>
                            <input id="cc-name" class="k-input" node_ref=input maxlength=MAX_CHANNEL_NAME.to_string() autocomplete="off" autofocus
                                placeholder=placeholder prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e))
                                aria-describedby="cc-h" aria-invalid=move || error.get().is_some().then_some("true")/>
                            <p class="k-hint" id="cc-h">{help}</p>
                        </div>
                        <Show when=move || !category()>
                            <div class="k-field">
                                <label class="k-label" for="cc-parent">"Catégorie"</label>
                                <select id="cc-parent" class="k-select" on:change=move |e| parent.set(event_target_value(&e))>
                                    <option value="" prop:selected=move || parent.get().is_empty()>"Aucune catégorie"</option>
                                    <For each=move || categories.get() key=|c| (c.id, c.name.clone()) let:c>
                                        {
                                            let id = c.id.to_string();
                                            let selected = id.clone();
                                            view! { <option value=id prop:selected=move || parent.get() == selected>{c.name.clone().unwrap_or_default()}</option> }
                                        }
                                    </For>
                                </select>
                            </div>
                        </Show>
                        <Show when=move || error.get().is_some()>
                            <p class="k-error" role="alert">{move || error.get()}</p>
                        </Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); }>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--primary" aria-busy=move || busy.get().to_string() disabled=move || busy.get()>
                            {move || if category() { "Créer la catégorie" } else { "Créer le salon" }}
                        </button>
                    </footer>
                </form>
            </div>
        </div>
    }
}

// ───────────────────────── ranger ─────────────────────────

/// Où va un salon qu'on monte (`-1`) ou descend (`1`) d'un cran dans la liste affichée : même
/// parent un rang plus haut ou plus bas, ou, au bout de sa catégorie, la catégorie voisine (la
/// liste « sans catégorie » comprise). Une catégorie ne se déplace que parmi les catégories.
fn step(channels: &[Channel], id: Id, delta: i32) -> Option<MoveChannel> {
    let rows = ordered(channels);
    let me = rows.iter().find(|c| c.id == id)?;
    if me.kind == ChannelKind::Category {
        let cats: Vec<Id> = rows.iter().filter(|c| c.kind == ChannelKind::Category).map(|c| c.id).collect();
        let to = cats.iter().position(|c| *c == id)? as i32 + delta;
        return (0..cats.len() as i32).contains(&to).then_some(MoveChannel { parent_id: None, index: to as u32 });
    }
    // Les groupes dans l'ordre d'affichage : sans catégorie, puis chaque catégorie.
    let mut groups: Vec<(Option<Id>, Vec<Id>)> = vec![(None, vec![])];
    for r in &rows {
        if r.kind == ChannelKind::Category {
            groups.push((Some(r.id), vec![]));
        } else if let Some(last) = groups.last_mut() {
            last.1.push(r.id);
        }
    }
    let (g, i) =
        groups.iter().enumerate().find_map(|(g, (_, ids))| ids.iter().position(|x| *x == id).map(|i| (g, i)))?;
    if delta < 0 {
        if i > 0 {
            Some(MoveChannel { parent_id: groups[g].0, index: (i - 1) as u32 })
        } else if g > 0 {
            Some(MoveChannel { parent_id: groups[g - 1].0, index: groups[g - 1].1.len() as u32 })
        } else {
            None
        }
    } else if i + 1 < groups[g].1.len() {
        Some(MoveChannel { parent_id: groups[g].0, index: (i + 1) as u32 })
    } else if g + 1 < groups.len() {
        Some(MoveChannel { parent_id: groups[g + 1].0, index: 0 })
    } else {
        None
    }
}

/// Réglages › Salons : la liste dans l'ordre d'affichage, avec de quoi monter et descendre chaque
/// salon au clavier, le modifier, ou en créer un.
#[component]
pub fn ChannelsAdmin(guild: Id) -> impl IntoView {
    let s = store();
    let error = RwSignal::new(None::<String>);
    let status = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let creating = RwSignal::new(None::<ChannelKind>);
    let rows = Memo::new(move |_| s.guild(guild).map(|g| ordered(&g.channels)).unwrap_or_default());
    let go = move |id: Id, delta: i32| {
        if busy.get_untracked() {
            return;
        }
        let Some(plan) = rows.with_untracked(|r| step(r, id, delta)) else { return };
        let (up, down) = (format!("chan-up-{id}"), format!("chan-down-{id}"));
        let focus = if delta < 0 { (up, down) } else { (down, up) };
        let name = s.channel(id).and_then(|c| c.name).unwrap_or_default();
        busy.set(true);
        error.set(None);
        status.set(String::new());
        spawn_local(async move {
            match crate::api::move_channel(id, &plan).await {
                Ok(changed) => {
                    for c in changed {
                        if let Some(g) = c.guild_id {
                            s.upsert_channel(g, c);
                        }
                    }
                    status.try_set(format!("« {name} » déplacé."));
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
            crate::guild::refocus(focus.0, focus.1).await;
        });
    };
    view! {
        <p class="app-hint">"Les salons s'affichent dans cet ordre. Monter le premier salon d'une catégorie le range dans la catégorie du dessus, descendre le dernier le range dans celle du dessous."</p>
        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
        <p class="k-sr" role="status">{move || status.get()}</p>
        <div class="app-chan-tools">
            <button type="button" class="k-btn k-btn--primary" on:click=move |_| creating.set(Some(ChannelKind::Text))>
                <Icon icon=lu::LuPlus small=true/>"Créer un salon"
            </button>
            <button type="button" class="k-btn" on:click=move |_| creating.set(Some(ChannelKind::Category))>
                <Icon icon=lu::LuFolderPlus small=true/>"Créer une catégorie"
            </button>
        </div>
        <ul class="app-chan-admin" aria-label="Salons et catégories, dans l'ordre d'affichage">
            <For each=move || rows.get() key=|c| c.id let:c>
                {
                    let id = c.id;
                    let live = move || s.channel(id);
                    let category = c.kind == ChannelKind::Category;
                    let name = move || live().and_then(|c| c.name).unwrap_or_default();
                    let private = move || live().is_some_and(|c| c.private);
                    let nested = move || {
                        rows.with(|r| {
                            let parent = r.iter().find(|x| x.id == id).and_then(|x| x.parent_id);
                            parent.is_some_and(|p| r.iter().any(|x| x.id == p && x.kind == ChannelKind::Category))
                        })
                    };
                    let can = move |delta: i32| rows.with(|r| step(r, id, delta).is_some());
                    let (what, the) = if category { ("la catégorie", "de la catégorie") } else { ("le salon", "du salon") };
                    let icon = match c.kind {
                        ChannelKind::Category => lu::LuFolder,
                        ChannelKind::Voice | ChannelKind::Stage => lu::LuVolume2,
                        ChannelKind::Announcement => lu::LuMegaphone,
                        _ => lu::LuHash,
                    };
                    let gid = c.guild_id.unwrap_or(guild);
                    view! {
                        <li class="app-chan-item" data-category=category.then_some("") data-nested=move || nested().then_some("")>
                            <span class="app-chan-icon" aria-hidden="true"><Icon icon=icon small=true/></span>
                            <span class="app-ellipsis app-chan-name">
                                {name}
                                <Show when=private>
                                    <Icon icon=lu::LuLock small=true/><span class="k-sr">" (privé)"</span>
                                </Show>
                            </span>
                            <button type="button" class="k-btn k-btn--icon" id=format!("chan-up-{id}") disabled=move || !can(-1)
                                aria-label=move || format!("Monter {what} {}", name()) title="Monter" on:click=move |_| go(id, -1)>
                                <Icon icon=lu::LuChevronUp small=true/>
                            </button>
                            <button type="button" class="k-btn k-btn--icon" id=format!("chan-down-{id}") disabled=move || !can(1)
                                aria-label=move || format!("Descendre {what} {}", name()) title="Descendre" on:click=move |_| go(id, 1)>
                                <Icon icon=lu::LuChevronDown small=true/>
                            </button>
                            <a class="k-btn k-btn--icon" href=format!("/channels/{gid}/{id}/settings")
                                aria-label=move || format!("Paramètres {the} {}", name()) title="Paramètres">
                                <Icon icon=lu::LuSettings small=true/>
                            </a>
                        </li>
                    }
                }
            </For>
        </ul>
        {move || creating.get().map(|kind| view! {
            <CreateChannelDialog guild=guild kind=kind parent=None go=false close=Callback::new(move |()| creating.set(None))/>
        })}
    }
}

// ───────────────────────── paramètres d'un salon ─────────────────────────

const SECTIONS: [(&str, &str); 2] = [("overview", "Aperçu"), ("permissions", "Permissions")];

/// `/channels/{serveur}/{salon}/settings` : les réglages d'un salon ou d'une catégorie, et ses
/// droits. Même cadre que les paramètres du serveur.
#[component]
pub fn ChannelSettings() -> impl IntoView {
    let s = store();
    let params = use_params_map();
    let gid = Memo::new(move |_| params.read().get("guild").and_then(|g| g.parse::<Id>().ok()));
    let cid = Memo::new(move |_| params.read().get("channel").and_then(|c| c.parse::<Id>().ok()));
    let navigate = use_navigate();
    let close = move || {
        let channel = cid.get_untracked().and_then(|c| untrack(|| s.channel(c)));
        let to = match (gid.get_untracked(), channel) {
            (Some(g), Some(c)) if c.kind != ChannelKind::Category => format!("/channels/{g}/{}", c.id),
            (Some(g), _) => format!("/channels/{g}"),
            _ => "/channels/@me".to_string(),
        };
        let back = gid.get_untracked().zip(cid.get_untracked());
        navigate(&to, Default::default());
        // La barre latérale a pu être redessinée pendant les réglages : si le focus est perdu,
        // il revient sur l'engrenage du salon.
        if let Some((g, c)) = back {
            spawn_local(async move {
                TimeoutFuture::new(120).await;
                let lost = document().active_element().is_none_or(|a| a.tag_name() == "BODY");
                if lost {
                    focus_by_selector(&format!("a[href=\"/channels/{g}/{c}/settings\"]"));
                }
            });
        }
    };
    let close2 = close.clone();
    let root = NodeRef::new();
    modal(root, close.clone());
    let exists = Memo::new(move |_| cid.get().is_some_and(|c| s.channel(c).is_some()));
    let kind = Memo::new(move |_| cid.get().and_then(|c| s.channel(c)).map(|c| c.kind));
    let allowed = Memo::new(move |_| gid.get().is_some_and(|g| s.guild_perms(g) & perms::MANAGE_CHANNELS != 0));
    let section = Memo::new(move |_| {
        let asked = params.read().get("section").unwrap_or_default();
        SECTIONS.iter().map(|x| x.0).find(|x| *x == asked).unwrap_or("overview")
    });
    let title = Memo::new(move |_| {
        cid.get().and_then(|c| s.channel(c)).and_then(|c| c.name).unwrap_or_else(|| "Paramètres du salon".into())
    });
    let dialog_label = move || {
        if kind.get() == Some(ChannelKind::Category) { "Paramètres de la catégorie" } else { "Paramètres du salon" }
    };
    view! {
        <div class="k-scrim app-settings" node_ref=root>
            <div class="k-settings" role="dialog" aria-modal="true" aria-label=dialog_label>
                <nav aria-label="Sections des paramètres du salon">
                    <h3>{move || title.get()}</h3>
                    {move || gid.get().zip(cid.get()).map(|(g, c)| {
                        SECTIONS.iter().map(|(slug, label)| view! {
                            <a class="k-navitem" href=format!("/channels/{g}/{c}/settings/{slug}")
                                aria-current=move || (section.get() == *slug).then_some("page")>{*label}</a>
                        }).collect_view()
                    })}
                </nav>
                <main>
                    <div class="app-settings-top">
                        <h2>{move || SECTIONS.iter().find(|x| x.0 == section.get()).map_or("Paramètres du salon", |x| x.1)}</h2>
                        <button class="k-btn k-btn--icon" aria-label="Fermer les paramètres (Échap)" on:click=move |_| close2() autofocus>
                            <Icon icon=lu::LuX/>
                        </button>
                    </div>
                    {move || {
                        let (Some(g), Some(c)) = (gid.get(), cid.get()) else {
                            return empty("Salon introuvable", "Ce lien est peut-être ancien.").into_any();
                        };
                        if !exists.get() {
                            return if s.conn.get() == Conn::Ready {
                                empty("Salon introuvable", "Il a été supprimé, ou tu n'y as plus accès.").into_any()
                            } else {
                                view! { <div aria-busy="true"><div class="k-skeleton app-skel-block"></div></div> }.into_any()
                            };
                        }
                        if !allowed.get() {
                            return empty("Rien à régler ici", "Il faut le droit « Gérer les salons » pour changer ce salon.").into_any();
                        }
                        match section.get() {
                            "permissions" => view! { <ChannelPermissions guild=g channel=c/> }.into_any(),
                            _ => view! { <Overview guild=g channel=c/> }.into_any(),
                        }
                    }}
                </main>
            </div>
        </div>
    }
}

/// Aperçu : nom, sujet, mode lent, privé, suppression.
#[component]
fn Overview(guild: Id, channel: Id) -> impl IntoView {
    let s = store();
    let current = Memo::new(move |_| s.channel(channel));
    let Some(first) = current.get_untracked() else { return ().into_any() };
    let kind = first.kind;
    let category = kind == ChannelKind::Category;
    let writable = is_writable(kind);
    let name = RwSignal::new(first.name.clone().unwrap_or_default());
    let topic = RwSignal::new(first.topic.clone().unwrap_or_default());
    let slow = RwSignal::new(first.slowmode_s.to_string());
    let private = RwSignal::new(first.private);
    let error = RwSignal::new(None::<String>);
    let status = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let deleting = RwSignal::new(None::<Channel>);
    let name_input = NodeRef::<html::Input>::new();
    // Privé par sa catégorie : le réglage se change là-bas.
    let inherited = Memo::new(move |_| {
        current.with(|c| c.as_ref().and_then(|c| c.parent_id)).is_some_and(|p| s.channel(p).is_some_and(|p| p.private))
    });
    let fill = move |c: &Channel| {
        name.try_set(c.name.clone().unwrap_or_default());
        topic.try_set(c.topic.clone().unwrap_or_default());
        slow.try_set(c.slowmode_s.to_string());
        private.try_set(c.private);
    };
    let dirty = Memo::new(move |_| {
        current.with(|c| {
            c.as_ref().is_some_and(|c| {
                name.get() != c.name.as_deref().unwrap_or("")
                    || (writable && topic.get() != c.topic.as_deref().unwrap_or(""))
                    || (writable && slow.get() != c.slowmode_s.to_string())
                    || (!inherited.get() && private.get() != c.private)
            })
        })
    });
    // Un changement venu d'ailleurs est repris tant qu'on n'a rien modifié soi-même.
    Effect::new(move || {
        if let Some(c) = current.get()
            && !untrack(|| dirty.get())
        {
            fill(&c);
        }
    });
    let save = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(cur) = current.get_untracked() else { return };
        if busy.get_untracked() || !dirty.get_untracked() {
            return;
        }
        let n = name.get_untracked().trim().to_string();
        if n.is_empty() {
            error.set(Some(if category { "Donne un nom à la catégorie." } else { "Donne un nom au salon." }.into()));
            if let Some(i) = name_input.get_untracked() {
                let _ = i.focus();
            }
            return;
        }
        let req = EditChannel {
            name: (Some(n.as_str()) != cur.name.as_deref()).then_some(n),
            topic: writable
                .then(|| topic.get_untracked().trim().to_string())
                .filter(|t| Some(t.as_str()) != Some(cur.topic.as_deref().unwrap_or(""))),
            slowmode_s: writable
                .then(|| slow.get_untracked().parse::<u32>().ok())
                .flatten()
                .filter(|n| *n != cur.slowmode_s),
            private: (!inherited.get_untracked() && private.get_untracked() != cur.private)
                .then(|| private.get_untracked()),
        };
        busy.set(true);
        error.set(None);
        status.set(String::new());
        spawn_local(async move {
            match crate::api::edit_channel(channel, &req).await {
                Ok(c) => {
                    s.upsert_channel(guild, c.clone());
                    fill(&c);
                    status.try_set("Modifications enregistrées.".into());
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let cancel = move |_| {
        error.set(None);
        if let Some(c) = current.get_untracked() {
            fill(&c);
        }
    };
    // Le dernier salon d'écriture d'un serveur reste : le serveur le refuse, on le dit d'avance.
    let last_text = Memo::new(move |_| {
        writable && s.guild(guild).is_some_and(|g| g.channels.iter().filter(|c| c.kind.is_textual()).count() <= 1)
    });
    view! {
        <form class="app-chan-form" on:submit=save novalidate>
            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            <div class="k-field">
                <label class="k-label" for="chan-name">{if category { "Nom de la catégorie" } else { "Nom du salon" }}</label>
                <input id="chan-name" class="k-input" node_ref=name_input maxlength=MAX_CHANNEL_NAME.to_string() autocomplete="off"
                    prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e))
                    aria-describedby="chan-name-h" aria-invalid=move || error.get().is_some().then_some("true")/>
                <p class="k-hint" id="chan-name-h">{if writable {
                    "Les majuscules et les espaces sont remplacés : « Salon de test » donne #salon-de-test."
                } else {
                    "Tout le monde voit le changement tout de suite."
                }}</p>
            </div>
            {writable.then(|| view! {
                <div class="k-field">
                    <label class="k-label" for="chan-topic">"Sujet du salon"</label>
                    <textarea id="chan-topic" class="k-input app-textarea" rows="3" maxlength=MAX_TOPIC.to_string()
                        aria-describedby="chan-topic-h"
                        prop:value=move || topic.get() on:input=move |e| topic.set(event_target_value(&e))></textarea>
                    <p class="k-hint" id="chan-topic-h">"Une ligne sur ce qui se dit ici : elle s'affiche dans l'en-tête du salon."</p>
                </div>
                <div class="k-setting">
                    <label for="chan-slow">"Mode lent"</label>
                    <select id="chan-slow" class="k-select" aria-describedby="chan-slow-h"
                        on:change=move |e| slow.set(event_target_value(&e))>
                        {SLOWMODE_STEPS.iter().map(|n| {
                            let v = n.to_string();
                            let mine = v.clone();
                            view! { <option value=v prop:selected=move || slow.get() == mine>{slow_label(*n)}</option> }
                        }).collect_view()}
                    </select>
                    <p id="chan-slow-h">"Chacun doit attendre ce délai entre deux messages. Les personnes qui gèrent les messages ou les salons n'attendent pas."</p>
                </div>
            })}
            <div class="k-setting">
                <label for="chan-private">{if category { "Catégorie privée" } else { "Salon privé" }}</label>
                <input id="chan-private" type="checkbox" role="switch" class="k-switch" aria-describedby="chan-private-h"
                    disabled=move || inherited.get()
                    prop:checked=move || private.get() || inherited.get()
                    on:change=move |e| private.set(event_target_checked(&e))/>
                <p id="chan-private-h">{move || if inherited.get() {
                    "Sa catégorie est privée : ce salon l'est aussi. Change-le dans les paramètres de la catégorie."
                } else if category {
                    "Seuls les rôles et les membres que tu ajoutes dans « Permissions » voient cette catégorie et ses salons."
                } else {
                    "Seuls les rôles et les membres que tu ajoutes dans « Permissions » voient ce salon."
                }}</p>
            </div>
            <p class="k-sr" role="status">{move || status.get()}</p>
            <Show when=move || dirty.get()>
                <div class="app-savebar" role="region" aria-label="Modifications du salon">
                    <span role="status"><Icon icon=lu::LuCircleAlert small=true/>"Modifications non enregistrées"</span>
                    <button type="button" class="k-btn k-btn--ghost" on:click=cancel>"Annuler"</button>
                    <button type="submit" class="k-btn k-btn--primary" aria-busy=move || busy.get().to_string()>"Enregistrer"</button>
                </div>
            </Show>
        </form>
        <Show when=move || !dirty.get() && !status.with(String::is_empty)>
            <p class="app-hint app-saved" aria-hidden="true"><Icon icon=lu::LuCheck small=true/>{move || status.get()}</p>
        </Show>
        <div class="app-roles-danger">
            <button type="button" class="k-btn k-btn--danger" disabled=move || last_text.get()
                aria-describedby=move || last_text.get().then_some("chan-del-h")
                on:click=move |_| deleting.set(current.get_untracked())>
                <Icon icon=lu::LuTrash2 small=true/>{if category { "Supprimer la catégorie" } else { "Supprimer le salon" }}
            </button>
            <Show when=move || last_text.get()>
                <p class="k-hint" id="chan-del-h">"C'est le dernier salon d'écriture du serveur : crée-en un autre avant de le supprimer."</p>
            </Show>
        </div>
        {move || deleting.get().map(|c| view! {
            <ConfirmDialog what=Confirm::DeleteChannel(c) close=Callback::new(move |()| deleting.set(None))/>
        })}
    }
    .into_any()
}

// ───────────────────────── droits par salon ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tri {
    Allow,
    Inherit,
    Deny,
}

const TRIS: [(Tri, &str); 3] = [(Tri::Allow, "Autoriser"), (Tri::Inherit, "Hériter"), (Tri::Deny, "Refuser")];

fn tri_of((allow, deny): (u64, u64), bit: u64) -> Tri {
    if allow & bit != 0 {
        Tri::Allow
    } else if deny & bit != 0 {
        Tri::Deny
    } else {
        Tri::Inherit
    }
}

fn with_tri((allow, deny): (u64, u64), bit: u64, t: Tri) -> (u64, u64) {
    let (allow, deny) = (allow & !bit, deny & !bit);
    match t {
        Tri::Allow => (allow | bit, deny),
        Tri::Deny => (allow, deny | bit),
        Tri::Inherit => (allow, deny),
    }
}

/// Un rôle ou un membre à qui le salon donne ou retire des droits.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Target {
    id: Id,
    kind: OverwriteKind,
}

#[component]
fn ChannelPermissions(guild: Id, channel: Id) -> impl IntoView {
    let s = store();
    let kind = s.channel(channel).map_or(ChannelKind::Text, |c| c.kind);
    let list = RwSignal::new(None::<Vec<PermissionOverwrite>>);
    let error = RwSignal::new(None::<String>);
    let status = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let selected = RwSignal::new(guild);
    // Brouillons par cible : (autorisés, refusés), tant qu'ils diffèrent de ce qui est enregistré.
    let drafts = RwSignal::new(HashMap::<Id, (u64, u64)>::new());
    // Cibles ajoutées ici, sans réglage enregistré encore.
    let extra = RwSignal::new(Vec::<Target>::new());
    let pick = RwSignal::new(String::new());
    spawn_local(async move {
        match crate::api::overwrites(channel).await {
            Ok(l) => {
                list.try_set(Some(l));
            }
            Err(e) => {
                error.try_set(Some(e.message));
            }
        }
    });
    let saved_of = move |id: Id| {
        list.with(|l| {
            l.as_ref().and_then(|l| l.iter().find(|o| o.target_id == id)).map_or((0, 0), |o| (o.allow, o.deny))
        })
    };
    let effective = move |id: Id| drafts.with(|d| d.get(&id).copied()).unwrap_or_else(|| saved_of(id));
    let name_of = move |t: Target| -> String {
        match t.kind {
            OverwriteKind::Role if t.id == guild => "@tout le monde".into(),
            OverwriteKind::Role => s
                .guild(guild)
                .and_then(|g| g.roles.into_iter().find(|r| r.id == t.id))
                .map_or_else(|| "Rôle supprimé".into(), |r| r.name),
            OverwriteKind::Member => s.name_of(Some(guild), t.id).unwrap_or_else(|| "Ancien membre".into()),
        }
    };
    // `@tout le monde`, puis les rôles du plus haut au plus bas, puis les membres.
    let targets = Memo::new(move |_| {
        let mut v = vec![Target { id: guild, kind: OverwriteKind::Role }];
        let saved: Vec<Target> =
            list.with(|l| l.iter().flatten().map(|o| Target { id: o.target_id, kind: o.kind }).collect());
        for t in saved.into_iter().chain(extra.get()) {
            if !v.iter().any(|x| x.id == t.id) {
                v.push(t);
            }
        }
        let position = |t: &Target| {
            s.guild(guild).and_then(|g| g.roles.iter().find(|r| r.id == t.id).map(|r| r.position)).unwrap_or(0)
        };
        v[1..].sort_by_key(|t| (t.kind == OverwriteKind::Member, std::cmp::Reverse(position(t)), name_of(*t)));
        v
    });
    // La cible choisie disparaît (réglages retirés) : retour à `@tout le monde`.
    Effect::new(move || {
        let cur = selected.get();
        if !targets.with(|t| t.iter().any(|x| x.id == cur)) {
            selected.set(guild);
        }
    });
    let candidates = Memo::new(move |_| {
        let Some(g) = s.guild(guild) else { return (vec![], vec![]) };
        let taken = targets.get();
        let free = |id: Id| !taken.iter().any(|t| t.id == id);
        let mut roles: Vec<_> = g.roles.iter().filter(|r| r.id != guild && free(r.id)).cloned().collect();
        roles.sort_by_key(|r| std::cmp::Reverse(r.position));
        let members: Vec<_> = g.members.iter().filter(|m| free(m.user.id)).cloned().collect();
        (roles, members)
    });
    let add = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let v = pick.get_untracked();
        let Some((tag, id)) = v.split_once(':') else { return };
        let Ok(id) = id.parse::<Id>() else { return };
        let kind = if tag == "r" { OverwriteKind::Role } else { OverwriteKind::Member };
        extra.update(|x| x.push(Target { id, kind }));
        selected.set(id);
        pick.set(String::new());
    };
    // Ai-je le droit de régler cette cible ? (Le serveur juge : ceci désactive ce qui sera refusé.)
    let editable = move |t: Target| match t.kind {
        OverwriteKind::Role => {
            t.id == guild
                || s.guild(guild)
                    .and_then(|g| g.roles.into_iter().find(|r| r.id == t.id))
                    .is_some_and(|r| s.can_manage_role(guild, &r))
        }
        OverwriteKind::Member => s.my_id() == Some(t.id) || s.outranks(guild, t.id),
    };
    let set_state = move |t: Target, bit: u64, state: Tri| {
        let next = with_tri(effective(t.id), bit, state);
        let saved = saved_of(t.id);
        drafts.update(|d| {
            if next == saved {
                d.remove(&t.id);
            } else {
                d.insert(t.id, next);
            }
        });
    };
    let reset = move |t: Target| {
        let saved = saved_of(t.id);
        drafts.update(|d| {
            if saved == (0, 0) {
                d.remove(&t.id);
            } else {
                d.insert(t.id, (0, 0));
            }
        });
    };
    let save = move |_| {
        if busy.get_untracked() {
            return;
        }
        let todo: Vec<(Target, u64, u64)> = targets
            .get_untracked()
            .into_iter()
            .filter_map(|t| drafts.with_untracked(|d| d.get(&t.id).copied()).map(|(a, d)| (t, a, d)))
            .collect();
        busy.set(true);
        error.set(None);
        status.set(String::new());
        spawn_local(async move {
            let mut failed = false;
            for (t, allow, deny) in todo {
                match crate::api::set_overwrite(channel, t.id, &SetOverwrite { kind: t.kind, allow, deny }).await {
                    Ok(l) => {
                        list.try_set(Some(l));
                        drafts.try_update(|d| {
                            d.remove(&t.id);
                        });
                        extra.try_update(|x| x.retain(|e| e.id != t.id));
                    }
                    Err(e) => {
                        error.try_set(Some(e.message));
                        failed = true;
                        break;
                    }
                }
            }
            if !failed {
                status.try_set("Permissions enregistrées.".into());
            }
            busy.try_set(false);
        });
    };
    let cancel = move |_| {
        error.set(None);
        drafts.set(HashMap::new());
    };
    let dirty = move || drafts.with(|d| !d.is_empty());
    let my_base = move || s.guild_perms(guild);
    let rows = move |t: Target| {
        let voice = matches!(kind, ChannelKind::Voice | ChannelKind::Stage);
        crate::guild::CATEGORIES
            .iter()
            .filter(|(title, _)| match *title {
                "Salons texte" => !voice,
                "Vocal" => voice || kind == ChannelKind::Category,
                _ => true,
            })
            .filter_map(|(title, list)| {
                let rows: Vec<_> = list.iter().filter(|(bit, ..)| perms::CHANNEL_SCOPE & bit != 0).collect();
                (!rows.is_empty()).then_some((*title, rows))
            })
            .map(|(title, rows)| {
                view! {
                    <fieldset class="app-perms">
                        <legend class="k-category">{title}</legend>
                        {rows.into_iter().map(|(bit, label, help)| {
                            let bit = *bit;
                            let sensitive = crate::guild::SENSITIVE.contains(&bit);
                            let (lid, hid) = (format!("ow-{bit}-l"), format!("ow-{bit}-h"));
                            // On ne règle que des droits que l'on a soi-même.
                            let ok = move || editable(t) && my_base() & bit != 0;
                            view! {
                                <div class="app-ow-row">
                                    <div class="app-ow-text">
                                        <span class="app-ow-label" id=lid.clone()>
                                            {sensitive.then(|| view! {
                                                <span class="app-sensitive" title="Droit sensible"><Icon icon=lu::LuShieldAlert/><span class="k-sr">"Droit sensible : "</span></span>
                                            })}
                                            {*label}
                                        </span>
                                        <p class="app-hint" id=hid.clone()>{*help}</p>
                                    </div>
                                    <div class="app-tri" role="radiogroup" aria-labelledby=lid aria-describedby=hid>
                                        {TRIS.iter().map(|(state, text)| {
                                            let state = *state;
                                            let icon = match state {
                                                Tri::Allow => lu::LuCheck,
                                                Tri::Inherit => lu::LuMinus,
                                                Tri::Deny => lu::LuX,
                                            };
                                            view! {
                                                <label data-state=match state { Tri::Allow => "allow", Tri::Inherit => "inherit", Tri::Deny => "deny" } title=*text>
                                                    <input type="radio" name=format!("ow-{bit}")
                                                        prop:checked=move || tri_of(effective(t.id), bit) == state
                                                        disabled=move || !ok()
                                                        on:change=move |_| set_state(t, bit, state)/>
                                                    <Icon icon=icon small=true/><span class="k-sr">{*text}</span>
                                                </label>
                                            }
                                        }).collect_view()}
                                    </div>
                                </div>
                            }
                        }).collect_view()}
                    </fieldset>
                }
            })
            .collect_view()
    };
    view! {
        <p class="app-hint">"Pour chaque droit : « Autoriser » ou « Refuser » s'ajoute aux droits du serveur dans ce salon, « Hériter » laisse les droits habituels. Un refus l'emporte sur un droit donné par un rôle, mais pas sur le propriétaire ni les administrateurs. On ne règle que des droits que l'on a soi-même."</p>
        <ul class="app-ow-key" aria-hidden="true">
            <li><Icon icon=lu::LuCheck small=true/>"Autoriser"</li>
            <li><Icon icon=lu::LuMinus small=true/>"Hériter"</li>
            <li><Icon icon=lu::LuX small=true/>"Refuser"</li>
        </ul>
        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
        <p class="k-sr" role="status">{move || status.get()}</p>
        <Show when=move || list.with(Option::is_some) fallback=move || view! { <div aria-busy="true"><div class="k-skeleton app-skel-block"></div></div> }>
            <div class="app-roles">
                <div class="app-roles-list">
                    <ul aria-label="Rôles et membres de ce salon">
                        <For each=move || targets.get() key=|t| t.id let:t>
                            <li class="app-role">
                                <button type="button" class="app-role-pick" aria-current=move || (selected.get() == t.id).then_some("true")
                                    on:click=move |_| selected.set(t.id)>
                                    <span class="app-role-icon" aria-hidden="true">
                                        {if t.kind == OverwriteKind::Role { view! { <i class="app-role-dot"></i> }.into_any() } else { view! { <Icon icon=lu::LuUser small=true/> }.into_any() }}
                                    </span>
                                    <span class="app-ellipsis">{move || name_of(t)}</span>
                                    <Show when=move || drafts.with(|d| d.contains_key(&t.id))><span class="k-sr">" (modifications non enregistrées)"</span></Show>
                                    <Show when=move || { effective(t.id) != (0, 0) }><span class="k-sr">" (réglé)"</span></Show>
                                </button>
                            </li>
                        </For>
                    </ul>
                    <form class="k-field" on:submit=add>
                        <label class="k-label" for="ow-add">"Ajouter un rôle ou un membre"</label>
                        <div class="app-add-row">
                            <select id="ow-add" class="k-select" on:change=move |e| pick.set(event_target_value(&e))>
                                <option value="" prop:selected=move || pick.get().is_empty()>"Choisis dans la liste"</option>
                                <optgroup label="Rôles">
                                    {move || candidates.get().0.into_iter().map(|r| view! { <option value=format!("r:{}", r.id)>{r.name}</option> }).collect_view()}
                                </optgroup>
                                <optgroup label="Membres">
                                    {move || candidates.get().1.into_iter().map(|m| {
                                        let label = format!("{} (@{})", m.nick.clone().unwrap_or_else(|| m.user.name().to_string()), m.user.username);
                                        view! { <option value=format!("m:{}", m.user.id)>{label}</option> }
                                    }).collect_view()}
                                </optgroup>
                            </select>
                            <button type="submit" class="k-btn" disabled=move || pick.get().is_empty()>"Ajouter"</button>
                        </div>
                    </form>
                </div>
                <div class="app-roles-edit">
                    {move || {
                        let id = selected.get();
                        targets.with(|t| t.iter().find(|x| x.id == id).copied()).map(|t| view! {
                            <h3 class="app-roles-title">{move || name_of(t)}</h3>
                            <Show when=move || !editable(t)>
                                <p class="app-hint">"Son rang est égal ou supérieur au tien : tu ne peux pas changer ses réglages."</p>
                            </Show>
                            {rows(t)}
                            <div>
                                <button type="button" class="k-btn k-btn--ghost" disabled=move || !editable(t) on:click=move |_| reset(t)>
                                    "Tout remettre sur « Hériter »"
                                </button>
                            </div>
                        })
                    }}
                </div>
            </div>
            <Show when=dirty>
                <div class="app-savebar" role="region" aria-label="Modifications des permissions">
                    <span role="status"><Icon icon=lu::LuCircleAlert small=true/>"Modifications non enregistrées"</span>
                    <button type="button" class="k-btn k-btn--ghost" on:click=cancel>"Annuler"</button>
                    <button type="button" class="k-btn k-btn--primary" on:click=save aria-busy=move || busy.get().to_string()>"Enregistrer"</button>
                </div>
            </Show>
        </Show>
    }
}
