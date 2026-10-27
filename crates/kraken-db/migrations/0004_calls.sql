-- Appels des MP et des groupes : le message d'appel (kind = 'call') retient qui y est entré et
-- quand il a pris fin (null : en cours).
alter table messages add column call_participants uuid[] not null default '{}';
alter table messages add column call_ended_at timestamptz;
create index messages_open_calls on messages (channel_id) where kind = 'call' and call_ended_at is null;
