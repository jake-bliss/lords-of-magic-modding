#!/usr/bin/env python3
"""The change report a build produces: every changed member, and for `.gs`, what changed in it.

"The bytes differ" is not a change report. A person reading this wants to know that
`units\\orinf.gs` had `hit_points` moved from 13 to 18 and nothing else, or that the only
difference is whitespace. Three existing pieces are reused rather than rewritten:

- `tools/compare_trees.py`'s unchanged / **reformatted** / modified split, which is the repo's
  existing way of saying "layout and comments only";
- `tools/gs_syntax.py`'s token normalisation, which is what that split is computed from. Its
  LF-only `;` comment rule was fixed on 2026-09-18, but it is still a second implementation of the
  same grammar and the two are not identical: Python's `str.isspace()` accepts non-ASCII whitespace
  where the Rust lexer's `is_ascii_whitespace` does not, and GS5R3's `shield_balkoth.gs` contains
  byte 0x85, which Python splits a name on and the engine's lexer does not. So this report computes
  the same split a second time from the Rust lexer and **reports when the two disagree** rather than
  trusting either silently;
- `reports/gameplay/symbols.tsv`, the 1,535-symbol gameplay index, to name which gameplay symbol a
  changed member defines.

Nothing here opens an archive or a game profile.
"""

from __future__ import annotations

import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import csv  # noqa: E402

from compare_trees import token_hash  # noqa: E402
from mpq_shape import Member  # noqa: E402

#: `compare_trees.compare`'s own vocabulary, kept identical so the two reports read the same way.
UNCHANGED = "unchanged"
REFORMATTED = "reformatted"
MODIFIED = "modified"
ADDED = "added"


@dataclass
class MemberChange:
    archive: str
    member: str
    status: str
    old_size: int | None
    new_size: int
    old_sha256: str | None
    new_sha256: str
    #: Human-readable lines describing *what* changed, for `.gs` members.
    detail: list[str] = field(default_factory=list)

    def as_row(self) -> dict[str, str]:
        return {
            "archive": self.archive,
            "member": self.member,
            "status": self.status,
            "old_size": "" if self.old_size is None else str(self.old_size),
            "new_size": str(self.new_size),
            "old_sha256": self.old_sha256 or "",
            "new_sha256": self.new_sha256,
            "detail": " | ".join(self.detail),
        }


def read_symbol_members(path: Path, profile: str) -> dict[str, list[str]]:
    """member name (folded) -> the gameplay symbols that member defines, for one profile."""
    index: dict[str, list[str]] = {}
    if not path.is_file():
        return index
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            if profile not in row["profiles"].split(","):
                continue
            index.setdefault(row["member"].casefold(), []).append(
                f"{row['name']} ({row['kind']})"
            )
    return index


def describe_gamescript_change(
    base_facts: dict | None,
    new_facts: dict,
    base_path: Path | None,
    new_path: Path,
) -> tuple[str, list[str]]:
    """Classify the change and say what moved. Returns (status, detail lines)."""
    detail: list[str] = []

    if base_facts is None:
        return ADDED, [f"new member, {new_facts['token_count']} tokens"]

    if base_facts["sha256"] == new_facts["sha256"]:
        return UNCHANGED, []

    if new_facts.get("parse_error"):
        # A member the build should never have reached; recorded rather than skipped so a report
        # produced by hand on unvalidated input still says so.
        error = new_facts["parse_error"]
        return MODIFIED, [
            f"DOES NOT LEX at line {error['line']} column {error['column']}: {error['message']}"
        ]

    token_identical = base_facts["token_sha256"] == new_facts["token_sha256"]
    status = REFORMATTED if token_identical else MODIFIED

    # The same question asked through the second implementation, `tools/gs_syntax.py`. A
    # disagreement means one of the two lexers mis-lexes one of the two files, and the reader needs
    # to know which answer they are looking at.
    if base_path is not None and base_path.is_file():
        python_identical = token_hash(base_path, base_facts["sha256"]) == token_hash(
            new_path, new_facts["sha256"]
        )
        if python_identical != token_identical:
            detail.append(
                "token comparison DISAGREES between the CR-aware Rust lexer "
                f"({'same' if token_identical else 'different'}) and tools/gs_syntax.py "
                f"({'same' if python_identical else 'different'}). The two are independent "
                "implementations of the same grammar and gs_syntax.py splits names on non-ASCII "
                "whitespace the engine's lexer keeps; the Rust answer is the one this report "
                "classifies on."
            )

    if token_identical:
        detail.append(
            "layout and comments only: the token stream is byte-identical "
            f"({new_facts['token_count']} tokens)"
        )
    else:
        delta = new_facts["token_count"] - base_facts["token_count"]
        detail.append(
            f"tokens {base_facts['token_count']} -> {new_facts['token_count']} "
            f"({delta:+d})"
        )

    base_scalars = base_facts.get("scalar_definitions", {})
    new_scalars = new_facts.get("scalar_definitions", {})
    changed = [
        f"{name}: {base_scalars[name]} -> {new_scalars[name]}"
        for name in sorted(set(base_scalars) & set(new_scalars))
        if base_scalars[name] != new_scalars[name]
    ]
    if changed:
        detail.append("values changed: " + "; ".join(changed))

    added_definitions = sorted(set(new_facts["definition_names"]) - set(base_facts["definition_names"]))
    dropped_definitions = sorted(set(base_facts["definition_names"]) - set(new_facts["definition_names"]))
    if added_definitions:
        detail.append("definitions added: " + ", ".join(added_definitions))
    if dropped_definitions:
        detail.append("definitions dropped: " + ", ".join(dropped_definitions))

    added_calls = sorted(set(new_facts["executable_names"]) - set(base_facts["executable_names"]))
    dropped_calls = sorted(set(base_facts["executable_names"]) - set(new_facts["executable_names"]))
    if added_calls:
        detail.append("now calls: " + ", ".join(added_calls))
    if dropped_calls:
        detail.append("no longer calls: " + ", ".join(dropped_calls))

    if base_facts["line_endings"] != new_facts["line_endings"]:
        detail.append(
            f"line endings {base_facts['line_endings']} -> {new_facts['line_endings']}"
        )

    if not detail:
        # Bytes moved, tokens did not, and no sub-check found anything. Say exactly that instead
        # of leaving the row silent.
        detail.append("bytes differ; no token, value, definition or call-site difference found")
    return status, detail


def build_change_report(
    *,
    sources: list,
    base_members: dict[str, Member],
    base_facts: dict[str, dict],
    mod_facts: dict[str, dict],
    base_files: dict[str, Path],
    symbol_members: dict[str, list[str]],
) -> list[MemberChange]:
    """One row per source member. `sources` are `mod_tree.SourceMember`s."""
    changes: list[MemberChange] = []
    for source in sorted(sources, key=lambda item: (item.archive, item.member)):
        base = base_members.get(source.relative)
        new_bytes = source.path.read_bytes()
        import hashlib

        new_sha = hashlib.sha256(new_bytes).hexdigest()

        detail: list[str] = []
        if source.is_gamescript:
            status, detail = describe_gamescript_change(
                base_facts.get(base.path) if base else None,
                mod_facts[source.relative],
                base_files.get(source.relative),
                source.path,
            )
        elif base is None:
            status = ADDED
        elif base.sha256 == new_sha:
            status = UNCHANGED
        else:
            status = MODIFIED
            detail.append(
                "binary member; this pipeline reports its size and digest and inspects nothing "
                "inside it"
            )

        symbols = symbol_members.get(base.path.casefold(), []) if base else []
        if symbols:
            detail.append("gameplay symbols defined here: " + ", ".join(sorted(symbols)))

        changes.append(
            MemberChange(
                archive=source.archive,
                member=source.member,
                status=status,
                old_size=base.size if base else None,
                new_size=len(new_bytes),
                old_sha256=base.sha256 if base else None,
                new_sha256=new_sha,
                detail=detail,
            )
        )
    return changes


def write_change_report(path: Path, changes: list[MemberChange]) -> None:
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            fieldnames=[
                "archive",
                "member",
                "status",
                "old_size",
                "new_size",
                "old_sha256",
                "new_sha256",
                "detail",
            ],
        )
        writer.writeheader()
        for change in changes:
            writer.writerow(change.as_row())


def render_change_report(changes: list[MemberChange]) -> str:
    lines: list[str] = []
    for change in changes:
        old_size = "-" if change.old_size is None else str(change.old_size)
        old_sha = (change.old_sha256 or "-")[:16]
        lines.append(
            f"  [{change.status}] {change.archive}  {change.member}\n"
            f"      size {old_size} -> {change.new_size}\n"
            f"      sha  {old_sha} -> {change.new_sha256[:16]}"
        )
        lines.extend(f"      {item}" for item in change.detail)
    return "\n".join(lines) if lines else "  (no members)"
