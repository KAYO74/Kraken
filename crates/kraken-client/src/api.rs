//! Appels REST `/api/v1`. La session voyage dans le cookie `HttpOnly` posé par le serveur.

use gloo_net::http::{Method, RequestBuilder};
use kraken_proto::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

pub type ApiResult<T> = Result<T, ApiError>;

fn network() -> ApiError {
    ApiError { code: "network".into(), message: "Le serveur ne répond pas. Vérifie ta connexion.".into() }
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

pub async fn send_message(channel: Id, content: &str, nonce: &str, reply_to: Option<Id>) -> ApiResult<Message> {
    let body = CreateMessage { content: content.into(), nonce: Some(nonce.into()), reply_to };
    call(Method::POST, &format!("/channels/{channel}/messages"), Some(body)).await
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
    call(Method::PATCH, &format!("/channels/{channel}"), Some(EditChannel { name: Some(name.into()) })).await
}

pub async fn add_recipient(channel: Id, user: Id) -> ApiResult<()> {
    call(Method::PUT, &format!("/channels/{channel}/recipients/{user}"), NO_BODY).await
}

pub async fn leave_group(channel: Id) -> ApiResult<()> {
    call(Method::DELETE, &format!("/channels/{channel}/recipients/@me"), NO_BODY).await
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
