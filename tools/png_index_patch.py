#!/usr/bin/env python3
"""Rewrite the palette indices of an 8-bit indexed PNG, keeping its palette byte-for-byte.

`--import-png-imp` and `--import-png-pbm` both refuse a PNG whose PLTE differs from the template's
own palette: the written asset keeps the template's colour map, so a remapped palette would make
every index mean a different colour than the editor saw. That leaves exactly one legal edit -- move
pixels between existing palette entries -- and no image editor on this machine can be trusted to
make it. `sips` and ImageMagick both re-quantise, reorder or drop a PLTE without saying so, which
turns "repaint the cursor" into "the import was refused" or, worse, into a silent recolour of the
whole file.

So this reads and writes the PNG itself. It handles exactly one shape -- colour type 3, bit depth 8
-- because that is the only shape the two importers accept and the only one the exporters produce.
Anything else is refused by name rather than converted.

Two operations, and the difference between them is the whole point:

    --swap A=B     exchange palette indices A and B across the WHOLE image
    --fill I       set every pixel inside --rect to index I

`--swap` is length-preserving by construction and `--fill` is not. The IMP and PBM writers both
compress with a run-length encoder whose packet boundaries depend only on where neighbouring bytes
stop being equal. A permutation of the index alphabet -- which a set of disjoint swaps is -- maps
equal neighbours to equal neighbours and unequal to unequal, so every run keeps its length and the
encoded payload keeps its byte count. A fill merges runs and the payload shrinks. Both are wanted:
one rung of the engine ladder must change pixels without changing size, and a later one must change
size on purpose. Neither is a good stand-in for the other, so neither is the default.

Usage:
  tools/png_index_patch.py INPUT.png OUTPUT.png --swap A=B [--swap C=D ...]
  tools/png_index_patch.py INPUT.png OUTPUT.png --fill INDEX [--rect X0 Y0 X1 Y1]
  tools/png_index_patch.py INPUT.png --histogram
  tools/png_index_patch.py INPUT.png --compare-to OTHER.png
"""

from __future__ import annotations

import argparse
import struct
import sys
import zlib
from dataclasses import dataclass
from pathlib import Path

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

#: Chunks carried from input to output unchanged, in the order the input held them. IDAT is
#: rebuilt and is deliberately absent; so is IEND, which the writer emits itself.
_REBUILT_CHUNKS = frozenset({b"IDAT", b"IEND"})


class PngError(Exception):
    """A PNG this tool will not edit, as opposed to an argument it will not accept."""


@dataclass
class IndexedPng:
    width: int
    height: int
    #: One byte per pixel, row-major, `width * height` long.
    indices: bytearray
    #: Every non-pixel chunk of the source, as `(type, payload)`, in source order.
    chunks: list[tuple[bytes, bytes]]

    def palette(self) -> bytes:
        for kind, payload in self.chunks:
            if kind == b"PLTE":
                return payload
        raise PngError("indexed PNG has no PLTE chunk")

    def at(self, x: int, y: int) -> int:
        return self.indices[y * self.width + x]


def _iter_chunks(data: bytes):
    if not data.startswith(PNG_SIGNATURE):
        raise PngError("not a PNG: the 8-byte signature is missing")
    offset = len(PNG_SIGNATURE)
    while offset < len(data):
        if offset + 8 > len(data):
            raise PngError("truncated PNG: a chunk header runs past the end of the file")
        (length,) = struct.unpack_from(">I", data, offset)
        kind = data[offset + 4 : offset + 8]
        start = offset + 8
        end = start + length
        if end + 4 > len(data):
            raise PngError(f"truncated PNG: chunk {kind!r} runs past the end of the file")
        payload = data[start:end]
        (stored_crc,) = struct.unpack_from(">I", data, end)
        actual_crc = zlib.crc32(kind + payload) & 0xFFFFFFFF
        if stored_crc != actual_crc:
            raise PngError(
                f"PNG chunk {kind!r} fails its own CRC "
                f"(stored {stored_crc:08x}, computed {actual_crc:08x})"
            )
        yield kind, payload
        offset = end + 4


def _unfilter(raw: bytes, width: int, height: int) -> bytearray:
    """Undo the per-scanline filters of an 8-bit, one-byte-per-pixel image.

    Only one byte per pixel is handled, which is what colour type 3 at bit depth 8 is, so the
    `bpp` of the filter equations is 1 and the Paeth predictor's `a`, `b`, `c` are the byte to the
    left, above, and above-left. Written out rather than taken from a library because there is no
    Pillow on this machine and adding a dependency to repaint one cursor would be the larger act.
    """
    stride = width
    expected = (stride + 1) * height
    if len(raw) != expected:
        raise PngError(
            f"decompressed image data is {len(raw)} bytes; a {width}x{height} 8-bit indexed "
            f"image is {expected}"
        )
    out = bytearray(stride * height)
    previous = bytearray(stride)
    position = 0
    for row in range(height):
        filter_type = raw[position]
        position += 1
        line = bytearray(raw[position : position + stride])
        position += stride
        if filter_type == 0:
            pass
        elif filter_type == 1:
            for index in range(1, stride):
                line[index] = (line[index] + line[index - 1]) & 0xFF
        elif filter_type == 2:
            for index in range(stride):
                line[index] = (line[index] + previous[index]) & 0xFF
        elif filter_type == 3:
            for index in range(stride):
                left = line[index - 1] if index else 0
                line[index] = (line[index] + ((left + previous[index]) >> 1)) & 0xFF
        elif filter_type == 4:
            for index in range(stride):
                left = line[index - 1] if index else 0
                up = previous[index]
                up_left = previous[index - 1] if index else 0
                estimate = left + up - up_left
                distance_left = abs(estimate - left)
                distance_up = abs(estimate - up)
                distance_up_left = abs(estimate - up_left)
                if distance_left <= distance_up and distance_left <= distance_up_left:
                    predictor = left
                elif distance_up <= distance_up_left:
                    predictor = up
                else:
                    predictor = up_left
                line[index] = (line[index] + predictor) & 0xFF
        else:
            raise PngError(f"unknown PNG filter type {filter_type} on row {row}")
        out[row * stride : (row + 1) * stride] = line
        previous = line
    return out


def read_indexed_png(data: bytes) -> IndexedPng:
    width = height = None
    chunks: list[tuple[bytes, bytes]] = []
    compressed = bytearray()
    for kind, payload in _iter_chunks(data):
        if kind == b"IHDR":
            width, height, depth, colour_type, compression, filter_method, interlace = (
                struct.unpack(">IIBBBBB", payload)
            )
            if colour_type != 3:
                raise PngError(
                    f"expected an indexed PNG (colour type 3); got colour type {colour_type}"
                )
            if depth != 8:
                raise PngError(f"expected an 8-bit indexed PNG; got bit depth {depth}")
            if compression != 0 or filter_method != 0:
                raise PngError("unsupported PNG compression or filter method")
            if interlace != 0:
                raise PngError("interlaced PNGs are not supported; re-export without interlacing")
            chunks.append((kind, payload))
        elif kind == b"IDAT":
            compressed.extend(payload)
        elif kind == b"IEND":
            continue
        else:
            chunks.append((kind, payload))
    if width is None or height is None:
        raise PngError("PNG has no IHDR chunk")
    image = IndexedPng(
        width=width,
        height=height,
        indices=_unfilter(zlib.decompress(bytes(compressed)), width, height),
        chunks=chunks,
    )
    # Reading the palette here rather than at use turns "no PLTE" into an error about the file
    # instead of an error about the edit.
    image.palette()
    return image


def write_indexed_png(image: IndexedPng) -> bytes:
    """Serialise with filter type 0 on every row.

    The filters are a compression choice and carry no information, so re-emitting them would only
    invite the reader to compare this file's bytes with the exporter's and conclude something from
    a difference that means nothing. Every chunk that is not pixel data is copied through
    untouched, which is what keeps PLTE -- and the tRNS the IMP exporter writes beside it --
    byte-identical to the export the importers check against.
    """
    stride = image.width
    raw = bytearray()
    for row in range(image.height):
        raw.append(0)
        raw.extend(image.indices[row * stride : (row + 1) * stride])

    out = bytearray(PNG_SIGNATURE)

    def emit(kind: bytes, payload: bytes) -> None:
        out.extend(struct.pack(">I", len(payload)))
        out.extend(kind)
        out.extend(payload)
        out.extend(struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF))

    for kind, payload in image.chunks:
        if kind in _REBUILT_CHUNKS:
            continue
        emit(kind, payload)
    emit(b"IDAT", zlib.compress(bytes(raw), 9))
    emit(b"IEND", b"")
    return bytes(out)


def apply_swaps(image: IndexedPng, swaps: list[tuple[int, int]]) -> int:
    """Exchange pairs of palette indices across the whole image. Returns pixels moved.

    The swaps must be **disjoint**: together they have to be a permutation of the index alphabet,
    because that is the property that makes the edit length-preserving under a run-length encoder.
    An index appearing in two pairs is not a permutation and is refused rather than applied in
    argument order, which would depend on a detail no caller should have to know.
    """
    seen: set[int] = set()
    for left, right in swaps:
        for index in (left, right):
            if not 0 <= index <= 255:
                raise ValueError(f"palette index {index} is outside 0..255")
            if index in seen:
                raise ValueError(
                    f"palette index {index} appears in more than one --swap; the swaps must be "
                    "disjoint so that together they are a permutation"
                )
            seen.add(index)

    mapping = list(range(256))
    for left, right in swaps:
        mapping[left], mapping[right] = right, left

    moved = 0
    for position, index in enumerate(image.indices):
        replacement = mapping[index]
        if replacement != index:
            image.indices[position] = replacement
            moved += 1
    return moved


def apply_fill(image: IndexedPng, index: int, rect: tuple[int, int, int, int]) -> int:
    """Set every pixel in the half-open rectangle to `index`. Returns pixels changed."""
    if not 0 <= index <= 255:
        raise ValueError(f"palette index {index} is outside 0..255")
    x0, y0, x1, y1 = rect
    if not (0 <= x0 < x1 <= image.width and 0 <= y0 < y1 <= image.height):
        raise ValueError(
            f"rectangle {x0} {y0} {x1} {y1} is not inside the {image.width}x{image.height} image; "
            "the rectangle is half-open, so x1 and y1 are one past the last pixel"
        )
    changed = 0
    for y in range(y0, y1):
        base = y * image.width
        for x in range(x0, x1):
            if image.indices[base + x] != index:
                image.indices[base + x] = index
                changed += 1
    return changed


def histogram(image: IndexedPng) -> list[tuple[int, int]]:
    """(index, count) for every index the image uses, most used first."""
    counts: dict[int, int] = {}
    for index in image.indices:
        counts[index] = counts.get(index, 0) + 1
    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))


def _parse_swap(value: str) -> tuple[int, int]:
    left, separator, right = value.partition("=")
    if not separator:
        raise argparse.ArgumentTypeError(f"expected A=B, got {value!r}")
    try:
        return int(left, 0), int(right, 0)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"{value!r}: {error}") from error


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path, nargs="?")
    parser.add_argument("--swap", type=_parse_swap, action="append", default=[], metavar="A=B")
    parser.add_argument("--fill", type=int, default=None, metavar="INDEX")
    parser.add_argument("--rect", type=int, nargs=4, default=None, metavar=("X0", "Y0", "X1", "Y1"))
    parser.add_argument(
        "--histogram",
        action="store_true",
        help="print the index histogram and the palette colour of each used index, and stop",
    )
    parser.add_argument(
        "--compare-to",
        type=Path,
        default=None,
        metavar="OTHER.png",
        help=(
            "compare pixel indices and palette with another indexed PNG and exit non-zero on any "
            "difference. This is how an expected value gets checked against what came back out of "
            "a packed archive, without either side being a digest somebody wrote down earlier"
        ),
    )
    arguments = parser.parse_args(argv)

    try:
        image = read_indexed_png(arguments.input.read_bytes())
    except (OSError, PngError, zlib.error) as error:
        print(f"{arguments.input}: {error}", file=sys.stderr)
        return 1

    if arguments.histogram:
        palette = image.palette()
        print("index\tcount\tr\tg\tb")
        for index, count in histogram(image):
            red, green, blue = palette[index * 3 : index * 3 + 3]
            print(f"{index}\t{count}\t{red}\t{green}\t{blue}")
        return 0

    if arguments.compare_to is not None:
        try:
            other = read_indexed_png(arguments.compare_to.read_bytes())
        except (OSError, PngError, zlib.error) as error:
            print(f"{arguments.compare_to}: {error}", file=sys.stderr)
            return 1
        if (image.width, image.height) != (other.width, other.height):
            print(
                f"size differs: {image.width}x{image.height} vs "
                f"{other.width}x{other.height}",
                file=sys.stderr,
            )
            return 1
        if image.palette() != other.palette():
            print("palette differs", file=sys.stderr)
            return 1
        differing = sum(1 for a, b in zip(image.indices, other.indices) if a != b)
        print(
            f"compare\t{arguments.input}\t{arguments.compare_to}\t"
            f"{image.width}x{image.height}\tdiffering-pixels={differing}"
        )
        return 0 if differing == 0 else 1

    if arguments.output is None:
        parser.error("an output path is required unless --histogram is given")
    if arguments.output.exists():
        print(f"output already exists; choose a fresh path: {arguments.output}", file=sys.stderr)
        return 1
    if bool(arguments.swap) == (arguments.fill is not None):
        parser.error("give exactly one of --swap and --fill")
    if arguments.fill is None and arguments.rect is not None:
        parser.error("--rect applies to --fill only; --swap is whole-image by design")

    try:
        if arguments.fill is not None:
            rect = tuple(arguments.rect) if arguments.rect else (0, 0, image.width, image.height)
            changed = apply_fill(image, arguments.fill, rect)
            operation = f"fill index {arguments.fill} over {rect}"
        else:
            changed = apply_swaps(image, arguments.swap)
            operation = "swap " + " ".join(f"{a}={b}" for a, b in arguments.swap)
    except (PngError, ValueError) as error:
        print(f"{arguments.input}: {error}", file=sys.stderr)
        return 1

    arguments.output.write_bytes(write_indexed_png(image))
    print(
        f"wrote\t{arguments.output}\t{image.width}x{image.height}\t{operation}\t"
        f"pixels-changed={changed}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
