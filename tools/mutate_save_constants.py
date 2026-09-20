#!/usr/bin/env python3
"""Mutate the `save.rs` constants and refusals one at a time and report which survive.

Covers the `LS_SPR_` record model, the corrected `LS_GAME` model, and every RECONSTRUCTED field
and refusal in the nine section encoders.

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

# The writer-side mutations, added 2026-09-19 with the nine section encoders. Every one of these
# targets something RECONSTRUCTED -- a count word, a length, a version gate or a refusal. A
# mutation of something REPLAYED would be invisible by construction and is deliberately not here:
# see the RECONSTRUCTED / REPLAYED split in `save.rs`'s module header.
ENCODERS = [
    # -- the corrected LS_GAME model ------------------------------------------------------------
    (
        "    pub const LEN: usize = 12;",
        "    pub const LEN: usize = 11;",
        "game record width 12 -> 11",
    ),
    (
        "    pub const LEN: usize = 12;",
        "    pub const LEN: usize = 13;",
        "game record width 12 -> 13",
    ),
    (
        "    pub const LEN: usize = 784;",
        "    pub const LEN: usize = 783;",
        "LS_USER record width 784 -> 783",
    ),
    (
        "    pub const LEN: usize = 784;",
        "    pub const LEN: usize = 785;",
        "LS_USER record width 784 -> 785",
    ),
    (
        "    pub const BLOCK_4FCC_LEN: usize = 32;",
        "    pub const BLOCK_4FCC_LEN: usize = 31;",
        "LS_GAME 32-byte block -> 31",
    ),
    (
        "    pub const BLOCK_4FCC_LEN: usize = 32;",
        "    pub const BLOCK_4FCC_LEN: usize = 33;",
        "LS_GAME 32-byte block -> 33",
    ),
    (
        "    pub const BLOCK_230CC_LEN: usize = 200;",
        "    pub const BLOCK_230CC_LEN: usize = 199;",
        "LS_GAME 200-byte block -> 199",
    ),
    (
        "    pub const BLOCK_230CC_LEN: usize = 200;",
        "    pub const BLOCK_230CC_LEN: usize = 201;",
        "LS_GAME 200-byte block -> 201",
    ),
    (
        "pub const OBSERVED_GAME_COUNTED_ARRAY_LEN: usize = 150;",
        "pub const OBSERVED_GAME_COUNTED_ARRAY_LEN: usize = 149;",
        "observed counted-array length 150 -> 149",
    ),
    (
        "pub const OBSERVED_GAME_COUNTED_ARRAY_LEN: usize = 150;",
        "pub const OBSERVED_GAME_COUNTED_ARRAY_LEN: usize = 151;",
        "observed counted-array length 150 -> 151",
    ),
    (
        "fn game_unguarded_length(raw: u32) -> usize {\n    raw as usize\n}",
        "fn game_unguarded_length(raw: u32) -> usize {\n    (raw as i32).max(0) as usize\n}",
        "LS_GAME counted-array length treated as guarded",
    ),
    (
        "            let count = game_signed_count(cursor.u32()?);",
        "            let count = cursor.u32()?;",
        "LS_GAME record count modelled unsigned",
    ),
    # The raw-bytes walker is a SECOND transcription on purpose. A mutation of one that the other
    # does not catch would mean the two had collapsed into one implementation.
    (
        "    if version >= 80 {\n        let count = (word(&mut at)? as i32).max(0) as usize;",
        "    if version >= 81 {\n        let count = (word(&mut at)? as i32).max(0) as usize;",
        "game walker gate 80 -> 81",
    ),
    (
        "    if version >= 105 {\n        skip(&mut at, 200)?;",
        "    if version >= 104 {\n        skip(&mut at, 200)?;",
        "game walker gate 105 -> 104",
    ),
    # -- the reconstructed counts ---------------------------------------------------------------
    (
        "        let array_count = self.regions.len().checked_sub(1).ok_or_else(|| {",
        "        let array_count = self.regions.len().checked_sub(0).ok_or_else(|| {",
        "LS_REGN array_count = len - 1 -> len",
    ),
    (
        "        let array_count = self.regions.len().checked_sub(1).ok_or_else(|| {",
        "        let array_count = self.regions.len().checked_sub(2).ok_or_else(|| {",
        "LS_REGN array_count = len - 1 -> len - 2",
    ),
    (
        "                count_word(tag, \"an alarm queue\", &contents.records)?,",
        "                contents.records.len().saturating_sub(1) as u32,",
        "LS_ALRM queue count written short",
    ),
    (
        "        push_u32(&mut out, count_word(tag, \"the second plane\", &self.plane)?);",
        "        push_u32(&mut out, self.plane_count);",
        "LS_MAP_ plane count replayed instead of reconstructed",
    ),
    (
        "        push_u32(out, count_word(tag, \"the roster slot list\", &self.slots)?);",
        "        push_u32(out, self.slot_count);",
        "LS_PLR_ roster slot count replayed instead of reconstructed",
    ),
    (
        "        push_u32(&mut out, declared_setup_len);",
        "        push_u32(&mut out, self.declared_setup_len);",
        "LS_MULT setup length replayed instead of reconstructed",
    ),
    (
        "            .div_ceil(32)\n            * 4;",
        "            .div_ceil(16)\n            * 4;",
        "bitset word width div_ceil(32) -> div_ceil(16)",
    ),
    # -- the refusals ---------------------------------------------------------------------------
    (
        "        (true, None) => Err(SaveError::section(",
        "        (true, None) if false => Err(SaveError::section(",
        "gate: missing gated field no longer refused",
    ),
    (
        "        (false, Some(_)) => Err(SaveError::section(",
        "        (false, Some(_)) if false => Err(SaveError::section(",
        "gate: surplus gated field no longer refused",
    ),
    (
        "        if self.records.len() != Self::RECORD_COUNT {",
        "        if self.records.len() > Self::RECORD_COUNT {",
        "LS_USER record-count refusal weakened to an upper bound",
    ),
    (
        "        if self.armies.len() != Self::ARMY_COUNT {",
        "        if self.armies.len() > Self::ARMY_COUNT {",
        "LS_PLR_ army-count refusal weakened to an upper bound",
    ),
    (
        "        if self.slot_index >= PlayerSection::MAX_SLOT_INDEX {",
        "        if self.slot_index > PlayerSection::MAX_SLOT_INDEX {",
        "LS_PLR_ slot-index bound off by one",
    ),
    (
        "        let name_len = u8::try_from(self.name_raw.len()).map_err(|_| {",
        "        let name_len = Ok::<u8, ()>(self.name_raw.len() as u8).map_err(|_: ()| {",
        "LS_REGN name length truncated instead of refused",
    ),
    (
        "        if present != AlarmQueue::ALL {",
        "        if false && present != AlarmQueue::ALL {",
        "LS_ALRM queue-order refusal removed",
    ),
    (
        "        if self.map.header_form != MapHeaderForm::Grid {",
        "        if false && self.map.header_form != MapHeaderForm::Grid {",
        "LS_MAP_ grid-form refusal removed",
    ),
    (
        "        if declared_cells != self.cells.len() {",
        "        if false && declared_cells != self.cells.len() {",
        "LS_REGN grid-size refusal removed",
    ),
    (
        "            if record.raw.len() != UserRecord::LEN {",
        "            if false && record.raw.len() != UserRecord::LEN {",
        "LS_USER per-record width refusal removed",
    ),
    (
        "    SectionTag::Version,\n    SectionTag::Multiplayer,\n    SectionTag::Map,",
        "    SectionTag::Multiplayer,\n    SectionTag::Version,\n    SectionTag::Map,",
        "writer section order: first two swapped",
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
    # `LS_GAME`'s ladder, added 2026-09-19. Written decimal and inside a `pub mod`, so it needs its
    # own pattern -- a regex that silently matched nothing here would have reported a clean sweep
    # over six gates it never touched.
    for name, value in re.findall(
        r"^    pub const (\w+): i32 = (\d+);$", text, re.MULTILINE
    ):
        original = f"    pub const {name}: i32 = {value};"
        for delta in (1, -1):
            moved = f"    pub const {name}: i32 = {int(value) + delta};"
            out.append((original, moved, f"game_section_versions::{name} {delta:+d}"))
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
        mutations += STRUCTURAL + ENCODERS

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
