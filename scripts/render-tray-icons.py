#!/usr/bin/env python3
"""Render tray icons: white A1 mark plus a status dot. Does not recolor the mark."""

from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
MARK = ROOT / "src-tauri/icons/src/stand-easy-ops-mark-white.svg"
OUT = ROOT / "src-tauri/icons"
SIZE = 32
# Lamp colours match the header status lamp.
DOTS = {
    "green": (46, 204, 113, 255),
    "amber": (241, 196, 15, 255),
    "red": (231, 76, 60, 255),
}


def render_mark(width: int, height: int) -> Image.Image:
    with tempfile.NamedTemporaryFile(suffix=".png") as tmp:
        subprocess.run(
            ["rsvg-convert", "-w", str(width), "-h", str(height), str(MARK), "-o", tmp.name],
            check=True,
        )
        return Image.open(tmp.name).convert("RGBA")


def compose(dot_rgba: tuple[int, int, int, int]) -> Image.Image:
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    mark = render_mark(20, 17)
    canvas.alpha_composite(mark, (2, 5))
    draw = ImageDraw.Draw(canvas)
    # 8px status dot, bottom-right, with a 1px dark ring so it reads on light bars.
    x0, y0, x1, y1 = 21, 21, 30, 30
    draw.ellipse((x0 - 1, y0 - 1, x1 + 1, y1 + 1), fill=(18, 23, 31, 220))
    draw.ellipse((x0, y0, x1, y1), fill=dot_rgba)
    return canvas


def main() -> None:
    for name, color in DOTS.items():
        dest = OUT / f"tray-{name}.png"
        compose(color).save(dest, "PNG")
        print(f"wrote {dest}")


if __name__ == "__main__":
    main()
