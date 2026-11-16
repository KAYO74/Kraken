# Traductions de l'interface

Le **français est la clé** : `t("Ouvrir les réglages")` rend ce texte en français, et sa traduction (`en/*.tsv`)
en anglais. Un texte absent du catalogue reste en français (jamais de panne). On n'invente aucun identifiant.

- Code : `src/i18n.rs` (`t`, `tf`, `tn`, `tk!`, `LangSelect`), logique pure et testée : `kraken-core/src/i18n.rs`.
- La langue est lue une fois au démarrage (choix `kraken.lang` : `auto`, `fr`, `en`, puis `navigator.languages`,
  repli français). Changer de langue enregistre et recharge la page : rien n'est réactif.
- Contrôle : `python3 scripts/i18n-check.py` (zéro erreur avant de rendre), `--todo [fichier…]` (les textes français
  pas encore enveloppés ; heuristique : relis aussi ton fichier à la main, et `// i18n-ok` en fin de ligne écarte un
  faux positif comme un nom d'exemple), `--missing` (les lignes `français<TAB>` à compléter), `--grep texte`
  (cherche une entrée existante), `--server` (les messages du serveur pas encore dans `server.tsv`).

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

## Dates, heures, durées

Toute mise en forme passe par la langue active : `crate::ui::clock` / `when` (12 h ou 24 h, réglage prioritaire),
`crate::i18n::locale()` pour `Intl` (`to_locale_date_string(&locale(), …)` : jamais `"fr-FR"` écrit en dur ni
`navigator.language`), `crate::i18n::duration(secs)` et `ago(ms)` (durées et « il y a 5 min »). Plus de mois, de jours,
de « hier » écrits à la main : `t("Aujourd'hui")` / `t("Hier")` pour les mots, `Intl` pour le reste. Les chiffres sont
latins (`-u-nu-latn`). Les initiales d'avatar : `ui::initials` (premier graphème, `Intl.Segmenter`).

## Erreurs du serveur

Le serveur écrit des messages français statiques. `api.rs` (`from_server`, appelé là où `ApiError` est lu) les traduit
avec `t()` : le message français est la clé, dans `en/server.tsv`. Un message avec une valeur glissée dedans reste en
français. Rien ne se traduit ailleurs : `e.message` est déjà dans la bonne langue. `--server` liste les messages du
serveur qui n'y sont pas encore.

## Conventions d'anglais

- Ton neutre et naturel, jamais mot à mot : « Se connecter » devient « Sign in », « Créer un serveur » « Create server ».
- Tutoiement : « you » ; pas d'argot ni de familiarité (« Sure », pas « Yo »).
- Ponctuation anglaise : pas d'espace avant `:`, `?`, `!`, `;` ; « … » devient "…" ; `…` (l'ellipse) est conservée.
- Casse : phrase (« Create a channel »), pas de capitales à chaque mot, sauf noms propres.
- Pluriel : deux clés (`tn`), l'anglais compte « 0 members » au pluriel.
- Libellé de bouton : un verbe d'action, court. Un nom accessible reste cohérent avec le texte visible.
- Un texte qui existe déjà garde la même traduction partout (`--grep`).
