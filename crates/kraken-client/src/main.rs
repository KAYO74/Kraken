//! Client KRAKEN : Leptos en WASM, servi par `kraken-server`. Les styles viennent de
//! `replica/design` (jetons + primitives `k-*`) et de `style/app.css` (mise en page).

mod api;
mod app;
mod auth;
mod chat;
mod home;
mod icons;
mod settings;
mod shell;
mod store;
mod ui;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
