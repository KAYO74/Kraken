-- Journal d'audit : lire les entrées d'une seule personne (`?actor=`) et la liste de ceux qui ont agi
-- dans un serveur sans parcourir tout le journal, qu'on garde sans limite. L'index de 0001
-- (guild_id, id desc) sert déjà la page « tout », par curseur.
create index audit_log_guild_actor_idx on audit_log (guild_id, actor_id, id desc);
