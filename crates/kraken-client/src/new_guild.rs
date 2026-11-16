//! S21 : créer un serveur (un modèle, un nom, une icône facultative) ou en rejoindre un avec une invitation.
//!
//! Le modèle est un groupe de boutons radio natifs (les flèches passent d'une carte à l'autre, une seule est dans
//! l'ordre de tabulation) ; le choix est écrit en toutes lettres (coche + bordure, pas la couleur seule), et la
//! liste « Sera créé » suit le choix. L'icône est envoyée juste après la création : si cet envoi échoue, le
//! serveur existe quand même et on y arrive avec une information à fermer (la page du serveur en dit plus), car
//! la création, elle, est faite. Une création qui échoue ne laisse rien derrière elle.

use kraken_core::templates::{self, SlotKind, Template};
use kraken_proto::GuildState;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::i18n::{t, tf};
use crate::icon_picker::{IconFile, IconPicker};
use crate::icons::{Icon, lu};
use crate::shell::first_text;
use crate::store::store;
use crate::tk;
use crate::ui::{Header, initials};

/// Le nom d'un salon du modèle, dans la langue de l'interface. Au catalogue, `annonces` est déjà le type de salon
/// « salon d'annonces » (`shell.rs`) : le salon du modèle a sa propre clé (voir `guild::label`).
fn slot_name(name: &str) -> String {
    if name == "annonces" { crate::guild::label(tk!("annonces{contexte}")) } else { t(name) }
}

/// Ce que dessine la carte d'un modèle.
fn template_icon(id: &str) -> icondata_core::Icon {
    match id {
        "communaute" => lu::LuUsers,
        "jeux" => lu::LuGamepad2,
        "etudes" => lu::LuGraduationCap,
        "amis" => lu::LuHeart,
        _ => lu::LuSquareDashed,
    }
}

/// L'icône choisie, gardée jusqu'à la création : l'image, son adresse temporaire (aperçu).
#[derive(Clone, PartialEq)]
struct Staged {
    file: IconFile,
    url: String,
}

/// L'aperçu d'une icône choisie. Une animation : sa première image, fixe, et l'animation elle-même (jamais en
/// mouvement réduit, voir `.app-gicon-preview`). La page n'a pas le droit de relire une adresse `blob:` (CSP),
/// d'où la première image tirée des octets qu'on a déjà.
#[component]
fn StagedImage(staged: Staged) -> impl IntoView {
    let frozen = RwSignal::new(None::<Option<String>>);
    let animated = staged.file.animated;
    if animated {
        let bytes = staged.file.bytes.clone();
        spawn_local(async move {
            let f = crate::photo::first_frame(&bytes, 256).await;
            frozen.try_set(Some(f));
        });
    }
    let url = staged.url;
    move || match (animated, frozen.get()) {
        (true, Some(Some(f))) => view! {
            <img class="k-still" src=f alt=""/>
            <img class="k-live" src=url.clone() alt=""/>
        }
        .into_any(),
        (true, None) => view! { <img src=url.clone() alt="" data-probing=""/> }.into_any(),
        _ => view! { <img src=url.clone() alt=""/> }.into_any(),
    }
}

/// Une carte de modèle : une coche et une bordure disent qu'elle est choisie.
#[component]
fn Card(template: &'static Template, chosen: RwSignal<&'static str>) -> impl IntoView {
    let id = template.id;
    let desc = format!("ng-tpl-{id}");
    view! {
        <label class="app-tpl" data-checked=move || (chosen.get() == id).then_some("")>
            <input class="k-sr" type="radio" name="template" value=id aria-describedby=desc.clone()
                prop:checked=move || chosen.get() == id on:change=move |_| chosen.set(id)/>
            <span class="app-tpl-icon"><Icon icon=template_icon(id)/></span>
            <span class="app-tpl-body">
                <b>{t(template.name)}</b>
                <span id=desc>{t(template.description)}</span>
            </span>
            <span class="app-tpl-check" aria-hidden="true"><Icon icon=lu::LuCheck small=true/></span>
        </label>
    }
}

#[component]
pub fn CreateGuild() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let chosen = RwSignal::new(templates::DEFAULT_ID);
    let name = RwSignal::new(String::new());
    // Le nom proposé arrive avec le compte (un rechargement de la page le précède), tant qu'on n'a rien écrit.
    let typed = RwSignal::new(false);
    Effect::new(move || {
        if !typed.get_untracked()
            && let Some(u) = s.me.get()
        {
            name.set(tf("Serveur de {nom}", &[("nom", u.name())]));
        }
    });
    let code = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // Seule une erreur de nom marque le champ Nom (une faute, un champ) ; les autres (réseau, serveur) n'en marquent aucun.
    let name_error = RwSignal::new(false);
    let name_input = NodeRef::<html::Input>::new();
    let retry = NodeRef::<html::Button>::new();
    // Une erreur apparaît pendant que le bouton « Créer le serveur » est grisé (donc sans focus) : le focus va au champ
    // fautif, ou à « Réessayer ».
    Effect::new(move || {
        if error.get().is_none() {
            return;
        }
        if name_error.get_untracked() {
            if let Some(i) = name_input.get() {
                let _ = i.focus();
            }
        } else if let Some(b) = retry.get() {
            let _ = b.focus();
        }
    });
    let join_error = RwSignal::new(None::<String>);
    let staged = RwSignal::new(None::<Staged>);
    let icon_error = RwSignal::new(None::<String>);
    let forget = move |old: Option<Staged>| {
        if let Some(old) = old {
            let _ = web_sys::Url::revoke_object_url(&old.url);
        }
    };
    on_cleanup(move || forget(staged.try_get_untracked().flatten()));
    let on_pick = Callback::new(move |file: IconFile| {
        let Some(url) = crate::emoji_settings::blob_url(&file.bytes) else {
            return icon_error.set(Some(crate::avatar::not_image()));
        };
        icon_error.set(None);
        forget(staged.get_untracked());
        staged.set(Some(Staged { file, url }));
    });
    let on_remove = Callback::new(move |()| {
        icon_error.set(None);
        forget(staged.get_untracked());
        staged.set(None);
    });

    let template = Memo::new(move |_| templates::find(chosen.get()).unwrap_or(templates::default_template()));
    let nav = navigate.clone();
    // Créer le serveur, puis lui envoyer l'icône choisie ; ouvrir son premier salon textuel.
    let create = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        name_error.set(false);
        let nav = nav.clone();
        spawn_local(async move {
            let made: Result<GuildState, _> =
                crate::api::create_guild(&name.get_untracked(), chosen.get_untracked()).await;
            match made {
                Ok(g) => {
                    let gid = g.guild.id;
                    let to = format!("/channels/{gid}/{}", first_text(&g).map(|c| c.to_string()).unwrap_or_default());
                    s.upsert_guild(g);
                    if let Some(st) = staged.get_untracked() {
                        match crate::api::set_guild_icon(gid, &st.file.bytes, st.file.mime).await {
                            Ok(g) => s.update_guild(g),
                            Err(e) => s.notice.set(Some(tf(
                                "Le serveur est créé, mais son icône n'a pas pu être envoyée : {erreur} Tu peux la choisir dans les paramètres du serveur, rubrique « Vue d'ensemble ».",
                                &[("erreur", &e.message)],
                            ))),
                        }
                    }
                    nav(&to, Default::default());
                }
                Err(e) => {
                    name_error.try_set(matches!(e.code.as_str(), "invalid" | "invalid_chars"));
                    error.try_set(Some(e.message));
                }
            }
            busy.try_set(false);
        });
    };
    let create2 = create.clone();
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        create();
    };
    let join = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let raw = code.get_untracked();
        let c = raw.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
        if c.is_empty() {
            join_error.set(Some(t("Colle un lien ou un code d'invitation.")));
            return;
        }
        navigate(&format!("/invite/{c}"), Default::default());
    };
    view! {
        <Header><h1 class="app-title">{t("Crée ton serveur")}</h1></Header>
        <div class="app-friends app-ng">
            <form class="app-ng-form" on:submit=submit>
                <p class="app-hint">{t("Un espace pour ta bande, avec des salons textuels et vocaux. Choisis un point de départ : tu pourras tout changer ensuite.")}</p>
                <section aria-labelledby="ng-tpl-h" class="app-ng-sec">
                    <h2 id="ng-tpl-h" class="app-ov-h">{t("Modèle")}</h2>
                    <div class="app-tpls" role="radiogroup" aria-labelledby="ng-tpl-h">
                        {templates::ALL.iter().map(|tpl| view! { <Card template=tpl chosen=chosen/> }).collect_view()}
                    </div>
                    <div class="app-ng-made" aria-live="polite">
                        <p class="k-label">{t("Sera créé :")}</p>
                        <ul>
                            {move || template.get().slots().map(|slot| {
                                let voice = slot.kind == SlotKind::Voice;
                                view! {
                                    <li>
                                        <Icon icon=if voice { lu::LuVolume2 } else { lu::LuHash } small=true/>
                                        <span class="k-sr">{if voice { t("Salon vocal") } else { t("Salon textuel") }}" "</span>
                                        {slot_name(slot.name)}
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    </div>
                </section>
                <section class="app-ng-sec">
                    <label class="k-field">
                        <span class="k-label">{t("Nom du serveur")}</span>
                        <input class="k-input" required minlength="2" maxlength="100" bind:value=name
                            node_ref=name_input on:input=move |_| typed.set(true)
                            aria-invalid=move || (error.get().is_some() && name_error.get()).then_some("true")
                            aria-describedby=move || (error.get().is_some() && name_error.get()).then_some("ng-err")/>
                    </label>
                </section>
                <section aria-labelledby="ng-icon-h" class="app-ng-sec">
                    <h2 id="ng-icon-h" class="app-ov-h">{t("Icône")}" "<span class="app-hint">{t("(facultative)")}</span></h2>
                    <IconPicker id="ng-icon" has_icon=Signal::derive(move || staged.get().is_some()) busy=busy error=icon_error
                        on_pick=on_pick on_remove=on_remove>
                        <span class="app-gicon-preview" role="img"
                            aria-label=move || if staged.get().is_some() { t("Icône choisie") } else { t("Pas d'icône : les initiales du serveur") }>
                            {move || match staged.get() {
                                Some(st) => view! { <StagedImage staged=st/> }.into_any(),
                                None => initials(&name.get()).into_any(),
                            }}
                        </span>
                    </IconPicker>
                </section>
                <Show when=move || error.get().is_some()>
                    <div class="k-banner k-banner--warning app-ng-error" id="ng-err" role="alert">
                        <Icon icon=lu::LuCircleAlert/>
                        <span>{move || error.get()}</span>
                        <button class="k-btn" type="button" node_ref=retry on:click={ let again = create2.clone(); move |_| again() }>{t("Réessayer")}</button>
                    </div>
                </Show>
                <div class="app-ng-actions">
                    <a class="k-btn k-btn--ghost" href="/channels/@me">{t("Annuler")}</a>
                    <button class="k-btn k-btn--primary k-btn--lg" type="submit" aria-busy=move || busy.get().to_string() disabled=move || busy.get()>
                        {t("Créer le serveur")}
                    </button>
                </div>
                <p class="k-hint app-ng-status" role="status">{move || busy.get().then(|| t("Création…"))}</p>
            </form>
            <form on:submit=join class="app-ng-join">
                <label class="k-field">
                    <span class="k-label">{t("Tu as déjà une invitation ?")}</span>
                    <input class="k-input" placeholder="https://…/invite/AbC123xy" bind:value=code aria-describedby="cj-e"/>
                    <Show when=move || join_error.get().is_some()>
                        <span class="k-error" id="cj-e" role="alert">{move || join_error.get()}</span>
                    </Show>
                </label>
                <button class="k-btn" type="submit">{t("Rejoindre")}</button>
            </form>
        </div>
    }
}
