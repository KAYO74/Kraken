-- Une tâche en lettre morte ne bloque plus sa clé : la même tâche peut être remise en file
-- (mot de passe oublié après une panne SMTP, purge des comptes).
alter table jobs drop constraint jobs_dedupe_key_key;
create unique index jobs_dedupe_live on jobs (dedupe_key) where failed_at is null;

-- Le nonce d'envoi dédoublonne dans un salon : réutilisé ailleurs, c'est un autre message.
drop index messages_author_id_nonce_idx;
create unique index messages_nonce on messages (author_id, channel_id, nonce) where nonce is not null;
