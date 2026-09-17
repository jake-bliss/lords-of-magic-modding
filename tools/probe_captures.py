"""Read `screencapture` BMPs and difference them into connected components.

Two things about the engine's BMP writer are non-standard, and both have already cost a wrong
reading:

- `bfOffBits` says 14 while the pixel data actually starts at byte 54, so the header field
  cannot be trusted and the offset is fixed here.
- **The pixel bytes are stored R, G, B, not the BMP-standard B, G, R.** Decoded per the
  standard the interface stone comes out blue and the terrain purple; decoded as written, the
  stone is brown and the grass green. Equality-only differencing is blind to this, which is why
  it survived earlier runs, but any colour conclusion drawn through a standards-compliant
  reader would be exactly wrong.

Differencing is by connected component rather than bounding box. The world map animates
between captures, so a naive box around every changed pixel once reported a 110x191 subject
that was really 49x67.
"""

from __future__ import annotations

import struct
from collections import deque
from dataclasses import dataclass
from pathlib import Path

PIXEL_OFFSET = 54


@dataclass(frozen=True)
class Capture:
    width: int
    height: int
    pixels: list[list[tuple[int, int, int]]]  # [row][column] = (r, g, b), top row first

    def pixel(self, x: int, y: int) -> tuple[int, int, int]:
        return self.pixels[y][x]


@dataclass(frozen=True)
class Component:
    pixels: list[tuple[int, int]]

    @property
    def size(self) -> int:
        return len(self.pixels)

    @property
    def bounds(self) -> tuple[int, int, int, int]:
        """(left, top, width, height)."""
        xs = [x for x, _ in self.pixels]
        ys = [y for _, y in self.pixels]
        return min(xs), min(ys), max(xs) - min(xs) + 1, max(ys) - min(ys) + 1


def read_capture(path: Path | str) -> Capture:
    data = Path(path).read_bytes()
    if data[:2] != b"BM":
        raise ValueError(f"{path}: not a BMP")
    width, height = struct.unpack_from("<ii", data, 18)
    bits = struct.unpack_from("<H", data, 28)[0]
    if bits != 24:
        raise ValueError(f"{path}: expected 24-bit pixels, found {bits}")
    height = abs(height)
    stride = (width * 3 + 3) // 4 * 4
    rows = []
    for y in range(height):
        base = PIXEL_OFFSET + (height - 1 - y) * stride  # BMP rows are stored bottom-up
        rows.append(
            [
                (data[base + x * 3], data[base + x * 3 + 1], data[base + x * 3 + 2])
                for x in range(width)
            ]
        )
    return Capture(width=width, height=height, pixels=rows)


def changed_components(before: Capture, after: Capture) -> list[Component]:
    """Every maximal 8-connected run of pixels that differ, largest first."""
    if (before.width, before.height) != (after.width, after.height):
        raise ValueError("captures differ in size")
    changed = {
        (x, y)
        for y in range(before.height)
        for x in range(before.width)
        if before.pixels[y][x] != after.pixels[y][x]
    }
    seen: set[tuple[int, int]] = set()
    components: list[Component] = []
    for start in changed:
        if start in seen:
            continue
        queue = deque([start])
        seen.add(start)
        member: list[tuple[int, int]] = []
        while queue:
            x, y = queue.popleft()
            member.append((x, y))
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    neighbour = (x + dx, y + dy)
                    if neighbour in changed and neighbour not in seen:
                        seen.add(neighbour)
                        queue.append(neighbour)
        components.append(Component(pixels=member))
    components.sort(key=lambda component: component.size, reverse=True)
    return components


def describe(before_path: Path | str, after_path: Path | str, minimum: int = 40) -> str:
    before = read_capture(before_path)
    after = read_capture(after_path)
    components = [c for c in changed_components(before, after) if c.size >= minimum]
    lines = [f"{Path(before_path).name} -> {Path(after_path).name}: {len(components)} components"]
    for component in components:
        left, top, width, height = component.bounds
        colours = sorted(
            {after.pixel(x, y) for x, y in component.pixels},
            key=lambda rgb: -sum(rgb),
        )[:4]
        lines.append(
            f"  n={component.size:5d} top-left=({left},{top}) {width}x{height} "
            f"brightest={colours}"
        )
    return "\n".join(lines)


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 3:
        raise SystemExit("usage: probe_captures.py BEFORE.bmp AFTER.bmp [AFTER2.bmp ...]")
    for later in sys.argv[2:]:
        print(describe(sys.argv[1], later))
