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
| licence | **AGPL-3.0-or-later** (choisie, `LICENSE`) | garde libres les forks hébergés ; `license = "AGPL-3.0-or-later"` dans chaque `Cargo.toml` |

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
- Journal d'audit (`audit_log`) : une ligne écrite **dans la transaction** de l'action (jamais après coup), `changes` en JSON. Ce que `changes` garde est décidé action par action : avant / après `{old, new}` pour un rôle, un salon (sujet, mode lent, privé), les droits d'un salon, plus le nom de la cible au moment de l'écriture (`target_name`) pour que la ligne reste lisible quand la cible disparaît ; l'inviteur et le salon d'une invitation ; le salon d'un message supprimé par un modérateur (jamais son texte) ; le salon et le message d'une épingle ajoutée ou retirée (`message_pin`, `message_unpin`, cible = l'auteur du message ; rien pour un MP, rien quand l'épingle ne change pas). L'auteur supprimé donne `actor_id` à `null` (`on delete set null`) et les lignes restent. Aucune purge par ancienneté (il part avec le serveur). Lecture : `GET /guilds/{id}/audit-log`. `guild_update` : `PATCH /guilds/{id}` écrit `changes` `name` avant / après ; `PUT` et `DELETE /guilds/{id}/icon` écrivent le détail `icon` (`changed` ou `removed`, jamais la clé du fichier).

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
| GET/PATCH /users/@me | lire, modifier le profil (nom affiché, bio 190, pronoms 40 ; champ absent = inchangé, vide = effacé ; sans caractère invisible Unicode (Default_Ignorable_Code_Point et U+2800), 400 `invalid_chars`, sauf U+200D et U+FE0F ; un texte sans caractère visible, 400 `invisible_text`) ; le PATCH partage le compteur de limite de `PUT /users/@me/presence` (429 `rate_limited` + `Retry-After`) | user | champs profil | user | S20, F08 |
| PUT/DELETE /users/@me/avatar, /users/@me/banner | photo et bannière (GIF/WebP animés gardés tels quels), servis sous /avatars et /banners ; image fixe refusée d'après son en-tête au-delà de 4096 px de côté ou 16 millions de pixels (400 `image_too_big`) ; 2 décodages à la fois sur tout le serveur, partagés avec les pièces jointes et les images d'aperçus ; en-tête lu avec 64 Mio de mémoire au plus. Une pièce jointe de plus de 8192 px de côté ou 40 millions de pixels (jugée sur l'en-tête) reste un fichier ordinaire : ni décodage, ni permis, ni vignette | user | image | user | S20 |
| PUT /users/@me/username, /email, /password | changer de pseudo (2 par heure, 429 `username_cooldown` ; noms réservés everyone, here, admin, administrator, moderator, mod, root, system, kraken, support, staff refusés aussi à l'inscription, 400 `username_reserved`), d'adresse (confirmée par e-mail si SMTP, l'ancienne reste valide) et de mot de passe (les autres appareils sont déconnectés) | user + mot de passe actuel (403 `bad_password`) | champs | user, compte, 204 | S06 |
| PUT /users/@me/presence | statut choisi : en ligne, inactif, ne pas déranger, invisible (gardé en base, 0009) | user | presence | presence | S04 |
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
| POST /guilds | crée serveur (+ @everyone) et la structure du modèle choisi : `template` parmi `vide` (défaut, absent = `vide`), `communaute`, `jeux`, `etudes`, `amis` (liste fixe de `kraken_core::templates`, partagée avec le client) ; un identifiant inconnu : 400 `invalid` (`template`), rien n'est créé. Le premier salon textuel du modèle est le salon système. Aucun rôle en plus de @everyone | user | name, template | guild | F05, S21 |
| GET/DELETE /guilds/{id} | lire, supprimer | membre / proprio | — | guild | S22 |
| PATCH /guilds/{id} | renommer (mêmes règles de nom qu'à la création : 2 à 100 caractères, rien d'invisible ; un nom inchangé n'écrit rien au journal) | MANAGE_GUILD / proprio | name | guild | S22 |
| PUT/DELETE /guilds/{id}/icon | icône du serveur : **le corps est l'image** (comme une photo de profil : PNG, JPEG, WebP, GIF reconnus à leurs premiers octets), 4 Mio au plus, côté de 8 px au moins (400 `image_too_small`), mêmes gardes de dimensions et de décodages que les photos. Image fixe : recadrée au centre en carré, réduite à 256 px, ré-encodée en PNG ; GIF / WebP animé : gardé tel quel. Fichier `KRAKEN_DATA_DIR/avatars/{clé}.{png,gif,webp}` (supprimé avec l'ancienne icône et avec le serveur). Retirer une icône absente n'écrit rien au journal. 60 écritures par minute et par client | MANAGE_GUILD / proprio | image | guild | S22 |
| GET /guild-icons/{clé} | l'image de l'icône, publique (un message ou une invitation montre le serveur à qui n'en est pas membre), `public, max-age=31536000, immutable` (la clé change avec l'image) | — | — | image | S02 |
| GET /guilds/{id}/channels, POST, PATCH (positions) | salons | membre / MANAGE_CHANNELS | salon | channels | S05, S23 |
| PATCH/DELETE /channels/{id} | modifier salon, mode lent, sujet | MANAGE_CHANNELS | champs | channel | S23 |
| PUT/DELETE /channels/{id}/permissions/{target} | surcharges | MANAGE_ROLES | allow, deny | 204 | F07 |
| GET/POST/PATCH/DELETE /guilds/{id}/roles[/{rid}] | rôles, ordre | MANAGE_ROLES (sous son rang) | name, color, perms, position | role(s) | F07 |
| GET /guilds/{id}/members?after= | membres paginés | membre | — | membres | S05 |
| PATCH /guilds/{id}/members/{uid} | pseudo, rôles, exclusion temp. | MANAGE_NICKS / MANAGE_ROLES / TIMEOUT | champs | membre | F07 |
| PATCH /guilds/{id}/members/@me | profil par serveur | membre | nick, avatar, bio | membre | F08 |
| DELETE /guilds/{id}/members/{uid} | expulser (ou quitter si @me) | KICK | reason | 204 | S22 |
| PUT/DELETE /guilds/{id}/bans/{uid} | bannir, débannir | BAN | reason, purge_s | 204 | S22 |
| POST /channels/{id}/invites, GET /guilds/{id}/invites | créer (400 `invite_limit` au-delà de 50 actives par personne et par serveur), lister (200 lignes au plus, les actives d'abord) | CREATE_INVITE / MANAGE_GUILD | max_age, max_uses | invite | F05 |
| GET /invites/{code} | aperçu | anon | — | serveur, nb membres | F06 |
| POST /invites/{code} | rejoindre (une exclusion temporaire gardée à un départ est rétablie, à la même échéance) | user non banni | — | guild | F06 |
| DELETE /invites/{code} | révoquer (suppression ; la trace reste dans `audit_log`) | créateur / MANAGE_GUILD | — | 204 | F06 |
| PUT/DELETE /guilds/{id}/members/{uid}/timeout | exclusion temporaire (60 s à 28 j), la lever ; 403 `timed_out` sur écrire, modifier ses messages, réagir, joindre, parler ; un exclu non administrateur n'a plus que voir et lire, donc plus aucun droit de modérer, gérer ou inviter (403) ; quitter ne la lève pas (table `member_timeouts`, rétablie au retour par invitation, effacée par un renvoi ou un bannissement) | TIMEOUT (sous son rang) | seconds, reason | membre | S22 |
| GET /channels/{id}/messages?before=&after=&limit= | historique | VIEW_CHANNEL+READ_HISTORY | — | messages | F01 |
| POST /channels/{id}/messages | envoyer (mode lent, AutoMod, mentions) ; un emoji perso s'écrit **`<:nom:id>`** dans `content` (`id` : UUID avec tirets) : le serveur garde le texte tel quel, le nom n'est là que pour le lire et pour l'afficher si l'emoji n'existe plus, c'est l'id qui compte. Le client écrit `:nom:` dans la saisie (celui du serveur du salon d'abord, puis ceux de ses autres serveurs) et le remplace par le jeton à l'envoi, à la modification aussi (sauf dans le code, dans un lien `http(s)://…` et derrière une barre inverse : `\:nom:` s'affiche `:nom:`) ; l'aperçu d'une réponse (200 caractères) retire en entier un jeton que la coupe tranche ; un message qui n'est fait que d'emojis perso (27 au plus) s'affiche en grand. Résumés, recherche et notifications lisent `:nom:` | SEND_MESSAGES | content, nonce, reply_to, attachment_ids, sticker_ids | message | F01 |
| PATCH/DELETE /channels/{id}/messages/{mid} | modifier (auteur, sauf exclu temporairement : 403 `timed_out`), supprimer (auteur, même exclu, ou MANAGE_MESSAGES) | — | content | message | F10 |
| PUT/DELETE /channels/{id}/pins/{mid} | épingler, désépingler (`messages.pinned_at`) : 200 et le message mis à jour (`pinned`) ; sans effet, ni événement ni audit, quand rien ne change ; 50 par salon (400 `pin_limit`, un verrou par salon tient la limite sous la concurrence) ; seuls les messages écrits (400 `pin_kind` pour un appel) ; un message d'un autre salon : 404 ; chaque changement : `MESSAGE_UPDATE` ; audit `message_pin` / `message_unpin` (salon et message, jamais le texte ; rien en MP). 60 écritures par minute et par client | MANAGE_MESSAGES dans un serveur ; tout participant en MP ou groupe | — | message | F10 |
| GET /channels/{id}/pins | les messages épinglés, le plus récemment épinglé d'abord, au format de l'historique (réactions, pièces jointes, aperçus) ; 50 au plus | VIEW_CHANNEL+READ_HISTORY | — | messages | S05 |
| PUT/DELETE /channels/{id}/messages/{mid}/reactions/{emoji}/@me | réagir ; `{emoji}` est un emoji Unicode de la liste, ou l'id (UUID écrit avec tirets, en minuscules) d'un emoji perso d'un serveur dont l'auteur de la réaction est membre (sinon 400 `emoji`) ; 20 emojis différents au plus par message. Les réactions d'un emoji retiré restent en base (le client écrit « emoji supprimé ») ; on ne peut plus qu'en retirer la sienne. Super réaction : plus tard | ADD_REACTIONS | burst | 204 | F10 |
| POST /channels/{id}/messages/{mid}/threads, POST /channels/{id}/threads | fil depuis un message, post de forum | CREATE_THREADS | name, tags | channel | F10, S24 |
| POST /channels/{id}/attachments | réserver un envoi | ATTACH_FILES | filename, size, mime | id + URL d'upload | F01 |
| PUT /uploads/{id} | flux du fichier (disque local) ou URL S3 présignée | porteur du jeton d'upload | octets | 204 | F01 |
| POST /channels/{id}/ack/{mid} | marquer lu (jamais en arrière) | VIEW_CHANNEL | — | read state | S02 |
| POST /guilds/{id}/ack | tout le serveur lu | membre | — | read states | S02 |
| GET /users/@me/mentions | boîte de réception | user | before | messages | S25 |
| PUT /users/@me/notifications | réglage par serveur ou salon | membre / VIEW_CHANNEL | guild/channel, level, mute, suppress_everyone | override | S09 |
| POST /users/@me/push | enregistrer un abonnement push | user | kind, endpoint, keys | 204 | S09 |
| GET /guilds/{id}/messages/search | recherche from: in: has: dates | membre (salons visibles seulement) | q, filtres | résultats | S26 |
| GET /guilds/{id}/emojis, GET /users/@me/emojis | les emojis du serveur (membres), ou tous ceux que je peux utiliser (serveurs dont je suis membre, du plus ancien au plus récent) ; la fiche : `id`, `guild_id`, `name`, `animated`, `creator` (id + nom). `GuildState.emojis` les porte aussi (READY, GET /guilds/{id}) | membre | — | emojis | S22 |
| POST /guilds/{id}/emojis?name= | ajouter : **le corps est l'image** (comme une photo de profil), reconnue à ses premiers octets (PNG, JPEG, WebP, GIF ; 400 `not_image`), 256 Kio au plus (413 `too_large`, sans lire le reste), nom de 2 à 32 caractères `[A-Za-z0-9_]` (400 `invalid_name`), unique sans tenir compte de la casse (409 `name_taken`), 50 par serveur (400 `emoji_limit`). Droit, nom et place sont jugés avant tout décodage. Même garde de dimensions que les photos (4096 px de côté, 16 millions de pixels, sur l'en-tête ; 2 décodages à la fois sur tout le serveur). Forme jugée elle aussi sur l'en-tête, avant tout décodage, fixe ou animée : petit côté de 8 px au moins (400 `image_too_small`), grand côté de 8 fois le petit au plus (400 `bad_ratio`). Image fixe : recadrée au centre en carré, réduite à 128 px (jamais agrandie), ré-encodée en PNG. GIF / WebP animé (`animated` est décidé par le serveur) : gardé tel quel, sans ses métadonnées, rapport largeur / hauteur de 2 pour 1 au plus (400 `bad_ratio`). Fichier `KRAKEN_DATA_DIR/emojis/{clé}.{png,gif,webp}`. 60 écritures par minute et par client | MANAGE_EMOJIS | image + `name` | emoji (201) | S22 |
| PATCH/DELETE /guilds/{id}/emojis/{eid} | renommer (l'id et l'image ne changent pas), retirer (le fichier part ; les messages et réactions qui le citent restent) | MANAGE_EMOJIS | name | emoji / 204 | S22 |
| GET /emojis/{eid}/image | l'image, pour toute session ouverte (une balise `<img>` porte le cookie) : un emoji s'utilise dans tous les serveurs de l'auteur et se lit chez tous ceux qui lisent le message, membres du serveur ou non ; l'id est un UUID. `Cache-Control: private, max-age=86400` (un jour, sans `immutable`) : un navigateur peut garder l'image d'un emoji retiré au plus un jour | session | — | image | S22 |
| GET/POST/PATCH/DELETE /guilds/{id}/stickers, /soundboard | médias perso | MANAGE_EMOJIS | nom, image | objet | S22 |
| POST /channels/{id}/soundboard/{sid} | jouer un son (de tout serveur) | USE_SOUNDBOARD + en vocal | — | 204 | S18 |
| GET/POST/PATCH/DELETE /guilds/{id}/events, PUT …/interest | événements | MANAGE_EVENTS / membre | champs | event | S27 |
| GET /guilds/{id}/audit-log | le journal, du plus récent au plus ancien. Réponse `AuditPage` : `entries`, `next` (l'id de la dernière entrée si une page plus ancienne existe, à repasser en `before`) et, **sur la première page seulement**, `actors` (celles et ceux qui ont agi, sans le filtre, 200 au plus, pour la liste « Membre »). Une entrée : `id` (UUID v7, c'est le curseur), `created_at` (ms), `actor` (`{id, name, avatar}`, `null` si le compte n'existe plus), `action`, `target` (`{kind: user|role|channel|emoji, id, name}`, `name` à `null` quand la cible n'existe plus et que son nom n'a pas été gardé ; `null` pour une invitation, qui n'a pas d'id), `reason`, `details` (les faits de la phrase, en texte : `role`, `seconds`, `kind`, `channel`, `max_uses`, `max_age_s`, `animated`, `inviter`, `allow`, `deny`, `subject`, `subject_kind`) et `changes` (avant / après : `{field, before, after}` pour `name`, `topic`, `slowmode_s`, `private`, `color`, `hoist`, `permissions`). Une liste blanche par action décide de ce qui sort : jamais le code d'une invitation, jamais le texte d'un message supprimé ; les noms de la cible, de l'auteur et des rôles ou salons cités se lisent au moment de la requête, à défaut le nom gardé à l'écriture. Paramètres : `before` (UUID), `limit` (50 par défaut, 1 à 100, ramené dans les bornes), `category` (`membres`, `roles`, `salons`, `invitations`, `emojis`, `serveur`), `actor` (UUID) ; un paramètre mal formé : 400 `invalid`. Non-membre : 404 ; membre sans le droit : 403 `perm`. Les administrateurs et le propriétaire lisent. 120 lectures par minute et par client (429 au-delà). Index `audit_log_guild_actor_idx` (migration 0011) pour le filtre `actor` | VIEW_AUDIT_LOG | before, limit, category, actor | `AuditPage` | S22 |
| GET/POST/PATCH/DELETE /guilds/{id}/automod | règles | MANAGE_GUILD | trigger, actions | règle | S22 |
| GET /discover?q=&category= | serveurs publics | user | — | serveurs | S30 |
| GET/DELETE /users/@me/connections, /oauth/grants | comptes liés, applis autorisées | user | — | liste | S15 |
| GET /oauth/authorize, POST /oauth/token | OAuth2 code + PKCE pour bots et applis | user / app | client_id, scopes | jeton | S15 |
| POST /applications, POST /applications/{id}/bot | créer appli et bot | user | name | app, jeton bot | S15 |
| GET /media/{key} | sert fichiers (origine séparée, `Content-Disposition`) | selon l'objet | — | octets | — |
| GET/PATCH /admin/instance | limites, inscriptions, SMTP | admin | config | config | — |

**Gateway** `GET /gateway` (WebSocket, JSON, zstd en option). Opcodes : `HELLO` (intervalle), `IDENTIFY` (jeton), `READY` (moi, serveurs, MP, réglages, read states), `HEARTBEAT/ACK`, `RESUME` (session + dernier `seq`, rejoue un tampon de 2 min), `DISPATCH` (événements numérotés), `PRESENCE_UPDATE`, `VOICE_STATE_UPDATE`, `REQUEST_MEMBERS` (liste paresseuse des gros serveurs). Événements : `MESSAGE_CREATE/UPDATE/DELETE`, `REACTION_ADD/REMOVE`, `TYPING_START`, `CHANNEL_*`, `GUILD_*`, `EMOJIS_UPDATE` (la liste complète des emojis du serveur, aux membres : ajout, renommage, retrait), `ROLE_*`, `MEMBER_*`, `RELATIONSHIP_*`, `PRESENCE_UPDATE`, `VOICE_STATE_UPDATE`, `VOICE_SERVER_UPDATE`, `SETTINGS_UPDATE`, `READ_STATE_UPDATE`, `NOTIFY_SETTINGS_UPDATE`. Chaque envoi est filtré par `VIEW_CHANNEL` du destinataire.

**Voix** : le client envoie `VOICE_STATE_UPDATE{channel_id}` → le serveur vérifie `CONNECT`, la limite du salon, puis répond `VOICE_SERVER_UPDATE{endpoint, token}` → signalisation SDP sur `wss://…/voice` → médias en UDP (ICE, DTLS-SRTP) vers le SFU. Partage d'écran = piste vidéo en simulcast (720p30 / 1080p60 / source). Chaque navigateur offre quatre transceivers `sendonly`, dans cet ordre : micro, image de l'écran, son de l'écran, caméra (le dernier, pour garder l'ordre des trois premiers) ; il ne renégocie jamais (`replaceTrack`), seul le SFU fait des offres. La caméra (`VoiceState.camera`, flux `<user>.camera`) n'attend pas de `watch` : le SFU la relaie à tout le salon dès qu'elle est annoncée (droit STREAM requis), contrairement à l'écran qu'il n'envoie qu'à qui le regarde ; chaque piste vidéo relayée a ses propres demandes d'image clé. E2EE : groupe MLS par appel, clés transmises via la signalisation, chiffrement SFrame côté client.

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
- **Raccourcis et Quick Switcher** : côté client seulement, rien au serveur. `crates/kraken-client/src/shortcuts.rs` : un tableau `SHORTCUTS` (libellé, groupe, touches par plateforme) sert à brancher (un seul écouteur de fenêtre, jamais de touche seule dans un champ) et à afficher l'aide (`?`, Ctrl/Cmd+/, Réglages › Raccourcis clavier). `kraken_core::search` classe et surligne (exact, début, début de mot, contient, lettres dans l'ordre ; sans casse ni accent), `kraken_core::grid` déplace le focus dans la grille d'emojis, `kraken_core::keys` donne le nom d'une touche d'après `key` et `code` (claviers russe et grec : la touche physique). Récents du Quick Switcher : clé `kraken.recents` du `localStorage` (identifiants seuls, 20 au plus), effacée à la déconnexion (`Store::sign_out`).
- **Messages épinglés** : `messages.pinned_at` (index partiel par salon) ; `Db::set_pinned` prend un verrou consultatif par salon pour que la limite de 50 tienne sous la concurrence, n'écrit ni événement ni audit quand rien ne change, et écrit l'audit dans la même transaction (serveur seulement). Côté client, `Store.pins` ne garde que les listes des panneaux ouverts, que `MESSAGE_UPDATE` et `MESSAGE_DELETE` tiennent à jour ; le panneau se relit à chaque ouverture et après une reconnexion. Pas de message système « X a épinglé » (`system_pin` est dans le schéma, rien ne l'écrit encore).
- **Langues de l'interface** : le français écrit dans le code est la clé de traduction (`t("Ouvrir les réglages")` ; `tf` pour les `{variables}`, `tn` pour les pluriels) ; `crates/kraken-client/i18n/en/<groupe>.tsv` donne l'anglais, un texte absent reste en français. `kraken_core::i18n` (logique pure et testée : `Lang`, `LangPref`, `parse_catalog`, règle de pluriel, `fill`) et `src/i18n.rs` (lecture de `kraken.lang`, `navigator.languages`, `<html lang dir>`, `LangSelect`). La langue est lue une fois au démarrage : le changement enregistre et recharge la page (drapeau `sessionStorage` : focus et phrase d'état après, identifiant gardé sur /login). Dates et heures : `Intl` avec la langue de l'interface (`i18n::locale()`, chiffres latins), 12/24 h prioritaire. Les messages d'erreur du serveur restent français et statiques : `api.rs` les traduit à l'endroit unique où `ApiError` est lu (`i18n/en/server.tsv`). Contrôle : `scripts/i18n-check.py`. Noms en écritures non latines : le ZWNJ passe entre deux lettres (`kraken_core::text::has_hidden`). Le droit-à-gauche est prêt (voir ci-dessous) ; aucune traduction de droite à gauche n'est encore livrée.
- **Droit-à-gauche (arabe, persan, hébreu)** : `<html dir>` (posé par `boot.js` d'après la langue, `i18n::is_rtl()` côté Rust, lu à chaque événement) pilote tout. Les feuilles de style n'emploient que des propriétés logiques (`margin-inline-start`, `inset-inline-end`, `text-align: start`, `border-start-start-radius`…) ; ce qui ne se convertit pas (tiroirs, ombres décalées, quarts de tour) passe par `--flip` (1, et -1 sous `[dir=rtl]`), les icônes qui indiquent un sens par `k-flip` (`icons.rs` : liste `DIRECTIONAL`). Texte bidirectionnel : `dir="auto"` sur le texte libre, `<bdi dir="ltr">` ou `dir="ltr"` sur les pseudos, e-mails, liens et codes, `pre` et `code` toujours de gauche à droite, isolats FSI/PDI dans `tf` quand la page est de droite à gauche. Polices du système (jetons `font.arabic`, `font.hebrew`), interligne `--line-scale`. Contrôle : `python3 scripts/css-logical-check.py` (échoue sur toute propriété physique sans `/* physical-ok: raison */` sur la ligne), exécuté par la CI à côté de `replica/design/tokens.py` ; géométrie vérifiée par `e2e/specs/rtl.spec.ts`.
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
