#!/usr/bin/env python3
"""Repaint a rectangle of a compressed PBM image without changing the file's length.

Why this exists at all: when it was written there was no PBM/LBM encoder in this repository, so an
edit that had to re-compress the image could not be made at all.

**That is no longer the reason.** `spikes/asset-viewer/src/pbm.rs` and
`tools/portrait-upscale/lbm_png.py` both encode. What survives is the narrower guarantee below: this
tool is the only way to change pixels **without changing the file's length**, which is what makes an
edit provable by a positional byte comparison against the shipped member.

ByteRun1 makes one class of edit available anyway. The BODY chunk is a sequence of packets:

    0..127    the next n + 1 bytes are literal
    129..255  the next single byte repeats 257 - n times
    128       no operation

A *repeat* packet is two bytes that stand for up to 128 pixels, so rewriting the second of those two
bytes repaints every pixel it covers and the file keeps its exact length. That is the whole
mechanism. Nothing here re-compresses, and nothing here changes a packet's length.

Two deliberate limits follow from that, and both are refusals rather than approximations:

  * A run that straddles the rectangle's edge is skipped, not split. Splitting it would need an
    extra control byte, which is precisely the length change this may not make. The consequence is
    that the repainted region has a ragged edge -- visible in the output, and honest about it.
  * Literal packets are never touched. A literal byte could be rewritten in place, but a literal
    run inside the rectangle is by definition detailed pixels rather than flat colour, and
    repainting those one at a time would produce a different picture from the one the caller asked
    for without saying so.

The result is not "the rectangle becomes colour N". It is "every flat run wholly inside the
rectangle becomes colour N", which is a weaker and true statement. Render the output and look at it
before drawing any conclusion from it; `--export-pbm` in the asset viewer is the intended check.
"""

from __future__ import annotations

import argparse
import struct
import sys
from dataclasses import dataclass
from pathlib import Path


class PbmPatchError(Exception):
    """A file this tool will not edit, with the reason it refused."""


@dataclass(frozen=True)
class PatchResult:
    runs_rewritten: int
    pixels_repainted: int
    rows_scanned: int
    width: int
    height: int


def iter_chunks(data: bytes):
    """Yield (chunk id, body offset, body size) for each IFF chunk after the FORM header."""
    offset = 12
    while offset + 8 <= len(data):
        chunk_id = bytes(data[offset : offset + 4])
        size = struct.unpack(">I", data[offset + 4 : offset + 8])[0]
        yield chunk_id, offset + 8, size
        # IFF chunks are word aligned: an odd size is followed by one pad byte.
        offset += 8 + size + (size & 1)


def patch(data: bytearray, index: int, rect: tuple[int, int, int, int]) -> PatchResult:
    """Repaint flat runs inside `rect`. Mutates `data` in place and never changes its length."""
    if not 0 <= index <= 255:
        raise PbmPatchError(f"palette index must be 0..255, got {index}")
    if bytes(data[:4]) != b"FORM" or bytes(data[8:12]) != b"PBM ":
        raise PbmPatchError("not an IFF FORM PBM file")

    header = body = None
    for chunk_id, start, size in iter_chunks(data):
        if chunk_id == b"BMHD":
            header = start
        elif chunk_id == b"BODY":
            body = (start, size)
    if header is None:
        raise PbmPatchError("no BMHD chunk")
    if body is None:
        raise PbmPatchError("no BODY chunk")

    width, height, _x, _y, planes, _mask, compression = struct.unpack(
        ">HHhhBBB", bytes(data[header : header + 11])
    )
    if compression != 1:
        raise PbmPatchError(
            f"BODY compression is {compression}; this tool only edits ByteRun1 (1). "
            "An uncompressed body can be edited directly and needs no packet walk."
        )
    if planes != 8:
        raise PbmPatchError(f"expected 8 bitplanes (one byte per pixel), got {planes}")

    x0, y0, x1, y1 = rect
    if not (0 <= x0 < x1 <= width and 0 <= y0 < y1 <= height):
        raise PbmPatchError(
            f"rectangle {rect} is not inside the {width}x{height} image"
        )

    start, size = body
    end = start + size
    offset, row, column = start, 0, 0
    runs = pixels = 0
    while offset < end and row < height:
        control = data[offset]
        if control == 128:
            offset += 1
            continue
        if control < 128:
            count = control + 1
            offset += 1 + count
        else:
            count = 257 - control
            # Wholly inside, or not at all -- see the module docstring on straddling runs.
            if y0 <= row < y1 and column >= x0 and column + count <= x1:
                data[offset + 1] = index
                runs += 1
                pixels += count
            offset += 2
        column += count
        while column >= width:
            column -= width
            row += 1
    return PatchResult(runs, pixels, row, width, height)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--index", type=int, required=True, help="palette index to paint with")
    parser.add_argument(
        "--rect",
        type=int,
        nargs=4,
        required=True,
        metavar=("X0", "Y0", "X1", "Y1"),
        help="half-open rectangle in pixels: x0 y0 x1 y1",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="report what would change and write nothing",
    )
    arguments = parser.parse_args(argv)

    original = arguments.source.read_bytes()
    data = bytearray(original)
    try:
        result = patch(data, arguments.index, tuple(arguments.rect))
    except PbmPatchError as error:
        print(f"pbm_patch: {error}", file=sys.stderr)
        return 1

    if len(data) != len(original):
        # Unreachable by construction; asserted because the whole tool rests on it.
        print("pbm_patch: internal error, length changed", file=sys.stderr)
        return 1

    print(
        f"{result.runs_rewritten} run(s), {result.pixels_repainted} pixel(s) -> index "
        f"{arguments.index}"
    )
    print(
        f"{result.rows_scanned}/{result.height} rows scanned, {len(data)} bytes unchanged"
    )
    if result.runs_rewritten == 0:
        # A silent no-op would be packed and installed and prove nothing.
        print("pbm_patch: no run lay wholly inside the rectangle", file=sys.stderr)
        return 1
    if not arguments.dry_run:
        arguments.output.write_bytes(bytes(data))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
