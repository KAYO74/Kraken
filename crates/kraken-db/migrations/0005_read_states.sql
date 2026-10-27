-- Non-lus et réglages de notification : une ligne par serveur, une par salon (le salon garde
-- aussi son serveur, null pour un MP), mises à jour en place.
create unique index notification_overrides_guild on notification_overrides (user_id, guild_id)
  where channel_id is null;
create unique index notification_overrides_channel on notification_overrides (user_id, channel_id)
  where channel_id is not null;
