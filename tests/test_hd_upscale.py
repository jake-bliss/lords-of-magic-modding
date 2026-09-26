"""hd_upscale.damage_score: the content check that catches an upscale of the right size whose pixels
are garbage (a tester's GPU wrote diagonal bands and speckle, 2026-09-26).

The unit tests run on procedural images: a textured sprite, a plausible upscale of it (each pixel
spread over its 2x2 block and softened, as every upscaler does), and the corruptions seen or feared
-- rows written at the wrong stride, a shear, noise blocks. The corpus test runs every render of a
real `--sprites` run when LOM_SPRITE_WORK points at its lomhd_work/sprites folder; renders are
game-derived and not in the repository, so it skips elsewhere."""

from __future__ import annotations

import os
import pathlib
import random
import struct
import sys
import unittest
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

import hd_upscale  # noqa: E402

W, H = 40, 30


def source(w: int = W, h: int = H, border: int = 3) -> bytes:
    """Straight RGBA: a transparent border, inside a smooth two-way gradient with some texture."""
    out = bytearray()
    for y in range(h):
        for x in range(w):
            inside = border <= x < w - border and border <= y < h - border
            out += (bytes(((x * 5 + y * 2) % 256, (y * 7) % 256, (x * y) % 256, 255)) if inside
                    else bytes((0x20, 0x22, 0x28, 0)))
    return bytes(out)


def upscale(w: int, h: int, rgba: bytes) -> bytes:
    """Each source pixel over its 2x2 block, blended a quarter toward its right and lower
    neighbours: smooth like a real upscale, and not a bare pixel repeat."""
    def px(x: int, y: int) -> bytes:
        x, y = min(x, w - 1), min(y, h - 1)
        return rgba[(y * w + x) * 4:(y * w + x) * 4 + 4]
    out = bytearray()
    for Y in range(2 * h):
        for X in range(2 * w):
            here, nxt = px(X // 2, Y // 2), px(X // 2 + (X & 1), Y // 2 + (Y & 1))
            out += bytes((3 * a + b) // 4 for a, b in zip(here, nxt))
    return bytes(out)


def stride(w: int, h: int, hd: bytes, extra: int) -> bytes:
    """The upscale read back with rows `extra` pixels too long: diagonal bands."""
    s2, row = (2 * w + extra) * 4, 2 * w * 4
    flat = hd + bytes(s2 * 2 * h)
    return b"".join(flat[y * s2:y * s2 + row] for y in range(2 * h))


def shear(w: int, h: int, hd: bytes) -> bytes:
    row = 2 * w * 4
    return b"".join(hd[y * row:(y + 1) * row][-4 * y % row:] + hd[y * row:(y + 1) * row][:-4 * y % row]
                    for y in range(2 * h))


def noise(w: int, h: int, hd: bytes, fraction: float, seed: int = 1) -> bytes:
    rng, out = random.Random(seed), bytearray(hd)
    blocks = [(bx, by) for by in range(0, 2 * h, 8) for bx in range(0, 2 * w, 8)]
    for bx, by in rng.sample(blocks, int(len(blocks) * fraction)):
        for y in range(by, min(2 * h, by + 8)):
            for x in range(bx, min(2 * w, bx + 8)):
                i = (y * 2 * w + x) * 4
                out[i:i + 3] = bytes(rng.randrange(256) for _ in range(3))
    return bytes(out)


class DamageScore(unittest.TestCase):
    def setUp(self) -> None:
        self.src = source()
        self.hd = upscale(W, H, self.src)

    def score(self, hd: bytes, src: bytes | None = None, w: int = W, h: int = H):
        return hd_upscale.damage_score(w, h, self.src if src is None else src, hd)

    def test_a_plausible_upscale_passes(self) -> None:
        s = self.score(self.hd)
        self.assertLess(s, 0.3)
        self.assertFalse(hd_upscale.looks_damaged(s))

    def test_rows_at_the_wrong_stride_are_caught(self) -> None:
        for extra in (1, 2, 3, 4):
            with self.subTest(extra=extra):
                self.assertTrue(hd_upscale.looks_damaged(self.score(stride(W, H, self.hd, extra))))

    def test_a_shear_and_heavy_noise_are_caught(self) -> None:
        self.assertTrue(hd_upscale.looks_damaged(self.score(shear(W, H, self.hd))))
        self.assertTrue(hd_upscale.looks_damaged(self.score(noise(W, H, self.hd, 0.5))))

    def test_what_the_upscaler_bleeds_into_the_transparent_edge_is_not_counted(self) -> None:
        """Everything outside the opaque interior -- the border and the ring next to it -- may be
        anything: the upscaler rightly bleeds the background into edges."""
        hd = bytearray(self.hd)
        for Y in range(2 * H):
            for X in range(2 * W):
                x, y = X // 2, Y // 2
                if not (4 <= x < W - 4 and 4 <= y < H - 4):
                    hd[(Y * 2 * W + X) * 4:(Y * 2 * W + X) * 4 + 3] = b"\xff\x00\xff"
        self.assertAlmostEqual(self.score(bytes(hd)), self.score(self.hd))

    def test_a_dithered_sprite_smoothed_by_the_upscaler_passes(self) -> None:
        """A checkerboard spell effect: every upscaler averages it, which is the right result. The
        texture term is what lets it through (esp06br scored 0.74 in the corpus, the highest)."""
        dither = bytearray()
        for y in range(H):
            for x in range(W):
                dither += bytes((200, 40, 40, 255) if (x + y) % 2 else (90, 20, 20, 255))
        smooth = bytes((145, 30, 30, 255)) * (4 * W * H)
        self.assertLess(self.score(smooth, bytes(dither)), hd_upscale.DAMAGE_THRESHOLD)

    def test_too_little_to_judge_passes_unjudged(self) -> None:
        tiny = source(12, 10)                          # a 6x4 opaque interior: 4x2 judgeable pixels
        self.assertIsNone(hd_upscale.damage_score(12, 10, tiny, bytes(12 * 10 * 16)))
        self.assertFalse(hd_upscale.looks_damaged(None))

    def test_rgb_pictures_are_judged_whole(self) -> None:
        rgb = b"".join(self.src[i:i + 3] for i in range(0, len(self.src), 4))
        hd3 = b"".join(self.hd[i:i + 3] for i in range(0, len(self.hd), 4))
        bad3 = b"".join(stride(W, H, self.hd, 2)[i:i + 3] for i in range(0, len(self.hd), 4))
        self.assertFalse(hd_upscale.looks_damaged(hd_upscale.damage_score(W, H, rgb, hd3, 3, 3)))
        self.assertTrue(hd_upscale.looks_damaged(hd_upscale.damage_score(W, H, rgb, bad3, 3, 3)))

    def test_sizes_that_do_not_fit_are_an_error(self) -> None:
        with self.assertRaises(ValueError):
            hd_upscale.damage_score(W, H, self.src, self.hd[:-4])


def read_prep(path: pathlib.Path) -> tuple[int, int, bytes]:
    """hd_sprites.write_png_rgba's own output: one IDAT, filter 0 on every row."""
    data = path.read_bytes()
    w, h = struct.unpack(">II", data[16:24])
    pos, idat = 8, b""
    while pos < len(data):
        n = struct.unpack(">I", data[pos:pos + 4])[0]
        if data[pos + 4:pos + 8] == b"IDAT":
            idat += data[pos + 8:pos + 8 + n]
        pos += 12 + n
    raw, row = zlib.decompress(idat), w * 4 + 1
    return w, h, b"".join(raw[y * row + 1:(y + 1) * row] for y in range(h))


@unittest.skipUnless(os.environ.get("LOM_SPRITE_WORK") and pathlib.Path(os.environ["LOM_SPRITE_WORK"]).is_dir(),
                     "set LOM_SPRITE_WORK to the lomhd_work/sprites folder of a --sprites run")
class Corpus(unittest.TestCase):
    """Every render of a real run, through the shipped check: none is damaged, and the wrong-stride
    and shear corruptions of a sample of them are caught."""

    def test_every_clean_render_passes_and_corruptions_of_them_do_not(self) -> None:
        import hd_sprites
        root = pathlib.Path(os.environ["LOM_SPRITE_WORK"])
        items = [(opt.name, r.name) for opt in sorted((root / "render").iterdir()) if opt.is_dir()
                 for r in sorted(opt.glob("*.png"))]
        self.assertGreater(len(items), 1000, "not a whole run")
        failed, judged, caught, tried = [], 0, 0, 0
        for start in range(0, len(items), 400):
            chunk = items[start:start + 400]
            preps = {item: read_prep(root / "prep" / item[1]) for item in chunk}
            pixels = hd_sprites.read_renders(root, [(f"render/{o}/{n}", 2 * preps[(o, n)][0], 2 * preps[(o, n)][1])
                                                    for o, n in chunk])
            for k, (option, name) in enumerate(chunk):
                w, h, src = preps[(option, name)]
                hd = pixels[f"render/{option}/{name}"]
                self.assertIsInstance(hd, bytes, name)
                score = hd_upscale.damage_score(w, h, src, hd)
                judged += score is not None
                if hd_upscale.looks_damaged(score):
                    failed.append((round(score, 3), option, name))
                if (start + k) % 25 == 0 and score is not None:
                    for bad in (stride(w, h, hd, 1), stride(w, h, hd, 3), shear(w, h, hd)):
                        tried += 1
                        caught += hd_upscale.looks_damaged(hd_upscale.damage_score(w, h, src, bad))
        self.assertEqual(failed, [])
        self.assertGreater(judged, 0.99 * len(items))
        self.assertGreater(caught / tried, 0.95, f"{caught}/{tried} corruptions caught")


if __name__ == "__main__":
    unittest.main()
