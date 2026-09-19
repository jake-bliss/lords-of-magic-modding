#!/usr/bin/env python3
"""Mutate the `LS_SPR_` constants in `save.rs` one at a time and report which survive.

A mutation sweep that lives outside the repository is a claim nobody else can check. This is the
harness behind the "N of M caught" number in `docs/save-format.md`; run it and the number comes
back, or it does not.

Each constant is moved **in both directions**. A one-directional sweep misses a gate moved the
other way -- the `LS_PLR_` work recorded exactly that failure -- and a survivor is not
automatically a defect: a constant that guards a branch the build cannot reach has no test that
can move it, and those are expected to survive and are listed as such.

    PYTHONDONTWRITEBYTECODE=1 python3 tools/mutate_save_constants.py
    PYTHONDONTWRITEBYTECODE=1 python3 tools/mutate_save_constants.py --quick   # gates only

Restores the file on exit, including on Ctrl-C.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
SOURCE = REPO / "spikes" / "asset-viewer" / "src" / "save.rs"
CRATE = REPO / "spikes" / "asset-viewer"

# Constants whose branch the build at 0x0055B1B0 == 111 cannot reach, so no test can move them.
# Listed rather than skipped: an expected survivor that stops being expected is a finding.
EXPECTED_SURVIVORS = {"SPR_NESTED_LAST_WORD_MIN"}

# Non-gate mutations: shapes, widths and signedness, each a claim about the instruction stream.
STRUCTURAL = [
    ("    (i32::MAX, 0x4C),", "    (i32::MAX, 0x50),", "slot blob width 0x4C -> 0x50"),
    ("    (0x67, 0x48),", "    (0x66, 0x48),", "slot ladder rung 0x67 -> 0x66"),
    ("    (0x67, 0x48),", "    (0x68, 0x48),", "slot ladder rung 0x67 -> 0x68"),
    ("        unknown_30: cursor.u32()?,", "        unknown_30: 0,", "base block 6 dwords -> 5"),
    (
        "                let entries = spr_signed_count16(cursor.u16()?);",
        "                let entries = spr_signed_count(cursor.u32()?) as u16;",
        "class 1 array count u16 -> u32",
    ),
    ("                    cursor.take(36)?;", "                    cursor.take(32)?;", "class 3 tail item 36 -> 32"),
    ("            cursor.take(0x5C)?;", "            cursor.take(0x58)?;", "class 9 blob 92 -> 88"),
    (
        "fn spr_signed_count(raw: u32) -> u32 {\n    (raw as i32).max(0) as u32\n}",
        "fn spr_signed_count(raw: u32) -> u32 {\n    raw\n}",
        "guarded counts modelled unsigned",
    ),
    (
        "fn spr_signed_count16(raw: u16) -> u16 {\n    (raw as i16).max(0) as u16\n}",
        "fn spr_signed_count16(raw: u16) -> u16 {\n    (raw as i32).max(0) as u16\n}",
        "16-bit count clamped at the wrong width",
    ),
    (
        "        let live_record_count = spr_signed_count(record_count);",
        "        let live_record_count = record_count;",
        "top-level record count modelled unsigned",
    ),
    (
        "    let items_a = spr_unguarded_count(cursor.u32()?);",
        "    let items_a = spr_signed_count(cursor.u32()?);",
        "unguarded list count treated as guarded",
    ),
    # A compensating pair: the aggregate size is unchanged, so neither the byte account nor the
    # round trip can see it. It is here to be *demonstrated* as a survivor, not caught -- which is
    # what the round-trip caveat in the docs is about.
    (
        "    cursor.take(12)?;\n    cursor.take(88)?;",
        "    cursor.take(16)?;\n    cursor.take(84)?;",
        "class 0 fixed reads 12+88 -> 16+84 (COMPENSATING, expected to survive)",
    ),
]

COMPENSATING = "COMPENSATING"


def gate_mutations(text: str) -> list[tuple[str, str, str]]:
    out = []
    for line in re.findall(r"^const (SPR_\w+): i32 = (0x[0-9A-Fa-f]+);$", text, re.MULTILINE):
        name, value = line
        original = f"const {name}: i32 = {value};"
        for delta in (1, -1):
            moved = f"const {name}: i32 = {hex(int(value, 16) + delta)};"
            out.append((original, moved, f"{name} {delta:+d}"))
    return out


def run_tests() -> bool:
    result = subprocess.run(
        ["cargo", "test", "--lib", "-q"], cwd=CRATE, capture_output=True, text=True
    )
    return "test result: ok" in result.stdout and "FAILED" not in result.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--quick", action="store_true", help="version gates only")
    args = parser.parse_args()

    original = SOURCE.read_text()
    mutations = gate_mutations(original)
    if not args.quick:
        mutations += STRUCTURAL

    caught, survivors = 0, []
    try:
        for old, new, label in mutations:
            if original.count(old) != 1:
                print(f"SKIP   {label}: pattern is not unique in the source")
                survivors.append((label, "pattern not found"))
                continue
            SOURCE.write_text(original.replace(old, new, 1))
            if run_tests():
                survivors.append((label, "tests still pass"))
                print(f"SURVIVED  {label}")
            else:
                caught += 1
                print(f"caught    {label}")
    finally:
        SOURCE.write_text(original)

    total = len(mutations)
    print(f"\n{caught} of {total} mutations caught")
    unexpected = [
        (label, why)
        for label, why in survivors
        if not any(name in label for name in EXPECTED_SURVIVORS) and COMPENSATING not in label
    ]
    for label, why in survivors:
        expected = any(name in label for name in EXPECTED_SURVIVORS) or COMPENSATING in label
        print(f"  survivor{' (expected)' if expected else ''}: {label} -- {why}")
    return 1 if unexpected else 0


if __name__ == "__main__":
    sys.exit(main())
