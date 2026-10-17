//! Routes : une par écran de l'inventaire (`replica/recon.md`). Les écrans pas encore
//! construits affichent `Stub` avec leur identifiant.

use leptos::prelude::*;
use leptos_router::components::{ParentRoute, Redirect, Route, Router, Routes};
use leptos_router::path;

use crate::store::{Store, keep_connected};
use crate::ui::{NotFound, Stub};
use crate::{auth, chat, home, settings, shell};

#[component]
pub fn App() -> impl IntoView {
    let store = Store::new();
    provide_context(store);
    keep_connected(store);
    settings::apply_saved_theme();

    view! {
        <Router>
            <Routes fallback=NotFound>
                <Route path=path!("/login") view=auth::Login/>
                <Route path=path!("/register") view=auth::Register/>
                <Route path=path!("/invite/:code") view=auth::InvitePage/>
                <Route path=path!("/forgot-password") view=auth::ForgotPassword/>
                <Route path=path!("/reset-password") view=auth::ResetPassword/>
                <Route path=path!("/verify-email") view=auth::VerifyEmail/>
                <ParentRoute path=path!("") view=shell::Shell>
                    <Route path=path!("") view=|| view! { <Redirect path="/channels/@me"/> }/>
                    <Route path=path!("channels/@me") view=home::Friends/>
                    <Route path=path!("channels/@me/:channel") view=chat::DmView/>
                    <Route path=path!("channels/:guild") view=chat::GuildIndex/>
                    <Route path=path!("channels/:guild/:channel") view=chat::GuildChannel/>
                    <Route path=path!("channels/:guild/:channel/threads") view=|| view! { <Stub id="S24" title="Fils et forum"/> }/>
                    <Route path=path!("channels/:guild/:channel/settings") view=|| view! { <Stub id="S23" title="Paramètres du salon"/> }/>
                    <Route path=path!("guilds/new") view=shell::CreateGuild/>
                    <Route path=path!("guilds/:guild/settings") view=|| view! { <Stub id="S22" title="Paramètres du serveur"/> }/>
                    <Route path=path!("guilds/:guild/events") view=|| view! { <Stub id="S27" title="Événements"/> }/>
                    <Route path=path!("inbox") view=|| view! { <Stub id="S25" title="Boîte de réception"/> }/>
                    <Route path=path!("search") view=|| view! { <Stub id="S26" title="Recherche de messages"/> }/>
                    <Route path=path!("switcher") view=|| view! { <Stub id="S29" title="Aller à une conversation"/> }/>
                    <Route path=path!("discover") view=|| view! { <Stub id="S30" title="Explorer les serveurs"/> }/>
                    <Route path=path!("users/:user") view=|| view! { <Stub id="S20" title="Profil"/> }/>
                    <Route path=path!("settings/:section") view=settings::Settings/>
                </ParentRoute>
            </Routes>
        </Router>
    }
}
