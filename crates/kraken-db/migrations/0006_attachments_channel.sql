-- Fichiers joints : un envoi (message_id null) appartient déjà à un salon, dont les droits
-- décident de qui peut le lire ; `preview` dit qu'une copie sans métadonnées existe pour l'image.
alter table attachments add column channel_id uuid references channels(id) on delete cascade;
alter table attachments add column preview boolean not null default false;
create index on attachments (channel_id);
