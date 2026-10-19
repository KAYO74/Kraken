#!/usr/bin/env python3
"""Génère tokens.css depuis tokens.json et vérifie le contraste WCAG 2.2 de chaque thème.

    python3 replica/design/tokens.py            # écrit tokens.css + rapport, sort 1 si un couple échoue AA
    python3 replica/design/tokens.py --check    # rapport seul, n'écrit rien

Bibliothèque standard uniquement. Seuils : texte 4.5:1, gros texte 3:1, UI ("ui") 3:1.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
NEEDS = {"normal": 4.5, "large": 3.0, "ui": 3.0}


def lum(hex_):
    h = hex_.lstrip("#")
    def ch(i):
        c = int(h[i:i + 2], 16) / 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return 0.2126 * ch(0) + 0.7152 * ch(2) + 0.0722 * ch(4)


def ratio(a, b):
    la, lb = sorted((lum(a), lum(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def themes(t):
    base = t["color"]
    out = {"dark": base}
    for name, over in t.get("themes", {}).items():
        if not name.startswith("_"):
            out[name] = {**base, **over}
    return out


def report(t):
    fails = 0
    for name, cols in themes(t).items():
        lines = []
        for p in t["pairs"]:
            fg, bg, kind = p[0], p[1], (p[2] if len(p) > 2 else "normal")
            r = ratio(cols[fg], cols[bg])
            ok = r >= NEEDS[kind]
            fails += not ok
            if not ok or "-v" in sys.argv:
                lines.append("  %s %5.2f:1  %-15s sur %-16s %s" % ("OK  " if ok else "FAIL", r, fg, bg, kind))
        worst = min(ratio(cols[p[0]], cols[p[1]]) / NEEDS[p[2] if len(p) > 2 else "normal"] for p in t["pairs"])
        print("%-6s %d couples, marge mini x%.2f" % (name, len(t["pairs"]), worst))
        print("\n".join(lines)) if lines else None
    print("%d échec(s) AA" % fails)
    return fails


def decls(cols, indent="  "):
    return "\n".join("%s--%s: %s;" % (indent, k, v) for k, v in cols.items() if not k.startswith("_"))


def css(t):
    px = lambda v: "%gpx" % v
    rem = lambda v: "%grem" % (v / 16)
    base = {**t["color"]}
    base["font-sans"] = t["font"]["sans"]
    base["font-mono"] = t["font"]["mono"]
    for k, s in t["type"].items():
        base["text-%s-size" % k] = rem(s["size"])
        base["text-%s-line" % k] = rem(s["line"])
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
           "/* Thèmes : data-theme=\"dark\" (défaut) | \"light\" | \"black\" | \"auto\" sur <html>. */",
           "/* Points de rupture (pas de variables CSS dans @media) : %s */" % ", ".join(
               "%s %dpx" % (k, v) for k, v in t["layout"].items() if k.startswith("bp-")),
           "", ":root {", "  color-scheme: dark;", decls(base), "}", ""]
    for name, over in th.items():
        scheme = "light" if name == "light" else "dark"
        out += [':root[data-theme="%s"] {' % name, "  color-scheme: %s;" % scheme, decls(over), "}", ""]
    out += ["@media (prefers-color-scheme: light) {", '  :root[data-theme="auto"] {', "    color-scheme: light;",
            decls(th["light"], "    "), "  }", "}", "",
            "@media (prefers-reduced-motion: reduce) {", "  :root { --motion-fast: 0ms; --motion-base: 0ms; --motion-slow: 0ms; }", "}", "",
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
