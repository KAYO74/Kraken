//! Salon textuel de serveur (S05), conversation privée (S19) ; le salon vocal (S18) est dans `call`.

use gloo_timers::future::TimeoutFuture;
use kraken_core::perms;
use kraken_proto::{Channel, ChannelKind, Id, MAX_MESSAGE_LEN, Message, User};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_params_map};

use wasm_bindgen::JsCast;

use crate::call::VoiceRoom;
use crate::emoji::EmojiPicker;
use crate::icons::{Icon, lu};
use crate::markdown::Body;
use crate::shell::first_text;
use crate::store::{Conn, Pending, store};
use crate::ui::{Avatar, Header, Stub, iso, modal, when};

thread_local! {
    static DRAFTS: std::cell::RefCell<std::collections::HashMap<Id, String>> = Default::default();
}

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
            Some(c) if c.kind == ChannelKind::Voice || c.kind == ChannelKind::Stage => view! { <VoiceRoom channel=c/> }.into_any(),
            Some(c) if c.kind == ChannelKind::Forum => view! { <Stub id="S24" title="Forum"/> }.into_any(),
            Some(c) => view! { <ChannelView channel=c/> }.into_any(),
        }}
    }
}

/// Un MP ou un groupe. La vue ne dépend que de l'id : un groupe renommé ou complété garde
/// sa saisie, son défilement et le focus.
#[component]
pub fn DmView() -> impl IntoView {
    let s = store();
    let channel = param("channel");
    let current = Memo::new(move |_| channel.get().and_then(|c| s.channel(c)).map(|c| c.id));
    view! {
        {move || match current.get().and_then(|id| untrack(|| s.channel(id))) {
            None => view! { <Loading/> }.into_any(),
            Some(c) => view! { <ChannelView channel=c/> }.into_any(),
        }}
    }
}

fn title_of(c: &Channel) -> String {
    match c.kind {
        ChannelKind::Dm | ChannelKind::GroupDm => crate::social::private_title(c),
        _ => c.name.clone().unwrap_or_default(),
    }
}

/// Fenêtres ouvertes depuis l'en-tête d'un MP ou d'un groupe.
#[derive(Clone, Copy, PartialEq)]
enum PrivateDialog {
    /// Ajouter des amis au groupe, ou créer un groupe depuis un MP.
    Add,
    Rename,
    Leave,
}

/// Ce que partagent le fil et la zone de saisie d'un salon ouvert.
#[derive(Clone, Copy)]
struct Chat {
    channel: Id,
    guild: Option<Id>,
    draft: RwSignal<String>,
    composer: NodeRef<html::Textarea>,
    /// Message en cours de modification, et son texte.
    editing: RwSignal<Option<Id>>,
    edit_text: RwSignal<String>,
    replying: RwSignal<Option<Message>>,
    picker: RwSignal<Option<Pick>>,
    deleting: RwSignal<Option<Message>>,
    /// Échec d'une action (réaction, modification…), affiché au-dessus de la saisie.
    notice: RwSignal<Option<String>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Pick {
    /// Réagir à ce message.
    React(Id),
    /// Insérer dans la saisie.
    Insert,
}

fn chat() -> Chat {
    expect_context()
}

fn my_id() -> Option<Id> {
    store().me.with_untracked(|m| m.as_ref().map(|m| m.id))
}

#[component]
fn ChannelView(channel: Channel) -> impl IntoView {
    let s = store();
    let cid = channel.id;
    let first_title = title_of(&channel);
    // Suit le salon dans le store : un groupe renommé change de titre sans quitter la vue.
    let title = Memo::new(move |_| s.channel(cid).map(|c| title_of(&c)).unwrap_or_else(|| first_title.clone()));
    let is_dm = channel.guild_id.is_none();
    let kind = channel.kind;
    // MP 1:1 : l'autre personne, pour le blocage et « créer un groupe ».
    let other = (kind == ChannelKind::Dm).then(|| channel.recipients.first().cloned()).flatten();
    let other_id = other.as_ref().map(|u| u.id);
    let other_blocked = Memo::new(move |_| other_id.is_some_and(|o| s.is_blocked(o)));
    let other_friend =
        Memo::new(move |_| other_id.is_some_and(|o| s.rel_kind(o) == Some(kraken_proto::RelationshipKind::Friend)));
    let dialog = RwSignal::new(None::<PrivateDialog>);
    // Le brouillon survit au changement de salon et au rafraîchissement de la vue.
    let draft = RwSignal::new(DRAFTS.with_borrow_mut(|d| d.remove(&cid)).unwrap_or_default());
    Effect::new(move || {
        let t = draft.get();
        DRAFTS.with_borrow_mut(|d| d.insert(cid, t));
    });
    let chat = Chat {
        channel: cid,
        guild: channel.guild_id,
        draft,
        composer: NodeRef::new(),
        editing: RwSignal::new(None),
        edit_text: RwSignal::new(String::new()),
        replying: RwSignal::new(None),
        picker: RwSignal::new(None),
        deleting: RwSignal::new(None),
        notice: RwSignal::new(None),
    };
    provide_context(chat);
    // Ouverte d'office seulement là où elle tient à côté du fil.
    let members_open =
        RwSignal::new(window().match_media("(min-width: 1024px)").ok().flatten().is_some_and(|m| m.matches()));
    let header_icon = match channel.kind {
        ChannelKind::Announcement => lu::LuMegaphone,
        ChannelKind::Dm | ChannelKind::GroupDm => lu::LuMessageCircle,
        _ => lu::LuHash,
    };
    let label = move || format!("Messages de {}", title.get());
    let place = Signal::derive(move || {
        if is_dm { format!("Écrire à {}", title.get()) } else { format!("Écrire dans #{}", title.get()) }
    });
    let other_name = other.as_ref().map(|u| u.name().to_string()).unwrap_or_default();
    let other_name2 = other_name.clone();
    let unblock_error = RwSignal::new(None::<String>);
    let unblock = move |_| {
        let Some(o) = other_id else { return };
        spawn_local(async move {
            match crate::api::remove_relationship(o).await {
                Ok(()) => s.remove_rel(o),
                Err(e) => unblock_error.set(Some(e.message)),
            }
        });
    };
    let close_dialog = Callback::new(move |()| dialog.set(None));

    // Charger l'historique à l'ouverture, et après une reconnexion.
    Effect::new(move || {
        let loaded = s.logs.with(|l| l.get(&cid).is_some_and(|x| x.loaded));
        if loaded || s.conn.get() != Conn::Ready {
            return;
        }
        spawn_local(async move {
            match crate::api::messages(cid).await {
                Ok(items) => s.log(cid, |l| {
                    // Seuls les messages arrivés pendant le chargement, plus récents que la page
                    // reçue, s'y ajoutent ; les anciens affichés sont remplacés par la page.
                    let newest = items.last().map(|m| m.id);
                    let live: Vec<Message> = l.items.drain(..).filter(|m| newest.is_none_or(|n| m.id > n)).collect();
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

    let on_pick = Callback::new(move |emoji: String| {
        match chat.picker.get_untracked() {
            Some(Pick::React(mid)) => react(chat, mid, emoji),
            Some(Pick::Insert) => insert_text(chat, &emoji),
            None => {}
        }
        chat.picker.set(None);
    });
    let on_close = Callback::new(move |()| chat.picker.set(None));

    view! {
        <Header>
            <Icon icon=header_icon/>
            <h1 class="app-title app-ellipsis">{move || title.get()}</h1>
            {channel.topic.clone().map(|t| { let tip = t.clone(); view! { <span class="app-topic app-ellipsis" title=tip>{t}</span> } })}
            <div class="app-header-tools">
                {(!is_dm).then(|| view! {
                    <a class="k-btn k-btn--icon" href=format!("/channels/{}/{cid}/threads", channel.guild_id.unwrap()) aria-label="Fils" title="Fils"><Icon icon=lu::LuMessageSquareText/></a>
                    <button class="k-btn k-btn--icon" aria-label="Liste des membres" title="Membres"
                        aria-expanded=move || members_open.get().to_string() on:click=move |_| members_open.update(|v| *v = !*v)>
                        <Icon icon=lu::LuUsers/>
                    </button>
                })}
                {(kind == ChannelKind::GroupDm).then(|| view! {
                    <button class="k-btn k-btn--icon" aria-label="Ajouter des amis au groupe" title="Ajouter des amis"
                        on:click=move |_| dialog.set(Some(PrivateDialog::Add))><Icon icon=lu::LuUserPlus/></button>
                    <button class="k-btn k-btn--icon" aria-label="Renommer le groupe" title="Renommer"
                        on:click=move |_| dialog.set(Some(PrivateDialog::Rename))><Icon icon=lu::LuPencil/></button>
                    <button class="k-btn k-btn--icon" aria-label="Quitter le groupe" title="Quitter le groupe"
                        on:click=move |_| dialog.set(Some(PrivateDialog::Leave))><Icon icon=lu::LuLogOut/></button>
                })}
                {(kind == ChannelKind::Dm).then(|| { let n = other_name2.clone(); view! {
                    <Show when=move || other_friend.get()>
                        <button class="k-btn k-btn--icon" aria-label=format!("Créer un groupe avec {n}") title="Créer un groupe"
                            on:click=move |_| dialog.set(Some(PrivateDialog::Add))><Icon icon=lu::LuUserPlus/></button>
                    </Show>
                } })}
                <a class="k-btn k-btn--icon" href="/search" aria-label="Rechercher" title="Rechercher"><Icon icon=lu::LuSearch/></a>
                <a class="k-btn k-btn--icon" href="/inbox" aria-label="Boîte de réception" title="Boîte de réception"><Icon icon=lu::LuInbox/></a>
            </div>
        </Header>
        <div class="app-body">
            <section class="app-chat" aria-label=label>
                <MessageList channel=cid title=title.into() is_dm=is_dm/>
                <Show when=move || other_blocked.get() fallback=move || view! { <Composer placeholder=place/> }>
                    <div class="app-dm-blocked" role="status">
                        <Icon icon=lu::LuBan/>
                        <span>{format!("Tu as bloqué {other_name} : vous ne pouvez plus vous écrire ici. L'historique reste lisible.")}</span>
                        <button class="k-btn k-btn--sm" on:click=unblock>"Débloquer"</button>
                    </div>
                    <Show when=move || unblock_error.get().is_some()><p class="k-error app-compose-note" role="alert">{move || unblock_error.get()}</p></Show>
                </Show>
                {move || chat.picker.get().map(|p| {
                    let title = if matches!(p, Pick::React(_)) { "Réagir avec un emoji" } else { "Insérer un emoji" };
                    view! { <EmojiPicker title=title on_pick=on_pick on_close=on_close/> }
                })}
            </section>
            {(!is_dm).then(|| view! { <Show when=move || members_open.get()><Members guild=channel.guild_id.unwrap()/></Show> })}
        </div>
        {move || chat.deleting.get().map(|m| view! { <DeleteDialog msg=m/> })}
        {move || dialog.get().map(|d| {
            use crate::social::{Confirm, ConfirmDialog, PeoplePicker, PickFor, RenameGroup};
            let c = untrack(|| s.channel(cid))?;
            Some(match d {
                PrivateDialog::Add if kind == ChannelKind::Dm => {
                    view! { <PeoplePicker purpose=PickFor::New(c.recipients.iter().map(|u| u.id).collect()) close=close_dialog/> }.into_any()
                }
                PrivateDialog::Add => {
                    let members = c.recipients.iter().map(|u| u.id).chain(my_id()).collect();
                    view! { <PeoplePicker purpose=PickFor::Group { channel: cid, members } close=close_dialog/> }.into_any()
                }
                PrivateDialog::Rename => view! { <RenameGroup channel=c close=close_dialog/> }.into_any(),
                PrivateDialog::Leave => view! { <ConfirmDialog what=Confirm::LeaveGroup(c) close=close_dialog/> }.into_any(),
            })
        })}
    }
}

/// Clé d'une ligne du fil : elle change (et la ligne est redessinée) seulement si le message
/// change.
fn row_key(m: &Message, cont: bool) -> (Id, bool, u64) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(m).unwrap_or_default().hash(&mut h);
    (m.id, cont, h.finish())
}

#[component]
fn MessageList(channel: Id, title: Signal<String>, is_dm: bool) -> impl IntoView {
    let s = store();
    let log = Memo::new(move |_| s.logs.with(|l| l.get(&channel).cloned().unwrap_or_default()));
    let status = Memo::new(move |_| log.with(|l| (l.loaded, l.error.clone())));
    let rows = Memo::new(move |_| {
        log.with(|l| {
            let mut prev: Option<(Option<Id>, i64)> = None;
            l.items
                .iter()
                .map(|m| {
                    let author = m.author.as_ref().map(|a| a.id);
                    let cont = m.reply.is_none()
                        && prev.is_some_and(|(a, t)| a == author && author.is_some() && m.created_at - t < 7 * 60_000);
                    prev = Some((author, m.created_at));
                    (m.clone(), cont)
                })
                .collect::<Vec<_>>()
        })
    });
    let pending = Memo::new(move |_| log.with(|l| l.pending.clone()));
    let scroller = NodeRef::<html::Div>::new();
    // Rester collé en bas quand un message arrive, sauf pour qui lit plus haut ; un envoi
    // de sa part y ramène toujours.
    let stick = StoredValue::new(true);
    // Où lit celui qui est remonté : la liste peut être redessinée, sa position doit rester la même.
    let reading = StoredValue::new(0);
    let sent = StoredValue::new(0usize);
    Effect::new(move || {
        let pending = log.with(|l| l.pending.len());
        let mine = pending > sent.get_value();
        sent.set_value(pending);
        let Some(el) = scroller.get() else { return };
        if stick.get_value() || mine {
            request_animation_frame(move || el.set_scroll_top(el.scroll_height()));
        } else {
            let top = reading.get_value();
            request_animation_frame(move || el.set_scroll_top(top));
        }
    });
    let on_scroll = move |_| {
        if let Some(el) = scroller.get_untracked() {
            stick.set_value(el.scroll_top() + el.client_height() >= el.scroll_height() - 48);
            reading.set_value(el.scroll_top());
        }
    };
    let retry = move |p: Pending| {
        s.log(channel, |l| l.pending.retain(|x| x.nonce != p.nonce));
        // Même nonce : si le premier envoi était arrivé, le serveur renvoie ce message-là.
        send(channel, p.content, p.reply_to, Some(p.nonce));
    };
    let forget = move |nonce: String| s.log(channel, |l| l.pending.retain(|x| x.nonce != nonce));
    let intro = move || {
        let title = title.get();
        if is_dm {
            format!("Ici commence ta conversation avec {title}. Elle reste entre vous.")
        } else {
            format!("Ici commence #{title}. Tout l'historique du salon se lit à partir d'ici.")
        }
    };
    view! {
        <div class="app-log" node_ref=scroller on:scroll=on_scroll role="log" tabindex="0" aria-label="Messages" aria-live="polite" aria-busy=move || (!status.get().0).to_string()>
            {move || match status.get() {
                (false, _) => view! {
                    <div aria-hidden="true">
                        {(0..6).map(|_| view! { <div class="app-skel-msg"><span class="k-skeleton"></span><div class="k-skeleton"></div></div> }).collect_view()}
                    </div>
                }.into_any(),
                (true, Some(e)) => view! {
                    <div class="k-empty"><b>"Impossible de charger les messages"</b><span>{e}</span>
                    <button class="k-btn" on:click=move |_| s.log(channel, |l| l.loaded = false)>"Réessayer"</button></div>
                }.into_any(),
                (true, None) => view! {
                    <div class="app-intro">
                        <span class="app-intro-mark" aria-hidden="true">{if is_dm { view! { <Icon icon=lu::LuMessageCircle/> } } else { view! { <Icon icon=lu::LuHash/> } }}</span>
                        <div><h2>{move || title.get()}</h2><p>{intro}</p></div>
                    </div>
                    <For each=move || rows.get() key=|(m, cont)| row_key(m, *cont) children=move |(m, cont)| view! { <MessageRow msg=m cont=cont/> }/>
                    <For each=move || pending.get() key=|p| (p.nonce.clone(), p.error.clone()) children=move |p| {
                        let me = s.me.get_untracked();
                        let failed = p.error.clone();
                        let (again, nonce) = (p.clone(), p.nonce.clone());
                        let (pending_attr, failed_attr) = (failed.is_none().then_some(""), failed.is_some().then_some(""));
                        view! {
                            <article class="k-msg" data-pending=pending_attr data-failed=failed_attr>
                                {me.clone().map(|u| view! { <Avatar user=u/> })}
                                <div>
                                    <header><b>{me.map(|u| u.name().to_string())}</b><time>{if failed.is_some() { "pas envoyé" } else { "envoi…" }}</time></header>
                                    <Body content=p.content.clone() mentions=vec![] everyone=false guild=None/>
                                    {failed.map(|e| view! {
                                        <p class="k-error" role="alert">{e}" "
                                            <button class="k-btn k-btn--link" on:click=move |_| retry(again.clone())>"Réessayer"</button>" · "
                                            <button class="k-btn k-btn--link app-link-danger" on:click=move |_| forget(nonce.clone())>"Supprimer"</button>
                                        </p>
                                    })}
                                </div>
                            </article>
                        }
                    }/>
                }.into_any(),
            }}
        </div>
    }
}

/// « Lou », « Lou et Sam », « Lou, Sam, Noa et 2 autres ».
fn people(names: Vec<String>) -> String {
    match names.as_slice() {
        [] => "Personne".into(),
        [a] => a.clone(),
        [a, b] => format!("{a} et {b}"),
        [a, b, c] => format!("{a}, {b} et {c}"),
        [a, b, c, rest @ ..] => format!("{a}, {b}, {c} et {} autres", rest.len()),
    }
}

#[component]
fn MessageRow(msg: Message, cont: bool) -> impl IntoView {
    let s = store();
    let chat = chat();
    let me = my_id();
    let id = msg.id;
    let name = msg.author.as_ref().map(|u| u.name().to_string()).unwrap_or_else(|| "Compte supprimé".into());
    let handle = msg.author.as_ref().map(|u| format!("@{}", u.username));
    let at = msg.created_at;
    let mine = me.is_some() && msg.author.as_ref().map(|a| a.id) == me;
    let manage = chat.guild.is_some_and(|g| s.guild_perms(g) & perms::MANAGE_MESSAGES != 0);
    let mentioned = msg.mention_everyone || msg.mentions.iter().any(|u| Some(u.id) == me);
    let class = match (cont, mentioned) {
        (true, true) => "k-msg k-msg--cont k-msg--mention",
        (true, false) => "k-msg k-msg--cont",
        (false, true) => "k-msg k-msg--mention",
        (false, false) => "k-msg",
    };
    let reply = msg.reply.clone().map(|r| match (r.id, r.author.clone()) {
        (Some(rid), author) => {
            let who = author.map(|a| a.name().to_string()).unwrap_or_else(|| "Compte supprimé".into());
            let excerpt = kraken_core::markdown::summary(&r.content);
            let label = format!("Aller au message d'origine, de {who}");
            view! {
                <button type="button" class="k-reply app-reply" aria-label=label title=excerpt.clone() on:click=move |_| jump(chat, rid)>
                    <Icon icon=lu::LuCornerDownRight small=true/><b>{format!("@{who}")}</b><span class="app-ellipsis">{excerpt.clone()}</span>
                </button>
            }.into_any()
        }
        (None, _) => view! {
            <p class="k-reply app-reply"><Icon icon=lu::LuCornerDownRight small=true/><span>"Le message d'origine a été supprimé."</span></p>
        }.into_any(),
    });
    let reactions = msg.reactions.clone();
    let reaction_bar = (!reactions.is_empty()).then(|| {
        let buttons = reactions.into_iter().map(|r| {
            let n = r.users.len();
            let by_me = me.is_some_and(|me| r.users.contains(&me));
            let names = r.users.iter().map(|u| s.name_of(chat.guild, *u).unwrap_or_else(|| "quelqu'un".into())).collect();
            let who = people(names);
            let verb = if n > 1 { "ont réagi" } else { "a réagi" };
            let title = format!("{who} {verb} avec {}", r.emoji);
            let label = format!("{} : {n} {}{}", r.emoji, if n > 1 { "réactions" } else { "réaction" }, if by_me { ", dont la tienne" } else { "" });
            let emoji = r.emoji.clone();
            view! {
                <button type="button" class="k-reaction" aria-pressed=by_me.to_string() aria-label=label title=title
                    on:click=move |_| react(chat, id, emoji.clone())>
                    <span aria-hidden="true">{r.emoji.clone()}</span><span aria-hidden="true">{n}</span>
                </button>
            }
        }).collect_view();
        view! {
            <div class="k-reactions" role="group" aria-label="Réactions">
                {buttons}
                <button type="button" class="k-reaction" aria-label="Autre réaction" title="Autre réaction"
                    on:click=move |_| chat.picker.set(Some(Pick::React(id)))><Icon icon=lu::LuSmilePlus small=true/></button>
            </div>
        }
    });
    let for_reply = msg.clone();
    let for_delete = msg.clone();
    let content = msg.content.clone();
    let body = StoredValue::new(msg.clone());
    let editing = move || chat.editing.get() == Some(id);
    // Auteur bloqué : le message est replié derrière une ligne, dépliable au clic.
    let author_id = msg.author.as_ref().map(|a| a.id);
    let blocked = Memo::new(move |_| author_id.is_some_and(|a| s.is_blocked(a)));
    let shown = RwSignal::new(false);
    view! {
        <Show when=move || blocked.get()>
            <div class="app-blocked">
                <Icon icon=lu::LuEyeOff small=true/>
                <span>"Message d'un compte que tu as bloqué."</span>
                <button type="button" class="k-btn k-btn--link" aria-expanded=move || shown.get().to_string() aria-controls=format!("msg-{id}")
                    on:click=move |_| shown.update(|v| *v = !*v)>
                    {move || if shown.get() { "Masquer" } else { "Afficher" }}
                </button>
            </div>
        </Show>
        <article id=format!("msg-{id}") class=class tabindex="-1" aria-label=format!("{name}, {}", when(at))
            hidden=move || blocked.get() && !shown.get()>
            {reply}
            {if cont {
                view! { <time class="app-gutter-time" datetime=iso(at)>{when(at).rsplit(' ').next().unwrap_or_default().to_string()}</time> }.into_any()
            } else {
                match msg.author.clone() {
                    Some(u) => view! { <Avatar user=u/> }.into_any(),
                    None => view! { <span class="k-avatar" aria-hidden="true">"?"</span> }.into_any(),
                }
            }}
            <div>
                {(!cont).then(|| view! { <header><b title=handle>{name.clone()}</b><time datetime=iso(at)>{when(at)}</time></header> })}
                {move || if editing() {
                    view! { <EditBox msg=body.get_value()/> }.into_any()
                } else {
                    let m = body.get_value();
                    let edited = m.edited_at.map(|e| view! { <span class="app-edited" title=format!("Modifié le {}", when(e))>"(édité)"</span> });
                    view! { <Body content=m.content mentions=m.mentions everyone=m.mention_everyone guild=chat.guild/>{edited} }.into_any()
                }}
                {reaction_bar}
            </div>
            <Show when=move || !editing()>
                <div class="k-msg-actions" role="toolbar" aria-label="Actions sur le message">
                    <button type="button" class="k-btn k-btn--icon" aria-label="Ajouter une réaction" title="Réagir"
                        on:click=move |_| chat.picker.set(Some(Pick::React(id)))><Icon icon=lu::LuSmilePlus/></button>
                    <button type="button" class="k-btn k-btn--icon" aria-label="Répondre" title="Répondre"
                        on:click={ let m = for_reply.clone(); move |_| start_reply(chat, m.clone()) }><Icon icon=lu::LuReply/></button>
                    {mine.then(|| { let c = content.clone(); view! {
                        <button type="button" class="k-btn k-btn--icon" aria-label="Modifier le message" title="Modifier"
                            on:click=move |_| { chat.edit_text.set(c.clone()); chat.editing.set(Some(id)); }><Icon icon=lu::LuPencil/></button>
                    } })}
                    {(mine || manage).then(|| { let m = for_delete.clone(); view! {
                        <button type="button" class="k-btn k-btn--icon app-link-danger" aria-label="Supprimer le message" title="Supprimer"
                            on:click=move |_| chat.deleting.set(Some(m.clone()))><Icon icon=lu::LuTrash2/></button>
                    } })}
                </div>
            </Show>
        </article>
    }
}

fn start_reply(chat: Chat, m: Message) {
    chat.replying.set(Some(m));
    if let Some(t) = chat.composer.get_untracked() {
        let _ = t.focus();
    }
}

/// Modifier son message : Entrée enregistre, Échap annule ; vide, on propose de le supprimer.
#[component]
fn EditBox(msg: Message) -> impl IntoView {
    let chat = chat();
    let s = store();
    let area = NodeRef::<html::Textarea>::new();
    Effect::new(move || {
        if let Some(t) = area.get() {
            let _ = t.focus();
            let n = t.value().encode_utf16().count() as u32;
            let _ = t.set_selection_range(n, n);
        }
    });
    let busy = RwSignal::new(false);
    let original = msg.content.clone();
    let m = StoredValue::new(msg);
    let save = move || {
        let text = chat.edit_text.get_untracked().trim().to_string();
        let msg = m.get_value();
        if text.is_empty() {
            chat.deleting.set(Some(msg));
            return;
        }
        if text == original.trim() {
            chat.editing.set(None);
            return;
        }
        if text.chars().count() > MAX_MESSAGE_LEN || busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match crate::api::edit_message(chat.channel, msg.id, &text).await {
                Ok(m) => {
                    s.log(chat.channel, |l| l.update(m));
                    chat.editing.set(None);
                }
                Err(e) => chat.notice.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    let save2 = save.clone();
    let cancel = move || chat.editing.set(None);
    view! {
        <form class="k-msg-edit" on:submit=move |e| { e.prevent_default(); save(); }>
            <textarea class="k-input" rows="2" aria-label="Modifier le message" node_ref=area bind:value=chat.edit_text
                on:keydown=move |e| {
                    if e.key() == "Escape" { e.prevent_default(); e.stop_propagation(); cancel(); }
                    else if e.key() == "Enter" && !e.shift_key() && !e.is_composing() { e.prevent_default(); save2(); }
                }></textarea>
            <p class="k-hint">"Entrée enregistre, Échap annule. "
                <button type="button" class="k-btn k-btn--link" on:click=move |_| cancel()>"Annuler"</button>" · "
                <button type="submit" class="k-btn k-btn--link" aria-busy=move || busy.get().to_string()>"Enregistrer"</button>
            </p>
        </form>
    }
}

#[component]
fn DeleteDialog(msg: Message) -> impl IntoView {
    let chat = chat();
    let s = store();
    let root = NodeRef::new();
    modal(root, move || chat.deleting.set(None));
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let id = msg.id;
    let theirs = msg.author.as_ref().map(|a| a.id) != my_id();
    let preview = kraken_core::markdown::summary(&msg.content);
    let confirm = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        busy.set(true);
        spawn_local(async move {
            match crate::api::delete_message(chat.channel, id).await {
                Ok(()) => {
                    s.log(chat.channel, |l| l.remove(id));
                    if chat.editing.get_untracked() == Some(id) {
                        chat.editing.set(None);
                    }
                    if chat.replying.with_untracked(|r| r.as_ref().is_some_and(|r| r.id == id)) {
                        chat.replying.set(None);
                    }
                    chat.deleting.set(None);
                }
                Err(e) => {
                    error.set(Some(e.message));
                    busy.set(false);
                }
            }
        });
    };
    view! {
        <div class="k-scrim" node_ref=root>
            <div class="k-dialog" role="alertdialog" aria-modal="true" aria-labelledby="msg-del-t" aria-describedby="msg-del-d">
                <header><h2 id="msg-del-t">"Supprimer ce message ?"</h2></header>
                <form on:submit=confirm>
                    <div>
                        <p id="msg-del-d">{if theirs {
                            "Il disparaîtra pour tout le monde, sans retour possible. Son auteur n'en sera pas prévenu."
                        } else {
                            "Il disparaîtra pour tout le monde, sans retour possible."
                        }}</p>
                        <blockquote class="app-quote app-del-preview">{preview}</blockquote>
                        <Show when=move || error.get().is_some()><p class="k-error" role="alert">{move || error.get()}</p></Show>
                    </div>
                    <footer>
                        <button type="button" class="k-btn k-btn--ghost" on:click=move |_| chat.deleting.set(None) autofocus>"Annuler"</button>
                        <button type="submit" class="k-btn k-btn--danger" aria-busy=move || busy.get().to_string()>"Supprimer"</button>
                    </footer>
                </form>
            </div>
        </div>
    }
}

/// Ajoute ma réaction, ou la retire si elle y est déjà.
fn react(chat: Chat, mid: Id, emoji: String) {
    let s = store();
    let Some(me) = my_id() else { return };
    let on = !s.logs.with_untracked(|l| {
        l.get(&chat.channel)
            .and_then(|l| l.items.iter().find(|m| m.id == mid))
            .is_some_and(|m| m.reactions.iter().any(|r| r.emoji == emoji && r.users.contains(&me)))
    });
    spawn_local(async move {
        match crate::api::react(chat.channel, mid, &emoji, on).await {
            Ok(()) => s.log(chat.channel, |l| l.react(mid, me, &emoji, on)),
            Err(e) => chat.notice.set(Some(e.message)),
        }
    });
}

/// Fait défiler jusqu'au message (en chargeant l'historique plus ancien s'il le faut) et le
/// met en évidence un instant.
fn jump(chat: Chat, id: Id) {
    let s = store();
    spawn_local(async move {
        for _ in 0..20 {
            let (here, oldest) = s.logs.with_untracked(|l| {
                let l = l.get(&chat.channel);
                (l.is_some_and(|l| l.items.iter().any(|m| m.id == id)), l.and_then(|l| l.items.first().map(|m| m.id)))
            });
            // Ids triés par date : plus récent que le plus ancien chargé et absent, il n'existe plus.
            let Some(oldest) = oldest.filter(|o| !here && *o > id) else { break };
            match crate::api::messages_before(chat.channel, oldest).await {
                Ok(v) if !v.is_empty() => s.log(chat.channel, |l| v.into_iter().for_each(|m| l.insert(m))),
                _ => break,
            }
        }
        // Après le rendu et le recalage du défilement.
        TimeoutFuture::new(80).await;
        let Some(el) = document().get_element_by_id(&format!("msg-{id}")) else {
            chat.notice.set(Some("Ce message n'est plus disponible.".into()));
            return;
        };
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Center);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
        let _ = el.set_attribute("data-flash", "");
        if let Ok(h) = el.clone().dyn_into::<web_sys::HtmlElement>() {
            let _ = h.focus();
        }
        TimeoutFuture::new(2_000).await;
        let _ = el.remove_attribute("data-flash");
    });
}

/// Position UTF-16 (celle du navigateur) → position en octets dans `s`.
fn byte_at(s: &str, utf16: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= utf16 {
            return i;
        }
        n += c.len_utf16();
    }
    s.len()
}

fn caret(chat: Chat) -> usize {
    let text = chat.draft.get_untracked();
    let pos = chat.composer.get_untracked().and_then(|t| t.selection_start().ok().flatten());
    pos.map_or(text.len(), |p| byte_at(&text, p as usize))
}

/// Remplace `text[from..to]` par `with` dans la saisie et place le curseur après.
fn splice(chat: Chat, from: usize, to: usize, with: &str) {
    let mut text = chat.draft.get_untracked();
    if from > to || !text.is_char_boundary(from) || !text.is_char_boundary(to) || to > text.len() {
        return;
    }
    text.replace_range(from..to, with);
    let at = text[..from + with.len()].encode_utf16().count() as u32;
    chat.draft.set(text);
    if let Some(t) = chat.composer.get_untracked() {
        request_animation_frame(move || {
            let _ = t.focus();
            let _ = t.set_selection_range(at, at);
        });
    }
}

fn insert_text(chat: Chat, s: &str) {
    let at = caret(chat);
    splice(chat, at, at, s);
}

fn nonce() -> String {
    format!("{:x}{:x}", js_sys::Date::now() as u64, (js_sys::Math::random() * 1e12) as u64)
}

/// Envoi optimiste : le message apparaît grisé, puis est remplacé par le MESSAGE_CREATE du
/// gateway (même nonce) ou par la réponse REST si le gateway est en retard.
fn send(channel: Id, content: String, reply_to: Option<Id>, again: Option<String>) {
    let s = store();
    let n = again.unwrap_or_else(nonce);
    s.log(channel, |l| l.pending.push(Pending { nonce: n.clone(), content: content.clone(), error: None, reply_to }));
    spawn_local(async move {
        match crate::api::send_message(channel, &content, &n, reply_to).await {
            Ok(m) => s.log(channel, |l| {
                l.pending.retain(|p| p.nonce != n);
                l.insert(m);
            }),
            Err(e) => s.log(channel, |l| {
                if let Some(p) = l.pending.iter_mut().find(|p| p.nonce == n) {
                    p.error = Some(e.message);
                }
            }),
        }
    });
}

/// Une suggestion de l'autocomplétion : texte inséré, nom montré, précision.
#[derive(Clone, PartialEq)]
struct Suggestion {
    insert: String,
    label: String,
    detail: String,
}

/// Le mot en cours de frappe s'il commence par `@` ou `#` : sa position et ce qui suit le signe.
fn token(text: &str, caret: usize) -> Option<(usize, char, String)> {
    // Le curseur a pu être lu sur un texte plus ancien.
    let before = text.get(..caret.min(text.len()))?;
    let start = before.rfind(['@', '#'])?;
    let sign = before[start..].chars().next()?;
    let q = &before[start + 1..];
    if q.chars().any(char::is_whitespace) || q.chars().count() > 32 {
        return None;
    }
    if before[..start].chars().next_back().is_some_and(char::is_alphanumeric) {
        return None;
    }
    Some((start, sign, q.to_lowercase()))
}

fn suggestions(chat: Chat, sign: char, q: &str) -> Vec<Suggestion> {
    let s = store();
    let mut out = Vec::new();
    if sign == '@' {
        let people: Vec<(User, Option<String>)> = match chat.guild {
            Some(g) => {
                s.guild(g).map(|g| g.members.into_iter().map(|m| (m.user, m.nick)).collect()).unwrap_or_default()
            }
            None => {
                let mut v: Vec<(User, Option<String>)> = s
                    .channel(chat.channel)
                    .map(|c| c.recipients.into_iter().map(|u| (u, None)).collect())
                    .unwrap_or_default();
                v.extend(s.me.get_untracked().map(|u| (u, None)));
                v
            }
        };
        for (u, nick) in people {
            let shown = nick.unwrap_or_else(|| u.name().to_string());
            if u.username.starts_with(q) || shown.to_lowercase().split_whitespace().any(|w| w.starts_with(q)) {
                out.push(Suggestion {
                    insert: format!("@{} ", u.username),
                    label: shown,
                    detail: format!("@{}", u.username),
                });
            }
        }
        out.truncate(8);
        if chat.guild.is_some_and(|g| s.guild_perms(g) & perms::MENTION_EVERYONE != 0) {
            for (word, what) in
                [("everyone", "Tous ceux qui voient ce salon"), ("here", "Ceux qui sont là en ce moment")]
            {
                if word.starts_with(q) {
                    out.push(Suggestion {
                        insert: format!("@{word} "),
                        label: format!("@{word}"),
                        detail: what.into(),
                    });
                }
            }
        }
    } else if let Some(g) = chat.guild.and_then(|g| s.guild(g)) {
        for c in g.channels.iter().filter(|c| c.kind.is_textual()) {
            let Some(name) = c.name.clone() else { continue };
            if name.to_lowercase().starts_with(q) && !name.chars().any(char::is_whitespace) {
                out.push(Suggestion { insert: format!("#{name} "), label: format!("#{name}"), detail: String::new() });
            }
        }
        out.truncate(8);
    }
    out
}

#[component]
fn Composer(placeholder: Signal<String>) -> impl IntoView {
    let s = store();
    let chat = chat();
    let text = chat.draft;
    let too_long = move || text.with(|t| t.chars().count() > MAX_MESSAGE_LEN);
    // Autocomplétion des mentions : le mot sous le curseur, la suggestion choisie, refermée par Échap.
    let at = RwSignal::new(0usize);
    let active = RwSignal::new(0usize);
    let dismissed = RwSignal::new(false);
    let found = Memo::new(move |_| {
        if dismissed.get() {
            return None;
        }
        let (start, sign, q) = text.with(|t| token(t, at.get()))?;
        let list = suggestions(chat, sign, &q);
        (!list.is_empty()).then_some((start, list))
    });
    let track = move || {
        at.set(caret(chat));
        dismissed.set(false);
        active.set(0);
    };
    let pick = move |i: usize| {
        let Some((start, list)) = found.get_untracked() else { return };
        if let Some(sg) = list.get(i) {
            splice(chat, start, at.get_untracked(), &sg.insert);
            dismissed.set(true);
        }
    };
    let submit = move || {
        let t = text.get_untracked();
        if t.trim().is_empty() || too_long() {
            return;
        }
        at.set(0);
        text.set(String::new());
        let reply = chat.replying.get_untracked().map(|m| m.id);
        chat.replying.set(None);
        send(chat.channel, t.trim().to_string(), reply, None);
    };
    let keydown = move |e: leptos::ev::KeyboardEvent| {
        let key = e.key();
        if let Some((_, list)) = found.get_untracked() {
            let n = list.len();
            match key.as_str() {
                "ArrowDown" => active.update(|a| *a = (*a + 1) % n),
                "ArrowUp" => active.update(|a| *a = (*a + n - 1) % n),
                "Enter" | "Tab" if !e.shift_key() => pick(active.get_untracked()),
                "Escape" => dismissed.set(true),
                _ => return,
            }
            e.prevent_default();
            return;
        }
        if key == "Escape" && chat.replying.with_untracked(Option::is_some) {
            e.prevent_default();
            chat.replying.set(None);
        } else if key == "ArrowUp" && text.with_untracked(String::is_empty) {
            // Flèche haut dans une saisie vide : modifier son dernier message.
            let me = my_id();
            let last = s.logs.with_untracked(|l| {
                l.get(&chat.channel)?
                    .items
                    .iter()
                    .rev()
                    .find(|m| me.is_some() && m.author.as_ref().map(|a| a.id) == me)
                    .cloned()
            });
            if let Some(m) = last {
                e.prevent_default();
                chat.edit_text.set(m.content.clone());
                chat.editing.set(Some(m.id));
            }
        } else if key == "Enter" && !e.shift_key() && !e.is_composing() {
            e.prevent_default();
            submit();
        }
    };
    let list_id = format!("ac-{}", chat.channel);
    let list_id2 = list_id.clone();
    let opt_id = move |i: usize| format!("ac-{}-{i}", chat.channel);
    view! {
        <div class="app-compose">
            {move || chat.notice.get().map(|n| view! {
                <p class="k-error app-compose-note" role="alert">{n}" "
                    <button type="button" class="k-btn k-btn--link" on:click=move |_| chat.notice.set(None)>"Fermer"</button></p>
            })}
            <Show when=too_long>
                <p class="k-error app-compose-note" role="alert">{move || format!("{} caractères sur {MAX_MESSAGE_LEN} : raccourcis un peu.", text.with(|t| t.chars().count()))}</p>
            </Show>
            {move || found.get().map(|(_, list)| view! {
                <ul class="k-autocomplete app-suggest" role="listbox" id=list_id.clone() aria-label="Suggestions">
                    {list.into_iter().enumerate().map(|(i, sg)| view! {
                        <li role="option" id=opt_id(i) aria-selected=move || (active.get() == i).to_string()
                            on:mousedown=move |e| { e.prevent_default(); pick(i); }>
                            <b>{sg.label}</b><small>{sg.detail}</small>
                        </li>
                    }).collect_view()}
                </ul>
            })}
            {move || chat.replying.get().map(|m| {
                let who = m.author.as_ref().map(|a| a.name().to_string()).unwrap_or_else(|| "Compte supprimé".into());
                view! {
                    <div class="app-replying">
                        <Icon icon=lu::LuReply small=true/>
                        <span class="app-ellipsis">"Tu réponds à "<b>{who}</b></span>
                        <button type="button" class="k-btn k-btn--icon" aria-label="Annuler la réponse" title="Annuler la réponse (Échap)"
                            on:click=move |_| chat.replying.set(None)><Icon icon=lu::LuX small=true/></button>
                    </div>
                }
            })}
            <form class="k-composer" on:submit=move |e| { e.prevent_default(); submit(); }>
                <button type="button" class="k-btn k-btn--icon" aria-label="Joindre un fichier (bientôt)" disabled><Icon icon=lu::LuPlus/></button>
                <textarea rows="1" placeholder=placeholder aria-label=placeholder bind:value=text node_ref=chat.composer
                    aria-autocomplete="list"
                    aria-controls=move || found.get().map(|_| list_id2.clone())
                    aria-activedescendant=move || found.get().map(|_| opt_id(active.get()))
                    on:input=move |_| track() on:click=move |_| track() on:keyup=move |e| {
                        if matches!(e.key().as_str(), "ArrowLeft" | "ArrowRight" | "Home" | "End") { track() }
                    }
                    on:keydown=keydown></textarea>
                <button type="button" class="k-btn k-btn--icon" aria-label="Insérer un emoji" title="Emoji"
                    on:click=move |_| chat.picker.set(Some(Pick::Insert))><Icon icon=lu::LuSmile/></button>
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
        <aside class="app-aside app-aside--members" aria-label="Membres">
            <h2 class="k-category">"Membres "<span class="k-badge k-badge--neutral">{move || members().len()}</span></h2>
            <ul class="app-members">
                <For each=members key=|m| (m.user.id, m.user.display_name.clone(), m.nick.clone()) let:m>
                    <li class="k-dm">
                        <Avatar user=m.user.clone() size="md"/>
                        <div class="app-ellipsis">
                            {m.nick.clone().unwrap_or_else(|| m.user.name().to_string())}
                            <small>{format!("@{}", m.user.username)}</small>
                        </div>
                    </li>
                </For>
            </ul>
        </aside>
    }
}
