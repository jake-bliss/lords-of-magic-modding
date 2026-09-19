#!/usr/bin/env python3
"""The Python half of `scripts/mod-build.sh`: the plan, the build id, and the change report.

The packing itself is `scripts/repack-archive.sh`, unchanged and uncopied -- there is one archive
writer in this repository and this is not a second one. What lives here is everything that has to
be decided *before* the writer runs and everything that has to be recorded *after* it.

Two subcommands, because the shell needs to interleave a repack between them:

  plan    resolve every source file to its base member, derive the build id, and print the
          `NAME=FILE` replacement arguments repack-archive.sh takes
  report  write build.json and the change report, after the archives exist
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import engine_acceptance  # noqa: E402
from mod_report import (  # noqa: E402
    build_change_report,
    read_symbol_members,
    render_change_report,
    write_change_report,
)
from mod_tree import ModTreeError, source_digest  # noqa: E402
from mod_tree import load as load_mod_tree  # noqa: E402
from mod_validate import normalise_member, read_gs_facts  # noqa: E402
from mpq_shape import Member, read_manifest  # noqa: E402


def resolve_base_members(tree, manifests: dict[str, list[Member]]) -> dict[str, Member]:
    """Tree-relative path -> the base member it replaces.

    Case-exact, and a name held by more than one entry is refused rather than picked between --
    the same two rules `mod_validate.check_member_resolution` applies. Validation has already run
    and passed by the time this executes, so anything raising here is a pipeline defect rather
    than a user error, and it says so.
    """
    resolved: dict[str, Member] = {}
    for source in tree.members:
        members = [
            member
            for member in manifests.get(source.archive, [])
            if member.path == source.member
        ]
        if len(members) > 1:
            raise ModTreeError(
                f"{source.relative}: {source.archive} holds {len(members)} entries named "
                f"{source.member!r}; validation should have refused this build"
            )
        if members:
            resolved[source.relative] = members[0]
    return resolved


def entry_kind(
    source, base: Member | None, expect_unchanged: frozenset[str] = frozenset()
) -> str:
    """What the repack is being asked to do with one source file.

    Three answers, and the repack has to be told which because the shape check's expectation is
    different for each:

    ``add``       the base archive has no such member. Validation has already insisted the mod
                  declare it in `new_members` and set `allow_new_members`.
    ``unchanged`` the member is named in the mod's `expect_unchanged` list (`mod.toml`): a
                  DECLARED no-op, such as a codec proving it can reproduce a shipped member byte
                  for byte. It is still written -- that is the point of a no-op repack -- and the
                  shape check is told to fail if the content moves rather than if it does not.
    ``replace``   everything else -- INCLUDING a file that happens to be byte-identical to the
                  member it replaces. An undeclared no-op is not this classification's business
                  to excuse: `tools/mpq_shape.py`'s `declared_change_not_applied` exists so that a
                  repack that silently changed nothing is refused rather than shipped, and that
                  rule can only fire while an UNDECLARED no-op is still classified as a declared
                  change.

    This never reads the file's bytes. Whether a member came back changed is a question
    `mpq_shape.compare` answers against the packed archive, checked against whichever
    expectation this function handed it; folding that answer into the classification itself is
    exactly how an unedited member once passed as a successful repack -- `entry_kind` computed
    "unchanged" from the file's own digest before the expectation existed to be checked against,
    so `declared_change_not_applied` could never fire.
    """
    if base is None:
        return "add"
    if normalise_member(source.member) in expect_unchanged:
        return "unchanged"
    return "replace"


def derive_build_id(
    *,
    tree_digest: str,
    base_digests: dict[str, str],
    tool_digests: dict[str, str],
) -> str:
    """A build id that is a function of the build's inputs, not of the clock.

    Building the same tree against the same archives with the same tools twice produces the same
    id and therefore the same output directory. That is the point: a build id that moved with the
    wall clock would make two identical builds look like two different things, and the install log
    would record a change that never happened.
    """
    digest = hashlib.sha256()
    digest.update(tree_digest.encode())
    for name, value in sorted(base_digests.items()):
        digest.update(f"\0base:{name}={value}".encode())
    for name, value in sorted(tool_digests.items()):
        digest.update(f"\0tool:{name}={value}".encode())
    return digest.hexdigest()[:12]


def git_commit(project_dir: Path) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(project_dir), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return "unknown"
    return result.stdout.strip()


def parse_named(value: str) -> tuple[str, str]:
    label, separator, rest = value.partition("=")
    if not separator or not label:
        raise argparse.ArgumentTypeError(f"expected LABEL=VALUE, got {value!r}")
    return label, rest


def command_plan(arguments) -> int:
    tree = load_mod_tree(arguments.mod)
    manifests = {
        label: read_manifest(Path(path)) for label, path in arguments.base_manifest
    }
    resolved = resolve_base_members(tree, manifests)
    expect_unchanged = frozenset(
        normalise_member(name) for name in tree.manifest.expect_unchanged
    )
    base_digests = dict(arguments.base_digest)
    tool_digests = dict(arguments.tool_digest)
    build_id = derive_build_id(
        tree_digest=source_digest(tree),
        base_digests=base_digests,
        tool_digests=tool_digests,
    )

    plan = {
        "build_id": build_id,
        "mod_id": tree.manifest.id,
        "source_digest": source_digest(tree),
        "archives": {
            archive: [
                {
                    "member": source.member,
                    "file": str(source.path),
                    "relative": source.relative,
                    "base_block_index": (
                        resolved[source.relative].block_index
                        if source.relative in resolved
                        else None
                    ),
                    "kind": entry_kind(
                        source, resolved.get(source.relative), expect_unchanged
                    ),
                }
                for source in tree.members_for(archive)
            ]
            for archive in tree.archives()
        },
    }
    Path(arguments.output).write_text(json.dumps(plan, indent=2) + "\n", encoding="utf-8")
    print(build_id)
    return 0


def command_report(arguments) -> int:
    tree = load_mod_tree(arguments.mod)
    manifests = {
        label: read_manifest(Path(path)) for label, path in arguments.base_manifest
    }
    resolved = resolve_base_members(tree, manifests)
    base_facts = read_gs_facts(Path(arguments.base_gs_facts))
    mod_facts = read_gs_facts(Path(arguments.mod_gs_facts))
    base_files = {label: Path(path) for label, path in arguments.base_file}
    symbol_members = read_symbol_members(
        Path(arguments.symbols), tree.manifest.base_profile
    )

    changes = build_change_report(
        sources=tree.members,
        base_members=resolved,
        base_facts=base_facts,
        mod_facts=mod_facts,
        base_files=base_files,
        symbol_members=symbol_members,
    )

    output_dir = Path(arguments.output_dir)
    write_change_report(output_dir / "change-report.tsv", changes)

    build = {
        "build_id": arguments.build_id,
        "mod": {
            "id": tree.manifest.id,
            "name": tree.manifest.name,
            "version": tree.manifest.version,
            "base_profile": tree.manifest.base_profile,
        },
        "source_digest": source_digest(tree),
        "git_commit": git_commit(Path(__file__).resolve().parent.parent),
        "base_archive_digests": dict(arguments.base_digest),
        "tool_digests": dict(arguments.tool_digest),
        "output_archive_digests": dict(arguments.output_digest),
        "changed_members": [change.as_row() for change in changes],
        # Recorded in the build rather than only in the docs, so a build carries its own caveat --
        # rendered from `tools/engine_acceptance.py` rather than written here. It is deliberately
        # authoritative, which is exactly why it must not be prose a hand can edit: it used to be a
        # literal, it went stale, and three successive tests that asserted things *about* the
        # literal were each defeated by rewording it. The build carries the structure as well as
        # the sentence, and both come from the same facts.
        "engine_acceptance": engine_acceptance.build_metadata(),
    }
    (output_dir / "build.json").write_text(
        json.dumps(build, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    print("== change report ==")
    print(render_change_report(changes))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    plan = subparsers.add_parser("plan")
    plan.add_argument("mod", type=Path)
    plan.add_argument("--base-manifest", action="append", required=True, type=parse_named)
    plan.add_argument("--base-digest", action="append", default=[], type=parse_named)
    plan.add_argument("--tool-digest", action="append", default=[], type=parse_named)
    plan.add_argument("--output", required=True)
    plan.set_defaults(handler=command_plan)

    report = subparsers.add_parser("report")
    report.add_argument("mod", type=Path)
    report.add_argument("--base-manifest", action="append", required=True, type=parse_named)
    report.add_argument("--base-gs-facts", required=True)
    report.add_argument("--mod-gs-facts", required=True)
    report.add_argument("--base-file", action="append", default=[], type=parse_named)
    report.add_argument("--base-digest", action="append", default=[], type=parse_named)
    report.add_argument("--tool-digest", action="append", default=[], type=parse_named)
    report.add_argument("--output-digest", action="append", default=[], type=parse_named)
    report.add_argument("--symbols", required=True)
    report.add_argument("--build-id", required=True)
    report.add_argument("--output-dir", required=True)
    report.set_defaults(handler=command_report)

    arguments = parser.parse_args(argv)
    try:
        return arguments.handler(arguments)
    except ModTreeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
