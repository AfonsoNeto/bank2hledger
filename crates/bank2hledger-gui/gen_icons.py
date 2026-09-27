#!/usr/bin/env python3
"""Generate the GUI's icon set (PNG, ICO, MSIX Assets) without external deps.

Design: Win11 accent-blue rounded square with three white 'ledger' bars and a
small deposit arrow wedge. Drawn parametrically at each size.
"""

import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
ICON_DIR = ROOT / "src-tauri" / "icons"
MSIX_ASSETS = ROOT / "msix" / "Assets"

BG = (0, 103, 192, 255)       # Windows 11 accent blue #0067C0
FG = (255, 255, 255, 255)


def rounded_rect_dist(x: float, y: float, size: float, radius: float) -> bool:
    """Point inside a rounded rect covering [0,size]^2 with corner radius."""
    if x < 0 or y < 0 or x > size or y > size:
        return False
    cx = min(max(x, radius), size - radius)
    cy = min(max(y, radius), size - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius**2


def draw(size: int) -> list[list[tuple[int, int, int, int]]]:
    px = [[(0, 0, 0, 0)] * size for _ in range(size)]
    radius = size * 0.22
    for yy in range(size):
        for xx in range(size):
            if rounded_rect_dist(xx + 0.5, yy + 0.5, size, radius):
                px[yy][xx] = BG

    def bar(x0: float, y0: float, w: float, h: float, color=FG):
        for yy in range(size):
            for xx in range(size):
                fx, fy = xx + 0.5, yy + 0.5
                if x0 <= fx <= x0 + w and y0 <= fy <= y0 + h:
                    px[yy][xx] = color

    # three ledger bars (a journal with lines), slightly uneven widths
    s = size / 256.0
    bar(52 * s, 62 * s, 152 * s, 22 * s)
    bar(52 * s, 117 * s, 108 * s, 22 * s)
    bar(52 * s, 172 * s, 132 * s, 22 * s)
    # deposit arrow: triangle pointing down-right into the last bar
    for yy in range(size):
        for xx in range(size):
            fx, fy = xx + 0.5, yy + 0.5
            u, v = (fx - 168 * s) / 40 * s, (fy - 108 * s) / 40 * s
            if 0 <= u <= 1 and 0 <= v <= 1 and u + v <= 1.05:
                px[yy][xx] = FG
    return px


def png_bytes(px: list[list[tuple[int, int, int, int]]]) -> bytes:
    height, width = len(px), len(px[0])
    raw = b"".join(
        b"\x00" + b"".join(struct.pack("4B", *p) for p in row) for row in px
    )

    def chunk(tag: bytes, data: bytes) -> bytes:
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico_bytes(images: list[tuple[int, bytes]]) -> bytes:
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for size, data in images:
        out += struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,
            0 if size >= 256 else size,
            0, 0, 1, 32, len(data), offset,
        )
        offset += len(data)
    return out + b"".join(data for _, data in images)


def main() -> None:
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    MSIX_ASSETS.mkdir(parents=True, exist_ok=True)
    rendered = {size: png_bytes(draw(size)) for size in (32, 44, 50, 128, 150, 256)}
    (ICON_DIR / "32x32.png").write_bytes(rendered[32])
    (ICON_DIR / "128x128.png").write_bytes(rendered[128])
    (ICON_DIR / "icon.png").write_bytes(rendered[256])
    (ICON_DIR / "icon.ico").write_bytes(
        ico_bytes([(32, rendered[32]), (256, rendered[256])])
    )
    (MSIX_ASSETS / "Square44x44Logo.png").write_bytes(rendered[44])
    (MSIX_ASSETS / "Square150x150Logo.png").write_bytes(rendered[150])
    (MSIX_ASSETS / "StoreLogo.png").write_bytes(rendered[50])
    print(f"icons written to {ICON_DIR} and {MSIX_ASSETS}")


if __name__ == "__main__":
    main()
