//! Social (F02, F03, F11) : choisir des amis pour un MP ou un groupe, renommer et quitter un
//! groupe, confirmer le retrait d'un ami, un blocage, un départ de serveur ou une sanction.

use kraken_proto::{
    BanMember, Channel, ChannelKind, Id, MAX_GROUP_MEMBERS, MAX_GROUP_NAME, MAX_REASON, RelationshipKind, Role, User,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::i18n::{t, tf};
use crate::icons::{Icon, lu};
use crate::store::store;
use crate::tk;
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
        if names.is_empty() { t("Groupe vide") } else { names.join(", ") }
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
    let action = move || {
        t(match (adding, count()) {
            (Some(_), _) => tk!("Ajouter au groupe"),
            (None, 0 | 1) => tk!("Ouvrir la conversation"),
            (None, _) => tk!("Créer le groupe"),
        })
    };
    let title = t(if adding.is_some() { tk!("Ajouter des amis au groupe") } else { tk!("Nouvelle conversation") });
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
                            tf(
                                "Un groupe réunit {max} personnes au plus : encore {room} place(s).",
                                &[("max", &MAX_GROUP_MEMBERS.to_string()), ("room", &room.to_string())],
                            )
                        } else {
                            tf(
                                "Un ami pour un message privé, plusieurs pour un groupe ({max} au plus).",
                                &[("max", &(MAX_GROUP_MEMBERS - 1).to_string())],
                            )
                        }}</p>
                        <div class="k-search">
                            <Icon icon=lu::LuSearch/>
                            <input dir="auto" class="k-input" type="search" placeholder=t("Chercher un ami") aria-label=t("Chercher parmi tes amis")
                                bind:value=query autofocus/>
                        </div>
                        <fieldset class="app-pick-list">
                            <legend class="k-sr">{t("Tes amis")}</legend>
                            {move || if friends.with(Vec::is_empty) {
                                let line = t(if s.friends.with(|f| f.iter().any(|r| r.kind == RelationshipKind::Friend)) {
                                    tk!("Aucun ami ne correspond, ou ils sont déjà tous dans ce groupe.")
                                } else {
                                    tk!("Ajoute d'abord des amis, depuis l'onglet « Ajouter un ami ».")
                                });
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
                                                    <span class="app-ellipsis"><b>{u.name().to_string()}</b><small><bdi dir="ltr">{format!("@{}", u.username)}</bdi></small></span>
                                                </label>
                                            }
                                        }
                                    </For>
                                }.into_any()
                            }}
                        </fieldset>
                        <p class="app-hint" role="status">{move || tf("{n} sur {room} choisi(s)", &[("n", &count().to_string()), ("room", &room.to_string())])}</p>
                        <Show when=group>
                            <label class="k-field">
                                <span class="k-label">{t("Nom du groupe (facultatif)")}</span>
                                <input dir="auto" class="k-input" maxlength=MAX_GROUP_NAME.to_string() placeholder=t("Les prénoms des membres, sinon") bind:value=name/>
                            </label>
                        </Show>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); }>{t("Annuler")}</button>
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
                <header><h2 id="ren-t">{t("Renommer le groupe")}</h2></header>
                <form on:submit=submit>
                    <div>
                        <label class="k-field">
                            <span class="k-label">{t("Nom du groupe")}</span>
                            <input dir="auto" class="k-input" maxlength=MAX_GROUP_NAME.to_string() bind:value=name autofocus
                                aria-describedby="ren-h" aria-invalid=move || error.get().is_some().then_some("true")/>
                        </label>
                        <p class="app-hint" id="ren-h">{t("Laisse vide pour afficher les prénoms des membres. Tout le groupe voit le changement.")}</p>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); }>{t("Annuler")}</button>
                        <button type="submit" class="k-btn k-btn--primary">{t("Enregistrer")}</button>
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
    /// Le propriétaire retire quelqu'un d'un groupe.
    RemoveMember(Channel, User),
    /// Quitter un serveur (id, nom, c'est le mien : il faut d'abord le transférer).
    LeaveGuild(Id, String, bool),
    /// Exclure du serveur (il peut revenir par invitation).
    Kick(Id, User),
    /// Bannir du serveur, avec une raison et l'effacement de ses derniers messages.
    Ban(Id, User),
    DeleteRole(Id, Role),
    /// Supprimer un salon de serveur, ou une catégorie (ses salons en sortent).
    DeleteChannel(Channel),
    /// Donner le serveur (id, nom) à ce membre.
    Transfer(Id, String, User),
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
            tf("Retirer {nom} de tes amis ?", &[("nom", u.name())]),
            t("Vos messages privés restent. Pour redevenir amis, l'un de vous devra renvoyer une demande."),
            t("Retirer"),
        ),
        Confirm::Block(u) => (
            tf("Bloquer {nom} ?", &[("nom", u.name())]),
            t(
                "Vous ne serez plus amis. Ce compte ne pourra plus t'écrire en privé ni te demander en ami, et ses messages seront repliés dans les salons que vous partagez. Il n'en est pas prévenu.",
            ),
            t("Bloquer"),
        ),
        Confirm::LeaveGroup(c) => (
            tf("Quitter « {nom} » ?", &[("nom", &private_title(c))]),
            t("Tu ne verras plus ce groupe ni ses messages. Pour y revenir, il faudra qu'un membre t'y ajoute."),
            t("Quitter le groupe"),
        ),
        Confirm::RemoveMember(c, u) => (
            tf("Retirer {nom} du groupe ?", &[("nom", u.name())]),
            tf(
                "Cette personne ne verra plus « {groupe} » ni ses messages. Tu pourras l'y ajouter de nouveau plus tard.",
                &[("groupe", &private_title(c))],
            ),
            t("Retirer du groupe"),
        ),
        Confirm::LeaveGuild(_, name, true) => (
            tf("Quitter « {nom} » ?", &[("nom", name)]),
            t(
                "Tu es le propriétaire de ce serveur : tu ne peux pas le quitter tant qu'il t'appartient. Transfère-le d'abord à un autre membre, puis tu pourras partir.",
            ),
            t("Transférer la propriété"),
        ),
        Confirm::LeaveGuild(_, name, false) => (
            tf("Quitter « {nom} » ?", &[("nom", name)]),
            t(
                "Tu ne verras plus ce serveur, ses salons ni ses membres. Pour y revenir, il te faudra une nouvelle invitation, et tes rôles ne te seront pas rendus.",
            ),
            t("Quitter le serveur"),
        ),
        Confirm::Kick(_, u) => (
            tf("Expulser {nom} ?", &[("nom", u.name())]),
            t(
                "Cette personne sort du serveur et perd ses rôles, mais ses messages restent. Elle pourra revenir avec une invitation.",
            ),
            t("Expulser"),
        ),
        Confirm::Ban(_, u) => (
            tf("Bannir {nom} ?", &[("nom", u.name())]),
            t(
                "Cette personne sort du serveur et ne pourra plus le rejoindre par invitation, tant que tu ne l'auras pas débannie dans les réglages du serveur.",
            ),
            t("Bannir"),
        ),
        Confirm::DeleteRole(_, r) => (
            tf("Supprimer le rôle « {nom} » ?", &[("nom", &r.name)]),
            t("Tous ceux qui l'ont le perdent, avec les droits qu'il leur donnait. Cette action est définitive."),
            t("Supprimer le rôle"),
        ),
        Confirm::DeleteChannel(c) => {
            let name = c.name.clone().unwrap_or_default();
            match c.kind {
                ChannelKind::Category => (
                    tf("Supprimer la catégorie « {nom} » ?", &[("nom", &name)]),
                    tf(
                        "Les salons qu'elle contient ne sont pas supprimés : ils sortent de la catégorie.{suite} Cette action est définitive.",
                        &[(
                            "suite",
                            &if c.private {
                                format!(" {}", t("Ses salons gardent les mêmes règles d'accès."))
                            } else {
                                String::new()
                            },
                        )],
                    ),
                    t("Supprimer la catégorie"),
                ),
                ChannelKind::Voice | ChannelKind::Stage => (
                    tf("Supprimer le salon vocal « {nom} » ?", &[("nom", &name)]),
                    t(
                        "Le salon disparaît pour tout le monde, et les personnes qui s'y trouvent sont déconnectées. Cette action est définitive.",
                    ),
                    t("Supprimer le salon"),
                ),
                _ => (
                    tf("Supprimer #{nom} ?", &[("nom", &name)]),
                    t(
                        "Le salon et tous ses messages, avec leurs fichiers, sont supprimés pour tout le monde. Cette action est définitive.",
                    ),
                    t("Supprimer le salon"),
                ),
            }
        }
        Confirm::Transfer(_, name, u) => (
            tf("Donner « {serveur} » à {nom} ?", &[("serveur", name), ("nom", u.name())]),
            t(
                "Cette personne devient propriétaire : elle aura tous les droits, et tu seras un membre comme un autre. Tu ne pourras pas le reprendre sans son accord.",
            ),
            t("Transférer la propriété"),
        ),
    };
    let reason = RwSignal::new(String::new());
    let hours = RwSignal::new("0".to_string());
    let moderation = matches!(what, Confirm::Kick(..) | Confirm::Ban(..));
    let banning = matches!(what, Confirm::Ban(..));
    let owner_leave = matches!(what, Confirm::LeaveGuild(_, _, true));
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
                Confirm::RemoveMember(c, u) => crate::api::remove_member(c.id, u.id).await.map(|()| None),
                Confirm::LeaveGuild(g, _, true) => {
                    // Rien à confirmer : on va là où l'on transfère.
                    navigate(&format!("/guilds/{g}/settings/owner"), Default::default());
                    close.try_run(());
                    return;
                }
                Confirm::LeaveGuild(g, _, false) => crate::api::leave_guild(*g).await.map(|()| None),
                Confirm::Kick(g, u) => crate::api::kick(*g, u.id, reason.get_untracked().trim()).await.map(|()| None),
                Confirm::Ban(g, u) => {
                    let reason = Some(reason.get_untracked().trim().to_string()).filter(|r| !r.is_empty());
                    let delete_hours = hours.get_untracked().parse().ok().filter(|h| *h > 0);
                    crate::api::ban(*g, u.id, &BanMember { reason, delete_hours }).await.map(|_| None)
                }
                Confirm::DeleteRole(g, r) => crate::api::delete_role(*g, r.id).await.map(|()| None),
                Confirm::DeleteChannel(c) => crate::api::delete_channel(c.id).await.map(|()| None),
                Confirm::Transfer(g, _, u) => crate::api::transfer_guild(*g, u.id).await.map(|()| None),
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
                        Confirm::RemoveMember(c, u) => s.dms.update(|d| {
                            d.iter_mut().filter(|x| x.id == c.id).for_each(|x| x.recipients.retain(|r| r.id != u.id))
                        }),
                        Confirm::LeaveGuild(g, _, _) => {
                            navigate("/channels/@me", Default::default());
                            s.guilds.update(|gs| gs.retain(|x| x.guild.id != g));
                        }
                        Confirm::Kick(g, u) | Confirm::Ban(g, u) => {
                            s.in_guild(g, |x| x.members.retain(|m| m.user.id != u.id))
                        }
                        Confirm::DeleteRole(g, r) => s.apply_role_delete(g, r.id),
                        Confirm::DeleteChannel(c) => {
                            if let Some(g) = c.guild_id {
                                navigate(&format!("/channels/{g}"), Default::default());
                            }
                            s.remove_channel(c.id);
                        }
                        Confirm::Transfer(g, _, u) => s.in_guild(g, |x| x.guild.owner_id = u.id),
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
                        {moderation.then(|| view! {
                            <div class="k-field">
                                <label class="k-label" for="cf-reason">{t("Raison (facultatif)")}</label>
                                <input dir="auto" id="cf-reason" class="k-input" maxlength=MAX_REASON autocomplete="off"
                                    prop:value=move || reason.get() on:input=move |e| reason.set(event_target_value(&e))/>
                            </div>
                        })}
                        {banning.then(|| view! {
                            <div class="k-field">
                                <label class="k-label" for="cf-hours">{t("Supprimer ses messages")}</label>
                                <select id="cf-hours" class="k-select" prop:value=move || hours.get()
                                    on:change=move |e| hours.set(event_target_value(&e))>
                                    // « Rien » et « 24 h » ont déjà une autre traduction au catalogue (un niveau de notification,
                                    // le format de l'heure) : le `{contexte}` vide en fait des entrées à part, invisibles en français.
                                    <option value="0">{tf("Rien{contexte}", &[("contexte", "")])}</option>
                                    <option value="24">{tf("24 h{contexte}", &[("contexte", "")])}</option>
                                    <option value="168">{t("7 jours")}</option>
                                </select>
                            </div>
                        })}
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); } autofocus>{t("Annuler")}</button>
                        <button type="submit" class=if owner_leave { "k-btn k-btn--primary" } else { "k-btn k-btn--danger" } aria-busy=move || busy.get().to_string()>{action}</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}
