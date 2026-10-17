#!/usr/bin/env python3
"""Logo KRAKEN : kraken de profil, style tribal au pochoir (encre claire pleine, négatifs noirs).

    python3 replica/brand/logo.py

Écrit replica/brand/logo.svg (icône d'appli, aussi favicon) et
crates/kraken-client/src/logo-glyph.svg (le glyphe seul, en couleur courante, pour l'appli).

Tout est calculé ici, rien n'est décalqué : chaque tentacule est une courbe de Bézier prolongée
d'une spirale (courbure qui croît), au contour effilé, avec ses ventouses côté intérieur.
L'œuvre est peinte dans un masque, dans l'ordre : blanc = encre, noir = négatif. Chaque forme
est détourée de noir, donc ce qui passe devant se découpe sur ce qui est derrière.
"""
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GAP = 9  # détourage, en unités du carré de 512
ART, BOX = [], []


def f(v):
    return f"{v:.1f}".rstrip("0").rstrip(".")


def bezier(p, n):
    out = []
    for i in range(n + 1):
        t = i / n
        a, b, c, d = (1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t * t, t ** 3
        out.append((a * p[0][0] + b * p[1][0] + c * p[2][0] + d * p[3][0],
                    a * p[0][1] + b * p[1][1] + c * p[2][1] + d * p[3][1]))
    return out


def centerline(ctrl, curl, kc, n=36, m=40, pw=1.8):
    """La Bézier, puis une spirale de longueur curl dont la courbure monte jusqu'à kc."""
    pts = bezier(ctrl, n)
    (x0, y0), (x, y) = pts[-2], pts[-1]
    t, ds = math.atan2(y - y0, x - x0), curl / m
    for i in range(1, m + 1):
        t += kc * (i / m) ** pw * ds
        x, y = x + math.cos(t) * ds, y + math.sin(t) * ds
        pts.append((x, y))
    lengths = [0.0]
    for (a, b), (c, d) in zip(pts, pts[1:]):
        lengths.append(lengths[-1] + math.hypot(c - a, d - b))
    frame = []
    for i, (x, y) in enumerate(pts):
        (ax, ay), (bx, by) = pts[max(i - 1, 0)], pts[min(i + 1, len(pts) - 1)]
        frame.append((x, y, math.atan2(by - ay, bx - ax), lengths[i] / lengths[-1]))
    return frame


def ink(d, gap=GAP):
    stroke = f' stroke="#000" stroke-width="{gap}" stroke-linejoin="round" paint-order="stroke"' if gap else ""
    ART.append(f'<path d="{d}" fill="#fff"{stroke}/>')


def cut(d):
    ART.append(f'<path d="{d}" fill="#000"/>')


def blade(a, b, w, bend=0.0):
    """Entaille effilée de a à b, épaisse de w au milieu, cintrée de bend (part de la longueur)."""
    (ax, ay), (bx, by) = a, b
    n = math.hypot(bx - ax, by - ay)
    nx, ny = -(by - ay) / n, (bx - ax) / n
    mx, my = (ax + bx) / 2 + nx * bend * n, (ay + by) / 2 + ny * bend * n
    return (f"M{f(ax)} {f(ay)}Q{f(mx + nx * w)} {f(my + ny * w)} {f(bx)} {f(by)}"
            f"Q{f(mx - nx * w)} {f(my - ny * w)} {f(ax)} {f(ay)}Z")


def tentacle(ctrl, curl, kc, w0, side=1, suck=(0.18, 0.9, 11), taper=0.85):
    """side : 1 si les ventouses sont à gauche du sens de marche (spirale anti-horaire), -1 sinon."""
    fr = centerline(ctrl, curl, kc)
    width = lambda u: w0 * (1 - u) ** taper + 1.0
    left = [(x + math.sin(t) * width(u) / 2, y - math.cos(t) * width(u) / 2) for x, y, t, u in fr]
    right = [(x - math.sin(t) * width(u) / 2, y + math.cos(t) * width(u) / 2) for x, y, t, u in fr]
    ring = left + right[::-1]
    BOX.extend(ring)
    ink("M" + " ".join(f"{f(x)} {f(y)}" for x, y in ring) + "Z")
    a, b, n = suck
    for j in range(n):
        u = a + (b - a) * j / (n - 1)
        x, y, t, _ = min(fr, key=lambda q: abs(q[3] - u))
        r = max(width(u) * 0.17, 1.6)
        off = side * (width(u) / 2 - r * 0.95)
        ART.append(f'<use href="#kraken-sucker" transform="translate({f(x + math.sin(t) * off)} '
                   f'{f(y - math.cos(t) * off)})rotate({round(math.degrees(t))})scale({f(r)})"/>')


# Bras de derrière : grande spirale à gauche, un bras qui file en bas à gauche
tentacle([(322, 256), (230, 330), (96, 380), (62, 300)], 150, 0.15, 64, side=-1, suck=(0.2, 0.88, 11))
tentacle([(330, 268), (318, 400), (196, 400), (116, 450)], 100, 0.17, 54, side=-1, suck=(0.25, 0.88, 9))
# Bras de devant, dont un qui croise
tentacle([(342, 262), (250, 340), (130, 340), (170, 452)], 110, -0.17, 58)
tentacle([(360, 270), (404, 372), (262, 392), (292, 468)], 95, -0.18, 50)

# Tête de profil : dôme balayé vers l'arrière, deux flammes qui filent vers l'arrière
ink("M446 208C454 136 404 52 312 42C246 36 184 52 132 76"
    "C188 88 232 108 262 130"
    "C226 146 180 148 148 120C178 170 240 176 282 178"
    "C246 196 198 198 166 172C200 226 264 232 302 236"
    "L306 288C330 296 350 296 370 290Z")
cut(blade((336, 120), (164, 78), 8, 0.08))
cut(blade((340, 150), (236, 132), 6, 0.08))

# Barbe : tentacules fins sous la face
for ctrl, curl, kc, w0 in [([(372, 258), (384, 330), (344, 380), (366, 432)], 55, -0.22, 30),
                           ([(400, 258), (426, 322), (398, 372), (428, 414)], 48, -0.24, 26),
                           ([(426, 252), (456, 298), (448, 342), (474, 372)], 40, -0.26, 22)]:
    tentacle(ctrl, curl, kc, w0, taper=0.9, suck=(0.3, 0.88, 7))

# Face, sans détourage : elle recouvre les racines de la barbe et des bras
ink("M320 214C340 176 420 168 448 206C460 238 446 272 410 286C376 296 330 296 300 284L300 250Z", gap=0)
cut(blade((332, 186), (454, 216), 14, -0.12))  # sourcil, qui plonge vers l'avant
cut(blade((356, 214), (428, 238), 17, 0.03))   # œil
ART.append('<circle cx="404" cy="229" r="9.5" fill="#fff"/><ellipse cx="405" cy="229" rx="2.6" ry="8.5"/>')
cut(blade((352, 258), (430, 256), 6, 0.14))    # joue

GLYPH = ('<defs><g id="kraken-sucker"><ellipse rx="1.75" ry="1.3"/><ellipse rx="1.05" ry="0.72" fill="#fff"/>'
         '<ellipse rx="0.55" ry="0.38"/></g></defs>'
         '<mask id="kraken-ink" maskUnits="userSpaceOnUse" x="0" y="0" width="512" height="512">'
         + "".join(ART) + '</mask><rect width="512" height="512" fill="currentColor" mask="url(#kraken-ink)"/>')

# Centré dans l'icône, avec une marge.
xs, ys = [x for x, _ in BOX] + [446, 132], [y for _, y in BOX] + [42]
cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
ICON = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="KRAKEN">'
        '<rect width="512" height="512" rx="112" fill="#0b0e10"/>'
        f'<g color="#f2efe8" transform="translate(256 256)scale(0.86)translate({f(-cx)} {f(-cy)})">'
        + GLYPH + '</g></svg>\n')

(ROOT / "replica/brand/logo.svg").write_text(ICON)
(ROOT / "crates/kraken-client/src/logo-glyph.svg").write_text(GLYPH + "\n")
side = max(max(xs) - min(xs), max(ys) - min(ys)) + 8
print(f'viewBox du glyphe : "{f(cx - side / 2)} {f(cy - side / 2)} {f(side)} {f(side)}"')
