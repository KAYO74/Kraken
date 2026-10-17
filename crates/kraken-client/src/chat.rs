//! Salon textuel de serveur (S05), conversation privée (S19), et l'entrée du vocal (S18).

use kraken_proto::{Channel, ChannelKind, Id, MAX_MESSAGE_LEN, Message};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::icons::{Icon, lu};
use crate::shell::first_text;
use crate::store::{Conn, Pending, store};
use crate::ui::{Avatar, Header, Stub, initials, iso, when};

fn param(name: &'static str) -> Memo<Option<Id>> {
    let p = use_params_map();
    Memo::new(move |_| p.read().get(name).and_then(|v| v.parse().ok()))
}

/// `/channels/{guild}` : ouvre le premier salon textuel.
#[component]
pub fn GuildIndex() -> impl IntoView {
    let s = store();
    let guild = param("guild");
    let navigate = use_navigate();
    Effect::new(move || {
        let Some(g) = guild.get().and_then(|g| s.guild(g)) else { return };
        if let Some(c) = first_text(&g) {
            navigate(&format!("/channels/{}/{c}", g.guild.id), NavigateOptions { replace: true, ..Default::default() });
        }
    });
    view! { <Loading/> }
}

#[component]
fn Loading() -> impl IntoView {
    let s = store();
    view! {
        {move || if s.conn.get() == Conn::Ready {
            view! { <Stub id="—" title="Introuvable"/> }.into_any()
        } else {
            view! { <div class="app-fill" aria-busy="true"><div class="k-skeleton app-skel-block"></div></div> }.into_any()
        }}
    }
}

#[component]
pub fn GuildChannel() -> impl IntoView {
    let s = store();
    let channel = param("channel");
    let current = Memo::new(move |_| channel.get().and_then(|c| s.channel(c)));
    view! {
        {move || match current.get() {
            None => view! { <Loading/> }.into_any(),
            Some(c) if c.kind == ChannelKind::Voice || c.kind == ChannelKind::Stage => view! { <Stub id="S18" title="Salon vocal"/> }.into_any(),
            Some(c) if c.kind == ChannelKind::Forum => view! { <Stub id="S24" title="Forum"/> }.into_any(),
            Some(c) => view! { <ChannelView channel=c/> }.into_any(),
        }}
    }
}

#[component]
pub fn DmView() -> impl IntoView {
    let s = store();
    let channel = param("channel");
    let current = Memo::new(move |_| channel.get().and_then(|c| s.channel(c)));
    view! {
        {move || match current.get() {
            None => view! { <Loading/> }.into_any(),
            Some(c) => view! { <ChannelView channel=c/> }.into_any(),
        }}
    }
}

fn title_of(c: &Channel) -> String {
    match c.kind {
        ChannelKind::Dm | ChannelKind::GroupDm => c
            .name
            .clone()
            .unwrap_or_else(|| c.recipients.iter().map(|u| u.name().to_string()).collect::<Vec<_>>().join(", ")),
        _ => c.name.clone().unwrap_or_default(),
    }
}

#[component]
fn ChannelView(channel: Channel) -> impl IntoView {
    let s = store();
    let cid = channel.id;
    let title = title_of(&channel);
    let is_dm = channel.guild_id.is_none();
    let members_open = RwSignal::new(true);
    let header_icon = match channel.kind {
        ChannelKind::Announcement => lu::LuMegaphone,
        ChannelKind::Dm | ChannelKind::GroupDm => lu::LuMessageCircle,
        _ => lu::LuHash,
    };
    let (title_h1, title_log, label) = (title.clone(), title.clone(), format!("Messages de {title}"));
    let place = if is_dm { format!("Écrire à {title}") } else { format!("Écrire dans #{title}") };

    // Charger l'historique à l'ouverture, et après une reconnexion.
    Effect::new(move || {
        let loaded = s.logs.with(|l| l.get(&cid).is_some_and(|x| x.loaded));
        if loaded || s.conn.get() != Conn::Ready {
            return;
        }
        spawn_local(async move {
            match crate::api::messages(cid).await {
                Ok(items) => s.log(cid, |l| {
                    let live: Vec<Message> =
                        l.items.drain(..).filter(|m| !items.iter().any(|x| x.id == m.id)).collect();
                    l.items = items;
                    l.items.extend(live);
                    l.loaded = true;
                    l.error = None;
                }),
                Err(e) => s.log(cid, |l| {
                    l.loaded = true;
                    l.error = Some(e.message);
                }),
            }
        });
    });

    view! {
        <Header>
            <Icon icon=header_icon/>
            <h1 class="app-title app-ellipsis">{title_h1}</h1>
            {channel.topic.clone().map(|t| { let tip = t.clone(); view! { <span class="app-topic app-ellipsis" title=tip>{t}</span> } })}
            <div class="app-header-tools">
                {(!is_dm).then(|| view! {
                    <a class="k-btn k-btn--icon" href=format!("/channels/{}/{cid}/threads", channel.guild_id.unwrap()) aria-label="Fils" title="Fils"><Icon icon=lu::LuMessageSquareText/></a>
                    <button class="k-btn k-btn--icon" aria-label="Liste des membres" title="Membres"
                        aria-expanded=move || members_open.get().to_string() on:click=move |_| members_open.update(|v| *v = !*v)>
                        <Icon icon=lu::LuUsers/>
                    </button>
                })}
                <a class="k-btn k-btn--icon" href="/search" aria-label="Rechercher" title="Rechercher"><Icon icon=lu::LuSearch/></a>
                <a class="k-btn k-btn--icon" href="/inbox" aria-label="Boîte de réception" title="Boîte de réception"><Icon icon=lu::LuInbox/></a>
            </div>
        </Header>
        <div class="app-body">
            <section class="app-chat" aria-label=label>
                <MessageList channel=cid title=title_log is_dm=is_dm/>
                <Composer channel=cid placeholder=place/>
            </section>
            {(!is_dm).then(|| view! { <Show when=move || members_open.get()><Members guild=channel.guild_id.unwrap()/></Show> })}
        </div>
    }
}

#[component]
fn MessageList(channel: Id, title: String, is_dm: bool) -> impl IntoView {
    let s = store();
    let log = Memo::new(move |_| s.logs.with(|l| l.get(&channel).cloned().unwrap_or_default()));
    let scroller = NodeRef::<html::Div>::new();
    // Rester collé en bas quand un message arrive.
    Effect::new(move || {
        log.track();
        if let Some(el) = scroller.get() {
            request_animation_frame(move || el.set_scroll_top(el.scroll_height()));
        }
    });
    let retry = move |p: Pending| {
        s.log(channel, |l| l.pending.retain(|x| x.nonce != p.nonce));
        send(channel, p.content);
    };
    view! {
        <div class="app-log" node_ref=scroller role="log" aria-live="polite" aria-busy=move || (!log.get().loaded).to_string()>
            {move || {
                let l = log.get();
                if !l.loaded {
                    return view! {
                        <div aria-hidden="true">
                            {(0..6).map(|_| view! { <div class="app-skel-msg"><span class="k-skeleton"></span><div class="k-skeleton"></div></div> }).collect_view()}
                        </div>
                    }.into_any();
                }
                if let Some(e) = l.error {
                    return view! {
                        <div class="k-empty"><b>"Impossible de charger les messages"</b><span>{e}</span>
                        <button class="k-btn" on:click=move |_| s.log(channel, |l| l.loaded = false)>"Réessayer"</button></div>
                    }.into_any();
                }
                let intro = if is_dm {
                    format!("Le début de ta conversation avec {title}.")
                } else {
                    format!("C'est le tout début de #{title}. Lance la discussion !")
                };
                let mut prev: Option<(Option<Id>, i64)> = None;
                let rows = l.items.into_iter().map(|m| {
                    let author = m.author.as_ref().map(|a| a.id);
                    let cont = prev.is_some_and(|(a, t)| a == author && author.is_some() && m.created_at - t < 7 * 60_000);
                    prev = Some((author, m.created_at));
                    view! { <MessageRow msg=m cont=cont/> }
                }).collect_view();
                let me = s.me.get();
                let pending = l.pending.into_iter().map(|p| {
                    let failed = p.error.clone();
                    let again = p.clone();
                    let me = me.clone();
                    let (pending_attr, failed_attr) = (failed.is_none().then_some(""), failed.is_some().then_some(""));
                    view! {
                        <article class="k-msg" data-pending=pending_attr data-failed=failed_attr>
                            {me.clone().map(|u| view! { <Avatar user=u/> })}
                            <div>
                                <header><b>{me.map(|u| u.name().to_string())}</b><time>"envoi…"</time></header>
                                <p>{p.content}</p>
                                {failed.map(|e| view! {
                                    <p class="k-error" role="alert">{e}" "<button class="k-btn k-btn--link" on:click=move |_| retry(again.clone())>"Réessayer"</button></p>
                                })}
                            </div>
                        </article>
                    }
                }).collect_view();
                view! {
                    <div class="app-intro"><span class="k-avatar k-avatar--lg" aria-hidden="true">{initials(&title)}</span><h2>{title.clone()}</h2><p>{intro}</p></div>
                    {rows}
                    {pending}
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn MessageRow(msg: Message, cont: bool) -> impl IntoView {
    let name = msg.author.as_ref().map(|u| u.name().to_string()).unwrap_or_else(|| "Compte supprimé".into());
    let at = msg.created_at;
    view! {
        <article class=if cont { "k-msg k-msg--cont" } else { "k-msg" } aria-label=format!("{name}, {}", when(at))>
            {if cont {
                view! { <time class="app-gutter-time" datetime=iso(at)>{when(at).rsplit(' ').next().unwrap_or_default().to_string()}</time> }.into_any()
            } else {
                match msg.author.clone() {
                    Some(u) => view! { <Avatar user=u/> }.into_any(),
                    None => view! { <span class="k-avatar" aria-hidden="true">"?"</span> }.into_any(),
                }
            }}
            <div>
                {(!cont).then(|| view! { <header><b>{name.clone()}</b><time datetime=iso(at)>{when(at)}</time></header> })}
                <p>{msg.content}</p>
            </div>
        </article>
    }
}

fn nonce() -> String {
    format!("{:x}{:x}", js_sys::Date::now() as u64, (js_sys::Math::random() * 1e12) as u64)
}

/// Envoi optimiste : le message apparaît grisé, puis est remplacé par le MESSAGE_CREATE du
/// gateway (même nonce) ou par la réponse REST si le gateway est en retard.
fn send(channel: Id, content: String) {
    let s = store();
    let n = nonce();
    s.log(channel, |l| l.pending.push(Pending { nonce: n.clone(), content: content.clone(), error: None }));
    spawn_local(async move {
        match crate::api::send_message(channel, &content, &n).await {
            Ok(m) => s.log(channel, |l| {
                l.pending.retain(|p| p.nonce != n);
                if !l.items.iter().any(|x| x.id == m.id) {
                    l.items.push(m);
                }
            }),
            Err(e) => s.log(channel, |l| {
                if let Some(p) = l.pending.iter_mut().find(|p| p.nonce == n) {
                    p.error = Some(e.message);
                }
            }),
        }
    });
}

#[component]
fn Composer(channel: Id, placeholder: String) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let too_long = move || text.with(|t| t.chars().count() > MAX_MESSAGE_LEN);
    let submit = move || {
        let t = text.get_untracked();
        if t.trim().is_empty() || too_long() {
            return;
        }
        text.set(String::new());
        send(channel, t.trim().to_string());
    };
    view! {
        <div class="app-compose">
            <Show when=too_long>
                <p class="k-error app-compose-note" role="alert">{move || format!("{} caractères sur {MAX_MESSAGE_LEN} : raccourcis un peu.", text.with(|t| t.chars().count()))}</p>
            </Show>
            <form class="k-composer" on:submit=move |e| { e.prevent_default(); submit(); }>
                <button type="button" class="k-btn k-btn--icon" aria-label="Joindre un fichier (bientôt)" disabled><Icon icon=lu::LuPlus/></button>
                <textarea rows="1" placeholder=placeholder.clone() aria-label=placeholder bind:value=text
                    on:keydown=move |e| if e.key() == "Enter" && !e.shift_key() && !e.is_composing() { e.prevent_default(); submit(); }></textarea>
                <button type="button" class="k-btn k-btn--icon" aria-label="Emoji (bientôt)" disabled><Icon icon=lu::LuSmile/></button>
                <button type="submit" class="k-btn k-btn--icon" aria-label="Envoyer"><Icon icon=lu::LuSendHorizontal/></button>
            </form>
        </div>
    }
}

#[component]
fn Members(guild: Id) -> impl IntoView {
    let s = store();
    let members = move || s.guild(guild).map(|g| g.members).unwrap_or_default();
    view! {
        <aside class="app-aside" aria-label="Membres">
            <h2 class="k-category">{move || format!("Membres — {}", members().len())}</h2>
            <ul class="app-members">
                <For each=members key=|m| m.user.id let:m>
                    <li class="k-dm">
                        <Avatar user=m.user.clone() size="md"/>
                        <div class="app-ellipsis">{m.nick.clone().unwrap_or_else(|| m.user.name().to_string())}</div>
                    </li>
                </For>
            </ul>
        </aside>
    }
}
