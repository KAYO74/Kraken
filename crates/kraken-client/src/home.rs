//! S01 : accueil Amis, onglets En ligne / Tous / En attente / Ajouter.

use kraken_proto::{ChannelKind, RelationshipKind, Status};
use leptos::prelude::*;

use crate::icons::{Icon, lu};
use crate::store::{Conn, store};
use crate::ui::{Avatar, Header, status_label};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Online,
    All,
    Pending,
    Add,
}

#[component]
pub fn Friends() -> impl IntoView {
    let s = store();
    let tab = RwSignal::new(Tab::Online);
    let query = RwSignal::new(String::new());
    let rows = Memo::new(move |_| {
        let q = query.get().to_lowercase();
        s.friends
            .get()
            .into_iter()
            .filter(|r| match tab.get() {
                Tab::Online => r.kind == RelationshipKind::Friend && r.status != Status::Offline,
                Tab::All => r.kind == RelationshipKind::Friend,
                Tab::Pending => matches!(r.kind, RelationshipKind::PendingIncoming | RelationshipKind::PendingOutgoing),
                Tab::Add => false,
            })
            .filter(|r| q.is_empty() || r.user.name().to_lowercase().contains(&q) || r.user.username.contains(&q))
            .collect::<Vec<_>>()
    });
    let tab_btn = move |t: Tab, label: &'static str, accent: bool| {
        view! {
            <button class=if accent { "k-tab k-tab--accent" } else { "k-tab" } role="tab"
                aria-selected=move || (tab.get() == t).to_string() on:click=move |_| tab.set(t)>{label}</button>
        }
    };
    let dm_with = move |uid| {
        s.dms.with(|d| {
            d.iter().find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == uid)).map(|c| c.id)
        })
    };

    view! {
        <Header>
            <Icon icon=lu::LuUsers/>
            <h1 class="app-title">"Amis"</h1>
            <div class="k-tabs" role="tablist" aria-label="Filtrer les amis">
                {tab_btn(Tab::Online, "En ligne", false)}
                {tab_btn(Tab::All, "Tous", false)}
                {tab_btn(Tab::Pending, "En attente", false)}
                {tab_btn(Tab::Add, "Ajouter un ami", true)}
            </div>
        </Header>
        <div class="app-body">
            <section class="app-friends" role="tabpanel">
                {move || if tab.get() == Tab::Add {
                    view! {
                        <form class="app-add" on:submit=|e| e.prevent_default()>
                            <h2>"Ajouter un ami"</h2>
                            <p class="app-hint">"Tape son pseudo KRAKEN exact."</p>
                            <div class="app-add-row">
                                <input class="k-input" placeholder="pseudo" aria-label="Pseudo de ton ami" disabled/>
                                <button class="k-btn k-btn--primary" disabled>"Envoyer la demande"</button>
                            </div>
                            <p class="app-hint">"Les demandes d'ami arrivent avec la tranche sociale."</p>
                        </form>
                    }.into_any()
                } else {
                    view! {
                        <div class="k-search app-search">
                            <Icon icon=lu::LuSearch/>
                            <input class="k-input" type="search" placeholder="Chercher" aria-label="Chercher parmi tes amis" bind:value=query/>
                        </div>
                        <h2 class="k-category">{move || {
                            let n = rows.get().len();
                            match tab.get() { Tab::Online => format!("En ligne — {n}"), Tab::Pending => format!("En attente — {n}"), _ => format!("Tous — {n}") }
                        }}</h2>
                        {move || if s.conn.get() != Conn::Ready && s.friends.with(|f| f.is_empty()) {
                            view! { <div aria-busy="true">{(0..4).map(|_| view! { <div class="k-skeleton app-skel-row"></div> }).collect_view()}</div> }.into_any()
                        } else if rows.get().is_empty() {
                            let (title, line) = match tab.get() {
                                Tab::Online => ("Personne en ligne pour l'instant", "Tes amis apparaîtront ici dès qu'ils se connectent."),
                                Tab::Pending => ("Aucune demande en attente", "Les demandes reçues et envoyées s'affichent ici."),
                                _ => ("Pas encore d'amis", "Ajoute quelqu'un avec son pseudo pour commencer."),
                            };
                            view! { <div class="k-empty"><b>{title}</b><span>{line}</span></div> }.into_any()
                        } else {
                            view! {
                                <ul class="app-list">
                                    <For each=move || rows.get() key=|r| (r.user.id, r.kind as u8, r.status as u8, r.user.display_name.clone()) let:r>
                                        {
                                            let sub = match r.kind {
                                                RelationshipKind::PendingIncoming => "Demande reçue".to_string(),
                                                RelationshipKind::PendingOutgoing => "Demande envoyée".to_string(),
                                                _ => status_label(r.status).to_string(),
                                            };
                                            let dm = dm_with(r.user.id);
                                            let name = r.user.name().to_string();
                                            view! {
                                                <li class="app-row">
                                                    <Avatar user=r.user.clone() size="" status=r.status/>
                                                    <div class="app-ellipsis"><b>{name.clone()}</b><small>{format!("@{} · {sub}", r.user.username)}</small></div>
                                                    {dm.map(|c| view! {
                                                        <a class="k-btn k-btn--icon" href=format!("/channels/@me/{c}") aria-label=format!("Écrire à {name}") title="Message">
                                                            <Icon icon=lu::LuMessageCircle/>
                                                        </a>
                                                    })}
                                                </li>
                                            }
                                        }
                                    </For>
                                </ul>
                            }.into_any()
                        }}
                    }.into_any()
                }}
            </section>
            <aside class="app-aside app-activity" aria-label="Activité">
                <h2>"En ce moment"</h2>
                <div class="k-empty"><b>"Calme plat"</b>"Quand un ami lance une activité ou un vocal, ça s'affiche ici."</div>
            </aside>
        </div>
    }
}
