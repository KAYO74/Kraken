#!/usr/bin/env python3
"""Garde-fou droit-à-gauche : les feuilles de style du client n'emploient que des propriétés LOGIQUES.

    python3 scripts/css-logical-check.py            # échoue (code 1) à la première propriété physique
    python3 scripts/css-logical-check.py -v         # liste aussi les lignes écartées par un marqueur
    python3 scripts/css-logical-check.py fichier…   # d'autres feuilles que celles de la liste par défaut

Bibliothèque standard uniquement. Vérifie `crates/kraken-client/style/app.css` et `replica/design/components.css` :
une ligne qui dit « gauche » ou « droite » en dur (margin-left, padding-right, border-left, left:, right:,
text-align: left|right, float, border-radius par coin, resize: horizontal, dégradé « to right », translateX sans
`var(--flip)`, ombre décalée en x, raccourci à 4 valeurs aux côtés inégaux…) casse la mise en page en arabe, en
persan et en hébreu. On écrit à la place `margin-inline-start`, `inset-inline-end`, `text-align: start`,
`border-start-start-radius`, `calc(… * var(--flip))`… (`--flip` vaut 1, et -1 sous `html[dir="rtl"]`, voir tokens.css).

Ce qui est vraiment physique (un centrage symétrique, une aiguille qui tourne, un miroir de caméra) se garde avec un
marqueur sur LA MÊME ligne : `/* physical-ok: la raison */` (la raison est obligatoire). Le mode d'emploi est dans
crates/kraken-client/i18n/README.md, section « Droit-à-gauche ».
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FILES = ["crates/kraken-client/style/app.css", "replica/design/components.css"]

MARK = re.compile(r"/\*\s*physical-ok:\s*(\S[^*]*?)\s*\*/")
SIDE = r"(?:left|right)"

# (motif sur « propriété: valeur », explication courte). Le motif voit le texte sans les commentaires.
PROPS = re.compile(
    r"(?<![\w-])("
    r"margin-(?:left|right)|padding-(?:left|right)|"
    r"border-(?:left|right)(?:-[a-z]+)*|"
    r"border-(?:top|bottom)-(?:left|right)-radius|"
    r"scroll-(?:margin|padding)-(?:left|right)|"
    r"left|right"
    r")\s*:"
)
VALUE_RULES = [
    ("text-align", re.compile(r"^\s*(left|right)\b"), "text-align : start ou end"),
    ("float", re.compile(r"^\s*(left|right)\b"), "float : inline-start ou inline-end"),
    ("clear", re.compile(r"^\s*(left|right)\b"), "clear : inline-start ou inline-end"),
    ("resize", re.compile(r"^\s*(horizontal|both)\b"), "resize : inline, block ou vertical"),
    ("transform-origin", re.compile(r"\b(left|right)\b"), "transform-origin : variable --flip ou bloc [dir=rtl]"),
    ("background-position", re.compile(r"\b(left|right)\b"), "background-position : bloc [dir=rtl]"),
    ("background", re.compile(r"\b(?:no-repeat|repeat-\w)\s+(?:left|right)\b|\b(?:left|right)\s+[\d.]+\w+\s+(?:top|center|bottom)\b"),
     "background (position) : bloc [dir=rtl]"),
    ("object-position", re.compile(r"\b(left|right)\b"), "object-position : bloc [dir=rtl]"),
]
GRADIENT = re.compile(r"\bto\s+(?:left|right)\b|\bto\s+(?:top|bottom)\s+(?:left|right)\b|\bto\s+(?:left|right)\s+(?:top|bottom)\b")
TRANSLATE_X = re.compile(r"translate(?:X|3d)?\(([^()]*(?:\([^()]*\)[^()]*)*)\)")
SHADOW_FIRST = re.compile(r"^\s*(?:inset\s+)?(-?[\d.]+(?:px|rem|em)?|calc\(.*?\)|var\([^)]*\))")
EDGE4 = {"margin", "padding", "border-width", "border-radius", "inset", "scroll-margin", "scroll-padding"}


def strip_comments(text):
    """Le texte sans commentaires ; garde les sauts de ligne (les numéros de ligne restent justes)."""
    return re.sub(r"/\*.*?\*/", lambda m: re.sub(r"[^\n]", " ", m.group(0)), text, flags=re.S)


def split_values(value):
    """Les valeurs d'un raccourci, séparées par des espaces hors parenthèses."""
    out, depth, cur = [], 0, ""
    for ch in value.strip():
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch.isspace() and depth == 0:
            if cur:
                out.append(cur)
                cur = ""
        else:
            cur += ch
    if cur:
        out.append(cur)
    return out


def declarations(line):
    """(propriété, valeur) de chaque déclaration de la ligne (les `;` hors parenthèses séparent)."""
    out = []
    for part in re.split(r";(?![^(]*\))", line):
        m = re.search(r"(?:^|[{\s])([a-z-]+)\s*:\s*(.*?)\s*(?:}\s*)*$", part)
        if m and not part.strip().startswith("@"):
            out.append((m.group(1), m.group(2)))
    return out


def problems(code):
    """Les défauts d'une ligne de CSS sans commentaire : liste d'explications."""
    found = []
    for m in PROPS.finditer(code):
        prop = m.group(1)
        # `left:` ou `right:` ne comptent que comme propriété (pas dans `:is(…)` ni une valeur) : début de déclaration.
        before = code[: m.start()].rstrip()
        if prop in ("left", "right") and before and before[-1] not in "{;":
            continue
        found.append("`%s` : propriété physique (version logique : %s)" % (prop, logical(prop)))
    for prop, value in declarations(code):
        for name, rx, why in VALUE_RULES:
            if prop == name and rx.search(value):
                found.append("`%s: %s` : %s" % (prop, value, why))
        if GRADIENT.search(value):
            found.append("dégradé « to left|right » : `%s` (variable --flip ou bloc [dir=rtl])" % value)
        for tm in TRANSLATE_X.finditer(value):
            if "--flip" not in tm.group(1) and re.search(r"[^\s0()]", tm.group(1).replace("%", "")) and not re.fullmatch(r"\s*0(px)?\s*", tm.group(1)):
                if tm.group(0).startswith("translateY"):
                    continue
                found.append("`%s` : décalage en x sans `* var(--flip)`" % tm.group(0))
        if re.search(r"scaleX\(", value) or re.search(r"scale\(\s*-", value):
            found.append("`%s` : miroir en x (une icône directionnelle prend la classe k-flip ; un vrai miroir, physical-ok)" % value)
        for rm in re.finditer(r"rotate\(([^()]*(?:\([^()]*\)[^()]*)*)\)", value):
            if "--flip" not in rm.group(1) and re.search(r"(?<![\d.])(?:-?90|-?270)deg", rm.group(1)):
                found.append("`%s` : quart de tour sans `* var(--flip)` (un chevron pointe de l'autre côté en RTL)" % rm.group(0))
        if prop == "box-shadow":
            for one in re.split(r",(?![^(]*\))", value):
                first = SHADOW_FIRST.match(one)
                if first and not re.fullmatch(r"0(px)?", first.group(1)) and "--flip" not in first.group(1) and not first.group(1).startswith("var("):
                    found.append("box-shadow décalée en x : `%s` (calc(… * var(--flip)))" % one.strip())
        if prop in EDGE4:
            vals = split_values(value.split("!important")[0].split("/")[0] if prop != "border-radius" else value.split("!important")[0].split("/")[0])
            if prop == "border-radius":
                if len(vals) == 3 and vals[0] != vals[1] or len(vals) == 4 and (vals[0] != vals[1] or vals[2] != vals[3]):
                    found.append("border-radius à coins inégaux : `%s` (border-start-start-radius…)" % value)
            elif prop == "inset":
                if len(vals) == 4 and vals[1] != vals[3]:
                    found.append("inset à gauche et droite inégaux : `%s` (inset-inline-start/end)" % value)
            elif len(vals) == 4 and vals[1] != vals[3]:
                found.append("`%s: %s` : gauche et droite inégaux (version logique : %s-inline-start/end)" % (prop, value, prop))
    return found


def logical(prop):
    if prop in ("left", "right"):
        return "inset-inline-" + ("start" if prop == "left" else "end")
    side = "start" if "left" in prop else "end"
    for p in ("margin", "padding", "border", "scroll-margin", "scroll-padding"):
        if prop.startswith(p + "-") and ("left" in prop or "right" in prop) and "radius" not in prop:
            return prop.replace("left", "inline-start").replace("right", "inline-end")
    if "radius" in prop:
        v = "start" if "top" in prop else "end"
        return "border-%s-%s-radius" % (v, side)
    return side


def check(path, verbose=False):
    raw = path.read_text(encoding="utf-8")
    lines = raw.splitlines()
    plain = strip_comments(raw).splitlines()
    bad, waived = [], []
    for n, (line, code) in enumerate(zip(lines, plain), 1):
        mark = MARK.search(line)
        issues = problems(code)
        if mark and issues:
            waived.append((n, mark.group(1), issues))
        elif issues:
            bad.append((n, line.strip(), issues))
        elif "physical-ok" in line and not mark:
            bad.append((n, line.strip(), ["marqueur `physical-ok` sans raison (`/* physical-ok: la raison */`)"]))
        elif mark and not issues:
            bad.append((n, line.strip(), ["marqueur `physical-ok` inutile : rien de physique sur cette ligne"]))
    return bad, waived


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    verbose = "-v" in sys.argv
    files = [Path(a) for a in args] if args else [ROOT / f for f in FILES]
    failures = 0
    for path in files:
        if not path.is_file():
            print("introuvable : %s" % path)
            failures += 1
            continue
        bad, waived = check(path, verbose)
        shown = path.relative_to(ROOT) if path.is_absolute() and ROOT in path.parents else path
        for n, line, issues in bad:
            failures += 1
            print("%s:%d: %s" % (shown, n, "; ".join(issues)))
            print("    %s" % (line if len(line) < 160 else line[:157] + "…"))
        if verbose:
            for n, why, issues in waived:
                print("%s:%d: écarté (%s) : %s" % (shown, n, why, "; ".join(issues)))
        print("%s : %d ligne(s) physique(s), %d écartée(s) par physical-ok" % (shown, len(bad), len(waived)))
    if failures:
        print("\n%d défaut(s) : propriétés logiques (margin-inline-start, inset-inline-end, text-align: start…) ou, "
              "pour du vraiment physique, `/* physical-ok: raison */` sur la ligne." % failures)
        return 1
    print("css-logical-check : 0 défaut")
    return 0


if __name__ == "__main__":
    sys.exit(main())
