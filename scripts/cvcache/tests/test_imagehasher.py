"""Golden test for the ComicTagger-compatible cover hashes.

The two fixture covers are the same real Comic Vine issue covers used by
the Rust `cr-image` golden test (issues 7 and 8). Their expected hashes
are ComicTagger's `ImageHasher` values stored in the reference
`localcv.db`. Because this hasher runs on Pillow — the same library
ComicTagger uses — it must reproduce those values exactly (Hamming 0),
unlike the Rust port which carries a documented JPEG-decoder tolerance.

Skipped when Pillow is not installed.
"""

import os
import unittest

from scripts.cvcache import imagehasher

_FIXTURES = os.path.join(
    os.path.dirname(__file__), "..", "..", "..",
    "crates", "cr-image", "tests", "fixtures",
)

# localcv.db comic_covers values for the two fixtures.
_GOLDEN = {
    "cover7.jpg": {"ahash": 51290160142786527,
                   "phash": 10756634926609361816},
    "cover8.jpg": {"ahash": 17730933771727232,
                   "phash": 14921684108427048273},
}


@unittest.skipUnless(imagehasher.PIL_AVAILABLE, "Pillow not installed")
class GoldenHashTest(unittest.TestCase):
    def _open(self, name):
        return imagehasher.Image.open(os.path.join(_FIXTURES, name))

    def test_average_hash_matches_comictagger_golden(self):
        for name, want in _GOLDEN.items():
            with self._open(name) as im:
                got = imagehasher.average_hash(im)
            self.assertEqual(
                imagehasher.hamming_distance(got, want["ahash"]), 0,
                f"{name}: ahash {got} vs golden {want['ahash']}",
            )

    def test_perception_hash_matches_comictagger_golden(self):
        for name, want in _GOLDEN.items():
            with self._open(name) as im:
                got = imagehasher.perception_hash(im)
            self.assertEqual(
                imagehasher.hamming_distance(got, want["phash"]), 0,
                f"{name}: phash {got} vs golden {want['phash']}",
            )


if __name__ == "__main__":
    unittest.main()
