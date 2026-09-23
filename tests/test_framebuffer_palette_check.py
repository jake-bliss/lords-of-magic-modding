"""Tests for the framebuffer channel-order check."""

import struct
import sys
import unittest
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

import framebuffer_palette_check as check  # noqa: E402


def chunk(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))


def indexed_png(width: int, height: int, indices: list[int], palette: list[tuple[int, int, int]]) -> bytes:
    rows = b"".join(b"\x00" + bytes(indices[y * width:(y + 1) * width]) for y in range(height))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 3, 0, 0, 0))
            + chunk(b"PLTE", b"".join(bytes(c) for c in palette))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def frame(width: int, height: int, pixels: dict[tuple[int, int], int]) -> bytes:
    body = [pixels.get((x, y), 0) for y in range(height) for x in range(width)]
    return b"LOMHDRAW" + struct.pack("<III", width, height, 16) + struct.pack(f"<{len(body)}H", *body)


# Red and green differ in every drawn entry, so a red/green swap cannot pass by accident.
PALETTE = [(0, 255, 0), (255, 0, 0), (200, 40, 16), (24, 96, 200), (120, 8, 64)]


class FramebufferPaletteCheckTest(unittest.TestCase):
    def sprite(self) -> bytes:
        # 3x2: a transparent pixel and a shadow pixel that must not be counted.
        return indexed_png(3, 2, [0, 2, 3, 4, 1, 2], PALETTE)

    def screen(self, order=(0, 1, 2), at=(4, 3)) -> bytes:
        pixels = {}
        for (sx, sy), index in {(1, 0): 2, (2, 0): 3, (0, 1): 4, (2, 1): 2}.items():
            c = PALETTE[index]
            pixels[(at[0] + sx, at[1] + sy)] = check.rgb565(*(c[i] for i in order))
        pixels[(at[0], at[1])] = 0xFFFF            # under the transparent pixel: anything
        pixels[(at[0] + 1, at[1] + 1)] = 0x1234    # under the shadow: anything
        return frame(10, 8, pixels)

    def test_the_exported_order_wins_when_it_is_the_drawn_order(self) -> None:
        result = check.check(self.screen(), self.sprite(), 4, 3)
        self.assertEqual(result["pixels"], 4)
        self.assertEqual(result["orderings"]["RGB"], 1.0)
        self.assertEqual(max(result["orderings"], key=result["orderings"].get), "RGB")
        self.assertTrue(result["one_colour_per_index"])

    def test_a_red_green_swap_is_named_as_one(self) -> None:
        result = check.check(self.screen(order=(1, 0, 2)), self.sprite(), 4, 3)
        self.assertEqual(result["orderings"]["GRB"], 1.0)
        self.assertLess(result["orderings"]["RGB"], 1.0)

    def test_transparent_and_shadow_pixels_are_not_compared(self) -> None:
        # Garbage under indices 0 and 1 changes nothing.
        self.assertEqual(check.check(self.screen(), self.sprite(), 4, 3)["pixels"], 4)

    def test_refuses_a_sprite_that_does_not_fit(self) -> None:
        with self.assertRaises(ValueError):
            check.check(self.screen(), self.sprite(), 8, 3)

    def test_refuses_a_frame_that_is_not_16_bit(self) -> None:
        data = bytearray(self.screen())
        struct.pack_into("<I", data, 16, 32)
        with self.assertRaises(ValueError):
            check.check(bytes(data), self.sprite(), 4, 3)


if __name__ == "__main__":
    unittest.main()
