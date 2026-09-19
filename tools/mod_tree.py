#!/usr/bin/env python3
"""The mod source tree: what a mod is on disk, and which archive member each file becomes.

Nothing here opens an archive, reads `~/Applications`, or writes anything. It turns a directory
into a list of `(archive, member name, local file)` triples and refuses the shapes it cannot map
unambiguously. Keeping that pure is what lets `tests/test_mod_tree.py` exercise every refusal
against directories built in a temporary folder, with no game installed.

The layout::

    mods/<mod-id>/
      mod.toml
      archives/gs.mpq/units/orinf.gs        ->  member  units\\orinf.gs
      archives/gs.mpq/START.GS              ->  member  START.GS
      archives/pic.mpq/LBM/ACTIONS.lbm      ->  member  LBM\\ACTIONS.lbm

The path *below* `archives/<archive-name>/` is the member name with `/` turned into `\\`. The
`archives/<archive-name>/` level is not optional and is not inferred from the file's extension:
`START.GS` and `gs\\hotkey.gs` are both real members of `gs.mpq`, one at the archive root and one
in a directory, and a single `gs/` source directory could not distinguish them.
"""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

# The installed profiles, by the label `scripts/inventory-installed-profiles.sh` already uses.
#
# `Steambuild 32 64bit DXVK.app` is the preserved baseline and has no second copy; every profile
# here is opened read-only by this pipeline and written by none of it. The development profile is
# deliberately absent: it is a *target*, never a base, and `tools/install_guard.py` owns it.
PROFILE_APPS = {
    "vanilla": "Steambuild 32 64bit DXVK.app",
    "patch302": "Lords of Magic 3.02.app",
    "gs5r3": "Lords of Magic GS5R3.app",
}

GAME_SUBPATH = (
    "Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/"
    "common/Lords of Magic Special Edition/English"
)

# The archives this pipeline is willing to repack.
#
# `imp.mpq`, `sndfx.mpq` and `special.mpq` were added 2026-09-18 for the engine-acceptance ladder.
# All three are byte-identical across the installed profiles, so a change to one still cannot be
# validated against a profile difference -- but that was never the reason to keep them out. For
# `imp.mpq` the reason was that no IMP this repository writes had ever been inside an archive:
# every one landed in a loose `.imp`, and loose files do not override MPQ members. For the two
# audio archives it is a storage class: every member is 0x80010000, STORED, where every archive
# acceptance this project has proven was 0x80010100, IMPLODE.
#
# Widening this set stays a deliberate act, not a default. Note what it costs: `PIPELINE_ARCHIVES`
# manifests every archive here on every validate and every build, whether or not a mod touches it.
SUPPORTED_ARCHIVES = ("gs.mpq", "pic.mpq", "imp.mpq", "sndfx.mpq", "special.mpq")

MOD_ID_PATTERN = re.compile(r"\A[a-z0-9][a-z0-9-]*\Z")

# Files an editor or the Finder leaves behind. Each would otherwise be inferred as a member name,
# fail to resolve against the base manifest, and be reported as a puzzling missing member rather
# than as the piece of litter it is.
LITTER_NAMES = frozenset({".DS_Store", "Thumbs.db", ".gitkeep", ".gitignore"})


class ModTreeError(Exception):
    """A mod tree that cannot be read at all, as opposed to one with findings in it."""


@dataclass(frozen=True)
class SourceMember:
    """One file in the mod tree and the archive member it is destined to become."""

    archive: str
    member: str
    path: Path
    #: The path as written in the tree, relative to the mod root, for error messages.
    relative: str

    @property
    def is_gamescript(self) -> bool:
        return self.member.lower().endswith(".gs")


@dataclass(frozen=True)
class ModManifest:
    id: str
    name: str
    version: str
    base_profile: str
    #: Members the mod declares it is *adding* to an archive, spelled as member names.
    new_members: tuple[str, ...] = ()
    #: Adding a member has no engine evidence behind it, so it needs two acts: naming the member
    #: and setting this. See `docs/build-pipeline.md`, "what this does not guarantee".
    allow_new_members: bool = False


@dataclass
class ModTree:
    root: Path
    manifest: ModManifest
    members: list[SourceMember] = field(default_factory=list)
    #: Files under `archives/` that could not be mapped to a member, as (relative path, reason).
    rejected: list[tuple[str, str]] = field(default_factory=list)

    def archives(self) -> list[str]:
        return sorted({member.archive for member in self.members})

    def members_for(self, archive: str) -> list[SourceMember]:
        return [member for member in self.members if member.archive == archive]


def member_name_for(relative_posix: str) -> str:
    """Turn a tree-relative POSIX path into an MPQ member name.

    Only the separator changes. Case is preserved exactly, because the mod tree's spelling is
    checked against the base manifest's spelling and a normalisation here would destroy the very
    difference that check exists to find.
    """
    return relative_posix.replace("/", "\\")


def read_manifest(path: Path) -> ModManifest:
    try:
        raw = tomllib.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ModTreeError(f"{path}: no mod.toml") from error
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as error:
        raise ModTreeError(f"{path}: mod.toml is not readable TOML: {error}") from error

    missing = [key for key in ("id", "name", "version", "base_profile") if key not in raw]
    if missing:
        raise ModTreeError(f"{path}: mod.toml is missing {', '.join(sorted(missing))}")

    for key in ("id", "name", "version", "base_profile"):
        if not isinstance(raw[key], str) or not raw[key]:
            raise ModTreeError(f"{path}: mod.toml key {key!r} must be a non-empty string")

    if not MOD_ID_PATTERN.match(raw["id"]):
        raise ModTreeError(
            f"{path}: mod id {raw['id']!r} must be lowercase letters, digits and hyphens. "
            "The id becomes a directory name under artifacts/build/, so it is kept to a "
            "character set that needs no quoting anywhere in the pipeline."
        )
    if raw["base_profile"] not in PROFILE_APPS:
        raise ModTreeError(
            f"{path}: base_profile {raw['base_profile']!r} is not one of "
            f"{', '.join(sorted(PROFILE_APPS))}"
        )

    new_members = raw.get("new_members", [])
    if not isinstance(new_members, list) or not all(
        isinstance(entry, str) for entry in new_members
    ):
        raise ModTreeError(f"{path}: new_members must be a list of member-name strings")

    allow_new = raw.get("allow_new_members", False)
    if not isinstance(allow_new, bool):
        raise ModTreeError(f"{path}: allow_new_members must be true or false")

    unknown = set(raw) - {
        "id",
        "name",
        "version",
        "base_profile",
        "new_members",
        "allow_new_members",
        "description",
    }
    if unknown:
        # A key the pipeline ignores is a key an author believes is doing something.
        raise ModTreeError(
            f"{path}: mod.toml has keys this pipeline does not understand: "
            f"{', '.join(sorted(unknown))}"
        )

    return ModManifest(
        id=raw["id"],
        name=raw["name"],
        version=raw["version"],
        base_profile=raw["base_profile"],
        new_members=tuple(new_members),
        allow_new_members=allow_new,
    )


def load_manifest(root: Path) -> ModManifest:
    """Read and check a mod's `mod.toml` WITHOUT requiring its `archives/` tree to exist yet.

    Seeding is the step that creates `archives/`, so it cannot use `load()` -- which requires the
    directory seeding is about to make. Splitting the manifest half out keeps the id check in one
    place rather than letting the bootstrap path skip it, which is how a mod directory renamed
    after creation would otherwise get past `mod-seed.sh` and fail only at build time.
    """
    root = Path(root)
    if not root.is_dir():
        raise ModTreeError(f"{root}: not a directory")

    manifest = read_manifest(root / "mod.toml")
    if manifest.id != root.name:
        raise ModTreeError(
            f"{root}: mod id {manifest.id!r} does not match the directory name {root.name!r}. "
            "They are kept equal so a build artifact can be traced back to one source directory "
            "by name alone."
        )
    return manifest


def load(root: Path) -> ModTree:
    """Read a mod tree, mapping every file under `archives/` to a member name."""
    root = Path(root)
    manifest = load_manifest(root)

    tree = ModTree(root=root, manifest=manifest)
    archives_root = root / "archives"
    if not archives_root.is_dir():
        raise ModTreeError(
            f"{root}: no archives/ directory. A mod tree's members are not committed -- seed them "
            f"from a local install first: scripts/mod-seed.sh {root} 'gs.mpq:units\\orinf.gs'"
        )

    seen: dict[tuple[str, str], str] = {}
    for path in sorted(archives_root.rglob("*")):
        if path.is_dir():
            continue
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            # A symlink would let a mod tree pack bytes from outside itself, which makes the
            # recorded source-tree digest a lie about what was built.
            tree.rejected.append((relative, "symlink; the mod tree must contain its own bytes"))
            continue
        if not path.is_file():
            tree.rejected.append((relative, "not a regular file"))
            continue

        parts = path.relative_to(archives_root).parts
        if path.name in LITTER_NAMES:
            tree.rejected.append(
                (relative, f"{path.name} is editor or Finder litter, not archive content")
            )
            continue
        if len(parts) < 2:
            tree.rejected.append(
                (
                    relative,
                    "a file directly under archives/ has no archive to belong to; "
                    "put it under archives/<archive-name>/",
                )
            )
            continue

        archive, *rest = parts
        if archive not in SUPPORTED_ARCHIVES:
            tree.rejected.append(
                (
                    relative,
                    f"unsupported archive directory {archive!r}; "
                    f"this pipeline packs {', '.join(SUPPORTED_ARCHIVES)}",
                )
            )
            continue

        member = member_name_for("/".join(rest))
        if "\\" in member and member.startswith("\\"):
            tree.rejected.append((relative, "member name would start with a separator"))
            continue

        # The archive's name hash is case-insensitive (Observed, docs/repack.md), so two tree
        # files differing only in case would collide into one member. Catching it here gives a
        # named reason rather than a silent last-writer-wins.
        key = (archive, member.casefold())
        if key in seen:
            tree.rejected.append(
                (
                    relative,
                    f"member name collides case-insensitively with {seen[key]!r}; "
                    "an MPQ cannot hold both",
                )
            )
            continue
        seen[key] = relative
        tree.members.append(
            SourceMember(archive=archive, member=member, path=path, relative=relative)
        )

    return tree


def source_digest(tree: ModTree) -> str:
    """A digest of everything the build reads from the mod tree.

    Covers `mod.toml` and every mapped member's name and bytes, so a build id derived from it
    changes whenever the input does. Rejected files are deliberately excluded: they are not input
    to the build, and including them would make the digest depend on litter.
    """
    import hashlib

    digest = hashlib.sha256()
    digest.update(b"mod.toml\0")
    digest.update(hashlib.sha256((tree.root / "mod.toml").read_bytes()).digest())
    for member in sorted(tree.members, key=lambda item: (item.archive, item.member)):
        digest.update(f"{member.archive}\0{member.member}\0".encode())
        digest.update(hashlib.sha256(member.path.read_bytes()).digest())
    return digest.hexdigest()
