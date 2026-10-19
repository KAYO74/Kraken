-- KRAKEN : schéma PostgreSQL 18 (uuidv7() natif). Première migration.
-- Règles : ids uuidv7 (triables par date, servent de curseur de pagination),
-- temps en timestamptz UTC, accès vérifié dans la couche données Rust (pas de RLS :
-- le serveur est le seul client de la base). Tables de liaison : clé composite.

create extension if not exists unaccent;

create or replace function set_updated_at() returns trigger language plpgsql as $$
begin new.updated_at = now(); return new; end $$;

-- ───────────── comptes ─────────────

create table users (
  id uuid primary key default uuidv7(),
  username text not null check (username ~ '^[a-z0-9_.]{2,32}$'),
  display_name text check (char_length(display_name) <= 32),
  email text,
  email_verified_at timestamptz,
  phone text,
  password_hash text,                       -- argon2id ; null pour les bots
  mfa_secret bytea,                         -- TOTP chiffré (clé de l'instance)
  mfa_recovery_hashes text[] not null default '{}',
  is_bot boolean not null default false,
  is_instance_admin boolean not null default false,
  avatar_key text, banner_key text, avatar_decoration text,
  bio text check (char_length(bio) <= 190),
  pronouns text check (char_length(pronouns) <= 40),
  accent_color int, profile_theme jsonb, name_style jsonb,
  entrance_sound_id uuid,                   -- fk ajoutée après soundboard_sounds
  locale text not null default 'fr',
  disabled_at timestamptz,                  -- désactivation volontaire
  deletion_requested_at timestamptz,        -- RGPD : purge après délai de grâce
  deleted_at timestamptz,                   -- compte anonymisé
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create unique index on users (username) where deleted_at is null;
create unique index on users (lower(email)) where email is not null;

-- une session = un « appareil connecté » (S06)
create table sessions (
  id uuid primary key default uuidv7(),
  user_id uuid not null references users(id) on delete cascade,
  token_hash bytea not null unique,         -- sha256 du jeton opaque
  name text not null, platform text not null,
  ip inet, last_seen_at timestamptz not null default now(),
  revoked_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on sessions (user_id) where revoked_at is null;

create table auth_tokens (                  -- vérif e-mail, reset mot de passe
  id uuid primary key default uuidv7(),
  user_id uuid not null references users(id) on delete cascade,
  kind text not null check (kind in ('verify_email','reset_password','change_email')),
  token_hash bytea not null unique,
  expires_at timestamptz not null,
  used_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on auth_tokens (user_id);

create table user_settings (                -- synchronisé entre appareils (S06–S14)
  user_id uuid primary key references users(id) on delete cascade,
  privacy jsonb not null default '{}', notifications jsonb not null default '{}',
  voice jsonb not null default '{}', appearance jsonb not null default '{}',
  accessibility jsonb not null default '{}', keybinds jsonb not null default '[]',
  guild_layout jsonb not null default '{"folders":[],"favorites":[]}',
  time_format text not null default 'auto' check (time_format in ('auto','12h','24h')),
  friend_requests_from text not null default 'everyone'
    check (friend_requests_from in ('everyone','friends_of_friends','server_members','nobody')),
  version bigint not null default 0,        -- concurrence optimiste multi-appareils
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create table push_subscriptions (           -- Web Push / UnifiedPush / FCM / APNs
  id uuid primary key default uuidv7(),
  session_id uuid not null references sessions(id) on delete cascade,
  kind text not null check (kind in ('webpush','unifiedpush','fcm','apns')),
  endpoint text not null, keys jsonb not null default '{}',
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique (session_id, endpoint)
);

-- ───────────── social ─────────────

-- une ligne par sens : amitié = deux lignes 'friend'
create table relationships (
  user_id uuid not null references users(id) on delete cascade,
  target_id uuid not null references users(id) on delete cascade,
  kind text not null check (kind in ('friend','pending_in','pending_out','blocked','ignored')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (user_id, target_id),
  check (user_id <> target_id)
);
create index on relationships (target_id);

create table connections (                  -- comptes liés via OAuth public (S15)
  id uuid primary key default uuidv7(),
  user_id uuid not null references users(id) on delete cascade,
  provider text not null, external_id text not null, display_name text not null,
  show_on_profile boolean not null default true, show_details boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique (user_id, provider, external_id)
);

-- ───────────── serveurs ─────────────

create table guilds (
  id uuid primary key default uuidv7(),
  owner_id uuid not null references users(id) on delete restrict,  -- transférer avant suppression
  name text not null check (char_length(name) between 2 and 100),
  icon_key text, banner_key text, description text,
  vanity_code text unique,
  verification_level smallint not null default 0 check (verification_level between 0 and 4),
  explicit_filter smallint not null default 0 check (explicit_filter between 0 and 2),
  discoverable boolean not null default false,
  system_channel_id uuid,                   -- fk ajoutée après channels
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on guilds (owner_id);
create index on guilds (discoverable) where discoverable;

-- le rôle @everyone a id = guild_id
create table roles (
  id uuid primary key default uuidv7(),
  guild_id uuid not null references guilds(id) on delete cascade,
  name text not null check (char_length(name) between 1 and 100),
  color int, icon_key text,
  position int not null default 0,
  permissions bigint not null default 0,    -- bitfield 64 bits (kraken-core::Permissions)
  hoist boolean not null default false, mentionable boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on roles (guild_id, position);

create table members (
  guild_id uuid not null references guilds(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  nick text check (char_length(nick) <= 32),
  avatar_key text, banner_key text, bio text,          -- profil par serveur
  entrance_sound_id uuid,
  timeout_until timestamptz,
  joined_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (guild_id, user_id)
);
create index on members (user_id);

create table member_roles (
  guild_id uuid not null, user_id uuid not null,
  role_id uuid not null references roles(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (guild_id, user_id, role_id),
  foreign key (guild_id, user_id) references members(guild_id, user_id) on delete cascade
);
create index on member_roles (role_id);

create table bans (
  guild_id uuid not null references guilds(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  moderator_id uuid references users(id) on delete set null,
  reason text,
  created_at timestamptz not null default now(),
  primary key (guild_id, user_id)
);

-- ───────────── salons ─────────────

create table channels (
  id uuid primary key default uuidv7(),
  guild_id uuid references guilds(id) on delete cascade,     -- null pour MP
  parent_id uuid references channels(id) on delete set null, -- catégorie ou salon d'un fil
  kind text not null check (kind in
    ('text','voice','announcement','category','forum','stage','dm','group_dm','thread')),
  name text check (char_length(name) <= 100),
  topic text check (char_length(topic) <= 1024),
  position int not null default 0,
  nsfw boolean not null default false,
  slowmode_s int not null default 0 check (slowmode_s between 0 and 21600),
  bitrate int check (bitrate between 8000 and 384000),
  user_limit smallint check (user_limit between 0 and 99),
  owner_id uuid references users(id) on delete set null,     -- créateur du groupe ou du fil
  icon_key text,
  dm_key text unique,                       -- 'uuidA:uuidB' trié : un seul MP par paire
  archived_at timestamptz, locked boolean not null default false,  -- fils
  applied_tags uuid[] not null default '{}',                       -- posts de forum
  last_message_id uuid,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  check ((kind in ('dm','group_dm')) = (guild_id is null)),
  check (kind <> 'dm' or dm_key is not null)
);
create index on channels (guild_id, position);
create index on channels (parent_id);
alter table guilds add foreign key (system_channel_id) references channels(id) on delete set null;

create table channel_recipients (           -- membres d'un MP ou groupe (max 10)
  channel_id uuid not null references channels(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  closed boolean not null default false,    -- masqué de la barre MP
  created_at timestamptz not null default now(),
  primary key (channel_id, user_id)
);
create index on channel_recipients (user_id);

create table permission_overwrites (
  channel_id uuid not null references channels(id) on delete cascade,
  target_id uuid not null,                  -- role_id ou user_id
  target_kind text not null check (target_kind in ('role','member')),
  allow bigint not null default 0, deny bigint not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (channel_id, target_id),
  check (allow & deny = 0)
);

create table thread_members (
  channel_id uuid not null references channels(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (channel_id, user_id)
);
create index on thread_members (user_id);

create table forum_tags (
  id uuid primary key default uuidv7(),
  channel_id uuid not null references channels(id) on delete cascade,
  name text not null, emoji text, moderated boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on forum_tags (channel_id);

-- ───────────── messages ─────────────

create table messages (
  id uuid primary key default uuidv7(),
  channel_id uuid not null references channels(id) on delete cascade,
  author_id uuid references users(id) on delete set null,   -- null = utilisateur supprimé
  kind text not null default 'default' check (kind in
    ('default','reply','system_join','system_pin','thread_created','call','forward')),
  content text not null default '' check (char_length(content) <= 4000),
  nonce text,                               -- idempotence de l'envoi client
  reply_to_id uuid references messages(id) on delete set null,
  forwarded_from_id uuid references messages(id) on delete set null,
  embeds jsonb not null default '[]',
  mention_everyone boolean not null default false,
  mention_roles uuid[] not null default '{}',
  pinned_at timestamptz,
  flags int not null default 0,             -- suppress_embeds, silent, ...
  edited_at timestamptz,
  search tsvector generated always as (to_tsvector('simple', content)) stored,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on messages (channel_id, id desc);
create index on messages (channel_id, author_id, id desc);   -- mode lent, from:
create index on messages (author_id);
create index on messages (reply_to_id) where reply_to_id is not null;
create index on messages (channel_id) where pinned_at is not null;
create unique index on messages (author_id, nonce) where nonce is not null;
create index on messages using gin (search);
alter table channels add foreign key (last_message_id) references messages(id) on delete set null;

create table message_mentions (             -- boîte de réception (S25), compteurs
  message_id uuid not null references messages(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (message_id, user_id)
);
create index on message_mentions (user_id, message_id desc);

-- envoi en deux temps : upload (message_id null) puis rattachement ; orphelins purgés par job
create table attachments (
  id uuid primary key default uuidv7(),
  uploader_id uuid references users(id) on delete set null,
  message_id uuid references messages(id) on delete cascade,
  filename text not null, mime text not null,
  size_bytes bigint not null check (size_bytes > 0),
  storage_key text not null unique,
  width int, height int, duration_ms int,
  spoiler boolean not null default false,
  alt_text text check (char_length(alt_text) <= 1024),
  status text not null default 'pending' check (status in ('pending','ready','failed')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on attachments (message_id);
create index on attachments (uploader_id);
create index on attachments (created_at) where message_id is null;

create table emojis (
  id uuid primary key default uuidv7(),
  guild_id uuid not null references guilds(id) on delete cascade,
  creator_id uuid references users(id) on delete set null,
  name text not null check (name ~ '^[A-Za-z0-9_]{2,32}$'),
  animated boolean not null default false, image_key text not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique (guild_id, name)
);

create table stickers (
  id uuid primary key default uuidv7(),
  guild_id uuid not null references guilds(id) on delete cascade,
  name text not null, description text,
  format text not null check (format in ('png','apng','gif','lottie')),
  image_key text not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on stickers (guild_id);

-- emoji_key : caractère unicode, ou id d'emoji perso (utilisable partout)
create table reactions (
  message_id uuid not null references messages(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  emoji_key text not null,
  burst boolean not null default false,     -- super réaction
  created_at timestamptz not null default now(),
  primary key (message_id, emoji_key, burst, user_id)
);
create index on reactions (user_id);

-- ───────────── voix ─────────────
-- VoiceState et Presence vivent en mémoire (kraken-server), pas en base.

create table soundboard_sounds (
  id uuid primary key default uuidv7(),
  guild_id uuid references guilds(id) on delete cascade,     -- null = son de l'instance
  creator_id uuid references users(id) on delete set null,
  name text not null, emoji text,
  volume real not null default 1 check (volume between 0 and 1),
  file_key text not null, duration_ms int not null check (duration_ms <= 5200),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on soundboard_sounds (guild_id);
alter table users add foreign key (entrance_sound_id) references soundboard_sounds(id) on delete set null;
alter table members add foreign key (entrance_sound_id) references soundboard_sounds(id) on delete set null;

-- ───────────── invitations, lecture, notifs ─────────────

create table invites (
  code text primary key check (code ~ '^[A-Za-z0-9]{6,16}$'),
  guild_id uuid not null references guilds(id) on delete cascade,
  channel_id uuid references channels(id) on delete cascade,
  inviter_id uuid references users(id) on delete set null,
  max_uses int not null default 0 check (max_uses >= 0),     -- 0 = illimité
  uses int not null default 0,
  temporary boolean not null default false,
  expires_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  check (max_uses = 0 or uses <= max_uses)
);
create index on invites (guild_id);
create index on invites (expires_at) where expires_at is not null;

create table read_states (
  user_id uuid not null references users(id) on delete cascade,
  channel_id uuid not null references channels(id) on delete cascade,
  last_read_message_id uuid,
  mention_count int not null default 0,
  updated_at timestamptz not null default now(),
  primary key (user_id, channel_id)
);
create index on read_states (channel_id);

-- guild_id seul = réglage du serveur ; channel_id = réglage du salon
create table notification_overrides (
  id uuid primary key default uuidv7(),
  user_id uuid not null references users(id) on delete cascade,
  guild_id uuid references guilds(id) on delete cascade,
  channel_id uuid references channels(id) on delete cascade,
  level text not null default 'default' check (level in ('default','all','mentions','none')),
  suppress_everyone boolean not null default false,
  muted_until timestamptz,                  -- 'infinity' = muet sans fin
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  check (guild_id is not null or channel_id is not null)
);
create unique index on notification_overrides
  (user_id, coalesce(guild_id, '00000000-0000-0000-0000-000000000000'),
            coalesce(channel_id, '00000000-0000-0000-0000-000000000000'));

-- ───────────── événements, modération ─────────────

create table scheduled_events (
  id uuid primary key default uuidv7(),
  guild_id uuid not null references guilds(id) on delete cascade,
  channel_id uuid references channels(id) on delete set null,
  creator_id uuid references users(id) on delete set null,
  name text not null, description text, location text, image_key text,
  start_at timestamptz not null, end_at timestamptz,
  timezone text not null,                   -- IANA, pour les récurrences
  recurrence jsonb,
  status text not null default 'scheduled' check (status in ('scheduled','active','completed','cancelled')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  check (end_at is null or end_at > start_at)
);
create index on scheduled_events (guild_id, start_at);

create table event_interests (
  event_id uuid not null references scheduled_events(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (event_id, user_id)
);
create index on event_interests (user_id);

create table audit_log (
  id uuid primary key default uuidv7(),
  guild_id uuid not null references guilds(id) on delete cascade,
  actor_id uuid references users(id) on delete set null,
  action text not null, target_id uuid, changes jsonb not null default '{}', reason text,
  created_at timestamptz not null default now()
);
create index on audit_log (guild_id, id desc);

create table automod_rules (
  id uuid primary key default uuidv7(),
  guild_id uuid not null references guilds(id) on delete cascade,
  name text not null, enabled boolean not null default true,
  trigger jsonb not null, actions jsonb not null,
  exempt_roles uuid[] not null default '{}', exempt_channels uuid[] not null default '{}',
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on automod_rules (guild_id);

-- ───────────── applications et bots (API publique) ─────────────

create table oauth_apps (
  id uuid primary key default uuidv7(),
  owner_id uuid not null references users(id) on delete cascade,
  bot_user_id uuid unique references users(id) on delete set null,
  name text not null, icon_key text, description text,
  client_secret_hash bytea not null,
  redirect_uris text[] not null default '{}',
  public boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on oauth_apps (owner_id);

create table oauth_grants (
  id uuid primary key default uuidv7(),
  user_id uuid not null references users(id) on delete cascade,
  app_id uuid not null references oauth_apps(id) on delete cascade,
  scopes text[] not null,
  guild_id uuid references guilds(id) on delete cascade,     -- ajout de bot à un serveur
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique (user_id, app_id)
);
create index on oauth_grants (app_id);

-- ───────────── tâches de fond (file Postgres, FOR UPDATE SKIP LOCKED) ─────────────

create table jobs (
  id uuid primary key default uuidv7(),
  kind text not null,                       -- thumbnail, embed_fetch, push, email, export, purge...
  payload jsonb not null,
  dedupe_key text unique,                   -- idempotence
  run_at timestamptz not null default now(),
  attempts int not null default 0, last_error text,
  locked_until timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
create index on jobs (run_at) where locked_until is null;

-- updated_at automatique partout où la colonne existe
do $$ declare t text; begin
  for t in select table_name from information_schema.columns
           where table_schema = 'public' and column_name = 'updated_at' loop
    execute format('create trigger set_updated_at before update on %I
                    for each row execute function set_updated_at()', t);
  end loop;
end $$;
