//! Photos de profil et bannières. Un GIF ou un WebP animé reste animé sur le serveur (le fichier
//! stocké n'est jamais touché) ; c'est ici qu'on décide de ce qu'on en montre : une image fixe, copie
//! de sa première image, dans les listes (membres, messages, rail), l'image animée dans la carte de
//! profil, et au survol ou au focus d'une ligne. En mouvement réduit elle reste fixe partout, sauf
//! sur demande (« Lire l'animation » sur la carte de profil). Le choix entre les deux est fait par
//! les feuilles de style (`--live` et `--still`, voir `style/app.css`) ; ce composant fournit les
//! deux images.
//!
//! Les images qui ne sont pas animées n'ont qu'un `<img>`, comme avant. Pour le savoir, la première
//! fois qu'une image est chargée on relit ses octets (le cache du navigateur les a, la clé de
//! l'image ne change jamais) et on regarde s'ils sont animés avec le même `sniff` que le serveur.

use std::cell::RefCell;
use std::collections::HashMap;

use kraken_core::image::{ImageKind, sniff};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

/// Ce qu'on sait d'une image.
#[derive(Clone, PartialEq)]
enum Still {
    /// Pas encore regardée.
    Unknown,
    /// Pas animée : une seule image suffit.
    Plain,
    /// Animée : l'adresse de données de sa première image.
    Frozen(String),
}

thread_local! {
    /// Ce qu'on sait déjà de chaque adresse d'image, pour ne la regarder qu'une fois.
    static SEEN: RefCell<HashMap<String, Still>> = RefCell::new(HashMap::new());
    /// Les images qui attendent la réponse d'une lecture en cours (la même photo revient sur chaque message).
    static WAITING: RefCell<HashMap<String, Vec<RwSignal<Still>>>> = RefCell::new(HashMap::new());
}

/// La première image d'une image animée, en PNG, réduite à `cap` pixels de côté au plus.
pub(crate) async fn first_frame(bytes: &[u8], cap: u32) -> Option<String> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::of1(&array);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).ok()?;
    let window = web_sys::window()?;
    let bitmap: web_sys::ImageBitmap =
        JsFuture::from(window.create_image_bitmap_with_blob(&blob).ok()?).await.ok()?.dyn_into().ok()?;
    let (w, h) = (bitmap.width().max(1), bitmap.height().max(1));
    let scale = (f64::from(cap) / f64::from(w.max(h))).min(1.0);
    let (cw, ch) = ((f64::from(w) * scale).round().max(1.0) as u32, (f64::from(h) * scale).round().max(1.0) as u32);
    let canvas: HtmlCanvasElement = window.document()?.create_element("canvas").ok()?.dyn_into().ok()?;
    canvas.set_width(cw);
    canvas.set_height(ch);
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d").ok()??.dyn_into().ok()?;
    ctx.draw_image_with_image_bitmap_and_dw_and_dh(&bitmap, 0.0, 0.0, f64::from(cw), f64::from(ch)).ok()?;
    bitmap.close();
    canvas.to_data_url_with_type("image/png").ok()
}

/// Regarde l'image à cette adresse : animée ou non, et sa première image si elle l'est.
async fn look(src: &str, cap: u32) -> Still {
    let Ok(resp) = gloo_net::http::Request::get(src).send().await else { return Still::Plain };
    if !resp.ok() {
        return Still::Plain;
    }
    let Ok(bytes) = resp.binary().await else { return Still::Plain };
    if sniff(&bytes).is_none_or(|f| !f.animated || !matches!(f.kind, ImageKind::Gif | ImageKind::Webp)) {
        return Still::Plain;
    }
    first_frame(&bytes, cap).await.map_or(Still::Plain, Still::Frozen)
}

/// Demande ce qu'on sait de cette image à `state` ; la première à la demander lance la lecture, les
/// autres attendent sa réponse.
fn want(src: &str, cap: u32, state: RwSignal<Still>) {
    if let Some(known) = SEEN.with_borrow(|m| m.get(src).cloned()) {
        state.try_set(known);
        return;
    }
    let first = WAITING.with_borrow_mut(|w| {
        let list = w.entry(src.to_string()).or_default();
        list.push(state);
        list.len() == 1
    });
    if first {
        let src = src.to_string();
        spawn_local(async move {
            let found = look(&src, cap).await;
            SEEN.with_borrow_mut(|m| m.insert(src.clone(), found.clone()));
            for waiting in WAITING.with_borrow_mut(|w| w.remove(&src)).unwrap_or_default() {
                waiting.try_set(found.clone());
            }
        });
    }
}

/// Une image de profil. `src` : son adresse ; `class` : la classe de l'image ; `cap` : le côté, en
/// pixels, de sa copie fixe ; `on_error` : appelé si elle ne se charge pas.
#[component]
pub fn Photo(
    src: String,
    #[prop(optional)] class: &'static str,
    #[prop(optional, into)] alt: String,
    cap: u32,
    #[prop(optional, into)] on_error: Option<Callback<()>>,
) -> impl IntoView {
    let state = RwSignal::new(SEEN.with_borrow(|m| m.get(&src).cloned()).unwrap_or(Still::Unknown));
    let img = NodeRef::<html::Img>::new();
    let seen = {
        let src = src.clone();
        move || {
            if state.get_untracked() == Still::Unknown {
                want(&src, cap, state);
            }
        }
    };
    // Une image déjà chargée (cache) peut l'être avant que l'écouteur ne soit posé.
    Effect::new({
        let seen = seen.clone();
        move || {
            if let Some(i) = img.get()
                && i.complete()
                && i.natural_width() > 0
            {
                seen();
            }
        }
    });
    let frozen = move || matches!(state.get(), Still::Frozen(_));
    let fail = move |_| {
        if let Some(f) = on_error {
            f.run(());
        }
    };
    let alt2 = alt.clone();
    let fixed = move || match state.get() {
        Still::Frozen(url) => {
            Some(view! { <img class=format!("{class} k-still") src=url alt=alt2.clone() on:error=fail/> })
        }
        _ => None,
    };
    view! {
        {fixed}
        <img node_ref=img class=move || if frozen() { format!("{class} k-live") } else { class.to_string() } src=src alt=alt
            loading="lazy" decoding="async" data-probing=move || (state.get() == Still::Unknown).then_some("")
            on:load=move |_| seen() on:error=fail/>
    }
}
