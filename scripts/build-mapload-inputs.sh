#!/usr/bin/env bash
# Build the input maps for the `mapload` probe, using only the project's own writer.
#
# The probe asks whether the engine will load a map this project wrote. These are those maps. They
# are built BEFORE the attended run and copied into the game's loose `map/` directory, which has no
# backup -- so every name here is one `tools/engine_probe.py` also lists in `generated_map_names()`,
# nothing existing is ever overwritten, and the shipped donor map is only ever read.
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_dir="${1:-${HOME}/Applications/Lords of Magic GS5R3.app}"
game_subpath='Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English'
map_dir="${app_dir}/${game_subpath}/map"
viewer="${project_dir}/spikes/asset-viewer/target/release/lom-asset-viewer"
donor="${map_dir}/URAK.scn"
work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

[[ -x "${viewer}" ]] || { echo "build the viewer first: (cd spikes/asset-viewer && cargo build --release)" >&2; exit 1; }
[[ -d "${map_dir}" ]] || { echo "missing map directory: ${map_dir}" >&2; exit 1; }
[[ -f "${donor}" ]] || { echo "missing donor map: ${donor}" >&2; exit 1; }

# One source of truth for the list. Hardcoding it here meant adding a rung would leave this script
# not creating the file, the install check not looking for it, and the probe logging a missing file
# as an engine rejection -- a plausible wrong answer rather than a crash.
mapfile -t input_names < <(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import engine_probe; print("\n".join(engine_probe.mapload_prebuilt_names()))')
(( ${#input_names[@]} > 0 )) || { echo "probe input list is empty" >&2; exit 1; }

# zm0 is the engine's own control, written during the run. Everything else is refused if present,
# because a leftover from a previous run would be silently measured as this run's result.
for name in "${input_names[@]%.scn}"; do
  if [[ -e "${map_dir}/${name}.scn" ]]; then
    echo "refusing to run: ${map_dir}/${name}.scn already exists (leftover from a previous run?)" >&2
    echo "remove the probe's generated maps first: scripts/restore-game-archives.sh" >&2
    exit 1
  fi
done

echo "== the writer must reproduce the donor exactly before anything is derived from it =="
"${viewer}" --map-roundtrip "${donor}"

echo "== zm1: our byte-identical round-trip of URAK.scn =="
# Produced by a no-op edit and then verified against the donor byte for byte. A plain `cp` would
# test the filesystem; this tests the writer, which is the point of the rung.
"${viewer}" --map-rewrite "${donor}" "${work_dir}/zm1.scn"
cmp "${donor}" "${work_dir}/zm1.scn" && echo "   zm1 is byte-identical to the donor"

echo "== zm2: a terrain edit =="
"${viewer}" --map-set-terrain "${work_dir}/zm1.scn" 10 20 water "${work_dir}/a.scn"
"${viewer}" --map-set-terrain "${work_dir}/a.scn"  11 20 lava  "${work_dir}/b.scn"
"${viewer}" --map-set-terrain "${work_dir}/b.scn"  10 21 snow  "${work_dir}/zm2.scn"

echo "== zm3: a placed sprite, which exercises the minted +24 field =="
"${viewer}" --map-place-sprite "${work_dir}/zm1.scn" 40 40 470 "${work_dir}/zm3.scn"

echo "== zm4: the border bit on an INTERIOR rectangle, a shape the engine has never been given =="
"${viewer}" --map-flag-rect "${work_dir}/zm1.scn" 30 30 33 33 "${work_dir}/zm4.scn"

echo "== zm5/zm6: created from nothing =="
"${viewer}" --map-create 64 64 land "${work_dir}/zm5.scn"
"${viewer}" --map-create 96 64 land "${work_dir}/zm6.scn"

echo "== every input must survive our own reader before the engine sees it =="
for name in "${input_names[@]%.scn}"; do
  "${viewer}" --map-roundtrip "${work_dir}/${name}.scn" > /dev/null
  printf '   %s  %s bytes  OK\n' "${name}.scn" "$(wc -c < "${work_dir}/${name}.scn" | tr -d ' ')"
done

echo "== installing into ${map_dir} =="
for name in "${input_names[@]%.scn}"; do
  cp -n "${work_dir}/${name}.scn" "${map_dir}/${name}.scn"
  printf '   wrote %s\n' "${map_dir}/${name}.scn"
done

cat <<'NOTE'

Inputs are in place. The probe's own file, zm0.scn, is written by the engine during the run.
Remove all of them afterwards with scripts/restore-game-archives.sh.
NOTE
