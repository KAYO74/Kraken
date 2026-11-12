//! Paramètres d'un serveur (S22) : rôles et leurs droits, bannis, propriété ; et la fiche d'un
//! membre où l'on lui donne ou retire des rôles. Le serveur reste seul juge de la hiérarchie :
//! l'interface désactive ce qui sera refusé, sans plus.

use std::collections::HashMap;

use gloo_timers::future::TimeoutFuture;
use kraken_core::perms;
use kraken_proto::{
    CreateInvite, CreateRole, EditRole, GuildInvite, Id, InviteState, MAX_REASON, ROLE_COLORS, Role, TimeoutMember,
    User,
};
use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use wasm_bindgen::JsCast;

use crate::icons::{Icon, lu};
use crate::social::{Confirm, ConfirmDialog};
use crate::store::{Conn, store};
use crate::ui::{Avatar, MoreMenu, modal, when};

/// Nom de chaque couleur de la palette (numéro − 1) : la teinte vient du thème, `--avatar-N-text`.
pub const COLOR_NAMES: [&str; ROLE_COLORS as usize] =
    ["Rouge", "Or", "Citron vert", "Vert", "Turquoise", "Bleu", "Violet", "Rose"];

pub(crate) type Perm = (u64, &'static str, &'static str);

/// Les droits par catégorie, avec ce que chacun permet.
pub(crate) const CATEGORIES: &[(&str, &[Perm])] = &[
    (
        "Général",
        &[
            (perms::VIEW_CHANNEL, "Voir les salons", "Voir les salons et lire ce qui s'y dit."),
            (perms::MANAGE_CHANNELS, "Gérer les salons", "Créer, renommer, déplacer et supprimer des salons."),
            (perms::MANAGE_GUILD, "Gérer le serveur", "Changer le nom, l'icône et les réglages du serveur."),
            (perms::MANAGE_EMOJIS, "Gérer les émojis", "Ajouter et retirer les émojis et les autocollants du serveur."),
            (perms::MANAGE_EVENTS, "Gérer les événements", "Créer, modifier et annuler les événements."),
            (perms::VIEW_AUDIT_LOG, "Voir le journal d'audit", "Consulter l'historique des actions de modération."),
            (perms::CREATE_INVITE, "Créer des invitations", "Inviter de nouvelles personnes avec un lien."),
        ],
    ),
    (
        "Membres",
        &[
            (perms::CHANGE_NICK, "Changer son surnom", "Choisir son propre surnom sur ce serveur."),
            (perms::MANAGE_NICKS, "Gérer les surnoms", "Changer le surnom des autres membres."),
            (perms::KICK, "Expulser des membres", "Faire sortir un membre, qui peut revenir avec une invitation."),
            (
                perms::BAN,
                "Bannir des membres",
                "Faire sortir un membre pour de bon : il ne peut plus revenir avec une invitation.",
            ),
            (perms::TIMEOUT, "Exclure temporairement", "Empêcher un membre de parler pendant un moment."),
            (
                perms::MANAGE_ROLES,
                "Gérer les rôles",
                "Créer, modifier et donner des rôles plus bas que le sien : de quoi distribuer des droits.",
            ),
        ],
    ),
    (
        "Salons texte",
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
        "Avancé",
        &[(
            perms::ADMINISTRATOR,
            "Administrateur",
            "Tous les droits, sans exception ni limite de rang : à réserver à des personnes de confiance.",
        )],
    ),
];

/// Droits sensibles : une icône d'alerte, en plus de leur phrase.
pub(crate) const SENSITIVE: [u64; 4] = [perms::ADMINISTRATOR, perms::BAN, perms::MANAGE_ROLES, perms::MANAGE_CHANNELS];

const SECTIONS: [(&str, &str); 7] = [
    ("roles", "Rôles"),
    ("channels", "Salons"),
    ("emojis", "Emojis"),
    ("invites", "Invitations"),
    ("bans", "Bannis"),
    ("audit", "Journal d'audit"),
    ("owner", "Propriété"),
];

/// Sections que je peux ouvrir dans ce serveur.
fn allowed(guild: Id) -> Vec<&'static str> {
    let s = store();
    let p = s.guild_perms(guild);
    s.me.track();
    let owner = s.my_id().is_some() && s.guild(guild).is_some_and(|g| Some(g.guild.owner_id) == s.my_id());
    // Qui administre le serveur (rôles, salons, bannis, propriété) voit les emojis, même sans pouvoir les changer.
    let admin = p & (perms::MANAGE_ROLES | perms::MANAGE_CHANNELS | perms::BAN) != 0 || owner;
    SECTIONS
        .iter()
        .map(|(slug, _)| *slug)
        .filter(|slug| match *slug {
            "roles" => p & perms::MANAGE_ROLES != 0,
            "channels" => p & perms::MANAGE_CHANNELS != 0,
            "emojis" => p & perms::MANAGE_EMOJIS != 0 || admin,
            // Qui gère le serveur voit toutes les invitations, qui peut seulement en créer voit les siennes.
            "invites" => p & (perms::MANAGE_GUILD | perms::CREATE_INVITE) != 0,
            "bans" => p & perms::BAN != 0,
            "audit" => p & perms::VIEW_AUDIT_LOG != 0,
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
    // Des mémos, pas des lectures directes du store : un changement sans rapport (un membre qui
    // arrive, un rôle modifié) ne doit pas redessiner la page ni effacer ce qu'on y saisit.
    let open = Memo::new(move |_| gid.get().map(allowed).unwrap_or_default());
    let exists = Memo::new(move |_| gid.get().is_some_and(|g| s.guilds.with(|gs| gs.iter().any(|x| x.guild.id == g))));
    let section = Memo::new(move |_| {
        let Some(_) = gid.get() else { return "" };
        let open = open.get();
        let asked = params.read().get("section").unwrap_or_default();
        open.iter().copied().find(|x| *x == asked).or(open.first().copied()).unwrap_or("")
    });
    let title = Memo::new(move |_| {
        gid.get().and_then(|g| s.guild(g)).map(|g| g.guild.name).unwrap_or_else(|| "Paramètres du serveur".into())
    });
    view! {
        <div class="k-scrim app-settings" node_ref=root>
            <div class="k-settings" role="dialog" aria-modal="true" aria-label="Paramètres du serveur">
                <nav aria-label="Sections des paramètres du serveur">
                    <h3>{move || title.get()}</h3>
                    {move || gid.get().map(|g| {
                        let open = open.get();
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
                        if !exists.get() {
                            return if s.conn.get() == Conn::Ready {
                                empty("Serveur introuvable", "Tu n'en fais plus partie, ou il n'existe pas.").into_any()
                            } else {
                                view! { <div aria-busy="true"><div class="k-skeleton app-skel-block"></div></div> }.into_any()
                            };
                        }
                        match section.get() {
                            "roles" => view! { <Roles guild=g/> }.into_any(),
                            "channels" => view! { <crate::channels::ChannelsAdmin guild=g/> }.into_any(),
                            "emojis" => view! { <crate::emoji_settings::EmojiSettings guild=g/> }.into_any(),
                            "invites" => view! { <Invites guild=g/> }.into_any(),
                            "bans" => view! { <Bans guild=g/> }.into_any(),
                            "audit" => view! { <crate::audit::AuditLog guild=g/> }.into_any(),
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
pub(crate) async fn refocus(prefer: String, fallback: String) {
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

/// Ce qu'on modifie d'un rôle avant d'enregistrer.
#[derive(Clone, PartialEq)]
struct Draft {
    name: String,
    color: Option<u32>,
    hoist: bool,
    permissions: u64,
}

impl Draft {
    fn of(r: &Role) -> Self {
        Self { name: r.name.clone(), color: r.color, hoist: r.hoist, permissions: r.permissions }
    }
}

type Drafts = RwSignal<HashMap<Id, Draft>>;

/// Applique un changement au brouillon du rôle ; il disparaît s'il redevient identique au rôle.
fn edit(drafts: Drafts, guild: Id, role: Id, f: impl FnOnce(&mut Draft)) {
    let Some(cur) = role_of(guild, role) else { return };
    let mut d = drafts.with_untracked(|m| m.get(&role).cloned()).unwrap_or_else(|| Draft::of(&cur));
    f(&mut d);
    drafts.update(|m| {
        if d == Draft::of(&cur) {
            m.remove(&role);
        } else {
            m.insert(role, d);
        }
    });
}

/// Le nom à montrer pour un rôle : `@everyone` s'appelle « @tout le monde » à l'écran.
fn role_label(guild: Id, r: &Role) -> String {
    if r.id == guild { "@tout le monde".into() } else { r.name.clone() }
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
    let drafts: Drafts = RwSignal::new(HashMap::new());
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
    // Combien de membres ont ce rôle (tout le monde pour `@everyone`).
    let count = move |id: Id| {
        s.guilds.with(|gs| {
            gs.iter().find(|g| g.guild.id == guild).map_or(0, |g| {
                if id == guild { g.members.len() } else { g.members.iter().filter(|m| m.roles.contains(&id)).count() }
            })
        })
    };
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
    let unsaved = move |id: Id| drafts.with(|d| d.contains_key(&id));
    view! {
        <div class="app-roles-page">
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
                                let (Some(from), Some(to)) =
                                    (order.iter().position(|x| *x == from_id), order.iter().position(|x| *x == id))
                                else {
                                    return;
                                };
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
                                        <Show when=move || unsaved(id)><span class="k-sr">" (modifications non enregistrées)"</span></Show>
                                        <span class="app-role-count" title="Membres qui ont ce rôle">
                                            <Icon icon=lu::LuUsers small=true/>{move || count(id)}
                                            <span class="k-sr">{move || if count(id) > 1 { " membres" } else { " membre" }}</span>
                                        </span>
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
                            <i class="app-role-dot"></i><span class="app-ellipsis">"@tout le monde"</span>
                            <Show when=move || unsaved(guild)><span class="k-sr">" (modifications non enregistrées)"</span></Show>
                            <span class="app-role-count" title="Membres qui ont ce rôle">
                                <Icon icon=lu::LuUsers small=true/>{move || count(guild)}
                                <span class="k-sr">{move || if count(guild) > 1 { " membres" } else { " membre" }}</span>
                            </span>
                        </button>
                    </li>
                </ul>
            </div>
            <div class="app-roles-edit">
                {move || selected.get().map(|id| view! { <RoleEditor guild=guild role=id selected=selected drafts=drafts/> })}
            </div>
        </div>
        </div>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RoleTab {
    Display,
    Permissions,
    Members,
}

const ROLE_TABS: [(RoleTab, &str, &str); 3] = [
    (RoleTab::Display, "display", "Affichage"),
    (RoleTab::Permissions, "perms", "Permissions"),
    (RoleTab::Members, "members", "Membres"),
];

#[component]
fn RoleEditor(guild: Id, role: Id, selected: RwSignal<Option<Id>>, drafts: Drafts) -> impl IntoView {
    let s = store();
    let everyone = role == guild;
    let current = move || role_of(guild, role);
    let editable = move || {
        let p = s.guild_perms(guild);
        p & perms::MANAGE_ROLES != 0 && current().is_some_and(|r| everyone || s.can_manage_role(guild, &r))
    };
    let tab = RwSignal::new(RoleTab::Display);
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let deleting = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    // Le brouillon s'il y en a un, sinon le rôle tel qu'il est enregistré.
    let draft = Memo::new(move |_| drafts.with(|m| m.get(&role).cloned()).or_else(|| current().map(|r| Draft::of(&r))));
    let dirty = move || drafts.with(|m| m.contains_key(&role));
    let tablist = NodeRef::<html::Div>::new();
    let keys = move |e: leptos::ev::KeyboardEvent| {
        let i = ROLE_TABS.iter().position(|x| x.0 == tab.get_untracked()).unwrap_or(0);
        let n = ROLE_TABS.len();
        let next = match e.key().as_str() {
            "ArrowRight" => (i + 1) % n,
            "ArrowLeft" => (i + n - 1) % n,
            "Home" => 0,
            "End" => n - 1,
            _ => return,
        };
        e.prevent_default();
        tab.set(ROLE_TABS[next].0);
        let btn = tablist
            .get_untracked()
            .and_then(|l| l.query_selector(&format!("#role-tab-{}", ROLE_TABS[next].1)).ok().flatten())
            .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok());
        if let Some(b) = btn {
            let _ = b.focus();
        }
    };
    let save = move |_| {
        let (Some(d), Some(cur)) = (draft.get_untracked(), current()) else { return };
        let req = EditRole {
            name: (d.name != cur.name).then(|| d.name.clone()),
            color: (d.color != cur.color).then(|| d.color.unwrap_or(0)),
            hoist: (d.hoist != cur.hoist).then_some(d.hoist),
            permissions: (d.permissions != cur.permissions).then_some(d.permissions),
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match crate::api::edit_role(guild, role, &req).await {
                Ok(r) => {
                    s.in_guild(guild, |g| {
                        if let Some(x) = g.roles.iter_mut().find(|x| x.id == r.id) {
                            *x = r;
                        }
                    });
                    drafts.update(|m| {
                        m.remove(&role);
                    });
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
        drafts.update(|m| {
            m.remove(&role);
        });
    };
    let my_base = move || s.guild_perms(guild);
    let tab_buttons = move || {
        ROLE_TABS
            .iter()
            .map(|&(t, id, label)| {
                let on = move || tab.get() == t;
                view! {
                    <button class="k-tab" role="tab" id=format!("role-tab-{id}") aria-controls="role-panel"
                        aria-selected=move || on().to_string() tabindex=move || if on() { "0" } else { "-1" }
                        on:click=move |_| tab.set(t)>{label}</button>
                }
            })
            .collect_view()
    };
    let display = move || {
        if everyone {
            return view! { <p class="app-hint">"Tout le monde a ce rôle, automatiquement : il n'a ni nom à changer, ni couleur. Ses droits (onglet Permissions) s'ajoutent à ceux de tous les autres rôles."</p> }.into_any();
        }
        view! {
            <div class="k-field">
                <label class="k-label" for="role-name">"Nom du rôle"</label>
                <input id="role-name" class="k-input" maxlength="100" autocomplete="off" disabled=move || !editable()
                    prop:value=move || draft.get().map(|d| d.name).unwrap_or_default()
                    on:input=move |e| { let v = event_target_value(&e); edit(drafts, guild, role, |d| d.name = v) }/>
            </div>
            <div class="k-field">
                <span class="k-label" id="role-color-l">"Couleur du nom"</span>
                <div class="app-swatches" role="radiogroup" aria-labelledby="role-color-l">
                    <button type="button" role="radio" class="k-swatch app-swatch" data-color="0" aria-label="Par défaut" title="Par défaut"
                        aria-checked=move || draft.get().is_some_and(|d| d.color.is_none()).to_string() disabled=move || !editable()
                        on:click=move |_| edit(drafts, guild, role, |d| d.color = None)>
                        <Icon icon=lu::LuBan small=true/>
                    </button>
                    {(1..=ROLE_COLORS).map(|c| {
                        let label = COLOR_NAMES[c as usize - 1];
                        let on = move || draft.get().is_some_and(|d| d.color == Some(c));
                        view! {
                            <button type="button" role="radio" class="k-swatch app-swatch" data-color=c.to_string()
                                aria-label=label title=label aria-checked=move || on().to_string() disabled=move || !editable()
                                on:click=move |_| edit(drafts, guild, role, |d| d.color = Some(c))>
                                <Show when=on><Icon icon=lu::LuCheck small=true/></Show>
                            </button>
                        }
                    }).collect_view()}
                </div>
                <p class="k-hint">"Huit teintes qui restent lisibles sur tous les thèmes. Le nom des membres prend la couleur de leur rôle coloré le plus haut."</p>
            </div>
            <div class="k-setting">
                <label for="role-hoist">"Afficher à part dans la liste des membres"</label>
                <input id="role-hoist" type="checkbox" role="switch" class="k-switch" aria-describedby="role-hoist-h"
                    disabled=move || !editable() prop:checked=move || draft.get().is_some_and(|d| d.hoist)
                    on:change=move |e| { let v = event_target_checked(&e); edit(drafts, guild, role, |d| d.hoist = v) }/>
                <p id="role-hoist-h">"Les membres qui ont ce rôle forment leur propre rubrique, au-dessus des autres."</p>
            </div>
            <div class="app-roles-danger">
                <button class="k-btn k-btn--danger" disabled=move || !editable() on:click=move |_| deleting.set(true)>
                    <Icon icon=lu::LuTrash2 small=true/>"Supprimer ce rôle"
                </button>
            </div>
        }
        .into_any()
    };
    let permissions = move || {
        let q = query.get().trim().to_lowercase();
        let shown: Vec<_> = CATEGORIES
            .iter()
            .map(|(title, list)| {
                let rows: Vec<&Perm> = list
                    .iter()
                    .filter(|(_, label, help)| {
                        q.is_empty() || label.to_lowercase().contains(&q) || help.to_lowercase().contains(&q)
                    })
                    .collect();
                (*title, rows)
            })
            .filter(|(_, rows)| !rows.is_empty())
            .collect();
        if shown.is_empty() {
            return view! { <p class="app-hint" role="status">"Aucun droit ne correspond à cette recherche."</p> }
                .into_any();
        }
        shown
            .into_iter()
            .map(|(title, rows)| {
                view! {
                    <fieldset class="app-perms">
                        <legend class="k-category">{title}</legend>
                        {rows.into_iter().map(|(bit, label, help)| {
                            let (bit, id) = (*bit, format!("perm-{bit}"));
                            let sensitive = SENSITIVE.contains(&bit);
                            // On n'accorde que ce que l'on a soi-même.
                            let allowed = move || editable() && (my_base() & bit != 0 || current().is_some_and(|r| r.permissions & bit != 0));
                            view! {
                                <div class="k-setting">
                                    <label for=id.clone()>
                                        {sensitive.then(|| view! {
                                            <span class="app-sensitive" title="Droit sensible"><Icon icon=lu::LuShieldAlert/><span class="k-sr">"Droit sensible : "</span></span>
                                        })}
                                        {*label}
                                    </label>
                                    <input id=id.clone() type="checkbox" role="switch" class="k-switch" aria-describedby=format!("{id}-h")
                                        disabled=move || !allowed() prop:checked=move || draft.get().is_some_and(|d| d.permissions & bit != 0)
                                        on:change=move |_| edit(drafts, guild, role, |d| d.permissions ^= bit)/>
                                    <p id=format!("{id}-h")>{*help}</p>
                                </div>
                            }
                        }).collect_view()}
                    </fieldset>
                }
            })
            .collect_view()
            .into_any()
    };
    view! {
        <Show when=move || current().is_some()>
            <h3 class="app-roles-title">{move || current().map(|r| role_label(guild, &r)).unwrap_or_default()}</h3>
            <div class="k-tabs" role="tablist" aria-label="Réglages du rôle" node_ref=tablist on:keydown=keys>{tab_buttons()}</div>
            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            <div id="role-panel" role="tabpanel" class="app-roles-panel" tabindex="-1" aria-labelledby=move || {
                let t = tab.get();
                format!("role-tab-{}", ROLE_TABS.iter().find(|x| x.0 == t).map_or("display", |x| x.1))
            }>
                {move || match tab.get() {
                    RoleTab::Display => display(),
                    RoleTab::Permissions => view! {
                        <div class="k-search app-search">
                            <Icon icon=lu::LuSearch/>
                            <input class="k-input" type="search" placeholder="Chercher un droit" aria-label="Chercher un droit"
                                prop:value=move || query.get() on:input=move |e| query.set(event_target_value(&e))/>
                        </div>
                        {permissions()}
                    }.into_any(),
                    RoleTab::Members => view! { <RoleMembers guild=guild role=role editable=Signal::derive(editable)/> }.into_any(),
                }}
            </div>
            <Show when=dirty>
                <div class="app-savebar" role="region" aria-label="Modifications du rôle">
                    <span role="status"><Icon icon=lu::LuCircleAlert small=true/><span class="app-bar-long">"Modifications non enregistrées"</span><span class="app-bar-short">"Non enregistré"</span></span>
                    <button type="button" class="k-btn k-btn--ghost" on:click=cancel>"Annuler"</button>
                    <button type="button" class="k-btn k-btn--primary" on:click=save aria-busy=move || busy.get().to_string()>"Enregistrer"</button>
                </div>
            </Show>
            {move || deleting.get().then(|| current().map(|r| view! {
                <ConfirmDialog what=Confirm::DeleteRole(guild, r) close=Callback::new(move |()| {
                    deleting.set(false);
                    if role_of(guild, role).is_none() { selected.set(None); }
                })/>
            }))}
        </Show>
    }
}

/// Onglet « Membres » d'un rôle : qui l'a, et de quoi le donner ou le retirer.
#[component]
fn RoleMembers(guild: Id, role: Id, editable: Signal<bool>) -> impl IntoView {
    let s = store();
    let error = RwSignal::new(None::<String>);
    let pick = RwSignal::new(String::new());
    let everyone = role == guild;
    let members = move || s.guild(guild).map(|g| g.members).unwrap_or_default();
    let holders = Memo::new(move |_| members().into_iter().filter(|m| m.roles.contains(&role)).collect::<Vec<_>>());
    let candidates = Memo::new(move |_| {
        members().into_iter().filter(|m| !m.roles.contains(&role) && s.outranks(guild, m.user.id)).collect::<Vec<_>>()
    });
    let set = move |uid: Id, on: bool| {
        error.set(None);
        spawn_local(async move {
            match crate::api::set_member_role(guild, uid, role, on).await {
                Ok(m) => {
                    s.in_guild(guild, |g| {
                        if let Some(x) = g.members.iter_mut().find(|x| x.user.id == m.user.id) {
                            *x = m;
                        }
                    });
                    pick.try_set(String::new());
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    if everyone {
        return view! { <p class="app-hint">"Tout le monde est dans ce rôle, automatiquement."</p> }.into_any();
    }
    let add = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if let Ok(uid) = pick.get_untracked().parse::<Id>() {
            set(uid, true);
        }
    };
    view! {
        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
        <form class="k-field" on:submit=add>
            <label class="k-label" for="role-add">"Ajouter un membre à ce rôle"</label>
            <div class="app-add-row">
                <select id="role-add" class="k-select" disabled=move || !editable.get()
                    prop:value=move || pick.get() on:change=move |e| pick.set(event_target_value(&e))>
                    <option value="">"Choisis un membre"</option>
                    {move || candidates.get().into_iter().map(|m| {
                        let label = format!("{} (@{})", m.nick.clone().unwrap_or_else(|| m.user.name().to_string()), m.user.username);
                        view! { <option value=m.user.id.to_string()>{label}</option> }
                    }).collect_view()}
                </select>
                <button type="submit" class="k-btn" disabled=move || !editable.get() || pick.get().is_empty()>"Ajouter"</button>
            </div>
        </form>
        <Show when=move || holders.with(Vec::is_empty)><p class="app-hint">"Personne n'a ce rôle pour l'instant."</p></Show>
        <ul class="app-members" aria-label="Membres qui ont ce rôle">
            <For each=move || holders.get() key=|m| (m.user.id, m.user.display_name.clone(), m.user.avatar.clone(), m.nick.clone()) let:m>
                {
                    let uid = m.user.id;
                    let name = m.nick.clone().unwrap_or_else(|| m.user.name().to_string());
                    let can = move || editable.get() && s.outranks(guild, uid);
                    view! {
                        <li class="k-dm">
                            <Avatar user=m.user.clone() size="md"/>
                            <div class="app-ellipsis">{name.clone()}<small>{format!("@{}", m.user.username)}</small></div>
                            <button class="k-btn k-btn--sm" aria-label=format!("Retirer {name} de ce rôle") disabled=move || !can()
                                on:click=move |_| set(uid, false)>"Retirer"</button>
                        </li>
                    }
                }
            </For>
        </ul>
    }
    .into_any()
}

/// Durées proposées pour une nouvelle invitation (secondes ; `never` : elle n'expire pas).
const INVITE_AGES: [(&str, &str); 7] = [
    ("1800", "30 minutes"),
    ("3600", "1 heure"),
    ("21600", "6 heures"),
    ("43200", "12 heures"),
    ("86400", "1 jour"),
    ("604800", "7 jours"),
    ("never", "Jamais"),
];

/// Nombres d'utilisations proposés (0 : sans limite).
const INVITE_USES: [(&str, &str); 7] = [
    ("0", "Illimité"),
    ("1", "1 utilisation"),
    ("5", "5 utilisations"),
    ("10", "10 utilisations"),
    ("25", "25 utilisations"),
    ("50", "50 utilisations"),
    ("100", "100 utilisations"),
];

/// Invitations du serveur : les vivantes d'abord, puis celles qui ne servent plus depuis peu.
/// Qui gère le serveur voit toutes les invitations, les autres seulement les leurs.
#[component]
fn Invites(guild: Id) -> impl IntoView {
    let s = store();
    let rows = RwSignal::new(None::<Vec<GuildInvite>>);
    let error = RwSignal::new(None::<String>);
    let said = RwSignal::new(String::new());
    // Le code copié à l'instant, et si la copie a réussi.
    let copied = RwSignal::new(None::<(String, bool)>);
    let age = RwSignal::new("604800".to_string());
    let uses = RwSignal::new("0".to_string());
    let busy = RwSignal::new(false);
    let list = NodeRef::<html::Ul>::new();
    let load = move || {
        spawn_local(async move {
            match crate::api::guild_invites(guild).await {
                Ok(l) => {
                    rows.try_set(Some(l));
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    load();
    let manage = Memo::new(move |_| s.guild_perms(guild) & perms::MANAGE_GUILD != 0);
    let can_create = Memo::new(move |_| s.guild_perms(guild) & perms::CREATE_INVITE != 0);
    let create = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let Some(channel) = s.guild(guild).as_ref().and_then(crate::shell::first_text) else {
            error.set(Some("Aucun salon où inviter.".into()));
            return;
        };
        let max_age = age.get_untracked().parse::<u32>().ok();
        let max_uses = uses.get_untracked().parse::<u32>().ok().filter(|n| *n > 0);
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match crate::api::create_invite_with(channel, &CreateInvite { max_age, max_uses, reuse: false }).await {
                Ok(i) => {
                    said.try_set(format!("Invitation {} créée.", i.code));
                    load();
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let revoke = move |code: String| {
        error.set(None);
        spawn_local(async move {
            match crate::api::revoke_invite(&code).await {
                Ok(()) => {
                    rows.try_update(|r| {
                        if let Some(r) = r {
                            r.retain(|i| i.code != code);
                        }
                    });
                    said.try_set(format!("Invitation {code} révoquée."));
                    // La ligne et son menu ont disparu : le focus revient à la liste.
                    if let Some(l) = list.get_untracked() {
                        let _ = l.focus();
                    }
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    let copy = move |code: String| {
        spawn_local(async move {
            let link = format!("{}/invite/{code}", crate::api::public_url().await);
            let done = crate::ui::copy_text(&link).await;
            copied.try_set(Some((code.clone(), done)));
            TimeoutFuture::new(2500).await;
            copied.try_update(|c| {
                if c.as_ref().is_some_and(|(x, _)| *x == code) {
                    *c = None;
                }
            });
        });
    };
    view! {
        <p class="app-hint">
            {move || if manage.get() {
                "Les invitations expirées ou épuisées restent listées une semaine. Une invitation révoquée cesse de marcher aussitôt."
            } else {
                "Tu ne vois que les invitations que tu as créées. Une invitation révoquée cesse de marcher aussitôt."
            }}
        </p>
        <Show when=move || can_create.get()>
            <form class="app-invite-form" on:submit=create aria-label="Nouvelle invitation">
                <div class="k-field">
                    <label class="k-label" for="inv-age">"Expire après"</label>
                    <select id="inv-age" class="k-select" on:change=move |e| age.set(event_target_value(&e))>
                        {INVITE_AGES.iter().map(|(v, label)| view! {
                            <option value=*v prop:selected=move || age.get() == *v>{*label}</option>
                        }).collect_view()}
                    </select>
                </div>
                <div class="k-field">
                    <label class="k-label" for="inv-uses">"Nombre d'utilisations"</label>
                    <select id="inv-uses" class="k-select" on:change=move |e| uses.set(event_target_value(&e))>
                        {INVITE_USES.iter().map(|(v, label)| view! {
                            <option value=*v prop:selected=move || uses.get() == *v>{*label}</option>
                        }).collect_view()}
                    </select>
                </div>
                <button type="submit" class="k-btn k-btn--primary" disabled=move || busy.get() aria-busy=move || busy.get().to_string()>"Créer une invitation"</button>
            </form>
        </Show>
        <p class="app-add-ok" role="status">{move || said.get()}</p>
        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
        <Show when=move || rows.with(|r| r.is_none()) && error.with(Option::is_none)>
            <div aria-busy="true"><div class="k-skeleton app-skel-row"></div></div>
        </Show>
        <Show when=move || rows.with(|r| r.as_ref().is_some_and(Vec::is_empty))>
            <div class="k-empty"><b>"Aucune invitation"</b><span>"Celles que tu crées apparaissent ici."</span></div>
        </Show>
        <ul class="app-list app-list--col" aria-label="Invitations" node_ref=list tabindex="-1">
            <For each=move || rows.get().unwrap_or_default() key=|i| (i.code.clone(), i.uses, i.state as u8) let:i>
                {
                    let now = crate::ui::now_ms();
                    let dead = i.state != InviteState::Active;
                    let code = i.code.clone();
                    let (c_copy, c_status, c_revoke, c_menu) = (code.clone(), code.clone(), code.clone(), code.clone());
                    let badge = match i.state {
                        InviteState::Active => None,
                        InviteState::Expired => Some("Expirée"),
                        InviteState::Exhausted => Some("Épuisée"),
                    };
                    let by = i.inviter.as_ref().map_or("un compte supprimé".to_string(), |u| u.name().to_string());
                    let channel = i.channel_name.clone().map(|n| format!(" · #{n}")).unwrap_or_default();
                    let used = if i.max_uses == 0 { format!("{} / ∞", i.uses) } else { format!("{} / {}", i.uses, i.max_uses) };
                    let used_text = match (i.uses, i.max_uses) {
                        (n, 0) => format!("{n} utilisation{}, sans limite", if n > 1 { "s" } else { "" }),
                        (n, m) => format!("{n} utilisation{} sur {m}", if n > 1 { "s" } else { "" }),
                    };
                    let expiry = match (i.state, i.expires_at) {
                        (InviteState::Expired, Some(t)) => format!("a expiré ({})", when(t)),
                        (InviteState::Expired, None) => "a expiré".to_string(),
                        (InviteState::Exhausted, _) => "plus aucune utilisation".to_string(),
                        (InviteState::Active, Some(t)) => format!("expire dans {}", crate::ui::left(t, now)),
                        (InviteState::Active, None) => "n'expire jamais".to_string(),
                    };
                    view! {
                        <li class="app-row app-invite" data-dead=dead.then_some("")>
                            <div>
                                <b><code class="app-code">{code}</code>
                                    {badge.map(|b| view! { " " <span class="k-badge k-badge--neutral">{b}</span> })}</b>
                                <small>{format!("Créée par {by}{channel}")}</small>
                                <small>
                                    <span aria-hidden="true">{used}</span><span class="k-sr">{used_text}</span>
                                    " · "{expiry}
                                </small>
                            </div>
                            <div class="app-row-actions">
                                <span class="app-copied" role="status">
                                    {move || copied.with(|c| c.as_ref().filter(|(x, _)| *x == c_status).map(|(_, ok)| if *ok { "Copié" } else { "Copie impossible" }))}
                                </span>
                                {(!dead).then(|| view! {
                                    <button type="button" class="k-btn" aria-label=format!("Copier le lien de l'invitation {c_copy}")
                                        on:click={ let c = c_copy.clone(); move |_| copy(c.clone()) }>
                                        <Icon icon=lu::LuCopy small=true/>"Copier"
                                    </button>
                                })}
                                <MoreMenu label=format!("Plus d'actions pour l'invitation {c_menu}")>
                                    <button class="k-menuitem is-danger" on:click=move |_| revoke(c_revoke.clone())>
                                        <Icon icon=lu::LuTrash2/>"Révoquer"
                                    </button>
                                </MoreMenu>
                            </div>
                        </li>
                    }
                }
            </For>
        </ul>
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
        <p class="app-hint">"Une personne bannie ne peut plus rejoindre le serveur avec une invitation. Lever le bannissement lui rend cette possibilité."</p>
        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
        <Show when=move || loaded.get() && list.with(Vec::is_empty)>
            <div class="k-empty"><b>"Personne n'est banni"</b><span>"Les bannissements apparaîtront ici."</span></div>
        </Show>
        <ul class="app-list app-list--col" aria-label="Personnes bannies">
            <For each=move || list.get() key=|b| (b.user.id, b.created_at) let:b>
                {
                    let uid = b.user.id;
                    let name = b.user.name().to_string();
                    view! {
                        <li class="app-row">
                            <Avatar user=b.user.clone() size="md"/>
                            <div>
                                <b>{name.clone()}</b>
                                <small>{format!("@{} · {}", b.user.username, when(b.created_at))}</small>
                                <small class="app-ban-reason">{b.reason.clone().map(|r| format!("Raison : {r}")).unwrap_or_else(|| "Aucune raison donnée".into())}</small>
                            </div>
                            <button class="k-btn" aria-label=format!("Lever le bannissement de {name}") on:click=move |_| unban(uid)>"Lever le bannissement"</button>
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
    let deleting = RwSignal::new(false);
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
        <section class="app-danger" aria-labelledby="danger-t">
            <h3 id="danger-t">"Zone dangereuse"</h3>
            <p class="app-hint">"Supprimer le serveur efface ses salons, ses messages, ses rôles et ses invitations, pour tout le monde. Il n'y a aucun moyen de revenir en arrière."</p>
            <button type="button" class="k-btn k-btn--danger" on:click=move |_| deleting.set(true)>"Supprimer le serveur…"</button>
        </section>
        // Montée hors de la fenêtre des réglages : à 390 px, rien ne la rogne ni ne la masque.
        {move || deleting.get().then(|| view! {
            <Portal>
                <DeleteGuildDialog guild=guild close=Callback::new(move |()| deleting.set(false))/>
            </Portal>
        })}
    }
}

/// Supprimer le serveur : dit ce qui disparaît, et n'agit qu'une fois son nom retapé.
#[component]
fn DeleteGuildDialog(guild: Id, close: Callback<()>) -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let root = NodeRef::<html::Div>::new();
    modal(root, move || {
        close.try_run(());
    });
    let name = s.guild(guild).map(|g| g.guild.name).unwrap_or_default();
    let typed = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let matches = Memo::new({
        let expected = name.clone();
        move |_| typed.with(|t| !t.trim().is_empty() && t.trim() == expected)
    });
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if !matches.get_untracked() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        spawn_local(async move {
            match crate::api::delete_guild(guild, typed.get_untracked().trim()).await {
                Ok(()) => {
                    // Fermer d'abord : la suite retire l'écran qui porte ce dialogue.
                    close.try_run(());
                    navigate("/channels/@me", Default::default());
                    s.remove_guild(guild);
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
            <div class="k-dialog app-dialog-tall" role="alertdialog" aria-modal="true" aria-labelledby="dg-t" aria-describedby="dg-d">
                <header><h2 id="dg-t">{format!("Supprimer « {name} » ?")}</h2></header>
                <form on:submit=submit>
                    <div>
                        <div id="dg-d">
                            <p>"Tout ça disparaît pour tout le monde, tout de suite :"</p>
                            <ul class="app-gone">
                                <li>"les salons, leurs messages et leurs fichiers,"</li>
                                <li>"les rôles et les droits,"</li>
                                <li>"les invitations."</li>
                            </ul>
                            <p><b>"C'est irréversible."</b>" Les personnes en vocal sont déconnectées."</p>
                        </div>
                        <div class="k-field">
                            <label class="k-label" for="dg-name">"Tape le nom du serveur"</label>
                            <input id="dg-name" class="k-input" autocomplete="off" spellcheck="false" aria-describedby="dg-hint"
                                prop:value=move || typed.get() on:input=move |e| typed.set(event_target_value(&e))/>
                            <small id="dg-hint" class="app-hint app-wrap">{format!("Pour confirmer, retape : {name}")}</small>
                        </div>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); } autofocus>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--danger" disabled=move || !matches.get() || busy.get() aria-busy=move || busy.get().to_string()>"Supprimer le serveur"</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}

/// Durées d'une exclusion temporaire : secondes et libellé (de 60 s à une semaine ; le serveur va jusqu'à 28 jours).
const TIMEOUT_STEPS: [(u32, &str); 6] = [
    (60, "60 secondes"),
    (300, "5 minutes"),
    (600, "10 minutes"),
    (3600, "1 heure"),
    (86_400, "1 jour"),
    (604_800, "1 semaine"),
];

/// Exclure temporairement un membre : une durée, une raison facultative. La personne garde la
/// lecture, mais ne peut plus écrire, réagir, joindre de fichier ni parler en vocal.
#[component]
pub fn TimeoutDialog(guild: Id, user: User, close: Callback<()>) -> impl IntoView {
    let s = store();
    let root = NodeRef::<html::Div>::new();
    modal(root, move || {
        close.try_run(());
    });
    let uid = user.id;
    let name = s.name_of(Some(guild), uid).unwrap_or_else(|| user.name().to_string());
    let seconds = RwSignal::new(TIMEOUT_STEPS[0].0);
    let reason = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        error.set(None);
        let reason = Some(reason.get_untracked().trim().to_string()).filter(|r| !r.is_empty());
        spawn_local(async move {
            match crate::api::timeout_member(guild, uid, &TimeoutMember { seconds: seconds.get_untracked(), reason })
                .await
            {
                Ok(m) => {
                    s.in_guild(guild, |g| {
                        if let Some(x) = g.members.iter_mut().find(|x| x.user.id == m.user.id) {
                            *x = m;
                        }
                    });
                    close.try_run(());
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
            <div class="k-dialog app-dialog-tall" role="dialog" aria-modal="true" aria-labelledby="to-t" aria-describedby="to-d">
                <header><h2 id="to-t">{format!("Exclure {name} temporairement")}</h2></header>
                <form on:submit=submit>
                    <div>
                        <p id="to-d">"Pendant ce temps, cette personne peut lire le serveur, mais ni écrire, ni réagir, ni envoyer de fichier, ni parler en vocal. L'exclusion se termine toute seule, et tu peux la lever avant."</p>
                        <fieldset class="app-radios" role="radiogroup" aria-labelledby="to-len">
                            <legend id="to-len" class="k-label">"Durée"</legend>
                            <div class="app-radio-grid">
                                {TIMEOUT_STEPS.iter().map(|(n, label)| {
                                    let id = format!("to-{n}");
                                    view! {
                                        <label class="app-radio" for=id.clone()>
                                            <input id=id.clone() type="radio" name="to-len" prop:checked=move || seconds.get() == *n
                                                on:change=move |_| seconds.set(*n)/>
                                            <span>{*label}</span>
                                        </label>
                                    }
                                }).collect_view()}
                            </div>
                        </fieldset>
                        <div class="k-field">
                            <label class="k-label" for="to-reason">"Raison (facultatif)"</label>
                            <input id="to-reason" class="k-input" maxlength=MAX_REASON autocomplete="off"
                                prop:value=move || reason.get() on:input=move |e| reason.set(event_target_value(&e))/>
                        </div>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { close.try_run(()); } autofocus>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--primary" aria-busy=move || busy.get().to_string() disabled=move || busy.get()>"Exclure"</button>
                    </footer>
                </form>
            </div>
        </div>
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
