#!/usr/bin/env python3
"""Lords of Magic HD art -- build the upscales from YOUR game and install the overlay.

    python lomhd_setup.py                       find the game, build, install
    python lomhd_setup.py --game "C:\\...\\English"
    python lomhd_setup.py --uninstall           put the game back exactly as it was
    python lomhd_setup.py --review              pick your own upscaler per picture first
    python lomhd_setup.py --sprites             also HD animated sprites (several hours; resumes;
                                                later runs remember it)
    python lomhd_setup.py --no-sprites          back to sprites that do not move only
    python lomhd_setup.py --terrain             also install HD terrain (patches lomse.exe)
    python lomhd_setup.py --terrain --force-terrain-folder
                                                replace a lomhd_terrain folder this mod did not make
    python lomhd_setup.py --report              after a problem: one zip to attach to a GitHub issue
    python lomhd_setup.py --report --with-save latest
                                                also include your newest savegame
    python lomhd_setup.py --report --with-dump  also include crash minidumps, unscrubbed (see below)

--report gathers lomhd.log, ddraw.ini, the install record, this release's own release.json, any
crash/hang .txt files a separate crash reporter wrote, a generated report.txt (OS and Wine detection,
Python and ImageMagick versions, the GPU if it can be read cheaply, what this mod recognises
lomse.exe/ddraw.dll as, the install record, the last setup's own closing summary, and the names and
sizes of the files and folders this mod or the game itself is known to write by exact name (lomse.exe
and its backup, ddraw.dll/ddraw.ini, the game's own archives, this mod's own lomhd_* files, and the
loose files/folders every install ships with -- never a loose prefix or suffix); anything else sitting
in the game folder is only counted, e.g. "+ 3 other files, 2 other folders (names not shown)", since a
name outside that list could be the player's own), and, only with --with-save, one savegame.
Nothing is
uploaded; the zip is written next to this script as lomhd-report-<timestamp>.zip (never overwriting
an earlier one, even from two runs started in the same second). A symlink, or anything that resolves
outside the game folder, is refused wherever a file is chosen for the report. A crash/hang file's own
name is never used inside the zip (only its timestamp, if it has the crash reporter's own shape) --
its filename could itself carry an account or character name; a savegame is renamed to a plain
savegame/save(.lom) for the same reason, and report.txt notes only that one was included, not which.

Every text file above is scrubbed before it goes in the zip: your home folder (replaced with `~`);
`C:\\Users\\<x>`, `C:\\Documents and Settings\\<x>`, `/Users/<x>`, `/home/<x>`, a `\\\\?\\` long-path
prefix, a UNC `\\\\host\\Users\\<x>`, and their Wine `Z:` equivalents, wherever they appear (with or
without a trailing slash) and not only your own account's; and your account name as a whole word. Text
that cannot be confidently decoded (including legacy Windows text -- UTF-8 is tried first, then
cp1252, rather than silently mangling non-ASCII characters scrub() could then never match) is left out
rather than copied in unscrubbed. Crash *minidumps* (`lomhd_crash_*.dmp`) hold paths as UTF-16 inside
a binary format text scrubbing cannot safely see into at all, so they are left out by default --
--with-dump includes them exactly as written, unscrubbed. **A savegame is binary too and is never
scrubbed: it may contain your in-game names. Only use --with-save if you are happy to share it.**

Choosing your own: --review renders every upscale option for every picture from your own game,
then opens a review page on this computer (http://127.0.0.1:8765) with the shipped picks already
selected. Change any you like; they are saved to my-upscale-choices.json next to this script, and
the next plain run uses that file instead of the shipped upscale-choices.json. Delete it to go back
to the shipped picks. Rendering every option takes several times longer than an install.

What it does, in order, and nothing else:

  1. Downloads the upscaler (Real-ESRGAN ncnn Vulkan, MIT) and the 4x-UltraSharp model
     (CC BY-NC-SA 4.0), each checked against a pinned SHA-256 before it is used.
  2. Reads the portraits and building pictures out of your own pic.mpq, and the sprites (map
     buildings, trees, units, spell effects) out of your own imp.mpq. No game art ships with this mod.
  3. Upscales each picture to 2x with the method picked for it in review (upscale-choices.json):
     character portraits on the approved palette pipeline, everything else in full colour. Kept in
     lomhd_work, so a later run makes only what is new: a changed picture, or a changed pick.
  4. Upscales each sprite that does not move (one frame) the same way, with its own pick, and the
     unit figures the army strip shows (the nine unit icon sheets, ~150 frames). With
     --sprites, also every frame of every animated sprite, each with its sprite's pick: several
     hours, cached in lomhd_work/sprites, so a run stopped part-way carries on where it was.
  5. Writes lomhd_portraits.pack beside lomse.exe, backs up your ddraw.dll to
     ddraw.dll.lomhd-backup and installs the overlay's ddraw.dll. Then, if lomse.exe is the one
     this mod knows, backs it up to lomse.exe.lomhd-backup and fixes a bug in it (one byte, checked
     before it is written): the game could hang when a Death Shade or Frozen Shade died in combat.
     Any other lomse.exe is left as it is.

With --terrain, after those five steps (and only if they succeeded):

  6. Reads the terrain atlases and their .til files out of your pic.mpq and upscales every tile to
     2x, quantized back to its atlas's own palette (tools/terrain_hd.py).
  7. Writes them to lomhd_terrain/til beside lomse.exe. pic.mpq is not changed. Then backs up
     lomse.exe to lomse.exe.lomhd-backup (if step 5 has not) and patches it (65 same-size edits
     and step 5's fix, every one checked before any is written) so the terrain is drawn at 2x.
     Last, so an interrupted run never leaves a patched game without its art.

Needs Python 3.9+, ImageMagick 7 (`magick` on PATH) and a GPU with Vulkan. Takes 20-60 minutes,
almost all of it steps 3 and 4 (with --sprites, several hours). Everything it downloads or makes
lives in `lomhd_work` next to this script.
"""
from __future__ import annotations

import argparse
import datetime
import getpass
import hashlib
import itertools
import json
import os
import pathlib
import platform
import re
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))

import exe_patch  # noqa: E402
import hd_portrait_pack  # noqa: E402
import hd_sprites  # noqa: E402
import hd_upscale  # noqa: E402
import lbm_png  # noqa: E402
import mpq_read  # noqa: E402
import terrain_hd  # noqa: E402

WORK = HERE / "lomhd_work"
PACK_NAME = "lomhd_portraits.pack"
BACKUP_NAME = "ddraw.dll.lomhd-backup"
RECORD_NAME = "lomhd_install.json"
SHIPPED_CHOICES = HERE / "upscale-choices.json"
MY_CHOICES = HERE / "my-upscale-choices.json"
IMP_NAMES = HERE / "imp-names.txt"
# DEVELOPER AID, not for players: build only the first N animated sprites (by name), so an
# end-to-end run of --sprites can finish in minutes. Unset, every animated sprite is built.
SPRITE_LIMIT_ENV = "LOMHD_DEV_SPRITE_LIMIT"

# HD terrain (--terrain). The exe half and the art half are installed and removed as a pair: the
# DLL serves lomhd_terrain only to the patched exe, so a stock exe beside a leftover folder is
# harmless, but a patched exe WITHOUT the folder draws scrambled terrain.
EXE_NAME = "lomse.exe"
EXE_BACKUP_NAME = "lomse.exe.lomhd-backup"
TERRAIN_DIR = "lomhd_terrain"
TERRAIN_SETS = ("terrain-hybrid-2x", "terrain-stride-1024")     # exe_patches/<name>.json
# A vanilla bug fix (Skarn's: the Shade death hang), applied on EVERY install, with or without
# --terrain. It needs no art, so unlike the terrain sets it is safe on its own.
FIX_SETS = ("fix-mirror-narrow",)
# GS5R3 lomse.exe, the binary the sets were derived from, and what applying them to it gives: the
# fix alone, and the terrain sets with the fix. Both are recomputed from the pristine bytes on every
# install and must agree; the constants are what recognise an already-patched exe when there are no
# pristine bytes to hand.
PRISTINE_EXE_SHA256 = "a505f399d5be73fe0a2215633f663717f28daeb3075bbcc05b47d40653669052"
FIXED_EXE_SHA256 = "7b6a104fd56c7d134402d472065ff103d143a6291d717489053c46e0c0d8c641"
PATCHED_EXE_SHA256 = "f70ac994d9d5b83d387429ec8eeeb95c07ac6e23f60bac8fc34f0e2c8f75ecf1"
# Terrain exes earlier releases wrote (0.4.0-0.5.0: the terrain sets without the fix). Still ours,
# even when the install record that names them is gone.
EARLIER_PATCHED_EXE_SHA256S = ("ddb438837c47f5299fe5b74e4c1de9c31fcbc7b218179c69ba043b9b93d0bee2",)
TERRAIN_STRIDE = 1024            # terrain-stride-1024: EVERY sampled atlas must be this wide
TERRAIN_TILE = 64                # TILESIZE after doubling
SHORT_IN_THE_ORIGINAL = {"jeff01.lbm": 2 * 480}   # jeff01.til declares 16 rows over a 480-tall atlas
GROUPS = {                       # group -> (folder in pic.mpq, the one size its members have)
    "portrait": ("portrait", (70, 67)),
    "building": ("lbm\\building", None),
    "keep": ("keeps", None),
    "screen": ("lbm", None),
    "panel": ("lbm\\panels", None),
    "sky": ("lbm\\skies", None),
    "library": ("library", None),
}
PLURAL = {"sky": "skies", "library": "library pages"}
MIN_COLOURS = 16                 # a picture plainer than this is found anywhere, and costs every frame

# --report: one zip a player can attach to a GitHub issue. Opt-in by nature (nothing leaves the
# machine on its own); the fixed set of files it may hold is spelled out at GAME_TEXT_FILES /
# CRASH_TEXT_GLOBS / CRASH_DUMP_GLOB rather than a directory walk, so it can never sweep up a save, a
# mod, or game art by accident.
SUMMARY_NAME = "lomhd_last_summary.txt"     # this run's closing report, read back by the next --report
ISSUES_URL = "https://github.com/jake-bliss/lords-of-magic-modding/issues"
SAVE_DIR_NAME = "savegame"
# Every one of these is TEXT and goes into the zip scrubbed (see scrub()), from the game folder if
# present. release.json (from HERE, not the game folder) is added to the zip the same way.
GAME_TEXT_FILES = ("lomhd.log", "ddraw.ini", RECORD_NAME)
# A crash reporter (built separately, in the DLL) is expected to write these; it may not exist yet on
# a given machine, which is not a reason for --report to fail. The .txt files are text and scrubbed
# like everything else; .dmp minidumps hold UTF-16 paths a text scrub cannot safely see inside, so
# they are left out by default (--with-dump includes them exactly as written, unscrubbed). Paired with
# a "kind" used for the zip's own member names, never the player's original filename -- see
# crash_arcname: a crash reporter could plausibly put an account or character name in its filename.
CRASH_TEXT_GLOBS = (("lomhd_crash_*.txt", "crash"), ("lomhd_hang_*.txt", "hang"))
CRASH_DUMP_GLOB = ("lomhd_crash_*.dmp", "crash")
REPORT_MAX_PER_GLOB = 5
DUMP_SIZE_CAP = 20 * 1024 * 1024            # a minidump can run far larger than a bug report should
HASH_FILES = ("lomse.exe", "ddraw.dll", "gs.mpq", "pic.mpq", "imp.mpq")

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


def magick_version(timeout: "float | None" = None) -> str:
    """`magick -version`'s own stdout, or "" if it is not on PATH or (with `timeout`) hangs. Install
    and --review call this with no timeout, as before; --report passes one, since a report must
    finish even when `magick` itself is broken on this machine."""
    try:
        return subprocess.run(["magick", "-version"], capture_output=True, text=True,
                              timeout=timeout).stdout
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return ""


def windows_magick_dirs() -> "list[str]":
    """Where a just-installed ImageMagick is on Windows. A console keeps the PATH it opened with, so
    right after `winget install` the new folder is only in the registry (a tester hit this: setup
    said ImageMagick was missing until the console was reopened). Also the default install folder,
    in case the installer did not add it to PATH at all."""
    import winreg  # Windows only; imported here so the module still loads everywhere else
    dirs: "list[str]" = []
    for root, key in ((winreg.HKEY_LOCAL_MACHINE,
                       r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
                      (winreg.HKEY_CURRENT_USER, "Environment")):
        try:
            with winreg.OpenKey(root, key) as handle:
                value = winreg.QueryValueEx(handle, "Path")[0]
        except OSError:
            continue
        dirs += [os.path.expandvars(d.strip().strip('"')) for d in value.split(";")
                 if "imagemagick" in d.lower()]
    for base in {os.environ.get("ProgramFiles", r"C:\Program Files"), r"C:\Program Files"}:
        dirs += sorted((str(p) for p in pathlib.Path(base).glob("ImageMagick-7*")), reverse=True)
    return [d for d in dirs if (pathlib.Path(d) / "magick.exe").is_file()]


def check_magick() -> None:
    out = magick_version()
    if "ImageMagick 7" not in out and os.name == "nt":
        # Put it on this process's PATH: every later `magick` call (here and in tools/) inherits it.
        found = windows_magick_dirs()
        if found:
            os.environ["PATH"] = os.pathsep.join(found + [os.environ.get("PATH", "")])
            out = magick_version()
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
    of one member (PORTRAIT\\ and portrait\\) resolve to the same hash entry, so they are one.

    The folder is kept between runs: a file is written only when this game's member differs from
    it, and anything that is not one of this run's pictures is removed, so the folder holds exactly
    this install's pictures either way."""
    archive = mpq_read.Archive(game / "pic.mpq")
    wanted = (HERE / "overlay-names.txt").read_text().splitlines() + archive.listfile()
    root = WORK / "originals"
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
        data = archive.read(lower)
        if not (out.is_file() and out.read_bytes() == data):
            out.write_bytes(data)
        try:
            w, h, px, *_ = lbm_png.decode(out)
        except Exception:
            out.unlink()
            continue
        size = GROUPS[group][1]
        # A flat picture (all black, one colour of sky) has probe slices that match any flat part
        # of any frame; each hit then costs a full comparison. Nothing to sharpen in it anyway.
        if (size and (w, h) != size) or not fits_the_overlay(w, h) or len(set(px)) < MIN_COLOURS:
            out.unlink()
            continue
        found[group].append(stem)
    # The pack keys pictures by name alone. Two folders holding one name (portrait\\black.lbm and
    # lbm\\black.lbm in the shipped archives) keep the first group's and report the other, rather
    # than stopping every install over one picture. (Claude review, 2026-09-23.)
    claimed: set[str] = set()
    for group in GROUPS:
        for stem in list(found[group]):
            if stem in claimed:
                found[group].remove(stem)
                (root / group / f"{stem}.lbm").unlink()
                say(f"     left out {GROUPS[group][0]}\\{stem}.lbm: another folder has a picture of that name")
            claimed.add(stem)
    # Another install's pictures (or a removed mod's) must not stay: the pack and upscale.py read
    # every file here as this game's.
    for folder in list(root.iterdir()) if root.is_dir() else []:
        keep = {f"{stem}.lbm" for stem in found.get(folder.name, [])} if folder.is_dir() else set()
        for stale in list(folder.iterdir()) if folder.is_dir() else [folder]:
            if stale.name not in keep:
                shutil.rmtree(stale) if stale.is_dir() else stale.unlink()
    if not found["portrait"]:
        fail("no portraits found in pic.mpq -- is this Lords of Magic Special Edition?")
    return found


def fits_the_overlay(w: int, h: int) -> bool:
    """What the pack writer would refuse, found before 20-60 minutes of upscaling rather than
    after: a mod install's oversized building is skipped, not a reason to install nothing."""
    p = hd_portrait_pack
    return w >= p.MIN_WIDTH and h >= p.MIN_HEIGHT and 2 * max(w, h) <= p.MAX_UPSCALE_SIDE


def choices_file() -> pathlib.Path:
    """The player's own picks when they made some with --review, else the shipped ones."""
    return MY_CHOICES if MY_CHOICES.is_file() else SHIPPED_CHOICES


def lbm_to_png(lbm: pathlib.Path, png: pathlib.Path) -> None:
    w, h, px, pal, _ = lbm_png.decode(lbm)
    ppm = png.with_suffix(".ppm")
    ppm.write_bytes(f"P6 {w} {h} 255\n".encode() + b"".join(bytes(pal[i]) for i in px))
    subprocess.run(["magick", str(ppm), f"PNG:{png}"], check=True, env=hd_upscale.magick_env())
    ppm.unlink()


def upscale_output(choice: str, stem: str) -> pathlib.Path:
    """Where upscale_all keeps one picture's upscale by one option."""
    out = WORK / "upscaled" / choice
    return out / "portrait" / f"{stem}.lbm" if choice == hd_upscale.APPROVED else out / f"{stem}.png"


def upscale_recipe() -> dict:
    """Everything an upscale depends on besides its original and its option: kept beside the
    upscales, which are all made again when it changes (a new release's models or resize)."""
    return json.loads(json.dumps({"recipe": hd_upscale.RECIPE, "options": hd_upscale.OPTIONS,
                                  "downloads": sorted(sha for _, sha in DOWNLOADS.values())}))


def upscale_all(found: dict[str, list[str]], exe: pathlib.Path, models: pathlib.Path,
                choices_path: pathlib.Path | None = None) -> list[pathlib.Path]:
    """Each image with the option picked for it in review, or the default for images that review
    never saw. Returns the folders holding the upscales. A rerun makes only the upscales that are
    missing: see below for what is dropped first."""
    choices = json.loads((choices_path or choices_file()).read_text())["choices"]
    plan: dict[str, list[tuple[str, str]]] = {}
    for group, stems in found.items():
        for stem in stems:
            choice = choices.get(f"{group}__{stem}") or hd_upscale.default_choice(group, stem)
            if choice == "original":
                continue                          # reviewed: no upscale beat the original
            if choice == hd_upscale.APPROVED and group != "portrait":
                choice = "ultrasharp-tta"         # the palette pipeline is sized for portraits
            plan.setdefault(choice, []).append((group, stem))

    picked = {stem: choice for choice, items in plan.items() for _, stem in items}

    # Kept between runs; until 2026-09-30 both folders were deleted every run, and a rerun with
    # nothing new re-rendered all ~1,281 pictures. What that deletion guarded against still holds:
    # a stale PNG is another install's picture under this install's name, and the pack cannot catch
    # it, because the originals it checks against are this run's (cross-model review, 2026-09-22).
    # So each picture's original is kept beside its PNG (png/<name>.lbm), compared by its bytes as
    # render_review does, and a picture whose original differs loses its PNG and every upscale.
    out, pngs = WORK / "upscaled", WORK / "png"
    stamp = out / "recipe.json"
    try:
        same_recipe = json.loads(stamp.read_text()) == upscale_recipe()
    except (OSError, ValueError):
        same_recipe = False
    if out.exists() and not same_recipe:
        shutil.rmtree(out)
    out.mkdir(parents=True, exist_ok=True)
    pngs.mkdir(parents=True, exist_ok=True)
    stamp.write_text(json.dumps(upscale_recipe()))
    # upscale.py's scratch, left by a run that was stopped: it would stop the next one.
    shutil.rmtree(out / hd_upscale.APPROVED / ".work", ignore_errors=True)
    current = set()
    for group, stems in found.items():
        for stem in stems:
            current.add(stem)
            lbm, kept = WORK / "originals" / group / f"{stem}.lbm", pngs / f"{stem}.lbm"
            if kept.is_file() and kept.read_bytes() == lbm.read_bytes():
                continue
            (pngs / f"{stem}.png").unlink(missing_ok=True)
            for option in [p.name for p in out.iterdir() if p.is_dir()]:
                upscale_output(option, stem).unlink(missing_ok=True)
            shutil.copyfile(lbm, kept)
    # An upscale by an option that is no longer its picture's pick (or of a picture this install
    # does not have) goes too: a stale option folder would duplicate names in the pack.
    for folder in [p for p in out.iterdir() if p.is_dir()]:
        option, suffix = folder.name, (".lbm" if folder.name == hd_upscale.APPROVED else ".png")
        files = folder / "portrait" if option == hd_upscale.APPROVED else folder
        for path in list(files.iterdir()) if files.is_dir() else []:
            if not (path.is_file() and path.name.endswith(suffix)
                    and picked.get(path.name[:-len(suffix)]) == option):
                shutil.rmtree(path) if path.is_dir() else path.unlink()
    for path in list(pngs.iterdir()):
        stem, _, ext = path.name.rpartition(".")
        if not (ext in ("png", "lbm") and stem in current and path.is_file()):
            shutil.rmtree(path) if path.is_dir() else path.unlink()

    def make_png(item: "tuple[str, str]") -> None:
        group, stem = item
        part = pngs / f"{stem}.part"                  # a PNG only appears whole
        lbm_to_png(WORK / "originals" / group / f"{stem}.lbm", part)
        os.replace(part, pngs / f"{stem}.png")

    folders = []
    for choice, items in sorted(plan.items()):
        todo = [(group, stem) for group, stem in items if not upscale_output(choice, stem).exists()]
        say(f"     {len(items):4d} with {choice}"
            + (f" ({len(items) - len(todo)} already made)" if len(todo) < len(items) else ""))
        dest = out / choice
        if choice == hd_upscale.APPROVED:
            if todo:
                names_file = WORK / "approved.txt"
                names_file.write_text("".join(f"portrait\\{stem}.lbm\n" for _, stem in todo))
                cmd = [sys.executable, str(HERE / "tools" / "upscale.py"), str(WORK / "originals"), str(dest),
                       "--names", str(names_file), "--esrgan", str(exe), "--models", str(models)]
                if subprocess.run(cmd).returncode != 0:
                    fail("upscaling did not finish. The game has not been touched.")
            (dest / "portrait").mkdir(parents=True, exist_ok=True)
            folders.append(dest / "portrait")
        else:
            # Every picked picture keeps its PNG, made or not this run: the pack's content check
            # makes a damaged upscale again from it (rerender_picture).
            hd_upscale.thread_map(make_png, [(g, s) for g, s in items if not (pngs / f"{s}.png").exists()])
            dest.mkdir(parents=True, exist_ok=True)
            if todo:
                hd_upscale.render(choice, {stem: pngs / f"{stem}.png" for _, stem in todo}, dest, exe, models)
            folders.append(dest)
    return folders


def render_review(found: dict[str, list[str]], exe: pathlib.Path, models: pathlib.Path) -> pathlib.Path:
    """Every option for every picture, in lomhd_work/review, laid out as the review page reads it:
    original/<group>__<name>.png and one folder per option. Resumable. A picture whose original
    differs from the one already there (another install, a mod) has its old renders removed."""
    review = WORK / "review"
    originals = review / "original"
    originals.mkdir(parents=True, exist_ok=True)
    options = [*hd_upscale.OPTIONS, hd_upscale.APPROVED]
    inputs: dict[str, dict[str, pathlib.Path]] = {}
    characters = []
    changed = []
    for group, stems in found.items():
        for stem in stems:
            key = f"{group}__{stem}"
            png = originals / f"{key}.png"
            lbm = WORK / "originals" / group / f"{stem}.lbm"
            # Compared by the picture's own bytes, kept beside its PNG: ImageMagick stamps every
            # PNG it writes with the time, so comparing PNGs made every run look changed and
            # re-render everything. (Claude review, 2026-09-23.)
            kept = originals / f"{key}.lbm"
            if not (png.exists() and kept.exists() and kept.read_bytes() == lbm.read_bytes()):
                changed.append((key, lbm))
            inputs.setdefault(group, {})[key] = png
            if hd_upscale.default_choice(group, stem) == hd_upscale.APPROVED:
                characters.append(stem)

    def refresh(item: "tuple[str, pathlib.Path]") -> None:
        key, lbm = item
        for option in options:
            (review / option / f"{key}.png").unlink(missing_ok=True)
        part = originals / f"{key}.part"          # not *.png: the page lists those
        lbm_to_png(lbm, part)
        os.replace(part, originals / f"{key}.png")
        shutil.copyfile(lbm, originals / f"{key}.lbm")

    hd_upscale.thread_map(refresh, changed)
    # Pictures from an install reviewed before (or a mod since removed) are not this game's: off the
    # page, or a pick could be saved for art this install never installs. (Codex review.)
    current = {key for batch in inputs.values() for key in batch}
    for folder in [originals, *(review / option for option in options)]:
        for stale in folder.glob("*.*") if folder.is_dir() else []:
            if stale.stem not in current:
                stale.unlink()
    total = sum(len(v) for v in inputs.values())
    for option in hd_upscale.OPTIONS:
        say(f"     {option}: {total} pictures")
        for group, batch in inputs.items():
            hd_upscale.render(option, batch, review / option, exe, models)
    approved = review / hd_upscale.APPROVED
    todo = [s for s in characters if not (approved / f"portrait__{s}.png").exists()]
    if todo:
        say(f"     {hd_upscale.APPROVED}: {len(todo)} character portraits")
        names = WORK / "review-approved.txt"
        names.write_text("".join(f"portrait\\{stem}.lbm\n" for stem in todo))
        lbms = WORK / "review-approved"
        cmd = [sys.executable, str(HERE / "tools" / "upscale.py"), str(WORK / "originals"), str(lbms),
               "--names", str(names), "--esrgan", str(exe), "--models", str(models)]
        if subprocess.run(cmd).returncode != 0:
            fail("upscaling did not finish. The game has not been touched.")
        approved.mkdir(parents=True, exist_ok=True)
        for stem in todo:
            lbm_to_png(lbms / "portrait" / f"{stem}.lbm", approved / f"portrait__{stem}.png")
    return review


def serve_review(review: pathlib.Path, port: int) -> None:
    """The review page, on this computer only, until Ctrl+C. Picks save as they are made. The
    browser opens only once the server has said it is listening; if it cannot start (the port is
    taken, say), setup stops and says so rather than showing whatever else answers there."""
    url = f"http://127.0.0.1:{port}"
    cmd = [sys.executable, str(HERE / "tools" / "serve.py"), "--renders", str(review),
           "--port", str(port), "--choices", str(MY_CHOICES), "--seed", str(SHIPPED_CHOICES)]
    server = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
    first = server.stdout.readline()
    if not first.startswith(url):
        server.wait()
        fail(f"the review page could not start on port {port} (is something else using it? "
             "try --port 8766).")
    say(f"\nReview page: {url}  (Ctrl+C here when you are done)")
    say(f"Your picks are saved to {MY_CHOICES.name}; the next plain run installs with them.")
    try:
        import webbrowser
        webbrowser.open(url)
    except Exception:
        pass
    try:
        server.wait()
    except KeyboardInterrupt:
        server.terminate()


# --- sprites -------------------------------------------------------------------------------------

def check_imp(game: pathlib.Path) -> None:
    """The sprites come from imp.mpq, which every Lords of Magic Special Edition install has."""
    if not (game / "imp.mpq").is_file():
        fail(f"{game} has no imp.mpq, which the HD sprites are made from. Is this Lords of Magic "
             "Special Edition? Nothing was installed.")


def sprite_choices() -> dict:
    """The upscaler for each sprite: the shipped picks, with any sprite__ picks the player saved to
    my-upscale-choices.json on top. Per sprite, like terrain_choices, so a file saved before sprites
    were built (or with only some of them) keeps the shipped pick for every other sprite."""
    choices = json.loads(SHIPPED_CHOICES.read_text())["choices"]
    if MY_CHOICES.is_file():
        mine = json.loads(MY_CHOICES.read_text())["choices"]
        choices.update({k: v for k, v in mine.items() if k.startswith("sprite__")})
    return {k: v for k, v in choices.items() if k.startswith("sprite__")}


def plan_sprites(game: pathlib.Path, animated: bool):
    """(plan, work folder, read_sprite) for the sprites of the player's own imp.mpq: which members
    the shipped names resolve to (imp.mpq has no listfile), and which frames can be packed, their
    upscaler inputs written. Work goes under lomhd_work/sprites, keyed per member by its path and
    its own bytes: another install's (or a mod's) version of a member is never reused, and a changed
    imp.mpq re-renders only the members that changed. Work for members that are gone is removed.
    A member that cannot be read is left out, never a reason to stop."""
    archive = mpq_read.Archive(game / "imp.mpq")
    read_sprite = hd_sprites.archive_reader(archive)
    root = WORK / "sprites"
    found = hd_sprites.resolve(archive, IMP_NAMES)
    resolved, skipped = found.resolved, found.skipped
    root.mkdir(parents=True, exist_ok=True)
    # Every member still present keeps its work, planned this run or not: a static-only run must
    # never cost a --sprites install its hours of animated renders.
    hd_sprites.prune(root, found.live)
    limit = os.environ.get(SPRITE_LIMIT_ENV)
    if animated and limit:
        # The unit icon sheets are built on every run; the limit counts only the other animated sprites.
        keep = sorted(n for n, (_, frames) in resolved.items()
                      if frames > 1 and n not in hd_sprites.STRIP_SHEETS)[:int(limit)]
        resolved = {n: v for n, v in resolved.items() if v[1] == 1 or n in keep or n in hd_sprites.STRIP_SHEETS}
        say(f"     {SPRITE_LIMIT_ENV}={limit}: only {len(keep)} animated sprites, plus the unit icon sheets "
            "(a developer aid)")
    plan = hd_sprites.plan(resolved, read_sprite, sprite_choices(), root, animated=animated)
    plan.skipped[:0] = skipped
    return plan, root, read_sprite


def upscale_sprites(sprites: list, root: pathlib.Path, exe: pathlib.Path, models: pathlib.Path,
                    again: str = "python lomhd_setup.py") -> None:
    def render(option, inputs, dest):
        hd_upscale.render(option, inputs, dest, exe, models)
    try:
        hd_sprites.render_all(sprites, root, render, log=lambda line: say(f"     {line}"))
    except (SystemExit, subprocess.CalledProcessError) as error:
        fail(f"upscaling sprites stopped: {error}\nThe game has not been touched. Run the same "
             f"command again ({again}) to carry on: every sprite already upscaled is kept.")


def rerender_picture(path: pathlib.Path, exe: pathlib.Path, models: pathlib.Path) -> None:
    """The content check's second chance for one picture: its upscale made again, in place, by the
    option that made it -- the folder it is in (upscale_all's layout)."""
    folder = path.parent
    path.unlink(missing_ok=True)
    if folder.parent.name == hd_upscale.APPROVED:            # upscaled/approved/portrait/<name>.lbm
        names = WORK / "approved-again.txt"
        names.write_text(f"{folder.name}\\{path.name}\n")
        subprocess.run([sys.executable, str(HERE / "tools" / "upscale.py"), str(WORK / "originals"),
                        str(folder.parent), "--names", str(names), "--esrgan", str(exe),
                        "--models", str(models)], check=True, capture_output=True)
    else:                                                    # upscaled/<option>/<name>.png
        hd_upscale.render(folder.name, {path.stem: WORK / "png" / f"{path.stem}.png"}, folder, exe, models)


def build_pack(pack: pathlib.Path, sprites, sprite_root: pathlib.Path, read_sprite, originals: list,
               upscaled: list, exe: "pathlib.Path | None" = None, models: "pathlib.Path | None" = None,
               pictures: "dict | None" = None):
    """Write the pack: static sprites, then each animated sprite as its own consecutive group, then
    the pictures. Returns (images, pictures left out, sprites left out, static counts, animated
    counts); the counts hold "packed" frames and "sprites", and, as `pictures` (if given) does for
    the pictures, "damaged" upscales the content check found and "remade" ones that then passed.
    With the upscaler (`exe`, `models`), a damaged upscale is made again once before it is left out."""
    skipped: list = []
    sprite_skipped = list(sprites.skipped)
    packed: dict = {}
    moving: dict = {}
    remake = None if exe is None else (
        lambda option, inputs, dest: hd_upscale.render(option, inputs, dest, exe, models))
    remake_picture = None if exe is None else (lambda path: rerender_picture(path, exe, models))
    # The content check and zlib, per frame and per picture, on worker processes; batches grow with
    # them, so each still has about one READ_BUDGET of pixels to work on. The pack is the same, byte
    # for byte, as with LOMHD_JOBS=1 (tests/test_lomhd_setup.py).
    workers = hd_upscale.jobs()
    sized = {"batch": hd_sprites.RENDER_BATCH * workers, "budget": hd_sprites.READ_BUDGET * workers}
    with hd_upscale.process_pool() as pool:
        count = hd_portrait_pack.write_records(pack, itertools.chain(
            hd_sprites.records(sprites.static, sprite_root, read_sprite, sprite_skipped, packed, rerender=remake,
                               log=lambda line: say(f"     {line}"), pool=pool, **sized),
            hd_sprites.records(sprites.animated, sprite_root, read_sprite, sprite_skipped, moving, rerender=remake,
                               log=lambda line: say(f"     {line}"), pool=pool, **sized),
            hd_portrait_pack.unmasked_records(originals, upscaled, skipped, originals, rerender=remake_picture,
                                              counts=pictures, pool=pool,
                                              budget=hd_portrait_pack.READ_BUDGET * workers)))
    packed.setdefault("packed", 0)
    moving.setdefault("packed", 0)
    moving.setdefault("sprites", 0)
    moving.setdefault("strip", 0)
    return count, skipped, sprite_skipped, packed, moving


def retry_command(args: argparse.Namespace, animated: bool, game: pathlib.Path) -> str:
    """The command that carries on after a failed run. It spells out the sprite mode this run is in:
    the install record is only written once a run finishes, so a bare retry could fall back to the
    previous install's mode (turning animated sprites back on after --no-sprites, say)."""
    return " ".join(["python lomhd_setup.py"]
                    + (["--sprites"] if animated else ["--no-sprites"] if args.no_sprites else [])
                    + (["--terrain"] if args.terrain else [])
                    + (["--force-terrain-folder"] if args.force_terrain_folder else [])
                    + ([f'--game "{game}"'] if args.game else []))


def sprite_mode(game: pathlib.Path, on: bool, off: bool) -> "tuple[bool, str]":
    """(build animated sprites?, why) from --sprites / --no-sprites and the install record: a plain
    run keeps whatever the last install had, so re-running setup (after --review, say) never drops
    the animated sprites a --sprites run spent hours on."""
    if on:
        return True, "on (--sprites)"
    if off:
        return False, "off (--no-sprites)"
    if read_record(game).get("sprites"):
        return True, "on, remembered from your last install (--no-sprites turns them off)"
    return False, "off (--sprites adds them)"


SKIP_KINDS = (                  # (what a skip reason says, how the summary counts it)
    ("looked damaged", "upscale looked damaged"),
    ("not in this archive", "not in your imp.mpq"),
    ("ambiguous", "two sprites share the name"),
    ("picked 'original'", "the original was picked in review"),
    ("no usable upscale pick", "no pick"),
    ("character limit", "name too long for the overlay"),
    ("no 8-pixel run", "too plain for the overlay to find"),
    ("render", "no usable upscale"),
    ("could not read", "could not be read"),
)


def damage_summary(counts: list, skipped: list) -> str:
    """What the content check did this run, for the end of the install. `counts` are the builders'
    ("damaged", "remade", "failed", "unjudged"); `skipped` their left-out lines."""
    found, remade, failed, unjudged = (sum(c.get(k, 0) for c in counts)
                                       for k in ("damaged", "remade", "failed", "unjudged"))
    left = sum("looked damaged" in line and "could not be made again" not in line for line in skipped)
    small = f" ({unjudged} too small to check)" if unjudged else ""
    if not found:
        return f"No upscale looked damaged{small}."
    parts = [f"{remade} came out clean"]
    if left:
        parts.append(f"{left} still looked damaged and were left out")
    if failed:
        parts.append(f"{failed} could not be made again and were left out")
    return (f"{found} upscales looked damaged and were made again: {', '.join(parts)}"
            + (" (the original shows for those; running setup again tries them once more)"
               if left or failed else "") + f".{small}")


def summarise_skips(skipped: list) -> str:
    kinds: dict = {}
    for line in skipped:
        kind = next((label for text, label in SKIP_KINDS if text in line), "too small or too large")
        kinds[kind] = kinds.get(kind, 0) + 1
    return ", ".join(f"{n} {kind}" for kind, n in sorted(kinds.items(), key=lambda kv: -kv[1]))


# --- install / uninstall -------------------------------------------------------------------------

def write_atomically(path: pathlib.Path, data: "bytes | pathlib.Path") -> None:
    """Whole or not at all: an interruption leaves the old file, never a half-written one. `data`
    is the bytes, or a file to copy them from (the pack is ~850 MB with full-screen art)."""
    part = path.with_name(path.name + ".lomhd-part")
    with part.open("wb") as f:
        if isinstance(data, pathlib.Path):
            with data.open("rb") as src:
                shutil.copyfileobj(src, f, 1 << 20)
        else:
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


def check_writable(game: pathlib.Path, terrain: bool = False) -> None:
    """Before the long step, not after it. Windows locks the ddraw.dll a running game has loaded
    (and, for --terrain, its lomse.exe), and a copy that fails there would do so after twenty
    minutes of upscaling."""
    dll = game / "ddraw.dll"
    probe = game / "lomhd_write_test.tmp"
    try:
        probe.write_bytes(b"")
        probe.unlink()
        for held in [dll] + ([game / EXE_NAME] if terrain else []):
            if held.is_file():
                with held.open("r+b"):
                    pass
    except OSError:
        fail(f"cannot write to {game}. Close the game (and cnc-ddraw's config tool) and run again; "
             "if it still fails, run the terminal as administrator.")


def install(game: pathlib.Path, pack: "bytes | pathlib.Path", record: dict, sprites: bool = False) -> None:
    """Back up the player's ddraw.dll once, record what was done, then install.

    The record is written BEFORE our DLL is copied, so an interruption at any point leaves either
    the original in place or a record that says how to restore it. Every state a real player can
    reach is recognised rather than refused: a re-run, an upgrade from an older release, a run that
    was interrupted, and Steam putting the original ddraw.dll back ("Verify integrity")."""
    dll, backup, record_path = game / "ddraw.dll", game / BACKUP_NAME, game / RECORD_NAME
    ours = record["ddraw_sha256"]
    previous = read_record(game)
    current, saved = file_hash(dll), file_hash(backup)
    ours_any = {ours, previous.get("ddraw_sha256"), *previous.get("overlay_sha256s", [])} - {None}

    if previous:
        had, backup_sha = previous["had_ddraw"], previous["backup_sha256"]
        restored = had and current == backup_sha        # Steam restored it, or an undo half-ran
        if current not in ours_any and not restored and not (current is None and not had):
            fail("ddraw.dll is neither the overlay's nor your original -- another mod replaced it. "
                 "Left untouched. Remove that mod first, or put your original back by hand.")
        if had and saved != backup_sha:
            if restored:                                # the original is right here: back it up again
                write_atomically(backup, dll)
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
        write_atomically(backup, dll)                   # whole or not at all, like the exe's
        if file_hash(backup) != current:
            fail("the backup of ddraw.dll did not verify. Nothing was installed.")
        had, backup_sha = True, current
    else:
        had, backup_sha = False, None

    write_atomically(record_path, json.dumps({
        "release": record["version"],
        "ddraw_sha256": ours,
        # Every overlay DLL this game has had. An upgrade interrupted after this record but before
        # the new DLL is copied leaves the OLD overlay DLL in place; without its hash here, both a
        # re-run and --uninstall took it for another mod's. (Codex review, 2026-09-23.)
        "overlay_sha256s": sorted(ours_any),
        "had_ddraw": had,
        "backup_sha256": backup_sha,
        "pack_sha256": sha256(pack) if isinstance(pack, pathlib.Path) else hashlib.sha256(pack).hexdigest(),
        # cnc-ddraw writes a default ddraw.ini on its first run when there is none -- the case on a
        # Windows Steam install, which ships no ddraw.dll at all. Uninstall removes it only then.
        "had_ini": previous.get("had_ini", (game / "ddraw.ini").exists()),
        # Whether this pack holds the animated sprites (--sprites). A later plain run keeps them
        # rather than quietly dropping hours of work; --no-sprites turns them off.
        "sprites": sprites,
        # Kept across a plain re-run: the terrain is still installed, and uninstall reads this.
        **({"terrain": previous["terrain"]} if "terrain" in previous else {}),
        # Likewise the patched lomse.exe (fix_exe), which the steps after this one may not change.
        **({"exe": previous["exe"]} if "exe" in previous else {}),
    }, indent=2).encode() + b"\n")
    write_atomically(game / PACK_NAME, pack)
    write_atomically(dll, (HERE / "ddraw.dll").read_bytes())


def uninstall(game: pathlib.Path, force_terrain_folder: bool = False) -> None:
    record_path = game / RECORD_NAME
    record = read_record(game)
    if not record:
        if record_path.exists() and (game / BACKUP_NAME).is_file():
            fail(f"{RECORD_NAME} is damaged. Run the install again (it recovers from the backup), "
                 "then --uninstall.")
        fail(f"no {RECORD_NAME} in {game}; the overlay does not look installed there.")
    # A running game holds ddraw.dll and lomse.exe: say so before anything is changed, rather than
    # stopping halfway with a traceback. lomse.exe only matters when it is one this mod wrote, the
    # only case uninstall writes it.
    check_writable(game, terrain=file_hash(game / EXE_NAME) in our_exe_hashes(record))
    terrain_removed = uninstall_terrain(game, record, force_terrain_folder)
    dll, backup = game / "ddraw.dll", game / BACKUP_NAME
    had, backup_sha = record["had_ddraw"], record["backup_sha256"]
    current = file_hash(dll)

    if had and current == backup_sha:
        pass                                            # the original is already back
    elif current in {record["ddraw_sha256"], *record.get("overlay_sha256s", [])}:
        if had:
            if file_hash(backup) != backup_sha:
                fail(f"{BACKUP_NAME} is missing or changed, so the original cannot be restored "
                     "safely. Nothing was removed.")
            write_atomically(dll, backup)
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
                 RECORD_NAME + ".lomhd-part", "lomhd.log", RECORD_NAME, SUMMARY_NAME,
                 SUMMARY_NAME + ".lomhd-part"):
        if (game / name).exists():
            (game / name).unlink()
    say(f"Uninstalled. ddraw.dll is {'your original again' if had else 'removed'}.")
    if terrain_removed:
        say(terrain_removed)
    if ini_saved:
        say(f"cnc-ddraw's settings file was set aside as {ini_saved.name} (the game ignores it; "
            "delete it if you like).")


# --- HD terrain (--terrain) ----------------------------------------------------------------------

def load_sets(names: "tuple[str, ...]") -> list:
    """Patch sets as shipped (JSON: tomllib is Python 3.11+ and setup promises 3.9), loaded through
    exe_patch's own validation. Every one must target the one binary this release knows."""
    sets = []
    for name in names:
        path = HERE / "exe_patches" / f"{name}.json"
        if not path.is_file():
            fail(f"{path.name} is missing from exe_patches. Unzip the mod again.")
        try:
            loaded = exe_patch.load_set(path)
        except (exe_patch.PatchError, KeyError, ValueError) as error:
            fail(f"{path.name} is damaged ({error}). Unzip the mod again.")
        if loaded.sha256 != PRISTINE_EXE_SHA256:
            fail(f"{path.name} targets a different lomse.exe than this release. Unzip the mod again.")
        sets.append(loaded)
    return sets


def terrain_sets() -> list:
    return load_sets(TERRAIN_SETS)


def terrain_exe_hashes(record: dict) -> set:
    """Every lomse.exe this mod wrote that holds the terrain edits, this release's and earlier ones'."""
    terrain = record.get("terrain", {})
    return {PATCHED_EXE_SHA256, *EARLIER_PATCHED_EXE_SHA256S, terrain.get("exe_patched_sha256"),
            *terrain.get("exe_patched_sha256s", [])} - {None}


def our_exe_hashes(record: dict) -> set:
    """Every lomse.exe this mod wrote: those, and the ones holding only the fix."""
    exe = record.get("exe", {})
    return terrain_exe_hashes(record) | {FIXED_EXE_SHA256, exe.get("patched_sha256"),
                                         *exe.get("patched_sha256s", [])} - {None}


def exe_plan(game: pathlib.Path, terrain: bool) -> "tuple[bytes, bytes, str] | str":
    """(pristine bytes, patched bytes, patched sha256) for this game's lomse.exe, or why not.

    The exe must be the pristine GS5R3 binary, or one this mod patched (a re-run or an upgrade), in
    which case the pristine bytes come from the verified backup. Anything else -- another patch, a
    different version -- gets nothing. The patch is the terrain sets and the fix with `terrain`, or
    when the exe already holds the terrain (a plain re-run after --terrain keeps it: the terrain is
    still installed); otherwise the fix alone."""
    exe, backup = game / EXE_NAME, game / EXE_BACKUP_NAME
    record = read_record(game)
    current, saved = file_hash(exe), file_hash(backup)
    if saved is not None and saved != PRISTINE_EXE_SHA256:
        return (f"{EXE_BACKUP_NAME} exists and is not the original lomse.exe. Move it aside by hand so "
                "nothing is overwritten.")
    if current == PRISTINE_EXE_SHA256:
        pristine = exe.read_bytes()
    elif current in our_exe_hashes(record):
        if saved is None:
            return (f"lomse.exe is already patched by this mod but {EXE_BACKUP_NAME} is missing, so the "
                    "original could not be restored later. Put the original back (Steam: Verify "
                    "integrity of game files) and run again.")
        pristine = backup.read_bytes()
    else:
        return ("lomse.exe is not the one this mod was made for (Lords of Magic Special Edition with "
                "the GS5R3 patch) and not one this mod patched -- another patch or a different version "
                "may have changed it.")
    if terrain or current in terrain_exe_hashes(record):
        sets, want = terrain_sets() + load_sets(FIX_SETS), PATCHED_EXE_SHA256
    else:
        sets, want = load_sets(FIX_SETS), FIXED_EXE_SHA256
    try:
        patched = exe_patch.apply(pristine, sets)
    except exe_patch.PatchError as error:
        return f"the patch does not apply to this lomse.exe: {error}"
    if hashlib.sha256(patched).hexdigest() != want:
        return "patching lomse.exe gave an unexpected result."
    return pristine, patched, want


def terrain_exe_plan(game: pathlib.Path) -> tuple[bytes, bytes]:
    """(pristine bytes, patched bytes) for --terrain, or a refusal naming why not, before anything
    is touched."""
    plan = exe_plan(game, terrain=True)
    if isinstance(plan, str):
        fail(f"{plan} HD terrain was not installed; lomse.exe was not touched.")
    return plan[0], plan[1]


def record_exe(record: dict, patched_sha: str) -> None:
    """Note in `record` the lomse.exe about to be written, among every one this game has had from
    us (as overlay_sha256s does for the DLL): an upgrade stopped before the exe is written leaves
    the previous one, which must still be recognised as ours. The caller writes the record."""
    previous = record.get("exe", {})
    record["exe"] = {
        "original_sha256": PRISTINE_EXE_SHA256,
        "patched_sha256": patched_sha,
        "patched_sha256s": sorted({patched_sha, *previous.get("patched_sha256s", [])}),
    }
    if "terrain" in record and patched_sha == PATCHED_EXE_SHA256:
        terrain = record["terrain"]
        terrain["exe_patched_sha256"] = patched_sha
        terrain["exe_patched_sha256s"] = sorted({patched_sha, *terrain.get("exe_patched_sha256s", [])})


def write_exe(game: pathlib.Path, patched: bytes, patched_sha: str, again: str, soft: bool = False) -> None:
    """Back lomse.exe up once (written beside itself, renamed, verified), then write the patched one
    the same way and verify it. exe_plan has checked that a missing backup means a pristine exe.
    `soft`: a write the OS refuses raises OSError (see write_game_file); a result that does not
    verify still stops the run."""
    exe, backup = game / EXE_NAME, game / EXE_BACKUP_NAME
    if file_hash(backup) is None:
        write_game_file(backup, exe, again, "lomse.exe was not touched.", soft)
        if file_hash(backup) != PRISTINE_EXE_SHA256:
            backup.unlink()
            fail("the backup of lomse.exe did not verify. lomse.exe was not touched.")
    if file_hash(exe) != patched_sha:
        write_game_file(exe, patched, again, "lomse.exe was not changed.", soft)
        if file_hash(exe) != patched_sha:
            fail("the patched lomse.exe did not verify. Run --uninstall to put the original back.")


def fix_exe(game: pathlib.Path, again: str = "") -> str:
    """Step 5, every install: FIX_SETS on lomse.exe, or the exe left alone. Returns what to tell the
    player. An exe this mod cannot patch is not a refusal -- the overlay works on any lomse.exe --
    it only means no fix. Record, backup, exe, in that order, as for --terrain."""
    exe = game / EXE_NAME
    plan = exe_plan(game, terrain=False)
    record = read_record(game)
    current = file_hash(exe)
    if isinstance(plan, str) and current in (FIXED_EXE_SHA256, PATCHED_EXE_SHA256):
        # The fix is already in: only the backup is not what it should be.
        if current == FIXED_EXE_SHA256:
            return (f"lomse.exe already has the Shade crash fix. {EXE_BACKUP_NAME} is missing or not the "
                    "original, so it was left as it is; --uninstall rebuilds the original from "
                    "lomse.exe itself.")
        return (f"lomse.exe already has the Shade crash fix (and HD terrain), but {EXE_BACKUP_NAME} is "
                "missing or not the original, so --uninstall cannot put the original back. Steam's "
                "'Verify integrity of game files' can.")
    if isinstance(plan, str) or not record:
        why = plan if isinstance(plan, str) else f"{RECORD_NAME} is missing or damaged."
        return f"The Shade crash fix was not applied: {why} lomse.exe was left as it is."
    _, patched, patched_sha = plan
    if current == patched_sha and file_hash(game / EXE_BACKUP_NAME) == PRISTINE_EXE_SHA256:
        pass                                    # already done: nothing to write, nor to probe
    elif not exe_writable(game):
        return ("The Shade crash fix was not applied: lomse.exe is read-only (the Shade crash fix "
                "needs to change it). lomse.exe was left as it is.")
    record_exe(record, patched_sha)
    write_atomically(game / RECORD_NAME, json.dumps(record, indent=2).encode() + b"\n")
    try:
        write_exe(game, patched, patched_sha, again, soft=True)
    except OSError:
        return ("The Shade crash fix was not applied: could not write lomse.exe (is the game "
                "running?). Close Lords of Magic and run setup again to apply it.")
    return (f"lomse.exe fixed (the Shade crash; the original is {EXE_BACKUP_NAME})"
            + (", HD terrain kept." if patched_sha == PATCHED_EXE_SHA256 else "."))


def exe_writable(game: pathlib.Path) -> bool:
    """Whether lomse.exe can be opened for writing: not read-only, not held by a running game."""
    try:
        with (game / EXE_NAME).open("r+b"):
            return True
    except OSError:
        return False


def terrain_names() -> list[str]:
    """The til\\*.lbm and til\\*.til members terrain_hd builds from. pic.mpq has no listfile, so
    the names ship with the mod (terrain-names.txt), the way overlay-names.txt does."""
    return [n.strip() for n in (HERE / "terrain-names.txt").read_text().splitlines() if n.strip()]


def terrain_choices() -> dict[str, str]:
    """The upscaler for each atlas: the shipped picks, with any terrain__ picks the player saved to
    my-upscale-choices.json on top. Per atlas, so a file saved before terrain existed (no terrain__
    keys) still builds with the shipped ones."""
    choices = json.loads(SHIPPED_CHOICES.read_text())["choices"]
    if MY_CHOICES.is_file():
        mine = json.loads(MY_CHOICES.read_text())["choices"]
        choices.update({k: v for k, v in mine.items() if k.startswith("terrain__")})
    return {k: v for k, v in choices.items() if k.startswith("terrain__")}


def extract_terrain(game: pathlib.Path) -> pathlib.Path:
    """Every name in terrain-names.txt, from the player's own pic.mpq, into lomhd_work/terrain/src."""
    archive = mpq_read.Archive(game / "pic.mpq")
    src = WORK / "terrain" / "src"
    if src.exists():
        shutil.rmtree(src)
    src.mkdir(parents=True)
    names = terrain_names()
    missing = [n for n in names if n.lower() not in archive]
    if missing:
        fail(f"pic.mpq has no {', '.join(missing[:5])}{' ...' if len(missing) > 5 else ''}. HD "
             "terrain needs the terrain tilesets of Lords of Magic Special Edition. The HD art is "
             "installed; lomse.exe was not touched.")
    for name in names:
        (src / name.lower().rpartition("\\")[2]).write_bytes(archive.read(name.lower()))
    return src


def lbm_size(path: pathlib.Path) -> tuple[int, int]:
    data = path.read_bytes()
    if data[:4] != b"FORM" or data[8:12] not in (b"PBM ", b"ILBM"):
        raise ValueError(f"{path.name}: not an LBM")
    i = 12
    while i + 8 <= len(data):
        tag, n = data[i:i + 4], int.from_bytes(data[i + 4:i + 8], "big")
        if tag == b"BMHD":
            return int.from_bytes(data[i + 8:i + 10], "big"), int.from_bytes(data[i + 10:i + 12], "big")
        i += 8 + n + (n & 1)
    raise ValueError(f"{path.name}: no BMHD")


def check_terrain_art(src: pathlib.Path, out: pathlib.Path) -> None:
    """mods/terrain-hd-art/rebuild.sh's checks, in Python: every .til and every atlas present,
    TILESIZE 64, every atlas exactly its .til's TILES grid at 64px and exactly 2x its original, and
    every atlas 1024 wide. With the stride patch in the exe, one atlas left at 512 renders sheared
    garbage, so a build that fails any of these is not installed."""
    names = [n.lower().rpartition("\\")[2] for n in terrain_names()]
    want_tils = sorted(n for n in names if n.endswith(".til"))
    want_lbms = sorted(n for n in names if n.endswith(".lbm") and n[:-4] not in terrain_hd.NOT_TEXTURES)
    tils = sorted(p.name for p in out.glob("*.til"))
    lbms = sorted(p.name for p in out.glob("*.lbm"))
    problems = []
    if tils != want_tils or lbms != want_lbms:
        problems.append(f"built {len(tils)} .til and {len(lbms)} atlases, want {len(want_tils)} and "
                        f"{len(want_lbms)}")
    for til in tils:
        text = (out / til).read_bytes().decode("latin-1")
        lbm = re.search(r"^LBM=\s*(\S+)", text, re.M | re.I)
        size = re.search(r"TILESIZE=\s*(\d+),\s*(\d+)", text)
        grid = re.search(r"TILES=\s*(\d+),\s*(\d+)", text)
        if not (lbm and size and grid):
            problems.append(f"{til}: no LBM=, TILESIZE= or TILES=")
            continue
        name = lbm[1].strip().lower()
        if name not in lbms:
            problems.append(f"{til} names {name}, which was not built")
            continue
        if (int(size[1]), int(size[2])) != (TERRAIN_TILE, TERRAIN_TILE):
            problems.append(f"{til}: TILESIZE {size[1]},{size[2]}")
        # The rasterizer reads TILES x 64 texels. jeff01.til declares 16 rows over a 480-tall
        # atlas, so it is held to exactly 2x its own height instead.
        w, h = lbm_size(out / name)
        want = (int(grid[1]) * TERRAIN_TILE, SHORT_IN_THE_ORIGINAL.get(name, int(grid[2]) * TERRAIN_TILE))
        if (w, h) != want:
            problems.append(f"{til}: {name} is {w}x{h}, want {want[0]}x{want[1]}")
    for name in lbms:
        w, h = lbm_size(out / name)
        ow, oh = lbm_size(src / name) if (src / name).is_file() else (0, 0)
        if w != TERRAIN_STRIDE:
            problems.append(f"{name}: {w} wide -- every atlas must be {TERRAIN_STRIDE}")
        if (w, h) != (2 * ow, 2 * oh):
            problems.append(f"{name}: {w}x{h} is not twice the original {ow}x{oh}")
    if problems:
        fail("the HD terrain build did not check out, so it was not installed (the HD art is; "
             "lomse.exe was not touched):\n  " + "\n  ".join(problems))


def build_terrain(game: pathlib.Path, esrgan: pathlib.Path, models: pathlib.Path) -> pathlib.Path:
    """The 2x atlases and doubled .til files, built and checked in lomhd_work. Touches nothing in
    the game folder. Tiles and renders are reused on a rerun (terrain_hd keys them by content)."""
    src = extract_terrain(game)
    out = WORK / "terrain" / "til"
    if out.exists():
        shutil.rmtree(out)
    try:
        report = terrain_hd.build(src, out, esrgan, models, WORK / "terrain" / "work", terrain_choices())
    except (SystemExit, subprocess.CalledProcessError) as error:
        # CalledProcessError: ImageMagick refused a file, most likely a damaged tile or render left
        # in the cache by an earlier run that was stopped hard.
        fail(f"building HD terrain stopped: {error}\nThe HD art is installed; lomse.exe was not "
             f"touched. If it stops again, delete {WORK / 'terrain'} (in lomhd_work, next to this "
             "script) and run again: it is only a cache.")
    for line in report:
        if "skipped" in line:
            say(f"     {line}")
    check_terrain_art(src, out)
    return out


def terrain_digest(til: pathlib.Path) -> str | None:
    """One hash for the art folder: every file's name and bytes. None when there is no folder, or
    when it holds anything but plain files (a folder the player added) -- no digest this mod wrote
    can match that, so it reads as not ours rather than failing. (Codex review.)"""
    if not til.is_dir():
        return None
    digest = hashlib.sha256()
    for path in sorted(til.iterdir()):
        if path.is_symlink() or not path.is_file():
            return None
        digest.update(path.name.encode() + b"\0" + sha256(path).encode() + b"\n")
    return digest.hexdigest()


def terrain_folder_owned(game: pathlib.Path, record: dict) -> bool:
    """Whether lomhd_terrain is exactly a folder this mod wrote: nothing in it but til, and til's
    digest one the install record holds. A folder the player edited, or put there by hand, is not."""
    dest = game / TERRAIN_DIR
    terrain = record.get("terrain", {})
    ours = {terrain.get("terrain_sha256"), *terrain.get("terrain_sha256s", [])} - {None}
    return (dest.is_dir() and {p.name for p in dest.iterdir()} == {"til"}
            and terrain_digest(dest / "til") in ours)


def check_terrain_folder(game: pathlib.Path, record: dict, force: bool) -> None:
    """Refuse to replace a lomhd_terrain this mod cannot show it made, unless told to."""
    dest = game / TERRAIN_DIR
    if (dest.exists() or dest.is_symlink()) and not force and not terrain_folder_owned(game, record):
        fail(f"{dest} is already there and is not the one this mod installed (it was changed since, "
             "or put there by hand), so it was not replaced. Move it out of the game folder, or run "
             "again with --force-terrain-folder to replace it. lomse.exe was not touched.")


def recover_terrain_swap(game: pathlib.Path, again: str) -> None:
    """A run stopped between moving the old lomhd_terrain aside and renaming the new one into place
    leaves lomhd_terrain.lomhd-old and no lomhd_terrain: a patched exe with no art, which draws
    scrambled terrain. Put the old folder back before anything else."""
    dest, old = game / TERRAIN_DIR, game / (TERRAIN_DIR + ".lomhd-old")
    if old.is_dir() and not (dest.exists() or dest.is_symlink()):
        try:
            os.replace(old, dest)
        except OSError:
            fail(f"an earlier run left {old.name}, and it could not be put back as {TERRAIN_DIR}. "
                 f"Close Lords of Magic and anything else using the game folder, then run "
                 f"python lomhd_setup.py {again} again.")
        say(f"     put back {TERRAIN_DIR} from a run that was interrupted")


def write_game_file(path: pathlib.Path, data: "bytes | pathlib.Path", again: str, state: str,
                    soft: bool = False) -> None:
    """write_atomically, for a file the running game holds: a locked file becomes a message saying
    what to do, not a traceback, and the half-written .lomhd-part goes. `soft` raises the OSError
    instead (after that cleanup), for a caller that carries on without the file."""
    try:
        write_atomically(path, data)
    except OSError as error:
        try:
            path.with_name(path.name + ".lomhd-part").unlink(missing_ok=True)
        except OSError:
            pass
        if soft:
            raise
        command = " ".join(["python lomhd_setup.py", again]).strip()
        fail(f"could not write {path.name} ({error.strerror or error}). Close Lords of Magic (and "
             f"anything else using the game folder) and run {command} again. {state}")


def install_terrain(game: pathlib.Path, built: pathlib.Path, force_folder: bool = False) -> None:
    """Record, then the art folder, then the exe -- the exe LAST, so an interrupted run leaves at
    worst the art without the patch, which the stock exe ignores.

    The art goes in whole or not at all: it is copied to a sibling folder and renamed into place,
    and the folder it replaces is only deleted once the new one is there. A lomhd_terrain this mod
    cannot show it made is refused unless `force_folder`. The exe is backed up once (written beside
    itself, renamed, verified), then written the same way and verified again."""
    again = "--terrain"
    recover_terrain_swap(game, again)
    _, patched = terrain_exe_plan(game)
    dest = game / TERRAIN_DIR
    digest = terrain_digest(built)
    record = read_record(game)
    if not record:
        fail(f"{RECORD_NAME} is missing or damaged after the HD art install. HD terrain was not "
             "installed; lomse.exe was not touched.")
    check_terrain_folder(game, record, force_folder)
    previous = record.get("terrain", {})
    record["terrain"] = {
        "exe_original_sha256": PRISTINE_EXE_SHA256,
        "exe_patched_sha256": PATCHED_EXE_SHA256,
        # Every patched exe this game has had, as overlay_sha256s does for the DLL: an upgrade whose
        # patch differs must still recognise the old one as ours.
        "exe_patched_sha256s": sorted({PATCHED_EXE_SHA256, *previous.get("exe_patched_sha256s", [])}),
        "terrain_sha256": digest,
        # Every art folder this game has had from us. The record is written before the folder is
        # swapped, so a run stopped in between must still know the folder left in place as ours.
        "terrain_sha256s": sorted({digest, *previous.get("terrain_sha256s", []),
                                   *([previous["terrain_sha256"]] if previous.get("terrain_sha256") else [])}),
    }
    record_exe(record, PATCHED_EXE_SHA256)
    write_atomically(game / RECORD_NAME, json.dumps(record, indent=2).encode() + b"\n")

    if terrain_digest(dest / "til") != digest or {p.name for p in dest.iterdir()} != {"til"}:
        # (A re-run with the same art leaves the folder as it is: nothing to replace.)
        part, old = game / (TERRAIN_DIR + ".lomhd-part"), game / (TERRAIN_DIR + ".lomhd-old")
        # After recover_terrain_swap, an `old` still here has lomhd_terrain beside it: the newer one.
        for stale in (part, old):
            if stale.exists():
                shutil.rmtree(stale)
        shutil.copytree(built, part / "til")
        if terrain_digest(part / "til") != digest:
            shutil.rmtree(part)
            fail("the copy of the HD terrain did not verify. lomse.exe was not touched.")
        locked = (f"could not put the new {TERRAIN_DIR} in place (something has a file in it open). "
                  f"Close Lords of Magic and anything else using the game folder, then run "
                  f"python lomhd_setup.py {again} again.")
        # Windows cannot rename a folder over another, so the old one steps aside first -- and comes
        # back if the new one cannot take its place: a patched exe without the folder draws
        # scrambled terrain.
        if dest.exists():
            try:
                os.replace(dest, old)
            except OSError:
                shutil.rmtree(part, ignore_errors=True)
                fail(f"{locked} The HD terrain already installed was left as it was.")
        try:
            os.replace(part, dest)
        except BaseException as error:
            restored = not old.exists()
            if old.exists() and not dest.exists():
                try:
                    os.replace(old, dest)
                    restored = True
                except OSError:
                    pass                    # recover_terrain_swap puts it back on the next run
            shutil.rmtree(part, ignore_errors=True)
            if isinstance(error, OSError):
                fail(f"{locked} " + ("The HD terrain already installed was left as it was."
                                     if restored else "The next run puts the previous one back."))
            raise
        if old.exists():
            shutil.rmtree(old, ignore_errors=True)   # the new folder is in place; a leftover is inert

    write_exe(game, patched, PATCHED_EXE_SHA256, again)


def unfixed(image: bytes) -> "bytes | None":
    """`image` with FIX_SETS reverted (each site's `new` bytes back to `old`), or None when a site
    does not hold its `new` bytes. The caller verifies the result's hash."""
    try:
        secs = exe_patch.sections(image)
        out = bytearray(image)
        for patch_set in load_sets(FIX_SETS):
            for p in patch_set.patches:
                off = exe_patch.va_to_offset(secs, p.va, len(p.new))
                if out[off:off + len(p.new)] != p.new:
                    return None
                out[off:off + len(p.old)] = p.old
        return bytes(out)
    except (exe_patch.PatchError, struct.error):
        return None


def terrain_edits_present(image: bytes) -> bool:
    """Whether ANY site of this mod's terrain patch holds its patched (`new`) bytes. An exe nobody
    recognises is safe beside lomhd_terrain only when none do: the folder then does nothing."""
    try:
        secs = exe_patch.sections(image)
    except (exe_patch.PatchError, struct.error):
        return False
    for patch_set in terrain_sets():
        for p in patch_set.patches:
            try:
                off = exe_patch.va_to_offset(secs, p.va, len(p.new))
            except exe_patch.PatchError:
                continue
            if image[off:off + len(p.new)] == p.new:
                return True
    return False


def uninstall_terrain(game: pathlib.Path, record: dict, force_folder: bool = False) -> str | None:
    """Put the original lomse.exe back, then remove lomhd_terrain. Returns what to tell the player,
    or None when there was nothing to undo.

    Hash discipline as for ddraw.dll: the exe is only replaced when it is the one this mod wrote,
    and only from a backup that verifies. If Steam's "Verify integrity" already restored it, the
    backup is simply dropped. An exe something else changed is left alone: if it still holds this
    mod's edits, the folder is still needed and nothing is removed; if it holds none, the folder is
    inert. Either way the folder is only deleted when it is the one this mod installed (or
    `force_folder`): a stock exe ignores it, so leaving a foreign one is safe."""
    again = "--uninstall"
    recover_terrain_swap(game, again)
    exe, backup, dest = game / EXE_NAME, game / EXE_BACKUP_NAME, game / TERRAIN_DIR
    terrain = record.get("terrain", {})
    original = terrain.get("exe_original_sha256",
                           record.get("exe", {}).get("original_sha256", PRISTINE_EXE_SHA256))
    ours = our_exe_hashes(record)
    current, saved = file_hash(exe), file_hash(backup)
    leftovers = ([game / (TERRAIN_DIR + s) for s in (".lomhd-part", ".lomhd-old")]
                 + [game / (EXE_NAME + ".lomhd-part"), game / (EXE_BACKUP_NAME + ".lomhd-part")])
    if not (terrain or saved is not None or current in ours or dest.exists()
            or any(p.exists() for p in leftovers)):
        return None

    exe_note = "lomse.exe is your original again"
    had_terrain = bool(terrain) or dest.exists() or dest.is_symlink()
    foreign_exe = restored = False
    if current == FIXED_EXE_SHA256 and saved != original:
        # A fix-only exe needs no backup: its one edit is reverted and the result verified.
        rebuilt = unfixed(exe.read_bytes())
        if rebuilt is None or hashlib.sha256(rebuilt).hexdigest() != original:
            fail(f"{EXE_BACKUP_NAME} is missing or changed, and the original lomse.exe could not be "
                 "rebuilt from lomse.exe. Nothing was removed. Steam's 'Verify integrity of game "
                 "files' puts the original back; then run --uninstall again.")
        write_game_file(exe, rebuilt, again, "Nothing was removed.")
        if file_hash(exe) != original:
            fail("the rebuilt lomse.exe did not verify. Nothing else was removed.")
        restored = True
    elif current in ours or current is None:
        if saved != original:
            fail(f"{EXE_BACKUP_NAME} is missing or changed, so the original lomse.exe cannot be "
                 "restored safely. Nothing was removed. Steam's 'Verify integrity of game files' "
                 "puts the original back; then run --uninstall again.")
        write_game_file(exe, backup, again, "Nothing was removed.")
        if file_hash(exe) != original:
            fail("the restored lomse.exe did not verify. Nothing else was removed.")
        restored = True
    elif current != original:
        if terrain_edits_present(exe.read_bytes()):
            fail("lomse.exe is neither this mod's patched one nor your original, but it still holds "
                 "this mod's terrain edits -- something else has changed it on top of them. Left as "
                 "it is, and nothing was removed (the edits still need lomhd_terrain). Steam's "
                 "'Verify integrity of game files' puts the original back; then run --uninstall "
                 "again.")
        foreign_exe = True
        exe_note = "lomse.exe was changed by something else since, and was left as it is"

    if foreign_exe and backup.exists():
        say(f"{EXE_BACKUP_NAME} (your original lomse.exe) was kept, since lomse.exe itself has been "
            "replaced; delete it if you do not need it.")
    elif file_hash(backup) == original:
        backup.unlink()
    elif backup.exists():
        say(f"{EXE_BACKUP_NAME} is not the original lomse.exe, so it was left where it is.")

    folder_note = f"{TERRAIN_DIR} is gone"
    if dest.exists() or dest.is_symlink():
        if force_folder or terrain_folder_owned(game, record):
            if dest.is_dir() and not dest.is_symlink():
                shutil.rmtree(dest)
            else:
                dest.unlink()
        else:
            folder_note = (f"{TERRAIN_DIR} was left in place: it is not the one this mod installed "
                           "(changed since, or put there by hand). The game ignores it now; delete it "
                           "if you do not want it")
    for path in leftovers:
        if path.is_dir():
            shutil.rmtree(path)
        elif path.exists():
            path.unlink()
    if not had_terrain:                         # only the fix was installed
        if restored:
            return f"The Shade crash fix was removed: {exe_note}."
        if foreign_exe:
            return f"The Shade crash fix: {exe_note}."
        return "lomse.exe was already your original (Steam may have put it back); its backup was removed."
    return f"HD terrain removed: {exe_note}, and {folder_note}."


# --- --report --------------------------------------------------------------------------------------

# Windows/Wine home-folder shapes that can carry an account name even when this Python is not running
# as that account: a crash written under a different Windows profile, or under Wine, whose Z: drive is
# the whole host filesystem (so a Mac/Linux home shows up as Z:\Users\<x>\ or Z:\home\<x>\ too), or a
# UNC path (\\server\Users\<x>\...). A long-path \\?\ prefix is optional in front of a drive or UNC
# root.
#
# The account segment can hold spaces ("Alice Smith"), so where it ends depends on what comes after
# it on the same line: if a slash follows anywhere later, the segment is everything up to that slash,
# spaces and all (tried first, below); only when no slash follows at all -- the path is just sitting
# in prose with nothing after it -- does the segment stop at the first space, quote, or punctuation
# (the second alternative, tried only once the first cannot match anywhere on the line).
_WIN_ACCOUNT_SEGMENT = r'(?:[^\r\n]+?(?=[\\/])|[^\\/\r\n]+?(?=$|["\'\s,;:)\]]))'
_POSIX_ACCOUNT_SEGMENT = r'(?:[^\r\n]+?(?=/)|[^/\r\n]+?(?=$|["\'\s,;:)\]]))'
_HOME_PATH_RE = re.compile(
    r"(?:\\\\\?\\)?"                                    # optional \\?\ long-path prefix
    r"(?:[A-Za-z]:|\\\\[^\\/\r\n]+)"                    # a drive letter, or a UNC \\host
    r"[\\/]+(?:Users|Documents and Settings|home)"      # ... or a bare POSIX form, below
    r"[\\/]+(" + _WIN_ACCOUNT_SEGMENT + r")"
    r"|/(?:Users|home)/(" + _POSIX_ACCOUNT_SEGMENT + r")",
    re.IGNORECASE | re.MULTILINE,
)


def _blank_matched_segment(match: "re.Match") -> str:
    """`match.group(0)` with only its captured account-name span replaced (group 1 or 2, whichever
    this alternative used) -- the path's own separators and prefix are kept."""
    whole = match.group(0)
    group = 1 if match.group(1) is not None else 2
    start, end = match.start(group) - match.start(0), match.end(group) - match.start(0)
    return whole[:start] + "<user>" + whole[end:]


def scrub(text: str) -> str:
    """`text` with anything naming this machine's account removed, so a report attached to a public
    GitHub issue never carries who ran it:

    - this Python's own home folder, replaced with `~`;
    - `C:\\Users\\<x>`, `C:\\Documents and Settings\\<x>`, `/Users/<x>`, `/home/<x>`, their Wine `Z:`
      equivalents, a `\\\\?\\` long-path prefix, and a UNC `\\\\host\\Users\\<x>` (see `_HOME_PATH_RE`)
      -- these can name an account this process is not running as, so they are checked regardless of
      what `pathlib.Path.home()` says, whether or not a separator follows the name;
    - this process's own account name (`$USER`/`$USERNAME`/`$LOGNAME`/`getpass.getuser()`), as a
      whole word only: `(?<![A-Za-z0-9])name(?![A-Za-z0-9])`. Because every character beside a 64-hex
      sha256 digest is itself alphanumeric, a whole-word match can never land inside one or split it;
      a short, common account name (`lom`, `ada`) can still coincide with an unrelated whole word or
      filename stem that is not the account at all (`lom.cfg`) -- there is no way to tell those apart
      from text alone, so this leans toward scrubbing the coincidence rather than missing the real
      thing. Names under 3 characters are left alone in standalone text -- at that length the ambiguity
      cuts the other way, an account named "Al" or "Bo" is more likely to blank real words than to earn
      its keep. This is a deliberate gap, not a missed case: report.txt itself never prints the account
      name as a field (nothing here echoes it outside of scrub()bed prose), and a short name embedded
      in a real path is still caught regardless -- the path patterns below key off the path's own
      shape, never the account name's length.

    This process's own home is replaced first, whole, so the common case (a report generated by the
    same account that hit the problem) reads as a clean `~` rather than `C:\\Users\\<user>`; the
    generic path patterns then catch anything that is left, including a path naming an account this
    process is not running as.

    The "capture up to the next slash" half of the path patterns can occasionally take more than the
    account name: `C:\\Users\\Alice and D:\\other\\b` becomes `C:\\Users\\<user>\\other\\b`, folding
    "and D:" into the blanked span because a later slash exists on the line at all. That is accepted,
    not a bug to tighten -- over-scrubbing a few extra words is the safe direction for a privacy tool;
    under-scrubbing is the one that leaks. Do not "fix" this by making the segment stop earlier."""
    try:
        home = str(pathlib.Path.home())
    except RuntimeError:
        home = ""
    if home:
        text = text.replace(home, "~")
    # The known-name pass runs before the generic path patterns: a multi-word name (a display name
    # with a space in it, say) is one literal token here, matched and replaced whole. Run the other
    # way around, the path pattern's segment terminator (which must stop at a bare space -- a path
    # can end mid-line with no separator at all) would blank only the name's first word and leave the
    # rest sitting right next to the result, which is worse than not having the path pattern at all.
    names = {os.environ.get(var) for var in ("USER", "USERNAME", "LOGNAME")}
    try:
        names.add(getpass.getuser())
    except OSError:
        pass
    for name in names:
        if name and len(name) >= 3:
            text = re.sub(r"(?<![A-Za-z0-9])" + re.escape(name) + r"(?![A-Za-z0-9])", "<user>",
                          text, flags=re.IGNORECASE)
    text = _HOME_PATH_RE.sub(_blank_matched_segment, text)
    return text


def decode_text_member(data: bytes) -> "str | None":
    """`data` decoded as text, or None when nothing here can say so confidently -- the caller must
    then leave the file out of the report rather than copy bytes scrub() cannot reliably see into.

    Tried in order: a UTF-16 BOM (a Windows tool, including plausibly a C++ crash reporter using wide
    strings, commonly writes one); no BOM but NUL-heavy, which is what UTF-16 without a BOM looks like
    to a byte count; UTF-8; then cp1252, the common fallback for legacy Windows text, which -- unlike
    replacing bad bytes with U+FFFD -- accepts almost every byte and keeps an accented name matchable
    by scrub() rather than turning it into a character no username could ever equal."""
    if data[:2] in (b"\xff\xfe", b"\xfe\xff"):
        try:
            return data.decode("utf-16")
        except UnicodeDecodeError:
            return None
    if data and data.count(b"\x00") > len(data) // 4:      # NUL-heavy: UTF-16 without a BOM
        for encoding in ("utf-16-le", "utf-16-be"):
            try:
                return data.decode(encoding)
            except UnicodeDecodeError:
                continue
        return None
    for encoding in ("utf-8", "cp1252"):
        try:
            return data.decode(encoding)
        except UnicodeDecodeError:
            continue
    return None


def read_scrubbed_text(path: pathlib.Path) -> "str | None":
    """A text member's contents, scrubbed and ready to go into the zip, or None if it could not be
    decoded confidently (decode_text_member) -- the caller leaves the file out rather than risk a
    leak inside text scrub() never got a real chance to read."""
    try:
        data = path.read_bytes()
    except OSError:
        return None
    text = decode_text_member(data)
    return scrub(text) if text is not None else None


def safe_game_file(game: pathlib.Path, path: pathlib.Path) -> bool:
    """Whether `path` is safe to copy into a report: a real file, never a symlink (its target is not
    even inspected -- a symlink planted in the game folder, by another program or by hand, is exactly
    how a file from outside it would get in), and its fully resolved location genuinely is inside
    `game`. Used for every file --report reads from the game folder: the crash/hang globs, the fixed
    game-folder members, and a named savegame."""
    try:
        if path.is_symlink() or not path.is_file():
            return False
        resolved, base = path.resolve(strict=True), game.resolve(strict=True)
    except OSError:
        return False
    return resolved == base or base in resolved.parents


def detect_wine() -> "str | None":
    """Wine's own version string, if this Python is running under it; None on a real Windows (or
    anywhere else). Two independent tells, since either alone has been seen missing on some builds:
    ntdll's own export, and the registry key Wine creates for itself."""
    if platform.system() != "Windows":
        return None
    try:
        import ctypes
        get_version = ctypes.windll.ntdll.wine_get_version
        get_version.restype = ctypes.c_char_p
        version = get_version()
        if version:
            return version.decode(errors="replace")
    except (AttributeError, OSError):
        pass
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Wine"):
            return "detected (HKCU\\Software\\Wine is present)"
    except OSError:
        pass
    return None


def gpu_name() -> "str | None":
    """The GPU's name, if cheaply available. None, never an exception: this is the one part of the
    report that shells out to the OS's own tools, and a report must never fail over it. Each probe
    has its own try, so a missing `wmic` (dropped from newer Windows builds) falls through to
    PowerShell instead of the whole function giving up."""
    system = platform.system()
    if system == "Windows":
        try:
            out = subprocess.run(["wmic", "path", "win32_VideoController", "get", "name"],
                                 capture_output=True, text=True, timeout=5)
            lines = [ln.strip() for ln in out.stdout.splitlines()
                    if ln.strip() and ln.strip().lower() != "name"]
            if lines:
                return lines[0]
        except Exception:
            pass
        try:
            out = subprocess.run(["powershell", "-NoProfile", "-Command",
                                  "(Get-CimInstance Win32_VideoController).Name"],
                                 capture_output=True, text=True, timeout=5)
            lines = [ln.strip() for ln in out.stdout.splitlines() if ln.strip()]
            if lines:
                return lines[0]
        except Exception:
            pass
        return None
    if system == "Darwin":
        try:
            out = subprocess.run(["system_profiler", "SPDisplaysDataType"],
                                 capture_output=True, text=True, timeout=5)
            for line in out.stdout.splitlines():
                if "Chipset Model" in line:
                    return line.split(":", 1)[1].strip()
        except Exception:
            pass
        return None
    return None


def release_info() -> dict:
    """release.json's own fields, or {} -- unlike release(), never a reason to stop: a report is
    exactly the thing a player needs when the release folder is not in the state setup expects."""
    try:
        return json.loads((HERE / "release.json").read_text(encoding="utf-8", errors="replace"))
    except (OSError, ValueError):
        return {}


def recognise_exe(sha: "str | None", record: dict) -> str:
    """What setup would call this lomse.exe, from the hashes it already knows -- the same recognition
    exe_plan and uninstall_terrain use, read back rather than re-derived."""
    if sha is None:
        return "missing"
    if sha == PRISTINE_EXE_SHA256:
        return "vanilla GS5R3 (unpatched)"
    if sha == FIXED_EXE_SHA256:
        return "patched by this mod (Shade crash fix)"
    if sha == PATCHED_EXE_SHA256:
        return "patched by this mod (Shade crash fix + HD terrain)"
    if sha in EARLIER_PATCHED_EXE_SHA256S:
        return "patched by an earlier release of this mod (HD terrain)"
    if sha in our_exe_hashes(record):
        return "patched by this mod (an earlier install)"
    return "unknown"


def recognise_dll(sha: "str | None", record: dict, release_record: dict) -> str:
    """What setup would call this ddraw.dll, from the release it shipped with and the install record."""
    if sha is None:
        return "missing"
    if release_record.get("ddraw_sha256") == sha:
        return "this release's overlay ddraw.dll"
    if sha == record.get("ddraw_sha256") or sha in record.get("overlay_sha256s", []):
        return "an earlier install's overlay ddraw.dll"
    return "not the overlay's (the player's original, or another mod)"


# Crash/hang files are known ONLY by this exact shape -- the crash reporter's own naming, timestamp
# included, with an optional _N disambiguator -- never by a loose prefix/suffix. "lomhd_crash_" plus
# anything else (a name, a word, nothing at all) is not this pattern and is therefore not a known name:
# it is counted like any other unrecognised file, not shown, even though it starts the same way.
_CRASH_FILE_RE = re.compile(r"^lomhd_(crash|hang)_\d{8}_\d{6}(?:_\d+)?\.(?:txt|dmp)$", re.IGNORECASE)


def _crash_file_kind(name: str) -> "str | None":
    """"crash" or "hang" if `name` is exactly the crash reporter's own naming shape (_CRASH_FILE_RE),
    else None. Used to keep a crash/hang file's listing entry as anonymous as its zip member already
    is -- see list_game_folder."""
    match = _CRASH_FILE_RE.match(name)
    return match.group(1).lower() if match else None


# Names the game-folder listing will show outright. Every one of these is EXACT (or, for the two that
# vary, a tight pattern) rather than a prefix or suffix: a loose "lomhd" or "ddraw." prefix let
# "lomhd_private Alice.txt", "lomhdAlice.txt" and "ddraw.private Alice" all through, which is exactly
# what this list exists to stop. Anything else -- a player's own folder or file sitting loose at the
# top level, which could be named after anything, including the player -- is only counted by
# list_game_folder, never named. A name earning its way onto this list is the point: it grows only when
# this mod or the game itself is confirmed to write that exact name.
_KNOWN_GAME_ARCHIVES = {"gs.mpq", "imp.mpq", "pic.mpq", "sndfx.mpq", "special.mpq"}  # docs/mpq-inventory.md
_DDRAW_INI_SAVED_RE = re.compile(r"^ddraw\.ini\.lomhd-saved\d*$", re.IGNORECASE)     # ...saved, ...saved2, ...
_KNOWN_GAME_ENTRY_NAMES = {
    # This mod's own files.
    "lomse.exe", "lomse.exe.lomhd-backup",
    "ddraw.dll", "ddraw.ini",
    "lomhd.log", "lomhd_install.json", "lomhd_portraits.pack", "lomhd_terrain",
    "lomhd_last_summary.txt", "lomhd_debug", "lomhd_trace",
    "lomhd_no_crash_reports", "lomhd_no_hang_reports", "lomhd_crash_reports",
    # The game's own top-level loose files and folders (docs/loose-files.md, every profile measured).
    "savegame", "multisav", "text", "wav", "smk", "shaders", "map", "custldr",
    "lom.cfg", "gs5r.cfg", "settings.cfg", "profile.txt", "gs_ms.txt", "quickstart",
    "army.log", "artifact.log", "chat.log", "combat.log", "hotkey.log", "spells.log", "thief.log",
    "lomlauncher.exe", "battle.snp", "standard.snp", "cnc-ddraw config.exe", "dpstub.exe",
    "gameuxinstallhelper.dll", "goggame.dll", "language.inf", "sierra.inf", "smackw32.dll",
    "storm.dll", "gs5r3 contributors.txt", "gs5r3 readme.txt", "lomse302.htm",
}


def _is_known_game_entry(name: str) -> bool:
    lower = name.lower()
    if lower in _KNOWN_GAME_ARCHIVES or lower in _KNOWN_GAME_ENTRY_NAMES:
        return True
    if _DDRAW_INI_SAVED_RE.match(name):
        return True
    return _CRASH_FILE_RE.match(name) is not None


def list_game_folder(game: pathlib.Path) -> "tuple[list[tuple[str, str]], int, int]":
    """(named entries, other-files count, other-folders count) for the game folder -- names and sizes
    only, never contents, and never a walk into a subfolder (lomhd_terrain's art, a save, a mod's own
    files, all kept a level down and so never listed by name here).

    Only a name _is_known_game_entry recognises is ever shown: a player's own file or folder sitting
    loose at the top level (a save backed up by hand, a screenshot, a folder named after the player)
    could be named after anything, so it is only counted -- see run_report, which turns the two counts
    into one summary line. A crash/hang file's entry is anonymised the same way its zip member is
    (crash_label): its own filename is exactly what this report exists to not repeat."""
    named: "list[tuple[str, str]]" = []
    other_files = other_folders = 0
    for p in sorted(game.iterdir()):
        is_dir = p.is_dir()
        if not _is_known_game_entry(p.name):
            if is_dir:
                other_folders += 1
            else:
                other_files += 1
            continue
        if is_dir:
            named.append((p.name + "\\", "<folder>"))
            continue
        kind = _crash_file_kind(p.name)
        name = crash_label(kind, p) if kind else p.name
        try:
            named.append((name, str(p.stat().st_size)))
        except OSError:
            named.append((name, "?"))
    return named, other_files, other_folders


CRASH_TIMESTAMP_RE = re.compile(r"\d{8}_\d{6}")


def crash_label(kind: str, path: pathlib.Path) -> str:
    """A name for `path` safe to put in a message a player will read in report.txt: its timestamp if
    it has the crash reporter's own YYYYMMDD_HHMMSS shape, else just its kind and extension. Never the
    file's own name -- used for a file that was left out, so nothing it might have been called (an
    account or character name; the crash reporter is not this mod's to control) ever appears in the
    zip at all, not even inside a note about why it isn't there."""
    match = CRASH_TIMESTAMP_RE.search(path.name)
    if match:
        return f"{kind}/{match.group(0)}{path.suffix.lower()}"
    return f"a {kind} file ({path.suffix.lower() or 'no extension'})"


def crash_arcname(kind: str, path: pathlib.Path, index: int) -> str:
    """The zip member name for a crash/hang file that IS going in: crash_label's own naming, but with
    a plain per-kind number (rather than "a crash file (.txt)", which two such files would collide on)
    when there is no usable timestamp."""
    match = CRASH_TIMESTAMP_RE.search(path.name)
    stem = match.group(0) if match else str(index)
    return f"{kind}/{stem}{path.suffix.lower()}"


def crash_files(game: pathlib.Path, with_dump: bool
               ) -> "tuple[list[tuple[pathlib.Path, str]], list[tuple[pathlib.Path, str]], list[str]]":
    """((path, arcname) for the text crash/hang files to include, likewise for dumps, notes about
    anything left out).

    Each glob in CRASH_TEXT_GLOBS and CRASH_DUMP_GLOB is capped at its own newest REPORT_MAX_PER_GLOB,
    counted only among files safe_game_file allows -- a symlink (or anything else that resolves
    outside the game folder) never occupies a slot a real file could have had, and is named in
    left_out (by crash_label, never its own filename) rather than silently skipped. A .dmp is only
    actually included with `with_dump`: it is always a candidate (so it can still be named and capped
    consistently), but otherwise goes to left_out explaining why (its paths cannot be scrubbed as
    text)."""
    text_included: "list[tuple[pathlib.Path, str]]" = []
    dump_included: "list[tuple[pathlib.Path, str]]" = []
    left_out: "list[str]" = []

    def candidates(pattern: str, kind: str) -> "list[pathlib.Path]":
        found = []
        for p in game.glob(pattern):
            if safe_game_file(game, p):
                found.append(p)
            else:
                left_out.append(f"{crash_label(kind, p)} (refused: a symlink, or it resolves outside "
                                "the game folder)")
        found.sort(key=lambda p: p.stat().st_mtime, reverse=True)
        return found[:REPORT_MAX_PER_GLOB]

    def under_cap(kind: str, p: pathlib.Path) -> bool:
        size = p.stat().st_size
        if size <= DUMP_SIZE_CAP:
            return True
        left_out.append(f"{crash_label(kind, p)} ({size} bytes, over the "
                        f"{DUMP_SIZE_CAP // (1 << 20)} MiB cap)")
        return False

    for pattern, kind in CRASH_TEXT_GLOBS:
        index = 1
        for p in candidates(pattern, kind):
            if under_cap(kind, p):
                text_included.append((p, crash_arcname(kind, p, index)))
                index += 1
    dump_pattern, dump_kind = CRASH_DUMP_GLOB
    index = 1
    for p in candidates(dump_pattern, dump_kind):
        if not with_dump:
            left_out.append(f"{crash_label(dump_kind, p)} (a binary minidump: its paths cannot be "
                            "scrubbed as text, so it is left out by default -- rerun with --with-dump "
                            "to include it as-is)")
        elif under_cap(dump_kind, p):
            dump_included.append((p, crash_arcname(dump_kind, p, index)))
            index += 1
    return text_included, dump_included, left_out


def is_save_file(path: pathlib.Path) -> bool:
    """Whether `path` looks like a save rather than shipped starting state or a stray file
    (docs/loose-files.md): a `.lom` file, or a player-named save with no extension at all.
    `quickstart` is the one no-extension exception -- shipped identically in every install, the "new
    game" state rather than anything a player saved."""
    name = path.name.lower()
    if name in ("desktop.ini", "quickstart"):
        return False
    return name.endswith(".lom") or "." not in name


def _unsafe_relative_name(name: str) -> bool:
    """Whether `name` could step outside the folder it is joined to: absolute (either slash
    convention), a `..` segment, or empty. Checked both ways since --with-save's NAME may come from a
    Windows player typing backslashes on a Mac/Linux copy of this script, or vice versa."""
    for cls in (pathlib.PurePosixPath, pathlib.PureWindowsPath):
        parts = cls(name)
        if parts.is_absolute() or not parts.parts or ".." in parts.parts:
            return True
    return False


def find_save(game: pathlib.Path, name: str) -> pathlib.Path:
    """The savegame --with-save asked for: the newest save for "latest", else that exact file --
    refusing an absolute path, a `..` segment, a symlink, or anything whose fully resolved location
    is not directly inside the savegame folder, before ever opening it."""
    saves = game / SAVE_DIR_NAME
    if name.lower() == "latest":
        candidates = ([p for p in saves.iterdir() if safe_game_file(game, p) and is_save_file(p)]
                     if saves.is_dir() else [])
        if not candidates:
            fail(f"no savegames found in {saves}.")
        candidates.sort(key=lambda p: p.stat().st_mtime, reverse=True)
        return candidates[0]
    if _unsafe_relative_name(name):
        fail(f'--with-save must name a file directly in {SAVE_DIR_NAME}, not "{name}".')
    path = saves / name
    try:
        same_folder = path.resolve(strict=True).parent == saves.resolve(strict=True)
    except OSError:
        fail(f"{path} does not exist.")
    if not (same_folder and safe_game_file(game, path) and is_save_file(path)):
        fail(f"{path} does not exist, or is not a savegame directly in {SAVE_DIR_NAME}.")
    return path


def save_arcname(path: pathlib.Path) -> str:
    """The zip member name for a savegame: never the player's own filename (a save is named by the
    player, or defaults to a real one like `autosave.lom` -- either way not this mod's to publish),
    just its kind (`.lom`, or no extension) via is_save_file's own rule."""
    return f"{SAVE_DIR_NAME}/save{path.suffix.lower()}"


def build_report_text(game: pathlib.Path, left_out: "list[str]", with_save: bool,
                      included_dumps: "list[str]") -> str:
    """report.txt's own contents: everything --report gathers that is not another file already in
    the zip. Scrubbed as a whole at the end, so nothing added above here has to remember to."""
    record = read_record(game)
    release_record = release_info()
    lines = [
        "Lords of Magic HD overlay -- diagnostic report",
        f"generated: {datetime.datetime.now().isoformat(timespec='seconds')}",
        f"game folder: {game}",
        "",
        f"OS: {platform.platform()}",
    ]
    wine = detect_wine()
    if wine:
        lines.append(f"Wine: {wine}")
    lines.append(f"Python: {platform.python_version()}")
    magick = magick_version(timeout=10)      # a report must not hang here even if magick is broken
    lines.append(f"magick -version: {magick.splitlines()[0] if magick.strip() else '(not found)'}")
    gpu = gpu_name()
    lines.append(f"GPU: {gpu if gpu else '(could not be read)'}")
    lines += ["", "Files this mod recognises:"]
    for name in HASH_FILES:
        path = game / name
        h = file_hash(path)
        if name == "lomse.exe":
            what = recognise_exe(h, record)
        elif name == "ddraw.dll":
            what = recognise_dll(h, record, release_record)
        else:
            what = "unknown (not tracked by this mod)"
        lines.append(f"  {name}: {h or '(missing)'} -- {what}")
    lines += ["", "Install record (lomhd_install.json):"]
    if record:
        lines.append(f"  release: {record.get('release', '?')}")
        lines.append(f"  animated sprites installed: {record.get('sprites', False)}")
        lines.append(f"  HD terrain installed: {'terrain' in record}")
    else:
        lines.append("  (none, or damaged)")
    lines += ["", "Last setup summary:"]
    summary = game / SUMMARY_NAME
    summary_text = read_scrubbed_text(summary) if safe_game_file(game, summary) else None
    lines.append(summary_text if summary_text is not None else "  (none stored)")
    if included_dumps:
        lines += ["", "Included crash dumps (binary, NOT scrubbed -- --with-dump asked for these "
                      "exactly as written; check them yourself before attaching if that matters):"]
        lines += [f"  {name}" for name in included_dumps]
    if left_out:
        lines += ["", "Left out of this report:"] + [f"  {note}" for note in left_out]
    if with_save:
        # Never the requested name or the save's own filename: a save is player-named (or a
        # character's), and both are exactly what this report must not carry unasked. See
        # save_arcname; the save itself sits at savegame/ in this zip if you want to check it.
        lines += ["", "Included a savegame (see savegame/ in this zip)."]
    lines += ["", "Game folder contents (names and sizes only; a name this mod does not "
                  "recognise is counted, not shown):"]
    named, other_files, other_folders = list_game_folder(game)
    lines += [f"  {name}\t{size}" for name, size in named]
    if other_files or other_folders:
        parts = []
        if other_files:
            parts.append(f"{other_files} other file{'s' if other_files != 1 else ''}")
        if other_folders:
            parts.append(f"{other_folders} other folder{'s' if other_folders != 1 else ''}")
        lines.append(f"  + {', '.join(parts)} (names not shown)")
    # The whole text is scrubbed here, once, rather than piecemeal above: nothing added to `lines`
    # later has to remember to. The other zip members are each scrubbed individually, at the point
    # they are read -- see read_scrubbed_text.
    return scrub("\n".join(lines) + "\n")


def finalize_report(built: pathlib.Path) -> pathlib.Path:
    """Give a finished report (already written whole, at `built`) its public name: lomhd-report-
    <timestamp>.zip, or -2.zip, -3.zip... if that name is taken.

    Each candidate name is claimed by *creating* it -- with a hard link, which shares `built`'s bytes
    without copying them and fails atomically with FileExistsError if the name is already there --
    rather than by checking whether the name exists and creating it as a second, separate step. Two
    runs started in the same second could both pass that check before either had created anything, and
    one report would silently overwrite the other. A filesystem that cannot hard-link the two paths
    (not every one can) falls back to an exclusive-create copy, same naming loop, same guarantee -- and
    if the copy itself fails partway, the candidate this run just created (never one an earlier run
    made: that would still be a FileExistsError, handled the same way as the hard-link case) is removed
    rather than left behind as a public-looking but half-written report."""
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    stem = f"lomhd-report-{stamp}"
    n = 1
    while True:
        candidate = HERE / (f"{stem}.zip" if n == 1 else f"{stem}-{n}.zip")
        try:
            os.link(built, candidate)
            return candidate
        except FileExistsError:
            n += 1
            continue
        except OSError:
            pass          # this filesystem cannot hard-link `built` to `candidate` -- copy instead
        try:
            with candidate.open("xb") as dst, built.open("rb") as src:
                shutil.copyfileobj(src, dst)
        except FileExistsError:
            n += 1
            continue
        except BaseException:
            candidate.unlink(missing_ok=True)
            raise
        return candidate


def run_report(game: pathlib.Path, with_save: "str | None", with_dump: bool) -> None:
    """--report: one zip, next to this script (the same place lomhd_work and my-upscale-choices.json
    already go, and simpler than guessing a Desktop folder that may not exist on every platform this
    runs on). Nothing here is uploaded; the player attaches the zip to an issue by hand.

    Built whole in a private temp file (tempfile.mkstemp, so two runs of --report started at once are
    never writing to the same path) and only given its public name (finalize_report) once every member
    has been added without error; the temp file is always removed after, whether that succeeded or
    not. A failure partway through a build therefore never leaves a half-written report where a player
    (or a later run) would find it, and never touches an earlier run's finished report."""
    text_crashes, dump_crashes, left_out = crash_files(game, with_dump)
    save_path = find_save(game, with_save) if with_save else None

    # Every text member is read and scrubbed up front, so a decode failure can be recorded in
    # left_out before report.txt (which lists left_out) is built, and the zip-writing pass below never
    # has to re-read a file or re-decide what to do with one that did not decode.
    text_members: "list[tuple[str, str]]" = []

    def add_text_member(arcname: str, path: pathlib.Path, label: str) -> None:
        text = read_scrubbed_text(path)
        if text is None:
            left_out.append(f"{label} (could not be decoded confidently as text -- left out)")
            return
        text_members.append((arcname, text))

    for name in GAME_TEXT_FILES:
        path = game / name
        if safe_game_file(game, path):
            add_text_member(name, path, name)
    release_json = HERE / "release.json"
    if release_json.is_file() and not release_json.is_symlink():
        add_text_member("release.json", release_json, "release.json")
    for path, arcname in text_crashes:
        # The label a decode failure would print is the arcname, never path.name: the original
        # filename is exactly what item 2 above strips, and a report must not put it back here.
        add_text_member(arcname, path, arcname)

    report_text = build_report_text(game, left_out, save_path is not None,
                                    [arcname for _, arcname in dump_crashes])

    fd, temp_name = tempfile.mkstemp(dir=HERE, prefix="lomhd-report-", suffix=".part")
    os.close(fd)
    built = pathlib.Path(temp_name)
    out_path = None
    try:
        with zipfile.ZipFile(built, "w", zipfile.ZIP_DEFLATED) as z:
            for arcname, text in text_members:
                z.writestr(arcname, text)
            for path, arcname in dump_crashes:            # binary, unscrubbed -- see build_report_text
                z.write(path, arcname=arcname)
            if save_path:
                z.write(save_path, arcname=save_arcname(save_path))
            z.writestr("report.txt", report_text)
        out_path = finalize_report(built)
    finally:
        built.unlink(missing_ok=True)
    say(f"Report written to {out_path}")
    if save_path:
        say("NOTE: a save may contain your in-game names; only attach it if you're happy to share it.")
    for note in left_out:
        say(f"  left out -- {note}")
    say(f"Attach {out_path.name} to an issue at {ISSUES_URL}")


def write_setup_summary(game: pathlib.Path, release_version: str, lines: "list[str]") -> None:
    """SUMMARY_NAME: this run's own closing report, for a later --report to read back -- dated and
    versioned at the top, since a report is usually read well after the run that wrote it."""
    header = [f"date: {datetime.datetime.now().isoformat(timespec='seconds')}",
             f"release: {release_version}", ""]
    write_atomically(game / SUMMARY_NAME, ("\n".join(header + lines) + "\n").encode())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--game", type=pathlib.Path,
                        help="the folder holding lomse.exe and pic.mpq")
    parser.add_argument("--uninstall", action="store_true")
    parser.add_argument("--review", action="store_true",
                        help="render every option and open a page to pick your own; installs nothing")
    parser.add_argument("--port", type=int, default=8765, help="the review page's port")
    parser.add_argument("--terrain", action="store_true",
                        help="also install HD terrain: patches lomse.exe and adds lomhd_terrain "
                             "(--uninstall undoes both)")
    sprite_flags = parser.add_mutually_exclusive_group()
    sprite_flags.add_argument("--sprites", action="store_true",
                              help="also every frame of every animated sprite: several hours on a "
                                   "typical GPU, and it resumes if stopped. Later runs remember it")
    sprite_flags.add_argument("--no-sprites", action="store_true",
                              help="leave the animated sprites out again after a --sprites install")
    parser.add_argument("--force-terrain-folder", action="store_true",
                        help="replace (or, with --uninstall, remove) a lomhd_terrain folder this mod "
                             "did not make or that was changed since")
    parser.add_argument("--report", action="store_true",
                        help="write one zip of diagnostics next to this script, to attach to a "
                             "GitHub issue; installs nothing, nothing is uploaded")
    parser.add_argument("--with-save", metavar="NAME",
                        help="with --report, also include that savegame (savegame\\NAME, or the "
                             "newest save if NAME is 'latest'); saves are never included otherwise")
    parser.add_argument("--with-dump", action="store_true",
                        help="with --report, also include any lomhd_crash_*.dmp minidumps exactly as "
                             "written; left out by default because, unlike every other file in the "
                             "report, a minidump's paths cannot be scrubbed")
    args = parser.parse_args()

    if sys.version_info < (3, 9):
        fail("Python 3.9 or newer is needed.")
    if args.with_save and not args.report:
        fail("--with-save only makes sense with --report.")
    if args.with_dump and not args.report:
        fail("--with-dump only makes sense with --report.")
    game = find_game(args.game)
    say(f"Game: {game}")
    if args.report:
        run_report(game, args.with_save, args.with_dump)
        return 0
    # First, whatever was asked: an interrupted swap leaves a patched exe without its art, and the
    # long steps below can fail before install_terrain would get to it. (Codex review.)
    recover_terrain_swap(game, "--uninstall" if args.uninstall else "--terrain")
    if args.uninstall:
        uninstall(game, args.force_terrain_folder)
        return 0

    if args.review:
        check_magick()
        say("1/3  Getting the upscaler")
        exe, models = upscaler()
        say("2/3  Reading pictures from your pic.mpq")
        found = extract_images(game)
        say("3/3  Rendering every option (the long step; it resumes if stopped)")
        serve_review(render_review(found, exe, models), args.port)
        return 0

    record = release()
    check_magick()
    check_imp(game)
    # lomse.exe only for --terrain, which cannot go without it. The fix (fix_exe) is skipped, with a
    # note, when the exe cannot be written: that is no reason to refuse the overlay.
    check_writable(game, args.terrain)
    if args.terrain:
        terrain_exe_plan(game)       # refuse an exe it cannot patch now, not after an hour's work
        check_terrain_folder(game, read_record(game), args.force_terrain_folder)
    steps = 7 if args.terrain else 5
    animated, why = sprite_mode(game, args.sprites, args.no_sprites)
    if choices_file() == MY_CHOICES:
        say(f"Using your own picks from {MY_CHOICES.name}")
    say(f"Animated sprites: {why}")
    say(f"1/{steps}  Getting the upscaler")
    exe, models = upscaler()
    say(f"2/{steps}  Reading pictures from your pic.mpq and sprites from your imp.mpq")
    found = extract_images(game)
    say("     " + ", ".join(f"{len(v)} {PLURAL.get(k, k + 's')}" for k, v in found.items() if v))
    sprites, sprite_root, read_sprite = plan_sprites(game, animated)
    frames = sum(len(s.frames) for s in sprites.animated)
    say(f"     {len(sprites.static)} sprites" + (f", {len(sprites.animated)} animated sprites "
                                                 f"({frames} frames)" if animated else
                                                 f", {len(sprites.animated)} unit icon sheets ({frames} frames)"))
    say(f"3/{steps}  Upscaling pictures (the long step)")
    upscaled = upscale_all(found, exe, models)
    say(f"4/{steps}  Upscaling sprites" + (" (the very long step; it resumes if stopped)"
                                          if animated else ""))
    again = retry_command(args, animated, game)
    upscale_sprites(sprites.static + sprites.animated, sprite_root, exe, models, again)
    say(f"5/{steps}  Building the pack and installing")
    originals = [WORK / "originals" / group for group in found if found[group]]
    pack = WORK / PACK_NAME
    pictures: dict = {}
    count, skipped, sprite_skipped, packed, moving = build_pack(pack, sprites, sprite_root, read_sprite,
                                                                originals, upscaled, exe, models, pictures)
    install(game, pack, record, animated)
    exe_note = fix_exe(game, "--terrain" if args.terrain else "")
    # Kept alongside the printed run (as SUMMARY_NAME, in the game folder): the closing report of the
    # last install that finished, for --report to read back on a later, separate run.
    summary_lines: "list[str]" = []

    def told(text: str) -> None:
        summary_lines.append(text)
        say(text)

    told(f"\nDone: {count} HD images installed in {game}.")
    told(exe_note)
    for line in skipped:
        told(f"  left out -- {line}")
    told(f"Sprites: {packed['packed']} packed" + (f", and {moving['sprites']} animated sprites "
                                                   f"({moving['packed']} frames)" if animated else "")
        + (f"; {len(sprite_skipped)} left out ({summarise_skips(sprite_skipped)})" if sprite_skipped else ""))
    told(f"Unit strip: {moving.get('strip', 0)} figure windows packed from the unit icon sheets")
    told(damage_summary([packed, moving, pictures], sprite_skipped + skipped))
    if animated:
        c = sprites.counts
        told(f"  of {c['frames']} animated frames, {c['repeats']} repeat another, {c['ineligible']} are too "
             f"small or too large for the overlay, {c['no_probe']} too plain for it to find")
    if sprite_skipped:
        report = WORK / "sprites-left-out.txt"
        report.write_text("".join(f"{line}\n" for line in sprite_skipped))
        told(f"  (every sprite left out, and why: {report})")
    if not animated:
        told("Animated sprites (units, spell effects) were left out, as --no-sprites asked."
             if args.no_sprites else
             "Animated sprites (units, spell effects) were not built: add --sprites for them.")
    if args.terrain:
        told(f"\n6/{steps}  Building HD terrain from your pic.mpq (the long step again)")
        built = build_terrain(game, exe, models)
        told(f"7/{steps}  Installing HD terrain and patching lomse.exe")
        install_terrain(game, built, args.force_terrain_folder)
        told(f"Done: HD terrain installed ({TERRAIN_DIR}\\til, and lomse.exe patched; the original "
             f"is {EXE_BACKUP_NAME}). Recommended window: 1280x960 (width/height in ddraw.ini).")
    told("To undo: python lomhd_setup.py --uninstall" + (f' --game "{game}"' if args.game else ""))
    write_setup_summary(game, record["version"], summary_lines)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
