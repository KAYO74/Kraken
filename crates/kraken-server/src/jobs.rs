//! Tâches de fond : e-mails (vérification, mot de passe oublié) et purge des comptes
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
        self.transport.send(msg).await.map(|_| ()).map_err(|e| e.to_string())
    }
}

pub async fn run(state: AppState) {
    if let Err(e) = state.db.enqueue("account_purge", serde_json::json!({}), Some("cron:account_purge"), 60).await {
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
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn handle(state: &AppState, job: Job) {
    let result = match job.kind.as_str() {
        "email_verify" => email(state, &job, "verify_email").await,
        "email_reset" => email(state, &job, "reset_password").await,
        "account_purge" => match state.db.purge_deleted_accounts(GRACE_DAYS).await {
            Ok(n) => {
                if n > 0 {
                    tracing::info!("{n} compte(s) anonymisé(s)");
                }
                let _ = state.db.job_reschedule(job.id, 24 * 3600).await;
                return;
            }
            Err(e) => Err(format!("{e:?}")),
        },
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
