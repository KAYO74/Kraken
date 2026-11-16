//! Réglages du serveur › Vue d'ensemble : l'icône (choisir, remplacer, retirer) et le nom. Réservé à qui a le
//! droit « Gérer le serveur » et au propriétaire ; le serveur juge de toute façon. Chaque changement est
//! immédiat, annoncé dans une zone `status`, et vu aussitôt par tous les membres (rail, sélecteurs).

use kraken_proto::{Guild, Id};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::icon_picker::{IconFile, IconPicker};
use crate::store::store;
use crate::ui::GuildIcon;

#[component]
pub fn Overview(guild: Id) -> impl IntoView {
    let s = store();
    let current =
        Memo::new(move |_| s.guilds.with(|gs| gs.iter().find(|g| g.guild.id == guild).map(|g| g.guild.clone())));
    let name_now = Memo::new(move |_| current.get().map(|g| g.name).unwrap_or_default());
    let has_icon = Memo::new(move |_| current.get().is_some_and(|g| g.icon.is_some()));

    // ───────── icône ─────────
    let icon_busy = RwSignal::new(false);
    let icon_error = RwSignal::new(None::<String>);
    let icon_note = RwSignal::new(None::<&'static str>);
    let done = move |sent: Result<Guild, kraken_proto::ApiError>| {
        match sent {
            Ok(g) => {
                s.update_guild(g);
                icon_note.try_set(Some("Icône mise à jour."));
            }
            Err(e) => {
                icon_error.try_set(Some(e.message));
            }
        }
        icon_busy.try_set(false);
    };
    let on_pick = Callback::new(move |f: IconFile| {
        icon_busy.set(true);
        icon_error.set(None);
        icon_note.set(None);
        spawn_local(async move { done(crate::api::set_guild_icon(guild, &f.bytes, f.mime).await) });
    });
    let on_remove = Callback::new(move |()| {
        icon_busy.set(true);
        icon_error.set(None);
        icon_note.set(None);
        spawn_local(async move { done(crate::api::remove_guild_icon(guild).await) });
    });

    // ───────── nom ─────────
    let draft = RwSignal::new(name_now.get_untracked());
    let name_busy = RwSignal::new(false);
    let name_error = RwSignal::new(None::<String>);
    let name_note = RwSignal::new(None::<&'static str>);
    let unchanged = Memo::new(move |_| draft.with(|d| d.trim() == name_now.get()));
    let save = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if name_busy.get_untracked() || unchanged.get_untracked() {
            return;
        }
        name_busy.set(true);
        name_error.set(None);
        name_note.set(None);
        let name = draft.get_untracked();
        spawn_local(async move {
            match crate::api::rename_guild(guild, &name).await {
                Ok(g) => {
                    draft.try_set(g.name.clone());
                    s.update_guild(g);
                    name_note.try_set(Some("Nom du serveur enregistré."));
                }
                Err(e) => {
                    name_error.try_set(Some(e.message));
                }
            }
            name_busy.try_set(false);
        });
    };

    view! {
        <div class="app-overview">
            <section aria-labelledby="ov-icon-h" class="app-ov-sec">
                <h3 id="ov-icon-h" class="app-ov-h">"Icône du serveur"</h3>
                <p class="app-hint">"Elle remplace les initiales du serveur dans la liste de gauche, chez tous ses membres."</p>
                <IconPicker id="ov-icon" has_icon=has_icon busy=icon_busy error=icon_error on_pick=on_pick on_remove=on_remove>
                    <span class="app-gicon-preview" role="img"
                        aria-label=move || if has_icon.get() { "Icône actuelle du serveur" } else { "Pas d'icône : les initiales du serveur" }>
                        {move || current.get().map(|g| view! { <GuildIcon guild=g cap=256/> })}
                    </span>
                </IconPicker>
                // Toujours là (une zone `status` doit exister avant son message).
                <p class="k-hint app-ov-note" role="status">{move || icon_note.get()}</p>
            </section>
            <section aria-labelledby="ov-name-h" class="app-ov-sec">
                <h3 id="ov-name-h" class="app-ov-h">"Nom"</h3>
                <form class="app-ov-name" on:submit=save>
                    <label class="k-field">
                        <span class="k-label">"Nom du serveur"</span>
                        <input class="k-input" required minlength="2" maxlength="100" bind:value=draft
                            aria-invalid=move || name_error.get().is_some().then_some("true") aria-describedby="ov-name-err"/>
                    </label>
                    <button class="k-btn k-btn--primary" type="submit" disabled=move || unchanged.get() || name_busy.get()
                        aria-busy=move || name_busy.get().to_string()>"Enregistrer"</button>
                </form>
                <Show when=move || name_error.get().is_some()>
                    <p class="k-error" id="ov-name-err" role="alert">{move || name_error.get()}</p>
                </Show>
                <p class="k-hint app-ov-note" role="status">{move || name_note.get()}</p>
            </section>
        </div>
    }
}
