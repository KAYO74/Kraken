//! Lecture du journal d'audit (`audit_log`) pour le droit « Voir le journal d'audit ». L'écriture se fait
//! ailleurs, dans la transaction de chaque action (`moderation::audit`, `audit_changes`).
//!
//! Le journal se garde sans limite et se lit par pages, du plus récent au plus ancien : l'id d'une entrée
//! est un UUID v7, trié par date, donc `before` (l'id de la dernière entrée vue) suffit de curseur.
//!
//! Ce que la ligne de la base a de brut (`changes`) ne sort jamais tel quel : `shape` ne garde, pour chaque
//! action, que les champs d'une liste blanche, lus avec prudence (une ligne écrite par une version plus
//! ancienne n'a pas toujours tous les champs). Les noms de la cible, de l'auteur et des rôles ou salons
//! cités se lisent au moment de la requête ; une cible qui n'existe plus retombe sur le nom gardé à
//! l'écriture, et sinon n'a pas de nom (le client écrit « supprimé »).

use std::collections::{BTreeMap, HashMap};

use kraken_core::perms;
use kraken_proto::*;
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{Postgres, QueryBuilder, Row};

use crate::{Db, DbError, Result};

/// Ce que l'on demande au journal.
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    /// Les entrées plus anciennes que celle-ci.
    pub before: Option<Id>,
    /// De 1 à [`MAX_AUDIT_PAGE`] (borné ici).
    pub limit: u32,
    pub category: Option<AuditCategory>,
    /// Seulement ce que cette personne a fait.
    pub actor: Option<Id>,
}

fn target_kind(action: &str) -> Option<AuditTargetKind> {
    use AuditTargetKind::*;
    Some(match action {
        "member_role_add"
        | "member_role_remove"
        | "member_kick"
        | "member_ban"
        | "member_unban"
        | "member_timeout"
        | "member_timeout_remove"
        | "owner_transfer"
        | "message_delete" => User,
        "role_create" | "role_update" | "role_delete" => Role,
        "channel_create" | "channel_update" | "channel_delete" | "channel_move" | "overwrite_update" => Channel,
        "emoji_create" | "emoji_update" | "emoji_delete" => Emoji,
        _ => return None,
    })
}

/// Une valeur JSON écrite en texte ; vide pour `null` et pour tout ce qui n'est pas un texte, un nombre ou un booléen.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn put(details: &mut BTreeMap<String, String>, changes: &Value, key: &str) {
    let t = changes.get(key).map(text).unwrap_or_default();
    if !t.is_empty() {
        details.insert(key.into(), t);
    }
}

/// `{ "old": …, "new": … }` : l'avant / après d'un champ. Une valeur simple (écrite avant que le journal
/// garde l'ancienne valeur) n'en est pas un.
fn diff(changes: &Value, field: &str) -> Option<AuditChange> {
    let c = changes.get(field)?;
    let (old, new) = (c.get("old")?, c.get("new")?);
    Some(AuditChange { field: field.into(), before: text(old), after: text(new) })
}

/// Ce que la liste blanche garde de `changes` pour cette action : les faits de la phrase, et les avant / après.
fn shape(action: &str, changes: &Value) -> (BTreeMap<String, String>, Vec<AuditChange>) {
    let mut details = BTreeMap::new();
    let fields: &[&str] = match action {
        "member_role_add" | "member_role_remove" => {
            put(&mut details, changes, "role");
            &[]
        }
        "member_timeout" => {
            put(&mut details, changes, "seconds");
            &[]
        }
        "channel_create" | "channel_delete" => {
            put(&mut details, changes, "kind");
            &[]
        }
        "overwrite_update" => {
            for k in ["allow", "deny"] {
                put(&mut details, changes, k);
            }
            if let Some(k) = changes.get("subject_kind").and_then(Value::as_str)
                && matches!(k, "role" | "member")
            {
                details.insert("subject_kind".into(), k.into());
            }
            &[]
        }
        "invite_create" => {
            for k in ["max_uses", "max_age_s", "channel"] {
                put(&mut details, changes, k);
            }
            &[]
        }
        "emoji_create" => {
            put(&mut details, changes, "animated");
            &[]
        }
        "message_delete" => {
            put(&mut details, changes, "channel");
            &[]
        }
        "role_update" => &["name", "color", "hoist", "permissions"],
        "channel_update" => &["name", "topic", "slowmode_s", "private"],
        "emoji_update" | "guild_update" => &["name"],
        _ => &[],
    };
    (details, fields.iter().filter_map(|f| diff(changes, f)).collect())
}

/// Le nom que la cible portait quand l'action a été écrite.
fn written_name(changes: &Value) -> Option<String> {
    let n = changes.get("target_name").or_else(|| changes.get("name"))?;
    let n = n.get("new").unwrap_or(n);
    Some(text(n)).filter(|n| !n.is_empty())
}

fn id_in(changes: &Value, key: &str) -> Option<Id> {
    changes.get(key)?.as_str()?.parse().ok()
}

/// Les noms de ces personnes (nom affiché, ou pseudo).
async fn user_names(db: &Db, ids: &[Id]) -> Result<HashMap<Id, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Id, String)> =
        sqlx::query_as("select id, coalesce(display_name, username) from users where id = any($1)")
            .bind(ids)
            .fetch_all(&db.pool)
            .await?;
    Ok(rows.into_iter().collect())
}

/// Les noms de ces rôles, salons ou emojis du serveur.
async fn names_in(db: &Db, table: &str, gid: Id, ids: &[Id]) -> Result<HashMap<Id, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    // `table` est l'un de trois mots écrits ici, jamais une saisie.
    let rows: Vec<(Id, Option<String>)> =
        sqlx::query_as(&format!("select id, name from {table} where guild_id = $1 and id = any($2)"))
            .bind(gid)
            .bind(ids)
            .fetch_all(&db.pool)
            .await?;
    Ok(rows.into_iter().filter_map(|(id, n)| Some((id, n?))).collect())
}

struct Raw {
    id: Id,
    created_at: i64,
    actor: Option<AuditActor>,
    action: String,
    target_id: Option<Id>,
    changes: Value,
    reason: Option<String>,
}

fn raw_from(r: &PgRow) -> sqlx::Result<Raw> {
    let actor_id: Option<Id> = r.try_get("actor_id")?;
    let actor_name: Option<String> = r.try_get("actor_name")?;
    let avatar: Option<String> = r.try_get("actor_avatar")?;
    Ok(Raw {
        id: r.try_get("id")?,
        created_at: r.try_get("at_ms")?,
        actor: actor_id.zip(actor_name).map(|(id, name)| AuditActor { id, name, avatar }),
        action: r.try_get("action")?,
        target_id: r.try_get("target_id")?,
        changes: r.try_get::<sqlx::types::Json<Value>, _>("changes")?.0,
        reason: r.try_get("reason")?,
    })
}

impl Db {
    /// Une page du journal d'audit du serveur, pour qui a le droit de le lire : un membre (sinon `NotFound`,
    /// on ne révèle rien) qui a « Voir le journal d'audit », administrateurs et propriétaire compris.
    pub async fn audit_log(&self, gid: Id, by: Id, q: &AuditQuery) -> Result<AuditPage> {
        let p = self.guild_perms(by, gid).await?;
        if p == 0 {
            return Err(DbError::NotFound);
        }
        if p & perms::VIEW_AUDIT_LOG == 0 {
            return Err(DbError::Forbidden("perm"));
        }
        let limit = q.limit.clamp(1, MAX_AUDIT_PAGE) as i64;

        let mut sql: QueryBuilder<Postgres> = QueryBuilder::new(
            "select a.id, (extract(epoch from a.created_at) * 1000)::int8 as at_ms, a.actor_id,
               coalesce(u.display_name, u.username) as actor_name, u.avatar_key as actor_avatar,
               a.action, a.target_id, a.changes, a.reason
             from audit_log a left join users u on u.id = a.actor_id where a.guild_id = ",
        );
        sql.push_bind(gid);
        if let Some(before) = q.before {
            sql.push(" and a.id < ").push_bind(before);
        }
        if let Some(c) = q.category {
            sql.push(" and a.action = any(").push_bind(c.actions().iter().map(|a| a.to_string()).collect::<Vec<_>>());
            sql.push(")");
        }
        if let Some(actor) = q.actor {
            sql.push(" and a.actor_id = ").push_bind(actor);
        }
        // Une ligne de plus que demandé : savoir s'il y a une page suivante.
        sql.push(" order by a.id desc limit ").push_bind(limit + 1);
        let mut rows: Vec<Raw> =
            sql.build().fetch_all(&self.pool).await?.iter().map(raw_from).collect::<sqlx::Result<_>>()?;
        let next = if rows.len() as i64 > limit {
            rows.truncate(limit as usize);
            rows.last().map(|r| r.id)
        } else {
            None
        };

        // Les noms à lire : cibles, rôles et salons cités, personnes citées.
        let (mut users, mut roles, mut channels, mut emojis) = (vec![], vec![], vec![], vec![]);
        for r in &rows {
            let subject = id_in(&r.changes, "target").filter(|_| r.action == "overwrite_update");
            match target_kind(&r.action) {
                Some(AuditTargetKind::User) => users.extend(r.target_id),
                Some(AuditTargetKind::Role) => roles.extend(r.target_id),
                Some(AuditTargetKind::Channel) => channels.extend(r.target_id),
                Some(AuditTargetKind::Emoji) => emojis.extend(r.target_id),
                None => {}
            }
            match r.changes.get("subject_kind").and_then(Value::as_str) {
                Some("role") => roles.extend(subject),
                Some("member") => users.extend(subject),
                // Écrit avant que le genre soit gardé : le rôle d'abord, la personne sinon.
                _ => {
                    roles.extend(subject);
                    users.extend(subject);
                }
            }
            if r.action == "invite_revoke" {
                users.extend(id_in(&r.changes, "inviter"));
            }
        }
        for v in [&mut users, &mut roles, &mut channels, &mut emojis] {
            v.sort();
            v.dedup();
        }
        let user_names = user_names(self, &users).await?;
        let role_names = names_in(self, "roles", gid, &roles).await?;
        let channel_names = names_in(self, "channels", gid, &channels).await?;
        let emoji_names = names_in(self, "emojis", gid, &emojis).await?;

        let entries = rows
            .into_iter()
            .map(|r| {
                let (mut details, changes) = shape(&r.action, &r.changes);
                let target = target_kind(&r.action).zip(r.target_id).map(|(kind, id)| {
                    let live = match kind {
                        AuditTargetKind::User => user_names.get(&id),
                        AuditTargetKind::Role => role_names.get(&id),
                        AuditTargetKind::Channel => channel_names.get(&id),
                        AuditTargetKind::Emoji => emoji_names.get(&id),
                    };
                    // Une personne qui a supprimé son compte n'a plus de nom ici : on n'en garde pas la trace.
                    let name = live.cloned().or_else(|| match kind {
                        AuditTargetKind::User => None,
                        _ => written_name(&r.changes),
                    });
                    AuditTarget { kind, id, name }
                });
                if r.action == "overwrite_update"
                    && let Some(id) = id_in(&r.changes, "target")
                {
                    let in_roles = role_names.get(&id);
                    let in_users = user_names.get(&id);
                    let (kind, name) = match details.get("subject_kind").map(String::as_str) {
                        Some("role") => ("role", in_roles),
                        Some(_) => ("member", in_users),
                        None if in_roles.is_some() => ("role", in_roles),
                        None => ("member", in_users),
                    };
                    details.insert("subject_kind".into(), kind.into());
                    if let Some(n) = name {
                        details.insert("subject".into(), n.clone());
                    }
                }
                if r.action == "invite_revoke"
                    && let Some(n) = id_in(&r.changes, "inviter").and_then(|i| user_names.get(&i))
                {
                    details.insert("inviter".into(), n.clone());
                }
                AuditEntry {
                    id: r.id,
                    created_at: r.created_at,
                    actor: r.actor,
                    action: r.action,
                    target,
                    reason: r.reason,
                    details,
                    changes,
                }
            })
            .collect();

        let actors = if q.before.is_none() { self.audit_actors(gid).await? } else { vec![] };
        Ok(AuditPage { entries, next, actors })
    }

    /// Tous ceux qui ont agi dans ce serveur, par nom (au plus [`MAX_AUDIT_ACTORS`]).
    async fn audit_actors(&self, gid: Id) -> Result<Vec<AuditActor>> {
        let rows = sqlx::query(
            "select u.id, coalesce(u.display_name, u.username) as name, u.avatar_key as avatar from users u
             where u.id in (select distinct actor_id from audit_log where guild_id = $1 and actor_id is not null)
             order by lower(coalesce(u.display_name, u.username)), u.id limit $2",
        )
        .bind(gid)
        .bind(MAX_AUDIT_ACTORS as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| Ok(AuditActor { id: r.try_get("id")?, name: r.try_get("name")?, avatar: r.try_get("avatar")? }))
            .collect::<sqlx::Result<_>>()
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn keeps_only_the_whitelist() {
        let (d, c) = shape(
            "invite_create",
            &json!({ "code": "SECRET12", "max_uses": 5, "max_age_s": null, "channel": "général", "other": 1 }),
        );
        assert_eq!(d.get("max_uses").map(String::as_str), Some("5"));
        assert_eq!(d.get("channel").map(String::as_str), Some("général"));
        assert!(!d.contains_key("code") && !d.contains_key("max_age_s") && !d.contains_key("other"));
        assert!(c.is_empty());
    }

    #[test]
    fn reads_before_and_after_and_ignores_plain_values() {
        let (_, c) = shape(
            "channel_update",
            &json!({ "name": { "old": "a", "new": "b" }, "topic": "ancienne écriture", "slowmode_s": { "old": 0, "new": 5 } }),
        );
        let fields: Vec<_> = c.iter().map(|c| (c.field.as_str(), c.before.as_str(), c.after.as_str())).collect();
        assert_eq!(fields, [("name", "a", "b"), ("slowmode_s", "0", "5")]);
        // Une action qui n'a pas d'avant / après n'en sort pas, quoi que contienne la ligne.
        assert!(shape("member_ban", &json!({ "name": { "old": "x", "new": "y" } })).1.is_empty());
    }

    #[test]
    fn written_name_follows_the_conventions() {
        assert_eq!(written_name(&json!({ "name": "annonces" })).as_deref(), Some("annonces"));
        assert_eq!(written_name(&json!({ "name": { "old": "a", "new": "b" } })).as_deref(), Some("b"));
        assert_eq!(written_name(&json!({ "name": "x", "target_name": "y" })).as_deref(), Some("y"));
        assert_eq!(written_name(&json!({ "name": "" })), None);
        assert_eq!(written_name(&json!({})), None);
    }
}
