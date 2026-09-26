#!/usr/bin/env python3
"""Bind save-file user record 0 to the marauder slot (player 15). Marauder probe, rung 2.

    python3 tools/marauder_save_bind.py check SAVE
    python3 tools/marauder_save_bind.py bind SAVE OUT --backup-dir DIR

`check` prints the eight `LS_USER` bindings: for each user record, the file offset of its `+0`
word and the player that word names. `bind` writes a COPY of SAVE to OUT with record 0's word
changed from 0 to 15, and never touches SAVE.

**Why that word.** `docs/save-format.md` records `LS_USER +0` as "the record's own index"; in the
engine it is the player the user is bound to. `currentuser` (`0x004E3DF0`) returns it for the
current user record, `setuserforplayer` (`0x0052CEF0`) searches the eight records for it, and the
load routine (`0x0052D090`) `fread`s each record whole into the in-memory user table. No script
operator can write it, which is why rung 1 is predicted not to bind; see `docs/marauder-probe.md`.

**There is one save parser, and this is not it.** The container walk, the section census (exactly
one `LS_USER`), the eight-records-of-784 rule and the encoder all live in
`spikes/asset-viewer/src/save.rs`. This module drives it through the `save_bind_user` example,
which parses, requires a byte-identical re-encode, edits the decoded record, re-encodes, and
refuses unless the only bytes that moved are the four of that word. The format has no checksum
and no compression, and the edit is fixed-width, so nothing else in the file has to change.

What this module adds, and checks independently of the Rust side:

- a **byte backup** of SAVE, a copy compared byte for byte with its source, never a digest --
  a hash can say a file changed but cannot give it back;
- the refusals: OUT must not exist and must not be SAVE; record 0 must be bound to 0 and nothing
  may already be bound to 15; inside `~/Applications`, OUT must pass `tools/install_guard.py`
  (the Development profile only), and the game must not be running;
- its own diff of SAVE against OUT, located by the offset `check` reported: same length, every
  differing byte inside those four, the word reading 15, and `check` on OUT reading
  `[15, 1, 2, ... 7]` with the rest unchanged.
"""

from __future__ import annotations

import argparse
import shutil
import struct
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import install_guard  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
VIEWER = ROOT / "spikes" / "asset-viewer"
BINARY = VIEWER / "target" / "debug" / "examples" / "save_bind_user"
APPLICATIONS = Path.home() / "Applications"

MARAUDER_PLAYER = 15  # WANDERING_MONSTER_PLAYER; tools/marauder_probe.py reads it from lomse.exe
RECORD = 0
USER_RECORDS = 8


class BindError(Exception):
    """Anything unexpected. Always a refusal; nothing is written after one."""


@dataclass(frozen=True)
class Binding:
    record: int
    offset: int
    player: int


def build_binary() -> Path:
    """Rebuild the Rust side first, every time: no step runs against a stale tool."""
    result = subprocess.run(
        ["cargo", "build", "--quiet", "--example", "save_bind_user"],
        cwd=VIEWER, capture_output=True, text=True)
    if result.returncode != 0 or not BINARY.is_file():
        raise BindError(f"could not build save_bind_user:\n{result.stderr}")
    return BINARY


def parse_check_output(text: str) -> list[Binding]:
    """The eight `record R  offset 0xO  player P` lines `save_bind_user --check` prints."""
    bindings = []
    for line in text.splitlines():
        if not line.startswith("record "):
            continue
        fields = line.split()
        if len(fields) != 6 or fields[2] != "offset" or fields[4] != "player":
            raise BindError(f"unexpected check line: {line!r}")
        bindings.append(Binding(int(fields[1]), int(fields[3], 16), int(fields[5])))
    if [b.record for b in bindings] != list(range(USER_RECORDS)):
        raise BindError(f"expected user records 0..7, got {[b.record for b in bindings]}")
    return bindings


def check(save: Path) -> list[Binding]:
    binary = build_binary()
    result = subprocess.run([str(binary), "--check", str(save)], capture_output=True, text=True)
    if result.returncode != 0:
        raise BindError(result.stderr.strip() or f"save_bind_user --check failed on {save}")
    bindings = parse_check_output(result.stdout)
    # Independent of the Rust side: the word at each reported offset is the reported player, and
    # record 0 sits right after the section's own tag.
    data = save.read_bytes()
    for binding in bindings:
        word = struct.unpack_from("<i", data, binding.offset)[0]
        if word != binding.player:
            raise BindError(f"record {binding.record}: offset {binding.offset:#x} holds {word}, "
                            f"check said {binding.player}")
    if data[bindings[0].offset - 8: bindings[0].offset - 1] != b"LS_USER":
        raise BindError(f"record 0 at {bindings[0].offset:#x} does not follow an LS_USER tag")
    return bindings


def backup(save: Path, backup_dir: Path) -> Path:
    """A byte copy of `save`, verified against it. Refuses to overwrite a different backup."""
    backup_dir.mkdir(parents=True, exist_ok=True)
    target = backup_dir / (save.name + ".orig")
    original = save.read_bytes()
    if target.exists():
        if target.read_bytes() != original:
            raise BindError(f"{target} exists and differs from {save}; refusing to overwrite it")
        return target
    shutil.copyfile(save, target)
    if target.read_bytes() != original:
        raise BindError(f"the backup {target} does not match {save} byte for byte")
    return target


def game_is_running() -> bool:
    """The pipeline's own guard (scripts/lib-mod-pipeline.sh), not a second one."""
    result = subprocess.run(
        ["bash", "-c", f'source "{ROOT}/scripts/lib-mod-pipeline.sh" && refuse_if_game_running'],
        capture_output=True, text=True)
    return result.returncode != 0


def approve_output(out: Path, applications_dir: Path) -> bool:
    """True when OUT is inside `applications_dir`, after `install_guard` approved it (the
    Development profile only). Anywhere else is not the guard's business and returns False."""
    resolved = out.expanduser().resolve()
    apps = applications_dir.expanduser().resolve()
    if apps != resolved and apps not in resolved.parents:
        return False
    try:
        install_guard.assert_writable(out, applications_dir)
    except install_guard.InstallRefused as error:
        raise BindError(str(error)) from error
    return True


def bind(save: Path, out: Path, backup_dir: Path, applications_dir: Path = APPLICATIONS,
         running=game_is_running) -> list[str]:
    save = save.expanduser()
    out = out.expanduser()
    if not save.is_file():
        raise BindError(f"no such save: {save}")
    if out.exists():
        raise BindError(f"{out} already exists")
    if out.resolve() == save.resolve():
        raise BindError("OUT is SAVE; this tool never edits in place")
    if approve_output(out, applications_dir) and running():
        raise BindError("the game is running; quit it before writing into its savegame folder")

    before = check(save)
    players = [b.player for b in before]
    if players[RECORD] != 0:
        raise BindError(f"record 0 is bound to {players[RECORD]}, not 0")
    if MARAUDER_PLAYER in players:
        raise BindError(f"player {MARAUDER_PLAYER} is already bound: {players}")

    original = save.read_bytes()
    saved_copy = backup(save, backup_dir)

    result = subprocess.run(
        [str(BINARY), str(save), str(out), "--record", str(RECORD),
         "--player", str(MARAUDER_PLAYER), "--expect", "0"],
        capture_output=True, text=True)
    if result.returncode != 0:
        if out.exists():
            out.unlink()
        raise BindError(result.stderr.strip() or "save_bind_user failed")

    try:
        edited = out.read_bytes()
        if save.read_bytes() != original:
            raise BindError(f"{save} changed during the run")
        offset = before[RECORD].offset
        if len(edited) != len(original):
            raise BindError(f"length moved {len(original)} -> {len(edited)}")
        moved = [i for i, (a, b) in enumerate(zip(original, edited)) if a != b]
        stray = [i for i in moved if not offset <= i < offset + 4]
        if stray or not moved:
            raise BindError(f"unexpected byte changes: {moved[:16]}")
        if struct.unpack_from("<I", edited, offset)[0] != MARAUDER_PLAYER:
            raise BindError(f"the word at {offset:#x} is not {MARAUDER_PLAYER}")
        after = [b.player for b in check(out)]
        wanted = [MARAUDER_PLAYER] + players[1:]
        if after != wanted:
            raise BindError(f"OUT reads {after}, expected {wanted}")
    except BindError:
        out.unlink()
        raise
    return [
        f"source  {save} ({len(original)} bytes, unchanged)",
        f"backup  {saved_copy} (byte copy, compared)",
        f"before  {players}",
        f"after   {after}",
        f"word    {offset:#x}: 0 -> {MARAUDER_PLAYER}; {len(moved)} byte(s) differ, all inside it",
        f"wrote   {out}",
    ]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p_check = sub.add_parser("check")
    p_check.add_argument("save", type=Path)
    p_bind = sub.add_parser("bind")
    p_bind.add_argument("save", type=Path)
    p_bind.add_argument("out", type=Path)
    p_bind.add_argument("--backup-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "check":
            for binding in check(args.save):
                print(f"record {binding.record}  offset {binding.offset:#x}  "
                      f"player {binding.player}")
        else:
            for line in bind(args.save, args.out, args.backup_dir):
                print(line)
    except BindError as error:
        print(f"REFUSED: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
