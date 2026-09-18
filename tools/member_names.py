#!/usr/bin/env python3
"""Recover MPQ member names from a pooled catalogue, and refuse the rest.

An MPQ stores a *hash* of each member's name, never the name. A name is
recovered only from a catalogue -- an archive's own `(listfile)` or an external
one. This repository has treated the unnamed remainder as a hard limit; it is
not, because names are archive-independent. A name learned from one profile's
listfile can be tested directly against another profile's archive.

The distinction this tool exists to enforce:

* a **proposal** is a name that something suggests -- a matching content digest,
  an entry in some catalogue, a hash-index coincidence;
* a **confirmation** is the target archive itself opening that name and handing
  back the block, the hash-table slot, and the bytes that the block-index
  manifest already recorded for that block.

Only confirmations are reported as names. Content matching is not used here at
all: it proposes a name wherever bytes agree and fails wherever a mod changed
them, so it both over- and under-reaches. `lom-mpq probe-names` does the opening
and this module does the deciding, so the deciding can be unit-tested against
archives that do not exist.

Nothing here opens an archive or writes a game profile.
"""

from __future__ import annotations

import argparse
import csv
import re
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

# StormLib's placeholder for a member whose name the archive does not know. It
# is a BLOCK-SLOT LABEL and nothing else: block N of one archive is not block N
# of another, so these must never be compared or joined across archives. A naive
# comparison of them reports 377 pure-artefact "differences" in `gs.mpq`.
#
# **Observed 2026-09-18** on vanilla `sndfx.mpq`, StormLib 9.40: a pseudo-name is
# not merely a label StormLib prints, it is a name StormLib *resolves
# positionally*, and the resolution ignores everything after the digits. All of
# `File00000022.wav`, `File00000022.xxx`, `file00000022.wav` and
# `File00000022.wavZQNOTREAL` open block 22. `File00000022` (no dot),
# `File22.wav`, `File000000022.wav` and `sub\File00000022.wav` do not open at
# all. The rule is therefore: case-insensitive `File`, exactly eight digits, a
# dot, then anything, with no directory component.
#
# Two consequences, and both are load-bearing here:
#
# 1. A pseudo-name must never enter the candidate pool. It would "confirm"
#    against its own block trivially and report a recovery that is nothing but
#    the block number written back out.
# 2. The extension StormLib prints is a *guess from the member's first bytes*,
#    not a name. `.xxx` is only what it falls back to when it cannot guess. An
#    archive whose listfile appears full of `File00000000.wav` entries -- vanilla
#    `sndfx.mpq` and `special.mpq` both do -- has no names at all.
PSEUDO_NAME = re.compile(r"^file\d{8}\.", re.IGNORECASE)

MANIFEST_COLUMNS = (
    "path",
    "block_index",
    "hash_index",
    "size",
    "compressed_size",
    "flags",
    "locale",
    "sha256",
)

PROBE_COLUMNS = ("name", "status", "block_index", "hash_index", "size", "sha256")

# MPQ name hashing is case-insensitive, so two spellings that differ only in
# case are one name to the archive and only one of them can be reported as
# measured. The case a recovered name is printed in is whatever the donor
# catalogue recorded; see `docs/member-names.md`.
def fold(name: str) -> str:
    return name.replace("/", "\\").lower()


def is_pseudo_name(name: str) -> bool:
    """True for a name StormLib resolves by block position rather than by hash."""
    return PSEUDO_NAME.match(name) is not None


@dataclass(frozen=True)
class Block:
    """One member of the target archive, addressed by block index."""

    path: str
    block_index: int
    hash_index: int
    size: int
    sha256: str

    @property
    def is_named(self) -> bool:
        return not is_pseudo_name(self.path)


@dataclass(frozen=True)
class ProbeHit:
    """One candidate name that the target archive opened."""

    name: str
    block_index: int
    hash_index: int
    size: int
    sha256: str


@dataclass
class Decision:
    block: Block
    confirmed: list[str] = field(default_factory=list)
    # A name the archive opened onto this block whose hash slot, size or digest
    # disagreed with the manifest. Kept and reported rather than dropped: a
    # non-empty column here means the instrument is not doing what it claims.
    rejected: list[tuple[str, str]] = field(default_factory=list)

    @property
    def state(self) -> str:
        if self.block.is_named:
            return "already-named"
        if len(self.confirmed) == 1:
            return "recovered"
        if len(self.confirmed) > 1:
            return "recovered-ambiguous"
        return "unnamed"

    @property
    def name(self) -> str:
        if self.block.is_named:
            return self.block.path
        if self.confirmed:
            return sorted(self.confirmed)[0]
        return ""


def read_manifest(handle) -> list[Block]:
    reader = csv.DictReader(handle, delimiter="\t")
    if reader.fieldnames is None or tuple(reader.fieldnames) != MANIFEST_COLUMNS:
        raise ValueError(f"unexpected manifest columns: {reader.fieldnames}")
    blocks = []
    for row in reader:
        blocks.append(
            Block(
                path=row["path"],
                block_index=int(row["block_index"]),
                hash_index=int(row["hash_index"]),
                size=int(row["size"]),
                sha256=row["sha256"],
            )
        )
    seen = Counter(block.block_index for block in blocks)
    duplicated = sorted(index for index, count in seen.items() if count > 1)
    if duplicated:
        raise ValueError(f"manifest repeats block indexes: {duplicated}")
    return blocks


def read_probe(handle) -> tuple[list[ProbeHit], list[str]]:
    """Return the names the archive opened, and the names it refused."""
    reader = csv.DictReader(handle, delimiter="\t")
    if reader.fieldnames is None or tuple(reader.fieldnames) != PROBE_COLUMNS:
        raise ValueError(f"unexpected probe columns: {reader.fieldnames}")
    hits: list[ProbeHit] = []
    absent: list[str] = []
    for row in reader:
        if row["status"] == "absent":
            absent.append(row["name"])
            continue
        if row["status"] != "present":
            raise ValueError(f"unexpected probe status: {row['status']!r}")
        hits.append(
            ProbeHit(
                name=row["name"],
                block_index=int(row["block_index"]),
                hash_index=int(row["hash_index"]),
                size=int(row["size"]),
                sha256=row["sha256"],
            )
        )
    return hits, absent


def read_catalogue(handle) -> list[str]:
    names = []
    for line in handle:
        name = line.strip()
        if name:
            names.append(name)
    return names


def pool_candidates(sources: dict[str, list[str]]) -> tuple[list[str], dict[str, set[str]]]:
    """Merge catalogues into one candidate list, remembering who contributed.

    Deduplication is case-insensitive because the archive's own lookup is. The
    first spelling seen wins, and every source that offered the name is recorded
    against it so per-source contribution can be reported honestly.
    """
    order: list[str] = []
    seen: dict[str, str] = {}
    attribution: dict[str, set[str]] = defaultdict(set)
    for label in sorted(sources):
        for name in sources[label]:
            if is_pseudo_name(name):
                continue
            key = fold(name)
            if key not in seen:
                seen[key] = name
                order.append(name)
            attribution[key].add(label)
    return order, attribution


# Suffix appended to a real name to build a name that cannot exist. It is
# alphabetic so the mutated name stays in the same shape class as the candidates
# it is drawn from -- a control built out of punctuation would test the
# tokenizer rather than the hash lookup.
CONTROL_SUFFIX = "ZQNOTREAL"


def make_controls(pool: list[str], stride: int = 25) -> list[str]:
    """Mutate every `stride`-th pooled name into one that cannot exist.

    Drawing controls from the candidate pool rather than inventing a handful by
    hand keeps them in the same distribution as the real candidates and makes
    the control count scale with the run, so the measured false-positive rate is
    over hundreds of trials rather than two.
    """
    if stride < 1:
        raise ValueError("stride must be positive")
    controls = [name + CONTROL_SUFFIX for name in pool[stride - 1 :: stride]]
    return [name for name in controls if not is_pseudo_name(name)]


def decide(blocks: list[Block], hits: list[ProbeHit]) -> list[Decision]:
    """Confirm names against blocks. A hit is only a proposal until it agrees.

    A confirmation requires all four of: the archive opened the name; the block
    it opened is this block; the hash-table slot StormLib reported is the slot
    the manifest recorded for this block; and the bytes read by name digest to
    what reading the block by index digested to. The hash-index term is what
    settles cases where several catalogue names claim one content digest -- a
    name's hash slot is a property of the name, so at most one spelling of a
    name can occupy a given slot.
    """
    by_block = {block.block_index: Decision(block=block) for block in blocks}
    for hit in hits:
        decision = by_block.get(hit.block_index)
        if decision is None:
            # The archive opened a name onto a block the manifest never listed.
            # Impossible for a manifest of the same archive; a loud failure is
            # better than a silent drop.
            raise ValueError(
                f"probe hit {hit.name!r} names block {hit.block_index}, "
                "which is absent from the manifest"
            )
        block = decision.block
        if is_pseudo_name(hit.name):
            decision.rejected.append((hit.name, "positional pseudo-name"))
        elif hit.hash_index != block.hash_index:
            decision.rejected.append((hit.name, "hash-index disagrees"))
        elif hit.size != block.size:
            decision.rejected.append((hit.name, "size disagrees"))
        elif hit.sha256 != block.sha256:
            decision.rejected.append((hit.name, "digest disagrees"))
        else:
            decision.confirmed.append(hit.name)
    return [by_block[block.block_index] for block in blocks]


def source_contribution(
    decisions: list[Decision], attribution: dict[str, set[str]]
) -> dict[str, dict[str, int]]:
    """Per source: blocks it could name, and blocks only it could name."""
    contribution: dict[str, dict[str, int]] = defaultdict(
        lambda: {"covered": 0, "unique": 0}
    )
    for decision in decisions:
        if decision.state not in {"recovered", "recovered-ambiguous"}:
            continue
        labels: set[str] = set()
        for name in decision.confirmed:
            labels |= attribution.get(fold(name), set())
        for label in labels:
            contribution[label]["covered"] += 1
        if len(labels) == 1:
            contribution[next(iter(labels))]["unique"] += 1
    return dict(contribution)


def summarize(decisions: list[Decision]) -> Counter:
    return Counter(decision.state for decision in decisions)


def write_resolution(handle, decisions: list[Decision]) -> None:
    writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
    writer.writerow(
        ["block_index", "hash_index", "state", "name", "alternatives", "rejected"]
    )
    for decision in decisions:
        alternatives = sorted(decision.confirmed)[1:] if not decision.block.is_named else []
        writer.writerow(
            [
                decision.block.block_index,
                decision.block.hash_index,
                decision.state,
                decision.name,
                ";".join(alternatives),
                ";".join(f"{name} ({why})" for name, why in decision.rejected),
            ]
        )


def write_listfile(handle, decisions: list[Decision]) -> int:
    """Emit every confirmed name, for `lom-mpq --listfile`.

    Recovered names only. A member the archive already names needs no help, and
    re-emitting it would make the file's size look like coverage it did not
    earn.
    """
    names = sorted(
        {
            decision.name
            for decision in decisions
            if decision.state in {"recovered", "recovered-ambiguous"}
        },
        key=fold,
    )
    for name in names:
        handle.write(name + "\n")
    return len(names)


def check_controls(controls: list[str], absent: list[str], hits: list[ProbeHit]) -> list[str]:
    """Every negative control must have been refused. Returns the failures.

    A bounded negative is only as strong as the instrument that produced it. If
    a name known not to exist opens anyway, every other "present" in the run is
    worthless and the tool must say so rather than report a total.
    """
    refused = {fold(name) for name in absent}
    opened = {fold(hit.name) for hit in hits}
    failures = []
    for control in controls:
        key = fold(control)
        if key in opened:
            failures.append(f"{control}: opened, but was expected to be absent")
        elif key not in refused:
            failures.append(f"{control}: was never probed")
    return failures


def read_sources(parser, assignments: list[str]) -> dict[str, list[str]]:
    sources: dict[str, list[str]] = {}
    for assignment in assignments:
        label, separator, path = assignment.partition("=")
        if not separator:
            parser.error(f"expected LABEL=PATH, got: {assignment}")
        with Path(path).open(encoding="utf-8", errors="replace", newline="") as handle:
            sources[label] = read_catalogue(handle)
    return sources


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--probe", type=Path)
    parser.add_argument(
        "--source",
        action="append",
        default=[],
        metavar="LABEL=PATH",
        help="a catalogue that contributed candidate names, for attribution",
    )
    parser.add_argument(
        "--controls",
        type=Path,
        help="file of names that must NOT open; any that does fails the run",
    )
    parser.add_argument("--resolution", type=Path)
    parser.add_argument("--listfile", type=Path)
    parser.add_argument(
        "--emit-pool",
        type=Path,
        help="write the pooled candidate names and exit; requires --source",
    )
    parser.add_argument(
        "--emit-controls",
        type=Path,
        help="write the derived negative controls alongside --emit-pool",
    )
    arguments = parser.parse_args(argv)

    if arguments.emit_pool is not None:
        sources = read_sources(parser, arguments.source)
        pool, _ = pool_candidates(sources)
        pool.sort(key=fold)
        with arguments.emit_pool.open("w", encoding="utf-8", newline="") as handle:
            handle.writelines(name + "\n" for name in pool)
        print(f"Pooled {len(pool)} distinct candidate name(s)")
        if arguments.emit_controls is not None:
            controls = make_controls(pool)
            with arguments.emit_controls.open(
                "w", encoding="utf-8", newline=""
            ) as handle:
                handle.writelines(name + "\n" for name in controls)
            print(f"Derived {len(controls)} negative control(s)")
        return 0

    if arguments.manifest is None or arguments.probe is None:
        parser.error("--manifest and --probe are required unless --emit-pool is given")

    # newline="" throughout: universal-newline translation would turn a stray
    # carriage return inside a member name into a row break.
    with arguments.manifest.open(encoding="utf-8", newline="") as handle:
        blocks = read_manifest(handle)
    with arguments.probe.open(encoding="utf-8", newline="") as handle:
        hits, absent = read_probe(handle)

    sources = read_sources(parser, arguments.source)
    _, attribution = pool_candidates(sources)

    if arguments.controls is not None:
        with arguments.controls.open(encoding="utf-8", newline="") as handle:
            controls = read_catalogue(handle)
        failures = check_controls(controls, absent, hits)
        if failures:
            for failure in failures:
                print(f"negative control failed: {failure}", file=sys.stderr)
            return 1
        print(f"Negative controls: {len(controls)} probed, {len(controls)} refused")

    decisions = decide(blocks, hits)
    counts = summarize(decisions)

    if arguments.resolution is not None:
        with arguments.resolution.open("w", encoding="utf-8") as handle:
            write_resolution(handle, decisions)
    if arguments.listfile is not None:
        with arguments.listfile.open("w", encoding="utf-8") as handle:
            written = write_listfile(handle, decisions)
        print(f"Wrote {written} recovered name(s) to {arguments.listfile}")

    total = len(blocks)
    print(f"{arguments.manifest}: {total} member(s)")
    for state in ("already-named", "recovered", "recovered-ambiguous", "unnamed"):
        print(f"  {state:<20} {counts.get(state, 0)}")
    rejected = sum(len(decision.rejected) for decision in decisions)
    print(f"  rejected proposals   {rejected}")

    contribution = source_contribution(decisions, attribution)
    if contribution:
        print("Recovered names by source (covered / only this source):")
        for label in sorted(contribution):
            numbers = contribution[label]
            print(f"  {label:<24} {numbers['covered']:>5} / {numbers['unique']:>5}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
