# Architecture : KRAKEN (reconstruction libre des fonctions de Discord, Nitro inclus)

Date : 2026-10-17. Sources : `recon.md`, `features.csv`. Tout en Rust, un seul dépôt, une seule base, un seul binaire serveur auto-hébergeable.

## Stack

| couche | choix | pourquoi |
| --- | --- | --- |
| serveur | **axum 0.8 + tokio**, binaire unique `kraken-server` | standard Rust pour HTTP + WebSocket, un process suffit jusqu'à des milliers de connectés |
| temps réel | WebSocket `/gateway` dans le même binaire, fan-out `tokio::sync::broadcast` par serveur | pas de broker à installer ; NATS seulement le jour où il faut plusieurs nœuds |
| voix / vidéo / écran | **SFU maison sur `str0m`** (WebRTC sans I/O, Rust), crate `kraken-voice`, lancé dans le même binaire ou à part (`--mode voice`) | libre, pas de LiveKit en Go ; simulcast pour le 1080p60 gratuit |
| E2EE vocal | MLS (`openmls`) + SFrame dans les flux | standard ouvert, équivalent du protocole de Discord |
| bruit / écho | `nnnoiseless` (RNNoise en Rust) + AEC du navigateur | remplace Krisp sous licence |
| base | **PostgreSQL 18** via **sqlx** (requêtes vérifiées à la compilation, migrations) | `uuidv7()` natif, recherche plein texte, file de tâches ; une seule base |
| client | **Leptos** (WASM, CSR) partagé web / desktop / mobile | UI en Rust, types partagés avec le serveur (`kraken-proto`) |
| desktop + mobile | **Tauri 2** | coque Rust légère autour du client Leptos ; iOS et Android dans la même chaîne |
| styles | CSS natif à variables (jetons de `/replica-design`) | pas de Node dans la chaîne de build |
| auth | maison : argon2id, jetons de session opaques hachés, TOTP (`totp-rs`) | auto-hébergeable sans service tiers |
| paiements | **aucun** | tout est gratuit, Nitro et boosts compris |
| e-mail | SMTP via `lettre`, facultatif | l'admin d'instance branche le fournisseur qu'il veut |
| tâches | table `jobs` + workers tokio (`FOR UPDATE SKIP LOCKED`) | durable, sans Redis |
| fichiers | crate `object_store` : disque local par défaut, S3 compatible (Garage, MinIO, R2) | une interface, deux déploiements |
| médias | `image` pour les miniatures, `ffmpeg` externe pour la vidéo | miniatures en tâche de fond |
| recherche | Postgres FTS (`tsvector` + GIN) ; Tantivy plus tard si besoin | rien à installer en plus |
| push | Web Push (VAPID) + UnifiedPush ; FCM/APNs si l'instance fournit ses clés | libre par défaut |
| hébergement | image Docker unique + `docker-compose.yml` (kraken, postgres, garage facultatif) ; instance publique sur un VPS | l'auto-hébergement est une fonction « must » |
| CI | GitHub Actions : `cargo fmt`, `clippy`, `test`, `sqlx prepare --check` | |
| licence | **AGPL-3.0-or-later** (proposé) | garde les forks hébergés libres |

Dépôt (workspace Cargo) :

```
crates/kraken-proto    types REST + gateway (serde), partagés client/serveur
crates/kraken-core     domaine : moteur de permissions, markdown, règles (sans I/O)
crates/kraken-db       sqlx, migrations/ (0001 = replica/schema.sql)
crates/kraken-server   axum : REST, gateway, workers, fichiers ; binaire
crates/kraken-voice    SFU str0m, signalisation, MLS
crates/kraken-client   Leptos (WASM)
crates/kraken-desktop  Tauri 2 (desktop + mobile)
```

## Schéma

Tables : **34**, dans `replica/schema.sql` (testé : création, contraintes, déclencheurs, recherche). Accès : **vérifiés dans la couche données Rust**, pas de RLS, car le serveur est le seul client de la base. Chaque handler passe par un extracteur `Authorized<Perm>` qui calcule les droits avant la requête.

- Ids `uuidv7()` : triés par date, ils servent de curseur (`before`, `after`) comme les snowflakes.
- Tables de liaison (`members`, `member_roles`, `reactions`, ...) : clé composite, pas d'`id`.
- Éphémère, en mémoire seulement : présence, états vocaux, saisie en cours.
- Suppression de compte : anonymisation (`deleted_at`), les messages restent sous « Utilisateur supprimé » (`author_id on delete set null` si purge).
- Supprimer un serveur ou un salon supprime tout son contenu (`cascade`) ; un propriétaire ne peut pas être supprimé tant qu'il possède un serveur (`restrict`).

Contraintes dures portées par la base :

| risque | réponse |
| --- | --- |
| double envoi d'un message (réseau, reconnexion) | index unique `(author_id, nonce)` |
| deux MP pour la même paire | `channels.dm_key` unique (`idA:idB` trié) |
| invitation utilisée au-delà de `max_uses` | `update ... set uses = uses + 1 where uses < max_uses and not expirée returning` + `check (uses <= max_uses)` |
| double demande d'ami, double réaction | clés primaires composites |
| droit à la fois accordé et refusé | `check (allow & deny = 0)` |
| pseudo pris | index unique partiel sur `username` |
| message trop long | `check (char_length(content) <= 4000)` |

Permissions (`kraken-core`, bitfield 64 bits) : `VIEW_CHANNEL, SEND_MESSAGES, SEND_IN_THREADS, CREATE_THREADS, EMBED_LINKS, ATTACH_FILES, ADD_REACTIONS, MENTION_EVERYONE, MANAGE_MESSAGES, READ_HISTORY, CONNECT, SPEAK, VIDEO, STREAM, USE_SOUNDBOARD, MUTE_MEMBERS, MOVE_MEMBERS, PRIORITY_SPEAKER, MANAGE_CHANNELS, MANAGE_ROLES, MANAGE_EMOJIS, MANAGE_EVENTS, KICK, BAN, TIMEOUT, MANAGE_GUILD, VIEW_AUDIT_LOG, CREATE_INVITE, CHANGE_NICK, MANAGE_NICKS, ADMINISTRATOR`. Calcul : `@everyone` ∪ rôles → surcharge `@everyone` du salon → surcharges des rôles (deny puis allow) → surcharge du membre. Propriétaire et `ADMINISTRATOR` court-circuitent. Les fonctions « Nitro » ne sont **pas** des permissions : elles sont actives pour tous, seules des limites d'instance existent (`KRAKEN_MAX_UPLOAD_MB=500`, `KRAKEN_MAX_GUILDS=200`, ...).

## API

REST sous `/api/v1`, JSON, jeton `Authorization: Session <t>` (ou `Bot <t>`). Qui : **anon**, **user** (connecté), **membre+X** (membre du serveur avec la permission X), **proprio**, **admin** (de l'instance). Les entrées et sorties sont les types de `kraken-proto`.

| méthode chemin | fait | qui | entrée | sortie | flow |
| --- | --- | --- | --- | --- | --- |
| POST /auth/register | crée le compte, envoie la vérif | anon | username, email, password | session | F12 |
| POST /auth/login | ouvre une session | anon | login, password, code MFA? | session ou `mfa_required` | F12 |
| POST /auth/logout | révoque la session courante | user | — | 204 | F12 |
| POST /auth/verify-email, /auth/reset, /auth/reset/confirm | jetons e-mail | anon | token, password | 204 | F12 |
| GET/PATCH /users/@me | lire, modifier compte et profil | user | champs profil, password actuel pour e-mail/mdp | user | S06, F08 |
| POST/DELETE /users/@me/mfa | activer, désactiver TOTP | user | code | codes de secours | S06 |
| GET /users/@me/sessions, DELETE /…/{id} | appareils, révocation | user | — | sessions | S06 |
| GET/PATCH /users/@me/settings | réglages synchronisés | user | patch + `version` | settings | S07–S14 |
| POST /users/@me/export | demande d'export RGPD | user | — | 202 (job) | S07 |
| POST /users/@me/disable, /users/@me/delete | désactiver, supprimer | user | password | 204 | S07 |
| GET /users/{id}/profile | profil (global + par serveur) | user | guild_id? | profil | S20 |
| GET /users/@me/relationships | amis, en attente, bloqués | user | — | liste | S01 |
| POST /users/@me/relationships | demande d'ami par pseudo | user | username | relation | F02 |
| PUT/DELETE /users/@me/relationships/{id} | accepter, bloquer, retirer | user | kind | relation | F02, F11 |
| GET /users/@me/channels, POST | liste MP, ouvrir MP ou groupe | user | recipients[] | channel | F03 |
| PUT/DELETE /channels/{id}/recipients/{uid} | groupe : ajouter, retirer | membre du groupe | — | 204 | S19 |
| POST /guilds | crée serveur (+ @everyone, salons du modèle) | user | name, icon, template | guild | F05 |
| GET/PATCH/DELETE /guilds/{id} | lire, modifier, supprimer | membre / MANAGE_GUILD / proprio | champs | guild | S22 |
| GET /guilds/{id}/channels, POST, PATCH (positions) | salons | membre / MANAGE_CHANNELS | salon | channels | S05, S23 |
| PATCH/DELETE /channels/{id} | modifier salon, mode lent, sujet | MANAGE_CHANNELS | champs | channel | S23 |
| PUT/DELETE /channels/{id}/permissions/{target} | surcharges | MANAGE_ROLES | allow, deny | 204 | F07 |
| GET/POST/PATCH/DELETE /guilds/{id}/roles[/{rid}] | rôles, ordre | MANAGE_ROLES (sous son rang) | name, color, perms, position | role(s) | F07 |
| GET /guilds/{id}/members?after= | membres paginés | membre | — | membres | S05 |
| PATCH /guilds/{id}/members/{uid} | pseudo, rôles, exclusion temp. | MANAGE_NICKS / MANAGE_ROLES / TIMEOUT | champs | membre | F07 |
| PATCH /guilds/{id}/members/@me | profil par serveur | membre | nick, avatar, bio | membre | F08 |
| DELETE /guilds/{id}/members/{uid} | expulser (ou quitter si @me) | KICK | reason | 204 | S22 |
| PUT/DELETE /guilds/{id}/bans/{uid} | bannir, débannir | BAN | reason, purge_s | 204 | S22 |
| POST /channels/{id}/invites, GET /guilds/{id}/invites | créer, lister | CREATE_INVITE / MANAGE_GUILD | max_age, max_uses | invite | F05 |
| GET /invites/{code} | aperçu | anon | — | serveur, nb membres | F06 |
| POST /invites/{code} | rejoindre | user non banni | — | guild | F06 |
| GET /channels/{id}/messages?before=&after=&limit= | historique | VIEW_CHANNEL+READ_HISTORY | — | messages | F01 |
| POST /channels/{id}/messages | envoyer (mode lent, AutoMod, mentions) | SEND_MESSAGES | content, nonce, reply_to, attachment_ids, sticker_ids | message | F01 |
| PATCH/DELETE /channels/{id}/messages/{mid} | modifier (auteur), supprimer (auteur ou MANAGE_MESSAGES) | — | content | message | F10 |
| PUT/DELETE /channels/{id}/pins/{mid} | épingler | MANAGE_MESSAGES | — | 204 | F10 |
| PUT/DELETE /channels/{id}/messages/{mid}/reactions/{emoji}/@me | réagir, super réaction | ADD_REACTIONS | burst | 204 | F10 |
| POST /channels/{id}/messages/{mid}/threads, POST /channels/{id}/threads | fil depuis un message, post de forum | CREATE_THREADS | name, tags | channel | F10, S24 |
| POST /channels/{id}/attachments | réserver un envoi | ATTACH_FILES | filename, size, mime | id + URL d'upload | F01 |
| PUT /uploads/{id} | flux du fichier (disque local) ou URL S3 présignée | porteur du jeton d'upload | octets | 204 | F01 |
| POST /channels/{id}/ack/{mid} | marquer lu | VIEW_CHANNEL | — | 204 | S02 |
| GET /users/@me/mentions | boîte de réception | user | before | messages | S25 |
| PUT /users/@me/notifications | réglage par serveur ou salon | user | guild/channel, level, muted_until | override | S09 |
| POST /users/@me/push | enregistrer un abonnement push | user | kind, endpoint, keys | 204 | S09 |
| GET /guilds/{id}/messages/search | recherche from: in: has: dates | membre (salons visibles seulement) | q, filtres | résultats | S26 |
| GET/POST/PATCH/DELETE /guilds/{id}/emojis, /stickers, /soundboard | médias perso | MANAGE_EMOJIS | nom, image | objet | S22 |
| POST /channels/{id}/soundboard/{sid} | jouer un son (de tout serveur) | USE_SOUNDBOARD + en vocal | — | 204 | S18 |
| GET/POST/PATCH/DELETE /guilds/{id}/events, PUT …/interest | événements | MANAGE_EVENTS / membre | champs | event | S27 |
| GET /guilds/{id}/audit-log | journal | VIEW_AUDIT_LOG | before, action | entrées | S22 |
| GET/POST/PATCH/DELETE /guilds/{id}/automod | règles | MANAGE_GUILD | trigger, actions | règle | S22 |
| GET /discover?q=&category= | serveurs publics | user | — | serveurs | S30 |
| GET/DELETE /users/@me/connections, /oauth/grants | comptes liés, applis autorisées | user | — | liste | S15 |
| GET /oauth/authorize, POST /oauth/token | OAuth2 code + PKCE pour bots et applis | user / app | client_id, scopes | jeton | S15 |
| POST /applications, POST /applications/{id}/bot | créer appli et bot | user | name | app, jeton bot | S15 |
| GET /media/{key} | sert fichiers (origine séparée, `Content-Disposition`) | selon l'objet | — | octets | — |
| GET/PATCH /admin/instance | limites, inscriptions, SMTP | admin | config | config | — |

**Gateway** `GET /gateway` (WebSocket, JSON, zstd en option). Opcodes : `HELLO` (intervalle), `IDENTIFY` (jeton), `READY` (moi, serveurs, MP, réglages, read states), `HEARTBEAT/ACK`, `RESUME` (session + dernier `seq`, rejoue un tampon de 2 min), `DISPATCH` (événements numérotés), `PRESENCE_UPDATE`, `VOICE_STATE_UPDATE`, `REQUEST_MEMBERS` (liste paresseuse des gros serveurs). Événements : `MESSAGE_CREATE/UPDATE/DELETE`, `REACTION_ADD/REMOVE`, `TYPING_START`, `CHANNEL_*`, `GUILD_*`, `ROLE_*`, `MEMBER_*`, `RELATIONSHIP_*`, `PRESENCE_UPDATE`, `VOICE_STATE_UPDATE`, `VOICE_SERVER_UPDATE`, `SETTINGS_UPDATE`, `READ_STATE_UPDATE`. Chaque envoi est filtré par `VIEW_CHANNEL` du destinataire.

**Voix** : le client envoie `VOICE_STATE_UPDATE{channel_id}` → le serveur vérifie `CONNECT`, la limite du salon, puis répond `VOICE_SERVER_UPDATE{endpoint, token}` → signalisation SDP sur `wss://…/voice` → médias en UDP (ICE, DTLS-SRTP) vers le SFU. Partage d'écran = piste vidéo en simulcast (720p30 / 1080p60 / source). E2EE : groupe MLS par appel, clés transmises via la signalisation, chiffrement SFrame côté client.

Webhooks entrants : aucun (pas de paiement). Sortants : webhooks de salon (`POST /webhooks/{id}/{token}` pour publier) et événements aux applis, avec `X-Kraken-Signature` (HMAC) et `Idempotency-Key`.

Tâches :

| tâche | quand | fait |
| --- | --- | --- |
| `thumbnail` | à chaque upload | miniatures, dimensions, durée, `status = ready` |
| `embed_fetch` | message avec lien | OpenGraph via `reqwest`, garde anti-SSRF, cache 24 h |
| `push` | message avec mention ou MP | Web Push / UnifiedPush selon les réglages |
| `email` | vérif, reset, notifs | SMTP, si configuré |
| `orphan_uploads` | toutes les heures | supprime les pièces jointes non rattachées après 24 h |
| `invites_expire` | toutes les heures | supprime les invitations expirées |
| `export` | à la demande | zip (profil, messages, fichiers) dans le stockage, lien 7 jours |
| `account_purge` | chaque jour | anonymise les comptes après 14 jours de grâce, efface leurs fichiers |
| `event_reminders` | chaque minute | notifie les intéressés 15 min avant |
| `sessions_cleanup` | chaque jour | IP effacées après 90 jours, sessions révoquées supprimées |

Uniquement des API publiques et officielles : OAuth de chaque plateforme pour les comptes liés, rien de l'API privée de Discord.

## Les parties qui mordent

- **Fuseaux horaires** : tout en `timestamptz` UTC ; le client affiche en local (12/24 h de S14) ; les événements récurrents gardent leur fuseau IANA pour l'heure d'été.
- **Idempotence** : `nonce` unique par auteur pour l'envoi ; `RESUME` rejoue par `seq` sans doublon ; `jobs.dedupe_key` ; webhooks sortants avec `Idempotency-Key`.
- **Courses** : invitations (update conditionnel), MP (`dm_key` + `on conflict`), demandes d'ami croisées (transaction qui transforme deux `pending` en `friend`), réorganisation des rôles (transaction unique), compteurs de mentions (`update … set mention_count = mention_count + 1`).
- **Limites de débit** : `governor` par route, par utilisateur et par IP ; `IDENTIFY` limité ; mode lent vérifié en base.
- **Taille des fichiers** : flux direct vers le stockage, jamais en mémoire ; plafond d'instance (500 Mo par défaut) vérifié à la réservation puis au flux.
- **Recherche** : FTS Postgres restreinte aux salons que l'utilisateur voit ; config `simple` + `unaccent` pour le multilingue.
- **Temps réel** : canaux bornés par connexion, un client trop lent est déconnecté puis fait `RESUME` ; liste de membres paresseuse au-delà de 1 000 membres.
- **Hors ligne** : le client garde une file d'envoi avec `nonce` et la vide à la reconnexion.
- **Délivrabilité e-mail** : facultatif ; une instance sans SMTP désactive la vérif ; SPF/DKIM documentés pour l'admin.
- **Multi-locataire** : une instance = un locataire ; le serveur (guild) est l'unité d'isolation, chaque requête est bornée par lui et passe par le moteur de permissions.
- **RGPD** : export, désactivation, suppression avec délai de grâce, anonymisation, rétention IP 90 jours.
- **Sécurité** : markdown rendu sans HTML brut, médias servis depuis une origine séparée, CSP stricte, anti-SSRF sur les aperçus, MFA, argon2id.
- **WebRTC dans les webviews** (déduit, à vérifier tôt) : bon sur Windows (WebView2), partiel sur macOS, fragile sous Linux (WebKitGTK). Plan B : pile média native Rust dans la coque Tauri.

## Ordre de construction

1. **Tranche verticale** (web seulement, moche) : S28 inscription/connexion → S21 créer un serveur → S05 salon texte → message en direct chez un second compte entré par invitation (F05, F06, F01).
   Tables : `users`, `sessions`, `guilds`, `roles`, `members`, `member_roles`, `channels`, `messages`, `invites`. Routes : `/auth/register`, `/auth/login`, `POST /guilds`, `/guilds/{id}/channels`, `/channels/{id}/messages`, `/invites`, `/gateway` (`IDENTIFY`, `READY`, `MESSAGE_CREATE`). Livrable : `docker compose up` qui marche.
2. **Must-haves, tranche 1** (texte) :
   - comptes : S06, S07, suppression RGPD, sessions.
   - social et MP : S01, S03, S19, présence, amis, blocage (F02, F11) ; `relationships`, `channel_recipients`.
   - serveurs et modération : S02, S22, S23, rôles et surcharges (F07), expulsion/ban/exclusion ; `permission_overwrites`, `bans`.
   - messages : modifier, supprimer, répondre, markdown, mentions, spoilers, réactions, aperçus, pièces jointes, emojis perso partout ; `attachments`, `reactions`, `emojis`, `message_mentions`.
   - notifications : non-lus, push, réglages par serveur/salon ; `read_states`, `notification_overrides`, `push_subscriptions`.
   - réglages : S11, S12, S13, S14 (thèmes, a11y, raccourcis, FR/EN) ; `user_settings`.
   - profil : S20 de base (avatar, bio, pronoms).
   - plateformes : coque Tauri desktop, image Docker et doc d'auto-hébergement.
3. **Must-haves, tranche 2** (voix) : S18, S10 ; salon vocal, appel MP (F03, F04), muet/sourdine, périphériques, VAD/PTT, caméra, partage d'écran. `kraken-voice`, états vocaux en mémoire.
4. **Should-haves** : MFA, export, statut perso, règles de demandes d'ami, filtres MP, mode lent, fils (S24), journal d'audit, épingles, envois 500 Mo, texte alternatif, autocollants, boîte de réception (S25), recherche (S26), Quick Switcher (S29), RNNoise, E2EE vocal, 1080p60, soundboard, avatar animé et bannière, thèmes de couleur, synchro des réglages, contraste élevé, raccourcis perso, applis OAuth et bots.
5. **Could-haves** : dossiers et favoris, explorer (S30), forums, AutoMod, filtre média sensible, transfert, super réactions, GIF, e-mails de notif, 384 kbps, arrière-plans vidéo, aperçu du stream, son d'entrée, déco et thème de profil, profil par serveur, styles de pseudo, icônes d'appli, mode streamer, TTS, événements (S27), comptes liés, mobile Tauri.
6. **Correctifs de `/replica-entrepreneur`** : pas encore lancé.
