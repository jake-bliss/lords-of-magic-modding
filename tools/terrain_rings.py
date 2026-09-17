#!/usr/bin/env python3
"""Read the `terrainrings` probe's saved maps into a transition-tile matrix.

The probe paints one 3x3 blob of every terrain type onto every terrain background and saves one map
per background. This reads the ring one cell outside each blob back out, so the numbers come from
the files rather than from a hand-written script that has to be re-derived every time.

Usage:
    python3 tools/terrain_rings.py DIRECTORY

`DIRECTORY` holds the probe's `zr*.scn` output. Nothing here writes, and no map data is stored in
the repository -- point it at the ignored artifacts directory.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import engine_probe

HEADER_SIZE = 16
CELL_SIZE = 8
HIGH_FLAG = 0x0080_0000

# The eight ring positions, in the order `LAND_TRANSITION_TILES` records them.
DIRECTIONS = [
    ((0, -1), "N"),
    ((0, 1), "S"),
    ((-1, 0), "W"),
    ((1, 0), "E"),
    ((-1, -1), "NW"),
    ((1, -1), "NE"),
    ((-1, 1), "SW"),
    ((1, 1), "SE"),
]


class Grid:
    def __init__(self, path: Path) -> None:
        raw = path.read_bytes()
        metadata, width, height, depth = struct.unpack_from("<4I", raw, 0)
        if depth != 8:
            raise ValueError(f"{path.name}: unsupported depth {depth}")
        if len(raw) < HEADER_SIZE + width * height * CELL_SIZE:
            raise ValueError(f"{path.name}: cell grid is truncated")
        self.path = path
        self.metadata = metadata
        self.width = width
        self.height = height
        self.raw = raw

    def tile(self, x: int, y: int) -> int:
        if not (0 <= x < self.width and 0 <= y < self.height):
            raise ValueError(f"({x}, {y}) is outside {self.width}x{self.height}")
        offset = HEADER_SIZE + (y * self.width + x) * CELL_SIZE
        return struct.unpack_from("<I", self.raw, offset)[0] & ~HIGH_FLAG

    def flagged(self, x: int, y: int) -> bool:
        offset = HEADER_SIZE + (y * self.width + x) * CELL_SIZE
        return bool(struct.unpack_from("<I", self.raw, offset)[0] & HIGH_FLAG)


def ring(grid: Grid, origin: tuple[int, int], size: int) -> dict[str, int]:
    """The tile at each of the eight ring positions around a `size`x`size` blob.

    Each direction is sampled at the middle of that edge, which is where the mapload run found a
    single consistent value per direction. A run whose edge is not uniform shows up as a spread in
    `--verbose`, not as a silently averaged number.
    """
    x0, y0 = origin
    centre = size // 2
    out: dict[str, int] = {}
    for (dx, dy), name in DIRECTIONS:
        x = x0 + (centre if dx == 0 else (-1 if dx < 0 else size))
        y = y0 + (centre if dy == 0 else (-1 if dy < 0 else size))
        out[name] = grid.tile(x, y)
    return out


def edge_spread(grid: Grid, origin: tuple[int, int], size: int) -> dict[str, set[int]]:
    """Every tile along each ring edge, so a non-uniform edge cannot be read as a single value."""
    x0, y0 = origin
    spread: dict[str, set[int]] = {name: set() for _, name in DIRECTIONS}
    for step in range(size):
        spread["N"].add(grid.tile(x0 + step, y0 - 1))
        spread["S"].add(grid.tile(x0 + step, y0 + size))
        spread["W"].add(grid.tile(x0 - 1, y0 + step))
        spread["E"].add(grid.tile(x0 + size, y0 + step))
    spread["NW"].add(grid.tile(x0 - 1, y0 - 1))
    spread["NE"].add(grid.tile(x0 + size, y0 - 1))
    spread["SW"].add(grid.tile(x0 - 1, y0 + size))
    spread["SE"].add(grid.tile(x0 + size, y0 + size))
    return spread


def core(grid: Grid, origin: tuple[int, int], size: int) -> set[int]:
    x0, y0 = origin
    return {grid.tile(x0 + dx, y0 + dy) for dy in range(size) for dx in range(size)}


def report(directory: Path, verbose: bool) -> int:
    size = engine_probe.RINGS_BLOB
    names = engine_probe.rings_map_names()
    failures: list[str] = []
    rows: dict[int, dict[int, dict[str, int]]] = {}

    for background in engine_probe.RINGS_TERRAINS:
        path = directory / Path(names[background]).name
        if not path.is_file():
            failures.append(f"missing {path.name}")
            continue
        grid = Grid(path)
        rows[background] = {}
        base = engine_probe.TERRAIN_BASE_TILES[background]
        # The far field must still be the forced background tile. If it is not, the row was
        # measured against something other than the background it claims and is worthless.
        corners = [(1, 1), (grid.width - 2, 1), (1, grid.height - 2), (grid.width - 2, grid.height - 2)]
        far = {grid.tile(x, y) for x, y in corners}
        if far != {base}:
            failures.append(
                f"{path.name}: far field is {sorted(far)}, expected [{base}] -- "
                "the background is not what this row assumes"
            )
        print(f"background {background}  tile {base}  {grid.width}x{grid.height}  {path.name}")
        for index, terrain in enumerate(engine_probe.RINGS_TERRAINS):
            origin = engine_probe.rings_blob_origin(index)
            values = ring(grid, origin, size)
            rows[background][terrain] = values
            spread = edge_spread(grid, origin, size)
            ragged = [name for name, tiles in spread.items() if len(tiles) > 1]
            marker = "  RAGGED:" + ",".join(sorted(ragged)) if ragged else ""
            same = " (same as background)" if terrain == background else ""
            print(
                f"   terrain {terrain:<3}"
                + " ".join(f"{name}={values[name]:<4}" for _, name in DIRECTIONS)
                + marker
                + same
            )
            if verbose:
                print(f"      core {sorted(core(grid, origin, size))}")
                for name, tiles in spread.items():
                    if len(tiles) > 1:
                        print(f"      edge {name} spread {sorted(tiles)}")
        print()

    # Which rows agree with which, so the "nine of eleven share a ring" shape can be checked per
    # background rather than assumed from the one that was measured first.
    print("=== per background: which painted terrains share a ring ===")
    for background, row in sorted(rows.items()):
        groups: dict[tuple[int, ...], list[int]] = {}
        for terrain, values in row.items():
            key = tuple(values[name] for _, name in DIRECTIONS)
            groups.setdefault(key, []).append(terrain)
        biggest = max(groups.values(), key=len)
        odd = sorted(t for group in groups.values() if group is not biggest for t in group)
        print(
            f"   background {background}: largest group {len(biggest)}/11 "
            f"{sorted(biggest)}  others {odd}"
        )

    print()
    print(f"failures\t{len(failures)}")
    for failure in failures:
        print(f"failure\t{failure}")
    return 1 if failures else 0


def main() -> int:
    args = [value for value in sys.argv[1:] if value != "--verbose"]
    verbose = "--verbose" in sys.argv[1:]
    if len(args) != 1:
        print(__doc__)
        return 2
    return report(Path(args[0]), verbose)


if __name__ == "__main__":
    raise SystemExit(main())
