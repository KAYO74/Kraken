-- Mode lent : le moment du dernier message de chacun dans un salon qui en a un. Une ligne par
-- (salon, personne), remplacée à chaque envoi accepté ; elle part avec le salon ou le compte.
create table slowmode_marks (
  channel_id uuid not null references channels(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  last_at timestamptz not null default now(),
  primary key (channel_id, user_id)
);
