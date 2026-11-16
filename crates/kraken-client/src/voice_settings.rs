//! Réglages › Voix et vidéo (S10) : le micro, la sortie audio et la caméra à utiliser, un test du micro,
//! l'aperçu de la caméra, les trois traitements du son et « Appuyer pour parler ». Tout reste dans le
//! `localStorage` de l'appareil (jamais envoyé au serveur) et la page marche sans stockage : le choix tient
//! alors jusqu'à la fermeture de l'onglet.
//!
//! Clés : `kraken.mic`, `kraken.speaker`, `kraken.cam` (identifiants d'appareil ; vide ou absent : celui
//! du système, et un appareil débranché retombe sur celui-là sans rien dire), `kraken.echo`,
//! `kraken.noise`, `kraken.agc` (`on` par défaut, `off`), `kraken.ptt` (`off` par défaut, `on`),
//! `kraken.ptt_key` (un `KeyboardEvent.code`) et `kraken.ptt_label` (son nom, tel que la personne le lit
//! sur son clavier : « A » pour la touche que `code` appelle `KeyQ` sur un clavier AZERTY).
//!
//! Le micro et la caméra choisis servent à la prochaine prise ; pendant un appel, un changement remplace
//! la piste en direct (`Voice::switch_mic`, `Voice::switch_camera`). La sortie audio se pose sur tous les
//! éléments `<audio>` de l'appel (`apply_sink`). Appuyer pour parler (`init_ptt`) écoute le clavier de la
//! fenêtre tant qu'un appel est en cours : la piste du micro suit la touche (`Voice::mic_open`).

use std::cell::{Cell, RefCell};

use gloo_timers::future::TimeoutFuture;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioContext, HtmlMediaElement, KeyboardEvent, MediaDeviceInfo, MediaDeviceKind, MediaStream};

use crate::call;
use crate::icons::{Icon, lu};
use crate::store::store;
use crate::voice::{self, Voice};

pub const MIC: &str = "kraken.mic";
pub const SPEAKER: &str = "kraken.speaker";
pub const CAM: &str = "kraken.cam";
pub const ECHO: &str = "kraken.echo";
pub const NOISE: &str = "kraken.noise";
pub const AGC: &str = "kraken.agc";
pub const PTT: &str = "kraken.ptt";
const PTT_KEY: &str = "kraken.ptt_key";
const PTT_LABEL: &str = "kraken.ptt_label";

const DEFAULT: &str = "Par défaut du système";
const NEEDS_HTTPS: &str =
    "Le micro et la caméra demandent l'instance en HTTPS (ou ouverte sur cet ordinateur, via localhost).";
const TAB_REFUSED: &str = "La touche Tab sert à naviguer dans la page : choisis-en une autre.";
const ENTER_REFUSED: &str = "La touche Entrée seule ne peut pas servir : choisis-en une autre.";
/// Le micro se referme un peu après le relâchement : la fin d'un mot n'est pas coupée.
const RELEASE_MS: u32 = 200;
/// L'annonce vocale attend que l'état se pose : des appuis rapprochés ne la font pas bavarder.
const SAY_MS: u32 = 400;

// ───── stockage ─────

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Ce que le stockage garde sous `key` (rien si absent, vide, ou si le stockage est refusé).
pub fn stored(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten().filter(|v| !v.is_empty())
}

/// Garde `value` sous `key` (`None` ou vide : l'oublie). Sans stockage, ne fait rien.
pub fn remember(key: &str, value: Option<&str>) {
    let Some(s) = storage() else { return };
    let _ = match value.filter(|v| !v.is_empty()) {
        Some(v) => s.set_item(key, v),
        None => s.remove_item(key),
    };
}

/// Un interrupteur gardé en `on` ou `off` ; `default` tant qu'il n'a pas été touché.
pub fn flag(key: &str, default: bool) -> bool {
    match stored(key).as_deref() {
        Some("on") => true,
        Some("off") => false,
        _ => default,
    }
}

fn remember_flag(key: &str, on: bool) {
    remember(key, Some(if on { "on" } else { "off" }));
}

// ───── appareils ─────

#[derive(Clone, PartialEq)]
struct Device {
    id: String,
    label: String,
}

#[derive(Clone, Default, PartialEq)]
struct Devices {
    inputs: Vec<Device>,
    outputs: Vec<Device>,
    cameras: Vec<Device>,
    /// Le navigateur donne leur nom aux appareils (il le fait après une autorisation).
    named: bool,
    /// La première lecture des appareils est finie : avant, on ne sait pas encore s'ils ont un nom.
    loaded: bool,
}

/// Les appareils que le navigateur connaît. Sans autorisation, il cache les noms (et parfois les
/// identifiants) : les appareils sans nom s'appellent « Micro 1 », « Micro 2 »… Les entrées virtuelles
/// `default` et `communications` sont laissées : « Par défaut du système » les remplace.
async fn enumerate() -> Devices {
    let mut out = Devices { loaded: true, ..Devices::default() };
    let Some(d) = voice::devices() else { return out };
    let Ok(promise) = d.enumerate_devices() else { return out };
    let Ok(list) = JsFuture::from(promise).await else { return out };
    for item in js_sys::Array::from(&list).iter() {
        let Ok(info) = item.dyn_into::<MediaDeviceInfo>() else { continue };
        let (id, label) = (info.device_id(), info.label());
        if id.is_empty() || id == "default" || id == "communications" {
            continue;
        }
        out.named |= !label.is_empty();
        let (bucket, name) = match info.kind() {
            MediaDeviceKind::Audioinput => (&mut out.inputs, "Micro"),
            MediaDeviceKind::Audiooutput => (&mut out.outputs, "Sortie"),
            MediaDeviceKind::Videoinput => (&mut out.cameras, "Caméra"),
            _ => continue,
        };
        let label = if label.is_empty() { format!("{name} {}", bucket.len() + 1) } else { label };
        bucket.push(Device { id, label });
    }
    out
}

/// Une seule demande d'accès au micro et à la caméra, pour que le navigateur donne leur nom aux appareils.
/// Les pistes obtenues sont arrêtées aussitôt. L'erreur dit ce qu'il faut à la personne.
async fn allow_access() -> Result<(), &'static str> {
    let Some(d) = voice::devices() else { return Err(NEEDS_HTTPS) };
    for constraints in [r#"{"audio":true,"video":true}"#, r#"{"audio":true}"#, r#"{"video":true}"#] {
        let Ok(wanted) = js_sys::JSON::parse(constraints) else { continue };
        let Ok(promise) = d.get_user_media_with_constraints(wanted.unchecked_ref()) else { continue };
        match JsFuture::from(promise).await {
            Ok(stream) => {
                voice::stop_all(&stream.unchecked_into::<MediaStream>());
                return Ok(());
            }
            Err(e) if voice::error_name(&e).as_deref() == Some("NotAllowedError") => {
                return Err("Le navigateur a refusé l'accès : les appareils gardent un nom générique.");
            }
            Err(_) => {}
        }
    }
    Err("Aucun micro ni aucune caméra n'a été trouvé sur cet appareil.")
}

/// Ce que le micro refusé ou absent dit à la personne.
fn mic_failure(e: &wasm_bindgen::JsValue) -> &'static str {
    match voice::error_name(e).as_deref() {
        Some("NotFoundError" | "OverconstrainedError" | "DevicesNotFoundError") => {
            "Aucun micro n'a été trouvé sur cet appareil."
        }
        Some("NotReadableError" | "AbortError" | "TrackStartError") => {
            "Le micro n'a pas pu démarrer : une autre application l'utilise peut-être."
        }
        _ => "Le navigateur a refusé l'accès au micro",
    }
}

/// Le navigateur sait choisir la sortie audio d'un élément (`setSinkId` : pas Firefox ni Safari).
pub fn can_sink() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.create_element("audio").ok())
        .is_some_and(|el| js_sys::Reflect::has(&el, &"setSinkId".into()).unwrap_or(false))
}

/// Envoie le son de `el` sur la sortie `id` (vide : celle du système). Si l'appareil a disparu, retombe
/// sur celle du système sans rien dire.
pub fn apply_sink(el: &HtmlMediaElement, id: &str) {
    let Some(set) =
        js_sys::Reflect::get(el, &"setSinkId".into()).ok().and_then(|f| f.dyn_into::<js_sys::Function>().ok())
    else {
        return;
    };
    let now = js_sys::Reflect::get(el, &"sinkId".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
    if now == id {
        return;
    }
    let Some(done) = set.call1(el, &id.into()).ok().and_then(|p| p.dyn_into::<js_sys::Promise>().ok()) else { return };
    if id.is_empty() {
        return;
    }
    let el = el.clone();
    spawn_local(async move {
        if JsFuture::from(done).await.is_err() {
            let _ = set.call1(&el, &"".into());
        }
    });
}

// ───── la page ─────

/// Le niveau (0 à 100) d'un bloc d'échantillons : de -60 dB (rien) à 0 dB (saturé).
fn percent(samples: &[f32]) -> u32 {
    let rms = (samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32).sqrt();
    if rms <= 0.0 {
        return 0;
    }
    (((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0) * 100.0).round() as u32
}

/// Un test du micro en cours : le flux et son contexte audio, à fermer ensemble.
struct Probe {
    stream: MediaStream,
    ctx: AudioContext,
}

/// Une liste d'appareils : « Par défaut du système », puis ce que le navigateur connaît. Un choix gardé
/// mais introuvable s'affiche comme le défaut (il revient tout seul si l'appareil est rebranché).
#[component]
fn DeviceSelect(
    id: &'static str,
    label: &'static str,
    options: Signal<Vec<Device>>,
    current: RwSignal<String>,
    #[prop(into)] on_pick: Callback<String>,
) -> impl IntoView {
    let shown = Memo::new(move |_| {
        let id = current.get();
        if options.with(|o| o.iter().any(|d| d.id == id)) { id } else { String::new() }
    });
    view! {
        <div class="k-field app-device-select">
            <label class="k-label" for=id>{label}</label>
            <select class="k-select" id=id on:change=move |e| {
                let picked = event_target_value(&e);
                current.set(picked.clone());
                on_pick.run(picked);
            }>
                <option value="" prop:selected=move || shown.get().is_empty()>{DEFAULT}</option>
                <For each=move || options.get() key=|d| (d.id.clone(), d.label.clone()) let:d>
                    {
                        let id = d.id.clone();
                        view! { <option value=d.id prop:selected=move || shown.get() == id>{d.label}</option> }
                    }
                </For>
            </select>
        </div>
    }
}

/// Une ligne de réglage à interrupteur : nom, interrupteur, explication.
#[component]
fn Switch(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    on: RwSignal<bool>,
    #[prop(into)] on_change: Callback<bool>,
) -> impl IntoView {
    let described = format!("{id}-h");
    view! {
        <div class="k-setting">
            <label for=id>{label}</label>
            <input id=id type="checkbox" role="switch" class="k-switch" aria-describedby=described.clone()
                prop:checked=move || on.get()
                on:change=move |e| {
                    let now = event_target_checked(&e);
                    on.set(now);
                    on_change.run(now);
                }/>
            <p id=described>{hint}</p>
        </div>
    }
}

/// L'aperçu de la caméra : l'image retournée comme un miroir, sans son.
#[component]
fn CameraPreview(stream: RwSignal<Option<MediaStream>, LocalStorage>) -> impl IntoView {
    let el = NodeRef::<html::Video>::new();
    Effect::new(move || {
        let Some(e) = el.get() else { return };
        stream.with(|s| call::attach(&e, s.as_ref()));
    });
    view! {
        <div class="app-cam">
            <video class="k-tile-video" data-mirror node_ref=el autoplay playsinline muted prop:muted=true
                aria-label="Image de ta caméra"></video>
        </div>
    }
}

/// Réglages › Voix et vidéo (S10).
#[component]
pub fn VoiceSettings() -> impl IntoView {
    let v = store().voice;
    let secure = voice::devices().is_some();
    let sink = can_sink();

    // Les appareils, relus quand on en branche ou débranche un (`devicechange`).
    let list = RwSignal::new(Devices::default());
    let refresh = move || {
        spawn_local(async move {
            let found = enumerate().await;
            list.try_set(found);
        });
    };
    refresh();
    let on_change = StoredValue::new_local(Closure::<dyn FnMut()>::new(refresh));
    if let Some(d) = voice::devices() {
        on_change.with_value(|f| {
            let _ = d.add_event_listener_with_callback("devicechange", f.as_ref().unchecked_ref());
        });
    }
    on_cleanup(move || {
        if let Some(d) = voice::devices() {
            on_change.try_with_value(|f| {
                let _ = d.remove_event_listener_with_callback("devicechange", f.as_ref().unchecked_ref());
            });
        }
    });
    let inputs = Signal::derive(move || list.with(|l| l.inputs.clone()));
    let outputs = Signal::derive(move || list.with(|l| l.outputs.clone()));
    let cameras = Signal::derive(move || list.with(|l| l.cameras.clone()));
    let asking = RwSignal::new(false);
    let access_error = RwSignal::new(None::<&'static str>);
    let allow = move |_| {
        asking.set(true);
        access_error.set(None);
        spawn_local(async move {
            if let Err(why) = allow_access().await {
                access_error.try_set(Some(why));
            }
            refresh();
            asking.try_set(false);
        });
    };

    let mic = RwSignal::new(stored(MIC).unwrap_or_default());
    let speaker = RwSignal::new(v.speaker.get_untracked());
    let cam = RwSignal::new(stored(CAM).unwrap_or_default());

    // Test du micro : un flux à part, jamais rejoué dans les haut-parleurs, arrêté en quittant la page.
    let testing = RwSignal::new(false);
    let level = RwSignal::new(0u32);
    let test_error = RwSignal::new(None::<&'static str>);
    let probe = StoredValue::<Option<Probe>, LocalStorage>::new_local(None);
    let test_gen = StoredValue::new(0u64);
    let stop_test = move || {
        test_gen.try_update_value(|g| *g += 1);
        probe.try_update_value(|p| {
            if let Some(p) = p.take() {
                voice::stop_all(&p.stream);
                let _ = p.ctx.close();
            }
        });
        testing.try_set(false);
        level.try_set(0);
    };
    let start_test = move || {
        let Some(d) = voice::devices() else { return };
        stop_test();
        test_error.set(None);
        testing.set(true);
        let mine = test_gen.get_value();
        let still = move || test_gen.try_get_value() == Some(mine);
        spawn_local(async move {
            let stream = match voice::microphone(&d).await {
                Ok(s) => s,
                Err(e) => {
                    if still() {
                        test_error.try_set(Some(mic_failure(&e)));
                        testing.try_set(false);
                    }
                    return;
                }
            };
            if !still() {
                voice::stop_all(&stream);
                return;
            }
            let Ok(ctx) = AudioContext::new() else {
                voice::stop_all(&stream);
                testing.try_set(false);
                return;
            };
            let (Ok(source), Ok(analyser)) = (ctx.create_media_stream_source(&stream), ctx.create_analyser()) else {
                voice::stop_all(&stream);
                let _ = ctx.close();
                testing.try_set(false);
                return;
            };
            analyser.set_fft_size(1024);
            let _ = source.connect_with_audio_node(&analyser);
            let _ = ctx.resume();
            probe.try_set_value(Some(Probe { stream, ctx }));
            let mut samples = vec![0f32; 1024];
            while still() {
                TimeoutFuture::new(70).await;
                if !still() {
                    break;
                }
                analyser.get_float_time_domain_data(&mut samples);
                // Monte d'un coup, redescend doucement : une voix par saccades reste lisible.
                let Some(before) = level.try_get_untracked() else { break };
                let now = percent(&samples).max(before * 7 / 8);
                if now != before {
                    level.try_set(now);
                }
            }
            drop(source);
        });
    };
    on_cleanup(stop_test);

    // Aperçu de la caméra : seulement sur demande, retourné comme un miroir, arrêté en quittant la page.
    let previewing = RwSignal::new(false);
    let preview = RwSignal::<Option<MediaStream>, LocalStorage>::new_local(None);
    let preview_error = RwSignal::new(None::<&'static str>);
    let preview_gen = StoredValue::new(0u64);
    let stop_preview = move || {
        preview_gen.try_update_value(|g| *g += 1);
        if let Some(Some(s)) = preview.try_update(Option::take) {
            voice::stop_all(&s);
        }
        previewing.try_set(false);
    };
    let start_preview = move || {
        let Some(d) = voice::devices() else { return };
        stop_preview();
        preview_error.set(None);
        previewing.set(true);
        let mine = preview_gen.get_value();
        spawn_local(async move {
            let result = voice::camera(&d).await;
            if preview_gen.try_get_value() != Some(mine) {
                if let Ok(s) = result {
                    voice::stop_all(&s);
                }
                return;
            }
            match result {
                Ok(s) => {
                    preview.try_set(Some(s));
                }
                Err(e) => {
                    preview_error.try_set(Some(voice::camera_failure(&e)));
                    previewing.try_set(false);
                }
            }
        });
    };
    on_cleanup(stop_preview);

    let pick_mic = move |id: String| {
        remember(MIC, Some(&id));
        v.switch_mic();
        if testing.get_untracked() {
            start_test();
        }
    };
    let pick_speaker = move |id: String| {
        remember(SPEAKER, Some(&id));
        v.speaker.set(id);
    };
    let pick_cam = move |id: String| {
        remember(CAM, Some(&id));
        v.switch_camera();
        if previewing.get_untracked() {
            start_preview();
        }
    };

    // Traitements du son : chacun s'applique à la prochaine prise du micro, et tout de suite en appel.
    let processing = |key: &'static str| {
        let on = RwSignal::new(flag(key, true));
        let apply = Callback::new(move |now: bool| {
            remember_flag(key, now);
            v.switch_mic();
            if testing.get_untracked() {
                start_test();
            }
        });
        (on, apply)
    };
    let (echo, set_echo) = processing(ECHO);
    let (noise, set_noise) = processing(NOISE);
    let (agc, set_agc) = processing(AGC);

    // Appuyer pour parler.
    let ptt = v.ptt;
    let capturing = RwSignal::new(false);
    let key_error = RwSignal::new(None::<&'static str>);
    let captured_at = StoredValue::new(0f64);
    let toggle_ptt = move |on: bool| remember_flag(PTT, on);
    let end_capture = move |error: Option<&'static str>| {
        capturing.set(false);
        key_error.set(error);
    };
    let on_key = move |e: KeyboardEvent| {
        if !capturing.get_untracked() {
            return;
        }
        // Rien de ce qui est tapé pendant le choix ne doit sortir du champ (Échap fermerait les réglages).
        e.prevent_default();
        e.stop_propagation();
        match e.code().as_str() {
            "Escape" => end_capture(None),
            "Tab" => key_error.set(Some(TAB_REFUSED)),
            "Enter" | "NumpadEnter" => key_error.set(Some(ENTER_REFUSED)),
            "" | "Unidentified" => {}
            code => {
                let key = PttKey::from_event(code, &e.key());
                remember(PTT_KEY, Some(&key.code));
                remember(PTT_LABEL, Some(&key.label));
                v.ptt_key.set(Some(key));
                captured_at.set_value(js_sys::Date::now());
                end_capture(None);
            }
        }
    };
    let on_key_up = move |e: KeyboardEvent| {
        // Espace relâché sur le bouton le « clique » : ce clic-là n'est pas un nouveau choix.
        if capturing.get_untracked() || js_sys::Date::now() - captured_at.get_value() < 400.0 {
            e.prevent_default();
        }
    };
    let on_key_click = move |_| {
        if js_sys::Date::now() - captured_at.get_value() < 400.0 {
            return;
        }
        if capturing.get_untracked() {
            end_capture(None);
        } else {
            key_error.set(None);
            capturing.set(true);
        }
    };

    view! {
        <div class="app-prefs">
            <p class="app-hint">
                "Ces réglages restent sur cet appareil. Un changement s'applique tout de suite, même pendant un appel."
            </p>
            {(!secure).then(|| view! { <p class="app-hint">{NEEDS_HTTPS}</p> })}

            <section class="app-choice-group" aria-labelledby="vv-devices-t">
                <h3 id="vv-devices-t" class="app-prefs-title">"Appareils"</h3>
                {move || (secure && list.with(|l| l.loaded && !l.named)).then(|| view! {
                    <div class="app-choice-group">
                        <p id="vv-access-h" class="app-hint">
                            "Le navigateur n'affiche le nom de tes appareils qu'une fois l'accès au micro et à la caméra autorisé."
                        </p>
                        <button type="button" class="k-btn k-btn--sm app-device-btn" aria-describedby="vv-access-h"
                            disabled=move || asking.get() on:click=allow>"Autoriser l'accès"</button>
                        {move || access_error.get().map(|e| view! { <p class="k-error" role="alert">{e}</p> })}
                    </div>
                })}
                <DeviceSelect id="vv-mic" label="Entrée audio" options=inputs current=mic on_pick=pick_mic/>
                {if sink {
                    view! {
                        <DeviceSelect id="vv-speaker" label="Sortie audio" options=outputs current=speaker on_pick=pick_speaker/>
                    }.into_any()
                } else {
                    view! {
                        <p class="app-hint">
                            "Ton navigateur ne permet pas de choisir la sortie audio : le son sort sur l'appareil par défaut du système (Chrome et Edge savent le faire)."
                        </p>
                    }.into_any()
                }}
                <DeviceSelect id="vv-cam" label="Caméra" options=cameras current=cam on_pick=pick_cam/>
            </section>

            <section class="app-choice-group" aria-labelledby="vv-test-t">
                <h3 id="vv-test-t" class="app-prefs-title">"Test du micro"</h3>
                <p id="vv-test-h" class="app-hint">"Parle : la barre monte avec ta voix. Rien n'est enregistré ni rejoué dans tes haut-parleurs."</p>
                <button type="button" class="k-btn app-device-btn" aria-describedby="vv-test-h" disabled=!secure
                    on:click=move |_| if testing.get_untracked() { stop_test() } else { start_test() }>
                    <Icon icon=lu::LuMic/>
                    {move || if testing.get() { "Arrêter le test" } else { "Tester le micro" }}
                </button>
                <div class="app-meter-row">
                    <span id="vv-meter-l" class="k-label">"Niveau du micro"</span>
                    <div class="app-meter" role="meter" aria-labelledby="vv-meter-l" aria-valuemin="0" aria-valuemax="100"
                        aria-valuenow=move || level.get().to_string()>
                        <span style:width=move || format!("{}%", level.get())></span>
                    </div>
                </div>
                {move || test_error.get().map(|e| view! { <p class="k-error" role="alert">{e}</p> })}
            </section>

            <section class="app-choice-group" aria-labelledby="vv-cam-t">
                <h3 id="vv-cam-t" class="app-prefs-title">"Aperçu de la caméra"</h3>
                <p id="vv-cam-h" class="app-hint">"La caméra ne s'allume que pendant l'aperçu, et s'éteint quand tu quittes cette page."</p>
                <button type="button" class="k-btn app-device-btn" aria-describedby="vv-cam-h" disabled=!secure
                    on:click=move |_| if previewing.get_untracked() { stop_preview() } else { start_preview() }>
                    <Icon icon=lu::LuVideo/>
                    {move || if previewing.get() { "Arrêter l'aperçu" } else { "Aperçu de la caméra" }}
                </button>
                {move || preview.with(Option::is_some).then(|| view! { <CameraPreview stream=preview/> })}
                {move || preview_error.get().map(|e| view! { <p class="k-error" role="alert">{e}</p> })}
            </section>

            <section aria-labelledby="vv-sound-t">
                <h3 id="vv-sound-t" class="app-prefs-title">"Traitement du son"</h3>
                <Switch id="vv-echo" label="Annulation d'écho" on=echo on_change=set_echo
                    hint="Retire de ton micro le son qui sort de tes haut-parleurs."/>
                <Switch id="vv-noise" label="Suppression du bruit" on=noise on_change=set_noise
                    hint="Atténue les bruits de fond : clavier, ventilateur, rue."/>
                <Switch id="vv-agc" label="Réglage automatique du volume" on=agc on_change=set_agc
                    hint="Garde ta voix à un niveau régulier, que tu parles fort ou doucement."/>
            </section>

            <section aria-labelledby="vv-ptt-t">
                <h3 id="vv-ptt-t" class="app-prefs-title">"Appuyer pour parler"</h3>
                <Switch id="vv-ptt" label="Appuyer pour parler" on=ptt on_change=toggle_ptt
                    hint="Ton micro ne s'ouvre que tant que tu tiens la touche choisie, et se referme un instant après (0,2 s). Ça ne marche que lorsque la fenêtre est au premier plan : un raccourci global viendra avec l'appli de bureau."/>
                <Show when=move || ptt.get()>
                    <div class="k-setting app-ptt-key">
                        <b id="vv-ptt-key-l">"Touche pour parler"</b>
                        <p id="vv-ptt-key-h" role="status">
                            {move || if capturing.get() {
                                "Appuie sur une touche… Échap annule"
                            } else if v.ptt_key.with(Option::is_none) {
                                "Aucune touche choisie : le micro reste ouvert comme d'habitude."
                            } else {
                                "Clique pour changer de touche."
                            }}
                        </p>
                        <button type="button" class="k-btn" id="vv-ptt-key" data-no-shortcuts aria-labelledby="vv-ptt-key-l vv-ptt-key"
                            aria-describedby=move || key_error.get().map(|_| "vv-ptt-key-err")
                            aria-invalid=move || key_error.get().map(|_| "true")
                            data-capturing=move || capturing.get().then_some("")
                            on:click=on_key_click on:keydown=on_key on:keyup=on_key_up on:blur=move |_| capturing.set(false)>
                            {move || if capturing.get() {
                                "En attente…".into_any()
                            } else if let Some(key) = v.ptt_key.get() {
                                view! { <span class="k-kbd"><kbd>{key.label}</kbd></span> }.into_any()
                            } else {
                                "Choisir une touche".into_any()
                            }}
                        </button>
                        {move || key_error.get().map(|e| view! { <p id="vv-ptt-key-err" class="k-error app-ptt-err" role="alert">{e}</p> })}
                    </div>
                </Show>
            </section>
        </div>
    }
}

// ───── appuyer pour parler ─────

/// La touche choisie : son `code` (la touche physique, quelle que soit la disposition) et le nom qu'on lui lit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PttKey {
    pub code: String,
    pub label: String,
}

impl PttKey {
    /// `code` et `key` d'un `KeyboardEvent` : le nom vient du caractère que la touche donne sur ce clavier
    /// (« A » sur un clavier AZERTY pour `KeyQ`), sinon de son `code`.
    fn from_event(code: &str, key: &str) -> Self {
        let mut chars = key.chars();
        let label = match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_whitespace() && !c.is_control() => c.to_uppercase().collect(),
            _ => key_label(code),
        };
        Self { code: code.into(), label }
    }
}

/// La touche gardée dans le stockage, si une l'est.
pub fn stored_key() -> Option<PttKey> {
    let code = stored(PTT_KEY)?;
    let label = stored(PTT_LABEL).unwrap_or_else(|| key_label(&code));
    Some(PttKey { code, label })
}

/// Le nom d'une touche d'après son `code`, pour celles qui n'écrivent pas de caractère.
fn key_label(code: &str) -> String {
    if let Some(letter) = code.strip_prefix("Key").filter(|l| l.len() == 1) {
        return letter.into();
    }
    if let Some(digit) = code.strip_prefix("Digit").filter(|d| d.len() == 1) {
        return digit.into();
    }
    if let Some(digit) = code.strip_prefix("Numpad").filter(|d| d.len() == 1) {
        return format!("Pavé {digit}");
    }
    match code {
        "Space" => "Espace",
        "ControlLeft" => "Ctrl gauche",
        "ControlRight" => "Ctrl droite",
        "ShiftLeft" => "Maj gauche",
        "ShiftRight" => "Maj droite",
        "AltLeft" => "Alt",
        "AltRight" => "Alt Gr",
        "CapsLock" => "Verr. maj.",
        "Backspace" => "Retour arrière",
        "ArrowUp" => "Flèche haut",
        "ArrowDown" => "Flèche bas",
        "ArrowLeft" => "Flèche gauche",
        "ArrowRight" => "Flèche droite",
        "Backquote" => "`",
        "Minus" => "-",
        "Equal" => "=",
        other => other,
    }
    .into()
}

thread_local! {
    /// Les écouteurs du clavier de la fenêtre, tant qu'appuyer pour parler est armé pendant un appel.
    static LISTENERS: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    /// Numéro du dernier appui ou relâchement : un relâchement différé ne vaut plus s'il y en a eu un autre.
    static RELEASE: Cell<u64> = const { Cell::new(0) };
    static SAY: Cell<u64> = const { Cell::new(0) };
}

fn is_modifier(code: &str) -> bool {
    ["Control", "Shift", "Alt", "Meta", "OS"].iter().any(|m| code.starts_with(m))
}

/// L'événement vient d'un champ de saisie : la touche y sert à écrire, pas à parler.
fn typing(e: &KeyboardEvent) -> bool {
    let Some(target) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) else { return false };
    matches!(target.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") || target.is_content_editable()
}

fn is_key(v: Voice, e: &KeyboardEvent) -> bool {
    v.ptt_key.with_untracked(|k| k.as_ref().is_some_and(|k| k.code == e.code()))
}

fn press(v: Voice) {
    RELEASE.set(RELEASE.get() + 1);
    if !v.ptt_held.get_untracked() {
        v.ptt_held.set(true);
    }
}

/// La touche est relâchée : le micro se ferme après `delay` ms, sauf si elle est reprise entre-temps.
fn release(v: Voice, delay: u32) {
    let n = RELEASE.get() + 1;
    RELEASE.set(n);
    if !v.ptt_held.get_untracked() {
        return;
    }
    if delay == 0 {
        v.ptt_held.set(false);
        return;
    }
    spawn_local(async move {
        TimeoutFuture::new(delay).await;
        if RELEASE.get() == n && v.ptt_held.get_untracked() {
            v.ptt_held.set(false);
        }
    });
}

/// Écoute le clavier de la fenêtre : la touche tenue ouvre le micro. Une touche qui reste enfoncée quand la
/// fenêtre perd le focus ou se cache ne doit pas rester coincée : on la relâche tout de suite.
fn listen(v: Voice) -> Vec<Box<dyn FnOnce()>> {
    let down = window_event_listener(leptos::ev::keydown, move |e: KeyboardEvent| {
        let code = e.code();
        let chord = !is_modifier(&code) && (e.ctrl_key() || e.meta_key() || e.alt_key());
        if !is_key(v, &e) || chord || e.is_composing() || typing(&e) {
            return;
        }
        // La touche choisie ne fait plus rien d'autre (défiler, cliquer le bouton qui a le focus).
        e.prevent_default();
        if !e.repeat() {
            press(v);
        }
    });
    let up = window_event_listener(leptos::ev::keyup, move |e: KeyboardEvent| {
        if !is_key(v, &e) {
            return;
        }
        if !typing(&e) {
            e.prevent_default();
        }
        release(v, RELEASE_MS);
    });
    let blur = window_event_listener(leptos::ev::blur, move |_| release(v, 0));
    let hidden = window_event_listener(leptos::ev::visibilitychange, move |_| {
        if document().hidden() {
            release(v, 0);
        }
    });
    vec![
        Box::new(move || down.remove()),
        Box::new(move || up.remove()),
        Box::new(move || blur.remove()),
        Box::new(move || hidden.remove()),
    ]
}

/// Branche appuyer pour parler sur la session : les écouteurs du clavier n'existent que pendant un appel
/// où le mode est armé (réglage actif et touche choisie), et l'annonce vocale suit la touche.
pub fn init_ptt(v: Voice) {
    Effect::new(move || {
        let armed = v.room.with(Option::is_some) && v.ptt_armed();
        let listening = LISTENERS.with_borrow(|l| !l.is_empty());
        if armed && !listening {
            LISTENERS.set(listen(v));
        } else if !armed {
            LISTENERS.take().into_iter().for_each(|remove| remove());
            RELEASE.set(RELEASE.get() + 1);
            if v.ptt_held.get_untracked() {
                v.ptt_held.set(false);
            }
        }
    });
    Effect::new(move |before: Option<bool>| {
        let held = v.ptt_held.get();
        let n = SAY.get() + 1;
        SAY.set(n);
        if before.is_some() {
            spawn_local(async move {
                TimeoutFuture::new(SAY_MS).await;
                if SAY.get() != n {
                    return;
                }
                let muted = v.mute.get_untracked() || v.deaf.get_untracked();
                v.ptt_say.set(match (held, muted) {
                    (false, _) => "Micro fermé",
                    (true, true) => "Micro coupé",
                    (true, false) => "Micro ouvert",
                });
            });
        }
        held
    });
}
