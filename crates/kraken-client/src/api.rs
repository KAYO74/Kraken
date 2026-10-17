//! Appels REST `/api/v1`. Le jeton de session est lu dans le stockage local à chaque appel.

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
    let mut rb = RequestBuilder::new(&format!("/api/v1{path}")).method(method);
    if let Some(t) = crate::store::saved_token() {
        rb = rb.header("Authorization", &format!("Session {t}"));
    }
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

pub async fn send_message(channel: Id, content: &str, nonce: &str) -> ApiResult<Message> {
    let body = CreateMessage { content: content.into(), nonce: Some(nonce.into()) };
    call(Method::POST, &format!("/channels/{channel}/messages"), Some(body)).await
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
