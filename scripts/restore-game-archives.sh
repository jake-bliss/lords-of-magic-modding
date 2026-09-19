#!/usr/bin/env bash
# Restore the GS5R3 archives to their recorded originals and collect any probe output.
#
# Standing permission to modify game files is conditional on a checksum-verified backup and a
# verified restore, so this script always ends by printing the two hashes it produced.
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# artifacts/ is gitignored, so a fresh worktree has neither the backups nor the listfile.
# Point LOM_ARTIFACTS_DIR at the main checkout's artifacts directory when running from one.
artifacts_dir="${LOM_ARTIFACTS_DIR:-${project_dir}/artifacts}"
backup_dir="${artifacts_dir}/experiment-backups/gs5r3-20260916"
app_dir="${1:-${HOME}/Applications/Lords of Magic GS5R3.app}"
game_subpath='Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English'
game_dir="${app_dir}/${game_subpath}"
capture_dir="${2:-${artifacts_dir}/engine-probe-captures}"

# shellcheck source=scripts/lib-mod-pipeline.sh
source "${project_dir}/scripts/lib-mod-pipeline.sh"
# shellcheck source=scripts/lib-game-archives.sh
source "${project_dir}/scripts/lib-game-archives.sh"

# `set -euo pipefail` above makes a normal, executed run of this script fail closed if either
# source fails or if `refuse_if_game_running` were somehow undefined: `errexit` stops the script at
# the failed `source`, and an undefined function is a `127`. Neither is true if a caller ever does
# `if source scripts/restore-game-archives.sh; then ...; fi` -- bash suspends `errexit` for the
# whole body of a sourced script run inside an `if`/`while`/`&&`/`||` test, so a failed source and a
# missing `refuse_if_game_running` could both go unnoticed and execution would still reach the
# restore below. Nothing in this repository invokes the script that way today, but the guarantee
# should not depend on how it is invoked rather than on what it does, so it is checked directly.
declare -F refuse_if_game_running >/dev/null || exit 1

# This guards an ATTENDED engine-probe restore: a false NEGATIVE here -- the guard staying quiet
# while the game is actually up -- lets this script `cp` a recorded-original archive over
# `gs.mpq`/`imp.mpq` while a live process still has them open, corrupting the session and the
# archives it is trying to preserve; no checksum afterwards can undo that. A false positive only
# costs a loud refusal and a re-run. (An earlier draft of this file had that backwards; see
# `scripts/lib-mod-pipeline.sh`'s own comment on `game_command_pattern` for the correct ordering,
# which is what this guard is written to.) It used to be `pgrep -f 'lomse.exe'` -- unanchored, and
# with an unescaped `.` that also fires on `lomseXexe` -- which is the same bare-name pattern
# `scripts/lib-mod-pipeline.sh` retired for matching this project's own tools and anything that
# merely quotes the name in an argument. `refuse_if_game_running` is that retirement's replacement
# and is now the only copy of this guard FUNCTION in the repository (the pattern it evaluates is
# also asserted equal, character for character, against a second copy in
# `tests/test_mod_pipeline.py` -- see `test_the_shell_and_python_patterns_are_the_same`).
refuse_if_game_running

# nullglob only drops patterns that match nothing, so zprobe.log is written as a pattern too.
shopt -s nullglob
captures=("${game_dir}"/z*.bmp "${game_dir}"/zprobe.lo[g])
# Every run gets its OWN directory, decided once and used by both collections below.
#
# Collecting into a flat directory silently overwrote whatever the previous run left under the same
# name. On 2026-09-17 that destroyed the elevation run's survey log -- the raw data behind the
# map2screen decode -- when the mapsize run reused `zprobe.log`. An attended run costs a human a
# game session; its output is not something to overwrite.
run_dir="${capture_dir}/run-$(date +%Y%m%d-%H%M%S)"
if [[ -e "${run_dir}" ]]; then
  echo "refusing to collect: ${run_dir} already exists" >&2
  exit 1
fi

if (( ${#captures[@]} )); then
  mkdir -p "${run_dir}"
  cp "${captures[@]}" "${run_dir}/"
  echo "collected ${#captures[@]} probe file(s) into ${run_dir}"
  rm -f "${captures[@]}"
fi

# The mapsize probe writes generated maps into the game's loose map/ directory. They are the
# result, so they are collected before being removed.
#
# That directory holds 366 shipped map files and no backup here covers it, so this works from an
# EXACT list of names taken from the probe generator itself, never from a glob. A pattern that
# could match a file this probe did not create has no place in a delete path with no undo.
generated=()
while IFS= read -r map_name; do
  # Spelled as an if rather than `[[ ]] &&` so its exit status can never interact with `set -e`.
  if [[ -e "${game_dir}/${map_name}" ]]; then
    generated+=("${game_dir}/${map_name}")
  fi
done < <(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import engine_probe; print("\n".join(engine_probe.generated_map_names()))')

if (( ${#generated[@]} )); then
  mkdir -p "${run_dir}"
  echo "collecting ${#generated[@]} generated map(s) into ${run_dir}"
  for path in "${generated[@]}"; do
    base="${path##*/}"
    # The source is hashed BEFORE the copy and the copy is hashed after. Comparing the copy against
    # the file it was just copied from proves nothing, and the original is about to be deleted --
    # this is the only moment at which a bad copy can still be caught.
    before="$(file_hash "${path}")"
    cp "${path}" "${run_dir}/${base}"
    after="$(file_hash "${run_dir}/${base}")"
    if [[ "${before}" != "${after}" ]]; then
      echo "keeping ${base}: the collected copy does not match the original" >&2
      continue
    fi
    rm -f "${path}"
    echo "  ${base} ${after}"
  done
fi

# Preflight every backup before copying any of them, so a missing or corrupt second backup cannot
# leave one archive restored and the other still modified.
echo "== verifying the backups against MANIFEST.sha256 =="
verify_backups "${backup_dir}"

echo "== restoring =="
restore_archives "${backup_dir}" "${game_dir}"
