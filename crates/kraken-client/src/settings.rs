//! Réglages utilisateur (S06 à S15) dans la modale `k-settings`. Mon compte (pseudo, e-mail, mot
//! de passe), Mon profil, Apparence, la liste des comptes bloqués (S08) et les notifications
//! (S09) sont branchés ; les autres sections ont leur route et leur place dans la navigation.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::icons::{Icon, lu};
use crate::store::store;
use crate::ui::{Avatar, modal};

const SECTIONS: &[(&str, &str, &str, &str)] = &[
    ("Toi", "account", "Mon compte", "S06"),
    ("Toi", "profiles", "Mon profil", "S20"),
    ("Toi", "privacy", "Tes données", "S07"),
    ("Toi", "messaging", "Qui peut t'écrire", "S08"),
    ("Toi", "apps", "Applications liées", "S15"),
    ("Préférences", "notifications", "Notifications", "S09"),
    ("Préférences", "voice", "Micro, son et caméra", "S10"),
    ("Préférences", "appearance", "Apparence", "S11"),
    ("Préférences", "accessibility", "Accessibilité", "S12"),
    ("Préférences", "keybinds", "Raccourcis clavier", "S13"),
    ("Préférences", "language", "Langue et heure", "S14"),
];

const THEME_KEY: &str = "kraken.theme";
const THEMES: &[(&str, &str, &str)] = &[
    ("dark", "Sombre", "Le thème par défaut, doux pour les yeux le soir."),
    ("light", "Clair", "Fond blanc, pour les pièces lumineuses."),
    ("black", "Noir", "Noir profond, idéal sur les écrans OLED."),
    ("auto", "Automatique", "Suit le réglage clair ou sombre de ton système."),
];

/// Le champ d'un formulaire du compte qu'une réponse d'erreur met en cause : la nouvelle valeur
/// (pseudo, adresse, nouveau mot de passe) ou le mot de passe actuel. Une erreur sans champ
/// (réseau, serveur) n'en marque aucun : l'alerte seule la dit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Culprit {
    Value,
    Password,
}

fn culprit_of(code: &str) -> Option<Culprit> {
    match code {
        "bad_password" => Some(Culprit::Password),
        "invalid" | "unchanged" | "conflict" | "username_cooldown" | "same_password" => Some(Culprit::Value),
        _ => None,
    }
}

fn set_theme(t: &str) {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = el.set_attribute("data-theme", t);
    }
}

fn saved_theme() -> String {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(THEME_KEY).ok().flatten())
        .unwrap_or_else(|| "dark".into())
}

pub fn apply_saved_theme() {
    set_theme(&saved_theme());
}

#[component]
pub fn Settings() -> impl IntoView {
    let params = use_params_map();
    let section = move || params.read().get("section").unwrap_or_default();
    let navigate = use_navigate();
    let close = move || navigate("/channels/@me", Default::default());
    let close2 = close.clone();
    let root = NodeRef::new();
    modal(root, close.clone());
    let mut group = "";
    let nav = SECTIONS.iter().map(|(g, slug, label, _)| {
        let head = (*g != group).then(|| view! { <h3>{*g}</h3> });
        group = g;
        view! {
            {head}
            <a class="k-navitem" href=format!("/settings/{slug}") aria-current=move || (section() == *slug).then_some("page")>{*label}</a>
        }
    }).collect_view();
    view! {
        <div class="k-scrim app-settings" node_ref=root>
            <div class="k-settings" role="dialog" aria-modal="true" aria-label="Réglages">
                <nav aria-label="Sections des réglages">{nav}</nav>
                <main>
                    <div class="app-settings-top">
                        <h2>{move || SECTIONS.iter().find(|s| s.1 == section()).map(|s| s.2).unwrap_or("Réglages")}</h2>
                        <button class="k-btn k-btn--icon" aria-label="Fermer les réglages (Échap)" on:click=move |_| close2() autofocus>
                            <Icon icon=lu::LuX/>
                        </button>
                    </div>
                    {move || match section().as_str() {
                        "account" => view! { <Account/> }.into_any(),
                        "profiles" => view! { <crate::profile::ProfileSettings/> }.into_any(),
                        "appearance" => view! { <Appearance/> }.into_any(),
                        "messaging" => view! { <Messaging/> }.into_any(),
                        "notifications" => view! { <Notifications/> }.into_any(),
                        other => {
                            let id = SECTIONS.iter().find(|s| s.1 == other).map(|s| s.3).unwrap_or("—");
                            view! { <div class="k-empty"><b>"Bientôt ici"</b>{format!("Section {id} prévue, pas encore construite.")}</div> }.into_any()
                        }
                    }}
                </main>
            </div>
        </div>
    }
}

#[component]
fn Account() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let deleting = RwSignal::new(false);
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let leave = move |all: bool| {
        let navigate = navigate.clone();
        spawn_local(async move {
            let _ = if all { crate::api::logout_all().await } else { crate::api::logout().await };
            s.sign_out();
            navigate("/login", Default::default());
        });
    };
    let leave2 = leave.clone();
    let delete = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        spawn_local(async move {
            match crate::api::delete_account(&password.get_untracked()).await {
                Ok(()) => {
                    s.sign_out();
                    let _ = web_sys::window().unwrap().location().set_href("/login");
                }
                Err(e) => error.set(Some(e.message)),
            }
        });
    };
    view! {
        <div class="app-acct-stack">
            <UsernameForm/>
            <EmailForm/>
            <PasswordForm/>
        </div>
        <div class="k-setting">
            <label for="logout">"Se déconnecter"</label>
            <button id="logout" class="k-btn" on:click=move |_| leave(false)>"Déconnexion"</button>
            <p>"Ferme la session sur cet appareil."</p>
        </div>
        <div class="k-setting">
            <label for="logout-all">"Se déconnecter partout"</label>
            <button id="logout-all" class="k-btn" on:click=move |_| leave2(true)>"Tous les appareils"</button>
            <p>"Ferme toutes tes sessions, y compris celle-ci. Utile après la perte d'un appareil."</p>
        </div>
        <div class="k-setting">
            <label for="delete">"Supprimer le compte"</label>
            <button id="delete" class="k-btn k-btn--danger" on:click=move |_| deleting.set(true)>"Supprimer"</button>
            <p>"Ton compte est fermé tout de suite et effacé au bout de 14 jours. Tes messages restent, signés « compte supprimé »."</p>
        </div>
        <Show when=move || deleting.get()>
            <div class="k-scrim" node_ref={
                let root = NodeRef::new();
                modal(root, move || deleting.set(false));
                root
            }>
                <div class="k-dialog" role="alertdialog" aria-modal="true" aria-labelledby="del-t">
                    <header><h2 id="del-t">"Supprimer ton compte ?"</h2></header>
                    <form on:submit=delete>
                        <div>
                            <p>"Tu ne pourras plus te connecter. Confirme avec ton mot de passe."</p>
                            <label class="k-field"><span class="k-label">"Mot de passe"</span>
                                <input class="k-input" type="password" autocomplete="current-password" required bind:value=password/></label>
                            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                        </div>
                        <footer>
                            <button type="button" class="k-btn k-btn--ghost" on:click=move |_| deleting.set(false) autofocus>"Annuler"</button>
                            <button type="submit" class="k-btn k-btn--danger">"Supprimer définitivement"</button>
                        </footer>
                    </form>
                </div>
            </div>
        </Show>
    }
}

/// Mon pseudo : unique, mêmes règles qu'à l'inscription, mot de passe demandé, deux changements
/// par heure au plus.
#[component]
fn UsernameForm() -> impl IntoView {
    let s = store();
    let name = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let culprit = RwSignal::new(None::<Culprit>);
    let note = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let current = move || s.me.get().map(|u| u.username).unwrap_or_default();
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        culprit.set(None);
        note.set(None);
        spawn_local(async move {
            match crate::api::change_username(name.get_untracked().trim(), &password.get_untracked()).await {
                Ok(u) => {
                    note.try_set(Some(format!("Ton pseudo est maintenant @{}.", u.username)));
                    s.update_user(u);
                    name.try_set(String::new());
                    password.try_set(String::new());
                }
                Err(e) => {
                    culprit.try_set(culprit_of(&e.code));
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    view! {
        <form class="app-acct" on:submit=submit aria-labelledby="acct-username-t">
            <h3 id="acct-username-t" class="app-acct-title">"Pseudo"</h3>
            <p id="acct-username-h" class="app-hint">{move || format!(
                "Ton pseudo actuel est @{}. C'est celui que tes amis tapent pour t'ajouter : minuscules, chiffres, points et tirets bas. Tu peux le changer {} fois par heure au plus.",
                current(),
                kraken_proto::USERNAME_CHANGES_PER_WINDOW,
            )}</p>
            <label class="k-field"><span class="k-label">"Nouveau pseudo"</span>
                <input class="k-input" name="new-username" autocomplete="off" autocapitalize="none" spellcheck="false" required
                    minlength="2" maxlength="32" pattern="[a-zA-Z0-9_.]+" bind:value=name
                    aria-describedby=move || if culprit.get() == Some(Culprit::Value) { "acct-username-h acct-username-err" } else { "acct-username-h" }
                    aria-invalid=move || (culprit.get() == Some(Culprit::Value)).then_some("true")/></label>
            <label class="k-field"><span class="k-label">"Mot de passe actuel"</span>
                <input class="k-input" type="password" name="username-password" autocomplete="current-password" required bind:value=password
                    aria-describedby=move || (culprit.get() == Some(Culprit::Password)).then_some("acct-username-err")
                    aria-invalid=move || (culprit.get() == Some(Culprit::Password)).then_some("true")/></label>
            <div class="app-acct-actions">
                <button class="k-btn k-btn--primary" type="submit" disabled=move || busy.get() aria-busy=move || busy.get().to_string()>"Changer le pseudo"</button>
                <Show when=move || note.get().is_some()><p class="app-hint" role="status">{move || note.get()}</p></Show>
            </div>
            <Show when=move || error.get().is_some()><p id="acct-username-err" class="k-error" role="alert">{move || error.get()}</p></Show>
        </form>
    }
}

/// Mon adresse e-mail. Une nouvelle adresse doit être confirmée par le lien qu'on y reçoit ;
/// d'ici là, l'ancienne reste valable.
#[component]
fn EmailForm() -> impl IntoView {
    let account = RwSignal::new(None::<kraken_proto::Account>);
    spawn_local(async move {
        if let Ok(a) = crate::api::account().await {
            account.try_set(Some(a));
        }
    });
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let culprit = RwSignal::new(None::<Culprit>);
    let note = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        culprit.set(None);
        note.set(None);
        spawn_local(async move {
            match crate::api::change_email(email.get_untracked().trim(), &password.get_untracked()).await {
                Ok(a) => {
                    note.try_set(Some(match &a.pending_email {
                        Some(p) => format!(
                            "Un message de confirmation est parti vers {p}. Ton adresse actuelle reste valable jusqu'à ce que tu ouvres le lien."
                        ),
                        None => "Adresse enregistrée.".to_string(),
                    }));
                    account.try_set(Some(a));
                    email.try_set(String::new());
                    password.try_set(String::new());
                }
                Err(e) => {
                    culprit.try_set(culprit_of(&e.code));
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let current = move || {
        account.with(|a| match a {
            None => "Chargement de ton adresse…".to_string(),
            Some(a) => match (&a.email, a.email_verified) {
                (Some(e), true) => format!("Ton adresse actuelle est {e}."),
                (Some(e), false) => format!("Ton adresse actuelle est {e} (pas encore confirmée)."),
                (None, _) => "Aucune adresse n'est enregistrée.".to_string(),
            },
        })
    };
    let pending = move || account.with(|a| a.as_ref().and_then(|a| a.pending_email.clone()));
    view! {
        <form class="app-acct" on:submit=submit aria-labelledby="acct-email-t">
            <h3 id="acct-email-t" class="app-acct-title">"Adresse e-mail"</h3>
            <p id="acct-email-h" class="app-hint">{current}</p>
            <Show when=move || pending().is_some()>
                <p class="app-hint app-acct-pending" role="status">{move || format!(
                    "En attente de confirmation : {}. Ouvre le lien reçu à cette adresse ; d'ici là, l'ancienne reste valable.",
                    pending().unwrap_or_default(),
                )}</p>
            </Show>
            <label class="k-field"><span class="k-label">"Nouvelle adresse e-mail"</span>
                <input class="k-input" type="email" name="new-email" autocomplete="email" required maxlength="254" bind:value=email
                    aria-describedby=move || if culprit.get() == Some(Culprit::Value) { "acct-email-h acct-email-err" } else { "acct-email-h" }
                    aria-invalid=move || (culprit.get() == Some(Culprit::Value)).then_some("true")/></label>
            <label class="k-field"><span class="k-label">"Mot de passe actuel"</span>
                <input class="k-input" type="password" name="email-password" autocomplete="current-password" required bind:value=password
                    aria-describedby=move || (culprit.get() == Some(Culprit::Password)).then_some("acct-email-err")
                    aria-invalid=move || (culprit.get() == Some(Culprit::Password)).then_some("true")/></label>
            <div class="app-acct-actions">
                <button class="k-btn k-btn--primary" type="submit" disabled=move || busy.get() aria-busy=move || busy.get().to_string()>"Changer l'adresse"</button>
                <Show when=move || note.get().is_some()><p class="app-hint" role="status">{move || note.get()}</p></Show>
            </div>
            <Show when=move || error.get().is_some()><p id="acct-email-err" class="k-error" role="alert">{move || error.get()}</p></Show>
        </form>
    }
}

/// Mon mot de passe : l'ancien et le nouveau (8 caractères au moins, comme à l'inscription). Les
/// autres appareils sont déconnectés, celui-ci reste connecté.
#[component]
fn PasswordForm() -> impl IntoView {
    let current = RwSignal::new(String::new());
    let new = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let culprit = RwSignal::new(None::<Culprit>);
    let note = RwSignal::new(None::<&'static str>);
    let busy = RwSignal::new(false);
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        culprit.set(None);
        note.set(None);
        spawn_local(async move {
            match crate::api::change_password(&current.get_untracked(), &new.get_untracked()).await {
                Ok(()) => {
                    note.try_set(Some("Mot de passe changé. Tes autres appareils ont été déconnectés."));
                    current.try_set(String::new());
                    new.try_set(String::new());
                }
                Err(e) => {
                    culprit.try_set(culprit_of(&e.code));
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    view! {
        <form class="app-acct" on:submit=submit aria-labelledby="acct-password-t">
            <h3 id="acct-password-t" class="app-acct-title">"Mot de passe"</h3>
            <p id="acct-password-h" class="app-hint">"8 caractères au moins. Tes autres appareils seront déconnectés ; celui-ci reste ouvert."</p>
            <label class="k-field"><span class="k-label">"Mot de passe actuel"</span>
                <input class="k-input" type="password" name="old-password" autocomplete="current-password" required bind:value=current
                    aria-describedby=move || (culprit.get() == Some(Culprit::Password)).then_some("acct-password-err")
                    aria-invalid=move || (culprit.get() == Some(Culprit::Password)).then_some("true")/></label>
            <label class="k-field"><span class="k-label">"Nouveau mot de passe"</span>
                <input class="k-input" type="password" name="new-password" autocomplete="new-password" required minlength="8"
                    bind:value=new
                    aria-describedby=move || if culprit.get() == Some(Culprit::Value) { "acct-password-h acct-password-err" } else { "acct-password-h" }
                    aria-invalid=move || (culprit.get() == Some(Culprit::Value)).then_some("true")/></label>
            <div class="app-acct-actions">
                <button class="k-btn k-btn--primary" type="submit" disabled=move || busy.get() aria-busy=move || busy.get().to_string()>"Changer le mot de passe"</button>
                <Show when=move || note.get().is_some()><p class="app-hint" role="status">{move || note.get()}</p></Show>
            </div>
            <Show when=move || error.get().is_some()><p id="acct-password-err" class="k-error" role="alert">{move || error.get()}</p></Show>
        </form>
    }
}

#[component]
fn Appearance() -> impl IntoView {
    let current = RwSignal::new(saved_theme());
    let pick = move |t: &'static str| {
        current.set(t.into());
        set_theme(t);
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(THEME_KEY, t);
        }
    };
    view! {
        <fieldset class="k-radios">
            <legend class="k-label app-legend">"Thème"</legend>
            {THEMES.iter().map(|(t, label, sub)| view! {
                <label class="k-radio">
                    <input type="radio" name="theme" prop:checked=move || current.get() == *t on:change=move |_| pick(t)/>
                    <div><b>{*label}</b><span>{*sub}</span></div>
                </label>
            }).collect_view()}
        </fieldset>
    }
}

/// S08 : qui peut t'écrire. Pour l'instant, les comptes bloqués et le moyen de les débloquer.
#[component]
fn Messaging() -> impl IntoView {
    let s = store();
    let error = RwSignal::new(None::<String>);
    let blocked = Memo::new(move |_| {
        s.friends.with(|f| {
            f.iter()
                .filter(|r| r.kind == kraken_proto::RelationshipKind::Blocked)
                .map(|r| r.user.clone())
                .collect::<Vec<_>>()
        })
    });
    let unblock = move |uid: kraken_proto::Id| {
        spawn_local(async move {
            match crate::api::remove_relationship(uid).await {
                Ok(()) => s.remove_rel(uid),
                Err(e) => error.set(Some(e.message)),
            }
        });
    };
    view! {
        <section class="app-blocked-list" aria-labelledby="blocked-t">
            <h3 id="blocked-t" class="k-label">"Comptes bloqués"</h3>
            <p class="app-hint">"Un compte bloqué ne peut plus t'écrire en privé ni te demander en ami ; ses messages sont repliés dans les salons que vous partagez. Il n'est prévenu de rien."</p>
            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            {move || if blocked.with(Vec::is_empty) {
                view! { <div class="k-empty"><b>"Personne n'est bloqué"</b><span>"Bloque quelqu'un depuis ta liste d'amis ou ses demandes."</span></div> }.into_any()
            } else {
                view! {
                    <ul class="app-members">
                        <For each=move || blocked.get() key=|u| u.id let:u>
                            {
                                let id = u.id;
                                let name = u.name().to_string();
                                view! {
                                    <li class="k-dm app-blocked-row">
                                        <Avatar user=u.clone() size="md"/>
                                        <div class="app-ellipsis">{name.clone()}<small>{format!("@{}", u.username)}</small></div>
                                        <button class="k-btn k-btn--sm" aria-label=format!("Débloquer {name}") on:click=move |_| unblock(id)>"Débloquer"</button>
                                    </li>
                                }
                            }
                        </For>
                    </ul>
                }.into_any()
            }}
        </section>
    }
}

/// S09 : notifications sur le bureau (par appareil : la permission est celle du navigateur), et
/// où régler chaque serveur ou salon.
#[component]
fn Notifications() -> impl IntoView {
    use crate::unread::{ask_permission, desktop_enabled, desktop_permission, desktop_supported, set_desktop_enabled};
    use web_sys::NotificationPermission as P;
    let on = RwSignal::new(desktop_enabled());
    let note = RwSignal::new(None::<&'static str>);
    let blocked = move || desktop_supported() && desktop_permission() == P::Denied;
    let toggle = move |e: leptos::ev::Event| {
        let want = event_target_checked(&e);
        note.set(None);
        if !want {
            set_desktop_enabled(false);
            on.set(false);
            return;
        }
        spawn_local(async move {
            // La question du navigateur ne vient qu'ici, après ce clic, jamais au chargement.
            let p = if desktop_permission() == P::Granted { P::Granted } else { ask_permission().await };
            let granted = p == P::Granted;
            set_desktop_enabled(granted);
            on.set(granted);
            if !granted {
                note.set(Some(
                    "Ton navigateur a refusé. Autorise les notifications pour ce site dans ses réglages, puis réessaie.",
                ));
            }
        });
    };
    view! {
        <div class="k-setting">
            <label for="desktop-notifs">"Notifications sur le bureau"</label>
            <input id="desktop-notifs" type="checkbox" role="switch" class="k-switch" aria-describedby="desktop-notifs-h"
                prop:checked=move || on.get() disabled=!desktop_supported() on:change=toggle/>
            <p id="desktop-notifs-h">{if desktop_supported() {
                "Quand KRAKEN n'est pas au premier plan : tes mentions, tes messages privés, et tout salon réglé sur « Tous les messages ». Un clic ouvre la conversation. Réglage propre à cet appareil."
            } else {
                "Ce navigateur (ou cette appli) ne sait pas afficher de notifications système. Les badges et le compteur de l'onglet restent là."
            }}</p>
        </div>
        <Show when=move || note.get().is_some() || (blocked() && !on.get())>
            <p class="k-error" role="alert">{move || note.get().unwrap_or("Les notifications sont bloquées pour ce site dans ton navigateur.")}</p>
        </Show>
        <section class="app-notify-help" aria-labelledby="notify-help-t">
            <h3 id="notify-help-t" class="k-label">"Par serveur et par salon"</h3>
            <p class="app-hint">"Clic droit sur un serveur, un salon ou une conversation (ou Maj+F10 au clavier) : « Marquer comme lu », sourdine de 15 minutes à sans fin, et ce qui te prévient : tous les messages, les mentions seulement, ou rien. La cloche en haut d'un salon ou de la liste des salons ouvre les mêmes réglages. Ils te suivent sur tous tes appareils."</p>
            <p class="app-hint">"Le nombre de mentions en attente s'affiche aussi dans le titre de l'onglet. Alt+Maj+↑ et Alt+Maj+↓ mènent au salon non lu précédent ou suivant."</p>
        </section>
    }
}
