#!/usr/bin/env python3
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import icons

here = Path(__file__).resolve().parent
font_dir = here / "fonts"
TOP = (244, 142, 88)
BOTTOM = (226, 84, 128)

def panel(scale):
    w, h = round(164 * scale), round(314 * scale)
    im = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(im)
    for y in range(h):
        t = y / (h - 1)
        d.line([(0, y), (w, y)], fill=tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3)))
    pts = icons.points()
    xs = [p[0] for p in pts]; ys = [p[1] for p in pts]
    lh = max(ys) - min(ys)
    k = h * 0.26 / lh
    cx, cy = (max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2
    ss = 4
    big = Image.new("L", (w * ss, h * ss), 0)
    bd = ImageDraw.Draw(big)
    line = [((w / 2 + (x - cx) * k) * ss, (h * 0.40 + (y - cy) * k) * ss) for x, y in pts]
    width = round(68 * k * ss * 1.1)
    icons.stroke(bd, line, width)
    mask = big.resize((w, h), Image.LANCZOS)
    im.paste((255, 255, 255), (0, 0), mask)
    f = ImageFont.truetype(str(font_dir / "GoogleSansFlex-600.ttf"), round(26 * scale))
    d.text((w / 2, h * 0.66), "ryNet", font=f, fill=(255, 255, 255), anchor="mm")
    f2 = ImageFont.truetype(str(font_dir / "GoogleSansFlex-500.ttf"), round(11 * scale))
    d.text((w / 2, h * 0.66 + 26 * scale), "VPN", font=f2, fill=(255, 236, 228), anchor="mm")
    return im

def msi():
    dialog = Image.new("RGB", (493, 312), (255, 255, 255))
    dialog.paste(panel(1).crop((0, 1, 164, 313)), (0, 0))
    dialog.convert("P", palette=Image.ADAPTIVE).save(here / "msi-dialog.bmp")
    banner = Image.new("RGB", (493, 58), (255, 255, 255))
    logo = icons.render(64).resize((46, 46), Image.LANCZOS)
    banner.paste(logo, (493 - 46 - 8, 6), logo)
    banner.convert("P", palette=Image.ADAPTIVE).save(here / "msi-banner.bmp")

def main():
    msi()
    for pct in (100, 125, 150, 175, 200, 250):
        s = pct / 100
        panel(s).save(here / f"wizard-{pct}.bmp")
        n = round(55 * s)
        small = icons.render(max(64, n)).resize((n, n), Image.LANCZOS)
        bg = Image.new("RGB", (n, n), (255, 255, 255))
        bg.paste(small, (0, 0), small)
        bg.save(here / f"small-{pct}.bmp")

if __name__ == "__main__":
    main()
