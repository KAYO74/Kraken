//! Salon vocal (S18) : avant d'entrer, qui est là et « Rejoindre » ; ensuite les tuiles des
//! participants (leur caméra à la place de l'avatar quand elle est allumée), l'écran regardé en
//! grand et la barre de commandes. Plus ce que la coquille montre
//! de l'appel : les participants sous chaque salon vocal, le panneau « En appel » (avec les
//! boutons micro et son), le son des micros reçus et le contrôle à distance de mon écran.
//!
//! Appels des MP et des groupes (S19) : la même session vocale, la conversation servant de salon
//! (`guild` à `None`). L'appel s'affiche en haut de la conversation ; un appel qui sonne chez moi
//! s'affiche partout, avec « Rejoindre » et « Refuser » (et une sonnerie synthétisée).

use std::cell::RefCell;
use std::rc::Rc;

use kraken_proto::{
    Channel, ControlRefusal, Id, MAX_WHEEL_STEPS, MouseButton, RemoteInput, User, VoiceState, contain_point,
};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen_futures::JsFuture;

use crate::i18n::{t, tf, tn};
use crate::icons::{Icon, lu};
use crate::native_voice;
use crate::store::{Store, store};
use crate::tk;
use crate::ui::{Avatar, Header, initials};
use crate::voice::{self, Control, ControlNote, Feed, ShareNote, Status};
use crate::voice_settings;

/// « Refuser » un appel ou une demande de contrôle. Le catalogue traduit déjà « Refuser » autrement (un droit refusé) :
/// le `{contexte}` vide en fait une entrée à part, invisible en français.
fn decline_label() -> String {
    tf("Refuser{contexte}", &[("contexte", "")])
}

/// Qui est dans ce salon vocal (ou cet appel), dans l'ordre d'arrivée.
fn present(s: Store, guild: Option<Id>, channel: Id) -> Vec<VoiceState> {
    s.present(guild, channel)
}

/// Le compte d'un membre et son nom sur ce serveur (surnom, sinon nom affiché) ; hors serveur,
/// son nom affiché.
fn member(s: Store, guild: Option<Id>, user: Id) -> (Option<User>, String) {
    let Some(guild) = guild else {
        let account = s
            .me
            .with_untracked(|m| m.clone().filter(|m| m.id == user))
            .or_else(|| s.dms.with_untracked(|d| d.iter().flat_map(|c| &c.recipients).find(|u| u.id == user).cloned()));
        return match account {
            Some(u) => {
                let n = u.name().to_string();
                (Some(u), n)
            }
            None => (None, t("Quelqu'un")),
        };
    };
    s.guilds
        .with_untracked(|gs| {
            let m = gs.iter().find(|g| g.guild.id == guild)?.members.iter().find(|m| m.user.id == user)?;
            Some((Some(m.user.clone()), m.nick.clone().unwrap_or_else(|| m.user.name().to_string())))
        })
        .unwrap_or_else(|| (None, t("Membre")))
}

fn face(user: Option<User>, name: &str, size: &'static str) -> AnyView {
    match user {
        Some(u) => view! { <Avatar user=u size=size/> }.into_any(),
        None => {
            let class = if size.is_empty() { "k-avatar".to_string() } else { format!("k-avatar k-avatar--{size}") };
            view! { <span class=class aria-hidden="true">{initials(name)}</span> }.into_any()
        }
    }
}

/// Branche un flux reçu (ou mon écran) sur un élément `<audio>` ou `<video>`, et le lance : le
/// clic sur « Rejoindre » ou « Regarder » autorise la lecture avec le son.
pub fn attach(el: &web_sys::HtmlMediaElement, stream: Option<&web_sys::MediaStream>) {
    if el.src_object().as_ref() == stream {
        return;
    }
    el.set_src_object(stream);
    if stream.is_some()
        && let Ok(p) = el.play()
    {
        spawn_local(async move {
            let _ = JsFuture::from(p).await;
        });
    }
}

/// Le salon vocal affiché dans la zone principale.
#[component]
pub fn VoiceRoom(channel: Channel) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let (guild, cid) = (channel.guild_id, channel.id);
    let name = channel.name.clone().unwrap_or_default();
    let here = Memo::new(move |_| v.room.get().is_some_and(|r| r.channel == cid));
    // Quitter l'écran du salon arrête de regarder (les micros, eux, continuent).
    on_cleanup(move || v.watch(None));
    let (title, label, lobby_name) = (name.clone(), tf("Salon vocal {nom}", &[("nom", &name)]), name.clone());
    view! {
        <Header>
            <Icon icon=lu::LuVolume2/>
            <h1 class="app-title app-ellipsis"><bdi>{title}</bdi></h1>
            <div class="app-header-tools">
                <a class="k-btn k-btn--icon" href="/search" aria-label=t("Rechercher") title=t("Rechercher")><Icon icon=lu::LuSearch/></a>
                <a class="k-btn k-btn--icon" href="/inbox" aria-label=t("Boîte de réception") title=t("Boîte de réception")><Icon icon=lu::LuInbox/></a>
            </div>
        </Header>
        <section class="app-call" aria-label=label>
            {move || if here.get() {
                view! { <Call guild=guild channel=cid/> }.into_any()
            } else {
                view! { <Lobby guild=guild channel=cid name=lobby_name.clone()/> }.into_any()
            }}
        </section>
    }
}

/// Avant d'entrer : qui est là, « Rejoindre », et ce qui empêcherait de parler.
#[component]
fn Lobby(guild: Option<Id>, channel: Id, name: String) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let who = Memo::new(move |_| present(s, guild, channel));
    let error = move || v.error.get().filter(|(c, _)| *c == channel).map(|(_, e)| e);
    let elsewhere = Memo::new(move |_| {
        let r = v.room.get().filter(|r| r.channel != channel)?;
        s.channel(r.channel).and_then(|c| c.name)
    });
    // Ces trois lectures suivent le signal du vocal natif : l'appli de bureau le connaît un instant après le démarrage.
    let supported = voice::supported;
    let insecure = move || voice::supported() && !voice::native() && voice::devices().is_none();
    view! {
        <div class="app-call-lobby">
            <span class="app-call-icon" aria-hidden="true"><Icon icon=lu::LuVolume2/></span>
            <h2><bdi>{name}</bdi></h2>
            <p class="app-hint" aria-live="polite">{move || match who.with(Vec::len) {
                0 => t("Personne pour l'instant."),
                n => tn("{n} personne dans le salon", "{n} personnes dans le salon", n),
            }}</p>
            <Show when=move || who.with(|w| !w.is_empty())>
                <ul class="app-call-who">
                    <For each=move || who.get() key=|x| x.user_id let:x>
                        {
                            let (u, n) = member(s, guild, x.user_id);
                            view! { <li>{face(u, &n, "sm")}<span class="app-ellipsis"><bdi>{n.clone()}</bdi></span></li> }
                        }
                    </For>
                </ul>
            </Show>
            <button class="k-btn k-btn--primary k-btn--lg" disabled=move || !supported() on:click=move |_| v.join(guild, channel)>
                <Icon icon=lu::LuPhone/>{t("Rejoindre")}
            </button>
            {move || voice::native().then(|| view! { <NativeLimits id="lobby-native-why"/> })}
            {move || elsewhere.get().map(|c| view! { <p class="app-hint">{tf("Tu quitteras {salon}.", &[("salon", &c)])}</p> })}
            // Vocal natif : la ligne d'erreur a sa place réservée (la pile est centrée : rien ne monte quand elle arrive).
            {move || if voice::native() {
                view! { <p class="k-error app-lobby-error" role="alert">{move || error().unwrap_or_default()}</p> }.into_any()
            } else {
                error().map(|e| view! { <p class="k-error" role="alert">{e}</p> }).into_any()
            }}
            {move || (!supported()).then(|| view! { <p class="k-error">{voice::unsupported_message()}</p>{voice::desktop().then(|| view! { <OpenInBrowser/> })}<p class="app-hint">{voice::unsupported_details()}</p> })}
            {move || insecure().then(|| view! { <p class="app-hint">{t(voice::INSECURE)}</p> })}
        </div>
    }
}

/// Dans l'appel : état de la connexion, écran regardé, tuiles, commandes.
#[component]
fn Call(guild: Option<Id>, channel: Id) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let who = Memo::new(move |_| present(s, guild, channel).into_iter().map(|x| x.user_id).collect::<Vec<_>>());
    // Vocal natif : qui partage son écran ou allume sa caméra sans que son image puisse s'afficher ici.
    let unseen = Memo::new(move |_| {
        if !voice::native() {
            return Vec::new();
        }
        let me = s.my_id();
        present(s, guild, channel)
            .into_iter()
            .filter(|x| Some(x.user_id) != me && (x.streaming || x.camera))
            .map(|x| member(s, guild, x.user_id).1)
            .collect::<Vec<_>>()
    });
    view! {
        <div class="app-call-main">
            {move || match v.status.get() {
                Status::Connecting => Some(view! {
                    <div class="k-banner" role="status"><Icon icon=lu::LuLoaderCircle/>
                        <span>{if guild.is_some() { t("Connexion au salon vocal…") } else { t("Connexion à l'appel…") }}</span></div>
                }.into_any()),
                Status::Unstable => Some(view! {
                    <div class="k-banner k-banner--warning" role="status"><Icon icon=lu::LuWifiOff/><span>{t("Connexion instable : on attend le retour du réseau…")}</span></div>
                }.into_any()),
                Status::Lost(why) => Some(view! {
                    <div class="k-banner k-banner--warning" role="alert">
                        <Icon icon=lu::LuWifiOff/><span>{why}</span>
                        <button class="k-btn k-btn--sm app-push" on:click=move |_| v.join(guild, channel)>{t("Reconnecter")}</button>
                    </div>
                }.into_any()),
                Status::Off | Status::Connected => None,
            }}
            {move || v.notice.get().map(|n| view! {
                <div class="k-banner" role="status">
                    <Icon icon=lu::LuCircleAlert/><span>{n}</span>
                    <button class="k-btn k-btn--icon" aria-label=t("Masquer ce message") title=t("Masquer") on:click=move |_| v.notice.set(None)>
                        <Icon icon=lu::LuX/>
                    </button>
                </div>
            })}
            {move || v.control_note.get().map(|(u, note)| {
                let who = member(s, guild, u).1;
                let text = match note {
                    ControlNote::Ended => tf("Le contrôle de l'écran de {nom} a pris fin.", &[("nom", &who)]),
                    ControlNote::Refused(ControlRefusal::Declined) => tf("{nom} a refusé ta demande de contrôle.", &[("nom", &who)]),
                    ControlNote::Refused(ControlRefusal::Busy) => tf("Quelqu'un d'autre contrôle déjà l'écran de {nom}.", &[("nom", &who)]),
                    ControlNote::Refused(ControlRefusal::Desktop) => tf(
                        "{nom} partage depuis un navigateur : pour être contrôlé, il faut partager depuis l'appli de bureau KRAKEN.",
                        &[("nom", &who)]),
                    ControlNote::Refused(ControlRefusal::Surface) => tf(
                        "{nom} partage une fenêtre ou un onglet : le contrôle demande le partage d'un écran entier.",
                        &[("nom", &who)]),
                    ControlNote::Refused(ControlRefusal::Unavailable) => tf(
                        "Le contrôle à distance n'est pas possible sur l'ordinateur de {nom} pour l'instant.",
                        &[("nom", &who)]),
                };
                view! {
                    <div class="k-banner" role="status">
                        <Icon icon=lu::LuMousePointer2/><span>{text}</span>
                        <button class="k-btn k-btn--icon" aria-label=t("Masquer ce message") title=t("Masquer") on:click=move |_| v.control_note.set(None)>
                            <Icon icon=lu::LuX/>
                        </button>
                    </div>
                }
            })}
            {move || {
                let who = unseen.get();
                (!who.is_empty()).then(|| view! {
                    <div class="k-banner" role="status">
                        <Icon icon=lu::LuMonitorOff/>
                        <span>{tf(native_voice::CANT_SHOW, &[("noms", &names(&who))])}</span>
                    </div>
                })
            }}
            {move || v.watching.get().map(|u| view! { <Stage guild=guild user=u/> })}
            <ul class="app-tiles" aria-label=t("Participants")>
                <For each=move || who.get() key=|u| *u let:u>
                    <Tile guild=guild channel=channel user=u/>
                </For>
            </ul>
        </div>
        <CallBar private=guild.is_none()/>
    }
}

/// Un participant : sa caméra, sinon l'aperçu de mon écran ou son avatar ; nom, micro et son
/// coupés, « En direct ».
#[component]
fn Tile(guild: Option<Id>, channel: Id, user: Id) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let state = Memo::new(move |_| present(s, guild, channel).into_iter().find(|x| x.user_id == user));
    let me = s.me.with_untracked(|m| m.as_ref().map(|m| m.id)) == Some(user);
    let (account, name) = member(s, guild, user);
    let flag = move |f: fn(&VoiceState) -> bool| state.with(|x| x.as_ref().is_some_and(f));
    let live = move || flag(|x| x.streaming);
    let watched = move || v.watching.get() == Some(user);
    let preview = Memo::new(move |_| me && v.preview.with(Option::is_some));
    // Sa caméra est allumée (le serveur l'a annoncée) et son image est là ; la mienne : l'aperçu.
    let camera = Memo::new(move |_| {
        if me {
            v.camera.get() && v.camera_preview.with(Option::is_some)
        } else {
            flag(|x| x.camera) && v.remote.with(|r| r.iter().any(|x| x.user == user && x.feed == Feed::Camera))
        }
    });
    let watch_label = {
        let name = name.clone();
        move || if watched() { t("Arrêter de regarder") } else { tf("Regarder l'écran de {nom}", &[("nom", &name)]) }
    };
    let avatar = {
        let name = name.clone();
        move || face(account.clone(), &name, "lg")
    };
    // Ma légende est « <mon nom> (toi) », caméra allumée ou non.
    let label = if me { tf("{nom} (toi)", &[("nom", &name)]) } else { name.clone() };
    let cam_label = if me { t("Aperçu de ta caméra") } else { tf("Caméra de {nom}", &[("nom", &name)]) };
    view! {
        <li class="k-tile" data-live=move || live().then_some("") data-camera=move || camera.get().then_some("")
            data-speaking=move || v.speaking.with(|x| x.contains(&user)).then_some("")>
            {move || if camera.get() {
                view! { <CameraVideo user=user me=me label=cam_label.clone()/> }.into_any()
            } else if preview.get() {
                view! { <Preview/> }.into_any()
            } else {
                avatar()
            }}
            <div class="k-tile-name">
                <span class="app-ellipsis"><bdi>{label}</bdi></span>
                <Show when=move || flag(|x| x.self_mute && !x.self_deaf)>
                    <span class="k-tile-flag" role="img" aria-label=t("micro coupé") title=t("Micro coupé")><Icon icon=lu::LuMicOff small=true/></span>
                </Show>
                <Show when=move || flag(|x| x.self_deaf)>
                    <span class="k-tile-flag" role="img" aria-label=t("son coupé") title=t("Son coupé")><Icon icon=lu::LuHeadphoneOff small=true/></span>
                </Show>
                <Show when=live><span class="k-badge k-badge--live">{t("En direct")}</span></Show>
            </div>
            <Show when=move || live() && !me && !voice::native()>
                <button class="k-btn k-btn--sm" aria-label=watch_label.clone() aria-pressed=move || watched().to_string()
                    on:click=move |_| v.watch(if watched() { None } else { Some(user) })>
                    <Icon icon=lu::LuMonitorPlay small=true/>
                    <span>{move || if watched() { t("Arrêter de regarder") } else { t("Regarder") }}</span>
                </button>
            </Show>
        </li>
    }
}

/// La caméra d'un participant, à la place de son avatar : l'image reçue (sans son : le micro
/// arrive à part), ou, pour la mienne, l'aperçu retourné comme un miroir.
#[component]
fn CameraVideo(user: Id, me: bool, label: String) -> impl IntoView {
    let v = store().voice;
    let el = NodeRef::<html::Video>::new();
    Effect::new(move || {
        let Some(e) = el.get() else { return };
        if me {
            v.camera_preview.with(|p| attach(&e, p.as_ref()));
        } else {
            v.remote
                .with(|r| attach(&e, r.iter().find(|x| x.user == user && x.feed == Feed::Camera).map(|x| &x.stream)));
        }
    });
    view! {
        <video class="k-tile-video" data-camera data-mirror=me.then_some("") node_ref=el autoplay playsinline muted
            prop:muted=true aria-label=label></video>
    }
}

/// L'aperçu de mon écran partagé, dans ma tuile.
#[component]
fn Preview() -> impl IntoView {
    let v = store().voice;
    let el = NodeRef::<html::Video>::new();
    Effect::new(move || {
        if let Some(e) = el.get() {
            v.preview.with(|p| attach(&e, p.as_ref()));
        }
    });
    view! { <video class="k-tile-video" node_ref=el autoplay playsinline muted aria-label=t("Aperçu de ton écran partagé")></video> }
}

/// Ce que je tiens enfoncé sur l'écran que je contrôle, et de quoi espacer les mouvements.
#[derive(Default)]
struct Capture {
    keys: Vec<String>,
    buttons: Vec<MouseButton>,
    /// Dernier mouvement envoyé (ms), mouvement en attente, minuterie lancée.
    sent_at: f64,
    pending: Option<(f32, f32)>,
    timer: bool,
    /// Molette accumulée, en crans.
    wheel: (f64, f64),
}

/// Un mouvement toutes les 16 ms au plus (le serveur en accepte 120 par seconde).
const MOVE_GAP_MS: f64 = 16.0;

/// La position du pointeur dans l'image partagée (0 à 1), hors des bandes noires.
fn video_point(el: &web_sys::HtmlVideoElement, x: i32, y: i32) -> Option<(f32, f32)> {
    let r = el.get_bounding_client_rect();
    let (vw, vh) = (f64::from(el.video_width()), f64::from(el.video_height()));
    contain_point(f64::from(x) - r.left(), f64::from(y) - r.top(), r.width(), r.height(), vw, vh)
}

/// L'écran regardé, en grand ; son son passe, sauf si le mien est coupé. Pendant le contrôle à
/// distance, la souris et le clavier sur l'image vont à l'ordinateur de celui qui partage.
#[component]
fn Stage(guild: Option<Id>, user: Id) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let (_, name) = member(s, guild, user);
    let el = NodeRef::<html::Video>::new();
    let frame = NodeRef::<html::Figure>::new();
    let playing = RwSignal::new(false);
    Effect::new(move || {
        let Some(e) = el.get() else { return };
        // La sortie audio choisie dans Voix et vidéo, sur cet élément comme sur ceux créés plus tard.
        voice_settings::apply_sink(&e, &v.speaker.get());
        v.remote.with(|r| attach(&e, r.iter().find(|x| x.user == user && x.feed == Feed::Screen).map(|x| &x.stream)));
    });
    let controlling = Memo::new(move |_| v.control.get() == Control::On(user));
    let asking = Memo::new(move |_| v.control.get() == Control::Asking(user));
    let cap = Rc::new(RefCell::new(Capture::default()));
    // Relâche ce que je tiens (touches, boutons) ; `send` : le dire à l'autre ordinateur.
    let release = {
        let cap = cap.clone();
        move |send: bool| {
            let mut c = cap.borrow_mut();
            for code in std::mem::take(&mut c.keys) {
                if send {
                    v.control_input(RemoteInput::Key { code, down: false });
                }
            }
            for button in std::mem::take(&mut c.buttons) {
                if send {
                    v.control_input(RemoteInput::Button { button, down: false });
                }
            }
            c.pending = None;
            c.wheel = (0.0, 0.0);
        }
    };
    {
        let release = release.clone();
        Effect::new(move || {
            if controlling.get() {
                if let Some(e) = el.get_untracked() {
                    let _ = e.focus();
                }
            } else {
                release(false);
            }
        });
    }
    let send_move = {
        let cap = cap.clone();
        move |p: (f32, f32)| {
            cap.borrow_mut().sent_at = js_sys::Date::now();
            v.control_input(RemoteInput::Move { x: p.0, y: p.1 });
        }
    };
    let on_move = {
        let cap = cap.clone();
        let send_move = send_move.clone();
        move |e: web_sys::PointerEvent| {
            let (true, Some(video)) = (controlling.get_untracked(), el.get_untracked()) else { return };
            let Some(p) = video_point(&video, e.client_x(), e.client_y()) else { return };
            let mut c = cap.borrow_mut();
            if js_sys::Date::now() - c.sent_at >= MOVE_GAP_MS {
                drop(c);
                send_move(p);
                return;
            }
            // Trop tôt : le dernier mouvement part un peu plus tard, pour finir au bon endroit.
            c.pending = Some(p);
            if !c.timer {
                c.timer = true;
                let (cap, send_move) = (cap.clone(), send_move.clone());
                gloo_timers::callback::Timeout::new(MOVE_GAP_MS as u32, move || {
                    let pending = {
                        let mut c = cap.borrow_mut();
                        c.timer = false;
                        c.pending.take()
                    };
                    if let Some(p) = pending.filter(|_| controlling.get_untracked()) {
                        send_move(p);
                    }
                })
                .forget();
            }
        }
    };
    let on_button = {
        let cap = cap.clone();
        let send_move = send_move.clone();
        move |e: web_sys::PointerEvent, down: bool| {
            let (true, Some(video)) = (controlling.get_untracked(), el.get_untracked()) else { return };
            e.prevent_default();
            let Some(button) = MouseButton::from_dom(e.button()) else { return };
            if down {
                let _ = video.focus();
                let _ = video.set_pointer_capture(e.pointer_id());
            }
            if let Some(p) = video_point(&video, e.client_x(), e.client_y()) {
                cap.borrow_mut().pending = None;
                send_move(p);
            }
            let mut c = cap.borrow_mut();
            let held = c.buttons.contains(&button);
            if down == held {
                return;
            }
            c.buttons.retain(|b| *b != button);
            if down {
                c.buttons.push(button);
            }
            drop(c);
            v.control_input(RemoteInput::Button { button, down });
        }
    };
    let on_down = {
        let on_button = on_button.clone();
        move |e: web_sys::PointerEvent| on_button(e, true)
    };
    let on_up = move |e: web_sys::PointerEvent| on_button(e, false);
    let on_wheel = {
        let cap = cap.clone();
        move |e: web_sys::WheelEvent| {
            if !controlling.get_untracked() {
                return;
            }
            e.prevent_default();
            // Un cran : 100 pixels, 3 lignes ou une page.
            let per_step = match e.delta_mode() {
                0 => 100.0,
                1 => 3.0,
                _ => 1.0,
            };
            let mut c = cap.borrow_mut();
            c.wheel.0 += e.delta_x() / per_step;
            c.wheel.1 += e.delta_y() / per_step;
            let max = f64::from(MAX_WHEEL_STEPS);
            let (dx, dy) = (c.wheel.0.trunc().clamp(-max, max), c.wheel.1.trunc().clamp(-max, max));
            if dx == 0.0 && dy == 0.0 {
                return;
            }
            c.wheel = (c.wheel.0.fract(), c.wheel.1.fract());
            drop(c);
            v.control_input(RemoteInput::Wheel { dx: dx as i32, dy: dy as i32 });
        }
    };
    let on_key = {
        let cap = cap.clone();
        let release = release.clone();
        move |e: web_sys::KeyboardEvent, down: bool| {
            if !controlling.get_untracked() {
                return;
            }
            let code = e.code();
            // Échap rend la main (on continue de regarder) ; Maj+Échap envoie Échap.
            if down && code == "Escape" && !e.shift_key() {
                e.prevent_default();
                release(true);
                v.stop_control();
                return;
            }
            e.prevent_default();
            e.stop_propagation();
            if !(RemoteInput::Key { code: code.clone(), down }).valid() {
                return;
            }
            let mut c = cap.borrow_mut();
            let held = c.keys.contains(&code);
            if down && (held || e.repeat()) {
                return;
            }
            if !down && !held {
                return;
            }
            c.keys.retain(|k| *k != code);
            if down {
                c.keys.push(code.clone());
            }
            drop(c);
            v.control_input(RemoteInput::Key { code, down });
        }
    };
    let on_keydown = {
        let on_key = on_key.clone();
        move |e: web_sys::KeyboardEvent| on_key(e, true)
    };
    let on_keyup = move |e: web_sys::KeyboardEvent| on_key(e, false);
    let on_blur = move |_| release(true);
    let hint = tf("Tu contrôles l'écran de {nom}. Échap pour relâcher (Maj+Échap envoie Échap).", &[("nom", &name)]);
    let request_label = tf("Demander le contrôle de l'écran de {nom}", &[("nom", &name)]);
    let full = move |_| {
        let d = document();
        if d.fullscreen_element().is_some() {
            d.exit_fullscreen();
        } else if let Some(f) = frame.get_untracked() {
            let _ = f.request_fullscreen();
        }
    };
    view! {
        <figure class="app-stage" node_ref=frame data-control=move || controlling.get().then_some("")>
            <video node_ref=el autoplay playsinline prop:muted=move || v.deaf.get()
                aria-label=tf("Écran partagé par {nom}", &[("nom", &name)]) on:playing=move |_| playing.set(true)
                tabindex=move || controlling.get().then_some("0")
                on:pointermove=on_move on:pointerdown=on_down on:pointerup=on_up on:wheel=on_wheel
                on:keydown=on_keydown on:keyup=on_keyup on:blur=on_blur
                on:contextmenu=move |e| if controlling.get_untracked() { e.prevent_default() }></video>
            <Show when=move || !playing.get()>
                <p class="app-stage-wait" role="status">{t("Chargement de l'écran…")}</p>
            </Show>
            <Show when=move || controlling.get()>
                <p class="app-stage-hint" role="status">{hint.clone()}</p>
            </Show>
            <figcaption>
                <span class="k-badge k-badge--live">{t("En direct")}</span>
                <b class="app-ellipsis"><bdi>{name.clone()}</bdi></b>
                {
                    let request_label = request_label.clone();
                    move || if controlling.get() {
                        view! {
                            <button class="k-btn k-btn--sm k-btn--danger app-push" on:click=move |_| v.stop_control()>
                                <Icon icon=lu::LuMousePointerBan small=true/><span>{t("Arrêter le contrôle")}</span>
                            </button>
                        }.into_any()
                    } else if asking.get() {
                        view! {
                            <span class="app-push app-stage-asking" role="status">{t("Demande envoyée…")}</span>
                            <button class="k-btn k-btn--sm" on:click=move |_| v.stop_control()>{t("Annuler la demande")}</button>
                        }.into_any()
                    } else {
                        view! {
                            <button class="k-btn k-btn--sm app-push" aria-label=request_label.clone() title=request_label.clone()
                                on:click=move |_| v.request_control(user)>
                                <Icon icon=lu::LuMousePointer2 small=true/><span>{t("Demander le contrôle")}</span>
                            </button>
                        }.into_any()
                    }
                }
                <button class="k-btn k-btn--icon" aria-label=t("Plein écran") title=t("Plein écran") on:click=full>
                    <Icon icon=lu::LuMaximize/>
                </button>
                <button class="k-btn k-btn--icon" aria-label=t("Arrêter de regarder") title=t("Arrêter de regarder") on:click=move |_| v.watch(None)>
                    <Icon icon=lu::LuX/>
                </button>
            </figcaption>
        </figure>
    }
}

/// Micro : pressé = coupé. Dans la barre de l'appel (`big`) ou le panneau « En appel ».
#[component]
pub fn MicToggle(#[prop(optional)] big: bool) -> impl IntoView {
    let v = store().voice;
    let off = move || v.mute.get() || v.deaf.get();
    view! {
        <button class=if big { "k-callbtn" } else { "k-btn k-btn--icon" } aria-label=t("Couper le micro")
            title=move || if off() { t("Réactiver le micro") } else { t("Couper le micro") }
            aria-pressed=move || off().to_string() on:click=move |_| v.toggle_mute()>
            {move || if off() { view! { <Icon icon=lu::LuMicOff/> } } else { view! { <Icon icon=lu::LuMic/> } }}
        </button>
    }
}

/// Son : pressé = je n'entends plus personne (et mon micro est coupé).
#[component]
pub fn DeafToggle(#[prop(optional)] big: bool) -> impl IntoView {
    let v = store().voice;
    let deaf = move || v.deaf.get();
    view! {
        <button class=if big { "k-callbtn" } else { "k-btn k-btn--icon" } aria-label=t("Couper le son")
            title=move || if deaf() { t("Réactiver le son") } else { t("Couper le son") }
            aria-pressed=move || deaf().to_string() on:click=move |_| v.toggle_deaf()>
            {move || if deaf() { view! { <Icon icon=lu::LuHeadphoneOff/> } } else { view! { <Icon icon=lu::LuHeadphones/> } }}
        </button>
    }
}

/// `private` : appel de MP ou de groupe (« Raccrocher » plutôt que « Quitter »).
#[component]
fn CallBar(private: bool) -> impl IntoView {
    let v = store().voice;
    let can_share = voice::can_share();
    let why = if voice::devices().is_none() {
        t("Le partage d'écran demande l'instance en HTTPS (ou ouverte via localhost).")
    } else {
        t("Ce navigateur ne permet pas de partager l'écran.")
    };
    // Vocal natif : ni caméra ni partage d'écran, dit par une phrase visible sous la barre.
    let native = voice::native;
    let share_ok = move || can_share && !native();
    let share_label = move || if v.streaming.get() { t("Arrêter le partage") } else { t("Partager ton écran") };
    // Sans le droit « Caméra » : bouton désactivé, la raison en infobulle et lue par `aria-describedby`.
    // Une caméra déjà allumée reste éteignable si le droit disparaît en cours d'appel.
    let cam_blocked = move || !v.camera.get() && !native() && v.camera_denied();
    let cam_title = move || {
        if native() {
            t(native_voice::NO_VIDEO)
        } else if cam_blocked() {
            t(voice::CAM_NO_RIGHT)
        } else if v.camera.get() {
            t("Couper la caméra")
        } else {
            t("Activer la caméra")
        }
    };
    view! {
        // Au-dessus de la barre, et la place du message est réservée : en apparaissant, il ne déplace
        // ni la barre ni les tuiles.
        <div class="app-call-note">
            {move || v.camera_error.get().map(|e| view! { <p class="k-error" role="alert">{e}</p> })}
        </div>
        <div class="k-callbar" role="group" aria-label=t("Commandes de l'appel")>
            <MicToggle big=true/>
            <DeafToggle big=true/>
            <button class="k-callbtn" data-on=move || v.camera.get().then_some("")
                disabled=move || native() || cam_blocked() || matches!(v.status.get(), Status::Lost(_) | Status::Off)
                aria-label=t("Caméra") title=cam_title aria-pressed=move || v.camera.get().to_string()
                aria-describedby=move || if native() { Some("call-native-why") } else { cam_blocked().then_some("call-cam-why") }
                on:click=move |_| v.toggle_camera()>
                {move || if v.camera.get() { view! { <Icon icon=lu::LuVideo/> } } else { view! { <Icon icon=lu::LuVideoOff/> } }}
                <span>{t("Caméra")}</span>
            </button>
            <button class="k-callbtn" data-on=move || v.streaming.get().then_some("")
                disabled=move || !share_ok() || matches!(v.status.get(), Status::Lost(_) | Status::Off)
                aria-label=share_label title=move || if native() { t(native_voice::NO_VIDEO) } else { share_label() }
                aria-describedby=move || if native() { Some("call-native-why") } else { (!can_share).then_some("call-share-why") }
                on:click=move |_| if v.streaming.get_untracked() { v.stop_share() } else { v.share() }>
                {move || if v.streaming.get() { view! { <Icon icon=lu::LuScreenShareOff/> } } else { view! { <Icon icon=lu::LuScreenShare/> } }}
                <span>{share_label}</span>
            </button>
            {if private {
                view! {
                    <button class="k-callbtn k-callbtn--danger" aria-label=t("Raccrocher") title=t("Raccrocher") on:click=move |_| v.leave()>
                        <Icon icon=lu::LuPhoneOff/><span>{t("Raccrocher")}</span>
                    </button>
                }.into_any()
            } else {
                view! {
                    <button class="k-callbtn k-callbtn--danger" aria-label=t("Quitter") title=t("Quitter le salon vocal") on:click=move |_| v.leave()>
                        <Icon icon=lu::LuPhoneOff/><span>{t("Quitter")}</span>
                    </button>
                }.into_any()
            }}
            // Touche tenue : « Micro ouvert » et un micro plein, pas seulement la couleur. Les deux textes
            // occupent la même case, pour que la barre ne bouge pas quand on tient ou relâche.
            {move || v.ptt_armed().then(|| view! {
                <span class="app-call-ptt" data-on=move || v.ptt_held.get().then_some("")>
                    <Icon icon=lu::LuMic small=true/>
                    <span class="app-call-ptt-text">
                        <span class="app-call-ptt-idle">
                            {t("Appuyer pour parler :")}" "
                            <span class="k-kbd"><kbd>{move || v.ptt_key.with(|k| k.as_ref().map(|k| k.label.clone()).unwrap_or_default())}</kbd></span>
                        </span>
                        <span class="app-call-ptt-held">{t("Micro ouvert")}</span>
                    </span>
                </span>
            })}
        </div>
        {move || native().then(|| view! { <NativeLimits id="call-native-why"/> })}
        {move || (!can_share && !native()).then(|| view! { <p class="app-hint app-call-why" id="call-share-why">{why.clone()}</p> })}
        {move || cam_blocked().then(|| view! { <p class="app-hint app-call-why" id="call-cam-why">{t(voice::CAM_NO_RIGHT)}</p> })}
    }
}

/// Sous un salon vocal de la barre latérale : qui y est.
#[component]
pub fn VoiceUsers(guild: Option<Id>, channel: Id, name: String, hidden: Signal<bool>) -> impl IntoView {
    let s = store();
    let who = Memo::new(move |_| present(s, guild, channel));
    view! {
        <Show when=move || who.with(|w| !w.is_empty())>
            <ul class="app-voice-users" aria-label=tf("Dans {nom}", &[("nom", &name)]) hidden=hidden>
                <For each=move || who.get() key=|x| (x.user_id, x.self_mute, x.self_deaf, x.streaming, x.camera) let:x>
                    {
                        let (u, n) = member(s, guild, x.user_id);
                        let user = x.user_id;
                        view! {
                            <li data-speaking=move || s.voice.speaking.with(|x| x.contains(&user)).then_some("")>
                                {face(u, &n, "sm")}
                                <span class="app-ellipsis"><bdi>{n.clone()}</bdi></span>
                                {(x.self_mute && !x.self_deaf).then(|| view! {
                                    <span role="img" aria-label=t("micro coupé") title=t("Micro coupé")><Icon icon=lu::LuMicOff small=true/></span>
                                })}
                                {x.self_deaf.then(|| view! {
                                    <span role="img" aria-label=t("son coupé") title=t("Son coupé")><Icon icon=lu::LuHeadphoneOff small=true/></span>
                                })}
                                {x.camera.then(|| view! {
                                    <span role="img" aria-label=t("caméra allumée") title=t("Caméra allumée")><Icon icon=lu::LuVideo small=true/></span>
                                })}
                                {x.streaming.then(|| view! { <span class="k-badge k-badge--live">{t("En direct")}</span> })}
                            </li>
                        }
                    }
                </For>
            </ul>
        </Show>
    }
}

/// En bas de la barre latérale pendant un appel : l'état, où, micro, son et quitter.
#[component]
pub fn VoicePanel() -> impl IntoView {
    let s = store();
    let v = s.voice;
    let place = Memo::new(move |_| {
        let r = v.room.get()?;
        // `where_` : le texte de l'info-bulle ; `parts` : le salon et le serveur, chacun isolé à l'affichage (deux noms saisis).
        let (href, where_, parts) = match r.guild {
            Some(gid) => {
                let g = s.guild(gid)?;
                let c = g.channels.iter().find(|c| c.id == r.channel).and_then(|c| c.name.clone()).unwrap_or_default();
                let shown = format!("{c} / {}", g.guild.name);
                (format!("/channels/{gid}/{}", r.channel), shown, Some((c, g.guild.name)))
            }
            None => {
                let title = s.channel(r.channel).map(|c| crate::social::private_title(&c)).unwrap_or_default();
                (format!("/channels/@me/{}", r.channel), tf("Appel avec {nom}", &[("nom", &title)]), None)
            }
        };
        Some((r, href, where_, parts))
    });
    let state = move || match v.status.get() {
        Status::Connected => ("on", tk!("En appel")),
        Status::Connecting => ("wait", tk!("Connexion…")),
        Status::Unstable => ("wait", tk!("Connexion instable")),
        Status::Lost(_) | Status::Off => ("lost", tk!("Connexion perdue")),
    };
    view! {
        {move || place.get().map(|(r, href, where_, parts)| {
            let shown = match parts {
                Some((c, g)) => view! { <bdi>{c}</bdi>" / "<bdi>{g}</bdi> }.into_any(),
                None => where_.clone().into_any(),
            };
            let leave = if r.guild.is_some() { t("Quitter le salon vocal") } else { t("Raccrocher") };
            view! {
                <section class="k-voicebar" aria-label=t("Connexion vocale")>
                    <div>
                        <b data-state=move || state().0 role="status">{move || t(state().1)}</b>
                        <a class="app-ellipsis" href=href title=where_.clone()>{shown}</a>
                    </div>
                    <MicToggle/>
                    <DeafToggle/>
                    <Show when=move || matches!(v.status.get(), Status::Lost(_))>
                        <button class="k-btn k-btn--icon" aria-label=t("Reconnecter") title=t("Reconnecter") on:click=move |_| v.join(r.guild, r.channel)>
                            <Icon icon=lu::LuRefreshCw/>
                        </button>
                    </Show>
                    <button class="k-btn k-btn--icon" aria-label=leave.clone() title=leave on:click=move |_| v.leave()>
                        <Icon icon=lu::LuPhoneOff/>
                    </button>
                </section>
            }
        })}
    }
}

/// Le son des micros reçus : un `<audio>` caché par personne, coupé avec le son (et envoyé sur la sortie
/// audio choisie dans les réglages).
#[component]
pub fn CallAudio() -> impl IntoView {
    let v = store().voice;
    let mics = Memo::new(move |_| {
        v.remote.with(|r| r.iter().filter(|x| x.feed == Feed::Mic).map(|x| x.user).collect::<Vec<_>>())
    });
    // Appuyer pour parler : l'état du micro dit à voix basse quand la touche se tient ou se relâche. La zone
    // n'existe que pendant un appel où le mode est armé ; elle est hors de « Connexion vocale ».
    let announce = move || v.room.with(Option::is_some) && v.ptt_armed();
    view! {
        <div hidden>
            <For each=move || mics.get() key=|u| *u let:u>
                <RemoteAudio user=u/>
            </For>
        </div>
        {move || announce().then(|| view! { <p class="k-sr" role="status" aria-live="polite">{move || t(v.ptt_say.get())}</p> })}
    }
}

#[component]
fn RemoteAudio(user: Id) -> impl IntoView {
    let v = store().voice;
    let el = NodeRef::<html::Audio>::new();
    Effect::new(move || {
        let Some(e) = el.get() else { return };
        voice_settings::apply_sink(&e, &v.speaker.get());
        v.remote.with(|r| attach(&e, r.iter().find(|x| x.user == user && x.feed == Feed::Mic).map(|x| &x.stream)));
    });
    view! { <audio node_ref=el autoplay prop:muted=move || v.deaf.get()></audio> }
}

/// Le contrôle à distance de mon écran, visible où que je sois pendant l'appel : qui le demande
/// (accepter ou refuser), qui l'a (« Arrêter le contrôle »), et ce qui l'a empêché.
#[component]
pub fn ControlPanel() -> impl IntoView {
    let s = store();
    let v = s.voice;
    let name = move |u: Id| v.room.get_untracked().map_or_else(|| t("Quelqu'un"), |r| member(s, r.guild, u).1);
    let shown = move || {
        v.controller.get().is_some() || v.requests.with(|r| !r.is_empty()) || v.share_note.with(Option::is_some)
    };
    view! {
        <Show when=shown>
            <section class="app-remote" aria-label=t("Contrôle à distance de ton écran")>
                {move || v.controller.get().map(|u| view! {
                    <div class="app-remote-row" data-on="">
                        <Icon icon=lu::LuMousePointer2/>
                        <p role="status">{tf("{nom} contrôle ton écran.", &[("nom", &name(u))])}</p>
                        <button class="k-btn k-btn--sm k-btn--danger" on:click=move |_| v.release_control(true)>
                            <Icon icon=lu::LuMousePointerBan small=true/><span>{t("Arrêter le contrôle")}</span>
                        </button>
                    </div>
                })}
                <For each=move || v.requests.get() key=|u| *u let:u>
                    {
                        let who = name(u);
                        let waiting = move || v.confirming.get() == Some(u);
                        let who_accept = who.clone();
                        view! {
                            <div class="app-remote-row" role="alert">
                                <Icon icon=lu::LuMousePointer2/>
                                <p>
                                    {tf("{nom} demande à contrôler ton écran (souris et clavier).", &[("nom", &who)])}
                                    <Show when=waiting>
                                        <span class="app-remote-wait">" "{t("Réponds dans la fenêtre de KRAKEN qui vient de s'ouvrir.")}</span>
                                    </Show>
                                </p>
                                <button class="k-btn k-btn--sm k-btn--primary" disabled=move || v.confirming.get().is_some()
                                    on:click=move |_| v.accept_control(u, who_accept.clone())>{t("Accepter")}</button>
                                <button class="k-btn k-btn--sm" disabled=waiting on:click=move |_| v.decline_control(u)>{decline_label()}</button>
                            </div>
                        }
                    }
                </For>
                {move || v.share_note.get().map(|(u, note)| {
                    let who = name(u);
                    let text = match note {
                        ShareNote::Desktop => tf(
                            "{nom} a demandé à contrôler ton écran. Le contrôle à distance n'est possible que depuis l'appli de bureau KRAKEN : partage ton écran depuis l'appli pour l'accepter.",
                            &[("nom", &who)]),
                        ShareNote::Surface => tf(
                            "{nom} a demandé à contrôler ton écran, mais tu partages une fenêtre ou un onglet. Partage ton écran entier pour pouvoir l'accepter.",
                            &[("nom", &who)]),
                        ShareNote::Message(m) => m,
                    };
                    view! {
                        <div class="app-remote-row">
                            <Icon icon=lu::LuCircleAlert/>
                            <p role="status">{text}</p>
                            <button class="k-btn k-btn--icon" aria-label=t("Masquer ce message") title=t("Masquer") on:click=move |_| v.share_note.set(None)>
                                <Icon icon=lu::LuX/>
                            </button>
                        </div>
                    }
                })}
            </section>
        </Show>
    }
}

// ───── appels des MP et des groupes ─────

/// « Lou », « Lou et Sam », « Lou, Sam et Noa ».
fn names(list: &[String]) -> String {
    match list {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => tf("{liste} et {dernier}", &[("liste", &rest.join(", ")), ("dernier", last.as_str())]),
    }
}

/// En haut d'un MP ou d'un groupe : l'appel en cours (ou le mien), qui y est, chez qui ça sonne.
#[component]
pub fn DmCall(channel: Id) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let here = Memo::new(move |_| v.room.get().is_some_and(|r| r.channel == channel));
    let who = Memo::new(move |_| present(s, None, channel));
    let ringing = Memo::new(move |_| {
        let me = s.my_id();
        let list = s.call(channel).map(|c| c.ringing).unwrap_or_default();
        list.into_iter().filter(|u| Some(*u) != me).map(|u| member(s, None, u).1).collect::<Vec<_>>()
    });
    let error = move || v.error.get().filter(|(c, _)| *c == channel).map(|(_, e)| e);
    let shown = move || here.get() || who.with(|w| !w.is_empty()) || error().is_some();
    view! {
        <Show when=shown>
            <section class="app-dmcall" aria-label=t("Appel")>
                {move || if here.get() {
                    view! { <Call guild=None channel=channel/> }.into_any()
                } else if who.with(|w| !w.is_empty()) {
                    view! {
                        <div class="app-dmcall-lobby">
                            <span class="app-call-icon" aria-hidden="true"><Icon icon=lu::LuPhoneCall/></span>
                            <p class="app-hint" role="status">{t("Appel en cours")}</p>
                            <ul class="app-call-who" aria-label=t("Dans l'appel")>
                                <For each=move || who.get() key=|x| x.user_id let:x>
                                    {
                                        let (u, n) = member(s, None, x.user_id);
                                        view! { <li>{face(u, &n, "sm")}<span class="app-ellipsis"><bdi>{n.clone()}</bdi></span></li> }
                                    }
                                </For>
                            </ul>
                            <button class="k-btn k-btn--primary" disabled=move || !voice::supported() title=move || (!voice::supported()).then(voice::unsupported_message)
                                aria-describedby=move || (!voice::supported()).then_some("dmcall-nosupport") on:click=move |_| v.join(None, channel)>
                                <Icon icon=lu::LuPhone/>{t("Rejoindre l'appel")}
                            </button>
                            {move || (!voice::supported()).then(|| view! { <p class="k-error app-dmcall-error" id="dmcall-nosupport">{voice::unsupported_message()}</p>{voice::desktop().then(|| view! { <OpenInBrowser/> })} })}
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }}
                <Show when=move || here.get() && ringing.with(|r| !r.is_empty())>
                    <p class="app-hint app-dmcall-ringing" role="status">
                        <Icon icon=lu::LuBellRing small=true/>{move || tf("Ça sonne chez {noms}…", &[("noms", &names(&ringing.get()))])}
                    </p>
                </Show>
                {move || error().map(|e| view! { <p class="k-error app-dmcall-error" role="alert">{e}</p> })}
            </section>
        </Show>
    }
}

/// Appli de bureau sans appel dans son navigateur intégré (ou avec la voix seulement) : continuer dans le navigateur du système. L'erreur a sa place
/// réservée (un paragraphe `alert` toujours là, sur sa ligne, d'une ligne de haut) : rien ne bouge quand elle arrive.
#[component]
pub(crate) fn OpenInBrowser(#[prop(optional)] small: bool) -> impl IntoView {
    let failed = RwSignal::new(None::<String>);
    view! {
        <button class="k-btn" class:k-btn--sm=small on:click=move |_| spawn_local(async move { failed.set(voice::open_in_browser().await.err()) })>
            <Icon icon=lu::LuExternalLink small=small/>{t("Ouvrir dans mon navigateur")}
        </button>
        <p class="k-error app-open-error" role="alert">{move || failed.get()}</p>
    }
}

/// Vocal natif (appli de bureau) : ce que l'appel ne fait pas encore, dit en toutes lettres, et le moyen d'y
/// arriver par le navigateur du système. `id` : ce que citent les boutons grisés par `aria-describedby`.
#[component]
fn NativeLimits(id: &'static str) -> impl IntoView {
    view! {
        <div class="app-native-note">
            <p class="app-hint" id=id>{t(native_voice::NO_VIDEO)}</p>
            <OpenInBrowser small=true/>
        </div>
    }
}

/// Sous l'appel entrant, dans l'appli de bureau sans WebRTC : pourquoi « Rejoindre » est grisé, et quoi faire.
const RING_WHY: &str = tk!("Pas de vocal dans l'appli de bureau : réponds depuis ton navigateur.");

/// L'infobulle d'un bouton d'appel : `normal`, ou pourquoi il est grisé quand WebRTC manque.
pub fn call_title(normal: String) -> String {
    if voice::supported() { normal } else { voice::unsupported_message() }
}

/// Le même pourquoi, pour les lecteurs d'écran : un texte caché que le bouton grisé cite par `aria-describedby=id`. Rien quand les
/// appels marchent. Suit le signal du vocal natif, connu un instant après le démarrage.
pub fn call_note(id: &str) -> impl IntoView + use<> {
    let id = id.to_string();
    move || {
        (!voice::supported()).then(|| view! { <span class="k-sr" id=id.clone()>{voice::unsupported_message()}</span> })
    }
}

/// Dans l'en-tête d'un MP ou d'un groupe sans appel en cours : lancer l'appel.
#[component]
pub fn StartCall(channel: Id, #[prop(into)] blocked: Signal<bool>) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let idle =
        Memo::new(move |_| v.room.get().is_none_or(|r| r.channel != channel) && present(s, None, channel).is_empty());
    let note = format!("call-no-{channel}");
    view! {
        <Show when=move || idle.get()>
            <button class="k-btn k-btn--icon" aria-label=t("Démarrer un appel") title=move || call_title(t("Démarrer un appel"))
                aria-describedby={ let note = note.clone(); move || (!voice::supported()).then(|| note.clone()) }
                disabled=move || blocked.get() || !voice::supported() on:click=move |_| v.join(None, channel)>
                <Icon icon=lu::LuPhoneCall/>
            </button>
            {call_note(&note)}
        </Show>
    }
}

thread_local! {
    /// La sonnerie en cours : son contexte audio et le minuteur qui la répète.
    static RINGER: RefCell<Option<(web_sys::AudioContext, gloo_timers::callback::Interval)>> = const { RefCell::new(None) };
}

/// Deux notes brèves, synthétisées (aucun fichier son).
fn chime(ctx: &web_sys::AudioContext) {
    let t = ctx.current_time();
    for (i, freq) in [659.25_f32, 880.0].into_iter().enumerate() {
        let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else { return };
        osc.set_type(web_sys::OscillatorType::Sine);
        osc.frequency().set_value(freq);
        let start = t + i as f64 * 0.2;
        let g = gain.gain();
        let _ = g.set_value_at_time(0.0, start);
        let _ = g.linear_ramp_to_value_at_time(0.12, start + 0.02);
        let _ = g.linear_ramp_to_value_at_time(0.0, start + 0.18);
        let _ = osc.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&ctx.destination());
        let _ = osc.start_with_when(start);
        let _ = osc.stop_with_when(start + 0.2);
    }
}

/// Sonne (toutes les 2,5 s) ou se tait.
fn ring(on: bool) {
    let running = RINGER.with_borrow(Option::is_some);
    if on && !running {
        let Ok(ctx) = web_sys::AudioContext::new() else { return };
        chime(&ctx);
        let again = ctx.clone();
        let timer = gloo_timers::callback::Interval::new(2_500, move || chime(&again));
        RINGER.set(Some((ctx, timer)));
    } else if !on && let Some((ctx, timer)) = RINGER.take() {
        timer.cancel();
        let _ = ctx.close();
    }
}

/// Les appels qui sonnent chez moi, où que je sois : qui appelle, « Rejoindre », « Refuser ».
#[component]
pub fn IncomingCalls() -> impl IntoView {
    let s = store();
    let v = s.voice;
    let incoming = Memo::new(move |_| {
        let Some(me) = s.me.with(|m| m.as_ref().map(|m| m.id)) else { return vec![] };
        let room = v.room.get().map(|r| r.channel);
        s.calls.with(|cs| {
            cs.iter().filter(|c| c.ringing.contains(&me) && Some(c.channel_id) != room).cloned().collect::<Vec<_>>()
        })
    });
    // La carte d'appel s'affiche toujours ; « Ne pas déranger » en coupe seulement la sonnerie.
    Effect::new(move || {
        ring(!incoming.with(Vec::is_empty) && !v.deaf.get() && s.presence.get() != kraken_proto::Presence::Dnd)
    });
    on_cleanup(|| ring(false));
    view! {
        <Show when=move || incoming.with(|c| !c.is_empty())>
            <section class="app-ring" aria-label=t("Appels entrants")>
                <For each=move || incoming.get() key=|c| (c.channel_id, c.message_id) let:c>
                    <RingCard call=c/>
                </For>
            </section>
        </Show>
    }
}

#[component]
fn RingCard(call: kraken_proto::Call) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let navigate = leptos_router::hooks::use_navigate();
    let cid = call.channel_id;
    let (account, who) = member(s, None, call.started_by);
    let group = s
        .channel(cid)
        .filter(|ch| ch.kind == kraken_proto::ChannelKind::GroupDm)
        .map(|ch| crate::social::private_title(&ch));
    let line = match &group {
        Some(g) => tf("t'appelle dans {salon}", &[("salon", g.as_str())]),
        None => t("t'appelle"),
    };
    let answer = move |_| {
        v.join(None, cid);
        navigate(&format!("/channels/@me/{cid}"), Default::default());
    };
    let decline = move |_| {
        let me = s.my_id();
        s.calls.update(|cs| {
            cs.iter_mut().filter(|c| c.channel_id == cid).for_each(|c| c.ringing.retain(|u| Some(*u) != me))
        });
        spawn_local(async move {
            let _ = crate::api::call_decline(cid).await;
        });
    };
    view! {
        <div class="app-ring-card" role="alertdialog" aria-label=tf("Appel de {nom}", &[("nom", &who)]) aria-describedby=format!("ring-{cid}")>
            {face(account, &who, "md")}
            <p id=format!("ring-{cid}")><b class="app-ellipsis"><bdi>{who.clone()}</bdi></b><span>{line}</span></p>
            <button class="k-btn k-btn--sm k-btn--primary" disabled=move || !voice::supported() title=move || (!voice::supported()).then(voice::unsupported_message)
                aria-describedby=move || (!voice::supported()).then(|| format!("ring-no-{cid}")) on:click=answer>
                <Icon icon=lu::LuPhone small=true/><span>{t("Rejoindre")}</span>
            </button>
            {call_note(&format!("ring-no-{cid}"))}
            {move || (voice::desktop() && !voice::supported()).then(|| view! {
                <p class="app-ring-why">{t(RING_WHY)}</p>
                <OpenInBrowser small=true/>
            })}
            <button class="k-btn k-btn--sm k-btn--danger" on:click=decline>
                <Icon icon=lu::LuPhoneOff small=true/><span>{decline_label()}</span>
            </button>
        </div>
    }
}
