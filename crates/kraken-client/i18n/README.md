# Traductions de l'interface

Le **français est la clé** : `t("Ouvrir les réglages")` rend ce texte en français, et sa traduction (`en/*.tsv`)
en anglais. Un texte absent du catalogue reste en français (jamais de panne). On n'invente aucun identifiant.

- Code : `src/i18n.rs` (`t`, `tf`, `tn`, `tk!`, `LangSelect`), logique pure et testée : `kraken-core/src/i18n.rs`.
- La langue est lue une fois au démarrage (choix `kraken.lang` : `auto`, `fr`, `en`, puis `navigator.languages`,
  repli français). Changer de langue enregistre et recharge la page : rien n'est réactif.
- Contrôle : `python3 scripts/i18n-check.py` (zéro erreur avant de rendre), `--todo [fichier…]` (les textes français
  pas encore enveloppés ; heuristique : relis aussi ton fichier à la main, et `// i18n-ok` en fin de ligne écarte un
  faux positif comme un nom d'exemple), `--missing` (les lignes `français<TAB>` à compléter), `--grep texte`
  (cherche une entrée existante), `--server` (les messages du serveur, `format!` compris, que `server.tsv` ne couvre pas).

## Catalogue

`i18n/en/<groupe>.tsv`, une ligne `français<TAB>anglais`. Échappements `\n`, `\t`, `\\` (et `\#` pour un texte qui
commence par `#`) ; lignes vides et lignes `#…` ignorées. Mêmes `{variables}` des deux côtés. Pas d'espace en bord de
texte : mets l'espace dans le code (`{t("Voici")}" "<a>…`). Un texte n'a qu'**une** entrée, tous fichiers confondus
(le doublon est une erreur) : cherche d'abord (`--grep`). Groupes : `core` (socle, réglages d'affichage), `chat`,
`guilds`, `voice`, `app`, `server` (messages du serveur, voir plus bas). Un fichier de plus : le lister dans `EN`
de `src/i18n.rs`.

## Envelopper un texte

```rust
use crate::i18n::{t, tf, tn};
use crate::tk;
```

| Cas | Écriture |
| --- | --- |
| Texte dans `view!` | `{t("Ouvrir les réglages")}` (un texte avec espace ou ponctuation au bord : `{t("Voici")}" "<a>…`) |
| Attribut | `aria-label=t("Fermer")`, `title=t("…")`, `placeholder=t("…")`, `alt=t("…")` |
| Variable | `tf("{n} événements", &[("n", &n.to_string())])` ; `tf("Inviter dans {nom}", &[("nom", &name)])` (le gabarit français est la clé) |
| Pluriel | `tn("{n} membre", "{n} membres", n)` : deux clés, `{n}` rempli ; autres variables : `tnf(un, plusieurs, n, &[…])` |
| Message d'état | `notice.set(Some(t("Copié")))`, `error.set(Some(tf(…)))` |
| `format!("…")` affiché | remplace par `tf` ; `format!` ne reste que pour ce qui n'est pas du texte (URL, classe, id) |
| `const X: &str = "…"` | marque `tk!("Texte")` dans la table, traduis à l'affichage : `t(x)` ; ou fais-en une `fn x() -> String { t("…") }` |
| Table d'options `(valeur, libellé, aide)` | `("dark", tk!("Sombre"), tk!("…"))`, puis `{t(label)}` (voir `display.rs`) |
| Prop qui porte du français | `legend=tk!("Thème")`, le composant fait `{t(legend)}` ; ou prop `String` : `title=t("…")` |
| `match` qui rend du texte | `t(match x { A => tk!("un"), B => tk!("deux") })` |
| Fonction `&'static str` → texte | rends un `String` (`Dot::label`) |

`tk!("…")` rend le texte tel quel et le marque pour l'outil de contrôle, qui ne voit sinon que les littéraux passés à
`t(`, `tf(`, `tn(`, `tnf(`. Une variable ou un `format!` donné à `t(` n'est jamais vérifié : évite.
Un `t` local d'un fichier (audit.rs en avait un) se renomme : `t` est la fonction de traduction.

## Ne pas traduire

Contenu des utilisateurs (messages, noms, sujets, bios), identifiants, classes CSS, URL, clés de stockage, rôles ARIA,
valeurs `data-*`, `console`/logs/`expect`, noms de produit (KRAKEN), codes d'erreur. Les noms de touches techniques
(Ctrl, Maj) passent par le catalogue (« Maj » devient « Shift »).

Les noms des catégories et des salons d'un **modèle de serveur** ne passent pas non plus par le catalogue : le serveur
les bâtit dans la langue que le client lui envoie (`CreateGuild.lang`), et l'aperçu « Sera créé » montre les mêmes noms,
lus dans `kraken_core::templates` (`Label { fr, en }`, une seule table). Seuls le nom et la description de la carte du
modèle sont des clés du catalogue.

## Dates, heures, durées

Toute mise en forme passe par la langue active : `crate::ui::clock` / `when` (12 h ou 24 h, réglage prioritaire),
`crate::i18n::locale()` pour `Intl` (`to_locale_date_string(&locale(), …)` : jamais `"fr-FR"` écrit en dur ni
`navigator.language`), `crate::i18n::duration(secs)` et `ago(ms)` (durées et « il y a 5 min »). Plus de mois, de jours,
de « hier » écrits à la main : `t("Aujourd'hui")` / `t("Hier")` pour les mots, `Intl` pour le reste. Les chiffres sont
latins (`-u-nu-latn`). Les initiales d'avatar : `ui::initials` (premier graphème, `Intl.Segmenter`).

## Erreurs du serveur

Le serveur écrit des messages français. `api.rs` (`from_server`, appelé là où `ApiError` est lu) les traduit : le
message français est la clé, dans `en/server.tsv`. Rien ne se traduit ailleurs : `e.message` est déjà dans la bonne
langue. `--server` liste les messages du serveur que le catalogue ne couvre pas.

Un message où le serveur glisse une valeur (`format!`, une constante de `kraken_core`) y est en **gabarit** à
`{variables}`, comme pour `tf` : `Ce salon a déjà {n} messages épinglés : désépingles-en un d'abord.`. Le client le
retrouve (`kraken_core::pattern` : les morceaux fixes du gabarit, dans l'ordre, le dernier en queue du message ; le
gabarit le plus précis d'abord), en tire les valeurs et remplit le gabarit anglais. Les valeurs restent telles quelles
(un nombre, `{f}` : un nom de champ technique, un pseudo), sauf `{champ}` : un libellé français que le catalogue traduit
quand il le connaît (« Nom affiché » devient « Display name »). Règles pour écrire un message de serveur à valeur :

- une phrase entière dans chaque `format!` (pas une durée ou un suffixe collé après coup) : `--server` compare le texte
  du code au gabarit en ignorant le nom des variables, et le test de `kraken-server/src/error.rs` instancie les erreurs
  avec les vraies constantes ;
- pas deux variables côte à côte (aucune frontière, le gabarit ne reconnaît rien) ;
- un gabarit qui sert aussi au client (`Cette image est trop petite : {n} pixels de côté au moins.`) vit dans
  `server.tsv` : une seule entrée par texte.

Un nouveau message d'erreur du serveur : l'ajouter à `server.tsv` (gabarit s'il a une valeur) ; le test de couverture
(`cargo test -p kraken-server error::`) et `--server` le disent sinon.

## Droit-à-gauche

Arabe, persan, hébreu : la mise en page est prête, les traductions viennent après (aucune langue de droite à gauche
n'est encore dans `Lang`). `boot.js` pose `<html dir="rtl">` pour `ar`, `fa`, `he` (la même table que `Lang::dir`), et
**tout suit l'attribut en direct** : les tests de `e2e/specs/rtl.spec.ts` le basculent à la main (`dir="rtl"` et `lang="fa"`
dans la console) sur une interface encore française. Pour voir un écran : `document.documentElement.dir = "rtl"`.

**Feuilles de style : que des propriétés logiques.** `margin-inline-start`, `padding-inline-end`, `inset-inline-start`,
`border-inline-start`, `text-align: start`, `border-start-start-radius`, `float: inline-end`, `resize: inline`. Jamais
`left`, `right`, `margin-left`, `border-right`, `text-align: left`, `float: left`, `border-top-left-radius`. Un raccourci à
quatre valeurs aux côtés inégaux (`padding: 4px 8px 4px 12px`) s'écrit `padding-block` et `padding-inline`. Ce qui n'a pas de
version logique se multiplie par `--flip` (`1`, et `-1` sous `html[dir="rtl"]`, jetons dans `tokens.css`) :
`translateX(calc(-120% * var(--flip)))` pour un tiroir, `box-shadow: inset calc(3px * var(--flip)) 0 0 …`,
`rotate(calc(-90deg * var(--flip)))`. Un dégradé « to right », un `background-position` ou un `transform-origin` en
pourcentage de gauche se double d'un bloc `[dir="rtl"]`.

**Le garde-fou** : `python3 scripts/css-logical-check.py` (la CI le lance à côté de `replica/design/tokens.py`) échoue sur
toute propriété physique de `style/app.css` et `replica/design/components.css`. Ce qui est vraiment physique (un centrage
symétrique, une aiguille, un miroir de caméra) se garde avec `/* physical-ok: la raison */` sur **la même ligne** ; la raison
est obligatoire, et un marqueur sur une ligne sans rien de physique est refusé aussi. `-v` liste les lignes écartées.

**Icônes.** Une icône qui indique un sens (flèche, chevron, répondre, envoyer, sortir, coin fléché, lien externe, panneau
latéral) se retourne : la classe `k-flip` (`:root[dir="rtl"] .k-flip { transform: scaleX(-1) }`). `icons.rs` l'ajoute seul
aux icônes de la liste `DIRECTIONAL` ; une nouvelle icône fléchée s'y ajoute, et `<Icon flip=true …/>` force le cas rare
(une icône ordinaire qui porte une direction ici). Ne se retournent pas : lecture, micro, caméra, cloche, croix, coche,
épingle, trombone, point d'interrogation, tout ce qui n'a pas de gauche ni de droite.

**Code Rust.** `crate::i18n::is_rtl()` lit `<html dir>` **à chaque appel** (jamais dans un `static`, jamais gardé au
démarrage) : à appeler à l'événement ou au rendu. Le sens des flèches du clavier suit le sens visuel : dans une barre
d'onglets, `ArrowRight` va à l'onglet suivant en LTR et à l'onglet précédent en RTL (et inversement pour `ArrowLeft`) ;
dans la grille d'emojis, `kraken_core::grid::Move::mirrored()` ; dans la visionneuse d'images, ← passe à la suivante en
RTL. Un texte qui nomme ces flèches (`t("Précédente (←)")`) passe par `crate::i18n::mirror_arrows(…)`. Le menu contextuel
garde dans sa `MenuAt` le sens lu à l'ouverture : son bord de fin touche le curseur, et il reste dans la fenêtre. Les
menus ancrés en CSS (`inset-inline-start: calc(100% + …)`) n'ont rien à calculer. `scrollLeft` est **négatif** en RTL
(Chrome, Firefox, Safari) : ne lis jamais `scrollLeft` sans penser au signe, ni `getBoundingClientRect().left` comme « le
début ».

**Texte bidirectionnel.**

- Un champ de texte libre (message, nom de serveur, bio, nom affiché, recherche) : `dir="auto"` ; la direction suit la
  saisie. Un champ technique (pseudo, e-mail, lien, code d'invitation, mot de passe) : `dir="ltr"`.
- Un fragment technique au milieu d'une phrase (pseudo `@nom`, e-mail, adresse, code, nombre ou date) : `<bdi dir="ltr">…</bdi>`
  (les `<bdi>` de la liste des noms de la PR #31 sont gardés), ou la classe `k-ltr` sur un élément en ligne. Les liens et les
  mentions d'un message, `pre`, `code`, `samp` et `.k-kbd` sont déjà `direction: ltr; unicode-bidi: isolate`, calés au début.
- Un texte saisi par une personne (nom de rôle, de serveur, de catégorie ou de salon, nom affiché, sujet, nom de groupe) : `<bdi>…</bdi>` autour du
  texte dans le DOM (`bdi` oriente d'après les lettres du nom et l'isole : « (Équipe) Alpha! » ne devient pas « !Alpha (Équipe) »), jamais `dir="auto"`
  sur un bloc qui s'aligne (le nom suivrait le sens du nom, pas celui de l'interface). Dans une ligne qui se coupe par `…` (`.app-ellipsis`…), un `bdi`
  premier enfant porte la coupure en RTL (règle de fin de `app.css`). Dans un `<option>` ou un `title` (du texte seul) : `crate::i18n::isolated(&nom)`,
  et `crate::ui::member_label(nom, pseudo)` pour « Nom (@pseudo) ». Un jeton technique d'une phrase (`@everyone`) passe par une `{variable}` de `tf`.
- `tf` et `tnf` isolent chaque valeur insérée (U+2068 … U+2069, `kraken_core::i18n::fill_isolated`) quand la page est de droite
  à gauche : un nom en latin au milieu d'une phrase arabe ne retourne pas la ponctuation voisine. Un `@` ou un `#` collé
  devant une `{variable}` est repris dans l'isolat. Les nombres de `{n}` restent nus. Côté traduction : mets la ponctuation
  de la phrase **hors** de la variable (`« {name} : … »`), jamais une marque de sens d'écriture dans le catalogue.

**Polices et interligne.** Un bloc `:lang(ar), :lang(fa), :lang(he)` (fin de `components.css`) prend une pile de polices du
**système** : jetons `font.arabic` et `font.hebrew`, avec Inter d'abord pour le latin, puis la première police du système qui
a l'écriture. Aucun fichier de police, aucune requête réseau. `--line-scale` (1 par défaut, 1,25 en arabe et en persan, 1,1 en
hébreu) agrandit l'interligne ; l'espacement des lettres et l'italique sont coupés en arabe et en persan (écriture cursive).
Le code garde JetBrains Mono.

**Écrire un composant** : propriétés logiques (le garde-fou le dit), `dir="auto"` ou `<bdi>` autour de ce qu'un utilisateur a
écrit, `k-flip` sur ce qui pointe, `--flip` sur ce qui se décale ; puis un test de géométrie dans `rtl.spec.ts` (la boîte
est le miroir exact de sa position en LTR : `x_rtl = largeur_fenêtre - droite_ltr`).

## Conventions d'anglais

- Ton neutre et naturel, jamais mot à mot : « Se connecter » devient « Sign in », « Créer un serveur » « Create server ».
- Tutoiement : « you » ; pas d'argot ni de familiarité (« Sure », pas « Yo »).
- Ponctuation anglaise : pas d'espace avant `:`, `?`, `!`, `;` ; « … » devient "…" ; `…` (l'ellipse) est conservée.
- Casse : phrase (« Create a channel »), pas de capitales à chaque mot, sauf noms propres.
- Pluriel : deux clés (`tn`), l'anglais compte « 0 members » au pluriel.
- Libellé de bouton : un verbe d'action, court. Un nom accessible reste cohérent avec le texte visible.
- Un texte qui existe déjà garde la même traduction partout (`--grep`).
