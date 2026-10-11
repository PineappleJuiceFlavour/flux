"""Import the owner's Flux logo and app icon (exact artwork) into launcher assets.

    python tools/branding/import_brand.py <logo.png> <icon.png>

The logo is drawn on a near-black background: its alpha is recovered from the distance to that
background color (then colors are un-premultiplied), so it sits on any backdrop. The icon tile is
cut out of its screenshot with an anti-aliased rounded-rectangle mask.
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets" / "images" / "branding"


def key_logo(path: Path) -> Image.Image:
    im = np.array(Image.open(path).convert("RGB")).astype(np.float32)
    h, w, _ = im.shape
    border = np.concatenate([im[:6].reshape(-1, 3), im[-6:].reshape(-1, 3), im[:, :6].reshape(-1, 3), im[:, -6:].reshape(-1, 3)])
    bg = np.median(border, axis=0)
    # Alpha: how far a pixel is from the background, relative to the brightest artwork (~ full opacity).
    dist = np.abs(im - bg).max(axis=2)
    alpha = np.clip((dist - 6) / 60, 0, 1)
    rgb = np.where(alpha[..., None] > 0.01, bg + (im - bg) / np.maximum(alpha[..., None], 0.01), 0)
    out = np.dstack([np.clip(rgb, 0, 255), alpha * 255]).astype(np.uint8)
    img = Image.fromarray(out, "RGBA")
    return img.crop(img.getbbox())


def cut_icon(path: Path) -> Image.Image:
    im = Image.open(path).convert("RGB")
    a = np.array(im).astype(np.int32)
    bg = a[0, 0]
    diff = np.abs(a - bg).sum(axis=2)
    rows = np.where((diff > 14).sum(axis=1) > a.shape[1] * 0.25)[0]
    cols = np.where((diff > 14).sum(axis=0) > a.shape[0] * 0.25)[0]
    x0, x1, y0, y1 = cols.min(), cols.max() + 1, rows.min(), rows.max() + 1
    tile = im.crop((x0, y0, x1, y1)).convert("RGBA")
    ss = 8
    w, h = tile.size
    mask = Image.new("L", (w * ss, h * ss), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, w * ss - 1, h * ss - 1], radius=int(min(w, h) * 0.2 * ss), fill=255)
    tile.putalpha(mask.resize((w, h), Image.LANCZOS))
    side = max(w, h)
    square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    square.alpha_composite(tile, ((side - w) // 2, (side - h) // 2))
    return square


def main():
    logo, icon = Path(sys.argv[1]), Path(sys.argv[2])
    OUT.mkdir(parents=True, exist_ok=True)
    key_logo(logo).save(OUT / "logo.png")
    ic = cut_icon(icon)
    ic.save(OUT / "icon.png")
    ic.resize((256, 256), Image.LANCZOS).save(
        ROOT / "package" / "windows.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    )
    print("logo", Image.open(OUT / "logo.png").size, "icon", ic.size)


if __name__ == "__main__":
    main()
