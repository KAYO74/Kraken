//! S01 : accueil Amis, onglets En ligne / Tous / En attente / Bloqués / Ajouter un ami (F02,
//! F11). Les listes suivent le gateway : une demande reçue ou acceptée s'affiche sans recharger.

use kraken_proto::{Relationship, RelationshipKind};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::i18n::{t, tf};
use crate::icons::{Icon, lu};
use crate::social::{Confirm, ConfirmDialog, call_user, open_dm};
use crate::store::Store;
use crate::store::{Conn, store};
use crate::tk;
use crate::ui::{Avatar, Dot, Header, MoreMenu};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Online,
    All,
    Pending,
    Blocked,
    Add,
}

const TABS: [(Tab, &str, &str); 5] = [
    (Tab::Online, "online", tk!("En ligne")),
    (Tab::All, "all", tk!("Tous")),
    (Tab::Pending, "pending", tk!("En attente")),
    (Tab::Blocked, "blocked", tk!("Bloqués")),
    (Tab::Add, "add", tk!("Ajouter un ami")),
];

/// « Refuser » une demande d'ami. Le catalogue traduit déjà « Refuser » autrement (un droit refusé) : le `{contexte}`
/// vide en fait une entrée à part, invisible en français (la même que celle de `call.rs`).
fn decline_label() -> String {
    tf("Refuser{contexte}", &[("contexte", "")])
}

fn slug(t: Tab) -> &'static str {
    TABS.iter().find(|x| x.0 == t).map_or("", |x| x.1)
}

fn shows(s: Store, t: Tab, r: &Relationship) -> bool {
    match t {
        Tab::Online => r.kind == RelationshipKind::Friend && s.dot_of(r.user.id) != Dot::Offline,
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
            .filter(|r| shows(s, tab.get(), r))
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
        // Les onglets se suivent de droite à gauche en RTL : la flèche droite revient en arrière (sens lu à l'événement).
        let forward = if crate::i18n::is_rtl() { "ArrowLeft" } else { "ArrowRight" };
        let next = match e.key().as_str() {
            k if k == forward => (i + 1) % n,
            "ArrowRight" | "ArrowLeft" => (i + n - 1) % n,
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
        .map(|&(which, id, label)| {
            let selected = move || tab.get() == which;
            view! {
                <button class="k-tab" role="tab" id=format!("tab-{id}") aria-controls="friends-panel"
                    aria-selected=move || selected().to_string() tabindex=move || if selected() { "0" } else { "-1" }
                    on:click=move |_| { tab.set(which); error.set(None); }>
                    {(which == Tab::Add).then(|| view! { <Icon icon=lu::LuUserPlus small=true/> })}{t(label)}
                    {(which == Tab::Pending).then(|| view! {
                        <Show when=move || { incoming.get() > 0 }>
                            <span class="k-badge k-badge--count">{move || incoming.get()}</span>
                            <span class="k-sr">" "{t("à accepter ou refuser")}</span>
                        </Show>
                    })}
                </button>
            }
        })
        .collect_view();

    view! {
        <Header>
            <Icon icon=lu::LuUsers/>
            <h1 class="app-title">{t("Amis")}</h1>
            <div class="k-tabs" role="tablist" aria-label=t("Filtrer les amis") node_ref=tablist on:keydown=keys>{tabs}</div>
        </Header>
        <div class="app-body">
            <section class="app-friends" role="tabpanel" id="friends-panel" aria-labelledby=move || format!("tab-{}", slug(tab.get()))>
                {move || if tab.get() == Tab::Add {
                    view! { <AddFriend/> }.into_any()
                } else {
                    view! {
                        <div class="k-search app-search">
                            <Icon icon=lu::LuSearch/>
                            <input dir="auto" class="k-input" type="search" placeholder=t("Chercher") aria-label=t("Chercher parmi tes amis") bind:value=query/>
                        </div>
                        <h2 class="k-category">
                            {move || t(match tab.get() {
                                Tab::Online => tk!("En ligne"),
                                Tab::Pending => tk!("En attente"),
                                Tab::Blocked => tk!("Bloqués"),
                                _ => tk!("Tous"),
                            })}" "
                            <span class="k-badge k-badge--neutral">{move || rows.get().len()}</span>
                        </h2>
                        <Show when=move || error.get().is_some()>
                            <p class="k-error app-friends-error" role="alert">{move || error.get()}" "
                                <button class="k-btn k-btn--link" on:click=move |_| error.set(None)>{t("Fermer")}</button></p>
                        </Show>
                        {move || if s.conn.get() != Conn::Ready && s.friends.with(|f| f.is_empty()) {
                            view! { <div aria-busy="true">{(0..4).map(|_| view! { <div class="k-skeleton app-skel-row"></div> }).collect_view()}</div> }.into_any()
                        } else if rows.get().is_empty() {
                            let (title, line) = match tab.get() {
                                Tab::Online => (t("Personne en ligne pour l'instant"), t("Tes amis apparaîtront ici dès qu'ils se connectent.")),
                                Tab::Pending => (t("Aucune demande en attente"), t("Les demandes reçues et envoyées s'affichent ici.")),
                                Tab::Blocked => (t("Tu n'as bloqué personne"), t("Un compte bloqué ne peut plus t'écrire en privé ni te demander en ami.")),
                                _ => (t("Pas encore d'amis"), t("Ajoute quelqu'un avec son pseudo pour commencer.")),
                            };
                            view! { <div class="k-empty"><b>{title}</b><span>{line}</span></div> }.into_any()
                        } else {
                            view! {
                                <ul class="app-list">
                                    <For each=move || rows.get() key=|r| (r.user.id, r.kind as u8, r.status as u8, r.user.display_name.clone(), r.user.avatar.clone()) let:r>
                                        <FriendRow r=r error=error confirm=confirm/>
                                    </For>
                                </ul>
                            }.into_any()
                        }}
                    }.into_any()
                }}
            </section>
            <aside class="app-aside app-activity" aria-label=t("Activité")>
                <h2>{t("Où sont tes amis")}</h2>
                <div class="k-empty"><b>{t("Aucun salon vocal occupé")}</b>{t("Les salons vocaux où tes amis discutent s'afficheront ici, pour les rejoindre en un clic.")}</div>
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
        RelationshipKind::PendingIncoming => t("Demande reçue"),
        RelationshipKind::PendingOutgoing => t("Demande envoyée"),
        RelationshipKind::Blocked => t("Bloqué"),
        RelationshipKind::Friend => String::new(),
    };
    let friend = r.kind == RelationshipKind::Friend;
    let dot = Signal::derive(move || s.dot_of(uid));
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
                <button class="k-btn k-btn--icon" aria-label=tf("Écrire à {nom}", &[("nom", &name)]) title=t("Message") on:click=move |_| write()>
                    <Icon icon=lu::LuMessageCircle/>
                </button>
                <button class="k-btn k-btn--icon" aria-label=tf("Appeler {nom}", &[("nom", &name)]) title=move || crate::call::call_title(t("Appeler"))
                    aria-describedby=move || (!crate::voice::supported()).then(|| format!("call-no-{uid}"))
                    disabled=move || !crate::voice::supported() on:click=move |_| ring()>
                    <Icon icon=lu::LuPhone/>
                </button>
                {crate::call::call_note(&format!("call-no-{uid}"))}
                // Retirer et bloquer : rangés sous ⋯, pas au rang d'écrire et d'appeler.
                <MoreMenu label=tf("Plus d'actions pour {nom}", &[("nom", &name)])>
                    <button class="k-menuitem" on:click=move |_| confirm.set(Some(Confirm::RemoveFriend(u1.clone())))>
                        <Icon icon=lu::LuUserMinus/>{t("Retirer de tes amis")}
                    </button>
                    <button class="k-menuitem is-danger" aria-label=tf("Bloquer {nom}", &[("nom", &name)])
                        on:click=move |_| confirm.set(Some(Confirm::Block(u2.clone())))>
                        <Icon icon=lu::LuBan/>{t("Bloquer")}
                    </button>
                </MoreMenu>
            }
            .into_any()
        }
        RelationshipKind::PendingIncoming => view! {
            <button class="k-btn k-btn--icon app-accept" aria-label=tf("Accepter la demande de {nom}", &[("nom", &name)]) title=t("Accepter")
                disabled=move || busy.get() on:click=move |_| act(true)>
                <Icon icon=lu::LuCheck/>
            </button>
            <button class="k-btn k-btn--icon" aria-label=tf("Refuser la demande de {nom}", &[("nom", &name)]) title=decline_label()
                disabled=move || busy.get() on:click=move |_| act(false)>
                <Icon icon=lu::LuX/>
            </button>
            <button class="k-btn k-btn--icon app-link-danger" aria-label=tf("Bloquer {nom}", &[("nom", &name)]) title=t("Bloquer")
                on:click=move |_| confirm.set(Some(Confirm::Block(user.clone())))>
                <Icon icon=lu::LuBan/>
            </button>
        }
        .into_any(),
        RelationshipKind::PendingOutgoing => view! {
            <button class="k-btn k-btn--icon" aria-label=tf("Annuler la demande à {nom}", &[("nom", &name)]) title=t("Annuler la demande")
                disabled=move || busy.get() on:click=move |_| act(false)>
                <Icon icon=lu::LuX/>
            </button>
        }
        .into_any(),
        RelationshipKind::Blocked => view! {
            <button class="k-btn k-btn--sm" aria-label=tf("Débloquer {nom}", &[("nom", &name)]) disabled=move || busy.get() on:click=move |_| act(false)>
                {t("Débloquer")}
            </button>
        }
        .into_any(),
    };
    view! {
        <li class="app-row">
            {if friend {
                view! { <Avatar user=r.user.clone() size="" status=dot/> }.into_any()
            } else {
                view! { <Avatar user=r.user.clone() size=""/> }.into_any()
            }}
            <div class="app-ellipsis">
                <b><bdi>{r.user.name().to_string()}</bdi></b>
                <small>{move || {
                    let state = if friend { dot.get().label() } else { sub.clone() };
                    view! { <bdi dir="ltr">{format!("@{} · {state}", r.user.username)}</bdi> }
                }}</small>
            </div>
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
                        tf("{nom} t'avait déjà demandé : vous êtes maintenant amis.", &[("nom", &who)])
                    } else {
                        tf(
                            "Demande envoyée à {nom} (@{pseudo}). Elle apparaît dans « En attente ».",
                            &[("nom", &who), ("pseudo", &r.user.username)],
                        )
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
            <h2>{t("Ajouter un ami")}</h2>
            <p class="app-hint" id="add-h">{t("Tape son pseudo KRAKEN, celui qui commence par @ sous son nom.")}</p>
            <div class="app-add-row">
                <input dir="ltr" class="k-input" placeholder=t("pseudo") aria-label=t("Pseudo de ton ami") bind:value=name
                    autocomplete="off" autocapitalize="none" spellcheck="false" maxlength="33" required
                    aria-describedby="add-h" aria-invalid=move || error.get().is_some().then_some("true")
                    on:input=move |_| { error.set(None); done.set(None); }/>
                <button class="k-btn k-btn--primary" type="submit" disabled=move || name.with(|n| n.trim().is_empty()) || busy.get()
                    aria-busy=move || busy.get().to_string()>{t("Envoyer la demande")}</button>
            </div>
            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            <Show when=move || done.get().is_some()><p class="app-add-ok" role="status">{move || done.get()}</p></Show>
        </form>
    }
}
