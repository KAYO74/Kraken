//! Appels REST `/api/v1`. La session voyage dans le cookie `HttpOnly` posé par le serveur.

use gloo_net::http::{Method, RequestBuilder};
use kraken_proto::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

pub type ApiResult<T> = Result<T, ApiError>;

fn network() -> ApiError {
    ApiError {
        code: "network".into(),
        message: "Le serveur ne répond pas. Vérifie ta connexion.".into(),
        timeout_until: None,
    }
}

async fn call<T: DeserializeOwned>(method: Method, path: &str, body: Option<impl Serialize>) -> ApiResult<T> {
    let rb = RequestBuilder::new(&format!("/api/v1{path}")).method(method);
    let req = match body {
        Some(b) => rb.json(&b),
        None => rb.build(),
    }
    .map_err(|_| network())?;
    let resp = req.send().await.map_err(|_| network())?;
    if resp.status() == 204 {
        return serde_json::from_str("null").map_err(|_| network());
    }
    if resp.ok() {
        resp.json().await.map_err(|_| network())
    } else {
        Err(resp.json().await.unwrap_or_else(|_| network()))
    }
}

const NO_BODY: Option<()> = None;

pub async fn instance() -> ApiResult<Instance> {
    call(Method::GET, "/instance", NO_BODY).await
}

thread_local! {
    static MAX_UPLOAD: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static PUBLIC_URL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Adresse publique de l'instance (liens d'invitation) ; à défaut, celle de la page.
pub async fn public_url() -> String {
    if let Some(u) = PUBLIC_URL.with_borrow(Clone::clone) {
        return u;
    }
    let origin = || web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
    match instance().await {
        Ok(i) => {
            let u = i.public_url.trim_end_matches('/');
            let u = if u.is_empty() { origin() } else { u.to_string() };
            PUBLIC_URL.set(Some(u.clone()));
            u
        }
        Err(_) => origin(),
    }
}

/// Taille au plus d'un fichier joint, d'après l'instance (25 Mo si elle ne répond pas).
pub async fn max_upload_bytes() -> u64 {
    if MAX_UPLOAD.get() == 0
        && let Ok(i) = instance().await
        && i.max_upload_bytes > 0
    {
        MAX_UPLOAD.set(i.max_upload_bytes);
    }
    match MAX_UPLOAD.get() {
        0 => DEFAULT_MAX_UPLOAD_BYTES,
        n => n,
    }
}

/// Un envoi en cours, que l'on peut interrompre.
#[derive(Clone)]
pub struct Upload(web_sys::XmlHttpRequest);

impl Upload {
    pub fn abort(&self) {
        let _ = self.0.abort();
    }
}

/// Envoie un fichier dans le salon avec `XMLHttpRequest` (le seul moyen d'avoir la progression
/// de l'envoi). `progress` reçoit une fraction entre 0 et 1.
pub fn upload<F: Fn(f64) + 'static>(
    channel: Id,
    file: &web_sys::File,
    progress: F,
) -> Result<(Upload, impl std::future::Future<Output = ApiResult<Attachment>> + use<F>), ApiError> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let xhr = web_sys::XmlHttpRequest::new().map_err(|_| network())?;
    let name = String::from(js_sys::encode_uri_component(&file.name()));
    xhr.open_with_async("POST", &format!("/api/v1/channels/{channel}/attachments?filename={name}"), true)
        .map_err(|_| network())?;
    if !file.type_().is_empty() {
        let _ = xhr.set_request_header("content-type", &file.type_());
    }
    let (tx, rx) = futures::channel::oneshot::channel::<ApiResult<Attachment>>();
    let tx = Rc::new(RefCell::new(Some(tx)));
    let finish = {
        let tx = tx.clone();
        move |r: ApiResult<Attachment>| {
            if let Some(tx) = tx.borrow_mut().take() {
                let _ = tx.send(r);
            }
        }
    };
    let on_progress = Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(move |e: web_sys::ProgressEvent| {
        if e.length_computable() && e.total() > 0.0 {
            progress((e.loaded() / e.total()).clamp(0.0, 1.0));
        }
    });
    if let Ok(up) = xhr.upload() {
        up.set_onprogress(Some(on_progress.as_ref().unchecked_ref()));
    }
    let on_load = {
        let (xhr, finish) = (xhr.clone(), finish.clone());
        Closure::<dyn FnMut()>::new(move || {
            let text = xhr.response_text().ok().flatten().unwrap_or_default();
            let status = xhr.status().unwrap_or(0);
            finish(if (200..300).contains(&status) {
                serde_json::from_str(&text).map_err(|_| network())
            } else {
                Err(serde_json::from_str(&text).unwrap_or_else(|_| network()))
            });
        })
    };
    let on_fail = {
        let finish = finish.clone();
        Closure::<dyn FnMut()>::new(move || finish(Err(network())))
    };
    xhr.set_onload(Some(on_load.as_ref().unchecked_ref()));
    xhr.set_onerror(Some(on_fail.as_ref().unchecked_ref()));
    xhr.set_onabort(Some(on_fail.as_ref().unchecked_ref()));
    xhr.send_with_opt_blob(Some(file)).map_err(|_| network())?;
    let handle = Upload(xhr);
    Ok((handle, async move {
        // Les fonctions JS doivent vivre jusqu'à la fin de l'envoi.
        let _keep = (on_progress, on_load, on_fail);
        rx.await.unwrap_or_else(|_| Err(network()))
    }))
}

pub async fn register(username: &str, email: &str, password: &str) -> ApiResult<AuthResp> {
    call(Method::POST, "/auth/register", Some(json!({ "username": username, "email": email, "password": password })))
        .await
}

pub async fn login(login: &str, password: &str) -> ApiResult<AuthResp> {
    call(Method::POST, "/auth/login", Some(json!({ "login": login, "password": password }))).await
}

pub async fn logout() -> ApiResult<()> {
    call(Method::POST, "/auth/logout", NO_BODY).await
}

pub async fn create_guild(name: &str) -> ApiResult<GuildState> {
    call(Method::POST, "/guilds", Some(CreateGuild { name: name.into() })).await
}

pub async fn messages(channel: Id) -> ApiResult<Vec<Message>> {
    call(Method::GET, &format!("/channels/{channel}/messages?limit=50"), NO_BODY).await
}

/// La page de messages plus anciens que `before`.
pub async fn messages_before(channel: Id, before: Id) -> ApiResult<Vec<Message>> {
    call(Method::GET, &format!("/channels/{channel}/messages?limit=100&before={before}"), NO_BODY).await
}

pub async fn send_message(
    channel: Id,
    content: &str,
    nonce: &str,
    reply_to: Option<Id>,
    attachments: Vec<AttachmentRef>,
) -> ApiResult<Message> {
    let body = CreateMessage { content: content.into(), nonce: Some(nonce.into()), reply_to, attachments };
    call(Method::POST, &format!("/channels/{channel}/messages"), Some(body)).await
}

/// L'auteur retire l'aperçu d'un lien de son message.
pub async fn remove_embed(channel: Id, id: Id, url: &str) -> ApiResult<Message> {
    let u = String::from(js_sys::encode_uri_component(url));
    call(Method::DELETE, &format!("/channels/{channel}/messages/{id}/embeds?url={u}"), NO_BODY).await
}

/// Retire de la saisie un fichier envoyé mais pas encore joint.
pub async fn delete_attachment(channel: Id, id: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/channels/{channel}/attachments/{id}"), NO_BODY).await
}

/// Adresse de l'original d'un fichier joint (le cookie de session accompagne l'`<img>` et le lien).
pub fn attachment_url(channel: Id, id: Id) -> String {
    format!("/api/v1/channels/{channel}/attachments/{id}")
}

/// Adresse d'une image d'aperçu de lien.
pub fn embed_image_url(channel: Id, key: &str) -> String {
    format!("/api/v1/channels/{channel}/embeds/{key}")
}

pub async fn edit_message(channel: Id, id: Id, content: &str) -> ApiResult<Message> {
    let body = EditMessage { content: content.into() };
    call(Method::PATCH, &format!("/channels/{channel}/messages/{id}"), Some(body)).await
}

pub async fn delete_message(channel: Id, id: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/channels/{channel}/messages/{id}"), NO_BODY).await
}

/// Ajoute (`on`) ou retire ma réaction.
pub async fn react(channel: Id, id: Id, emoji: &str, on: bool) -> ApiResult<()> {
    let e = String::from(js_sys::encode_uri_component(emoji));
    let method = if on { Method::PUT } else { Method::DELETE };
    call(method, &format!("/channels/{channel}/messages/{id}/reactions/{e}/@me"), NO_BODY).await
}

pub async fn create_invite(channel: Id) -> ApiResult<Invite> {
    call(Method::POST, &format!("/channels/{channel}/invites"), Some(CreateInvite::default())).await
}

/// Une invitation avec ses réglages (durée, nombre d'usages).
pub async fn create_invite_with(channel: Id, req: &CreateInvite) -> ApiResult<Invite> {
    call(Method::POST, &format!("/channels/{channel}/invites"), Some(req)).await
}

/// Les invitations du serveur : vivantes, puis mortes depuis peu.
pub async fn guild_invites(guild: Id) -> ApiResult<Vec<GuildInvite>> {
    call(Method::GET, &format!("/guilds/{guild}/invites"), NO_BODY).await
}

pub async fn revoke_invite(code: &str) -> ApiResult<()> {
    call(Method::DELETE, &format!("/invites/{code}"), NO_BODY).await
}

pub async fn invite(code: &str) -> ApiResult<Invite> {
    call(Method::GET, &format!("/invites/{code}"), NO_BODY).await
}

pub async fn join(code: &str) -> ApiResult<GuildState> {
    call(Method::POST, &format!("/invites/{code}"), NO_BODY).await
}

pub async fn voice_join(channel: Id, body: &VoiceJoin) -> ApiResult<VoiceAnswer> {
    call(Method::POST, &format!("/channels/{channel}/voice"), Some(body)).await
}

pub async fn voice_leave() -> ApiResult<()> {
    call(Method::DELETE, "/voice", NO_BODY).await
}

/// Ne plus faire sonner cet appel chez moi.
pub async fn call_decline(channel: Id) -> ApiResult<()> {
    call(Method::POST, &format!("/channels/{channel}/call/decline"), NO_BODY).await
}

pub async fn update_me(display_name: &str) -> ApiResult<User> {
    call(Method::PATCH, "/users/@me", Some(UpdateMe { display_name: Some(display_name.into()) })).await
}

/// Envoie la photo de profil (déjà recadrée) : le corps de la requête est l'image elle-même.
pub async fn set_avatar(bytes: &[u8], mime: &str) -> ApiResult<User> {
    let body = js_sys::Uint8Array::from(bytes);
    let req = RequestBuilder::new("/api/v1/users/@me/avatar")
        .method(Method::PUT)
        .header("content-type", mime)
        .body(body)
        .map_err(|_| network())?;
    let resp = req.send().await.map_err(|_| network())?;
    if resp.ok() {
        resp.json().await.map_err(|_| network())
    } else if resp.status() == 413 {
        Err(resp.json().await.unwrap_or_else(|_| too_large()))
    } else {
        Err(resp.json().await.unwrap_or_else(|_| network()))
    }
}

fn too_large() -> ApiError {
    let mo = MAX_AVATAR_BYTES / (1024 * 1024);
    ApiError {
        code: "too_large".into(),
        message: format!("Cette image pèse trop lourd : {mo} Mo au plus."),
        timeout_until: None,
    }
}

/// Retirer la photo de profil.
pub async fn remove_avatar() -> ApiResult<User> {
    call(Method::DELETE, "/users/@me/avatar", NO_BODY).await
}

pub async fn logout_all() -> ApiResult<()> {
    call(Method::POST, "/auth/logout-all", NO_BODY).await
}

pub async fn delete_account(password: &str) -> ApiResult<()> {
    call(Method::POST, "/users/@me/delete", Some(json!({ "password": password }))).await
}

pub async fn forgot_password(email: &str) -> ApiResult<()> {
    call(Method::POST, "/auth/reset", Some(json!({ "email": email }))).await
}

pub async fn reset_password(token: &str, password: &str) -> ApiResult<()> {
    call(Method::POST, "/auth/reset/confirm", Some(json!({ "token": token, "password": password }))).await
}

pub async fn verify_email(token: &str) -> ApiResult<()> {
    call(Method::POST, "/auth/verify-email", Some(json!({ "token": token }))).await
}

// ───────── social ─────────

/// Demande d'ami par pseudo unique (ou acceptation de la sienne).
pub async fn send_friend_request(username: &str) -> ApiResult<Relationship> {
    call(Method::POST, "/users/@me/relationships", Some(FriendRequest { username: username.into() })).await
}

/// Accepter une demande (`Friend`) ou bloquer (`Blocked`).
pub async fn set_relationship(user: Id, kind: RelationshipKind) -> ApiResult<Relationship> {
    call(Method::PUT, &format!("/users/@me/relationships/{user}"), Some(SetRelationship { kind })).await
}

/// Retirer un ami, refuser ou annuler une demande, débloquer.
pub async fn remove_relationship(user: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/users/@me/relationships/{user}"), NO_BODY).await
}

/// Le MP avec une personne, ou un nouveau groupe avec plusieurs amis.
pub async fn open_private(recipients: Vec<Id>, name: Option<String>) -> ApiResult<Channel> {
    call(Method::POST, "/users/@me/channels", Some(OpenPrivate { recipients, name })).await
}

pub async fn rename_group(channel: Id, name: &str) -> ApiResult<Channel> {
    call(
        Method::PATCH,
        &format!("/channels/{channel}"),
        Some(EditChannel { name: Some(name.into()), ..Default::default() }),
    )
    .await
}

pub async fn add_recipient(channel: Id, user: Id) -> ApiResult<()> {
    call(Method::PUT, &format!("/channels/{channel}/recipients/{user}"), NO_BODY).await
}

/// Fermer un MP : il quitte ma liste, et y revient avec le prochain message.
pub async fn close_dm(channel: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/channels/{channel}"), NO_BODY).await
}

/// Retirer quelqu'un d'un groupe (propriétaire seulement).
pub async fn remove_member(channel: Id, user: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/channels/{channel}/recipients/{user}"), NO_BODY).await
}

pub async fn leave_group(channel: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/channels/{channel}/recipients/@me"), NO_BODY).await
}

// ───────── salons d'un serveur ─────────

pub async fn create_channel(guild: Id, req: &CreateChannel) -> ApiResult<Channel> {
    call(Method::POST, &format!("/guilds/{guild}/channels"), Some(req)).await
}

pub async fn edit_channel(channel: Id, req: &EditChannel) -> ApiResult<Channel> {
    call(Method::PATCH, &format!("/channels/{channel}"), Some(req)).await
}

/// Supprimer un salon ou une catégorie, avec ses messages.
pub async fn delete_channel(channel: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/channels/{channel}"), NO_BODY).await
}

/// Rend les salons dont la place a changé.
pub async fn move_channel(channel: Id, req: &MoveChannel) -> ApiResult<Vec<Channel>> {
    call(Method::PUT, &format!("/channels/{channel}/position"), Some(req)).await
}

pub async fn overwrites(channel: Id) -> ApiResult<Vec<PermissionOverwrite>> {
    call(Method::GET, &format!("/channels/{channel}/permissions"), NO_BODY).await
}

/// Pose la surcharge d'un rôle ou d'un membre ; rend toutes celles du salon.
pub async fn set_overwrite(channel: Id, target: Id, req: &SetOverwrite) -> ApiResult<Vec<PermissionOverwrite>> {
    call(Method::PUT, &format!("/channels/{channel}/permissions/{target}"), Some(req)).await
}

// ───────── non-lus et notifications ─────────

/// J'ai lu ce salon jusqu'à ce message.
pub async fn ack(channel: Id, message: Id) -> ApiResult<ReadState> {
    call(Method::POST, &format!("/channels/{channel}/ack/{message}"), NO_BODY).await
}

/// Tout le serveur lu.
pub async fn ack_guild(guild: Id) -> ApiResult<Vec<ReadState>> {
    call(Method::POST, &format!("/guilds/{guild}/ack"), NO_BODY).await
}

pub async fn set_notify(req: &SetNotify) -> ApiResult<NotifySetting> {
    call(Method::PUT, "/users/@me/notifications", Some(req)).await
}

// ───────── rôles, modération, départ ─────────

pub async fn create_role(guild: Id, req: &CreateRole) -> ApiResult<Role> {
    call(Method::POST, &format!("/guilds/{guild}/roles"), Some(req)).await
}

pub async fn edit_role(guild: Id, role: Id, req: &EditRole) -> ApiResult<Role> {
    call(Method::PATCH, &format!("/guilds/{guild}/roles/{role}"), Some(req)).await
}

pub async fn delete_role(guild: Id, role: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/guilds/{guild}/roles/{role}"), NO_BODY).await
}

/// Rend les rôles dont la position a changé.
pub async fn reorder_roles(guild: Id, ids: Vec<Id>) -> ApiResult<Vec<Role>> {
    call(Method::PUT, &format!("/guilds/{guild}/roles"), Some(ReorderRoles { ids })).await
}

pub async fn set_member_role(guild: Id, user: Id, role: Id, on: bool) -> ApiResult<Member> {
    let method = if on { Method::PUT } else { Method::DELETE };
    call(method, &format!("/guilds/{guild}/members/{user}/roles/{role}"), NO_BODY).await
}

pub async fn kick(guild: Id, user: Id, reason: &str) -> ApiResult<()> {
    let reason = String::from(js_sys::encode_uri_component(reason));
    call(Method::DELETE, &format!("/guilds/{guild}/members/{user}?reason={reason}"), NO_BODY).await
}

pub async fn ban(guild: Id, user: Id, req: &BanMember) -> ApiResult<Ban> {
    call(Method::PUT, &format!("/guilds/{guild}/bans/{user}"), Some(req)).await
}

pub async fn unban(guild: Id, user: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/guilds/{guild}/bans/{user}"), NO_BODY).await
}

pub async fn bans(guild: Id) -> ApiResult<Vec<Ban>> {
    call(Method::GET, &format!("/guilds/{guild}/bans"), NO_BODY).await
}

pub async fn leave_guild(guild: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/guilds/{guild}/members/@me"), NO_BODY).await
}

/// Supprimer le serveur (son propriétaire, qui retape le nom).
pub async fn delete_guild(guild: Id, name: &str) -> ApiResult<()> {
    call(Method::DELETE, &format!("/guilds/{guild}"), Some(DeleteGuild { name: name.to_string() })).await
}

/// Exclure temporairement un membre.
pub async fn timeout_member(guild: Id, user: Id, req: &TimeoutMember) -> ApiResult<Member> {
    call(Method::PUT, &format!("/guilds/{guild}/members/{user}/timeout"), Some(req)).await
}

pub async fn lift_timeout(guild: Id, user: Id) -> ApiResult<Member> {
    call(Method::DELETE, &format!("/guilds/{guild}/members/{user}/timeout"), NO_BODY).await
}

pub async fn transfer_guild(guild: Id, to: Id) -> ApiResult<()> {
    call(Method::POST, &format!("/guilds/{guild}/owner"), Some(TransferOwner { user_id: to })).await
}
