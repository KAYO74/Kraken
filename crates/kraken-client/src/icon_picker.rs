//! Choisir l'icône d'un serveur : un bouton, ou une image déposée sur la zone, puis la fenêtre de recadrage
//! des photos de profil (carré, 256 px) pour une image fixe. Une animation (GIF, WebP) part telle quelle, après
//! vérification de son poids. Le composant rend l'image prête (`IconFile`) à qui l'a posé : les réglages du serveur
//! l'envoient aussitôt, la page de création la garde pour l'envoyer juste après avoir créé le serveur. Le serveur
//! juge de toute façon (format, dimensions, poids).

use kraken_core::image::sniff;
use kraken_proto::MAX_GUILD_ICON_BYTES;
use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen_futures::JsFuture;

use crate::avatar::{CropDialog, Which, looks_like_image, not_image, too_big};
use crate::icons::{Icon, lu};

/// Une image prête à devenir l'icône : ses octets tels qu'ils partiront, et leur type.
#[derive(Clone, PartialEq)]
pub struct IconFile {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub animated: bool,
}

fn heavy_animation() -> String {
    format!("Cette animation pèse trop lourd : {} Mo au plus.", MAX_GUILD_ICON_BYTES / (1024 * 1024))
}

/// Un fichier choisi ou déposé : une animation passe tout de suite ; une image fixe s'ouvre dans la fenêtre de
/// recadrage (`crop` : son adresse temporaire).
async fn take(
    file: web_sys::File,
    crop: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    on_pick: Callback<IconFile>,
) {
    error.try_set(None);
    // Une image fixe est recadrée et réduite avant l'envoi : seule la source peut peser lourd.
    if file.size() > Which::Icon.max_bytes() as f64 {
        error.try_set(Some(too_big(Which::Icon)));
        return;
    }
    let head = match file.slice_with_i32_and_i32(0, 12) {
        Ok(b) => JsFuture::from(b.array_buffer()).await.ok(),
        Err(_) => None,
    };
    let head = head.map(|b| js_sys::Uint8Array::new(&b).to_vec()).unwrap_or_default();
    if !looks_like_image(&head) {
        error.try_set(Some(not_image()));
        return;
    }
    // GIF et WebP : animé ou non ? Un fichier animé n'est pas recadré, il part tel quel.
    if head.starts_with(b"GIF8") || head.get(8..12) == Some(b"WEBP") {
        let all = match JsFuture::from(file.array_buffer()).await {
            Ok(b) => js_sys::Uint8Array::new(&b).to_vec(),
            Err(_) => {
                error.try_set(Some(not_image()));
                return;
            }
        };
        if let Some(found) = sniff(&all).filter(|f| f.animated) {
            if all.len() > MAX_GUILD_ICON_BYTES {
                error.try_set(Some(heavy_animation()));
                return;
            }
            on_pick.run(IconFile { bytes: all, mime: found.kind.mime(), animated: true });
            return;
        }
    }
    match web_sys::Url::create_object_url_with_blob(&file) {
        Ok(url) => {
            crop.try_set(Some(url));
        }
        Err(_) => {
            error.try_set(Some(not_image()));
        }
    }
}

/// Le choix de l'icône : l'aperçu (`children`, dessiné par l'appelant), « Choisir une image », « Retirer l'icône »
/// s'il y en a une, la règle, l'erreur du champ, et la zone de dépôt qui couvre le tout. `id` préfixe les
/// identifiants de la zone (un par page).
#[component]
pub fn IconPicker(
    id: &'static str,
    #[prop(into)] has_icon: Signal<bool>,
    #[prop(into)] busy: Signal<bool>,
    error: RwSignal<Option<String>>,
    on_pick: Callback<IconFile>,
    on_remove: Callback<()>,
    children: Children,
) -> impl IntoView {
    let input = NodeRef::<html::Input>::new();
    let crop = RwSignal::new(None::<String>);
    let dragging = RwSignal::new(false);
    let depth = StoredValue::new(0i32);

    let pick = move |file: web_sys::File| {
        if busy.get_untracked() {
            return;
        }
        spawn_local(take(file, crop, error, on_pick));
    };
    let on_change = move |_| {
        error.set(None);
        let Some(input) = input.get_untracked() else { return };
        let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
        // Le même fichier, choisi deux fois de suite, doit redéclencher le changement.
        input.set_value("");
        pick(file);
    };
    let remove = move |_| {
        error.set(None);
        on_remove.run(());
        // Le bouton disparaît avec l'icône : le focus passe au choix d'une image.
        if let Some(i) = input.get_untracked() {
            let _ = i.focus();
        }
    };

    // Glisser une image sur la zone : un cadre dit qu'elle sera prise. Le compteur absorbe les dragenter /
    // dragleave des éléments enfants.
    let carries_files = |e: &web_sys::DragEvent| {
        e.data_transfer().is_some_and(|dt| {
            let types = dt.types();
            (0..types.length()).any(|i| types.get(i).as_string().as_deref() == Some("Files"))
        })
    };
    let on_drag_enter = move |e: web_sys::DragEvent| {
        if !carries_files(&e) {
            return;
        }
        e.prevent_default();
        depth.update_value(|d| *d += 1);
        dragging.set(true);
    };
    let on_drag_over = move |e: web_sys::DragEvent| {
        if carries_files(&e) {
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
        if !carries_files(&e) {
            return;
        }
        e.prevent_default();
        error.set(None);
        let files = crate::files::files_of(e.data_transfer().and_then(|d| d.files()));
        let many = files.len() > 1;
        if let Some(file) = files.into_iter().next() {
            if many {
                error.set(Some("Une seule image à la fois : seul le premier fichier a été pris.".into()));
            }
            pick(file);
        }
    };

    let cropped =
        Callback::new(move |bytes: Vec<u8>| on_pick.run(IconFile { bytes, mime: "image/png", animated: false }));
    let crop_done = Callback::new(move |result: Option<Result<(), String>>| {
        crop.set(None);
        if let Some(Err(e)) = result {
            error.set(Some(e));
        }
    });
    let (file_id, hint_id, err_id) = (format!("{id}-file"), format!("{id}-hint"), format!("{id}-err"));
    let described = format!("{hint_id} {err_id}");
    view! {
        <div class="app-gicon-edit" data-over=move || dragging.get().then_some("")
            on:dragenter=on_drag_enter on:dragover=on_drag_over on:dragleave=on_drag_leave on:drop=on_drop>
            <div class="app-avatar-edit">
                {children()}
                <div class="app-avatar-side">
                    <div class="app-avatar-actions">
                        <input id=file_id.clone() class="k-sr app-file" type="file" accept="image/png,image/jpeg,image/webp,image/gif"
                            node_ref=input on:change=on_change aria-describedby=described
                            aria-invalid=move || error.get().is_some().then_some("true")/>
                        <label class="k-btn" for=file_id><Icon icon=lu::LuImagePlus/>"Choisir une image"</label>
                        <Show when=move || has_icon.get()>
                            <button type="button" class="k-btn k-btn--ghost app-link-danger" disabled=move || busy.get() on:click=remove>
                                "Retirer l'icône"
                            </button>
                        </Show>
                    </div>
                    <p id=hint_id class="k-hint">{format!("PNG, JPEG, WebP ou GIF. Une image fixe est recadrée en carré ; une animation reste telle quelle et pèse {} Mo au plus.", MAX_GUILD_ICON_BYTES / (1024 * 1024))}</p>
                    <Show when=move || error.get().is_some()>
                        <p id=err_id.clone() class="k-error" role="alert">{move || error.get()}</p>
                    </Show>
                </div>
            </div>
            <div class="app-gicon-drop" aria-hidden="true"><Icon icon=lu::LuUpload/>"Dépose l'image pour l'utiliser"</div>
        </div>
        // Montée hors de la page (et des réglages) : sinon `.k-settings main > *` rétrécit son fond sombre à la
        // largeur du contenu et laisse la navigation cliquable derrière.
        {move || crop.get().map(|url| view! { <Portal><CropDialog url=url.clone() which=Which::Icon done=crop_done cropped=cropped/></Portal> })}
    }
}
