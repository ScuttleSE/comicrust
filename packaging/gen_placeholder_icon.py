#!/usr/bin/env python3
"""Generate the PLACEHOLDER app icons (packaging/icons/).

Stdlib only (zlib + struct). Draw per-pixel shapes at 256 px and
downsample to 128 px. The output files are committed; regenerate and
replace when real art exists:

    python3 packaging/gen_placeholder_icon.py
"""

import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(__file__), "icons")
APP_ID = "io.github.ScuttleSE.comicrust"

BG = (0x2E, 0x3B, 0x4E, 255)      # dark slate rounded square
PAGE = (0xF4, 0xF4, 0xF2, 255)    # comic page
LINE = (0xB9, 0xC2, 0xCB, 255)    # page text lines
MARK = (0xC8, 0x3B, 0x2F, 255)    # red bookmark


def rounded(x, y, x0, y0, x1, y1, r):
    if not (x0 <= x < x1 and y0 <= y < y1):
        return False
    cx = min(max(x, x0 + r), x1 - r - 1)
    cy = min(max(y, y0 + r), y1 - r - 1)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def pixel(x, y):
    if rounded(x, y, 8, 8, 248, 248, 44):
        if rounded(x, y, 72, 52, 184, 204, 10):
            if x < 84 and y < 108:
                return MARK  # bookmark ribbon down the page's left edge
            if 100 <= y <= 108 and x < 164:
                return LINE
            if 118 <= y <= 126 and x < 150:
                return LINE
            return PAGE
        return BG
    return (0, 0, 0, 0)


def render(size):
    step = 256 // size
    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            r = g = b = a = n = 0
            for dy in range(step):
                for dx in range(step):
                    pr, pg, pb, pa = pixel(x * step + dx, y * step + dy)
                    r += pr
                    g += pg
                    b += pb
                    a += pa
                    n += 1
            if a == 0:
                row += b"\x00\x00\x00\x00"
            else:
                row += bytes((r // n, g // n, b // n, a // n))
        rows.append(bytes(row))
    return rows


def png(size, rows):
    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    body = b"".join(b"\x00" + row for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(body, 9))
        + chunk(b"IEND", b"")
    )


for size in (128, 256):
    path = os.path.join(OUT, f"{APP_ID}.{size}.png")
    with open(path, "wb") as f:
        f.write(png(size, render(size)))
    print("wrote", path)
