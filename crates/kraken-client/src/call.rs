//! Salon vocal (S18) : avant d'entrer, qui est là et « Rejoindre » ; ensuite les tuiles des
//! participants, l'écran regardé en grand et la barre de commandes. Plus ce que la coquille montre
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

use crate::icons::{Icon, lu};
use crate::store::{Store, store};
use crate::ui::{Avatar, Header, initials};
use crate::voice::{self, Control, ControlNote, ShareNote, Status};

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
            None => (None, "Quelqu'un".into()),
        };
    };
    s.guilds
        .with_untracked(|gs| {
            let m = gs.iter().find(|g| g.guild.id == guild)?.members.iter().find(|m| m.user.id == user)?;
            Some((Some(m.user.clone()), m.nick.clone().unwrap_or_else(|| m.user.name().to_string())))
        })
        .unwrap_or_else(|| (None, "Membre".into()))
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
fn attach(el: &web_sys::HtmlMediaElement, stream: Option<&web_sys::MediaStream>) {
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
    let (title, label, lobby_name) = (name.clone(), format!("Salon vocal {name}"), name.clone());
    view! {
        <Header>
            <Icon icon=lu::LuVolume2/>
            <h1 class="app-title app-ellipsis">{title}</h1>
            <div class="app-header-tools">
                <a class="k-btn k-btn--icon" href="/search" aria-label="Rechercher" title="Rechercher"><Icon icon=lu::LuSearch/></a>
                <a class="k-btn k-btn--icon" href="/inbox" aria-label="Boîte de réception" title="Boîte de réception"><Icon icon=lu::LuInbox/></a>
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
    let supported = voice::supported();
    let insecure = supported && voice::devices().is_none();
    view! {
        <div class="app-call-lobby">
            <span class="app-call-icon" aria-hidden="true"><Icon icon=lu::LuVolume2/></span>
            <h2>{name}</h2>
            <p class="app-hint" aria-live="polite">{move || match who.with(Vec::len) {
                0 => "Personne pour l'instant.".to_string(),
                1 => "1 personne dans le salon".to_string(),
                n => format!("{n} personnes dans le salon"),
            }}</p>
            <Show when=move || who.with(|w| !w.is_empty())>
                <ul class="app-call-who">
                    <For each=move || who.get() key=|x| x.user_id let:x>
                        {
                            let (u, n) = member(s, guild, x.user_id);
                            view! { <li>{face(u, &n, "sm")}<span class="app-ellipsis">{n.clone()}</span></li> }
                        }
                    </For>
                </ul>
            </Show>
            <button class="k-btn k-btn--primary k-btn--lg" disabled=!supported on:click=move |_| v.join(guild, channel)>
                <Icon icon=lu::LuPhone/>"Rejoindre"
            </button>
            {move || elsewhere.get().map(|c| view! { <p class="app-hint">{format!("Tu quitteras {c}.")}</p> })}
            {move || error().map(|e| view! { <p class="k-error" role="alert">{e}</p> })}
            {(!supported).then(|| view! { <p class="k-error">"Ton navigateur ne permet pas les appels vocaux. Essaie avec un navigateur récent."</p> })}
            {insecure.then(|| view! { <p class="app-hint">{voice::INSECURE}</p> })}
        </div>
    }
}

/// Dans l'appel : état de la connexion, écran regardé, tuiles, commandes.
#[component]
fn Call(guild: Option<Id>, channel: Id) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let who = Memo::new(move |_| present(s, guild, channel).into_iter().map(|x| x.user_id).collect::<Vec<_>>());
    view! {
        <div class="app-call-main">
            {move || match v.status.get() {
                Status::Connecting => Some(view! {
                    <div class="k-banner" role="status"><Icon icon=lu::LuLoaderCircle/>
                        <span>{if guild.is_some() { "Connexion au salon vocal…" } else { "Connexion à l'appel…" }}</span></div>
                }.into_any()),
                Status::Unstable => Some(view! {
                    <div class="k-banner k-banner--warning" role="status"><Icon icon=lu::LuWifiOff/><span>"Connexion instable : on attend le retour du réseau…"</span></div>
                }.into_any()),
                Status::Lost(why) => Some(view! {
                    <div class="k-banner k-banner--warning" role="alert">
                        <Icon icon=lu::LuWifiOff/><span>{why}</span>
                        <button class="k-btn k-btn--sm app-push" on:click=move |_| v.join(guild, channel)>"Reconnecter"</button>
                    </div>
                }.into_any()),
                Status::Off | Status::Connected => None,
            }}
            {move || v.notice.get().map(|n| view! {
                <div class="k-banner" role="status">
                    <Icon icon=lu::LuCircleAlert/><span>{n}</span>
                    <button class="k-btn k-btn--icon" aria-label="Masquer ce message" title="Masquer" on:click=move |_| v.notice.set(None)>
                        <Icon icon=lu::LuX/>
                    </button>
                </div>
            })}
            {move || v.control_note.get().map(|(u, note)| {
                let who = member(s, guild, u).1;
                let text = match note {
                    ControlNote::Ended => format!("Le contrôle de l'écran de {who} a pris fin."),
                    ControlNote::Refused(ControlRefusal::Declined) => format!("{who} a refusé ta demande de contrôle."),
                    ControlNote::Refused(ControlRefusal::Busy) => format!("Quelqu'un d'autre contrôle déjà l'écran de {who}."),
                    ControlNote::Refused(ControlRefusal::Desktop) => format!(
                        "{who} partage depuis un navigateur : pour être contrôlé, il faut partager depuis l'appli de bureau KRAKEN."),
                    ControlNote::Refused(ControlRefusal::Surface) => format!(
                        "{who} partage une fenêtre ou un onglet : le contrôle demande le partage d'un écran entier."),
                    ControlNote::Refused(ControlRefusal::Unavailable) => format!(
                        "Le contrôle à distance n'est pas possible sur l'ordinateur de {who} pour l'instant."),
                };
                view! {
                    <div class="k-banner" role="status">
                        <Icon icon=lu::LuMousePointer2/><span>{text}</span>
                        <button class="k-btn k-btn--icon" aria-label="Masquer ce message" title="Masquer" on:click=move |_| v.control_note.set(None)>
                            <Icon icon=lu::LuX/>
                        </button>
                    </div>
                }
            })}
            {move || v.watching.get().map(|u| view! { <Stage guild=guild user=u/> })}
            <ul class="app-tiles" aria-label="Participants">
                <For each=move || who.get() key=|u| *u let:u>
                    <Tile guild=guild channel=channel user=u/>
                </For>
            </ul>
        </div>
        <CallBar private=guild.is_none()/>
    }
}

/// Un participant : avatar (ou l'aperçu de mon écran), nom, micro et son coupés, « En direct ».
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
    let preview = move || me && v.preview.with(Option::is_some);
    let watch_label = {
        let name = name.clone();
        move || if watched() { "Arrêter de regarder".to_string() } else { format!("Regarder l'écran de {name}") }
    };
    let avatar = {
        let name = name.clone();
        move || face(account.clone(), &name, "lg")
    };
    view! {
        <li class="k-tile" data-live=move || live().then_some("")
            data-speaking=move || v.speaking.with(|x| x.contains(&user)).then_some("")>
            <Show when=preview fallback=avatar>
                <Preview/>
            </Show>
            <div class="k-tile-name">
                <span class="app-ellipsis">{name.clone()}{me.then_some(" (toi)")}</span>
                <Show when=move || flag(|x| x.self_mute && !x.self_deaf)>
                    <span class="k-tile-flag" role="img" aria-label="micro coupé" title="Micro coupé"><Icon icon=lu::LuMicOff small=true/></span>
                </Show>
                <Show when=move || flag(|x| x.self_deaf)>
                    <span class="k-tile-flag" role="img" aria-label="son coupé" title="Son coupé"><Icon icon=lu::LuHeadphoneOff small=true/></span>
                </Show>
                <Show when=live><span class="k-badge k-badge--live">"En direct"</span></Show>
            </div>
            <Show when=move || live() && !me>
                <button class="k-btn k-btn--sm" aria-label=watch_label.clone() aria-pressed=move || watched().to_string()
                    on:click=move |_| v.watch(if watched() { None } else { Some(user) })>
                    <Icon icon=lu::LuMonitorPlay small=true/>
                    <span>{move || if watched() { "Arrêter de regarder" } else { "Regarder" }}</span>
                </button>
            </Show>
        </li>
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
    view! { <video class="k-tile-video" node_ref=el autoplay playsinline muted aria-label="Aperçu de ton écran partagé"></video> }
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
        v.remote.with(|r| attach(&e, r.iter().find(|x| x.user == user && x.screen).map(|x| &x.stream)));
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
    let hint = format!("Tu contrôles l'écran de {name}. Échap pour relâcher (Maj+Échap envoie Échap).");
    let request_label = format!("Demander le contrôle de l'écran de {name}");
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
                aria-label=format!("Écran partagé par {name}") on:playing=move |_| playing.set(true)
                tabindex=move || controlling.get().then_some("0")
                on:pointermove=on_move on:pointerdown=on_down on:pointerup=on_up on:wheel=on_wheel
                on:keydown=on_keydown on:keyup=on_keyup on:blur=on_blur
                on:contextmenu=move |e| if controlling.get_untracked() { e.prevent_default() }></video>
            <Show when=move || !playing.get()>
                <p class="app-stage-wait" role="status">"Chargement de l'écran…"</p>
            </Show>
            <Show when=move || controlling.get()>
                <p class="app-stage-hint" role="status">{hint.clone()}</p>
            </Show>
            <figcaption>
                <span class="k-badge k-badge--live">"En direct"</span>
                <b class="app-ellipsis">{name.clone()}</b>
                {
                    let request_label = request_label.clone();
                    move || if controlling.get() {
                        view! {
                            <button class="k-btn k-btn--sm k-btn--danger app-push" on:click=move |_| v.stop_control()>
                                <Icon icon=lu::LuMousePointerBan small=true/><span>"Arrêter le contrôle"</span>
                            </button>
                        }.into_any()
                    } else if asking.get() {
                        view! {
                            <span class="app-push app-stage-asking" role="status">"Demande envoyée…"</span>
                            <button class="k-btn k-btn--sm" on:click=move |_| v.stop_control()>"Annuler la demande"</button>
                        }.into_any()
                    } else {
                        view! {
                            <button class="k-btn k-btn--sm app-push" aria-label=request_label.clone() title=request_label.clone()
                                on:click=move |_| v.request_control(user)>
                                <Icon icon=lu::LuMousePointer2 small=true/><span>"Demander le contrôle"</span>
                            </button>
                        }.into_any()
                    }
                }
                <button class="k-btn k-btn--icon" aria-label="Plein écran" title="Plein écran" on:click=full>
                    <Icon icon=lu::LuMaximize/>
                </button>
                <button class="k-btn k-btn--icon" aria-label="Arrêter de regarder" title="Arrêter de regarder" on:click=move |_| v.watch(None)>
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
        <button class=if big { "k-callbtn" } else { "k-btn k-btn--icon" } aria-label="Couper le micro"
            title=move || if off() { "Réactiver le micro" } else { "Couper le micro" }
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
        <button class=if big { "k-callbtn" } else { "k-btn k-btn--icon" } aria-label="Couper le son"
            title=move || if deaf() { "Réactiver le son" } else { "Couper le son" }
            aria-pressed=move || deaf().to_string() on:click=move |_| v.toggle_deaf()>
            {move || if deaf() { view! { <Icon icon=lu::LuHeadphoneOff/> } } else { view! { <Icon icon=lu::LuHeadphones/> } }}
        </button>
    }
}

/// `private` : appel de MP ou de groupe (« Raccrocher » plutôt que « Quitter »).
#[component]
fn CallBar(private: bool) -> impl IntoView {
    let v = store().voice;
    let share_ok = voice::can_share();
    let why = if voice::devices().is_none() {
        "Le partage d'écran demande l'instance en HTTPS (ou ouverte via localhost)."
    } else {
        "Ce navigateur ne permet pas de partager l'écran."
    };
    let share_label = move || if v.streaming.get() { "Arrêter le partage" } else { "Partager ton écran" };
    view! {
        <div class="k-callbar" role="group" aria-label="Commandes de l'appel">
            <MicToggle big=true/>
            <DeafToggle big=true/>
            <button class="k-callbtn" data-on=move || v.streaming.get().then_some("")
                disabled=move || !share_ok || matches!(v.status.get(), Status::Lost(_) | Status::Off)
                aria-label=share_label title=share_label aria-describedby=(!share_ok).then_some("call-share-why")
                on:click=move |_| if v.streaming.get_untracked() { v.stop_share() } else { v.share() }>
                {move || if v.streaming.get() { view! { <Icon icon=lu::LuScreenShareOff/> } } else { view! { <Icon icon=lu::LuScreenShare/> } }}
                <span>{share_label}</span>
            </button>
            {if private {
                view! {
                    <button class="k-callbtn k-callbtn--danger" aria-label="Raccrocher" title="Raccrocher" on:click=move |_| v.leave()>
                        <Icon icon=lu::LuPhoneOff/><span>"Raccrocher"</span>
                    </button>
                }.into_any()
            } else {
                view! {
                    <button class="k-callbtn k-callbtn--danger" aria-label="Quitter" title="Quitter le salon vocal" on:click=move |_| v.leave()>
                        <Icon icon=lu::LuPhoneOff/><span>"Quitter"</span>
                    </button>
                }.into_any()
            }}
        </div>
        {(!share_ok).then(|| view! { <p class="app-hint app-call-why" id="call-share-why">{why}</p> })}
    }
}

/// Sous un salon vocal de la barre latérale : qui y est.
#[component]
pub fn VoiceUsers(guild: Option<Id>, channel: Id, name: String, hidden: Signal<bool>) -> impl IntoView {
    let s = store();
    let who = Memo::new(move |_| present(s, guild, channel));
    view! {
        <Show when=move || who.with(|w| !w.is_empty())>
            <ul class="app-voice-users" aria-label=format!("Dans {name}") hidden=hidden>
                <For each=move || who.get() key=|x| (x.user_id, x.self_mute, x.self_deaf, x.streaming) let:x>
                    {
                        let (u, n) = member(s, guild, x.user_id);
                        let user = x.user_id;
                        view! {
                            <li data-speaking=move || s.voice.speaking.with(|x| x.contains(&user)).then_some("")>
                                {face(u, &n, "sm")}
                                <span class="app-ellipsis">{n.clone()}</span>
                                {(x.self_mute && !x.self_deaf).then(|| view! {
                                    <span role="img" aria-label="micro coupé" title="Micro coupé"><Icon icon=lu::LuMicOff small=true/></span>
                                })}
                                {x.self_deaf.then(|| view! {
                                    <span role="img" aria-label="son coupé" title="Son coupé"><Icon icon=lu::LuHeadphoneOff small=true/></span>
                                })}
                                {x.streaming.then(|| view! { <span class="k-badge k-badge--live">"En direct"</span> })}
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
        let (href, where_) = match r.guild {
            Some(gid) => {
                let g = s.guild(gid)?;
                let c = g.channels.iter().find(|c| c.id == r.channel).and_then(|c| c.name.clone()).unwrap_or_default();
                (format!("/channels/{gid}/{}", r.channel), format!("{c} / {}", g.guild.name))
            }
            None => {
                let title = s.channel(r.channel).map(|c| crate::social::private_title(&c)).unwrap_or_default();
                (format!("/channels/@me/{}", r.channel), format!("Appel avec {title}"))
            }
        };
        Some((r, href, where_))
    });
    let state = move || match v.status.get() {
        Status::Connected => ("on", "En appel"),
        Status::Connecting => ("wait", "Connexion…"),
        Status::Unstable => ("wait", "Connexion instable"),
        Status::Lost(_) | Status::Off => ("lost", "Connexion perdue"),
    };
    view! {
        {move || place.get().map(|(r, href, where_)| {
            let leave = if r.guild.is_some() { "Quitter le salon vocal" } else { "Raccrocher" };
            view! {
                <section class="k-voicebar" aria-label="Connexion vocale">
                    <div>
                        <b data-state=move || state().0 role="status">{move || state().1}</b>
                        <a class="app-ellipsis" href=href title=where_.clone()>{where_.clone()}</a>
                    </div>
                    <MicToggle/>
                    <DeafToggle/>
                    <Show when=move || matches!(v.status.get(), Status::Lost(_))>
                        <button class="k-btn k-btn--icon" aria-label="Reconnecter" title="Reconnecter" on:click=move |_| v.join(r.guild, r.channel)>
                            <Icon icon=lu::LuRefreshCw/>
                        </button>
                    </Show>
                    <button class="k-btn k-btn--icon" aria-label=leave title=leave on:click=move |_| v.leave()>
                        <Icon icon=lu::LuPhoneOff/>
                    </button>
                </section>
            }
        })}
    }
}

/// Le son des micros reçus : un `<audio>` caché par personne, coupé avec le son.
#[component]
pub fn CallAudio() -> impl IntoView {
    let v = store().voice;
    let mics =
        Memo::new(move |_| v.remote.with(|r| r.iter().filter(|x| !x.screen).map(|x| x.user).collect::<Vec<_>>()));
    view! {
        <div hidden>
            <For each=move || mics.get() key=|u| *u let:u>
                <RemoteAudio user=u/>
            </For>
        </div>
    }
}

#[component]
fn RemoteAudio(user: Id) -> impl IntoView {
    let v = store().voice;
    let el = NodeRef::<html::Audio>::new();
    Effect::new(move || {
        let Some(e) = el.get() else { return };
        v.remote.with(|r| attach(&e, r.iter().find(|x| x.user == user && !x.screen).map(|x| &x.stream)));
    });
    view! { <audio node_ref=el autoplay prop:muted=move || v.deaf.get()></audio> }
}

/// Le contrôle à distance de mon écran, visible où que je sois pendant l'appel : qui le demande
/// (accepter ou refuser), qui l'a (« Arrêter le contrôle »), et ce qui l'a empêché.
#[component]
pub fn ControlPanel() -> impl IntoView {
    let s = store();
    let v = s.voice;
    let name = move |u: Id| v.room.get_untracked().map_or_else(|| "Quelqu'un".to_string(), |r| member(s, r.guild, u).1);
    let shown = move || {
        v.controller.get().is_some() || v.requests.with(|r| !r.is_empty()) || v.share_note.with(Option::is_some)
    };
    view! {
        <Show when=shown>
            <section class="app-remote" aria-label="Contrôle à distance de ton écran">
                {move || v.controller.get().map(|u| view! {
                    <div class="app-remote-row" data-on="">
                        <Icon icon=lu::LuMousePointer2/>
                        <p role="status">{format!("{} contrôle ton écran.", name(u))}</p>
                        <button class="k-btn k-btn--sm k-btn--danger" on:click=move |_| v.release_control(true)>
                            <Icon icon=lu::LuMousePointerBan small=true/><span>"Arrêter le contrôle"</span>
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
                                    {format!("{who} demande à contrôler ton écran (souris et clavier).")}
                                    <Show when=waiting>
                                        <span class="app-remote-wait">" Réponds dans la fenêtre de KRAKEN qui vient de s'ouvrir."</span>
                                    </Show>
                                </p>
                                <button class="k-btn k-btn--sm k-btn--primary" disabled=move || v.confirming.get().is_some()
                                    on:click=move |_| v.accept_control(u, who_accept.clone())>"Accepter"</button>
                                <button class="k-btn k-btn--sm" disabled=waiting on:click=move |_| v.decline_control(u)>"Refuser"</button>
                            </div>
                        }
                    }
                </For>
                {move || v.share_note.get().map(|(u, note)| {
                    let who = name(u);
                    let text = match note {
                        ShareNote::Desktop => format!(
                            "{who} a demandé à contrôler ton écran. Le contrôle à distance n'est possible que depuis l'appli de bureau KRAKEN : partage ton écran depuis l'appli pour l'accepter."),
                        ShareNote::Surface => format!(
                            "{who} a demandé à contrôler ton écran, mais tu partages une fenêtre ou un onglet. Partage ton écran entier pour pouvoir l'accepter."),
                        ShareNote::Message(m) => m,
                    };
                    view! {
                        <div class="app-remote-row">
                            <Icon icon=lu::LuCircleAlert/>
                            <p role="status">{text}</p>
                            <button class="k-btn k-btn--icon" aria-label="Masquer ce message" title="Masquer" on:click=move |_| v.share_note.set(None)>
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
        [rest @ .., last] => format!("{} et {last}", rest.join(", ")),
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
            <section class="app-dmcall" aria-label="Appel">
                {move || if here.get() {
                    view! { <Call guild=None channel=channel/> }.into_any()
                } else if who.with(|w| !w.is_empty()) {
                    view! {
                        <div class="app-dmcall-lobby">
                            <span class="app-call-icon" aria-hidden="true"><Icon icon=lu::LuPhoneCall/></span>
                            <p class="app-hint" role="status">"Appel en cours"</p>
                            <ul class="app-call-who" aria-label="Dans l'appel">
                                <For each=move || who.get() key=|x| x.user_id let:x>
                                    {
                                        let (u, n) = member(s, None, x.user_id);
                                        view! { <li>{face(u, &n, "sm")}<span class="app-ellipsis">{n.clone()}</span></li> }
                                    }
                                </For>
                            </ul>
                            <button class="k-btn k-btn--primary" disabled=!voice::supported() on:click=move |_| v.join(None, channel)>
                                <Icon icon=lu::LuPhone/>"Rejoindre l'appel"
                            </button>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }}
                <Show when=move || here.get() && ringing.with(|r| !r.is_empty())>
                    <p class="app-hint app-dmcall-ringing" role="status">
                        <Icon icon=lu::LuBellRing small=true/>{move || format!("Ça sonne chez {}…", names(&ringing.get()))}
                    </p>
                </Show>
                {move || error().map(|e| view! { <p class="k-error app-dmcall-error" role="alert">{e}</p> })}
            </section>
        </Show>
    }
}

/// Dans l'en-tête d'un MP ou d'un groupe sans appel en cours : lancer l'appel.
#[component]
pub fn StartCall(channel: Id, #[prop(into)] blocked: Signal<bool>) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let idle =
        Memo::new(move |_| v.room.get().is_none_or(|r| r.channel != channel) && present(s, None, channel).is_empty());
    let supported = voice::supported();
    view! {
        <Show when=move || idle.get()>
            <button class="k-btn k-btn--icon" aria-label="Démarrer un appel" title="Démarrer un appel"
                disabled=move || blocked.get() || !supported on:click=move |_| v.join(None, channel)>
                <Icon icon=lu::LuPhoneCall/>
            </button>
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
    Effect::new(move || ring(!incoming.with(Vec::is_empty) && !v.deaf.get()));
    on_cleanup(|| ring(false));
    view! {
        <Show when=move || incoming.with(|c| !c.is_empty())>
            <section class="app-ring" aria-label="Appels entrants">
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
        Some(g) => format!("t'appelle dans {g}"),
        None => "t'appelle".to_string(),
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
        <div class="app-ring-card" role="alertdialog" aria-label=format!("Appel de {who}") aria-describedby=format!("ring-{cid}")>
            {face(account, &who, "md")}
            <p id=format!("ring-{cid}")><b class="app-ellipsis">{who.clone()}</b><span>{line}</span></p>
            <button class="k-btn k-btn--sm k-btn--primary" disabled=!voice::supported() on:click=answer>
                <Icon icon=lu::LuPhone small=true/><span>"Rejoindre"</span>
            </button>
            <button class="k-btn k-btn--sm k-btn--danger" on:click=decline>
                <Icon icon=lu::LuPhoneOff small=true/><span>"Refuser"</span>
            </button>
        </div>
    }
}
