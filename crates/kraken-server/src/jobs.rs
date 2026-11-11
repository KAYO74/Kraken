//! Tâches de fond : e-mails (vérification, nouvelle adresse, mot de passe oublié) et purge des comptes
//! supprimés. Reprise exponentielle, lettre morte après 5 échecs (`kraken-db::jobs`).

use std::time::Duration;

use kraken_db::Job;
use lettre::message::Mailbox;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use uuid::Uuid;

use crate::AppState;
use crate::auth::{new_token, token_hash};

const MAX_ATTEMPTS: i32 = 5;
const GRACE_DAYS: i32 = 14;

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Mailer {
    pub fn new(url: &str, from: &str) -> Result<Self, String> {
        let transport =
            AsyncSmtpTransport::<Tokio1Executor>::from_url(url).map_err(|e| format!("KRAKEN_SMTP_URL : {e}"))?.build();
        let from = from.parse().map_err(|e| format!("KRAKEN_SMTP_FROM : {e}"))?;
        Ok(Self { transport, from })
    }

    async fn send(&self, to: &str, subject: &str, body: String) -> Result<(), String> {
        let msg = Message::builder()
            .from(self.from.clone())
            .to(to.parse().map_err(|e| format!("destinataire : {e}"))?)
            .subject(subject)
            .body(body)
            .map_err(|e| e.to_string())?;
        // Le texte d'un refus SMTP cite souvent le destinataire : seul le code va dans les logs.
        self.transport.send(msg).await.map(|_| ()).map_err(|e| match e.status() {
            Some(code) => format!("refus SMTP {code}"),
            None => "SMTP injoignable".into(),
        })
    }
}

pub async fn run(state: AppState) {
    if let Err(e) = state.db.enqueue("account_purge", serde_json::json!({}), Some("cron:account_purge"), 60).await {
        tracing::error!("tâches : {e:?}");
    }
    if let Err(e) =
        state.db.enqueue("attachment_sweep", serde_json::json!({}), Some("cron:attachment_sweep"), 600).await
    {
        tracing::error!("tâches : {e:?}");
    }
    loop {
        match state.db.claim_jobs(10).await {
            Ok(jobs) if !jobs.is_empty() => {
                for job in jobs {
                    handle(&state, job).await;
                }
                continue;
            }
            Ok(_) => {}
            Err(e) => tracing::error!("tâches : {e:?}"),
        }
        // Au plus 5 s d'attente, moins si une tâche vient d'être ajoutée (aperçu de lien).
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            _ = state.jobs_wake.notified() => {}
        }
    }
}

async fn handle(state: &AppState, job: Job) {
    let result = match job.kind.as_str() {
        "email_verify" => email(state, &job, "verify_email").await,
        "email_reset" => email(state, &job, "reset_password").await,
        "email_change" => email_change(state, &job).await,
        "embed_fetch" => crate::embed::run(state, &job).await,
        "attachment_sweep" => {
            // Tâche permanente, comme la purge des comptes : reprise toutes les heures.
            crate::attach::sweep(state).await;
            let _ = state.db.job_reschedule(job.id, 3600).await;
            return;
        }
        "account_purge" => {
            // Tâche permanente : jamais en lettre morte, reprise dans une heure en cas d'échec.
            let next = match state.db.purge_deleted_accounts(GRACE_DAYS).await {
                Ok((n, avatars, files)) => {
                    for key in avatars {
                        crate::avatar::forget(state, Some(key)).await;
                    }
                    crate::attach::remove_files(state, files).await;
                    if n > 0 {
                        tracing::info!("{n} compte(s) anonymisé(s)");
                    }
                    24 * 3600
                }
                Err(e) => {
                    tracing::error!("purge des comptes : {e:?}");
                    3600
                }
            };
            let _ = state.db.job_reschedule(job.id, next).await;
            return;
        }
        other => Err(format!("tâche inconnue : {other}")),
    };
    let saved = match result {
        Ok(()) => state.db.job_done(job.id).await,
        Err(e) => {
            tracing::warn!("tâche {} ({}) en échec : {e}", job.kind, job.attempts);
            state.db.job_failed(&job, &e, MAX_ATTEMPTS).await
        }
    };
    if let Err(e) = saved {
        tracing::error!("tâches : {e:?}");
    }
}

/// Confirmation d'une nouvelle adresse : le lien part vers la nouvelle adresse (pas l'ancienne,
/// qui reste celle du compte jusqu'au clic). Le jeton porte l'adresse qu'il confirme : une
/// demande plus récente rend les liens précédents inutiles.
async fn email_change(state: &AppState, job: &Job) -> Result<(), String> {
    let mailer = state.mailer.as_ref().ok_or("pas de SMTP configuré")?;
    let uid: Uuid =
        job.payload.get("user_id").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).ok_or("user_id manquant")?;
    let Some((to, username)) = state.db.pending_email(uid).await.map_err(|e| format!("{e:?}"))? else {
        return Ok(()); // demande annulée, ou compte supprimé entre-temps
    };
    let token = new_token();
    state.db.create_email_change_token(uid, &to, &token_hash(&token), 24 * 60).await.map_err(|e| format!("{e:?}"))?;
    let base = state.cfg.public_url.trim_end_matches('/');
    let body = format!(
        "Bonjour {username},\n\nPour confirmer cette nouvelle adresse e-mail pour ton compte KRAKEN, ouvre ce lien dans les 24 heures :\n{base}/verify-email?token={token}\n\n\
         Tu n'as rien demandé ? Ignore ce message : ton compte garde son adresse actuelle.\n"
    );
    mailer.send(&to, "Confirme ta nouvelle adresse e-mail KRAKEN", body).await
}

/// Crée le jeton au moment de l'envoi : il n'est jamais stocké en clair, même dans la file.
async fn email(state: &AppState, job: &Job, kind: &str) -> Result<(), String> {
    let mailer = state.mailer.as_ref().ok_or("pas de SMTP configuré")?;
    let uid: Uuid =
        job.payload.get("user_id").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).ok_or("user_id manquant")?;
    let Some((to, username, verified)) = state.db.user_email(uid).await.map_err(|e| format!("{e:?}"))? else {
        return Ok(()); // compte supprimé entre-temps
    };
    if kind == "verify_email" && verified {
        return Ok(());
    }
    let token = new_token();
    let ttl = if kind == "verify_email" { 24 * 60 } else { 30 };
    state.db.create_auth_token(uid, kind, &token_hash(&token), ttl).await.map_err(|e| format!("{e:?}"))?;
    let base = state.cfg.public_url.trim_end_matches('/');
    let (subject, body) = if kind == "verify_email" {
        (
            "Confirme ton adresse e-mail KRAKEN",
            format!(
                "Bonjour {username},\n\nPour confirmer cette adresse, ouvre ce lien dans les 24 heures :\n{base}/verify-email?token={token}\n\n\
                 Tu n'as pas créé de compte KRAKEN ? Tu peux ignorer ce message.\n"
            ),
        )
    } else {
        (
            "Nouveau mot de passe KRAKEN",
            format!(
                "Bonjour {username},\n\nQuelqu'un, sans doute toi, a demandé un nouveau mot de passe pour ce compte. \
                 Ce lien sert une seule fois, pendant 30 minutes :\n{base}/reset-password?token={token}\n\n\
                 Si ce n'est pas toi, ne fais rien : ton mot de passe ne change pas.\n"
            ),
        )
    };
    mailer.send(&to, subject, body).await
}
