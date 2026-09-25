"""IMP sprites made into HD pack records: which frames, prepared how, upscaled with what, packed as
what. One code path for the player's setup (`release/hd-overlay/lomhd_setup.py`) and the dev tools
(`tools/hd-review/sprite_pack.py`, `anim_frames.py`), so what is tested is what players run.

Everything is read from the player's own imp.mpq with `imp_read` (a port of the asset viewer's
decoder) and written under a work folder keyed by the archive's content; no game art ships.

STATIC sprites -- a member with exactly one frame in total -- become one record each, `sprite__<name>`,
group 0, never MIRROR: one upscale covers the whole sprite the way one covers a portrait.

ANIMATED sprites become one record per distinct frame, `anim__<name>#NNN`, with one upscaler for
the whole sprite -- the one picked for it in review (`upscale-choices.json`, `sprite__<name>`), where
one still frame stood for the whole animation because a model switching between frames would flicker.
What the overlay needs of them (2026-09-23, measured over the captures in the research log):

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

Each record's LOW-RES half (palette + indices, what the matcher compares on screen) is the frame
exactly as the archive stores it -- no shadow-clearing, no background fill, because the game still
draws the shadow and the transparent key as stored. The upscaler's INPUT is the reviewed
preparation instead (`hd-review/sprite_originals.py`): the key and the shadow (index 1) transparent
over a neutral grey, everything else opaque -- written here directly, pixel-identical to the
`clear_shadow` + `magick -background #202228 -alpha background PNG32:` the review used (tested).

The HD half is the render, which `hd_upscale.render` always makes exactly 2x the frame. A render of
a DIFFERENT size is refused, not resized: it was made from a different original.

Work files are keyed by archive content, member path and frame, and every step resumes: a rerun
redoes only what is missing. Rendering goes in batches, so an interrupted run loses one batch.
"""
from __future__ import annotations

import dataclasses
import hashlib
import os
import pathlib
import shutil
import struct
import subprocess
import tempfile
import zlib
from typing import Callable, Dict, Iterator, List, Optional, Tuple

import hd_portrait_pack as pack
import hd_upscale
import imp_read

MAX_RECORD_NAME_LEN = 39         # the DLL's name[40]: 39 characters plus a null terminator
RENDER_BATCH = 400
READ_BUDGET = 64 << 20           # bytes of HD RGBA held at once while packing
PREP_BACKGROUND = (0x20, 0x22, 0x28)    # #202228: a neutral dark grey bleeds least visibly
ORIGINAL = "original"            # a review pick meaning "no upscale beat the original"


@dataclasses.dataclass
class Frame:
    index: int                   # the frame's index in its member, as --describe-imp numbers them
    width: int
    height: int
    stem: str                    # the prepared input's and the render's file name, without .png
    record: str                  # the pack record's name
    mirror: bool                 # searched for flipped too: its sprite's, or a repeat's that is


@dataclasses.dataclass
class Sprite:
    name: str
    member: str
    option: str
    mirror: bool
    animated: bool
    frames: List[Frame]


def static_record_name(name: str) -> str:
    return f"sprite__{name}"


def record_name(name: str, index: int) -> str:
    return f"anim__{name}#{index:03d}"


def mirrored(member: str) -> bool:
    """Map armies are drawn flipped when they face the other way. Only units were measured."""
    return member.lower().startswith("units\\")


def member_key(member: str) -> str:
    """A short, case-normalized key for one member PATH, not just its basename: two different
    members resolved for one name (see `imp_members.resolve_members`) never share a work file."""
    return hashlib.sha256(member.lower().encode("utf-8")).hexdigest()[:12]


def archive_key(archive: pathlib.Path) -> str:
    """A short, content-based key for an archive, so a work file can never leak into a different
    archive's pack -- or a rebuilt archive that kept its path and size."""
    digest = hashlib.sha256()
    with archive.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()[:16]


def frame_identity(width: int, height: int, indices: bytes, palette, key: int) -> bytes:
    """What the matcher and the upscaler see of a frame: its size, and per pixel either which of
    the two see-through kinds it is (the key, the shadow) or its colour. Two frames with the same
    identity are the same record."""
    digest = hashlib.sha1(f"{width}x{height}".encode())
    for value in indices:
        if value == key:
            digest.update(b"\x00k")
        elif value == pack.SHADOW_INDEX:
            digest.update(b"\x00s")
        else:
            digest.update(b"\x01" + bytes(palette[value]))
    return digest.digest()


def ineligible(name: str, width: int, height: int, indices: bytes, palette, key: int) -> Optional[Tuple[str, str]]:
    """(count, reason) when the DLL could not use this frame as a masked record, else None."""
    if not pack.masked_is_eligible(width, height, indices, key):
        return "ineligible", (f"{width}x{height} is too small, too large, or has no row with a long "
                              "enough run of opaque pixels for the matcher")
    # The upscale is always exactly (2w, 2h), so the DLL's limit on its side can be checked now: a
    # narrow-but-tall sprite can pass the rule above and still upscale past what the DLL loads, and
    # ONE oversized record makes the DLL refuse the WHOLE pack.
    try:
        pack.check_reader_limits(name, width, height, width * 2, height * 2, masked=True)
    except SystemExit as error:
        return "ineligible", str(error)
    if not pack.masked_probe_slices(width, height, indices, key, palette):
        return "no_probe", (f"no 8-pixel run of {pack.SPRITE_MIN_COLOURS} colours: the overlay could "
                            "make no probe, so it could never be found")
    return None


# --- the upscaler's input --------------------------------------------------------------------------

def _png_chunk(kind: bytes, body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))


def write_png_rgba(path: pathlib.Path, width: int, height: int, rgba: bytes) -> None:
    """An 8-bit RGBA PNG, whole or not at all (an existing file counts as done)."""
    stride = width * 4
    raw = b"".join(b"\x00" + rgba[y * stride:(y + 1) * stride] for y in range(height))
    data = (b"\x89PNG\r\n\x1a\n" + _png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
            + _png_chunk(b"IDAT", zlib.compress(raw, 6)) + _png_chunk(b"IEND", b""))
    part = path.with_name(path.name + ".part")
    part.write_bytes(data)
    os.replace(part, path)


def prepared_rgba(indices: bytes, palette, key: int) -> bytes:
    """The reviewed preparation of one frame, as straight RGBA: the key and the shadow index fully
    transparent over PREP_BACKGROUND (an upscaler bleeds what is under an edge into it), every
    other pixel its palette colour, opaque."""
    clear = bytes(PREP_BACKGROUND) + b"\x00"
    table = [bytes(colour) + b"\xff" for colour in palette]
    table[key] = clear
    table[pack.SHADOW_INDEX] = clear
    return b"".join(table[value] for value in indices)


# --- planning --------------------------------------------------------------------------------------

@dataclasses.dataclass
class Plan:
    static: List[Sprite]
    animated: List[Sprite]
    skipped: List[str]
    counts: Dict[str, int]


def pick(choices: dict, name: str) -> Tuple[Optional[str], Optional[str]]:
    """(option, None) for a usable pick, else (None, why not)."""
    choice = choices.get(static_record_name(name))
    if choice == ORIGINAL:
        return None, f"{name}: picked {ORIGINAL!r} in review (no upscale beat it)"
    if choice not in hd_upscale.OPTIONS:
        return None, f"{name}: no usable upscale pick ({choice!r})"
    return choice, None


def plan(resolved: Dict[str, Tuple[str, int]], read_sprite: Callable[[str], "imp_read.Sprite"],
         choices: dict, root: pathlib.Path, *, animated: bool = True,
         log: Callable[[str], None] = lambda _: None) -> Plan:
    """Every static sprite, and (`animated`) every frame of every animated one, that can be packed,
    with its upscaler input written under `root/prep`; with what was left out and why.

    `resolved` is name -> (member, frame count), as `imp_members.resolve_members` gives it;
    `read_sprite(member)` decodes one member. Members are read one at a time, in name order, so
    only one member's pixels are ever held."""
    skipped: List[str] = []
    counts = dict.fromkeys(("frames", "repeats", "ineligible", "no_probe"), 0)
    static: List[Sprite] = []
    moving: List[Sprite] = []
    seen: Dict[bytes, Frame] = {}
    prep = root / "prep"
    prep.mkdir(parents=True, exist_ok=True)

    names = sorted(name for name, (_, frames) in resolved.items() if frames == 1 or animated)
    for n, name in enumerate(names, 1):
        member, frames = resolved[name]
        is_animated = frames > 1
        option, why = pick(choices, name)
        if option is None:
            skipped.append(why)
            continue
        last = record_name(name, frames - 1) if is_animated else static_record_name(name)
        if len(last) > MAX_RECORD_NAME_LEN:
            skipped.append(f"{name}: record name {last!r} is longer than the DLL's "
                           f"{MAX_RECORD_NAME_LEN}-character limit")
            continue
        try:
            sprite = read_sprite(member)
        except (imp_read.ImpError, OSError, ValueError) as error:
            skipped.append(f"{name}: could not read {member} ({error})")
            continue
        if len(sprite.frames) != frames:
            skipped.append(f"{name}: {member} has {len(sprite.frames)} frames, not {frames}")
            continue
        mirror = is_animated and mirrored(member)
        entry = Sprite(name, member, option, mirror, is_animated, [])
        for index in range(frames):
            shown = sprite.resolved_frame(index)
            w, h, indices, key = shown.width, shown.height, shown.indices, sprite.color_key
            if is_animated:
                counts["frames"] += 1
            refused = ineligible(name, w, h, indices, sprite.palette, key)
            if refused:
                if is_animated:
                    counts[refused[0]] += 1
                else:
                    skipped.append(f"{name}: {refused[1]}")
                continue
            if is_animated:
                identity = frame_identity(w, h, indices, sprite.palette, key)
                if identity in seen:
                    counts["repeats"] += 1
                    seen[identity].mirror |= mirror
                    continue
                stem, record = f"{member_key(member)}__{name}__{index:03d}", record_name(name, index)
            else:
                stem, record = f"{member_key(member)}__{name}", static_record_name(name)
            frame = Frame(index, w, h, stem, record, mirror)
            if is_animated:
                seen[identity] = frame
            entry.frames.append(frame)
            prepped = prep / f"{stem}.png"
            if not prepped.exists():
                write_png_rgba(prepped, w, h, prepared_rgba(indices, sprite.palette, key))
        if entry.frames:
            (moving if is_animated else static).append(entry)
        if n % 200 == 0:
            log(f"{n}/{len(names)} sprites read")
    return Plan(static, moving, skipped, counts)


# --- rendering -------------------------------------------------------------------------------------

def render_all(sprites: List[Sprite], root: pathlib.Path, render, batch: int = RENDER_BATCH,
               log: Callable[[str], None] = print) -> None:
    """Upscale every planned frame with its sprite's pick, `batch` frames per call to `render`
    (`hd_upscale.render`'s signature without the model paths), into `root/render/<option>`.
    Resumes: rendered frames stay."""
    by_option: Dict[str, Dict[str, pathlib.Path]] = {}
    for sprite in sprites:
        for frame in sprite.frames:
            by_option.setdefault(sprite.option, {})[frame.stem] = root / "prep" / f"{frame.stem}.png"
    for option, inputs in sorted(by_option.items()):
        dest = root / "render" / option
        todo = {k: p for k, p in inputs.items() if not (dest / f"{k}.png").exists()}
        log(f"{option}: {len(inputs)} frames, {len(todo)} to render")
        keys = sorted(todo)
        for start in range(0, len(keys), batch):
            render(option, {k: todo[k] for k in keys[start:start + batch]}, dest)
            log(f"  {min(start + batch, len(keys))}/{len(keys)}")


# --- reading renders back ---------------------------------------------------------------------------

def read_renders(root: pathlib.Path, wanted: List[Tuple[str, int, int]]) -> Dict[str, object]:
    """rel path -> RGBA bytes, or the reason it was refused, for renders under `root` expected at
    (w, h) each. ONE magick per call, reading `@list` (never a path list on the command line:
    Windows caps it at 32,767 characters), relative to `root` so a space in the player's folder
    cannot split a name. A render not exactly (w, h) is refused, never resized."""
    out: Dict[str, object] = {}
    todo = []
    for rel, w, h in wanted:
        path = root / rel
        if not path.is_file():
            out[rel] = "render is missing"
            continue
        try:
            rw, rh = hd_upscale.png_size(path)
        except (OSError, ValueError):
            rw, rh = -1, -1
        if (rw, rh) != (w, h):
            out[rel] = f"render is {rw}x{rh}, expected {w}x{h}: made from a different original"
            continue
        todo.append((rel, w, h))
    if not todo:
        return out
    with tempfile.TemporaryDirectory(dir=root) as scratch:
        name = pathlib.Path(scratch).name
        (root / name / "list.txt").write_text("".join(f"{rel}\n" for rel, _, _ in todo))
        result = subprocess.run(["magick", f"@{name}/list.txt", "-depth", "8", "+adjoin",
                                 f"RGBA:{name}/%05d.rgba"], cwd=root, capture_output=True, text=True)
        if result.returncode != 0 and len(todo) > 1:
            # One damaged render must not cost the batch: read each on its own instead.
            for item in todo:
                out.update(read_renders(root, [item]))
            return out
        for n, (rel, w, h) in enumerate(todo):
            got = root / name / f"{n:05d}.rgba"
            data = got.read_bytes() if got.is_file() else b""
            if len(data) != w * h * 4:
                out[rel] = (f"{len(data)} bytes of RGBA, {w * h * 4} expected"
                            + (f" ({result.stderr.strip()[:200]})" if result.returncode else ""))
            else:
                out[rel] = data
    return out


def records(sprites: List[Sprite], root: pathlib.Path, read_sprite: Callable[[str], "imp_read.Sprite"],
            skipped: List[str], counts: Optional[Dict[str, int]] = None, first_group: int = 1,
            batch: int = RENDER_BATCH, budget: int = READ_BUDGET, read=None) -> Iterator[Tuple[bytes, bytes, bytes]]:
    """Yield (entry, zidx, zhd) for every planned frame, in order: static sprites group 0, each
    animated sprite its own group, consecutive. The low-res half is decoded again from the archive
    (`read_sprite`), one member at a time. A frame whose render is missing or the wrong size is
    reported in `skipped` and left out; its sprite's other frames still go in. `counts` (if given)
    gets "packed" frames and "sprites" packed."""
    counts = counts if counts is not None else {}
    counts.setdefault("packed", 0)
    counts.setdefault("sprites", 0)
    group = first_group
    queue: List[Sprite] = []
    queued = held = 0

    def flush() -> Iterator[Tuple[bytes, bytes, bytes]]:
        nonlocal group
        wanted = [(f"render/{s.option}/{f.stem}.png", f.width * 2, f.height * 2)
                  for s in queue for f in s.frames]
        pixels = (read or read_renders)(root, wanted)
        for sprite in queue:
            this_group = 0
            if sprite.animated:
                if group > pack.MAX_GROUP:
                    skipped.append(f"{sprite.name}: more than {pack.MAX_GROUP} animated sprites")
                    continue
                this_group = group
            try:
                decoded = read_sprite(sprite.member)
            except (imp_read.ImpError, OSError, ValueError) as error:
                skipped.append(f"{sprite.name}: could not read {sprite.member} ({error})")
                continue
            packed = 0
            for frame in sprite.frames:
                shown = decoded.resolved_frame(frame.index)
                rgba = pixels[f"render/{sprite.option}/{frame.stem}.png"]
                if not isinstance(rgba, bytes):
                    skipped.append(f"{frame.record}: {sprite.option} {rgba}")
                    continue
                flags = pack.FLAG_MASKED | (pack.FLAG_MIRROR if frame.mirror else 0)
                try:
                    record = pack.encode_record(frame.record, shown.width, shown.height, shown.indices,
                                                decoded.palette, shown.width * 2, shown.height * 2, rgba,
                                                flags=flags, key=decoded.color_key, group=this_group)
                except ValueError as error:
                    skipped.append(f"{frame.record}: {error}")
                    continue
                packed += 1
                yield record
            if packed:
                counts["packed"] += packed
                counts["sprites"] += 1
                if sprite.animated:
                    group += 1

    for sprite in sprites:
        queue.append(sprite)
        queued += len(sprite.frames)
        held += sum(f.width * f.height * 16 for f in sprite.frames)
        if queued >= batch or held >= budget:
            yield from flush()
            queue, queued, held = [], 0, 0
    if queue:
        yield from flush()


def archive_reader(archive) -> Callable[[str], "imp_read.Sprite"]:
    """`read_sprite` for an `mpq_read.Archive`: a member the archive cannot give is an ImpError."""
    import mpq_read

    def read_sprite(member: str) -> "imp_read.Sprite":
        try:
            data = archive.read(member.lower())
        except (mpq_read.MpqError, KeyError) as error:
            raise imp_read.ImpError(f"not readable from the archive ({error})") from error
        return imp_read.parse(data)
    return read_sprite


def clear_stale(root: pathlib.Path, keep: str) -> None:
    """Remove other archives' work folders beside `root/keep`: a changed imp.mpq (a mod, a patch)
    would otherwise leave its predecessor's frames on disk for ever."""
    if root.is_dir():
        for other in root.iterdir():
            if other.is_dir() and other.name != keep:
                shutil.rmtree(other, ignore_errors=True)
