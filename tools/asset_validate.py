#!/usr/bin/env python3
"""Content validation for the paletted IFF images a mod tree replaces.

This is the answer to the Phase 5 box "add automated dimension and palette validation". It reads
one member's bytes and says whether they are an image the engine's decoder could read: the IFF
chunk walk lands exactly on the end of the FORM, the BMHD describes dimensions and a compression
this pipeline has actually seen, the CMAP is a whole number of RGB triples, the BODY decodes to
exactly the pixels the BMHD promised, and **every pixel index the BODY produces addresses a colour
the CMAP actually holds**.

Everything asserted here was measured first, **Observed 2026-09-18**. The population has to be named
exactly, because three different true numbers describe it and an earlier draft of this file mixed
them. Vanilla `pic.mpq` holds **1,071 entries**: 1,045 IFF `FORM`/`PBM ` images and 26 `.til`
tilesets, which begin `LBM=` and are not images. Of the 1,045 images, **1,044 carry a recovered
`.lbm` name and one does not** -- `File00001070.xxx`, a 640x381 PBM, the single member of vanilla
`pic.mpq` that resisted name recovery in PR #66.

That last member matters for what this module can actually reach. `is_image_member` selects on the
filename suffix, so **the shipping validator covers the 1,044 named members, not all 1,045**. The
sweep below used a wider selection than the validator ships; the figures are stated against the 1,045
because that is what was measured, and the coverage delivered is 1,044.

Across the three installed profiles the same distinction produces two more numbers, and both are
right:

- **3,467** image entries (1,045 + 1,045 + 1,377);
- **3,464** of those carry an `.lbm` name (1,044 + 1,044 + 1,376);
- **3,463** distinct files result from extracting them to a directory, because GS5R3's `pic.mpq`
  holds `portrait\\AIpotM.lbm` **twice** under one byte-identical name. `name -> block` is not
  injective in this corpus (see `docs/mpq-inventory.md`), so a directory loses one of the two. The
  sweep was run over an extracted tree and therefore reports 3,463.

Measured on the 1,045 images of vanilla `pic.mpq`:

- every one declares 8 planes, masking 0, compression 1 (ByteRun1) and a 768-byte CMAP -- 256
  entries, so no shipped member can put a pixel index out of palette range and the palette check
  passes 1,045 of 1,045;
- every FORM size walks the chunk list to exactly the end of the file, with no trailing bytes;
- the chunks present are BMHD, CMAP, BODY, CRNG, DPPS and TINY.

Two measurements decided the two rules that would otherwise have been guessed wrong, and both are
cases where a stricter validator would have rejected shipped content:

**Rows are padded to an even byte count.** Decoding ByteRun1 rows as `width` bytes makes a packet
appear to run past the end of the row in 88 of the 1,045 members -- and those 88 are exactly the 88
members whose width is odd. Decoding rows as `(width + 1) & ~1` bytes, the ILBM row-padding rule,
leaves zero overruns in the whole corpus. `spikes/asset-viewer/src/pbm.rs` handles the same 88
files by *clamping* a packet at the row boundary, which reaches the right pixels but describes the
cause as an edge case in the encoder rather than as padding.

**A packet that still crosses a padded row boundary is reported, not refused.** Swept across all
3,463 extracted images of the three installed profiles' `pic.mpq`, exactly one member has one:
`PORTRAIT/decr5p00.lbm` in GS5R3, 70x67, whose BODY runs 30 overlong packets from row 36 to the
last row. Clamping them at the row boundary discards 1,353 pixels and leaves the final 531 bytes of
the BODY unreachable, which is why the shipped file renders at all. That is a defect in a shipped
member, so this module clamps exactly as the engine's decoder does and reports a warning: refusing
it would make the validator reject content the game ships, which is the failure mode
`docs/roadmap.md` describes as "a validator demanding UTF-8 would reject shipped members". A BODY
that cannot fill the image at all is still an error, because no decoder can invent those pixels.

**A single trailing zero byte in the BODY is normal.** 65 members have exactly one 0x00 left over
after the last row decodes, and in all 65 the BODY chunk size is even -- the encoder padded the
chunk to an even size *inside* the declared size. No member has more than one. So one trailing zero
is accepted silently and anything beyond it is reported.

The pad column is decoded but is not a pixel: the palette check covers the `width` columns the
image actually shows, which is what the engine draws.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

ERROR = "error"
WARNING = "warning"
NOTE = "note"

#: Member-name suffixes this module claims it can read. Case-folded before comparison because the
#: archive holds both `.lbm` and `.LBM` spellings (1,033 and 11 of them in vanilla `pic.mpq`).
IMAGE_SUFFIXES = (".lbm",)

#: Compression codes with a decoder here. 0 is uncompressed, 1 is ByteRun1. Every shipped member
#: is 1; 0 is implemented because the format defines it and a re-encoder may emit it.
SUPPORTED_COMPRESSION = (0, 1)

#: The only plane count this pipeline can read: one byte per pixel, chunky, as PBM stores it.
CHUNKY_PLANES = 8

BMHD_LENGTH = 20


@dataclass(frozen=True)
class AssetFinding:
    """One thing wrong with a member's bytes. The caller supplies the location."""

    severity: str
    check: str
    message: str


@dataclass(frozen=True)
class RowCrossing:
    """A ByteRun1 packet that ran past the end of its row, and how much was clamped away."""

    row: int
    count: int
    discarded: int


@dataclass(frozen=True)
class ImageSummary:
    """What a member turned out to be, for a caller that wants to count coverage."""

    width: int
    height: int
    planes: int
    compression: int
    palette_entries: int
    pixels_checked: int


class AssetError(Exception):
    """A structural failure that makes the rest of the member unreadable."""


# Chunks that occur at most once per member. CRNG is deliberately absent: it is a colour-cycling
# range record and a file carries a list of them (up to 16, measured). See the duplicate check.
SINGLETON_CHUNKS = (b"BMHD", b"CMAP", b"BODY", b"DPPS", b"TINY")


def is_image_member(member: str) -> bool:
    return member.casefold().endswith(IMAGE_SUFFIXES)


def iter_chunks(data: bytes, start: int, end: int):
    """Walk IFF chunks in `data[start:end]`, yielding `(chunk_id, payload_start, size)`.

    Raises `AssetError` on a chunk whose declared size runs past `end`, and leaves it to the caller
    to check that the walk finished exactly on `end` -- a walk that stops short is a different
    defect from one that overruns, and reporting them as one message loses which happened.
    """
    cursor = start
    while cursor + 8 <= end:
        chunk_id = data[cursor : cursor + 4]
        (size,) = struct.unpack_from(">I", data, cursor + 4)
        payload = cursor + 8
        if payload + size > end:
            raise AssetError(
                f"chunk {_chunk_name(chunk_id)} at offset {cursor} declares {size} bytes, which "
                f"runs {payload + size - end} byte(s) past the end of the FORM at {end}"
            )
        yield chunk_id, payload, size
        cursor = payload + size + (size & 1)
    # Stopping short and overrunning are different defects and must not share a message -- this
    # function's docstring says so, and the single message below used to violate it by reporting a
    # NEGATIVE byte count on the overrun branch ("-1 byte(s) belong to no chunk"). An overrun here
    # can only come from the final chunk's word-alignment pad falling outside the declared FORM
    # size, because a chunk whose declared size runs past `end` was already refused above.
    if cursor > end:
        raise AssetError(
            f"the chunk walk ended at offset {cursor}, past the end of the FORM at {end}: the "
            f"final chunk's {cursor - end} pad byte(s) fall outside the declared FORM size"
        )
    if cursor != end:
        raise AssetError(
            f"the chunk walk ended at offset {cursor} but the FORM ends at {end}; "
            f"{end - cursor} byte(s) belong to no chunk"
        )


def _chunk_name(chunk_id: bytes) -> str:
    return chunk_id.decode("ascii", "replace")


def decode_body(
    body: bytes, row_bytes: int, height: int, compression: int
) -> tuple[bytes, int, list[RowCrossing]]:
    """Decode a BODY into exactly `row_bytes * height` bytes.

    Returns the pixels, the number of BODY bytes consumed, and every packet that had to be clamped
    at a row boundary. Raises `AssetError` naming the row when the stream runs out before the image
    is full, which is the one case no decoder can recover. See the module docstring for why the row
    is padded and why a crossing packet is clamped rather than refused.
    """
    expected = row_bytes * height
    if compression == 0:
        if len(body) < expected:
            raise AssetError(
                f"uncompressed BODY holds {len(body)} bytes; {height} rows of {row_bytes} bytes "
                f"need {expected}"
            )
        return body[:expected], expected, []
    if compression != 1:
        raise AssetError(f"no decoder for compression {compression}")

    # Bound the allocation BEFORE making it. `expected` comes from the BMHD, which is attacker- or
    # accident-controlled: a 4-byte header declaring 65535x65535 asks for 4.3 GB from a member that
    # may be under a kilobyte. Raising AssetError here turns that into the finding this module
    # exists to produce; allocating first turns it into a MemoryError that escapes `validate_image`
    # and aborts the whole build gate with a traceback naming no member.
    #
    # The ceiling is the format's own: a ByteRun1 repeat packet is 2 input bytes for at most 128
    # output bytes, so no BODY can expand more than 64x. That is a true upper bound, not a guess at
    # a plausible image size, so it can never refuse a member the decoder would have accepted.
    maximum_output = len(body) * 64
    if expected > maximum_output:
        raise AssetError(
            f"BMHD declares {height} rows of {row_bytes} byte(s) = {expected} pixels, which a "
            f"{len(body)}-byte ByteRun1 BODY cannot produce: the format's maximum expansion is "
            f"64x, so this BODY can yield at most {maximum_output} byte(s)"
        )

    output = bytearray(expected)
    crossings: list[RowCrossing] = []
    written = 0
    cursor = 0
    for row in range(height):
        row_end = written + row_bytes
        while written < row_end:
            if cursor >= len(body):
                raise AssetError(
                    f"BODY ends inside row {row} of {height}: the packet stream ran out with "
                    f"{row_end - written} of {row_bytes} byte(s) in that row still undecoded"
                )
            control = body[cursor]
            cursor += 1
            if control < 128:
                count = control + 1
                if cursor + count > len(body):
                    raise AssetError(
                        f"row {row}: a literal packet of {count} byte(s) at offset {cursor - 1} "
                        f"runs past the end of the {len(body)}-byte BODY"
                    )
                kept = min(count, row_end - written)
                output[written : written + kept] = body[cursor : cursor + kept]
                cursor += count
            else:
                if control == 128:
                    # The no-op packet the format reserves. It writes nothing.
                    continue
                count = 257 - control
                if cursor >= len(body):
                    raise AssetError(
                        f"row {row}: a repeat packet at offset {cursor - 1} has no value byte "
                        f"before the end of the {len(body)}-byte BODY"
                    )
                kept = min(count, row_end - written)
                output[written : written + kept] = bytes([body[cursor]]) * kept
                cursor += 1
            if count > kept:
                crossings.append(RowCrossing(row=row, count=count, discarded=count - kept))
            written += kept
    return bytes(output), cursor, crossings


def validate_image(data: bytes) -> tuple[list[AssetFinding], ImageSummary | None]:
    """Every finding about one member's bytes, plus what it turned out to be.

    Returns `(findings, None)` when the member could not be decoded far enough to describe.
    """
    findings: list[AssetFinding] = []

    if len(data) < 12 or data[0:4] != b"FORM" or data[8:12] != b"PBM ":
        findings.append(
            AssetFinding(
                ERROR,
                "iff-structure",
                f"not an IFF FORM PBM image: the first 12 bytes are {data[:12]!r}. Every one of "
                "the 1,045 images in the base pic.mpq begins 'FORM' + size + 'PBM '.",
            )
        )
        return findings, None

    (form_size,) = struct.unpack_from(">I", data, 4)
    form_end = 8 + form_size
    if form_end > len(data):
        findings.append(
            AssetFinding(
                ERROR,
                "iff-structure",
                f"the FORM declares {form_size} bytes, which needs a {form_end}-byte file; this "
                f"member is {len(data)} bytes",
            )
        )
        return findings, None
    if form_end < len(data):
        findings.append(
            AssetFinding(
                WARNING,
                "iff-structure",
                f"{len(data) - form_end} byte(s) follow the end of the FORM at {form_end}. No "
                "member of the base pic.mpq has any: all 1,045 FORM sizes end exactly at the end "
                "of the file.",
            )
        )

    # LAST occurrence wins, which is what `spikes/asset-viewer/src/pbm.rs` does -- its walk assigns
    # `body = Some(...)` on every match rather than only the first. A validator that reads the FIRST
    # of a duplicated chunk checks different bytes than the decoder renders, so a member could pass
    # this check and still fail to decode. Measured: reading the first, a FORM carrying a clean BODY
    # followed by a second BODY of out-of-range indices drew no error here while `PbmImage::decode`
    # refused it.
    #
    # Only the SINGLETON chunks are reported when duplicated. `CRNG` is a colour-cycling range and
    # a file carries a list of them, so a repeat is the format working normally. **Observed
    # 2026-09-18** over the 3,463 images extracted from the three profiles' `pic.mpq`: `CRNG` occurs
    # up to 16 times in one member and more than once in 3,079 of them, while `BMHD`, `CMAP`,
    # `BODY`, `DPPS` and `TINY` never occur twice in any member. An earlier draft of this check
    # warned on ANY duplicate and fired on 916 of vanilla's 1,044 members -- a validator rejecting
    # shipped content, which is the failure this module's docstring opens by warning about.
    chunks: dict[bytes, tuple[int, int]] = {}
    duplicates: list[bytes] = []
    try:
        for chunk_id, payload, size in iter_chunks(data, 12, form_end):
            if (
                chunk_id in chunks
                and chunk_id in SINGLETON_CHUNKS
                and chunk_id not in duplicates
            ):
                duplicates.append(chunk_id)
            chunks[chunk_id] = (payload, size)
    except AssetError as error:
        findings.append(AssetFinding(ERROR, "iff-structure", str(error)))
        return findings, None

    for chunk_id in duplicates:
        findings.append(
            AssetFinding(
                WARNING,
                "iff-structure",
                f"{chunk_id.decode('latin1')} appears more than once. The last one is what this "
                "check and the decoder both read, so the earlier copies are unreachable data. No "
                f"member of the base corpus carries two {chunk_id.decode('latin1')} chunks.",
            )
        )

    header = _chunk_bytes(data, chunks.get(b"BMHD"))
    if header is None:
        findings.append(AssetFinding(ERROR, "bmhd", "the image has no BMHD chunk"))
        return findings, None
    if len(header) < BMHD_LENGTH:
        findings.append(
            AssetFinding(
                ERROR,
                "bmhd",
                f"the BMHD chunk is {len(header)} bytes; the header is {BMHD_LENGTH}",
            )
        )
        return findings, None

    width, height = struct.unpack_from(">HH", header, 0)
    planes = header[8]
    masking = header[9]
    compression = header[10]
    (transparent,) = struct.unpack_from(">H", header, 12)

    fatal = False
    if width == 0 or height == 0:
        findings.append(
            AssetFinding(
                ERROR, "bmhd", f"the BMHD declares {width}x{height}; neither may be zero"
            )
        )
        fatal = True
    if planes != CHUNKY_PLANES:
        findings.append(
            AssetFinding(
                ERROR,
                "bmhd",
                f"the BMHD declares {planes} plane(s). This pipeline reads one byte per pixel "
                f"only, and all 1,045 images in the base pic.mpq declare {CHUNKY_PLANES}.",
            )
        )
        fatal = True
    if compression not in SUPPORTED_COMPRESSION:
        findings.append(
            AssetFinding(
                ERROR,
                "bmhd",
                f"the BMHD declares compression {compression}; this pipeline decodes "
                f"{' and '.join(str(value) for value in SUPPORTED_COMPRESSION)} "
                "(uncompressed and ByteRun1)",
            )
        )
        fatal = True
    if masking not in (0, 2):
        findings.append(
            AssetFinding(
                WARNING,
                "bmhd",
                f"the BMHD declares masking {masking}, which the decoder ignores. All 1,045 "
                "images in the base pic.mpq declare 0.",
            )
        )

    palette = _chunk_bytes(data, chunks.get(b"CMAP"))
    palette_entries = 0
    if palette is None:
        findings.append(
            AssetFinding(
                ERROR,
                "cmap",
                "the image has no CMAP chunk, so its pixel indices address nothing",
            )
        )
        fatal = True
    else:
        if len(palette) % 3 != 0:
            findings.append(
                AssetFinding(
                    ERROR,
                    "cmap",
                    f"the CMAP chunk is {len(palette)} bytes, which is not a whole number of RGB "
                    f"triples ({len(palette) % 3} byte(s) over)",
                )
            )
            fatal = True
        palette_entries = len(palette) // 3
        if palette_entries == 0:
            findings.append(
                AssetFinding(ERROR, "cmap", "the CMAP chunk holds no colours at all")
            )
            fatal = True
        elif planes == CHUNKY_PLANES and palette_entries != 1 << CHUNKY_PLANES:
            findings.append(
                AssetFinding(
                    WARNING,
                    "cmap",
                    f"the CMAP holds {palette_entries} colour(s) but the BMHD declares "
                    f"{planes} planes, which can address {1 << planes}. Every image in the base "
                    f"pic.mpq carries all {1 << CHUNKY_PLANES}.",
                )
            )
        if palette_entries and transparent >= palette_entries:
            findings.append(
                AssetFinding(
                    ERROR if masking == 2 else WARNING,
                    "bmhd",
                    f"the BMHD's transparent index {transparent} is not one of the CMAP's "
                    f"{palette_entries} colour(s)"
                    + (
                        ""
                        if masking == 2
                        else f"; masking is {masking} so the decoder does not use it"
                    ),
                )
            )

    body = _chunk_bytes(data, chunks.get(b"BODY"))
    if body is None:
        findings.append(AssetFinding(ERROR, "body", "the image has no BODY chunk"))
        fatal = True
    if fatal or body is None:
        return findings, None

    # The ILBM row-padding rule: a row occupies an even number of bytes. Measured, not assumed --
    # see the module docstring for the 88 odd-width members this explains.
    row_bytes = (width + 1) & ~1
    try:
        pixels, consumed, crossings = decode_body(body, row_bytes, height, compression)
    except AssetError as error:
        findings.append(AssetFinding(ERROR, "body", str(error)))
        return findings, None

    if crossings:
        first = crossings[0]
        findings.append(
            AssetFinding(
                WARNING,
                "body",
                f"{len(crossings)} ByteRun1 packet(s) cross a scanline boundary, first in row "
                f"{first.row} where a {first.count}-byte packet overruns by {first.discarded}. "
                f"{sum(crossing.discarded for crossing in crossings)} pixel(s) are discarded to "
                f"keep the {row_bytes}-byte rows aligned, which is what the engine's decoder "
                "does. One shipped member behaves this way -- GS5R3's PORTRAIT/decr5p00.lbm, "
                "which discards 1,353 -- so this is reported rather than refused.",
            )
        )

    # The silent acceptance is gated on the BODY size being EVEN, which is the condition the rule
    # was measured under: in all 65 base members with a trailing 0x00, the BODY chunk size is even,
    # because the encoder padded the chunk to an even size INSIDE the declared size. A trailing zero
    # on an ODD-sized BODY cannot be that pad, so it is genuinely unreachable data and is reported.
    # Accepting it at any parity would have made the code looser than the evidence behind it.
    leftover = body[consumed:]
    leftover_is_pad = leftover == b"\x00" and len(body) % 2 == 0
    if leftover != b"" and not leftover_is_pad:
        findings.append(
            AssetFinding(
                WARNING,
                "body",
                f"{len(leftover)} byte(s) of the BODY are left over after {height} rows of "
                f"{row_bytes} bytes decode. One trailing 0x00 is the chunk's even-size pad and "
                f"occurs in 65 base members; these bytes are {leftover[:8]!r}.",
            )
        )

    out_of_range: dict[int, int] = {}
    first: tuple[int, int, int] | None = None
    for row in range(height):
        base = row * row_bytes
        for column in range(width):
            index = pixels[base + column]
            if index >= palette_entries:
                out_of_range[index] = out_of_range.get(index, 0) + 1
                if first is None:
                    first = (column, row, index)
    if first is not None:
        listed = ", ".join(
            f"{index} ({count} pixel(s))" for index, count in sorted(out_of_range.items())
        )
        findings.append(
            AssetFinding(
                ERROR,
                "palette-index",
                f"{sum(out_of_range.values())} pixel(s) use a palette index the CMAP's "
                f"{palette_entries} colour(s) do not cover, first at column {first[0]} row "
                f"{first[1]} (index {first[2]}). Indices out of range: {listed}.",
            )
        )

    # Pixels the palette check REFUTED are not pixels it confirmed. Counting `width * height` here
    # would let a member with every pixel out of range report its full area as "confirmed to address
    # a colour", which is the exact inversion of what the coverage block is for.
    return findings, ImageSummary(
        width=width,
        height=height,
        planes=planes,
        compression=compression,
        palette_entries=palette_entries,
        pixels_checked=width * height - sum(out_of_range.values()),
    )


def _chunk_bytes(data: bytes, located: tuple[int, int] | None) -> bytes | None:
    if located is None:
        return None
    payload, size = located
    return data[payload : payload + size]
