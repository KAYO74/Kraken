//! Réglages d'affichage de l'appareil : Apparence (S11), Accessibilité (S12), Langue et heure (S14).
//! Chaque choix s'applique tout de suite et reste sur cet appareil (voir `prefs.rs`) ; il n'y a
//! rien à enregistrer. Un aperçu en haut de page montre le résultat en direct.

use kraken_proto::{Id, User};
use leptos::prelude::*;

use crate::i18n::{LangSelect, t};
use crate::prefs::{self, Pref};
use crate::tk;
use crate::ui::{Avatar, clock};

/// Un choix : sa valeur, son nom et ce qu'il fait. Le nom et l'aide sont des textes français que `Choices`
/// traduit à l'affichage (`tk!` les marque pour `scripts/i18n-check.py`).
type Opt = (&'static str, &'static str, &'static str);

const THEMES: &[Opt] = &[
    ("auto", tk!("Automatique (système)"), tk!("Suit le réglage clair ou sombre de ton système.")),
    ("dark", tk!("Sombre"), tk!("Le thème par défaut, doux pour les yeux le soir.")),
    ("light", tk!("Clair"), tk!("Fond blanc, pour les pièces lumineuses.")),
    ("black", tk!("Noir"), tk!("Noir profond, idéal sur les écrans OLED.")),
];
const TEXT_SIZES: &[Opt] = &[
    ("14", tk!("Petit"), "14 px"),
    ("16", tk!("Normal"), "16 px"),
    ("18", tk!("Grand"), "18 px"),
    ("20", tk!("Très grand"), "20 px"),
];
const ZOOMS: &[&str] = &["90", "100", "110", "125", "150"];
const DENSITIES: &[Opt] = &[
    ("comfortable", tk!("Confortable"), tk!("Des messages aérés, avec des avatars de 40 px.")),
    ("compact", tk!("Compacte"), tk!("Des messages resserrés, avec des avatars de 24 px.")),
];
const MOTIONS: &[Opt] = &[
    ("auto", tk!("Automatique"), tk!("Suit le réglage de ton système.")),
    ("reduce", tk!("Réduit"), tk!("Coupe les animations de l'interface et fige les images animées partout.")),
    ("normal", tk!("Normal"), tk!("Garde les animations, même si ton système en demande moins.")),
];
const CONTRASTS: &[Opt] = &[
    ("normal", tk!("Normal"), tk!("Les couleurs d'origine du thème.")),
    ("high", tk!("Élevé"), tk!("Texte plus net (7 pour 1), bordures bien visibles, anneau de focus plus épais.")),
    ("auto", tk!("Automatique"), tk!("Suit ton système, quand il demande un contraste plus élevé.")),
];
const CLOCKS: &[Opt] = &[
    ("auto", tk!("Automatique"), tk!("Suit la langue de l'interface.")),
    ("12", tk!("12 h"), "2:30 PM"),
    ("24", tk!("24 h"), "14:30"),
];

/// Un groupe de boutons radio qui règle `pref`. Les flèches du clavier passent d'un choix à l'autre ;
/// le choix s'applique tout de suite. `on_pick` est appelé après chaque changement. `legend` et `hint` sont des
/// textes français (`tk!`), traduits ici.
#[component]
fn Choices(
    pref: &'static Pref,
    name: &'static str,
    legend: &'static str,
    options: &'static [Opt],
    #[prop(optional)] hint: &'static str,
    #[prop(optional, into)] on_pick: Option<Callback<&'static str>>,
) -> impl IntoView {
    let current = RwSignal::new(prefs::get(pref));
    let described_by = (!hint.is_empty()).then(|| format!("{name}-h"));
    let hint_id = described_by.clone();
    // Trois choix tiennent sur une ligne dans une fenêtre large (style/app.css) : pas de carte seule en dessous.
    let columns = if options.len() == 3 { "k-radios app-choices app-choices--3" } else { "k-radios app-choices" };
    view! {
        <fieldset class="app-choice-group" aria-describedby=described_by>
            <legend class="k-label">{t(legend)}</legend>
            {hint_id.map(|id| view! { <p id=id class="app-hint">{t(hint)}</p> })}
            <div class=columns>
                {options.iter().map(|&(value, label, sub)| {
                    let (label_id, sub_id) = (format!("{name}-{value}-l"), format!("{name}-{value}-d"));
                    view! {
                        <label class="k-radio">
                            <input type="radio" name=name value=value aria-labelledby=label_id.clone() aria-describedby=sub_id.clone()
                                prop:checked=move || current.get() == value
                                on:change=move |_| {
                                    current.set(value);
                                    prefs::set(pref, value);
                                    if let Some(f) = on_pick {
                                        f.run(value);
                                    }
                                }/>
                            <div><b id=label_id>{t(label)}</b><span id=sub_id>{t(sub)}</span></div>
                        </label>
                    }
                }).collect_view()}
            </div>
        </fieldset>
    }
}

/// Deux messages d'exemple qui suivent en direct le thème, la taille du texte, le zoom, la densité, le
/// contraste et les liens. Ce n'est qu'une image : rien dedans ne se focalise ni ne se lit à voix haute.
#[component]
fn Preview() -> impl IntoView {
    let person = |n: u128, name: &str| User {
        id: Id::from_u128(n),
        username: name.to_lowercase(),
        display_name: Some(name.into()),
        avatar: None,
        banner: None,
        bio: None,
        pronouns: None,
        created_at: None,
    };
    let (nacre, recif) = (person(1, "Nacre"), person(5, "Récif"));
    let at = |minutes: u32| {
        let d = js_sys::Date::new_0();
        d.set_hours(14);
        d.set_minutes(minutes);
        d.get_time() as i64
    };
    view! {
        <section class="app-preview" aria-labelledby="preview-t">
            <h3 id="preview-t" class="app-preview-title">{t("Aperçu")}</h3>
            <div class="app-preview-body" aria-hidden="true" inert>
                <div class="k-msg">
                    <Avatar user=nacre/>
                    <div>
                        <header><b>"Nacre"</b><time>{clock(at(30))}</time></header>
                        <p>{t("Salut ! On se retrouve ce soir dans le vocal ?")}</p>
                    </div>
                </div>
                <div class="k-msg">
                    <Avatar user=recif/>
                    <div>
                        <header><b>"Récif"</b><time>{clock(at(32))}</time></header> // i18n-ok : nom d'exemple
                        <p>{t("Avec plaisir, vers 21 h. Voici")}" "<a>{t("le programme de la soirée")}</a>"."</p>
                    </div>
                </div>
            </div>
        </section>
    }
}

/// Réglages › Apparence (S11).
#[component]
pub fn Appearance() -> impl IntoView {
    let zoom = RwSignal::new(prefs::get(&prefs::ZOOM));
    let on_zoom = move |e: leptos::ev::Event| {
        prefs::set(&prefs::ZOOM, &event_target_value(&e));
        zoom.set(prefs::get(&prefs::ZOOM));
    };
    view! {
        <div class="app-prefs">
            <Preview/>
            <p class="app-hint">{t("Ces réglages s'appliquent tout de suite et restent sur cet appareil.")}</p>
            <Choices pref=&prefs::THEME name="theme" legend=tk!("Thème") options=THEMES/>
            <Choices pref=&prefs::TEXT name="text" legend=tk!("Taille du texte") options=TEXT_SIZES
                hint=tk!("Agrandit ou réduit tout le texte de l'appli.")/>
            <div class="app-choice-group">
                <label class="k-field app-pref-select">
                    <span class="k-label">{t("Zoom de l'interface")}</span>
                    <select class="k-select" aria-describedby="zoom-h" prop:disabled=prefs::zoom_locked on:change=on_zoom>
                        {ZOOMS.iter().map(|&z| view! {
                            <option value=z prop:selected=move || zoom.get() == z>{format!("{z} %")}</option>
                        }).collect_view()}
                    </select>
                </label>
                <p id="zoom-h" class="app-hint">
                    {t("Agrandit ou réduit toute l'interface, texte compris. Le zoom est limité par la largeur de la fenêtre.")}
                </p>
            </div>
            <Choices pref=&prefs::DENSITY name="density" legend=tk!("Densité") options=DENSITIES
                hint=tk!("L'espace entre les messages et la taille des avatars.")/>
        </div>
    }
}

/// Réglages › Accessibilité (S12).
#[component]
pub fn Accessibility() -> impl IntoView {
    let underline = RwSignal::new(prefs::get(&prefs::UNDERLINE) == "on");
    let toggle = move |e: leptos::ev::Event| {
        let on = event_target_checked(&e);
        underline.set(on);
        prefs::set(&prefs::UNDERLINE, if on { "on" } else { "off" });
    };
    view! {
        <div class="app-prefs">
            <Preview/>
            <p class="app-hint">{t("Ces réglages s'appliquent tout de suite et restent sur cet appareil.")}</p>
            <Choices pref=&prefs::MOTION name="motion" legend=tk!("Mouvement") options=MOTIONS
                hint=tk!("Les GIF et WebP animés restent fixes dans les listes ; ils bougent au survol, au focus et sur la carte de profil. En mouvement réduit, ils restent fixes partout : « Lire l'animation » les lance sur la carte de profil.")/>
            <Choices pref=&prefs::CONTRAST name="contrast" legend=tk!("Contraste") options=CONTRASTS/>
            <div class="k-setting">
                <label for="underline">{t("Souligner les liens")}</label>
                <input id="underline" type="checkbox" role="switch" class="k-switch" aria-describedby="underline-h"
                    prop:checked=move || underline.get() on:change=toggle/>
                <p id="underline-h">{t("Les liens du texte sont toujours soulignés, pas seulement colorés.")}</p>
            </div>
        </div>
    }
}

/// Réglages › Langue et heure (S14).
#[component]
pub fn Language() -> impl IntoView {
    let tick = RwSignal::new(0u32);
    let sample = {
        let d = js_sys::Date::new_0();
        d.set_hours(14);
        d.set_minutes(30);
        d.get_time() as i64
    };
    view! {
        <div class="app-prefs">
            <LangSelect id="language"/>
            <div class="app-choice-group">
                <Choices pref=&prefs::CLOCK name="clock" legend=tk!("Format de l'heure") options=CLOCKS
                    on_pick=move |_| tick.update(|t| *t += 1)/>
                <p class="app-example" role="status">
                    {t("Exemple :")}" "<time>{move || { tick.track(); clock(sample) }}</time>
                </p>
            </div>
        </div>
    }
}
