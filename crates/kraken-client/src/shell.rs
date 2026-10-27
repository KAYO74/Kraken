//! La coquille : rail des serveurs (S02) avec mon compte en bas (S04), barre latérale
//! (S03 ou salons), zone principale. Plus la création / l'adhésion à un serveur (S21).

use kraken_proto::{Channel, ChannelKind, GuildState, Id};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::components::Outlet;
use leptos_router::hooks::{use_location, use_navigate};

use crate::call::{CallAudio, ControlPanel, IncomingCalls, VoicePanel, VoiceUsers};
use crate::icons::{Icon, lu};
use crate::store::{Conn, store};
use crate::ui::{Avatar, Logo, initials, modal};

/// Serveur affiché d'après l'URL `/channels/{guild}/…`, `None` sur l'accueil.
pub fn current_ids() -> Memo<(Option<Id>, Option<Id>)> {
    let loc = use_location();
    Memo::new(move |_| {
        let path = loc.pathname.get();
        let mut seg = path.trim_start_matches('/').split('/');
        if seg.next() != Some("channels") {
            return (None, None);
        }
        let g = seg.next().and_then(|g| g.parse().ok());
        let c = seg.next().and_then(|c| c.parse().ok());
        (g, c)
    })
}

/// Premier salon textuel d'un serveur, dans l'ordre d'affichage.
pub fn first_text(g: &GuildState) -> Option<Id> {
    ordered(&g.channels).into_iter().find(|c| c.kind.is_textual()).map(|c| c.id)
}

/// Salons dans l'ordre d'affichage : hors catégorie d'abord, puis chaque catégorie suivie
/// de ses salons.
pub fn ordered(channels: &[Channel]) -> Vec<Channel> {
    let mut top: Vec<&Channel> = channels.iter().filter(|c| c.parent_id.is_none()).collect();
    top.sort_by_key(|c| (c.kind == ChannelKind::Category, c.position));
    let mut out = Vec::new();
    for c in top {
        out.push(c.clone());
        if c.kind == ChannelKind::Category {
            let mut kids: Vec<&Channel> = channels.iter().filter(|k| k.parent_id == Some(c.id)).collect();
            kids.sort_by_key(|k| k.position);
            out.extend(kids.into_iter().cloned());
        }
    }
    out
}

#[component]
pub fn Shell() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    Effect::new(move || {
        if !s.signed_in.get() {
            let next = use_location().pathname.get_untracked();
            navigate(&format!("/login?next={next}"), NavigateOptions { replace: true, ..Default::default() });
        }
    });
    // En mobile, choisir un salon ou une conversation ferme le tiroir ; choisir un serveur
    // dans le rail le laisse ouvert pour montrer ses salons.
    let close_on_link = move |e: leptos::ev::MouseEvent| {
        use wasm_bindgen::JsCast;
        let link = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest("a").ok().flatten());
        if link.is_some() {
            s.drawer.set(false);
        }
    };
    let ids = current_ids();

    view! {
        <div class="app" data-drawer=move || s.drawer.get().then_some("open")>
            // Hors routeur : le navigateur suit l'ancre, et le focus va bien dans le contenu.
            <a class="app-skip" href="#main" rel="external" on:click=|_| {
                use wasm_bindgen::JsCast;
                let main = document().get_element_by_id("main").and_then(|m| m.dyn_into::<web_sys::HtmlElement>().ok());
                if let Some(m) = main { let _ = m.focus(); }
            }>"Aller au contenu"</a>
            <Rail/>
            <div class="app-nav" on:click=close_on_link>
                {move || match ids.get().0 {
                    Some(g) => view! { <GuildSidebar guild=g/> }.into_any(),
                    None => view! { <HomeSidebar/> }.into_any(),
                }}
                <VoicePanel/>
            </div>
            <div class="app-scrim" on:click=move |_| s.drawer.set(false)></div>
            <main class="app-main" id="main" tabindex="-1">
                <Show when=move || s.conn.get() != Conn::Ready>
                    <div class="k-banner k-banner--warning app-conn" role="status">
                        <Icon icon=lu::LuCircleAlert/>
                        <span>{move || if s.conn.get() == Conn::Connecting { "Connexion au serveur…" } else { "Connexion perdue, nouvelle tentative…" }}</span>
                    </div>
                </Show>
                <Outlet/>
            </main>
            <CallAudio/>
            <ControlPanel/>
            <IncomingCalls/>
        </div>
    }
}

#[component]
fn Rail() -> impl IntoView {
    let s = store();
    let ids = current_ids();
    let home_current = move || ids.get().0.is_none().then_some("page");
    view! {
        <div class="k-rail app-rail">
            <nav class="app-rail-list" aria-label="Serveurs">
                <a class="k-server k-server--home" href="/channels/@me" aria-label="Accueil et messages privés" aria-current=home_current>
                    <Logo/>
                </a>
                <hr/>
                <For each=move || s.guilds.get() key=|g| g.guild.id let:g>
                    {
                        let id = g.guild.id;
                        let href = format!("/channels/{id}");
                        let name = g.guild.name.clone();
                        view! {
                            <a class="k-server" href=href title=name.clone() aria-label=format!("Serveur {name}")
                                aria-current=move || (ids.get().0 == Some(id)).then_some("page")>
                                {initials(&name)}
                            </a>
                        }
                    }
                </For>
                <a class="k-server k-server--action" href="/guilds/new" aria-label="Créer ou rejoindre un serveur" title="Créer ou rejoindre">
                    <Icon icon=lu::LuPlus/>
                </a>
                <a class="k-server k-server--action" href="/discover" aria-label="Explorer les serveurs publics" title="Explorer">
                    <Icon icon=lu::LuCompass/>
                </a>
            </nav>
            <Me/>
        </div>
    }
}

#[component]
fn HomeSidebar() -> impl IntoView {
    let s = store();
    let ids = current_ids();
    let loc = use_location();
    let on_friends = move || (loc.pathname.get() == "/channels/@me").then_some("page");
    let picking = RwSignal::new(false);
    let incoming = Memo::new(move |_| {
        s.friends.with(|f| f.iter().filter(|r| r.kind == kraken_proto::RelationshipKind::PendingIncoming).count())
    });
    view! {
        <nav class="k-sidebar app-sidebar" aria-label="Messages privés">
            <a class="k-btn k-btn--sm app-find" href="/switcher"><Icon icon=lu::LuSearch small=true/>"Aller vers une personne ou un salon"</a>
            <a class="k-channel" href="/channels/@me" aria-current=on_friends><Icon icon=lu::LuUsers/>"Amis"
                <Show when=move || { incoming.get() > 0 }>
                    <span class="k-badge k-badge--count">{move || incoming.get()}</span>
                    <span class="k-sr">{move || if incoming.get() > 1 { " (demandes en attente)" } else { " (demande en attente)" }}</span>
                </Show>
            </a>
            <div class="app-dm-head">
                <h2 class="k-category">"Messages privés"</h2>
                <button class="k-btn k-btn--icon" aria-label="Nouvelle conversation ou groupe" title="Nouvelle conversation"
                    on:click=move |_| picking.set(true)>
                    <Icon icon=lu::LuPlus small=true/>
                </button>
            </div>
            <Show when=move || s.dms.with(|d| d.is_empty()) && s.conn.get() == Conn::Ready>
                <p class="app-hint">"Aucune conversation pour l'instant."</p>
            </Show>
            <For each=move || s.dms.get() key=|c| (c.id, c.name.clone(), c.recipients.iter().map(|u| u.name().to_string()).collect::<Vec<_>>()) let:c>
                {
                    let id = c.id;
                    let title = crate::social::private_title(&c);
                    let first = c.recipients.first().cloned();
                    let group = c.kind == ChannelKind::GroupDm;
                    let count = c.recipients.len() + 1;
                    let calling = Memo::new(move |_| !s.present(None, id).is_empty());
                    view! {
                        <a class="k-dm" href=format!("/channels/@me/{id}")
                            aria-current=move || (ids.get().1 == Some(id) || loc.pathname.get().ends_with(&id.to_string())).then_some("page")>
                            {match (group, first) {
                                (false, Some(u)) => view! { <Avatar user=u size="md"/> }.into_any(),
                                _ => view! { <span class="k-avatar k-avatar--md" aria-hidden="true">{initials(&title)}</span> }.into_any(),
                            }}
                            <div>{title.clone()}{group.then(|| view! { <small>{format!("{count} membres")}</small> })}</div>
                            <Show when=move || calling.get()>
                                <span class="app-dm-call" role="img" aria-label="Appel en cours" title="Appel en cours"><Icon icon=lu::LuPhoneCall small=true/></span>
                            </Show>
                        </a>
                        <VoiceUsers guild=None channel=id name=title.clone() hidden=Signal::derive(|| false)/>
                    }
                }
            </For>
        </nav>
        <Show when=move || picking.get()>
            <crate::social::PeoplePicker purpose=crate::social::PickFor::New(vec![]) close=Callback::new(move |()| picking.set(false))/>
        </Show>
    }
}

#[component]
fn GuildSidebar(guild: Id) -> impl IntoView {
    let s = store();
    let ids = current_ids();
    let collapsed = RwSignal::new(Vec::<Id>::new());
    let invite_open = RwSignal::new(false);
    // Sans les états vocaux : un micro coupé ne redessine pas toute la liste (ni ne vole le focus).
    let g = Memo::new(move |_| {
        s.guild(guild).map(|mut g| {
            g.voice_states.clear();
            g
        })
    });
    view! {
        <nav class="k-sidebar app-sidebar" aria-label="Salons">
            {move || match g.get() {
                None => view! {
                    <div aria-busy="true">
                        <div class="k-skeleton app-skel-line"></div>
                        <div class="k-skeleton app-skel-line"></div>
                        <div class="k-skeleton app-skel-line"></div>
                    </div>
                }.into_any(),
                Some(g) => {
                    let name = g.guild.name.clone();
                    let tip = name.clone();
                    let gid = g.guild.id;
                    let rows = ordered(&g.channels);
                    view! {
                        <div class="app-guild-head">
                            <h2 title=tip>{name}</h2>
                            <button class="k-btn k-btn--icon" aria-label="Inviter des amis" title="Inviter" on:click=move |_| invite_open.set(true)>
                                <Icon icon=lu::LuUserPlus/>
                            </button>
                            <a class="k-btn k-btn--icon" href=format!("/guilds/{gid}/settings") aria-label="Paramètres du serveur" title="Paramètres">
                                <Icon icon=lu::LuSettings/>
                            </a>
                        </div>
                        <a class="k-channel" href=format!("/guilds/{gid}/events")><Icon icon=lu::LuCalendar/>"Événements"</a>
                        {rows.into_iter().map(|c| {
                            let cid = c.id;
                            let name = c.name.clone().unwrap_or_default();
                            if c.kind == ChannelKind::Category {
                                let open = move || !collapsed.with(|v| v.contains(&cid));
                                view! {
                                    <button class="k-category" aria-expanded=move || open().to_string()
                                        on:click=move |_| collapsed.update(|v| if v.contains(&cid) { v.retain(|x| *x != cid) } else { v.push(cid) })>
                                        <Icon icon=lu::LuChevronDown small=true/>{name}
                                    </button>
                                }.into_any()
                            } else {
                                let parent = c.parent_id;
                                let hidden = move || parent.is_some_and(|p| collapsed.with(|v| v.contains(&p)))
                                    && ids.get().1 != Some(cid);
                                let voice = matches!(c.kind, ChannelKind::Voice | ChannelKind::Stage);
                                let (icon, kind) = match c.kind {
                                    _ if voice => (lu::LuVolume2, "salon vocal "),
                                    ChannelKind::Announcement => (lu::LuMegaphone, "annonces "),
                                    ChannelKind::Forum => (lu::LuMessagesSquare, "forum "),
                                    _ => (lu::LuHash, ""),
                                };
                                view! {
                                    <a class="k-channel" href=format!("/channels/{gid}/{cid}") hidden=hidden
                                        aria-current=move || (ids.get().1 == Some(cid)).then_some("page")>
                                        <Icon icon=icon/><span class="k-sr">{kind}</span><span class="app-ellipsis">{name.clone()}</span>
                                    </a>
                                    {voice.then(|| view! { <VoiceUsers guild=Some(gid) channel=cid name=name hidden=Signal::derive(hidden)/> })}
                                }.into_any()
                            }
                        }).collect_view()}
                    }.into_any()
                }
            }}
        </nav>
        <Show when=move || invite_open.get()>
            <InviteDialog guild=guild close=move || invite_open.set(false)/>
        </Show>
    }
}

#[component]
fn InviteDialog(guild: Id, close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let s = store();
    let ids = current_ids();
    let link = RwSignal::new(None::<Result<String, String>>);
    let copied = RwSignal::new(false);
    let channel = ids
        .get_untracked()
        .1
        .filter(|c| s.channel(*c).is_some_and(|c| c.kind.is_textual()))
        .or_else(|| s.guild(guild).as_ref().and_then(first_text));
    spawn_local(async move {
        let Some(c) = channel else {
            link.set(Some(Err("Aucun salon où inviter.".into())));
            return;
        };
        let base = crate::api::public_url().await;
        link.set(Some(
            crate::api::create_invite(c).await.map(|i| format!("{base}/invite/{}", i.code)).map_err(|e| e.message),
        ));
    });
    let input = NodeRef::<leptos::html::Input>::new();
    let copy = move |_| {
        use wasm_bindgen::JsCast;
        let Some(Ok(l)) = link.get_untracked() else { return };
        let w = web_sys::window().unwrap();
        // En HTTP (instance sur le réseau local), pas d'API presse-papiers : on copie la sélection.
        let done = if w.is_secure_context() {
            let _ = w.navigator().clipboard().write_text(&l);
            true
        } else {
            input.get_untracked().is_some_and(|i| {
                i.select();
                w.document()
                    .and_then(|d| d.dyn_into::<web_sys::HtmlDocument>().ok())
                    .and_then(|d| d.exec_command("copy").ok())
                    .unwrap_or(false)
            })
        };
        copied.set(done);
    };
    let name = s.guild(guild).map(|g| g.guild.name).unwrap_or_default();
    let root = NodeRef::new();
    modal(root, close);
    view! {
        <div class="k-scrim" node_ref=root on:click=move |_| close()>
            <div class="k-dialog" role="dialog" aria-modal="true" aria-labelledby="inv-t" on:click=|e| e.stop_propagation()>
                <header><h2 id="inv-t">{format!("Inviter dans {name}")}</h2></header>
                <div>
                    {move || match link.get() {
                        None => view! { <div class="k-skeleton app-skel-line" aria-busy="true"></div> }.into_any(),
                        Some(Err(e)) => view! { <p class="k-error" role="alert">{e}</p> }.into_any(),
                        Some(Ok(l)) => view! {
                            <label class="k-field">
                                <span class="k-label">"Lien à partager"</span>
                                <input class="k-input" readonly node_ref=input value=l.clone() on:focus=|e| { event_target::<web_sys::HtmlInputElement>(&e).select(); }/>
                            </label>
                            <p class="app-hint">"Ce lien n'expire pas. Toute personne qui l'a peut rejoindre."</p>
                        }.into_any(),
                    }}
                </div>
                <footer>
                    <button class="k-btn k-btn--ghost" on:click=move |_| close() autofocus>"Fermer"</button>
                    <button class="k-btn k-btn--primary" on:click=copy disabled=move || !matches!(link.get(), Some(Ok(_)))>
                        {move || if copied.get() { "Copié" } else { "Copier le lien" }}
                    </button>
                </footer>
            </div>
        </div>
    }
}

/// S04 : mon compte, en bas du rail. L'avatar ouvre une carte : nom, pseudo, état de la
/// connexion, réglages, déconnexion. Échap ou un clic ailleurs la referme.
#[component]
fn Me() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let open = RwSignal::new(false);
    let button = NodeRef::<leptos::html::Button>::new();
    let wrap = NodeRef::<leptos::html::Div>::new();
    let card = NodeRef::<leptos::html::Div>::new();
    let close = move |refocus: bool| {
        open.set(false);
        if refocus && let Some(b) = button.get_untracked() {
            let _ = b.focus();
        }
    };
    // À l'ouverture, le focus va sur la première entrée.
    Effect::new(move || {
        if open.get()
            && let Some(first) = card.get().and_then(|c| c.query_selector("a, button").ok().flatten())
        {
            use wasm_bindgen::JsCast;
            if let Ok(h) = first.dyn_into::<web_sys::HtmlElement>() {
                let _ = h.focus();
            }
        }
    });
    let esc = window_event_listener(leptos::ev::keydown, move |e| {
        if e.key() == "Escape" && open.get_untracked() {
            e.stop_propagation();
            close(true);
        }
    });
    let outside = window_event_listener(leptos::ev::pointerdown, move |e| {
        use wasm_bindgen::JsCast;
        if !open.get_untracked() {
            return;
        }
        let inside = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
            .zip(wrap.get_untracked())
            .is_some_and(|(t, w)| w.contains(Some(&t)));
        if !inside {
            close(false);
        }
    });
    on_cleanup(move || {
        esc.remove();
        outside.remove();
    });
    // Le focus qui quitte la carte (Tab après la dernière entrée) la referme.
    let on_focusout = move |e: leptos::ev::FocusEvent| {
        use wasm_bindgen::JsCast;
        let next = e.related_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
        let stays = next.clone().zip(wrap.get_untracked()).is_some_and(|(n, w)| w.contains(Some(&n)));
        if next.is_some() && !stays {
            open.set(false);
        }
    };
    // Choisir une entrée ferme la carte, et le tiroir en mobile.
    let picked = move |_| {
        open.set(false);
        s.drawer.set(false);
    };
    let leave = move |_| {
        let navigate = navigate.clone();
        open.set(false);
        spawn_local(async move {
            let _ = crate::api::logout().await;
            s.sign_out();
            navigate("/login", Default::default());
        });
    };
    let presence =
        move || if s.conn.get() == Conn::Ready { kraken_proto::Status::Online } else { kraken_proto::Status::Offline };
    let state = move || match s.conn.get() {
        Conn::Ready => "Connecté",
        Conn::Connecting => "Connexion…",
        Conn::Offline => "Hors ligne",
    };
    view! {
        <div class="app-me" node_ref=wrap on:focusout=on_focusout>
            <button class="k-me" node_ref=button aria-label="Mon compte" title="Mon compte"
                aria-expanded=move || open.get().to_string() aria-controls=move || open.get().then_some("me-card")
                on:click=move |_| open.update(|o| *o = !*o)>
                {move || s.me.get().map(|u| view! { <Avatar user=u status=presence()/> })}
            </button>
            <Show when=move || open.get()>
                <div class="k-mecard" id="me-card" role="group" aria-label="Mon compte" node_ref=card>
                    <header>
                        {move || s.me.get().map(|u| view! { <Avatar user=u size="md" status=presence()/> })}
                        <div>
                            <b class="app-ellipsis">{move || s.me.get().map(|u| u.name().to_string()).unwrap_or_default()}</b>
                            <small class="app-ellipsis">{move || s.me.get().map(|u| format!("@{}", u.username)).unwrap_or_default()}</small>
                        </div>
                    </header>
                    <p class="k-mecard-state" data-state=move || (s.conn.get() == Conn::Ready).then_some("on")>{state}</p>
                    <a class="k-mecard-item" href="/settings/account" on:click=picked>
                        <Icon icon=lu::LuUserPen/>"Réglages"<span class="k-sr">" utilisateur"</span>
                    </a>
                    <a class="k-mecard-item" href="/settings/appearance" on:click=picked>
                        <Icon icon=lu::LuPalette/>"Apparence"
                    </a>
                    <hr/>
                    <button class="k-mecard-item is-danger" on:click={ let leave = leave.clone(); leave }>
                        <Icon icon=lu::LuLogOut/>"Se déconnecter"
                    </button>
                </div>
            </Show>
        </div>
    }
}

/// S21 : créer un serveur ou en rejoindre un avec une invitation.
#[component]
pub fn CreateGuild() -> impl IntoView {
    let s = store();
    let navigate = use_navigate();
    let default_name = s.me.get_untracked().map(|u| format!("Le repaire de {}", u.name())).unwrap_or_default();
    let name = RwSignal::new(default_name);
    let code = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let join_error = RwSignal::new(None::<String>);

    let nav = navigate.clone();
    let create = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let nav = nav.clone();
        busy.set(true);
        spawn_local(async move {
            match crate::api::create_guild(&name.get_untracked()).await {
                Ok(g) => {
                    let to = format!(
                        "/channels/{}/{}",
                        g.guild.id,
                        first_text(&g).map(|c| c.to_string()).unwrap_or_default()
                    );
                    s.upsert_guild(g);
                    nav(&to, Default::default());
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    let join = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        let raw = code.get_untracked();
        let c = raw.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
        if c.is_empty() {
            join_error.set(Some("Colle un lien ou un code d'invitation.".into()));
            return;
        }
        navigate(&format!("/invite/{c}"), Default::default());
    };
    view! {
        <div class="k-scrim">
            <div class="k-dialog app-dialog-wide" role="dialog" aria-modal="true" aria-labelledby="cg-t">
                <header><h2 id="cg-t">"Ton propre serveur"</h2></header>
                <form on:submit=create>
                    <div>
                        <p>"Un espace pour ta bande, avec des salons texte et vocal. Tu pourras tout changer ensuite."</p>
                        <label class="k-field">
                            <span class="k-label">"Nom du serveur"</span>
                            <input class="k-input" required minlength="2" maxlength="100" bind:value=name
                                aria-invalid=move || error.get().is_some().then_some("true") aria-describedby="cg-e"/>
                            <Show when=move || error.get().is_some()>
                                <span class="k-error" id="cg-e" role="alert">{move || error.get()}</span>
                            </Show>
                        </label>
                    </div>
                    <footer>
                        <a class="k-btn k-btn--ghost" href="/channels/@me">"Annuler"</a>
                        <button class="k-btn k-btn--primary" type="submit" aria-busy=move || busy.get().to_string() disabled=move || busy.get()>"Créer"</button>
                    </footer>
                </form>
                <form on:submit=join class="app-dialog-alt">
                    <label class="k-field">
                        <span class="k-label">"Tu as déjà une invitation ?"</span>
                        <input class="k-input" placeholder="https://…/invite/AbC123xy" bind:value=code aria-describedby="cj-e"/>
                        <Show when=move || join_error.get().is_some()>
                            <span class="k-error" id="cj-e" role="alert">{move || join_error.get()}</span>
                        </Show>
                    </label>
                    <button class="k-btn" type="submit">"Rejoindre"</button>
                </form>
            </div>
        </div>
    }
}
