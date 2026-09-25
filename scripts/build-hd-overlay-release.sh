#!/usr/bin/env bash
# Assemble the HD portrait overlay release zip: the overlay DLL, the setup script and the tools it
# runs on the player's machine. NO game art -- the player's machine derives it from their own
# pic.mpq and imp.mpq -- and the zip is checked for that before it is written.
#
#   scripts/build-hd-overlay-release.sh VERSION [CNC_DDRAW_REPO]
set -euo pipefail

VERSION=${1:?usage: $0 VERSION [CNC_DDRAW_REPO]}
FORK=${2:-$HOME/personal-projects/cnc-ddraw-lom}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NAME=lomhd-portraits-$VERSION
OUT=$ROOT/dist/$NAME

# The DLL is built from a clean tree, from scratch: a stale object once linked two layouts of one
# struct into a DLL that still loaded and ran (docs/hd-overlay.md).
[ -z "$(git -C "$FORK" status --porcelain)" ] || { echo "cnc-ddraw fork has uncommitted changes" >&2; exit 1; }
COMMIT=$(git -C "$FORK" rev-parse --short HEAD)
LDFLAGS_REPRO="-Wl,--enable-stdcall-fixup -s -static -shared -Wl,--no-insert-timestamp"
build_dll() {
  make -C "$FORK" clean >/dev/null
  make -C "$FORK" -j8 LDFLAGS="$LDFLAGS_REPRO" >/dev/null
  shasum -a 256 "$FORK/ddraw.dll" | cut -d' ' -f1
}
# --no-insert-timestamp: without it every build of one commit differs, so nobody could check the
# shipped DLL against the source. Built twice and compared, because the claim is only worth making
# per release: on 2026-09-22 one build in ten came out different for a reason not yet found.
FIRST=$(build_dll)
SECOND=$(build_dll)
[ "$FIRST" = "$SECOND" ] || { echo "refusing: two clean builds of $COMMIT differ ($FIRST vs $SECOND)" >&2; exit 1; }
# `lomhd_setup.py --terrain` installs art only this DLL can serve: a DLL without the lomhd_terrain
# folder support would leave a patched lomse.exe drawing scrambled terrain.
grep -qa 'art from lomhd_terrain' "$FORK/ddraw.dll" ||
  { echo "refusing: $COMMIT's ddraw.dll cannot serve lomhd_terrain (build from the fork's HD-terrain branch)" >&2; exit 1; }

rm -rf "$OUT" "$OUT.zip"
mkdir -p "$OUT/tools" "$OUT/exe_patches"
cp "$FORK/ddraw.dll" "$OUT/"
cp "$ROOT/release/hd-overlay/"{lomhd_setup.py,README.md,NOTICES.md,overlay-names.txt,terrain-names.txt,imp-names.txt,upscale-choices.json} "$OUT/"
cp "$ROOT/tools/"{mpq_read.py,hd_portrait_pack.py,hd_upscale.py,exe_patch.py,terrain_hd.py,imp_read.py,hd_sprites.py,imp_members.py} "$ROOT/tools/portrait-upscale/"{upscale.py,lbm_png.py} "$ROOT/tools/hd-review/"{serve.py,review.html} "$OUT/tools/"
# The terrain patch sets ship as JSON: setup promises Python 3.9, and tomllib is 3.11+. exe_patch
# loads a .json set through the same validation as the .toml it came from.
for set in terrain-hybrid-2x terrain-stride-1024; do
  python3 "$ROOT/tools/exe_patch.py" json "$ROOT/tools/exe_patches/$set.toml" "$OUT/exe_patches/$set.json" >/dev/null
done
cp "$FORK/LICENSE" "$OUT/LICENSE-cnc-ddraw.txt"
python3 - "$OUT" "$VERSION" "$COMMIT" <<'PY'
import hashlib, json, pathlib, sys
out, version, commit = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
(out / "release.json").write_text(json.dumps({
    "version": version,
    "cnc_ddraw_commit": commit,
    "ddraw_sha256": hashlib.sha256((out / "ddraw.dll").read_bytes()).hexdigest(),
}, indent=2) + "\n")
PY

# No game art or game binary, ever: refuse the build if anything that could hold it slipped in.
if find "$OUT" \( -iname '*.lbm' -o -iname '*.mpq' -o -iname '*.pack' -o -iname '*.png' -o -iname '*.raw' \
                 -o -iname '*.til' -o -iname '*.imp' -o -iname '*.rgba' -o -iname '*.exe' -o -iname '*.lomhd-backup' \) | grep -q .; then
  echo "refusing: game-derived files in $OUT" >&2; exit 1
fi
# And nothing that is not on this list: a new file has to be added here on purpose.
EXPECTED="LICENSE-cnc-ddraw.txt NOTICES.md README.md ddraw.dll exe_patches/terrain-hybrid-2x.json
exe_patches/terrain-stride-1024.json imp-names.txt lomhd_setup.py overlay-names.txt release.json
terrain-names.txt tools/exe_patch.py tools/hd_portrait_pack.py tools/hd_sprites.py tools/hd_upscale.py
tools/imp_members.py tools/imp_read.py tools/lbm_png.py tools/mpq_read.py tools/review.html tools/serve.py
tools/terrain_hd.py tools/upscale.py upscale-choices.json"
GOT=$(cd "$OUT" && find . -type f | sed 's|^\./||' | LC_ALL=C sort | tr '\n' ' ')
WANT=$(echo "$EXPECTED" | tr ' ' '\n' | LC_ALL=C sort | tr '\n' ' ')
[ "$GOT" = "$WANT" ] || { echo "refusing: $OUT holds [$GOT], expected [$WANT]" >&2; exit 1; }

(cd "$ROOT/dist" && zip -qr "$NAME.zip" "$NAME")
echo "$ROOT/dist/$NAME.zip ($(du -h "$ROOT/dist/$NAME.zip" | cut -f1)), cnc-ddraw $COMMIT"
