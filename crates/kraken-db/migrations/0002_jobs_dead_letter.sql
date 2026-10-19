-- File de tâches : une tâche qui échoue trop souvent est mise de côté (lettre morte)
-- au lieu d'être retentée sans fin. Les tâches récurrentes gardent leur ligne.
alter table jobs add column failed_at timestamptz;
drop index if exists jobs_run_at_idx;
create index jobs_due_idx on jobs (run_at) where failed_at is null;
