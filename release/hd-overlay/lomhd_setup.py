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
     character portraits on the approved palette pipeline, everything else in full colour.
  4. Upscales each sprite that does not move (one frame) the same way, with its own pick. With
     --sprites, also every frame of every animated sprite, each with its sprite's pick: several
     hours, cached in lomhd_work/sprites, so a run stopped part-way carries on where it was.
  5. Writes lomhd_portraits.pack beside lomse.exe, backs up your ddraw.dll to
     ddraw.dll.lomhd-backup and installs the overlay's ddraw.dll.

With --terrain, after those five steps (and only if they succeeded):

  6. Reads the terrain atlases and their .til files out of your pic.mpq and upscales every tile to
     2x, quantized back to its atlas's own palette (tools/terrain_hd.py).
  7. Writes them to lomhd_terrain/til beside lomse.exe. pic.mpq is not changed. Then backs up
     lomse.exe to lomse.exe.lomhd-backup and patches it (65 same-size edits, every one checked
     before any is written) so the terrain is drawn at 2x. Last, so an interrupted run never leaves
     a patched game without its art.

Needs Python 3.9+, ImageMagick 7 (`magick` on PATH) and a GPU with Vulkan. Takes 20-60 minutes,
almost all of it steps 3 and 4 (with --sprites, several hours). Everything it downloads or makes
lives in `lomhd_work` next to this script.
"""
from __future__ import annotations

import argparse
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
import urllib.error
import urllib.request
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))

import exe_patch  # noqa: E402
import hd_portrait_pack  # noqa: E402
import hd_sprites  # noqa: E402
import hd_upscale  # noqa: E402
import imp_read  # noqa: E402
import lbm_png  # noqa: E402
import mpq_read  # noqa: E402
import terrain_hd  # noqa: E402
from imp_members import candidate_members, resolve_members  # noqa: E402

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
# GS5R3 lomse.exe, the binary the sets were derived from, and what applying both sets to it gives.
# The second is recomputed from the pristine bytes on every install and must agree; the constant is
# what recognises an already-patched exe when there are no pristine bytes to hand.
PRISTINE_EXE_SHA256 = "a505f399d5be73fe0a2215633f663717f28daeb3075bbcc05b47d40653669052"
PATCHED_EXE_SHA256 = "ddb438837c47f5299fe5b74e4c1de9c31fcbc7b218179c69ba043b9b93d0bee2"
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
    subprocess.run(["magick", str(ppm), f"PNG:{png}"], check=True)
    ppm.unlink()


def upscale_all(found: dict[str, list[str]], exe: pathlib.Path, models: pathlib.Path,
                choices_path: pathlib.Path | None = None) -> list[pathlib.Path]:
    """Each image with the option picked for it in review, or the default for images that review
    never saw. Returns the folders holding the upscales."""
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
                lbm_to_png(WORK / "originals" / group / f"{stem}.lbm", png)
                inputs[stem] = png
            hd_upscale.render(choice, inputs, dest, exe, models)
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
                for option in options:
                    (review / option / f"{key}.png").unlink(missing_ok=True)
                part = originals / f"{key}.part"          # not *.png: the page lists those
                lbm_to_png(lbm, part)
                os.replace(part, png)
                shutil.copyfile(lbm, kept)
            inputs.setdefault(group, {})[key] = png
            if hd_upscale.default_choice(group, stem) == hd_upscale.APPROVED:
                characters.append(stem)
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
    digests: dict = {}
    unreadable: dict = {}

    def frame_count(member: str):
        if member.lower() not in archive:
            return None
        try:
            sprite = read_sprite(member)
        except imp_read.ImpError as error:
            unreadable[member] = str(error)
            return None
        digests[member] = sprite.digest
        return len(sprite.frames)

    candidates = candidate_members(IMP_NAMES)
    resolved, skipped = resolve_members(candidates, frame_count)
    for n, line in enumerate(skipped):         # "not in this archive" is not why, for a damaged one
        name = line.split(":", 1)[0]
        errors = [f"{m} ({unreadable[m]})" for m in candidates.get(name, []) if m in unreadable]
        if errors and line.endswith("not in this archive"):
            skipped[n] = f"{name}: could not read {', '.join(errors)}"
    root.mkdir(parents=True, exist_ok=True)
    hd_sprites.prune(root, {hd_sprites.member_key(m, digests[m]) for m, _ in resolved.values()})
    limit = os.environ.get(SPRITE_LIMIT_ENV)
    if animated and limit:
        keep = sorted(n for n, (_, frames) in resolved.items() if frames > 1)[:int(limit)]
        resolved = {n: v for n, v in resolved.items() if v[1] == 1 or n in keep}
        say(f"     {SPRITE_LIMIT_ENV}={limit}: only {len(keep)} animated sprites (a developer aid)")
    plan = hd_sprites.plan(resolved, read_sprite, sprite_choices(), root, animated=animated)
    plan.skipped[:0] = skipped
    return plan, root, read_sprite


def upscale_sprites(sprites: list, root: pathlib.Path, exe: pathlib.Path, models: pathlib.Path) -> None:
    def render(option, inputs, dest):
        hd_upscale.render(option, inputs, dest, exe, models)
    try:
        hd_sprites.render_all(sprites, root, render, log=lambda line: say(f"     {line}"))
    except (SystemExit, subprocess.CalledProcessError) as error:
        fail(f"upscaling sprites stopped: {error}\nThe game has not been touched. Run again to carry "
             "on: every sprite already upscaled is kept.")


def build_pack(pack: pathlib.Path, sprites, sprite_root: pathlib.Path, read_sprite, originals: list,
               upscaled: list):
    """Write the pack: static sprites, then each animated sprite as its own consecutive group, then
    the pictures. Returns (images, pictures left out, sprites left out, static counts, animated
    counts); the counts hold "packed" frames and "sprites"."""
    skipped: list = []
    sprite_skipped = list(sprites.skipped)
    packed: dict = {}
    moving: dict = {}
    count = hd_portrait_pack.write_records(pack, itertools.chain(
        hd_sprites.records(sprites.static, sprite_root, read_sprite, sprite_skipped, packed),
        hd_sprites.records(sprites.animated, sprite_root, read_sprite, sprite_skipped, moving),
        hd_portrait_pack.unmasked_records(originals, upscaled, skipped, originals)))
    packed.setdefault("packed", 0)
    moving.setdefault("packed", 0)
    moving.setdefault("sprites", 0)
    return count, skipped, sprite_skipped, packed, moving


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
    ("not in this archive", "not in your imp.mpq"),
    ("ambiguous", "two sprites share the name"),
    ("picked 'original'", "the original was picked in review"),
    ("no usable upscale pick", "no pick"),
    ("character limit", "name too long for the overlay"),
    ("no 8-pixel run", "too plain for the overlay to find"),
    ("render", "no usable upscale"),
    ("could not read", "could not be read"),
)


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
    # stopping halfway with a traceback.
    check_writable(game, terrain=True)
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
                 RECORD_NAME + ".lomhd-part", "lomhd.log", RECORD_NAME):
        if (game / name).exists():
            (game / name).unlink()
    say(f"Uninstalled. ddraw.dll is {'your original again' if had else 'removed'}.")
    if terrain_removed:
        say(terrain_removed)
    if ini_saved:
        say(f"cnc-ddraw's settings file was set aside as {ini_saved.name} (the game ignores it; "
            "delete it if you like).")


# --- HD terrain (--terrain) ----------------------------------------------------------------------

def terrain_sets() -> list:
    """The two patch sets, as shipped (JSON: tomllib is Python 3.11+ and setup promises 3.9), loaded
    through exe_patch's own validation. Both must target the one binary this release knows."""
    sets = []
    for name in TERRAIN_SETS:
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


def terrain_exe_plan(game: pathlib.Path) -> tuple[bytes, bytes]:
    """(pristine bytes, patched bytes) for this game's lomse.exe, or a refusal naming why not.

    The exe must be the pristine GS5R3 binary, or one this mod patched (a re-run or an upgrade), in
    which case the pristine bytes come from the verified backup. Anything else -- another patch, a
    different version -- is refused before anything is touched."""
    exe, backup = game / EXE_NAME, game / EXE_BACKUP_NAME
    record = read_record(game).get("terrain", {})
    ours = {PATCHED_EXE_SHA256, *record.get("exe_patched_sha256s", [])}
    current, saved = file_hash(exe), file_hash(backup)
    if saved is not None and saved != PRISTINE_EXE_SHA256:
        fail(f"{EXE_BACKUP_NAME} exists and is not the original lomse.exe. Move it aside by hand so "
             "nothing is overwritten. The game's lomse.exe was not touched.")
    if current == PRISTINE_EXE_SHA256:
        pristine = exe.read_bytes()
    elif current in ours:
        if saved is None:
            fail(f"lomse.exe is already patched for HD terrain but {EXE_BACKUP_NAME} is missing, so "
                 "the original could not be restored later. Put the original back (Steam: Verify "
                 "integrity of game files) and run again.")
        pristine = backup.read_bytes()
    else:
        fail("lomse.exe is not the one HD terrain was made for (Lords of Magic Special Edition with "
             "the GS5R3 patch) and not one this mod patched -- another patch or a different version "
             "may have changed it. HD terrain was not installed; lomse.exe was not touched.")
    try:
        patched = exe_patch.apply(pristine, terrain_sets())
    except exe_patch.PatchError as error:
        fail(f"the terrain patch does not apply to this lomse.exe: {error}")
    if hashlib.sha256(patched).hexdigest() != PATCHED_EXE_SHA256:
        fail("patching lomse.exe gave an unexpected result. HD terrain was not installed.")
    return pristine, patched


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


def write_game_file(path: pathlib.Path, data: "bytes | pathlib.Path", again: str, state: str) -> None:
    """write_atomically, for a file the running game holds: a locked file becomes a message saying
    what to do, not a traceback, and the half-written .lomhd-part goes."""
    try:
        write_atomically(path, data)
    except OSError as error:
        try:
            path.with_name(path.name + ".lomhd-part").unlink(missing_ok=True)
        except OSError:
            pass
        fail(f"could not write {path.name} ({error.strerror or error}). Close Lords of Magic (and "
             f"anything else using the game folder) and run python lomhd_setup.py {again} again. "
             f"{state}")


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
    exe, backup = game / EXE_NAME, game / EXE_BACKUP_NAME
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

    if file_hash(backup) is None:              # terrain_exe_plan: so the exe is the pristine one
        write_game_file(backup, exe, again, "lomse.exe was not touched.")
        if file_hash(backup) != PRISTINE_EXE_SHA256:
            backup.unlink()
            fail("the backup of lomse.exe did not verify. lomse.exe was not touched.")
    if file_hash(exe) != PATCHED_EXE_SHA256:
        write_game_file(exe, patched, again, "lomse.exe was not changed.")
        if file_hash(exe) != PATCHED_EXE_SHA256:
            fail("the patched lomse.exe did not verify. Run --uninstall to put the original back.")


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
    original = terrain.get("exe_original_sha256", PRISTINE_EXE_SHA256)
    ours = {PATCHED_EXE_SHA256, terrain.get("exe_patched_sha256"), *terrain.get("exe_patched_sha256s", [])} - {None}
    current, saved = file_hash(exe), file_hash(backup)
    leftovers = ([game / (TERRAIN_DIR + s) for s in (".lomhd-part", ".lomhd-old")]
                 + [game / (EXE_NAME + ".lomhd-part"), game / (EXE_BACKUP_NAME + ".lomhd-part")])
    if not (terrain or saved is not None or current in ours or dest.exists()
            or any(p.exists() for p in leftovers)):
        return None

    exe_note = "lomse.exe is your original again"
    foreign_exe = False
    if current in ours or current is None:
        if saved != original:
            fail(f"{EXE_BACKUP_NAME} is missing or changed, so the original lomse.exe cannot be "
                 "restored safely. Nothing was removed. Steam's 'Verify integrity of game files' "
                 "puts the original back; then run --uninstall again.")
        write_game_file(exe, backup, again, "Nothing was removed.")
        if file_hash(exe) != original:
            fail("the restored lomse.exe did not verify. Nothing else was removed.")
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
    return f"HD terrain removed: {exe_note}, and {folder_note}."


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
    args = parser.parse_args()

    if sys.version_info < (3, 9):
        fail("Python 3.9 or newer is needed.")
    game = find_game(args.game)
    say(f"Game: {game}")
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
                                                 f"({frames} frames)" if animated else ""))
    say(f"3/{steps}  Upscaling pictures (the long step)")
    upscaled = upscale_all(found, exe, models)
    say(f"4/{steps}  Upscaling sprites" + (" (the very long step; it resumes if stopped)"
                                          if animated else ""))
    upscale_sprites(sprites.static + sprites.animated, sprite_root, exe, models)
    say(f"5/{steps}  Building the pack and installing")
    originals = [WORK / "originals" / group for group in found if found[group]]
    pack = WORK / PACK_NAME
    count, skipped, sprite_skipped, packed, moving = build_pack(pack, sprites, sprite_root, read_sprite,
                                                                originals, upscaled)
    install(game, pack, record, animated)
    say(f"\nDone: {count} HD images installed in {game}.")
    for line in skipped:
        say(f"  left out -- {line}")
    say(f"Sprites: {packed['packed']} packed" + (f", and {moving['sprites']} animated sprites "
                                                  f"({moving['packed']} frames)" if animated else "")
        + (f"; {len(sprite_skipped)} left out ({summarise_skips(sprite_skipped)})" if sprite_skipped else ""))
    if animated:
        c = sprites.counts
        say(f"  of {c['frames']} animated frames, {c['repeats']} repeat another, {c['ineligible']} are too "
            f"small or too large for the overlay, {c['no_probe']} too plain for it to find")
    if sprite_skipped:
        report = WORK / "sprites-left-out.txt"
        report.write_text("".join(f"{line}\n" for line in sprite_skipped))
        say(f"  (every sprite left out, and why: {report})")
    if not animated:
        say("Animated sprites (units, spell effects) were not built: add --sprites for them.")
    if args.terrain:
        say(f"\n6/{steps}  Building HD terrain from your pic.mpq (the long step again)")
        built = build_terrain(game, exe, models)
        say(f"7/{steps}  Installing HD terrain and patching lomse.exe")
        install_terrain(game, built, args.force_terrain_folder)
        say(f"Done: HD terrain installed ({TERRAIN_DIR}\\til, and lomse.exe patched; the original "
            f"is {EXE_BACKUP_NAME}). Recommended window: 1280x960 (width/height in ddraw.ini).")
    say("To undo: python lomhd_setup.py --uninstall" + (f' --game "{game}"' if args.game else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
