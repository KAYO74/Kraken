//! Comptes et sessions : argon2id, jetons opaques dont seul le hash SHA-256 est gardé.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::Json;
use axum::extract::{FromRef, FromRequestParts, State};
use axum::http::{StatusCode, header, request::Parts};
use base64::Engine;
use kraken_proto::{AuthResp, LoginReq, RegisterReq, User};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::error::{ApiResult, AppError};

pub async fn hash_password(pw: String) -> Option<String> {
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
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
    })
    .await
    .unwrap_or(false)
}

pub fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

async fn open_session(state: &AppState, user: User) -> AuthResp {
    let mut raw = [0u8; 32];
    rand::rng().fill_bytes(&mut raw);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    state.db.create_session(user.id, token_hash(&token)).await;
    AuthResp { token, user }
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> ApiResult<(StatusCode, Json<AuthResp>)> {
    let username = req.username.trim().to_lowercase();
    if !kraken_db::valid_username(&username) {
        return Err(AppError::invalid("Pseudo : 2 à 32 caractères parmi a-z, 0-9, _ et ."));
    }
    let email = req.email.trim();
    if !email.contains('@') || email.len() > 254 {
        return Err(AppError::invalid("Adresse e-mail invalide."));
    }
    if req.password.chars().count() < 8 {
        return Err(AppError::invalid("Mot de passe : 8 caractères au minimum."));
    }
    let hash = hash_password(req.password).await.ok_or_else(|| AppError::invalid("Mot de passe refusé."))?;
    let user = state.db.create_user(&username, email, hash).await?;
    Ok((StatusCode::CREATED, Json(open_session(&state, user).await)))
}

pub async fn login(State(state): State<AppState>, Json(req): Json<LoginReq>) -> ApiResult<Json<AuthResp>> {
    // Même réponse pour un compte inconnu et un mauvais mot de passe.
    let bad = || AppError(StatusCode::UNAUTHORIZED, "bad_credentials", "Identifiant ou mot de passe incorrect.".into());
    let (user, hash) = state.db.user_credentials(&req.login).await.ok_or_else(bad)?;
    if !verify_password(req.password, hash).await {
        return Err(bad());
    }
    Ok(Json(open_session(&state, user).await))
}

pub async fn logout(State(state): State<AppState>, auth: Auth) -> StatusCode {
    state.db.delete_session(&auth.token_hash).await;
    StatusCode::NO_CONTENT
}

pub async fn me(auth: Auth) -> Json<User> {
    Json(auth.user)
}

/// Utilisateur authentifié par `Authorization: Session <jeton>`.
pub struct Auth {
    pub user: User,
    token_hash: String,
}

impl<S: Send + Sync> FromRequestParts<S> for Auth
where
    AppState: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, s: &S) -> Result<Self, AppError> {
        let state = AppState::from_ref(s);
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Session "))
            .ok_or_else(AppError::unauthorized)?;
        let token_hash = token_hash(token);
        let user = state.db.session_user(&token_hash).await.ok_or_else(AppError::unauthorized)?;
        Ok(Self { user, token_hash })
    }
}
