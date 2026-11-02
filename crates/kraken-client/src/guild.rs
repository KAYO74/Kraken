//! Paramètres d'un serveur (S22) : rôles et leurs droits, bannis, propriété ; et la fiche d'un
//! membre où l'on lui donne ou retire des rôles. Le serveur reste seul juge de la hiérarchie :
//! l'interface désactive ce qui sera refusé, sans plus.

use gloo_timers::future::TimeoutFuture;
use kraken_core::perms;
use kraken_proto::{CreateRole, EditRole, Id, ROLE_COLORS, Role};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use wasm_bindgen::JsCast;

use crate::icons::{Icon, lu};
use crate::social::{Confirm, ConfirmDialog};
use crate::store::{Conn, store};
use crate::ui::{Avatar, modal, when};

/// Nom de chaque couleur de la palette (numéro − 1) : la teinte vient du thème, `--avatar-N-text`.
pub const COLOR_NAMES: [&str; ROLE_COLORS as usize] =
    ["Rouge", "Or", "Citron vert", "Vert", "Turquoise", "Bleu", "Violet", "Rose"];

type Perm = (u64, &'static str, &'static str);

/// Les droits par catégorie, avec ce que chacun permet.
const CATEGORIES: &[(&str, &[Perm])] = &[
    (
        "Général",
        &[
            (perms::VIEW_CHANNEL, "Voir les salons", "Voir les salons et lire ce qui s'y dit."),
            (perms::MANAGE_CHANNELS, "Gérer les salons", "Créer, renommer, déplacer et supprimer des salons."),
            (perms::MANAGE_GUILD, "Gérer le serveur", "Changer le nom, l'icône et les réglages du serveur."),
            (
                perms::MANAGE_ROLES,
                "Gérer les rôles",
                "Créer des rôles, les modifier et les donner, à condition qu'ils soient plus bas que son propre rôle.",
            ),
            (perms::MANAGE_EMOJIS, "Gérer les émojis", "Ajouter et retirer les émojis et les autocollants du serveur."),
            (perms::MANAGE_EVENTS, "Gérer les événements", "Créer, modifier et annuler les événements."),
            (perms::VIEW_AUDIT_LOG, "Voir le journal d'audit", "Consulter l'historique des actions de modération."),
            (perms::CREATE_INVITE, "Créer des invitations", "Inviter de nouvelles personnes avec un lien."),
            (
                perms::ADMINISTRATOR,
                "Administrateur",
                "Tous les droits, sans exception. À réserver à des personnes de confiance.",
            ),
        ],
    ),
    (
        "Messages",
        &[
            (perms::SEND_MESSAGES, "Écrire des messages", "Envoyer des messages dans les salons textuels."),
            (perms::SEND_IN_THREADS, "Écrire dans les fils", "Répondre dans les fils de discussion."),
            (perms::CREATE_THREADS, "Créer des fils", "Ouvrir un nouveau fil à partir d'un message."),
            (perms::EMBED_LINKS, "Aperçu des liens", "Faire afficher un aperçu des liens postés."),
            (perms::ATTACH_FILES, "Joindre des fichiers", "Envoyer des images et des fichiers."),
            (perms::ADD_REACTIONS, "Ajouter des réactions", "Réagir à un message avec un émoji."),
            (
                perms::MENTION_EVERYONE,
                "Mentionner tout le monde",
                "Prévenir tous les membres d'un coup avec @everyone.",
            ),
            (perms::MANAGE_MESSAGES, "Gérer les messages", "Supprimer les messages des autres."),
            (perms::READ_HISTORY, "Lire l'historique", "Relire les messages envoyés avant son arrivée."),
        ],
    ),
    (
        "Vocal",
        &[
            (perms::CONNECT, "Se connecter", "Rejoindre les salons vocaux."),
            (perms::SPEAK, "Parler", "Prendre la parole dans un salon vocal."),
            (perms::VIDEO, "Caméra", "Allumer sa caméra."),
            (perms::STREAM, "Partager son écran", "Diffuser son écran dans un salon vocal."),
            (perms::USE_SOUNDBOARD, "Table de sons", "Jouer des sons de la table du serveur."),
            (perms::PRIORITY_SPEAKER, "Voix prioritaire", "Baisser le volume des autres quand on parle."),
            (perms::MUTE_MEMBERS, "Couper le micro des autres", "Rendre muet un membre dans un salon vocal."),
            (perms::MOVE_MEMBERS, "Déplacer des membres", "Faire passer un membre d'un salon vocal à un autre."),
        ],
    ),
    (
        "Membres",
        &[
            (perms::CHANGE_NICK, "Changer son surnom", "Choisir son propre surnom sur ce serveur."),
            (perms::MANAGE_NICKS, "Gérer les surnoms", "Changer le surnom des autres membres."),
            (perms::KICK, "Expulser des membres", "Faire sortir un membre, qui peut revenir avec une invitation."),
            (perms::BAN, "Bannir des membres", "Faire sortir un membre pour de bon, et voir la liste des bannis."),
            (perms::TIMEOUT, "Exclure temporairement", "Empêcher un membre de parler pendant un moment."),
        ],
    ),
];

const SECTIONS: [(&str, &str); 3] = [("roles", "Rôles"), ("bans", "Bannis"), ("owner", "Propriété")];

/// Sections que je peux ouvrir dans ce serveur.
fn allowed(guild: Id) -> Vec<&'static str> {
    let s = store();
    let p = s.guild_perms(guild);
    let owner = s.my_id().is_some() && s.guild(guild).is_some_and(|g| Some(g.guild.owner_id) == s.my_id());
    SECTIONS
        .iter()
        .map(|(slug, _)| *slug)
        .filter(|slug| match *slug {
            "roles" => p & perms::MANAGE_ROLES != 0,
            "bans" => p & perms::BAN != 0,
            _ => owner,
        })
        .collect()
}

#[component]
pub fn GuildSettings() -> impl IntoView {
    let s = store();
    let params = use_params_map();
    let gid = Memo::new(move |_| params.read().get("guild").and_then(|g| g.parse::<Id>().ok()));
    let navigate = use_navigate();
    let close = move || {
        let to = gid.get_untracked().map_or("/channels/@me".to_string(), |g| format!("/channels/{g}"));
        navigate(&to, Default::default());
    };
    let close2 = close.clone();
    let root = NodeRef::new();
    modal(root, close.clone());
    let section = Memo::new(move |_| {
        let Some(g) = gid.get() else { return "" };
        let open = allowed(g);
        let asked = params.read().get("section").unwrap_or_default();
        open.iter().copied().find(|x| *x == asked).or(open.first().copied()).unwrap_or("")
    });
    let title = move || {
        gid.get().and_then(|g| s.guild(g)).map(|g| g.guild.name).unwrap_or_else(|| "Paramètres du serveur".into())
    };
    view! {
        <div class="k-scrim app-settings" node_ref=root>
            <div class="k-settings" role="dialog" aria-modal="true" aria-label="Paramètres du serveur">
                <nav aria-label="Sections des paramètres du serveur">
                    <h3>{title}</h3>
                    {move || gid.get().map(|g| {
                        let open = allowed(g);
                        SECTIONS.iter().filter(|(slug, _)| open.contains(slug)).map(|(slug, label)| view! {
                            <a class="k-navitem" href=format!("/guilds/{g}/settings/{slug}")
                                aria-current=move || (section.get() == *slug).then_some("page")>{*label}</a>
                        }).collect_view()
                    })}
                </nav>
                <main>
                    <div class="app-settings-top">
                        <h2>{move || SECTIONS.iter().find(|x| x.0 == section.get()).map_or("Paramètres du serveur", |x| x.1)}</h2>
                        <button class="k-btn k-btn--icon" aria-label="Fermer les paramètres (Échap)" on:click=move |_| close2() autofocus>
                            <Icon icon=lu::LuX/>
                        </button>
                    </div>
                    {move || {
                        let Some(g) = gid.get() else { return empty("Serveur introuvable", "Ce lien est peut-être ancien.").into_any() };
                        if s.guild(g).is_none() {
                            return if s.conn.get() == Conn::Ready {
                                empty("Serveur introuvable", "Tu n'en fais plus partie, ou il n'existe pas.").into_any()
                            } else {
                                view! { <div aria-busy="true"><div class="k-skeleton app-skel-block"></div></div> }.into_any()
                            };
                        }
                        match section.get() {
                            "roles" => view! { <Roles guild=g/> }.into_any(),
                            "bans" => view! { <Bans guild=g/> }.into_any(),
                            "owner" => view! { <Ownership guild=g/> }.into_any(),
                            _ => empty("Rien à régler ici", "Tes rôles ne te donnent accès à aucun réglage de ce serveur.").into_any(),
                        }
                    }}
                </main>
            </div>
        </div>
    }
}

fn empty(title: &'static str, text: &'static str) -> impl IntoView {
    view! { <div class="k-empty"><b>{title}</b><span>{text}</span></div> }
}

/// Le rôle, tel que le store le connaît (suivi).
fn role_of(guild: Id, id: Id) -> Option<Role> {
    store().guilds.with(|gs| gs.iter().find(|g| g.guild.id == guild)?.roles.iter().find(|r| r.id == id).cloned())
}

/// Nouvel ordre après avoir déplacé l'élément `from` à la place `to`.
fn moved(ids: &[Id], from: usize, to: usize) -> Vec<Id> {
    let mut v = ids.to_vec();
    let x = v.remove(from);
    v.insert(to, x);
    v
}

/// Rend le focus à un bouton qu'un déplacement vient de redessiner (`fallback` s'il est devenu inactif).
async fn refocus(prefer: String, fallback: String) {
    TimeoutFuture::new(60).await;
    for id in [prefer, fallback] {
        if let Some(el) = document().get_element_by_id(&id)
            && !el.has_attribute("disabled")
            && let Ok(h) = el.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = h.focus();
            return;
        }
    }
}

#[component]
fn Roles(guild: Id) -> impl IntoView {
    let s = store();
    let selected = RwSignal::new(None::<Id>);
    let error = RwSignal::new(None::<String>);
    let status = RwSignal::new(String::new());
    let dragging = RwSignal::new(None::<Id>);
    let over = RwSignal::new(None::<Id>);
    let busy = RwSignal::new(false);
    // Les rôles personnalisés, du plus haut au plus bas ; `@everyone` est toujours dessous.
    let roles = Memo::new(move |_| {
        s.guild(guild)
            .map(|g| {
                let mut r: Vec<Role> = g.roles.into_iter().filter(|r| r.id != guild).collect();
                r.sort_by_key(|r| std::cmp::Reverse(r.position));
                r
            })
            .unwrap_or_default()
    });
    let ids = move || roles.with(|v| v.iter().map(|r| r.id).collect::<Vec<_>>());
    // Sélection par défaut, ou si le rôle choisi a disparu.
    Effect::new(move || {
        let cur = selected.get();
        let known = cur.is_some_and(|c| c == guild || roles.with(|v| v.iter().any(|r| r.id == c)));
        if !known {
            selected.set(Some(roles.with(|v| v.first().map(|r| r.id)).unwrap_or(guild)));
        }
    });
    let manageable = move |i: usize| roles.with(|v| v.get(i).is_some_and(|r| s.can_manage_role(guild, r)));
    let reorder = move |order: Vec<Id>, focus: Option<(String, String)>| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match crate::api::reorder_roles(guild, order).await {
                Ok(changed) => {
                    s.in_guild(guild, |g| {
                        for c in changed {
                            if let Some(r) = g.roles.iter_mut().find(|r| r.id == c.id) {
                                *r = c;
                            }
                        }
                    });
                    status.set("Ordre des rôles modifié.".into());
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
            if let Some((a, b)) = focus {
                refocus(a, b).await;
            }
        });
    };
    let step = move |id: Id, delta: isize| {
        let order = ids();
        let Some(from) = order.iter().position(|x| *x == id) else { return };
        let to = from as isize + delta;
        if to < 0 || to as usize >= order.len() {
            return;
        }
        let (up, down) = (format!("role-up-{id}"), format!("role-down-{id}"));
        let focus = if delta < 0 { (up.clone(), down.clone()) } else { (down, up) };
        reorder(moved(&order, from, to as usize), Some(focus));
        status.set(String::new());
    };
    let create = move |_| {
        error.set(None);
        spawn_local(async move {
            let req = CreateRole { name: "Nouveau rôle".into(), color: None, hoist: false, permissions: 0 };
            match crate::api::create_role(guild, &req).await {
                Ok(r) => {
                    let id = r.id;
                    s.in_guild(guild, |g| {
                        if !g.roles.iter().any(|x| x.id == id) {
                            g.roles.push(r);
                        }
                    });
                    selected.set(Some(id));
                    TimeoutFuture::new(60).await;
                    if let Some(i) = document()
                        .get_element_by_id("role-name")
                        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
                    {
                        let _ = i.focus();
                        i.select();
                    }
                }
                Err(e) => error.set(Some(e.message)),
            }
        });
    };
    let can_create = move || s.guild_perms(guild) & perms::MANAGE_ROLES != 0;
    view! {
        <p class="app-hint">"Plus un rôle est haut dans la liste, plus il est puissant. On ne peut modifier, donner ni déplacer que les rôles plus bas que le sien."</p>
        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
        <p class="k-sr" role="status">{move || status.get()}</p>
        <div class="app-roles">
            <div class="app-roles-list">
                <button class="k-btn k-btn--primary" on:click=create disabled=move || !can_create()>
                    <Icon icon=lu::LuPlus small=true/>"Créer un rôle"
                </button>
                <ul aria-label="Rôles, du plus haut au plus bas">
                    <For each=ids key=|id| *id let:id>
                        {
                            let role = move || role_of(guild, id);
                            let index = move || roles.with(|v| v.iter().position(|r| r.id == id));
                            let can_move = move |delta: isize| {
                                let Some(i) = index() else { return false };
                                let j = i as isize + delta;
                                j >= 0 && manageable(i) && manageable(j as usize)
                            };
                            let name = move || role().map(|r| r.name).unwrap_or_default();
                            let drag_ok = move || index().is_some_and(manageable);
                            let drop_on = move |e: leptos::ev::DragEvent| {
                                e.prevent_default();
                                over.set(None);
                                let Some(from_id) = dragging.get_untracked() else { return };
                                dragging.set(None);
                                let order = ids();
                                let (Some(from), Some(to)) = (order.iter().position(|x| *x == from_id), order.iter().position(|x| *x == id)) else { return };
                                if from != to {
                                    reorder(moved(&order, from, to), None);
                                }
                            };
                            view! {
                                <li class="app-role" draggable=move || drag_ok().to_string()
                                    data-over=move || (over.get() == Some(id) && dragging.get() != Some(id)).then_some("")
                                    data-dragging=move || (dragging.get() == Some(id)).then_some("")
                                    on:dragstart=move |e: leptos::ev::DragEvent| {
                                        if !drag_ok() { e.prevent_default(); return }
                                        dragging.set(Some(id));
                                        if let Some(dt) = e.data_transfer() {
                                            dt.set_effect_allowed("move");
                                            let _ = dt.set_data("text/plain", &id.to_string());
                                        }
                                    }
                                    on:dragover=move |e: leptos::ev::DragEvent| {
                                        if dragging.get_untracked().is_some() && drag_ok() {
                                            e.prevent_default();
                                            over.set(Some(id));
                                        }
                                    }
                                    on:dragleave=move |_| if over.get_untracked() == Some(id) { over.set(None) }
                                    on:dragend=move |_| { dragging.set(None); over.set(None); }
                                    on:drop=drop_on>
                                    <span class="app-role-grip" aria-hidden="true"><Icon icon=lu::LuGripVertical small=true/></span>
                                    <button class="app-role-pick" aria-current=move || (selected.get() == Some(id)).then_some("true")
                                        on:click=move |_| selected.set(Some(id))>
                                        <i class="app-role-dot" data-color=move || role().and_then(|r| r.color).map(|c| c.to_string())></i>
                                        <span class="app-ellipsis">{name}</span>
                                    </button>
                                    <button class="k-btn k-btn--icon" id=format!("role-up-{id}") disabled=move || !can_move(-1)
                                        aria-label=move || format!("Monter le rôle {}", name()) title="Monter"
                                        on:click=move |_| step(id, -1)><Icon icon=lu::LuChevronUp small=true/></button>
                                    <button class="k-btn k-btn--icon" id=format!("role-down-{id}") disabled=move || !can_move(1)
                                        aria-label=move || format!("Descendre le rôle {}", name()) title="Descendre"
                                        on:click=move |_| step(id, 1)><Icon icon=lu::LuChevronDown small=true/></button>
                                </li>
                            }
                        }
                    </For>
                    <li class="app-role app-role--base">
                        <button class="app-role-pick" aria-current=move || (selected.get() == Some(guild)).then_some("true")
                            on:click=move |_| selected.set(Some(guild))>
                            <i class="app-role-dot"></i><span class="app-ellipsis">"@everyone"</span>
                        </button>
                    </li>
                </ul>
            </div>
            <div class="app-roles-edit">
                {move || selected.get().map(|id| view! { <RoleEditor guild=guild role=id selected=selected/> })}
            </div>
        </div>
    }
}

#[component]
fn RoleEditor(guild: Id, role: Id, selected: RwSignal<Option<Id>>) -> impl IntoView {
    let s = store();
    let everyone = role == guild;
    let current = move || role_of(guild, role);
    let editable = move || {
        let p = s.guild_perms(guild);
        p & perms::MANAGE_ROLES != 0 && current().is_some_and(|r| everyone || s.can_manage_role(guild, &r))
    };
    let error = RwSignal::new(None::<String>);
    let saved = RwSignal::new(false);
    let deleting = RwSignal::new(false);
    let name = RwSignal::new(current().map(|r| r.name).unwrap_or_default());
    Effect::new(move || {
        if let Some(r) = current() {
            name.set(r.name);
        }
    });
    let apply = move |req: EditRole| {
        error.set(None);
        saved.set(false);
        spawn_local(async move {
            match crate::api::edit_role(guild, role, &req).await {
                Ok(r) => {
                    s.in_guild(guild, |g| {
                        if let Some(x) = g.roles.iter_mut().find(|x| x.id == r.id) {
                            *x = r;
                        }
                    });
                    saved.set(true);
                }
                Err(e) => error.set(Some(e.message)),
            }
        });
    };
    let rename = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        apply(EditRole { name: Some(name.get_untracked()), ..Default::default() });
    };
    let my_base = move || s.guild_perms(guild);
    view! {
        <Show when=move || current().is_some()>
            <h3 class="app-roles-title">{move || if everyone { "@everyone".to_string() } else { current().map(|r| r.name).unwrap_or_default() }}</h3>
            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            <Show when=move || saved.get()><p class="app-hint" role="status">"Enregistré."</p></Show>
            {(!everyone).then(|| view! {
                <form class="k-field" on:submit=rename>
                    <label class="k-label" for="role-name">"Nom du rôle"</label>
                    <div class="app-add-row">
                        <input id="role-name" class="k-input" maxlength="100" autocomplete="off" disabled=move || !editable()
                            prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e))/>
                        <button type="submit" class="k-btn" disabled=move || !editable()>"Renommer"</button>
                    </div>
                </form>
                <div class="k-field">
                    <span class="k-label" id="role-color-l">"Couleur"</span>
                    <div class="app-swatches" role="radiogroup" aria-labelledby="role-color-l">
                        <button type="button" role="radio" class="k-swatch app-swatch" data-color="0" aria-label="Aucune couleur" title="Aucune"
                            aria-checked=move || current().is_some_and(|r| r.color.is_none()).to_string() disabled=move || !editable()
                            on:click=move |_| apply(EditRole { color: Some(0), ..Default::default() })>
                            <Icon icon=lu::LuBan small=true/>
                        </button>
                        {(1..=ROLE_COLORS).map(|c| {
                            let label = COLOR_NAMES[c as usize - 1];
                            let on = move || current().is_some_and(|r| r.color == Some(c));
                            view! {
                                <button type="button" role="radio" class="k-swatch app-swatch" data-color=c.to_string()
                                    aria-label=label title=label aria-checked=move || on().to_string() disabled=move || !editable()
                                    on:click=move |_| apply(EditRole { color: Some(c), ..Default::default() })>
                                    <Show when=on><Icon icon=lu::LuCheck small=true/></Show>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                </div>
                <div class="k-setting">
                    <label for="role-hoist">"Afficher à part dans la liste des membres"</label>
                    <input id="role-hoist" type="checkbox" role="switch" class="k-switch" aria-describedby="role-hoist-h"
                        disabled=move || !editable() prop:checked=move || current().is_some_and(|r| r.hoist)
                        on:change=move |e| apply(EditRole { hoist: Some(event_target_checked(&e)), ..Default::default() })/>
                    <p id="role-hoist-h">"Les membres qui ont ce rôle forment leur propre rubrique, au-dessus des autres."</p>
                </div>
            })}
            {CATEGORIES.iter().map(|(title, list)| view! {
                <fieldset class="app-perms">
                    <legend class="k-category">{*title}</legend>
                    {list.iter().map(|(bit, label, help)| {
                        let (bit, id) = (*bit, format!("perm-{bit}"));
                        // On n'accorde que ce que l'on a soi-même.
                        let allowed = move || editable() && (my_base() & bit != 0 || current().is_some_and(|r| r.permissions & bit != 0));
                        view! {
                            <div class="k-setting">
                                <label for=id.clone()>{*label}</label>
                                <input id=id.clone() type="checkbox" role="switch" class="k-switch" aria-describedby=format!("{id}-h")
                                    disabled=move || !allowed() prop:checked=move || current().is_some_and(|r| r.permissions & bit != 0)
                                    on:change=move |_| {
                                        let now = current().map_or(0, |r| r.permissions);
                                        apply(EditRole { permissions: Some(now ^ bit), ..Default::default() });
                                    }/>
                                <p id=format!("{id}-h")>{*help}</p>
                            </div>
                        }
                    }).collect_view()}
                </fieldset>
            }).collect_view()}
            {(!everyone).then(|| view! {
                <div class="app-roles-danger">
                    <button class="k-btn k-btn--danger" disabled=move || !editable() on:click=move |_| deleting.set(true)>
                        <Icon icon=lu::LuTrash2 small=true/>"Supprimer ce rôle"
                    </button>
                </div>
            })}
            {move || deleting.get().then(|| current().map(|r| view! {
                <ConfirmDialog what=Confirm::DeleteRole(guild, r) close=Callback::new(move |()| {
                    deleting.set(false);
                    if role_of(guild, role).is_none() { selected.set(None); }
                })/>
            }))}
        </Show>
    }
}

#[component]
fn Bans(guild: Id) -> impl IntoView {
    let s = store();
    let error = RwSignal::new(None::<String>);
    let loaded = RwSignal::new(false);
    spawn_local(async move {
        match crate::api::bans(guild).await {
            Ok(list) => {
                s.bans.update(|b| {
                    b.insert(guild, list);
                });
                loaded.try_set(true);
            }
            Err(e) => {
                error.try_set(Some(e.message));
            }
        }
    });
    let list = Memo::new(move |_| s.bans.with(|b| b.get(&guild).cloned().unwrap_or_default()));
    let unban = move |uid: Id| {
        error.set(None);
        spawn_local(async move {
            match crate::api::unban(guild, uid).await {
                Ok(()) => s.bans.update(|b| {
                    if let Some(l) = b.get_mut(&guild) {
                        l.retain(|x| x.user.id != uid);
                    }
                }),
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    view! {
        <p class="app-hint">"Une personne bannie ne peut plus rejoindre le serveur avec une invitation. Débannir lui rend cette possibilité."</p>
        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
        <Show when=move || loaded.get() && list.with(Vec::is_empty)>
            <div class="k-empty"><b>"Personne n'est banni"</b><span>"Les bannissements apparaîtront ici."</span></div>
        </Show>
        <ul class="app-members app-bans" aria-label="Personnes bannies">
            <For each=move || list.get() key=|b| (b.user.id, b.created_at) let:b>
                {
                    let uid = b.user.id;
                    let name = b.user.name().to_string();
                    view! {
                        <li class="k-dm app-ban">
                            <Avatar user=b.user.clone() size="md"/>
                            <div class="app-ellipsis">
                                {name.clone()}
                                <small>{format!("@{} · {}", b.user.username, when(b.created_at))}</small>
                                <small>{b.reason.clone().map(|r| format!("Raison : {r}")).unwrap_or_else(|| "Aucune raison donnée".into())}</small>
                            </div>
                            <button class="k-btn" aria-label=format!("Débannir {name}") on:click=move |_| unban(uid)>"Débannir"</button>
                        </li>
                    }
                }
            </For>
        </ul>
    }
}

#[component]
fn Ownership(guild: Id) -> impl IntoView {
    let s = store();
    let target = RwSignal::new(String::new());
    let confirm = RwSignal::new(None::<Confirm>);
    let others = Memo::new(move |_| {
        s.guild(guild)
            .map(|g| g.members.into_iter().filter(|m| Some(m.user.id) != s.my_id()).collect::<Vec<_>>())
            .unwrap_or_default()
    });
    let ask = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Ok(uid) = target.get_untracked().parse::<Id>() else { return };
        let (Some(g), Some(m)) =
            (s.guild(guild), others.with_untracked(|o| o.iter().find(|m| m.user.id == uid).cloned()))
        else {
            return;
        };
        confirm.set(Some(Confirm::Transfer(guild, g.guild.name, m.user)));
    };
    view! {
        <p class="app-hint">"Le propriétaire a tous les droits et ne peut pas être sanctionné. Pour quitter le serveur, il faut d'abord le confier à un autre membre."</p>
        <form class="k-field" on:submit=ask>
            <label class="k-label" for="owner-to">"Nouveau propriétaire"</label>
            <div class="app-add-row">
                <select id="owner-to" class="k-select" prop:value=move || target.get() on:change=move |e| target.set(event_target_value(&e))>
                    <option value="">"Choisis un membre"</option>
                    {move || others.get().into_iter().map(|m| {
                        let label = format!("{} (@{})", m.nick.clone().unwrap_or_else(|| m.user.name().to_string()), m.user.username);
                        view! { <option value=m.user.id.to_string()>{label}</option> }
                    }).collect_view()}
                </select>
                <button type="submit" class="k-btn k-btn--danger" disabled=move || target.get().is_empty()>"Transférer…"</button>
            </div>
        </form>
        {move || confirm.get().map(|what| view! { <ConfirmDialog what=what close=Callback::new(move |()| confirm.set(None))/> })}
    }
}

/// Fiche d'un membre : qui c'est, ses rôles, et de quoi lui en donner ou retirer.
#[component]
pub fn MemberDialog(guild: Id, user: Id, close: Callback<()>) -> impl IntoView {
    let s = store();
    let root = NodeRef::<html::Div>::new();
    modal(root, move || {
        close.try_run(());
    });
    let error = RwSignal::new(None::<String>);
    let member = move || s.guild(guild).and_then(|g| g.members.into_iter().find(|m| m.user.id == user));
    // Le membre est parti ou a été sanctionné pendant qu'on regardait sa fiche.
    Effect::new(move || {
        if member().is_none() {
            close.try_run(());
        }
    });
    let roles = Memo::new(move |_| {
        s.guild(guild)
            .map(|g| {
                let mut r: Vec<Role> = g.roles.into_iter().filter(|r| r.id != guild).collect();
                r.sort_by_key(|r| std::cmp::Reverse(r.position));
                r
            })
            .unwrap_or_default()
    });
    let can_assign = move || s.guild_perms(guild) & perms::MANAGE_ROLES != 0 && s.outranks(guild, user);
    let toggle = move |role: Id, on: bool| {
        error.set(None);
        spawn_local(async move {
            match crate::api::set_member_role(guild, user, role, on).await {
                Ok(m) => s.in_guild(guild, |g| {
                    if let Some(x) = g.members.iter_mut().find(|x| x.user.id == m.user.id) {
                        *x = m;
                    }
                }),
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog app-dialog-wide" role="dialog" aria-modal="true" aria-labelledby="mb-t">
                {move || member().map(|m| {
                    let name = m.nick.clone().unwrap_or_else(|| m.user.name().to_string());
                    let color = s.role_color(Some(guild), user);
                    view! {
                        <header class="app-member-head">
                            <Avatar user=m.user.clone() size="lg"/>
                            <div>
                                <h2 id="mb-t" data-role-color=color.map(|c| c.to_string())>{name}</h2>
                                <small class="app-hint">{format!("@{} · membre depuis {}", m.user.username, when(m.joined_at))}</small>
                            </div>
                        </header>
                    }
                })}
                <div>
                    <h3 class="k-category" id="mb-roles">"Rôles"</h3>
                    <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    <Show when=move || member().is_some_and(|m| m.roles.is_empty()) && !can_assign()>
                        <p class="app-hint">"Aucun rôle."</p>
                    </Show>
                    <ul class="app-role-checks" aria-labelledby="mb-roles">
                        <For each=move || roles.get() key=|r| (r.id, r.name.clone(), r.color, r.position) let:r>
                            {
                                let rid = r.id;
                                let held = move || member().is_some_and(|m| m.roles.contains(&rid));
                                let editable = {
                                    let r = r.clone();
                                    move || can_assign() && s.can_manage_role(guild, &r)
                                };
                                let shown = {
                                    let editable = editable.clone();
                                    move || editable() || held()
                                };
                                let id = format!("mb-r-{rid}");
                                view! {
                                    <Show when=shown.clone()>
                                        <li>
                                            <input id=id.clone() type="checkbox" class="app-pick-box" disabled={let e = editable.clone(); move || !e()}
                                                prop:checked=held on:change=move |e| toggle(rid, event_target_checked(&e))/>
                                            <label for=id.clone()>
                                                <i class="app-role-dot" data-color=r.color.map(|c| c.to_string())></i>{r.name.clone()}
                                            </label>
                                        </li>
                                    </Show>
                                }
                            }
                        </For>
                    </ul>
                    <Show when=move || !can_assign() && s.guild_perms(guild) & perms::MANAGE_ROLES != 0>
                        <p class="app-hint">"Son rang est égal ou supérieur au tien : tu ne peux pas changer ses rôles."</p>
                    </Show>
                </div>
                <footer>
                    <button type="button" class="k-btn" on:click=move |_| { close.try_run(()); } autofocus>"Fermer"</button>
                </footer>
            </div>
        </div>
    }
}
