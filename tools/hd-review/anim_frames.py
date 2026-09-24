"""Every frame of every ANIMATED IMP sprite, upscaled and made into format-5 pack records.

Used by `sprite_pack.py --animated`. A dev tool, like it: animated sprites are not part of the
player release yet.

One upscaler per sprite -- the one picked for it in review (`upscale-choices.json`,
`sprite__<name>`), where one still frame stood for the whole animation because a model switching
between frames would flicker. Every frame is prepared exactly as that still was
(`sprite_originals`: shadow cleared, transparent pixels a neutral grey) and upscaled with it.

What the overlay needs of a frame (2026-09-23, measured over the captures in the research log):

- A sprite's frames are one GROUP, consecutive in the pack, so the overlay can load an animation's
  next frames before they are drawn.
- Frames of members under `units\\` are MIRROR: the game draws a map army facing the other way as
  its sprite flipped left to right. A front unicorn matched 85.5% flipped and under 30% as stored.
- A frame identical to one already packed -- the same pixels, the same transparency -- is packed
  once: 15,648 of 50,713 frames were repeats, 10,100 of them within their own sprite. The first
  keeps its record, MIRROR if any copy is (a `units\\` frame can repeat a building's that sorts
  first: Claude review, 2026-09-23); the matcher finds it wherever either was drawn.
- A frame the DLL could make no probe for -- every 8-pixel run of too few colours -- could never
  be found, and is left out.

Frames are exported, prepared and rendered under WORK (gitignored), keyed by the archive's content
and the member path -- every step, so a name that resolves to another member is never given the
first one's upscale -- and every step resumes: a rerun redoes only what is missing. Rendering goes
in batches, so an interrupted three-hour run loses one batch, not the lot.
"""
from __future__ import annotations

import concurrent.futures
import dataclasses
import hashlib
import pathlib
import shutil
import subprocess
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(HERE))
import hd_portrait_pack as pack  # noqa: E402
import hd_upscale  # noqa: E402
from png_index_patch import PngError, read_indexed_png  # noqa: E402

WORK = ROOT / "artifacts" / "hd-review" / "_anim"
RENDER_BATCH = 400
MAX_RECORD_NAME_LEN = 39         # the DLL's name[40]
PREP_BACKGROUND = "#202228"      # sprite_originals: a neutral dark grey bleeds least visibly


@dataclasses.dataclass
class Frame:
    index: int
    raw: pathlib.Path            # the export, exactly as the archive stores it
    prepped: pathlib.Path        # what the upscaler is given; its stem keys the render too
    record: str                  # the pack record's name
    mirror: bool                 # searched for flipped too: its sprite's, or a repeat's that is


@dataclasses.dataclass
class Sprite:
    name: str
    member: str
    option: str
    mirror: bool
    frames: list[Frame]


def member_key(member: str) -> str:
    return hashlib.sha256(member.lower().encode()).hexdigest()[:12]


def record_name(name: str, index: int) -> str:
    return f"anim__{name}#{index:03d}"


def mirrored(member: str) -> bool:
    """Map armies are drawn flipped when they face the other way. Only units were measured."""
    return member.lower().startswith("units\\")


def frame_identity(png, key: int) -> bytes:
    """What the matcher and the upscaler see of a frame: its size, and per pixel either which of
    the two see-through kinds it is (the key, the shadow) or its colour. Two frames with the same
    identity are the same record."""
    plte = png.palette()
    digest = hashlib.sha1(f"{png.width}x{png.height}".encode())
    for value in png.indices:
        if value == key:
            digest.update(b"\x00k")
        elif value == pack.SHADOW_INDEX:
            digest.update(b"\x00s")
        else:
            digest.update(b"\x01" + bytes(plte[value * 3:value * 3 + 3]))
    return digest.digest()


def prepare(raw: pathlib.Path, prepped: pathlib.Path) -> None:
    """The still the review was made from, for one frame: shadow cleared, transparent pixels grey,
    straight RGBA (`sprite_originals` does the same to each sprite's representative frame)."""
    from sprite_originals import clear_shadow
    tmp = prepped.with_suffix(".tmp.png")
    shutil.copy(raw, tmp)
    try:
        clear_shadow(tmp)
        subprocess.run(["magick", str(tmp), "-background", PREP_BACKGROUND, "-alpha", "background",
                        f"PNG32:{tmp}"], check=True, capture_output=True)
        tmp.replace(prepped)
    finally:
        tmp.unlink(missing_ok=True)


def export(viewer: pathlib.Path, archive: pathlib.Path, member: str, index: int, out: pathlib.Path,
           listfile: pathlib.Path) -> str | None:
    """Export one frame unless it already is; the error text if it cannot be."""
    if out.exists():
        return None
    tmp = out.with_suffix(".tmp.png")
    result = subprocess.run([str(viewer), "--export-imp-frame", str(archive), member, str(index),
                             str(tmp), "--listfile", str(listfile)], capture_output=True, text=True)
    if result.returncode != 0 or not tmp.exists():
        tmp.unlink(missing_ok=True)
        return result.stderr.strip()[:200] or "no output"
    tmp.replace(out)
    return None


def plan(archive: pathlib.Path, viewer: pathlib.Path, listfile: pathlib.Path,
         resolved: dict[str, tuple[str, int]], choices: dict, work: pathlib.Path,
         fingerprint: str, workers: int = 8) -> tuple[list[Sprite], list[str], dict[str, int]]:
    """(sprites, skipped, counts): every animated sprite with a usable pick, its frames exported,
    checked and prepared, repeats dropped. `resolved` is name -> (member, frame count), as
    `imp_members.resolve_members` gives it."""
    skipped: list[str] = []
    counts = {"frames": 0, "ineligible": 0, "repeats": 0, "unreadable": 0, "no_probe": 0}
    sprites: list[Sprite] = []
    seen: dict[bytes, Frame] = {}
    root = work / fingerprint

    todo = []
    for name, (member, frames) in sorted(resolved.items()):
        if frames < 2:
            continue
        choice = choices.get(f"sprite__{name}")
        if choice not in hd_upscale.OPTIONS:
            skipped.append(f"{name}: no usable upscale pick ({choice!r})")
            continue
        if len(record_name(name, frames - 1)) > MAX_RECORD_NAME_LEN:
            skipped.append(f"{name}: record names would pass the DLL's {MAX_RECORD_NAME_LEN} characters")
            continue
        folder = root / "raw" / f"{member_key(member)}__{name}"
        folder.mkdir(parents=True, exist_ok=True)
        todo.append((name, member, frames, choice, folder))

    jobs = [(member, i, folder / f"{i:03d}.png") for name, member, frames, _, folder in todo
            for i in range(frames)]
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        errors = list(pool.map(lambda j: export(viewer, archive, j[0], j[1], j[2], listfile), jobs))
    failed = {(member, i): error for (member, i, _), error in zip(jobs, errors) if error}

    prep_dir = root / "prep"
    prep_dir.mkdir(parents=True, exist_ok=True)
    to_prepare = []
    for name, member, frames, choice, folder in todo:
        sprite = Sprite(name, member, choice, mirrored(member), [])
        for i in range(frames):
            counts["frames"] += 1
            if (member, i) in failed:
                skipped.append(f"{name} frame {i}: could not export ({failed[member, i]})")
                continue
            raw = folder / f"{i:03d}.png"
            try:
                png = read_indexed_png(raw.read_bytes())
            except (PngError, zlib.error, OSError):
                counts["unreadable"] += 1
                continue
            key = transparent_key(png)
            indices = bytes(png.indices)
            if key is None or not pack.masked_is_eligible(png.width, png.height, indices, key):
                counts["ineligible"] += 1
                continue
            try:
                pack.check_reader_limits(name, png.width, png.height, png.width * 2, png.height * 2,
                                         masked=True)
            except SystemExit:
                counts["ineligible"] += 1
                continue
            if not pack.masked_probe_slices(png.width, png.height, indices, key, pad_palette(png.palette())):
                counts["no_probe"] += 1
                continue
            identity = frame_identity(png, key)
            if identity in seen:
                counts["repeats"] += 1
                seen[identity].mirror |= sprite.mirror
                continue
            prepped = prep_dir / f"{member_key(member)}__{name}__{i:03d}.png"
            frame = Frame(i, raw, prepped, record_name(name, i), sprite.mirror)
            seen[identity] = frame
            sprite.frames.append(frame)
            if not prepped.exists():
                to_prepare.append((raw, prepped))
        if sprite.frames:
            sprites.append(sprite)

    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        list(pool.map(lambda job: prepare(*job), to_prepare))
    return sprites, skipped, counts


def render_all(sprites: list[Sprite], work: pathlib.Path, fingerprint: str, render,
               batch: int = RENDER_BATCH, log=print) -> None:
    """Upscale every planned frame with its sprite's pick, `batch` frames per call to `render`
    (`hd_upscale.render`'s signature without the model paths). Resumes: rendered frames stay."""
    by_option: dict[str, dict[str, pathlib.Path]] = {}
    for sprite in sprites:
        for frame in sprite.frames:
            by_option.setdefault(sprite.option, {})[frame.prepped.stem] = frame.prepped
    for option, inputs in sorted(by_option.items()):
        dest = work / fingerprint / "render" / option
        todo = {k: p for k, p in inputs.items() if not (dest / f"{k}.png").exists()}
        log(f"  {option}: {len(inputs)} frames, {len(todo)} to render")
        keys = sorted(todo)
        for start in range(0, len(keys), batch):
            render(option, {k: todo[k] for k in keys[start:start + batch]}, dest)
            log(f"    {min(start + batch, len(keys))}/{len(keys)}")


def records(sprites: list[Sprite], work: pathlib.Path, fingerprint: str, load_hd_rgba,
            skipped: list[str], first_group: int = 1):
    """Yield (entry, zidx, zhd) for every planned frame, one group per sprite, in order. A frame
    whose render is missing or the wrong size is reported in `skipped` and left out; its sprite's
    other frames still go in."""
    group = first_group
    for sprite in sprites:
        if group > pack.MAX_GROUP:
            skipped.append(f"{sprite.name}: more than {pack.MAX_GROUP} animated sprites")
            continue
        packed = 0
        for frame in sprite.frames:
            flags = pack.FLAG_MASKED | (pack.FLAG_MIRROR if frame.mirror else 0)
            render = work / fingerprint / "render" / sprite.option / f"{frame.prepped.stem}.png"
            if not render.exists():
                skipped.append(f"{frame.record}: {sprite.option} render is missing")
                continue
            png = read_indexed_png(frame.raw.read_bytes())
            key = transparent_key(png)
            try:
                hw, hh, rgba = load_hd_rgba(render, png.width, png.height)
                record = pack.encode_record(frame.record, png.width, png.height, bytes(png.indices),
                                            pad_palette(png.palette()), hw, hh, rgba, flags=flags,
                                            key=key, group=group)
            except (ValueError, SystemExit, subprocess.CalledProcessError) as error:
                skipped.append(f"{frame.record}: {error}")
                continue
            packed += 1
            yield record
        if packed:
            group += 1


def transparent_key(png):
    from sprite_pack import transparent_key as key_of
    return key_of(png)


def pad_palette(plte: bytes):
    from sprite_pack import pad_palette as pad
    return pad(plte)
