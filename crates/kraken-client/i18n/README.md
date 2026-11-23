# Traductions de l'interface

Le **français est la clé** : `t("Ouvrir les réglages")` rend ce texte en français, et sa traduction (`en/*.tsv`,
`fa/*.tsv`, `de/*.tsv`) en anglais, en persan ou en allemand. Un texte absent du catalogue reste en français (jamais de
panne), et le **persan et l'allemand ont une chaîne de repli** : la langue, puis l'anglais, puis le texte français (voir
« Chaîne de repli »). On n'invente aucun identifiant.

- Code : `src/i18n.rs` (`t`, `tf`, `tn`, `tk!`, `LangSelect`), logique pure et testée : `kraken-core/src/i18n.rs`.
- La langue est lue une fois au démarrage (choix `kraken.lang` : `auto`, `fr`, `en`, `fa`, `de`, puis `navigator.languages`,
  repli français). Changer de langue enregistre et recharge la page : rien n'est réactif.
- Contrôle : `python3 scripts/i18n-check.py` (zéro erreur avant de rendre), `--todo [fichier…]` (les textes français
  pas encore enveloppés ; heuristique : relis aussi ton fichier à la main, et `// i18n-ok` en fin de ligne écarte un
  faux positif comme un nom d'exemple), `--missing [--lang de]` (les lignes `français<TAB>` à compléter dans une langue),
  `--stats` (clés à traduire, entrées et part traduite par langue et par fichier), `-v` (liste aussi les clés qui
  manquent à une langue partielle), `--grep texte` (cherche une entrée existante), `--server` (les messages du serveur,
  `format!` compris, que `server.tsv` ne couvre pas).

## Catalogue

`i18n/<langue>/<groupe>.tsv`, une ligne `français<TAB>traduction`. Échappements `\n`, `\t`, `\\` (et `\#` pour un texte qui
commence par `#`) ; lignes vides et lignes `#…` ignorées. Mêmes `{variables}` des deux côtés. Pas d'espace en bord de
texte : mets l'espace dans le code (`{t("Voici")}" "<a>…`). Un texte n'a qu'**une** entrée par langue, tous fichiers confondus
(le doublon est une erreur) : cherche d'abord (`--grep`). Groupes (les mêmes six fichiers pour chaque langue) : `core`
(socle, réglages d'affichage), `chat`, `guilds`, `voice`, `app`, `server` (messages du serveur, voir plus bas). Un fichier de
plus : le lister dans `EN`, `FA` **et** `DE` de `src/i18n.rs` (le contrôle le dit sinon).

## Chaîne de repli

Un texte se cherche dans le catalogue de la langue, puis dans celui de son repli (`Lang::fallback`, `kraken-core`), puis reste
le texte français de la clé. Le français n'a pas de catalogue ; l'anglais retombe sur le français ; **le persan et l'allemand
retombent sur l'anglais**, puis sur le français (`de` → `en` → `fr`). Le catalogue persan (ou allemand) se construit donc en
chargeant d'abord les six fichiers `en/`, puis les six fichiers `fa/` (ou `de/`) par-dessus (`kraken_core::i18n::catalog_for`,
`Catalog::add_over` : la dernière entrée gagne) ; un texte pas encore traduit s'affiche en anglais, jamais en panne, et
`fa/*.tsv` ou `de/*.tsv` n'ont besoin que de ce qui est traduit (les catalogues persan et allemand sont complets : le repli
ne sert plus qu'à un texte ajouté au code et pas encore traduit, ou, pendant la construction d'une nouvelle langue, à tout ce
qui lui manque ; l'interface s'affiche alors en anglais, avec le bon `<html lang>`). `sources(lang)` de `src/i18n.rs` donne les fichiers propres à une langue, la
chaîne de repli est celle de `Lang::fallback`. Les noms des salons d'un modèle de serveur suivent la même chaîne : `.fa("…")`
ou `.de("…")` sur un `text(…)`, `voice(…)` ou `group(…)` de `kraken-core/src/templates.rs` donne le nom persan ou allemand,
sinon l'anglais. Pendant la construction d'un catalogue, un pluriel (`tn`) en repli suit la règle de la langue, pas celle de
l'anglais : pour le persan, « 0 member » au lieu de « 0 members » tant que la paire n'est pas traduite (les deux formes
persanes sont identiques, le défaut disparaît avec la traduction) ; l'allemand a la même règle que l'anglais, rien ne diffère.

`scripts/i18n-check.py` traite chaque dossier de langue : clés sans traduction, doublons, lignes mal formées et
`{variables}` différentes sont des **erreurs** pour toutes, **sauf** les clés manquantes d'une langue de `PARTIAL` (constante
en tête du script, **vide** : les catalogues anglais, persan et allemand sont complets), qui seraient comptées et signalées en
avertissement (`-v` ou `--missing --lang <code>` les listent). Une langue qu'on ajoute y entre le temps de la traduction et en
sort à la fin du travail : ses clés manquantes deviennent alors des erreurs, comme en anglais. Après un ajout du code,
`python3 scripts/i18n-check.py --missing --lang de` (ou `fa`) liste ce qui manque. Les entrées en trop (clé absente du code) restent des avertissements.

## Ajouter une langue

1. **`kraken-core/src/i18n.rs`** : une variante de `Lang`, puis chaque `match` (le compilateur les montre) : `code`, `tag`
   (BCP-47 sans région), `autonym` (le nom de la langue dans son écriture), `dir` (`Dir::Rtl` pour une langue de droite à
   gauche), `fallback` (l'anglais, en général), `plural` (règle CLDR de la langue), `Lang::ALL` (l'ordre du sélecteur).
   `from_tag` reconnaît seule `fa`, `fa-IR`, `fa_AF`, `de-AT`, `de_CH`… Tests : `tags_map_to_languages`,
   `plural_rules_follow_the_language`, `persian_and_german_fall_back_on_english_then_on_french`, à compléter.
2. **`kraken-core/src/span.rs`** : `duration` et `ago` dans la langue (mêmes cas que les autres, une branche par langue,
   chiffres latins), un test par cas ; **`templates.rs`** : un champ facultatif de plus dans `Label` (`de: Option<&'static str>`),
   son constructeur (`.de("…")` sur `Slot` et `Group`) et la branche de `Label::get` (le nom d'un salon de modèle, sinon l'anglais).
3. **Le catalogue** : `i18n/<code>/` avec les six fichiers (`core`, `chat`, `guilds`, `voice`, `app`, `server`), au début
   un commentaire `#` ; la liste de la langue dans `src/i18n.rs` (une constante de six `include_str!`, et `sources`).
   `python3 scripts/i18n-check.py --missing --lang <code>` donne les lignes `français<TAB>` à compléter.
4. **`boot.js`** : la table `langs` (celle de `Lang::ALL`) et, si la langue s'écrit de droite à gauche, `rtl` (celle de
   `Lang::dir`) : `<html lang dir>` est posé avant le client WASM, avec la même règle de résolution que `LangPref::resolve`.
5. **Le sélecteur** (`LangSelect`) lit `Lang::ALL` : la langue y apparaît seule, avec son `lang` et son nom dans son écriture.
6. **Le vérificateur** : mets la langue dans `PARTIAL` (vide aujourd'hui) tant que le catalogue est incomplet (`LANG_FALLBACK` dit son repli),
   et retire-la à la fin. Contrôles propres à une langue : `persian_problems`, `german_problems` dans `scripts/i18n-check.py`.
7. **Tests** : `e2e/specs/i18n-fa.spec.ts` (ou `i18n-de.spec.ts`, plus court, pour une langue de gauche à droite) est le modèle (sélecteur, rechargement, repli, `navigator.languages`, dates et
   durées, 390 px, axe), et les options attendues du sélecteur dans `i18n.spec.ts` et `display-accessibility.spec.ts`.
8. **Écriture de droite à gauche** : rien à écrire de plus, voir « Droit-à-gauche ».
9. **E-mails du serveur** : une branche de plus dans chacune des trois fonctions de `kraken-core/src/mails.rs` (voir « E-mails du serveur »).

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
lus dans `kraken_core::templates` (`Label { fr, en, fa, de }`, `fa` et `de` facultatifs : sans eux l'anglais ; une seule table). Seuls le nom et la description de la carte du
modèle sont des clés du catalogue.

## Dates, heures, durées

Toute mise en forme passe par la langue active : `crate::ui::clock` / `when` (12 h ou 24 h, réglage prioritaire),
`crate::i18n::locale()` pour `Intl` (`to_locale_date_string(&locale(), …)` : jamais `"fr-FR"` écrit en dur ni
`navigator.language`), `crate::i18n::duration(secs)` et `ago(ms)` (durées et « il y a 5 min »). Plus de mois, de jours,
de « hier » écrits à la main : `t("Aujourd'hui")` / `t("Hier")` pour les mots, `Intl` pour le reste. Les chiffres sont
latins (`-u-nu-latn`), dans toutes les langues, le persan compris. La locale est celle de la langue de l'interface, avec la
région du navigateur quand elle va avec (`en-GB`, `fa-AF`, `de-AT`, `de-CH`) : `fa-u-nu-latn` pour le persan, `de-u-nu-latn`
(ou `de-AT-u-nu-latn`) pour l'allemand (calendrier grégorien, « 23.11.2026 », 24 h en « Automatique »). Le calendrier est celui que `Intl`
donne par défaut à la langue : **le calendrier persan** (solaire hégirien : 2 آذر 1405, ordre « année mois jour, jour de la
semaine » pour la date longue, c'est le motif CLDR), qu'on garde. Le format 12 / 24 h en « Automatique » suit la langue :
24 h pour le persan (`h23` d'`Intl`), 12 h pour l'anglais américain, 24 h pour le français ; le choix « 12 h » ou « 24 h »
écrit « 2:30 PM » (le suffixe `AM`/`PM` est le même dans toutes les langues). `span::duration` et `span::ago` écrivent les
durées de chaque langue (persan : « 5 دقیقه پیش », « الان », « 1 ساعت و 30 دقیقه », « 2 روز » ; allemand : « vor 5 Min. »,
« gerade eben », « 1 Std. 30 Min. », « 2 Tage »). Les initiales d'avatar :
`ui::initials` (premier graphème, `Intl.Segmenter`).

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

## E-mails du serveur

Les trois e-mails du serveur (confirmation de l'adresse à l'inscription, nouveau mot de passe, confirmation d'une nouvelle
adresse) ne passent **pas** par le catalogue : ils partent dans la langue de l'interface de la personne, et leurs textes
vivent côté serveur, dans **`kraken-core/src/mails.rs`** (`verify_email`, `reset_password`, `email_change`, chacun rend un
`Mail { subject, body }` ; une branche `Lang::Fr`, `Lang::En`, `Lang::Fa`, `Lang::De` par e-mail, le français étant le texte d'origine).

- **Le chemin de la langue** : le client joint `lang` (`crate::i18n::lang().code()`) à l'inscription, à « Mot de passe oublié ? »
  et au changement d'adresse (`api.rs`, champ facultatif de `RegisterReq`, `EmailReq` et `ChangeEmail`) ; le serveur le lit comme
  celui de `CreateGuild` (`api::request_lang`, `Lang::from_tag` : `fa-IR` vaut `fa`, absent ou inconnu vaut le français) et met le
  code normalisé (`fr|en|fa|de`) dans la charge utile de la tâche ; `jobs.rs` le relit (`payload_lang`) et ne contient aucun texte.
  Une tâche sans `lang` (déjà en file avant la 0.24.0) part en français.
- **Ajouter une langue** : après la variante de `Lang` (voir « Ajouter une langue »), le compilateur montre les trois `match` de
  `mails.rs` à compléter ; les tests de ce fichier parcourent `Lang::ALL` et vérifient chaque langue (lien seul sur sa ligne,
  pseudo, KRAKEN, aucune `{variable}` restante, chiffres, lettres et marques propres à la langue). Une langue que `Lang` ne connaît pas (`nl`) n'a pas de
  modèle : sa demande reçoit les e-mails en français.
- **Allemand** : tutoiement (« du », « dein »), noms en majuscule, mots composés avec trait d'union (« KRAKEN-Konto »,
  « E-Mail-Adresse »), aucune ponctuation française, le lien seul sur sa ligne. Les sujets : « Bestätige deine KRAKEN-E-Mail-Adresse »,
  « Neues KRAKEN-Passwort », « Bestätige deine neue KRAKEN-E-Mail-Adresse » (« Conventions d'allemand »).
- **Persan** : mêmes conventions que l'interface (« Conventions de persan ») : forme « شما » sous-entendue, impératifs polis,
  chiffres latins (« 24 ساعت », « 30 دقیقه »), « ی » et « ک » persans, demi-espace entre deux lettres seulement, « ، ؟ ؛ », aucune
  marque de sens d'écriture, KRAKEN et le pseudo en latin, le lien seul sur sa ligne.
- **Contrôle** : `cargo test -p kraken-core mails`, `cargo test -p kraken-server jobs::` (sujet RFC 2047 en UTF-8 et corps UTF-8
  de chaque langue) et `cargo test -p kraken-server --test mail_lang` (la langue envoyée, normalisée dans la file).

## Droit-à-gauche

Le **persan (`fa`) est la première langue de droite à gauche livrée** ; l'arabe et l'hébreu suivront avec leurs traductions
(l'allemand, de gauche à droite, n'a rien de plus à faire ici : `Dir::Ltr`, `boot.js` pose `dir="ltr"`).
`Lang::dir` donne `Dir::Rtl` pour `fa`, `init()` (`src/i18n.rs`) pose `<html lang="fa" dir="rtl">` au démarrage et `boot.js`
le pose avant le client (même table : `rtl` pour `ar`, `fa`, `he`), puis **tout suit l'attribut en direct**. Les tests de
`e2e/specs/rtl.spec.ts` basculent `dir="rtl"` et `lang="fa"` à la main sur une interface française pour la géométrie ;
`e2e/specs/i18n-fa.spec.ts` vérifie la vraie langue (sélecteur, rechargement, `navigator.languages`). Pour voir un écran :
choisis « فارسی » dans Réglages › Langue et heure, ou `document.documentElement.dir = "rtl"` dans la console. Un texte qui
n'est pas encore traduit en persan s'affiche en anglais (repli) dans une page de droite à gauche : sa ponctuation de fin
saute de côté (« .Sign in »), c'est le comportement du texte bidirectionnel, qui disparaît avec la traduction.

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
  Un texte qui devient une **donnée** (valeur d'un champ qui part au serveur, nom enregistré) se remplit avec `tf_data(` : même
  gabarit, jamais d'isolat (le serveur refuse ces marques dans un nom).

**Polices et interligne.** Un bloc `:lang(ar), :lang(fa), :lang(he)` (fin de `components.css`) prend une pile de polices du
**système** (la langue persane y entre par `<html lang="fa">`, et chaque `<option lang="fa">` du sélecteur) : jetons `font.arabic` et `font.hebrew`, avec Inter d'abord pour le latin, puis la première police du système qui
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

## Conventions de persan

- Ton neutre et naturel, jamais mot à mot ; un seul registre (tutoiement ou forme de politesse) dans toute l'interface, choisi
  par la personne qui traduit et noté en tête de `fa/core.tsv`.
- **Chiffres latins** (0-9), jamais persans (۰-۹) ni arabo-indiens (٠-٩) : c'est le choix de toute l'interface (`-u-nu-latn`) et
  `scripts/i18n-check.py` refuse les autres dans une traduction. Le nombre d'un pluriel est un `{n}` latin.
- **Lettres persanes** : « ی » (U+06CC) et « ک » (U+06A9), jamais le « ي » (U+064A) et le « ك » (U+0643) arabes (erreur du
  contrôle).
- **Demi-espace** (ZWNJ, U+200C) pour joindre les morceaux d'un mot qui ne se lient pas : « می‌خواهم », « کتاب‌خانه »,
  « برنامه‌ها ». Jamais en tête ni en queue d'un texte ou d'une ligne (erreur), jamais doublé ni collé à une espace
  (avertissement). Le client ne l'ajoute pas : `kraken_core::text::has_hidden` le laisse passer entre deux lettres dans un nom.
- Ponctuation persane : « ، » (virgule), « ؟ » (point d'interrogation), « ؛ » (point-virgule), guillemets « » ; le point et
  les deux-points sont ceux du latin. **Jamais de marque de sens d'écriture** (U+200E, U+200F, U+061C, U+202A à U+202E,
  U+2066 à U+2069 : erreur du contrôle) : `tf` isole les valeurs insérées quand la page est de droite à gauche, la ponctuation
  de la phrase se met **hors** de la `{variable}`.
- Pluriel : deux clés comme partout (`tn`), le persan n'accorde pas le nom après un nombre : les deux traductions sont
  identiques (« {n} عضو »). Règle CLDR : « un » pour 0 et 1, « autre » sinon.
- Les noms de produit et les termes techniques (KRAKEN, WebRTC, `@pseudo`) restent en latin dans la phrase persane ; un
  fragment technique isolé garde `<bdi dir="ltr">` côté code.
- Un texte qui existe déjà garde la même traduction partout (`--grep`).

## Conventions d'allemand

L'allemand (`de`, « Deutsch », de gauche à droite) est la quatrième langue de l'interface. Son catalogue (`de/*.tsv`) se
construit par-dessus l'anglais (repli `de` → `en` → `fr`) ; il est complet (1 503 textes, comme l'anglais et le persan) et `PARTIAL` est vide. Écrit
par Claude, sans relecture par une personne de langue allemande : une relecture est la bienvenue.

- **Tutoiement** (« du », « dein », « dir »), comme le français « tu » : jamais le « Sie » de politesse ni « Ihr/Ihre ». Ton
  naturel et court, jamais mot à mot ; pas de forme genrée à astérisque, à deux-points ou à trait bas : une formulation neutre
  (« Mitglieder », « Personen », « jemand ») ou le pluriel. Même mot pour la même chose partout (`--grep`).
- **Majuscule aux noms** (« Nachricht », « Einstellungen »), minuscule aux verbes, adjectifs et adverbes. Les libellés de
  boutons sont un verbe à l'infinitif ou un nom court (« Senden », « Server erstellen »), sans point final. **« ß »** et les
  trémas (ä, ö, ü) s'écrivent tels quels, jamais « ss » (sauf nom propre) ni « ae/oe/ue ». Mots composés d'un seul tenant
  (« Sprachkanal », « Audit-Log » avec trait d'union devant un terme étranger ou un sigle : « E-Mail-Adresse », « KRAKEN-Konto »,
  « GIF-Suche »), jamais « Sprach Kanal » (pas d'espace dans un composé).
- **Ponctuation allemande, jamais française** : aucune espace (insécable, fine ou ordinaire) avant `:`, `?`, `!`, `;` ;
  **« … » de la source devient „…“** (guillemets bas ouvrants et hauts fermants, U+201E et U+201C ; citation dans une
  citation : ‚…‘). Un exemple de code ou une saisie littérale garde ses guillemets droits ou sa mise en forme d'origine. L'ellipse
  `…` est conservée. Après deux-points, une phrase entière prend une majuscule (« Achtung: Das wird gelöscht. »). Le
  contrôle (`german_problems`) refuse l'espace insécable ou fine avant `: ? ! ;` (erreur) et signale l'espace ordinaire et les
  guillemets « » (avertissement).
- **Chiffres latins**, comme partout. Le nombre d'un pluriel est un `{n}` latin ; **deux clés** (`tn`), règle CLDR de
  l'anglais : « un » pour 1 seulement (« 0 Nachrichten », « 1 Nachricht », « 2 Nachrichten »). Abréviations de durée : « Min. »,
  « Std. », « s », « Tage » (`kraken_core::span`) ; « gerade eben », « vor 5 Min. ». Les dates et heures viennent d'`Intl` (24 h).
- **Noms de produit et termes techniques latins, jamais traduits** : KRAKEN, WebRTC, GIF, URL, HTTPS, QR, Markdown, @pseudo.
  « Server » reste « Server » (jamais « Gilde »).
- **Glossaire commun** (le français est le texte du code, la colonne de droite est la traduction à employer partout) :

| Français | Allemand | Remarque |
| --- | --- | --- |
| serveur | Server | pluriel « Server » ; jamais « Gilde » |
| salon, salon textuel, salon vocal | Kanal, Textkanal, Sprachkanal | « Kanäle » au pluriel |
| catégorie | Kategorie | |
| message, message privé (MP) | Nachricht, Direktnachricht | « Direktnachrichten » pour la liste des MP |
| amis, ami | Freunde, Freund | formulation neutre quand c'est possible (« Freundesliste ») |
| membre(s) | Mitglied(er) | |
| rôle(s) | Rolle(n) | |
| permissions | Berechtigungen | |
| bloquer | Blockieren | entre personnes ; « Sperren » n'est pas employé (trop proche du ban) |
| bannir | Bannen | « Bann » pour la mesure |
| expulser | Rauswerfen | « Kick » seulement dans une liste de droits calquée sur la source |
| exclure temporairement | Timeout | verbe : « in den Timeout schicken » ; durée : « Timeout für 10 Min. » |
| sourdine, couper le micro | Stummschalten | « stummgeschaltet » (état) |
| invitation | Einladung | « Einladungslink » pour le lien |
| emoji | Emoji | |
| réaction | Reaktion | |
| épingler, désépingler | Anheften, Nicht mehr anheften | « angeheftete Nachrichten » ; « Lösen » accepté en bouton court |
| réglages | Einstellungen | |
| compte, mon compte | Konto, Mein Konto | |
| profil | Profil | |
| pseudo | Benutzername | |
| nom affiché | Anzeigename | |
| mot de passe | Passwort | |
| e-mail | E-Mail | « E-Mail-Adresse » |
| En ligne, Hors ligne, Ne pas déranger, Invisible, Absent | Online, Offline, Bitte nicht stören, Unsichtbar, Abwesend | |
| appel | Anruf | « Anruf beenden » pour raccrocher |
| caméra | Kamera | |
| partager l'écran | Bildschirm teilen | |
| micro | Mikrofon | |
| haut-parleur, sortie audio | Lautsprecher | |
| journal d'audit | Audit-Log | « Protokoll » pour un journal ordinaire |
| modèle (de serveur) | Vorlage | |
| thème (apparence) | Thema | sujet d'un salon : « Kanalthema » (pour ne pas confondre) |

- **Mise en page** : l'allemand a de longs mots composés. La feuille de style (`style/app.css`, fin du fichier) coupe le texte
  courant par césure automatique (`:lang(de) { hyphens: auto }` sur les paragraphes, aides, libellés de carte ; jamais sur les
  champs de saisie, le code, les pseudos ni les liens) et laisse les libellés étroits (boutons, onglets, pastilles) passer à
  la ligne ou se couper plutôt que déborder ; un bouton à texte peut donc faire deux lignes. Si un composé déborde quand
  même, **ne coupe pas le mot à la main** : propose une traduction plus courte (« Benachrichtigungen » plutôt que
  « Benachrichtigungseinstellungen verwalten ») ou signale le texte au designer.
- Un texte qui existe déjà garde la même traduction partout (`--grep`).
