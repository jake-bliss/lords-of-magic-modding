#!/usr/bin/env python3
"""Decode Lords of Magic IMP sprites. Python standard library only.

    python3 tools/imp_read.py FILE.imp            one line per frame, as --describe-imp counts them

This exists for the player's machine, like `mpq_read.py`: the HD overlay's setup builds sprite
upscales from the player's own imp.mpq on Windows, where the asset viewer (Rust, StormLib) does not
run. It is a PORT of `spikes/asset-viewer/src/imp.rs` -- `ImpSprite::parse` and the pixel path it
calls (`read_frame_pixels`, `packed_sizes`, `pixel_layout_for`, `unpack_pixels`, the RLE decoders)
-- not a re-derivation, and it is checked against that decoder rather than against itself:
tests/test_imp_read.py requires every member's frame count to equal `--describe-imp`'s and every
frame's indices, palette and colour key to equal `--export-imp-frame`'s. Keep the two in step: a
rule changed in imp.rs is changed here too. (A codec in this project re-derived rather than ported
once silently dropped a feature; see docs/research-log.md.)

What the format holds, as the Rust reads it:

  header      byte 0 flags: 0x30 depth (0x00 8bpp, 0x10 1bpp, 0x20 2bpp, 0x30 4bpp), 0x01 RLE;
              byte 2 the record variant; byte 3 the colour key (the transparent palette index);
              u16 max width @4, u16 max height @6, u32 palette offset @8,
              u16 sequence count @26, u32 sequence table offset @28
  palette     256 entries stored BLUE, GREEN, RED, pad (measured in the game's own framebuffer)
  sequences   16-byte records: 11 bytes metadata, u8 facing count, u32 facing table offset
  facings     8-byte records: u16 metadata, u16 frame count, u32 frame table offset
  frames      16-byte records: u8 flags, u8 hotspot count, u16 w, u16 h, u16 encoded size,
              u32 hotspot array (or an origin pair), u32 pixels. Flag 0x08 makes the pixel dword a
              frame index (a duplicate); 0x04 makes it another frame's pixel offset (shared pixels).

Any failure anywhere refuses the whole member, as the Rust parser does.
"""
from __future__ import annotations

import argparse
import dataclasses
import hashlib
import pathlib
import struct
from typing import List, Optional, Tuple

FILE_HEADER_SIZE = 32
SEQUENCE_RECORD_SIZE = 16
FACING_RECORD_SIZE = 8
FRAME_RECORD_SIZE = 16
HOTSPOT_RECORD_SIZE = 6
HOTSPOT_ALIGNMENT = 8
PALETTE_COLORS = 256
PALETTE_BYTES = PALETTE_COLORS * 4
FRAME_FLAG_SHARED_PIXELS = 0x04
FRAME_FLAG_DUPLICATE = 0x08
FILE_FLAG_RLE = 0x01
FILE_FLAG_DEPTH = 0x30
DEPTHS = {0x00: 8, 0x10: 1, 0x20: 2, 0x30: 4}


class ImpError(ValueError):
    pass


@dataclasses.dataclass
class Frame:
    flags: int
    width: int                   # 0 for a duplicate or shared-pixel frame: see resolved_frame
    height: int
    indices: bytes               # one palette index per pixel, row by row; empty for those too
    source_frame: Optional[int]  # the frame a duplicate or shared-pixel record stands for
    record_offset: int
    hotspot_count: int
    pixels_offset: Optional[int]
    packed_size: Optional[int]


@dataclasses.dataclass
class Facing:
    metadata: int
    first_frame: int
    frame_count: int


@dataclasses.dataclass
class Sequence:
    metadata: bytes
    first_facing: int
    facing_count: int
    first_frame: int
    frame_count: int


@dataclasses.dataclass
class Sprite:
    file_flags: int
    record_variant: int
    compressed: bool
    bits_per_pixel: int
    maximum_width: int
    maximum_height: int
    color_key: int
    palette: List[Tuple[int, int, int]]      # 256 x (r, g, b)
    sequences: List[Sequence]
    facings: List[Facing]
    frames: List[Frame]
    duplicate_frame_count: int
    digest: str = ""             # the member's own bytes, hashed: what a cache of its frames keys on

    def resolved_frame(self, index: int) -> Frame:
        """The frame whose pixels `index` shows: itself, or what its duplicate chain ends at."""
        current = index
        for _ in range(len(self.frames) + 1):
            if not 0 <= current < len(self.frames):
                raise ImpError(f"IMP frame index {current} is out of range")
            frame = self.frames[current]
            if frame.source_frame is None:
                return frame
            current = frame.source_frame
        raise ImpError("IMP duplicate-frame references contain a facing")


def _require_range(source: bytes, offset: int, count: int, item_size: int, label: str) -> None:
    if offset + count * item_size > len(source):
        raise ImpError(f"IMP {label} is truncated")


def _u16(source: bytes, offset: int) -> int:
    if offset + 2 > len(source):
        raise ImpError("IMP u16 is truncated")
    return source[offset] | source[offset + 1] << 8


def _u32(source: bytes, offset: int) -> int:
    if offset + 4 > len(source):
        raise ImpError("IMP u32 is truncated")
    return struct.unpack_from("<I", source, offset)[0]


def _hotspot_bytes_for(count: int) -> int:
    return (count * HOTSPOT_RECORD_SIZE + HOTSPOT_ALIGNMENT - 1) & ~(HOTSPOT_ALIGNMENT - 1)


def parse(source: bytes) -> Sprite:
    """`ImpSprite::parse`: every sequence, facing and frame record, each frame's pixels decoded."""
    if len(source) < FILE_HEADER_SIZE:
        raise ImpError("IMP file header is truncated")
    file_flags = source[0]
    record_variant = source[2]
    color_key = source[3]
    compressed = bool(file_flags & FILE_FLAG_RLE)
    bits_per_pixel = DEPTHS[file_flags & FILE_FLAG_DEPTH]
    maximum_width = _u16(source, 4)
    maximum_height = _u16(source, 6)
    palette_offset = _u32(source, 8)
    sequence_count = _u16(source, 26)
    sequence_table_offset = _u32(source, 28)
    if maximum_width == 0 or maximum_height == 0:
        raise ImpError("IMP maximum dimensions must be nonzero")
    if sequence_count == 0:
        raise ImpError("IMP has no animation sequences")
    _require_range(source, sequence_table_offset, sequence_count, SEQUENCE_RECORD_SIZE, "sequence table")
    _require_range(source, palette_offset, 1, PALETTE_BYTES, "palette")

    # Stored blue, green, red, pad (imp.rs: measured in the game's own framebuffer, 2026-09-23).
    raw_palette = source[palette_offset:palette_offset + PALETTE_BYTES]
    palette = [(raw_palette[i + 2], raw_palette[i + 1], raw_palette[i]) for i in range(0, PALETTE_BYTES, 4)]

    duplicate_frame_count = 0
    sequences: List[Sequence] = []
    facings: List[Facing] = []
    frames: List[Frame] = []
    pixel_sources: dict = {}

    for sequence_index in range(sequence_count):
        sequence_offset = sequence_table_offset + sequence_index * SEQUENCE_RECORD_SIZE
        sequence_metadata = bytes(source[sequence_offset:sequence_offset + 11])
        sequence_facings = source[sequence_offset + 11]
        facing_table_offset = _u32(source, sequence_offset + 12)
        if sequence_facings == 0:
            raise ImpError(f"IMP sequence {sequence_index} has no facings")
        _require_range(source, facing_table_offset, sequence_facings, FACING_RECORD_SIZE, "facing table")
        sequence_first_facing = len(facings)
        sequence_first_frame = len(frames)

        for facing_index in range(sequence_facings):
            facing_offset = facing_table_offset + facing_index * FACING_RECORD_SIZE
            facing_metadata = _u16(source, facing_offset)
            facing_frames = _u16(source, facing_offset + 2)
            frame_table_offset = _u32(source, facing_offset + 4)
            _require_range(source, frame_table_offset, 1, FRAME_RECORD_SIZE, "frame table")
            _require_range(source, frame_table_offset, facing_frames, FRAME_RECORD_SIZE, "frame table")
            facing_first_frame = len(frames)
            for frame_index in range(facing_frames):
                frame_offset = frame_table_offset + frame_index * FRAME_RECORD_SIZE
                frame_hotspots = source[frame_offset + 1]
                frame_flags = source[frame_offset]
                width = _u16(source, frame_offset + 2)
                height = _u16(source, frame_offset + 4)
                encoded_size = _u16(source, frame_offset + 6)
                auxiliary = _u32(source, frame_offset + 8)
                pixels_offset = _u32(source, frame_offset + 12)
                empty_frame = width == 0 and height == 0
                if not empty_frame and (width == 0 or height == 0):
                    raise ImpError(f"IMP frame {frame_index} in facing {facing_index} has partial "
                                   "zero dimensions")
                if width > maximum_width or height > maximum_height:
                    raise ImpError(f"IMP frame {frame_index} in facing {facing_index} exceeds "
                                   "maximum dimensions")
                if frame_hotspots > 0:
                    _require_range(source, auxiliary, 1, _hotspot_bytes_for(frame_hotspots), "frame hotspots")
                    _require_range(source, auxiliary, 1, frame_hotspots * HOTSPOT_RECORD_SIZE, "frame hotspots")
                shared_pixels = bool(frame_flags & FRAME_FLAG_SHARED_PIXELS)
                duplicate_frame = shared_pixels or bool(frame_flags & FRAME_FLAG_DUPLICATE)
                if duplicate_frame:
                    if shared_pixels:
                        if pixels_offset not in pixel_sources:
                            raise ImpError("IMP shared-pixel frame references unknown pixel offset "
                                           f"{pixels_offset}")
                        source_frame = pixel_sources[pixels_offset]
                    else:
                        if pixels_offset >= len(frames):
                            raise ImpError(f"IMP duplicate frame reference {pixels_offset} is out of range")
                        source_frame = pixels_offset
                    duplicate_frame_count += 1
                    frames.append(Frame(frame_flags, 0, 0, b"", source_frame, frame_offset,
                                        frame_hotspots, None, None))
                    continue
                if empty_frame:
                    packed = b""
                else:
                    packed, _ = read_frame_pixels(source, pixels_offset, width, height, bits_per_pixel,
                                                  compressed, record_variant, encoded_size)
                indices = unpack_pixels(packed, width, height, bits_per_pixel)
                logical_index = len(frames)
                if not empty_frame:
                    pixel_sources.setdefault(pixels_offset, logical_index)
                frames.append(Frame(frame_flags, width, height, indices, None, frame_offset,
                                    frame_hotspots, pixels_offset, len(packed)))
            facings.append(Facing(facing_metadata, facing_first_frame, facing_frames))
        sequences.append(Sequence(sequence_metadata, sequence_first_facing, sequence_facings,
                                  sequence_first_frame, len(frames) - sequence_first_frame))

    return Sprite(file_flags, record_variant, compressed, bits_per_pixel, maximum_width,
                  maximum_height, color_key, palette, sequences, facings, frames, duplicate_frame_count,
                  hashlib.sha256(source).hexdigest()[:16])


# --- pixels --------------------------------------------------------------------------------------

def read_frame_pixels(source: bytes, pixels_offset: int, width: int, height: int, bits_per_pixel: int,
                      compressed: bool, record_variant: int, encoded_size: int) -> Tuple[bytes, int]:
    """(bit-packed pixels, stored bytes consumed). A compressed variant-0 record declares no
    payload length, so its stream is decoded until it reaches ANY acceptable packed size; every
    other variant declares one."""
    if width == 0 or height == 0:
        raise ImpError("IMP frame has no pixels to read; empty frames carry no payload")
    pixel_count = width * height
    sizes = packed_sizes(width, height, bits_per_pixel)
    if compressed:
        if record_variant == 0:
            if pixels_offset > len(source):
                raise ImpError("IMP frame pixel offset is invalid")
            return _decode_rle_until_size(source, pixels_offset, sizes)
        _require_range(source, pixels_offset, 1, encoded_size, "frame pixels")
        return _decode_rle_exact(source, pixels_offset, pixels_offset + encoded_size, sizes), encoded_size
    if record_variant != 0:
        if encoded_size not in sizes:
            raise ImpError(f"IMP raw frame declares unsupported packed size {encoded_size}")
        packed_size = encoded_size
    else:
        packed_size = next((s for s in sizes if s * 8 >= pixel_count * bits_per_pixel), None)
        if packed_size is None:
            raise ImpError("IMP raw frame has no packed size")
    _require_range(source, pixels_offset, 1, packed_size, "frame pixels")
    return bytes(source[pixels_offset:pixels_offset + packed_size]), packed_size


@dataclasses.dataclass(frozen=True)
class PackedSizes:
    tight_floor: int             # one bitstream, final partial byte dropped (1bpp only)
    tight_ceil: int              # one bitstream, final partial byte padded
    row_padded: int              # every scanline restarted on a byte boundary

    @classmethod
    def for_frame(cls, width: int, height: int, bits_per_pixel: int) -> "PackedSizes":
        bits = width * height * bits_per_pixel
        row_bytes = (width * bits_per_pixel + 7) // 8
        return cls(bits // 8, (bits + 7) // 8, row_bytes * height)

    def acceptable(self, bits_per_pixel: int) -> List[int]:
        sizes = ([self.tight_floor, self.tight_ceil, self.row_padded] if bits_per_pixel == 1
                 else [self.tight_ceil, self.row_padded])
        return sorted(set(sizes))


def packed_sizes(width: int, height: int, bits_per_pixel: int) -> List[int]:
    """Every byte length a frame of this shape may store its packed pixels in, sorted."""
    return PackedSizes.for_frame(width, height, bits_per_pixel).acceptable(bits_per_pixel)


TIGHT, ROW_PADDED = "tight", "row-padded"


def layouts_are_identical(width: int, height: int, bits_per_pixel: int) -> bool:
    return width * bits_per_pixel % 8 == 0 or height <= 1


def pixel_layout_for(packed_size: int, width: int, height: int, bits_per_pixel: int) -> str:
    """imp.rs's rule, in its order: identical layouts read tight; a length that is the row-padded
    one (and differs from the tight one) reads row-padded; everything else -- including the
    ambiguous equal-length shapes -- reads tight, a documented choice rather than a deduction."""
    sizes = PackedSizes.for_frame(width, height, bits_per_pixel)
    if layouts_are_identical(width, height, bits_per_pixel):
        return TIGHT
    if packed_size == sizes.row_padded and sizes.row_padded != sizes.tight_ceil:
        return ROW_PADDED
    return TIGHT


def _decode_rle_packet(source: bytes, position: int, output: bytearray) -> int:
    """One packet from `position`; returns where the next one starts. A control byte below 0x80
    repeats the next byte control + 3 times -- not ByteRun1 -- and any other is 0x100 - control
    literal bytes."""
    if position >= len(source):
        raise ImpError("IMP RLE control byte is truncated")
    control = source[position]
    position += 1
    if control < 0x80:
        if position >= len(source):
            raise ImpError("IMP RLE repeated value is truncated")
        output += bytes((source[position],)) * (control + 3)
        return position + 1
    count = 0x100 - control
    end = position + count
    if end > len(source):
        raise ImpError("IMP RLE literal is truncated")
    output += source[position:end]
    return end


def _decode_rle_exact(source: bytes, start: int, end: int, sizes: List[int]) -> bytes:
    window = source[start:end]
    position, output = 0, bytearray()
    while position < len(window):
        position = _decode_rle_packet(window, position, output)
    if len(output) not in sizes:
        raise ImpError(f"IMP RLE expands to unsupported packed size {len(output)}")
    return bytes(output)


def _decode_rle_until_size(source: bytes, start: int, sizes: List[int]) -> Tuple[bytes, int]:
    maximum_size = max(sizes) if sizes else 0
    output = bytearray()
    if 0 in sizes:
        return b"", 0
    position = start
    while True:
        position = _decode_rle_packet(source, position, output)
        if len(output) in sizes:
            return bytes(output), position - start
        if len(output) > maximum_size:
            raise ImpError(f"IMP RLE expands beyond the largest supported packed size {maximum_size}")


def _unpack_table(bits_per_pixel: int) -> List[bytes]:
    per_byte = 8 // bits_per_pixel
    mask = (1 << bits_per_pixel) - 1
    return [bytes((value >> (8 - bits_per_pixel * (sub + 1))) & mask for sub in range(per_byte))
            for value in range(256)]


UNPACK = {bpp: _unpack_table(bpp) for bpp in (1, 2, 4)}


def unpack_pixels(packed: bytes, width: int, height: int, bits_per_pixel: int) -> bytes:
    """One palette index per pixel, most significant bits first, in the layout
    `pixel_layout_for` names. Missing trailing pixels (a 1bpp tight-floor frame) read as 0."""
    if len(packed) not in packed_sizes(width, height, bits_per_pixel):
        raise ImpError(f"IMP packed pixels have unsupported size {len(packed)}")
    pixel_count = width * height
    if bits_per_pixel == 8:
        return bytes(packed)
    table = UNPACK[bits_per_pixel]
    if pixel_layout_for(len(packed), width, height, bits_per_pixel) == ROW_PADDED:
        row_bytes = (width * bits_per_pixel + 7) // 8
        pixels = b"".join(b"".join(table[b] for b in packed[r:r + row_bytes])[:width]
                          for r in range(0, len(packed) - len(packed) % row_bytes, row_bytes))
    else:
        pixels = b"".join(table[b] for b in packed)
    pixels = pixels[:pixel_count]
    return pixels + bytes(pixel_count - len(pixels))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("imp", type=pathlib.Path)
    args = parser.parse_args()
    sprite = parse(args.imp.read_bytes())
    for index, frame in enumerate(sprite.frames):
        shown = sprite.resolved_frame(index)
        source = "direct" if frame.source_frame is None else f"source:{frame.source_frame}"
        print(f"frame\t{index}\t0x{frame.flags:02x};{source}\t{shown.width}x{shown.height}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
