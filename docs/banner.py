"""Bannière du README (docs/images/banniere.png) : le logo, tel quel aux coins arrondis, et le nom.

Usage : python3 docs/banner.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
W, H = 1600, 480
BG, SURFACE, ACCENT, TEXT, MUTED = "#121214", "#1a1a1e", "#5cc2bd", "#fbfbfb", "#abacb2"
FONTS = Path("/usr/share/fonts/opentype/inter")


def font(name, size):
    return ImageFont.truetype(str(FONTS / f"Inter-{name}.otf"), size)


img = Image.new("RGB", (W, H), BG)
d = ImageDraw.Draw(img)
d.rounded_rectangle((24, 24, W - 24, H - 24), radius=36, fill=SURFACE)

logo = Image.open(ROOT / "replica/brand/source.png").convert("RGBA").resize((320, 320), Image.LANCZOS)
mask = Image.new("L", logo.size, 0)
ImageDraw.Draw(mask).rounded_rectangle((0, 0, 319, 319), radius=int(320 * 0.22), fill=255)
logo.putalpha(mask)
img.paste(logo, (100, (H - 320) // 2), logo)

x = 490
d.text((x, 108), "KRAKEN", font=font("Bold", 120), fill=TEXT)
d.text((x, 256), "Messagerie libre, gratuite et auto-hébergeable", font=font("SemiBold", 44), fill=ACCENT)
d.text((x, 324), "Salons, messages, vocal, partage d'écran, contrôle à distance.", font=font("Regular", 32), fill=MUTED)
d.text((x, 368), "Écrite en Rust, faite avec l'IA, sans abonnement.", font=font("Regular", 32), fill=MUTED)

out = ROOT / "docs/images/banniere.png"
img.quantize(colors=256, method=Image.Quantize.FASTOCTREE).save(out, optimize=True)
print(out.relative_to(ROOT))
