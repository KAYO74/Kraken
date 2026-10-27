//! Non-lus, mentions et notifications (S02, S09, S25) : ce que le store en sait, la lecture
//! automatique du salon affiché (regroupée avant d'aller au serveur), les notifications sur le
//! bureau, le compteur dans le titre de l'onglet, le menu « Marquer comme lu » et la boîte
//! « Notifications » d'un serveur ou d'un salon.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use gloo_timers::future::TimeoutFuture;
use kraken_core::notify::{self, Level, Seen};
use kraken_proto::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::icons::{Icon, lu};
use crate::store::{Store, store};
use crate::ui::{modal, when};

/// Attente avant d'envoyer les lectures au serveur : plusieurs messages lus d'affilée (ou
/// plusieurs salons parcourus) partent ensemble, un acquittement par salon.
const ACK_DELAY_MS: u32 = 800;

/// Notifications sur le bureau voulues sur cet appareil (la permission est celle du navigateur).
const DESKTOP_KEY: &str = "kraken.desktop_notifications";

thread_local! {
    static QUEUE: RefCell<HashMap<Id, Id>> = RefCell::new(HashMap::new());
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

fn level_of(l: NotifyLevel) -> Option<Level> {
    match l {
        NotifyLevel::Default => None,
        NotifyLevel::All => Some(Level::All),
        NotifyLevel::Mentions => Some(Level::Mentions),
        NotifyLevel::Nothing => Some(Level::Nothing),
    }
}

impl Store {
    /// Mon réglage pour ce serveur (suivi).
    pub fn guild_setting(&self, gid: Id) -> Option<NotifySetting> {
        self.notify.with(|n| n.iter().find(|x| x.channel_id.is_none() && x.guild_id == Some(gid)).cloned())
    }

    /// Mon réglage pour ce salon (suivi).
    pub fn channel_setting(&self, cid: Id) -> Option<NotifySetting> {
        self.notify.with(|n| n.iter().find(|x| x.channel_id == Some(cid)).cloned())
    }

    fn is_muted(&self, s: Option<&NotifySetting>) -> bool {
        notify::muted(s.and_then(|s| s.muted_until), self.now.get())
    }

    /// Serveur en sourdine (suivi).
    pub fn guild_muted(&self, gid: Id) -> bool {
        self.is_muted(self.guild_setting(gid).as_ref())
    }

    /// Salon en sourdine, lui ou son serveur (suivi).
    pub fn channel_muted(&self, c: &Channel) -> bool {
        self.is_muted(self.channel_setting(c.id).as_ref()) || c.guild_id.is_some_and(|g| self.guild_muted(g))
    }

    /// Le niveau qui s'applique à ce salon (suivi).
    pub fn level(&self, c: &Channel) -> Level {
        let ch = self.channel_setting(c.id).and_then(|s| level_of(s.level));
        let g = c.guild_id.and_then(|g| self.guild_setting(g)).and_then(|s| level_of(s.level));
        notify::effective_level(c.guild_id.is_none(), ch, g)
    }

    /// Des messages que je n'ai pas lus (suivi).
    pub fn unread(&self, cid: Id) -> bool {
        let Some(last) = self.last.with(|l| l.get(&cid).copied()) else { return false };
        self.reads.with(|r| r.get(&cid).and_then(|x| x.last_read)).is_none_or(|read| read < last)
    }

    /// Non lu et pas en sourdine : le salon s'affiche en gras (suivi).
    pub fn shows_unread(&self, c: &Channel) -> bool {
        c.kind.is_textual() && self.unread(c.id) && !self.channel_muted(c)
    }

    /// Mentions en attente dans ce salon ; rien au niveau « rien » (suivi).
    pub fn mentions(&self, c: &Channel) -> u32 {
        let n = self.reads.with(|r| r.get(&c.id).map_or(0, |x| x.mention_count));
        if n == 0 || self.level(c) == Level::Nothing { 0 } else { n }
    }

    /// Un serveur a du non-lu (hors salons et serveur en sourdine), et ses mentions (suivi).
    pub fn guild_badge(&self, gid: Id) -> (bool, u32) {
        let channels = self.guilds.with(|gs| gs.iter().find(|g| g.guild.id == gid).map(|g| g.channels.clone()));
        let channels = channels.unwrap_or_default();
        let muted = self.guild_muted(gid);
        let unread = !muted && channels.iter().any(|c| self.shows_unread(c));
        (unread, channels.iter().map(|c| self.mentions(c)).sum())
    }

    /// Mentions en attente dans mes MP et groupes (suivi).
    pub fn private_mentions(&self) -> u32 {
        self.dms.with(|d| d.iter().map(|c| self.mentions(c)).sum())
    }

    /// Toutes mes mentions en attente : le titre de l'onglet (suivi).
    pub fn total_mentions(&self) -> u32 {
        let guilds: Vec<Id> = self.guilds.with(|gs| gs.iter().map(|g| g.guild.id).collect());
        guilds.into_iter().map(|g| self.guild_badge(g).1).sum::<u32>() + self.private_mentions()
    }

    /// Retient le dernier message de ces salons.
    pub(crate) fn note_last(&self, channels: &[Channel]) {
        let found: Vec<(Id, Id)> = channels.iter().filter_map(|c| Some((c.id, c.last_message_id?))).collect();
        if found.is_empty() {
            return;
        }
        self.last.update(|l| {
            for (c, m) in found {
                let e = l.entry(c).or_insert(m);
                *e = (*e).max(m);
            }
        });
    }

    /// Le dernier message d'un salon a été supprimé : le précédent connu prend sa place.
    pub(crate) fn forget_last(&self, cid: Id, id: Id) {
        if self.last.with_untracked(|l| l.get(&cid) != Some(&id)) {
            return;
        }
        let prev = self.logs.with_untracked(|l| l.get(&cid).and_then(|x| x.items.last().map(|m| m.id)));
        self.last.update(|l| match prev {
            Some(p) => {
                l.insert(cid, p);
            }
            None => {
                l.remove(&cid);
            }
        });
    }

    /// État de lecture reçu du serveur ; une lecture plus avancée faite ici en attendant reste.
    pub(crate) fn merge_read(&self, r: ReadState) {
        self.reads.update(|m| {
            let ahead =
                m.get(&r.channel_id).and_then(|x| x.last_read).is_some_and(|l| r.last_read.is_none_or(|n| l > n));
            if !ahead {
                m.insert(r.channel_id, r);
            }
        });
    }

    pub(crate) fn upsert_notify(&self, n: NotifySetting) {
        self.notify.update(|list| {
            let same = |x: &NotifySetting| match n.channel_id {
                Some(c) => x.channel_id == Some(c),
                None => x.channel_id.is_none() && x.guild_id == n.guild_id,
            };
            match list.iter_mut().find(|x| same(x)) {
                Some(x) => *x = n,
                None => list.push(n),
            }
        });
    }

    /// Un message arrive : dernier message du salon, lecture (le mien, ou le salon affiché dans
    /// une fenêtre active), sinon compteur de mentions et notification sur le bureau.
    pub(crate) fn arrived(&self, m: &Message) {
        let cid = m.channel_id;
        if self.last.with_untracked(|l| l.get(&cid).is_some_and(|x| *x >= m.id)) {
            return; // déjà vu (renvoi après reconnexion)
        }
        self.last.update(|l| {
            l.insert(cid, m.id);
        });
        // Une conversation active remonte en tête de la liste des MP.
        if self.dms.with_untracked(|d| d.iter().position(|c| c.id == cid).is_some_and(|i| i > 0)) {
            self.dms.update(|d| {
                if let Some(i) = d.iter().position(|c| c.id == cid) {
                    let c = d.remove(i);
                    d.insert(0, c);
                }
            });
        }
        let Some(me) = self.my_id() else { return };
        let author = m.author.as_ref().map(|a| a.id);
        if author == Some(me) {
            self.reads.update(|r| {
                r.insert(cid, ReadState { channel_id: cid, last_read: Some(m.id), mention_count: 0 });
            });
            return;
        }
        if self.viewing.get_untracked() == Some(cid) && self.focused.get_untracked() {
            ack(*self, cid, m.id);
            return;
        }
        let Some(c) = untrack(|| self.channel(cid)) else { return };
        let (counts, notifies) = untrack(|| {
            let ignore = c.guild_id.and_then(|g| self.guild_setting(g)).is_some_and(|s| s.suppress_everyone);
            let mentions: Vec<Id> = m.mentions.iter().map(|u| u.id).collect();
            let seen = Seen {
                author,
                mentions: &mentions,
                everyone: m.mention_everyone,
                reply_to: m.reply.as_ref().and_then(|r| r.author.as_ref()).map(|u| u.id),
                private: c.guild_id.is_none(),
            };
            let mention = notify::is_mention(me, &seen, ignore) && !author.is_some_and(|a| self.is_blocked(a));
            let level = self.level(&c);
            let blocked = author.is_some_and(|a| self.is_blocked(a));
            (notify::counts(level, mention), !blocked && notify::notifies(level, self.channel_muted(&c), mention))
        });
        if counts {
            self.reads.update(|r| {
                let e = r.entry(cid).or_insert(ReadState { channel_id: cid, last_read: None, mention_count: 0 });
                e.mention_count += 1;
            });
        }
        if notifies && !self.focused.get_untracked() {
            desktop_notification(*self, &c, m);
        }
    }
}

/// J'ai lu ce salon jusqu'à `mid` : tout de suite ici, au serveur un peu après (regroupé).
pub fn ack(s: Store, cid: Id, mid: Id) {
    let done = s
        .reads
        .with_untracked(|r| r.get(&cid).is_some_and(|x| x.mention_count == 0 && x.last_read.is_some_and(|l| l >= mid)));
    if done {
        return;
    }
    s.reads.update(|r| {
        let e = r.entry(cid).or_insert(ReadState { channel_id: cid, last_read: None, mention_count: 0 });
        e.last_read = Some(e.last_read.map_or(mid, |l| l.max(mid)));
        e.mention_count = 0;
    });
    QUEUE.with_borrow_mut(|q| {
        let e = q.entry(cid).or_insert(mid);
        *e = (*e).max(mid);
    });
    if ARMED.replace(true) {
        return;
    }
    spawn_local(async move {
        TimeoutFuture::new(ACK_DELAY_MS).await;
        ARMED.set(false);
        let batch: Vec<(Id, Id)> = QUEUE.with_borrow_mut(|q| q.drain().collect());
        for (c, m) in batch {
            if let Ok(r) = crate::api::ack(c, m).await {
                s.merge_read(r);
            }
        }
    });
}

/// Tout ce salon lu, jusqu'à son dernier message.
pub fn mark_channel_read(s: Store, cid: Id) {
    if let Some(last) = s.last.with_untracked(|l| l.get(&cid).copied()) {
        ack(s, cid, last);
    }
}

/// Tout le serveur lu.
pub fn mark_guild_read(s: Store, gid: Id) {
    let channels = s.guild(gid).map(|g| g.channels).unwrap_or_default();
    let last = s.last.get_untracked();
    s.reads.update(|r| {
        for c in &channels {
            if let Some(m) = last.get(&c.id) {
                let e = r.entry(c.id).or_insert(ReadState { channel_id: c.id, last_read: None, mention_count: 0 });
                e.last_read = Some(e.last_read.map_or(*m, |l| l.max(*m)));
                e.mention_count = 0;
            }
        }
    });
    spawn_local(async move {
        if let Ok(states) = crate::api::ack_guild(gid).await {
            states.into_iter().for_each(|r| s.merge_read(r));
        }
    });
}

// ───────── fenêtre, titre, bureau ─────────

fn window_active() -> bool {
    let doc = document();
    doc.has_focus().unwrap_or(true) && doc.visibility_state() == web_sys::VisibilityState::Visible
}

/// Suit le focus de la fenêtre, avance l'horloge des sourdines, écrit le titre de l'onglet.
pub fn init(s: Store) {
    s.focused.set(window_active());
    let on = window_event_listener(leptos::ev::focus, move |_| s.focused.set(true));
    let off = window_event_listener(leptos::ev::blur, move |_| s.focused.set(false));
    let vis = window_event_listener(leptos::ev::visibilitychange, move |_| s.focused.set(window_active()));
    // Écouteurs de toute la vie de la page.
    std::mem::forget((on, off, vis));
    spawn_local(async move {
        loop {
            TimeoutFuture::new(30_000).await;
            s.now.set(js_sys::Date::now() as i64);
        }
    });
    Effect::new(move || {
        let n = s.total_mentions();
        document().set_title(&if n > 0 { format!("({n}) KRAKEN") } else { "KRAKEN".into() });
    });
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Ce navigateur sait-il afficher des notifications ? (Pas l'appli de bureau sous macOS.)
pub fn desktop_supported() -> bool {
    js_sys::Reflect::has(&window(), &"Notification".into()).unwrap_or(false)
}

pub fn desktop_permission() -> web_sys::NotificationPermission {
    if desktop_supported() { web_sys::Notification::permission() } else { web_sys::NotificationPermission::Denied }
}

/// Notifications voulues sur cet appareil et permises par le navigateur.
pub fn desktop_enabled() -> bool {
    desktop_permission() == web_sys::NotificationPermission::Granted
        && storage().and_then(|s| s.get_item(DESKTOP_KEY).ok().flatten()).is_some()
}

pub fn set_desktop_enabled(on: bool) {
    if let Some(st) = storage() {
        let _ = if on { st.set_item(DESKTOP_KEY, "1") } else { st.remove_item(DESKTOP_KEY) };
    }
}

/// Demande la permission au navigateur (après un clic seulement) ; rend la réponse.
pub async fn ask_permission() -> web_sys::NotificationPermission {
    if !desktop_supported() {
        return web_sys::NotificationPermission::Denied;
    }
    // La réponse de la question fait foi (certains navigateurs tardent à mettre à jour
    // `Notification.permission`).
    let answer = match web_sys::Notification::request_permission() {
        Ok(p) => wasm_bindgen_futures::JsFuture::from(p).await.ok().and_then(|v| v.as_string()),
        Err(_) => None,
    };
    match answer.as_deref() {
        Some("granted") => web_sys::NotificationPermission::Granted,
        Some("denied") => web_sys::NotificationPermission::Denied,
        _ => web_sys::Notification::permission(),
    }
}

/// Où mène un salon.
pub fn href(c: &Channel) -> String {
    match c.guild_id {
        Some(g) => format!("/channels/{g}/{}", c.id),
        None => format!("/channels/@me/{}", c.id),
    }
}

fn desktop_notification(s: Store, c: &Channel, m: &Message) {
    if !desktop_enabled() {
        return;
    }
    let who = m.author.as_ref().map(|a| a.name().to_string()).unwrap_or_else(|| "Compte supprimé".into());
    let title = match c.guild_id {
        None if c.kind == ChannelKind::Dm => who.clone(),
        None => format!("{who} dans {}", crate::social::private_title(c)),
        Some(g) => {
            let server = s.guild(g).map(|g| g.guild.name).unwrap_or_default();
            format!("{who} (#{}, {server})", c.name.clone().unwrap_or_default())
        }
    };
    let body = if m.call.is_some() { "Appel en cours".to_string() } else { kraken_core::markdown::summary(&m.content) };
    let opts = web_sys::NotificationOptions::new();
    opts.set_body(&body);
    opts.set_tag(&c.id.to_string());
    opts.set_icon("/icon-192.png");
    let Ok(n) = web_sys::Notification::new_with_options(&title, &opts) else { return };
    let to = href(c);
    let n2 = n.clone();
    let click = Closure::once_into_js(move || {
        let _ = window().focus();
        s.goto.set(Some(to));
        n2.close();
    });
    n.set_onclick(Some(click.unchecked_ref()));
}

// ───────── menu « Marquer comme lu » et boîte « Notifications » ─────────

/// Ce que vise le menu ou la boîte : un serveur, ou un salon (MP compris).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Guild(Id),
    Channel(Id),
}

#[derive(Clone, Copy, PartialEq)]
struct MenuAt {
    target: Target,
    x: f64,
    y: f64,
}

/// Menu et boîte ouverts dans la coquille.
#[derive(Clone, Copy)]
pub struct NotifyUi {
    menu: RwSignal<Option<MenuAt>>,
    pub dialog: RwSignal<Option<Target>>,
}

pub fn ui() -> NotifyUi {
    expect_context()
}

pub fn provide_ui() -> NotifyUi {
    let u = NotifyUi { menu: RwSignal::new(None), dialog: RwSignal::new(None) };
    provide_context(u);
    u
}

impl NotifyUi {
    /// Clic droit sur un serveur ou un salon.
    pub fn on_context(self, target: Target) -> impl Fn(leptos::ev::MouseEvent) + Copy {
        move |e: leptos::ev::MouseEvent| {
            e.prevent_default();
            self.menu.set(Some(MenuAt { target, x: e.client_x() as f64, y: e.client_y() as f64 }));
        }
    }

    /// Maj+F10 ou la touche Menu sur un serveur ou un salon.
    pub fn on_key(self, target: Target) -> impl Fn(leptos::ev::KeyboardEvent) + Copy {
        move |e: leptos::ev::KeyboardEvent| {
            if e.key() == "ContextMenu" || (e.shift_key() && e.key() == "F10") {
                e.prevent_default();
                let rect = e
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .map(|el| el.get_bounding_client_rect());
                let (x, y) = rect.map_or((0.0, 0.0), |r| (r.left() + 16.0, r.bottom()));
                self.menu.set(Some(MenuAt { target, x, y }));
            }
        }
    }
}

fn target_title(s: Store, t: Target) -> String {
    match t {
        Target::Guild(g) => s.guild(g).map(|g| g.guild.name).unwrap_or_default(),
        Target::Channel(c) => match s.channel(c) {
            Some(c) if c.guild_id.is_none() => crate::social::private_title(&c),
            Some(c) => format!("#{}", c.name.unwrap_or_default()),
            None => String::new(),
        },
    }
}

fn target_muted(s: Store, t: Target) -> Option<i64> {
    let setting = match t {
        Target::Guild(g) => s.guild_setting(g),
        Target::Channel(c) => s.channel_setting(c),
    };
    setting.and_then(|x| x.muted_until).filter(|u| notify::muted(Some(*u), s.now.get()))
}

async fn save(t: Target, req: SetNotify) -> Result<NotifySetting, String> {
    let req = match t {
        Target::Guild(g) => SetNotify { guild_id: Some(g), ..req },
        Target::Channel(c) => SetNotify { channel_id: Some(c), ..req },
    };
    crate::api::set_notify(&req).await.map_err(|e| e.message)
}

/// Menu contextuel d'un serveur ou d'un salon : flèches, Début, Fin, Échap.
#[component]
pub fn TargetMenu() -> impl IntoView {
    let s = store();
    let u = ui();
    let root = NodeRef::<leptos::html::Div>::new();
    let before = StoredValue::new(None::<web_sys::HtmlElement>);
    let close = move |refocus: bool| {
        u.menu.set(None);
        if refocus && let Some(el) = before.get_value() {
            let _ = el.focus();
        }
    };
    Effect::new(move || {
        if u.menu.get().is_some() {
            if before.get_value().is_none() {
                before.set_value(document().active_element().and_then(|e| e.dyn_into().ok()));
            }
            if let Some(first) = root.get().and_then(|r| r.query_selector("[role=menuitem]").ok().flatten())
                && let Ok(h) = first.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = h.focus();
            }
        } else {
            before.set_value(None);
        }
    });
    let outside = window_event_listener(leptos::ev::pointerdown, move |e| {
        if u.menu.get_untracked().is_none() {
            return;
        }
        let inside = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
            .zip(root.get_untracked())
            .is_some_and(|(t, r)| r.contains(Some(&t)));
        if !inside {
            u.menu.set(None);
        }
    });
    on_cleanup(move || outside.remove());
    let keys = move |e: leptos::ev::KeyboardEvent| {
        let Some(r) = root.get_untracked() else { return };
        let items: Vec<web_sys::HtmlElement> = r
            .query_selector_all("[role=menuitem]")
            .map(|l| (0..l.length()).filter_map(|i| l.item(i)?.dyn_into().ok()).collect())
            .unwrap_or_default();
        let n = items.len();
        if n == 0 {
            return;
        }
        let at = document()
            .active_element()
            .and_then(|a| items.iter().position(|i| *i == *a.unchecked_ref::<web_sys::HtmlElement>()));
        let next = match e.key().as_str() {
            "ArrowDown" => at.map_or(0, |i| (i + 1) % n),
            "ArrowUp" => at.map_or(n - 1, |i| (i + n - 1) % n),
            "Home" => 0,
            "End" => n - 1,
            "Escape" => {
                e.prevent_default();
                e.stop_propagation();
                close(true);
                return;
            }
            "Tab" => {
                close(false);
                return;
            }
            _ => return,
        };
        e.prevent_default();
        let _ = items[next].focus();
    };
    view! {
        {move || u.menu.get().map(|m| {
            let t = m.target;
            let title = target_title(s, t);
            let muted = target_muted(s, t).is_some();
            let read = move |_| {
                match t {
                    Target::Guild(g) => mark_guild_read(s, g),
                    Target::Channel(c) => mark_channel_read(s, c),
                }
                close(true);
            };
            let unmute = move |_| {
                spawn_local(async move {
                    if let Ok(n) = save(t, SetNotify { mute: Some(Mute::Off), ..Default::default() }).await {
                        s.upsert_notify(n);
                    }
                });
                close(true);
            };
            let open = move |_| {
                close(false);
                u.dialog.set(Some(t));
            };
            let style = format!("position: fixed; left: min({}px, 100vw - 220px); top: min({}px, 100vh - 160px)", m.x, m.y);
            view! {
                <div class="k-menu" role="menu" aria-label=format!("Actions pour {title}") node_ref=root style=style on:keydown=keys>
                    <button type="button" role="menuitem" tabindex="-1" on:click=read>"Marquer comme lu"<Icon icon=lu::LuCheckCheck small=true/></button>
                    <hr/>
                    {if muted {
                        view! { <button type="button" role="menuitem" tabindex="-1" on:click=unmute>"Retirer la sourdine"<Icon icon=lu::LuBell small=true/></button> }.into_any()
                    } else {
                        view! { <button type="button" role="menuitem" tabindex="-1" on:click=open>"Mettre en sourdine…"<Icon icon=lu::LuBellOff small=true/></button> }.into_any()
                    }}
                    <button type="button" role="menuitem" tabindex="-1" on:click=open>"Notifications…"<Icon icon=lu::LuSettings small=true/></button>
                </div>
            }
        })}
    }
}

const MUTES: &[(&str, &str, Option<u32>)] = &[
    ("15", "Pendant 15 minutes", Some(15)),
    ("60", "Pendant 1 heure", Some(60)),
    ("480", "Pendant 8 heures", Some(480)),
    ("1440", "Pendant 24 heures", Some(1440)),
    ("forever", "Jusqu'à ce que je la retire", None),
];

/// Boîte « Notifications » d'un serveur ou d'un salon : sourdine, niveau, `@everyone`.
#[component]
pub fn NotifyDialog(target: Target) -> impl IntoView {
    let s = store();
    let u = ui();
    let close = move || u.dialog.set(None);
    let root = NodeRef::new();
    modal(root, close);
    let error = RwSignal::new(None::<String>);
    let title = target_title(s, target);
    let channel = match target {
        Target::Channel(c) => s.channel(c),
        Target::Guild(_) => None,
    };
    let private = channel.as_ref().is_some_and(|c| c.guild_id.is_none());
    let setting = Memo::new(move |_| match target {
        Target::Guild(g) => s.guild_setting(g),
        Target::Channel(c) => s.channel_setting(c),
    });
    let level = Memo::new(move |_| setting.get().map_or(NotifyLevel::Default, |x| x.level));
    let muted = Memo::new(move |_| target_muted(s, target));
    let apply = move |req: SetNotify| {
        error.set(None);
        spawn_local(async move {
            match save(target, req).await {
                Ok(n) => s.upsert_notify(n),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let on_mute = move |e: leptos::ev::Event| {
        let v = event_target_value(&e);
        let mute = match v.as_str() {
            "off" => Mute::Off,
            "forever" => Mute::Forever,
            "current" => return,
            m => Mute::Minutes(m.parse().unwrap_or(15)),
        };
        apply(SetNotify { mute: Some(mute), ..Default::default() });
    };
    // Ce qu'hérite un salon : le niveau de son serveur.
    let inherited = channel.clone().map(|c| {
        let s2 = s;
        move || {
            let l = c.guild_id.and_then(|g| s2.guild_setting(g)).and_then(|x| level_of(x.level));
            match notify::effective_level(c.guild_id.is_none(), None, l) {
                Level::All => "tous les messages",
                Level::Mentions => "les mentions seulement",
                Level::Nothing => "rien",
            }
        }
    });
    let mut levels: Vec<(NotifyLevel, String)> = Vec::new();
    match (&inherited, private) {
        (Some(_), true) => levels.push((NotifyLevel::Default, "Par défaut : tous les messages".into())),
        (Some(_), false) => levels.push((NotifyLevel::Default, "Comme le serveur".into())),
        (None, _) => {}
    }
    levels.push((NotifyLevel::All, "Tous les messages".into()));
    if !private {
        levels.push((
            NotifyLevel::Mentions,
            if channel.is_none() {
                "Les mentions seulement (par défaut)".into()
            } else {
                "Les mentions seulement".into()
            },
        ));
    }
    levels.push((NotifyLevel::Nothing, "Rien".into()));
    let server_default = channel.is_none();
    let radios = levels
        .into_iter()
        .map(|(l, label)| {
            let checked = move || level.get() == l || (server_default && l == NotifyLevel::Mentions && level.get() == NotifyLevel::Default);
            let sub = match l {
                NotifyLevel::Default => inherited.map(|f| view! { <span>{move || format!("Aujourd'hui : {}", f())}</span> }.into_any()),
                NotifyLevel::All => Some(view! { <span>"Chaque nouveau message peut s'afficher sur le bureau."</span> }.into_any()),
                NotifyLevel::Mentions => Some(view! { <span>"Seulement quand on te nomme, répond à ton message ou appelle tout le monde."</span> }.into_any()),
                NotifyLevel::Nothing => Some(view! { <span>"Aucune notification, et plus de compteur de mentions."</span> }.into_any()),
            };
            view! {
                <label class="k-radio">
                    <input type="radio" name="notify-level" prop:checked=checked
                        on:change=move |_| apply(SetNotify { level: Some(l), ..Default::default() })/>
                    <div><b>{label}</b>{sub}</div>
                </label>
            }
        })
        .collect_view();
    let suppress = Memo::new(move |_| setting.get().is_some_and(|x| x.suppress_everyone));
    view! {
        <div class="k-scrim" node_ref=root on:click=move |_| close()>
            <div class="k-dialog app-dialog-wide" role="dialog" aria-modal="true" aria-labelledby="notify-t" on:click=|e| e.stop_propagation()>
                <header><h2 id="notify-t">{format!("Notifications de {title}")}</h2></header>
                <div class="app-notify">
                    <label class="k-field">
                        <span class="k-label">"Sourdine"</span>
                        <select class="k-select" aria-describedby="notify-mute-h" on:change=on_mute>
                            <option value="off" prop:selected=move || muted.get().is_none()>"Pas de sourdine"</option>
                            {move || muted.get().map(|u| {
                                let label = if u == MUTED_FOREVER { "En sourdine, sans fin".to_string() } else { format!("En sourdine jusqu'à {}", when(u)) };
                                view! { <option value="current" prop:selected=true>{label}</option> }
                            })}
                            {MUTES.iter().map(|(v, label, _)| view! { <option value=*v>{*label}</option> }).collect_view()}
                        </select>
                        <span class="k-hint" id="notify-mute-h">"En sourdine : ni gras, ni notification. Les mentions restent comptées."</span>
                    </label>
                    <fieldset class="k-radios">
                        <legend class="k-label app-legend">"Me prévenir pour"</legend>
                        {radios}
                    </fieldset>
                    {server_default.then(|| view! {
                        <div class="k-setting">
                            <label for="notify-everyone">"Ignorer @everyone et @here"</label>
                            <input id="notify-everyone" type="checkbox" role="switch" class="k-switch" prop:checked=move || suppress.get()
                                on:change=move |e| apply(SetNotify { suppress_everyone: Some(event_target_checked(&e)), ..Default::default() })/>
                            <p>"Ces appels à tout le serveur ne comptent plus comme des mentions."</p>
                        </div>
                    })}
                    <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                </div>
                <footer>
                    <button type="button" class="k-btn k-btn--primary" on:click=move |_| close() autofocus>"Fermer"</button>
                </footer>
            </div>
        </div>
    }
}

/// Le nombre écrit dans un badge : « 99+ » au-delà.
pub fn count_text(n: u32) -> String {
    if n > 99 { "99+".into() } else { n.to_string() }
}

/// Badge de mentions : le nombre affiché, doublé en texte pour les lecteurs d'écran.
#[component]
pub fn MentionBadge(#[prop(into)] count: Signal<u32>) -> impl IntoView {
    view! {
        <Show when=move || { count.get() > 0 }>
            <span class="k-badge k-badge--count" aria-hidden="true">{move || count_text(count.get())}</span>
            <span class="k-sr">{move || match count.get() { 1 => ", 1 mention".to_string(), n => format!(", {n} mentions") }}</span>
        </Show>
    }
}

/// « non lu » pour les lecteurs d'écran.
#[component]
pub fn UnreadText(#[prop(into)] on: Signal<bool>) -> impl IntoView {
    view! { <Show when=move || on.get()><span class="k-sr">", non lu"</span></Show> }
}

/// Salon en sourdine : cloche barrée, et « en sourdine » pour les lecteurs d'écran.
#[component]
pub fn MutedMark(#[prop(into)] on: Signal<bool>) -> impl IntoView {
    view! {
        <Show when=move || on.get()>
            <span class="k-muted-icon" aria-hidden="true"><Icon icon=lu::LuBellOff small=true/></span>
            <span class="k-sr">", en sourdine"</span>
        </Show>
    }
}
