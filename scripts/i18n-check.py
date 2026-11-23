#!/usr/bin/env python3
"""Contrôle des traductions de l'interface (Python 3 standard, sans dépendance).

Le français écrit dans le code est la clé ; `crates/kraken-client/i18n/<langue>/<groupe>.tsv` donne sa
traduction (une ligne « français<TAB>traduction », échappements \\n \\t \\\\ \\#, lignes vides et # ignorées).
Mode d'emploi et conventions : crates/kraken-client/i18n/README.md.

    python3 scripts/i18n-check.py                 contrôle complet (zéro erreur attendu)
    python3 scripts/i18n-check.py --todo          les textes français pas encore enveloppés, par fichier
    python3 scripts/i18n-check.py --todo shell.rs auth.rs     seulement ces fichiers (nom ou fin de chemin)
                                                  (`// i18n-ok` en fin de ligne : français à ne pas traduire, ignoré)
    python3 scripts/i18n-check.py --grep Fermer   les entrées de catalogue qui contiennent ce texte
    python3 scripts/i18n-check.py --missing       les lignes « français<TAB> » à compléter (clés sans entrée anglaise)
    python3 scripts/i18n-check.py --missing --lang fa     idem pour une autre langue (le persan : tout ce qui reste à traduire)
    python3 scripts/i18n-check.py --stats         clés à traduire, entrées et part traduite, par langue et par fichier
    python3 scripts/i18n-check.py -v              le contrôle complet, avec la liste des clés manquantes d'une langue partielle
    python3 scripts/i18n-check.py --server        les messages français du serveur (format! compris) que server.tsv ne couvre pas

Contrôle complet, pour **chaque** langue qui a un dossier sous `i18n/` (`en`, `fa`). ERREURS (code de sortie 1) : un
littéral passé à t(, tf(, tf_data(, tn(, tnf( ou tk!( sans entrée dans la langue ; un nom d'emoji, un texte de modèle ou un message
du serveur sans entrée ; une entrée en double (dans un fichier ou entre fichiers) ; une ligne de catalogue mal formée ;
des {variables} différentes entre la clé et la traduction ; une marque de sens d'écriture (U+200E, U+200F, U+061C,
U+202A à U+202E, U+2066 à U+2069) dans une traduction ; un fichier .tsv absent de la liste du client (`EN`, `FA` de
src/i18n.rs) ; un dossier de langue que `kraken_core::i18n::Lang` ne connaît pas. Pour le persan, en plus : un chiffre
persan ou arabo-indien (۰-۹, ٠-٩ : les chiffres sont latins), un demi-espace (U+200C) en tête ou en queue d'une ligne,
un « ي » ou un « ك » arabes (le persan écrit « ی » et « ک »).

AVERTISSEMENTS : entrée de catalogue inutilisée (clé absente du code, sauf server.tsv, vérifié contre les sources du
serveur) ; ponctuation française dans une traduction anglaise (espace avant : ? ! ;, guillemets « ») ; demi-espace collé à
une espace ou doublé (persan) ; et, **tant que la langue est dans `PARTIAL`**, ses clés sans traduction (comptées, et
listées avec -v ou --missing --lang) : une langue partielle retombe sur son repli (`Lang::fallback`), la page marche déjà.
`PARTIAL` se vide à la fin du travail de traduction : les clés manquantes deviennent alors des erreurs, comme en anglais.
`--todo`, `--server`, `--missing` et `--stats` informent seulement (code de sortie 0).
"""
import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLIENT = ROOT / "crates" / "kraken-client"
SRC = CLIENT / "src"
CATALOGS = CLIENT / "i18n"
SERVER_DIRS = [ROOT / "crates" / "kraken-server" / "src", ROOT / "crates" / "kraken-db" / "src"]
# Les noms français des emojis du sélecteur (donnée du noyau) : le client les traduit à l'affichage avec `t(nom)`.
EMOJI_SRC = ROOT / "crates" / "kraken-core" / "src" / "emoji.rs"
# Les modèles de serveur (donnée du noyau) : le nom et la description d'une carte sont traduits à l'affichage ; les noms
# des catégories et des salons, eux, existent déjà dans les deux langues dans le noyau (`Label`, jamais au catalogue).
TEMPLATES_SRC = ROOT / "crates" / "kraken-core" / "src" / "templates.rs"
# Langues dont le catalogue est encore en construction : leurs clés manquantes sont des avertissements (comptés, listés avec
# -v ou --missing --lang), pas des erreurs. À RETIRER à la fin de la PR qui ajoute la langue (le persan : PR #33).
PARTIAL: set[str] = set()
# La langue de référence : elle porte tous les messages du serveur (server.tsv), que chaque autre langue doit traduire aussi.
REFERENCE = "en"
# Le repli d'une langue (même table que `Lang::fallback` dans le noyau), pour le dire dans les messages.
LANG_FALLBACK = {"fa": "en"}
# Le noyau, où `Lang::code` dit quelles langues existent (chaque dossier de i18n/ doit y avoir son code).
CORE_I18N = ROOT / "crates" / "kraken-core" / "src" / "i18n.rs"
# Persan : chiffres persans (۰-۹) et arabo-indiens (٠-٩), lettres arabes ي et ك (le persan écrit ی et ک), demi-espace.
INDIC_DIGITS = re.compile("[\u06f0-\u06f9\u0660-\u0669]")
ARABIC_LETTERS = {"\u064a": "ی (U+06CC)", "\u0643": "ک (U+06A9)"}
ZWNJ = "\u200c"
# Marques de sens d'écriture : jamais dans un catalogue (le client isole les valeurs lui-même, voir README « Droit-à-gauche »).
BIDI_MARKS = re.compile("[\u200e\u200f\u061c\u202a-\u202e\u2066-\u2069]")
# Fonctions dont les arguments littéraux sont des clés de traduction : nom -> numéros d'arguments (depuis 0).
KEY_ARGS = {"t": (0,), "tf": (0,), "tf_data": (0,), "tn": (0, 1), "tnf": (0, 1), "tk!": (0,)}


# ───────────── lecture du code Rust ─────────────

class Tok:
    __slots__ = ("kind", "val", "line")

    def __init__(self, kind, val, line):
        self.kind, self.val, self.line = kind, val, line


ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}


def decode(raw):
    """Le contenu d'un littéral Rust « normal » (échappements, continuation de ligne `\\` + saut)."""
    out, i = [], 0
    while i < len(raw):
        c = raw[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        i += 1
        d = raw[i] if i < len(raw) else ""
        if d in ESCAPES:
            out.append(ESCAPES[d])
            i += 1
        elif d == "u":
            m = re.match(r"u\{([0-9a-fA-F_]+)\}", raw[i:])
            if m:
                out.append(chr(int(m.group(1).replace("_", ""), 16)))
                i += m.end()
            else:
                i += 1
        elif d == "x":
            out.append(chr(int(raw[i + 1:i + 3], 16)))
            i += 3
        elif d in "\n\r":
            while i < len(raw) and raw[i] in " \t\r\n":
                i += 1
        else:
            i += 1
    return "".join(out)


def tokenize(text):
    """Les jetons utiles (identifiants, littéraux, ponctuation), sans les commentaires."""
    toks, i, n, line = [], 0, len(text), 1
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
        elif c.isspace():
            i += 1
        elif text.startswith("//", i):
            while i < n and text[i] != "\n":
                i += 1
        elif text.startswith("/*", i):
            depth = 0
            while i < n:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                    if depth == 0:
                        break
                else:
                    line += text[i] == "\n"
                    i += 1
        elif c == '"' or (c in "rb" and re.match(r'(br|rb|r|b)(#*)"', text[i:i + 12]) and not _ident_before(text, i)):
            m = re.match(r'(br|rb|r|b)?(#*)"', text[i:i + 12])
            prefix, hashes = m.group(1) or "", m.group(2)
            start, first_line = i + m.end(), line
            if "r" in prefix:
                end = text.find('"' + hashes, start)
                end = n if end < 0 else end
                raw, i = text[start:end], end + 1 + len(hashes)
                val = raw
            else:
                j = start
                while j < n and text[j] != '"':
                    j += 2 if text[j] == "\\" else 1
                raw, i = text[start:j], j + 1
                val = decode(raw)
            line += raw.count("\n")
            toks.append(Tok("str", val, first_line))
        elif c == "'":
            m = re.match(r"'(\\.[^']*|[^\\'])'", text[i:i + 12])
            if m:
                toks.append(Tok("char", m.group(0), line))
                i += m.end()
            else:
                m = re.match(r"'[A-Za-z_][A-Za-z0-9_]*", text[i:])
                toks.append(Tok("life", m.group(0) if m else "'", line))
                i += m.end() if m else 1
        elif c.isalpha() or c == "_":
            m = re.match(r"[A-Za-z_][A-Za-z0-9_]*", text[i:])
            toks.append(Tok("id", m.group(0), line))
            i += m.end()
        elif c.isdigit():
            m = re.match(r"[0-9][0-9A-Za-z_.]*", text[i:])
            toks.append(Tok("num", m.group(0), line))
            i += m.end()
        else:
            toks.append(Tok("p", c, line))
            i += 1
    return toks


def _ident_before(text, i):
    return i > 0 and (text[i - 1].isalnum() or text[i - 1] == "_")


class Frame:
    __slots__ = ("name", "args")

    def __init__(self, name):
        self.name, self.args = name, 0


def test_ranges(toks):
    """Les indices des jetons des éléments `#[cfg(test)]` et `#[test]` (leur attribut et leur corps)."""
    skip, i = set(), 0
    while i < len(toks):
        if toks[i].val == "#" and i + 1 < len(toks) and toks[i + 1].val == "[":
            j, depth = i + 1, 0
            while j < len(toks):
                depth += toks[j].kind == "p" and toks[j].val == "["
                depth -= toks[j].kind == "p" and toks[j].val == "]"
                if depth == 0:
                    break
                j += 1
            attr = "".join(t.val for t in toks[i:j + 1] if t.kind != "str")
            if re.search(r"cfg\(test\)|\[(wasm_bindgen_)?test\]", attr):
                k, d = j + 1, 0
                while k < len(toks):
                    if toks[k].val == "{":
                        d += 1
                    elif toks[k].val == "}":
                        d -= 1
                        if d <= 0:
                            break
                    elif toks[k].val == ";" and d == 0:
                        break
                    k += 1
                skip.update(range(i, k + 1))
            i = j + 1
        else:
            i += 1
    return skip


def scan(toks):
    """Pour chaque littéral : (jeton, noms des appels qui l'enferment, est-ce une clé, attribut, suite, test, tête).

    Une clé : argument littéral direct de t(, tf(, tf_data(, tn(, tnf( ou tk!(. `tête` : le premier mot de l'élément de
    premier niveau (`const`, `static`, `fn`, `struct`…).
    """
    out, stack = [], []
    skip = test_ranges(toks)
    head, at_start = "", True
    for i, t in enumerate(toks):
        if not stack and at_start:
            if t.val in ("pub", "#", "!") or (t.val == "[" and i and toks[i - 1].val == "#"):
                pass
            else:
                head, at_start = t.val, False
        if t.kind == "p" and t.val in "([{":
            prev = toks[i - 1] if i else None
            name = ""
            if prev is not None and prev.kind == "id":
                name = prev.val
            elif prev is not None and prev.val == "!" and i >= 2 and toks[i - 2].kind == "id":
                name = toks[i - 2].val + "!"
            stack.append(Frame(name))
        elif t.kind == "p" and t.val in ")]}":
            if stack:
                stack.pop()
            if not stack and t.val == "}":
                at_start = True
        elif t.kind == "p" and t.val == ";" and not stack:
            at_start = True
        elif t.kind == "p" and t.val == "," and stack:
            stack[-1].args += 1
        elif t.kind == "str":
            top = stack[-1] if stack else None
            key = bool(top and top.name in KEY_ARGS and top.args in KEY_ARGS[top.name]
                       and toks[i - 1].kind == "p" and toks[i - 1].val in "(,")
            attr = None
            if i >= 2 and toks[i - 1].val == "=" and toks[i - 1].kind == "p":
                parts, j = [], i - 2
                while j >= 0 and (toks[j].kind == "id" or toks[j].val == "-"):
                    parts.append(toks[j].val)
                    j -= 1
                attr = "".join(reversed(parts))
            after = "".join(x.val for x in toks[i + 1:i + 6] if x.kind != "str")
            before = toks[i - 2].val + toks[i - 1].val if i >= 2 else (toks[i - 1].val if i else "")
            out.append((t, [f.name for f in stack], key, attr, after, i in skip, head + "|" + before))
    return out


def rs_files(base):
    return sorted(p for p in base.rglob("*.rs"))


def rel(p):
    return str(p.relative_to(ROOT))


# ───────────── catalogues ─────────────

def unescape(cell):
    out, i = [], 0
    while i < len(cell):
        c = cell[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        if i + 1 >= len(cell) or cell[i + 1] not in "nt\\#":
            return None
        out.append({"n": "\n", "t": "\t", "\\": "\\", "#": "#"}[cell[i + 1]])
        i += 2
    return "".join(out)


def escape(text):
    return text.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t")


def load_catalog(lang_dir, errors):
    """{clé: (valeur, fichier, ligne)} pour une langue ; signale mal formé et doublons."""
    entries = {}
    for f in sorted(lang_dir.glob("*.tsv")):
        for no, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
            if not line.strip() or line.startswith("#"):
                continue
            cells = line.split("\t")
            if len(cells) != 2:
                errors.append(f"{rel(f)}:{no}: ligne mal formée (une seule tabulation attendue) : {line[:70]!r}")
                continue
            key, value = unescape(cells[0]), unescape(cells[1])
            if key is None or value is None or not key or not value:
                errors.append(f"{rel(f)}:{no}: ligne mal formée (texte vide ou échappement inconnu) : {line[:70]!r}")
                continue
            if key in entries:
                _, other, other_no = entries[key]
                errors.append(f"{rel(f)}:{no}: doublon de {key[:60]!r} (déjà dans {rel(other)}:{other_no})")
                continue
            entries[key] = (value, f, no)
    return entries


def placeholders(text):
    return sorted(re.findall(r"\{([A-Za-z0-9_]+)\}", text))


def shape(text):
    """Un texte où chaque `{…}` est anonyme : `{MAX_PINS}` du serveur et `{n}` du catalogue se reconnaissent."""
    return re.sub(r"\{[^{}]*\}", "{}", text)


# ───────────── heuristique « français non enveloppé » ─────────────

FRENCH_WORDS = set("""
le la les un une des du de d l et ou où à au aux en dans sur sous avec sans pour par pas ne n ni
ce cet cette ces mon ma mes ton ta tes son sa ses notre votre leur nos vos leurs
je tu il elle on nous vous ils elles me te se lui y qui que qu quoi dont
est es sont être avoir as ai a avez ont peux peut peuvent veux doit dois faut fait faire
plus moins très trop aussi déjà encore toujours jamais bien mal tout tous toute toutes
ici là alors donc mais car si quand comme depuis pendant avant après vers chez entre
fermer ouvrir annuler envoyer enregistrer supprimer modifier valider retour suivant précédent
rejoindre quitter créer ajouter retirer copier coller choisir sélectionner afficher masquer
oui non aucun aucune autre autres nouveau nouvelle nouveaux ancien
serveur serveurs salon salons membre membres message messages ami amis appel compte
mot passe réglages paramètres langue heure jour jours minute minutes seconde secondes
connexion déconnecter connecter erreur impossible réessayer chargement
""".split())
ACCENTS = re.compile(r"[àâäçéèêëîïôöùûüœæÀÂÇÉÈÊËÎÏÔÙÛÜŒÆ«»]")
TECHNICAL = re.compile(r"^[\w\-./:#=%\[\]{}()@&?,;+*|\\<>$!~^\"']*$")
TEXT_ATTRS = {"aria-label", "aria-description", "aria-roledescription", "aria-placeholder", "aria-valuetext",
              "title", "placeholder", "alt", "label", "legend", "hint", "sub", "summary", "heading"}
VIEW_NAMES = {"view!", "view"}
SHOWN_CALLS = {"format!", "set", "try_set", "Some", "Err", "push_str", "push", "write!", "writeln!"}
DEV_CALLS = {"expect", "panic!", "assert!", "assert_eq!", "assert_ne!", "debug_assert!", "unreachable!",
             "todo!", "unimplemented!", "println!", "eprintln!", "console_error!", "log!", "debug!", "info!",
             "warn!", "error!", "trace!", "log", "error", "warn", "info", "debug", "log_1", "error_1", "warn_1"}


def looks_french(text):
    if not re.search(r"[A-Za-zÀ-ÿ]{2}", text):
        return False
    if ACCENTS.search(text):
        return True
    words = re.findall(r"[a-zàâçéèêëîïôùûü]+", text.lower().replace("'", " ").replace("’", " "))
    if " " not in text.strip() and TECHNICAL.match(text) and not (len(words) == 1 and words[0] in FRENCH_WORDS and text[:1].isupper()):
        return False
    return any(w in FRENCH_WORDS and len(w) > 1 for w in words) or (len(words) == 1 and words[0] in FRENCH_WORDS)


def candidate(tok, names, key, attr, after, in_test, head):
    """Ce littéral est-il du français affiché, pas encore enveloppé ?"""
    head, _, before = head.partition("|")
    if key or in_test or not looks_french(tok.val):
        return False
    if any(n in DEV_CALLS for n in names):
        return False
    in_view = any(n in VIEW_NAMES for n in names)
    if in_view:
        return attr is None or attr in TEXT_ATTRS
    if attr in TEXT_ATTRS:
        return True
    if any(n in SHOWN_CALLS for n in names):
        return True
    if after.startswith((".into()", ".to_string()", ".to_owned()")):
        return True
    # Résultat d'un `match` (`=> "texte"`) ou bloc réduit à un littéral (`{ "texte" }`).
    return head in ("const", "static") or before.endswith("=>") or (before.endswith("{") and after.startswith("}"))


# ───────────── commandes ─────────────

def client_literals():
    found = []
    for f in rs_files(SRC):
        toks = tokenize(f.read_text(encoding="utf-8"))
        for tok, names, key, attr, after, in_test, head in scan(toks):
            found.append((f, tok, names, key, attr, after, in_test, head))
    return found


def server_literals():
    out = []
    for base in SERVER_DIRS:
        for f in rs_files(base):
            for tok, names, key, attr, after, in_test, head in scan(tokenize(f.read_text(encoding="utf-8"))):
                out.append((f, tok, names, in_test))
    return out


_OK = {}


def ok_lines(f):
    """Les lignes d'un fichier marquées `// i18n-ok` : du français qui n'est pas à traduire (un nom d'exemple…)."""
    if f not in _OK:
        text = f.read_text(encoding="utf-8").splitlines()
        _OK[f] = {n for n, line in enumerate(text, 1) if "i18n-ok" in line}
    return _OK[f]


def cmd_todo(filters):
    rows = []
    for f, tok, names, key, attr, after, in_test, head in client_literals():
        if filters and not any(f.name == x or rel(f).endswith("/" + x) for x in filters):
            continue
        if tok.line in ok_lines(f):
            continue
        if candidate(tok, names, key, attr, after, in_test, head):
            rows.append((rel(f), tok.line, re.sub(r"\s+", " ", tok.val).strip()))
    rows.sort()
    for path, line, text in rows:
        shown = text if len(text) <= 100 else text[:97] + "..."
        print(f"{path}:{line}: {shown}")
    files = len({r[0] for r in rows})
    print(f"-- {len(rows)} texte(s) probablement français non enveloppé(s) dans {files} fichier(s) (heuristique : à juger)")


def cmd_grep(needle):
    errors = []
    low = needle.lower()
    hits = 0
    for lang_dir in sorted(p for p in CATALOGS.iterdir() if p.is_dir()):
        for key, (value, f, no) in load_catalog(lang_dir, errors).items():
            if low in key.lower() or low in value.lower():
                print(f"{rel(f)}:{no}: {key!r} -> {value!r}")
                hits += 1
    print(f"-- {hits} entrée(s)")


SQL = re.compile(r"\s*(select|insert|update|delete|with|alter|create|drop|from)\b", re.I)
# Les appels qui fabriquent une erreur d'API : une phrase qui finit par un point y est un message pour l'utilisateur.
ERROR_CALLS = {"invalid", "AppError", "Self"}


def cmd_server():
    """Les messages français du serveur (`format!` compris) que le client ne sait pas traduire : ni dans `server.tsv`
    tels quels, ni couverts par un de ses gabarits à `{variables}` (comparés sans le nom des variables)."""
    cat = {}
    errors = []
    if (CATALOGS / "en").is_dir():
        cat = load_catalog(CATALOGS / "en", errors)
    known = set(cat) | {shape(k) for k in cat}
    rows = set()
    for f, tok, names, in_test in server_literals():
        text = tok.val
        if in_test or text in known or shape(text) in known or tok.line in ok_lines(f):
            continue
        # Pas des messages d'API : la base ne donne que des raisons (`DbError::Invalid("pin_limit")`), les tests et la
        # graine des textes de démonstration, les tâches de fond et les aperçus de liens leurs journaux et leurs e-mails.
        if "kraken-db" in f.parts or f.name.startswith("tests") or f.name in ("seed.rs", "jobs.rs", "fetch.rs"):
            continue
        if SQL.match(text) or any(n in DEV_CALLS or n in ("tracing", "info!", "warn!") for n in names):
            continue
        sentence = any(n in ERROR_CALLS for n in names) and text.rstrip().endswith((".", "…", "!", "?")) and " " in text.strip()
        if not sentence and (not looks_french(text) or " " not in text.strip()):
            continue
        rows.add((rel(f), tok.line, re.sub(r"\s+", " ", text).strip()))
    for path, line, text in sorted(rows):
        print(f"{path}:{line}: {text if len(text) <= 110 else text[:107] + '...'}")
    print(f"-- {len(rows)} message(s) français du serveur absents du catalogue (heuristique : à juger ; un message avec une "
          "valeur glissée doit avoir son gabarit à {variables} dans server.tsv ; `// i18n-ok` en fin de ligne écarte un "
          "texte qui n'est pas pour l'interface)")


def lang_dirs():
    """Les dossiers de langue sous i18n/ (`en`, `fa`), triés."""
    return sorted(p for p in CATALOGS.iterdir() if p.is_dir()) if CATALOGS.is_dir() else []


def known_langs():
    """Les codes de `Lang::code` dans le noyau (`fr`, `en`, `fa`) : chaque dossier de i18n/ doit y avoir le sien."""
    if not CORE_I18N.is_file():
        return None
    m = re.search(r"pub fn code\(self\) -> &'static str \{(.*?)\n    \}", CORE_I18N.read_text(encoding="utf-8"), re.S)
    return set(re.findall(r'"([a-z]+)"', m.group(1))) if m else None


class Needs:
    """Ce que le catalogue d'une langue doit couvrir : les textes du code, les noms d'emojis, les textes de modèles et
    les messages du serveur (ceux de `server.tsv` de la langue de référence)."""

    def __init__(self, catalogs):
        self.keys = {}
        for f, tok, names, key, attr, after, in_test, head in client_literals():
            if key:
                self.keys.setdefault(tok.val, []).append((f, tok.line))
        self.emojis = emoji_names()
        self.templates = template_names()
        ref = catalogs.get(REFERENCE, {})
        self.server = {k: (f, no) for k, (v, f, no) in ref.items() if f.name == "server.tsv"}

    def all(self):
        return set(self.keys) | self.emojis | self.templates | set(self.server)

    def missing(self, cat):
        """[(où, clé, ce que c'est, texte affiché)] pour ce que `cat` ne couvre pas, une fois par clé, dans l'ordre : le
        code, les emojis, les modèles, le serveur."""
        out, seen = [], set()

        def add(place, key, shown, what):
            if key not in cat and key not in seen:
                seen.add(key)
                out.append((place, key, shown, what))

        for key, places in sorted(self.keys.items(), key=lambda kv: (rel(kv[1][0][0]), kv[1][0][1])):
            f, line = places[0]
            more = f" (+{len(places) - 1})" if len(places) > 1 else ""
            add(f"{rel(f)}:{line}", key, f"{key!r}{more}", "le texte")
        for n in sorted(self.emojis):
            add(rel(EMOJI_SRC), n, repr(n), "le nom d'emoji")
        for n in sorted(self.templates):
            add(rel(TEMPLATES_SRC), n, repr(n), "le texte de modèle")
        for k, (f, no) in sorted(self.server.items(), key=lambda kv: kv[1][1]):
            add(f"{rel(f)}:{no}", k, repr(k), "le message du serveur")
        return out


def load_all(errors):
    return {d.name: load_catalog(d, errors) for d in lang_dirs()}


def cmd_missing(lang):
    """Les clés sans entrée dans une langue (la première par défaut), en lignes « français<TAB> » prêtes à compléter."""
    errors = []
    dirs = lang_dirs()
    if lang is None:
        lang = dirs[0].name if dirs else None
    if lang is None or not (CATALOGS / lang).is_dir():
        print(f"pas de catalogue pour la langue {lang!r} (dossiers : {', '.join(d.name for d in dirs)})", file=sys.stderr)
        return 1
    catalogs = load_all(errors)
    needs = Needs(catalogs)
    seen = {}
    for place, key, shown, what in needs.missing(catalogs[lang]):
        seen.setdefault(key, place)
    for key in seen:
        print(f"{escape(key)}\t")
    print(f"-- {len(seen)} clé(s) sans traduction en {lang}", file=sys.stderr)
    return 0


def emoji_names():
    """Les noms d'emojis de `kraken-core` : `("👍", "pouce levé, d'accord"),` dans la constante `EMOJIS`."""
    if not EMOJI_SRC.is_file():
        return set()
    text = EMOJI_SRC.read_text(encoding="utf-8")
    start = text.find("pub const EMOJIS")
    end = text.find("];", start)
    body = text[start:end] if start >= 0 and end > start else ""
    return {decode(m.group(1)) for m in re.finditer(r'\(\s*"(?:[^"\\]|\\.)*"\s*,\s*"((?:[^"\\]|\\.)*)"\s*\)', body)}


def template_names():
    """Les textes français des cartes de modèles de `kraken-core` : `name:` et `description:` de la constante `ALL`.
    Les noms des catégories et des salons (`group("fr", "en", …)`, `text("fr", "en")`, `voice("fr", "en")`) portent déjà
    leur anglais dans le noyau : le catalogue ne les traduit pas."""
    if not TEMPLATES_SRC.is_file():
        return set()
    text = TEMPLATES_SRC.read_text(encoding="utf-8")
    start = text.find("pub const ALL")
    end = text.find("\n];", start)
    body = text[start:end] if start >= 0 and end > start else ""
    lit = r'"((?:[^"\\]|\\.)*)"'
    found = re.findall(r"\b(?:name|description):\s*" + lit, body)
    return {decode(x) for x in found}


def persian_problems(key, value):
    """Ce qui ne va pas dans une traduction persane (les chiffres sont latins, le demi-espace joint deux lettres)."""
    found = []
    if INDIC_DIGITS.search(value):
        found.append(("erreur", "chiffre persan ou arabo-indien (۰-۹, ٠-٩) : les chiffres sont latins (0-9)"))
    for bad, good in ARABIC_LETTERS.items():
        if bad in value:
            found.append(("erreur", f"lettre arabe {bad} (U+{ord(bad):04X}) : le persan écrit {good}"))
    for line in value.split("\n"):
        if line.startswith(ZWNJ) or line.endswith(ZWNJ):
            found.append(("erreur", "demi-espace (U+200C) en tête ou en queue d'une ligne"))
            break
    if ZWNJ * 2 in value or re.search(ZWNJ + r"\s|\s" + ZWNJ, value):
        found.append(("avertissement", "demi-espace doublé ou collé à une espace"))
    return found


def cmd_check(verbose=False):
    errors, warnings = [], []
    dirs = lang_dirs()
    if not dirs:
        print("aucun catalogue dans crates/kraken-client/i18n/")
        return 1
    catalogs = load_all(errors)
    needs = Needs(catalogs)
    i18n_rs = (SRC / "i18n.rs").read_text(encoding="utf-8")
    server_texts = {shape(tok.val) for _, tok, _, in_test in server_literals() if not in_test}
    codes = known_langs()
    coverage = []
    for lang_dir in dirs:
        lang = lang_dir.name
        cat = catalogs[lang]
        if codes is not None and lang not in codes:
            errors.append(f"{rel(lang_dir)}: dossier de langue inconnu de `Lang::code` ({rel(CORE_I18N)}) : {sorted(codes)}")
        # Chaque fichier du dossier doit être lu par le client (liste `EN`, `FA`… de src/i18n.rs).
        for f in sorted(lang_dir.glob("*.tsv")):
            if f"i18n/{lang}/{f.name}" not in i18n_rs:
                errors.append(f"{rel(f)}: fichier absent de la liste de la langue dans crates/kraken-client/src/i18n.rs")
        missing = needs.missing(cat)
        required = needs.all()
        coverage.append((lang, len(required) - len(missing), len(required)))
        if lang in PARTIAL:
            # Langue en construction : ses clés manquantes sont comptées, pas fatales (le repli les couvre).
            if missing:
                examples = ", ".join(shown if len(shown) <= 40 else shown[:37] + "..." for _, _, shown, _ in missing[:3])
                warnings.append(
                    f"[{lang}] catalogue partiel (`PARTIAL` dans scripts/i18n-check.py) : {len(missing)} clé(s) sur {len(required)} sans "
                    f"traduction, repli {LANG_FALLBACK.get(lang, 'français')} ; `--missing --lang {lang}` les liste (premières : {examples})")
            if verbose:
                for place, _, shown, what in missing:
                    warnings.append(f"{place}: [{lang}] pas de traduction pour {what} {shown}")
        else:
            for place, _, shown, what in missing:
                errors.append(f"{place}: [{lang}] pas de traduction pour {what} {shown}")
        for key, (value, f, no) in cat.items():
            if placeholders(key) != placeholders(value):
                errors.append(f"{rel(f)}:{no}: [{lang}] variables différentes : {key[:50]!r} {placeholders(key)} / {placeholders(value)}")
            if BIDI_MARKS.search(value):
                errors.append(f"{rel(f)}:{no}: [{lang}] marque de sens d'écriture dans la traduction (le client isole les valeurs lui-même) : {value[:60]!r}")
            if lang == "en" and (re.search(r"\s[:?!;]", value) or "«" in value or "»" in value):
                warnings.append(f"{rel(f)}:{no}: [{lang}] ponctuation française dans la traduction : {value[:60]!r}")
            if lang == "fa":
                for level, what in persian_problems(key, value):
                    (errors if level == "erreur" else warnings).append(f"{rel(f)}:{no}: [{lang}] {what} : {value[:60]!r}")
            if f.name == "server.tsv":
                if shape(key) not in server_texts:
                    warnings.append(f"{rel(f)}:{no}: [{lang}] message absent des sources du serveur : {key[:60]!r}")
            elif key not in needs.keys and key not in needs.emojis and key not in needs.templates:
                warnings.append(f"{rel(f)}:{no}: [{lang}] entrée inutilisée : {key[:60]!r}")
    for e in errors:
        print("ERREUR " + e)
    for w in warnings:
        print("avertissement " + w)
    total = sum(len(c) for c in catalogs.values())
    print(f"-- {len(needs.keys)} clé(s) dans le code, {total} entrée(s) de catalogue, {len(errors)} erreur(s), {len(warnings)} avertissement(s)")
    print("-- couverture : " + ", ".join(
        f"{lang} {done}/{need}" + (" (partiel)" if lang in PARTIAL else "") for lang, done, need in coverage))
    return 1 if errors else 0


def percent(done, total):
    return f"{100 * done / total:5.1f} %".replace(".", ",") if total else "  n/a"


def cmd_stats():
    """Les clés à traduire et, par langue, les entrées par fichier et la part traduite."""
    errors = []
    catalogs = load_all(errors)
    needs = Needs(catalogs)
    required = needs.all()
    print(f"À traduire : {len(required)} clé(s) = {len(needs.keys)} texte(s) du code, {len(needs.emojis)} nom(s) d'emoji, "
          f"{len(needs.templates)} texte(s) de modèle, {len(needs.server)} message(s) du serveur (certains se recoupent)")
    files = sorted({f.name for d in lang_dirs() for f in d.glob("*.tsv")})
    head = f"{'langue':<8}" + "".join(f"{f[:-4]:>8}" for f in files) + f"{'entrées':>9}{'traduites':>11}{'manquantes':>12}{'part':>10}"
    print(head)
    for lang, cat in catalogs.items():
        by_file = {f: sum(1 for _, ff, _ in cat.values() if ff.name == f) for f in files}
        done = len(required & set(cat))
        label = lang + ("*" if lang in PARTIAL else "")
        print(f"{label:<8}" + "".join(f"{by_file[f]:>8}" for f in files)
              + f"{len(cat):>9}{done:>11}{len(required) - done:>12}{percent(done, len(required)):>10}")
    if PARTIAL & set(catalogs):
        print("* langue partielle (`PARTIAL`) : ses clés manquantes sont des avertissements, le repli les affiche")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--todo", nargs="*", metavar="FICHIER", help="textes français pas encore enveloppés")
    ap.add_argument("--grep", metavar="TEXTE", help="cherche dans les catalogues")
    ap.add_argument("--server", action="store_true", help="messages du serveur absents de server.tsv")
    ap.add_argument("--missing", action="store_true", help="lignes de catalogue à compléter")
    ap.add_argument("--lang", metavar="CODE", help="avec --missing : la langue (la première par défaut)")
    ap.add_argument("--stats", action="store_true", help="clés à traduire, entrées et part traduite par langue")
    ap.add_argument("-v", "--verbose", action="store_true", help="liste aussi les clés manquantes d'une langue partielle")
    args = ap.parse_args()
    if args.lang and not args.missing:
        ap.error("--lang va avec --missing")
    if args.todo is not None:
        cmd_todo(args.todo)
        return 0
    if args.grep:
        cmd_grep(args.grep)
        return 0
    if args.server:
        cmd_server()
        return 0
    if args.missing:
        return cmd_missing(args.lang)
    if args.stats:
        return cmd_stats()
    return cmd_check(args.verbose)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BrokenPipeError:  # `| head` : le lecteur a fermé le tube, ce n'est pas une erreur
        sys.stderr.close()
        sys.exit(0)
