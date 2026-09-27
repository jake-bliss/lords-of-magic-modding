#!/usr/bin/env bash
# Back up, and later restore, the Development profile's savegame folder around the marauder probe.
#
#   scripts/marauder-probe-savegames.sh backup
#   scripts/marauder-probe-savegames.sh restore BACKUP_DIR
#
# backup   copies the whole folder to a new, timestamped directory under the keep area and
#          verifies the copy with `diff -r`. Refuses if the destination exists.
# restore  first copies the probe's saves (mprobe*) aside to a new, timestamped evidence directory,
#          verified with `cmp`; then moves the current folder aside (kept, not deleted); then copies
#          BACKUP_DIR back into place and verifies it with `diff -r`.
#
# Both refuse while the game is running, and every write inside ~/Applications goes through
# tools/install_guard.py. The keep area is outside every worktree, because worktrees are removed
# when their branch merges: LOM_KEEP_DIR, default /Users/jakebliss/personal-projects/lom-artifacts-keep.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

keep_dir="${LOM_KEEP_DIR:-/Users/jakebliss/personal-projects/lom-artifacts-keep}"
backups="${keep_dir}/save-backups"
savegame="$(dev_profile_root)/${game_subpath}/savegame"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"

usage() {
  sed -n '2,16p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

fresh_dir() {
  # A new directory, refusing any path that already exists.
  [[ ! -e "$1" ]] || die "refusing: $1 already exists"
  mkdir -p "$(dirname "$1")"
  mkdir "$1"
}

(( $# >= 1 )) || usage
command="$1"
shift

refuse_if_game_running
[[ -d "${savegame}" ]] || die "no savegame folder at ${savegame}"
approve_dev_path "${savegame}" >/dev/null

case "${command}" in
  backup)
    (( $# == 0 )) || usage
    target="${backups}/dev-savegame-${stamp}"
    [[ ! -e "${target}" ]] || die "refusing: ${target} already exists"
    mkdir -p "${backups}"
    cp -Rp "${savegame}" "${target}"
    diff -r "${savegame}" "${target}" >/dev/null \
      || die "the copy at ${target} differs from ${savegame}; do not rely on it"
    echo "== savegame backed up =="
    echo "  from ${savegame}"
    echo "  to   ${target}  (diff -r: identical)"
    echo "  restore with: $0 restore '${target}'"
    ;;

  restore)
    (( $# == 1 )) || usage
    source_dir="$1"
    [[ -d "${source_dir}" && ! -L "${source_dir}" ]] || die "no such backup (or a symlink): ${source_dir}"
    # Only a folder this script's `backup` made: an evidence folder or any other directory would
    # otherwise be installed as the profile's saves, and diff -r would happily confirm the copy.
    case "$(basename "${source_dir}")" in
      dev-savegame-2*Z) ;;
      *) die "refusing: ${source_dir} is not a dev-savegame-<stamp> backup made by '$0 backup'" ;;
    esac
    [[ "$(cd "$(dirname "${source_dir}")" && pwd)" == "$(cd "${backups}" && pwd)" ]] \
      || die "refusing: ${source_dir} is not under ${backups}"
    [[ "$(cd "${source_dir}" && pwd)" != "$(cd "${savegame}" && pwd)" ]] \
      || die "the backup is the live folder"

    # 1. The probe's saves are evidence: keep them before anything moves.
    evidence="${backups}/marauder-probe-evidence-${stamp}"
    fresh_dir "${evidence}"
    kept=0
    for save in "${savegame}"/mprobe*; do
      [[ -f "${save}" ]] || continue
      cp -p "${save}" "${evidence}/"
      cmp -s "${save}" "${evidence}/$(basename "${save}")" \
        || die "the evidence copy of ${save} does not match it"
      kept=$(( kept + 1 ))
    done
    echo "== ${kept} probe save(s) kept in ${evidence} =="

    # 2. The live folder is moved aside, not deleted.
    aside="${backups}/dev-savegame-replaced-${stamp}"
    [[ ! -e "${aside}" ]] || die "refusing: ${aside} already exists"
    approved="$(approve_dev_path "${savegame}")"
    mv "${approved}" "${aside}"
    echo "  live folder moved aside to ${aside}"

    # 3. The backup goes back into place, verified. Any failure from here puts the live folder
    # back, so the profile is never left without its saves.
    put_back() {
      echo "restore failed: putting the live folder back from ${aside}" >&2
      rm -rf "${approved}.partial"
      [[ -e "${approved}" ]] && mv "${approved}" "${approved}.partial"
      mv "${aside}" "${approved}"
    }
    trap put_back ERR
    cp -Rp "${source_dir}" "${approved}"
    [[ -d "${approved}" && ! -L "${approved}" ]] || { put_back; die "restored path is not a real folder"; }
    diff -r "${source_dir}" "${approved}" >/dev/null \
      || { put_back; die "the restored folder differs from ${source_dir}"; }
    trap - ERR
    echo "== savegame restored from ${source_dir} (diff -r: identical) =="
    ;;

  *) usage ;;
esac
