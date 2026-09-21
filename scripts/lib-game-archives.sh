#!/usr/bin/env bash
# Shared guards for anything that writes to the installed game archives.
#
# Standing permission to modify game files is conditional on a checksum-verified backup and a
# verified restore. That is only worth anything if the *backup* is checked against something
# independent of itself: comparing a restored archive against the file it was just copied from is
# a tautology that would happily certify a backup which had been overwritten by a probed archive.
# MANIFEST.sha256 is that independent record.

ARCHIVE_NAMES=(gs imp pic)

# Echo the hash MANIFEST.sha256 records for a backup file, matched by basename.
manifest_hash() {
  local manifest="$1" basename_wanted="$2" line hash path
  while read -r hash path; do
    [[ "${hash}" == "" ]] && continue
    if [[ "$(basename "${path}")" == "${basename_wanted}" ]]; then
      echo "${hash}"
      return 0
    fi
  done < "${manifest}"
  return 1
}

file_hash() {
  shasum -a 256 "$1" | cut -d' ' -f1
}

# Refuse to proceed unless every backup exists and still matches the manifest.
verify_backups() {
  local backup_dir="$1"
  local manifest="${backup_dir}/MANIFEST.sha256"
  local archive backup recorded actual
  if [[ ! -f "${manifest}" ]]; then
    echo "no MANIFEST.sha256 in ${backup_dir}; refusing to touch the game archives." >&2
    return 1
  fi
  for archive in "${ARCHIVE_NAMES[@]}"; do
    backup="${backup_dir}/${archive}.mpq.orig"
    if [[ ! -r "${backup}" ]]; then
      echo "backup missing or unreadable: ${backup}" >&2
      return 1
    fi
    if ! recorded="$(manifest_hash "${manifest}" "${archive}.mpq.orig")"; then
      echo "MANIFEST.sha256 records no hash for ${archive}.mpq.orig" >&2
      return 1
    fi
    actual="$(file_hash "${backup}")"
    if [[ "${actual}" != "${recorded}" ]]; then
      echo "BACKUP CORRUPT: ${backup}" >&2
      echo "  manifest ${recorded}" >&2
      echo "  actual   ${actual}" >&2
      echo "Do not install or restore from this backup." >&2
      return 1
    fi
  done
  return 0
}

# Copy the verified backups over the live archives, then check the result against the manifest
# rather than against the backups.
restore_archives() {
  local backup_dir="$1" game_dir="$2"
  local manifest="${backup_dir}/MANIFEST.sha256"
  local archive recorded actual status=0
  for archive in "${ARCHIVE_NAMES[@]}"; do
    cp "${backup_dir}/${archive}.mpq.orig" "${game_dir}/${archive}.mpq"
  done
  for archive in "${ARCHIVE_NAMES[@]}"; do
    recorded="$(manifest_hash "${manifest}" "${archive}.mpq.orig")"
    actual="$(file_hash "${game_dir}/${archive}.mpq")"
    if [[ "${actual}" == "${recorded}" ]]; then
      echo "  ${archive}.mpq ${actual}  OK"
    else
      echo "  ${archive}.mpq ${actual}  MISMATCH (manifest ${recorded})" >&2
      status=1
    fi
  done
  return "${status}"
}
