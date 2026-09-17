#!/usr/bin/env bash
# Install the sprite placement / palette probe into the GS5R3 game archives.
#
# Refuses to touch anything unless gs.mpq and imp.mpq match the recorded originals, so a probe
# can never be layered on top of a previous one. Undo with scripts/restore-game-archives.sh.
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# artifacts/ is gitignored, so a fresh worktree has neither the backups nor the listfile.
# Point LOM_ARTIFACTS_DIR at the main checkout's artifacts directory when running from one.
artifacts_dir="${LOM_ARTIFACTS_DIR:-${project_dir}/artifacts}"
backup_dir="${artifacts_dir}/experiment-backups/gs5r3-20260916"
app_dir="${1:-${HOME}/Applications/Lords of Magic GS5R3.app}"
game_subpath='Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English'
game_dir="${app_dir}/${game_subpath}"
viewer_dir="${project_dir}/spikes/asset-viewer"
listfile="${artifacts_dir}/reference-listfiles/lords-of-magic.txt"
# shellcheck source=scripts/lib-game-archives.sh
source "${project_dir}/scripts/lib-game-archives.sh"

work_dir="$(mktemp -d)"
# Injection is not atomic: gs.mpq and imp.mpq are written by four separate calls. Anything that
# fails after the first write would otherwise leave the game half-modified while reporting failure,
# so the exit trap rolls both archives back unless the script reached the end.
installed=0
cleanup() {
  local status=$?
  if (( status != 0 )) && (( installed == 0 )); then
    echo "install failed; rolling the archives back" >&2
    restore_archives "${backup_dir}" "${game_dir}" >&2 || true
  fi
  rm -rf "${work_dir}"
  exit "${status}"
}
trap cleanup EXIT

for path in "${game_dir}/gs.mpq" "${game_dir}/imp.mpq" "${backup_dir}/gs.mpq.orig" \
            "${backup_dir}/imp.mpq.orig" "${listfile}"; do
  [[ -f "${path}" ]] || { echo "missing: ${path}" >&2; exit 1; }
done

echo "== verifying the backups against MANIFEST.sha256 =="
verify_backups "${backup_dir}"

echo "== verifying the archives are pristine =="
for archive in "${ARCHIVE_NAMES[@]}"; do
  live="$(file_hash "${game_dir}/${archive}.mpq")"
  original="$(file_hash "${backup_dir}/${archive}.mpq.orig")"
  if [[ "${live}" != "${original}" ]]; then
    echo "${archive}.mpq does not match the recorded original; restore before installing." >&2
    exit 1
  fi
  echo "  ${archive}.mpq ${live}"
done

echo "== building tools =="
(cd "${viewer_dir}" && cargo build --release --quiet)
(cd "${viewer_dir}" && cargo build --release --quiet --example mpq_replace --example author_palette)
viewer="${viewer_dir}/target/release/lom-asset-viewer"
mpq_replace="${viewer_dir}/target/release/examples/mpq_replace"
author_palette="${viewer_dir}/target/release/examples/author_palette"

echo "== preparing sprites =="
# zzctl.imp is byte-identical to the donor: it isolates "an added member cannot be read" from
# "our palette edit broke the file". zzpal.imp carries five raw-byte palette entries.
"${viewer}" --extract "${game_dir}/imp.mpq" 'imp\tree4e.imp' "${work_dir}/tree4e.imp" \
  --listfile "${listfile}" >/dev/null
cp "${work_dir}/tree4e.imp" "${work_dir}/zzctl.imp"
"${author_palette}" "${work_dir}/tree4e.imp" "${work_dir}/zzpal.imp"
mkdir -p "${artifacts_dir}"
# The index map records which frame pixels carry each authored palette entry, so the capture
# can be read without guessing which blob is which colour.
cp "${work_dir}/zzpal.imp.map" "${artifacts_dir}/zzpal-index-map.txt"

echo "== preparing scripts =="
"${viewer}" --extract "${game_dir}/gs.mpq" 'gs\hotkey.gs' "${work_dir}/hotkey.gs" \
  --listfile "${listfile}" >/dev/null
"${viewer}" --extract "${game_dir}/gs.mpq" 'START.GS' "${work_dir}/START.GS" \
  --listfile "${listfile}" >/dev/null
PYTHONPATH="${project_dir}/tools" python3 - "${work_dir}" <<'PY'
import pathlib
import sys

import engine_probe

work = pathlib.Path(sys.argv[1])
hotkey = (work / "hotkey.gs").read_text(encoding="latin-1")
(work / "hotkey_probe.gs").write_text(engine_probe.install(hotkey), encoding="latin-1")
start = (work / "START.GS").read_text(encoding="latin-1")
(work / "START_fast.GS").write_text(engine_probe.disable_intro(start), encoding="latin-1")
print("  probe installed into hotkey.gs; intro movies disabled in START.GS")
PY

echo "== injecting =="
"${mpq_replace}" "${game_dir}/imp.mpq" 'imp\zzctl.imp' "${work_dir}/zzctl.imp"
"${mpq_replace}" "${game_dir}/imp.mpq" 'imp\zzpal.imp' "${work_dir}/zzpal.imp"
"${mpq_replace}" "${game_dir}/gs.mpq" 'gs\hotkey.gs' "${work_dir}/hotkey_probe.gs"
"${mpq_replace}" "${game_dir}/gs.mpq" 'START.GS' "${work_dir}/START_fast.GS"

echo "== verifying read-back =="
"${viewer}" --extract "${game_dir}/gs.mpq" 'gs\hotkey.gs' "${work_dir}/rb_hotkey.gs" \
  --listfile "${listfile}" >/dev/null
"${viewer}" --extract "${game_dir}/gs.mpq" 'START.GS' "${work_dir}/rb_START.GS" \
  --listfile "${listfile}" >/dev/null
cmp "${work_dir}/rb_hotkey.gs" "${work_dir}/hotkey_probe.gs"
cmp "${work_dir}/rb_START.GS" "${work_dir}/START_fast.GS"
echo "  scripts read back byte-identical"

# Newly added members are findable by hash but not by the listfile, so check them that way.
(cd "${viewer_dir}" && cargo run --release --quiet --example read_member -- \
  "${game_dir}/imp.mpq" 'imp\zzctl.imp')
(cd "${viewer_dir}" && cargo run --release --quiet --example read_member -- \
  "${game_dir}/imp.mpq" 'imp\zzpal.imp')

installed=1

echo
echo "Ready. Launch 'Lords of Magic GS5R3.app', start a single-player game, reach the world map,"
echo "and TAP z once. The probe now fires only once per launch even if the key repeats."
echo "Afterwards run scripts/restore-game-archives.sh."
