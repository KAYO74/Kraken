#!/usr/bin/env python3
"""Icônes KRAKEN à partir du logo fourni par Fred (replica/brand/source.png : kraken noir et turquoise sur fond vert, carré).

    python3 replica/brand/icons.py      # demande Pillow

Le logo est gardé tel quel (pas de recadrage ni de recoloration), seulement réduit.
Écrit dans replica/brand/icons/ :
- icon-{16..1024}.png, favicon.ico : le web (favicon, écran d'accueil iOS et Android, rail) ;
- 32x32.png, 128x128.png, 128x128@2x.png, icon.png, icon.ico, icon.icns : le jeu attendu par Tauri.
"""
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
OUT = HERE / "icons"

src = Image.open(HERE / "source.png").convert("RGB")
sizes = {s: src.resize((s, s), Image.LANCZOS) for s in (16, 32, 48, 64, 128, 180, 192, 256, 512, 1024)}


def png(img, path):
    """Deux couleurs et leurs bords lissés : une palette de 64 teintes suffit et divise le poids."""
    img.quantize(64, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(path, optimize=True)


OUT.mkdir(exist_ok=True)
for s, img in sizes.items():
    png(img, OUT / f"icon-{s}.png")
sizes[48].save(OUT / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)])
png(sizes[32], OUT / "32x32.png")
png(sizes[128], OUT / "128x128.png")
png(sizes[256], OUT / "128x128@2x.png")
png(sizes[512], OUT / "icon.png")
sizes[256].save(OUT / "icon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
sizes[1024].save(OUT / "icon.icns")
print("icônes écrites dans", OUT.relative_to(HERE.parents[1]))
