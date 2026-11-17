use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use kraken_db::DbError;
use kraken_proto::{
    ApiError, MAX_CHANNEL_NAME, MAX_CHANNELS, MAX_EMOJIS, MAX_GROUP_MEMBERS, MAX_GROUP_NAME, MAX_INVITE_USES,
    MAX_INVITES_PER_MEMBER, MAX_OVERWRITES, MAX_PINS, MAX_PURGE_HOURS, MAX_REASON, MAX_ROLES, MAX_TOPIC,
    USERNAME_CHANGES_PER_WINDOW,
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
    /// Invitation inconnue (jamais créée, ou révoquée).
    pub fn invite_unknown() -> Self {
        Self(StatusCode::NOT_FOUND, "invite_unknown", "Cette invitation n'existe pas ou a été révoquée.".into())
    }
    /// Exclu temporairement jusqu'à `until` (ms Unix) : lire reste permis, pas le reste. Le
    /// texte du message porte la fin ; `into_response` la met aussi dans le champ `timeout_until`.
    pub fn timed_out(until: i64) -> Self {
        Self(StatusCode::FORBIDDEN, "timed_out", until.to_string())
    }
    /// Mot de passe actuel refusé (changement de pseudo, d'adresse ou de mot de passe, suppression).
    pub fn bad_password() -> Self {
        Self(StatusCode::FORBIDDEN, "bad_password", "Mot de passe incorrect.".into())
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
            DbError::Conflict("emoji_name") => {
                Self(StatusCode::CONFLICT, "name_taken", "Ce nom est déjà pris dans ce serveur.".into())
            }
            DbError::Conflict(f) => Self(StatusCode::CONFLICT, "conflict", format!("Déjà utilisé : {f}.")),
            DbError::Cooldown(secs) => Self(StatusCode::TOO_MANY_REQUESTS, "username_cooldown", secs.to_string()),
            DbError::Invalid("username") => Self::invalid("Pseudo : 2 à 32 caractères parmi a-z, 0-9, _ et ."),
            DbError::Invalid("reactions") => Self::invalid("20 emojis différents au plus sur un message."),
            DbError::Invalid("emoji") => Self(
                StatusCode::BAD_REQUEST,
                "emoji",
                "Seuls les emojis de la sélection ou ceux de tes serveurs servent de réaction.".into(),
            ),
            DbError::Invalid("emoji_name") => Self(
                StatusCode::BAD_REQUEST,
                "invalid_name",
                "Le nom d'un emoji compte de 2 à 32 caractères : lettres sans accent, chiffres et _.".into(),
            ),
            DbError::Invalid("emoji_limit") => Self(
                StatusCode::BAD_REQUEST,
                "emoji_limit",
                format!("Ce serveur a déjà ses {MAX_EMOJIS} emojis : retires-en un pour en ajouter."),
            ),
            DbError::Invalid("pin_limit") => Self(
                StatusCode::BAD_REQUEST,
                "pin_limit",
                format!("Ce salon a déjà {MAX_PINS} messages épinglés : désépingles-en un d'abord."),
            ),
            DbError::Invalid("pin_kind") => Self(
                StatusCode::BAD_REQUEST,
                "pin_kind",
                "Seuls les messages écrits (texte ou réponse) peuvent être épinglés.".into(),
            ),
            DbError::Invalid("reply_to") => Self::invalid("Le message d'origine n'est plus dans ce salon."),
            DbError::Invalid("self") => Self(StatusCode::BAD_REQUEST, "self", "C'est ton propre pseudo.".into()),
            DbError::Invalid("recipients") => {
                Self::invalid(format!("Un groupe réunit de 3 à {MAX_GROUP_MEMBERS} personnes, toi compris."))
            }
            DbError::Invalid("group_full") => {
                Self::invalid(format!("Ce groupe est complet : {MAX_GROUP_MEMBERS} personnes au plus."))
            }
            DbError::Invalid("guild_name") => Self::invalid("Le nom d'un serveur compte de 2 à 100 caractères."),
            DbError::Invalid("guild_name_chars") => Self(
                StatusCode::BAD_REQUEST,
                "invalid_chars",
                "Le nom du serveur contient un caractère interdit (tabulation, saut de ligne ou autre caractère de contrôle).".into(),
            ),
            DbError::Invalid("template") => Self::invalid("Ce modèle de serveur n'existe pas."),
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
            DbError::Gone("expired") => Self(
                StatusCode::NOT_FOUND,
                "invite_expired",
                "Cette invitation a expiré. Demande-en une nouvelle.".into(),
            ),
            DbError::Gone(_) => Self(
                StatusCode::NOT_FOUND,
                "invite_exhausted",
                "Cette invitation a déjà servi autant de fois que permis. Demande-en une nouvelle.".into(),
            ),
            DbError::Invalid("invite_limit") => Self(
                StatusCode::BAD_REQUEST,
                "invite_limit",
                format!("Tu as déjà {MAX_INVITES_PER_MEMBER} invitations actives. Révoque-en une."),
            ),
            DbError::Invalid("invite_age") => Self::invalid("Une invitation dure 7 jours au plus, ou n'expire jamais."),
            DbError::Invalid("invite_uses") => {
                Self::invalid(format!("Une invitation sert {MAX_INVITE_USES} fois au plus (0 : sans limite)."))
            }
            DbError::Invalid("name_mismatch") => Self(
                StatusCode::BAD_REQUEST,
                "name_mismatch",
                "Le nom retapé ne correspond pas à celui du serveur.".into(),
            ),
            DbError::Invalid("timeout_seconds") => {
                Self::invalid("Une exclusion temporaire dure de 1 minute à 28 jours.")
            }
            DbError::Invalid("overwrite_kind") => {
                Self::invalid("Ce réglage existe déjà pour un autre genre de cible (rôle ou membre).")
            }
            DbError::Invalid("overwrites") => {
                Self::invalid(format!("{MAX_OVERWRITES} réglages de droits au plus par salon."))
            }
            DbError::Invalid("name") => Self::invalid(format!("Le nom tient en {MAX_GROUP_NAME} caractères au plus.")),
            DbError::Invalid("mute") => Self::invalid("Une sourdine dure de 1 minute à 30 jours, ou sans fin."),
            DbError::Invalid("target") => Self::invalid("Un serveur ou un salon, l'un des deux."),
            DbError::Invalid("self_target") => Self::invalid("Tu ne peux pas faire ça sur toi-même."),
            DbError::Invalid("hidden_chars") => Self(
                StatusCode::BAD_REQUEST,
                "invalid_chars",
                "Les caractères invisibles (espace de largeur nulle, changement de sens d'écriture, remplisseur) ne sont pas acceptés dans un nom ou un motif.".into(),
            ),
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
            DbError::Forbidden("admin_target") => Self(
                StatusCode::FORBIDDEN,
                "forbidden",
                "Un administrateur ne peut pas être exclu temporairement.".into(),
            ),
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
            // Épingler dans un MP bloqué : mêmes codes que l'envoi, une phrase qui parle d'épingles (et qui ne dit
            // pas, quand c'est l'autre qui a bloqué, que c'est lui).
            DbError::Forbidden("pin_blocked") => Self(
                StatusCode::FORBIDDEN,
                "dm_blocked",
                "Tu ne peux pas épingler ni désépingler ici : tu as bloqué cette personne.".into(),
            ),
            DbError::Forbidden("pin_refused") => Self(
                StatusCode::FORBIDDEN,
                "dm_refused",
                "Tu ne peux pas épingler ni désépingler ici : la conversation est bloquée.".into(),
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

impl AppError {
    /// Ce que la réponse porte : le statut, le corps (code, message, fin d'exclusion) et l'attente qu'elle annonce
    /// (`Retry-After`).
    fn parts(self) -> (StatusCode, ApiError, Option<u32>) {
        // Mode lent : le message dit combien de secondes attendre ; l'en-tête le dit aux machines.
        let wait = (self.1 == "slowmode")
            .then(|| self.2.split(|c: char| !c.is_ascii_digit()).find_map(|d| d.parse::<u32>().ok()))
            .flatten();
        // `timed_out` : la fin de l'exclusion voyage dans le message jusqu'ici, où elle devient un champ.
        let (message, timeout_until) = match (self.1, self.2.parse::<i64>()) {
            ("timed_out", Ok(until)) => (
                "Tu es exclu temporairement : tu peux lire, mais pas écrire, réagir, envoyer de fichier ni parler."
                    .to_string(),
                Some(until),
            ),
            _ => (self.2, None),
        };
        // `username_cooldown` : les secondes à attendre voyagent dans le message jusqu'ici.
        let (message, wait) = match (self.1, message.parse::<u32>()) {
            ("username_cooldown", Ok(secs)) => (
                // Deux phrases entières (pas une durée collée dans une troisième) : le client en connaît le gabarit.
                if secs < 90 {
                    format!(
                        "Tu as déjà changé ton pseudo {USERNAME_CHANGES_PER_WINDOW} fois en une heure. Réessaie dans {secs} s."
                    )
                } else {
                    format!(
                        "Tu as déjà changé ton pseudo {USERNAME_CHANGES_PER_WINDOW} fois en une heure. Réessaie dans {} min.",
                        secs.div_ceil(60)
                    )
                },
                Some(secs),
            ),
            _ => (message, wait),
        };
        (self.0, ApiError { code: self.1.into(), message, timeout_until }, wait)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body, wait) = self.parts();
        let mut res = (status, Json(body)).into_response();
        if let Some(n) = wait {
            res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(n));
        }
        res
    }
}

/// Le contrôle de couverture des messages : ce que le client sait traduire (`kraken-client/i18n/en/server.tsv`,
/// lu par `api.rs::from_server`). Un message est couvert s'il est dans le catalogue tel quel, ou si un de ses
/// gabarits à `{variables}` le reconnaît (même code que le client : le gabarit le plus précis d'abord).
#[cfg(test)]
pub(crate) mod coverage {
    use kraken_core::i18n::{Catalog, parse_catalog};
    use kraken_core::pattern::{templates_of, translate};

    /// Les catalogues anglais du client, tous fichiers confondus (les libellés `{champ}` viennent d'un autre fichier).
    fn english() -> Catalog {
        let mut all = Catalog::default();
        for src in [
            include_str!("../../kraken-client/i18n/en/core.tsv"),
            include_str!("../../kraken-client/i18n/en/chat.tsv"),
            include_str!("../../kraken-client/i18n/en/guilds.tsv"),
            include_str!("../../kraken-client/i18n/en/voice.tsv"),
            include_str!("../../kraken-client/i18n/en/app.tsv"),
            include_str!("../../kraken-client/i18n/en/server.tsv"),
        ] {
            all.add(src);
        }
        assert_eq!((all.malformed, all.duplicates), (0, 0));
        all
    }

    /// Le message en anglais tel que le client le montre (`api.rs::from_server` : la recherche exacte, puis le
    /// gabarit de `server.tsv` qui le reconnaît, avec le même code), ou `None` si le catalogue ne le couvre pas.
    pub(crate) fn english_of(message: &str) -> Option<String> {
        let all = english();
        if let Some(en) = all.get(message) {
            return Some(en.to_string());
        }
        let server = parse_catalog(include_str!("../../kraken-client/i18n/en/server.tsv"));
        translate(&templates_of(&server), message, |k| all.get(k).map(str::to_string))
    }

    /// Le message du serveur est traduisible : le catalogue le couvre, et l'anglais n'est ni le français ni un
    /// gabarit resté ouvert.
    #[track_caller]
    pub(crate) fn assert_translated(message: &str) {
        let Some(en) = english_of(message) else {
            panic!("message du serveur sans gabarit dans server.tsv : {message:?}")
        };
        assert!(!en.contains(['{', '}']), "{message:?} -> {en:?} : variable non remplie");
        // Une phrase française ne reste pas dans la traduction (accents, « : » avec espace avant).
        assert!(!en.contains(['é', 'è', 'ê', 'à', 'ù', 'ç', 'ô', '«', '»', '…']) || en == message, "{en:?}");
        assert_ne!(en, message, "{message:?} : anglais identique au français");
    }
}

#[cfg(test)]
mod tests {
    use super::coverage::{assert_translated, english_of};
    use super::*;

    /// Toutes les raisons que `From<DbError>` connaît, lues dans ce fichier : une raison ajoutée sans gabarit
    /// casse ce test (`DbError::Invalid("pin_limit")` → `("Invalid", "pin_limit")`).
    fn known_reasons() -> Vec<(&'static str, &'static str)> {
        let src: &'static str = include_str!("error.rs");
        let body = &src[..src.find("impl AppError {\n    /// Ce que la réponse porte").unwrap()];
        let mut out = Vec::new();
        for kind in ["Invalid", "Conflict", "Forbidden", "Gone"] {
            let needle = format!("DbError::{kind}(\"");
            let mut rest = body;
            while let Some(i) = rest.find(&needle) {
                let after = &rest[i + needle.len()..];
                let reason = &after[..after.find('"').unwrap()];
                out.push((kind, reason));
                rest = after;
            }
        }
        out
    }

    fn db_error(kind: &str, reason: &'static str) -> DbError {
        match kind {
            "Invalid" => DbError::Invalid(reason),
            "Conflict" => DbError::Conflict(reason),
            "Forbidden" => DbError::Forbidden(reason),
            _ => DbError::Gone(reason),
        }
    }

    fn message(e: AppError) -> String {
        e.parts().1.message
    }

    #[test]
    fn every_message_of_every_known_db_error_has_an_english_template_or_text() {
        let reasons = known_reasons();
        assert!(reasons.len() >= 60, "la lecture de error.rs a échoué : {}", reasons.len());
        // Tous les manques d'un coup : on les ajoute ensemble à `server.tsv`.
        let missing: Vec<_> = reasons
            .into_iter()
            .map(|(kind, reason)| message(AppError::from(db_error(kind, reason))))
            .filter(|m| english_of(m).is_none())
            .collect();
        assert!(missing.is_empty(), "sans gabarit dans server.tsv : {missing:#?}");
        // Les raisons qu'aucun bras ne connaît tombent sur « Déjà utilisé » / « Valeur refusée » / « Introuvable ».
        for e in [
            DbError::Conflict("email_address"),
            DbError::Invalid("champ_inconnu"),
            DbError::Gone("exhausted"),
            DbError::NotFound,
        ] {
            assert_translated(&message(AppError::from(e)));
        }
        assert_eq!(message(AppError::from(DbError::Invalid("limit_x"))), "Valeur refusée : limit_x.");
        assert_eq!(english_of("Valeur refusée : limit_x.").as_deref(), Some("Value rejected: limit_x."));
        assert_eq!(english_of("Déjà utilisé : email.").as_deref(), Some("Already in use: email."));
    }

    #[test]
    fn the_messages_that_carry_a_number_are_built_from_the_real_constants() {
        let en = |e: DbError| english_of(&message(AppError::from(e))).unwrap();
        assert_eq!(
            en(DbError::Invalid("pin_limit")),
            format!("This channel already has {MAX_PINS} pinned messages: unpin one first.")
        );
        assert_eq!(
            en(DbError::Invalid("emoji_limit")),
            format!("This server already has its {MAX_EMOJIS} emojis: remove one to add another.")
        );
        assert_eq!(
            en(DbError::Invalid("recipients")),
            format!("A group has 3 to {MAX_GROUP_MEMBERS} people, including you.")
        );
        assert_eq!(
            en(DbError::Invalid("group_full")),
            format!("This group is full: {MAX_GROUP_MEMBERS} people at most.")
        );
        assert_eq!(
            en(DbError::Invalid("channel_name")),
            format!("A channel name is 1 to {MAX_CHANNEL_NAME} characters long, with no control characters.")
        );
        assert_eq!(
            en(DbError::Invalid("channels")),
            format!("A server can have at most {MAX_CHANNELS} channels and categories.")
        );
        assert_eq!(
            en(DbError::Invalid("topic")),
            format!("A topic is {MAX_TOPIC} characters at most, and only text channels have one.")
        );
        assert_eq!(
            en(DbError::Invalid("invite_limit")),
            format!("You already have {MAX_INVITES_PER_MEMBER} active invitations. Revoke one.")
        );
        assert_eq!(
            en(DbError::Invalid("invite_uses")),
            format!("An invitation can be used {MAX_INVITE_USES} times at most (0: no limit).")
        );
        assert_eq!(
            en(DbError::Invalid("overwrites")),
            format!("At most {MAX_OVERWRITES} permission settings per channel.")
        );
        assert_eq!(en(DbError::Invalid("name")), format!("The name is {MAX_GROUP_NAME} characters at most."));
        assert_eq!(en(DbError::Invalid("reason")), format!("The reason is {MAX_REASON} characters at most."));
        assert_eq!(
            en(DbError::Invalid("hours")),
            format!("You can delete messages from the last {MAX_PURGE_HOURS} hours at most.")
        );
        assert_eq!(
            en(DbError::Invalid("roles")),
            format!("At most {MAX_ROLES} roles, and the order must list them all.")
        );
        assert_eq!(en(DbError::Slowmode(30)), "Slow mode: 30 s until your next message.");
    }

    #[test]
    fn the_username_cooldown_reads_in_seconds_then_in_minutes() {
        let n = USERNAME_CHANGES_PER_WINDOW;
        for (secs, fr, en) in [
            (1, "1 s", "1 s"),
            (89, "89 s", "89 s"),
            (90, "2 min", "2 min"),
            (3000, "50 min", "50 min"),
            (3600, "60 min", "60 min"),
        ] {
            let (_, body, wait) = AppError::from(DbError::Cooldown(secs)).parts();
            assert_eq!(wait, Some(secs));
            assert_eq!(
                body.message,
                format!("Tu as déjà changé ton pseudo {n} fois en une heure. Réessaie dans {fr}.")
            );
            assert_eq!(
                english_of(&body.message).unwrap(),
                format!("You've already changed your username {n} times in an hour. Try again in {en}.")
            );
        }
    }

    #[test]
    fn the_answers_built_by_the_response_layer_are_covered_too() {
        for e in [
            AppError::unauthorized(),
            AppError::forbidden(),
            AppError::not_found(),
            AppError::invite_unknown(),
            AppError::bad_password(),
            AppError::timed_out(1_700_000_000_000),
        ] {
            assert_translated(&message(e));
        }
        // La fin de l'exclusion est un champ, pas une phrase.
        let (_, body, _) = AppError::timed_out(42).parts();
        assert_eq!(body.timeout_until, Some(42));
        // Le mode lent annonce son attente en en-tête aussi.
        let (status, body, wait) = AppError::from(DbError::Slowmode(7)).parts();
        assert_eq!((status, body.code.as_str(), wait), (StatusCode::TOO_MANY_REQUESTS, "slowmode", Some(7)));
        assert_eq!(body.message, "Mode lent : encore 7 s avant ton prochain message.");
    }
}
