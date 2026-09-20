"""Walk every IMP file's record tables from raw bytes, as a cross-check on the Rust decoder.

This exists to make corpus claims about IMP *structure* falsifiable without re-deriving them
through the code that produced them. It deliberately **shares no code with `src/imp.rs`** — it
re-reads the record layout from the file bytes with `struct.unpack`, so agreement between the two
is evidence and disagreement names a real defect in one of them. Running the decoder twice would
not have been evidence at all.

It was written on 2026-09-19 to test one specific claim, and refuted it. A test in
`src/imp_playback.rs` excused a playback refusal wherever "the corpus holds a facing with no
frames". This walk reports **zero** facings with `frame_count == 0` across all 14,921 facing
records in GS5R3's 1,800-member `imp.mpq` — while reporting 6,552 zero-*dimension* frames across
107 files, which is a different shape and the one that genuinely justifies the viewer's
blank-frame skipping. The two had been conflated.

That second number is the point of the tool, not a bonus. A scan that reports "none" is only worth
something if it can be shown to find a comparable shape when one is present; the zero-dimension
frame count is the positive control that keeps the zero honest. Any new claim of the form "no IMP
record in the corpus has property X" should come with a count of some property Y that the same
walk does find.

The published controls it reproduces, which is how you know the walk lands on the right offsets:
1,800 members, 4,667 sequences, 14,921 facing records (`docs/imp-format.md`). A run that misses
any of those three is mis-parsing, not making a discovery.

Record layout, from `docs/imp-format.md` and the record sizes in `src/imp.rs`:

    file header  32 bytes   sequence count u16 @26, sequence table u32 @28
    sequence     16 bytes   facing count u8 @+11, facing table u32 @+12
    facing        8 bytes   metadata u16 @+0, frame_count u16 @+2, frame table u32 @+4
    frame        16 bytes   width u16 @+2, height u16 @+4

Input is a directory of extracted IMP members, e.g. from

    .build/lom-mpq extract "$ENGLISH/imp.mpq" OUTDIR --listfile artifacts/.../lords-of-magic.txt

Nothing here reads an archive, a game profile, or writes anything. It reads extracted files.
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

FILE_HEADER_SIZE = 32
SEQUENCE_RECORD_SIZE = 16
FACING_RECORD_SIZE = 8
FRAME_RECORD_SIZE = 16

# The counts `docs/imp-format.md` publishes for GS5R3's `imp.mpq`. A walk that does not reproduce
# all three has landed on the wrong offsets, and its findings mean nothing.
GS5R3_CONTROLS = {"members": 1800, "sequences": 4667, "facings": 14921}


def _u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def _u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def walk(data: bytes) -> dict[str, int]:
    """Tally one IMP file's record tables. Raises on any out-of-range table pointer."""
    if len(data) < FILE_HEADER_SIZE:
        raise ValueError("file header is truncated")
    sequence_count = _u16(data, 26)
    sequence_table = _u32(data, 28)
    if sequence_count == 0:
        raise ValueError("no animation sequences")

    tally = {
        "sequences": 0,
        "facings": 0,
        "zero_frame_facings": 0,
        "frames": 0,
        "zero_dimension_frames": 0,
    }
    for sequence_index in range(sequence_count):
        sequence = sequence_table + sequence_index * SEQUENCE_RECORD_SIZE
        if sequence + SEQUENCE_RECORD_SIZE > len(data):
            raise ValueError(f"sequence record {sequence_index} out of range")
        facing_count = data[sequence + 11]
        facing_table = _u32(data, sequence + 12)
        tally["sequences"] += 1
        for facing_index in range(facing_count):
            facing = facing_table + facing_index * FACING_RECORD_SIZE
            if facing + FACING_RECORD_SIZE > len(data):
                raise ValueError(f"facing record {facing_index} out of range")
            frame_count = _u16(data, facing + 2)
            frame_table = _u32(data, facing + 4)
            tally["facings"] += 1
            if frame_count == 0:
                tally["zero_frame_facings"] += 1
            for frame_index in range(frame_count):
                frame = frame_table + frame_index * FRAME_RECORD_SIZE
                if frame + FRAME_RECORD_SIZE > len(data):
                    raise ValueError(f"frame record {frame_index} out of range")
                tally["frames"] += 1
                if _u16(data, frame + 2) == 0 and _u16(data, frame + 4) == 0:
                    tally["zero_dimension_frames"] += 1
    return tally


def scan(root: Path) -> dict[str, object]:
    totals = {
        "members": 0,
        "sequences": 0,
        "facings": 0,
        "zero_frame_facings": 0,
        "frames": 0,
        "zero_dimension_frames": 0,
    }
    zero_frame_examples: list[str] = []
    zero_dimension_files: set[str] = set()
    skipped: list[tuple[str, str]] = []

    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() == ".imp"):
        name = str(path.relative_to(root))
        try:
            tally = walk(path.read_bytes())
        except Exception as error:  # noqa: BLE001 - a refusal is a reportable result here
            skipped.append((name, str(error)))
            continue
        totals["members"] += 1
        for key, value in tally.items():
            totals[key] += value
        if tally["zero_frame_facings"] and len(zero_frame_examples) < 10:
            zero_frame_examples.append(name)
        if tally["zero_dimension_frames"]:
            zero_dimension_files.add(name)

    return {
        "totals": totals,
        "zero_frame_examples": zero_frame_examples,
        "zero_dimension_files": len(zero_dimension_files),
        "skipped": skipped,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("directory", type=Path, help="directory of extracted .imp members")
    parser.add_argument(
        "--expect-gs5r3-controls",
        action="store_true",
        help="fail unless the three published GS5R3 control counts are reproduced exactly",
    )
    arguments = parser.parse_args(argv)
    if not arguments.directory.is_dir():
        print(f"not a directory: {arguments.directory}", file=sys.stderr)
        return 2

    report = scan(arguments.directory)
    totals = report["totals"]
    for key in (
        "members",
        "sequences",
        "facings",
        "zero_frame_facings",
        "frames",
        "zero_dimension_frames",
    ):
        print(f"{key}\t{totals[key]}")
    print(f"zero_dimension_files\t{report['zero_dimension_files']}")
    print(f"skipped\t{len(report['skipped'])}")
    for name, error in report["skipped"][:10]:
        print(f"skipped_member\t{name}\t{error}")
    for name in report["zero_frame_examples"]:
        print(f"zero_frame_facing\t{name}")

    if arguments.expect_gs5r3_controls:
        measured = {key: totals[key] for key in GS5R3_CONTROLS}
        if measured != GS5R3_CONTROLS:
            print(
                f"control mismatch: measured {measured}, published {GS5R3_CONTROLS} -- this walk "
                f"is mis-parsing, so nothing else it reports means anything",
                file=sys.stderr,
            )
            return 1
    return 1 if report["skipped"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
