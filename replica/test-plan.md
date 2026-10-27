# Plan de test : KRAKEN

Build : c313fe7  Date : 2026-10-17  Env : local, PostgreSQL 16 (CI : 18), données de démo (`--seed`), Chromium 1440×900 et 390×844

Automatisé = `e2e/specs/*.spec.ts` (Playwright). Chaque test échoue sur une erreur console, une réponse 5xx, et passe axe (WCAG 2.1 AA) sur les écrans clés. Lancer : `cd e2e && npm ci && npx playwright test` (serveur démarré avec `KRAKEN_AUTH_PER_MIN=10000`, sinon la limite anti-abus bloque les inscriptions en série). Côté serveur, `cargo test` couvre les mêmes règles sans navigateur (unicité, droits, sessions, file de tâches).

Résultat : 40 cas automatisés, 40 passés (et 120/120 sur trois passes d'affilée). 5 bugs trouvés, 5 corrigés (voir `bugs.md`).

| cas | parcours | type | étapes | attendu | auto | résultat |
| --- | --- | --- | --- | --- | --- | --- |
| F01-H1 | F01 message | nominal | créer un serveur, écrire, Entrée, recharger | message affiché une fois, toujours là après rechargement | e2e | ok |
| F01-E1 | | accents, emoji, Maj+Entrée | « Ça déménage 🐙 », deux lignes | texte intact, retour à la ligne gardé | e2e | ok |
| F01-E2 | | vide | espaces puis Envoyer | rien n'est envoyé | e2e | ok |
| F01-E3 | | très long | 4001 caractères puis 4000 | compteur, envoi bloqué ; 4000 passe sans déborder | e2e | ok |
| F01-E4 | | double clic | double clic sur Envoyer | un seul message | e2e | ok |
| F01-E5 | | deux onglets | même compte, deux onglets | le message apparaît dans les deux | e2e | ok |
| F01-E6 | | hors ligne | couper le réseau, écrire, rétablir, Réessayer | erreur visible, un seul message au final | e2e | ok |
| F01-E7 | | réponse perdue | le serveur enregistre, la réponse se perd, Réessayer | pas de doublon | e2e | ok (BUG-004) |
| F01-E8 | | clavier seul | Tab depuis le lien d'évitement jusqu'au champ | message envoyé sans souris | e2e | ok |
| F01-E9 | | mobile 390 px | tiroir, serveur, salon, écrire | tiroir refermé, pas de défilement horizontal, axe ok | e2e | ok (BUG-005) |
| F01-N1 | | données d'un autre | non-membre : URL du salon, API lecture, écriture, invitation | 404 partout, rien affiché | e2e | ok |
| F01-N2 | | salon privé | nouveau membre d'Abysse | #staff absent pour lui, visible pour l'équipe | e2e | ok (BUG-003) |
| F01-E10 | | mode lent, lecture seule, pièce jointe | | | non construit | bloqué |
| F02-H1 | F02 amis | nominal (liste) | onglets En ligne, Tous, En attente | compteurs justes, demande reçue visible | e2e | ok |
| F02-E1 | | filtre | « LOU », puis un nom inconnu | insensible à la casse, 0 sans erreur | e2e | ok |
| F02-E2 | | vide | nouveau compte | états vides expliqués | e2e | ok |
| F02-H3 | | onglets au clavier | flèches, Début, Fin | choix et focus suivent | e2e | ok |
| F02-H2 | | envoyer, accepter | pseudo de l'autre, en majuscules avec @ | reçue en direct, amis des deux côtés | e2e | ok |
| F02-E3 | | erreurs | inconnu, le sien, en double, déjà amis | message clair, champ marqué invalide | e2e | ok |
| F02-E4 | | refuser, annuler, retirer | | l'autre côté suit en direct ; retrait confirmé | e2e | ok |
| F03-H1 | F03 MP | nominal | Tous → Écrire à Lou → message | MP ouvert, message envoyé | e2e | ok |
| F03-E1 | | MP de groupe | ouvrir « Équipée du soir » | historique affiché | e2e | ok |
| F03-N1 | | MP d'autrui | URL et API du MP Démo↔Lou par un tiers | 404, rien affiché | e2e | ok |
| F03-H2 | | groupe | 2 amis, nom, renommer, ajouter, quitter | tout le groupe suit en direct | e2e | ok |
| F03-H3 | | nouveau MP | choix d'un ami, puis groupe depuis le MP | MP créé, l'autre le reçoit | e2e | ok |
| F03-N2 | | refus | groupe de 11, MP avec un inconnu | 400, 403 | e2e | ok |
| F03-H4 | | appel | | | non construit | bloqué |
| F04-H1 | F04 vocal et partage d'écran | nominal | deux comptes dans un serveur neuf : Rejoindre, partager, Regarder, micro et son, arrêter, Quitter | chacun voit l'autre (tuiles, barre latérale, « En appel ») et entend son micro ; l'écran partagé joue chez qui regarde ; icônes micro et son coupés chez l'autre ; écran puis participant retirés ; axe ok | e2e (micro et écran factices de Chromium) | ok |
| F04-N1 | | sans accès | non-membre (API et adresse du salon), salon caché, salon textuel, sans session | 404, 404, 400, 401 ; personne en vocal | e2e | ok |
| F04-N2 | | sans la permission CONNECT, salon plein (25) | | 403, 409 | cargo test | ok |
| F04-M1 | | Firefox, Safari, appli de bureau, vrai partage d'écran avec son | | | manuel | non fait |
| F05-H1 | F05 créer et inviter | nominal | créer, Inviter des amis, copier le lien | lien `/invite/xxxxxxxx`, axe ok | e2e | ok |
| F05-N1 | | nom vide, court, espaces | formulaire et API | refusé par le navigateur et par le serveur (400) | e2e | ok |
| F05-E1 | | nom de 60 caractères | | coupé proprement, pas de débordement | e2e | ok |
| F05-E2 | | icône trop lourde | | | non construit | bloqué |
| F06-H1 | F06 rejoindre | nominal | lien → aperçu → Rejoindre → discuter | messages en direct dans les deux sens, membre ajouté | e2e | ok |
| F06-N1 | | lien inconnu ou mal formé | | « Invitation invalide » | e2e | ok |
| F06-N2 | | usage unique épuisé | second invité | « Invitation invalide » | e2e | ok |
| F06-E1 | | déjà membre | rejoindre deux fois | pas de doublon | e2e | ok |
| F06-E2 | | visiteur sans compte | aperçu → Créer un compte → retour | revient sur l'invitation, rejoint | e2e | ok |
| F06-E3 | | salon visé | invitation vers #général, un autre salon en tête | ouvre #général | e2e | ok (BUG-003) |
| F06-N3 | | banni | | refusé (403) | cargo test | ok |
| F07 | F07 rôles et permissions de salon | interface | | | non construit (calcul testé : `kraken-core`) | bloqué |
| F08 | F08 profil | tous | | | non construit | bloqué |
| F09 | F09 micro | tous | | | non construit | bloqué |
| F10 | F10 réagir, répondre, épingler, fil | tous | | | non construit | bloqué |
| F11-H1 | F11 bloquer | bloquer un ami | confirmer | l'autre ne peut plus écrire ni demander ; MP fermé ; débloquer | e2e | ok |
| F11-H2 | | liste des bloqués | Réglages › Qui peut t'écrire | Débloquer | e2e | ok |
| F11-E1 | | salon partagé | message de zed dans #général | replié, Afficher / Masquer | e2e | ok |
| F12-H1 | F12 inscription | nominal | formulaire → Créer mon compte | accueil, axe ok | e2e | ok |
| F12-H2 | | déconnexion puis connexion | par pseudo, puis par e-mail en majuscules | accueil les deux fois | e2e | ok (BUG-001) |
| F12-N1 | | pseudo pris | | message clair | e2e | ok |
| F12-N2 | | e-mail déjà utilisé | | message clair | e2e | ok |
| F12-N3 | | mauvais mot de passe | | alerte, champ `aria-invalid` | e2e | ok |
| F12-E1 | | pseudo accentué, emoji, espace, 40 caractères | | refusé ; coupé à 32 | e2e | ok |
| F12-E2 | | double clic | | un compte, pas d'erreur | e2e | ok |
| F12-E3 | | clavier seul | Tab, Entrée | connecté | e2e | ok |
| F12-E4 | | session expirée | « Se déconnecter partout » dans un autre navigateur | l'autre onglet revient à la connexion | e2e | ok |
| F12-E5 | | page protégée sans session | /settings/appearance | connexion puis retour à la page | e2e | ok |
| F12-E6 | | mobile 390 px | inscription | pas de défilement horizontal | e2e | ok |
| F12-E7 | | changer de compte dans l'onglet | déconnexion A, connexion B | serveurs de B seulement | e2e | ok (BUG-001, BUG-002) |
| F12-N4 | | mot de passe oublié sans SMTP | | explication, pas de plantage | e2e | ok |
| F12-N5 | | compte supprimé | Supprimer le compte → se reconnecter | refusé | e2e | ok |
| F12-M1 | | e-mails reçus (vérification, nouveau mot de passe) | instance avec SMTP | e-mail reçu, lien valable une fois | manuel | non fait : pas de SMTP ici |
| F12-M2 | | MFA, appareils à révoquer | | | non construit | bloqué |

## Passe manuelle

Faite dans Chromium piloté par Playwright pendant l'écriture des tests (captures d'échec dans `e2e/test-results/`, non versionnées). Reste à faire à la main, sur une instance réelle : réception des e-mails (F12-M1), réseau lent (3G), lecteur d'écran (VoiceOver, NVDA), Safari et Firefox.
