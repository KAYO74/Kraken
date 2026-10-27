# Système de design KRAKEN

Rôles, échelles et composants reconstruits depuis les captures du recon (client desktop, thème sombre). Aucun logo, icône, illustration, son, texte ni police de l'original.

| fichier | rôle |
| --- | --- |
| `tokens.json` | source unique : couleurs par rôle (3 thèmes), typo, espacements, rayons, ombres, mouvement, gabarits |
| `tokens.py` | génère `tokens.css` et vérifie le contraste WCAG de chaque thème (sort 1 sur un échec AA) |
| `tokens.css` | variables CSS + utilitaires rôle → classe (`bg-surface`, `text-muted`, `type-sm`) |
| `components.css` | primitives `k-*`, uniquement des `var(--…)` |
| `gallery.html` | toutes les primitives isolées, dans tous leurs états, avec bascule de thème (`?theme=light`) |
| `gallery-dark.png`, `gallery-light.png` | capture de contrôle de la galerie |

```bash
python3 replica/design/tokens.py        # régénère tokens.css + rapport de contraste
```

## Décisions

- **Pas de Tailwind** : l'architecture exclut Node de la chaîne de build. `tokens.css` fournit le même contrat (classes nommées par rôle, jamais de hex dans un composant) ; les composants Leptos émettent les classes `k-*`.
- **Couleur d'accent = placeholder** (`#2b7a78`, sarcelle neutre). La couleur de marque de l'original est notée comme rôle `accent` et n'est pas reprise ; `/replica-brand` fixe la vraie.
- **Gris gardés tels que mesurés** (7 surfaces + 2 bordures) : ce sont des rôles génériques, pas de la marque.
- **Accessibilité corrigée dans les jetons**, là où l'original est sous le seuil : texte indicatif `#6c6d75` (3,0 à 3,5:1) remonté à `text-faint` (≥ 4,5:1) ; champs sans bordure dotés d'une `border-input` ≥ 3:1 ; piste d'interrupteur éteint visible (3:1).
- **Thèmes** : `dark` (défaut, mesuré), `light` et `black` (déduits, non capturés), `auto` suit le système. Contraste élevé (should) viendra comme 4e thème.
- **Police** : Inter (OFL), repli Noto Sans puis système ; mono JetBrains Mono (OFL). Embarquées dans le client, jamais via CDN. L'original utilise une police propriétaire.
- **Icônes** : Lucide (ISC), via le crate `icondata` côté Leptos. Les SVG de la galerie sont des tracés simples écrits à la main.

## Mesures (1 px CSS)

| élément | valeur | capture |
| --- | --- | --- |
| barre de titre | 32 | toutes |
| rail des serveurs | 72, icône 40, rayon 12, pas 8 | 08.23.58 |
| barre latérale | 304 | 08.23.58, 08.28.07 |
| en-tête de salon | 48 | 08.28.07 |
| panneau latéral (activité) | ≈ 400 → 360 | 08.23.58 |
| saisie | min 56, marge 16, rayon 8 | 08.28.07 |
| modale réglages | nav 252, contenu 700 max, rayon 12, voile noir 72 % | 08.25.05 |
| écart entre groupes de messages | 16 (réglable 0–24) | 08.26.27 |
| zoom | 50–200 %, défaut 100 | 08.26.27 |
| texte | 12 / 14 / 16 / 20 / 24 / 32 ; messages 16/22 | toutes |

Espacements base 4 : 0, 2, 4, 8, 12, 16, 20, 24, 32, 40, 48, 64 (`--space-0` à `--space-11`). Rayons : 4, 6, 8, 12, 16, pilule. Ombres : `low`, `pop` (menus), `modal`, `scrim`. Mouvement : 100 / 180 / 280 ms, coupé par `prefers-reduced-motion`. Points de rupture : mobile < 640 (rail + barre en tiroir), tablette < 1024 (panneau membres masqué), large ≥ 1440.

## Contraste

`tokens.py` : 62 couples × 3 thèmes, **0 échec AA** (marge mini ×1,01 sombre, ×1,03 clair, ×1,12 noir). Le `contrast.py` du skill donne le même résultat sur le thème par défaut.

## Composants

Chaque bloc : variantes, tailles, états, jetons, a11y, écrans. Tout état interactif a un `:focus-visible` (anneau 2 px `accent-text`, ≥ 3:1).

```
Bouton                          .k-btn
  variants  primary, secondary (défaut), ghost, danger, link, icon
  sizes     sm 32, md 40, lg 48 ; icon 32×32
  states    défaut, survol, actif, focus-visible, désactivé (opacité .5), chargement (aria-busy, spinner), basculé (aria-pressed, icône rouge)
  tokens    primary accent/on-accent ; secondary control/text-strong ; danger danger-solid/on-danger ; radius md ; type sm/600
  a11y      vrai <button> ; icon-only = aria-label ; chargement garde le libellé pour les lecteurs (k-sr)
  used on   partout (S01, S05, S06–S15, S21, S22)

Icône de serveur                .k-server dans .k-rail
  variants  serveur (initiales ou image), accueil (--home : le logo), action (--action : bordure tiretée, accent-text)
  sizes     40, rayon md, filet 1 px border-strong
  states    défaut (surface-raised), survol (bordure accent-text), sélectionné (aria-current : fond accent-soft,
            anneau accent-text détaché de 2 px), non-lu (data-unread : point accent en haut à droite), mentions (badge count)
  tokens    surface-raised, surface-hover, accent-soft, accent-text, border-strong, bg (détache l'anneau et le point)
  a11y      <a> dans <nav aria-label="Serveurs"> ; aria-label annonce non-lu et mentions ; focus-visible décalé hors de l'anneau
  used on   S02 ; dossier de serveurs (could) = même tuile, 2×2 miniatures
  pourquoi  pas de barre blanche à gauche ni de pastille ronde qui s'arrondit au survol : la sélection est un anneau

Mon compte                      .k-me, .k-mecard
  variants  bouton avatar en bas du rail ; carte ouverte à droite du rail
  contenu   avatar + nom affiché + @pseudo, état de la connexion (point success / status-offline),
            « Réglages » (→ Compte et sécurité), « Apparence », séparateur, « Se déconnecter » (danger)
  tokens    surface-overlay, border-strong, shadow-pop, radius lg ; entrées surface-hover, danger-solid au survol
  a11y      <button aria-expanded aria-controls> « Mon compte » ; carte role="group" étiquetée ; le focus va à la
            première entrée ; Échap ou un clic ailleurs ferme, Échap rend le focus au bouton ; Tab hors de la carte ferme
  used on   S04 (remplace le panneau utilisateur flottant sous la barre latérale) ; en mobile, dans le tiroir

Élément de salon / catégorie    .k-channel, .k-category
  variants  texte (#), vocal, annonces, forum, privé (cadenas) ; catégorie repliable
  states    défaut (text-faint), survol, sélectionné (aria-current), non-lu (data-unread, gras + text-strong), muet (data-muted : couleur text-faint sans gras, cloche barrée
            .k-muted-icon ; plus d'opacité .5, qui faisait échouer le contraste), badge mentions
  tokens    surface-hover, text-faint → text-strong, radius md, hauteur 36
  a11y      <a> ; catégorie = <button aria-expanded> ; le type de salon est dans le nom accessible (« vocal », « privé »)
  used on   S05, S22, S23

Élément MP                      .k-dm
  variants  1:1 (présence), groupe (« n membres »)
  states    défaut, survol, sélectionné, non-lu (data-unread, gras + text-strong), muet (data-muted, comme un salon), badge mentions,
            bouton fermer visible au survol et au focus
  tokens    surface-hover, text-faint, text-muted (sous-ligne)
  a11y      lien + bouton fermer séparé ; fermer est atteignable au clavier
  used on   S03

Avatar + présence               .k-avatar, .k-presence
  sizes     xs 16, sm 24, md 32, base 40, lg 80, xl 120 ; groupe empilé (could)
  forme     carré à coins doux (rayon xs → xl selon la taille), pas un disque ; la pastille de présence reste ronde
  states    en ligne (success), inactif (warning), ne pas déranger (danger), hors ligne (status-offline), en stream (status-stream), mobile (glyphe)
  tokens    control (groupes, serveurs) ; une personne sans image : avatar-N / avatar-N-text (N = 1 à 8, data-tint tiré de l'id du compte, stable partout) ; anneau = --presence-ring (fond du parent)
  a11y      role="img" + aria-label « nom, statut » ; le statut n'est jamais porté par la couleur seule (forme différente par statut au build : plein, croissant, barre, creux)
  used on   partout

Champ texte / recherche         .k-input, .k-search, .k-field
  states    vide (text-faint), saisi, survol, focus-visible, erreur (aria-invalid + .k-error lié par aria-describedby), désactivé
  tokens    input, border-input (≥ 3:1), radius md, hauteur 40 (recherche 36)
  a11y      <label> toujours présent (visible ou aria-label) ; l'erreur est annoncée (aria-describedby, role="alert" à la soumission)
  used on   S01, S03, S05, S06, S15, S21, S28

Interrupteur                    .k-switch
  states    on, off, désactivé, focus-visible
  tokens    off border-input, on accent, bouton on-accent, 40×24
  a11y      <input type="checkbox" role="switch"> + <label for> ; Espace bascule
  used on   S06–S15

Radio en liste                  .k-radio dans .k-radios
  variants  titre + description
  states    coché, non coché, survol, focus-visible
  tokens    surface-overlay → surface-hover, anneau accent-text
  a11y      <fieldset> + <legend> ; flèches pour changer
  used on   S07, S08, S12, S14

Liste déroulante                .k-select
  variants  native ; listbox ARIA (combobox) quand il faut icône ou drapeau
  states    fermée, ouverte, sélection, focus-visible, désactivée
  tokens    input, border-input, radius md
  a11y      native en priorité ; la listbox perso suit le motif combobox WAI-ARIA (Échap ferme, saisie filtre)
  used on   S08, S10, S11, S14

Curseur                         .k-slider + .k-ticks
  variants  continu, à crans étiquetés (valeur par défaut marquée success)
  states    défaut, glissé, focus-visible
  tokens    accent (accent-color), text-muted (graduations)
  a11y      <input type="range"> étiqueté ; aria-valuetext pour les unités (« 100 % », « 16 px »)
  used on   S10, S12

Onglets pilule                  .k-tabs, .k-tab
  variants  texte, accent (« Ajouter »)
  states    actif (aria-selected, control), inactif, survol
  a11y      role="tablist"/"tab" ; flèches gauche/droite ; tabpanel lié
  used on   S01, S25

Badge                           .k-badge
  variants  nouveau (accent), neutre (bêta), compteur (danger-solid, « 99+ » au-delà), en direct (--live, danger-solid)
  a11y      le compteur est doublé dans le nom accessible du parent (« 4 mentions »)
  used on   S02, S03, S05, S06, S11

Bandeau                         .k-banner
  variants  info, avertissement
  states    visible, fermé (persisté par utilisateur)
  tokens    info-soft + bordure info ; mention-soft + bordure warning
  a11y      role="status" ; bouton fermer étiqueté
  used on   S01, S06, S13

Ligne de réglage                .k-setting, .k-related
  variants  contrôle à droite (interrupteur, bouton, liste) ; « réglages liés » (lien carte)
  tokens    titre text-strong, description text-muted, séparateur border
  a11y      le titre est le <label> du contrôle ; la description est reliée par aria-describedby
  used on   S06–S15

Modale réglages                 .k-scrim > .k-settings (nav + main), .k-navitem, .k-subnav
  states    ouverte, section active (aria-current), sous-section active, recherche (filtre la nav)
  tokens    surface-raised, nav surface, voile shadow-scrim, radius lg, ombre modal
  a11y      role="dialog" aria-modal ; focus piégé, Échap ferme, focus rendu au déclencheur
  used on   S06–S17 ; plein écran sous 640

Dialogue de confirmation        .k-dialog
  variants  confirmation, danger (bouton danger à droite)
  a11y      role="dialog" (alertdialog si destructif) ; focus initial sur l'action sûre
  used on   S22, F05, F11

Message                         .k-msg
  variants  normal (avatar + en-tête), suite (--cont), compact (--compact), réponse (.k-reply), système (--system)
  states    survol (fond surface-raised + barre d'actions), en édition (.k-msg-edit), mentionné (--mention : fond accent-soft arrondi, sans barre latérale), épinglé, envoi en attente (opacité .5), échec (texte danger + Réessayer)
  tokens    text, text-strong (auteur, couleur de rôle si présente), text-faint (heure), accent-soft (mention)
  a11y      <article> avec aria-labelledby auteur ; barre d'actions role="toolbar", visible au focus ; raccourcis E/R/P du S13 ; heure dans <time datetime>
  used on   S05, S19, S24, S12 (aperçu)

Nouveaux messages               .k-newline
  variants  une seule : filet sur toute la largeur, libellé « Nouveaux messages » à droite
  states    placée avant le premier message non lu des autres ; reste tant que le salon est ouvert, disparaît quand on écrit
  tokens    danger (filet et texte, 4.5:1 sur surface), type xs/600
  a11y      role="separator" avec aria-label ; le libellé visible est aria-hidden pour ne pas être lu deux fois
  used on   S05, S19

Embed                           .k-embed
  variants  lien simple, vidéo (vignette 16:9), couleur de bord (--embed-color)
  states    chargé, masqué (préférence S11), chargement (skeleton)
  tokens    surface-overlay, radius sm, bord 4
  a11y      le titre est un lien ; la vignette vidéo a un texte alternatif
  used on   S05

Pièce jointe                    .k-attachment, .k-spoiler
  variants  image, vidéo, fichier, spoiler
  states    masquée (spoiler, filtre média S08), révélée, téléversement (barre de progression)
  a11y      texte alternatif saisi à l'envoi ; spoiler = <button> « Afficher le contenu masqué »
  used on   S05, S08

Réaction                        .k-reaction
  variants  emoji, emoji perso, super réaction (animation coupée en mouvement réduit)
  states    à moi (aria-pressed, accent-soft + accent-text), pas à moi, survol (liste des réacteurs en info-bulle)
  a11y      <button aria-pressed> ; nom « emoji, n réactions, dont toi »
  used on   S05, S12

Zone de saisie                  .k-composer, .k-autocomplete
  variants  salon, MP, édition
  states    vide (placeholder text-faint), saisie, autocomplétion (emoji :, mention @, commande /), lecture seule (message d'explication), mode lent (compte à rebours), hors ligne (file d'attente)
  tokens    surface-overlay, border → border-input au focus, min 56
  a11y      <textarea> étiqueté ; autocomplétion = combobox (aria-activedescendant) ; Entrée envoie, Maj+Entrée nouvelle ligne
  used on   S05, S19

Grille de pastilles             .k-swatches, .k-swatch
  variants  thèmes, icônes d'appli, arrière-plans vidéo
  states    sélectionné (aria-checked, anneau accent-text), nouveau (badge)
  a11y      role="radiogroup" ; chaque pastille a un nom
  used on   S10, S11

Ligne de raccourci              .k-shortcut, .k-kbd
  states    défaut ; raccourci perso (bouton modifier / supprimer)
  a11y      <kbd> ; les touches sont lues dans l'ordre
  used on   S13, menus

Carte d'application autorisée   .k-card (header + <details>)
  states    repliée, dépliée, retrait (dialogue de confirmation)
  a11y      <details>/<summary> natif
  used on   S15

Menu contextuel                 .k-menu
  variants  éléments, séparateur, danger, raccourci, sous-menu (flèche)
  states    survol / focus (surface-hover, comme la carte « Mon compte » et le menu ⋯), danger survol (danger-solid)
  a11y      role="menu"/"menuitem" ; flèches, Entrée, Échap ; ouvert au clic droit et par Maj+F10
  used on   partout

Menu « ⋯ » (plus d'actions)     MoreMenu : .app-more, .k-menu.app-more-menu, .k-menuitem
  variants  entrée (icône + libellé), danger (.is-danger)
  states    survol (surface-hover), danger survol (danger-solid)
  a11y      bouton ⋯ « Plus d'actions pour <nom> » avec aria-expanded ; focus sur la 1re entrée ; Échap rend le focus au bouton ; clic ailleurs ou focus qui sort referment
  used on   S01 cartes d'amis (retirer, bloquer : jamais au rang d'écrire et d'appeler)

Info-bulle                      .k-tooltip
  a11y      role="tooltip", liée par aria-describedby ; apparaît au focus comme au survol ; jamais seul porteur d'une info
  used on   partout

Toast                           .k-toast
  variants  succès, erreur
  a11y      role="status" (erreur : role="alert") ; reste au moins 5 s, pause au survol
  used on   copie de lien, erreurs d'envoi

Tuile de participant vocal      .k-tile, .k-tile-name, .k-tile-video
  variants  avatar lg centré, ou aperçu de mon écran partagé (vidéo, object-fit contain)
  states    parle (data-speaking : contour success), en direct (data-live : bordure danger-solid)
  tokens    surface-raised, border ; nom sur shadow-scrim + on-media ; media derrière la vidéo
  a11y      <li> d'une liste « Participants » ; micro / son coupés en role="img" étiquetés ;
            « Regarder » nomme la personne (« Regarder l'écran de Lou ») et porte aria-pressed
  used on   S18

Barre de commandes d'appel      .k-callbar, .k-callbtn
  variants  bascule (micro, son : aria-pressed, danger-solid quand pressé), partage (data-on : accent),
            quitter (--danger)
  tokens    control, control-hover, danger-solid, accent ; boutons ronds de space-10
  a11y      role="group" étiqueté ; chaque bouton a son aria-label (le texte disparaît sous 640 px) ;
            partage désactivé relié à son explication par aria-describedby
  used on   S18

Panneau « En appel »            .k-voicebar
  states    en appel (success), connexion / instable (warning), perdue (danger + « Reconnecter »)
  contenu   état, salon / serveur (lien), micro et son (bascules aria-pressed), quitter
  tokens    surface-raised, bordure border-strong, radius lg
  a11y      section étiquetée « Connexion vocale » ; l'état en role="status" ; « Quitter le salon vocal »
  used on   coquille, en bas de la barre latérale, seulement pendant un appel (les bascules micro et son n'existent qu'ici
            et dans la barre de l'appel)

États vides et chargement       .k-empty, .k-skeleton
  a11y      skeleton aria-hidden + aria-busy sur la zone ; animation coupée en mouvement réduit
  used on   S01 (« rien pour l'instant »), S13, S16, toutes les listes au chargement
```

## Écrans non capturés

S18 à S30 réutilisent ces primitives. Faits au build : tuile de participant vocal, barre de commandes d'appel, panneau « En appel » (ci-dessus). Prévus, avec les mêmes jetons : popout de profil (bannière + avatar lg), liste de membres (groupes par rôle), Quick Switcher (combobox plein écran).

Étape suivante : `/replica-build`.
