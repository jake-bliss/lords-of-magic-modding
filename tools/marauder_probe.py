#!/usr/bin/env python3
"""The marauder-probe edit to `gs\\hotkey.gs`, and the offline checks that it is the only edit.

    python3 tools/marauder_probe.py apply  SHIPPED_HOTKEY_GS OUT
    python3 tools/marauder_probe.py verify SHIPPED_HOTKEY_GS EDITED_HOTKEY_GS
    python3 tools/marauder_probe.py keys   SHIPPED_HOTKEY_GS
    python3 tools/marauder_probe.py wmp    LOMSE_EXE [PLACEDNG_GS]

The probe asks whether a human can be bound to the wandering-monster player (the Marauders). It is
one build on top of cheat-keys-true: the same `/cheat_keys` flip, plus four hotkeys inserted at one
verified site. `docs/marauder-probe.md` is the run sheet; this module is what makes the build
reproducible and checkable without the engine.

Every rule here is derived from the shipped member rather than assumed:

- The keys the probe binds are refused if the shipped member already binds them, in either
  namespace the file uses (`ASCII_VAL` and `VK_VAL` are separate slots -- observed in gameplay
  2026-09-19, docs/cheat-keys-ladder.md). The parser is checked against the member itself: it must
  account for every `addhotkey` in the file, or it refuses to say which keys are free.
- The insertion anchor and the flag token must each occur exactly once.
- `WANDERING_MONSTER_PLAYER` is read from the engine's own constant table in `lomse.exe`, and
  cross-checked against the one place the corpus spells the slot as a literal
  (`0 15{pop 1 add}enumplayerarmies` in `gs\\placedng.gs`'s monster generator).

`gs\\hotkey.gs` is a single line of printable ASCII with no line terminator. The inserted block
follows suit -- no newline, no non-ASCII -- so the member keeps the shape the engine already reads.
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gs_syntax  # noqa: E402

HOTKEY_MEMBER = "gs\\hotkey.gs"

FLAG_BEFORE = b"/cheat_keys false def"
FLAG_AFTER = b"/cheat_keys true def"

# The probe block goes immediately before this binding. It is the first binding after the
# cheat_keys tier closes and still inside hotkey.gs's own `20 dict begin ... end`, so the probe keys
# sit in the same scope as every other in-game key. Ctrl+V is the version box; nothing about it
# depends on what precedes it.
ANCHOR = b'ASCII_VAL 22{storydict begin"Lords of Magic v3.01 December 3, 1998"open_messagebox_dialog end}addhotkey'

# The four probe keys. Upper case only, so each needs Shift and is hard to press by accident.
# Every body is gated by `getmultiplayerflag not`, like the cheat tier. No body defines a name:
# a hotkey runs under whatever dictionary stack is current when the key is pressed, and a `def`
# there would land in a dictionary nobody chose.
#
# Stack effects, operator by operator, are traced in mods/marauder-probe/README.md.
READOUT = (
    'ASCII_VAL"J"0 get{getmultiplayerflag not{'
    '{"t="currentturn" cu="currentuser" cp="currentplayer" wmp="WANDERING_MONSTER_PLAYER'
    '" tc="thiscomputer'
    '"  P0 f="0 getplayerfaith" ai="0 getplayeraistatus" cc="0 getcontrollingcomputer'
    '"  MAR f="WANDERING_MONSTER_PLAYER getplayerfaith'
    '" ai="WANDERING_MONSTER_PLAYER getplayeraistatus'
    '" cc="WANDERING_MONSTER_PLAYER getcontrollingcomputer'
    '"  nMAR="0 WANDERING_MONSTER_PLAYER{pop 1 add}enumplayerarmies'
    '"  lord="currentuser getleaderlocation pop'
    '}build_statement '
    'incombat{center_balloonhelp_quick}{storydict begin open_messagebox_dialog end}ifelse'
    '}if}addhotkey'
)
SPAWN = (
    'ASCII_VAL"N"0 get{getmultiplayerflag not incombat not and getzoommode WORLD_SCREEN eq not and{'
    '320 190 getlocationatscreenxy dup -1 gt{'
    'UNITTYPELAND findemptylocation dup -1 gt{'
    'dup xy_to_x_y unittypedict /decr1 get WANDERING_MONSTER_PLAYER -1 addunit '
    'processgamemessages xy_to_x_y armyat dup -1 gt{centeronarmy}{pop}ifelse rendermap'
    '}{pop}ifelse'
    '}{pop}ifelse'
    '}if}addhotkey'
)
TAKEOVER = (
    'ASCII_VAL"U"0 get{getmultiplayerflag not incombat not and{'
    'WANDERING_MONSTER_PLAYER 0 setplayeraistatus '
    '0 1 setplayeraistatus '
    'WANDERING_MONSTER_PLAYER thiscomputer setcontrollingcomputer '
    'WANDERING_MONSTER_PLAYER setuserforplayer rendermap'
    '}if}addhotkey'
)
HANDBACK = (
    'ASCII_VAL"H"0 get{getmultiplayerflag not incombat not and{'
    '0 0 setplayeraistatus '
    'WANDERING_MONSTER_PLAYER 1 setplayeraistatus '
    '0 setuserforplayer rendermap'
    '}if}addhotkey'
)

PROBE_KEYS = {"J": READOUT, "N": SPAWN, "U": TAKEOVER, "H": HANDBACK}
PROBE_BLOCK = (" ".join(PROBE_KEYS.values()) + " ").encode("ascii")


class ProbeError(Exception):
    """Raised when an input does not have the shape the probe was verified against."""


# --------------------------------------------------------------------------------------------
# Hotkey bindings
# --------------------------------------------------------------------------------------------

def _code(token_list: list[str], index: int) -> tuple[int, int]:
    """The key code starting at token_list[index], and how many tokens it spans.

    Two spellings occur: a decimal literal (`90`), and a one-character string indexed by
    `0 get` (`"Z"0 get`).
    """
    token = token_list[index]
    if token.isdigit():
        return int(token), 1
    if len(token) == 3 and token[0] == token[2] == '"':
        if token_list[index + 1: index + 3] != ["0", "get"]:
            raise ProbeError(f"string key code {token} is not followed by `0 get`")
        return ord(token[1]), 3
    raise ProbeError(f"unrecognised key code token {token!r}")


def bound_hotkeys(source: bytes) -> dict[str, set[int]]:
    """Every key code `source` binds, by namespace (`ASCII_VAL` / `VK_VAL`).

    Covers the three binding shapes gs\\hotkey.gs uses:

        ASCII_VAL 90{...}addhotkey
        ASCII_VAL"Z"0 get{...}addhotkey
        {...}dup ASCII_VAL exch 122 exch addhotkey addhotkey      (one body, second key)

    Bindings inside procedures that are defined but never called in the file (`test_hotkeys`,
    `spell_hotkeys`) are counted too. That is deliberately conservative: a key any code path could
    bind is not offered as free.

    Refuses unless every `addhotkey` token in the source is matched by exactly one key-code site,
    so a binding shape this parser does not know cannot hide a collision.
    """
    token_list = gs_syntax.tokens(source.decode("ascii"))
    bound: dict[str, set[int]] = {"ASCII_VAL": set(), "VK_VAL": set()}
    sites = 0
    index = 0
    while index < len(token_list):
        token = token_list[index]
        if token in bound:
            following = index + 1
            if token_list[following] == "exch":
                following += 1
            code, _span = _code(token_list, following)
            bound[token].add(code)
            sites += 1
        index += 1
    registrations = token_list.count("addhotkey")
    if sites != registrations:
        raise ProbeError(
            f"{sites} key-code sites but {registrations} addhotkey calls; a binding shape is "
            "unaccounted for, so no key can be declared free")
    return bound


def probe_codes() -> set[int]:
    return {ord(key) for key in PROBE_KEYS}


# --------------------------------------------------------------------------------------------
# The edit
# --------------------------------------------------------------------------------------------

def _exactly_once(haystack: bytes, needle: bytes, label: str) -> int:
    count = haystack.count(needle)
    if count != 1:
        raise ProbeError(f"expected exactly one {label} ({needle[:60]!r}...), found {count}")
    return haystack.index(needle)


def check_shipped(shipped: bytes) -> None:
    """Every precondition the edit relies on, stated against the shipped member."""
    try:
        shipped.decode("ascii")
    except UnicodeDecodeError as error:
        raise ProbeError(f"the shipped member is not ASCII: {error}") from error
    if b"\r" in shipped or b"\n" in shipped:
        raise ProbeError("the shipped member has line terminators; the probe was verified against "
                         "a single-line member")
    _exactly_once(shipped, FLAG_BEFORE, "cheat_keys flag")
    _exactly_once(shipped, ANCHOR, "insertion anchor")
    if PROBE_BLOCK in shipped:
        raise ProbeError("the probe block is already present")
    collisions = probe_codes() & bound_hotkeys(shipped)["ASCII_VAL"]
    if collisions:
        names = ", ".join(sorted(chr(code) for code in collisions))
        raise ProbeError(f"probe key(s) already bound in the shipped member: {names}")


def apply(shipped: bytes) -> bytes:
    """The shipped member with the flag flipped and the probe block inserted before the anchor."""
    check_shipped(shipped)
    flipped = shipped.replace(FLAG_BEFORE, FLAG_AFTER)
    anchor = _exactly_once(flipped, ANCHOR, "insertion anchor")
    return flipped[:anchor] + PROBE_BLOCK + flipped[anchor:]


def verify(shipped: bytes, edited: bytes) -> list[str]:
    """Assert `edited` is exactly `apply(shipped)`, and describe the two edits."""
    expected = apply(shipped)
    if edited != expected:
        shortest = min(len(edited), len(expected))
        first = next((i for i in range(shortest) if edited[i] != expected[i]), shortest)
        raise ProbeError(
            f"the edited member is not the shipped member plus the two probe edits; first "
            f"difference at byte {first} (edited {len(edited)} bytes, expected {len(expected)})")
    flag_at = shipped.index(FLAG_BEFORE)
    anchor_at = shipped.index(ANCHOR)
    bound_after = bound_hotkeys(edited)["ASCII_VAL"]
    missing = probe_codes() - bound_after
    if missing:
        raise ProbeError(f"probe keys not bound after the edit: {sorted(missing)}")
    return [
        f"flag   at shipped byte {flag_at}: {FLAG_BEFORE.decode()!r} -> {FLAG_AFTER.decode()!r}",
        f"insert at shipped byte {anchor_at} (before the Ctrl+V binding): "
        f"{len(PROBE_BLOCK)} bytes, keys {', '.join(PROBE_KEYS)}",
        f"shipped {len(shipped)} bytes, edited {len(edited)} bytes, delta {len(edited) - len(shipped)} "
        f"= {len(FLAG_AFTER) - len(FLAG_BEFORE)} + {len(PROBE_BLOCK)}",
        "nothing else differs (edited == apply(shipped), byte for byte)",
    ]


# --------------------------------------------------------------------------------------------
# WANDERING_MONSTER_PLAYER, from the engine
# --------------------------------------------------------------------------------------------

def _sections(exe: bytes) -> tuple[int, list[tuple[int, int, int]]]:
    if exe[:2] != b"MZ":
        raise ProbeError("not a PE executable")
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    if exe[pe:pe + 4] != b"PE\0\0":
        raise ProbeError("no PE header")
    count = struct.unpack_from("<H", exe, pe + 6)[0]
    optional = struct.unpack_from("<H", exe, pe + 20)[0]
    image_base = struct.unpack_from("<I", exe, pe + 24 + 28)[0]
    table = pe + 24 + optional
    sections = []
    for i in range(count):
        _name, _vsize, va, raw_size, raw = struct.unpack_from("<8sIIII", exe, table + 40 * i)
        sections.append((va, raw, raw_size))
    return image_base, sections


def engine_constant(exe: bytes, name: str) -> int:
    """The value `lomse.exe` registers for a GameScript constant.

    The engine keeps its constants as a table of (name pointer, u32 value) pairs; the entry for
    `name` is found by the one pointer to its NUL-terminated string. Refuses unless there is exactly
    one such string and exactly one pointer to it.
    """
    image_base, sections = _sections(exe)

    def file_to_va(offset: int) -> int | None:
        for va, raw, raw_size in sections:
            if raw <= offset < raw + raw_size:
                return image_base + va + offset - raw
        return None

    needle = b"\0" + name.encode("ascii") + b"\0"
    if exe.count(needle) != 1:
        raise ProbeError(f"expected one NUL-delimited {name!r} string, found {exe.count(needle)}")
    string_va = file_to_va(exe.index(needle) + 1)
    if string_va is None:
        raise ProbeError(f"{name!r} string is outside every section")
    pointer = struct.pack("<I", string_va)
    sites = []
    at = exe.find(pointer)
    while at != -1:
        sites.append(at)
        at = exe.find(pointer, at + 1)
    if len(sites) != 1:
        raise ProbeError(f"expected one pointer to {name!r}, found {len(sites)}")
    return struct.unpack_from("<I", exe, sites[0] + 4)[0]


# The one place the vanilla corpus spells the marauder slot as a number rather than by name: the
# monster generator's army count in gs\placedng.gs.
PLACEDNG_LITERAL = b"0 15{pop 1 add}enumplayerarmies"


# --------------------------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------------------------

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p_apply = sub.add_parser("apply")
    p_apply.add_argument("shipped", type=Path)
    p_apply.add_argument("out", type=Path)
    p_verify = sub.add_parser("verify")
    p_verify.add_argument("shipped", type=Path)
    p_verify.add_argument("edited", type=Path)
    p_keys = sub.add_parser("keys")
    p_keys.add_argument("shipped", type=Path)
    p_wmp = sub.add_parser("wmp")
    p_wmp.add_argument("exe", type=Path)
    p_wmp.add_argument("placedng", type=Path, nargs="?")
    args = parser.parse_args(argv)

    try:
        if args.command == "apply":
            args.out.write_bytes(apply(args.shipped.read_bytes()))
            print(f"wrote {args.out}")
        elif args.command == "verify":
            for line in verify(args.shipped.read_bytes(), args.edited.read_bytes()):
                print(line)
        elif args.command == "keys":
            bound = bound_hotkeys(args.shipped.read_bytes())
            for namespace, codes in bound.items():
                print(f"{namespace} bound: {' '.join(str(c) for c in sorted(codes))}")
            for key in PROBE_KEYS:
                state = "BOUND" if ord(key) in bound["ASCII_VAL"] else "free"
                print(f"probe key {key} (ASCII {ord(key)}): {state}")
            if probe_codes() & bound["ASCII_VAL"]:
                return 1
        elif args.command == "wmp":
            value = engine_constant(args.exe.read_bytes(), "WANDERING_MONSTER_PLAYER")
            print(f"WANDERING_MONSTER_PLAYER = {value} (lomse.exe constant table)")
            if args.placedng is not None:
                literal = args.placedng.read_bytes().count(PLACEDNG_LITERAL)
                print(f"placedng.gs literal {PLACEDNG_LITERAL.decode()!r}: {literal} occurrence(s)")
                if literal != 1 or value != 15:
                    print("the engine constant and the corpus literal do not agree")
                    return 1
    except ProbeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
