-- Profil, compte et présence. La bannière (`banner_key`), la bio et les pronoms existent déjà
-- dans `users` (0001) ; il manque :
--  * le statut choisi par la personne (en ligne, inactif, ne pas déranger, invisible) ; l'état
--    « hors ligne » et le passage automatique à « inactif » ne s'écrivent pas, ils viennent des
--    connexions ouvertes ;
--  * l'adresse e-mail en attente de confirmation (l'ancienne reste valable jusqu'à la
--    confirmation) et, sur le jeton envoyé, l'adresse qu'il confirme ;
--  * l'historique des changements de pseudo, pour en limiter la fréquence.
alter table users add column presence text not null default 'online'
  check (presence in ('online', 'idle', 'dnd', 'invisible'));
alter table users add column pending_email text check (char_length(pending_email) <= 254);
alter table auth_tokens add column email text;

create table username_changes (
  user_id uuid not null references users(id) on delete cascade,
  changed_at timestamptz not null default now()
);
create index username_changes_recent on username_changes (user_id, changed_at desc);
