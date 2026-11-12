//! Profil : la page Réglages › Mon profil (nom, pronoms, bio, photo, bannière, aperçu) et la carte
//! de profil qu'ouvre un clic sur le nom ou l'avatar de quelqu'un (liste des membres, message).

use kraken_proto::{Id, MAX_BIO, MAX_DISPLAY_NAME, MAX_PRONOUNS, RelationshipKind, User};
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};

use crate::icons::{Icon, lu};
use crate::photo::Photo;
use crate::social::{Confirm, ConfirmDialog, open_dm};
use crate::store::{ProfileTarget, Store, store};
use crate::ui::{Avatar, StatusDot, banner_url, modal, tint};

/// Ouvre la carte de profil de cette personne. `guild` : le serveur d'où on la regarde, dont on
/// montre alors les rôles de cette personne.
pub fn open_card(user: User, guild: Option<Id>) {
    store().card.set(Some(ProfileTarget { user, guild }));
}

/// « 11 novembre 2026 », dans la langue de l'interface.
fn long_date(ms: i64) -> String {
    let date = js_sys::Date::new(&(ms as f64).into());
    let options = js_sys::Object::new();
    for (key, value) in [("day", "numeric"), ("month", "long"), ("year", "numeric")] {
        let _ = js_sys::Reflect::set(&options, &JsValue::from_str(key), &JsValue::from_str(value));
    }
    date.to_locale_date_string("fr-FR", &options).into()
}

fn capital(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Nombre d'unités UTF-16 : ce que compte `maxlength` dans un champ.
fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// La fiche la plus à jour de cette personne : la mienne, celle du membre du serveur, celle d'un
/// ami, à défaut celle du clic.
fn live_user(s: Store, t: &ProfileTarget) -> User {
    let id = t.user.id;
    if let Some(me) = s.me.with(|m| m.clone().filter(|m| m.id == id)) {
        return me;
    }
    let member = t.guild.and_then(|g| {
        s.guilds.with(|gs| {
            gs.iter()
                .find(|x| x.guild.id == g)
                .and_then(|x| x.members.iter().find(|m| m.user.id == id))
                .map(|m| m.user.clone())
        })
    });
    member
        .or_else(|| s.friends.with(|f| f.iter().find(|r| r.user.id == id).map(|r| r.user.clone())))
        .unwrap_or_else(|| t.user.clone())
}

/// « Lire l'animation » : en mouvement réduit, les images animées de la carte (photo, bannière) restent
/// fixes ; ce bouton les anime (ou les fige de nouveau). Les feuilles de style ne le montrent qu'en
/// mouvement réduit et quand la carte a une image animée.
#[component]
fn PlayAnimation() -> impl IntoView {
    let playing = RwSignal::new(false);
    let toggle = move |e: leptos::ev::MouseEvent| {
        let on = !playing.get_untracked();
        playing.set(on);
        let card = e
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest(".app-card").ok().flatten());
        if let Some(card) = card {
            let _ = if on { card.set_attribute("data-play", "") } else { card.remove_attribute("data-play") };
        }
    };
    view! {
        <button type="button" class="k-btn k-btn--sm app-play" aria-pressed=move || playing.get().to_string() on:click=toggle>
            <Icon icon=lu::LuPlay small=true/>"Lire l'animation"
        </button>
    }
}

/// Le contenu d'une carte de profil : bannière, avatar, noms, pronoms, bio, dates, rôles du
/// serveur. Sert à la fenêtre qui s'ouvre sur un clic et à l'aperçu des réglages.
#[component]
fn CardBody(user: Signal<User>, guild: Option<Id>, name_id: &'static str, size: &'static str) -> impl IntoView {
    let s = store();
    // L'identifiant se lit à chaque fois : dans l'aperçu des réglages, la fiche est d'abord vide
    // (le READY n'est pas encore arrivé) et ne prend le vrai identifiant, donc le vrai statut, qu'ensuite.
    let id = move || user.with(|u| u.id);
    let dot = Signal::derive(move || s.dot_of(id()));
    // L'avatar ne se redessine pas à chaque lettre tapée dans la bio de l'aperçu.
    let face = Memo::new(move |_| {
        let mut u = user.get();
        (u.bio, u.pronouns, u.banner, u.created_at) = (None, None, None, None);
        u
    });
    let banner = move || {
        let u = user.get();
        let image = u.banner.map(|k| view! { <Photo src=banner_url(&k) cap=640/> });
        view! { <div class="app-card-banner" data-tint=tint(u.id)>{image}</div> }
    };
    // Rôles du serveur (sans @everyone), du plus haut au plus bas, et depuis quand il y est.
    let member = Memo::new(move |_| {
        guild.and_then(|g| {
            s.guilds.with(|gs| {
                let state = gs.iter().find(|x| x.guild.id == g)?;
                let m = state.members.iter().find(|m| m.user.id == id())?;
                let mut roles: Vec<_> =
                    state.roles.iter().filter(|r| r.id != g && m.roles.contains(&r.id)).cloned().collect();
                roles.sort_by_key(|r| std::cmp::Reverse(r.position));
                Some((roles, m.joined_at))
            })
        })
    });
    let name = move || {
        let u = user.get();
        guild.and_then(|g| s.name_of(Some(g), u.id)).unwrap_or_else(|| u.name().to_string())
    };
    view! {
        {banner}
        <div class="app-card-body">
            <div class="app-card-avatar">
                {move || view! { <Avatar user=face.get() size=size status=dot/> }}
            </div>
            <p class="app-card-name" id=name_id>{name}</p>
            <p class="app-card-handle">
                {move || format!("@{}", user.get().username)}
                {move || user.get().pronouns.filter(|p| !p.is_empty()).map(|p| view! { <span class="app-card-pronouns">{p}</span> })}
            </p>
            <p class="app-card-status">
                <span class="app-dot-inline"><StatusDot status=dot/></span>
                {move || capital(dot.get().label())}
            </p>
            <PlayAnimation/>
            {move || user.get().bio.filter(|b| !b.is_empty()).map(|b| view! {
                <section class="app-card-section" aria-label="À propos">
                    <h3 class="app-card-label">"À propos"</h3>
                    <p class="app-card-bio">{b}</p>
                </section>
            })}
            <dl class="app-card-dates">
                {move || user.get().created_at.map(|t| view! {
                    <div><dt>"Membre depuis"</dt><dd>{long_date(t)}</dd></div>
                })}
                {move || member.get().map(|(_, at)| view! {
                    <div><dt>"Sur ce serveur depuis"</dt><dd>{long_date(at)}</dd></div>
                })}
            </dl>
            {move || member.get().filter(|(roles, _)| !roles.is_empty()).map(|(roles, _)| view! {
                <section class="app-card-section" aria-labelledby=format!("{name_id}-roles")>
                    <h3 class="app-card-label" id=format!("{name_id}-roles")>"Rôles"</h3>
                    <ul class="app-chips">
                        {roles.into_iter().map(|r| view! {
                            <li class="app-chip" data-role-color=r.color.map(|c| c.to_string())>
                                <span class="app-role-dot" aria-hidden="true"></span>{r.name}
                            </li>
                        }).collect_view()}
                    </ul>
                </section>
            })}
        </div>
    }
}

/// La fenêtre qui s'ouvre quand `Store::card` est renseigné. Montée une fois, dans la coquille.
#[component]
pub fn ProfileCardHost() -> impl IntoView {
    let s = store();
    let who = Memo::new(move |_| s.card.with(|c| c.as_ref().map(|t| t.user.id)));
    view! {
        {move || who.get().map(|_| view! { <Portal><CardDialog/></Portal> })}
    }
}

#[component]
fn CardDialog() -> impl IntoView {
    let s = store();
    let target = s.card.get_untracked().expect("la carte est ouverte");
    let guild = target.guild;
    let id = target.user.id;
    let close = move || s.card.set(None);
    let root = NodeRef::new();
    modal(root, close);
    let user = Signal::derive({
        let target = target.clone();
        move || live_user(s, &target)
    });
    let label = move || {
        format!(
            "Profil de {}",
            guild.and_then(|g| s.name_of(Some(g), id)).unwrap_or_else(|| user.get().name().to_string())
        )
    };
    let mine = s.my_id() == Some(id);
    let error = RwSignal::new(None::<String>);
    let note = RwSignal::new(None::<&'static str>);
    let busy = RwSignal::new(false);
    let blocking = RwSignal::new(false);
    let rel = Memo::new(move |_| s.rel_kind(id));
    let open = open_dm(id, error);
    let write = move |_: leptos::ev::MouseEvent| {
        open();
        close();
    };
    let ask = move |_| {
        busy.set(true);
        error.set(None);
        note.set(None);
        let username = user.get_untracked().username;
        spawn_local(async move {
            match crate::api::send_friend_request(&username).await {
                Ok(r) => {
                    note.try_set(Some(if r.kind == RelationshipKind::Friend {
                        "Vous êtes maintenant amis."
                    } else {
                        "Demande d'ami envoyée."
                    }));
                    s.upsert_rel(r);
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let accept = move |_| {
        busy.set(true);
        error.set(None);
        note.set(None);
        spawn_local(async move {
            match crate::api::set_relationship(id, RelationshipKind::Friend).await {
                Ok(r) => {
                    s.upsert_rel(r);
                    note.try_set(Some("Vous êtes maintenant amis."));
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let unblock = move |_| {
        busy.set(true);
        error.set(None);
        note.set(None);
        spawn_local(async move {
            match crate::api::remove_relationship(id).await {
                Ok(()) => {
                    s.remove_rel(id);
                    note.try_set(Some("Compte débloqué."));
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let actions = move || {
        if mine {
            return view! {
                <a class="k-btn k-btn--primary" href="/settings/profiles" on:click=move |_| close()>"Modifier mon profil"</a>
            }
            .into_any();
        }
        let write = write.clone();
        let blocked = rel.get() == Some(RelationshipKind::Blocked);
        let relation = match rel.get() {
            None => Some(view! {
                <button type="button" class="k-btn" disabled=move || busy.get() on:click=ask>
                    <Icon icon=lu::LuUserPlus/>"Ajouter en ami"
                </button>
            }.into_any()),
            Some(RelationshipKind::PendingOutgoing) => Some(view! {
                <button type="button" class="k-btn" disabled>"Demande envoyée"</button>
            }.into_any()),
            Some(RelationshipKind::PendingIncoming) => Some(view! {
                <button type="button" class="k-btn" disabled=move || busy.get() on:click=accept>
                    <Icon icon=lu::LuCheck/>"Accepter la demande"
                </button>
            }.into_any()),
            Some(RelationshipKind::Friend) => Some(view! { <span class="k-badge k-badge--neutral">"Ami"</span> }.into_any()),
            Some(RelationshipKind::Blocked) => Some(view! {
                <button type="button" class="k-btn" disabled=move || busy.get() on:click=unblock>"Débloquer"</button>
            }.into_any()),
        };
        view! {
            {(!blocked).then(|| view! {
                <button type="button" class="k-btn k-btn--primary" on:click=write>
                    <Icon icon=lu::LuMessageCircle/>"Envoyer un message"
                </button>
            })}
            {relation}
            {(!blocked).then(|| view! {
                <button type="button" class="k-btn k-btn--ghost app-link-danger" on:click=move |_| blocking.set(true)>
                    <Icon icon=lu::LuBan/>"Bloquer"
                </button>
            })}
        }
        .into_any()
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog app-card" role="dialog" aria-modal="true" aria-label=label>
                <button type="button" class="k-btn k-btn--icon app-card-close" aria-label="Fermer le profil (Échap)" autofocus
                    on:click=move |_| close()>
                    <Icon icon=lu::LuX/>
                </button>
                <CardBody user=user guild=guild name_id="card-name" size="xl"/>
                <div class="app-card-actions">{actions}</div>
                <Show when=move || error.get().is_some()><p class="k-error app-card-note" role="alert">{move || error.get()}</p></Show>
                <Show when=move || note.get().is_some()><p class="app-card-note" role="status">{move || note.get()}</p></Show>
            </div>
        </div>
        {move || blocking.get().then(|| view! {
            <Portal>
                <ConfirmDialog what=Confirm::Block(user.get_untracked()) close=Callback::new(move |()| blocking.set(false))/>
            </Portal>
        })}
    }
}

/// Réglages › Mon profil : ce que les autres voient de moi, et l'aperçu de ma carte.
#[component]
pub fn ProfileSettings() -> impl IntoView {
    let s = store();
    let display = RwSignal::new(String::new());
    let pronouns = RwSignal::new(String::new());
    let bio = RwSignal::new(String::new());
    // Suit le profil enregistré (le READY peut arriver après l'ouverture des réglages) sans
    // écraser la saisie quand autre chose change dans le compte.
    let saved_name = Memo::new(move |_| s.me.with(|m| m.as_ref().and_then(|u| u.display_name.clone())));
    let saved_pronouns = Memo::new(move |_| s.me.with(|m| m.as_ref().and_then(|u| u.pronouns.clone())));
    let saved_bio = Memo::new(move |_| s.me.with(|m| m.as_ref().and_then(|u| u.bio.clone())));
    Effect::new(move || display.set(saved_name.get().unwrap_or_default()));
    Effect::new(move || pronouns.set(saved_pronouns.get().unwrap_or_default()));
    Effect::new(move || bio.set(saved_bio.get().unwrap_or_default()));
    let saved = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let save = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        saved.set(false);
        error.set(None);
        busy.set(true);
        spawn_local(async move {
            let sent = crate::api::update_profile(
                display.get_untracked().trim(),
                bio.get_untracked().trim(),
                pronouns.get_untracked().trim(),
            )
            .await;
            match sent {
                Ok(u) => {
                    display.try_set(u.display_name.clone().unwrap_or_default());
                    pronouns.try_set(u.pronouns.clone().unwrap_or_default());
                    bio.try_set(u.bio.clone().unwrap_or_default());
                    s.update_user(u);
                    saved.try_set(true);
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let username = move || s.me.get().map(|u| u.username).unwrap_or_default();
    // L'aperçu montre ce qui est tapé, pas encore enregistré.
    let preview = Signal::derive(move || {
        let mut u = s.me.get().unwrap_or_else(|| User {
            id: Id::nil(),
            username: String::new(),
            display_name: None,
            avatar: None,
            banner: None,
            bio: None,
            pronouns: None,
            created_at: None,
        });
        let non_empty = |v: String| Some(v.trim().to_string()).filter(|v| !v.is_empty());
        u.display_name = non_empty(display.get());
        u.pronouns = non_empty(pronouns.get());
        u.bio = non_empty(bio.get());
        u
    });
    view! {
        <div class="app-profile">
            <form class="app-profile-form" on:submit=save aria-labelledby="profile-form-t">
                <h3 id="profile-form-t" class="app-profile-title">"Ce que les autres voient"</h3>
                <label class="k-label" for="display-name">"Nom affiché"</label>
                <input id="display-name" class="k-input" maxlength=MAX_DISPLAY_NAME.to_string() placeholder=username bind:value=display
                    aria-describedby="display-name-h" autocomplete="nickname"/>
                <p id="display-name-h" class="k-hint">{move || format!("Ton pseudo unique @{} ne change pas ici : il reste écrit en dessous.", username())}</p>
                <label class="k-label" for="pronouns">"Pronoms"</label>
                <input id="pronouns" class="k-input" maxlength=MAX_PRONOUNS.to_string() placeholder="elle/elles, il/lui, iel/iels…" bind:value=pronouns
                    aria-describedby="pronouns-h"/>
                <p id="pronouns-h" class="k-hint">{move || format!("Facultatif. {} / {MAX_PRONOUNS}", units(&pronouns.get()))}</p>
                <label class="k-label" for="bio">"À propos de toi"</label>
                <textarea id="bio" class="k-input app-bio" rows="4" maxlength=MAX_BIO.to_string() bind:value=bio aria-describedby="bio-h"
                    placeholder="Quelques mots sur toi, tes jeux, ta façon de discuter…"></textarea>
                <p id="bio-h" class="k-hint app-count">{move || format!("{} / {MAX_BIO}", units(&bio.get()))}</p>
                <div class="app-profile-save">
                    <button class="k-btn k-btn--primary" type="submit" disabled=move || busy.get() aria-busy=move || busy.get().to_string()>"Enregistrer"</button>
                    <Show when=move || saved.get()><p class="app-hint" role="status">"Profil enregistré."</p></Show>
                </div>
                <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            </form>
            <section class="app-profile-images" aria-labelledby="profile-images-t">
                <h3 id="profile-images-t" class="app-profile-title">"Photo et bannière"</h3>
                <div class="k-card app-account"><crate::avatar::AvatarBlock/></div>
                <crate::avatar::BannerBlock/>
            </section>
            <section class="app-profile-preview" aria-labelledby="profile-preview-t">
                <h3 id="profile-preview-t" class="app-profile-title">"Aperçu de ta carte"</h3>
                <div class="app-card app-card--preview">
                    <CardBody user=preview guild=None name_id="preview-name" size="lg"/>
                </div>
            </section>
        </div>
    }
}
