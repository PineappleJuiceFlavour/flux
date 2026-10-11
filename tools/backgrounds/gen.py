"""Flux launcher backgrounds, drawn from code (original art, no game assets).

Every scene is drawn twice from the same description:
  * pixel  - on a 384x216 canvas with a posterized, Bayer-dithered sky, then scaled 5x nearest-neighbour
  * smooth - on a 4x supersampled 1920x1080 canvas with smooth gradients and glows, then downsampled

Moving parts (clouds, skater, car, ...) are separate sprites the launcher animates on top.

    python tools/backgrounds/gen.py            # writes assets/images/backgrounds/<id>/{pixel,smooth}/...
"""
from __future__ import annotations

import math
import random
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

OUT = Path(__file__).resolve().parents[2] / "assets" / "images" / "backgrounds"
W, H = 384, 216          # logical canvas (pixel-art resolution)
FINAL = (1920, 1080)

BAYER4 = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]) / 16.0 - 0.5


def hexc(h: str, a: int = 255):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), a)


def lerp(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(len(a)))


class Canvas:
    """Draw in logical 384x216 coordinates; `s` is the pixel scale of the backing image."""

    def __init__(self, pixel: bool):
        self.pixel = pixel
        self.s = 1 if pixel else 10  # smooth: 3840x2160, downsampled 2x to 1920x1080
        self.img = Image.new("RGBA", (W * self.s, H * self.s), (0, 0, 0, 255))
        self.d = ImageDraw.Draw(self.img)

    def _p(self, pts):
        return [(x * self.s, y * self.s) for x, y in pts]

    def rect(self, x0, y0, x1, y1, color):
        s = self.s
        self.d.rectangle([x0 * s, y0 * s, x1 * s - 1, y1 * s - 1], fill=color)

    def poly(self, pts, color):
        self.d.polygon(self._p(pts), fill=color)

    def ellipse(self, x0, y0, x1, y1, color):
        s = self.s
        self.d.ellipse([x0 * s, y0 * s, x1 * s - 1, y1 * s - 1], fill=color)

    def line(self, pts, color, width=1):
        self.d.line(self._p(pts), fill=color, width=max(1, int(width * self.s)))

    def px(self, x, y, color):
        self.rect(x, y, x + 1, y + 1, color)

    def vgradient(self, y0, y1, stops, steps=10):
        """Vertical gradient through color stops [(t, color)], dithered into bands in pixel mode."""
        s = self.s
        h = (y1 - y0) * s
        t = np.linspace(0, 1, h)[:, None]
        cols = np.zeros((h, 4), dtype=np.float32)
        for (ta, ca), (tb, cb) in zip(stops, stops[1:]):
            m = (t[:, 0] >= ta) & (t[:, 0] <= tb)
            k = ((t[m, 0] - ta) / max(1e-6, tb - ta))[:, None]
            cols[m] = np.array(ca) * (1 - k) + np.array(cb) * k
        band = np.repeat(cols[:, None, :], W * s, axis=1)
        if self.pixel:
            # Posterize along t with Bayer dithering between bands: the classic pixel-art sky.
            tt = np.repeat(t, W * s, axis=1)
            yy, xx = np.mgrid[0:h, 0:W * s]
            q = np.floor(tt * steps + 0.5 + BAYER4[yy % 4, xx % 4]) / steps
            q = np.clip(q, 0, 1)
            band = np.zeros_like(band)
            for (ta, ca), (tb, cb) in zip(stops, stops[1:]):
                m = (q >= ta) & (q <= tb)
                k = ((q - ta) / max(1e-6, tb - ta))[..., None]
                band = np.where(m[..., None], np.array(ca) * (1 - k) + np.array(cb) * k, band)
        region = Image.fromarray(band.astype(np.uint8), "RGBA")
        self.img.paste(region, (0, y0 * s))

    def glow(self, cx, cy, r, color, strength=0.6):
        """Soft radial glow; in pixel mode its alpha is posterized with Bayer dithering."""
        s = self.s
        size = self.img.size
        yy, xx = np.mgrid[0:size[1], 0:size[0]].astype(np.float32)
        dist = np.sqrt((xx / s - cx) ** 2 + (yy / s - cy) ** 2) / r
        a = np.clip(1 - dist, 0, 1) ** 1.6 * strength
        if self.pixel:
            levels = 5
            a = np.floor(a * levels + 0.5 + BAYER4[yy.astype(int) % 4, xx.astype(int) % 4]) / levels
            a = np.clip(a, 0, strength)
        layer = np.zeros((size[1], size[0], 4), dtype=np.uint8)
        layer[..., 0], layer[..., 1], layer[..., 2] = color[0], color[1], color[2]
        layer[..., 3] = (a * 255).astype(np.uint8)
        self.img.alpha_composite(Image.fromarray(layer, "RGBA"))

    def finish(self) -> Image.Image:
        if self.pixel:
            return self.img.resize(FINAL, Image.NEAREST)
        return self.img.resize(FINAL, Image.LANCZOS)


def ridge(rng, x0, x1, base, amp, block, smooth_scale):
    """Heights for a hill line: blocky steps (Minecraft-like) when block > 1."""
    phase = [rng.uniform(0, 6.28) for _ in range(3)]
    out = []
    for x in range(x0, x1 + 1):
        xx = (x // block) * block if block > 1 else x
        v = (math.sin(xx / smooth_scale + phase[0]) * 0.6 + math.sin(xx / (smooth_scale * 0.43) + phase[1]) * 0.3
             + math.sin(xx / (smooth_scale * 0.17) + phase[2]) * 0.1)
        y = base - amp * v
        if block > 1:
            y = round(y / block) * block
        out.append((x, y))
    return out


def stars(c: Canvas, rng, count, y_max, colors=("#ffffff", "#cfe3ff", "#ffe6c7")):
    for _ in range(count):
        x, y = rng.randrange(W), rng.randrange(int(y_max))
        col = hexc(rng.choice(colors), rng.randrange(120, 255))
        c.px(x, y, col)
        if not c.pixel and rng.random() < 0.15:
            c.glow(x + 0.5, y + 0.5, 1.6, col, 0.35)


# --------------------------------------------------------------------------------------------- scenes

def skatepark(c: Canvas):
    rng = random.Random(3)
    G = 168  # ground line
    c.vgradient(0, G, [(0, hexc("#140c2e")), (0.4, hexc("#3f1a58")), (0.75, hexc("#b8406c")), (1, hexc("#ff9a4a"))], steps=12)
    stars(c, rng, 80, 70)
    # Setting sun above the skyline, with synthwave slats.
    c.glow(292, 112, 70, hexc("#ff7a3d"), 0.55)
    c.ellipse(258, 78, 326, 146, hexc("#ffcf6b"))
    for i, y in enumerate(range(108, 146, 5)):
        c.rect(256, y, 328, y + 1 + i // 2, hexc("#c8466c"))
    # Two layers of skyline with lit windows, kept low so the sky stays open.
    for layer, (col, top, wmin, wmax, lit) in enumerate(((hexc("#35204f"), 122, 10, 22, 0.08), (hexc("#1b0f2e"), 132, 14, 30, 0.14))):
        x = -5
        while x < W:
            w = rng.randint(wmin, wmax)
            h = rng.randint(10, 34) + layer * 4
            y0 = top - h + rng.randint(0, 12)
            c.rect(x, y0, x + w, G, col)
            if rng.random() < 0.3:
                c.rect(x + w // 2 - 1, y0 - 6, x + w // 2 + 1, y0, col)
            for wy in range(y0 + 3, G - 4, 4):
                for wx in range(x + 2, x + w - 2, 3):
                    if rng.random() < lit:
                        c.rect(wx, wy, wx + 1, wy + 2, hexc(rng.choice(["#ffd27a", "#ffb36b", "#9fe7ff"])))
            x += w + rng.randint(0, 3)
    # Concrete plaza.
    c.vgradient(G, H, [(0, hexc("#625e76")), (1, hexc("#2a2636"))], steps=6)
    c.rect(0, G, W, G + 2, hexc("#9a96b2"))
    for x in range(0, W + 80, 56):
        c.line([(x, G + 2), (x - 60, H)], hexc("#3a3648"), 1)
    # Quarter pipe on the left, with abstract paint.
    R = 52
    pts = [(x, G - R + R * (1 - math.sqrt(max(0, 1 - (x / R) ** 2)))) for x in range(0, R + 1)]
    pts = [(x, (G - R) + (R - math.sqrt(max(0, R * R - (R - x) ** 2)))) for x in range(0, R + 1)]
    c.poly([(0, G - R)] + pts + [(R, G), (0, G)], hexc("#7c7894"))
    c.rect(0, G - R - 3, 12, G - R, hexc("#c9c5df"))
    c.rect(0, G - R, 4, G, hexc("#5a5670"))
    for gx, gy, gw, gh, col in ((8, G - 34, 16, 9, "#ff4fa3"), (20, G - 24, 12, 7, "#3de1ff"), (6, G - 18, 10, 6, "#ffe14d")):
        c.rect(gx, gy, gx + gw, gy + gh, hexc(col, 210))
    # Funbox with a long grind rail.
    c.poly([(140, G), (154, G - 16), (234, G - 16), (248, G)], hexc("#77738e"))
    c.rect(154, G - 19, 234, G - 15, hexc("#aaa6c4"))
    c.rect(104, G - 30, 282, G - 27, hexc("#e2e0f0"))
    for px_ in (112, 192, 272):
        c.rect(px_, G - 27, px_ + 3, G, hexc("#4c4862"))
    # Stair set with a handrail on the right.
    for i in range(6):
        c.rect(300 + i * 12, G - (6 - i) * 6, W, G - (5 - i) * 6, hexc("#6f6b86" if i % 2 else "#7d7995"))
    c.line([(294, G - 50), (374, G - 14)], hexc("#e2e0f0"), 1.4)
    for x0 in (298, 336, 372):
        c.rect(x0, G - 50 + (x0 - 294) * 36 // 80, x0 + 2, G, hexc("#4c4862"))
    # Street lamps.
    for lx in (84, 360):
        c.rect(lx, G - 64, lx + 2, G, hexc("#2b2740"))
        c.rect(lx - 5, G - 66, lx + 7, G - 63, hexc("#2b2740"))
        c.glow(lx + 1, G - 60, 22, hexc("#ffd88a"), 0.45)
        c.rect(lx - 3, G - 63, lx + 5, G - 61, hexc("#fff1c4"))


def blocky_valley(c: Canvas):
    rng = random.Random(7)
    c.vgradient(0, 150, [(0, hexc("#5f97f5")), (0.7, hexc("#a6c8ff")), (1, hexc("#d9e8ff"))], steps=9)
    c.rect(300, 26, 324, 50, hexc("#fff6b0"))  # square sun
    c.glow(312, 38, 26, hexc("#fff3a0"), 0.35)
    # Far mountains, then two hill layers, each in 4px blocks.
    for base, amp, col_top, col_body, block, sc in (
        (104, 22, hexc("#9fb7d8"), hexc("#8aa3c7"), 4, 40),
        (124, 16, hexc("#5fa84a"), hexc("#7a5636"), 4, 28),
        (150, 12, hexc("#6cc04a"), hexc("#86603c"), 4, 20),
    ):
        line = ridge(rng, 0, W, base, amp, block, sc)
        for x, y in line[:-1]:
            c.rect(x, int(y), x + 1, H, col_body)
            c.rect(x, int(y), x + 1, int(y) + block, col_top)
        if col_body != hexc("#8aa3c7"):
            for _ in range(500):  # dirt speckle
                x = rng.randrange(W)
                y0 = int(line[x][1]) + block + rng.randrange(0, 60)
                if y0 < H:
                    c.px(x, y0, hexc("#6b4a2e") if rng.random() < 0.6 else hexc("#9a7148"))
        if col_top == hexc("#5fa84a"):
            for tx in range(10, W, 46):  # trees on the middle hills
                ty = int(line[tx][1])
                c.rect(tx, ty - 14, tx + 3, ty, hexc("#6b4a2a"))
                c.rect(tx - 6, ty - 26, tx + 9, ty - 13, hexc("#3f8f34"))
                c.rect(tx - 3, ty - 30, tx + 6, ty - 25, hexc("#3f8f34"))
                for _ in range(10):
                    c.px(tx - 6 + rng.randrange(15), ty - 26 + rng.randrange(13), hexc("#2f7429"))
    # A river across the front valley.
    c.rect(0, 176, W, 188, hexc("#3f76e4"))
    for _ in range(90):
        x, y = rng.randrange(W - 4), rng.randrange(177, 187)
        c.rect(x, y, x + rng.randint(2, 5), y + 1, hexc("#7fa8ff"))
    c.rect(0, 188, W, H, hexc("#6cc04a"))
    c.rect(0, 192, W, H, hexc("#86603c"))
    for x in range(0, W, 8):  # grass block edge
        c.rect(x, 188, x + 8, 192, hexc("#5fb043") if (x // 8) % 2 else hexc("#6cc04a"))
        if rng.random() < 0.25:
            c.rect(x + 3, 184, x + 4, 188, hexc("#3f8f34"))
            c.px(x + 3, 183, hexc(rng.choice(["#ff5c5c", "#ffe14d", "#ffffff"])))


def cartoon_town(c: Canvas):
    rng = random.Random(11)
    c.vgradient(0, 150, [(0, hexc("#3fb2ff")), (0.75, hexc("#8fd8ff")), (1, hexc("#d6f3ff"))], steps=9)
    # Distant hills and two cooling towers puffing steam.
    for base, amp, col in ((118, 10, hexc("#88c27a")), (128, 8, hexc("#6fb062"))):
        line = ridge(rng, 0, W, base, amp, 1, 30)
        c.poly(line + [(W, H), (0, H)], col)
    for tx in (36, 72):
        c.poly([(tx, 72), (tx + 22, 72), (tx + 18, 96), (tx + 24, 124), (tx - 2, 124), (tx + 4, 96)], hexc("#7d8f86"))
        c.rect(tx + 1, 72, tx + 21, 75, hexc("#5f6f68"))
        for i in range(5):
            r = 6 + i * 2
            c.ellipse(tx + 11 - r + i * 3, 60 - i * 9 - r, tx + 11 + r + i * 3, 60 - i * 9 + r, hexc("#f4f7f5", 230 - i * 30))
    # A row of pastel houses with picket fences.
    palette = ["#f7a8c0", "#b79cff", "#8fe0a8", "#ffd36b", "#8cc8ff", "#ffb07a"]
    x = 100
    while x < W:
        w = rng.randint(38, 52)
        h = rng.randint(24, 30)
        y1 = 150
        col = hexc(rng.choice(palette))
        roof = hexc(rng.choice(["#7a4b3a", "#5a5f7a", "#9a3d3d", "#4b6b4b"]))
        c.rect(x, y1 - h, x + w, y1, col)
        c.poly([(x - 4, y1 - h), (x + w // 2, y1 - h - 16), (x + w + 4, y1 - h)], roof)
        c.rect(x + w // 2 - 4, y1 - 14, x + w // 2 + 4, y1, hexc("#6a3f2a"))
        for wx in (x + 5, x + w - 13):
            c.rect(wx, y1 - h + 6, wx + 8, y1 - h + 13, hexc("#fff7c2"))
            c.rect(wx + 3, y1 - h + 6, wx + 4, y1 - h + 13, hexc("#ffffff"))
        if rng.random() < 0.6:  # round tree beside the house
            c.rect(x + w + 8, y1 - 16, x + w + 10, y1, hexc("#7a5636"))
            c.ellipse(x + w + 1, y1 - 34, x + w + 18, y1 - 14, hexc("#4fae4a"))
        x += w + rng.randint(18, 26)
    c.rect(0, 150, W, 158, hexc("#7ccf6a"))  # lawns
    for fx in range(0, W, 5):  # picket fence
        c.rect(fx, 146, fx + 3, 156, hexc("#ffffff"))
        c.px(fx + 1, 145, hexc("#ffffff"))
    c.rect(0, 149, W, 150, hexc("#e8e8e8"))
    # Sidewalk and road.
    c.rect(0, 158, W, 166, hexc("#d9d4c7"))
    c.rect(0, 166, W, H, hexc("#4a4a52"))
    for rx in range(0, W, 28):
        c.rect(rx, 190, rx + 14, 192, hexc("#ffd400"))
    c.rect(0, 166, W, 167, hexc("#2f2f36"))
    c.rect(0, 210, W, H, hexc("#d9d4c7"))


def neon_city(c: Canvas):
    rng = random.Random(5)
    G = 132
    c.vgradient(0, G, [(0, hexc("#05030f")), (0.55, hexc("#1c0b3a")), (1, hexc("#6b1f7e"))], steps=10)
    stars(c, rng, 110, 80)
    c.glow(250, 92, 80, hexc("#ff3fa4"), 0.5)
    c.ellipse(206, 48, 294, 136, hexc("#ff6ac1"))
    c.ellipse(206, 48, 294, 92, hexc("#ffb36b"))
    for i, y in enumerate(range(84, 136, 5)):
        c.rect(204, y, 296, y + 1 + i // 3, hexc("#3a1257"))
    x = 0
    while x < W:
        w = rng.randint(10, 24)
        h = rng.randint(8, 30)
        c.rect(x, G - h, x + w, G, hexc("#0d0820"))
        c.rect(x, G - h, x + w, G - h + 1, hexc("#ff3fa4" if rng.random() < 0.5 else "#2de2ff", 200))
        for wy in range(G - h + 3, G - 2, 3):
            for wx in range(x + 2, x + w - 1, 3):
                if rng.random() < 0.1:
                    c.px(wx, wy, hexc("#2de2ff", 200))
        x += w + rng.randint(1, 4)
    c.rect(0, G, W, H, hexc("#0a0418"))
    c.glow(250, G, 120, hexc("#ff3fa4"), 0.25)
    for i in range(1, 16):
        y = G + (i ** 1.75) * 0.75
        if y < H:
            c.rect(0, int(y), W, int(y) + 1, hexc("#2de2ff", 150))
    for gx in range(-2 * W, 3 * W, 26):
        c.line([(250, G), (gx, H)], hexc("#ff3fa4", 130), 0.6)


def galaxy(c: Canvas):
    rng = random.Random(9)
    c.vgradient(0, H, [(0, hexc("#04040c")), (0.5, hexc("#0e0a24")), (1, hexc("#1a0f33"))], steps=8)
    for _ in range(6):
        cx, cy = rng.randrange(W), rng.randrange(H)
        c.glow(cx, cy, rng.randint(30, 70), hexc(rng.choice(["#6a2cff", "#ff3f8e", "#2dd4ff"])), 0.25)
    stars(c, rng, 260, H)
    # A blocky planet in the corner.
    cx, cy, r = 300, 180, 70
    for y in range(cy - r, H):
        for x in range(cx - r, min(W, cx + r)):
            if (x - cx) ** 2 + (y - cy) ** 2 <= r * r and (x % 4 == 0 and y % 4 == 0):
                n = math.sin(x * 0.11) + math.cos(y * 0.13) + math.sin((x + y) * 0.05)
                col = "#3f76e4" if n < 0.2 else ("#5fb043" if n < 1.2 else "#d8c48a")
                c.rect(x, y, x + 4, y + 4, hexc(col))
    c.glow(cx, cy, r + 8, hexc("#5fa8ff"), 0.18)


def nether(c: Canvas):
    rng = random.Random(13)
    c.vgradient(0, H, [(0, hexc("#1a0505")), (0.6, hexc("#4a0d0d")), (1, hexc("#ff5a1f"))], steps=10)
    for base, amp, col in ((60, 16, hexc("#2a0808")), (40, 20, hexc("#3a0c0c"))):
        line = ridge(rng, 0, W, base, amp, 4, 22)
        c.poly([(0, 0), (W, 0)] + [(x, y) for x, y in reversed(line)], col)  # ceiling
    for base, amp, col in ((160, 14, hexc("#4d1111")), (176, 10, hexc("#2e0909"))):
        line = ridge(rng, 0, W, base, amp, 4, 26)
        c.poly(line + [(W, H), (0, H)], col)
    c.rect(0, 196, W, H, hexc("#ff7a1f"))
    for _ in range(80):
        x, y = rng.randrange(W - 4), rng.randrange(196, H)
        c.rect(x, y, x + rng.randint(2, 6), y + 1, hexc(rng.choice(["#ffd23f", "#ff4a1a"])))
    c.glow(192, 205, 120, hexc("#ff6a1f"), 0.3)


SCENES = {
    "skatepark": ("Skatepark", skatepark),
    "blocky-valley": ("Blocky Valley", blocky_valley),
    "cartoon-town": ("Cartoon Town", cartoon_town),
    "neon-city": ("Neon City", neon_city),
    "galaxy": ("Galaxy", galaxy),
    "nether": ("Nether", nether),
}

# ------------------------------------------------------------------------------------------- sprites


def sprite(pixel_rows: list[str], palette: dict[str, str], scale: int) -> Image.Image:
    h, w = len(pixel_rows), max(len(r) for r in pixel_rows)
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    for y, row in enumerate(pixel_rows):
        for x, ch in enumerate(row):
            if ch in palette:
                img.putpixel((x, y), hexc(palette[ch]))
    return img.resize((w * scale, h * scale), Image.NEAREST)


SKATER = [
    "......hh......",
    ".....hhhh.....",
    ".....ffff.....",
    ".....ffff.....",
    "......ss......",
    "....sssssss...",
    "...s.sssss.s..",
    "..f..sssss..f.",
    ".....sssss....",
    ".....pp.pp....",
    ".....pp.pp....",
    "....pp...pp...",
    "....pp...pp...",
    "...kk.....kk..",
    ".bbbbbbbbbbbb.",
    "..w........w..",
]
SKATER_PAL = {"h": "#2b2b38", "f": "#e8b48c", "s": "#ff5a1f", "p": "#3d4a7a", "k": "#f2f2f2", "b": "#c99a5b", "w": "#e8e8e8"}

CAR = [
    ".......cccccccccc.........",
    "......cggggcggggcc........",
    ".....cggggggcgggggcc......",
    "cccccccccccccccccccccccccc",
    "cccccccccccccccccccccccccy",
    "cccccccccccccccccccccccccc",
    "cddcccccccccccccccccccddcc",
    "..tttt..............tttt..",
    "..twwt..............twwt..",
    "...tt................tt...",
]
CAR_PAL = {"c": "#f278a8", "g": "#bfe8ff", "y": "#fff3a0", "d": "#c9557f", "t": "#202028", "w": "#9a9aa8"}

BLOCK_CLOUD = [
    "....wwwwwwww........",
    "..wwwwwwwwwwwwww....",
    "wwwwwwwwwwwwwwwwwwww",
    "wwwwwwwwwwwwwwwwwwww",
    "..gggggggggggggggg..",
]
PUFF_CLOUD_R = [(10, 10, 9), (22, 7, 11), (34, 10, 9), (22, 13, 9)]


def puff_cloud(scale: int, smooth: bool) -> Image.Image:
    s = scale * (4 if smooth else 1)
    img = Image.new("RGBA", (46 * s, 24 * s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    for cx, cy, r in PUFF_CLOUD_R:
        d.ellipse([(cx - r) * s, (cy - r) * s, (cx + r) * s, (cy + r) * s], fill=(255, 255, 255, 235))
    d.rectangle([2 * s, 13 * s, 44 * s, 19 * s], fill=(255, 255, 255, 235))
    if smooth:
        return img.resize((46 * scale, 24 * scale), Image.LANCZOS)
    return img


SPRITES = {
    "skatepark": {"mover": (SKATER, SKATER_PAL)},
    "cartoon-town": {"mover": (CAR, CAR_PAL), "cloud": "puff"},
    "blocky-valley": {"cloud": "block"},
}


def write_sprites(scene: str, style: str, folder: Path):
    spec = SPRITES.get(scene, {})
    scale = 5  # one logical pixel = 5 screen pixels at 1080p, same as the background
    if "mover" in spec:
        rows, pal = spec["mover"]
        sprite(rows, pal, scale).save(folder / "mover.png")
    if spec.get("cloud") == "block":
        sprite(BLOCK_CLOUD, {"w": "#ffffff", "g": "#e3ecff"}, scale * 2).save(folder / "cloud.png")
    elif spec.get("cloud") == "puff":
        puff_cloud(scale, style == "smooth").save(folder / "cloud.png")


def main():
    for scene, (title, fn) in SCENES.items():
        for style in ("pixel", "smooth"):
            c = Canvas(pixel=style == "pixel")
            fn(c)
            folder = OUT / scene / style
            folder.mkdir(parents=True, exist_ok=True)
            img = c.finish().convert("RGB")
            img.save(folder / "background.png", optimize=True)
            img.resize((384, 216), Image.LANCZOS if style == "smooth" else Image.NEAREST).save(folder / "thumb.png")
            write_sprites(scene, style, folder)
            print(f"{scene}/{style}: {title}")


if __name__ == "__main__":
    main()
