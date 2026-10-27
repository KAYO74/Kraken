//! Social (F02, F03, F11) : choisir des amis pour un MP ou un groupe, renommer et quitter un
//! groupe, confirmer le retrait d'un ami ou un blocage.

use kraken_proto::{Channel, ChannelKind, Id, MAX_GROUP_MEMBERS, MAX_GROUP_NAME, RelationshipKind, User};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::icons::{Icon, lu};
use crate::store::store;
use crate::ui::{Avatar, modal};

/// Ouvre (ou crée) le MP avec cette personne, puis y va.
pub fn open_dm(uid: Id, error: RwSignal<Option<String>>) -> impl Fn() + Clone + 'static {
    let navigate = use_navigate();
    move || {
        let s = store();
        let navigate = navigate.clone();
        // Déjà dans la liste : rien à demander au serveur.
        let known = s.dms.with_untracked(|d| {
            d.iter().find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == uid)).map(|c| c.id)
        });
        if let Some(c) = known {
            navigate(&format!("/channels/@me/{c}"), Default::default());
            return;
        }
        spawn_local(async move {
            match crate::api::open_private(vec![uid], None).await {
                Ok(c) => {
                    let id = c.id;
                    s.upsert_dm(c);
                    navigate(&format!("/channels/@me/{id}"), Default::default());
                }
                Err(e) => error.set(Some(e.message)),
            }
        });
    }
}

/// Appelle cette personne : ouvre (ou crée) le MP avec elle, y lance l'appel, puis y va.
pub fn call_user(uid: Id, error: RwSignal<Option<String>>) -> impl Fn() + Clone + 'static {
    let navigate = use_navigate();
    move || {
        let s = store();
        let navigate = navigate.clone();
        let known = s.dms.with_untracked(|d| {
            d.iter().find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == uid)).map(|c| c.id)
        });
        let go = move |cid: Id| {
            s.voice.join(None, cid);
            navigate(&format!("/channels/@me/{cid}"), Default::default());
        };
        match known {
            Some(c) => go(c),
            None => spawn_local(async move {
                match crate::api::open_private(vec![uid], None).await {
                    Ok(c) => {
                        let id = c.id;
                        s.upsert_dm(c);
                        go(id);
                    }
                    Err(e) => error.set(Some(e.message)),
                }
            }),
        }
    }
}

/// Le titre d'un MP ou d'un groupe : son nom, sinon les noms des autres.
pub fn private_title(c: &Channel) -> String {
    c.name.clone().unwrap_or_else(|| {
        let names: Vec<String> = c.recipients.iter().map(|u| u.name().to_string()).collect();
        if names.is_empty() { "Groupe vide".into() } else { names.join(", ") }
    })
}

/// Pour quoi on choisit des amis.
#[derive(Clone, PartialEq)]
pub enum PickFor {
    /// Un MP (une personne) ou un nouveau groupe (plusieurs), avec ces amis déjà cochés.
    New(Vec<Id>),
    /// Compléter ce groupe : ses membres actuels (moi compris) n'y sont pas proposés.
    Group { channel: Id, members: Vec<Id> },
}

/// Choisir des amis : une case par ami, recherche, compteur, nom du groupe dès deux personnes.
#[component]
pub fn PeoplePicker(purpose: PickFor, close: Callback<()>) -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let root = NodeRef::new();
    modal(root, move || {
        close.try_run(());
    });
    let (start, exclude, room, adding) = match &purpose {
        PickFor::New(pre) => (pre.clone(), vec![], MAX_GROUP_MEMBERS - 1, None),
        PickFor::Group { channel, members } => {
            (vec![], members.clone(), MAX_GROUP_MEMBERS.saturating_sub(members.len()), Some(*channel))
        }
    };
    let picked = RwSignal::new(start);
    let query = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let friends = Memo::new(move |_| {
        let q = query.get().trim().to_lowercase();
        s.friends.with(|f| {
            f.iter()
                .filter(|r| r.kind == RelationshipKind::Friend && !exclude.contains(&r.user.id))
                .map(|r| r.user.clone())
                .filter(|u| q.is_empty() || u.name().to_lowercase().contains(&q) || u.username.contains(&q))
                .collect::<Vec<User>>()
        })
    });
    let count = move || picked.with(Vec::len);
    let group = move || adding.is_none() && count() >= 2;
    let action = move || match (adding, count()) {
        (Some(_), _) => "Ajouter au groupe",
        (None, 0 | 1) => "Ouvrir la conversation",
        (None, _) => "Créer le groupe",
    };
    let title = if adding.is_some() { "Ajouter des amis au groupe" } else { "Nouvelle conversation" };
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let ids = picked.get_untracked();
        if ids.is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        let group_name = Some(name.get_untracked().trim().to_string()).filter(|n| !n.is_empty() && ids.len() > 1);
        spawn_local(async move {
            let done = match adding {
                Some(cid) => {
                    let mut out = Ok(cid);
                    for u in ids {
                        if let Err(e) = crate::api::add_recipient(cid, u).await {
                            out = Err(e);
                            break;
                        }
                    }
                    out
                }
                None => crate::api::open_private(ids, group_name).await.map(|c| {
                    let id = c.id;
                    s.upsert_dm(c);
                    id
                }),
            };
            busy.set(false);
            match done {
                Ok(id) => {
                    close.try_run(());
                    navigate(&format!("/channels/@me/{id}"), Default::default());
                }
                Err(e) => error.set(Some(e.message)),
            }
        });
    };
    view! {
        <div class="k-scrim" node_ref=root on:click=move |_| { close.try_run(()); }>
            <div class="k-dialog app-picker-dialog" role="dialog" aria-modal="true" aria-labelledby="pick-t" aria-describedby="pick-h"
                on:click=|e| e.stop_propagation()>
                <header><h2 id="pick-t">{title}</h2></header>
                <form on:submit=submit>
                    <div>
                        <p id="pick-h">{if adding.is_some() {
                            format!("Un groupe réunit {MAX_GROUP_MEMBERS} personnes au plus : encore {room} place(s).")
                        } else {
                            format!("Un ami pour un message privé, plusieurs pour un groupe ({} au plus).", MAX_GROUP_MEMBERS - 1)
                        }}</p>
                        <div class="k-search">
                            <Icon icon=lu::LuSearch/>
                            <input class="k-input" type="search" placeholder="Chercher un ami" aria-label="Chercher parmi tes amis"
                                bind:value=query autofocus/>
                        </div>
                        <fieldset class="app-pick-list">
                            <legend class="k-sr">"Tes amis"</legend>
                            {move || if friends.with(Vec::is_empty) {
                                let line = if s.friends.with(|f| f.iter().any(|r| r.kind == RelationshipKind::Friend)) {
                                    "Aucun ami ne correspond, ou ils sont déjà tous dans ce groupe."
                                } else {
                                    "Ajoute d'abord des amis, depuis l'onglet « Ajouter un ami »."
                                };
                                view! { <p class="app-hint">{line}</p> }.into_any()
                            } else {
                                view! {
                                    <For each=move || friends.get() key=|u| u.id let:u>
                                        {
                                            let id = u.id;
                                            let on = move || picked.with(|p| p.contains(&id));
                                            view! {
                                                <label class="app-pick">
                                                    <input type="checkbox" prop:checked=on
                                                        disabled=move || { !on() && count() >= room }
                                                        on:change=move |_| picked.update(|p| if p.contains(&id) { p.retain(|x| *x != id) } else { p.push(id) })/>
                                                    <Avatar user=u.clone() size="md"/>
                                                    <span class="app-ellipsis"><b>{u.name().to_string()}</b><small>{format!("@{}", u.username)}</small></span>
                                                </label>
                                            }
                                        }
                                    </For>
                                }.into_any()
                            }}
                        </fieldset>
                        <p class="app-hint" role="status">{move || format!("{} sur {room} choisi(s)", count())}</p>
                        <Show when=group>
                            <label class="k-field">
                                <span class="k-label">"Nom du groupe (facultatif)"</span>
                                <input class="k-input" maxlength=MAX_GROUP_NAME.to_string() placeholder="Les prénoms des membres, sinon" bind:value=name/>
                            </label>
                        </Show>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); }>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--primary" disabled=move || count() == 0 || busy.get()
                            aria-busy=move || busy.get().to_string()>{action}</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}

/// Renommer un groupe. Vide : on revient aux noms des membres.
#[component]
pub fn RenameGroup(channel: Channel, close: Callback<()>) -> impl IntoView {
    let s = store();
    let root = NodeRef::new();
    modal(root, move || {
        close.try_run(());
    });
    let name = RwSignal::new(channel.name.clone().unwrap_or_default());
    let error = RwSignal::new(None::<String>);
    let cid = channel.id;
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        spawn_local(async move {
            match crate::api::rename_group(cid, &name.get_untracked()).await {
                Ok(c) => {
                    close.try_run(());
                    s.upsert_dm(c);
                }
                Err(e) => error.set(Some(e.message)),
            }
        });
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog" role="dialog" aria-modal="true" aria-labelledby="ren-t">
                <header><h2 id="ren-t">"Renommer le groupe"</h2></header>
                <form on:submit=submit>
                    <div>
                        <label class="k-field">
                            <span class="k-label">"Nom du groupe"</span>
                            <input class="k-input" maxlength=MAX_GROUP_NAME.to_string() bind:value=name autofocus
                                aria-describedby="ren-h" aria-invalid=move || error.get().is_some().then_some("true")/>
                        </label>
                        <p class="app-hint" id="ren-h">"Laisse vide pour afficher les prénoms des membres. Tout le groupe voit le changement."</p>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); }>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--primary">"Enregistrer"</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}

/// Ce qu'on demande de confirmer.
#[derive(Clone, PartialEq)]
pub enum Confirm {
    RemoveFriend(User),
    Block(User),
    LeaveGroup(Channel),
}

/// Confirmer un retrait d'ami, un blocage ou un départ de groupe.
#[component]
pub fn ConfirmDialog(what: Confirm, close: Callback<()>) -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let root = NodeRef::new();
    modal(root, move || {
        close.try_run(());
    });
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let (title, body, action) = match &what {
        Confirm::RemoveFriend(u) => (
            format!("Retirer {} de tes amis ?", u.name()),
            "Vos messages privés restent. Pour redevenir amis, l'un de vous devra renvoyer une demande.".to_string(),
            "Retirer",
        ),
        Confirm::Block(u) => (
            format!("Bloquer {} ?", u.name()),
            "Vous ne serez plus amis. Ce compte ne pourra plus t'écrire en privé ni te demander en ami, et ses messages seront repliés dans les salons que vous partagez. Il n'en est pas prévenu.".to_string(),
            "Bloquer",
        ),
        Confirm::LeaveGroup(c) => (
            format!("Quitter « {} » ?", private_title(c)),
            "Tu ne verras plus ce groupe ni ses messages. Pour y revenir, il faudra qu'un membre t'y ajoute.".to_string(),
            "Quitter le groupe",
        ),
    };
    let what = StoredValue::new(what);
    let confirm = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        let navigate = navigate.clone();
        spawn_local(async move {
            let what = what.get_value();
            let done = match &what {
                Confirm::RemoveFriend(u) => crate::api::remove_relationship(u.id).await.map(|()| None),
                Confirm::Block(u) => crate::api::set_relationship(u.id, RelationshipKind::Blocked).await.map(Some),
                Confirm::LeaveGroup(c) => crate::api::leave_group(c.id).await.map(|()| None),
            };
            match done {
                // Fermer d'abord : la suite peut retirer l'écran qui porte ce dialogue.
                Ok(r) => {
                    close.try_run(());
                    match what {
                        Confirm::RemoveFriend(u) => s.remove_rel(u.id),
                        Confirm::Block(_) => r.into_iter().for_each(|r| s.upsert_rel(r)),
                        Confirm::LeaveGroup(c) => {
                            navigate("/channels/@me", Default::default());
                            s.dms.update(|d| d.retain(|x| x.id != c.id));
                        }
                    }
                }
                Err(e) => {
                    busy.try_set(false);
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog" role="alertdialog" aria-modal="true" aria-labelledby="cf-t" aria-describedby="cf-d">
                <header><h2 id="cf-t">{title}</h2></header>
                <form on:submit=confirm>
                    <div>
                        <p id="cf-d">{body}</p>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); } autofocus>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--danger" aria-busy=move || busy.get().to_string()>{action}</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}
