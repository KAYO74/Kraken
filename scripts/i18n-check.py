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
    python3 scripts/i18n-check.py --server        les messages français du serveur absents de server.tsv

Contrôle complet. ERREURS (code de sortie 1) : un littéral passé à t(, tf(, tn(, tnf( ou tk!( sans entrée
dans une langue ; une entrée en double (dans un fichier ou entre fichiers) ; une ligne de catalogue mal
formée ; des {variables} différentes entre la clé et la traduction ; un fichier .tsv absent de la liste
`EN` de src/i18n.rs. AVERTISSEMENTS : entrée de catalogue inutilisée (sauf server.tsv, vérifié contre les
sources du serveur) ; ponctuation française dans une traduction (espace avant : ? ! ;, guillemets « »).
`--todo` et `--server` informent seulement (code de sortie 0).
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
# Fonctions dont les arguments littéraux sont des clés de traduction : nom -> numéros d'arguments (depuis 0).
KEY_ARGS = {"t": (0,), "tf": (0,), "tn": (0, 1), "tnf": (0, 1), "tk!": (0,)}


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

    Une clé : argument littéral direct de t(, tf(, tn(, tnf( ou tk!(. `tête` : le premier mot de l'élément de
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


def cmd_server():
    cat = {}
    errors = []
    if (CATALOGS / "en").is_dir():
        cat = load_catalog(CATALOGS / "en", errors)
    rows = set()
    for f, tok, names, in_test in server_literals():
        if in_test or tok.val in cat or not looks_french(tok.val) or " " not in tok.val.strip():
            continue
        if any(n in DEV_CALLS or n in ("tracing", "info!", "warn!") for n in names):
            continue
        rows.add((rel(f), tok.line, re.sub(r"\s+", " ", tok.val).strip()))
    for path, line, text in sorted(rows):
        print(f"{path}:{line}: {text if len(text) <= 110 else text[:107] + '...'}")
    print(f"-- {len(rows)} message(s) français du serveur absents du catalogue (heuristique : à juger ; "
          "un message avec une valeur interpolée reste en français)")


def cmd_missing():
    """Les clés du code sans entrée dans la première langue, en lignes « français<TAB> » prêtes à compléter."""
    errors = []
    lang_dirs = sorted(p for p in CATALOGS.iterdir() if p.is_dir())
    cat = load_catalog(lang_dirs[0], errors) if lang_dirs else {}
    seen = {}
    for f, tok, names, key, attr, after, in_test, head in client_literals():
        if key and tok.val not in cat:
            seen.setdefault(tok.val, (rel(f), tok.line))
    for text, (path, line) in sorted(seen.items(), key=lambda kv: kv[1]):
        print(f"{escape(text)}\t")
    print(f"-- {len(seen)} clé(s) sans traduction", file=sys.stderr)
    return 0


def cmd_check():
    errors, warnings = [], []
    lang_dirs = sorted(p for p in CATALOGS.iterdir() if p.is_dir()) if CATALOGS.is_dir() else []
    if not lang_dirs:
        print("aucun catalogue dans crates/kraken-client/i18n/")
        return 1
    literals = client_literals()
    keys = {}
    for f, tok, names, key, attr, after, in_test, head in literals:
        if key:
            keys.setdefault(tok.val, []).append((f, tok.line))
    i18n_rs = (SRC / "i18n.rs").read_text(encoding="utf-8")
    server_texts = {tok.val for _, tok, _, in_test in server_literals() if not in_test}
    for lang_dir in lang_dirs:
        lang = lang_dir.name
        # Chaque fichier du dossier doit être lu par le client (liste de src/i18n.rs).
        if lang == "en":
            for f in sorted(lang_dir.glob("*.tsv")):
                if f"i18n/en/{f.name}" not in i18n_rs:
                    errors.append(f"{rel(f)}: fichier absent de la liste `EN` de crates/kraken-client/src/i18n.rs")
        cat = load_catalog(lang_dir, errors)
        for key, places in sorted(keys.items(), key=lambda kv: (rel(kv[1][0][0]), kv[1][0][1])):
            if key not in cat:
                f, line = places[0]
                more = f" (+{len(places) - 1})" if len(places) > 1 else ""
                errors.append(f"{rel(f)}:{line}: [{lang}] pas de traduction pour {key!r}{more}")
        for key, (value, f, no) in cat.items():
            if placeholders(key) != placeholders(value):
                errors.append(f"{rel(f)}:{no}: [{lang}] variables différentes : {key[:50]!r} {placeholders(key)} / {placeholders(value)}")
            if lang == "en" and (re.search(r"\s[:?!;]", value) or "«" in value or "»" in value):
                warnings.append(f"{rel(f)}:{no}: [{lang}] ponctuation française dans la traduction : {value[:60]!r}")
            if f.name == "server.tsv":
                if key not in server_texts:
                    warnings.append(f"{rel(f)}:{no}: [{lang}] message absent des sources du serveur : {key[:60]!r}")
            elif key not in keys:
                warnings.append(f"{rel(f)}:{no}: [{lang}] entrée inutilisée : {key[:60]!r}")
    for e in errors:
        print("ERREUR " + e)
    for w in warnings:
        print("avertissement " + w)
    total = sum(len(load_catalog(d, [])) for d in lang_dirs)
    print(f"-- {len(keys)} clé(s) dans le code, {total} entrée(s) de catalogue, {len(errors)} erreur(s), {len(warnings)} avertissement(s)")
    return 1 if errors else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--todo", nargs="*", metavar="FICHIER", help="textes français pas encore enveloppés")
    ap.add_argument("--grep", metavar="TEXTE", help="cherche dans les catalogues")
    ap.add_argument("--server", action="store_true", help="messages du serveur absents de server.tsv")
    ap.add_argument("--missing", action="store_true", help="lignes de catalogue à compléter")
    args = ap.parse_args()
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
        return cmd_missing()
    return cmd_check()


if __name__ == "__main__":
    sys.exit(main())
