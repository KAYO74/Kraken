# Bugs : KRAKEN

Trouvés par la suite `e2e/` (Playwright, Chromium) le 2026-10-17, build 2ece10f. Chaque correctif a d'abord un test qui échoue, gardé ensuite.

| | S1 | S2 | S3 | S4 |
| --- | --- | --- | --- | --- |
| trouvés | 1 | 1 | 3 | 0 |
| corrigés | 1 | 1 | 3 | 0 |

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

## À vérifier (non reproduit comme bug)

- Sans gateway (WebSocket bloqué par un proxy), l'appli reste sur ses squelettes : tout l'état vient du READY. Prévoir un repli REST ou un message explicite.
- Mot de passe oublié sur une instance sans SMTP : le serveur répond 501 et l'écran l'explique. Masquer plutôt le lien quand l'instance n'envoie pas d'e-mails.
- Lancer `e2e/` sur la base de démo ajoute des membres au serveur Abysse (cas F01-N2). Sans effet en CI (base neuve), mais à nettoyer avant des captures locales.
