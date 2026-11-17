//! La coquille : rail des serveurs (S02) avec mon compte en bas (S04), barre latérale
//! (S03 ou salons), zone principale. Plus la création / l'adhésion à un serveur (S21).

use kraken_proto::{Channel, ChannelKind, GuildState, Id, Presence};
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::components::Outlet;
use leptos_router::hooks::{use_location, use_navigate};

use crate::call::{CallAudio, ControlPanel, IncomingCalls, VoicePanel, VoiceUsers};
use crate::i18n::{t, tf, tn};
use crate::icons::{Icon, lu};
use crate::store::{Conn, store};
use crate::tk;
use crate::ui::{Avatar, Dot, GuildIcon, Logo, StatusDot, initials, modal};
use crate::unread::{MentionBadge, MutedMark, NotifyDialog, Target, TargetMenu, UnreadText};
use crate::update::{UpdateEntry, UpdateTile, Updater};

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
/// de ses salons. Un salon dont la catégorie m'est cachée (privée) s'affiche hors catégorie.
pub fn ordered(channels: &[Channel]) -> Vec<Channel> {
    let shown = |id: Id| channels.iter().any(|c| c.id == id && c.kind == ChannelKind::Category);
    let mut top: Vec<&Channel> = channels.iter().filter(|c| c.parent_id.is_none_or(|p| !shown(p))).collect();
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
    // Après un changement de langue fait dans les Réglages, la zone d'information dit la langue choisie
    // (dans la nouvelle langue), puis s'efface d'elle-même.
    if let Some(said) = crate::i18n::take_announcement() {
        s.notice.set(Some(said.clone()));
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(6_000).await;
            s.notice.try_update(|n| {
                if n.as_deref() == Some(said.as_str()) {
                    *n = None;
                }
            });
        });
    }
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
    // Le serveur affiché disparaît de ma liste (parti, exclu, banni, y compris depuis un autre
    // appareil) : retour à l'accueil. Un serveur qui n'a jamais été là garde sa page « Introuvable ».
    let seen = StoredValue::new(None::<Id>);
    let home = use_navigate();
    Effect::new(move || {
        let Some(g) = ids.get().0 else { return seen.set_value(None) };
        if s.guild(g).is_some() {
            seen.set_value(Some(g));
        } else if seen.get_value() == Some(g) {
            seen.set_value(None);
            home("/channels/@me", NavigateOptions { replace: true, ..Default::default() });
        }
    });
    // Le salon affiché disparaît (supprimé, ou devenu privé sans moi) : on rouvre le serveur.
    let seen_channel = StoredValue::new(None::<Id>);
    let reopen = use_navigate();
    Effect::new(move || {
        let (Some(g), Some(c)) = ids.get() else { return seen_channel.set_value(None) };
        if s.channel(c).is_some() {
            seen_channel.set_value(Some(c));
        } else if seen_channel.get_value() == Some(c) && s.guild(g).is_some() {
            seen_channel.set_value(None);
            reopen(&format!("/channels/{g}"), NavigateOptions { replace: true, ..Default::default() });
        }
    });
    let notify_ui = crate::unread::provide_ui();
    // Clic sur une notification du bureau : on va au salon.
    let go = use_navigate();
    Effect::new(move || {
        if let Some(to) = s.goto.get() {
            s.goto.set(None);
            go(&to, Default::default());
        }
    });
    // Tous les raccourcis clavier (tableau unique : `shortcuts.rs`) et les endroits visités (« Récents »).
    let keys = crate::shortcuts::install(ids);
    crate::switcher::track_visits(ids);

    view! {
        <div class="app" data-drawer=move || s.drawer.get().then_some("open")>
            // Hors routeur : le navigateur suit l'ancre, et le focus va bien dans le contenu.
            <a class="app-skip" href="#main" rel="external" on:click=|_| {
                use wasm_bindgen::JsCast;
                let main = document().get_element_by_id("main").and_then(|m| m.dyn_into::<web_sys::HtmlElement>().ok());
                if let Some(m) = main { let _ = m.focus(); }
            }>{t("Aller au contenu")}</a>
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
                        <span>{move || if s.conn.get() == Conn::Connecting { t("Connexion au serveur…") } else { t("Connexion perdue, nouvelle tentative…") }}</span>
                    </div>
                </Show>
                // Zone `status` toujours présente : un message qui arrive dans une zone déjà là est annoncé.
                <div class="app-notice" role="status">
                    <Show when=move || s.notice.get().is_some()>
                        <div class="k-banner app-conn">
                            <Icon icon=lu::LuInfo/>
                            <span>{move || s.notice.get()}</span>
                            <button type="button" class="k-btn k-btn--icon" aria-label=t("Fermer l'information") on:click=move |_| s.notice.set(None)>
                                <Icon icon=lu::LuX small=true/>
                            </button>
                        </div>
                    </Show>
                </div>
                <Outlet/>
            </main>
            <CallAudio/>
            <ControlPanel/>
            <IncomingCalls/>
            <TargetMenu/>
            <crate::profile::ProfileCardHost/>
            {move || notify_ui.dialog.get().map(|t| view! { <NotifyDialog target=t/> })}
            // Les fenêtres des raccourcis sortent de la coquille (elles passent devant les réglages, ouverts ou non).
            <Show when=move || keys.switcher.get()>
                <Portal><crate::switcher::Switcher/></Portal>
            </Show>
            <Show when=move || keys.help.get()>
                <Portal><crate::shortcuts::ShortcutHelp close=Callback::new(move |()| keys.help.set(false))/></Portal>
            </Show>
            // Annonces des raccourcis (micro, son) : une zone polie sans rôle `status`, pour ne pas doubler les autres.
            <p class="k-sr" aria-live="polite" aria-atomic="true">{move || keys.say.get()}</p>
        </div>
    }
}

#[component]
fn Rail() -> impl IntoView {
    let s = store();
    let ids = current_ids();
    // Appli de bureau : veille des mises à jour (la tuile, et l'entrée de « Mon compte »).
    provide_context(Updater::start());
    let home_current = move || ids.get().0.is_none().then_some("page");
    let u = crate::unread::ui();
    let private = Memo::new(move |_| s.private_mentions());
    let home_label = move || match private.get() {
        0 => t("Accueil et messages privés"),
        n => format!("{}, {}", t("Accueil et messages privés"), tn("{n} mention", "{n} mentions", n)),
    };
    view! {
        <div class="k-rail app-rail">
            <nav class="app-rail-list" aria-label=t("Serveurs")>
                <a class="k-server k-server--home" href="/channels/@me" aria-label=home_label aria-current=home_current>
                    <Logo/>
                    <Show when=move || { private.get() > 0 }>
                        <span class="k-badge k-badge--count" aria-hidden="true">{move || crate::unread::count_text(private.get())}</span>
                    </Show>
                </a>
                <hr/>
                <For each=move || s.guilds.get() key=|g| g.guild.id let:g>
                    {
                        let id = g.guild.id;
                        let href = format!("/channels/{id}");
                        // Nom et icône suivent `GUILD_UPDATE` : la ligne du rail garde son id, pas son contenu.
                        let first = g.guild.clone();
                        let info = Memo::new(move |_| {
                            s.guilds.with(|gs| gs.iter().find(|x| x.guild.id == id).map(|x| x.guild.clone())).unwrap_or_else(|| first.clone())
                        });
                        let name = move || info.get().name;
                        let badge = Memo::new(move |_| s.guild_badge(id));
                        let label = move || {
                            let (unread, n) = badge.get();
                            let mut l = tf("Serveur {nom}", &[("nom", &name())]);
                            if unread { l.push_str(", "); l.push_str(&t("non lu")); }
                            if n > 0 { l.push_str(", "); l.push_str(&tn("{n} mention", "{n} mentions", n)); }
                            l
                        };
                        view! {
                            <a class="k-server" href=href title=name aria-label=label
                                data-unread=move || badge.get().0.then_some("")
                                aria-current=move || (ids.get().0 == Some(id)).then_some("page")
                                on:contextmenu=u.on_context(Target::Guild(id)) on:keydown=u.on_key(Target::Guild(id))>
                                {move || view! { <GuildIcon guild=info.get()/> }}
                                <Show when=move || { badge.get().1 > 0 }>
                                    <span class="k-badge k-badge--count" aria-hidden="true">{move || crate::unread::count_text(badge.get().1)}</span>
                                </Show>
                            </a>
                        }
                    }
                </For>
                <a class="k-server k-server--action" href="/guilds/new" aria-label=t("Créer ou rejoindre un serveur") title=t("Créer ou rejoindre")>
                    <Icon icon=lu::LuPlus/>
                </a>
                <a class="k-server k-server--action" href="/discover" aria-label=t("Explorer les serveurs publics") title=t("Explorer")>
                    <Icon icon=lu::LuCompass/>
                </a>
            </nav>
            <UpdateTile/>
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
    let navigate = use_navigate();
    let incoming = Memo::new(move |_| {
        s.friends.with(|f| f.iter().filter(|r| r.kind == kraken_proto::RelationshipKind::PendingIncoming).count())
    });
    view! {
        <nav class="k-sidebar app-sidebar" aria-label=t("Messages privés")>
            <crate::switcher::SwitcherButton/>
            <a class="k-channel" href="/channels/@me" aria-current=on_friends><Icon icon=lu::LuUsers/>{t("Amis")}
                <Show when=move || { incoming.get() > 0 }>
                    <span class="k-badge k-badge--count">{move || incoming.get()}</span>
                    <span class="k-sr">" "{move || tn("(demande en attente)", "(demandes en attente)", incoming.get())}</span>
                </Show>
            </a>
            <div class="app-dm-head">
                <h2 class="k-category">{t("Messages privés")}</h2>
                <button class="k-btn k-btn--icon" aria-label=t("Nouvelle conversation ou groupe") title=t("Nouvelle conversation")
                    on:click=move |_| picking.set(true)>
                    <Icon icon=lu::LuPlus small=true/>
                </button>
            </div>
            <Show when=move || s.dms.with(|d| d.is_empty()) && s.conn.get() == Conn::Ready>
                <p class="app-hint">{t("Aucune conversation pour l'instant.")}</p>
            </Show>
            <For each=move || s.dms.get() key=|c| (c.id, c.name.clone(), c.owner_id, c.recipients.iter().map(|u| (u.name().to_string(), u.avatar.clone())).collect::<Vec<_>>()) let:c>
                {
                    let id = c.id;
                    let title = crate::social::private_title(&c);
                    let first = c.recipients.first().cloned();
                    let group = c.kind == ChannelKind::GroupDm;
                    let count = c.recipients.len() + 1;
                    let calling = Memo::new(move |_| !s.present(None, id).is_empty());
                    let u = crate::unread::ui();
                    let unread = Memo::new(move |_| s.channel(id).is_some_and(|c| s.shows_unread(&c)));
                    let muted = Memo::new(move |_| s.channel(id).is_some_and(|c| s.channel_muted(&c)));
                    let mentions = Memo::new(move |_| s.channel(id).map_or(0, |c| s.mentions(&c)));
                    // Fermer un MP (pas un groupe, qu'on quitte) : il revient avec le prochain message.
                    let close_dm = (!group).then(|| {
                        let navigate = navigate.clone();
                        let name = title.clone();
                        let close = move |_| {
                            let navigate = navigate.clone();
                            spawn_local(async move {
                                if crate::api::close_dm(id).await.is_ok() {
                                    if loc.pathname.get_untracked().ends_with(&id.to_string()) {
                                        navigate("/channels/@me", Default::default());
                                    }
                                    s.dms.update(|d| d.retain(|c| c.id != id));
                                }
                            });
                        };
                        view! {
                            <button type="button" class="k-btn k-btn--icon app-dm-close" aria-label=tf("Fermer la conversation avec {nom}", &[("nom", &name)])
                                title=t("Fermer la conversation") on:click=close>
                                <Icon icon=lu::LuX small=true/>
                            </button>
                        }
                    });
                    view! {
                        <div class="app-dm-item">
                        <a class="k-dm" href=format!("/channels/@me/{id}")
                            data-unread=move || unread.get().then_some("") data-muted=move || muted.get().then_some("")
                            on:contextmenu=u.on_context(Target::Channel(id)) on:keydown=u.on_key(Target::Channel(id))
                            aria-current=move || (ids.get().1 == Some(id) || loc.pathname.get().ends_with(&id.to_string())).then_some("page")>
                            {match (group, first) {
                                (false, Some(u)) => {
                                    let uid = u.id;
                                    let dot = Signal::derive(move || s.dot_of(uid));
                                    view! { <Avatar user=u size="md" status=dot/> }.into_any()
                                }
                                _ => view! { <span class="k-avatar k-avatar--md" aria-hidden="true">{initials(&title)}</span> }.into_any(),
                            }}
                            <div>{title.clone()}<UnreadText on=unread/>{group.then(|| view! { <small>{tn("{n} membre", "{n} membres", count)}</small> })}</div>
                            <Show when=move || calling.get()>
                                <span class="app-dm-call" role="img" aria-label=t("Appel en cours") title=t("Appel en cours")><Icon icon=lu::LuPhoneCall small=true/></span>
                            </Show>
                            <MutedMark on=muted/><MentionBadge count=mentions/>
                        </a>
                        {close_dm}
                        </div>
                        <VoiceUsers guild=None channel=id name=title.clone() hidden=Signal::derive(|| false)/>
                    }
                }
            </For>
        </nav>
        // Les fenêtres sortent de la barre latérale (voir `GuildSidebar`).
        <Show when=move || picking.get()>
            <Portal>
                <crate::social::PeoplePicker purpose=crate::social::PickFor::New(vec![]) close=Callback::new(move |()| picking.set(false))/>
            </Portal>
        </Show>
    }
}

#[component]
fn GuildSidebar(guild: Id) -> impl IntoView {
    let s = store();
    let ids = current_ids();
    let collapsed = RwSignal::new(Vec::<Id>::new());
    let invite_open = RwSignal::new(false);
    let leaving = RwSignal::new(None::<crate::social::Confirm>);
    // Créer un salon : son type, et la catégorie où le ranger.
    let creating = RwSignal::new(None::<(ChannelKind, Option<Id>)>);
    let u = crate::unread::ui();
    // Un mémo, pas une lecture directe : les droits ne redessinent la liste que s'ils changent.
    let can_manage = Memo::new(move |_| s.guild_perms(guild) & kraken_core::perms::MANAGE_CHANNELS != 0);
    // Sans les états vocaux ni les derniers messages : un micro coupé ou un message reçu ne
    // redessine pas toute la liste (ni ne vole le focus).
    let g = Memo::new(move |_| {
        s.guild(guild).map(|mut g| {
            g.voice_states.clear();
            g.channels.iter_mut().for_each(|c| c.last_message_id = None);
            g
        })
    });
    view! {
        <nav class="k-sidebar app-sidebar" aria-label=t("Salons")>
            <crate::switcher::SwitcherButton/>
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
                    let manage = can_manage.get();
                    view! {
                        <div class="app-guild-head">
                            <h2 title=tip>{name}</h2>
                            <button class="k-btn k-btn--icon" aria-label=t("Inviter des amis") title=t("Inviter") on:click=move |_| invite_open.set(true)>
                                <Icon icon=lu::LuUserPlus/>
                            </button>
                            <button class="k-btn k-btn--icon" aria-label=t("Notifications du serveur") title=t("Notifications")
                                on:click=move |_| u.dialog.set(Some(Target::Guild(gid)))>
                                {move || if s.guild_muted(gid) { view! { <Icon icon=lu::LuBellOff/> } } else { view! { <Icon icon=lu::LuBell/> } }}
                            </button>
                            <a class="k-btn k-btn--icon" href=format!("/guilds/{gid}/settings") aria-label=t("Paramètres du serveur") title=t("Paramètres")>
                                <Icon icon=lu::LuSettings/>
                            </a>
                            <crate::ui::MoreMenu label=t("Plus d'actions pour ce serveur")>
                                {
                                    let mine = s.my_id() == Some(g.guild.owner_id);
                                    let (id, label) = (gid, g.guild.name.clone());
                                    view! {
                                        {manage.then(|| view! {
                                            <button class="k-menuitem" on:click=move |e| {
                                                crate::ui::focus_menu_trigger(&e);
                                                creating.set(Some((ChannelKind::Text, None)));
                                            }>
                                                <Icon icon=lu::LuPlus/>{t("Créer un salon")}
                                            </button>
                                            <button class="k-menuitem" on:click=move |e| {
                                                crate::ui::focus_menu_trigger(&e);
                                                creating.set(Some((ChannelKind::Category, None)));
                                            }>
                                                <Icon icon=lu::LuFolderPlus/>{t("Créer une catégorie")}
                                            </button>
                                        })}
                                        {mine.then(|| view! {
                                            <a class="k-menuitem" href=format!("/guilds/{gid}/settings/owner")>
                                                <Icon icon=lu::LuCrown/>{t("Transférer la propriété")}
                                            </a>
                                        })}
                                        <hr/>
                                        <button class="k-menuitem is-danger" on:click=move |_| leaving.set(Some(crate::social::Confirm::LeaveGuild(id, label.clone(), mine)))>
                                            <Icon icon=lu::LuLogOut/>{t("Quitter le serveur")}
                                        </button>
                                    }
                                }
                            </crate::ui::MoreMenu>
                        </div>
                        <a class="k-channel" href=format!("/guilds/{gid}/events")><Icon icon=lu::LuCalendar/>{t("Événements")}</a>
                        {rows.into_iter().map(|c| {
                            let cid = c.id;
                            let name = c.name.clone().unwrap_or_default();
                            if c.kind == ChannelKind::Category {
                                let open = move || !collapsed.with(|v| v.contains(&cid));
                                let title_id = format!("cat-{cid}");
                                view! {
                                    <div class="app-cat">
                                        <button class="k-category" id=title_id.clone() aria-expanded=move || open().to_string()
                                            on:click=move |_| collapsed.update(|v| if v.contains(&cid) { v.retain(|x| *x != cid) } else { v.push(cid) })>
                                            <Icon icon=lu::LuChevronDown small=true/>{name}
                                            {c.private.then(|| view! { <Icon icon=lu::LuLock small=true/><span class="k-sr">" "{t("(privée)")}</span> })}
                                        </button>
                                        {manage.then(|| view! {
                                            <button type="button" class="k-btn k-btn--icon app-row-act" aria-label=t("Créer un salon") aria-describedby=title_id.clone()
                                                title=t("Créer un salon dans cette catégorie") on:click=move |_| creating.set(Some((ChannelKind::Text, Some(cid))))>
                                                <Icon icon=lu::LuPlus small=true/>
                                            </button>
                                            <a class="k-btn k-btn--icon app-row-act" href=format!("/channels/{gid}/{cid}/settings") aria-label=t("Paramètres de la catégorie")
                                                aria-describedby=title_id.clone() title=t("Paramètres de la catégorie")>
                                                <Icon icon=lu::LuSettings small=true/>
                                            </a>
                                        })}
                                    </div>
                                }.into_any()
                            } else {
                                let parent = c.parent_id;
                                let hidden = move || parent.is_some_and(|p| collapsed.with(|v| v.contains(&p)))
                                    && ids.get().1 != Some(cid);
                                let voice = matches!(c.kind, ChannelKind::Voice | ChannelKind::Stage);
                                let private = c.private;
                                // Lu avant le nom du salon par les lecteurs d'écran : « salon vocal général ».
                                let (icon, kind) = match c.kind {
                                    _ if voice => (lu::LuVolume2, if private { t("salon vocal privé") } else { t("salon vocal") }),
                                    ChannelKind::Announcement => (lu::LuMegaphone, if private { t("annonces privées") } else { t("annonces") }),
                                    ChannelKind::Forum => (lu::LuMessagesSquare, if private { t("forum privé") } else { t("forum") }),
                                    _ if private => (lu::LuLock, t("salon privé")),
                                    _ => (lu::LuHash, String::new()),
                                };
                                let kind = if kind.is_empty() { kind } else { format!("{kind} ") };
                                let unread = Memo::new(move |_| s.channel(cid).is_some_and(|c| s.shows_unread(&c)));
                                let muted = Memo::new(move |_| s.channel(cid).is_some_and(|c| s.channel_muted(&c)));
                                let mentions = Memo::new(move |_| s.channel(cid).map_or(0, |c| s.mentions(&c)));
                                let textual = c.kind.is_textual();
                                let name_id = format!("chan-{cid}");
                                view! {
                                    <div class="app-chan" hidden=hidden>
                                        <a class="k-channel" href=format!("/channels/{gid}/{cid}")
                                            data-unread=move || unread.get().then_some("") data-muted=move || muted.get().then_some("")
                                            on:contextmenu=move |e| if textual { u.on_context(Target::Channel(cid))(e) }
                                            on:keydown=move |e| if textual { u.on_key(Target::Channel(cid))(e) }
                                            aria-current=move || (ids.get().1 == Some(cid)).then_some("page")>
                                            <Icon icon=icon/><span class="k-sr">{kind}</span><span class="app-ellipsis" id=name_id.clone()>{name.clone()}</span>
                                            <UnreadText on=unread/><MutedMark on=muted/><MentionBadge count=mentions/>
                                        </a>
                                        {manage.then(|| view! {
                                            <a class="k-btn k-btn--icon app-row-act" href=format!("/channels/{gid}/{cid}/settings") aria-label=t("Paramètres du salon")
                                                aria-describedby=name_id.clone() title=t("Paramètres du salon")>
                                                <Icon icon=lu::LuSettings small=true/>
                                            </a>
                                        })}
                                    </div>
                                    {voice.then(|| view! { <VoiceUsers guild=Some(gid) channel=cid name=name hidden=Signal::derive(hidden)/> })}
                                }.into_any()
                            }
                        }).collect_view()}
                    }.into_any()
                }
            }}
        </nav>
        // Les fenêtres sont montées hors de `.app-nav` : à 390 px, la barre latérale est un tiroir
        // fixe, et tout ce qui est dedans passe sous la barre des serveurs.
        <Show when=move || invite_open.get()>
            <Portal>
                <InviteDialog guild=guild close=move || invite_open.set(false)/>
            </Portal>
        </Show>
        {move || leaving.get().map(|what| view! {
            <Portal>
                <crate::social::ConfirmDialog what=what.clone() close=Callback::new(move |()| leaving.set(None))/>
            </Portal>
        })}
        {move || creating.get().map(|(kind, parent)| view! {
            <Portal>
                <crate::channels::CreateChannelDialog guild=guild kind=kind parent=parent go=true close=Callback::new(move |()| creating.set(None))
                    restore=".app-guild-head .app-more > button"/>
            </Portal>
        })}
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
            link.set(Some(Err(t("Aucun salon où inviter."))));
            return;
        };
        let base = crate::api::public_url().await;
        link.set(Some(
            crate::api::create_invite(c).await.map(|i| format!("{base}/invite/{}", i.code)).map_err(|e| e.message),
        ));
    });
    let copy = move |_| {
        let Some(Ok(l)) = link.get_untracked() else { return };
        spawn_local(async move {
            copied.try_set(crate::ui::copy_text(&l).await);
        });
    };
    let name = s.guild(guild).map(|g| g.guild.name).unwrap_or_default();
    let root = NodeRef::new();
    modal(root, close);
    view! {
        <div class="k-scrim" node_ref=root on:click=move |_| close()>
            <div class="k-dialog" role="dialog" aria-modal="true" aria-labelledby="inv-t" on:click=|e| e.stop_propagation()>
                <header><h2 id="inv-t">{tf("Inviter dans {nom}", &[("nom", &name)])}</h2></header>
                <div>
                    {move || match link.get() {
                        None => view! { <div class="k-skeleton app-skel-line" aria-busy="true"></div> }.into_any(),
                        Some(Err(e)) => view! { <p class="k-error" role="alert">{e}</p> }.into_any(),
                        Some(Ok(l)) => view! {
                            <label class="k-field">
                                <span class="k-label">{t("Lien à partager")}</span>
                                <input dir="ltr" class="k-input" readonly value=l.clone() on:focus=|e| { event_target::<web_sys::HtmlInputElement>(&e).select(); }/>
                            </label>
                            <p class="app-hint">{t("Ce lien n'expire pas. Toute personne qui l'a peut rejoindre.")}</p>
                        }.into_any(),
                    }}
                </div>
                <footer>
                    <button class="k-btn k-btn--ghost" on:click=move |_| close() autofocus>{t("Fermer")}</button>
                    <button class="k-btn k-btn--primary" on:click=copy disabled=move || !matches!(link.get(), Some(Ok(_)))>
                        {move || if copied.get() { t("Copié") } else { t("Copier le lien") }}
                    </button>
                </footer>
            </div>
        </div>
    }
}

/// Les statuts qu'on peut choisir, avec la pastille et ce que chacun change.
/// Textes français à traduire à l'affichage (`tk!` les marque pour `scripts/i18n-check.py`).
const PRESENCES: [(Presence, Dot, &str, &str); 4] = [
    (Presence::Online, Dot::Online, tk!("En ligne"), ""),
    (Presence::Idle, Dot::Idle, tk!("Inactif"), ""),
    (Presence::Dnd, Dot::Dnd, tk!("Ne pas déranger"), tk!("Ni notification du bureau, ni sonnerie d'appel.")),
    (Presence::Invisible, Dot::Invisible, tk!("Invisible"), tk!("Les autres te voient hors ligne.")),
];

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
    let dot = Signal::derive(move || s.my_dot());
    // Choisir son statut : appliqué tout de suite, rétabli si le serveur le refuse.
    let presence_error = RwSignal::new(None::<String>);
    let choose = move |p: Presence| {
        let before = s.presence.get_untracked();
        if before == p {
            return;
        }
        s.presence.set(p);
        presence_error.set(None);
        spawn_local(async move {
            if let Err(e) = crate::api::set_presence(p).await {
                s.presence.try_set(before);
                presence_error.try_set(Some(e.message));
            }
        });
    };
    let state = move || match s.conn.get() {
        Conn::Ready => t("Connecté"),
        Conn::Connecting => t("Connexion…"),
        Conn::Offline => t("Hors ligne"),
    };
    view! {
        <div class="app-me" node_ref=wrap on:focusout=on_focusout>
            <button class="k-me" node_ref=button aria-label=move || tf("Mon compte, {statut}", &[("statut", &dot.get().label())]) title=t("Mon compte")
                aria-expanded=move || open.get().to_string() aria-controls=move || open.get().then_some("me-card")
                on:click=move |_| open.update(|o| *o = !*o)>
                {move || s.me.get().map(|u| view! { <Avatar user=u status=dot/> })}
            </button>
            <Show when=move || open.get()>
                <div class="k-mecard" id="me-card" role="group" aria-label=t("Mon compte") node_ref=card>
                    <header>
                        {move || s.me.get().map(|u| view! { <Avatar user=u size="md" status=dot/> })}
                        <div>
                            <b class="app-ellipsis">{move || s.me.get().map(|u| u.name().to_string()).unwrap_or_default()}</b>
                            <small class="app-ellipsis"><bdi dir="ltr">{move || s.me.get().map(|u| format!("@{}", u.username)).unwrap_or_default()}</bdi></small>
                        </div>
                    </header>
                    <p class="k-mecard-state" data-state=move || (s.conn.get() == Conn::Ready).then_some("on")>{state}</p>
                    <fieldset class="app-presence">
                        <legend class="k-sr">{t("Mon statut")}</legend>
                        {PRESENCES.iter().map(|&(p, shown, label, hint)| view! {
                            <label class="k-mecard-item app-presence-opt">
                                <input type="radio" name="presence" class="k-sr" prop:checked=move || s.presence.get() == p
                                    on:change=move |_| choose(p)/>
                                <span class="app-dot-inline"><StatusDot status=Signal::derive(move || shown)/></span>
                                <span class="app-presence-text"><b>{t(label)}</b><small>{t(hint)}</small></span>
                                <Show when=move || s.presence.get() == p><Icon icon=lu::LuCheck small=true/></Show>
                            </label>
                        }).collect_view()}
                    </fieldset>
                    <Show when=move || presence_error.get().is_some()>
                        <p class="k-error app-presence-error" role="alert">{move || presence_error.get()}</p>
                    </Show>
                    <a class="k-mecard-item" href="/settings/account" aria-label=t("Réglages utilisateur") on:click=picked>
                        <Icon icon=lu::LuUserPen/>{t("Réglages")}
                    </a>
                    <a class="k-mecard-item" href="/settings/appearance" on:click=picked>
                        <Icon icon=lu::LuPalette/>{t("Apparence")}
                    </a>
                    <UpdateEntry me_open=open/>
                    <hr/>
                    <button class="k-mecard-item is-danger" on:click={ let leave = leave.clone(); leave }>
                        <Icon icon=lu::LuLogOut/>{t("Se déconnecter")}
                    </button>
                </div>
            </Show>
        </div>
    }
}
