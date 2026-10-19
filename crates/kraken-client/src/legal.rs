//! Pages Confidentialité et Conditions, lisibles sans compte. L'exploitant de l'instance
//! renseigne son contact et ses sous-traitants (`KRAKEN_CONTACT`, `KRAKEN_PROCESSORS`).

use kraken_proto::Instance;
use leptos::prelude::*;
use leptos::task::spawn_local;

fn instance() -> RwSignal<Instance> {
    let info = RwSignal::new(Instance::default());
    spawn_local(async move {
        if let Ok(i) = crate::api::instance().await {
            info.try_set(i);
        }
    });
    info
}

#[component]
fn Page(title: &'static str, children: Children) -> impl IntoView {
    view! {
        <main class="app-center app-auth">
            <article class="k-dialog app-legal">
                <header><h1>{title}</h1></header>
                <div>{children()}</div>
                <footer><a href="/privacy">"Confidentialité"</a>" · "<a href="/terms">"Conditions"</a>" · "<a href="/login">"Connexion"</a></footer>
            </article>
        </main>
    }
}

#[component]
pub fn Privacy() -> impl IntoView {
    let i = instance();
    view! {
        <Page title="Confidentialité">
            <h2>"Qui s'occupe de tes données"</h2>
            <p>"L'exploitant de cette instance KRAKEN, c'est-à-dire la personne ou l'organisation qui l'héberge. Contact : "
                {move || i.with(|i| i.contact.clone()).unwrap_or_else(|| "non renseigné par l'exploitant".into())}"."</p>
            <h2>"Ce qui est enregistré"</h2>
            <p>"Ton adresse e-mail, ton pseudo, ton nom affiché, ton mot de passe sous forme hachée (argon2id, jamais en clair), \
                tes serveurs et tes messages, et tes sessions (navigateur et date de connexion)."</p>
            <h2>"Ce qui ne l'est pas"</h2>
            <p>"Ni publicité, ni traceur, ni mesure d'audience, ni numéro de téléphone, ni pièce d'identité. \
                Ton adresse IP sert seulement à limiter les essais de connexion : elle reste en mémoire une minute et n'est pas enregistrée."</p>
            <h2>"Cookies"</h2>
            <p>"Un seul, indispensable : celui qui garde ta session ouverte."</p>
            <h2>"Sous-traitants"</h2>
            <p>{move || i.with(|i| i.processors.clone()).unwrap_or_else(|| "L'hébergeur du serveur et, si les e-mails sont activés, \
                le service d'envoi choisis par l'exploitant.".into())}</p>
            <h2>"Durée et suppression"</h2>
            <p>"Tes données restent tant que ton compte existe. Depuis Réglages, Mon compte, tu peux le supprimer : il est fermé \
                tout de suite et effacé au bout de 14 jours. Tes messages restent dans les salons, signés « compte supprimé »."</p>
            <h2>"Tes droits"</h2>
            <p>"Accès, rectification, effacement, portabilité, opposition : écris au contact ci-dessus. \
                Tu peux aussi saisir l'autorité de protection des données de ton pays (la CNIL en France)."</p>
        </Page>
    }
}

#[component]
pub fn Terms() -> impl IntoView {
    let i = instance();
    view! {
        <Page title="Conditions d'utilisation">
            <h2>"Le service"</h2>
            <p>"KRAKEN est gratuit, sans formule payante. Il est fourni tel quel, sans garantie de disponibilité."</p>
            <h2>"Logiciel libre"</h2>
            <p>"KRAKEN est publié sous licence AGPL-3.0. "
                <a href=move || i.with(|i| i.source_url.clone())>"Code source de cette instance"</a>"."</p>
            <h2>"Règles"</h2>
            <p>"Pas de contenu illégal, de harcèlement, de menaces, de spam ni de partage de données d'autrui sans son accord. \
                Tu dois avoir l'âge que la loi de ton pays demande pour t'inscrire seul (15 ans en France)."</p>
            <h2>"Modération"</h2>
            <p>"Chaque serveur est modéré par ses propriétaires. L'exploitant de l'instance peut retirer un contenu ou fermer \
                un compte qui enfreint ces règles ou la loi."</p>
            <h2>"Contact"</h2>
            <p>{move || i.with(|i| i.contact.clone()).unwrap_or_else(|| "Non renseigné par l'exploitant.".into())}</p>
        </Page>
    }
}
