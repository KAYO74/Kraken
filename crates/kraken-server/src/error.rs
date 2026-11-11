use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use kraken_db::DbError;
use kraken_proto::{
    ApiError, MAX_CHANNEL_NAME, MAX_CHANNELS, MAX_GROUP_MEMBERS, MAX_GROUP_NAME, MAX_OVERWRITES, MAX_PURGE_HOURS,
    MAX_REASON, MAX_ROLES, MAX_TOPIC,
};

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
            DbError::Conflict("last_text") => Self(
                StatusCode::CONFLICT,
                "last_text",
                "C'est le dernier salon d'écriture du serveur : crée-en un autre avant de le supprimer.".into(),
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
            DbError::Invalid("guild_name") => Self::invalid("Le nom d'un serveur compte de 2 à 100 caractères."),
            DbError::Invalid("channel_name") => Self::invalid(format!(
                "Le nom d'un salon compte de 1 à {MAX_CHANNEL_NAME} caractères, sans caractère de contrôle."
            )),
            DbError::Invalid("channels") => {
                Self::invalid(format!("Un serveur compte {MAX_CHANNELS} salons et catégories au plus."))
            }
            DbError::Invalid("topic") => Self::invalid(format!(
                "Le sujet tient en {MAX_TOPIC} caractères au plus, et seuls les salons d'écriture en ont un."
            )),
            DbError::Invalid("slowmode") => Self::invalid(
                "Mode lent : aucun, 5 s, 10 s, 30 s, 1 min, 5 min, 15 min ou 1 h, et seulement dans un salon d'écriture.",
            ),
            DbError::Invalid("parent_id") => Self::invalid("Cette catégorie n'existe pas ou n'est pas visible."),
            DbError::Invalid("overwrite") => Self::invalid("Un droit ne peut pas être à la fois autorisé et refusé."),
            DbError::Invalid("overwrites") => {
                Self::invalid(format!("{MAX_OVERWRITES} réglages de droits au plus par salon."))
            }
            DbError::Invalid("name") => Self::invalid(format!("Le nom tient en {MAX_GROUP_NAME} caractères au plus.")),
            DbError::Invalid("mute") => Self::invalid("Une sourdine dure de 1 minute à 30 jours, ou sans fin."),
            DbError::Invalid("target") => Self::invalid("Un serveur ou un salon, l'un des deux."),
            DbError::Invalid("self_target") => Self::invalid("Tu ne peux pas faire ça sur toi-même."),
            DbError::Invalid("role_name") => Self::invalid("Le nom d'un rôle compte de 1 à 100 caractères."),
            DbError::Invalid("color") => Self::invalid("Cette couleur n'existe pas dans la palette."),
            DbError::Invalid("reason") => Self::invalid(format!("La raison tient en {MAX_REASON} caractères au plus.")),
            DbError::Invalid("hours") => {
                Self::invalid(format!("On peut effacer les messages de {MAX_PURGE_HOURS} heures au plus."))
            }
            DbError::Invalid("roles") => {
                Self::invalid(format!("{MAX_ROLES} rôles au plus, et l'ordre doit les citer tous."))
            }
            DbError::Invalid("permissions") => Self::invalid("Droit inconnu."),
            DbError::Invalid("everyone") => Self::invalid("@everyone ne peut être ni renommé, ni supprimé, ni donné."),
            DbError::Invalid(f) => Self::invalid(format!("Valeur refusée : {f}.")),
            DbError::Forbidden("hierarchy") => Self(
                StatusCode::FORBIDDEN,
                "hierarchy",
                "Son rang est égal ou supérieur au tien (le propriétaire est intouchable).".into(),
            ),
            DbError::Forbidden("owner_leave") => Self(
                StatusCode::FORBIDDEN,
                "owner_leave",
                "Tu possèdes ce serveur : transfère-le à un autre membre avant de le quitter.".into(),
            ),
            DbError::Forbidden("owner_only") => {
                Self(StatusCode::FORBIDDEN, "forbidden", "Seul le propriétaire peut faire ça.".into())
            }
            DbError::Forbidden("lockout") => Self(
                StatusCode::FORBIDDEN,
                "lockout",
                "Ce changement te retirerait l'accès à ce salon, ou le droit de le gérer.".into(),
            ),
            DbError::Slowmode(n) => Self(
                StatusCode::TOO_MANY_REQUESTS,
                "slowmode",
                format!("Mode lent : encore {n} s avant ton prochain message."),
            ),
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
        // Mode lent : le message dit combien de secondes attendre ; l'en-tête le dit aux machines.
        let wait = (self.1 == "slowmode")
            .then(|| self.2.split(|c: char| !c.is_ascii_digit()).find_map(|d| d.parse::<u32>().ok()))
            .flatten();
        let mut res = (self.0, Json(ApiError { code: self.1.into(), message: self.2 })).into_response();
        if let Some(n) = wait {
            res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(n));
        }
        res
    }
}
