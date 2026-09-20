"""ComicTagger-compatible perceptual cover hashes (ADR-074), standalone.

A faithful re-implementation of ComicTagger's `ImageHasher`
(`comictaggerlib/imagehasher.py`): `average_hash`, `difference_hash`,
and `perception_hash`. The load-bearing fidelity comes from Pillow —
the same library ComicTagger runs on — so the resample (Lanczos) and
the grayscale (`L`) match bit-for-bit. MEASURED (2026-09-20): hashing
the exact `cv_url` a `localcv.db` row used reproduces its stored
`ct_ahash`/`ct_phash` at Hamming distance 0.

The hash math itself is trivial arithmetic; only Pillow is required.
No ComicTagger code is copied (it is Apache-2.0; this project is GPL).

Values are unsigned 64-bit and are stored as TEXT decimals in the
cache, matching the Rust `comictagger_hash` and the localcv import.
"""

from __future__ import annotations

import itertools
import math
import statistics
from statistics import median

try:
    from PIL import Image

    PIL_AVAILABLE = True
except ImportError:  # pragma: no cover - exercised only without Pillow
    PIL_AVAILABLE = False


def _flat(image) -> list:
    # Pillow renamed getdata() -> get_flattened_data(); use whichever the
    # installed Pillow exposes, so the script works across versions.
    getter = getattr(image, "get_flattened_data", None)
    if getter is not None:
        return list(getter())
    return list(image.getdata())


def average_hash(image, width: int = 8, height: int = 8) -> int:
    """ComicTagger `average_hash`: Lanczos resize to width x height, L
    grayscale, one bit per pixel above the mean, MSB-first."""
    resized = image.resize((width, height), Image.Resampling.LANCZOS).convert("L")
    pixels = _flat(resized)
    avg = statistics.mean(pixels)
    h = 0
    for i, p in enumerate(pixels):
        if p > avg:
            h |= 1 << (len(pixels) - 1 - i)
    return h


def difference_hash(image, width: int = 8, height: int = 8) -> int:
    """ComicTagger `difference_hash`: a (width+1) x height Lanczos grid,
    bit set where a pixel is brighter than its right neighbour."""
    resized = image.resize((width + 1, height), Image.Resampling.LANCZOS).convert("L")
    pixels = _flat(resized)
    h = 0
    z = (width * height) - 1
    for y in range(height):
        for x in range(width):
            idx = x + ((width + 1) * y)
            if pixels[idx] < pixels[idx + 1]:
                h |= 1 << z
            z -= 1
    return h


def _dct1(block: list) -> list:
    n = len(block)
    out = [0.0] * n
    for k in range(n):
        s = 0.0
        for i in range(n):
            s += block[i] * math.cos(math.pi * k * (2 * i + 1) / (2 * n))
        out[k] = s
    return out


def _dct2(block: list, axis: int = 0) -> list:
    rows = len(block)
    cols = len(block[0])
    out = [[0.0] * cols for _ in range(rows)]
    if axis == 0:
        for i in range(rows):
            out[i] = _dct1(list(block[i]))
    else:
        for j in range(cols):
            column = [block[i][j] for i in range(rows)]
            dc = _dct1(column)
            for i in range(rows):
                out[i][j] = dc[i]
    return out


def perception_hash(image) -> int:
    """ComicTagger `perception_hash`: a 32x32 Lanczos grid, its top-left
    8x8 DCT block thresholded at the median, MSB-first."""
    size = 32
    resized = image.convert("L").resize((size, size), Image.Resampling.LANCZOS)
    data = _flat(resized)
    grid = [data[r * size : r * size + size] for r in range(size)]
    dct = _dct2(_dct2(grid, axis=0), axis=1)
    low = list(itertools.chain.from_iterable(row[:8] for row in dct[:8]))
    med = median(low)
    h = 0
    for i, p in enumerate(low):
        if p > med:
            h |= 1 << (len(low) - 1 - i)
    return h


def hamming_distance(h1: int, h2: int) -> int:
    return (int(h1) ^ int(h2)).bit_count()
