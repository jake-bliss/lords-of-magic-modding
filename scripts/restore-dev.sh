#!/usr/bin/env bash
# One-command rollback of the development profile.
#
#   scripts/restore-dev.sh                       restore the pristine archives
#   scripts/restore-dev.sh --to MOD_ID BUILD_ID  restore a specific earlier build
#
# Writes only inside `Lords of Magic Development.app`, through tools/install_guard.py.
#
# The verification rule, taken from scripts/lib-game-archives.sh: the result is checked against an
# INDEPENDENT record, never against the file it was just copied from. Comparing a restored archive
# with its own source proves the copy succeeded and nothing else -- it would happily certify a
# pristine copy that had itself been overwritten by a build. For a pristine restore that record is
# `.lom-pipeline/MANIFEST.sha256`, written when the profile was created. For a build restore it is
# that build's `build.json`.
#
# Every run ends by printing the hashes it produced, as every script in this repository does.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

to_build=0
positional=()
while (( $# )); do
  case "$1" in
    --to) to_build=1; shift ;;
    --help|-h) sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) positional+=("$1"); shift ;;
  esac
done

dev_root="$(dev_profile_root)"
metadata_dir="$(dev_metadata_dir)"
manifest="${metadata_dir}/MANIFEST.sha256"

[[ -d "${dev_root}" ]] || die "no development profile at ${dev_root}"
[[ -f "${manifest}" ]] || die "no pristine manifest at ${manifest}; this profile was not created \
by scripts/install-dev.sh --create-profile, so there is nothing to roll back to."

refuse_if_game_running

dev_game_dir="${dev_root}/${game_subpath}"

# The hash `MANIFEST.sha256` records for a file, matched by the path it records.
manifest_hash() {
  local wanted="$1" hash path
  while read -r hash path; do
    [[ "${path}" == "${wanted}" ]] && { echo "${hash}"; return 0; }
  done < "${manifest}"
  return 1
}

if (( to_build )); then
  (( ${#positional[@]} == 2 )) || die "usage: $0 --to MOD_ID BUILD_ID"
  mod_id="${positional[0]}"
  build_id="${positional[1]}"
  build_dir="${artifacts_dir}/build/${mod_id}/${build_id}"
  [[ -f "${build_dir}/build.json" ]] || die "no such build: ${build_dir}"

  echo "== restoring ${mod_id}/${build_id} =="
  mapfile -t rows < <(python3 -c '
import json, sys
build = json.load(open(sys.argv[1]))
for archive, digest in sorted(build["output_archive_digests"].items()):
    print(archive, digest, sep="\t")
' "${build_dir}/build.json")
  source_dir="${build_dir}"
  label="build.json"
else
  (( ${#positional[@]} == 0 )) || die "usage: $0 [--to MOD_ID BUILD_ID]"
  echo "== restoring the pristine archives =="
  rows=()
  for archive in "${PIPELINE_ARCHIVES[@]}"; do
    recorded="$(manifest_hash "pristine/${archive}")" \
      || die "MANIFEST.sha256 records no hash for pristine/${archive}"
    rows+=("${archive}"$'\t'"${recorded}")
  done
  source_dir="${metadata_dir}/pristine"
  label="MANIFEST.sha256"
fi

(( ${#rows[@]} )) || die "nothing to restore"

# Preflight every source against the record before copying any of them, so a corrupt second
# archive cannot leave the profile half-restored.
echo "== verifying the sources against ${label} =="
for row in "${rows[@]}"; do
  archive="${row%%$'\t'*}"
  recorded="${row##*$'\t'}"
  [[ -r "${source_dir}/${archive}" ]] || die "missing or unreadable: ${source_dir}/${archive}"
  actual="$(file_hash "${source_dir}/${archive}")"
  if [[ "${actual}" != "${recorded}" ]]; then
    die "SOURCE CORRUPT: ${source_dir}/${archive}
  ${label} ${recorded}
  actual     ${actual}
Do not restore from this copy."
  fi
  echo "  ${archive} ${actual}  OK"
  approve_dev_path "${dev_game_dir}/${archive}" >/dev/null
done

echo "== restoring =="
status=0
for row in "${rows[@]}"; do
  archive="${row%%$'\t'*}"
  recorded="${row##*$'\t'}"
  approved="$(approve_dev_path "${dev_game_dir}/${archive}")"
  cp -c "${source_dir}/${archive}" "${approved}"
  actual="$(file_hash "${approved}")"
  if [[ "${actual}" == "${recorded}" ]]; then
    echo "  ${archive} ${actual}  OK"
  else
    echo "  ${archive} ${actual}  MISMATCH (${label} ${recorded})" >&2
    status=1
  fi
done

if [[ -f "${metadata_dir}/INSTALLS.tsv" ]]; then
  timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  for row in "${rows[@]}"; do
    archive="${row%%$'\t'*}"
    recorded="${row##*$'\t'}"
    if (( to_build )); then
      printf '%s\t%s\t%s\t%s\t%s\n' \
        "${timestamp}" "${mod_id}" "${build_id}" "${archive}" "${recorded}" \
        >> "${metadata_dir}/INSTALLS.tsv"
    else
      printf '%s\t%s\t%s\t%s\t%s\n' \
        "${timestamp}" "-" "pristine" "${archive}" "${recorded}" \
        >> "${metadata_dir}/INSTALLS.tsv"
    fi
  done
fi

echo
echo "== result =="
echo "  ${dev_root}"
echo "  install log: ${metadata_dir}/INSTALLS.tsv"
exit "${status}"
