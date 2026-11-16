//! Raccourcis clavier. Un seul tableau (`SHORTCUTS`) dit chaque raccourci : son libellé, son groupe, ses
//! touches. Il sert à les brancher (`lookup`, un seul écouteur sur la fenêtre) et à les afficher (la fenêtre
//! `?` ou Ctrl+/ et Réglages › Raccourcis clavier), donc l'aide ne peut pas dériver de ce qui marche.
//!
//! Règles : un raccourci à modificateur marche partout, même dans un champ ; une touche seule (`?`) jamais
//! pendant qu'on écrit. Rien n'est capté pendant le choix de la touche d'appuyer pour parler
//! (`data-no-shortcuts`), ni pendant une composition de texte. Ctrl+T, N et W sont au navigateur : aucun
//! raccourci ne les prend. Les raccourcis ne se personnalisent pas, et aucun ne marche hors de la fenêtre.

use kraken_core::grid::cycle;
use kraken_proto::{Channel, ChannelKind, Id};
use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::shell::ordered;
use crate::store::{Store, store};
use crate::ui::{modal, modal_open, trap_tab};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Navigation,
    Messages,
    Call,
    General,
}

impl Group {
    const ALL: [Group; 4] = [Self::Navigation, Self::Messages, Self::Call, Self::General];

    fn title(self) -> &'static str {
        match self {
            Self::Navigation => "Navigation",
            Self::Messages => "Messages",
            Self::Call => "Appel",
            Self::General => "Général",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Navigation => "navigation",
            Self::Messages => "messages",
            Self::Call => "appel",
            Self::General => "general",
        }
    }
}

/// Ce qu'un raccourci branché fait.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Switcher,
    ChannelPrev,
    ChannelNext,
    ServerPrev,
    ServerNext,
    UnreadPrev,
    UnreadNext,
    Help,
    Mute,
    Deafen,
}

/// Une combinaison. `key` : `KeyboardEvent.key` en minuscules. `primary` : Ctrl, ou Cmd sur Mac.
#[derive(Clone, Copy, Debug)]
pub struct Chord {
    pub primary: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: &'static str,
}

const C: u8 = 1;
const A: u8 = 2;
const S: u8 = 4;

const fn chord(mods: u8, key: &'static str) -> Chord {
    Chord { primary: mods & C != 0, alt: mods & A != 0, shift: mods & S != 0, key }
}

#[derive(Clone, Copy, Debug)]
pub struct Shortcut {
    pub label: &'static str,
    pub group: Group,
    /// Une ou plusieurs façons de le faire.
    pub chords: &'static [Chord],
    /// `None` : un raccourci du champ de saisie ou d'une fenêtre, géré là où il s'applique, listé ici pour l'aide.
    pub action: Option<Action>,
}

const fn sc(label: &'static str, group: Group, chords: &'static [Chord], action: Option<Action>) -> Shortcut {
    Shortcut { label, group, chords, action }
}

pub const SHORTCUTS: &[Shortcut] = &[
    sc("Aller vers un salon, une personne ou un serveur", Group::Navigation, &[chord(C, "k")], Some(Action::Switcher)),
    sc("Salon précédent", Group::Navigation, &[chord(A, "arrowup")], Some(Action::ChannelPrev)),
    sc("Salon suivant", Group::Navigation, &[chord(A, "arrowdown")], Some(Action::ChannelNext)),
    sc("Serveur précédent", Group::Navigation, &[chord(C | A, "arrowup")], Some(Action::ServerPrev)),
    sc("Serveur suivant", Group::Navigation, &[chord(C | A, "arrowdown")], Some(Action::ServerNext)),
    sc("Salon non lu précédent", Group::Navigation, &[chord(A | S, "arrowup")], Some(Action::UnreadPrev)),
    sc("Salon non lu suivant", Group::Navigation, &[chord(A | S, "arrowdown")], Some(Action::UnreadNext)),
    sc("Envoyer le message", Group::Messages, &[chord(0, "enter")], None),
    sc("Aller à la ligne", Group::Messages, &[chord(S, "enter")], None),
    sc("Modifier mon dernier message (saisie vide)", Group::Messages, &[chord(0, "arrowup")], None),
    sc("Annuler la réponse ou la modification", Group::Messages, &[chord(0, "escape")], None),
    sc("Couper ou réactiver le micro", Group::Call, &[chord(C | S, "m")], Some(Action::Mute)),
    sc("Couper ou réactiver le son", Group::Call, &[chord(C | S, "d")], Some(Action::Deafen)),
    sc("Afficher les raccourcis clavier", Group::General, &[chord(C, "/"), chord(0, "?")], Some(Action::Help)),
    sc("Fermer la fenêtre ouverte", Group::General, &[chord(0, "escape")], None),
];

/// Ce que l'aide dit des raccourcis, partout où elle les liste.
pub const NOTE: &str = "Lecture seule : ces raccourcis ne se changent pas encore, et aucun ne marche hors de la fenêtre de KRAKEN (pas de raccourci global pour l'instant).";

/// L'appareil est un Mac (ou un iPhone, un iPad) : Cmd plutôt que Ctrl, symboles ⌘ ⌥ ⇧.
pub fn is_mac() -> bool {
    thread_local! {
        static MAC: std::cell::OnceCell<bool> = const { std::cell::OnceCell::new() };
    }
    MAC.with(|m| {
        *m.get_or_init(|| {
            let Some(w) = web_sys::window() else { return false };
            let n = w.navigator();
            let platform = n.platform().unwrap_or_default();
            let agent = n.user_agent().unwrap_or_default();
            ["Mac", "iPhone", "iPad"].iter().any(|p| platform.contains(p)) || agent.contains("Mac OS")
        })
    })
}

/// Les touches d'une combinaison, chacune avec ce que dit un lecteur d'écran quand le symbole ne se lit pas.
fn parts(c: &Chord, mac: bool) -> Vec<(String, Option<&'static str>)> {
    let mut out = Vec::new();
    if c.primary {
        out.push(if mac { ("⌘".to_string(), Some("Commande")) } else { ("Ctrl".to_string(), None) });
    }
    if c.alt {
        out.push(if mac { ("⌥".to_string(), Some("Option")) } else { ("Alt".to_string(), None) });
    }
    if c.shift {
        out.push(if mac { ("⇧".to_string(), Some("Majuscule")) } else { ("Maj".to_string(), Some("Majuscule")) });
    }
    out.push(match c.key {
        "arrowup" => ("↑".to_string(), Some("flèche haut")),
        "arrowdown" => ("↓".to_string(), Some("flèche bas")),
        "escape" => ("Échap".to_string(), None),
        "enter" => ("Entrée".to_string(), None),
        other => (other.to_uppercase(), None),
    });
    out
}

fn chord_view(c: &Chord, mac: bool) -> impl IntoView + use<> {
    let keys = parts(c, mac)
        .into_iter()
        .map(|(shown, spoken)| match spoken {
            Some(said) => {
                view! { <kbd><span aria-hidden="true">{shown}</span><span class="k-sr">{said}</span></kbd> }.into_any()
            }
            None => view! { <kbd>{shown}</kbd> }.into_any(),
        })
        .collect_view();
    view! { <span class="k-kbd">{keys}</span> }
}

/// Les touches d'un raccourci, pour le bouton du Quick Switcher et les infobulles : « Ctrl K », « ⌘ K ».
pub fn keys_view(action: Action) -> impl IntoView {
    let mac = is_mac();
    SHORTCUTS.iter().find(|s| s.action == Some(action)).and_then(|s| s.chords.first()).map(|c| chord_view(c, mac))
}

/// Les raccourcis, groupe par groupe, en listes de définitions : le libellé à gauche, les touches à droite.
/// `prefix` rend les identifiants uniques quand la liste est à l'écran deux fois (réglages et fenêtre).
#[component]
pub fn ShortcutList(prefix: &'static str) -> impl IntoView {
    let mac = is_mac();
    Group::ALL
        .iter()
        .map(|&g| {
            let (labelled, own) = (format!("keys-{prefix}-{}", g.slug()), format!("keys-{prefix}-{}", g.slug()));
            let rows = SHORTCUTS
                .iter()
                .filter(|s| s.group == g)
                .map(|s| {
                    let chords = s
                        .chords
                        .iter()
                        .enumerate()
                        .map(|(i, c)| view! { {(i > 0).then(|| view! { <span class="app-keys-or">"ou"</span> })}{chord_view(c, mac)} })
                        .collect_view();
                    view! { <div class="k-shortcut"><dt>{s.label}</dt><dd>{chords}</dd></div> }
                })
                .collect_view();
            view! {
                <section class="app-keys" aria-labelledby=labelled>
                    <h3 class="app-keys-title" id=own>{g.title()}</h3>
                    <dl>{rows}</dl>
                </section>
            }
        })
        .collect_view()
}

/// Réglages › Raccourcis clavier.
#[component]
pub fn KeybindSettings() -> impl IntoView {
    view! {
        <div class="app-prefs">
            <p class="k-hint">{NOTE}</p>
            <ShortcutList prefix="settings"/>
        </div>
    }
}

/// La fenêtre d'aide (`?` ou Ctrl/Cmd+/).
#[component]
pub fn ShortcutHelp(close: Callback<()>) -> impl IntoView {
    let root = NodeRef::new();
    modal(root, move || close.run(()));
    view! {
        <div class="k-scrim" node_ref=root on:click=move |_| close.run(()) on:keydown=move |e| trap_tab(&e, root)>
            <div class="k-dialog app-help" role="dialog" aria-modal="true" aria-labelledby="help-t" on:click=|e| e.stop_propagation()>
                <header><h2 id="help-t">"Raccourcis clavier"</h2></header>
                <div>
                    <p class="k-hint">{NOTE}</p>
                    <ShortcutList prefix="help"/>
                </div>
                <footer><button class="k-btn k-btn--primary" on:click=move |_| close.run(()) autofocus>"Fermer"</button></footer>
            </div>
        </div>
    }
}

/// Les fenêtres que les raccourcis ouvrent, et ce que l'annonce vocale dit.
#[derive(Clone, Copy)]
pub struct Ui {
    pub switcher: RwSignal<bool>,
    pub help: RwSignal<bool>,
    pub say: RwSignal<String>,
}

pub fn ui() -> Ui {
    expect_context()
}

// ───────── branchement ─────────

fn chord_matches(c: &Chord, e: &KeyboardEvent, mac: bool) -> bool {
    if e.key().to_lowercase() != c.key {
        return false;
    }
    let primary = if mac { e.meta_key() && !e.ctrl_key() } else { e.ctrl_key() && !e.meta_key() };
    let none = !e.ctrl_key() && !e.meta_key();
    if (c.primary && !primary) || (!c.primary && !none) || c.alt != e.alt_key() {
        return false;
    }
    // Un symbole (`?`, `/`) s'écrit avec Maj sur bien des claviers (AZERTY) : Maj ne compte pas pour lui.
    let symbol = c.key.chars().all(|ch| !ch.is_alphanumeric()) && c.key.chars().count() == 1;
    symbol || c.shift == e.shift_key()
}

/// Le raccourci branché que cet événement déclenche.
fn lookup(e: &KeyboardEvent, mac: bool) -> Option<Action> {
    SHORTCUTS
        .iter()
        .filter(|s| s.action.is_some())
        .find(|s| s.chords.iter().any(|c| chord_matches(c, e, mac)))
        .and_then(|s| s.action)
}

fn target_of(e: &KeyboardEvent) -> Option<web_sys::HtmlElement> {
    e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
}

/// L'événement vient d'un champ de saisie : la touche y sert à écrire.
fn typing(e: &KeyboardEvent) -> bool {
    target_of(e)
        .is_some_and(|t| matches!(t.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") || t.is_content_editable())
}

/// Un endroit qui garde toutes les touches (le choix de la touche d'appuyer pour parler).
fn reserved(e: &KeyboardEvent) -> bool {
    target_of(e).is_some_and(|t| t.closest("[data-no-shortcuts]").ok().flatten().is_some())
}

/// Les salons de la barre latérale, dans son ordre, sans les catégories ; ceux de mes MP sur l'accueil.
fn channels(s: Store, guild: Option<Id>) -> Vec<Channel> {
    untrack(|| match guild {
        Some(g) => s
            .guild(g)
            .map(|g| ordered(&g.channels).into_iter().filter(|c| c.kind != ChannelKind::Category).collect())
            .unwrap_or_default(),
        None => s.dms.get_untracked(),
    })
}

/// Où mène le salon précédent ou suivant (`unread` : seulement ceux qui ont du non-lu ou une mention).
fn channel_step(s: Store, ids: (Option<Id>, Option<Id>), forward: bool, unread: bool) -> Option<String> {
    let (guild, current) = ids;
    let list = channels(s, guild);
    let at = list.iter().position(|c| Some(c.id) == current);
    if !unread {
        return cycle(list.len(), at, forward).map(|i| crate::unread::href(&list[i]));
    }
    let wanted = |c: &&Channel| untrack(|| s.shows_unread(c) || s.mentions(c) > 0);
    let found = if forward {
        list.iter().skip(at.map_or(0, |i| i + 1)).find(wanted)
    } else {
        list.iter().take(at.unwrap_or(list.len())).rev().find(wanted)
    };
    found.map(crate::unread::href)
}

/// Le serveur précédent ou suivant dans le rail (depuis l'accueil : le premier ou le dernier).
fn server_step(s: Store, guild: Option<Id>, forward: bool) -> Option<String> {
    let list: Vec<Id> = s.guilds.with_untracked(|g| g.iter().map(|g| g.guild.id).collect());
    let at = guild.and_then(|g| list.iter().position(|x| *x == g));
    cycle(list.len(), at, forward).map(|i| crate::switcher::guild_href(s, list[i]))
}

/// Micro ou son coupé ou rétabli ; la nouvelle valeur est annoncée.
fn toggle_call(s: Store, ui: Ui, deafen: bool) {
    let v = s.voice;
    if deafen {
        v.toggle_deaf();
    } else {
        v.toggle_mute();
    }
    let (mute, deaf) = (v.mute.get_untracked(), v.deaf.get_untracked());
    ui.say.set(
        match (deafen, mute || deaf, deaf) {
            (true, _, true) => "Son coupé, micro coupé",
            (true, _, false) => "Son réactivé",
            (false, true, _) => "Micro coupé",
            (false, false, _) => "Micro réactivé",
        }
        .to_string(),
    );
}

/// Branche tous les raccourcis sur la fenêtre. À appeler dans la coquille, sous le routeur.
pub fn install(ids: Memo<(Option<Id>, Option<Id>)>) -> Ui {
    let s = store();
    let ui = Ui { switcher: RwSignal::new(false), help: RwSignal::new(false), say: RwSignal::new(String::new()) };
    provide_context(ui);
    let mac = is_mac();
    let listener = window_event_listener(leptos::ev::keydown, move |e: KeyboardEvent| {
        if e.is_composing() || e.default_prevented() || reserved(&e) {
            return;
        }
        let Some(action) = lookup(&e, mac) else { return };
        // Une touche seule n'est jamais prise pendant qu'on écrit, ni quand c'est celle d'appuyer pour parler.
        let v = s.voice;
        let ptt = v.ptt.get_untracked() && v.ptt_key.with_untracked(|k| k.as_ref().is_some_and(|k| k.code == e.code()));
        if !(e.ctrl_key() || e.meta_key() || e.alt_key()) && (typing(&e) || ptt) {
            return;
        }
        let once = !e.repeat();
        let handled = match action {
            Action::Switcher => {
                if once {
                    ui.help.set(false);
                    ui.switcher.update(|o| *o = !*o);
                }
                true
            }
            Action::Help => {
                if once {
                    ui.switcher.set(false);
                    ui.help.update(|o| *o = !*o);
                }
                true
            }
            Action::Mute | Action::Deafen => {
                let in_call = s.voice.room.with_untracked(Option::is_some);
                if in_call && once {
                    toggle_call(s, ui, action == Action::Deafen);
                }
                in_call
            }
            // Une fenêtre est ouverte : on ne change pas de salon derrière elle.
            _ if modal_open() => false,
            _ => {
                let to = match action {
                    Action::ChannelPrev | Action::ChannelNext => {
                        channel_step(s, ids.get_untracked(), action == Action::ChannelNext, false)
                    }
                    Action::UnreadPrev | Action::UnreadNext => {
                        channel_step(s, ids.get_untracked(), action == Action::UnreadNext, true)
                    }
                    _ => server_step(s, ids.get_untracked().0, action == Action::ServerNext),
                };
                to.map(|to| s.goto.set(Some(to))).is_some()
            }
        };
        if handled {
            e.prevent_default();
        }
    });
    on_cleanup(move || listener.remove());
    ui
}
