# Bugs : KRAKEN

Trouvés par la suite `e2e/` (Playwright, Chromium) le 2026-10-17, build 2ece10f. Chaque correctif a d'abord un test qui échoue, gardé ensuite.

| | S1 | S2 | S3 | S4 |
| --- | --- | --- | --- | --- |
| trouvés | 1 | 6 | 17 | 19 |
| corrigés | 1 | 6 | 17 | 19 |

### BUG-001 : se reconnecter après une déconnexion ne mène nulle part

- Gravité : S2
- Parcours / cas : F12 / F12-H2, F12-E7
- Écran : S28 (connexion)
- Build : 2ece10f  Navigateur : Chromium 141, 1440 px

Étapes
1. Connecté, ouvrir Réglages → Mon compte, cliquer « Déconnexion ».
2. Sur la page de connexion, saisir le même compte, « Se connecter ».

Attendu : l'accueil s'ouvre.
Constaté : la requête répond 200, l'onglet reste sur `/login` sans message. Il faut recharger la page.
Preuve : test `F12-H2` (délai dépassé sur `waitForURL('**/channels/@me')`) ; l'URL après déconnexion est `/login?next=/login`.
Cause : la redirection de la coquille s'exécutait après la navigation vers `/login` et prenait `/login` comme page de retour.
Statut : corrigé dans c313fe7 (les pages de connexion ne sont plus acceptées comme page de retour).

### BUG-002 : après déconnexion, le gateway de l'appareil reste ouvert

- Gravité : S1 (sécurité)
- Parcours / cas : F12 / F12-E7 ; test serveur `logout_closes_that_sessions_gateway`
- Écran : tous
- Build : 2ece10f

Étapes
1. Compte A connecté dans un onglet (gateway ouvert).
2. Déconnexion de cet appareil.
3. Se connecter avec le compte B dans le même onglet.

Attendu : la session révoquée ne reçoit plus rien ; B voit ses serveurs.
Constaté : la connexion WebSocket ouverte avec la session révoquée continuait de recevoir les événements de A (messages de ses serveurs). Dans l'onglet, B ne recevait jamais son READY : liste de serveurs vide jusqu'au rechargement.
Preuve : `cargo test -p kraken-server --test slice logout_closes` échoue avant correctif (aucune fermeture en 2 s) ; `F12-E7` échoue sur « Serveur Repaire de Lia » absent.
Cause : le hub indexait les connexions par utilisateur seulement ; seule « déconnexion partout » les fermait.
Statut : corrigé dans c313fe7 (chaque connexion garde l'empreinte de sa session, la déconnexion ferme celles-là).

### BUG-003 : une invitation ouvre le premier salon, pas celui qu'elle vise

- Gravité : S3
- Parcours / cas : F06 / F06-E3, F01-N2
- Écran : S03 (invitation) puis S05
- Build : 2ece10f

Étapes
1. Dans Abysse, créer une invitation vers #général.
2. Avec un autre compte, ouvrir le lien, « Rejoindre le serveur ».

Attendu : #général s'ouvre.
Constaté : #annonces s'ouvre (premier salon texte de la liste).
Preuve : capture d'échec de `F01-N2` (en-tête « annonces »).
Statut : corrigé dans c313fe7.

### BUG-004 : « Réessayer » peut dupliquer un message

- Gravité : S3
- Parcours / cas : F01 / F01-E7
- Écran : S05
- Build : 2ece10f

Étapes
1. Gateway muet (événements perdus), envoyer un message dont la réponse HTTP se perd alors que le serveur l'a enregistré.
2. Cliquer « Réessayer ».

Attendu : un seul message.
Constaté : deux messages identiques, avec deux nonces différents.
Preuve : `F01-E7`, 4 échecs sur 4 avant correctif (« Received length: 2 »).
Statut : corrigé dans c313fe7 (Réessayer renvoie le même nonce ; le serveur rend le message existant).

### BUG-005 : l'historique des messages ne se fait pas défiler au clavier

- Gravité : S3 (accessibilité)
- Parcours / cas : F01 / F01-E9
- Écran : S05, 390 px
- Build : 2ece10f

Étapes
1. Ouvrir un salon avec assez de messages pour défiler.
2. Naviguer au clavier seul.

Attendu : la zone des messages reçoit le focus et défile avec les flèches.
Constaté : aucune étape de tabulation ne l'atteint.
Preuve : axe, règle `scrollable-region-focusable` sur `.app-log`.
Statut : corrigé dans c313fe7 (`tabindex="0"` et nom accessible « Messages »).

## Relecture du code (2026-10-19)

Relecture de toute la pile (sécurité, base, gateway, client, exploitation) : 51 constats, 46 confirmés, regroupés ci-dessous en 38 bugs distincts. Tests : `crates/*/src/tests.rs`, `crates/kraken-server/tests/slice.rs`, `e2e/specs/regressions.spec.ts`.

| ID | Gravité | Bug | État |
| --- | --- | --- | --- |
| SEC-1 | S2 | Limite de débit contournable : clé prise à gauche de `X-Forwarded-For` (choisie par le client) | corrigé : entrée de droite, IP validée, test `limit::tests` |
| OPS-2 | S2 | Table de la limite de débit jamais purgée (mémoire sans fin) | corrigé : purge chaque minute |
| SEC-2 | S2 | Tâche en lettre morte garde sa clé : mot de passe oublié bloqué pour toujours, purge arrêtée | corrigé : index unique partiel (0003), purge jamais en lettre morte |
| SEC-3 | S2 | Suppression de compte impossible pour qui a créé un serveur | corrigé : serveur vide supprimé, sinon transmis au plus ancien membre |
| SEC-1c | S2 | Redirection ouverte via `?next=/%5Chôte` | corrigé, e2e |
| SEC-6 | S3 | Un /64 IPv6 contourne la limite | corrigé : un /64 = un client |
| SEC-5 | S3 | Mot de passe de suppression sans limite ; argon2 sans plafond (mémoire) | corrigé : route limitée, sémaphore argon2 |
| OPS-5 | S3 | Derrière un proxy qui réécrit `Host`, toute écriture renvoie 403 | corrigé : `Origin` comparé aussi à `KRAKEN_PUBLIC_URL` |
| OPS-8 | S3 | Fichiers du client sans `Cache-Control` : vieux client après mise à jour | corrigé : `no-cache` |
| GW-1 | S3 | Panne passagère de la base au IDENTIFY : le client se déconnecte | corrigé : coupure simple, le client se reconnecte |
| CHAT-1 | S3 | Après reconnexion, les vieux messages passent en bas | corrigé |
| CHAT-2 | S3 | Chaque message reçu ramène en bas qui lit l'historique | corrigé, e2e |
| NAV-1 | S3 | Replier une catégorie ne cache pas ses salons | corrigé, e2e |
| MOB-1 | S3 | Sous 1024 px, liste des membres inaccessible | corrigé : panneau par-dessus le fil, e2e |
| A11Y-1 | S3 | Les modales ne prennent pas le focus ; Échap inopérant | corrigé, e2e |
| A11Y-2 | S3 | Le lien d'évitement ne déplace pas le focus | corrigé, e2e |
| OPS-3, OPS-6, OPS-7 | S3 | Compose : port ouvert à tous, sans TLS, URL publique par défaut, mot de passe dans l'URL | corrigé : Caddy en HTTPS seul exposé, domaine obligatoire, `PGPASSWORD` |
| UI-1 | S4 | Échap dans la confirmation ferme tous les réglages | corrigé, e2e |
| SEC-7, ERR-2 | S4 | Caractère nul : erreur 500 au lieu de 400 | corrigé, test |
| SEC-8 | S4 | Texte du refus SMTP (avec l'adresse) dans les logs | corrigé : code seul |
| API-1 | S4 | Diffusion perdue si elle échoue après l'insertion | corrigé : un renvoi rediffuse |
| ORD-1, ORD-2 | S4 | Messages simultanés dans le désordre ; `last_message_id` qui recule | corrigé |
| ERR-1 | S4 | Route `/api/v1/*` inconnue : page HTML au lieu d'un 404 JSON | corrigé, test |
| DB-3 | S4 | Nonce réutilisé dans un autre salon : renvoie le mauvais message | corrigé : nonce par salon, test |
| DB-4 | S4 | Double clic sur « Rejoindre » : erreur alors que l'adhésion a marché | corrigé, test |
| DB-5 | S4 | Création de serveur et suppression de compte en même temps | corrigé : verrou de ligne |
| GW-2, STORE-1 | S4 | Salons et membres en double après le READY | corrigé : dédoublonnés |
| CRASH-1 | S4 | Quitter la page pendant l'adhésion fait planter l'appli | corrigé |
| CHAT-4 | S4 | Brouillon de MP effacé quand l'autre change de profil | corrigé : brouillons gardés par salon |
| SET-1 | S4 | Saisie du nom affiché écrasée | corrigé |
| CHAT-5 | S4 | Champ de saisie qui ne grandit pas | corrigé |
| SHUT-1 | S4 | SIGTERM ignoré dans le conteneur | corrigé |
| OPS-10 | S4 | E-mails envoyés depuis `no-reply@localhost` | corrigé : domaine de `KRAKEN_PUBLIC_URL` |
| OPS-11, OPS-13 | S4 | `.env.example` et README trompeurs | corrigé |

## À vérifier (non reproduit comme bug)

- Sans gateway (WebSocket bloqué par un proxy), l'appli reste sur ses squelettes : tout l'état vient du READY. Prévoir un repli REST ou un message explicite.
- Mot de passe oublié sur une instance sans SMTP : le serveur répond 501 et l'écran l'explique. Masquer plutôt le lien quand l'instance n'envoie pas d'e-mails.
- Lancer `e2e/` sur la base de démo ajoute des membres au serveur Abysse (cas F01-N2). Sans effet en CI (base neuve), mais à nettoyer avant des captures locales.
