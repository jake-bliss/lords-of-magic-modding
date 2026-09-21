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
# Which probe to install: "ladder" (the four-rung compositing diagnostic), "elevation",
# "mapsize" (the oversized-map ladder, issue #22 -- now closed), "flatground" (the built-mesh
# probe that closes the map2screen y convention), "maptag" (the cell-tag and trailing-record probe
# for issue #4), "mapload" (does the engine accept a map THIS PROJECT wrote?), "terrainrings"
# (the full 11x11 setterrain transition matrix, plus the 0x00800000 call isolation and a dump of the
# script-assigned sprite-type table), or "unitanchor" (does the UNIT draw path apply the same
# hotspot-record-0 anchor as the terrain-sprite path hotspots.md's own record-0 confirmation used?).
#
# "mapload" is the only probe with prerequisites: its rungs 1-6 load files that must already be in
# the game's map/ directory, built by scripts/build-mapload-inputs.sh. Installing it without them
# would spend the user's attended session loading files that are not there, so this script refuses.
probe="${LOM_PROBE:-ladder}"
game_subpath='Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English'
game_dir="${app_dir}/${game_subpath}"
viewer_dir="${project_dir}/spikes/asset-viewer"
listfile="${artifacts_dir}/reference-listfiles/lords-of-magic.txt"
# shellcheck source=scripts/lib-game-archives.sh
source "${project_dir}/scripts/lib-game-archives.sh"

work_dir="$(mktemp -d)"
# Injection is not atomic: gs.mpq and imp.mpq are written by four separate calls. Anything that
# fails after the first write would otherwise leave the game half-modified while reporting failure,
# so the exit trap rolls both archives back.
#
# `writing` gates that rollback, and it is only set immediately before the first write. Rolling back
# on a path that never wrote anything would be actively destructive: refusing a non-pristine archive
# (say, a different installation passed as app_dir) would then copy THIS machine's archives over it,
# and a failed backup verification would write an unverified backup over a live game -- the exact
# thing the verification exists to prevent.
installed=0
writing=0
cleanup() {
  local status=$?
  if (( status != 0 )) && (( installed == 0 )) && (( writing == 1 )); then
    echo "install failed after writing; rolling the archives back" >&2
    if verify_backups "${backup_dir}"; then
      restore_archives "${backup_dir}" "${game_dir}" >&2 || true
    else
      echo "BACKUPS DID NOT VERIFY; leaving the archives as they are rather than making it worse." >&2
      echo "The game archives are modified. Restore them by hand from a known-good copy." >&2
    fi
  fi
  rm -rf "${work_dir}"
  exit "${status}"
}
trap cleanup EXIT

for path in "${game_dir}/gs.mpq" "${game_dir}/imp.mpq" "${backup_dir}/gs.mpq.orig" \
            "${backup_dir}/imp.mpq.orig" "${listfile}"; do
  [[ -f "${path}" ]] || { echo "missing: ${path}" >&2; exit 1; }
done

if [[ "${probe}" == "mapload" ]]; then
  echo "== the mapload probe needs its input maps in place first =="
  # Read from the probe generator, never inline: see build-mapload-inputs.sh for what drifting
  # copies of this list would cost.
  mapfile -t input_names < <(PYTHONPATH="${project_dir}/tools" python3 -c \
    'import engine_probe; print("\n".join(engine_probe.mapload_prebuilt_names()))')
  (( ${#input_names[@]} > 0 )) || { echo "probe input list is empty" >&2; exit 1; }
  missing=0
  for name in "${input_names[@]}"; do
    if [[ ! -f "${game_dir}/map/${name}" ]]; then
      echo "   MISSING: ${game_dir}/map/${name}" >&2
      missing=1
    fi
  done
  if (( missing )); then
    echo "" >&2
    echo "Run this first, then install again:" >&2
    echo "  scripts/build-mapload-inputs.sh '${app_dir}'" >&2
    exit 1
  fi
  # zm0 is the engine's own control, written during the run. A leftover from a previous run would
  # be loaded at rung 0 instead of a freshly saved one, which silently removes the control.
  if [[ -e "${game_dir}/map/zm0.scn" ]]; then
    echo "refusing to install: ${game_dir}/map/zm0.scn already exists." >&2
    echo "Rung 0's control must be written by the engine during the run, not left over." >&2
    exit 1
  fi
  echo "   all ${#input_names[@]} input maps present, and no stale zm0.scn"
fi

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

echo "== preparing sprites (probe: ${probe}) =="
# The elevation probe places shipped art through a custom type, which the ladder run proved
# renders, so it needs no injected sprites at all -- two fewer archive writes.
if [[ "${probe}" == "ladder" ]]; then
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
fi

echo "== preparing scripts =="
"${viewer}" --extract "${game_dir}/gs.mpq" 'gs\hotkey.gs' "${work_dir}/hotkey.gs" \
  --listfile "${listfile}" >/dev/null
"${viewer}" --extract "${game_dir}/gs.mpq" 'START.GS' "${work_dir}/START.GS" \
  --listfile "${listfile}" >/dev/null
PYTHONPATH="${project_dir}/tools" LOM_PROBE="${probe}" python3 - "${work_dir}" <<'PY'
import os
import pathlib
import sys

import engine_probe

work = pathlib.Path(sys.argv[1])
hotkey = (work / "hotkey.gs").read_text(encoding="latin-1")
(work / "hotkey_probe.gs").write_text(
    engine_probe.install(hotkey, probe=os.environ.get("LOM_PROBE", "ladder")), encoding="latin-1"
)
start = (work / "START.GS").read_text(encoding="latin-1")
(work / "START_fast.GS").write_text(engine_probe.disable_intro(start), encoding="latin-1")
print(f"  {os.environ.get('LOM_PROBE', 'ladder')} probe installed into hotkey.gs; "
      "intro movies disabled in START.GS")
PY

# `screencapture` refuses to overwrite, so a stale capture from an earlier attempt would survive the
# run and be collected as if it were this run's output -- a plate diffed against itself reads as
# "the sprite did not render", which is the exact conclusion this probe exists to test.
#
# Removed by an EXACT list of names taken from the probe generator itself, never a `z*.bmp` glob.
# A glob is a standing offer to delete a file this project never created -- a user's own
# `English/zReference.bmp` sitting in the same directory, say -- and nothing could bring it back.
# Same provenance principle as the loose `map/` cleanup below.
echo "== clearing stale probe output =="
while IFS= read -r capture_name; do
  stale="${game_dir}/${capture_name}"
  [[ -e "${stale}" ]] || continue
  echo "  removing stale ${capture_name}"
  rm -f "${stale}"
done < <(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import engine_probe; print("\n".join(engine_probe.all_capture_names()))')
# The mapsize probe writes into the game's loose map/ directory, which holds 366 shipped files and
# no backup here covers it -- the manifest covers gs.mpq and imp.mpq only.
#
# So this removes an EXACT list of names, taken from the probe generator itself, and never a glob.
# A `zz*.scn` glob is a standing offer to delete somebody's own `zzCustom.scn`, and nothing could
# bring it back. One source of truth means the list cannot drift from what the probe writes.
#
# OUTPUTS ONLY. The mapload probe's inputs are also probe-created files, and restore removes them,
# but they must exist when the game starts -- clearing them here deleted the six maps whose presence
# this script had just verified, which would have spent an attended session loading nothing.
while IFS= read -r map_name; do
  stale="${game_dir}/${map_name}"
  [[ -e "${stale}" ]] || continue
  echo "  removing stale ${map_name}"
  rm -f "${stale}"
done < <(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import engine_probe; print("\n".join(engine_probe.generated_map_outputs()))')

echo "== injecting =="
writing=1
if [[ "${probe}" == "ladder" ]]; then
  "${mpq_replace}" "${game_dir}/imp.mpq" 'imp\zzctl.imp' "${work_dir}/zzctl.imp"
  "${mpq_replace}" "${game_dir}/imp.mpq" 'imp\zzpal.imp' "${work_dir}/zzpal.imp"
fi
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
# "the engine cannot read an added archive member" is rung 2's whole hypothesis, so an unreadable
# member here must stop the install rather than send someone to burn an attended run on it.
for member in $([[ "${probe}" == "ladder" ]] && echo 'imp\zzctl.imp imp\zzpal.imp'); do
  read_back="$(cd "${viewer_dir}" && cargo run --release --quiet --example read_member -- \
    "${game_dir}/imp.mpq" "${member}")"
  echo "  ${read_back}"
  case "${read_back}" in
    *"READ FAILED"*|*"not a parseable IMP"*)
      echo "injected member is unreadable; aborting." >&2
      exit 1
      ;;
  esac
done

installed=1

echo
if [[ "${probe}" == "flatground" ]]; then
  echo "Ready. Launch 'Lords of Magic GS5R3.app' and open the MAP EDITOR, then TAP z once."
  echo "The probe replaces the editor's map with a 64x64 one it builds itself, photographs the"
  echo "same six cells three times -- flat, on a raised plateau, and on single-cell spikes --"
  echo "and removes every sprite it places. Nothing is saved and no game map is touched."
  echo "Expect three redraws and six captures. Do not save anything afterwards."
elif [[ "${probe}" == "maptag" ]]; then
  echo "Ready. Launch 'Lords of Magic GS5R3.app' and open the MAP EDITOR (not a game), then TAP z"
  echo "once. The probe replaces the editor's map with a 64x64 one it builds itself, paints two"
  echo "rows with known values -- one with forcetexture, one with setterrain -- and saves that"
  echo "state as both a .scn and a .smp. It then places three terrain sprites, saves again,"
  echo "removes them, and saves a third time. Four files in map/, all named zzt*, all collected"
  echo "and deleted by the restore script."
  echo
  echo "Expect one redraw and two captures. The painted rows are at y 8 and y 12 and the camera"
  echo "is on (16,16), so you should see two short bands of odd-looking terrain."
  echo
  echo "The three sprites are at (18,14), (19,14) and (18,15), which the projection puts inside"
  echo "the frame. They SHOULD be visible. If you see no sprites appear, say so -- that is a real"
  echo "result and not a normal outcome. An earlier run placed them off screen and the two"
  echo "captures came back byte-identical, which cost a screenshot each and proved nothing."
  echo
  echo "Do not save anything yourself afterwards, and do not open one of the zzt maps in the editor."
elif [[ "${probe}" == "mapsize" ]]; then
  echo "Ready. Launch 'Lords of Magic GS5R3.app' and open the MAP EDITOR (not a game), then TAP z"
  echo "once. It generates and saves a 128, a 256 and a 512 map in turn, which takes a while --"
  echo "512x512 is sixteen times the work of a normal map. Watch zprobe.log for 'gen done' lines."
  echo "It places no sprites and destroys nothing. Do not save the game afterwards."
elif [[ "${probe}" == "unitanchor" ]]; then
  echo "Ready. Launch 'Lords of Magic GS5R3.app', start a single-player game, reach the world map"
  echo "with your starting army visible on screen and NOT adjacent to a hostile stack, and TAP z"
  echo "once. It works THREE empty cells around your army in turn. On the first only, a shipped"
  echo "orchard appears and is removed. Then, on each of the three: units/imp/licr2a.imp appears"
  echo "as a terrain sprite and is removed (the control that recovers that cell's anchor), and an"
  echo "Elephant is recruited to your side, captured, and deleted. Expect three units to flash"
  echo "into existence and vanish -- that is the probe's own gated cleanup, not a bug. Read"
  echo "zprobe.log before quitting: any 'cleanup REFUSED' line means that unit is STILL on the"
  echo "map and must be removed by hand. See docs/unit-anchor-run-sheet.md."
  echo "Do not save the game afterwards."
elif [[ "${probe}" == "unitindex" ]]; then
  echo "Ready. Launch 'Lords of Magic GS5R3.app', start a single-player game, reach the world map"
  echo "with your starting army CENTRED and room around it, and TAP z once. It works two empty"
  echo "cells. On the first it places a shipped Elephant and removes it -- that is the CONTROL,"
  echo "and it proves the placement path works this session. It then DEFINES A NEW UNIT TYPE at"
  echo "runtime and places that on the second cell. Both should look like an Elephant; the new"
  echo "type deliberately reuses the shipped art, so the question is whether it draws AT ALL."
  echo "Expect two units to flash into existence and vanish. Five captures."
  echo
  echo "Read zprobe.log BEFORE quitting. The line that matters most is:"
  echo "    rung2 lastunittype <n> expected <n>"
  echo "Those two numbers should be equal, and should be 155. Any 'cleanup REFUSED' line means"
  echo "that unit is STILL on the map and must be removed by hand."
  echo
  echo "DO NOT SAVE. The new unit type exists in no archive, so a save would reference an index"
  echo "nothing on disk defines. It disappears by itself when the game exits."
  echo "See docs/unit-index-run-sheet.md."
else
  echo "Ready. Launch 'Lords of Magic GS5R3.app', start a single-player game, reach the world map,"
  echo "and TAP z once. The probe now fires only once per launch even if the key repeats."
fi
echo "Afterwards run scripts/restore-game-archives.sh."
