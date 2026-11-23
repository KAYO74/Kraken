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
use kraken_proto::{AuditActor, AuditChange, AuditEntry, AuditTarget, Id, User};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};

use crate::guild::{CATEGORIES, COLOR_NAMES, label};
use crate::i18n::{t, tf, tn, tnf};
use crate::icons::{Icon, lu};
use crate::tk;
use crate::ui::{Avatar, clock, iso, now_ms};

/// Les types d'action du filtre, dans l'ordre de la liste : le mot de l'adresse, et son libellé.
const KINDS: [(&str, &str); 6] = [
    ("salons", tk!("Salons")),
    ("roles", tk!("Rôles")),
    ("membres", tk!("Membres")),
    ("invitations", tk!("Invitations")),
    ("serveur", tk!("Serveur")),
    ("emojis", tk!("Emojis")),
];

/// Ce que dit la personne dont le compte n'existe plus.
fn gone_actor() -> String {
    t("Membre supprimé")
}

fn gone_target() -> String {
    t("(supprimé)")
}

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

fn seg(s: impl Into<String>) -> Seg {
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
        "message_pin" => (lu::LuPin, false),
        "message_unpin" => (lu::LuPinOff, false),
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
    target.as_ref().and_then(|x| x.name.clone()).unwrap_or_else(gone_target)
}

/// Une phrase du journal d'après son gabarit : le français est la clé, traduit en entier (jamais des morceaux
/// recollés, l'ordre des mots change d'une langue à l'autre). Chaque `{nom}` du gabarit prend sa valeur : un nom
/// propre en gras (`b`), le reste en texte (`seg`). Le gabarit est marqué `tk!` pour le contrôle des traductions.
fn phrase(key: &str, vars: &[(&str, Seg)]) -> Vec<Seg> {
    let template = t(key);
    let mut out = Vec::new();
    let mut rest = template.as_str();
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let found = after.find('}').and_then(|end| {
            let name = &after[..end];
            vars.iter().find(|(k, _)| *k == name).map(|(_, v)| (end, v.clone()))
        });
        match found {
            Some((end, value)) => {
                if open > 0 {
                    out.push(seg(&rest[..open]));
                }
                out.push(value);
                rest = &after[end + 1..];
            }
            None => {
                out.push(seg(&rest[..=open]));
                rest = after;
            }
        }
    }
    if !rest.is_empty() {
        out.push(seg(rest));
    }
    out
}

/// « a créé le salon vocal Cartes », « a supprimé la catégorie Archives » : d'après le type gardé à l'écriture,
/// sinon un salon.
fn channel_sentence(created: bool, kind: Option<&str>, who: Seg, name: &str) -> Vec<Seg> {
    let hashed = || b(format!("#{name}"));
    let (key, value) = match (created, kind) {
        (true, Some("voice")) => (tk!("{auteur} a créé le salon vocal {nom}"), b(name)),
        (true, Some("category")) => (tk!("{auteur} a créé la catégorie {nom}"), b(name)),
        (true, Some("announcement")) => (tk!("{auteur} a créé le salon d'annonces {nom}"), hashed()),
        (true, Some("forum")) => (tk!("{auteur} a créé le forum {nom}"), hashed()),
        (true, Some("stage")) => (tk!("{auteur} a créé la scène {nom}"), b(name)),
        (true, _) => (tk!("{auteur} a créé le salon {nom}"), hashed()),
        (false, Some("voice")) => (tk!("{auteur} a supprimé le salon vocal {nom}"), b(name)),
        (false, Some("category")) => (tk!("{auteur} a supprimé la catégorie {nom}"), b(name)),
        (false, Some("announcement")) => (tk!("{auteur} a supprimé le salon d'annonces {nom}"), hashed()),
        (false, Some("forum")) => (tk!("{auteur} a supprimé le forum {nom}"), hashed()),
        (false, Some("stage")) => (tk!("{auteur} a supprimé la scène {nom}"), b(name)),
        (false, _) => (tk!("{auteur} a supprimé le salon {nom}"), hashed()),
    };
    phrase(key, &[("auteur", who), ("nom", value)])
}

fn emoji_code(name: &str) -> String {
    format!(":{name}:")
}

/// La phrase complète de l'événement, auteur compris : « lou a banni sam ».
fn predicate(e: &AuditEntry, who: Seg) -> Vec<Seg> {
    let target = || target_name(&e.target);
    let channel = || {
        let n = target();
        if e.target.as_ref().is_some_and(|x| x.name.is_some()) { format!("#{n}") } else { n }
    };
    let role = || detail(e, "role").map_or_else(|| t("(rôle)"), str::to_string);
    let at = |name: &str| format!("#{name}");
    let auteur = ("auteur", who.clone());
    match e.action.as_str() {
        "member_role_add" => phrase(
            tk!("{auteur} a ajouté le rôle {role} à {cible}"),
            &[auteur, ("role", b(role())), ("cible", b(target()))],
        ),
        "member_role_remove" => phrase(
            tk!("{auteur} a retiré le rôle {role} à {cible}"),
            &[auteur, ("role", b(role())), ("cible", b(target()))],
        ),
        "member_kick" => phrase(tk!("{auteur} a expulsé {cible}"), &[auteur, ("cible", b(target()))]),
        "member_ban" => phrase(tk!("{auteur} a banni {cible}"), &[auteur, ("cible", b(target()))]),
        "member_unban" => phrase(tk!("{auteur} a levé le bannissement de {cible}"), &[auteur, ("cible", b(target()))]),
        "member_timeout" => match detail(e, "seconds").and_then(|v| v.parse::<u64>().ok()) {
            Some(secs) => phrase(
                tk!("{auteur} a exclu {cible} pendant {duree}"),
                &[auteur, ("cible", b(target())), ("duree", seg(crate::i18n::duration(secs)))],
            ),
            None => phrase(tk!("{auteur} a exclu {cible} temporairement"), &[auteur, ("cible", b(target()))]),
        },
        "member_timeout_remove" => {
            phrase(tk!("{auteur} a levé l'exclusion de {cible}"), &[auteur, ("cible", b(target()))])
        }
        "owner_transfer" => {
            phrase(tk!("{auteur} a transféré la propriété du serveur à {cible}"), &[auteur, ("cible", b(target()))])
        }
        "role_create" => phrase(tk!("{auteur} a créé le rôle {cible}"), &[auteur, ("cible", b(target()))]),
        "role_update" => phrase(tk!("{auteur} a modifié le rôle {cible}"), &[auteur, ("cible", b(target()))]),
        "role_delete" => phrase(tk!("{auteur} a supprimé le rôle {cible}"), &[auteur, ("cible", b(target()))]),
        "role_reorder" => phrase(tk!("{auteur} a changé l'ordre des rôles"), &[auteur]),
        "channel_create" => channel_sentence(true, detail(e, "kind"), who, &target()),
        "channel_update" => phrase(tk!("{auteur} a modifié le salon {salon}"), &[auteur, ("salon", b(channel()))]),
        "channel_delete" => channel_sentence(false, detail(e, "kind"), who, &target()),
        "channel_move" => phrase(tk!("{auteur} a déplacé le salon {salon}"), &[auteur, ("salon", b(channel()))]),
        "overwrite_update" => {
            let subject = detail(e, "subject").map_or_else(gone_target, str::to_string);
            let (allow, deny) = (bits(detail(e, "allow")), bits(detail(e, "deny")));
            let vars = [auteur, ("sujet", b(subject)), ("salon", b(channel()))];
            let key = match (allow | deny == 0, detail(e, "subject_kind") == Some("role")) {
                (true, true) => tk!("{auteur} a retiré les droits propres du rôle {sujet} dans le salon {salon}"),
                (true, false) => tk!("{auteur} a retiré les droits propres de {sujet} dans le salon {salon}"),
                (false, true) => tk!("{auteur} a modifié les droits du rôle {sujet} dans le salon {salon}"),
                (false, false) => tk!("{auteur} a modifié les droits de {sujet} dans le salon {salon}"),
            };
            phrase(key, &vars)
        }
        "invite_create" => match detail(e, "channel") {
            Some(c) => {
                phrase(tk!("{auteur} a créé une invitation pour le salon {salon}"), &[auteur, ("salon", b(at(c)))])
            }
            None => phrase(tk!("{auteur} a créé une invitation"), &[auteur]),
        },
        "invite_revoke" => match detail(e, "inviter") {
            Some(who) => phrase(tk!("{auteur} a révoqué une invitation de {nom}"), &[auteur, ("nom", b(who))]),
            None => phrase(tk!("{auteur} a révoqué une invitation"), &[auteur]),
        },
        "emoji_create" => {
            phrase(tk!("{auteur} a ajouté l'emoji {emoji}"), &[auteur, ("emoji", b(emoji_code(&target())))])
        }
        "emoji_update" => match change(e, "name") {
            Some(c) => phrase(
                tk!("{auteur} a renommé l'emoji {avant} en {apres}"),
                &[auteur, ("avant", b(emoji_code(&c.before))), ("apres", b(emoji_code(&c.after)))],
            ),
            None => phrase(tk!("{auteur} a modifié l'emoji {emoji}"), &[auteur, ("emoji", b(emoji_code(&target())))]),
        },
        "emoji_delete" => {
            phrase(tk!("{auteur} a supprimé l'emoji {emoji}"), &[auteur, ("emoji", b(emoji_code(&target())))])
        }
        "message_delete" => match detail(e, "channel") {
            Some(c) => phrase(
                tk!("{auteur} a supprimé un message de {cible} dans le salon {salon}"),
                &[auteur, ("cible", b(target())), ("salon", b(at(c)))],
            ),
            None => phrase(tk!("{auteur} a supprimé un message de {cible}"), &[auteur, ("cible", b(target()))]),
        },
        "message_pin" => match detail(e, "channel") {
            Some(c) => phrase(tk!("{auteur} a épinglé un message dans {salon}"), &[auteur, ("salon", b(at(c)))]),
            None => phrase(tk!("{auteur} a épinglé un message"), &[auteur]),
        },
        "message_unpin" => match detail(e, "channel") {
            Some(c) => phrase(tk!("{auteur} a désépinglé un message dans {salon}"), &[auteur, ("salon", b(at(c)))]),
            None => phrase(tk!("{auteur} a désépinglé un message"), &[auteur]),
        },
        "guild_update" => match (change(e, "name"), detail(e, "icon")) {
            (Some(c), _) => phrase(
                tk!("{auteur} a renommé le serveur « {avant} » en « {apres} »"),
                &[auteur, ("avant", b(&c.before)), ("apres", b(&c.after))],
            ),
            (None, Some("removed")) => phrase(tk!("{auteur} a retiré l'icône du serveur"), &[auteur]),
            (None, Some(_)) => phrase(tk!("{auteur} a changé l'icône du serveur"), &[auteur]),
            (None, None) => phrase(tk!("{auteur} a modifié le serveur"), &[auteur]),
        },
        other => phrase(
            tk!("{auteur} a effectué une action que cette version ne connaît pas ({action})"),
            &[auteur, ("action", b(other))],
        ),
    }
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
        .map(|(_, name, _)| label(name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Une ligne de précision sous la phrase (invitation, droits), hors motif.
fn extra(e: &AuditEntry) -> Option<String> {
    match e.action.as_str() {
        "invite_create" => {
            let age = detail(e, "max_age_s").and_then(|v| v.parse::<u64>().ok()).map(crate::i18n::duration);
            let uses = detail(e, "max_uses").and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 0);
            Some(match (age, uses) {
                (Some(a), Some(n)) => tnf(
                    "Valable {duree}, {n} utilisation au plus",
                    "Valable {duree}, {n} utilisations au plus",
                    n,
                    &[("duree", &a)],
                ),
                (Some(a), None) => tf("Valable {duree}, utilisations illimitées", &[("duree", &a)]),
                (None, Some(n)) => {
                    tn("Sans date limite, {n} utilisation au plus", "Sans date limite, {n} utilisations au plus", n)
                }
                (None, None) => t("Sans date limite, utilisations illimitées"),
            })
        }
        "overwrite_update" => {
            let (allow, deny) = (perm_names(bits(detail(e, "allow"))), perm_names(bits(detail(e, "deny"))));
            match (allow.is_empty(), deny.is_empty()) {
                (true, true) => None,
                (false, true) => Some(tf("Autorisé : {droits}", &[("droits", &allow)])),
                (true, false) => Some(tf("Refusé : {droits}", &[("droits", &deny)])),
                (false, false) => {
                    Some(tf("Autorisé : {autorises}. Refusé : {refuses}", &[("autorises", &allow), ("refuses", &deny)]))
                }
            }
        }
        _ => None,
    }
}

fn field_label(field: &str) -> String {
    t(match field {
        "name" => tk!("Nom"),
        "topic" => tk!("Sujet"),
        "slowmode_s" => tk!("Mode lent"),
        "private" => tk!("Visibilité"),
        "color" => tk!("Couleur"),
        "hoist" => tk!("Affiché à part"),
        "permissions" => tk!("Droits"),
        _ => tk!("Réglage"),
    })
}

/// Une valeur de l'avant / après, dans la langue de l'interface.
fn value_text(field: &str, v: &str) -> String {
    match field {
        "name" if v.is_empty() => t("(vide)"),
        "topic" if v.is_empty() => t("(aucun)"),
        "slowmode_s" => crate::channels::slow_label(v.parse().unwrap_or(0)),
        "private" => {
            if v == "true" {
                t("Privé")
            } else {
                t("Public")
            }
        }
        "hoist" => {
            if v == "true" {
                t("Oui")
            } else {
                t("Non")
            }
        }
        "color" => match v.parse::<usize>() {
            Ok(n) if (1..=COLOR_NAMES.len()).contains(&n) => t(COLOR_NAMES[n - 1]),
            _ => t("Aucune"),
        },
        _ => v.to_string(),
    }
}

/// Ce qui change, prêt à être posé dans la ligne : des paires (libellé, valeurs). Pour les droits, le
/// détail de ce qui est ajouté et retiré, pas deux listes entières.
fn diff_rows(changes: &[AuditChange]) -> Vec<(String, Vec<(String, String)>)> {
    changes
        .iter()
        .map(|c| {
            let label = field_label(&c.field);
            if c.field == "permissions" {
                let (old, new) = (bits(Some(&c.before)), bits(Some(&c.after)));
                let mut rows = Vec::new();
                let (added, removed) = (perm_names(new & !old), perm_names(old & !new));
                if !added.is_empty() {
                    rows.push((t("Ajoutés"), added));
                }
                if !removed.is_empty() {
                    rows.push((t("Retirés"), removed));
                }
                (label, rows)
            } else {
                (
                    label,
                    vec![(t("Avant"), value_text(&c.field, &c.before)), (t("Après"), value_text(&c.field, &c.after))],
                )
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
    js_date(ms).to_locale_date_string(&crate::i18n::locale(), &options).into()
}

/// « mercredi 12 novembre 2026 à 14:30 » : l'heure suit le choix 12 / 24 h.
fn full_date(ms: i64) -> String {
    tf("{date} à {heure}", &[("date", &long_date(ms, true)), ("heure", &clock(ms))])
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
        t("Aujourd'hui")
    } else if yesterday.to_date_string() == key {
        t("Hier")
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
    let shown = crate::i18n::ago(now_ms() - ms).unwrap_or_else(|| clock(ms));
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
            // `bdi` : un nom qui contient un changement de sens d'écriture ne retourne pas la fin de la phrase.
            Seg::B(x) => view! { <b><bdi>{x}</bdi></b> }.into_any(),
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
        (0, _) => t("Aucun événement"),
        (n, false) => tn("{n} événement", "{n} événements", n),
        (n, true) => tn("{n} événement affiché", "{n} événements affichés", n),
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
                    note.try_set(Some(tn("{n} événement de plus", "{n} événements de plus", n)));
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
        Phase::Loading => t("Chargement du journal…"),
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
            <p class="app-hint">{t("Les actions de gestion et de modération de ce serveur, de la plus récente à la plus ancienne. Le journal est en lecture seule : rien ne s'y efface.")}</p>
            <form class="app-audit-filters" role="search" aria-label=t("Filtrer le journal d'audit") on:submit=|e| e.prevent_default()>
                <div class="k-field">
                    <label class="k-label" for="audit-category">{t("Action")}</label>
                    <select id="audit-category" class="k-select" on:change=move |e| {
                        category.set(event_target_value(&e));
                        fetch(false);
                    }>
                        <option value="" prop:selected=move || category.get().is_empty()>{t("Toutes")}</option>
                        {KINDS.iter().map(|(slug, lbl)| view! {
                            <option value=*slug prop:selected=move || category.get() == *slug>{t(lbl)}</option>
                        }).collect_view()}
                    </select>
                </div>
                <div class="k-field">
                    <label class="k-label" for="audit-actor">{t("Membre")}</label>
                    <select id="audit-actor" class="k-select" on:change=move |e| {
                        actor.set(event_target_value(&e));
                        fetch(false);
                    }>
                        <option value="" prop:selected=move || actor.get().is_empty()>{t("Tous")}</option>
                        {move || actors.get().into_iter().map(|a| {
                            let id = a.id.to_string();
                            let mine = id.clone();
                            view! { <option value=id prop:selected=move || actor.get() == mine>{a.name}</option> }
                        }).collect_view()}
                    </select>
                </div>
                <div class="app-audit-buttons">
                    <Show when=move || filtered.get()>
                        <button type="button" class="k-btn k-btn--ghost" on:click=reset>{t("Réinitialiser")}</button>
                    </Show>
                    <button type="button" class="k-btn k-btn--ghost" on:click=move |_| fetch(false)>
                        <Icon icon=lu::LuRefreshCw/>{t("Actualiser")}
                    </button>
                </div>
            </form>
            // Toujours là (une zone `status` doit exister avant son message) et de hauteur réservée : rien ne saute.
            <p class="k-hint app-audit-count" role="status" data-empty=move || (phase.get() == Phase::Ready && entries.with(Vec::is_empty)).then_some("")>{status}</p>
            {move || match phase.get() {
                Phase::Loading => view! {
                    <div class="app-audit-skels" aria-hidden="true">
                        {(0..5).map(|_| view! { <div class="k-skeleton app-audit-skel"></div> }).collect_view()}
                    </div>
                }.into_any(),
                Phase::Failed(message) => view! {
                    <div class="k-empty">
                        <p class="k-error" role="alert">{message}</p>
                        <button type="button" class="k-btn" on:click=move |_| fetch(false)>{t("Réessayer")}</button>
                    </div>
                }.into_any(),
                Phase::Ready if entries.with_untracked(Vec::is_empty) => view! {
                    <div class="k-empty">
                        <b>{if filtered.get_untracked() { t("Aucun événement ne correspond") } else { t("Aucun événement") }}</b>
                        <span>{if filtered.get_untracked() {
                            t("Essaie un autre type d'action ou un autre membre.")
                        } else {
                            t("Les actions de gestion et de modération du serveur apparaîtront ici.")
                        }}</span>
                        {filtered.get_untracked().then(|| view! {
                            <button type="button" class="k-btn" on:click=reset>{t("Réinitialiser")}</button>
                        })}
                    </div>
                }.into_any(),
                Phase::Ready => view! {
                    <For each=move || groups.get() key=|d| (d.label.clone(), d.items.len()) let:day>
                        <div class="app-audit-day">
                            <h3>{day.label.clone()}</h3>
                            <ul class="app-list app-list--col app-audit-list" aria-label=t("Événements du journal")>
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
                                on:click=move |_| fetch(true)>{t("Charger plus")}</button>
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
    let who = entry.actor.as_ref().map_or_else(gone_actor, |a| a.name.clone());
    let avatar = match &entry.actor {
        Some(a) => view! { <span aria-hidden="true"><Avatar user=user_of(a) size="md"/></span> }.into_any(),
        None => {
            view! { <span class="k-avatar k-avatar--md" aria-hidden="true"><Icon icon=lu::LuUser/></span> }.into_any()
        }
    };
    let sentence = predicate(&entry, b(who));
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
                {extra.map(|x| view! { <small class="app-ban-reason"><bdi>{x}</bdi></small> })}
                {reason.map(|r| view! { <small class="app-ban-reason">{t("Raison :")}" "<bdi>{r}</bdi></small> })}
                {has_diff.then(|| {
                    let controls = diff_id.clone();
                    view! {
                        <button type="button" class="k-btn k-btn--ghost k-btn--sm app-audit-toggle"
                            aria-expanded=move || expanded.get().to_string() aria-controls=controls
                            on:click=move |_| expanded.update(|e| *e = !*e)>
                            {move || if expanded.get() {
                                view! { <Icon icon=lu::LuChevronUp small=true/>{t("Masquer les changements")} }.into_any()
                            } else {
                                view! { <Icon icon=lu::LuChevronDown small=true/>{t("Voir les changements")} }.into_any()
                            }}
                        </button>
                    }
                })}
                {has_diff.then(|| view! {
                    <dl class="app-audit-diff" id=diff_id.clone() hidden=move || !expanded.get()>
                        {rows.into_iter().map(|(label, values)| view! {
                            <div>
                                <dt>{label}</dt>
                                {values.into_iter().map(|(which, v)| view! {
                                    <dd><span class="app-audit-which">{which}</span><span><bdi>{v}</bdi></span></dd>
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
