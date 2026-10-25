//! Salon vocal (S18) : avant d'entrer, qui est là et « Rejoindre » ; ensuite les tuiles des
//! participants, l'écran regardé en grand et la barre de commandes. Plus ce que la coquille montre
//! de l'appel : les participants sous chaque salon vocal, le panneau « Vocal connecté », les
//! boutons micro et son, et le son des micros reçus.

use kraken_proto::{Channel, Id, User, VoiceState};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen_futures::JsFuture;

use crate::icons::{Icon, lu};
use crate::store::{Store, store};
use crate::ui::{Avatar, Header, initials};
use crate::voice::{self, Status};

/// Qui est dans ce salon vocal, dans l'ordre d'arrivée.
fn present(s: Store, guild: Id, channel: Id) -> Vec<VoiceState> {
    s.guilds.with(|gs| {
        gs.iter()
            .find(|g| g.guild.id == guild)
            .map(|g| g.voice_states.iter().filter(|v| v.channel_id == Some(channel)).cloned().collect())
            .unwrap_or_default()
    })
}

/// Le compte d'un membre et son nom sur ce serveur (surnom, sinon nom affiché).
fn member(s: Store, guild: Id, user: Id) -> (Option<User>, String) {
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
    let (guild, cid) = (channel.guild_id.unwrap_or_default(), channel.id);
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
fn Lobby(guild: Id, channel: Id, name: String) -> impl IntoView {
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
fn Call(guild: Id, channel: Id) -> impl IntoView {
    let s = store();
    let v = s.voice;
    let who = Memo::new(move |_| present(s, guild, channel).into_iter().map(|x| x.user_id).collect::<Vec<_>>());
    view! {
        <div class="app-call-main">
            {move || match v.status.get() {
                Status::Connecting => Some(view! {
                    <div class="k-banner" role="status"><Icon icon=lu::LuLoaderCircle/><span>"Connexion au salon vocal…"</span></div>
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
            {move || v.watching.get().map(|u| view! { <Stage guild=guild user=u/> })}
            <ul class="app-tiles" aria-label="Participants">
                <For each=move || who.get() key=|u| *u let:u>
                    <Tile guild=guild channel=channel user=u/>
                </For>
            </ul>
        </div>
        <CallBar/>
    }
}

/// Un participant : avatar (ou l'aperçu de mon écran), nom, micro et son coupés, « En direct ».
#[component]
fn Tile(guild: Id, channel: Id, user: Id) -> impl IntoView {
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

/// L'écran regardé, en grand ; son son passe, sauf si le mien est coupé.
#[component]
fn Stage(guild: Id, user: Id) -> impl IntoView {
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
    let full = move |_| {
        let d = document();
        if d.fullscreen_element().is_some() {
            d.exit_fullscreen();
        } else if let Some(f) = frame.get_untracked() {
            let _ = f.request_fullscreen();
        }
    };
    view! {
        <figure class="app-stage" node_ref=frame>
            <video node_ref=el autoplay playsinline prop:muted=move || v.deaf.get()
                aria-label=format!("Écran partagé par {name}") on:playing=move |_| playing.set(true)></video>
            <Show when=move || !playing.get()>
                <p class="app-stage-wait" role="status">"Chargement de l'écran…"</p>
            </Show>
            <figcaption>
                <span class="k-badge k-badge--live">"En direct"</span>
                <b class="app-ellipsis">{name.clone()}</b>
                <button class="k-btn k-btn--icon app-push" aria-label="Plein écran" title="Plein écran" on:click=full>
                    <Icon icon=lu::LuMaximize/>
                </button>
                <button class="k-btn k-btn--icon" aria-label="Arrêter de regarder" title="Arrêter de regarder" on:click=move |_| v.watch(None)>
                    <Icon icon=lu::LuX/>
                </button>
            </figcaption>
        </figure>
    }
}

/// Micro : pressé = coupé. Dans la barre de l'appel (`big`) ou le panneau utilisateur.
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

#[component]
fn CallBar() -> impl IntoView {
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
            <button class="k-callbtn k-callbtn--danger" aria-label="Quitter" title="Quitter le salon vocal" on:click=move |_| v.leave()>
                <Icon icon=lu::LuPhoneOff/><span>"Quitter"</span>
            </button>
        </div>
        {(!share_ok).then(|| view! { <p class="app-hint app-call-why" id="call-share-why">{why}</p> })}
    }
}

/// Sous un salon vocal de la barre latérale : qui y est.
#[component]
pub fn VoiceUsers(guild: Id, channel: Id, name: String, hidden: Signal<bool>) -> impl IntoView {
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

/// Au-dessus du panneau utilisateur pendant un appel : où, l'état, et quitter.
#[component]
pub fn VoicePanel() -> impl IntoView {
    let s = store();
    let v = s.voice;
    let place = Memo::new(move |_| {
        let r = v.room.get()?;
        let g = s.guild(r.guild)?;
        let c = g.channels.iter().find(|c| c.id == r.channel).and_then(|c| c.name.clone()).unwrap_or_default();
        Some((r, format!("{c} / {}", g.guild.name)))
    });
    let state = move || match v.status.get() {
        Status::Connected => ("on", "Vocal connecté"),
        Status::Connecting => ("wait", "Connexion…"),
        Status::Unstable => ("wait", "Connexion instable"),
        Status::Lost(_) | Status::Off => ("lost", "Connexion perdue"),
    };
    view! {
        {move || place.get().map(|(r, where_)| view! {
            <section class="k-voicebar" aria-label="Connexion vocale">
                <div>
                    <b data-state=move || state().0 role="status">{move || state().1}</b>
                    <a class="app-ellipsis" href=format!("/channels/{}/{}", r.guild, r.channel) title=where_.clone()>{where_.clone()}</a>
                </div>
                <Show when=move || matches!(v.status.get(), Status::Lost(_))>
                    <button class="k-btn k-btn--icon" aria-label="Reconnecter" title="Reconnecter" on:click=move |_| v.join(r.guild, r.channel)>
                        <Icon icon=lu::LuRefreshCw/>
                    </button>
                </Show>
                <button class="k-btn k-btn--icon" aria-label="Quitter le salon vocal" title="Quitter" on:click=move |_| v.leave()>
                    <Icon icon=lu::LuPhoneOff/>
                </button>
            </section>
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
