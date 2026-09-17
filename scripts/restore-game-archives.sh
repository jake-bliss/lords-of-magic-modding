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

if pgrep -f 'lomse.exe' >/dev/null 2>&1; then
  echo "lomse.exe is still running; quit the game first." >&2
  exit 1
fi

# nullglob only drops patterns that match nothing, so zprobe.log is written as a pattern too.
shopt -s nullglob
captures=("${game_dir}"/z*.bmp "${game_dir}"/zprobe.lo[g])
if (( ${#captures[@]} )); then
  mkdir -p "${capture_dir}"
  cp "${captures[@]}" "${capture_dir}/"
  echo "collected ${#captures[@]} probe file(s) into ${capture_dir}"
  rm -f "${captures[@]}"
fi

cp "${backup_dir}/gs.mpq.orig" "${game_dir}/gs.mpq"
cp "${backup_dir}/imp.mpq.orig" "${game_dir}/imp.mpq"

echo "== restored, verifying =="
status=0
for archive in gs imp; do
  live="$(shasum -a 256 "${game_dir}/${archive}.mpq" | cut -d' ' -f1)"
  original="$(shasum -a 256 "${backup_dir}/${archive}.mpq.orig" | cut -d' ' -f1)"
  if [[ "${live}" == "${original}" ]]; then
    echo "  ${archive}.mpq ${live}  OK"
  else
    echo "  ${archive}.mpq ${live}  MISMATCH (expected ${original})" >&2
    status=1
  fi
done
exit "${status}"
