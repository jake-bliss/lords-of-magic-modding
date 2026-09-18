#!/usr/bin/env python3
"""Compare two MPQ archive manifests and refuse an output that lost shape.

The manifests come from `lom-mpq manifest`, which addresses members by block
index rather than by name. That matters: PIC5R3 holds two *distinct* members
under the single name `portrait\\AIpotM.lbm`, so any check built from extracted
files -- or from names alone -- cannot tell 1,406 members from 1,405.

Nothing here reads an archive or writes a game profile. It reads two manifests
and decides whether the output is allowed to be installed.
"""

from __future__ import annotations

import argparse
import csv
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

# StormLib regenerates the internal listfile whenever it writes an archive, so
# its bytes cannot survive a repack. That is tolerated, and it is safe to
# tolerate only because the listfile's job -- naming members -- is checked
# directly: a name the rewritten listfile lost would turn that member into a
# `File%08u.xxx` slot in the output manifest and surface as a missing member
# plus an added one. Nothing else is exempt. An added `(attributes)` member, for
# instance, is a real difference and is reported as one.
INTERNAL_REWRITABLE_MEMBERS = frozenset({"(listfile)"})

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


@dataclass(frozen=True)
class Member:
    path: str
    block_index: int
    hash_index: int
    size: int
    compressed_size: int
    flags: str
    locale: int
    sha256: str

    def identity(self) -> tuple[str, int, int, str]:
        """Everything about a member that a faithful repack must preserve.

        Compressed size is excluded on purpose: it is a property of the packer,
        not of the content, and a repack is allowed to change it.
        """
        return (self.sha256, self.size, self.locale, self.flags)


@dataclass(frozen=True)
class Finding:
    kind: str
    path: str
    detail: str
    failure: bool


@dataclass
class ShapeReport:
    source_entries: int
    output_entries: int
    unchanged_members: int
    findings: list[Finding]

    @property
    def failures(self) -> list[Finding]:
        return [finding for finding in self.findings if finding.failure]

    @property
    def ok(self) -> bool:
        return not self.failures


def read_manifest(path: Path) -> list[Member]:
    with path.open(newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if tuple(reader.fieldnames or ()) != MANIFEST_COLUMNS:
            raise ValueError(
                f"{path}: unexpected manifest columns {reader.fieldnames!r}"
            )
        members = []
        for row in reader:
            members.append(
                Member(
                    path=row["path"],
                    block_index=int(row["block_index"]),
                    hash_index=int(row["hash_index"]),
                    size=int(row["size"]),
                    compressed_size=int(row["compressed_size"]),
                    flags=row["flags"],
                    locale=int(row["locale"]),
                    sha256=row["sha256"],
                )
            )
    return members


def group_by_path(members: list[Member]) -> dict[str, list[Member]]:
    groups: dict[str, list[Member]] = defaultdict(list)
    for member in members:
        groups[member.path].append(member)
    return dict(groups)


def _describe(members: list[Member]) -> str:
    return ", ".join(
        f"{member.sha256[:16]} {member.size}B {member.flags}"
        for member in sorted(members, key=lambda item: item.sha256)
    )


def compare(
    source: list[Member],
    output: list[Member],
    expected_changes: set[str] | None = None,
) -> ShapeReport:
    """Decide whether `output` has the same shape as `source`.

    `expected_changes` names the members a repack declared it would replace.
    A declared member whose content did not actually change is a failure: a
    repack that silently did nothing must not pass as a successful repack.
    """
    expected_changes = set(expected_changes or ())
    source_groups = group_by_path(source)
    output_groups = group_by_path(output)
    findings: list[Finding] = []
    unchanged = 0

    if not output:
        findings.append(
            Finding(
                "output_archive_empty",
                "",
                "the output archive contains no members",
                True,
            )
        )

    for name in sorted(expected_changes):
        if name not in source_groups:
            findings.append(
                Finding(
                    "declared_change_not_in_source",
                    name,
                    "a replacement was declared for a member the source does not have",
                    True,
                )
            )

    for name in sorted(source_groups.keys() | output_groups.keys()):
        in_source = source_groups.get(name, [])
        in_output = output_groups.get(name, [])
        source_count = len(in_source)
        output_count = len(in_output)

        if source_count != output_count:
            if output_count == 0:
                kind, detail = (
                    "member_missing",
                    f"present in the source ({source_count}), absent from the output",
                )
            elif source_count == 0:
                kind, detail = (
                    "member_added",
                    f"absent from the source, present in the output ({output_count})",
                )
            else:
                kind, detail = (
                    "member_count_changed",
                    f"{source_count} entries in the source, {output_count} in the output",
                )
            findings.append(Finding(kind, name, detail, True))
            continue

        if name in expected_changes:
            # A declared replacement may change the content. It may not change
            # how the member is stored: a member that came back uncompressed, or
            # under a different locale, is not the member the engine expects.
            if Counter((item.flags, item.locale) for item in in_source) != Counter(
                (item.flags, item.locale) for item in in_output
            ):
                findings.append(
                    Finding(
                        "declared_change_altered_storage",
                        name,
                        f"{_describe(in_source)} -> {_describe(in_output)}",
                        True,
                    )
                )
            elif Counter(member.sha256 for member in in_source) == Counter(
                member.sha256 for member in in_output
            ):
                findings.append(
                    Finding(
                        "declared_change_not_applied",
                        name,
                        "a replacement was declared but the content is unchanged",
                        True,
                    )
                )
            else:
                findings.append(
                    Finding(
                        "member_changed",
                        name,
                        f"{_describe(in_source)} -> {_describe(in_output)}",
                        False,
                    )
                )
            continue

        if name in INTERNAL_REWRITABLE_MEMBERS:
            findings.extend(_compare_internal_member(name, in_source, in_output))
            continue

        source_identities = Counter(member.identity() for member in in_source)
        output_identities = Counter(member.identity() for member in in_output)
        if source_identities == output_identities:
            unchanged += source_count
            continue

        if Counter(member.sha256 for member in in_source) != Counter(
            member.sha256 for member in in_output
        ):
            kind, detail = (
                "undeclared_content_change",
                f"{_describe(in_source)} -> {_describe(in_output)}",
            )
        else:
            kind, detail = (
                "undeclared_metadata_change",
                f"{_describe(in_source)} -> {_describe(in_output)}",
            )
        findings.append(Finding(kind, name, detail, True))

    findings = _label_case_folds(findings)
    return ShapeReport(len(source), len(output), unchanged, findings)


def _compare_internal_member(
    name: str, in_source: list[Member], in_output: list[Member]
) -> list[Finding]:
    """Allow a rewritten internal member's bytes, but not its metadata."""
    source_metadata = Counter((member.flags, member.locale) for member in in_source)
    output_metadata = Counter((member.flags, member.locale) for member in in_output)
    if source_metadata != output_metadata:
        return [
            Finding(
                "internal_member_metadata_change",
                name,
                f"{_describe(in_source)} -> {_describe(in_output)}",
                True,
            )
        ]
    if Counter(member.sha256 for member in in_source) == Counter(
        member.sha256 for member in in_output
    ):
        return []
    return [
        Finding(
            "internal_listfile_rewritten",
            name,
            f"{_describe(in_source)} -> {_describe(in_output)}; "
            "member names are verified individually",
            False,
        )
    ]


def _label_case_folds(findings: list[Finding]) -> list[Finding]:
    """Name the case-fold class explicitly instead of reporting two loose halves.

    A member that vanished and reappeared under a name differing only in case is
    a specific, recurring failure on a case-insensitive filesystem, and saying so
    is more useful than one `member_missing` plus one `member_added`.
    """
    added = {
        finding.path.casefold(): finding
        for finding in findings
        if finding.kind == "member_added"
    }
    # The partners are collected before anything is emitted. Deciding as we go
    # only worked when the missing half happened to sort first, which is a
    # property of the example names, not of the data.
    consumed = {
        added[finding.path.casefold()].path
        for finding in findings
        if finding.kind == "member_missing" and finding.path.casefold() in added
    }
    labelled: list[Finding] = []
    for finding in findings:
        if finding.kind == "member_missing" and finding.path.casefold() in added:
            partner = added[finding.path.casefold()]
            labelled.append(
                Finding(
                    "member_case_folded",
                    finding.path,
                    f"the output names it `{partner.path}`; "
                    "the two differ only in case",
                    True,
                )
            )
        elif finding.kind == "member_added" and finding.path in consumed:
            continue
        else:
            labelled.append(finding)
    return labelled


def format_report(report: ShapeReport) -> str:
    lines = [
        "== MPQ shape check ==",
        f"  source members   {report.source_entries}",
        f"  output members   {report.output_entries}",
        f"  proven unchanged {report.unchanged_members}",
    ]
    for finding in report.findings:
        marker = "FAIL" if finding.failure else "ok  "
        location = finding.path or "(archive)"
        lines.append(f"  {marker} {finding.kind} {location}: {finding.detail}")
    lines.append("  RESULT: " + ("shape preserved" if report.ok else "REFUSED"))
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--expect-changed",
        action="append",
        default=[],
        metavar="ARCHIVE_NAME",
        help="a member the repack declared it replaced; may be repeated",
    )
    arguments = parser.parse_args(argv)

    report = compare(
        read_manifest(arguments.source),
        read_manifest(arguments.output),
        set(arguments.expect_changed),
    )
    print(format_report(report))
    return 0 if report.ok else 1


if __name__ == "__main__":
    sys.exit(main())
