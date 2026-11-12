//! Réglages du serveur › Journal d'audit : ce qui s'est passé dans le serveur (rôles, membres, salons,
//! invitations, emojis), du plus récent au plus ancien, en lecture seule. Chaque événement est une phrase
//! complète (« lou a banni sam »), avec le motif quand il y en a un, l'heure, et pour un renommage ou un
//! changement de réglage un « Voir les changements » qui déplie l'avant / après dans la ligne.
//!
//! Les pages viennent du serveur par curseur (`next`) : un bouton « Charger plus », jamais de défilement
//! infini, et le focus passe à la première ligne arrivée. Deux filtres (le type d'action, la personne) et
//! « Actualiser » : les nouvelles entrées n'arrivent pas en direct. Tous les noms et motifs sont posés comme
//! du texte, jamais comme du HTML.

use gloo_timers::future::TimeoutFuture;
use kraken_core::span;
use kraken_proto::{AuditActor, AuditChange, AuditEntry, AuditTarget, Id, User};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};

use crate::guild::{CATEGORIES, COLOR_NAMES};
use crate::icons::{Icon, lu};
use crate::ui::{Avatar, clock, iso, now_ms};

/// Les types d'action du filtre, dans l'ordre de la liste : le mot de l'adresse, et son libellé.
const KINDS: [(&str, &str); 6] = [
    ("salons", "Salons"),
    ("roles", "Rôles"),
    ("membres", "Membres"),
    ("invitations", "Invitations"),
    ("serveur", "Serveur"),
    ("emojis", "Emojis"),
];

/// Ce que dit la personne dont le compte n'existe plus.
const GONE_ACTOR: &str = "Membre supprimé";
const GONE_TARGET: &str = "(supprimé)";

#[derive(Clone, PartialEq)]
enum Phase {
    Loading,
    Ready,
    Failed(String),
}

/// Un morceau de phrase : du texte, ou un nom propre (en gras).
#[derive(Clone, Debug, PartialEq)]
enum Seg {
    T(String),
    B(String),
}

fn t(s: impl Into<String>) -> Seg {
    Seg::T(s.into())
}

fn b(s: impl Into<String>) -> Seg {
    Seg::B(s.into())
}

/// L'icône de l'action, et si elle est dangereuse (suppression, bannissement, exclusion) : la couleur ne
/// fait que doubler la phrase, qui dit déjà tout.
fn visual(action: &str) -> (icondata_core::Icon, bool) {
    match action {
        "member_ban" => (lu::LuBan, true),
        "member_kick" => (lu::LuLogOut, true),
        "member_unban" => (lu::LuUserPlus, false),
        "member_timeout" => (lu::LuTimer, true),
        "member_timeout_remove" => (lu::LuTimerOff, false),
        "member_role_add" | "member_role_remove" => (lu::LuUsers, false),
        "owner_transfer" => (lu::LuCrown, false),
        "role_delete" => (lu::LuTrash2, true),
        "role_create" | "role_update" | "role_reorder" => (lu::LuShield, false),
        "channel_delete" => (lu::LuTrash2, true),
        "channel_create" | "channel_update" | "channel_move" => (lu::LuHash, false),
        "overwrite_update" => (lu::LuLock, false),
        "invite_create" | "invite_revoke" => (lu::LuLink, false),
        "emoji_delete" => (lu::LuTrash2, true),
        "emoji_create" | "emoji_update" => (lu::LuSmile, false),
        "message_delete" => (lu::LuTrash2, true),
        "guild_update" => (lu::LuPencil, false),
        _ => (lu::LuScrollText, false),
    }
}

fn detail<'a>(e: &'a AuditEntry, key: &str) -> Option<&'a str> {
    e.details.get(key).map(String::as_str).filter(|v| !v.is_empty())
}

fn change<'a>(e: &'a AuditEntry, field: &str) -> Option<&'a AuditChange> {
    e.changes.iter().find(|c| c.field == field)
}

fn target_name(target: &Option<AuditTarget>) -> String {
    target.as_ref().and_then(|t| t.name.clone()).unwrap_or_else(|| GONE_TARGET.into())
}

/// « le salon #général », « le salon vocal Cartes », « la catégorie Archives » : d'après le type gardé à
/// l'écriture, sinon un salon.
fn channel_phrase(kind: Option<&str>, name: &str) -> Vec<Seg> {
    match kind {
        Some("voice") => vec![t("le salon vocal "), b(name)],
        Some("category") => vec![t("la catégorie "), b(name)],
        Some("announcement") => vec![t("le salon d'annonces "), b(format!("#{name}"))],
        Some("forum") => vec![t("le forum "), b(format!("#{name}"))],
        Some("stage") => vec![t("la scène "), b(name)],
        _ => vec![t("le salon "), b(format!("#{name}"))],
    }
}

fn emoji_code(name: &str) -> String {
    format!(":{name}:")
}

/// La phrase complète de l'événement, sans l'auteur : « a banni lou ».
fn predicate(e: &AuditEntry) -> Vec<Seg> {
    let target = || target_name(&e.target);
    let channel = || {
        let n = target();
        if e.target.as_ref().is_some_and(|t| t.name.is_some()) { format!("#{n}") } else { n }
    };
    let mut s = Vec::new();
    match e.action.as_str() {
        "member_role_add" => {
            s.extend([t("a ajouté le rôle "), b(detail(e, "role").unwrap_or("(rôle)")), t(" à "), b(target())]);
        }
        "member_role_remove" => {
            s.extend([t("a retiré le rôle "), b(detail(e, "role").unwrap_or("(rôle)")), t(" à "), b(target())]);
        }
        "member_kick" => s.extend([t("a expulsé "), b(target())]),
        "member_ban" => s.extend([t("a banni "), b(target())]),
        "member_unban" => s.extend([t("a levé le bannissement de "), b(target())]),
        "member_timeout" => {
            s.extend([t("a exclu "), b(target())]);
            match detail(e, "seconds").and_then(|v| v.parse::<u64>().ok()) {
                Some(secs) => s.push(t(format!(" pendant {}", span::duration(secs)))),
                None => s.push(t(" temporairement")),
            }
        }
        "member_timeout_remove" => s.extend([t("a levé l'exclusion de "), b(target())]),
        "owner_transfer" => s.extend([t("a transféré la propriété du serveur à "), b(target())]),
        "role_create" => s.extend([t("a créé le rôle "), b(target())]),
        "role_update" => s.extend([t("a modifié le rôle "), b(target())]),
        "role_delete" => s.extend([t("a supprimé le rôle "), b(target())]),
        "role_reorder" => s.push(t("a changé l'ordre des rôles")),
        "channel_create" => {
            s.push(t("a créé "));
            s.extend(channel_phrase(detail(e, "kind"), &target()));
        }
        "channel_update" => s.extend([t("a modifié le salon "), b(channel())]),
        "channel_delete" => {
            s.push(t("a supprimé "));
            s.extend(channel_phrase(detail(e, "kind"), &target()));
        }
        "channel_move" => s.extend([t("a déplacé le salon "), b(channel())]),
        "overwrite_update" => {
            let who = detail(e, "subject").unwrap_or(GONE_TARGET);
            let (allow, deny) = (bits(detail(e, "allow")), bits(detail(e, "deny")));
            s.push(t(if allow | deny == 0 { "a retiré les droits propres " } else { "a modifié les droits " }));
            s.push(t(if detail(e, "subject_kind") == Some("role") { "du rôle " } else { "de " }));
            s.extend([b(who), t(" dans le salon "), b(channel())]);
        }
        "invite_create" => {
            s.push(t("a créé une invitation"));
            if let Some(c) = detail(e, "channel") {
                s.extend([t(" pour le salon "), b(format!("#{c}"))]);
            }
        }
        "invite_revoke" => {
            s.push(t("a révoqué une invitation"));
            if let Some(who) = detail(e, "inviter") {
                s.extend([t(" de "), b(who)]);
            }
        }
        "emoji_create" => s.extend([t("a ajouté l'emoji "), b(emoji_code(&target()))]),
        "emoji_update" => match change(e, "name") {
            Some(c) => {
                s.extend([t("a renommé l'emoji "), b(emoji_code(&c.before)), t(" en "), b(emoji_code(&c.after))])
            }
            None => s.extend([t("a modifié l'emoji "), b(emoji_code(&target()))]),
        },
        "emoji_delete" => s.extend([t("a supprimé l'emoji "), b(emoji_code(&target()))]),
        "message_delete" => {
            s.extend([t("a supprimé un message de "), b(target())]);
            if let Some(c) = detail(e, "channel") {
                s.extend([t(" dans le salon "), b(format!("#{c}"))]);
            }
        }
        "guild_update" => match change(e, "name") {
            Some(c) => s.extend([t("a renommé le serveur « "), b(&c.before), t(" » en « "), b(&c.after), t(" »")]),
            None => s.push(t("a modifié le serveur")),
        },
        other => s.extend([t("a effectué une action que cette version ne connaît pas ("), b(other), t(")")]),
    }
    s
}

fn bits(v: Option<&str>) -> u64 {
    v.and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Les noms des droits portés par ces bits, dans l'ordre des réglages d'un rôle.
fn perm_names(bits: u64) -> String {
    CATEGORIES
        .iter()
        .flat_map(|(_, perms)| perms.iter())
        .filter(|(bit, ..)| bits & bit != 0)
        .map(|(_, name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Une ligne de précision sous la phrase (invitation, droits), hors motif.
fn extra(e: &AuditEntry) -> Option<String> {
    match e.action.as_str() {
        "invite_create" => {
            let age = detail(e, "max_age_s").and_then(|v| v.parse::<u64>().ok());
            let uses = detail(e, "max_uses").and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 0);
            let first = match age {
                Some(a) => format!("Valable {}", span::duration(a)),
                None => "Sans date limite".into(),
            };
            Some(match uses {
                Some(1) => format!("{first}, 1 utilisation au plus"),
                Some(n) => format!("{first}, {n} utilisations au plus"),
                None => format!("{first}, utilisations illimitées"),
            })
        }
        "overwrite_update" => {
            let (allow, deny) = (perm_names(bits(detail(e, "allow"))), perm_names(bits(detail(e, "deny"))));
            match (allow.is_empty(), deny.is_empty()) {
                (true, true) => None,
                (false, true) => Some(format!("Autorisé : {allow}")),
                (true, false) => Some(format!("Refusé : {deny}")),
                (false, false) => Some(format!("Autorisé : {allow}. Refusé : {deny}")),
            }
        }
        _ => None,
    }
}

fn field_label(field: &str) -> &'static str {
    match field {
        "name" => "Nom",
        "topic" => "Sujet",
        "slowmode_s" => "Mode lent",
        "private" => "Visibilité",
        "color" => "Couleur",
        "hoist" => "Affiché à part",
        "permissions" => "Droits",
        _ => "Réglage",
    }
}

/// Une valeur de l'avant / après, en français.
fn value_text(field: &str, v: &str) -> String {
    match field {
        "name" if v.is_empty() => "(vide)".into(),
        "topic" if v.is_empty() => "(aucun)".into(),
        "slowmode_s" => crate::channels::slow_label(v.parse().unwrap_or(0)),
        "private" => (if v == "true" { "Privé" } else { "Public" }).into(),
        "hoist" => (if v == "true" { "Oui" } else { "Non" }).into(),
        "color" => match v.parse::<usize>() {
            Ok(n) if (1..=COLOR_NAMES.len()).contains(&n) => COLOR_NAMES[n - 1].into(),
            _ => "Aucune".into(),
        },
        _ => v.to_string(),
    }
}

/// Ce qui change, prêt à être posé dans la ligne : des paires (libellé, valeurs). Pour les droits, le
/// détail de ce qui est ajouté et retiré, pas deux listes entières.
fn diff_rows(changes: &[AuditChange]) -> Vec<(&'static str, Vec<(&'static str, String)>)> {
    changes
        .iter()
        .map(|c| {
            let label = field_label(&c.field);
            if c.field == "permissions" {
                let (old, new) = (bits(Some(&c.before)), bits(Some(&c.after)));
                let mut rows = Vec::new();
                let (added, removed) = (perm_names(new & !old), perm_names(old & !new));
                if !added.is_empty() {
                    rows.push(("Ajoutés", added));
                }
                if !removed.is_empty() {
                    rows.push(("Retirés", removed));
                }
                (label, rows)
            } else {
                (label, vec![("Avant", value_text(&c.field, &c.before)), ("Après", value_text(&c.field, &c.after))])
            }
        })
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

// ───────── dates ─────────

fn js_date(ms: i64) -> js_sys::Date {
    js_sys::Date::new(&(ms as f64).into())
}

/// « 12 novembre 2026 », avec ou sans le jour de la semaine, dans la langue de l'interface.
fn long_date(ms: i64, weekday: bool) -> String {
    let options = js_sys::Object::new();
    let mut parts = vec![("day", "numeric"), ("month", "long"), ("year", "numeric")];
    if weekday {
        parts.insert(0, ("weekday", "long"));
    }
    for (key, value) in parts {
        let _ = js_sys::Reflect::set(&options, &JsValue::from_str(key), &JsValue::from_str(value));
    }
    js_date(ms).to_locale_date_string("fr-FR", &options).into()
}

/// « mercredi 12 novembre 2026 à 14:30 » : l'heure suit le choix 12 / 24 h.
fn full_date(ms: i64) -> String {
    format!("{} à {}", long_date(ms, true), clock(ms))
}

/// Le jour, pour regrouper : la date locale sans l'heure.
fn day_key(ms: i64) -> String {
    js_date(ms).to_date_string().into()
}

fn day_label(ms: i64, now: i64) -> String {
    let today = js_date(now);
    let yesterday = js_date(now);
    yesterday.set_date(today.get_date() - 1);
    let key = day_key(ms);
    if today.to_date_string() == key {
        "Aujourd'hui".into()
    } else if yesterday.to_date_string() == key {
        "Hier".into()
    } else {
        long_date(ms, true)
    }
}

#[derive(Clone, PartialEq)]
struct Day {
    label: String,
    items: Vec<AuditEntry>,
}

/// Les entrées (déjà du plus récent au plus ancien) rangées par jour.
fn by_day(entries: &[AuditEntry], now: i64) -> Vec<Day> {
    let mut days: Vec<(String, Day)> = Vec::new();
    for e in entries {
        let key = day_key(e.created_at);
        match days.last_mut() {
            Some((k, d)) if *k == key => d.items.push(e.clone()),
            _ => days.push((key, Day { label: day_label(e.created_at, now), items: vec![e.clone()] })),
        }
    }
    days.into_iter().map(|(_, d)| d).collect()
}

/// L'heure d'un événement : « il y a 5 min » s'il est tout récent, sinon l'heure du jour (12 ou 24 h selon
/// le choix). La date et l'heure complètes sont en infobulle et lues telles quelles par un lecteur d'écran.
fn when_view(ms: i64) -> impl IntoView {
    let shown = span::ago(now_ms() - ms).unwrap_or_else(|| clock(ms));
    let full = full_date(ms);
    let title = full.clone();
    view! {
        <time datetime=iso(ms) title=title>
            <span aria-hidden="true">{shown}</span>
            <span class="k-sr">{full}</span>
        </time>
    }
}

fn user_of(a: &AuditActor) -> User {
    User {
        id: a.id,
        username: a.name.clone(),
        display_name: None,
        avatar: a.avatar.clone(),
        banner: None,
        bio: None,
        pronouns: None,
        created_at: None,
    }
}

fn segs_view(segs: Vec<Seg>) -> impl IntoView {
    segs.into_iter()
        .map(|s| match s {
            Seg::T(x) => x.into_any(),
            Seg::B(x) => view! { <b>{x}</b> }.into_any(),
        })
        .collect_view()
}

/// Donne le focus à la ligne de cet événement (la première arrivée d'une page de plus).
fn focus_row(id: Id) {
    spawn_local(async move {
        TimeoutFuture::new(60).await;
        let sel = format!("[data-audit=\"{id}\"]");
        if let Some(row) =
            document().query_selector(&sel).ok().flatten().and_then(|r| r.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = row.focus();
        }
    });
}

fn focus_by_id(id: &'static str) {
    spawn_local(async move {
        TimeoutFuture::new(60).await;
        if let Some(el) = document().get_element_by_id(id).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    });
}

fn count_text(n: usize, more: bool) -> String {
    match (n, more) {
        (0, _) => "Aucun événement".into(),
        (1, false) => "1 événement".into(),
        (n, false) => format!("{n} événements"),
        (n, true) => format!("{n} événements affichés"),
    }
}

#[component]
pub fn AuditLog(guild: Id) -> impl IntoView {
    let category = RwSignal::new(String::new());
    let actor = RwSignal::new(String::new());
    let entries = RwSignal::new(Vec::<AuditEntry>::new());
    let next = RwSignal::new(None::<Id>);
    let actors = RwSignal::new(Vec::<AuditActor>::new());
    let phase = RwSignal::new(Phase::Loading);
    let busy = RwSignal::new(false);
    let more_error = RwSignal::new(None::<String>);
    let note = RwSignal::new(None::<String>);
    // Numéro de la dernière demande « depuis le début » : une réponse qui arrive après un changement de
    // filtre n'écrase rien.
    let seq = StoredValue::new(0u32);

    // `true` : la page suivante, sinon tout depuis le début (premier affichage, filtre, « Actualiser »).
    let fetch = move |more: bool| {
        if more {
            if busy.get_untracked() {
                return;
            }
            busy.set(true);
            more_error.set(None);
        } else {
            seq.update_value(|s| *s += 1);
            phase.set(Phase::Loading);
            entries.set(Vec::new());
            next.set(None);
            note.set(None);
            busy.set(false);
            more_error.set(None);
        }
        let me = seq.get_value();
        let before = if more { next.get_untracked() } else { None };
        let (c, a) = (category.get_untracked(), actor.get_untracked());
        spawn_local(async move {
            let result = crate::api::audit_log(guild, before, &c, &a).await;
            if seq.try_get_value() != Some(me) {
                return;
            }
            match result {
                Ok(page) if more => {
                    let first = page.entries.first().map(|e| e.id);
                    let n = page.entries.len();
                    entries.try_update(|v| v.extend(page.entries));
                    next.try_set(page.next);
                    note.try_set(Some(match n {
                        1 => "1 événement de plus".to_string(),
                        n => format!("{n} événements de plus"),
                    }));
                    busy.try_set(false);
                    if let Some(id) = first {
                        focus_row(id);
                    }
                }
                Ok(page) => {
                    entries.try_set(page.entries);
                    next.try_set(page.next);
                    actors.try_set(page.actors);
                    phase.try_set(Phase::Ready);
                }
                Err(e) if more => {
                    more_error.try_set(Some(e.message));
                    busy.try_set(false);
                }
                Err(e) => {
                    phase.try_set(Phase::Failed(e.message));
                }
            }
        });
    };
    fetch(false);

    let filtered = Memo::new(move |_| !category.get().is_empty() || !actor.get().is_empty());
    let reset = move |_| {
        category.set(String::new());
        actor.set(String::new());
        fetch(false);
        focus_by_id("audit-category");
    };
    let groups = Memo::new(move |_| {
        let now = now_ms();
        entries.with(|v| by_day(v, now))
    });
    let status = move || match phase.get() {
        Phase::Loading => "Chargement du journal…".to_string(),
        Phase::Failed(_) => String::new(),
        Phase::Ready => {
            let base = count_text(entries.with(Vec::len), next.with(Option::is_some));
            match note.get() {
                Some(n) => format!("{n} · {base}"),
                None => base,
            }
        }
    };

    view! {
        <div class="app-audit">
            <p class="app-hint">"Les actions de gestion et de modération de ce serveur, de la plus récente à la plus ancienne. Le journal est en lecture seule : rien ne s'y efface."</p>
            <form class="app-audit-filters" role="search" aria-label="Filtrer le journal d'audit" on:submit=|e| e.prevent_default()>
                <div class="k-field">
                    <label class="k-label" for="audit-category">"Action"</label>
                    <select id="audit-category" class="k-select" on:change=move |e| {
                        category.set(event_target_value(&e));
                        fetch(false);
                    }>
                        <option value="" prop:selected=move || category.get().is_empty()>"Toutes"</option>
                        {KINDS.iter().map(|(slug, label)| view! {
                            <option value=*slug prop:selected=move || category.get() == *slug>{*label}</option>
                        }).collect_view()}
                    </select>
                </div>
                <div class="k-field">
                    <label class="k-label" for="audit-actor">"Membre"</label>
                    <select id="audit-actor" class="k-select" on:change=move |e| {
                        actor.set(event_target_value(&e));
                        fetch(false);
                    }>
                        <option value="" prop:selected=move || actor.get().is_empty()>"Tous"</option>
                        {move || actors.get().into_iter().map(|a| {
                            let id = a.id.to_string();
                            let mine = id.clone();
                            view! { <option value=id prop:selected=move || actor.get() == mine>{a.name}</option> }
                        }).collect_view()}
                    </select>
                </div>
                <div class="app-audit-buttons">
                    <Show when=move || filtered.get()>
                        <button type="button" class="k-btn k-btn--ghost" on:click=reset>"Réinitialiser"</button>
                    </Show>
                    <button type="button" class="k-btn k-btn--ghost" on:click=move |_| fetch(false)>
                        <Icon icon=lu::LuRefreshCw/>"Actualiser"
                    </button>
                </div>
            </form>
            // Toujours là (une zone `status` doit exister avant son message) et de hauteur réservée : rien ne saute.
            <p class="k-hint app-audit-count" role="status">{status}</p>
            {move || match phase.get() {
                Phase::Loading => view! {
                    <div class="app-audit-skels" aria-hidden="true">
                        {(0..5).map(|_| view! { <div class="k-skeleton app-audit-skel"></div> }).collect_view()}
                    </div>
                }.into_any(),
                Phase::Failed(message) => view! {
                    <div class="k-empty">
                        <p class="k-error" role="alert">{message}</p>
                        <button type="button" class="k-btn" on:click=move |_| fetch(false)>"Réessayer"</button>
                    </div>
                }.into_any(),
                Phase::Ready if entries.with_untracked(Vec::is_empty) => view! {
                    <div class="k-empty">
                        <b>{if filtered.get_untracked() { "Aucun événement ne correspond" } else { "Aucun événement" }}</b>
                        <span>{if filtered.get_untracked() {
                            "Essaie un autre type d'action ou un autre membre."
                        } else {
                            "Les actions de gestion et de modération du serveur apparaîtront ici."
                        }}</span>
                        {filtered.get_untracked().then(|| view! {
                            <button type="button" class="k-btn" on:click=reset>"Réinitialiser"</button>
                        })}
                    </div>
                }.into_any(),
                Phase::Ready => view! {
                    <For each=move || groups.get() key=|d| (d.label.clone(), d.items.len()) let:day>
                        <div class="app-audit-day">
                            <h3>{day.label.clone()}</h3>
                            <ul class="app-list app-list--col app-audit-list" aria-label="Événements du journal">
                                {day.items.into_iter().map(|e| view! { <Row entry=e/> }).collect_view()}
                            </ul>
                        </div>
                    </For>
                    <Show when=move || more_error.get().is_some()>
                        <p class="k-error" role="alert">{move || more_error.get()}</p>
                    </Show>
                    <Show when=move || next.get().is_some()>
                        <div class="app-audit-more">
                            <button type="button" class="k-btn" aria-busy=move || busy.get().then_some("true")
                                on:click=move |_| fetch(true)>"Charger plus"</button>
                        </div>
                    </Show>
                }.into_any(),
            }}
        </div>
    }
}

/// Un événement : l'avatar de l'auteur, la phrase, l'heure, le motif, et si l'on a l'avant / après de ce
/// qui a changé un bouton qui le déplie dans la ligne.
#[component]
fn Row(entry: AuditEntry) -> impl IntoView {
    let expanded = RwSignal::new(false);
    let id = entry.id;
    let (icon, danger) = visual(&entry.action);
    let who = entry.actor.as_ref().map_or_else(|| GONE_ACTOR.to_string(), |a| a.name.clone());
    let avatar = match &entry.actor {
        Some(a) => view! { <span aria-hidden="true"><Avatar user=user_of(a) size="md"/></span> }.into_any(),
        None => {
            view! { <span class="k-avatar k-avatar--md" aria-hidden="true"><Icon icon=lu::LuUser/></span> }.into_any()
        }
    };
    let sentence = {
        let mut s = vec![b(who)];
        s.push(t(" "));
        s.extend(predicate(&entry));
        s
    };
    let reason = entry.reason.clone().filter(|r| !r.is_empty());
    let extra = extra(&entry);
    let rows = diff_rows(&entry.changes);
    let diff_id = format!("audit-diff-{id}");
    let has_diff = !rows.is_empty();
    view! {
        <li class="app-row app-audit-row" data-audit=id.to_string() tabindex="-1">
            {avatar}
            <div>
                <p class="app-audit-sentence">{segs_view(sentence)}</p>
                <small class="app-audit-when">{when_view(entry.created_at)}</small>
                {extra.map(|x| view! { <small class="app-ban-reason">{x}</small> })}
                {reason.map(|r| view! { <small class="app-ban-reason">"Raison : "{r}</small> })}
                {has_diff.then(|| {
                    let controls = diff_id.clone();
                    view! {
                        <button type="button" class="k-btn k-btn--ghost k-btn--sm app-audit-toggle"
                            aria-expanded=move || expanded.get().to_string() aria-controls=controls
                            on:click=move |_| expanded.update(|e| *e = !*e)>
                            {move || if expanded.get() { "Masquer les changements" } else { "Voir les changements" }}
                        </button>
                    }
                })}
                {has_diff.then(|| view! {
                    <dl class="app-audit-diff" id=diff_id.clone() hidden=move || !expanded.get()>
                        {rows.into_iter().map(|(label, values)| view! {
                            <div>
                                <dt>{label}</dt>
                                {values.into_iter().map(|(which, v)| view! {
                                    <dd><span class="app-audit-which">{which}</span><span>{v}</span></dd>
                                }).collect_view()}
                            </div>
                        }).collect_view()}
                    </dl>
                })}
            </div>
            <span class="app-audit-icon" data-danger=danger.then_some("") aria-hidden="true"><Icon icon=icon/></span>
        </li>
    }
}
