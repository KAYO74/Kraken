//! `GET /guilds/{id}/audit-log` : le journal d'audit du serveur, par pages (voir `kraken-db::audit`). Membre du
//! serveur qui a « Voir le journal d'audit » (administrateurs et propriétaire compris), sinon 403 ; qui n'est
//! pas membre reçoit 404. Les paramètres mal formés sont refusés en 400, avec la même forme d'erreur que le
//! reste de l'API.

use axum::Json;
use axum::extract::{Path, Query, State};
use kraken_db::AuditQuery;
use kraken_proto::*;
use serde::Deserialize;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiResult, AppError};

/// Les paramètres arrivent en texte : un UUID, un nombre ou un mot de travers est une erreur 400 de l'API, pas
/// le refus brut de l'extracteur.
#[derive(Deserialize)]
pub struct Params {
    before: Option<String>,
    limit: Option<String>,
    category: Option<String>,
    actor: Option<String>,
}

fn id(name: &str, v: &Option<String>) -> ApiResult<Option<Id>> {
    match v.as_deref().filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => v.parse().map(Some).map_err(|_| AppError::invalid(format!("`{name}` : un identifiant attendu."))),
    }
}

impl Params {
    fn parse(&self) -> ApiResult<AuditQuery> {
        let limit = match self.limit.as_deref().filter(|v| !v.is_empty()) {
            None => AUDIT_PAGE,
            // Hors bornes : ramené dans [1, 100], comme pour les messages.
            Some(v) => v
                .parse::<i64>()
                .map(|n| n.clamp(1, i64::from(MAX_AUDIT_PAGE)) as u32)
                .map_err(|_| AppError::invalid("`limit` : un nombre attendu."))?,
        };
        let category = match self.category.as_deref().filter(|v| !v.is_empty()) {
            None => None,
            Some(v) => Some(AuditCategory::parse(v).ok_or_else(|| {
                AppError::invalid("`category` : membres, roles, salons, invitations, emojis ou serveur.")
            })?),
        };
        Ok(AuditQuery { before: id("before", &self.before)?, limit, category, actor: id("actor", &self.actor)? })
    }
}

pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Path(gid): Path<Id>,
    Query(q): Query<Params>,
) -> ApiResult<Json<AuditPage>> {
    let q = q.parse()?;
    Ok(Json(state.db.audit_log(gid, auth.user.id, &q).await?))
}
