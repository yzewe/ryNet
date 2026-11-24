#!/usr/bin/env python3
import sys
from pathlib import Path

from PIL import Image, ImageDraw

LOGO = [
    330, 540,
    400, 560, 452, 560, 446, 620,
    440, 676, 360, 700, 330, 760,
    300, 820, 330, 856, 420, 862,
    520, 870, 640, 864, 712, 822,
    782, 780, 772, 700, 700, 668,
    640, 640, 610, 600, 622, 548,
    634, 496, 730, 470, 724, 380,
    718, 280, 620, 214, 512, 214,
    400, 214, 330, 260, 312, 330,
    302, 372, 298, 400, 300, 430,
]
BG = (246, 216, 205)
ORANGE = (244, 142, 88)
PINK = (232, 92, 128)
OFF_BG = (222, 222, 226)
OFF_LINE = (104, 104, 112)

def points():
    pts = [(LOGO[0], LOGO[1])]
    for o in range(2, len(LOGO), 6):
        p0 = pts[-1]
        p1, p2, p3 = (LOGO[o], LOGO[o + 1]), (LOGO[o + 2], LOGO[o + 3]), (LOGO[o + 4], LOGO[o + 5])
        for j in range(1, 49):
            t = j / 48
            u = 1 - t
            pts.append(tuple(u ** 3 * p0[k] + 3 * u * u * t * p1[k] + 3 * u * t * t * p2[k] + t ** 3 * p3[k] for k in range(2)))
    return pts

def stroke(draw, line, width):
    r = width / 2
    step = max(1.0, r / 4)
    prev = None
    for x, y in line:
        if prev is not None:
            dx, dy = x - prev[0], y - prev[1]
            dist = (dx * dx + dy * dy) ** 0.5
            n = int(dist / step)
            for i in range(1, n + 1):
                px, py = prev[0] + dx * i / (n + 1), prev[1] + dy * i / (n + 1)
                draw.ellipse((px - r, py - r, px + r, py + r), fill=255)
        draw.ellipse((x - r, y - r, x + r, y + r), fill=255)
        prev = (x, y)

def render(size, off=False):
    ss = 8
    n = size * ss
    small = size <= 40
    pad = 0 if small else round(n * 0.04)
    radius = round((n - 2 * pad) * (0.3 if small else 0.26))
    bg = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ImageDraw.Draw(bg).rounded_rectangle((pad, pad, n - pad - 1, n - pad - 1), radius, fill=(*(OFF_BG if off else BG), 255))
    pts = points()
    ys = [p[1] for p in pts]
    xs = [p[0] for p in pts]
    h = max(ys) - min(ys)
    frac = 0.62 if small else 0.56
    k = (n - 2 * pad) * frac / h
    cx, cy = (max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2
    boost = {16: 2.1, 20: 1.9, 24: 1.75, 32: 1.5, 40: 1.35}.get(size, 1.0)
    width = max(1, round(68 * k * boost))
    line = [(n / 2 + (x - cx) * k, n / 2 + (y - cy) * k) for x, y in pts]
    mask = Image.new("L", (n, n), 0)
    stroke(ImageDraw.Draw(mask), line, width)
    if off:
        grad = Image.new("RGBA", (n, n), (*OFF_LINE, 255))
    else:
        grad = Image.new("RGBA", (n, n))
        gd = ImageDraw.Draw(grad)
        for yy in range(n):
            t = yy / (n - 1)
            gd.line([(0, yy), (n, yy)], fill=tuple(round(ORANGE[i] + (PINK[i] - ORANGE[i]) * t) for i in range(3)) + (255,))
    bg.paste(grad, (0, 0), mask)
    return bg.resize((size, size), Image.LANCZOS)

def main():
    res = Path(sys.argv[1])
    ico = Path(sys.argv[2])
    (res / "icons").mkdir(parents=True, exist_ok=True)
    for s in (16, 20, 24, 32, 40, 48, 64, 128, 256):
        render(s).save(res / "icons" / f"app-{s}.png")
    for s in (16, 20, 24, 32, 40, 48, 64):
        render(s, off=True).save(res / "icons" / f"tray-off-{s}.png")
    big = render(256)
    big.save(ico, sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)],
             append_images=[render(s) for s in (16, 20, 24, 32, 40, 48, 64, 128)])

if __name__ == "__main__":
    main()
