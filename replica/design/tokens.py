#!/usr/bin/env python3
"""Génère tokens.css depuis tokens.json et vérifie le contraste WCAG 2.2 de chaque thème.

    python3 replica/design/tokens.py            # écrit tokens.css + rapport, sort 1 si un couple échoue
    python3 replica/design/tokens.py --check    # rapport seul, n'écrit rien
    python3 replica/design/tokens.py -v         # liste aussi les couples qui passent

Bibliothèque standard uniquement. Deux niveaux, pour chacun des thèmes dark, light et black :
- normal (AA) : texte 4.5:1, gros texte 3:1, interface ("ui") 3:1 ;
- élevé (AAA, data-contrast="high") : texte 7:1, gros texte 4.5:1, interface 3:1, plus les couples
  de bordures de contrast.pairs.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
NEEDS = {
    "normal": {"normal": 4.5, "large": 3.0, "ui": 3.0},
    "high": {"normal": 7.0, "large": 4.5, "ui": 3.0},
}
LEVEL_NAMES = {"normal": "normal (AA)", "high": "élevé (AAA)"}


def lum(hex_):
    h = hex_.lstrip("#")
    def ch(i):
        c = int(h[i:i + 2], 16) / 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return 0.2126 * ch(0) + 0.7152 * ch(2) + 0.0722 * ch(4)


def ratio(a, b):
    la, lb = sorted((lum(a), lum(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def resolve(cols):
    """Un rôle \"@autre\" prend la valeur de l'autre rôle (text-muted -> text-secondary en contraste élevé)."""
    return {k: (cols[v[1:]] if v.startswith("@") else v) for k, v in cols.items()}


def themes(t, level="normal"):
    """Les couleurs complètes de chaque thème, au niveau de contraste demandé."""
    base = t["color"]
    out = {"dark": base}
    for name, over in t.get("themes", {}).items():
        if not name.startswith("_"):
            out[name] = {**base, **over}
    if level == "high":
        high = t["contrast"]["high"]
        out = {name: resolve({**cols, **high.get(name, {})}) for name, cols in out.items()}
    return out


def pairs_of(t, level):
    return t["pairs"] + (t["contrast"]["pairs"] if level == "high" else [])


def report(t):
    fails = 0
    for level in ("normal", "high"):
        needs = NEEDS[level]
        pairs = pairs_of(t, level)
        for name, cols in themes(t, level).items():
            lines = []
            for p in pairs:
                fg, bg, kind = p[0], p[1], (p[2] if len(p) > 2 else "normal")
                r = ratio(cols[fg], cols[bg])
                ok = r >= needs[kind]
                fails += not ok
                if not ok or "-v" in sys.argv:
                    lines.append("  %s %5.2f:1  %-15s sur %-16s %s" % ("OK  " if ok else "FAIL", r, fg, bg, kind))
            worst = min(ratio(cols[p[0]], cols[p[1]]) / needs[p[2] if len(p) > 2 else "normal"] for p in pairs)
            print("%-6s %-12s %d couples, marge mini x%.2f" % (name, LEVEL_NAMES[level], len(pairs), worst))
            print("\n".join(lines)) if lines else None
    print("%d échec(s) (AA en normal, AAA en élevé)" % fails)
    return fails


def decls(cols, indent="  "):
    return "\n".join("%s--%s: %s;" % (indent, k, v) for k, v in cols.items() if not k.startswith("_"))


def number(v):
    return "%g" % v


def css(t):
    px = lambda v: "%gpx" % v
    rem = lambda v: "%grem" % (v / 16)
    base = {**t["color"]}
    base["font-sans"] = t["font"]["sans"]
    base["font-mono"] = t["font"]["mono"]
    base["text-scale"] = 1
    base["zoom"] = 1
    # Les unités de fenêtre sont multipliées par le zoom de body : ces longueurs valent le vrai écran.
    base["vw"] = "calc(100vw / var(--zoom))"
    base["vh"] = "calc(100vh / var(--zoom))"
    base["dvh"] = "calc(100dvh / var(--zoom))"
    for k, s in t["type"].items():
        base["text-%s-size" % k] = "calc(%s * var(--text-scale))" % rem(s["size"])
        base["text-%s-line" % k] = "calc(%s * var(--text-scale))" % rem(s["line"])
        base["text-%s-weight" % k] = s["weight"]
    for k, v in t["weight"].items():
        base["weight-%s" % k] = v
    for i, v in enumerate(t["space"]):
        base["space-%d" % i] = px(v)
    for k, v in t["radius"].items():
        base["radius-%s" % k] = px(v)
    for k, v in t["shadow"].items():
        base["shadow-%s" % k] = v
    for k, v in t["motion"].items():
        base["motion-%s" % k] = v
    for k, v in t["layout"].items():
        if not k.startswith("bp-"):
            base["size-%s" % k] = px(v)
    for k, v in t["z"].items():
        base["z-%s" % k] = v
    th = {k: v for k, v in t["themes"].items() if not k.startswith("_")}
    out = ["/* Généré par tokens.py depuis tokens.json. Ne pas éditer à la main. */",
           "/* Sur <html> : data-theme=\"dark\" (défaut) | \"light\" | \"black\" | \"auto\" ; data-text=\"14\"|\"18\"|\"20\" ; data-zoom=\"90\"|\"110\"|\"125\"|\"150\" ;",
           "   data-density=\"compact\" ; data-contrast=\"high\" ; data-motion=\"reduce\". Le client les pose avant le premier rendu (boot.js). */",
           "/* Points de rupture (pas de variables CSS dans @media) : %s */" % ", ".join(
               "%s %dpx" % (k, v) for k, v in t["layout"].items() if k.startswith("bp-")),
           "", ":root {", "  color-scheme: dark;", decls(base), "}", ""]
    for name, over in th.items():
        scheme = "light" if name == "light" else "dark"
        out += [':root[data-theme="%s"] {' % name, "  color-scheme: %s;" % scheme, decls(over), "}", ""]
    out += ["@media (prefers-color-scheme: light) {", '  :root[data-theme="auto"] {', "    color-scheme: light;",
            decls(th["light"], "    "), "  }", "}", ""]
    # Taille du texte et zoom de l'interface.
    for k, v in t["scale"]["text"].items():
        out += [':root[data-text="%s"] { --text-scale: %s; }' % (k, number(v))]
    for k, v in t["scale"]["zoom"].items():
        out += [':root[data-zoom="%s"] { --zoom: %s; }' % (k, number(v))]
    out += [""]
    # Densité compacte.
    out += [':root[data-density="compact"] {', decls({"size-%s" % k: v for k, v in t["density"]["compact"].items()}), "}", ""]
    # Contraste élevé : l'épaisseur du focus, puis un jeu de couleurs par thème (les sélecteurs ne se
    # recouvrent pas : un thème ne reçoit jamais les valeurs d'un autre).
    high = themes(t, "high")
    keys = lambda name: {k: v for k, v in high[name].items() if k in t["contrast"]["high"][name]}
    out += [':root[data-contrast="high"] { --size-focus: %s; }' % px(t["contrast"]["focus"]), "",
            ':root[data-contrast="high"]:is(:not([data-theme]), [data-theme="dark"], [data-theme="auto"]) {', decls(keys("dark")), "}"]
    for name in ("light", "black"):
        out += [':root[data-contrast="high"][data-theme="%s"] {' % name, decls(keys(name)), "}"]
    out += ["@media (prefers-color-scheme: light) {", '  :root[data-contrast="high"][data-theme="auto"] {',
            decls(keys("light"), "    "), "  }", "}", ""]
    out += ["/* Mouvement réduit : choisi dans Réglages > Accessibilité, ou suivi du système (prefers-reduced-motion) par le client. */",
            ':root[data-motion="reduce"] { --motion-fast: 0ms; --motion-base: 0ms; --motion-slow: 0ms; }', "",
            "/* Utilitaires rôle -> classe (équivalent du thème Tailwind, sans chaîne Node). */"]
    for k in t["color"]:
        out.append(".bg-%s { background-color: var(--%s); }" % (k, k))
        if k.startswith("text"):
            out.append(".%s { color: var(--%s); }" % ("text-default" if k == "text" else k, k))
        elif k.startswith("border"):
            out.append(".%s { border-color: var(--%s); }" % (k, k))
        else:
            out.append(".text-%s { color: var(--%s); }" % (k, k))
    for k in t["type"]:
        out.append(".type-%s { font-size: var(--text-%s-size); line-height: var(--text-%s-line); font-weight: var(--text-%s-weight); }" % (k, k, k, k))
    return "\n".join(out) + "\n"


def main():
    t = json.loads((HERE / "tokens.json").read_text(encoding="utf-8"))
    if "--check" not in sys.argv:
        (HERE / "tokens.css").write_text(css(t), encoding="utf-8")
        print("tokens.css écrit")
    return 1 if report(t) else 0


if __name__ == "__main__":
    sys.exit(main())
