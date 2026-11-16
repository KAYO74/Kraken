//! Quick Switcher (Ctrl/Cmd+K, ou le bouton « Aller vers… » de la barre latérale) : un champ, des
//! résultats (salons texte et vocaux, messages privés et groupes, personnes, serveurs), Entrée y va.
//! Champ en combobox ARIA : le focus reste dans le champ, `aria-activedescendant` désigne l'option active.
//! Le classement et le surlignage sont dans `kraken_core::search` (testés là).
//!
//! Opérateurs : `#` salons, `@` personnes (et MP, groupes, par le nom de leurs membres), `*` serveurs.
//! Champ vide : les 5 derniers endroits visités (`kraken.recents` dans le stockage : des identifiants, rien d'autre).

use std::cell::RefCell;

use kraken_core::grid::cycle;
use kraken_core::search::{self, Scope};
use kraken_proto::{Channel, ChannelKind, Guild, Id, RelationshipKind, User};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::icons::{Icon, lu};
use crate::shell::{current_ids, ordered};
use crate::shortcuts::{self, Action};
use crate::store::{Store, store};
use crate::ui::{Avatar, initials, modal, tint, trap_tab};
use crate::unread::{MentionBadge, UnreadText};

const RECENTS_KEY: &str = "kraken.recents";
const SHOWN_RECENTS: usize = 5;
/// Résultats affichés au plus.
const MAX_RESULTS: usize = 12;

// ───────── récents ─────────

thread_local! {
    /// Les endroits visités, le plus récent d'abord ; lus dans le stockage au premier besoin.
    static RECENTS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn recents<R>(f: impl FnOnce(&mut Vec<String>) -> R) -> R {
    RECENTS.with_borrow_mut(|r| {
        let list = r.get_or_insert_with(|| {
            storage()
                .and_then(|s| s.get_item(RECENTS_KEY).ok().flatten())
                .map(|raw| search::parse_recents(&raw))
                .unwrap_or_default()
        });
        f(list)
    })
}

/// Note ce salon (ou MP) comme le dernier endroit visité.
pub fn visit(id: Id) {
    let id = id.to_string();
    recents(|l| {
        if l.first() == Some(&id) {
            return;
        }
        search::remember(l, &id);
        if let Some(s) = storage() {
            let _ = s.set_item(RECENTS_KEY, &l.join(","));
        }
    });
}

/// Suit le salon affiché : chaque salon ou MP qui existe dans mes listes devient le dernier visité.
pub fn track_visits(ids: Memo<(Option<Id>, Option<Id>)>) {
    let s = store();
    let shown =
        Memo::new(move |_| ids.get().1.filter(|c| s.channel(*c).is_some_and(|c| c.kind != ChannelKind::Category)));
    Effect::new(move || {
        if let Some(c) = shown.get() {
            visit(c);
        }
    });
}

/// Où mène un serveur : son dernier salon visité, sinon la page du serveur (qui ouvre le premier salon).
pub fn guild_href(s: Store, g: Id) -> String {
    let last = untrack(|| {
        recents(|l| l.clone())
            .iter()
            .filter_map(|x| x.parse::<Id>().ok())
            .find(|c| s.channel(*c).is_some_and(|c| c.guild_id == Some(g)))
    });
    match last {
        Some(c) => format!("/channels/{g}/{c}"),
        None => format!("/channels/{g}"),
    }
}

// ───────── ce qu'on peut chercher ─────────

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Recent,
    Channels,
    Dms,
    People,
    Servers,
}

impl Section {
    const ALL: [Section; 5] = [Self::Recent, Self::Channels, Self::Dms, Self::People, Self::Servers];

    fn title(self) -> &'static str {
        match self {
            Self::Recent => "Récents",
            Self::Channels => "Salons",
            Self::Dms => "Messages privés",
            Self::People => "Personnes",
            Self::Servers => "Serveurs",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Channels => "channels",
            Self::Dms => "dms",
            Self::People => "people",
            Self::Servers => "servers",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Hash,
    Lock,
    Voice,
    Announcement,
    Forum,
}

/// Ce qui est dessiné avant le nom.
#[derive(Clone, PartialEq)]
enum Lead {
    Glyph(Glyph),
    Person(User),
    Initials(Id, String),
}

#[derive(Clone, PartialEq)]
struct Entry {
    section: Section,
    /// Identifiant stable de l'option (`sw-o-<key>`).
    key: String,
    /// Le salon ou le MP, pour ses non-lus.
    channel: Option<Id>,
    /// La personne dont on ouvre le MP.
    person: Option<Id>,
    href: Option<String>,
    name: String,
    detail: String,
    /// Ce que dit un lecteur d'écran avant le nom.
    kind: &'static str,
    lead: Lead,
    /// Les textes comparés à la requête : le nom d'abord.
    texts: Vec<String>,
}

fn channel_entry(c: &Channel, guild: &Guild) -> Option<Entry> {
    let (glyph, kind) = match c.kind {
        ChannelKind::Category | ChannelKind::Thread | ChannelKind::Dm | ChannelKind::GroupDm => return None,
        ChannelKind::Voice | ChannelKind::Stage => {
            (Glyph::Voice, if c.private { "Salon vocal privé" } else { "Salon vocal" })
        }
        ChannelKind::Announcement => (Glyph::Announcement, "Annonces"),
        ChannelKind::Forum => (Glyph::Forum, "Forum"),
        _ if c.private => (Glyph::Lock, "Salon privé"),
        _ => (Glyph::Hash, "Salon textuel"),
    };
    let name = c.name.clone().unwrap_or_default();
    Some(Entry {
        section: Section::Channels,
        key: format!("c-{}", c.id),
        channel: Some(c.id),
        person: None,
        href: Some(format!("/channels/{}/{}", guild.id, c.id)),
        texts: vec![name.clone()],
        name,
        detail: guild.name.clone(),
        kind,
        lead: Lead::Glyph(glyph),
    })
}

fn dm_entry(c: &Channel) -> Entry {
    let group = c.kind == ChannelKind::GroupDm;
    let name = crate::social::private_title(c);
    let mut texts = vec![name.clone()];
    for u in &c.recipients {
        texts.push(u.name().to_string());
        texts.push(u.username.clone());
    }
    let lead = match (group, c.recipients.first()) {
        (false, Some(u)) => Lead::Person(u.clone()),
        _ => Lead::Initials(c.id, initials(&name)),
    };
    let detail = match (group, c.recipients.first()) {
        (false, Some(u)) => format!("@{}", u.username),
        _ => format!("{} membres", c.recipients.len() + 1),
    };
    Entry {
        section: Section::Dms,
        key: format!("c-{}", c.id),
        channel: Some(c.id),
        person: None,
        href: Some(format!("/channels/@me/{}", c.id)),
        name,
        detail,
        kind: if group { "Groupe" } else { "Message privé" },
        lead,
        texts,
    }
}

fn person_entry(u: &User) -> Entry {
    Entry {
        section: Section::People,
        key: format!("u-{}", u.id),
        channel: None,
        person: Some(u.id),
        href: None,
        name: u.name().to_string(),
        detail: format!("@{}", u.username),
        kind: "Personne",
        lead: Lead::Person(u.clone()),
        texts: vec![u.name().to_string(), u.username.clone()],
    }
}

fn server_entry(g: &Guild) -> Entry {
    Entry {
        section: Section::Servers,
        key: format!("g-{}", g.id),
        channel: None,
        person: None,
        href: None,
        name: g.name.clone(),
        detail: String::new(),
        kind: "Serveur",
        lead: Lead::Initials(g.id, initials(&g.name)),
        texts: vec![g.name.clone()],
    }
}

/// Tout ce que la portée permet de chercher, dans l'ordre : salons (serveur par serveur, comme la barre
/// latérale), MP et groupes, personnes (amis sans MP ouvert : les autres sont déjà dans les MP), serveurs.
fn candidates(s: Store, scope: Scope) -> Vec<Entry> {
    let mut out = Vec::new();
    if matches!(scope, Scope::All | Scope::Channels) {
        s.guilds.with(|gs| {
            for g in gs {
                out.extend(ordered(&g.channels).iter().filter_map(|c| channel_entry(c, &g.guild)));
            }
        });
    }
    if scope == Scope::All || scope == Scope::People {
        let mut talking: Vec<Id> = Vec::new();
        s.dms.with(|dms| {
            for c in dms {
                out.push(dm_entry(c));
                if c.kind == ChannelKind::Dm {
                    talking.extend(c.recipients.iter().map(|u| u.id));
                }
            }
        });
        s.friends.with(|fs| {
            let friends = fs.iter().filter(|r| r.kind == RelationshipKind::Friend && !talking.contains(&r.user.id));
            out.extend(friends.map(|r| person_entry(&r.user)));
        });
    }
    if matches!(scope, Scope::All | Scope::Servers) {
        s.guilds.with(|gs| out.extend(gs.iter().map(|g| server_entry(&g.guild))));
    }
    out
}

/// Un endroit d'après son identifiant (salon, MP ou serveur) ; `None` s'il n'existe plus.
fn entry_for(s: Store, id: Id) -> Option<Entry> {
    match s.channel(id) {
        Some(c) if c.guild_id.is_none() => Some(dm_entry(&c)),
        Some(c) => c.guild_id.and_then(|g| s.guild(g)).and_then(|g| channel_entry(&c, &g.guild)),
        None => s.guild(id).map(|g| server_entry(&g.guild)),
    }
}

#[derive(Clone, PartialEq)]
struct Row {
    section: Section,
    entry: Entry,
    /// Passages du nom à surligner.
    ranges: Vec<(usize, usize)>,
}

/// Les résultats pour cette saisie, groupés (Récents, Salons, MP, Personnes, Serveurs).
fn search_rows(s: Store, raw: &str, current: Option<Id>) -> Vec<Row> {
    let (scope, text) = search::scope_of(raw);
    let mut rows: Vec<Row> = if text.is_empty() && scope == Scope::All {
        let ids = recents(|l| l.clone());
        ids.iter()
            .filter_map(|x| x.parse::<Id>().ok())
            .filter(|id| Some(*id) != current)
            .filter_map(|id| entry_for(s, id))
            .take(SHOWN_RECENTS)
            .map(|entry| Row { section: Section::Recent, entry, ranges: vec![] })
            .collect()
    } else if text.is_empty() {
        candidates(s, scope)
            .into_iter()
            .take(MAX_RESULTS)
            .map(|entry| Row { section: entry.section, entry, ranges: vec![] })
            .collect()
    } else {
        let items = candidates(s, scope).into_iter().map(|e| {
            let texts = e.texts.clone();
            (e, texts)
        });
        search::rank(text, items, MAX_RESULTS)
            .into_iter()
            .map(|r| Row { section: r.item.section, ranges: if r.via == 0 { r.ranges } else { vec![] }, entry: r.item })
            .collect()
    };
    // Les groupes dans leur ordre, chaque groupe gardant son classement.
    rows.sort_by_key(|r| r.section);
    rows
}

// ───────── aller quelque part ─────────

/// Ferme le Quick Switcher et le tiroir de navigation (mobile).
fn done(s: Store, ui: shortcuts::Ui) {
    ui.switcher.set(false);
    s.drawer.set(false);
}

/// Ouvre l'endroit choisi, de la même façon qu'un clic dans la barre latérale ou le rail.
fn go(s: Store, ui: shortcuts::Ui, e: &Entry, error: RwSignal<Option<String>>) {
    if let Some(href) = &e.href {
        s.goto.set(Some(href.clone()));
        return done(s, ui);
    }
    if let Some(uid) = e.person {
        let known = s.dms.with_untracked(|d| {
            d.iter().find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == uid)).map(|c| c.id)
        });
        if let Some(c) = known {
            s.goto.set(Some(format!("/channels/@me/{c}")));
            return done(s, ui);
        }
        spawn_local(async move {
            match crate::api::open_private(vec![uid], None).await {
                Ok(c) => {
                    let id = c.id;
                    s.upsert_dm(c);
                    s.goto.set(Some(format!("/channels/@me/{id}")));
                    done(s, ui);
                }
                Err(e) => {
                    error.try_set(Some(e.message));
                }
            }
        });
        return;
    }
    // Un serveur : son dernier salon visité, sinon le premier.
    if let Some(g) = e.key.strip_prefix("g-").and_then(|g| g.parse::<Id>().ok()) {
        s.goto.set(Some(guild_href(s, g)));
        done(s, ui);
    }
}

/// Le bouton « Aller vers… » en haut des barres latérales : le seul moyen à la souris et au toucher.
#[component]
pub fn SwitcherButton() -> impl IntoView {
    let ui = shortcuts::ui();
    let shortcut = if shortcuts::is_mac() { "Meta+K" } else { "Control+K" };
    view! {
        <button type="button" class="k-btn k-btn--sm app-find" aria-haspopup="dialog" aria-keyshortcuts=shortcut
            on:click=move |_| ui.switcher.set(true)>
            <Icon icon=lu::LuSearch small=true/>
            <span class="app-find-text">"Aller vers…"</span>
            <span class="app-find-kbd" aria-hidden="true">{shortcuts::keys_view(Action::Switcher)}</span>
        </button>
    }
}

fn glyph_icon(g: Glyph) -> icondata_core::Icon {
    match g {
        Glyph::Hash => lu::LuHash,
        Glyph::Lock => lu::LuLock,
        Glyph::Voice => lu::LuVolume2,
        Glyph::Announcement => lu::LuMegaphone,
        Glyph::Forum => lu::LuMessagesSquare,
    }
}

fn option_id(row: &Row) -> String {
    format!("sw-o-{}", row.entry.key)
}

/// Une option : l'icône ou l'avatar, le nom (lettres trouvées en gras et soulignées), le détail à droite.
fn option_view(
    s: Store,
    row: Row,
    index: usize,
    at: Memo<usize>,
    active: RwSignal<usize>,
    pick: Callback<usize>,
) -> impl IntoView {
    let id = option_id(&row);
    let Row { entry: e, ranges, .. } = row;
    let channel = e.channel;
    let unread = Memo::new(move |_| channel.and_then(|c| s.channel(c)).is_some_and(|c| s.shows_unread(&c)));
    let mentions = Memo::new(move |_| channel.and_then(|c| s.channel(c)).map_or(0, |c| s.mentions(&c)));
    let lead = match e.lead {
        Lead::Glyph(g) => view! { <Icon icon=glyph_icon(g)/> }.into_any(),
        Lead::Person(u) => view! { <span aria-hidden="true"><Avatar user=u size="sm"/></span> }.into_any(),
        Lead::Initials(id, text) => {
            view! { <span class="k-avatar k-avatar--sm" data-tint=tint(id) aria-hidden="true">{text}</span> }.into_any()
        }
    };
    let name = search::segments(&e.name, &ranges)
        .into_iter()
        .map(|(text, hit)| if hit { view! { <mark>{text}</mark> }.into_any() } else { text.into_any() })
        .collect_view();
    let detail = (!e.detail.is_empty()).then(|| view! { <small class="app-sw-detail">{e.detail.clone()}</small> });
    view! {
        <div class="k-option app-sw-opt" role="option" id=id aria-selected=move || (at.get() == index).to_string()
            on:mousedown=|e| e.prevent_default()
            on:mousemove=move |e| if (e.movement_x() != 0 || e.movement_y() != 0) && active.get_untracked() != index { active.set(index) }
            on:click=move |_| pick.run(index)>
            <span class="app-sw-lead">{lead}</span>
            <span class="k-sr">{e.kind}" "</span>
            <span class="app-sw-name">{name}</span>
            <UnreadText on=unread/>
            <span class="app-sw-dot" aria-hidden="true" hidden=move || !unread.get()></span>
            <MentionBadge count=mentions/>
            {detail}
        </div>
    }
}

#[component]
pub fn Switcher() -> impl IntoView {
    let s = store();
    let ui = shortcuts::ui();
    let current = current_ids().get_untracked().1;
    let q = RwSignal::new(String::new());
    let active = RwSignal::new(0usize);
    let error = RwSignal::new(None::<String>);
    let root = NodeRef::<html::Div>::new();
    let list = NodeRef::<html::Div>::new();
    modal(root, move || ui.switcher.set(false));

    let results = Memo::new(move |_| search_rows(s, &q.get(), current));
    let count = Memo::new(move |_| results.with(Vec::len));
    // L'option active : toujours dans la liste, même si elle raccourcit.
    let at = Memo::new(move |_| active.get().min(count.get().saturating_sub(1)));
    let recent_mode = Memo::new(move |_| q.with(|q| search::scope_of(q) == (Scope::All, "")));
    Effect::new(move |_| {
        q.track();
        active.set(0);
        error.set(None);
    });
    let active_id = move || results.with(|r| r.get(at.get()).map(option_id));
    // L'option active reste visible : en haut de liste, on remonte jusqu'à son titre de groupe.
    Effect::new(move |_| {
        let i = at.get();
        results.track();
        if i == 0
            && let Some(l) = list.get_untracked()
        {
            l.set_scroll_top(0);
        }
        if let Some(el) = active_id().and_then(|id| document().get_element_by_id(&id)) {
            let opts = web_sys::ScrollIntoViewOptions::new();
            opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
            el.scroll_into_view_with_scroll_into_view_options(&opts);
        }
    });

    let pick = Callback::new(move |i: usize| {
        if let Some(row) = results.with_untracked(|r| r.get(i).cloned()) {
            go(s, ui, &row.entry, error);
        }
    });
    let on_key = move |e: leptos::ev::KeyboardEvent| {
        let n = count.get_untracked();
        let here = at.get_untracked();
        let to = match e.key().as_str() {
            "ArrowDown" => cycle(n, Some(here), true),
            "ArrowUp" => cycle(n, Some(here), false),
            "Home" => (n > 0).then_some(0),
            "End" => n.checked_sub(1),
            "Enter" if !e.is_composing() => {
                e.prevent_default();
                pick.run(here);
                return;
            }
            _ => return,
        };
        e.prevent_default();
        if let Some(i) = to {
            active.set(i);
        }
    };
    let status = move || {
        let n = count.get();
        match (recent_mode.get(), n) {
            (true, 0) => "Aucun lieu récent".to_string(),
            (true, 1) => "1 lieu récent".to_string(),
            (true, n) => format!("{n} lieux récents"),
            (false, 0) => "Aucun résultat".to_string(),
            (false, 1) => "1 résultat".to_string(),
            (false, n) => format!("{n} résultats"),
        }
    };
    let groups = move || {
        let rows = results.get();
        Section::ALL
            .iter()
            .filter_map(|&section| {
                let items: Vec<(usize, Row)> = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.section == section)
                    .map(|(i, r)| (i, r.clone()))
                    .collect();
                if items.is_empty() {
                    return None;
                }
                let (labelled, own) = (format!("sw-g-{}", section.slug()), format!("sw-g-{}", section.slug()));
                Some(view! {
                    <div role="group" aria-labelledby=labelled>
                        <div class="k-listbox-group" id=own>{section.title()}</div>
                        {items.into_iter().map(|(i, r)| option_view(s, r, i, at, active, pick)).collect_view()}
                    </div>
                })
            })
            .collect_view()
    };
    let empty = move || {
        let typed = q.get().trim().to_string();
        if recent_mode.get() {
            view! { <div class="k-empty"><b>"Rien de récent"</b><span>"Tape le nom d'un salon, d'une personne ou d'un serveur."</span></div> }.into_any()
        } else {
            view! { <div class="k-empty"><b class="app-sw-typed">{format!("Aucun résultat pour « {typed} »")}</b></div> }.into_any()
        }
    };
    view! {
        <div class="k-scrim app-sw-scrim" node_ref=root on:click=move |_| ui.switcher.set(false) on:keydown=move |e| trap_tab(&e, root)>
            <div class="k-dialog app-sw" role="dialog" aria-modal="true" aria-label="Aller vers" on:click=|e| e.stop_propagation()>
                <div class="app-sw-top">
                    <div class="app-sw-field">
                        <input class="k-input" type="text" role="combobox" aria-autocomplete="list" autocomplete="off" spellcheck="false"
                            autocapitalize="off" enterkeyhint="go" maxlength="100" autofocus
                            aria-label="Salon, personne ou serveur" placeholder="Aller vers un salon, une personne, un serveur"
                            aria-describedby="sw-hint"
                            aria-expanded=move || (count.get() > 0).to_string()
                            aria-controls=move || (count.get() > 0).then_some("sw-list")
                            aria-activedescendant=active_id
                            bind:value=q on:keydown=on_key/>
                        <button type="button" class="k-btn k-btn--icon app-sw-close" aria-label="Fermer" title="Fermer (Échap)"
                            on:click=move |_| ui.switcher.set(false)>
                            <Icon icon=lu::LuX/>
                        </button>
                    </div>
                    <p class="k-hint" id="sw-hint">"# salons · @ personnes et messages privés · * serveurs"</p>
                </div>
                <p class="k-hint app-sw-count" role="status">{status}</p>
                <Show when=move || error.get().is_some()>
                    <p class="k-error app-sw-error" role="alert">{move || error.get()}</p>
                </Show>
                <div class="app-sw-body">
                    <Show when=move || { count.get() > 0 } fallback=empty>
                        <div class="k-listbox app-sw-list" id="sw-list" role="listbox" aria-label="Résultats" node_ref=list>{groups}</div>
                    </Show>
                </div>
                <footer class="app-sw-foot" aria-hidden="true">
                    <span><span class="k-kbd"><kbd>"↑"</kbd><kbd>"↓"</kbd></span>" choisir"</span>
                    <span><span class="k-kbd"><kbd>"↵"</kbd></span>" ouvrir"</span>
                    <span><span class="k-kbd"><kbd>"Échap"</kbd></span>" fermer"</span>
                </footer>
            </div>
        </div>
    }
}
