//! File de tâches durable dans la table `jobs` : prise avec `FOR UPDATE SKIP LOCKED`,
//! reprise exponentielle, lettre morte (`failed_at`) après trop d'échecs.

use kraken_proto::Id;
use serde_json::Value;
use sqlx::Row;

use crate::{Db, Result};

#[derive(Debug, Clone)]
pub struct Job {
    pub id: Id,
    pub kind: String,
    pub payload: Value,
    pub attempts: i32,
}

impl Db {
    /// Ajoute une tâche. Avec `dedupe`, une tâche de même clé déjà en file n'est pas doublée.
    pub async fn enqueue(&self, kind: &str, payload: Value, dedupe: Option<&str>, delay_s: i32) -> Result<()> {
        sqlx::query(
            "insert into jobs (kind, payload, dedupe_key, run_at) values ($1, $2, $3, now() + make_interval(secs => $4))
             on conflict (dedupe_key) do nothing",
        )
        .bind(kind)
        .bind(payload)
        .bind(dedupe)
        .bind(delay_s)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Prend jusqu'à `n` tâches échues, verrouillées 5 minutes (un worker qui meurt les rend).
    pub async fn claim_jobs(&self, n: i64) -> Result<Vec<Job>> {
        let rows = sqlx::query(
            "update jobs set locked_until = now() + interval '5 minutes', attempts = attempts + 1
             where id in (select id from jobs
                          where run_at <= now() and failed_at is null and (locked_until is null or locked_until < now())
                          order by run_at limit $1 for update skip locked)
             returning id, kind, payload, attempts",
        )
        .bind(n)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(Job {
                    id: r.try_get("id")?,
                    kind: r.try_get("kind")?,
                    payload: r.try_get("payload")?,
                    attempts: r.try_get("attempts")?,
                })
            })
            .collect::<sqlx::Result<_>>()
            .map_err(Into::into)
    }

    pub async fn job_done(&self, id: Id) -> Result<()> {
        sqlx::query("delete from jobs where id = $1").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Tâche récurrente : prochaine exécution dans `in_s` secondes.
    pub async fn job_reschedule(&self, id: Id, in_s: i32) -> Result<()> {
        sqlx::query(
            "update jobs set run_at = now() + make_interval(secs => $2), locked_until = null, attempts = 0, last_error = null
             where id = $1",
        )
        .bind(id)
        .bind(in_s)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Échec : nouvel essai dans 2^tentatives minutes, ou lettre morte après `max` tentatives.
    pub async fn job_failed(&self, job: &Job, error: &str, max: i32) -> Result<()> {
        sqlx::query(
            "update jobs set last_error = $2, locked_until = null,
                    failed_at = case when attempts >= $3 then now() end,
                    run_at = now() + make_interval(mins => least(power(2, attempts)::int, 1440))
             where id = $1",
        )
        .bind(job.id)
        .bind(error)
        .bind(max)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
