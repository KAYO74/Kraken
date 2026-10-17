//! Icônes Lucide (ISC) rendues dans la classe `k-icon`.

use leptos::prelude::*;

pub use icondata_lu as lu;

#[component]
pub fn Icon(icon: icondata_core::Icon, #[prop(optional)] small: bool) -> impl IntoView {
    view! {
        <svg
            class=if small { "k-icon k-icon--sm" } else { "k-icon" }
            viewBox=icon.view_box.unwrap_or("0 0 24 24")
            aria-hidden="true"
            inner_html=icon.data
        ></svg>
    }
}
