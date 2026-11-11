//! Client KRAKEN : Leptos en WASM, servi par `kraken-server`. Les styles viennent de
//! `replica/design` (jetons + primitives `k-*`) et de `style/app.css` (mise en page).

mod api;
mod app;
mod auth;
mod avatar;
mod call;
mod channels;
mod chat;
mod emoji;
mod files;
mod guild;
mod home;
mod icons;
mod legal;
mod markdown;
mod profile;
mod settings;
mod shell;
mod social;
mod store;
mod ui;
mod unread;
mod update;
mod voice;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
