#!/usr/bin/env python3
r"""Patch `lomse.exe` from declared patch sets, verifying every site before writing any.

A patch set is a TOML file under `tools/exe_patches/`. Each one names the exact binary it was
derived from, and lists instruction-sized edits by virtual address:

    [target]
    sha256 = "a505f399..."

    [[patch]]
    va = 0x512aa9
    old = "c1 fb 07"
    new = "c1 fb 06"
    what = "texture row stride: v >> 7 -> v >> 6"

and optionally scans, which assert how many times a byte pattern occurs in a VA range and that
every occurrence is covered by a patch in the same set:

    [[scan]]
    start = 0x512710
    end = 0x5179da
    regex = '\xc1[\xf8-\xff]\x07(?:\x81[\xe0-\xe7]|\x25)\x00\xfe\xff\xff'
    after = '\xc1[\xf8-\xff]\x06(?:\x81[\xe0-\xe7]|\x25)\x00\xfc\xff\xff'
    count = 18

A scan is what makes "the set patches every site" a checked claim rather than a remembered one: a
site list transcribed from a disassembly can drop one, and a binary with one 512-byte stride left
behind renders garbage along one edge of one triangle orientation, which reads as an art defect.

Nothing is written until every patch in every set has matched and every scan has passed. A patcher
that edits as it goes and raises on the fifth site has already handed back a half-patched binary,
and that binary once went in front of a person for a reading (feedback: verify every site, then
write).

    tools/exe_patch.py check  IN --set A.toml [--set B.toml ...]
    tools/exe_patch.py build  IN OUT --set A.toml [--set B.toml ...]
"""

from __future__ import annotations

import argparse
import hashlib
import re
import struct
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path


class PatchError(Exception):
    """A patch set that does not apply cleanly. Nothing has been written when this is raised."""


@dataclass(frozen=True)
class Section:
    name: str
    va: int
    vsize: int
    raw: int
    rsize: int


@dataclass(frozen=True)
class Patch:
    source: str
    va: int
    old: bytes
    new: bytes
    what: str


@dataclass(frozen=True)
class Scan:
    source: str
    start: int
    end: int
    regex: bytes
    count: int
    after: bytes | None


@dataclass(frozen=True)
class PatchSet:
    path: Path
    sha256: str
    patches: tuple[Patch, ...]
    scans: tuple[Scan, ...]


def sections(image: bytes) -> list[Section]:
    """The PE section table. Offsets are derived here, never copied from notes: this binary has
    four sections, and a note that described two once converted an .rdata VA with the .data
    formula and read a page of zeros."""
    if image[:2] != b"MZ":
        raise PatchError("not a PE image: no MZ header")
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    if image[pe:pe + 4] != b"PE\0\0":
        raise PatchError("not a PE image: no PE signature")
    count = struct.unpack_from("<H", image, pe + 6)[0]
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    image_base = struct.unpack_from("<I", image, pe + 24 + 28)[0]
    table = pe + 24 + optional_size
    out = []
    for i in range(count):
        entry = table + 40 * i
        name = image[entry:entry + 8].rstrip(b"\0").decode("latin-1")
        vsize, rva, rsize, raw = struct.unpack_from("<IIII", image, entry + 8)
        out.append(Section(name, image_base + rva, vsize, raw, rsize))
    return out


def va_to_offset(secs: list[Section], va: int, length: int = 1) -> int:
    """File offset of `va`, refusing anything that is not wholly inside one section's raw data."""
    for s in secs:
        if s.va <= va and va + length <= s.va + min(s.vsize, s.rsize):
            return s.raw + (va - s.va)
    raise PatchError(f"VA {va:#x}+{length} is not inside any section's file data")


def _hex(text: str) -> bytes:
    return bytes.fromhex(text.replace(" ", ""))


def _regex(text: str) -> bytes:
    """A TOML literal string like '\\xc1[\\xf8-\\xff]' as a bytes regex."""
    return text.encode("latin-1").decode("unicode_escape").encode("latin-1")


def load_set(path: Path) -> PatchSet:
    data = tomllib.loads(path.read_text())
    try:
        sha = data["target"]["sha256"].lower()
    except KeyError:
        raise PatchError(f"{path.name}: no [target] sha256") from None
    patches = []
    for i, p in enumerate(data.get("patch", [])):
        old, new = _hex(p["old"]), _hex(p["new"])
        where = f"{path.name} patch {i} at {p['va']:#x}"
        if len(old) != len(new):
            raise PatchError(f"{where}: old is {len(old)} bytes, new is {len(new)}")
        if old == new:
            raise PatchError(f"{where}: old and new are identical")
        if not p.get("what"):
            raise PatchError(f"{where}: no 'what' -- every edit says what it is")
        patches.append(Patch(path.name, p["va"], old, new, p["what"]))
    scans = []
    for s in data.get("scan", []):
        after = _regex(s["after"]) if "after" in s else None
        scans.append(Scan(path.name, s["start"], s["end"], _regex(s["regex"]), s["count"], after))
    if not patches:
        raise PatchError(f"{path.name}: no patches")
    return PatchSet(path, sha, tuple(patches), tuple(scans))


def plan(image: bytes, sets: list[PatchSet]) -> list[tuple[int, Patch]]:
    """Every (offset, patch) the sets would apply to `image`, after checking all of them.

    Raises on the first class of problem it finds, but only after collecting every failing site
    in that class, so a report names all the drifted sites at once rather than one per rerun."""
    digest = hashlib.sha256(image).hexdigest()
    for s in sets:
        if s.sha256 != digest:
            raise PatchError(
                f"{s.path.name} targets {s.sha256[:16]}, input is {digest[:16]}. "
                "Patch sets apply to the pristine binary only; they do not stack on a patched one."
            )
    secs = sections(image)
    applied: list[tuple[int, Patch]] = []
    failures = []
    for s in sets:
        for p in s.patches:
            try:
                off = va_to_offset(secs, p.va, len(p.old))
            except PatchError as err:
                failures.append(f"{p.source} {p.va:#x}: {err}")
                continue
            have = image[off:off + len(p.old)]
            if have != p.old:
                failures.append(
                    f"{p.source} {p.va:#x} ({p.what}): expected {p.old.hex(' ')}, found {have.hex(' ')}"
                )
            applied.append((off, p))
    if failures:
        raise PatchError("sites do not match:\n  " + "\n  ".join(failures))

    spans = sorted((off, off + len(p.old), p) for off, p in applied)
    for (a0, a1, pa), (b0, b1, pb) in zip(spans, spans[1:]):
        if b0 < a1:
            raise PatchError(f"overlapping edits: {pa.source} {pa.va:#x} and {pb.source} {pb.va:#x}")

    for s in sets:
        spans_by_va = [(p.va, p.va + len(p.old)) for p in s.patches]
        for scan in s.scans:
            start = va_to_offset(secs, scan.start)
            end = va_to_offset(secs, scan.end - 1) + 1
            hits = [scan.start + m.start() for m in re.finditer(scan.regex, image[start:end], re.S)]
            if len(hits) != scan.count:
                raise PatchError(
                    f"{scan.source}: scan {scan.start:#x}-{scan.end:#x} found {len(hits)} sites, "
                    f"declared {scan.count}"
                )
            missed = [h for h in hits if not any(a <= h < b for a, b in spans_by_va)]
            if missed:
                raise PatchError(
                    f"{scan.source}: scan sites no patch covers: {', '.join(hex(m) for m in missed)}"
                )

    # Coverage by start address is not enough: a site whose first instruction is patched and whose
    # second is not still starts inside a patch. The pattern must be gone from the result.
    out = bytearray(image)
    for off, p in applied:
        out[off:off + len(p.new)] = p.new
    for s in sets:
        for scan in s.scans:
            start = va_to_offset(secs, scan.start)
            end = va_to_offset(secs, scan.end - 1) + 1
            left = [scan.start + m.start() for m in re.finditer(scan.regex, bytes(out[start:end]), re.S)]
            if left:
                raise PatchError(
                    f"{scan.source}: pattern still present after patching at "
                    f"{', '.join(hex(m) for m in left)}"
                )
            # The old pattern vanishing is not the new one appearing: patching one instruction of
            # a two-instruction site breaks the match too. `after` is the finished form of a site.
            if scan.after is not None:
                done = len(re.findall(scan.after, bytes(out[start:end]), re.S))
                if done != scan.count:
                    raise PatchError(
                        f"{scan.source}: {done} of {scan.count} sites are in their patched form"
                    )
    return applied


def apply(image: bytes, sets: list[PatchSet]) -> bytes:
    out = bytearray(image)
    for off, p in plan(image, sets):
        out[off:off + len(p.new)] = p.new
    return bytes(out)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("check", "build"):
        c = sub.add_parser(name)
        c.add_argument("input", type=Path)
        if name == "build":
            c.add_argument("output", type=Path)
        c.add_argument("--set", dest="sets", type=Path, action="append", required=True)
    args = ap.parse_args(argv)
    try:
        image = args.input.read_bytes()
        sets = [load_set(p) for p in args.sets]
        if args.cmd == "check":
            n = len(plan(image, sets))
            print(f"ok: {n} sites match across {len(sets)} set(s)")
            return 0
        if args.output.exists() and args.output.resolve() == args.input.resolve():
            raise PatchError("refusing to write over the input; build a copy")
        out = apply(image, sets)
        args.output.write_bytes(out)
        print(f"wrote {args.output} sha256 {hashlib.sha256(out).hexdigest()}")
        for s in sets:
            print(f"  {s.path.name}: {len(s.patches)} edits")
        return 0
    except PatchError as err:
        print(f"exe_patch: {err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
