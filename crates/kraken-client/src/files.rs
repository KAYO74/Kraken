//! Fichiers joints et aperçus de liens dans un salon : plateau des fichiers en attente au-dessus
//! de la saisie (envoi avec progression), calque de dépôt, fichiers et images dans les messages,
//! visionneuse d'images, cartes d'aperçu de lien.

use kraken_proto::{Attachment, Embed, Id, MAX_ATTACHMENTS, Message};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api;
use crate::icons::{Icon, lu};
use crate::store::store;
use crate::ui::modal;

/// Où en est un fichier du plateau.
#[derive(Clone)]
pub enum FileState {
    Queued,
    Uploading,
    Done(Attachment),
    Failed(String),
}

/// Un fichier choisi, déposé ou collé, en attente d'être joint au prochain message.
#[derive(Clone)]
pub struct PendingFile {
    pub key: u32,
    pub file: web_sys::File,
    pub name: String,
    pub size: u64,
    /// Image que le navigateur sait montrer : une vignette locale.
    pub preview: Option<String>,
    pub state: FileState,
    pub progress: f64,
    pub alt: String,
    pub upload: Option<api::Upload>,
}

/// Le plateau d'un salon.
pub type Tray = RwSignal<Vec<PendingFile>, LocalStorage>;

pub fn new_tray() -> Tray {
    RwSignal::new_local(Vec::new())
}

thread_local! {
    static NEXT_KEY: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

/// « 850 o », « 12,4 Ko », « 1,2 Mo ».
pub fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["o", "Ko", "Mo", "Go"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} o") } else { format!("{v:.1} {}", UNITS[u]).replace('.', ",") }
}

fn is_previewable(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/webp" | "image/gif")
}

/// Ajoute des fichiers au plateau (au plus [`MAX_ATTACHMENTS`], chacun sous la limite du serveur)
/// et lance l'envoi. Les refus sont dits dans `notice`.
pub fn add_files(tray: Tray, channel: Id, files: Vec<web_sys::File>, notice: RwSignal<Option<String>>) {
    if files.is_empty() {
        return;
    }
    spawn_local(async move {
        let max = api::max_upload_bytes().await;
        let mut refused = Vec::new();
        for file in files {
            let size = file.size() as u64;
            let name = file.name();
            if tray.with_untracked(Vec::len) >= MAX_ATTACHMENTS {
                notice.set(Some(format!("{MAX_ATTACHMENTS} fichiers au plus par message.")));
                break;
            }
            if size == 0 {
                refused.push(format!("« {name} » est vide."));
                continue;
            }
            if size > max {
                refused.push(format!("« {name} » pèse {} : {} au plus.", size_label(size), size_label(max)));
                continue;
            }
            let preview =
                is_previewable(&file.type_()).then(|| web_sys::Url::create_object_url_with_blob(&file).ok()).flatten();
            let key = NEXT_KEY.with(|k| k.replace(k.get() + 1));
            tray.update(|v| {
                v.push(PendingFile {
                    key,
                    file,
                    name,
                    size,
                    preview,
                    state: FileState::Queued,
                    progress: 0.0,
                    alt: String::new(),
                    upload: None,
                })
            });
        }
        if !refused.is_empty() {
            notice.set(Some(refused.join(" ")));
        }
        pump(tray, channel);
    });
}

fn with_file(tray: Tray, key: u32, f: impl FnOnce(&mut PendingFile)) {
    tray.try_update(|v| {
        if let Some(p) = v.iter_mut().find(|p| p.key == key) {
            f(p)
        }
    });
}

/// Envoie le prochain fichier en attente, un à la fois : les fichiers gardent l'ordre du plateau.
fn pump(tray: Tray, channel: Id) {
    let Some((busy, next)) = tray.try_with_untracked(|v| {
        (
            v.iter().any(|f| matches!(f.state, FileState::Uploading)),
            v.iter().find(|f| matches!(f.state, FileState::Queued)).map(|f| (f.key, f.file.clone())),
        )
    }) else {
        return;
    };
    let Some((key, file)) = next.filter(|_| !busy) else { return };
    with_file(tray, key, |p| p.state = FileState::Uploading);
    match api::upload(channel, &file, move |frac| with_file(tray, key, |p| p.progress = frac)) {
        Ok((handle, done)) => {
            with_file(tray, key, |p| p.upload = Some(handle));
            spawn_local(async move {
                let result = done.await;
                // Retiré pendant l'envoi : le fichier arrivé n'a plus de raison d'être.
                let still_there = tray.try_with_untracked(|v| v.iter().any(|p| p.key == key)).unwrap_or(false);
                match result {
                    Ok(a) if !still_there => {
                        let _ = api::delete_attachment(channel, a.id).await;
                    }
                    Ok(a) => with_file(tray, key, |p| {
                        p.state = FileState::Done(a);
                        p.progress = 1.0;
                        p.upload = None;
                    }),
                    Err(e) => with_file(tray, key, |p| {
                        p.state = FileState::Failed(e.message);
                        p.upload = None;
                    }),
                }
                pump(tray, channel);
            });
        }
        Err(e) => {
            with_file(tray, key, |p| p.state = FileState::Failed(e.message));
            pump(tray, channel);
        }
    }
}

/// Retire un fichier du plateau : interrompt son envoi, ou le supprime du serveur s'il est arrivé.
pub fn remove_file(tray: Tray, channel: Id, key: u32) {
    let Some(p) = tray.try_with_untracked(|v| v.iter().find(|p| p.key == key).cloned()).flatten() else { return };
    tray.update(|v| v.retain(|x| x.key != key));
    if let Some(u) = &p.upload {
        u.abort();
    }
    if let Some(url) = &p.preview {
        let _ = web_sys::Url::revoke_object_url(url);
    }
    if let FileState::Done(a) = p.state {
        spawn_local(async move {
            let _ = api::delete_attachment(channel, a.id).await;
        });
    }
}

/// Vide le plateau (les fichiers sont partis avec le message) et rend ce qu'il portait.
pub fn take_done(tray: Tray) -> Vec<Attachment> {
    let all = tray.get_untracked();
    for p in &all {
        if let Some(url) = &p.preview {
            let _ = web_sys::Url::revoke_object_url(url);
        }
    }
    tray.set(Vec::new());
    all.into_iter()
        .filter_map(|p| match p.state {
            FileState::Done(mut a) => {
                a.alt = Some(p.alt.trim().to_string()).filter(|t| !t.is_empty());
                Some(a)
            }
            _ => None,
        })
        .collect()
}

/// Quitter le salon : envois interrompus, vignettes libérées.
pub fn dispose_tray(tray: Tray) {
    for p in tray.try_get_untracked().unwrap_or_default() {
        if let Some(u) = &p.upload {
            u.abort();
        }
        if let Some(url) = &p.preview {
            let _ = web_sys::Url::revoke_object_url(url);
        }
    }
}

/// Où en est le plateau : `(en cours, en échec, prêts)`.
pub fn tray_status(tray: Tray) -> (usize, usize, usize) {
    tray.with(|v| {
        let busy = v.iter().filter(|p| matches!(p.state, FileState::Queued | FileState::Uploading)).count();
        let failed = v.iter().filter(|p| matches!(p.state, FileState::Failed(_))).count();
        (busy, failed, v.len() - busy - failed)
    })
}

/// Les fichiers d'un événement de dépôt ou de collage.
pub fn files_of(list: Option<web_sys::FileList>) -> Vec<web_sys::File> {
    let Some(list) = list else { return vec![] };
    (0..list.length()).filter_map(|i| list.item(i)).collect()
}

/// Le plateau au-dessus de la saisie.
#[component]
pub fn TrayView(tray: Tray, channel: Id) -> impl IntoView {
    let keys = Memo::new(move |_| tray.with(|v| v.iter().map(|p| p.key).collect::<Vec<_>>()));
    view! {
        <Show when=move || !keys.with(Vec::is_empty)>
            <ul class="app-pending" aria-label="Fichiers à joindre">
                <For each=move || keys.get() key=|k| *k let:key>
                    <PendingTile tray=tray channel=channel key=key/>
                </For>
            </ul>
        </Show>
    }
}

#[component]
fn PendingTile(tray: Tray, channel: Id, key: u32) -> impl IntoView {
    let item = move || tray.with(|v| v.iter().find(|p| p.key == key).cloned());
    let name = move || item().map(|p| p.name).unwrap_or_default();
    let progress = move || item().map(|p| p.progress).unwrap_or_default();
    let uploading = move || item().is_some_and(|p| matches!(p.state, FileState::Uploading | FileState::Queued));
    let failed = move || item().and_then(|p| if let FileState::Failed(e) = p.state { Some(e) } else { None });
    let preview = move || item().and_then(|p| p.preview);
    let alt = RwSignal::new(String::new());
    Effect::new(move || {
        let a = alt.get();
        with_file(tray, key, |p| p.alt = a);
    });
    view! {
        <li class="app-pending-item">
            <div class="app-tile" data-failed=move || failed().map(|_| "")>
                {move || match preview() {
                    Some(src) => view! { <img src=src alt=""/> }.into_any(),
                    None => view! {
                        <div class="app-tile-file">
                            <Icon icon=lu::LuFile/>
                            <span class="app-tile-name">{name()}</span>
                            <small>{move || item().map(|p| size_label(p.size)).unwrap_or_default()}</small>
                        </div>
                    }.into_any(),
                }}
                <button type="button" class="k-btn k-btn--icon app-tile-x" aria-label=move || format!("Retirer {}", name())
                    title="Retirer" on:click=move |_| remove_file(tray, channel, key)>
                    <Icon icon=lu::LuX small=true/>
                </button>
                <Show when=uploading>
                    <div class="app-tile-bar" role="progressbar" aria-label=move || format!("Envoi de {}", name())
                        aria-valuemin="0" aria-valuemax="100" aria-valuenow=move || (progress() * 100.0).round().to_string()>
                        <span style=move || format!("width: {}%", (progress() * 100.0).round())></span>
                    </div>
                </Show>
                {move || failed().map(|e| { let t = e.clone(); view! { <p class="app-tile-error" role="alert" title=t>{e}</p> } })}
            </div>
            <Show when=move || preview().is_some()>
                <input type="text" class="k-input app-alt" maxlength="1024" placeholder="Description"
                    aria-label=move || format!("Texte alternatif de {}", name()) bind:value=alt/>
            </Show>
        </li>
    }
}

/// Le calque « Dépose pour joindre… » par-dessus le fil.
#[component]
pub fn DropOverlay(dragging: RwSignal<bool>, place: Signal<String>) -> impl IntoView {
    view! {
        <Show when=move || dragging.get()>
            <div class="app-drop" aria-hidden="true">
                <Icon icon=lu::LuUpload/>
                <p>{move || format!("Dépose pour joindre à {}", place.get())}</p>
            </div>
        </Show>
    }
}

// ───────── dans les messages ─────────

/// Une image à la visionneuse : son message et son rang.
#[derive(Clone, PartialEq)]
pub struct ViewerState {
    pub channel: Id,
    pub images: Vec<Attachment>,
    pub index: usize,
}

/// Les fichiers d'un message : vignettes d'images (qui ouvrent la visionneuse), cartes pour le reste.
#[component]
pub fn Attachments(channel: Id, files: Vec<Attachment>, viewer: RwSignal<Option<ViewerState>>) -> impl IntoView {
    let (images, others): (Vec<Attachment>, Vec<Attachment>) = files.into_iter().partition(|a| a.width.is_some());
    let multi = images.len() > 1;
    let thumbs = images.iter().cloned().enumerate().map(|(i, a)| {
        let all = images.clone();
        let label = format!("Ouvrir l'image {}", a.filename);
        let alt = a.alt.clone().unwrap_or_else(|| a.filename.clone());
        let src = format!("{}/preview", api::attachment_url(channel, a.id));
        view! {
            <button type="button" class="app-thumb" aria-label=label
                on:click=move |_| viewer.set(Some(ViewerState { channel, images: all.clone(), index: i }))>
                <img src=src alt=alt width=a.width.unwrap_or_default() height=a.height.unwrap_or_default() loading="lazy" decoding="async"/>
            </button>
        }
    }).collect_view();
    let cards = others.into_iter().map(|a| {
        let url = api::attachment_url(channel, a.id);
        let download = format!("{url}?download=1");
        let label = format!("Télécharger {}", a.filename);
        view! {
            <div class="k-attachment">
                <span class="k-attachment__icon"><Icon icon=lu::LuFile/></span>
                <div>
                    <a href=download.clone() download=a.filename.clone()>{a.filename.clone()}</a>
                    <div class="type-xs text-muted">{size_label(a.size)}</div>
                </div>
                <a class="k-btn k-btn--sm" href=download download=a.filename.clone() aria-label=label>"Télécharger"</a>
            </div>
        }
    }).collect_view();
    view! {
        {(!images.is_empty()).then(|| view! { <div class="app-media" data-multi=multi.then_some("")>{thumbs}</div> })}
        {cards}
    }
}

/// Visionneuse : l'image du message en grand, ← → pour les autres images du message, Échap ferme.
#[component]
pub fn Viewer(state: ViewerState, viewer: RwSignal<Option<ViewerState>>) -> impl IntoView {
    let root = NodeRef::<html::Div>::new();
    modal(root, move || viewer.set(None));
    let index = RwSignal::new(state.index);
    let images = StoredValue::new(state.images);
    let channel = state.channel;
    let count = images.with_value(Vec::len);
    let current = move || images.with_value(|v| v.get(index.get()).cloned());
    let step = move |d: isize| {
        let n = index.get_untracked() as isize + d;
        if (0..count as isize).contains(&n) {
            index.set(n as usize);
        }
    };
    let keys = window_event_listener(leptos::ev::keydown, move |e| match e.key().as_str() {
        "ArrowLeft" => {
            e.prevent_default();
            step(-1)
        }
        "ArrowRight" => {
            e.prevent_default();
            step(1)
        }
        _ => {}
    });
    on_cleanup(move || keys.remove());
    let url = move || current().map(|a| api::attachment_url(channel, a.id)).unwrap_or_default();
    let on_backdrop = move |e: leptos::ev::MouseEvent| {
        if e.target() == e.current_target() {
            viewer.set(None);
        }
    };
    view! {
        <div class="k-scrim app-viewer-scrim" node_ref=root on:click=on_backdrop>
            <div class="k-dialog app-viewer" role="dialog" aria-modal="true" aria-label="Visionneuse d'images">
                <header class="app-viewer-bar">
                    <div class="app-viewer-name">
                        <b class="app-ellipsis">{move || current().map(|a| a.filename).unwrap_or_default()}</b>
                        <small>{move || {
                            let size = current().map(|a| size_label(a.size)).unwrap_or_default();
                            if count > 1 { format!("{size} · {} sur {count}", index.get() + 1) } else { size }
                        }}</small>
                    </div>
                    <a class="k-btn k-btn--ghost k-btn--sm" href=url target="_blank" rel="noopener noreferrer"
                        aria-label="Ouvrir l'original">
                        <Icon icon=lu::LuExternalLink small=true/><span>"Ouvrir l'original"</span>
                    </a>
                    <a class="k-btn k-btn--ghost k-btn--sm" href=move || format!("{}?download=1", url())
                        download=move || current().map(|a| a.filename).unwrap_or_default() aria-label="Télécharger">
                        <Icon icon=lu::LuDownload small=true/><span>"Télécharger"</span>
                    </a>
                    <button type="button" class="k-btn k-btn--ghost k-btn--sm" autofocus aria-label="Fermer"
                        on:click=move |_| viewer.set(None)>
                        <Icon icon=lu::LuX small=true/><span>"Fermer"</span>
                    </button>
                </header>
                <div class="app-viewer-stage">
                    <button type="button" class="k-btn k-btn--icon app-viewer-nav" aria-label="Image précédente" title="Précédente (←)"
                        disabled=move || index.get() == 0 hidden={count < 2} on:click=move |_| step(-1)>
                        <Icon icon=lu::LuChevronLeft/>
                    </button>
                    <img src=move || format!("{}/preview", url())
                        alt=move || current().map(|a| a.alt.unwrap_or(a.filename)).unwrap_or_default()/>
                    <button type="button" class="k-btn k-btn--icon app-viewer-nav" aria-label="Image suivante" title="Suivante (→)"
                        disabled=move || { index.get() + 1 >= count } hidden={count < 2} on:click=move |_| step(1)>
                        <Icon icon=lu::LuChevronRight/>
                    </button>
                </div>
            </div>
        </div>
    }
}

// ───────── aperçus de liens ─────────

/// Une image assez large va au-dessus du texte, une petite à droite.
fn wide(e: &Embed) -> bool {
    e.image.as_ref().is_some_and(|i| i.width >= 300 && i.height >= 150)
}

#[component]
pub fn EmbedCard(
    channel: Id,
    message: Id,
    embed: Embed,
    mine: bool,
    notice: RwSignal<Option<String>>,
) -> impl IntoView {
    let s = store();
    let host = embed.site_name.clone().or_else(|| {
        let rest = embed.url.split_once("://")?.1;
        Some(rest.split(['/', '?', '#']).next()?.trim_start_matches("www.").to_string())
    });
    let class = match (&embed.image, wide(&embed)) {
        (None, _) => "k-embed",
        (Some(_), true) => "k-embed k-embed--wide",
        (Some(_), false) => "k-embed k-embed--thumb",
    };
    let image = embed.image.as_ref().map(|i| api::embed_image_url(channel, &i.key));
    let favicon = embed.favicon.as_ref().map(|k| api::embed_image_url(channel, k));
    let is_wide = wide(&embed);
    let url = embed.url.clone();
    let remove = move |_| {
        let url = url.clone();
        spawn_local(async move {
            match api::remove_embed(channel, message, &url).await {
                Ok(m) => s.log(channel, |l| l.update(m)),
                Err(e) => notice.set(Some(e.message)),
            }
        });
    };
    let title = embed.title.clone().unwrap_or_else(|| embed.url.clone());
    let label = format!("Aperçu de {}", host.clone().unwrap_or_default());
    let img = move || image.clone().map(|src| view! { <img src=src alt="" loading="lazy" decoding="async"/> });
    view! {
        <article class=class aria-label=label>
            {is_wide.then(img.clone())}
            <small>
                {favicon.map(|src| view! { <img src=src alt="" width="16" height="16"/> })}
                <span class="app-ellipsis">{host}</span>
            </small>
            <a href=embed.url.clone() target="_blank" rel="noopener noreferrer nofollow ugc">{title}</a>
            {embed.description.clone().map(|d| view! { <p>{d}</p> })}
            {(!is_wide).then(img)}
            {mine.then(|| view! {
                <button type="button" class="k-btn k-btn--icon app-embed-x" aria-label="Retirer l'aperçu" title="Retirer l'aperçu"
                    on:click=remove>
                    <Icon icon=lu::LuX small=true/>
                </button>
            })}
        </article>
    }
}

/// Fichiers et aperçus d'un message, sous son texte.
#[component]
pub fn MessageMedia(
    msg: Message,
    mine: bool,
    viewer: RwSignal<Option<ViewerState>>,
    notice: RwSignal<Option<String>>,
) -> impl IntoView {
    let (cid, mid) = (msg.channel_id, msg.id);
    view! {
        {(!msg.attachments.is_empty()).then(|| view! { <Attachments channel=cid files=msg.attachments.clone() viewer=viewer/> })}
        {msg.embeds.iter().cloned().map(|e| view! { <EmbedCard channel=cid message=mid embed=e mine=mine notice=notice/> }).collect_view()}
    }
}
