# Journal de construction : KRAKEN

Une ligne par écran. Statut : **fait**, **partiel** (manque listé), **route** (l'écran a son URL et un écran d'attente, contenu à venir). Captures dans `replica/clone-screens/` (1728×1117, plus 390 px), régénérées par `scripts/screens.cjs`.

| ID | date | statut | ce qui manque | plus dur que prévu |
| --- | --- | --- | --- | --- |
| coquille | 2026-10-17 | fait | coque Tauri | pas de `trunk` (téléchargements GitHub bloqués ici) : `scripts/build-client.sh` appelle `wasm-bindgen` directement |
| S02 rail | 2026-10-17 | partiel | non-lus, mentions, dossiers, glisser-déposer | le tiroir mobile : choisir un serveur garde le tiroir ouvert, choisir un salon le ferme |
| S03 barre MP | 2026-10-17 | partiel | nouveau MP, fermer une conversation, présence dans la liste | |
| S04 mon compte (bas du rail) | 2026-10-17 | partiel | menu de statut ; le 2026-10-26 le panneau utilisateur devient une carte « Mon compte » au bas du rail, micro et son passent dans le panneau « En appel » | |
| S28 connexion / inscription | 2026-10-17 | partiel | MFA, QR, mot de passe oublié, vérif e-mail | |
| F06 page d'invitation | 2026-10-17 | fait | | |
| S21 créer / rejoindre | 2026-10-17 | partiel | modèles, icône | |
| S05 salon textuel | 2026-10-17 | partiel | modifier, supprimer, répondre, réactions, pièces jointes, markdown, mentions, non-lus, épingles, pagination vers le haut | l'envoi optimiste (nonce, grisé, échec + Réessayer) et le rechargement après reconnexion |
| S19 MP / groupe | 2026-10-17 | partiel | appel, ajout de membres, profil latéral | |
| S01 Amis | 2026-10-17 | partiel | ajouter, accepter, refuser, bloquer ; panneau d'activité réel | |
| S06 Compte | 2026-10-17 | partiel | modifier pseudo, e-mail, mot de passe ; MFA ; appareils ; suppression | |
| S11 Apparence | 2026-10-17 | partiel | thèmes perso, icônes d'appli, synchro entre appareils | les pastilles de thème demandaient des couleurs hors jetons : remplacées par une liste de radios |
| S07 S08 S09 S10 S12 S13 S14 S15 | 2026-10-17 | route | tout le contenu | |
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

## Écarts à savoir

- Données : en mémoire pendant le build, puis PostgreSQL avec `/replica-backend` (même API, aucun écran modifié). La tranche verticale a été rejouée sur Postgres.
- Polices Inter et JetBrains Mono pas encore embarquées : repli sur la police système.
- `.wasm` de 4 Mo sans `wasm-opt` (non installé ici).
- `docker compose` écrit avec le backend ; l'image est construite par la CI, pas ici (pas de démon Docker).
