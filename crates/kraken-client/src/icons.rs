//! Icônes Lucide (ISC) rendues dans la classe `k-icon`.

use leptos::prelude::*;

pub use icondata_lu as lu;

/// Les icônes qui indiquent un sens (flèche, chevron, répondre, envoyer, sortir, coin fléché, lien externe, panneau latéral,
/// annuler) : en droite-à-gauche, `Icon` leur ajoute la classe `k-flip`, qui les retourne en miroir (components.css). Les autres
/// (lecture, micro, cloche, croix, coche…) ne se retournent pas. Une nouvelle icône fléchée : l'ajouter ici.
const DIRECTIONAL: &[icondata_core::Icon] = &[
    lu::LuArrowLeft,
    lu::LuArrowRight,
    lu::LuArrowLeftToLine,
    lu::LuArrowRightToLine,
    lu::LuChevronLeft,
    lu::LuChevronRight,
    lu::LuChevronsLeft,
    lu::LuChevronsRight,
    lu::LuCornerDownLeft,
    lu::LuCornerDownRight,
    lu::LuCornerUpLeft,
    lu::LuCornerUpRight,
    lu::LuReply,
    lu::LuReplyAll,
    lu::LuForward,
    lu::LuSend,
    lu::LuSendHorizontal,
    lu::LuLogIn,
    lu::LuLogOut,
    lu::LuExternalLink,
    lu::LuUndo,
    lu::LuRedo,
    lu::LuPanelLeft,
    lu::LuPanelRight,
    lu::LuPanelLeftOpen,
    lu::LuPanelLeftClose,
    lu::LuPanelRightOpen,
    lu::LuPanelRightClose,
];

/// Cette icône indique-t-elle un sens (donc se retourne en RTL) ?
pub fn directional(icon: icondata_core::Icon) -> bool {
    DIRECTIONAL.iter().any(|d| std::ptr::eq(*d, icon))
}

#[component]
pub fn Icon(icon: icondata_core::Icon, #[prop(optional)] small: bool, #[prop(optional)] flip: bool) -> impl IntoView {
    let class = match (small, flip || directional(icon)) {
        (false, false) => "k-icon",
        (true, false) => "k-icon k-icon--sm",
        (false, true) => "k-icon k-flip",
        (true, true) => "k-icon k-icon--sm k-flip",
    };
    view! {
        <svg
            class=class
            viewBox=icon.view_box.unwrap_or("0 0 24 24")
            aria-hidden="true"
            inner_html=icon.data
        ></svg>
    }
}
