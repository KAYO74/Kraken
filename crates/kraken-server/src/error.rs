use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kraken_db::DbError;
use kraken_proto::{ApiError, MAX_GROUP_MEMBERS, MAX_GROUP_NAME};

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
            DbError::Conflict("already_friends") => {
                Self(StatusCode::CONFLICT, "already_friends", "Vous êtes déjà amis.".into())
            }
            DbError::Conflict("already_sent") => Self(
                StatusCode::CONFLICT,
                "already_sent",
                "Ta demande est déjà partie : elle attend sa réponse dans « En attente ».".into(),
            ),
            DbError::Conflict(f) => Self(StatusCode::CONFLICT, "conflict", format!("Déjà utilisé : {f}.")),
            DbError::Invalid("reactions") => Self::invalid("20 emojis différents au plus sur un message."),
            DbError::Invalid("emoji") => Self::invalid("Seuls les emojis servent de réaction."),
            DbError::Invalid("reply_to") => Self::invalid("Le message d'origine n'est plus dans ce salon."),
            DbError::Invalid("self") => Self(StatusCode::BAD_REQUEST, "self", "C'est ton propre pseudo.".into()),
            DbError::Invalid("recipients") => {
                Self::invalid(format!("Un groupe réunit de 3 à {MAX_GROUP_MEMBERS} personnes, toi compris."))
            }
            DbError::Invalid("group_full") => {
                Self::invalid(format!("Ce groupe est complet : {MAX_GROUP_MEMBERS} personnes au plus."))
            }
            DbError::Invalid("name") => Self::invalid(format!("Le nom tient en {MAX_GROUP_NAME} caractères au plus.")),
            DbError::Invalid(f) => Self::invalid(format!("Valeur refusée : {f}.")),
            DbError::Forbidden("banned") => {
                Self(StatusCode::FORBIDDEN, "banned", "Tu as été banni de ce serveur.".into())
            }
            DbError::Forbidden("blocked") => Self(
                StatusCode::FORBIDDEN,
                "blocked",
                "Tu as bloqué ce compte. Débloque-le d'abord, dans l'onglet « Bloqués ».".into(),
            ),
            DbError::Forbidden("request_refused") => Self(
                StatusCode::FORBIDDEN,
                "request_refused",
                "Cette personne n'accepte pas de demande d'ami de ta part.".into(),
            ),
            DbError::Forbidden("dm_blocked") => Self(
                StatusCode::FORBIDDEN,
                "dm_blocked",
                "Tu as bloqué cette personne : débloque-la pour lui écrire.".into(),
            ),
            DbError::Forbidden("dm_refused") => Self(
                StatusCode::FORBIDDEN,
                "dm_refused",
                "Ton message n'a pas pu être remis : cette personne ne reçoit pas de message de ta part.".into(),
            ),
            DbError::Forbidden("dm_stranger") => Self(
                StatusCode::FORBIDDEN,
                "dm_stranger",
                "Pour écrire à quelqu'un, soyez amis ou membres d'un même serveur.".into(),
            ),
            DbError::Forbidden("not_friend") => Self(
                StatusCode::FORBIDDEN,
                "not_friend",
                "Seuls tes amis peuvent entrer dans un groupe que tu crées ou complètes.".into(),
            ),
            DbError::Forbidden("owner") => Self(
                StatusCode::FORBIDDEN,
                "forbidden",
                "Seul le propriétaire du groupe peut en retirer quelqu'un.".into(),
            ),
            DbError::Forbidden("call_blocked") => Self(
                StatusCode::FORBIDDEN,
                "call_blocked",
                "Tu as bloqué cette personne : débloque-la pour l'appeler.".into(),
            ),
            DbError::Forbidden("call_refused") => Self(
                StatusCode::FORBIDDEN,
                "call_refused",
                "Impossible d'appeler cette personne : elle ne reçoit pas d'appel de ta part.".into(),
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
