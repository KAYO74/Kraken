# Journal de construction : KRAKEN

Une ligne par écran. Statut : **fait**, **partiel** (manque listé), **route** (l'écran a son URL et un écran d'attente, contenu à venir). Captures dans `replica/clone-screens/` (1728×1117, plus 390 px), régénérées par `scripts/screens.cjs`.

| ID | date | statut | ce qui manque | plus dur que prévu |
| --- | --- | --- | --- | --- |
| coquille | 2026-10-17 | fait | coque Tauri | pas de `trunk` (téléchargements GitHub bloqués ici) : `scripts/build-client.sh` appelle `wasm-bindgen` directement |
| S02 rail | 2026-10-17 | partiel | non-lus, mentions, dossiers, glisser-déposer | le tiroir mobile : choisir un serveur garde le tiroir ouvert, choisir un salon le ferme |
| S03 barre MP | 2026-10-17 | partiel | nouveau MP, fermer une conversation, présence dans la liste | |
| S04 panneau utilisateur | 2026-10-17 | partiel | micro et casque sont visuels (pas encore de vocal), menu de statut | |
| S28 connexion / inscription | 2026-10-17 | partiel | MFA, QR, mot de passe oublié, vérif e-mail | |
| F06 page d'invitation | 2026-10-17 | fait | | |
| S21 créer / rejoindre | 2026-10-17 | partiel | modèles, icône | |
| S05 salon textuel | 2026-10-17 | partiel | modifier, supprimer, répondre, réactions, pièces jointes, markdown, mentions, non-lus, épingles, pagination vers le haut | l'envoi optimiste (nonce, grisé, échec + Réessayer) et le rechargement après reconnexion |
| S19 MP / groupe | 2026-10-17 | partiel | appel, ajout de membres, profil latéral | |
| S01 Amis | 2026-10-17 | partiel | ajouter, accepter, refuser, bloquer ; panneau d'activité réel | |
| S06 Compte | 2026-10-17 | partiel | modifier pseudo, e-mail, mot de passe ; MFA ; appareils ; suppression | |
| S11 Apparence | 2026-10-17 | partiel | thèmes perso, icônes d'appli, synchro entre appareils | les pastilles de thème demandaient des couleurs hors jetons : remplacées par une liste de radios |
| S07 S08 S09 S10 S12 S13 S14 S15 | 2026-10-17 | route | tout le contenu | |
| S18 S20 S22 S23 S24 S25 S26 S27 S29 S30 | 2026-10-17 | route | tout le contenu | |

## Tranche verticale (F12 → F05 → F06 → F01)

Faite le 2026-10-17, sur la couche données **en mémoire** (`kraken-db`, mêmes signatures que la future version Postgres) :
inscription de deux comptes, création d'un serveur, lien d'invitation, adhésion, message reçu en direct par l'autre compte (gateway), et dans l'autre sens.
Vérifiée par `cargo test -p kraken-server` (HTTP + WebSocket réels) et `e2e/slice.cjs` (deux navigateurs Chromium, zéro erreur console).

## Écarts à savoir

- Données : en mémoire pendant le build, puis PostgreSQL avec `/replica-backend` (même API, aucun écran modifié). La tranche verticale a été rejouée sur Postgres.
- Polices Inter et JetBrains Mono pas encore embarquées : repli sur la police système.
- `.wasm` de 4 Mo sans `wasm-opt` (non installé ici).
- `docker compose` écrit avec le backend ; l'image est construite par la CI, pas ici (pas de démon Docker).
