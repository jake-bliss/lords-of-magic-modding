#!/usr/bin/env bash
# Put the development profile on one rung of the HD-terrain ladder. Launches nothing.
#
#   scripts/terrain-hd-ladder.sh 0|1|2|restore
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
#   restore puts back the pristine exe, the gs5r3-base archives and the window size.
#
# The window is set to 1280x960 for every rung, so the 2x frame is shown 1:1 rather than scaled.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

ART_BUILD="${LOM_TERRAIN_ART_BUILD:-6c996e7484b7}"
BASE_BUILD="${LOM_BASE_BUILD:-ad9fece3123a}"
sets_dir="${project_dir}/tools/exe_patches"
viewport=("${sets_dir}/viewport-2x.toml" "${sets_dir}/terrain-render-2x.toml")
stride="${sets_dir}/terrain-stride-1024.toml"

rung="${1:-}"
[[ "${rung}" =~ ^(0|1|2|restore)$ ]] || die "usage: $0 0|1|2|restore"

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
  restore)
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
