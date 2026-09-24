#!/usr/bin/env bash
# Put the development profile on one rung of the HD-terrain ladder. Launches nothing.
#
#   scripts/terrain-hd-ladder.sh 0|1|2|hybrid|folder|restore
#
# One attended session, four readings. Each rung changes ONE thing, so whichever rung breaks names
# the step that failed rather than leaving "HD terrain did not work" to be guessed at.
#
#   rung 0  viewport 2x, ORIGINAL art, stride untouched.
#           Expect: terrain magnified 2x and blocky (each texel a 2x2 block), units and the
#           interface at their old size in the top-left 640x480. This is the control: it says the
#           viewport build is sound before any art changes. A black or frozen band along the right
#           or bottom of the map means a 640x384 limit is still in the render path.
#   rung 1  + the 2x atlases and TILESIZE 64, stride STILL 512.
#           Expect: terrain SCRAMBLED -- stripes, each tile showing slices of two. That proves the
#           engine loaded 1024-wide atlases and doubled its texel rects from TILESIZE. Looking
#           exactly like rung 0 means the art never loaded; a crash at map load means the LBM
#           loader refuses 1024-wide rows.
#   rung 2  + the texture stride at 1024.
#           Expect: terrain sharp and correct, at the same magnification as rung 0.
#   (rung 3 is rung 2's install: enter a battle and a location view, which use other atlases.)
#   Since 2026-09-24 every rung also carries sprites-2x.toml (map sprites and the cursor across the
#   whole map); the ladder itself was run before it existed.
#   hybrid  the build Jake chose (2026-09-24): the game stays 640x480 as stock and only the terrain
#           rasterizes at 2x (terrain-hybrid-2x.toml + the stride), composited by the fork's ddraw.dll.
#           Needs that DLL installed first -- the exe alone draws a magnified quarter of the map,
#           and this rung refuses to install without it.
#           Expect: the whole 640x480 game at its normal layout and zoom, units and interface at
#           their normal size, terrain sharper than vanilla. The quarter-map picture means the DLL
#           did not switch on; vanilla-looking terrain means it switched on but drew nothing (the
#           debug log's "terrain:" lines say which).
#   folder  the hybrid exe with the STOCK pic.mpq, and the 2x terrain served by the DLL from
#           lomhd_terrain\til\ beside the exe -- how a release installs it without rewriting pic.mpq.
#           Expect: exactly the hybrid rung's picture. Scrambled, striped terrain (rung 1's look)
#           means the DLL did not serve the folder: the stride edits then read 1x atlases at 1024.
#           The debug log's "terrain: art from lomhd_terrain, N files served" says which.
#   restore puts back the pristine exe, the gs5r3-base archives and the window size, and removes
#           the terrain folder.
#
# The window is set to 1280x960 for every rung, so the 2x frame is shown 1:1 rather than scaled.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

ART_BUILD="${LOM_TERRAIN_ART_BUILD:-6c996e7484b7}"
BASE_BUILD="${LOM_BASE_BUILD:-ad9fece3123a}"
sets_dir="${project_dir}/tools/exe_patches"
viewport=("${sets_dir}/viewport-2x.toml" "${sets_dir}/terrain-render-2x.toml" "${sets_dir}/sprites-2x.toml")
stride="${sets_dir}/terrain-stride-1024.toml"
hybrid="${sets_dir}/terrain-hybrid-2x.toml"

rung="${1:-}"
[[ "${rung}" =~ ^(0|1|2|hybrid|folder|restore)$ ]] || die "usage: $0 0|1|2|hybrid|folder|restore"

refuse_if_game_running
metadata_dir="$(dev_metadata_dir)"
dev_game_dir="$(dev_profile_root)/${game_subpath}"
ini="${dev_game_dir}/ddraw.ini"
saved_ini="${metadata_dir}/ddraw.ini.before-terrain-hd"

set_window() {
  if [[ ! -f "${saved_ini}" ]]; then
    cp "${ini}" "$(approve_dev_path "${saved_ini}")"
  fi
  local tmp
  tmp="$(mktemp)"
  # Only the first width=/height= pair: the [ddraw] section. Per-game sections below it carry their
  # own width=0 lines that must stay as they are.
  awk -v w="$1" -v h="$2" '
    !dw && /^width=/  { print "width=" w;  dw = 1; next }
    !dh && /^height=/ { print "height=" h; dh = 1; next }
    { print }' "${ini}" > "${tmp}"
  cp "${tmp}" "$(approve_dev_path "${ini}")"
  rm -f "${tmp}"
}

terrain_dir="${dev_game_dir}/lomhd_terrain"
art_tree="${project_dir}/mods/terrain-hd-art/archives/pic.mpq/til"

remove_terrain_dir() {
  if [[ -d "${terrain_dir}" ]]; then
    rm -rf "$(approve_dev_path "${terrain_dir}")"
  fi
}

install_pic() {
  scripts/install-dev.sh "$1" "$2" | tail -3
}

cd "${project_dir}"
case "${rung}" in
  0)
    scripts/install-dev-exe.sh "${viewport[@]}"
    install_pic gs5r3-base "${BASE_BUILD}"
    set_window 1280 960
    ;;
  1)
    scripts/install-dev-exe.sh "${viewport[@]}"
    install_pic terrain-hd-art "${ART_BUILD}"
    set_window 1280 960
    ;;
  2)
    scripts/install-dev-exe.sh "${viewport[@]}" "${stride}"
    install_pic terrain-hd-art "${ART_BUILD}"
    set_window 1280 960
    ;;
  hybrid)
    # The string only the terrain-composite build of the fork's DLL carries.
    grep -qa 'map copies, %ld builds' "${dev_game_dir}/ddraw.dll" ||
      die "the installed ddraw.dll has no HD terrain composite -- install the fork's claude/hybrid-terrain build first"
    remove_terrain_dir
    scripts/install-dev-exe.sh "${hybrid}" "${stride}"
    install_pic terrain-hd-art "${ART_BUILD}"
    set_window 1280 960
    ;;
  folder)
    grep -qa 'art from lomhd_terrain' "${dev_game_dir}/ddraw.dll" ||
      die "the installed ddraw.dll cannot serve lomhd_terrain -- install the fork's claude/hybrid-terrain build first"
    [[ "$(find "${art_tree}" -maxdepth 1 -type f | wc -l | tr -d ' ')" == 46 ]] ||
      die "expected 46 files (20 atlases, 26 .til) in ${art_tree}; run mods/terrain-hd-art/rebuild.sh"
    scripts/install-dev-exe.sh "${hybrid}" "${stride}"
    install_pic gs5r3-base "${BASE_BUILD}"
    remove_terrain_dir
    mkdir -p "$(approve_dev_path "${terrain_dir}/til")"
    cp "${art_tree}"/* "$(approve_dev_path "${terrain_dir}/til")/"
    set_window 1280 960
    ;;
  restore)
    remove_terrain_dir
    scripts/install-dev-exe.sh --restore
    install_pic gs5r3-base "${BASE_BUILD}"
    if [[ -f "${saved_ini}" ]]; then
      cp "${saved_ini}" "$(approve_dev_path "${ini}")"
      # Consumed: a later ladder saves the window as it is THEN, not as it was before this one.
      rm -f "$(approve_dev_path "${saved_ini}")"
    fi
    ;;
esac
echo "  development profile is on rung ${rung}"
