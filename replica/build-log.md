# Journal de construction : KRAKEN

Une ligne par écran. Statut : **fait**, **partiel** (manque listé), **route** (l'écran a son URL et un écran d'attente, contenu à venir). Captures dans `replica/clone-screens/` (1728×1117, plus 390 px), régénérées par `scripts/screens.cjs`.

| ID | date | statut | ce qui manque | plus dur que prévu |
| --- | --- | --- | --- | --- |
| coquille | 2026-10-17 | fait | coque Tauri | pas de `trunk` (téléchargements GitHub bloqués ici) : `scripts/build-client.sh` appelle `wasm-bindgen` directement |
| S02 rail | 2026-10-17 | partiel | non-lus, mentions, dossiers, glisser-déposer | le tiroir mobile : choisir un serveur garde le tiroir ouvert, choisir un salon le ferme |
| S03 barre MP | 2026-10-17 | partiel | fermer une conversation, présence dans la liste ; le 2026-10-26 « + » ouvre un MP ou crée un groupe | |
| S04 mon compte (bas du rail) | 2026-10-17 | partiel | menu de statut ; le 2026-10-26 le panneau utilisateur devient une carte « Mon compte » au bas du rail, micro et son passent dans le panneau « En appel » | |
| S28 connexion / inscription | 2026-10-17 | partiel | MFA, QR, mot de passe oublié, vérif e-mail | |
| F06 page d'invitation | 2026-10-17 | fait | | |
| S21 créer / rejoindre | 2026-10-17 | partiel | modèles, icône | |
| S05 salon textuel | 2026-10-17 | partiel | modifier, supprimer, répondre, réactions, pièces jointes, markdown, mentions, non-lus, épingles, pagination vers le haut | l'envoi optimiste (nonce, grisé, échec + Réessayer) et le rechargement après reconnexion |
| S19 MP / groupe | 2026-10-17 | partiel | appel, profil latéral ; le 2026-10-26 ajout de membres, renommer, quitter, MP fermé par un blocage | |
| S01 Amis | 2026-10-17 | partiel | panneau d'activité réel ; le 2026-10-26 ajouter, accepter, refuser, annuler, retirer, bloquer, onglet Bloqués | |
| S06 Compte | 2026-10-17 | partiel | modifier pseudo, e-mail, mot de passe ; MFA ; appareils ; suppression | |
| S11 Apparence | 2026-10-17 | partiel | thèmes perso, icônes d'appli, synchro entre appareils | les pastilles de thème demandaient des couleurs hors jetons : remplacées par une liste de radios |
| S08 qui peut t'écrire | 2026-10-26 | partiel | filtres de MP et de demandes d'ami ; comptes bloqués et Débloquer faits | |
| S07 S09 S10 S12 S13 S14 S15 | 2026-10-17 | route | tout le contenu | |
| S18 salon vocal | 2026-10-20 | fait | caméra, choix des périphériques, push-to-talk, aperçu du stream avant de regarder, salons Stage (traités en vocal) | `MediaStream.clone()` de web-sys est la méthode JavaScript (nouvelles pistes), pas `Clone` : le partage ne s'arrêtait pas et le contour « parle » restait muet ; `Clone::clone(&x)` partout |
| S20 S22 S23 S24 S25 S26 S27 S29 S30 | 2026-10-17 | route | tout le contenu | |

## Tranche verticale (F12 → F05 → F06 → F01)

Faite le 2026-10-17, sur la couche données **en mémoire** (`kraken-db`, mêmes signatures que la future version Postgres) :
inscription de deux comptes, création d'un serveur, lien d'invitation, adhésion, message reçu en direct par l'autre compte (gateway), et dans l'autre sens.
Vérifiée par `cargo test -p kraken-server` (HTTP + WebSocket réels) et `e2e/specs/f05-f06-invite.spec.ts` (deux navigateurs Chromium, zéro erreur console).

## Salons vocaux et partage d'écran (S18, 2026-10-20)

Client : une session vocale globale (`voice.rs`) et ses écrans (`call.rs`). Vérifié en vrai contre `kraken-server --seed` (UDP sur 127.0.0.1) avec deux Chromium à micro et écran factices (`--use-fake-ui-for-media-stream --use-fake-device-for-media-stream`, qui suffisent aussi à `getDisplayMedia`) : chacun voit et entend l'autre (niveau mesuré par WebAudio), l'écran partagé n'arrive qu'à qui regarde (1440×810, environ 17 i/s reçues), micro et son coupés vus de l'autre côté, fin de partage par le bouton du navigateur, page sans micro (écoute et regarde), micro refusé (entre en sourdine), vocal indisponible (503), connexion reprise par un autre onglet puis « Reconnecter » ; axe sans violation avant et pendant l'appel.

## Contrôle à distance (S18, 2026-10-26)

Ajout propre à KRAKEN (l'original n'en a pas). Dans un appel, qui regarde un écran clique « Demander le contrôle » ; le serveur (`kraken-voice`) relaie demande, réponse, actions et fin par le canal de données, seulement entre un spectateur de cet écran et celui qui le partage, dans le même salon ; seul le spectateur accepté envoie des actions (120 par seconde, rafales de 240, le reste jeté) ; tout s'arrête avec le partage, le « ne plus regarder », un départ ou une coupure. Côté partage, seule l'appli de bureau (0.4.0) joue les actions (`apps/desktop/src/remote.rs`, enigo) : commandes réservées à l'origine de l'instance (capacité ajoutée au lancement), fenêtre native « Autoriser ? » à chaque fois, jeton par session, fin au menu « Arrêter le contrôle à distance » (Ctrl+Alt+Maj+K), au rechargement de la page ou à la fermeture. Un navigateur qui partage refuse d'office (« il faut l'appli de bureau »).

Vérifié : `kraken-voice/tests/roundtrip.rs` (règles du relais, en vrai sur UDP), tests unitaires (positions dans l'image, touches, choix de l'écran), `e2e/specs/remote-control.spec.ts` (refus d'office dans un navigateur ; appli de bureau simulée par un faux `invoke` : demande, accord, actions reçues dans l'ordre, Échap, arrêt par l'appli, par l'un ou l'autre, refus, fin du partage). Pas vérifiable ici : la fenêtre native et la saisie simulée sur un vrai bureau (pas d'écran), macOS et Windows.

Limites : le navigateur ne dit pas quel écran il partage (écran de même taille, sinon de même rapport, sinon principal) ; Wayland refusé ; sous Windows, les fenêtres lancées en administrateur ne reçoivent rien.

## Messages complets (S05, F10, 2026-10-26)

Modifier son message (crayon, ou flèche haut dans une saisie vide ; Entrée enregistre, Échap annule, « (édité) »), le supprimer après confirmation (son auteur, ou qui a `MANAGE_MESSAGES` : propriétaire, rôle Modération), répondre (citation au-dessus, clic qui y mène en chargeant l'historique plus ancien au besoin, « message d'origine supprimé » ensuite), réactions Unicode (sélecteur de 86 emojis du système avec recherche en français, compteur, qui a réagi au survol, 20 emojis différents au plus, 300 par minute et par client), markdown (gras, italique, souligné, barré, code, blocs, citations, liens `http(s)` détectés, spoilers dévoilés au clic ou au clavier) et mentions (`@pseudo` surligné pour qui est visé, `@everyone`/`@here` seulement avec `MENTION_EVERYONE`, `#salon` en lien, autocomplétion `@` et `#`). Un envoi raté se réessaie ou se supprime. Le markdown est analysé dans `kraken-core` (le serveur y lit les mentions, le client le dessine en nœuds DOM, jamais en HTML) ; événements `MESSAGE_UPDATE`, `MESSAGE_DELETE`, `REACTION_ADD`, `REACTION_REMOVE`. Le schéma avait déjà les colonnes et tables : pas de migration.

Vérifié : tests `kraken-core` (markdown, emojis), `kraken-db` (`edit_delete_reply_react_and_mention`), `kraken-server/tests/slice.rs` (`edit_delete_reply_and_react` : second compte refusé en 403, inconnu en 404, événements reçus en direct), `e2e/specs/f10-messages.spec.ts` (11 parcours, dont modération par le rôle de démo et axe).

Limites : les boutons du client tiennent compte des rôles, pas des surcharges de salon (le serveur, lui, les applique) ; `@here` vaut `@everyone` tant que la présence ne compte pas dans les notifications ; pas encore d'emojis perso, de super réactions ni de notifications de mention.

## Social (S01, S03, S08, S19 ; F02, F03, F11 ; 2026-10-26)

Amis : demande par pseudo unique (avec ou sans `@`, casse ignorée) depuis l'onglet « Ajouter un ami », avec une réponse claire dans chaque cas (pseudo inconnu, le sien, déjà amis, déjà demandé, compte qu'on a bloqué, compte qui refuse) ; demandes reçues en tête de « En attente », annoncées sur l'onglet et sur le lien Amis ; accepter, refuser, annuler, retirer un ami (après confirmation). Deux demandes croisées font une amitié (verrou de transaction sur la paire). Bloquer (après confirmation) défait l'amitié ou les demandes des deux côtés sans prévenir l'autre ; le compte bloqué ne peut plus écrire dans le MP, en ouvrir un, ni redemander ; ses messages sont repliés derrière « Afficher » dans les salons et groupes partagés ; débloquer depuis l'onglet « Bloqués », le MP ou Réglages › Qui peut t'écrire. MP et groupes : « + » dans la barre des MP ouvre un choix d'amis (recherche, compteur) : un ami ouvre le MP, plusieurs créent un groupe (10 personnes au plus, nom facultatif) ; un MP s'ouvre entre amis ou membres d'un même serveur ; dans un groupe, tout membre renomme et ajoute ses amis, chacun le quitte, le propriétaire retire (API) et passe la main en partant. Onglets d'amis au clavier (flèches, Début, Fin). Événements `RELATIONSHIP_ADD/UPDATE/REMOVE`, `CHANNEL_CREATE/UPDATE/DELETE` (le salon tel que chacun le voit) ; la présence n'est plus donnée qu'aux amis. Demandes, blocages, nouveaux MP et ajouts à un groupe : limite de `KRAKEN_AUTH_PER_MIN` par minute et par client, compteur à part. Démo : `zed`, bloqué par `demo`, a un message replié dans #général. Pas de migration : `relationships`, `channels.dm_key` et `channel_recipients` existaient.

Vérifié : `kraken-db` (`friends_requests_and_blocks`, `direct_and_group_messages` : demandes croisées simultanées, blocage dans les deux sens, groupe de 11 refusé, propriétaire remplacé, groupe vide supprimé), `kraken-server/tests/social.rs` (trois et cinq comptes : erreurs et codes, événements reçus en direct, compte bloqué refusé en MP et en demande pendant qu'un tiers écrit normalement, intrus en 404, non-propriétaire en 403, limite en 429), `e2e/specs/f02-f11-social.spec.ts` (10 parcours, axe sur les onglets, les dialogues et le fil replié) et `f02-f03-home.spec.ts` mis à jour.

Limites : « ignorer » et les réglages de qui peut écrire ou demander (S08) pas encore ; pas de fermeture d'un MP ; retirer quelqu'un d'un groupe passe par l'API seulement ; pas de message système « X a rejoint le groupe ».

## Appels en MP et en groupe (S19, F03, F04 ; 2026-10-27)

Appeler sans serveur, depuis un MP, un groupe, ou le bouton « Appeler » d'une ligne d'ami (qui ouvre le MP et y lance l'appel). Pas de pile parallèle : la conversation sert de salon au SFU (`POST /channels/{id}/voice`, `DELETE /voice`), avec les mêmes pistes, le même canal de données et donc micro, son coupé, partage d'écran regardé et contrôle à distance (appli de bureau) tels quels. Les états vocaux d'un appel ont `guild_id` à `null` et ne vont qu'aux participants de la conversation. Entrer dans une conversation sans appel en cours le lance : un message d'appel est posté (`kind = 'call'`), et ça sonne chez les autres participants, sauf ceux qui ont un blocage avec l'appelant ; un MP avec un compte bloqué (dans un sens ou dans l'autre) refuse l'appel (403 `call_blocked` / `call_refused`), un non-participant reçoit 404. La sonnerie s'affiche sur toute page (en bas à droite, « Rejoindre » / « Refuser », deux notes synthétisées par WebAudio, aucun fichier son), s'arrête quand on décroche, refuse (`POST /channels/{id}/call/decline`, même limite que les autres routes, compteur à part) ou après 30 s (`KRAKEN_RING_SECONDS`). L'appel s'affiche en haut de la conversation (tuiles, écran regardé, barre de commandes avec « Raccrocher » ; ou, pour qui n'y est pas, qui y est et « Rejoindre l'appel »), avec une icône et la liste des présents sous la conversation dans la barre latérale ; le panneau du bas dit « Appel avec … ». Il prend fin quand le dernier part : le message d'appel garde qui y est entré et sa durée ; qui ne l'a pas pris lit « Appel manqué de … », l'appelant resté seul « Personne n'a répondu à ton appel. ». Événements `CALL_UPDATE` (qui fait sonner) et `CALL_DELETE`, appels et présents dans `READY`. Retiré d'un groupe pendant l'appel : on en sort. Migration `0004_calls.sql` (participants et fin d'appel sur le message) ; au démarrage, les appels restés ouverts sont marqués terminés.

Vérifié : `kraken-voice/tests/roundtrip.rs` (`private_calls` : appels sans serveur, passage d'un appel à l'autre ou à un salon), `kraken-server/tests/call.rs` (intrus en 404, sans session en 401, message d'appel, sonnerie, `READY`, décrocher, revenir, fin et participants, blocage dans les deux sens en 403 sans message laissé ; groupe : blocage exclu de la sonnerie, refus, fin de sonnerie au délai, retiré du groupe pendant l'appel, nouvel appel ensuite), `e2e/specs/calls.spec.ts` (MP depuis Amis : sonnerie, Rejoindre, son entendu des deux côtés, micro coupé vu, écran partagé regardé, contrôle à distance accepté, raccrocher puis revenir, trace dans le fil ; groupe à trois : deux décrochent, un refuse puis rejoint, trois tuiles ; sonnerie sans réponse arrêtée après 30 s, « Appel manqué », blocage et intrus refusés ; axe sur la sonnerie, l'appel et le fil).

Limites : pas de caméra (comme les salons) ; pas de « refaire sonner » pendant un appel ; la sonnerie peut rester muette tant que la page n'a reçu aucun clic (règle d'autoplay du navigateur) ; pas de notification système hors de l'onglet.

## Écarts à savoir

- Données : en mémoire pendant le build, puis PostgreSQL avec `/replica-backend` (même API, aucun écran modifié). La tranche verticale a été rejouée sur Postgres.
- Polices Inter et JetBrains Mono pas encore embarquées : repli sur la police système.
- `.wasm` de 4 Mo sans `wasm-opt` (non installé ici).
- `docker compose` écrit avec le backend ; l'image est construite par la CI, pas ici (pas de démon Docker).
