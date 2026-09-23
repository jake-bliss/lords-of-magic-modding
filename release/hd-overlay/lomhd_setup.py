#!/usr/bin/env python3
"""Lords of Magic HD art -- build the upscales from YOUR game and install the overlay.

    python lomhd_setup.py                       find the game, build, install
    python lomhd_setup.py --game "C:\\...\\English"
    python lomhd_setup.py --uninstall           put the game back exactly as it was

What it does, in order, and nothing else:

  1. Reads the portraits and building pictures out of your own pic.mpq. No game art ships with
     this mod.
  2. Downloads the upscaler (Real-ESRGAN ncnn Vulkan, MIT) and the 4x-UltraSharp model
     (CC BY-NC-SA 4.0), each checked against a pinned SHA-256 before it is used.
  3. Upscales each picture to 2x with the method picked for it in review (upscale-choices.json):
     character portraits on the approved palette pipeline, everything else in full colour.
  4. Writes lomhd_portraits.pack beside lomse.exe.
  5. Backs up your ddraw.dll to ddraw.dll.lomhd-backup and installs the overlay's ddraw.dll.

Needs Python 3.9+, ImageMagick 7 (`magick` on PATH) and a GPU with Vulkan. Takes 20-60 minutes,
almost all of it step 3. Everything it downloads or makes lives in `lomhd_work` next to this script.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import platform
import shutil
import stat
import subprocess
import sys
import urllib.error
import urllib.request
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))

import hd_portrait_pack  # noqa: E402
import hd_upscale  # noqa: E402
import lbm_png  # noqa: E402
import mpq_read  # noqa: E402

WORK = HERE / "lomhd_work"
PACK_NAME = "lomhd_portraits.pack"
BACKUP_NAME = "ddraw.dll.lomhd-backup"
RECORD_NAME = "lomhd_install.json"
GROUPS = {                       # group -> (folder in pic.mpq, the one size its members have)
    "portrait": ("portrait", (70, 67)),
    "building": ("lbm\\building", None),
}

ESRGAN = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/"
MODELS = ("https://raw.githubusercontent.com/upscayl/upscayl/"
          "6cfaf45b2aae2847cba4f2313b57ca20a0ddd79c/resources/models/")
DOWNLOADS = {
    "windows": (ESRGAN + "realesrgan-ncnn-vulkan-20220424-windows.zip",
                "abc02804e17982a3be33675e4d471e91ea374e65b70167abc09e31acb412802d"),
    "macos": (ESRGAN + "realesrgan-ncnn-vulkan-20220424-macos.zip",
              "e0ad05580abfeb25f8d8fb55aaf7bedf552c375b5b4d9bd3c8d59764d2cc333a"),
    "ultrasharp-4x.param": (MODELS + "ultrasharp-4x.param",
                            "0136ca83686809a8f17f7111f11b951e8db93610e24b7f4137c9ffe4dbc4a806"),
    "ultrasharp-4x.bin": (MODELS + "ultrasharp-4x.bin",
                          "fb3e279d40d4cddb44db4e684d59e68d0aa39852c8cc14dc3f23ccc7e6eee9c1"),
}

STEAM_GUESSES = [
    pathlib.Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    / "Steam/steamapps/common/Lords of Magic Special Edition/English",
    pathlib.Path(os.environ.get("ProgramFiles", r"C:\Program Files"))
    / "Steam/steamapps/common/Lords of Magic Special Edition/English",
]


def say(text: str) -> None:
    print(text, flush=True)


def fail(text: str) -> "NoReturn":  # noqa: F821
    raise SystemExit(f"\nSTOPPED: {text}")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


# --- finding things ------------------------------------------------------------------------------

def find_game(given: pathlib.Path | None) -> pathlib.Path:
    candidates = [given] if given else STEAM_GUESSES
    for folder in candidates:
        if folder and (folder / "lomse.exe").is_file() and (folder / "pic.mpq").is_file():
            return folder.resolve()
    if given:
        fail(f"{given} does not hold lomse.exe and pic.mpq. Point --game at the folder that does "
             "(for Steam: ...\\Lords of Magic Special Edition\\English).")
    fail("could not find the game. Run again with --game \"<folder holding lomse.exe>\".")


def release() -> dict:
    record = json.loads((HERE / "release.json").read_text())
    dll = HERE / "ddraw.dll"
    if not dll.is_file() or sha256(dll) != record["ddraw_sha256"]:
        fail("ddraw.dll next to this script is missing or not the one this release shipped. "
             "Unzip the mod again.")
    return record


def check_magick() -> None:
    try:
        out = subprocess.run(["magick", "-version"], capture_output=True, text=True).stdout
    except FileNotFoundError:
        out = ""
    if "ImageMagick 7" not in out:
        fail("ImageMagick 7 is needed and `magick` was not found on PATH.\n"
             "  Windows: winget install ImageMagick.ImageMagick   (then open a NEW terminal)\n"
             "  macOS:   brew install imagemagick")


# --- the upscaler --------------------------------------------------------------------------------

def fetch(key: str, dest: pathlib.Path) -> pathlib.Path:
    url, want = DOWNLOADS[key]
    if dest.is_file() and sha256(dest) == want:
        return dest
    say(f"  downloading {url.rsplit('/', 1)[-1]} ...")
    part = dest.with_suffix(dest.suffix + ".part")
    try:
        with urllib.request.urlopen(url, timeout=60) as response, part.open("wb") as f:
            shutil.copyfileobj(response, f)
    except (OSError, urllib.error.URLError) as error:
        hint = ""
        if "CERTIFICATE" in str(error).upper() and platform.system() == "Darwin":
            hint = ("\n  macOS with python.org Python: run 'Install Certificates.command' from the "
                    "Python folder in Applications, then try again.")
        fail(f"could not download {url}: {error}{hint}")
    got = sha256(part)
    if got != want:
        part.unlink()
        fail(f"{url} did not match its pinned SHA-256 (got {got}). Nothing was installed.")
    part.replace(dest)
    return dest


def upscaler() -> tuple[pathlib.Path, pathlib.Path]:
    system = platform.system()
    kind = {"Windows": "windows", "Darwin": "macos"}.get(system)
    if kind is None:
        fail(f"no pinned upscaler for {system}. Windows and macOS are supported.")
    tools = WORK / "upscaler"
    tools.mkdir(parents=True, exist_ok=True)
    exe_name = "realesrgan-ncnn-vulkan" + (".exe" if kind == "windows" else "")
    exe = tools / kind / exe_name
    if not exe.is_file():
        archive = fetch(kind, tools / f"{kind}.zip")
        with zipfile.ZipFile(archive) as z:
            z.extractall(tools / kind)
        if not exe.is_file():
            fail(f"{archive.name} did not contain {exe_name}")
    # zipfile drops the executable bit, and macOS quarantines downloads.
    exe.chmod(exe.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    if kind == "macos":
        subprocess.run(["xattr", "-dr", "com.apple.quarantine", str(tools / kind)],
                       capture_output=True)
    models = tools / "models"
    models.mkdir(exist_ok=True)
    for key in ("ultrasharp-4x.param", "ultrasharp-4x.bin"):
        fetch(key, models / key)
    # The animevideo models ship inside the Real-ESRGAN zip itself (checked for both zips).
    for name in hd_upscale.MODEL_FILES:
        if not (models / name).exists():
            bundled = tools / kind / "models" / name
            if not bundled.is_file():
                fail(f"{name} is missing from the Real-ESRGAN download.")
            shutil.copy(bundled, models / name)
    return exe, models


# --- building the pack ---------------------------------------------------------------------------

def extract_images(game: pathlib.Path) -> dict[str, list[str]]:
    """Every image the overlay covers, from the player's own pic.mpq, written to
    lomhd_work/originals/<group>/<name>.lbm, lowercase. Returns group -> names.

    Names come from the list shipped with this mod plus the archive's own (listfile). Two spellings
    of one member (PORTRAIT\\ and portrait\\) resolve to the same hash entry, so they are one."""
    archive = mpq_read.Archive(game / "pic.mpq")
    wanted = (HERE / "overlay-names.txt").read_text().splitlines() + archive.listfile()
    root = WORK / "originals"
    if root.exists():
        shutil.rmtree(root)
    found: dict[str, list[str]] = {group: [] for group in GROUPS}
    seen = set()
    for name in wanted:
        lower = name.strip().lower()
        folder, _, member = lower.rpartition("\\")
        group = next((g for g, (prefix, _) in GROUPS.items() if folder == prefix), None)
        if group is None or not member.endswith(".lbm") or lower in seen or lower not in archive:
            continue
        seen.add(lower)
        stem = member[:-4]
        out = root / group / f"{stem}.lbm"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(archive.read(lower))
        try:
            w, h, *_ = lbm_png.decode(out)
        except Exception:
            out.unlink()
            continue
        size = GROUPS[group][1]
        if (size and (w, h) != size) or not fits_the_overlay(w, h):
            out.unlink()
            continue
        found[group].append(stem)
    stems = [s for names in found.values() for s in names]
    if len(stems) != len(set(stems)):
        fail("two overlay images share a name across folders; this pack format cannot hold both.")
    if not found["portrait"]:
        fail("no portraits found in pic.mpq -- is this Lords of Magic Special Edition?")
    return found


def fits_the_overlay(w: int, h: int) -> bool:
    """What the pack writer would refuse, found before 20-60 minutes of upscaling rather than
    after: a mod install's oversized building is skipped, not a reason to install nothing."""
    p = hd_portrait_pack
    return w >= p.MIN_WIDTH and h >= p.MIN_HEIGHT and 2 * max(w, h) <= p.MAX_UPSCALE_SIDE


def upscale_all(found: dict[str, list[str]], exe: pathlib.Path, models: pathlib.Path) -> list[pathlib.Path]:
    """Each image with the option picked for it in review (upscale-choices.json), or the default
    for images that review never saw. Returns the folders holding the upscales."""
    choices = json.loads((HERE / "upscale-choices.json").read_text())["choices"]
    plan: dict[str, list[tuple[str, str]]] = {}
    for group, stems in found.items():
        for stem in stems:
            choice = choices.get(f"{group}__{stem}") or hd_upscale.default_choice(group, stem)
            if choice == "original":
                continue                          # reviewed: no upscale beat the original
            if choice == hd_upscale.APPROVED and group != "portrait":
                choice = "ultrasharp-tta"         # the palette pipeline is sized for portraits
            plan.setdefault(choice, []).append((group, stem))

    out, pngs = WORK / "upscaled", WORK / "png"
    for stale in (out, pngs):
        # A stale option folder would duplicate names. A stale PNG is worse: it is another
        # install's picture under this install's name, and the pack cannot catch it, because the
        # originals it checks against are this run's. Found by cross-model review, 2026-09-22.
        if stale.exists():
            shutil.rmtree(stale)
    folders = []
    for choice, items in sorted(plan.items()):
        say(f"     {len(items):4d} with {choice}")
        dest = out / choice
        if choice == hd_upscale.APPROVED:
            names_file = WORK / "approved.txt"
            names_file.write_text("".join(f"portrait\\{stem}.lbm\n" for _, stem in items))
            cmd = [sys.executable, str(HERE / "tools" / "upscale.py"), str(WORK / "originals"), str(dest),
                   "--names", str(names_file), "--esrgan", str(exe), "--models", str(models)]
            if subprocess.run(cmd).returncode != 0:
                fail("upscaling did not finish. The game has not been touched.")
            folders.append(dest / "portrait")
        else:
            inputs = {}
            pngs.mkdir(parents=True, exist_ok=True)
            for group, stem in items:
                png = pngs / f"{stem}.png"
                w, h, px, pal, _ = lbm_png.decode(WORK / "originals" / group / f"{stem}.lbm")
                ppm = png.with_suffix(".ppm")
                ppm.write_bytes(f"P6 {w} {h} 255\n".encode() + b"".join(bytes(pal[i]) for i in px))
                subprocess.run(["magick", str(ppm), str(png)], check=True)
                ppm.unlink()
                inputs[stem] = png
            hd_upscale.render(choice, inputs, dest, exe, models)
            folders.append(dest)
    return folders


# --- install / uninstall -------------------------------------------------------------------------

def write_atomically(path: pathlib.Path, data: bytes) -> None:
    """Whole or not at all: an interruption leaves the old file, never a half-written one."""
    part = path.with_name(path.name + ".lomhd-part")
    with part.open("wb") as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    os.replace(part, path)


def read_record(game: pathlib.Path) -> dict:
    """The install record, or {} when there is none or it is damaged. A damaged record is treated
    as missing: install then recovers from the verified backup, as for an interrupted install."""
    try:
        record = json.loads((game / RECORD_NAME).read_text())
        return record if {"ddraw_sha256", "had_ddraw", "backup_sha256"} <= set(record) else {}
    except (OSError, ValueError):
        return {}


def file_hash(path: pathlib.Path) -> str | None:
    return sha256(path) if path.is_file() else None


def check_writable(game: pathlib.Path) -> None:
    """Before the long step, not after it. Windows locks the ddraw.dll a running game has loaded,
    and a copy that fails there would do so after twenty minutes of upscaling."""
    dll = game / "ddraw.dll"
    probe = game / "lomhd_write_test.tmp"
    try:
        probe.write_bytes(b"")
        probe.unlink()
        if dll.is_file():
            with dll.open("r+b"):
                pass
    except OSError:
        fail(f"cannot write to {game}. Close the game (and cnc-ddraw's config tool) and run again; "
             "if it still fails, run the terminal as administrator.")


def install(game: pathlib.Path, pack: bytes, record: dict) -> None:
    """Back up the player's ddraw.dll once, record what was done, then install.

    The record is written BEFORE our DLL is copied, so an interruption at any point leaves either
    the original in place or a record that says how to restore it. Every state a real player can
    reach is recognised rather than refused: a re-run, an upgrade from an older release, a run that
    was interrupted, and Steam putting the original ddraw.dll back ("Verify integrity")."""
    dll, backup, record_path = game / "ddraw.dll", game / BACKUP_NAME, game / RECORD_NAME
    ours = record["ddraw_sha256"]
    previous = read_record(game)
    current, saved = file_hash(dll), file_hash(backup)
    ours_any = {ours, previous.get("ddraw_sha256")} - {None}

    if previous:
        had, backup_sha = previous["had_ddraw"], previous["backup_sha256"]
        restored = had and current == backup_sha        # Steam restored it, or an undo half-ran
        if current not in ours_any and not restored and not (current is None and not had):
            fail("ddraw.dll is neither the overlay's nor your original -- another mod replaced it. "
                 "Left untouched. Remove that mod first, or put your original back by hand.")
        if had and saved != backup_sha:
            if restored:                                # the original is right here: back it up again
                shutil.copy2(dll, backup)
            else:
                fail(f"{BACKUP_NAME} is missing or changed, so your original could not be restored "
                     "later. Nothing was installed.")
    elif saved is not None:
        # A backup with no record: a run interrupted between the backup and the record.
        if current == saved:
            had, backup_sha = True, saved
        elif current in ours_any:
            had, backup_sha = True, saved               # ours was copied; the record was not written
        else:
            fail(f"{backup} already exists and is not a backup of the current ddraw.dll. Move it "
                 "aside by hand so nothing is overwritten.")
    elif current is not None and current not in ours_any:
        shutil.copy2(dll, backup)
        if file_hash(backup) != current:
            fail("the backup of ddraw.dll did not verify. Nothing was installed.")
        had, backup_sha = True, current
    else:
        had, backup_sha = False, None

    write_atomically(record_path, json.dumps({
        "release": record["version"],
        "ddraw_sha256": ours,
        "had_ddraw": had,
        "backup_sha256": backup_sha,
        "pack_sha256": hashlib.sha256(pack).hexdigest(),
        # cnc-ddraw writes a default ddraw.ini on its first run when there is none -- the case on a
        # Windows Steam install, which ships no ddraw.dll at all. Uninstall removes it only then.
        "had_ini": previous.get("had_ini", (game / "ddraw.ini").exists()),
    }, indent=2).encode() + b"\n")
    write_atomically(game / PACK_NAME, pack)
    write_atomically(dll, (HERE / "ddraw.dll").read_bytes())


def uninstall(game: pathlib.Path) -> None:
    record_path = game / RECORD_NAME
    record = read_record(game)
    if not record:
        if record_path.exists() and (game / BACKUP_NAME).is_file():
            fail(f"{RECORD_NAME} is damaged. Run the install again (it recovers from the backup), "
                 "then --uninstall.")
        fail(f"no {RECORD_NAME} in {game}; the overlay does not look installed there.")
    dll, backup = game / "ddraw.dll", game / BACKUP_NAME
    had, backup_sha = record["had_ddraw"], record["backup_sha256"]
    current = file_hash(dll)

    if had and current == backup_sha:
        pass                                            # the original is already back
    elif current == record["ddraw_sha256"]:
        if had:
            if file_hash(backup) != backup_sha:
                fail(f"{BACKUP_NAME} is missing or changed, so the original cannot be restored "
                     "safely. Nothing was removed.")
            shutil.copy2(backup, dll)
        else:
            dll.unlink()
    elif not (current is None and not had):
        fail("ddraw.dll is no longer the overlay's (another mod replaced it). Left as it is.")

    if had and file_hash(backup) == backup_sha:
        backup.unlink()
    # Set aside, never deleted: cnc-ddraw wrote it, but the player may have tuned it since, and a
    # file we cannot prove unmodified is not ours to destroy. The game ignores the renamed copy.
    # (Codex review of 9863bf9: deleting it could lose a player's settings.)
    ini_saved = None
    if record.get("had_ini") is False and (game / "ddraw.ini").is_file():
        ini_saved = game / "ddraw.ini.lomhd-saved"
        n = 1
        while ini_saved.exists():
            n += 1
            ini_saved = game / f"ddraw.ini.lomhd-saved{n}"
        os.replace(game / "ddraw.ini", ini_saved)
    for name in (PACK_NAME, PACK_NAME + ".lomhd-part", "ddraw.dll.lomhd-part",
                 RECORD_NAME + ".lomhd-part", "lomhd.log", RECORD_NAME):
        if (game / name).exists():
            (game / name).unlink()
    say(f"Uninstalled. ddraw.dll is {'your original again' if had else 'removed'}.")
    if ini_saved:
        say(f"cnc-ddraw's settings file was set aside as {ini_saved.name} (the game ignores it; "
            "delete it if you like).")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--game", type=pathlib.Path,
                        help="the folder holding lomse.exe and pic.mpq")
    parser.add_argument("--uninstall", action="store_true")
    args = parser.parse_args()

    if sys.version_info < (3, 9):
        fail("Python 3.9 or newer is needed.")
    game = find_game(args.game)
    say(f"Game: {game}")
    if args.uninstall:
        uninstall(game)
        return 0

    record = release()
    check_magick()
    check_writable(game)
    say("1/4  Getting the upscaler")
    exe, models = upscaler()
    say("2/4  Reading portraits and building pictures from your pic.mpq")
    found = extract_images(game)
    say("     " + ", ".join(f"{len(v)} {k}s" for k, v in found.items()))
    say("3/4  Upscaling (the long step)")
    upscaled = upscale_all(found, exe, models)
    say("4/4  Building the pack and installing")
    originals = [WORK / "originals" / group for group in found if found[group]]
    pack, skipped = hd_portrait_pack.build(originals, upscaled, originals)
    count = len(hd_portrait_pack.read(pack))
    if count == 0:
        fail("the pack came out empty. The game has not been touched.")
    install(game, pack, record)
    say(f"\nDone: {count} HD images installed in {game}.")
    for line in skipped:
        say(f"  left out -- {line}")
    say("To undo: python lomhd_setup.py --uninstall" + (f' --game "{game}"' if args.game else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
