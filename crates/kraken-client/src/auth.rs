//! S28 connexion et inscription ; page d'invitation (F06).

use kraken_proto::Invite;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};

use crate::shell::first_text;
use crate::store::store;

/// Où aller après la connexion : `?next=` s'il pointe dans l'appli, sinon l'accueil.
fn next_path() -> Memo<String> {
    let q = use_query_map();
    Memo::new(move |_| {
        q.read()
            .get("next")
            .filter(|n| n.starts_with('/') && !n.starts_with("//"))
            .unwrap_or_else(|| "/channels/@me".into())
    })
}

#[component]
fn AuthCard(title: &'static str, sub: &'static str, children: Children) -> impl IntoView {
    view! {
        <main class="app-center app-auth">
            <div class="k-dialog app-auth-card">
                <header><h1>{title}</h1><p class="app-hint">{sub}</p></header>
                {children()}
            </div>
        </main>
    }
}

#[component]
pub fn Login() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let next = next_path();
    let login = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let nav = navigate.clone();
    Effect::new(move || {
        if s.token.get().is_some() {
            nav(&next.get_untracked(), NavigateOptions { replace: true, ..Default::default() });
        }
    });
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match crate::api::login(&login.get_untracked(), &password.get_untracked()).await {
                Ok(a) => s.sign_in(a),
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    view! {
        <AuthCard title="Content de te revoir" sub="Connecte-toi pour retrouver tes serveurs.">
            <form on:submit=submit>
                <div class="app-form">
                    <label class="k-field"><span class="k-label">"Pseudo ou e-mail"</span>
                        <input class="k-input" name="login" autocomplete="username" required bind:value=login autofocus/></label>
                    <label class="k-field"><span class="k-label">"Mot de passe"</span>
                        <input class="k-input" name="password" type="password" autocomplete="current-password" required bind:value=password
                            aria-invalid=move || error.get().is_some().then_some("true") aria-describedby="login-e"/></label>
                    <Show when=move || error.get().is_some()><p class="k-error" id="login-e" role="alert">{move || error.get()}</p></Show>
                </div>
                <footer class="app-auth-foot">
                    <a href=move || format!("/register?next={}", next.get())>"Pas encore de compte ? S'inscrire"</a>
                    <button class="k-btn k-btn--primary" type="submit" aria-busy=move || busy.get().to_string() disabled=move || busy.get()>"Se connecter"</button>
                </footer>
            </form>
        </AuthCard>
    }
}

#[component]
pub fn Register() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let next = next_path();
    let email = RwSignal::new(String::new());
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    Effect::new(move || {
        if s.token.get().is_some() {
            navigate(&next.get_untracked(), NavigateOptions { replace: true, ..Default::default() });
        }
    });
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let r = crate::api::register(&username.get_untracked(), &email.get_untracked(), &password.get_untracked())
                .await;
            match r {
                Ok(a) => s.sign_in(a),
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    view! {
        <AuthCard title="Créer un compte" sub="Gratuit, sans abonnement, tout compris.">
            <form on:submit=submit>
                <div class="app-form">
                    <label class="k-field"><span class="k-label">"E-mail"</span>
                        <input class="k-input" type="email" autocomplete="email" required bind:value=email autofocus/></label>
                    <label class="k-field"><span class="k-label">"Pseudo"</span>
                        <input class="k-input" autocomplete="username" required minlength="2" maxlength="32" pattern="[a-zA-Z0-9_.]+" bind:value=username aria-describedby="reg-h"/>
                        <span class="app-hint" id="reg-h">"Lettres, chiffres, _ et . ; 2 à 32 caractères."</span></label>
                    <label class="k-field"><span class="k-label">"Mot de passe"</span>
                        <input class="k-input" type="password" autocomplete="new-password" required minlength="8" bind:value=password/></label>
                    <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                </div>
                <footer class="app-auth-foot">
                    <a href=move || format!("/login?next={}", next.get())>"Déjà inscrit ? Se connecter"</a>
                    <button class="k-btn k-btn--primary" type="submit" aria-busy=move || busy.get().to_string() disabled=move || busy.get()>"Créer mon compte"</button>
                </footer>
            </form>
        </AuthCard>
    }
}

#[component]
pub fn InvitePage() -> impl IntoView {
    let s = store();
    let params = use_params_map();
    let code = move || params.read().get("code").unwrap_or_default();
    let invite = RwSignal::new(None::<Result<Invite, String>>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    Effect::new(move || {
        let c = code();
        spawn_local(async move { invite.set(Some(crate::api::invite(&c).await.map_err(|e| e.message))) });
    });
    let navigate = use_navigate();
    let join = move |_| {
        let navigate = navigate.clone();
        busy.set(true);
        spawn_local(async move {
            match crate::api::join(&code()).await {
                Ok(g) => {
                    let to = format!(
                        "/channels/{}/{}",
                        g.guild.id,
                        first_text(&g).map(|c| c.to_string()).unwrap_or_default()
                    );
                    s.upsert_guild(g);
                    navigate(&to, Default::default());
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    let here = move || format!("/invite/{}", code());
    view! {
        <main class="app-center app-auth">
            <div class="k-dialog app-auth-card">
                {move || match invite.get() {
                    None => view! { <div class="app-form" aria-busy="true"><div class="k-skeleton app-skel-block"></div></div> }.into_any(),
                    Some(Err(_)) => view! {
                        <header><h1>"Invitation invalide"</h1></header>
                        <div><p>"Ce lien a expiré, a atteint sa limite ou n'a jamais existé. Demande-en un nouveau."</p></div>
                        <footer><a class="k-btn" href="/channels/@me">"Retour à l'accueil"</a></footer>
                    }.into_any(),
                    Some(Ok(i)) => { let join = join.clone(); view! {
                        <header class="app-invite-head">
                            <span class="k-avatar k-avatar--lg" aria-hidden="true">{crate::ui::initials(&i.guild.name)}</span>
                            <p class="app-hint">"Tu es invité à rejoindre"</p>
                            <h1>{i.guild.name.clone()}</h1>
                            <p class="app-hint">{format!("{} membre{}", i.member_count, if i.member_count > 1 { "s" } else { "" })}</p>
                        </header>
                        <Show when=move || error.get().is_some()><div><p class="k-error" role="alert">{move || error.get()}</p></div></Show>
                        <footer>
                            {move || if s.token.get().is_some() {
                                view! { <button class="k-btn k-btn--primary k-btn--lg app-grow" on:click=join.clone() disabled=move || busy.get() aria-busy=move || busy.get().to_string()>"Rejoindre le serveur"</button> }.into_any()
                            } else {
                                view! {
                                    <a class="k-btn" href=move || format!("/login?next={}", here())>"Se connecter"</a>
                                    <a class="k-btn k-btn--primary" href=move || format!("/register?next={}", here())>"Créer un compte"</a>
                                }.into_any()
                            }}
                        </footer>
                    }.into_any() }
                }}
            </div>
        </main>
    }
}
