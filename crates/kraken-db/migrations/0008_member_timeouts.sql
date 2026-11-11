-- Exclusion temporaire gardée à la sortie : quelqu'un qui quitte un serveur pendant qu'il est
-- exclu retrouve son exclusion, jusqu'à son terme, s'il y revient par une invitation. Une ligne
-- par (serveur, personne) ; elle est consommée au retour, effacée par un renvoi ou un
-- bannissement, et part avec le serveur ou le compte.
create table member_timeouts (
  guild_id uuid not null references guilds(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  until timestamptz not null,
  primary key (guild_id, user_id)
);
