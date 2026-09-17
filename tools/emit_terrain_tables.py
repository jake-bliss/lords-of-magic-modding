#!/usr/bin/env python3
"""Emit the Rust terrain-transition and sprite-type tables from a `terrainrings` run.

`docs/map-format.md` claims those constants are generated from the run's saved maps rather than
transcribed. A reviewer pointed out that the generator was a throwaway script, so the claim was not
checkable and a transcription error could have been copied into both the constants and the test
fixture that is supposed to catch one. This is that generator, committed.

Usage:
    python3 tools/emit_terrain_tables.py DIRECTORY [--check]

`DIRECTORY` holds the probe's `zr*.scn` maps and its `zprobe.log`. With `--check` it compares what
it would emit against what `spikes/asset-viewer/src/map.rs` currently contains and exits non-zero on
any difference, which is how the claim stays true. The run artifacts are proprietary-derived and are
not in the repository, so `--check` is a thing to run against the ignored artifacts directory, not a
unit test.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import engine_probe
from terrain_rings import DIRECTIONS, Grid, full_ring, ring

# Entries the probe counted but that produced no log line, in the 2026-09-17 run.
KNOWN_SILENT_ENTRIES = 1

MAP_RS = (
    Path(__file__).resolve().parents[1]
    / "spikes"
    / "asset-viewer"
    / "src"
    / "map.rs"
)


def measured_rings(directory: Path) -> dict[int, list[int]]:
    """The ring shared by the terrains that share one, per background.

    A background is only given a ring when the terrains that agree on it agree on **every** ring
    cell, not just the eight sampled midpoints. Anything less could hand a ragged ring to a painter
    as if it were eight clean tiles.
    """
    out: dict[int, list[int]] = {}
    size = engine_probe.RINGS_BLOB
    for background in engine_probe.RINGS_TERRAINS:
        path = directory / Path(engine_probe.rings_map_names()[background]).name
        if not path.is_file():
            continue
        grid = Grid(path)
        base = engine_probe.TERRAIN_BASE_TILES[background]
        candidates: dict[tuple[int, ...], int] = {}
        for terrain in engine_probe.RINGS_TERRAINS:
            origin = engine_probe.rings_blob_origin(terrain)
            full = full_ring(grid, origin, size)
            if any(len(tiles) > 1 for tiles in full.values()):
                continue  # ragged: not a per-direction ring at all
            sampled = ring(grid, origin, size)
            key = tuple(sampled[name] for _, name in DIRECTIONS)
            if set(key) == {base}:
                continue  # no transition: the background's own type, or a non-blending background
            candidates[key] = candidates.get(key, 0) + 1
        if not candidates:
            continue
        best, count = max(candidates.items(), key=lambda item: item[1])
        # A ring only counts as this background's rule if most painted terrains agree on it. One
        # terrain agreeing with itself is not a rule.
        if count >= 5:
            out[background] = list(best)
    return out


def sprite_table(directory: Path) -> tuple[list[tuple[str, int]], list[str], list[str], int, int]:
    """The dumped table, plus the accounting that says how complete it is.

    Returns `(pairs, arrays, name_only, records, counted)`.

    The section is bounded at **both** ends. An earlier version sliced only from the start marker,
    so the two trailing lines -- the probe's own count and the run's done line -- were tallied as
    dict entries. That made every "197 entries / 10 unaccounted" figure in the documentation two too
    high, and `--check` validated the wrong number, which is the opposite of what a checker is for.
    The true figures are 195 logged records and a `zcount` of 196.

    `counted` is that `zcount`, read back so the two can be compared. An empty dict and a failed
    enumeration look identical without it, and nothing was reading it.
    """
    log = directory / "zprobe.log"
    text = log.read_bytes().decode("latin-1")
    start = text.find("sprite type table start")
    end = text.find("sprite type table done")
    section = text[start:end] if end > start else text[start:]
    pairs = sorted(
        ((name, int(value)) for value, name in re.findall(
            r"sprite type\s+(\d+)\s+name\s+/(\S+)", section
        )),
        key=lambda pair: pair[1],
    )
    arrays = sorted(set(re.findall(r"sprite type\s+<array>\s+name\s+/(\S+)", section)))
    records = [
        row.strip()
        for row in section.split("\r")
        if row.strip() and row.strip() != "sprite type table start"
    ]
    name_only = sorted(
        row.replace("name", "").replace("/", "").strip()
        for row in records
        if "<array>" not in row and not re.match(r"sprite type\s+\d+\s+name\s+/", row)
    )
    counted_match = re.search(r"sprite type table done count\s+(\d+)", text)
    counted = int(counted_match.group(1)) if counted_match else -1
    return pairs, arrays, name_only, len(records), counted


def committed_rings() -> dict[int, list[int]]:
    source = MAP_RS.read_text()
    offsets = [
        int(offset)
        for _, _, offset in re.findall(
            r"TransitionOffset \{ direction: \((-?\d+), (-?\d+)\), offset: (-?\d+) \}", source
        )
    ]
    anchors = {
        int(terrain): int(anchor)
        for terrain, anchor in re.findall(
            r"terrain_type: (\d+), behaviour: TransitionBehaviour::Blends \{ anchor: (\d+) \}",
            source,
        )
    }
    return {
        background: [anchor + offset for offset in offsets]
        for background, anchor in anchors.items()
    }


def committed_sprites() -> list[tuple[str, int]]:
    source = MAP_RS.read_text()
    block = source[source.find("pub const TERRAIN_SPRITE_TYPES") :]
    block = block[: block.find("];")]
    return [(name, int(value)) for name, value in re.findall(r'\("(\S+)", (\d+)\)', block)]


def main() -> int:
    args = [value for value in sys.argv[1:] if not value.startswith("--")]
    check = "--check" in sys.argv[1:]
    if len(args) != 1:
        print(__doc__)
        return 2
    directory = Path(args[0])

    rings = measured_rings(directory)
    pairs, arrays, name_only, records, counted = sprite_table(directory)

    if check:
        problems: list[str] = []
        if rings != committed_rings():
            for background in sorted(set(rings) | set(committed_rings())):
                measured = rings.get(background)
                stored = committed_rings().get(background)
                if measured != stored:
                    problems.append(
                        f"background {background}: measured {measured}, committed {stored}"
                    )
        if pairs != committed_sprites():
            problems.append(
                f"sprite table: measured {len(pairs)} pairs, committed {len(committed_sprites())}"
            )
            for measured, stored in zip(pairs, committed_sprites()):
                if measured != stored:
                    problems.append(f"  first difference: measured {measured}, committed {stored}")
                    break
        # The probe counted its own iterations. If that disagrees with the rows it managed to log,
        # entries were enumerated and produced nothing -- which is a finding, not a rounding error.
        # One entry enumerated without producing a line in the 2026-09-17 run -- possibly a `cvs`
        # failure on one key, possibly a key whose name printed empty. Identifying it needs another
        # keypress. It is pinned rather than merely reported, because a checker that fails on a
        # known gap every time is a checker people stop running, while an unpinned gap is one that
        # grows unnoticed.
        silent = counted - records if counted >= 0 else 0
        print(f"enumerated-without-logging\t{silent}")
        if silent != KNOWN_SILENT_ENTRIES:
            problems.append(
                f"{silent} dict entries enumerated without logging; "
                f"{KNOWN_SILENT_ENTRIES} is the known gap from the 2026-09-17 run"
            )
        print(f"backgrounds-with-a-ring\t{len(rings)}")
        print(f"sprite-pairs\t{len(pairs)}")
        print(f"sprite-arrays\t{len(arrays)}")
        print(f"sprite-name-only\t{len(name_only)}")
        for name in name_only:
            print(f"name-only\t{name}")
        print(f"dict-records-logged\t{records}")
        print(f"probe-counted\t{counted}")
        print(f"unaccounted-records\t{records - len(pairs) - len(arrays)}")
        print(f"problems\t{len(problems)}")
        for problem in problems:
            print(f"problem\t{problem}")
        return 1 if problems else 0

    for background, values in sorted(rings.items()):
        print(f"// background {background}: {values}")
    print(f"// anchors: {{ {', '.join(f'{b}: {v[7] - 1}' for b, v in sorted(rings.items()))} }}")
    for name, value in pairs:
        print(f'    ("{name}", {value}),')
    for name in arrays:
        print(f'    "{name}",')
    for name in name_only:
        print(f'    "{name}",  // logged a name, no usable value')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
