//! Comptes et sessions. Mots de passe en argon2id ; jetons de session opaques dont seul le
//! SHA-256 est en base. Le client web reçoit le jeton dans un cookie `HttpOnly` ; les clients
//! natifs et les bots l'envoient dans `Authorization: Session <jeton>`.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::Json;
use axum::extract::{FromRef, FromRequestParts, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts};
use axum::response::{AppendHeaders, IntoResponse};
use base64::Engine;
use kraken_proto::*;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use tokio::sync::Semaphore;

use crate::AppState;
use crate::error::{ApiResult, AppError};

const COOKIE: &str = "kraken_session";

/// Argon2 prend ~19 Mio et un cœur par appel : au plus deux appels par cœur à la fois.
static ARGON2: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(std::thread::available_parallelism().map_or(4, |n| n.get() * 2)));

pub async fn hash_password(pw: String) -> Option<String> {
    let _permit = ARGON2.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let mut bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut bytes);
        let salt = SaltString::encode_b64(&bytes).ok()?;
        Argon2::default().hash_password(pw.as_bytes(), &salt).ok().map(|h| h.to_string())
    })
    .await
    .ok()
    .flatten()
}

async fn verify_password(pw: String, hash: String) -> bool {
    let Ok(_permit) = ARGON2.acquire().await else { return false };
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
    })
    .await
    .unwrap_or(false)
}

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Jeton aléatoire de 256 bits, encodé pour les URL.
pub fn new_token() -> String {
    let mut raw = [0u8; 32];
    rand::rng().fill_bytes(&mut raw);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

fn cookie(state: &AppState, token: &str, max_age: u32) -> HeaderValue {
    let secure = if state.cfg.secure_cookies() { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"))
        .expect("ascii")
}

fn clear_cookie(state: &AppState) -> AppendHeaders<[(header::HeaderName, HeaderValue); 1]> {
    AppendHeaders([(header::SET_COOKIE, cookie(state, "", 0))])
}

/// Jeton présenté par la requête : en-tête `Authorization`, sinon cookie (le booléen dit
/// lequel).
pub fn presented_token(headers: &HeaderMap) -> Option<(String, bool)> {
    if let Some(t) =
        headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Session "))
    {
        return Some((t.to_string(), false));
    }
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(&format!("{COOKIE}=")).map(str::to_string))
        .filter(|t| !t.is_empty())
        .map(|t| (t, true))
}

/// Une requête authentifiée par cookie doit venir de notre propre origine (anti-CSRF et
/// anti-détournement de WebSocket) : l'adresse publique de l'instance, ou l'hôte demandé.
pub fn same_origin(headers: &HeaderMap, public_url: &str) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return false };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    origin == public_url.trim_end_matches('/') || origin.split_once("://").is_some_and(|(_, rest)| Some(rest) == host)
}

fn platform(headers: &HeaderMap) -> (String, &'static str) {
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("inconnu");
    (ua.chars().take(120).collect(), if ua.contains("Mozilla") { "web" } else { "api" })
}

async fn open_session(state: &AppState, headers: &HeaderMap, user: User) -> ApiResult<impl IntoResponse + use<>> {
    let token = new_token();
    let (name, platform) = platform(headers);
    state.db.create_session(user.id, &token_hash(&token), &name, platform).await?;
    let set = cookie(state, &token, 60 * 60 * 24 * 365);
    Ok((AppendHeaders([(header::SET_COOKIE, set)]), Json(AuthResp { token, user })))
}

fn check_password(pw: &str) -> ApiResult<()> {
    let n = pw.chars().count();
    if !(8..=256).contains(&n) {
        return Err(AppError::invalid("Mot de passe : 8 caractères au minimum."));
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterReq>,
) -> ApiResult<impl IntoResponse> {
    let username = req.username.trim().to_lowercase();
    if !kraken_db::valid_username(&username) {
        return Err(AppError::invalid("Pseudo : 2 à 32 caractères parmi a-z, 0-9, _ et ."));
    }
    let email = req.email.trim();
    if !email.contains('@') || email.len() > 254 || email.chars().any(char::is_whitespace) {
        return Err(AppError::invalid("Adresse e-mail invalide."));
    }
    check_password(&req.password)?;
    let hash = hash_password(req.password).await.ok_or_else(|| AppError::invalid("Mot de passe refusé."))?;
    let user = state.db.create_user(&username, email, hash).await?;
    if state.mailer.is_some() {
        state.db.enqueue("email_verify", serde_json::json!({ "user_id": user.id }), None, 0).await?;
    }
    Ok((StatusCode::CREATED, open_session(&state, &headers, user).await?))
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> ApiResult<impl IntoResponse> {
    // Même réponse pour un compte inconnu et un mauvais mot de passe.
    let bad = || AppError(StatusCode::UNAUTHORIZED, "bad_credentials", "Identifiant ou mot de passe incorrect.".into());
    let Some((user, hash)) = state.db.user_credentials(&req.login).await? else {
        // Coût comparable à une vraie vérification, pour ne pas révéler les comptes existants.
        let _ = hash_password(req.password).await;
        return Err(bad());
    };
    if !verify_password(req.password, hash).await {
        return Err(bad());
    }
    open_session(&state, &headers, user).await
}

pub async fn logout(State(state): State<AppState>, auth: Auth) -> ApiResult<impl IntoResponse> {
    state.db.revoke_session(&auth.token_hash).await?;
    state.hub.kick_session(auth.user.id, &auth.token_hash);
    Ok((StatusCode::NO_CONTENT, clear_cookie(&state)))
}

/// Ferme toutes les sessions de ce compte, sur tous les appareils.
pub async fn logout_all(State(state): State<AppState>, auth: Auth) -> ApiResult<impl IntoResponse> {
    state.db.revoke_all_sessions(auth.user.id).await?;
    state.hub.kick(auth.user.id);
    Ok((StatusCode::NO_CONTENT, clear_cookie(&state)))
}

pub async fn me(auth: Auth) -> Json<User> {
    Json(auth.user)
}

/// Changer son nom affiché. Le pseudo unique ne change pas ; il reste visible sous le nom.
pub async fn update_me(State(state): State<AppState>, auth: Auth, Json(req): Json<UpdateMe>) -> ApiResult<Json<User>> {
    let name = req.display_name.as_deref().map(str::trim).filter(|n| !n.is_empty());
    if let Some(n) = name
        && (n.chars().count() > MAX_DISPLAY_NAME || n.chars().any(char::is_control))
    {
        return Err(AppError::invalid(format!(
            "Nom affiché : {MAX_DISPLAY_NAME} caractères au plus, sans retour à la ligne."
        )));
    }
    let user = state.db.set_display_name(auth.user.id, name).await?;
    let mut audience = state.db.related_users(user.id).await?;
    audience.push(user.id);
    audience.sort_unstable();
    audience.dedup();
    state.hub.send(&audience, Event::UserUpdate(user.clone()));
    Ok(Json(user))
}

pub async fn verify_email(State(state): State<AppState>, Json(req): Json<TokenReq>) -> ApiResult<StatusCode> {
    let uid = state
        .db
        .use_auth_token("verify_email", &token_hash(&req.token))
        .await?
        .ok_or_else(|| AppError::invalid("Lien invalide ou expiré."))?;
    state.db.mark_email_verified(uid).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Mot de passe oublié. Répond pareil que le compte existe ou non.
pub async fn reset(State(state): State<AppState>, Json(req): Json<EmailReq>) -> ApiResult<StatusCode> {
    if state.mailer.is_none() {
        return Err(AppError(
            StatusCode::NOT_IMPLEMENTED,
            "no_email",
            "Cette instance n'envoie pas d'e-mails. Demande à son administrateur.".into(),
        ));
    }
    if let Some((uid, _)) = state.db.user_by_email(&req.email).await? {
        let key = format!("reset:{uid}");
        state.db.enqueue("email_reset", serde_json::json!({ "user_id": uid }), Some(&key), 0).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reset_confirm(State(state): State<AppState>, Json(req): Json<ResetConfirmReq>) -> ApiResult<StatusCode> {
    check_password(&req.password)?;
    let uid = state
        .db
        .use_auth_token("reset_password", &token_hash(&req.token))
        .await?
        .ok_or_else(|| AppError::invalid("Lien invalide ou expiré. Refais une demande."))?;
    let hash = hash_password(req.password).await.ok_or_else(|| AppError::invalid("Mot de passe refusé."))?;
    state.db.set_password(uid, &hash).await?;
    state.db.revoke_all_sessions(uid).await?;
    state.hub.kick(uid);
    Ok(StatusCode::NO_CONTENT)
}

/// Suppression du compte, confirmée par le mot de passe. Effet immédiat pour l'utilisateur,
/// anonymisation définitive après le délai de grâce.
pub async fn delete_account(
    State(state): State<AppState>,
    auth: Auth,
    Json(req): Json<PasswordReq>,
) -> ApiResult<impl IntoResponse> {
    let hash = state.db.password_hash(auth.user.id).await?.unwrap_or_default();
    if !verify_password(req.password, hash).await {
        return Err(AppError(StatusCode::FORBIDDEN, "bad_password", "Mot de passe incorrect.".into()));
    }
    state.db.request_deletion(auth.user.id).await?;
    state.hub.kick(auth.user.id);
    Ok((StatusCode::ACCEPTED, clear_cookie(&state)))
}

/// Utilisateur authentifié.
pub struct Auth {
    pub user: User,
    token_hash: Vec<u8>,
}

impl<S: Send + Sync> FromRequestParts<S> for Auth
where
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, s: &S) -> Result<Self, AppError> {
        let state = AppState::from_ref(s);
        let (token, from_cookie) = presented_token(&parts.headers).ok_or_else(AppError::unauthorized)?;
        let safe = matches!(parts.method, Method::GET | Method::HEAD);
        if from_cookie && !safe && !same_origin(&parts.headers, &state.cfg.public_url) {
            return Err(AppError::forbidden());
        }
        let token_hash = token_hash(&token);
        let user = state.db.session_user(&token_hash).await?.ok_or_else(AppError::unauthorized)?;
        Ok(Self { user, token_hash })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_is_the_public_url_or_the_host() {
        let h = |origin: &str, host: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::ORIGIN, origin.parse().unwrap());
            h.insert(header::HOST, host.parse().unwrap());
            h
        };
        let public = "https://chat.example.org/";
        // Derrière un proxy qui réécrit Host.
        assert!(same_origin(&h("https://chat.example.org", "127.0.0.1:8080"), public));
        assert!(same_origin(&h("http://127.0.0.1:8080", "127.0.0.1:8080"), public));
        assert!(!same_origin(&h("https://evil.example", "127.0.0.1:8080"), public));
        assert!(!same_origin(&HeaderMap::new(), public));
    }
}
