//! Interface multilingue : français, anglais, persan (la première langue de droite à gauche) et allemand, prête pour
//! d'autres langues. Mode d'emploi complet et conventions de traduction : `crates/kraken-client/i18n/README.md`.
//!
//! **Le français est la clé.** `t("Ouvrir les réglages")` rend ce texte tel quel en français, et sa traduction
//! (`crates/kraken-client/i18n/<langue>/<groupe>.tsv`) dans une autre langue ; un texte absent du catalogue
//! reste en français, jamais de panne. Le persan et l'allemand ont une chaîne de repli : leur catalogue se construit sur
//! celui de l'anglais (`kraken_core::i18n::Lang::fallback`), donc un texte pas encore traduit s'affiche en anglais,
//! puis en français. `tf` remplace des `{variables}` après la traduction, `tn` choisit le gabarit au
//! singulier ou au pluriel selon la règle de la langue.
//!
//! La langue est lue **une fois**, au démarrage ([`init`], avant le premier rendu), d'après le choix gardé
//! (`kraken.lang` du `localStorage` : `auto`, `fr`, `en`, `fa`, `de`) et, en « Automatique », les langues du navigateur.
//! Changer de langue enregistre le choix et recharge la page ([`choose`]) : rien n'est réactif.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use kraken_core::i18n::{Catalog, Lang, LangPref, Plural, catalog_for, fill, fill_isolated};
use leptos::prelude::*;

/// Le choix de langue, dans le `localStorage` de l'appareil (jamais envoyé au serveur).
const KEY: &str = "kraken.lang";
/// Posé juste avant le rechargement (`sessionStorage`) : `settings` ou `auth`, lu et effacé au démarrage suivant.
const FLAG_KEY: &str = "kraken.lang-flag";
/// Préfixe des valeurs gardées d'un rechargement à l'autre (`sessionStorage`), lues et effacées au démarrage.
const KEEP_PREFIX: &str = "kraken.lang-keep.";

/// Le catalogue anglais, un fichier par groupe (un par domaine, pour qu'on puisse y travailler en parallèle).
/// Un fichier de `i18n/en/` qui n'est pas dans cette liste n'est jamais lu : `scripts/i18n-check.py` le signale.
const EN: &[&str] = &[
    include_str!("../i18n/en/core.tsv"),
    include_str!("../i18n/en/chat.tsv"),
    include_str!("../i18n/en/guilds.tsv"),
    include_str!("../i18n/en/voice.tsv"),
    include_str!("../i18n/en/app.tsv"),
    include_str!("../i18n/en/server.tsv"),
];

/// Le catalogue persan : les mêmes six groupes que l'anglais, chargés **après** lui (voir [`catalog`]).
const FA: &[&str] = &[
    include_str!("../i18n/fa/core.tsv"),
    include_str!("../i18n/fa/chat.tsv"),
    include_str!("../i18n/fa/guilds.tsv"),
    include_str!("../i18n/fa/voice.tsv"),
    include_str!("../i18n/fa/app.tsv"),
    include_str!("../i18n/fa/server.tsv"),
];

/// Le catalogue allemand : les mêmes six groupes que l'anglais, chargés **après** lui (voir [`catalog`]).
const DE: &[&str] = &[
    include_str!("../i18n/de/core.tsv"),
    include_str!("../i18n/de/chat.tsv"),
    include_str!("../i18n/de/guilds.tsv"),
    include_str!("../i18n/de/voice.tsv"),
    include_str!("../i18n/de/app.tsv"),
    include_str!("../i18n/de/server.tsv"),
];

/// Les fichiers de catalogue d'une langue, sans son repli. Le français n'en a pas : ses textes sont les clés.
fn sources(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Fr => &[],
        Lang::En => EN,
        Lang::Fa => FA,
        Lang::De => DE,
    }
}

/// Le catalogue de la langue : celui de son repli d'abord (`Lang::fallback` : l'anglais pour le persan et l'allemand),
/// puis ses propres fichiers par-dessus, la dernière entrée gagnant. Ce que le persan ou l'allemand ne traduit pas est
/// donc en anglais, et ce que l'anglais ne traduit pas reste le texte français de la clé.
fn catalog(lang: Lang) -> Catalog {
    catalog_for(lang, &sources)
}

struct State {
    pref: LangPref,
    lang: Lang,
    catalog: Catalog,
    /// L'étiquette BCP-47 donnée à `Intl` (voir [`locale`]).
    locale: String,
    /// Le rechargement vient d'un changement de langue : rendre le focus au sélecteur, une seule fois.
    focus: Cell<bool>,
    /// Le changement vient des Réglages : dire la nouvelle langue dans la zone d'état, une seule fois.
    announce: Cell<bool>,
    /// Ce que la page d'avant a laissé (identifiant déjà saisi…), lu une seule fois.
    kept: RefCell<HashMap<String, String>>,
}

thread_local! {
    static STATE: State = State::load();
}

fn session() -> Option<web_sys::Storage> {
    web_sys::window()?.session_storage().ok().flatten()
}

fn local() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Les langues du navigateur, par ordre de préférence.
fn browser_languages() -> Vec<String> {
    let Some(nav) = web_sys::window().map(|w| w.navigator()) else { return vec![] };
    let mut out: Vec<String> = nav.languages().iter().filter_map(|v| v.as_string()).collect();
    if out.is_empty()
        && let Some(one) = nav.language()
    {
        out.push(one);
    }
    out
}

/// Une étiquette qui ressemble à une étiquette de langue (lettres, chiffres, tirets), pour ne rien passer d'autre à `Intl`.
fn plausible_tag(tag: &str) -> bool {
    tag.len() >= 2 && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn stored_pref() -> LangPref {
    let read = |s: Option<web_sys::Storage>| s.and_then(|s| s.get_item(KEY).ok().flatten());
    // Le stockage de l'appareil, puis celui de l'onglet (quand le premier refuse l'écriture).
    read(local()).or_else(|| read(session())).and_then(|v| LangPref::from_code(&v)).unwrap_or(LangPref::Auto)
}

impl State {
    fn load() -> Self {
        let pref = stored_pref();
        let browser = browser_languages();
        let lang = pref.resolve(&browser.iter().map(String::as_str).collect::<Vec<_>>());
        let catalog = catalog(lang);
        // L'étiquette de la langue, avec la région du navigateur quand elle va avec (`en-GB` : 24 h, jour avant
        // mois ; `fa-AF` ; `de-AT`, `de-CH`), sinon la langue seule. Chiffres latins partout : l'extension `-u-nu-latn` (le calendrier, lui,
        // reste celui que `Intl` donne à la langue : le calendrier persan pour `fa`).
        let base = browser
            .iter()
            .find(|t| plausible_tag(t) && Lang::from_tag(t) == Some(lang))
            .map(|t| t.split("-u-").next().unwrap_or(t).to_string())
            .unwrap_or_else(|| lang.tag().to_string());
        let locale = format!("{base}-u-nu-latn");
        let (mut focus, mut announce) = (false, false);
        let mut kept = HashMap::new();
        if let Some(s) = session() {
            let flag = s.get_item(FLAG_KEY).ok().flatten();
            focus = flag.is_some();
            announce = flag.as_deref() == Some("settings");
            let _ = s.remove_item(FLAG_KEY);
            let keys: Vec<String> = (0..s.length().unwrap_or(0)).filter_map(|i| s.key(i).ok().flatten()).collect();
            for key in keys.into_iter().filter(|k| k.starts_with(KEEP_PREFIX)) {
                if let Some(v) = s.get_item(&key).ok().flatten() {
                    kept.insert(key[KEEP_PREFIX.len()..].to_string(), v);
                }
                let _ = s.remove_item(&key);
            }
        }
        State {
            pref,
            lang,
            catalog,
            locale,
            focus: Cell::new(focus),
            announce: Cell::new(announce),
            kept: RefCell::new(kept),
        }
    }
}

/// Au démarrage, avant le premier rendu : lit la langue et pose `lang` et `dir` sur `<html>` (`dir="rtl"` en persan ;
/// `boot.js` l'a déjà posé avant le client, avec la même règle).
pub fn init() {
    let lang = lang();
    if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = root.set_attribute("lang", lang.code());
        let _ = root.set_attribute("dir", lang.dir().as_str());
    }
}

/// La page s'écrit-elle de droite à gauche en ce moment ? Lit `document.documentElement.dir` **à chaque appel** : rien n'est gardé
/// en mémoire au démarrage, donc un `dir` changé en cours de route (les tests de mise en page, un futur sélecteur de langue sans
/// rechargement) est suivi à l'événement suivant. À appeler au moment de l'événement ou du rendu, jamais dans un `static`.
pub fn is_rtl() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .and_then(|r| r.get_attribute("dir"))
        .is_some_and(|d| d.trim().eq_ignore_ascii_case("rtl"))
}

/// Un texte qui parle d'une flèche du clavier (`Précédente (←)`) : en RTL, la flèche qui revient en arrière pointe vers la droite.
pub fn mirror_arrows(text: String) -> String {
    if is_rtl() {
        text.chars()
            .map(|c| match c {
                '←' => '→',
                '→' => '←',
                c => c,
            })
            .collect()
    } else {
        text
    }
}

/// Un texte saisi par une personne (nom de rôle, de salon, de serveur, nom affiché) pour un endroit qui ne prend que du texte
/// (`<option>`, `title`) : isolé (U+2068 … U+2069) quand la page est de droite à gauche, tel quel sinon. Dans un nœud du DOM,
/// préfère `<bdi>` autour du texte.
pub fn isolated(text: &str) -> String {
    if is_rtl() { kraken_core::i18n::isolate(text) } else { text.to_string() }
}

/// [`fill`] ; en RTL, [`fill_isolated`] (les noms et pseudos insérés gardent leur propre sens d'écriture).
fn fill_here(template: &str, vars: &[(&str, &str)]) -> String {
    if is_rtl() { fill_isolated(template, vars) } else { fill(template, vars) }
}

/// La langue de l'interface.
pub fn lang() -> Lang {
    STATE.with(|s| s.lang)
}

/// Le choix de l'utilisateur (« Automatique » ou une langue).
pub fn pref() -> LangPref {
    STATE.with(|s| s.pref)
}

/// L'étiquette BCP-47 pour `Intl` (dates, heures, segmentation) : la langue de l'interface, jamais celle du
/// navigateur si elle en diffère. Chiffres latins.
pub fn locale() -> String {
    STATE.with(|s| s.locale.clone())
}

/// Traduit un texte écrit en français : sa traduction dans la langue de l'interface, ou le texte lui-même.
pub fn t(key: &str) -> String {
    STATE.with(|s| s.catalog.get(key).unwrap_or(key).to_string())
}

/// [`t`], puis remplace chaque `{nom}` du gabarit par sa valeur : `tf("{n} événements", &[("n", &n.to_string())])`.
/// Le gabarit français est la clé ; la traduction garde les mêmes `{noms}`.
pub fn tf(key: &str, vars: &[(&str, &str)]) -> String {
    fill_here(&t(key), vars)
}

/// [`tf`] pour un texte qui devient une donnée (valeur d'un champ à envoyer, nom enregistré) : jamais isolé, même en
/// droite à gauche, car les marques d'isolation (U+2066 à U+2069) n'ont rien à faire dans une donnée.
pub fn tf_data(key: &str, vars: &[(&str, &str)]) -> String {
    fill(&t(key), vars)
}

/// Le gabarit au singulier ou au pluriel selon la règle de la langue, traduit, avec `{n}` remplacé :
/// `tn("{n} membre", "{n} membres", n)`. Les deux gabarits français sont deux clés du catalogue.
pub fn tn(one: &str, other: &str, n: impl TryInto<u64>) -> String {
    tnf(one, other, n, &[])
}

/// [`tn`] avec d'autres variables que `{n}`.
pub fn tnf(one: &str, other: &str, n: impl TryInto<u64>, vars: &[(&str, &str)]) -> String {
    let n = n.try_into().unwrap_or(0);
    let key = match lang().plural(n) {
        Plural::One => one,
        Plural::Other => other,
    };
    let count = n.to_string();
    let mut all = vec![("n", count.as_str())];
    all.extend_from_slice(vars);
    fill_here(&t(key), &all)
}

/// Une durée courte (« 5 min », « 1 h 30 », « 2 jours ») dans la langue de l'interface.
pub fn duration(secs: u64) -> String {
    kraken_core::span::duration(lang(), secs)
}

/// Le temps écoulé depuis un événement récent (« à l'instant », « il y a 5 min »), `None` au-delà d'une heure.
pub fn ago(elapsed_ms: i64) -> Option<String> {
    kraken_core::span::ago(lang(), elapsed_ms)
}

/// Pour une table de constantes : rend le texte français tel quel, et le marque comme clé de traduction pour
/// `scripts/i18n-check.py` (qui n'en verrait pas autrement la traduction). À l'affichage : `t(le_texte)`.
///
/// ```ignore
/// const THEMES: &[(&str, &str)] = &[("dark", tk!("Sombre"))];
/// … {t(label)} …
/// ```
#[macro_export]
macro_rules! tk {
    ($text:literal) => {
        $text
    };
}

/// Enregistre le choix de langue et recharge la page. `announce` : le changement vient des Réglages, la page
/// d'après le dit dans la zone d'état. Sans stockage de l'appareil, le choix tient jusqu'à la fermeture de l'onglet.
pub fn choose(pref: LangPref, announce: bool) {
    let saved = local().is_some_and(|s| s.set_item(KEY, pref.code()).is_ok());
    if !saved && let Some(s) = session() {
        let _ = s.set_item(KEY, pref.code());
    }
    if let Some(s) = session() {
        let _ = s.set_item(FLAG_KEY, if announce { "settings" } else { "auth" });
    }
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

/// Garde une valeur (jamais un mot de passe) pour la page d'après un changement de langue ; [`unstash`] la rend.
pub fn stash(name: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    if let Some(s) = session() {
        let _ = s.set_item(&format!("{KEEP_PREFIX}{name}"), value);
    }
}

/// La valeur que [`stash`] a gardée avant le rechargement, une seule fois.
pub fn unstash(name: &str) -> Option<String> {
    STATE.with(|s| s.kept.borrow_mut().remove(name))
}

/// Après un changement de langue fait dans les Réglages : la phrase à dire, une seule fois.
pub fn take_announcement() -> Option<String> {
    STATE.with(|s| s.announce.replace(false)).then(|| tf("Langue : {nom}.", &[("nom", lang().autonym())]))
}

/// Après un changement de langue : le focus doit revenir au sélecteur (vrai une seule fois).
fn take_focus() -> bool {
    STATE.with(|s| s.focus.replace(false))
}

/// Le sélecteur de langue : « Langue de l'appareil » (Automatique), puis chaque langue dans son écriture. Un
/// choix enregistre et recharge la page. Dans les Réglages : l'aide dit que la page se recharge (et qu'un
/// brouillon non envoyé sera perdu), et le sélecteur est grisé pendant un appel, raison dite en toutes lettres.
/// Sur les pages de connexion (`auth`) : jamais grisé, plus compact ; `keep` garde les champs déjà saisis
/// (jamais un mot de passe) avant le rechargement.
#[component]
pub fn LangSelect(
    /// Identifiant du sélecteur ; l'aide est `{id}-h`, la raison de l'appel `{id}-c`.
    id: &'static str,
    #[prop(optional)] auth: bool,
    #[prop(optional, into)] keep: Option<Callback<()>>,
) -> impl IntoView {
    let select = NodeRef::<leptos::html::Select>::new();
    let current = pref();
    // Retour du rechargement : le focus revient au sélecteur, une fois la page posée.
    Effect::new(move || {
        if select.get().is_some() && take_focus() {
            request_animation_frame(move || {
                if let Some(s) = select.try_get_untracked().flatten() {
                    let _ = s.focus();
                }
            });
        }
    });
    // En appel (salon vocal, vidéo, appel de MP), changer de langue couperait la communication.
    let in_call = move || !auth && crate::store::store().voice.room.with(Option::is_some);
    let draft = !auth && crate::chat::has_unsent_draft();
    let on_change = move |e: leptos::ev::Event| {
        let Some(choice) = LangPref::from_code(&event_target_value(&e)) else { return };
        if choice == current || in_call() {
            return;
        }
        if let Some(keep) = keep {
            keep.run(());
        }
        choose(choice, !auth);
    };
    let (help, call) = (format!("{id}-h"), format!("{id}-c"));
    let described = {
        let (help, call) = (help.clone(), call.clone());
        move || if in_call() { format!("{help} {call}") } else { help.clone() }
    };
    let option = |value: &'static str, lang: Option<&'static str>, text: String| {
        view! { <option value=value lang=lang selected=current.code() == value>{text}</option> }
    };
    view! {
        <div class=if auth { "app-lang app-lang--auth" } else { "app-choice-group" }>
            <label class="k-field app-pref-select">
                <span class="k-label">{t("Langue")}</span>
                <select class="k-select" id=id node_ref=select aria-describedby=described disabled=in_call on:change=on_change>
                    {option("auto", None, t("Langue de l'appareil"))}
                    {Lang::ALL.into_iter().map(|l| option(l.code(), Some(l.code()), l.autonym().to_string())).collect_view()}
                </select>
            </label>
            <p id=help class="app-hint">
                {t("Le changement recharge la page.")}
                {draft.then(|| view! { " " {t("Le brouillon non envoyé sera perdu.")} })}
            </p>
            <Show when=in_call>
                <p id=call.clone() class="app-hint">{t("Quitte l'appel pour changer de langue.")}</p>
            </Show>
        </div>
    }
}
