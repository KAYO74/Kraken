//! S01 : accueil Amis, onglets En ligne / Tous / En attente / Bloqués / Ajouter un ami (F02,
//! F11). Les listes suivent le gateway : une demande reçue ou acceptée s'affiche sans recharger.

use kraken_proto::{Relationship, RelationshipKind, Status};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::icons::{Icon, lu};
use crate::social::{Confirm, ConfirmDialog, call_user, open_dm};
use crate::store::{Conn, store};
use crate::ui::{Avatar, Header, MoreMenu, status_label};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Online,
    All,
    Pending,
    Blocked,
    Add,
}

const TABS: [(Tab, &str, &str); 5] = [
    (Tab::Online, "online", "En ligne"),
    (Tab::All, "all", "Tous"),
    (Tab::Pending, "pending", "En attente"),
    (Tab::Blocked, "blocked", "Bloqués"),
    (Tab::Add, "add", "Ajouter un ami"),
];

fn slug(t: Tab) -> &'static str {
    TABS.iter().find(|x| x.0 == t).map_or("", |x| x.1)
}

fn shows(t: Tab, r: &Relationship) -> bool {
    match t {
        Tab::Online => r.kind == RelationshipKind::Friend && r.status != Status::Offline,
        Tab::All => r.kind == RelationshipKind::Friend,
        Tab::Pending => matches!(r.kind, RelationshipKind::PendingIncoming | RelationshipKind::PendingOutgoing),
        Tab::Blocked => r.kind == RelationshipKind::Blocked,
        Tab::Add => false,
    }
}

#[component]
pub fn Friends() -> impl IntoView {
    let s = store();
    let tab = RwSignal::new(Tab::Online);
    let query = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let confirm = RwSignal::new(None::<Confirm>);
    let rows = Memo::new(move |_| {
        let q = query.get().to_lowercase();
        let mut v: Vec<Relationship> = s
            .friends
            .get()
            .into_iter()
            .filter(|r| shows(tab.get(), r))
            .filter(|r| q.is_empty() || r.user.name().to_lowercase().contains(&q) || r.user.username.contains(&q))
            .collect();
        // Demandes reçues d'abord : ce sont elles qui attendent une réponse.
        v.sort_by_key(|r| r.kind != RelationshipKind::PendingIncoming);
        v
    });
    let incoming = Memo::new(move |_| {
        s.friends.with(|f| f.iter().filter(|r| r.kind == RelationshipKind::PendingIncoming).count())
    });
    // Onglets au clavier : flèches, Début et Fin déplacent le choix et le focus.
    let tablist = NodeRef::<leptos::html::Div>::new();
    let keys = move |e: leptos::ev::KeyboardEvent| {
        let i = TABS.iter().position(|x| x.0 == tab.get_untracked()).unwrap_or(0);
        let n = TABS.len();
        let next = match e.key().as_str() {
            "ArrowRight" => (i + 1) % n,
            "ArrowLeft" => (i + n - 1) % n,
            "Home" => 0,
            "End" => n - 1,
            _ => return,
        };
        e.prevent_default();
        tab.set(TABS[next].0);
        let btn = tablist
            .get_untracked()
            .and_then(|l| l.query_selector(&format!("#tab-{}", TABS[next].1)).ok().flatten())
            .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok());
        if let Some(b) = btn {
            let _ = b.focus();
        }
    };
    let tabs = TABS
        .iter()
        .map(|&(t, id, label)| {
            let selected = move || tab.get() == t;
            view! {
                <button class="k-tab" role="tab" id=format!("tab-{id}") aria-controls="friends-panel"
                    aria-selected=move || selected().to_string() tabindex=move || if selected() { "0" } else { "-1" }
                    on:click=move |_| { tab.set(t); error.set(None); }>
                    {(t == Tab::Add).then(|| view! { <Icon icon=lu::LuUserPlus small=true/> })}{label}
                    {(t == Tab::Pending).then(|| view! {
                        <Show when=move || { incoming.get() > 0 }>
                            <span class="k-badge k-badge--count">{move || incoming.get()}</span>
                            <span class="k-sr">" à accepter ou refuser"</span>
                        </Show>
                    })}
                </button>
            }
        })
        .collect_view();

    view! {
        <Header>
            <Icon icon=lu::LuUsers/>
            <h1 class="app-title">"Amis"</h1>
            <div class="k-tabs" role="tablist" aria-label="Filtrer les amis" node_ref=tablist on:keydown=keys>{tabs}</div>
        </Header>
        <div class="app-body">
            <section class="app-friends" role="tabpanel" id="friends-panel" aria-labelledby=move || format!("tab-{}", slug(tab.get()))>
                {move || if tab.get() == Tab::Add {
                    view! { <AddFriend/> }.into_any()
                } else {
                    view! {
                        <div class="k-search app-search">
                            <Icon icon=lu::LuSearch/>
                            <input class="k-input" type="search" placeholder="Chercher" aria-label="Chercher parmi tes amis" bind:value=query/>
                        </div>
                        <h2 class="k-category">
                            {move || match tab.get() {
                                Tab::Online => "En ligne ",
                                Tab::Pending => "En attente ",
                                Tab::Blocked => "Bloqués ",
                                _ => "Tous ",
                            }}
                            <span class="k-badge k-badge--neutral">{move || rows.get().len()}</span>
                        </h2>
                        <Show when=move || error.get().is_some()>
                            <p class="k-error app-friends-error" role="alert">{move || error.get()}" "
                                <button class="k-btn k-btn--link" on:click=move |_| error.set(None)>"Fermer"</button></p>
                        </Show>
                        {move || if s.conn.get() != Conn::Ready && s.friends.with(|f| f.is_empty()) {
                            view! { <div aria-busy="true">{(0..4).map(|_| view! { <div class="k-skeleton app-skel-row"></div> }).collect_view()}</div> }.into_any()
                        } else if rows.get().is_empty() {
                            let (title, line) = match tab.get() {
                                Tab::Online => ("Personne en ligne pour l'instant", "Tes amis apparaîtront ici dès qu'ils se connectent."),
                                Tab::Pending => ("Aucune demande en attente", "Les demandes reçues et envoyées s'affichent ici."),
                                Tab::Blocked => ("Tu n'as bloqué personne", "Un compte bloqué ne peut plus t'écrire en privé ni te demander en ami."),
                                _ => ("Pas encore d'amis", "Ajoute quelqu'un avec son pseudo pour commencer."),
                            };
                            view! { <div class="k-empty"><b>{title}</b><span>{line}</span></div> }.into_any()
                        } else {
                            view! {
                                <ul class="app-list">
                                    <For each=move || rows.get() key=|r| (r.user.id, r.kind as u8, r.status as u8, r.user.display_name.clone()) let:r>
                                        <FriendRow r=r error=error confirm=confirm/>
                                    </For>
                                </ul>
                            }.into_any()
                        }}
                    }.into_any()
                }}
            </section>
            <aside class="app-aside app-activity" aria-label="Activité">
                <h2>"Où sont tes amis"</h2>
                <div class="k-empty"><b>"Aucun salon vocal occupé"</b>"Les salons vocaux où tes amis discutent s'afficheront ici, pour les rejoindre en un clic."</div>
            </aside>
        </div>
        {move || confirm.get().map(|what| view! { <ConfirmDialog what=what close=Callback::new(move |()| confirm.set(None))/> })}
    }
}

/// Une personne et ce qu'on peut faire avec elle, selon la relation.
#[component]
fn FriendRow(r: Relationship, error: RwSignal<Option<String>>, confirm: RwSignal<Option<Confirm>>) -> impl IntoView {
    let s = store();
    let uid = r.user.id;
    let name = r.user.name().to_string();
    let sub = match r.kind {
        RelationshipKind::PendingIncoming => "Demande reçue",
        RelationshipKind::PendingOutgoing => "Demande envoyée",
        RelationshipKind::Blocked => "Bloqué",
        RelationshipKind::Friend => status_label(r.status),
    };
    let status = (r.kind == RelationshipKind::Friend).then_some(r.status);
    let busy = RwSignal::new(false);
    // Accepter, refuser, annuler, débloquer : un clic, sans confirmation (rien d'irréversible).
    let act = move |accept: bool| {
        busy.set(true);
        spawn_local(async move {
            let done = if accept {
                crate::api::set_relationship(uid, RelationshipKind::Friend).await.map(|r| s.upsert_rel(r))
            } else {
                crate::api::remove_relationship(uid).await.map(|()| s.remove_rel(uid))
            };
            if let Err(e) = done {
                busy.try_set(false);
                error.set(Some(e.message));
            }
        });
    };
    let user = r.user.clone();
    let actions = match r.kind {
        RelationshipKind::Friend => {
            let write = open_dm(uid, error);
            let ring = call_user(uid, error);
            let (u1, u2) = (user.clone(), user.clone());
            view! {
                <button class="k-btn k-btn--icon" aria-label=format!("Écrire à {name}") title="Message" on:click=move |_| write()>
                    <Icon icon=lu::LuMessageCircle/>
                </button>
                <button class="k-btn k-btn--icon" aria-label=format!("Appeler {name}") title="Appeler"
                    disabled=!crate::voice::supported() on:click=move |_| ring()>
                    <Icon icon=lu::LuPhone/>
                </button>
                // Retirer et bloquer : rangés sous ⋯, pas au rang d'écrire et d'appeler.
                <MoreMenu label=format!("Plus d'actions pour {name}")>
                    <button class="k-menuitem" on:click=move |_| confirm.set(Some(Confirm::RemoveFriend(u1.clone())))>
                        <Icon icon=lu::LuUserMinus/>"Retirer de tes amis"
                    </button>
                    <button class="k-menuitem is-danger" aria-label=format!("Bloquer {name}")
                        on:click=move |_| confirm.set(Some(Confirm::Block(u2.clone())))>
                        <Icon icon=lu::LuBan/>"Bloquer"
                    </button>
                </MoreMenu>
            }
            .into_any()
        }
        RelationshipKind::PendingIncoming => view! {
            <button class="k-btn k-btn--icon app-accept" aria-label=format!("Accepter la demande de {name}") title="Accepter"
                disabled=move || busy.get() on:click=move |_| act(true)>
                <Icon icon=lu::LuCheck/>
            </button>
            <button class="k-btn k-btn--icon" aria-label=format!("Refuser la demande de {name}") title="Refuser"
                disabled=move || busy.get() on:click=move |_| act(false)>
                <Icon icon=lu::LuX/>
            </button>
            <button class="k-btn k-btn--icon app-link-danger" aria-label=format!("Bloquer {name}") title="Bloquer"
                on:click=move |_| confirm.set(Some(Confirm::Block(user.clone())))>
                <Icon icon=lu::LuBan/>
            </button>
        }
        .into_any(),
        RelationshipKind::PendingOutgoing => view! {
            <button class="k-btn k-btn--icon" aria-label=format!("Annuler la demande à {name}") title="Annuler la demande"
                disabled=move || busy.get() on:click=move |_| act(false)>
                <Icon icon=lu::LuX/>
            </button>
        }
        .into_any(),
        RelationshipKind::Blocked => view! {
            <button class="k-btn k-btn--sm" aria-label=format!("Débloquer {name}") disabled=move || busy.get() on:click=move |_| act(false)>
                "Débloquer"
            </button>
        }
        .into_any(),
    };
    view! {
        <li class="app-row">
            {match status {
                Some(st) => view! { <Avatar user=r.user.clone() size="" status=st/> }.into_any(),
                None => view! { <Avatar user=r.user.clone() size=""/> }.into_any(),
            }}
            <div class="app-ellipsis"><b>{r.user.name().to_string()}</b><small>{format!("@{} · {sub}", r.user.username)}</small></div>
            <div class="app-row-actions">{actions}</div>
        </li>
    }
}

/// Onglet « Ajouter un ami » : le pseudo unique, une réponse claire dans tous les cas.
#[component]
fn AddFriend() -> impl IntoView {
    let s = store();
    let name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let done = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let n = name.get_untracked();
        if n.trim().is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        done.set(None);
        spawn_local(async move {
            match crate::api::send_friend_request(&n).await {
                Ok(r) => {
                    let who = r.user.name().to_string();
                    let line = if r.kind == RelationshipKind::Friend {
                        format!("{who} t'avait déjà demandé : vous êtes maintenant amis.")
                    } else {
                        format!("Demande envoyée à {who} (@{}). Elle apparaît dans « En attente ».", r.user.username)
                    };
                    s.upsert_rel(r);
                    name.try_set(String::new());
                    done.try_set(Some(line));
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    view! {
        <form class="app-add" on:submit=submit>
            <h2>"Ajouter un ami"</h2>
            <p class="app-hint" id="add-h">"Tape son pseudo KRAKEN, celui qui commence par @ sous son nom."</p>
            <div class="app-add-row">
                <input class="k-input" placeholder="pseudo" aria-label="Pseudo de ton ami" bind:value=name
                    autocomplete="off" autocapitalize="none" spellcheck="false" maxlength="33" required
                    aria-describedby="add-h" aria-invalid=move || error.get().is_some().then_some("true")
                    on:input=move |_| { error.set(None); done.set(None); }/>
                <button class="k-btn k-btn--primary" type="submit" disabled=move || name.with(|n| n.trim().is_empty()) || busy.get()
                    aria-busy=move || busy.get().to_string()>"Envoyer la demande"</button>
            </div>
            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            <Show when=move || done.get().is_some()><p class="app-add-ok" role="status">{move || done.get()}</p></Show>
        </form>
    }
}
