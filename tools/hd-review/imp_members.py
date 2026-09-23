"""Which IMP member path a specific archive actually holds for a given sprite name -- shared by
`sprite_pack.py` (building packs) and `sprite_originals.py` (rendering review originals), so both
resolve a name to the same member, and neither can dedupe away a spelling before finding out
whether it, rather than some other spelling of the same name, is the one this archive holds.

A recovered listfile aggregates member paths across several game profiles. Two spellings of one
path (folder or extension case) name the same archive entry; two paths that only share a basename
-- different folders, such as `aura\\agx06b.imp` and `imp\\agx06b.imp` -- do not, and can each be
present in a given archive independently of the other. Deduplicating by basename BEFORE checking
which spellings a specific archive holds can silently keep an absent spelling while a present one
under a different folder is never even tried -- observed for real on the pristine archive, which
holds five such names under `imp\\`/other folders while a listfile-recovered `aura\\` spelling of
each is also present, but as a different, unrelated sprite.
"""
from __future__ import annotations

import pathlib
from typing import Callable, TypeVar

T = TypeVar("T")


def candidate_members(listfile: pathlib.Path) -> dict[str, list[str]]:
    """name -> every DISTINCT (case-insensitively) member path the listfile gives that basename,
    in sorted order. Two spellings of one path collapse to a single candidate here; two paths that
    only share a basename do not, because only trying each one against a specific archive can tell
    whether they are the same sprite or two unrelated ones (see `resolve_members`)."""
    seen_paths: dict[str, str] = {}
    for line in listfile.read_text().splitlines():
        member = line.strip()
        if member.lower().endswith(".imp"):
            seen_paths.setdefault(member.lower(), member)
    by_name: dict[str, list[str]] = {}
    for member in sorted(seen_paths.values(), key=str.lower):
        name = member.split("\\")[-1].rsplit(".", 1)[0].lower()
        by_name.setdefault(name, []).append(member)
    return by_name


def resolve_members(candidates_by_name: dict[str, list[str]],
                    probe: Callable[[str], "T | None"]) -> tuple[dict[str, tuple[str, "T"]], list[str]]:
    """name -> (member, probe(member)) for every name resolving to exactly one PRESENT candidate,
    plus a skip reason for every name that does not.

    `probe(member)` returns the caller's own information about `member` if it is present in the
    archive being built from, or `None` if it is not -- callers differ in what they need once a
    member is known present (a frame count, a sequence/facing description), but resolving WHICH
    member a name means is the same problem either way. Trying every candidate before picking one
    is what lets a name with two recovered spellings, only one of them present here, resolve
    correctly; it is also what catches the case where BOTH are present, sharing one record name
    only by coincidence -- reported as an ambiguous collision rather than a silent guess."""
    resolved: dict[str, tuple[str, "T"]] = {}
    skipped: list[str] = []
    for name, candidates in candidates_by_name.items():
        present = [(member, info) for member in candidates for info in [probe(member)] if info is not None]
        if not present:
            skipped.append(f"{name}: not in this archive")
        elif len(present) > 1:
            paths = ", ".join(member for member, _ in present)
            skipped.append(f"{name}: {len(present)} different members share this name in this "
                           f"archive ({paths}); ambiguous, left out")
        else:
            resolved[name] = present[0]
    return resolved, skipped
