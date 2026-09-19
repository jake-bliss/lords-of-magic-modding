#!/usr/bin/env python3
"""Mutate the `LS_SPR_` constants in `save.rs` one at a time and report which survive.

A mutation sweep that lives outside the repository is a claim nobody else can check. This is the
harness behind the "N of M caught" number in `docs/save-format.md`.

Three things this harness does that the obvious version does not, each because the obvious version
reports a *better* number when it is broken:

1.  **It verifies its baseline before it mutates anything**, and aborts if the suite is not green.
    Inferring "caught" from the absence of `test result: ok` in stdout means a suite that cannot
    build, cannot take the target lock, or fails for an unrelated reason marks **every** mutant
    caught and exits 0. The failure mode is indistinguishable from total success. A sister branch
    lost three commits to a red baseline making one survivor look killed; this polarity is worse.
    Success is `returncode == 0`, never a substring.

2.  **Its expected-survivor list is matched exactly, never by substring.** A new gate whose name
    contains an existing expected one -- `SPR_NESTED_LAST_WORD_MIN_EXTRA` -- would otherwise be
    excused silently, and an expected-survivor list that can grow on its own is a harness that
    stops testing one mutant at a time.

3.  **A mutant that does not compile is counted separately from one the tests killed.** Both are
    "not survived", but only the second is evidence that a test can see the constant.

    PYTHONDONTWRITEBYTECODE=1 python3 tools/mutate_save_constants.py
    PYTHONDONTWRITEBYTECODE=1 python3 tools/mutate_save_constants.py --quick   # gates only

Restores the file on exit, including on Ctrl-C, and re-checks the baseline afterwards so a run
that died mid-mutation cannot leave a poisoned tree behind unremarked.
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

# Exact mutation ids that no test can kill, each with the reason it cannot be killed. Matched by
# equality against the observed survivor set, and the run fails if the two sets differ in EITHER
# direction: an unexpected survivor is a coverage gap, and an expected survivor that stopped
# surviving means the exemption is now excusing nothing and should go.
EXPECTED_SURVIVORS = {
    "SPR_NESTED_LAST_WORD_MIN +1": (
        "guards a branch behind a second test against the build constant at 0x0055B1B0, "
        "which is 111 and decides it at compile time"
    ),
    "SPR_NESTED_LAST_WORD_MIN -1": ("same branch, other direction"),
    "class 0 fixed reads 12+88 -> 16+84": (
        "a COMPENSATING pair: the aggregate size is unchanged, so neither the byte account nor "
        "the round trip can see it. Present to demonstrate that blind spot, not to be caught"
    ),
}

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
    # The mirror of the line above. Its absence was a real gap: the 32-bit helper is the one this
    # whole section is built around, and a review found `(raw as i16)` passing the entire suite.
    (
        "fn spr_signed_count(raw: u32) -> u32 {\n    (raw as i32).max(0) as u32\n}",
        "fn spr_signed_count(raw: u32) -> u32 {\n    (raw as i16).max(0) as u32\n}",
        "32-bit count clamped at the wrong width",
    ),
    (
        "fn spr_signed_count(raw: u32) -> u32 {\n    (raw as i32).max(0) as u32\n}",
        "fn spr_signed_count(raw: u32) -> u32 {\n    (raw as i32).clamp(0, i16::MAX as i32) as u32\n}",
        "guarded count saturates instead of clamping at zero only",
    ),
    (
        "        let live_record_count = spr_signed_count(record_count);",
        "        let live_record_count = record_count;",
        "top-level record count modelled unsigned",
    ),
    # One per unguarded callsite. A single shared mutation would let two of the three go untested
    # while the sweep stayed green -- which is exactly what a review found.
    (
        "    let items_a = spr_unguarded_count(cursor.u32()?);",
        "    let items_a = spr_signed_count(cursor.u32()?);",
        "unguarded items_a treated as guarded",
    ),
    (
        "        let items_b = spr_unguarded_count(cursor.u32()?);",
        "        let items_b = spr_signed_count(cursor.u32()?);",
        "unguarded items_b treated as guarded",
    ),
    (
        "                let items = spr_unguarded_count(cursor.u32()?);",
        "                let items = spr_signed_count(cursor.u32()?);",
        "unguarded nested group items treated as guarded",
    ),
    (
        "    cursor.take(12)?;\n    cursor.take(88)?;",
        "    cursor.take(16)?;\n    cursor.take(84)?;",
        "class 0 fixed reads 12+88 -> 16+84",
    ),
]

SURVIVED, KILLED, UNCOMPILABLE, SKIPPED = "survived", "killed", "uncompilable", "skipped"


def gate_mutations(text: str) -> list[tuple[str, str, str]]:
    out = []
    for name, value in re.findall(
        r"^const (SPR_\w+): i32 = (0x[0-9A-Fa-f]+);$", text, re.MULTILINE
    ):
        original = f"const {name}: i32 = {value};"
        for delta in (1, -1):
            moved = f"const {name}: i32 = {hex(int(value, 16) + delta)};"
            out.append((original, moved, f"{name} {delta:+d}"))
    return out


def run_suite() -> subprocess.CompletedProcess:
    return subprocess.run(
        ["cargo", "test", "--lib", "-q"], cwd=CRATE, capture_output=True, text=True
    )


def classify(result: subprocess.CompletedProcess) -> str:
    if result.returncode == 0:
        return SURVIVED
    combined = result.stdout + result.stderr
    if "could not compile" in combined or re.search(r"^error\[E", combined, re.MULTILINE):
        return UNCOMPILABLE
    return KILLED


def check_baseline(when: str) -> subprocess.CompletedProcess:
    print(f"baseline ({when}): running the suite unmodified ...")
    result = run_suite()
    summary = " / ".join(
        line.strip() for line in result.stdout.splitlines() if line.startswith("test result:")
    )
    if result.returncode != 0:
        print(f"BASELINE IS NOT GREEN (exit {result.returncode}). Nothing was measured.")
        print("--- stderr tail ---")
        print("\n".join(result.stderr.splitlines()[-30:]))
        print("--- stdout tail ---")
        print("\n".join(result.stdout.splitlines()[-30:]))
        sys.exit(2)
    print(f"baseline ({when}): GREEN, exit 0 -- {summary or 'no test-result lines'}")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--quick", action="store_true", help="version gates only")
    args = parser.parse_args()

    original = SOURCE.read_text()
    mutations = gate_mutations(original)
    if not args.quick:
        mutations += STRUCTURAL

    labels = [label for _, _, label in mutations]
    duplicates = {label for label in labels if labels.count(label) > 1}
    if duplicates:
        print(f"mutation ids are not unique, so results cannot be attributed: {duplicates}")
        return 2

    # Nothing is measured until this passes.
    check_baseline("before")

    outcomes: dict[str, str] = {}
    try:
        for old, new, label in mutations:
            if original.count(old) != 1:
                # **Not a survivor.** Folding a skip into the survivor list lets an exempt
                # mutation whose pattern stopped matching be absorbed as "expected" -- the sweep
                # quietly stops running it while the headline number holds. A skip is its own
                # outcome and always fails the run.
                outcomes[label] = SKIPPED
                print(f"SKIPPED   {label}   (pattern is not unique in the source: NOT MEASURED)")
                continue
            SOURCE.write_text(original.replace(old, new, 1))
            outcome = classify(run_suite())
            outcomes[label] = outcome
            print(
                {
                    SURVIVED: f"SURVIVED  {label}",
                    KILLED: f"killed    {label}",
                    UNCOMPILABLE: f"no-build  {label}   (not evidence a test can see it)",
                }[outcome]
            )
    finally:
        SOURCE.write_text(original)

    # A run that died mid-mutation must not leave a poisoned tree, and the next reader must not
    # have to take that on trust.
    check_baseline("after restore")

    survivors = {label for label, outcome in outcomes.items() if outcome == SURVIVED}
    skipped = {label for label, outcome in outcomes.items() if outcome == SKIPPED}
    killed = sum(1 for outcome in outcomes.values() if outcome == KILLED)
    uncompilable = sum(1 for outcome in outcomes.values() if outcome == UNCOMPILABLE)

    print(f"\n{killed} of {len(mutations)} mutations killed by the tests")
    if uncompilable:
        print(f"{uncompilable} did not compile -- counted separately, they prove nothing")
    if skipped:
        print(f"{len(skipped)} were NOT MEASURED because their pattern no longer matches")
    print(f"{len(survivors)} survived")

    expected = set(EXPECTED_SURVIVORS)
    for label in sorted(survivors):
        reason = EXPECTED_SURVIVORS.get(label)
        print(f"  survivor: {label}" + (f"  -- expected: {reason}" if reason else "  -- UNEXPECTED"))

    unexpected = survivors - expected
    vanished = expected - survivors
    if skipped:
        print(
            f"\nFAIL: {len(skipped)} mutation(s) were not measured at all: {sorted(skipped)}\n"
            "  Their patterns no longer match the source. Update them; a sweep that silently\n"
            "  drops a mutant reports a score it did not earn."
        )
    if unexpected:
        print(f"\nFAIL: {len(unexpected)} unexpected survivor(s): {sorted(unexpected)}")
    if vanished:
        print(
            f"\nFAIL: {len(vanished)} expected survivor(s) were killed: {sorted(vanished)}\n"
            "  An exemption that no longer excuses anything must be deleted, not left standing."
        )
    return 1 if (unexpected or vanished or skipped) else 0


if __name__ == "__main__":
    sys.exit(main())
