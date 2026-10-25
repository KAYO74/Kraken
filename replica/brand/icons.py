#!/usr/bin/env python3
"""Icônes KRAKEN à partir du logo fourni par Fred (replica/brand/source.png : kraken noir et turquoise sur fond vert, carré).

    python3 replica/brand/icons.py      # demande Pillow

Le logo est gardé tel quel (pas de recadrage ni de recoloration), seulement réduit, avec des coins
arrondis (un masque de transparence, comme une icône d'appli).
Écrit dans replica/brand/icons/ :
- icon-{16..1024}.png, favicon.ico : le web (favicon, écran d'accueil iOS et Android, rail) ;
- 32x32.png, 128x128.png, 128x128@2x.png, icon.png, icon.ico, icon.icns : le jeu attendu par Tauri.
"""
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
OUT = HERE / "icons"

# Rayon des coins : 22 % du côté, la forme des icônes d'appli.
RADIUS = 0.22

src = Image.open(HERE / "source.png").convert("RGBA")
# Masque dessiné en grand puis réduit avec l'image : des bords lisses à toutes les tailles.
mask = Image.new("L", src.size, 0)
ImageDraw.Draw(mask).rounded_rectangle((0, 0, src.width - 1, src.height - 1), radius=int(src.width * RADIUS), fill=255)
src.putalpha(mask)
sizes = {s: src.resize((s, s), Image.LANCZOS) for s in (16, 32, 48, 64, 128, 180, 192, 256, 512, 1024)}


def png(img, path):
    """Deux couleurs, leurs bords lissés et la transparence des coins : 64 teintes suffisent."""
    img.quantize(64, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE).save(path, optimize=True)


OUT.mkdir(exist_ok=True)
for s, img in sizes.items():
    png(img, OUT / f"icon-{s}.png")
sizes[48].save(OUT / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)])
# Tauri refuse les PNG à palette : son jeu reste en RGBA.
for name, s in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)):
    sizes[s].save(OUT / name, optimize=True)
sizes[256].save(OUT / "icon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
sizes[1024].save(OUT / "icon.icns")
print("icônes écrites dans", OUT.relative_to(HERE.parents[1]))
