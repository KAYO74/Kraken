//! Réglages du serveur › Emojis : les emplacements du serveur (50 au plus), l'ajout d'une image (bouton ou
//! glisser-déposer), le renommage sur place et le retrait. Qui n'a pas le droit « Gérer les émojis » voit la
//! liste sans pouvoir la changer.
//!
//! Une image fixe passe par la fenêtre de recadrage des photos de profil (carré, 128 px) avant la fenêtre
//! d'ajout, où l'on choisit le nom ; une animation (GIF, WebP) part telle quelle, après vérification de son
//! poids : 256 Ko au plus. Le serveur juge de toute façon (format, dimensions, nom, place).

use gloo_timers::future::TimeoutFuture;
use kraken_core::emoji::{name_from_file, valid_name};
use kraken_core::image::sniff;
use kraken_core::perms;
use kraken_proto::{Emoji, Id, MAX_EMOJI_BYTES, MAX_EMOJIS};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::avatar::{CropDialog, Which, looks_like_image, not_image, too_big};
use crate::emoji::{EmojiImg, shortcode};
use crate::icons::{Icon, lu};
use crate::store::{Store, store};
use crate::ui::{MoreMenu, focus_menu_trigger, modal};

const RULE: &str = "Le nom compte de 2 à 32 caractères : lettres sans accent, chiffres et _.";

/// Une image prête à devenir un emoji : ses octets tels qu'ils partiront, son nom proposé, et l'adresse
/// temporaire qui sert à l'aperçu.
#[derive(Clone, PartialEq)]
struct Ready {
    bytes: Vec<u8>,
    mime: &'static str,
    animated: bool,
    name: String,
    url: String,
}

/// Ce que la page partage avec ses fenêtres.
#[derive(Clone, Copy)]
struct Ui {
    error: RwSignal<Option<String>>,
    note: RwSignal<Option<String>>,
    /// La source d'une image fixe à recadrer : son adresse temporaire, et le nom de son fichier.
    crop: RwSignal<Option<(String, String)>>,
    adding: RwSignal<Option<Ready>>,
}

// Le store est passé en paramètre : après un `.await`, le contexte réactif n'est plus là pour le retrouver.
fn emojis_of(s: Store, guild: Id) -> Vec<Emoji> {
    s.guilds.with_untracked(|gs| gs.iter().find(|g| g.guild.id == guild).map(|g| g.emojis.clone()).unwrap_or_default())
}

fn taken(s: Store, guild: Id, name: &str, except: Option<Id>) -> bool {
    emojis_of(s, guild).iter().any(|e| Some(e.id) != except && e.name.eq_ignore_ascii_case(name))
}

/// Le nom proposé, ou lui suivi de `_2`, `_3`… s'il est pris.
fn free_name(s: Store, guild: Id, name: &str) -> String {
    if !taken(s, guild, name, None) {
        return name.to_string();
    }
    (2..)
        .map(|n| {
            let tail = format!("_{n}");
            let head: String = name.chars().take(32 - tail.len()).collect();
            format!("{head}{tail}")
        })
        .find(|c| !taken(s, guild, c, None))
        .unwrap_or_else(|| name.to_string())
}

fn full() -> String {
    format!("Ce serveur a déjà ses {MAX_EMOJIS} emojis : retires-en un pour en ajouter.")
}

/// Une adresse temporaire pour ces octets (aperçu), à libérer avec `revoke_object_url`. Le navigateur
/// reconnaît l'image à ses octets : pas besoin de lui dire son type.
fn blob_url(bytes: &[u8]) -> Option<String> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

/// Un fichier choisi ou déposé : une animation passe tout de suite à la fenêtre d'ajout ; une image fixe
/// s'ouvre d'abord dans la fenêtre de recadrage.
async fn take(s: Store, file: web_sys::File, guild: Id, ui: Ui) {
    ui.error.set(None);
    ui.note.set(None);
    if emojis_of(s, guild).len() >= MAX_EMOJIS {
        return ui.error.set(Some(full()));
    }
    // Une image fixe est recadrée et réduite avant l'envoi : seule la source peut peser lourd.
    if file.size() > Which::Emoji.max_bytes() as f64 {
        return ui.error.set(Some(too_big(Which::Emoji)));
    }
    let head = match file.slice_with_i32_and_i32(0, 12) {
        Ok(b) => JsFuture::from(b.array_buffer()).await.ok(),
        Err(_) => None,
    };
    let head = head.map(|b| js_sys::Uint8Array::new(&b).to_vec()).unwrap_or_default();
    if !looks_like_image(&head) {
        return ui.error.set(Some(not_image()));
    }
    let name = name_from_file(&file.name());
    if head.starts_with(b"GIF8") || head.get(8..12) == Some(b"WEBP") {
        let all = match JsFuture::from(file.array_buffer()).await {
            Ok(b) => js_sys::Uint8Array::new(&b).to_vec(),
            Err(_) => return ui.error.set(Some(not_image())),
        };
        if let Some(found) = sniff(&all).filter(|f| f.animated) {
            if all.len() > MAX_EMOJI_BYTES {
                return ui
                    .error
                    .set(Some(format!("Cette animation pèse trop lourd : {} Ko au plus.", MAX_EMOJI_BYTES / 1024)));
            }
            let mime = found.kind.mime();
            let Some(url) = blob_url(&all) else { return ui.error.set(Some(not_image())) };
            ui.adding.set(Some(Ready { bytes: all, mime, animated: true, name: free_name(s, guild, &name), url }));
            return;
        }
    }
    match web_sys::Url::create_object_url_with_blob(&file) {
        Ok(url) => ui.crop.set(Some((url, name))),
        Err(_) => ui.error.set(Some(not_image())),
    }
}

/// Redonne le focus au bouton ⋯ de la ligne de cet emoji.
fn focus_row(id: Id) {
    spawn_local(async move {
        TimeoutFuture::new(60).await;
        let sel = format!("[data-emoji=\"{id}\"] .app-more > button");
        if let Some(b) =
            document().query_selector(&sel).ok().flatten().and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = b.focus();
        }
    });
}

fn focus_add() {
    spawn_local(async move {
        TimeoutFuture::new(60).await;
        if let Some(b) =
            document().get_element_by_id("emoji-add").and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = b.focus();
        }
    });
}

#[component]
pub fn EmojiSettings(guild: Id) -> impl IntoView {
    let s = store();
    let ui = Ui {
        error: RwSignal::new(None),
        note: RwSignal::new(None),
        crop: RwSignal::new(None),
        adding: RwSignal::new(None),
    };
    let input = NodeRef::<html::Input>::new();
    let renaming = RwSignal::new(None::<Id>);
    let deleting = RwSignal::new(None::<Emoji>);
    let dragging = RwSignal::new(false);
    let depth = StoredValue::new(0i32);
    let emojis = Memo::new(move |_| {
        s.guilds.with(|gs| gs.iter().find(|g| g.guild.id == guild).map(|g| g.emojis.clone()).unwrap_or_default())
    });
    let ids = Memo::new(move |_| emojis.with(|v| v.iter().map(|e| e.id).collect::<Vec<_>>()));
    let count = Memo::new(move |_| ids.with(Vec::len));
    let can_manage = Memo::new(move |_| s.guild_perms(guild) & perms::MANAGE_EMOJIS != 0);

    let choose = move |_| {
        ui.error.set(None);
        ui.note.set(None);
        if count.get_untracked() >= MAX_EMOJIS {
            ui.error.set(Some(full()));
        } else if let Some(i) = input.get_untracked() {
            i.click();
        }
    };
    let on_change = move |_| {
        let Some(input) = input.get_untracked() else { return };
        let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
        // Le même fichier, choisi deux fois de suite, doit redéclencher le changement.
        input.set_value("");
        spawn_local(take(s, file, guild, ui));
    };

    // Glisser une image sur la page : un cadre dit qu'elle sera prise. Le compteur absorbe les dragenter /
    // dragleave des éléments enfants.
    let carries_files = |e: &web_sys::DragEvent| {
        e.data_transfer().is_some_and(|dt| {
            let types = dt.types();
            (0..types.length()).any(|i| types.get(i).as_string().as_deref() == Some("Files"))
        })
    };
    let on_drag_enter = move |e: web_sys::DragEvent| {
        if !can_manage.get_untracked() || !carries_files(&e) {
            return;
        }
        e.prevent_default();
        depth.update_value(|d| *d += 1);
        dragging.set(true);
    };
    let on_drag_over = move |e: web_sys::DragEvent| {
        if can_manage.get_untracked() && carries_files(&e) {
            e.prevent_default();
        }
    };
    let on_drag_leave = move |_: web_sys::DragEvent| {
        depth.update_value(|d| *d = (*d - 1).max(0));
        if depth.get_value() == 0 {
            dragging.set(false);
        }
    };
    let on_drop = move |e: web_sys::DragEvent| {
        depth.set_value(0);
        dragging.set(false);
        if !can_manage.get_untracked() || !carries_files(&e) {
            return;
        }
        e.prevent_default();
        let files = crate::files::files_of(e.data_transfer().and_then(|d| d.files()));
        let many = files.len() > 1;
        if let Some(file) = files.into_iter().next() {
            spawn_local(async move {
                take(s, file, guild, ui).await;
                if many && ui.error.get_untracked().is_none() {
                    ui.note.try_set(Some("Un emoji à la fois : seul le premier fichier a été pris.".into()));
                }
            });
        }
    };

    // Image fixe recadrée : la fenêtre d'ajout s'ouvre après la fermeture de celle du recadrage (leurs piles de
    // focus ne se mêlent pas).
    let cropped = Callback::new(move |bytes: Vec<u8>| {
        let name = ui.crop.get_untracked().map(|(_, n)| n).unwrap_or_default();
        let Some(url) = blob_url(&bytes) else { return ui.error.set(Some(not_image())) };
        let ready = Ready { bytes, mime: "image/png", animated: false, name: free_name(s, guild, &name), url };
        spawn_local(async move {
            TimeoutFuture::new(40).await;
            ui.adding.try_set(Some(ready));
        });
    });
    let crop_done = Callback::new(move |result: Option<Result<(), String>>| {
        ui.crop.set(None);
        if let Some(Err(e)) = result {
            ui.error.set(Some(e));
        }
    });
    let added = Callback::new(move |e: Emoji| {
        ui.adding.set(None);
        ui.note.set(Some(format!("Emoji {} ajouté.", shortcode(&e.name))));
        focus_add();
    });
    let add_close = Callback::new(move |()| ui.adding.set(None));

    let add_button = move || {
        view! {
            <button type="button" id="emoji-add" class="k-btn k-btn--primary" aria-describedby="emoji-hint" on:click=choose>
                <Icon icon=lu::LuPlus/>"Ajouter un emoji"
            </button>
        }
    };

    view! {
        <div class="app-emojis" data-over=move || dragging.get().then_some("")
            on:dragenter=on_drag_enter on:dragover=on_drag_over on:dragleave=on_drag_leave on:drop=on_drop>
            <p class="app-hint">"Ajoute des images à ce serveur : ses membres les écrivent dans leurs messages avec "<code>":nom:"</code>" ou les choisissent dans le sélecteur d'emojis, et s'en servent aussi pour réagir dans leurs autres serveurs."</p>
            <div class="app-emoji-head">
                <p class="app-emoji-count"><b>{move || count.get()}</b>" / "{MAX_EMOJIS}" emplacements"</p>
                <Show when=move || { can_manage.get() && count.get() > 0 }>{add_button}</Show>
            </div>
            <input id="emoji-file" type="file" class="k-sr app-file" tabindex="-1" aria-hidden="true"
                accept="image/png,image/jpeg,image/webp,image/gif" node_ref=input on:change=on_change/>
            <p id="emoji-hint" class="k-hint">{format!("PNG, JPEG, WebP ou GIF. Une image fixe est recadrée en carré et réduite à 128 px ; une animation reste telle quelle et pèse {} Ko au plus.", MAX_EMOJI_BYTES / 1024)}</p>
            <Show when=move || { !can_manage.get() }>
                <p class="k-hint">"Il faut le droit « Gérer les émojis » pour ajouter, renommer ou retirer un emoji."</p>
            </Show>
            <Show when=move || { can_manage.get() && count.get() >= MAX_EMOJIS }>
                <p class="k-hint">"Tous les emplacements sont pris : retire un emoji pour en ajouter un autre."</p>
            </Show>
            <Show when=move || ui.error.get().is_some()><p class="k-error" role="alert">{move || ui.error.get()}</p></Show>
            <Show when=move || ui.note.get().is_some()><p class="k-hint app-emoji-note" role="status">{move || ui.note.get()}</p></Show>
            <Show when=move || { count.get() == 0 }>
                <div class="k-empty">
                    <b>"Aucun emoji pour l'instant"</b>
                    <span>"Les emojis du serveur apparaîtront ici, et dans le sélecteur de ses membres."</span>
                    {move || can_manage.get().then(add_button)}
                </div>
            </Show>
            <ul class="app-emoji-list" aria-label="Emojis du serveur">
                <For each=move || ids.get() key=|id| *id let:id>
                    <Row guild=guild id=id emojis=emojis can_manage=can_manage renaming=renaming deleting=deleting/>
                </For>
            </ul>
            <div class="app-emoji-drop" aria-hidden="true"><Icon icon=lu::LuUpload/>"Dépose l'image pour l'ajouter"</div>
        </div>
        {move || ui.crop.get().map(|(url, _)| view! { <CropDialog url=url which=Which::Emoji done=crop_done cropped=cropped/> })}
        {move || ui.adding.get().map(|r| view! { <AddDialog guild=guild ready=r added=added close=add_close/> })}
        {move || deleting.get().map(|e| view! { <DeleteDialog guild=guild emoji=e done=ui.note close=Callback::new(move |()| deleting.set(None))/> })}
    }
}

/// Une ligne : l'image, `:nom:`, qui l'a ajouté, et ⋯ (renommer, supprimer).
#[component]
fn Row(
    guild: Id,
    id: Id,
    emojis: Memo<Vec<Emoji>>,
    can_manage: Memo<bool>,
    renaming: RwSignal<Option<Id>>,
    deleting: RwSignal<Option<Emoji>>,
) -> impl IntoView {
    let me = move || emojis.with(|v| v.iter().find(|e| e.id == id).cloned());
    let name = move || me().map(|e| e.name).unwrap_or_default();
    let by = move || {
        me().map(|e| match e.creator {
            Some(c) => format!("ajouté par {}", c.name),
            None => "ajouté par un ancien membre".to_string(),
        })
    };
    let first = name();
    let editing = move || renaming.get() == Some(id);
    view! {
        <li class="app-row app-emoji-row" data-emoji=id.to_string()>
            <span class="app-emoji-thumb" aria-hidden="true"><EmojiImg id=id name=first/></span>
            <div>
                <Show when=move || !editing() fallback=move || view! { <RenameField guild=guild id=id name=name() renaming=renaming/> }>
                    <b class="app-emoji-code">{move || shortcode(&name())}</b>
                    <small>{by}</small>
                </Show>
            </div>
            <Show when=move || can_manage.get() && !editing()>
                // Redessiné au renommage : son nom accessible dit toujours de quel emoji il s'agit.
                {move || view! {
                    <MoreMenu label=format!("Actions pour {}", shortcode(&name()))>
                        <button type="button" class="k-menuitem" on:click=move |e| {
                            focus_menu_trigger(&e);
                            renaming.set(Some(id));
                        }><Icon icon=lu::LuPencil/>"Renommer"</button>
                        <button type="button" class="k-menuitem is-danger" on:click=move |e| {
                            focus_menu_trigger(&e);
                            deleting.set(me());
                        }><Icon icon=lu::LuTrash2/>"Supprimer"</button>
                    </MoreMenu>
                }}
            </Show>
        </li>
    }
}

/// Le nom devenu champ : Entrée valide, Échap annule.
#[component]
fn RenameField(guild: Id, id: Id, name: String, renaming: RwSignal<Option<Id>>) -> impl IntoView {
    let s = store();
    let value = RwSignal::new(name.clone());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let field = NodeRef::<html::Input>::new();
    Effect::new(move || {
        if let Some(i) = field.get() {
            let _ = i.focus();
            i.select();
        }
    });
    let cancel = move || {
        renaming.set(None);
        focus_row(id);
    };
    let original = name.clone();
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        let n = value.get_untracked().trim().to_string();
        if n == original {
            return cancel();
        }
        if !valid_name(&n) {
            return error.set(Some(RULE.into()));
        }
        if taken(s, guild, &n, Some(id)) {
            return error.set(Some("Ce nom est déjà pris dans ce serveur.".into()));
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match crate::api::rename_emoji(guild, id, &n).await {
                Ok(e) => {
                    s.in_guild(guild, |g| {
                        if let Some(x) = g.emojis.iter_mut().find(|x| x.id == e.id) {
                            *x = e;
                        }
                    });
                    renaming.try_set(None);
                    focus_row(id);
                }
                Err(e) => {
                    busy.try_set(false);
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    let err_id = format!("emoji-rename-e-{id}");
    let hint_id = format!("emoji-rename-h-{id}");
    let (err_id2, hint_id2) = (err_id.clone(), hint_id.clone());
    view! {
        <form class="app-emoji-rename" on:submit=submit>
            <input class="k-input" type="text" node_ref=field autocomplete="off" spellcheck="false" maxlength="32"
                aria-label=format!("Nouveau nom de {}", shortcode(&name))
                aria-invalid=move || error.get().is_some().then_some("true")
                aria-describedby=move || if error.get().is_some() { format!("{hint_id2} {err_id2}") } else { hint_id2.clone() }
                prop:value=move || value.get() on:input=move |e| { value.set(event_target_value(&e)); error.set(None); }
                on:keydown:capture=move |e| if e.key() == "Escape" {
                    e.prevent_default();
                    e.stop_propagation();
                    cancel();
                }/>
            <small id=hint_id class="k-hint">"Entrée valide, Échap annule."</small>
            <Show when=move || error.get().is_some()><small id=err_id.clone() class="k-error" role="alert">{move || error.get()}</small></Show>
        </form>
    }
}

/// Le nom de l'emoji, avant d'envoyer.
#[component]
fn AddDialog(guild: Id, ready: Ready, added: Callback<Emoji>, close: Callback<()>) -> impl IntoView {
    let s = store();
    let root = NodeRef::<html::Div>::new();
    modal(root, move || close.run(()));
    let url = ready.url.clone();
    on_cleanup({
        let url = url.clone();
        move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    });
    let name = RwSignal::new(ready.name.clone());
    let field_error = RwSignal::new(None::<String>);
    let dialog_error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let field = NodeRef::<html::Input>::new();
    let ready = StoredValue::new(ready);
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        let n = name.get_untracked().trim().to_string();
        let refuse = if !valid_name(&n) {
            Some(RULE.to_string())
        } else if taken(s, guild, &n, None) {
            Some("Ce nom est déjà pris dans ce serveur.".to_string())
        } else {
            None
        };
        if let Some(r) = refuse {
            field_error.set(Some(r));
            if let Some(i) = field.get_untracked() {
                let _ = i.focus();
            }
            return;
        }
        busy.set(true);
        field_error.set(None);
        dialog_error.set(None);
        let r = ready.get_value();
        spawn_local(async move {
            match crate::api::add_emoji(guild, &n, &r.bytes, r.mime).await {
                Ok(e) => {
                    s.in_guild(guild, |g| {
                        if !g.emojis.iter().any(|x| x.id == e.id) {
                            g.emojis.push(e.clone());
                        }
                    });
                    added.run(e);
                }
                Err(e) => {
                    busy.try_set(false);
                    if matches!(e.code.as_str(), "name_taken" | "invalid_name") {
                        field_error.try_set(Some(e.message));
                        if let Some(i) = field.get_untracked() {
                            let _ = i.focus();
                        }
                    } else {
                        dialog_error.try_set(Some(e.message));
                    }
                }
            }
        });
    };
    // L'aperçu d'une animation : sa première image, fixe, et l'animation elle-même au survol ou au focus (jamais
    // en mouvement réduit). La page n'a pas le droit de relire une adresse `blob:` (CSP), d'où la première
    // image tirée des octets qu'on a déjà.
    let preview = ready.with_value(|r| (r.animated, r.url.clone()));
    let frozen = RwSignal::new(None::<Option<String>>);
    if preview.0 {
        let bytes = ready.with_value(|r| r.bytes.clone());
        spawn_local(async move {
            let f = crate::photo::first_frame(&bytes, 128).await;
            frozen.try_set(Some(f));
        });
    }
    let shown = {
        let url = preview.1.clone();
        move || match (preview.0, frozen.get()) {
            (true, Some(Some(f))) => view! {
                <img class="app-emoji-add-img k-still" src=f alt="Aperçu de l'emoji"/>
                <img class="app-emoji-add-img k-live" src=url.clone() alt=""/>
            }
            .into_any(),
            (true, None) => {
                view! { <img class="app-emoji-add-img" src=url.clone() alt="Aperçu de l'emoji" data-probing=""/> }
                    .into_any()
            }
            _ => view! { <img class="app-emoji-add-img" src=url.clone() alt="Aperçu de l'emoji"/> }.into_any(),
        }
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog app-emoji-add" role="dialog" aria-modal="true" aria-labelledby="emoji-add-t">
                <header><h2 id="emoji-add-t">"Ajouter un emoji"</h2></header>
                <form on:submit=submit>
                    <div>
                        <div class="app-emoji-add-body">
                            <span class="app-emoji-add-preview" data-animated=preview.0.then_some("")>{shown}</span>
                            <div class="k-field">
                                <label class="k-label" for="emoji-name">"Nom"</label>
                                <input id="emoji-name" class="k-input" type="text" autocomplete="off" spellcheck="false" maxlength="32" autofocus
                                    node_ref=field
                                    aria-invalid=move || field_error.get().is_some().then_some("true")
                                    aria-describedby=move || if field_error.get().is_some() { "emoji-name-h emoji-name-e" } else { "emoji-name-h" }
                                    prop:value=move || name.get()
                                    on:input=move |e| { name.set(event_target_value(&e)); field_error.set(None); }/>
                                <p id="emoji-name-h" class="k-hint">"Dans un message : "<code>{move || shortcode(name.get().trim())}</code></p>
                                <Show when=move || field_error.get().is_some()><p id="emoji-name-e" class="k-error" role="alert">{move || field_error.get()}</p></Show>
                            </div>
                        </div>
                        <Show when=move || dialog_error.get().is_some()><p class="k-error" role="alert">{move || dialog_error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| close.run(())>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--primary" aria-busy=move || busy.get().to_string()>"Ajouter l'emoji"</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}

/// Confirmer le retrait.
#[component]
fn DeleteDialog(guild: Id, emoji: Emoji, done: RwSignal<Option<String>>, close: Callback<()>) -> impl IntoView {
    let s = store();
    let root = NodeRef::<html::Div>::new();
    modal(root, move || close.run(()));
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let code = shortcode(&emoji.name);
    let title = format!("Supprimer {code} ?");
    let (id, label) = (emoji.id, code.clone());
    let confirm = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let label = label.clone();
        spawn_local(async move {
            match crate::api::delete_emoji(guild, id).await {
                Ok(()) => {
                    s.in_guild(guild, |g| g.emojis.retain(|x| x.id != id));
                    close.run(());
                    done.try_set(Some(format!("Emoji {label} supprimé.")));
                    focus_add();
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
            <div class="k-dialog" role="alertdialog" aria-modal="true" aria-labelledby="emoji-del-t" aria-describedby="emoji-del-d">
                <header><h2 id="emoji-del-t">{title}</h2></header>
                <form on:submit=confirm>
                    <div>
                        <p id="emoji-del-d">"L'emoji disparaît du serveur et du sélecteur. Les messages et les réactions qui l'utilisent restent, mais n'affichent plus que son nom. Cette action est définitive."</p>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| close.run(()) autofocus>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--danger" aria-busy=move || busy.get().to_string()>"Supprimer l'emoji"</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}
