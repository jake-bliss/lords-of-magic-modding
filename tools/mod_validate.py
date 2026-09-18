#!/usr/bin/env python3
"""Static validation of a mod source tree, before anything is packed.

This opens no installed game file for writing and needs no StormLib. Its inputs are all files
somebody else produced: `lom-mpq manifest` output for each base archive, `lom-asset-viewer
--gs-facts` output for the base archive and for the mod tree, and the committed corpus vocabulary
in `reports/gs/`. That is what makes every check here testable against a machine with no game on
it.

Two rules govern what this module reports.

**Every finding names a file, a position where one exists, and what specifically is wrong.** A
finding that says "encoding problem" has failed; one that says "the base member had no line
endings and this file has 34 bare LF -- it was reflowed" has not.

**Every bounded negative is published with its blind spot.** "No unresolved references" is only as
strong as the instrument, and this instrument is a *vocabulary* check: it knows whether a name
exists somewhere in the base profile's corpus, not whether it is reachable from the member that
calls it. The coverage block at the end of a run states that, with counts, and the caller is
expected to print it.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from mod_tree import (  # noqa: E402
    ModTree,
    ModTreeError,
    SourceMember,
)
from mod_tree import load as load_mod_tree  # noqa: E402
from mpq_shape import Member, read_manifest  # noqa: E402

ERROR = "error"
WARNING = "warning"
NOTE = "note"

SEVERITY_ORDER = {ERROR: 0, WARNING: 1, NOTE: 2}


@dataclass(frozen=True)
class Finding:
    severity: str
    check: str
    location: str
    message: str

    def __str__(self) -> str:
        return f"{self.severity}: {self.location}: [{self.check}] {self.message}"


@dataclass
class Coverage:
    """What the run could *not* examine, so a clean result is read with its limits attached."""

    counts: dict[str, int] = field(default_factory=dict)
    notes: dict[str, str] = field(default_factory=dict)

    def record(self, key: str, count: int, note: str) -> None:
        self.counts[key] = self.counts.get(key, 0) + count
        self.notes[key] = note

    def lines(self) -> list[str]:
        return [
            f"  {key}: {self.counts[key]}  -- {self.notes[key]}"
            for key in sorted(self.counts)
        ]


@dataclass
class ValidationReport:
    findings: list[Finding] = field(default_factory=list)
    coverage: Coverage = field(default_factory=Coverage)

    def add(self, severity: str, check: str, location: str, message: str) -> None:
        self.findings.append(Finding(severity, check, location, message))

    @property
    def errors(self) -> list[Finding]:
        return [finding for finding in self.findings if finding.severity == ERROR]

    @property
    def ok(self) -> bool:
        return not self.errors

    def sorted_findings(self) -> list[Finding]:
        return sorted(
            self.findings,
            key=lambda finding: (
                SEVERITY_ORDER[finding.severity],
                finding.location,
                finding.check,
            ),
        )


# --------------------------------------------------------------------------------------------
# Inputs
# --------------------------------------------------------------------------------------------


def read_gs_facts(path: Path) -> dict[str, dict]:
    """JSON Lines from `--gs-facts`, keyed by the name each record carries."""
    facts: dict[str, dict] = {}
    with path.open(encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as error:
                raise ModTreeError(f"{path}:{number}: not JSON: {error}") from error
            facts[record["name"]] = record
    return facts


def read_vocabulary(path: Path) -> dict[str, str]:
    """`reports/gs/vocabulary-<profile>.tsv`, name -> class.

    Case-sensitive, because the interpreter resolves names case-sensitively -- the same correction
    `reports/gs/vocabulary.md` records, and folding case here would undo it.
    """
    vocabulary: dict[str, str] = {}
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            vocabulary[row["name"]] = row["class"]
    return vocabulary


def normalise_member(name: str) -> str:
    return name.replace("/", "\\").casefold()


# --------------------------------------------------------------------------------------------
# Checks
# --------------------------------------------------------------------------------------------


def check_tree_shape(tree: ModTree, report: ValidationReport) -> None:
    for relative, reason in tree.rejected:
        report.add(ERROR, "tree-shape", relative, reason)
    if not tree.members:
        report.add(
            ERROR,
            "tree-shape",
            str(tree.root),
            "the mod tree maps no archive members at all; a build would change nothing",
        )


def check_member_resolution(
    tree: ModTree,
    manifests: dict[str, list[Member]],
    report: ValidationReport,
) -> dict[str, Member]:
    """Resolve each source file against its base archive manifest, case-exactly.

    Returns the base member for every source file that resolved, keyed by the tree-relative path.
    """
    declared_new = {normalise_member(name) for name in tree.manifest.new_members}
    resolved: dict[str, Member] = {}

    for archive in tree.archives():
        if archive not in manifests:
            report.add(
                ERROR,
                "base-manifest",
                f"archives/{archive}",
                f"no base manifest was supplied for {archive}; nothing in it can be validated",
            )
            continue

        by_exact: dict[str, list[Member]] = defaultdict(list)
        by_folded: dict[str, list[Member]] = defaultdict(list)
        for member in manifests[archive]:
            by_exact[member.path].append(member)
            by_folded[member.path.casefold()].append(member)

        for source in tree.members_for(archive):
            exact = by_exact.get(source.member, [])
            folded = by_folded.get(source.member.casefold(), [])

            if len(exact) > 1 or (not exact and len(folded) > 1):
                # PIC5R3 holds two distinct members under the one byte-identical name
                # `portrait\AIpotM.lbm`, at blocks 1108 and 1144. A mod tree has one file per
                # name and therefore cannot say which block it means. Refusing is the only
                # honest answer; a repack that picked one would silently pick.
                candidates = exact or folded
                blocks = ", ".join(str(member.block_index) for member in candidates)
                report.add(
                    ERROR,
                    "ambiguous-member",
                    source.relative,
                    f"{archive} holds {len(candidates)} entries under the name "
                    f"{source.member!r} (blocks {blocks}). A source tree addresses members by "
                    "name and cannot express which block it means, so this member cannot be "
                    "replaced by this pipeline.",
                )
                continue

            if exact:
                resolved[source.relative] = exact[0]
                continue

            if folded:
                report.add(
                    ERROR,
                    "case-mismatch",
                    source.relative,
                    f"the base archive spells this member {folded[0].path!r}, the tree spells "
                    f"it {source.member!r}. The MPQ name hash is case-insensitive so a repack "
                    "would still find it, but the tree and the archive disagreeing about a name "
                    "is how a rename gets made by accident.",
                )
                continue

            if normalise_member(source.member) in declared_new:
                if tree.manifest.allow_new_members:
                    report.add(
                        WARNING,
                        "new-member",
                        source.relative,
                        f"{source.member!r} is not in the base archive and is being ADDED. No "
                        "evidence exists that the engine tolerates a member added to an archive: "
                        "every engine-verified write so far has replaced an existing member. "
                        "This is Inferred, and untested.",
                    )
                else:
                    report.add(
                        ERROR,
                        "new-member",
                        source.relative,
                        f"{source.member!r} is declared in new_members but allow_new_members is "
                        "false. Adding a member is refused by default because it has never been "
                        "put in front of the engine; set allow_new_members = true to accept "
                        "that risk deliberately.",
                    )
                continue

            report.add(
                ERROR,
                "missing-member",
                source.relative,
                f"{archive} has no member named {source.member!r}. If this file is meant to add "
                "a new member, name it in mod.toml's new_members.",
            )

    return resolved


def check_gamescript_syntax(
    tree: ModTree, mod_facts: dict[str, dict], report: ValidationReport
) -> None:
    for source in tree.members:
        if not source.is_gamescript:
            continue
        facts = mod_facts.get(source.relative)
        if facts is None:
            report.add(
                ERROR,
                "lex",
                source.relative,
                "no lexer facts were produced for this file; --gs-facts did not see it",
            )
            continue
        error = facts.get("parse_error")
        if error:
            report.add(
                ERROR,
                "lex",
                f"{source.relative}:{error['line']}:{error['column']}",
                f"{error['message']} (byte offset {error['offset']})",
            )
            continue
        for anomaly in facts.get("procedure_anomalies", []):
            report.add(
                ERROR,
                "lex",
                f"{source.relative}:{anomaly['line']}:{anomaly['column']}",
                f"{anomaly['message']} (byte offset {anomaly['offset']})",
            )


def check_encoding(
    tree: ModTree,
    mod_facts: dict[str, dict],
    base_facts: dict[str, dict],
    resolved: dict[str, Member],
    report: ValidationReport,
) -> None:
    """Report what changed about the bytes, not whether they match a guessed rule.

    The corpus survey behind these rules, **Observed in a local binary 2026-09-18** across all
    4,692 `.gs` members of the three profiles' `gs.mpq`:

    - the only control bytes present are TAB, CR and LF -- zero occurrences of any other byte
      below 0x20 and zero of 0x7f, in 4,692 members;
    - bytes above 0x7e occur in 17 members, and **not one of those 17 is valid UTF-8**;
    - 3,050 members contain no line ending at all, 1,337 are pure CRLF, 49 pure bare CR, 23 pure
      bare LF, and 233 mix CRLF with a bare form.

    So there is no single line-ending style to assert and no character encoding to demand. The
    check is a comparison against the base member, which is the only baseline that means anything.
    """
    for source in tree.members:
        if not source.is_gamescript:
            continue
        facts = mod_facts.get(source.relative)
        if facts is None:
            continue

        alphabet = facts["alphabet"]
        if alphabet["unexpected_control"]:
            listed = ", ".join(
                f"0x{int(byte):02x}x{count}"
                for byte, count in sorted(alphabet["unexpected_control"].items())
            )
            report.add(
                ERROR,
                "encoding",
                source.relative,
                f"contains control bytes that occur nowhere in the 4,692-member .gs corpus: "
                f"{listed}. Only TAB, CR and LF appear in any shipped member.",
            )

        base = base_facts.get(resolved[source.relative].path) if source.relative in resolved else None
        if base is None:
            continue

        base_endings = base["line_endings"]
        new_endings = facts["line_endings"]
        if base_endings != new_endings:
            report.add(
                WARNING,
                "line-endings",
                source.relative,
                f"the line-ending census changed. base "
                f"{_describe_endings(base_endings)}; this file "
                f"{_describe_endings(new_endings)}. A change here means the file was reflowed or "
                "re-terminated by an editor, which is a different artifact from the one you "
                "meant to edit.",
            )

        base_high = {int(byte) for byte in base["alphabet"]["high"]}
        new_high = {int(byte) for byte in facts["alphabet"]["high"]}
        introduced = new_high - base_high
        if introduced:
            report.add(
                WARNING,
                "encoding",
                source.relative,
                "introduces bytes above 0x7e that the base member does not contain: "
                + ", ".join(f"0x{byte:02x}" for byte in sorted(introduced)),
            )
        if base_high and not new_high and facts["alphabet"]["valid_utf8"]:
            report.add(
                WARNING,
                "encoding",
                source.relative,
                "the base member contains bytes above 0x7e and is not valid UTF-8; this file has "
                "none and is. That is the signature of an editor re-saving the file as UTF-8, "
                "which changes every high byte into a two-byte sequence the engine has never "
                "been shown to read.",
            )


def _describe_endings(endings: dict[str, int]) -> str:
    parts = [
        f"{endings['crlf']} CRLF",
        f"{endings['bare_cr']} bare CR",
        f"{endings['bare_lf']} bare LF",
    ]
    if not any(endings.values()):
        return "has no line ending at all (one single line)"
    return ", ".join(parts)


def check_symbols(
    tree: ModTree,
    mod_facts: dict[str, dict],
    base_facts: dict[str, dict],
    vocabulary: dict[str, str],
    resolved: dict[str, Member],
    report: ValidationReport,
) -> None:
    """Duplicate definitions, and every reference resolving against the *post-change* corpus.

    The post-change corpus is the base corpus with the edited members swapped out: a name the base
    member defined and this file dropped stops existing, and a name this file adds starts existing.
    That is the only corpus a reference check can honestly be run against, because checking against
    the base corpus would approve a mod that deletes the definition it relies on.
    """
    replaced_members = {
        resolved[source.relative].path
        for source in tree.members
        if source.is_gamescript and source.relative in resolved
    }

    # definition -> the base members that define it, excluding the ones being replaced.
    definitions: dict[str, set[str]] = defaultdict(set)
    unlexable_base_members = 0
    for name, facts in base_facts.items():
        if name in replaced_members:
            continue
        if facts.get("parse_error"):
            unlexable_base_members += 1
            continue
        for definition in facts["definition_names"]:
            definitions[definition].add(name)

    report.coverage.record(
        "base-members-that-do-not-lex",
        unlexable_base_members,
        "base .gs members excluded from the definition map because they do not lex; "
        "a duplicate or a reference involving one of them cannot be seen",
    )

    added_definitions: dict[str, str] = {}
    for source in tree.members:
        if not source.is_gamescript:
            continue
        facts = mod_facts.get(source.relative)
        if facts is None or facts.get("parse_error"):
            continue

        base = base_facts.get(resolved[source.relative].path) if source.relative in resolved else None
        base_definitions = set(base["definition_names"]) if base else set()
        new_definitions = set(facts["definition_names"])

        for definition in sorted(new_definitions - base_definitions):
            elsewhere = definitions.get(definition, set())
            if elsewhere:
                report.add(
                    ERROR,
                    "duplicate-definition",
                    source.relative,
                    f"defines {definition!r}, which is already defined by "
                    f"{', '.join(sorted(elsewhere))}. The interpreter resolves a name to one "
                    "binding; two definitions means one of them silently loses.",
                )
            if definition in added_definitions:
                report.add(
                    ERROR,
                    "duplicate-definition",
                    source.relative,
                    f"defines {definition!r}, which {added_definitions[definition]} in this same "
                    "mod also defines",
                )
            else:
                added_definitions[definition] = source.relative

        for definition in sorted(base_definitions - new_definitions):
            # A definition the edit dropped. Whether that matters depends on who called it.
            callers = [
                name
                for name, other in base_facts.items()
                if name not in replaced_members
                and definition in other.get("executable_names", {})
            ]
            if callers:
                shown = ", ".join(sorted(callers)[:5])
                more = f" and {len(callers) - 5} more" if len(callers) > 5 else ""
                report.add(
                    ERROR,
                    "dropped-definition",
                    source.relative,
                    f"no longer defines {definition!r}, which {len(callers)} other member(s) "
                    f"call: {shown}{more}",
                )
            else:
                report.add(
                    NOTE,
                    "dropped-definition",
                    source.relative,
                    f"no longer defines {definition!r}; no other member in the base corpus calls "
                    "it by that name",
                )

    # Reference resolution.
    #
    # The post-change corpus is the base corpus with the replaced members taken out, plus
    # everything the mod's own files define -- not merely what they *add*. A member that both
    # defines and calls a name, which is the shape of every unit, spell and artifact record
    # (`units\orinf.gs` calls `orinf` 18 times and defines it once), would otherwise be reported
    # as calling something nothing defines the moment it was excluded from the base map.
    post_change = set(definitions) | set(added_definitions)
    for source in tree.members:
        facts = mod_facts.get(source.relative)
        if source.is_gamescript and facts and not facts.get("parse_error"):
            post_change.update(facts["definition_names"])
    vocabulary_only = 0
    checked_references = 0
    for source in tree.members:
        if not source.is_gamescript:
            continue
        facts = mod_facts.get(source.relative)
        if facts is None or facts.get("parse_error"):
            continue
        for name in sorted(facts["executable_names"]):
            checked_references += 1
            if name in post_change:
                continue
            klass = vocabulary.get(name)
            if klass is None:
                report.add(
                    ERROR,
                    "unresolved-reference",
                    source.relative,
                    f"calls {name!r}, which no member of the base profile defines and which does "
                    f"not appear anywhere in that profile's {len(vocabulary):,}-name executable "
                    "vocabulary. The overwhelmingly likely cause is a typo.",
                )
                continue
            if klass == "script-definition":
                # The vocabulary says some member defines it, but the definition map built here
                # did not find it -- usually because the defining member does not lex.
                report.add(
                    WARNING,
                    "unresolved-reference",
                    source.relative,
                    f"calls {name!r}. The published vocabulary classes it as a script definition, "
                    "but no member in the post-change definition map defines it. Check the "
                    "base-members-that-do-not-lex count below before reading this as a defect.",
                )
                continue
            vocabulary_only += 1

    report.coverage.record(
        "references-resolved-by-vocabulary-presence-only",
        vocabulary_only,
        "executable names accepted because the base profile's vocabulary lists them as a "
        "primitive, a native host call, a constant or unclassified residue. That is PRESENCE in "
        "the corpus, not REACHABILITY from the calling member: this check cannot tell a name the "
        "interpreter would find from one defined in a dictionary that is never open",
    )
    report.coverage.record(
        "executable-references-examined",
        checked_references,
        "distinct executable names examined across the mod's .gs files",
    )


def check_run_targets_and_assets(
    tree: ModTree,
    mod_facts: dict[str, dict],
    manifests: dict[str, list[Member]],
    report: ValidationReport,
) -> None:
    """`run` targets and path-shaped literals must name something in the base profile.

    Resolution is case-insensitive because the MPQ name hash is (**Observed**, `docs/repack.md`:
    an archive cannot even hold two names differing only in case). This is the one place the
    pipeline folds case on purpose, and it is the opposite rule from the mod-tree-versus-manifest
    check, which is case-exact.
    """
    all_names: set[str] = set()
    for members in manifests.values():
        all_names.update(normalise_member(member.path) for member in members)
    all_names.update(normalise_member(source.member) for source in tree.members)

    unchecked_strings = 0
    checked_paths = 0
    for source in tree.members:
        if not source.is_gamescript:
            continue
        facts = mod_facts.get(source.relative)
        if facts is None or facts.get("parse_error"):
            continue
        unchecked_strings += facts.get("non_path_string_count", 0)

        for target in facts.get("static_run_dependencies", []):
            if normalise_member(target) not in all_names:
                report.add(
                    ERROR,
                    "missing-run-target",
                    source.relative,
                    f"`run`s {target!r}, which is not a member of any base archive supplied and "
                    "is not added by this mod",
                )

        for literal in facts.get("path_strings", []):
            if literal in facts.get("static_run_dependencies", []):
                continue
            checked_paths += 1
            if normalise_member(literal) not in all_names:
                report.add(
                    WARNING,
                    "missing-asset",
                    source.relative,
                    f"references the path-shaped literal {literal!r}, which names no member of "
                    "any base archive supplied. Some literals are built at run time from pieces, "
                    "so this is a warning rather than an error.",
                )

    report.coverage.record(
        "string-literals-not-examined-as-paths",
        unchecked_strings,
        "string literals that do not look like a path and were therefore never checked against "
        "any archive. A path assembled at run time from fragments is in this count and is "
        "invisible to this check",
    )
    report.coverage.record(
        "path-shaped-literals-checked",
        checked_paths,
        "path-shaped string literals resolved against the base profile's archives",
    )


def check_unvalidatable_content(tree: ModTree, report: ValidationReport) -> None:
    """Say plainly what this validator does not read at all."""
    non_script = [source for source in tree.members if not source.is_gamescript]
    report.coverage.record(
        "members-with-no-content-validation",
        len(non_script),
        "non-.gs members. Their bytes are packed as given and nothing here inspects them: there "
        "is no image, sprite or map validation in this pipeline",
    )
    pic_members = [source for source in tree.members if source.archive == "pic.mpq"]
    if pic_members:
        report.add(
            WARNING,
            "engine-acceptance",
            str(tree.root),
            f"{len(pic_members)} member(s) target pic.mpq. A rewritten pic.mpq has NEVER been "
            "put in front of the engine. Storage class is NOT the open question: every one of "
            "the 1,071 members of the baseline pic.mpq carries flags 0x80010100 "
            "(EXISTS | ENCRYPTED | IMPLODE), measured 2026-09-18, which is the same class as the "
            "gs.mpq member the engine accepted on 2026-09-16. What is untested is whether the "
            "engine reads a rewritten pic.mpq at all.",
        )


# --------------------------------------------------------------------------------------------
# Driver
# --------------------------------------------------------------------------------------------


def validate(
    tree: ModTree,
    manifests: dict[str, list[Member]],
    base_facts: dict[str, dict],
    mod_facts: dict[str, dict],
    vocabulary: dict[str, str],
) -> ValidationReport:
    report = ValidationReport()
    check_tree_shape(tree, report)
    resolved = check_member_resolution(tree, manifests, report)
    check_gamescript_syntax(tree, mod_facts, report)
    check_encoding(tree, mod_facts, base_facts, resolved, report)
    check_symbols(tree, mod_facts, base_facts, vocabulary, resolved, report)
    check_run_targets_and_assets(tree, mod_facts, manifests, report)
    check_unvalidatable_content(tree, report)
    return report


def parse_named_path(value: str) -> tuple[str, Path]:
    label, _, raw = value.partition("=")
    if not label or not raw:
        raise argparse.ArgumentTypeError(f"expected ARCHIVE=PATH, got {value!r}")
    return label, Path(raw)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mod", type=Path, help="the mod source tree")
    parser.add_argument(
        "--base-manifest",
        action="append",
        required=True,
        type=parse_named_path,
        metavar="ARCHIVE=PATH",
        help="lom-mpq manifest output for one base archive; repeatable",
    )
    parser.add_argument("--base-gs-facts", required=True, type=Path)
    parser.add_argument("--mod-gs-facts", required=True, type=Path)
    parser.add_argument("--vocabulary", required=True, type=Path)
    arguments = parser.parse_args(argv)

    try:
        tree = load_mod_tree(arguments.mod)
    except ModTreeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    manifests = {label: read_manifest(path) for label, path in arguments.base_manifest}
    report = validate(
        tree,
        manifests,
        read_gs_facts(arguments.base_gs_facts),
        read_gs_facts(arguments.mod_gs_facts),
        read_vocabulary(arguments.vocabulary),
    )

    print(f"== validating {tree.manifest.id} {tree.manifest.version} ==")
    print(f"  base profile: {tree.manifest.base_profile}")
    print(f"  members:      {len(tree.members)} across {', '.join(tree.archives()) or 'nothing'}")
    print()
    if report.findings:
        for finding in report.sorted_findings():
            print(finding)
    else:
        print("no findings")
    print()
    print("== what this run could not check ==")
    for line in report.coverage.lines():
        print(line)
    print()
    if report.ok:
        print(f"validate PASSED with {len(report.findings)} finding(s), 0 errors")
        return 0
    print(f"validate FAILED: {len(report.errors)} error-severity finding(s)", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
