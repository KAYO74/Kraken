use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kraken_db::DbError;
use kraken_proto::ApiError;

/// Erreur d'API : un code stable que le client traduit, et un message de secours.
#[derive(Debug)]
pub struct AppError(pub StatusCode, pub &'static str, pub String);

pub type ApiResult<T> = Result<T, AppError>;

impl AppError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid", msg.into())
    }
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "unauthorized", "Session absente ou expirée.".into())
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "forbidden", "Tu n'as pas la permission de faire ça.".into())
    }
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "not_found", "Introuvable.".into())
    }
}

impl From<DbError> for AppError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::NotFound => Self::not_found(),
            DbError::Conflict("username") => Self(StatusCode::CONFLICT, "conflict", "Ce pseudo est déjà pris.".into()),
            DbError::Conflict("email") => {
                Self(StatusCode::CONFLICT, "conflict", "Un compte existe déjà avec cette adresse.".into())
            }
            DbError::Conflict(f) => Self(StatusCode::CONFLICT, "conflict", format!("Déjà utilisé : {f}.")),
            DbError::Invalid(f) => Self::invalid(format!("Valeur refusée : {f}.")),
            DbError::Forbidden("banned") => {
                Self(StatusCode::FORBIDDEN, "banned", "Tu as été banni de ce serveur.".into())
            }
            DbError::Forbidden("owner") => Self(
                StatusCode::CONFLICT,
                "owns_guilds",
                "Transfère ou supprime d'abord les serveurs dont tu es propriétaire.".into(),
            ),
            DbError::Forbidden(_) => Self::forbidden(),
            DbError::Internal(e) => {
                tracing::error!("base : {e}");
                Self(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Erreur du serveur, réessaie.".into())
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(ApiError { code: self.1.into(), message: self.2 })).into_response()
    }
}
