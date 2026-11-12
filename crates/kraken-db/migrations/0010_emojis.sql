-- Emojis perso de serveur. La table `emojis` existe depuis 0001 (nom de 2 à 32 caractères, image
-- gardée sous `image_key`, unique par (serveur, nom)). Il manque :
--  * un nom unique sans tenir compte de la casse : `Blob` et `blob` désignent le même emoji pour qui
--    écrit `:blob:`. La contrainte de 0001, sensible à la casse, devient inutile : l'index ci-dessous
--    refuse tout ce qu'elle refusait ;
--  * un index sur le serveur, pour les listes (le plus ancien d'abord : l'id est un UUID v7, trié
--    par date) et le décompte (50 au plus par serveur).
alter table emojis drop constraint emojis_guild_id_name_key;
create unique index emojis_guild_lower_name_idx on emojis (guild_id, lower(name));
create index emojis_guild_idx on emojis (guild_id, id);
