#!/usr/bin/env python3
"""The one path this pipeline is allowed to write inside `~/Applications`.

The weak form of this check is "the target is not the baseline". It fails open: it approves every
path nobody thought to name, including `~/Applications/Lords of Magic 3.02.app`, a typo, a relative
path that escapes upwards, and a symlink pointing at the baseline. The strong form is an
**allowlist of exactly one** directory, and that is what this module implements. Every write the
installer performs is routed through :func:`resolve_write_target`, so an unlisted path is not
merely disapproved -- there is no code path that produces a handle to it.

`Steambuild 32 64bit DXVK.app` is the preserved baseline and has no second copy. The loose `map/`
directory inside every profile has no backup at all. Neither fact is encoded as a special case
here, because a special case is a list of things to remember; the allowlist is a list of one thing
to permit.

Nothing in this module touches the filesystem except to *resolve* paths. It creates nothing.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path, PurePath

#: The development profile's directory name. The installer may create and write this and nothing
#: else. It is a constant rather than an argument so that no caller can widen it.
DEV_PROFILE_NAME = "Lords of Magic Development.app"


class InstallRefused(Exception):
    """A write that the allowlist does not permit. Always a refusal, never a warning."""


@dataclass(frozen=True)
class WriteTarget:
    """A path the allowlist has approved, with the root it was approved against."""

    root: Path
    path: Path


def dev_profile_root(applications_dir: Path | str) -> Path:
    """The single permitted root, under a caller-supplied `Applications` directory.

    `applications_dir` is a parameter only so the refusal tests can point the whole mechanism at a
    temporary directory. Production callers pass `~/Applications`; a test passing its own directory
    exercises exactly the same code, which is the point -- a guard with a test-only bypass is not
    the guard that runs.
    """
    return Path(applications_dir).expanduser() / DEV_PROFILE_NAME


def _resolve_through_existing(path: Path) -> Path:
    """Resolve symlinks in the part of `path` that exists, keeping the rest literal.

    `Path.resolve()` on a non-existent path resolves its existing ancestors already, but it also
    normalises `..` lexically *after* following links on some platforms. Walking down from the
    deepest existing ancestor makes the order explicit: links are followed first, and the
    not-yet-created tail is appended verbatim.
    """
    path = Path(os.path.abspath(path.expanduser()))
    existing = path
    tail: list[str] = []
    while not existing.exists() and existing != existing.parent:
        tail.append(existing.name)
        existing = existing.parent
    resolved = existing.resolve()
    for name in reversed(tail):
        resolved = resolved / name
    return resolved


def resolve_write_target(
    requested: Path | str, applications_dir: Path | str
) -> WriteTarget:
    """Approve `requested` or raise. The only way to obtain a writable path.

    Refuses, in order:

    - a path containing `..`, before any resolution, so the refusal names what was written rather
      than what it resolved to;
    - a development-profile root that is itself a symlink, which would otherwise resolve to its
      target and be compared against a root resolved the same way -- the hole that makes
      "compare the resolved paths" insufficient on its own;
    - anything that is not the root or a descendant of it.
    """
    requested_pure = PurePath(str(requested)).as_posix()
    if ".." in PurePath(requested_pure).parts:
        raise InstallRefused(
            f"refusing a path containing '..': {requested}. "
            "The allowlist is checked against the path as given as well as resolved."
        )

    root = dev_profile_root(applications_dir)
    if root.is_symlink():
        # If the root were a symlink into the baseline, resolving both sides would make every
        # write to the baseline compare equal to the allowed root and pass.
        raise InstallRefused(
            f"refusing: {root} is a symlink. The development profile must be a real directory, "
            "because a link would let an approved path resolve into a profile that has no backup."
        )

    resolved_root = _resolve_through_existing(root)
    resolved = _resolve_through_existing(Path(str(requested)))

    if resolved != resolved_root and resolved_root not in resolved.parents:
        raise InstallRefused(
            f"refusing to write outside the development profile.\n"
            f"  requested: {requested}\n"
            f"  resolved:  {resolved}\n"
            f"  permitted: {resolved_root} (and nothing else)"
        )
    return WriteTarget(root=resolved_root, path=resolved)


def assert_writable(requested: Path | str, applications_dir: Path | str) -> Path:
    """`resolve_write_target` when only the approved path is wanted."""
    return resolve_write_target(requested, applications_dir).path


def game_directory(profile_root: Path | str) -> Path:
    """The `English/` directory inside a profile bundle, where the archives live."""
    from mod_tree import GAME_SUBPATH  # noqa: PLC0415 -- avoids a module-level cycle

    return Path(profile_root) / GAME_SUBPATH


def _main() -> int:
    """A tiny front end so shell callers get the same guard the Python callers get."""
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--applications-dir", required=True)
    parser.add_argument("path", help="the path to approve")
    arguments = parser.parse_args()
    try:
        approved = assert_writable(arguments.path, arguments.applications_dir)
    except InstallRefused as refusal:
        print(refusal, file=__import__("sys").stderr)
        return 1
    print(approved)
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
