//! Photo de profil et bannière : les blocs des réglages et la fenêtre de recadrage. Le recadrage se
//! fait dans le navigateur (canvas) et seule l'image recadrée est envoyée ; le serveur vérifie,
//! recadre et réduit de toute façon. Un GIF ou un WebP animé n'est pas recadré (un canvas ne
//! garde qu'une image) : il part tel quel, et s'affiche recadré au centre.

use kraken_core::image::{ImageKind, sniff};
use kraken_proto::{BANNER_HEIGHT, BANNER_WIDTH, MAX_AVATAR_BYTES, MAX_BANNER_BYTES};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};

use crate::store::{Store, store};
use crate::ui::{Avatar, banner_url, modal};

const MAX_ZOOM: f64 = 4.0;

/// Ce que l'on recadre : la photo (carrée) ou la bannière (3 pour 1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Avatar,
    Banner,
}

/// Géométrie du recadrage : rapport largeur / hauteur, taille de l'image envoyée, du cadre et
/// marge du canvas de travail autour de lui (ce qui sera rogné y reste visible).
struct Shape {
    ratio: f64,
    out: (u32, u32),
    frame: (f64, f64),
    margin: f64,
}

impl Shape {
    fn stage(&self) -> (f64, f64) {
        (self.frame.0 + 2.0 * self.margin, self.frame.1 + 2.0 * self.margin)
    }
}

impl Which {
    fn shape(self) -> Shape {
        match self {
            Self::Avatar => Shape { ratio: 1.0, out: (512, 512), frame: (256.0, 256.0), margin: 32.0 },
            Self::Banner => Shape {
                ratio: f64::from(BANNER_WIDTH) / f64::from(BANNER_HEIGHT),
                out: (BANNER_WIDTH, BANNER_HEIGHT),
                frame: (432.0, 144.0),
                margin: 24.0,
            },
        }
    }

    fn max_bytes(self) -> usize {
        match self {
            Self::Avatar => MAX_AVATAR_BYTES,
            Self::Banner => MAX_BANNER_BYTES,
        }
    }

    fn saved(self) -> &'static str {
        match self {
            Self::Avatar => "Photo enregistrée.",
            Self::Banner => "Bannière enregistrée.",
        }
    }

    fn removed(self) -> &'static str {
        match self {
            Self::Avatar => "Photo retirée.",
            Self::Banner => "Bannière retirée.",
        }
    }
}

fn not_image() -> String {
    "Ce fichier n'est pas une image lisible : choisis un PNG, un JPEG, un WebP ou un GIF.".into()
}

fn too_big(which: Which) -> String {
    format!("Cette image pèse trop lourd : {} Mo au plus.", which.max_bytes() / (1024 * 1024))
}

/// PNG, JPEG, WebP ou GIF, d'après les premiers octets du fichier.
fn looks_like_image(head: &[u8]) -> bool {
    head.starts_with(&[0x89, b'P', b'N', b'G'])
        || head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(b"GIF8")
        || (head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP")
}

/// Un fichier choisi : refusé s'il est trop lourd ou n'est pas une image ; une animation part
/// aussitôt telle quelle ; le reste s'ouvre dans la fenêtre de recadrage (`picked`).
async fn take_file(
    s: Store,
    file: web_sys::File,
    which: Which,
    picked: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    note: RwSignal<Option<&'static str>>,
    busy: RwSignal<bool>,
) {
    if file.size() > which.max_bytes() as f64 {
        error.set(Some(too_big(which)));
        return;
    }
    // Reconnaître l'image à ses premiers octets avant même de la charger.
    let head = match file.slice_with_i32_and_i32(0, 12) {
        Ok(b) => JsFuture::from(b.array_buffer()).await.ok(),
        Err(_) => None,
    };
    let head = head.map(|b| js_sys::Uint8Array::new(&b).to_vec()).unwrap_or_default();
    if !looks_like_image(&head) {
        error.set(Some(not_image()));
        return;
    }
    // GIF et WebP : animé ou non ? Un fichier animé n'est pas recadré, il part tel quel.
    if head.starts_with(b"GIF8") || head.get(8..12) == Some(b"WEBP") {
        let all = match JsFuture::from(file.array_buffer()).await {
            Ok(b) => js_sys::Uint8Array::new(&b).to_vec(),
            Err(_) => return error.set(Some(not_image())),
        };
        if let Some(found) = sniff(&all).filter(|f| f.animated && matches!(f.kind, ImageKind::Gif | ImageKind::Webp)) {
            busy.set(true);
            let sent = match which {
                Which::Avatar => crate::api::set_avatar(&all, found.kind.mime()).await,
                Which::Banner => crate::api::set_banner(&all, found.kind.mime()).await,
            };
            match sent {
                Ok(u) => {
                    s.update_user(u);
                    note.set(Some(which.saved()));
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
            return;
        }
    }
    match web_sys::Url::create_object_url_with_blob(&file) {
        Ok(url) => picked.set(Some(url)),
        Err(_) => error.set(Some(not_image())),
    }
}

/// Bloc « photo » du profil : l'avatar, changer, retirer.
#[component]
pub fn AvatarBlock() -> impl IntoView {
    let s = store();
    let input = NodeRef::<html::Input>::new();
    let picked = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let note = RwSignal::new(None::<&'static str>);
    let busy = RwSignal::new(false);
    let has_photo = Memo::new(move |_| s.me.with(|m| m.as_ref().is_some_and(|u| u.avatar.is_some())));
    let on_change = move |_| {
        error.set(None);
        note.set(None);
        let Some(input) = input.get() else { return };
        let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
        // Le même fichier, choisi deux fois de suite, doit redéclencher le changement.
        input.set_value("");
        spawn_local(take_file(s, file, Which::Avatar, picked, error, note, busy));
    };
    let remove = move |_| {
        busy.set(true);
        error.set(None);
        note.set(None);
        spawn_local(async move {
            match crate::api::remove_avatar().await {
                Ok(u) => {
                    s.update_user(u);
                    note.set(Some(Which::Avatar.removed()));
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    let done = Callback::new(move |result: Option<Result<(), String>>| {
        picked.set(None);
        match result {
            Some(Ok(())) => note.set(Some(Which::Avatar.saved())),
            Some(Err(e)) => error.set(Some(e)),
            None => {}
        }
    });
    view! {
        <div class="app-avatar-edit">
            {move || s.me.get().map(|u| view! { <Avatar user=u size="xl"/> })}
            <div class="app-avatar-side">
                {move || s.me.get().map(|u| view! {
                    <div><b class="text-strong">{u.name().to_string()}</b><div class="type-sm text-muted">{format!("@{}", u.username)}</div></div>
                })}
                <div class="app-avatar-actions">
                    <input id="avatar-file" class="k-sr app-file" type="file" accept="image/png,image/jpeg,image/webp,image/gif"
                        node_ref=input on:change=on_change aria-describedby="avatar-hint"/>
                    <label class="k-btn" for="avatar-file">"Changer la photo"</label>
                    <Show when=move || has_photo.get()>
                        <button type="button" class="k-btn k-btn--ghost app-link-danger" disabled=move || busy.get() on:click=remove>"Retirer"</button>
                    </Show>
                </div>
                <p id="avatar-hint" class="k-hint">{format!("PNG, JPEG, WebP ou GIF, {} Mo max. Un GIF ou un WebP animé reste animé (il n'est pas recadré).", MAX_AVATAR_BYTES / (1024 * 1024))}</p>
                <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                <Show when=move || note.get().is_some()><p class="k-hint" role="status">{move || note.get()}</p></Show>
            </div>
        </div>
        {move || picked.get().map(|url| view! { <CropDialog url=url which=Which::Avatar done=done/> })}
    }
}

/// Bloc « bannière » du profil : l'image large, changer, retirer.
#[component]
pub fn BannerBlock() -> impl IntoView {
    let s = store();
    let input = NodeRef::<html::Input>::new();
    let picked = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    let note = RwSignal::new(None::<&'static str>);
    let busy = RwSignal::new(false);
    let banner = Memo::new(move |_| s.me.with(|m| m.as_ref().and_then(|u| u.banner.clone())));
    let on_change = move |_| {
        error.set(None);
        note.set(None);
        let Some(input) = input.get() else { return };
        let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
        input.set_value("");
        spawn_local(take_file(s, file, Which::Banner, picked, error, note, busy));
    };
    let remove = move |_| {
        busy.set(true);
        error.set(None);
        note.set(None);
        spawn_local(async move {
            match crate::api::remove_banner().await {
                Ok(u) => {
                    s.update_user(u);
                    note.set(Some(Which::Banner.removed()));
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    let done = Callback::new(move |result: Option<Result<(), String>>| {
        picked.set(None);
        match result {
            Some(Ok(())) => note.set(Some(Which::Banner.saved())),
            Some(Err(e)) => error.set(Some(e)),
            None => {}
        }
    });
    view! {
        <div class="app-banner-edit">
            <div class="app-banner-box">
                {move || banner.get().map(|k| view! { <img class="app-banner-img" src=banner_url(&k) alt="Ta bannière actuelle"/> })}
                <Show when=move || banner.get().is_none()><span class="app-hint">"Pas de bannière"</span></Show>
            </div>
            <div class="app-avatar-actions">
                <input id="banner-file" class="k-sr app-file" type="file" accept="image/png,image/jpeg,image/webp,image/gif"
                    node_ref=input on:change=on_change aria-describedby="banner-hint"/>
                <label class="k-btn" for="banner-file">"Changer la bannière"</label>
                <Show when=move || banner.get().is_some()>
                    <button type="button" class="k-btn k-btn--ghost app-link-danger" disabled=move || busy.get() on:click=remove>"Retirer la bannière"</button>
                </Show>
            </div>
            <p id="banner-hint" class="k-hint">{format!("PNG, JPEG, WebP ou GIF, {} Mo max, de forme large (3 pour 1). Un GIF ou un WebP animé reste animé (il n'est pas recadré).", MAX_BANNER_BYTES / (1024 * 1024))}</p>
            <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
            <Show when=move || note.get().is_some()><p class="k-hint" role="status">{move || note.get()}</p></Show>
        </div>
        {move || picked.get().map(|url| view! { <CropDialog url=url which=Which::Banner done=done/> })}
    }
}

/// La partie de l'image qui sera gardée, de la forme `ratio` : centre (borné pour que le cadre
/// reste dans l'image) et dimensions, en pixels de l'image.
fn crop_rect(w: f64, h: f64, ratio: f64, zoom: f64, c: (f64, f64)) -> (f64, f64, f64, f64) {
    let (bw, bh) = if w / h > ratio { (h * ratio, h) } else { (w, w / ratio) };
    let (cw, ch) = (bw / zoom, bh / zoom);
    (c.0.clamp(cw / 2.0, w - cw / 2.0), c.1.clamp(ch / 2.0, h - ch / 2.0), cw, ch)
}

fn context(c: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    c.get_context("2d").ok().flatten()?.dyn_into().ok()
}

/// Dessine la zone choisie, remplissant tout le canvas (aperçus et image envoyée).
fn draw_crop(c: &HtmlCanvasElement, img: &HtmlImageElement, (cx, cy, cw, ch): (f64, f64, f64, f64)) {
    let Some(ctx) = context(c) else { return };
    let (ow, oh) = (f64::from(c.width()), f64::from(c.height()));
    ctx.clear_rect(0.0, 0.0, ow, oh);
    let _ = ctx.draw_image_with_html_image_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        img,
        cx - cw / 2.0,
        cy - ch / 2.0,
        cw,
        ch,
        0.0,
        0.0,
        ow,
        oh,
    );
}

/// L'image recadrée en PNG.
async fn export(img: &HtmlImageElement, crop: (f64, f64, f64, f64), out: (u32, u32)) -> Option<Vec<u8>> {
    let canvas: HtmlCanvasElement = document().create_element("canvas").ok()?.dyn_into().ok()?;
    canvas.set_width(out.0);
    canvas.set_height(out.1);
    draw_crop(&canvas, img, crop);
    let (tx, rx) = futures::channel::oneshot::channel::<Option<web_sys::Blob>>();
    let cb = Closure::once_into_js(move |b: Option<web_sys::Blob>| {
        let _ = tx.send(b);
    });
    canvas.to_blob_with_type(cb.unchecked_ref(), "image/png").ok()?;
    let blob = rx.await.ok()??;
    let buf = JsFuture::from(blob.array_buffer()).await.ok()?;
    Some(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Recadrer : glisser ou flèches pour déplacer, curseur ou + et − pour zoomer.
#[component]
fn CropDialog(url: String, which: Which, done: Callback<Option<Result<(), String>>>) -> impl IntoView {
    let s = store();
    let shape = which.shape();
    let (frame_w, margin, ratio, out) = (shape.frame.0, shape.margin, shape.ratio, shape.out);
    let (stage_w, stage_h) = shape.stage();
    let root = NodeRef::<html::Div>::new();
    modal(root, move || {
        done.run(None);
    });
    let img = StoredValue::new_local(HtmlImageElement::new().expect("img"));
    let ready = RwSignal::new(false);
    let size = RwSignal::new((1.0_f64, 1.0_f64));
    let zoom = RwSignal::new(1.0_f64);
    let center = RwSignal::new((0.0_f64, 0.0_f64));
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let stage = NodeRef::<html::Canvas>::new();
    let (big, mid, small, wide) = (
        NodeRef::<html::Canvas>::new(),
        NodeRef::<html::Canvas>::new(),
        NodeRef::<html::Canvas>::new(),
        NodeRef::<html::Canvas>::new(),
    );

    {
        let el = img.get_value();
        let loaded = el.clone();
        let onload = Closure::once_into_js(move || {
            let (w, h) = (f64::from(loaded.natural_width()), f64::from(loaded.natural_height()));
            if w < 1.0 || h < 1.0 {
                done.run(Some(Err(not_image())));
                return;
            }
            size.set((w, h));
            center.set((w / 2.0, h / 2.0));
            ready.set(true);
        });
        let onerror = Closure::once_into_js(move || {
            done.run(Some(Err(not_image())));
        });
        el.set_onload(Some(onload.unchecked_ref()));
        el.set_onerror(Some(onerror.unchecked_ref()));
        el.set_src(&url);
        // L'adresse temporaire ne sert qu'à charger l'image : libérée à la fermeture.
        on_cleanup(move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        });
    }

    let crop = move || {
        let (w, h) = size.get();
        crop_rect(w, h, ratio, zoom.get(), center.get())
    };
    Effect::new(move || {
        if !ready.get() {
            return;
        }
        let c = crop();
        let img = img.get_value();
        if let Some(ctx) = stage.get().as_ref().and_then(context) {
            ctx.clear_rect(0.0, 0.0, stage_w, stage_h);
            let k = frame_w / c.2;
            let (w, h) = size.get_untracked();
            let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                &img,
                margin - (c.0 - c.2 / 2.0) * k,
                margin - (c.1 - c.3 / 2.0) * k,
                w * k,
                h * k,
            );
        }
        for p in [big, mid, small, wide] {
            if let Some(p) = p.get() {
                draw_crop(&p, &img, c);
            }
        }
    });

    // Déplace l'image de `dx`, `dy` pixels du canvas de travail.
    let pan = move |dx: f64, dy: f64| {
        let (cx, cy, cw, _) = crop();
        let k = frame_w / cw;
        center.set((cx - dx / k, cy - dy / k));
    };
    let set_zoom = move |z: f64| zoom.set(z.clamp(1.0, MAX_ZOOM));
    let drag = StoredValue::new(None::<(f64, f64)>);
    let on_down = move |e: leptos::ev::PointerEvent| {
        e.prevent_default();
        if let Some(t) = e.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            let _ = t.set_pointer_capture(e.pointer_id());
        }
        drag.set_value(Some((f64::from(e.client_x()), f64::from(e.client_y()))));
    };
    let on_move = move |e: leptos::ev::PointerEvent| {
        let Some((px, py)) = drag.get_value() else { return };
        let (x, y) = (f64::from(e.client_x()), f64::from(e.client_y()));
        drag.set_value(Some((x, y)));
        // Pixels de l'écran → pixels du canvas (affiché à la largeur de son cadre).
        let shown = stage.get_untracked().map_or(stage_w, |c| f64::from(c.client_width().max(1)));
        let k = stage_w / shown;
        pan((x - px) * k, (y - py) * k);
    };
    let on_up = move |_| drag.set_value(None);
    let on_key = move |e: leptos::ev::KeyboardEvent| {
        let step = 16.0;
        match e.key().as_str() {
            "ArrowLeft" => pan(-step, 0.0),
            "ArrowRight" => pan(step, 0.0),
            "ArrowUp" => pan(0.0, -step),
            "ArrowDown" => pan(0.0, step),
            "+" | "=" => set_zoom(zoom.get_untracked() + 0.1),
            "-" | "_" => set_zoom(zoom.get_untracked() - 0.1),
            _ => return,
        }
        e.prevent_default();
    };
    let apply = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if !ready.get_untracked() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let c = untrack(crop);
        let img = img.get_value();
        spawn_local(async move {
            let Some(bytes) = export(&img, c, out).await else {
                busy.try_set(false);
                error.try_set(Some("Impossible de préparer l'image. Essaie-en une autre.".into()));
                return;
            };
            let sent = match which {
                Which::Avatar => crate::api::set_avatar(&bytes, "image/png").await,
                Which::Banner => crate::api::set_banner(&bytes, "image/png").await,
            };
            match sent {
                Ok(u) => {
                    s.update_user(u);
                    done.run(Some(Ok(())));
                }
                Err(e) => {
                    busy.try_set(false);
                    error.try_set(Some(e.message));
                }
            }
        });
    };
    let (title, title_id) = match which {
        Which::Avatar => ("Recadrer ta photo", "crop-t"),
        Which::Banner => ("Recadrer ta bannière", "crop-banner-t"),
    };
    let previews = match which {
        Which::Avatar => view! {
            <div class="app-crop-previews" aria-hidden="true">
                <figure><canvas node_ref=big class="k-avatar k-avatar--xl" width="240" height="240"></canvas><figcaption class="k-hint">"Grand"</figcaption></figure>
                <figure><canvas node_ref=mid class="k-avatar k-avatar--md" width="64" height="64"></canvas><figcaption class="k-hint">"Moyen"</figcaption></figure>
                <figure><canvas node_ref=small class="k-avatar k-avatar--sm" width="48" height="48"></canvas><figcaption class="k-hint">"Petit"</figcaption></figure>
            </div>
        }
        .into_any(),
        Which::Banner => view! {
            <div class="app-crop-previews app-crop-previews--wide" aria-hidden="true">
                <figure><canvas node_ref=wide class="app-banner-preview" width="480" height="160"></canvas><figcaption class="k-hint">"Aperçu"</figcaption></figure>
            </div>
        }
        .into_any(),
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog app-dialog-wide" role="dialog" aria-modal="true" aria-labelledby=title_id>
                <header><h2 id=title_id>{title}</h2></header>
                <form on:submit=apply>
                    <div>
                        <div class="app-crop" data-wide=(which == Which::Banner).then_some("")>
                            <div class="app-crop-main">
                                <div class="app-crop-stage" aria-busy=move || (!ready.get()).to_string()
                                    style=format!("aspect-ratio: {} / {}", stage_w, stage_h)>
                                    <canvas node_ref=stage width=stage_w.to_string() height=stage_h.to_string() tabindex="0"
                                        role="application" aria-label="Zone de recadrage : les flèches déplacent l'image, + et − zooment."
                                        on:pointerdown=on_down on:pointermove=on_move on:pointerup=on_up on:pointercancel=on_up
                                        on:keydown=on_key></canvas>
                                    <i class="app-crop-frame" aria-hidden="true"
                                        style=format!("inset: {}% {}%", margin / stage_h * 100.0, margin / stage_w * 100.0)></i>
                                </div>
                                <label class="k-field">
                                    <span class="k-label">"Zoom"</span>
                                    <input class="app-range" type="range" min="1" max=MAX_ZOOM.to_string() step="0.01"
                                        prop:value=move || zoom.get().to_string()
                                        on:input=move |e| set_zoom(event_target_value(&e).parse().unwrap_or(1.0))/>
                                </label>
                                <p class="k-hint">"Glisse l'image pour la placer, ou utilise les flèches du clavier. Le cadre montre ce qui sera gardé."</p>
                            </div>
                            {previews}
                        </div>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| { done.run(None); } autofocus>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--primary" disabled=move || !ready.get() || busy.get()
                            aria-busy=move || busy.get().to_string()>"Appliquer"</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}
