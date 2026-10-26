# Parité : KRAKEN face à l'original

Build : 522f995  Date : 2026-10-17

**Verdict : pas livrable.** 38 fonctions indispensables sur 40 ne sont pas finies (dont 12 à moitié faites). Aucun bug S1 ou S2 ouvert (`bugs.md`).

| mesure | score | lecture |
| --- | --- | --- |
| fonctions | 13,4 / 100 | 93 fonctions comptées (11 hors périmètre exclues), 3 faites, 14 partielles |
| mise en page | 26,4 / 100 | 4 écrans comparés, tous « différents » |
| global | 16,0 / 100 | |

Ces chiffres sont honnêtes : la tranche verticale (compte, serveur, invitation, discussion en direct) marche de bout en bout et est testée, mais tout ce qui fait Discord au quotidien autour (vocal, réactions, réponses, non-lus, rôles, profils) reste à construire.

## Mise en page, écran par écran

Comparaison des contours, sans les couleurs, à 1728×1117 (captures de l'original : client macOS du propriétaire, gardées hors git ; images de différence dans le dossier privé du projet, `replica-diffs/`).

| écran | score | ce qui diffère surtout | cause |
| --- | --- | --- | --- |
| S01 Accueil Amis | 22,5 | toute la zone centrale, panneau « En ligne » | l'original a une barre de titre pleine largeur, des entrées Boutique/Quêtes/Nitro et un bandeau ; le clone a moins d'amis et un panneau d'activité vide |
| S05 Salon textuel | 26,8 | colonne des messages, bas à droite | l'original affiche de grandes intégrations vidéo et une barre de saisie chargée (cadeau, GIF, autocollants, applis) : fonctions absentes du clone |
| S06 Réglages › Compte | 28,4 | panneau de droite, menu de gauche | menu de réglages bien plus long dans l'original (Nitro, facturation, applis) ; le clone n'a que la carte d'identité, déconnexion et suppression |
| S11 Réglages › Apparence | 27,9 | centre, menu de gauche | l'original montre ses thèmes en vignettes colorées, dégradés et icônes d'appli ; le clone, 4 choix de thème en liste |

La structure (rail, colonne de navigation, en-tête, contenu, panneau latéral) est la même ; l'écart vient surtout de contenu et de fonctions manquants, pas de la disposition. Les écrans S19, S21, S28 et les vues mobiles n'ont pas de capture de l'original : non comparés.

## Comportement, parcours par parcours

| parcours | l'original | le clone | décision |
| --- | --- | --- | --- |
| F01 message | 2 clics, puis Entrée | pareil | garder |
| F01 échec d'envoi | message en rouge, Réessayer ou Supprimer | Réessayer seulement | corriger : pouvoir abandonner un envoi raté |
| F05 créer et inviter | 5 clics (choix d'un modèle) | 4 clics : nom prérempli, pas de modèle | garder, ajouter les modèles en option |
| F05 lien d'invitation | expire au bout de 7 jours par défaut, réutilisé | nouveau lien sans expiration à chaque ouverture | corriger : 7 jours par défaut, réutiliser le lien, réglages d'expiration dans la boîte |
| F06 rejoindre | lien, aperçu, Accepter | pareil, et un visiteur sans compte revient sur l'invitation après inscription | garder |
| F12 connexion | e-mail ou téléphone | pseudo ou e-mail | garder (pas de téléphone exigé) |
| F12 inscription | date de naissance, captcha | e-mail, pseudo, mot de passe | garder ; prévoir un captcha libre si l'instance est publique |
| F12 e-mails | vérification et alertes de connexion | vérification et mot de passe oublié, si l'instance a un SMTP | garder ; ajouter l'alerte de nouvelle connexion |
| Réglages | synchronisés entre appareils | thème gardé dans le navigateur seulement | corriger avec la table `user_settings` existante |
| Présence | 4 statuts au choix | en ligne ou hors ligne réels | corriger (sélecteur de statut dans la carte « Mon compte ») |

## À construire ensuite (dans l'ordre)

1. **Messages complets** : modifier, supprimer, répondre, réactions, markdown et mentions (S05, F10). C'est le cœur de l'usage quotidien.
2. **Social** : ajouter un ami, accepter ou refuser, bloquer, ouvrir un nouveau MP ou un groupe (F02, F03, F11).
3. **Non-lus et mentions** dans le rail et la liste des salons, puis notifications bureau (S02, S09, S25).
4. **Pièces jointes et aperçus de liens**, avec stockage compatible S3 et plafonds réglables par l'instance.
5. **Vocal** : salon vocal, muet et sourdine, appel en MP, partage d'écran (F03, F04, S18). Le plus gros morceau (SFU str0m).

Viennent ensuite les rôles et la modération (F07, S22), le profil (S20) et les réglages restants (S07 à S14).

---

Sortie brute de `parity.py` :

### Parity: 16.0 / 100

features 13.4  (93 counted, must-haves 2 of 40 done)
layout   26.4  (4 screens compared)

Not shippable yet: 38 must-have features are not done.

### By area, weakest first
- moderation                     0.0  (5 features)
- emojis                         0.0  (5 features)
- notifications                  0.0  (5 features)
- recherche                      0.0  (2 features)
- voix                           0.0  (11 features)
- video                          0.0  (5 features)
- profil                         0.0  (6 features)
- accessibilite                  0.0  (3 features)
- systeme                        0.0  (3 features)
- integrations                   0.0  (1 features)
- messages                      13.0  (11 features)
- social                        17.6  (7 features)
- salons                        27.3  (5 features)
- mp                            30.0  (4 features)
- serveurs                      32.1  (8 features)
- apparence                     33.3  (5 features)
- comptes                       36.7  (6 features)
- plateformes                   50.0  (1 features)

### Missing, in build order
- [must] accessibilite: Taille du texte densité zoom, no  (S12)
- [must] comptes: Modifier pseudo e-mail téléphone mot de passe, no  (S06 ; téléphone facultatif)
- [must] emojis: Emojis perso de serveur, no  (S22)
- [must] emojis: Emojis perso utilisables partout et animés, no  (nitro : gratuit)
- [must] messages: Aperçus de liens (embeds), no  (S05)
- [must] messages: Markdown mentions et spoilers, no  (S05 S11)
- [must] messages: Pièces jointes images vidéos fichiers, no  (S05)
- [must] messages: Réactions emoji, no  (S05)
- [must] messages: Répondre à un message, no  (S05)
- [must] moderation: Expulser bannir exclure temporairement, no  (S22)
- [must] moderation: Rôles avec couleur et permissions, no  (S22 F07)
- [must] notifications: Non-lus et accusés de lecture, no  (S13 raccourcis non-lus)
- [must] notifications: Notifications bureau et push, no  (S09)
- [must] notifications: Réglages de notif par serveur et salon, no  (S09)
- [must] profil: Profil avec avatar bio pronoms, no  (S20)
- [must] social: Ajouter un ami par pseudo, no  (F02)
- [must] social: Bloquer ignorer et débloquer, no  (S08 F11)
- [must] systeme: Langues multiples et format 12 24 h, no  (S14 ; FR et EN d'abord)
- [must] systeme: Raccourcis clavier par défaut, no  (S13)
- [must] video: Caméra en appel, no  (S10 S18)
- [must] video: Partage d'écran, no  (S18 F04)
- [must] voix: Appel en MP, no  (F03)
- [must] voix: Choix des périphériques et test micro, no  (S10)
- [must] voix: Détection de voix et push-to-talk, no  (S10)
- [must] voix: Muet et sourdine, no  (S04)
- [must] voix: Salon vocal, no  (S18 F04)
- [must] comptes: Suppression et désactivation du compte, partial  (S07 ; suppression faite (fermeture immédiate, anonymisation à 14 jours), désactivation pas encore)
- [must] messages: Envoyer modifier supprimer un message, partial  (S05 ; envoi seulement (optimiste, nonce, réessai))
- [must] mp: MP 1:1, partial  (S19 ; lire et écrire, ouvrir un nouveau MP pas encore)
- [must] mp: MP de groupe, partial  (S03 ; lire et écrire, créer un groupe pas encore)
- [must] plateformes: Client desktop web et mobile, partial  (web responsive (390 à 1728 px) ; coque Tauri pas encore)
- [must] salons: Salons privés (surcharges de permissions), partial  (S05 S23 ; moteur et contrôle d'accès faits, pas d'écran d'édition)
- [must] salons: Salons texte annonces et catégories, partial  (S05 ; affichage et catégories repliables, création par API seulement)
- [must] serveurs: Créer un serveur (avec modèles), partial  (S21 F05 ; sans modèles ni icône)
- [must] serveurs: Rail des serveurs avec non-lus et mentions, partial  (S02 ; sans non-lus ni mentions)
- [must] serveurs: Rejoindre via invitation (expiration et usages max), partial  (F06 ; expiration et usages max côté API, pas encore dans l'UI)
- [must] social: Demandes d'ami en attente (accepter refuser), partial  (S01 ; affichées, pas encore d'action)
- [must] social: Présence (en ligne inactif ne pas déranger invisible), partial  (S01 S04 ; en ligne/hors ligne réels, autres statuts pas encore réglables)
- [should] accessibilite: Contraste élevé saturation mouvement réduit, no  (S12)
- [should] apparence: Synchro des réglages entre appareils, no  (S11)
- [should] apparence: Thèmes de couleur perso, no  (S11 ; nitro : gratuit)
- [should] comptes: Authentification multi-facteurs (TOTP), no  (S06)
- [should] comptes: Demande d'export de ses données, no  (S07 ; RGPD)
- [should] emojis: Autocollants perso partout, no  (nitro : gratuit)
- [should] integrations: Applications OAuth autorisées (bots), no  (S15 ; API publique KRAKEN pour bots)
- [should] messages: Envois lourds jusqu'à 500 Mo, no  (nitro : gratuit ; plafond réglable par l'instance)
- [should] messages: Texte alternatif des images, no  (S12)
- [should] messages: Épingler un message, no  (S05)
- [should] moderation: Journal d'audit, no  (S22)
- [should] mp: Filtre anti-spam des MP, no  (S08)
- [should] mp: Permissions de MP par serveur et demandes de message, no  (S08)
- [should] notifications: Boîte de réception mentions, no  (S25)
- [should] profil: Avatar animé et bannière, no  (S20 ; nitro : gratuit)
- [should] recherche: Quick Switcher Ctrl+K, no  (S29)
- [should] recherche: Recherche dans les messages, no  (S26)
- [should] salons: Fils de discussion, no  (S24)
- [should] salons: Mode lent et sujet du salon, no  (S23)
- [should] social: Qui peut m'envoyer une demande d'ami, no  (S08)
- [should] social: Statut personnalisé et activité en cours, no  (S01)
- [should] systeme: Raccourcis clavier perso, no  (S13)
- [should] video: Stream 1080p60 et plus, no  (nitro : gratuit)
- [should] voix: Chiffrement de bout en bout du vocal, no  (S07 ; MLS)
- [should] voix: Soundboard, no  (S10 S22)
- [should] voix: Suppression de bruit et annulation d'écho, no  (S10 ; RNNoise au lieu de Krisp)
- [should] comptes: Liste et révocation des appareils connectés, partial  (S06 ; « se déconnecter partout » fait, liste des appareils pas encore)
- [could] accessibilite: Synthèse vocale des messages, no  (S12)
- [could] apparence: Icônes d'appli alternatives, no  (S11 ; nitro : gratuit ; icônes originales)
- [could] apparence: Mode streamer, no  (S11)
- [could] emojis: GIF dans la zone de saisie, no  (S05 ; fournisseur GIF libre ou clé de l'instance)
- [could] emojis: Super réactions, no  (nitro : gratuit)
- [could] messages: Transférer un message, no  (S13)
- [could] moderation: AutoMod (mots interdits spam), no  (S22)
- [could] moderation: Filtre de contenu média sensible (flou bloquer), no  (S08 ; nécessite un classifieur)
- [could] notifications: Notifications par e-mail, no  (S09)
- [could] profil: Comptes liés affichés sur le profil, no  (S15 ; OAuth public de chaque plateforme)
- [could] profil: Profil par serveur, no  (S20 ; nitro : gratuit)
- [could] profil: Styles de pseudo (police effet couleur), no  (S12 ; nitro : gratuit)
- [could] profil: Thème de profil et déco d'avatar, no  (S20 ; nitro : gratuit)
- [could] salons: Salons forum, no  (S24)
- [could] serveurs: Dossiers de serveurs, no  (S02)
- [could] serveurs: Explorer des serveurs publics, no  (S30)
- [could] serveurs: Plus de serveurs par compte (200), no  (nitro : gratuit ; plafond réglable par l'instance)
- [could] serveurs: Serveurs favoris, no  (S11 ; nitro : gratuit)
- [could] serveurs: Événements de serveur, no  (S27)
- [could] social: Panneau d'activité des amis, no  (S01)
- [could] video: Aperçu du stream, no  (S10)
- [could] video: Flou et arrière-plans vidéo perso, no  (S10 ; nitro : gratuit ; images originales)
- [could] voix: Qualité audio 384 kbps, no  (boost : gratuit)
- [could] voix: Son d'entrée en vocal, no  (S10 ; nitro : gratuit)
- [could] voix: Sons du soundboard de tous les serveurs, no  (nitro : gratuit)

### Left out on purpose (not scored)
- Abonnement Nitro: tout est gratuit dans KRAKEN
- Boosts de serveur payants: avantages actifs pour tous
- Boutique et Orbes: commerce de Discord
- Quêtes sponsorisées: publicité de partenaires
- Cadeaux et facturation: pas de paiement
- Récompenses Xbox Game Pass: accord commercial de Discord
- Activités de jeux partenaires: contenu sous licence
- Jeux connectés (SDK social): SDK propriétaire
- Suppression de bruit Krisp: sous licence ; voir RNNoise
- Sons icônes arrière-plans et mascotte de Discord: propriété de Discord ; créer des originaux
- Vérification d'âge par prestataire et Centre familial: à repenser plus tard

### Yours, not in the original (not scored)
- Auto-hébergement d'une instance

### Screens
- replica/clone-screens/S01.png             22.5
- replica/clone-screens/S05.png             26.8
- replica/clone-screens/S11.png             27.9
- replica/clone-screens/S06.png             28.4
