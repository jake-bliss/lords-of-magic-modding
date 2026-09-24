#!/usr/bin/env bash
# Stage the 2x terrain atlases and their .til files into this mod's pic.mpq tree.
#
# The art is derived from the game's own atlases, so it never enters git (mods/*/archives/ is
# ignored). tools/terrain_hd.py builds the stage: every tile upscaled alone with the reviewed
# anime2x/anime4x choice, then quantized back to its atlas's palette without crossing a tile edge.
#
#   python3 tools/terrain_hd.py SRC OUT --esrgan PATH --models DIR
#
# Re-runnable. Refuses a stage that is missing any atlas a .til names, or whose atlas is not exactly
# the .til's TILES grid at 64px, because with tools/exe_patches/terrain-stride-1024.toml in the
# binary EVERY sampled atlas must be 1024 wide -- one left at 512 renders sheared garbage.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
mod=mods/terrain-hd-art
stage="${LOM_TERRAIN_HD_STAGE:-${HOME}/personal-projects/backups/terrain-hd-20260924/pertile/stage}"
[[ -d "${stage}" ]] || { echo "no stage at ${stage} (set LOM_TERRAIN_HD_STAGE)" >&2; exit 1; }

out="${mod}/archives/pic.mpq/til"
rm -rf "${mod}/archives"
mkdir -p "${out}"
python3 - "${stage}" "${out}" <<'PY'
import pathlib, re, shutil, struct, sys

stage, out = map(pathlib.Path, sys.argv[1:])

def lbm_size(path):
    data = path.read_bytes()
    assert data[:4] == b"FORM" and data[8:12] in (b"PBM ", b"ILBM"), path
    i = 12
    while i < len(data):
        tag, n = data[i:i + 4], struct.unpack(">I", data[i + 4:i + 8])[0]
        if tag == b"BMHD":
            return struct.unpack(">HH", data[i + 8:i + 12])
        i += 8 + n + (n & 1)
    raise SystemExit(f"{path.name}: no BMHD")

SHORT_IN_THE_ORIGINAL = {"jeff01.lbm": 2 * 480}
tils = sorted(stage.glob("*.til"))
lbms = {p.name.lower(): p for p in stage.glob("*.lbm")}
assert len(tils) == 26 and len(lbms) == 20, (len(tils), len(lbms))
named = set()
for til in tils:
    text = til.read_bytes().decode("latin-1")
    lbm = re.search(r"^LBM=\s*(\S+)", text, re.M | re.I)
    size = re.search(r"TILESIZE=\s*(\d+),\s*(\d+)", text)
    grid = re.search(r"TILES=\s*(\d+),\s*(\d+)", text)
    assert lbm and size and grid, til.name
    name = lbm[1].strip().lower()
    assert name in lbms, f"{til.name} names {name}, not staged"
    assert (int(size[1]), int(size[2])) == (64, 64), f"{til.name}: TILESIZE {size[1]},{size[2]}"
    named.add(name)
    # The rasterizer reads TILES x 64 texels; an atlas shorter than that is read past its end.
    # One shipped tileset already is: jeff01.til declares 16 rows (512px at 1x) over a 480-tall
    # atlas, so its last row of tiles was never whole. It is held to exactly 2x ITS height instead.
    w, h = lbm_size(lbms[name])
    want = (int(grid[1]) * 64, SHORT_IN_THE_ORIGINAL.get(name, int(grid[2]) * 64))
    assert (w, h) == want, f"{til.name}: {name} is {w}x{h}, want {want[0]}x{want[1]}"
for name, path in sorted(lbms.items()):
    w, h = lbm_size(path)
    assert w == 1024, f"{name}: {w} wide -- every sampled atlas must be 1024"
    shutil.copyfile(path, out / name)
for til in tils:
    shutil.copyfile(til, out / til.name.lower())
unused = sorted(set(lbms) - named)
print(f"staged {len(lbms)} atlases, {len(tils)} tilesets; atlases no .til names: {unused or 'none'}")
PY
