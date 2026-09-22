#!/usr/bin/env bash
# Assemble the HD portrait overlay release zip: the overlay DLL, the setup script and the tools it
# runs on the player's machine. NO game art -- the player's machine derives it from their own
# pic.mpq -- and the zip is checked for that before it is written.
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
make -C "$FORK" clean >/dev/null
# --no-insert-timestamp: without it every build of one commit differs, so nobody could check the
# shipped DLL against the source. With it, two clean builds are byte-identical.
make -C "$FORK" -j8 LDFLAGS="-Wl,--enable-stdcall-fixup -s -static -shared -Wl,--no-insert-timestamp" >/dev/null

rm -rf "$OUT" "$OUT.zip"
mkdir -p "$OUT/tools"
cp "$FORK/ddraw.dll" "$OUT/"
cp "$ROOT/release/hd-overlay/"{lomhd_setup.py,README.md,NOTICES.md,portrait-names.txt} "$OUT/"
cp "$ROOT/tools/"{mpq_read.py,hd_portrait_pack.py} "$ROOT/tools/portrait-upscale/"{upscale.py,lbm_png.py} "$OUT/tools/"
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

# No game art, ever: refuse the build if anything that could hold it slipped in.
if find "$OUT" \( -iname '*.lbm' -o -iname '*.mpq' -o -iname '*.pack' -o -iname '*.png' -o -iname '*.raw' \) | grep -q .; then
  echo "refusing: game-derived files in $OUT" >&2; exit 1
fi

(cd "$ROOT/dist" && zip -qr "$NAME.zip" "$NAME")
echo "$ROOT/dist/$NAME.zip ($(du -h "$ROOT/dist/$NAME.zip" | cut -f1)), cnc-ddraw $COMMIT"
