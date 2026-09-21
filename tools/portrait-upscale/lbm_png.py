#!/usr/bin/env python3
"""LBM (FORM PBM) <-> PNG, with no third-party imaging library available.

Decoding and encoding both live here so a member's palette and its unmodelled chunks survive a
round trip. `spikes/asset-viewer/src/pbm.rs` is the other implementation of this format in the
repository and is the authority; this module exists because the upscaling pipeline is Python. Where
the two could drift, this one follows `pbm.rs`:

  * **Rows are padded to an even number of bytes.** `row_bytes = (width + 1) & ~1`. Measured, not
    assumed -- `tools/asset_validate.py` records the 88 odd-width `pic.mpq` members that decode
    only under this rule. An earlier version of this file concatenated BODY as a flat stream, which
    is correct for even widths and silently shifts one pixel per row for odd ones.
  * **Chunks that are not modelled are carried through verbatim.** `CRNG` colour cycling appears in
    645 of the 749 shipped portraits and is functional data; `DPPS` likewise. A writer that emits
    only what it models drops them silently. `pbm.rs` says this in as many words and this module
    had to learn it twice.
  * **`TINY` is the one chunk deliberately dropped**, because it is a thumbnail *derived* from
    BODY rather than independent metadata: preserving it verbatim over rewritten pixels ships a
    file whose thumbnail is the old artwork. Regenerating it needs a downscaler this repo does not
    have. Dropping it is format-valid on the corpus's own evidence (128 of 1,045 shipped PBMs carry
    no `TINY`) and was **Observed in gameplay 2026-09-20** to render correctly.
"""
from __future__ import annotations

import pathlib
import struct
import zlib

#: Chunks derived from BODY, so they cannot be carried across a pixel change.
DERIVED_CHUNKS = {b"TINY"}


def iter_chunks(data: bytes):
    """Yield (id, body) for each IFF chunk after the FORM header, in file order."""
    offset = 12
    while offset + 8 <= len(data):
        chunk_id = data[offset:offset + 4]
        size = struct.unpack(">I", data[offset + 4:offset + 8])[0]
        yield chunk_id, data[offset + 8:offset + 8 + size]
        offset += 8 + size + (size & 1)      # IFF chunks are word aligned


def row_bytes(width: int) -> int:
    """Bytes a single row occupies. See the module docstring: rows are padded to even."""
    return (width + 1) & ~1


def decode(path) -> tuple[int, int, bytes, list[tuple[int, int, int]], list[tuple[bytes, bytes]]]:
    """Return (width, height, indices, palette, chunks).

    `indices` is width*height with the row padding removed; `chunks` is every chunk as read, so a
    caller can re-emit the ones this module does not model.
    """
    data = pathlib.Path(path).read_bytes()
    if data[:4] != b"FORM" or data[8:12] != b"PBM ":
        raise ValueError(f"{path}: not an IFF FORM PBM image")

    chunks = list(iter_chunks(data))
    header = palette_bytes = body = None
    for chunk_id, payload in chunks:
        if chunk_id == b"BMHD":
            header = payload
        elif chunk_id == b"CMAP":
            palette_bytes = payload
        elif chunk_id == b"BODY":
            body = payload
    if header is None or palette_bytes is None or body is None:
        raise ValueError(f"{path}: missing BMHD, CMAP or BODY")
    if len(header) < 20:
        raise ValueError(f"{path}: BMHD is {len(header)} bytes, expected at least 20")

    width, height = struct.unpack(">HH", header[:4])
    planes, compression = header[8], header[10]
    if planes != 8:
        raise ValueError(f"{path}: {planes} planes; this module only handles 8-bit PBM")
    if compression not in (0, 1):
        raise ValueError(f"{path}: unknown compression {compression}")
    if len(palette_bytes) % 3:
        raise ValueError(f"{path}: CMAP is {len(palette_bytes)} bytes, not a multiple of 3")

    stride = row_bytes(width)
    if compression == 1:
        raw = bytearray()
        i = 0
        while i < len(body) and len(raw) < stride * height:
            control = body[i]
            i += 1
            if control < 128:
                raw += body[i:i + control + 1]
                i += control + 1
            elif control > 128:
                raw += bytes([body[i]]) * (257 - control)
                i += 1
    else:
        raw = bytearray(body[:stride * height])

    indices = bytearray()
    for y in range(height):
        indices += raw[y * stride:y * stride + width]     # drop the pad byte on odd widths
    palette = [tuple(palette_bytes[i * 3:i * 3 + 3]) for i in range(len(palette_bytes) // 3)]
    return width, height, bytes(indices), palette, chunks


def byterun1(row: bytes) -> bytes:
    """ByteRun1 for one row. Packets never span rows, matching pbm.rs."""
    out = bytearray()
    i, n = 0, len(row)
    while i < n:
        run = 1
        while i + run < n and row[i + run] == row[i] and run < 128:
            run += 1
        if run >= 2:
            out.append(257 - run)
            out.append(row[i])
            i += run
        else:
            literal = bytearray()
            while i < n and len(literal) < 128:
                if i + 2 < n and row[i] == row[i + 1] == row[i + 2]:
                    break
                literal.append(row[i])
                i += 1
            out.append(len(literal) - 1)
            out.extend(literal)
    return bytes(out)


def encode(dest, width: int, height: int, indices: bytes,
           palette: list[tuple[int, int, int]], chunks: list[tuple[bytes, bytes]]) -> int:
    """Write a FORM PBM, rewriting BMHD/CMAP/BODY and carrying every other chunk across.

    `chunks` is the list `decode` returned for the source member. Chunks in DERIVED_CHUNKS are
    dropped; anything else this module does not model is emitted verbatim, in its original order.
    """
    if len(indices) != width * height:
        raise ValueError(f"expected {width * height} indices for {width}x{height}, got {len(indices)}")
    if any(index >= len(palette) for index in indices):
        raise ValueError("an index addresses a colour the palette does not hold")

    source = {chunk_id: payload for chunk_id, payload in chunks}
    header = bytearray(source[b"BMHD"])
    struct.pack_into(">HH", header, 0, width, height)
    struct.pack_into(">hh", header, 16, width, height)     # pageWidth/pageHeight track the image
    header[10] = 1                                          # this encoder only emits ByteRun1

    stride = row_bytes(width)
    body = bytearray()
    for y in range(height):
        row = bytearray(indices[y * width:(y + 1) * width])
        if stride != width:
            row.append(row[-1] if row else 0)               # pad byte; value is not displayed
        body += byterun1(bytes(row))

    rebuilt = {
        b"BMHD": bytes(header),
        b"CMAP": b"".join(bytes(colour) for colour in palette),
        b"BODY": bytes(body),
    }
    out = bytearray()
    for chunk_id, payload in chunks:
        if chunk_id in DERIVED_CHUNKS:
            continue
        payload = rebuilt.get(chunk_id, payload)
        out += chunk_id + struct.pack(">I", len(payload)) + payload
        if len(payload) & 1:
            out += b"\x00"

    blob = b"FORM" + struct.pack(">I", 4 + len(out)) + b"PBM " + bytes(out)
    pathlib.Path(dest).write_bytes(blob)
    return len(blob)


def write_png(path, width: int, height: int, rgb_rows, scale: int = 1) -> None:
    """Write 8-bit truecolour PNG. `rgb_rows` is height rows of width (r, g, b) tuples."""
    raw = b""
    for row in rgb_rows:
        line = b"".join(bytes(pixel) * scale for pixel in row)
        raw += (b"\x00" + line) * scale

    def chunk(kind: bytes, payload: bytes) -> bytes:
        body = kind + payload
        return struct.pack(">I", len(payload)) + body + struct.pack(">I", zlib.crc32(body) & 0xffffffff)

    header = struct.pack(">IIBBBBB", width * scale, height * scale, 8, 2, 0, 0, 0)
    pathlib.Path(path).write_bytes(
        b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def read_png_rgb(path):
    """Minimal PNG reader: 8-bit truecolour or truecolour+alpha, non-interlaced."""
    data = pathlib.Path(path).read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    offset, idat = 8, b""
    width = height = depth = colour = interlace = None
    while offset < len(data):
        size = struct.unpack(">I", data[offset:offset + 4])[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        if kind == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", payload[:10])
            interlace = payload[12]
        elif kind == b"IDAT":
            idat += payload
        elif kind == b"IEND":
            break
        offset += 12 + size
    if depth != 8 or colour not in (2, 6):
        raise ValueError(f"{path}: unsupported PNG (depth={depth} colour={colour})")
    if interlace:
        raise ValueError(f"{path}: interlaced PNG is not supported")

    channels = 3 if colour == 2 else 4
    raw = zlib.decompress(idat)
    stride = width * channels
    rows, previous, position = [], bytearray(stride), 0
    for _ in range(height):
        filter_type = raw[position]
        position += 1
        line = bytearray(raw[position:position + stride])
        position += stride
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b = previous[i]
            c = previous[i - channels] if i >= channels else 0
            if filter_type == 0:
                pass
            elif filter_type == 1:
                line[i] = (line[i] + a) & 255
            elif filter_type == 2:
                line[i] = (line[i] + b) & 255
            elif filter_type == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif filter_type == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                predictor = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + predictor) & 255
            else:
                # Silently treating an unknown filter as None returns plausible corrupt pixels,
                # which is worse than refusing: the caller cannot tell it happened.
                raise ValueError(f"{path}: unknown PNG scanline filter {filter_type}")
        rows.append([tuple(line[x * channels:x * channels + 3]) for x in range(width)])
        previous = line
    return width, height, rows
