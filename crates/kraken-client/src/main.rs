//! Client KRAKEN : Leptos en WASM, servi par `kraken-server`. Les styles viennent de
//! `replica/design` (jetons + primitives `k-*`) et de `style/app.css` (mise en page).

mod api;
mod app;
mod audit;
mod auth;
mod avatar;
mod call;
mod channels;
mod chat;
mod display;
mod emoji;
mod emoji_settings;
mod files;
mod guild;
mod home;
mod i18n;
mod icon_picker;
mod icons;
mod legal;
mod markdown;
mod new_guild;
mod overview;
mod photo;
mod pins;
mod prefs;
mod profile;
mod settings;
mod shell;
mod shortcuts;
mod social;
mod store;
mod switcher;
mod ui;
mod unread;
mod update;
mod voice;
mod voice_settings;

fn main() {
    console_error_panic_hook::set_once();
    // La langue d'abord : `<html lang>` et les textes du premier rendu.
    i18n::init();
    leptos::mount::mount_to_body(app::App);
}
