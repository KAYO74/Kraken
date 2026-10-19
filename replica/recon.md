# Carte de recon : Discord (desktop, web, mobile)

Clone : **KRAKEN**, open source et libre, écrit en Rust.
Périmètre : messagerie communautaire pour joueurs (serveurs, salons texte et vocaux, MP, amis) **avec toutes les fonctionnalités Nitro gratuites**. Pas de boutique, pas d'abonnement, pas de boosts payants.
Pour : tout le monde, gratuitement ; auto-hébergeable.
Date : 2026-10-17

Règles : reconstruction « clean room » des fonctionnalités et des parcours. Aucun code, asset, icône, logo (dont la mascotte), son ou texte de Discord. Les captures sont une référence de mise en page uniquement et ne sont **jamais versionnées** : elles contiennent des données personnelles (pseudo, e-mail et téléphone masqués, amis, comptes liés).

## Sources

| # | source | URL | notes |
| --- | --- | --- | --- |
| 1 | 42 captures du compte du propriétaire (client desktop macOS, FR, 2026-10-16) | dossier partagé du projet, hors git | source principale : accueil Amis, salon de serveur, tous les paramètres utilisateur |
| 2 | Centre d'aide | https://support.discord.com | référence pour les écrans non capturés (vocal, serveur, rôles) ; non relu dans cette session |
| 3 | Page Nitro | https://discord.com/nitro | liste des fonctions payantes à rendre gratuites ; non relue dans cette session |
| 4 | Docs développeur (API publique) | https://discord.com/developers/docs | modèle de données (Guild, Channel, Message, Role, permissions) ; non relue dans cette session |
| 5 | Blog / nouveautés | https://discord.com/blog | priorités récentes (thèmes, E2EE vocal, sons d'entrée) |

Les lignes 2 à 5 viennent de la connaissance générale du produit ; les écrans qui en dépendent sont marqués « non capturé ».

## Boucle principale

Rejoindre un serveur entre amis, discuter par écrit dans des salons, sauter dans un salon vocal et partager son écran pendant qu'on joue.

## Écrans

Captures citées par leur heure (`08.25.14` = `Capture d’écran 2026-10-16 à 08.25.14.png`).

| ID | écran | accès | rôle | composants clés | états vus |
| --- | --- | --- | --- | --- | --- |
| S01 | Accueil Amis | icône app en haut du rail | liste d'amis + activité | onglets En ligne/Tous/En attente/Ajouter, recherche, ligne ami (MP, menu ⋮), panneau « En ligne » | rempli, vide (« Tout est calme… »), bandeau info fermable — 08.23.58 |
| S02 | Rail des serveurs | toujours visible, gauche | naviguer entre serveurs | icône accueil, icônes serveur, dossier de serveurs, + (créer/rejoindre), boussole (explorer) | sélectionné, non-lu (point), dossier — 08.23.58, 08.28.07 |
| S03 | Barre MP | colonne 2 sur l'accueil | MP et raccourcis | recherche « lance une conversation », Amis, Bibliothèque, Nitro, Boutique, Quêtes, MP 1:1 et groupes (nb membres), + nouveau MP | rempli, présence — 08.23.58 |
| S04 | Panneau utilisateur | bas gauche, global | statut et audio rapide | avatar + statut, micro et casque avec menus, roue paramètres | en ligne — partout |
| S05 | Salon textuel de serveur | clic sur un salon | discuter | en-tête (nom, sujet, fils, notifs, épinglés, membres, recherche), liste des salons (Événements, annonces, catégories repliables, texte, vocal, privé), messages (embed vidéo, réponse, pièce jointe masquée), zone de saisie (+, cadeau, GIF, autocollant, emoji, apps), inviter | rempli — 08.28.07 |
| S06 | Paramètres › Compte | roue › Compte | identité et sécurité | nom d'utilisateur, e-mail et téléphone masqués + « Afficher », mot de passe, MFA, appareils connectés, statut du compte, groupe d'âge | rempli — 08.24.12, 08.24.18 |
| S07 | Paramètres › Données et confidentialité | roue | usage des données, visibilité du profil | interrupteurs, radios « profil complet visible par », vérification E2EE vocale permanente, demande de données | rempli — 08.24.51, 08.24.55 |
| S08 | Paramètres › Permissions de messagerie | roue | filtres et qui peut me contacter | filtres média sensible (Flou/Bloquer par contexte), filtres spam (3 radios), MP par serveur, demandes d'ami (tout le monde, amis d'amis, membres), jeux connectés, comptes bloqués + Débloquer | rempli — 08.25.05 à 08.25.16 |
| S09 | Paramètres › Notifications | roue | alertes | bureau, événements déclencheurs, sons (pack saisonnier, nouveau message, appel), badge non-lu, e-mails par catégorie, avancé | rempli — 08.25.19 à 08.25.26 |
| S10 | Paramètres › Voix et vidéo | roue | périphériques | micro/haut-parleur + volumes, test micro, profil d'entrée (isolation/studio/perso), sensibilité VAD, suppression de bruit, écho ; test caméra, arrière-plans (flou, perso, animés) ; aperçu stream ; sons d'action (18+) ; volume soundboard, **son d'entrée** ; diagnostic, réinitialiser | rempli — 08.25.49 à 08.25.59 |
| S11 | Paramètres › Apparence | roue | look du client | thèmes clair/sombre/plus sombre/noir/auto, **thème perso et dégradés de couleur**, sync multi-appareils, **icône de l'appli** (≈25 variantes), affichage médias, spoilers, fils en vue fractionnée, serveurs favoris, boîte de discussion, recherche MP, mode streamer, bibliothèque | rempli — 08.26.03 à 08.26.20 |
| S12 | Paramètres › Accessibilité | roue | lisibilité | aperçu live, taille texte 12–24 px, soulignage liens, styles de pseudo, densité 3 niveaux, affichage compact, espacement, zoom 50–200 %, saturation, contraste élevé, couleurs de rôle, mouvement réduit, vitesse TTS, descriptions d'image | rempli — 08.26.23 à 08.26.40 |
| S13 | Paramètres › Système (raccourcis) | roue | raccourcis clavier | raccourcis perso (+ Ajouter), liste par défaut par catégorie (messages, navigation, glisser-déposer, discussion, voix, divers), capture de perf | vide (aucun perso), rempli — 08.26.44 à 08.27.03 |
| S14 | Paramètres › Langue et heure | roue | locale | sélecteur de langue avec drapeaux, format 12 h/24 h/auto | fermé, ouvert — 08.27.07, 08.27.10 |
| S15 | Paramètres › Applications connectées | roue | comptes tiers et OAuth | grille de plateformes, carte connexion (afficher sur profil), applis autorisées (permissions, retirer) | rempli — 08.27.37 à 08.27.44 |
| S16 | Paramètres › Inventaire des cadeaux | roue | codes cadeau | champ code, badges de don, liste vide | vide — 08.25.37 |
| S17 | Paramètres › Facturation | roue | paiement | solde, moyens de paiement, historique | vide — 08.25.41 |
| S18 | Salon vocal / appel | clic salon vocal, bouton appel en MP | parler, vidéo, partage d'écran | grille participants, contrôles (micro, caméra, écran, soundboard, quitter), qualité du stream | non capturé |
| S19 | Conversation MP / groupe | clic MP dans S03 | discuter en privé | comme S05 + appel, ajout de membres, profil latéral | non capturé |
| S20 | Profil (popout + plein) et éditeur de profil | clic avatar ; roue › Modifier les profils | identité visible | bannière, avatar (animé), déco, effet, bio, thème de profil, profil par serveur, comptes liés | non capturé |
| S21 | Créer / rejoindre un serveur | + du rail | onboarding serveur | modèles, nom, icône, champ invitation | non capturé |
| S22 | Paramètres du serveur | menu du nom du serveur | administrer | vue d'ensemble, rôles, emojis, autocollants, soundboard, membres, invitations, modération/AutoMod, journal d'audit, intégrations | non capturé |
| S23 | Paramètres de salon | roue du salon | droits par salon | nom, sujet, mode lent, permissions par rôle/membre | non capturé |
| S24 | Fil et salon forum | bouton fils de S05 | discussions ciblées | liste de fils, vue fractionnée, posts forum avec tags | non capturé |
| S25 | Boîte de réception | icône en haut à droite | mentions et non-lus | onglets mentions/non-lus, tout marquer lu | non capturé |
| S26 | Recherche de messages | champ recherche de S05 | retrouver | filtres from:/in:/has:/date, résultats | non capturé |
| S27 | Événements de serveur | « Événements » de S05 | planifier | liste, création, « intéressé » | non capturé |
| S28 | Connexion / inscription | lancement déconnecté | entrer | e-mail ou pseudo, mot de passe, MFA, QR | non capturé |
| S29 | Quick Switcher | Cmd/Ctrl+K | sauter à un salon | champ + résultats | non capturé |
| S30 | Explorer les serveurs | boussole du rail | découvrir | catégories, recherche | non capturé |

Hors périmètre KRAKEN (vus mais non reconstruits) : Nitro (S03, remplacé par rien : tout est inclus), Boutique, Quêtes, Bibliothèque de jeux, Boost de serveur, Abonnements, S16, S17.

## Parcours

```
F01 Envoyer un message dans un salon
    S02 serveur -> S05 salon -> écrire -> Entrée
    clics : 2
    bords : salon en lecture seule, mode lent, message > limite, hors ligne (file d'attente), pièce jointe trop lourde

F02 Ajouter un ami
    S01 -> onglet Ajouter -> saisir le pseudo -> Envoyer ; l'autre accepte dans « En attente »
    clics : 3
    bords : pseudo inconnu, déjà ami, demande refusée par ses réglages (S08), compte bloqué

F03 Ouvrir un MP puis appeler
    S01 ligne ami -> icône message -> S19 -> bouton appel -> S18
    clics : 3
    bords : destinataire hors ligne, MP refusés (S08), appel manqué (notif S09)

F04 Rejoindre le vocal et partager l'écran
    S05 clic salon vocal -> S18 -> Partager l'écran -> choisir la fenêtre et la qualité (1080p60 ou plus, gratuit)
    clics : 3
    bords : pas de permission micro/écran, salon plein, changement de périphérique, vérification E2EE

F05 Créer un serveur et inviter
    S02 + -> S21 modèle -> nom/icône -> Créer -> S05 Inviter -> copier le lien
    clics : 5
    bords : nom vide, image trop lourde, lien expiré ou à usage limité

F06 Rejoindre via invitation
    lien -> aperçu -> Accepter -> S05
    clics : 2
    bords : invitation expirée, banni, déjà membre, règles à accepter

F07 Créer un rôle et restreindre un salon
    S22 Rôles -> Créer -> nom/couleur/permissions -> assigner ; S23 -> permissions du rôle
    clics : ~8
    bords : rôle au-dessus du sien, retrait de ses propres droits

F08 Personnaliser son profil (fonctions Nitro, gratuites)
    S04 roue -> Modifier les profils -> S20 bannière, avatar animé, thème, profil par serveur -> Enregistrer
    clics : 4
    bords : format ou taille d'image, GIF trop lourd, modifications non enregistrées

F09 Régler son micro
    S04 roue -> S10 -> choisir le micro -> Test du micro -> sensibilité/suppression de bruit
    clics : 4
    bords : aucun périphérique, permission OS refusée

F10 Réagir, répondre, épingler, créer un fil
    survol d'un message S05 -> action (raccourcis E, R, P, + ; fil Cmd+Maj+Opt+N)
    clics : 1–2

F11 Bloquer puis débloquer
    profil S20 -> Bloquer ; S08 Comptes bloqués -> Débloquer
    clics : 2 + 3

F12 Inscription et connexion
    S28 -> e-mail, pseudo, mot de passe -> vérif e-mail -> S01 ; MFA facultative dans S06
    bords : pseudo pris, e-mail déjà utilisé, code MFA faux, appareil à révoquer
```

## Composants

| composant | variantes | états | utilisé sur |
| --- | --- | --- | --- |
| Icône de serveur | serveur, accueil, dossier, action (+, explorer) | défaut, survol, sélectionné (pilule), non-lu, badge mentions | S02 |
| Élément de salon | texte, vocal, annonces, forum, privé (cadenas), catégorie repliable | défaut, survol, sélectionné, non-lu (gras), muet | S05 |
| Élément MP | 1:1, groupe (nb membres) | défaut, sélectionné, non-lu, fermeture au survol | S03 |
| Avatar | tailles xs à xl, groupe empilé | indicateur de présence : en ligne, inactif, ne pas déranger, hors ligne, mobile, en stream | partout |
| Bouton | primaire (accent), secondaire (gris), danger (texte rouge), lien, « essai » blanc | défaut, survol, focus, désactivé, chargement | partout |
| Interrupteur | simple, avec icônes on/off (a11y) | on, off, désactivé | S06–S15 |
| Radio | liste avec description | coché, non coché | S07, S08, S12, S14 |
| Liste déroulante | simple, avec icône/drapeau | fermée, ouverte, sélection | S08, S10, S11, S14 |
| Curseur | continu, à crans étiquetés | défaut, glissé | S10, S12 |
| Champ de recherche | barre, filtre de liste | vide, saisi | S01, S03, S05, S06, S15 |
| Onglets pilule | texte, accent (Ajouter) | actif, inactif | S01 |
| Modale paramètres plein écran | nav gauche (sections, sous-sections, recherche), contenu à défilement, fermer | — | S06–S17 |
| Ligne de réglage | titre + description + contrôle à droite, lien « Paramètres connexes » | — | S06–S15 |
| Bandeau info | info, avertissement | fermable | S01, S06, S13 |
| Badge texte | NOUVEAU, BÊTA, compteur | — | S03, S06, S11 |
| Message | normal, compact, réponse, système | survol (barre d'actions), en édition, épinglé, mentionné | S05, S12 |
| Embed | lien vidéo, lien simple (bordure colorée) | chargé, masqué | S05 |
| Pièce jointe | image, vidéo, fichier, spoiler | masquée, révélée | S05 |
| Réaction | emoji, emoji perso, super réaction | à moi, pas à moi, compteur | S05, S12 |
| Zone de saisie | salon, MP, édition | vide, saisie, autocomplétion (emoji, mention, commande) | S05 |
| Grille de pastilles | thèmes couleur, icônes d'appli, arrière-plans vidéo | sélectionné, NOUVEAU | S10, S11 |
| Ligne de raccourci | libellé + touches | — | S13 |
| Carte d'application autorisée | en-tête, lignes repliables, retirer | — | S15 |
| Panneau utilisateur | — | micro coupé, casque coupé, en appel | S04 |
| Menu contextuel, info-bulle, toast | — | — | partout |

## Modèle de données déduit

```
User          id, username (unique), display_name, email, phone?, password_hash, mfa_secret?,
              avatar, avatar_decoration, banner, bio, pronouns, accent_color, profile_theme,
              name_style, locale, created_at
              preuve : S06, S12 (styles de pseudo), S20   confiance : haute
Device        id, user_id, name, platform, last_seen, revoked_at
              preuve : S06 « Appareils connectés »   confiance : haute
Presence      user_id, status (online|idle|dnd|invisible|offline), custom_status, activities json   (éphémère)
              preuve : S01, S03   confiance : haute
Relationship  user_id, target_id, kind (friend|pending_in|pending_out|blocked|ignored), created_at
              preuve : S01 onglets, S08 bloqués   confiance : haute
UserSettings  user_id, privacy json, notifications json, voice json, appearance json, a11y json,
              keybinds json, locale, time_format   (synchronisé entre appareils)
              preuve : S06–S14   confiance : haute
Guild         id, name, icon, banner, owner_id, description, vanity_code, verification_level,
              content_filter, system_channel_id, created_at
              preuve : S02, S05   confiance : haute
GuildFolder   user_id, name, color, guild_ids[], position
              preuve : S02 icône empilée   confiance : moyenne
Channel       id, guild_id?, parent_id?, type (text|voice|announcement|category|forum|stage|dm|group_dm|thread),
              name, topic, position, nsfw, slowmode_s, bitrate, user_limit, last_message_id
              preuve : S05   confiance : haute
DmRecipient   channel_id, user_id
              preuve : S03 groupes « 5 membres »   confiance : haute
Role          id, guild_id, name, color, icon, position, permissions (bitfield 64 bits), hoist, mentionable
              preuve : S12 couleurs de rôle, docs API   confiance : haute
Member        guild_id, user_id, nick, avatar?, banner?, bio? (profil par serveur), joined_at, timeout_until
MemberRole    guild_id, user_id, role_id
              preuve : docs API   confiance : haute
Overwrite     channel_id, target_id, target_type (role|member), allow, deny
              preuve : S05 « Salons privés »   confiance : haute
Message       id (snowflake), channel_id, author_id, content (≤ 4000), reply_to_id?, thread_id?,
              pinned, flags, created_at, edited_at
              preuve : S05, S13   confiance : haute
Attachment    id, message_id, filename, size, mime, storage_key, spoiler, alt_text
Embed         message_id, data json
Reaction      message_id, user_id, emoji (unicode | emoji_id), burst
              preuve : S05, S12   confiance : haute
Emoji         id, guild_id, name, animated, image
Sticker       id, guild_id, name, format, image
SoundboardSound id, guild_id?, name, file, volume, emoji
EntranceSound user_id, guild_id?, sound_id
              preuve : S05 saisie, S10   confiance : haute
Invite        code, guild_id, channel_id, inviter_id, max_uses, uses, expires_at
ReadState     user_id, channel_id, last_read_message_id, mention_count
NotifOverride user_id, guild_id?, channel_id?, level (all|mentions|none), muted_until
VoiceState    user_id, channel_id, session_id, self_mute, self_deaf, video, streaming, server_mute
              preuve : S04, S09, S13   confiance : haute
ScheduledEvent id, guild_id, channel_id?, name, description, start_at, end_at, interested_count
              preuve : S05 « Événements »   confiance : moyenne
Connection    user_id, provider, external_id, display_name, show_on_profile, show_details
OAuthGrant    user_id, app_id, scopes[], granted_at
              preuve : S15   confiance : moyenne
AuditLog      id, guild_id, actor_id, action, target_id, changes json, created_at
AutoModRule   id, guild_id, trigger json, actions json
              preuve : aucune capture   confiance : supposition
```

Relations : User n-n Guild via Member ; Member n-n Role ; Guild 1-n Channel, Role, Emoji, Sticker, SoundboardSound, Invite, ScheduledEvent ; Channel 1-n Message 1-n Attachment, Embed, Reaction ; Channel(dm) n-n User via DmRecipient ; User n-n User via Relationship ; Channel 1-n Overwrite.

## Matrice des fonctionnalités

Voir `features.csv` : 105 lignes. Must 41, should 28, could 25, skip 11. Dont 18 fonctions Nitro ou boost rendues gratuites.

## Fonctions Nitro rendues gratuites

Toutes celles-ci sont payantes chez Discord et **incluses par défaut** dans KRAKEN (lignes `nitro` dans `features.csv`) : emojis et autocollants perso partout, emojis animés, envois jusqu'à la limite fixée par l'instance (défaut 500 Mo), stream 1080p60 et plus, messages longs (4000), avatar animé, bannière, thème de profil, déco d'avatar, profil par serveur, styles de pseudo, thèmes de couleur du client, icône d'appli alternative, arrière-plans vidéo perso, sons du soundboard partout, son d'entrée, super réactions, plus de serveurs et de dossiers, serveurs favoris. Les avantages de **boost** (qualité audio, emplacements emojis, URL perso, bannière, icône animée) sont actifs pour tous les serveurs ; seul l'admin de l'instance fixe des plafonds.

## Logo

Référence donnée : l'icône d'appli de Discord (bulle carrée arrondie, fond uni, glyphe blanc), visible en haut du rail et dans S11 « Icône de l'appli ». KRAKEN reprend le **format** (carré arrondi, couleur pleine, glyphe blanc monochrome, déclinable en variantes) avec un **glyphe original** : une tête de kraken stylisée. Interdit : la mascotte de Discord, sa couleur exacte en marque, sa typo. Brief détaillé à faire avec `/replica-brand`.

## Hors périmètre (ne peut pas ou ne doit pas être cloné)

- Le réseau d'utilisateurs et les serveurs existants de Discord.
- Boutique, Orbes, Quêtes (contenu sponsorisé), cadeaux, facturation, boosts payants, abonnements serveurs.
- Partenariats : récompenses Xbox Game Pass, Activités de jeux partenaires, intégration « jeux connectés » (SDK propriétaire).
- Suppression de bruit Krisp (sous licence) : remplacer par une alternative libre (ex. RNNoise).
- Packs de sons, arrière-plans, icônes et thèmes de Discord : en créer des originaux.
- Connexions tierces : possibles seulement via l'OAuth public de chaque plateforme, au cas par cas.
- Vérification d'âge par prestataire tiers et Centre familial : à repenser plus tard.

## Taille

Écrans 30 (17 capturés), parcours 12, entités 28.
Parties difficiles : passerelle temps réel (WebSocket, présence, fan-out à grande échelle), voix/vidéo/partage d'écran (WebRTC + SFU, E2EE vocal type MLS), moteur de permissions (rôles + surcharges par salon), stockage et transcodage des médias, recherche plein texte, notifications push, clients multi-plateformes (desktop, web, mobile).
Taille : **XL** pour tout Discord, donc à redécouper. Tranche 1 (L, un trimestre) : comptes, amis, MP, serveurs, salons texte, rôles/permissions, invitations, réactions, pièces jointes. Tranche 2 : vocal, vidéo, partage d'écran. Tranche 3 : fonctions Nitro de personnalisation, fils/forums, événements, AutoMod.

Étape suivante : `/replica-architect`.
