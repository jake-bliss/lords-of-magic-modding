#!/usr/bin/env python3
"""The interface's small icons, cut from the sprite SHEETS they live on, as format-5 pack records.

    python3 tools/hd-review/sheet_icons.py OUT.pack --lbm LBM_DIR --scripts GS_DIR \
        --esrgan realesrgan-ncnn-vulkan --models MODELS_DIR

A dev tool, like `sprite_pack.py`: not part of the player release yet.

Why (2026-09-23, over 93 captured frames): the map bar's arrows, zoom and footprint buttons, the
eye, the status icons, all of it is drawn from a handful of 640-wide sheets -- `intspr1`,
`eoturn`, `staticon` and its GS5R3 variants -- one rectangle at a time. The overlay only knows a
picture drawn whole or cut off at an edge, so an icon taken from the middle of a sheet was never
found, though the sheets themselves were packed. Cut into their icons, 28 of them were found
exactly (>= 90% of their pixels) in the same frames.

Where the rectangles come from, in order:

- The game's own scripts: `/intspr1_page"lbm/intspr1.lbm"lbm def` names a sheet, and
  `intspr1_page 341 0 70 67 doodad` cuts one icon from it. These are exact, and the only way to
  split icons that touch each other on the sheet (the eight faith emblems do).
- For the rest -- rectangles the scripts compute, or that native code cuts -- each separate shape on
  the sheet's key colour. Shapes too small to hold a probe (the developers' text labels) fall out
  by the same rule as every sprite.

Each icon is a MASKED record keyed on index 0, the pure-green chroma key of every UI sheet (not the
most common index: on `label` that is a real colour). A sheet whose index 0 is not that green is
refused. Each sheet is upscaled ONCE, with the upscaler picked for it in review
(`screen__<sheet>`), key colour and index 1 turned neutral grey as for sprites, and every icon is cropped from
that at 2x -- one model per sheet, so neighbouring icons match. An icon identical to one already cut
(staticon and staticon5 share most of theirs) is packed once.
"""
from __future__ import annotations

import argparse
import collections
import dataclasses
import hashlib
import json
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(ROOT / "tools" / "portrait-upscale"))
sys.path.insert(0, str(HERE))
import hd_portrait_pack as pack  # noqa: E402
import hd_upscale  # noqa: E402
import lbm_png  # noqa: E402

SHEETS = ("intspr1", "eoturn", "staticon", "staticon5", "staticon5r3a", "indicate", "barter",
          "editbits", "label", "slidtest")
WORK = ROOT / "artifacts" / "hd-review" / "_sheets"
PREP_BACKGROUND = (0x20, 0x22, 0x28)     # sprite_originals' neutral grey
PREP_VERSION = 2                         # bump when the upscaler's input changes: renders are cached
CHROMA_KEY = (0, 255, 0)                 # index 0 on every UI sheet the game keys
MAX_RECORD_NAME_LEN = 39
MIN_CUT = (8, 4)                         # the pack's smallest masked record

PAGE = re.compile(r'/(\w+)\s*"([^"]+\.lbm)"\s*lbm\b', re.I)
CUT = re.compile(r'\b(\w+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+doodad\b')
COMMENT = re.compile(r';[^\r\n]*')


@dataclasses.dataclass(frozen=True)
class Icon:
    sheet: str
    x: int
    y: int
    w: int
    h: int
    source: str                  # "script" or "shape"

    @property
    def record(self) -> str:
        return f"icon__{self.sheet}@{self.x},{self.y},{self.w}x{self.h}"


def script_cuts(texts) -> dict[str, set[tuple[int, int, int, int]]]:
    """sheet -> literal (x, y, w, h) cuts, from the text of every script. Any variable bound to an
    LBM is a page -- the scripts name them freely (`/unitinfo_staticon"LBM/STATICON.lbm"lbm def`,
    `/eoturnbuttonpage...`; Claude review, 2026-09-23). A variable bound to more than one file is
    ambiguous and ignored, and so is anything after a `;`: commented-out cuts are not drawn."""
    files: dict[str, set[str]] = collections.defaultdict(set)
    cuts: dict[str, set[tuple[int, int, int, int]]] = collections.defaultdict(set)
    for text in texts:
        text = COMMENT.sub("", text)
        for page, path in PAGE.findall(text):
            files[page].add(pathlib.PureWindowsPath(path.replace("/", "\\")).stem.lower())
        for page, *rect in CUT.findall(text):
            cuts[page].add(tuple(int(v) for v in rect))
    out: dict[str, set] = collections.defaultdict(set)
    for page, rects in cuts.items():
        if len(files.get(page, ())) == 1:
            out[next(iter(files[page]))] |= rects
    return out


def shapes(w: int, h: int, idx: bytes, key: int) -> list[tuple[int, int, int, int]]:
    """The bounding box of every 8-connected region of non-key pixels."""
    seen = bytearray(w * h)
    out = []
    for start in range(w * h):
        if seen[start] or idx[start] == key:
            continue
        seen[start] = 1
        stack = [start]
        x0 = x1 = start % w
        y0 = y1 = start // w
        while stack:
            p = stack.pop()
            x, y = p % w, p // w
            x0, x1, y0, y1 = min(x0, x), max(x1, x), min(y0, y), max(y1, y)
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h:
                        q = ny * w + nx
                        if not seen[q] and idx[q] != key:
                            seen[q] = 1
                            stack.append(q)
        out.append((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    return out


def overlaps(a, b) -> bool:
    return a[0] < b[0] + b[2] and b[0] < a[0] + a[2] and a[1] < b[1] + b[3] and b[1] < a[1] + a[3]


def icons_of(sheet: str, w: int, h: int, idx: bytes, key: int, cuts) -> list[Icon]:
    """The script's cuts that fit the sheet, then every shape no cut touches. A cut inside another
    cut from the same corner (the scripts cut some buttons at 26x17 and 27x18) is the larger one.
    A cut of half the sheet or more is the scripts loading the page, and one under MIN_CUT a
    placeholder (`eoturnbuttonpage 0 0 373 309` and `200 0 1 1`): neither is an icon, and neither
    may hide the shapes under it -- the first hid the gem wheel."""
    rects = [r for r in sorted(cuts) if r[0] + r[2] <= w and r[1] + r[3] <= h and
             r[2] >= MIN_CUT[0] and r[3] >= MIN_CUT[1] and 2 * r[2] * r[3] < w * h]
    rects = [r for r in rects if not any(o != r and o[:2] == r[:2] and o[2] >= r[2] and o[3] >= r[3]
                                         for o in rects)]
    icons = [Icon(sheet, *r, "script") for r in rects]
    for r in shapes(w, h, idx, key):
        if not any(overlaps(r, c) for c in rects):
            icons.append(Icon(sheet, *r, "shape"))
    return icons


def crop(idx: bytes, w: int, icon: Icon) -> bytes:
    return b"".join(idx[(icon.y + j) * w + icon.x:(icon.y + j) * w + icon.x + icon.w] for j in range(icon.h))


def identity(indices: bytes, palette, key: int, w: int, h: int) -> bytes:
    digest = hashlib.sha1(f"{w}x{h}".encode())
    for v in indices:
        digest.update(b"\x00k" if v == key else b"\x00s" if v == pack.SHADOW_INDEX else
                      b"\x01" + bytes(palette[v]))
    return digest.digest()


def load_sheet(lbm_dir: pathlib.Path, sheet: str):
    """(w, h, indices, 256-entry palette), or None if the install has no such sheet."""
    path = next((p for p in lbm_dir.iterdir() if p.stem.lower() == sheet and p.suffix.lower() == ".lbm"), None)
    if path is None:
        return None
    w, h, idx, pal, _ = lbm_png.decode(path)
    return w, h, bytes(idx), list(pal) + [(0, 0, 0)] * (256 - len(pal))


def plan(lbm_dir: pathlib.Path, cuts, skipped: list[str]):
    """[(sheet, w, h, idx, palette, key, [Icon])] -- every icon the overlay could find. Repeats
    across sheets are dropped later, by `records`, over the icons actually packed."""
    out = []
    for sheet in SHEETS:
        loaded = load_sheet(lbm_dir, sheet)
        if loaded is None:
            skipped.append(f"{sheet}: not in this install")
            continue
        w, h, idx, pal = loaded
        if tuple(pal[0]) != CHROMA_KEY:
            skipped.append(f"{sheet}: index 0 is {tuple(pal[0])}, not the chroma key")
            continue
        key = 0        # not the most common index: on `label` that is a real colour (Claude review)
        keep = []
        for icon in icons_of(sheet, w, h, idx, key, cuts.get(sheet, ())):
            cell = crop(idx, w, icon)
            if len(icon.record) > MAX_RECORD_NAME_LEN:
                skipped.append(f"{icon.record}: name too long")
                continue
            if not pack.masked_is_eligible(icon.w, icon.h, cell, key) or \
                    not pack.masked_probe_slices(icon.w, icon.h, cell, key, pal):
                continue                                  # a label, a dot: never findable
            try:
                pack.check_reader_limits(icon.record, icon.w, icon.h, icon.w * 2, icon.h * 2, masked=True)
            except SystemExit as error:
                skipped.append(f"{icon.record}: {error}")
                continue
            keep.append(icon)
        out.append((sheet, w, h, idx, pal, key, keep))
    return out


def prepared_png(dest: pathlib.Path, w: int, h: int, idx: bytes, pal, key: int) -> None:
    """The sheet as the upscaler sees it: full colour, the key colour and index 1 neutral grey.
    Index 1 is pure red on every sheet and the overlay never draws it (masked records skip it, as
    for sprites), but left red it bleeds into the pixels beside it (Claude review, 2026-09-23).
    The game does not draw it as red: over 93 frames no bar icon shows a run of it."""
    rows = [[PREP_BACKGROUND if v in (key, pack.SHADOW_INDEX) else tuple(pal[v])
             for v in idx[y * w:(y + 1) * w]] for y in range(h)]
    lbm_png.write_png(dest, w, h, rows)


def records(planned, renders: dict[str, pathlib.Path], skipped: list[str]):
    """Yield (entry, zidx, zhd) for every planned icon, cropped from its sheet's 2x render. An icon
    identical to one already yielded -- staticon and staticon5 share most -- is yielded once."""
    seen: set[bytes] = set()
    for sheet, w, h, idx, pal, key, icons in planned:
        render = renders.get(sheet)
        if render is None or not icons:
            continue
        size = subprocess.run(["magick", "identify", "-format", "%w %h", str(render)], check=True,
                              capture_output=True, text=True).stdout.split()
        if [int(v) for v in size] != [2 * w, 2 * h]:      # the size, not the byte count: a 40x20
            skipped.append(f"{sheet}: render is not {2 * w}x{2 * h}")   # sheet is 20x40's bytes (Codex)
            continue
        rgb = subprocess.run(["magick", str(render), "-depth", "8", "rgb:-"], check=True,
                             capture_output=True).stdout
        for icon in icons:
            cell = crop(idx, w, icon)
            ident = identity(cell, pal, key, icon.w, icon.h)
            if ident in seen:
                continue
            seen.add(ident)
            rgba = bytearray()
            for j in range(icon.h * 2):
                at = ((icon.y * 2 + j) * w * 2 + icon.x * 2) * 3
                row = rgb[at:at + icon.w * 2 * 3]
                for i in range(icon.w * 2):
                    rgba += row[i * 3:i * 3 + 3] + b"\xff"
            yield pack.encode_record(icon.record, icon.w, icon.h, cell, pal,
                                     icon.w * 2, icon.h * 2, bytes(rgba), flags=pack.FLAG_MASKED,
                                     key=key, group=0)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--lbm", type=pathlib.Path, required=True, help="the install's pic.mpq LBM folder")
    parser.add_argument("--scripts", type=pathlib.Path, required=True, help="the install's extracted .gs scripts")
    parser.add_argument("--choices", type=pathlib.Path,
                        default=ROOT / "release" / "hd-overlay" / "upscale-choices.json")
    parser.add_argument("--esrgan", type=pathlib.Path, required=True)
    parser.add_argument("--models", type=pathlib.Path, required=True)
    args = parser.parse_args()

    texts = [p.read_bytes().decode("latin-1") for p in sorted(args.scripts.rglob("*")) if p.suffix.lower() == ".gs"]
    cuts = script_cuts(texts)
    skipped: list[str] = []
    planned = plan(args.lbm, cuts, skipped)
    choices = json.loads(args.choices.read_text())["choices"]

    renders = {}
    for sheet, w, h, idx, pal, key, icons in planned:
        option = choices.get(f"screen__{sheet}")
        if option not in hd_upscale.OPTIONS:
            skipped.append(f"{sheet}: no usable upscale pick ({option!r})")
            continue
        digest = hashlib.sha256(idx + bytes(v for c in pal for v in c) +
                                f"{w}x{h}|{key}|{PREP_BACKGROUND}|{PREP_VERSION}".encode()).hexdigest()[:16]
        folder = WORK / digest
        folder.mkdir(parents=True, exist_ok=True)
        src = folder / f"{sheet}.png"
        if not src.exists():
            prepared_png(src, w, h, idx, pal, key)
        hd_upscale.render(option, {sheet: src}, folder / option, args.esrgan, args.models)
        renders[sheet] = folder / option / f"{sheet}.png"
        by = collections.Counter(i.source for i in icons)
        print(f"{sheet}: {len(icons)} icons ({by['script']} script cuts, {by['shape']} shapes), {option}")

    n = pack.write_records(args.out, records(planned, renders, skipped))
    print(f"{n} icons packed; {len(skipped)} skipped")
    for line in skipped:
        print("  " + line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
