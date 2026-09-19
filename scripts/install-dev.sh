#!/usr/bin/env bash
# Install a build into the development profile -- and into nothing else, ever.
#
#   scripts/install-dev.sh --create-profile [--recreate]
#   scripts/install-dev.sh --record-pristine
#   scripts/install-dev.sh MOD_ID BUILD_ID
#
# --record-pristine adds a pristine copy and a manifest line for any archive in PIPELINE_ARCHIVES
# the existing development profile has no record of. It exists because widening that list --
# `imp.mpq` was added on 2026-09-18 -- would otherwise force `--recreate` on a profile that is
# perfectly good, and `--recreate` throws away every install in it. It refuses to touch an archive
# already recorded, so it can never overwrite a pristine copy with a modded one; the way to
# re-record an archive is still to recreate the profile.
#
# Creating the profile is a separate, attended step. It is not done implicitly by an install,
# because creating it is the one moment the pipeline reads the preserved baseline, and that should
# happen when a person asked for it and is watching.
#
# THE THREE INSTALLED PROFILES UNDER ~/Applications ARE NOT WRITTEN BY THIS SCRIPT.
# `Steambuild 32 64bit DXVK.app` is the preserved baseline and has no second copy. The check that
# guarantees this is not "the target is not the baseline" -- that form fails open on every path
# nobody thought to name. It is tools/install_guard.py, an allowlist of exactly one directory,
# through which every write below is routed. See tests/test_install_guard.py.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

# The profile the development profile is cloned from. Read-only, once, at creation.
BASELINE_PROFILE=vanilla

usage() {
  sed -n '2,16p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
}

create_profile=0
record_pristine=0
recreate=0
positional=()
while (( $# )); do
  case "$1" in
    --create-profile) create_profile=1; shift ;;
    --record-pristine) record_pristine=1; shift ;;
    --recreate) recreate=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) positional+=("$1"); shift ;;
  esac
done

dev_root="$(dev_profile_root)"
metadata_dir="$(dev_metadata_dir)"

# ---------------------------------------------------------------------------------------------
# Creating the development profile
# ---------------------------------------------------------------------------------------------
if (( create_profile )); then
  (( ${#positional[@]} == 0 )) || die "--create-profile takes no other arguments"
  refuse_if_game_running

  baseline_root="${applications_dir}/$(profile_app "${BASELINE_PROFILE}")"
  baseline_game_dir="$(profile_game_dir "${BASELINE_PROFILE}")"
  [[ -d "${baseline_root}" ]] || die "baseline profile not found: ${baseline_root}"

  if [[ -e "${dev_root}" ]]; then
    if (( recreate )); then
      # Approved by the allowlist first, so the path this deletes cannot be anything else.
      approved="$(approve_dev_path "${dev_root}")"
      echo "== removing the existing development profile =="
      echo "  ${approved}"
      rm -rf "${approved}"
    else
      die "development profile already exists: ${dev_root}
Refusing to overwrite it. Pass --recreate to delete and rebuild it from the baseline, which
discards every build installed into it."
    fi
  fi

  approve_dev_path "${dev_root}" >/dev/null

  # Same-volume check before the clone, not after. `cp -c` degrades to a full copy across volumes
  # and says nothing about it; a 3.7 GB surprise should be refused, not absorbed.
  source_device="$(stat -f '%d' "${baseline_root}")"
  target_device="$(stat -f '%d' "${applications_dir}")"
  if [[ "${source_device}" != "${target_device}" ]]; then
    die "the baseline and ${applications_dir} are on different volumes (${source_device} vs \
${target_device}).
APFS clonefile only works within a volume, so this would be a real 3.7 GB copy. Refusing rather
than doing it silently; re-run deliberately if that is what you want."
  fi

  free_before="$(df -k "${applications_dir}" | awk 'NR==2 {print $4}')"
  echo "== cloning the baseline =="
  echo "  from ${baseline_root}"
  echo "  to   ${dev_root}"
  # -c asks for APFS clonefile. Measured 2026-09-18 by the user: 3.7 GB in 2.7 s with a zero df
  # delta. `du` will report the full 3.5 GB afterwards and is lying -- it counts cloned blocks in
  # full. Only the df delta below measures this honestly.
  cp -c -R "${baseline_root}" "${dev_root}"
  free_after="$(df -k "${applications_dir}" | awk 'NR==2 {print $4}')"
  consumed_kb=$(( free_before - free_after ))
  echo "  df delta: ${consumed_kb} KiB consumed"
  if (( consumed_kb > 102400 )); then
    echo "  WARNING: the clone consumed ${consumed_kb} KiB. A clonefile copy consumes no" >&2
    echo "  measurable space; this one did not clone, and the disk now holds a second full" >&2
    echo "  3.7 GB profile. Reported rather than absorbed." >&2
  fi

  dev_game_dir="${dev_root}/${game_subpath}"
  [[ -d "${dev_game_dir}" ]] || die "the clone has no ${game_subpath}"

  mkdir -p "${metadata_dir}/pristine"
  manifest="${metadata_dir}/MANIFEST.sha256"
  : > "${manifest}"
  echo "== recording the pristine archives =="
  for archive in "${PIPELINE_ARCHIVES[@]}"; do
    baseline_hash="$(file_hash "${baseline_game_dir}/${archive}")"
    clone_hash="$(file_hash "${dev_game_dir}/${archive}")"
    if [[ "${baseline_hash}" != "${clone_hash}" ]]; then
      die "the clone of ${archive} does not match the baseline:
  baseline ${baseline_hash}
  clone    ${clone_hash}
Refusing to record a pristine manifest that is already wrong."
    fi
    # The pristine copy lives inside the development profile, cloned again, so that a rollback
    # never has to open the baseline at all. The baseline is read exactly once PER ARCHIVE here,
    # at creation -- but --record-pristine below reads it again for any archive that joins
    # PIPELINE_ARCHIVES after this profile already exists, which is the whole reason that path
    # exists rather than forcing --recreate.
    cp -c "${dev_game_dir}/${archive}" "${metadata_dir}/pristine/${archive}"
    stored_hash="$(file_hash "${metadata_dir}/pristine/${archive}")"
    [[ "${stored_hash}" == "${baseline_hash}" ]] \
      || die "the pristine copy of ${archive} does not match what was copied"
    echo "${baseline_hash}  pristine/${archive}" >> "${manifest}"
    echo "  ${archive} ${baseline_hash}"
  done

  LOM_PROFILE_JSON="${metadata_dir}/PROFILE.json" \
  LOM_ARCHIVES="${PIPELINE_ARCHIVES[*]}" \
  LOM_BASELINE_PROFILE="${BASELINE_PROFILE}" \
  LOM_BASELINE_PATH="${baseline_root}" \
  python3 - <<'PROFILE_JSON'
import datetime
import json
import os
import pathlib

pathlib.Path(os.environ["LOM_PROFILE_JSON"]).write_text(
    json.dumps(
        {
            "profile": "development",
            "created": datetime.datetime.now(datetime.UTC).isoformat(),
            "cloned_from_profile": os.environ["LOM_BASELINE_PROFILE"],
            "cloned_from_path": os.environ["LOM_BASELINE_PATH"],
            "archives": os.environ["LOM_ARCHIVES"].split(),
            "note": (
                "MANIFEST.sha256 in this directory records the PRISTINE archive hashes. "
                "restore-dev verifies against that record, never against the file it copied "
                "from -- comparing a restored archive with its own source is a tautology that "
                "would certify a pristine copy which had itself been overwritten."
            ),
        },
        indent=2,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PROFILE_JSON

  : > "${metadata_dir}/INSTALLS.tsv"
  printf 'timestamp\tmod_id\tbuild_id\tarchive\tsha256\n' >> "${metadata_dir}/INSTALLS.tsv"

  echo
  echo "== result =="
  echo "  ${dev_root}"
  echo "  pristine manifest: ${manifest}"
  echo "  Nothing has been installed into it. It is a clean copy of the ${BASELINE_PROFILE} profile."
  exit 0
fi

# ---------------------------------------------------------------------------------------------
# Recording a pristine copy for an archive the profile predates
# ---------------------------------------------------------------------------------------------
if (( record_pristine )); then
  (( ${#positional[@]} == 0 )) || die "--record-pristine takes no other arguments"
  refuse_if_game_running

  manifest="${metadata_dir}/MANIFEST.sha256"
  [[ -f "${manifest}" ]] || die "no development profile with a pristine manifest at ${manifest}.
Create it first: scripts/install-dev.sh --create-profile"

  baseline_game_dir="$(profile_game_dir "${BASELINE_PROFILE}")"
  dev_game_dir="${dev_root}/${game_subpath}"
  recorded=0

  to_record=()
  for archive in "${PIPELINE_ARCHIVES[@]}"; do
    if grep -qF "  pristine/${archive}" "${manifest}"; then
      echo "  ${archive} already recorded; leaving it alone"
      continue
    fi
    to_record+=("${archive}")
  done

  # Preflight every unrecorded archive, read-only, before writing any of them -- the same
  # all-or-nothing-before-any-write rule the install path below already follows. Without it, an
  # archive later in PIPELINE_ARCHIVES failing its check (a profile copy that no longer matches
  # the baseline, say) would leave the EARLIER archives in this run already recorded and the rest
  # not, a half-migrated manifest that looks no different from one nobody had touched yet.
  for archive in ${to_record[@]+"${to_record[@]}"}; do
    [[ -f "${baseline_game_dir}/${archive}" ]] || die "the baseline has no ${archive}"
    [[ -f "${dev_game_dir}/${archive}" ]] || die "the development profile has no ${archive}"

    baseline_hash="$(file_hash "${baseline_game_dir}/${archive}")"
    profile_hash="$(file_hash "${dev_game_dir}/${archive}")"
    # The profile's own copy is only a legitimate pristine seed if it still matches the baseline.
    # If it does not, something installed it, and recording it would enshrine a mod as the thing
    # every later rollback returns to.
    [[ "${baseline_hash}" == "${profile_hash}" ]] || die \
      "the development profile's ${archive} does not match the baseline:
  baseline ${baseline_hash}
  profile  ${profile_hash}
Refusing to record a pristine copy of an archive that is already modified. Restore it from the
baseline by hand, or recreate the profile."

    approved="$(approve_dev_path "${metadata_dir}/pristine/${archive}")"
    [[ -e "${approved}" ]] && die "a pristine copy already exists without a manifest line:
  ${approved}
Refusing to overwrite it."
  done

  for archive in ${to_record[@]+"${to_record[@]}"}; do
    baseline_hash="$(file_hash "${baseline_game_dir}/${archive}")"
    approved="$(approve_dev_path "${metadata_dir}/pristine/${archive}")"
    cp -c "${baseline_game_dir}/${archive}" "${approved}"
    stored_hash="$(file_hash "${approved}")"
    [[ "${stored_hash}" == "${baseline_hash}" ]] \
      || die "the pristine copy of ${archive} does not match the baseline it was copied from"
    echo "${baseline_hash}  pristine/${archive}" >> "${manifest}"
    echo "  ${archive} ${baseline_hash}  recorded"
    recorded=$(( recorded + 1 ))
  done

  echo
  echo "== result =="
  echo "  ${recorded} archive(s) newly recorded in ${manifest}"
  echo "  Nothing in the game directory was written."
  exit 0
fi

# ---------------------------------------------------------------------------------------------
# Installing a build
# ---------------------------------------------------------------------------------------------
(( ${#positional[@]} == 2 )) || { usage; exit 2; }
mod_id="${positional[0]}"
build_id="${positional[1]}"

refuse_if_game_running

build_dir="${artifacts_dir}/build/${mod_id}/${build_id}"
[[ -d "${build_dir}" ]] || die "no such build: ${build_dir}"
[[ -f "${build_dir}/build.json" ]] || die "build has no build.json: ${build_dir}"

[[ -d "${dev_root}" ]] || die "no development profile. Create it first:
  scripts/install-dev.sh --create-profile"
[[ -f "${metadata_dir}/MANIFEST.sha256" ]] \
  || die "the development profile has no pristine manifest; it was not created by this pipeline"

dev_game_dir="${dev_root}/${game_subpath}"

echo "== installing =="
echo "  build ${mod_id}/${build_id}"
echo "  into  ${dev_root}"

# Every archive the build produced, with the digest build.json recorded for it.
mapfile -t install_rows < <(python3 -c '
import json, sys
build = json.load(open(sys.argv[1]))
for archive, digest in sorted(build["output_archive_digests"].items()):
    print(archive, digest, sep="\t")
' "${build_dir}/build.json")

(( ${#install_rows[@]} )) || die "build.json records no output archives"

# Preflight everything before writing anything, so a bad second archive cannot leave the profile
# half-installed. Same ordering rule as scripts/restore-game-archives.sh.
#
# Including that the archive has a pristine manifest line. PIPELINE_ARCHIVES has grown since this
# profile may have been created -- imp.mpq, sndfx.mpq and special.mpq joined it on 2026-09-18 --
# and --record-pristine is a separate, OPTIONAL step for exactly that migration. Nothing before
# this line checked for the line itself, only that MANIFEST.sha256 exists at all: an install of an
# archive this profile never recorded a pristine copy of would succeed and then leave that archive
# (and, once restore-dev.sh refuses on it, every OTHER archive too) with no way back except
# --recreate or a hand-write into ~/Applications -- the one write the allowlist exists to prevent.
for row in "${install_rows[@]}"; do
  archive="${row%%$'\t'*}"
  recorded="${row##*$'\t'}"
  grep -qF "  pristine/${archive}" "${metadata_dir}/MANIFEST.sha256" \
    || die "no pristine copy of ${archive} in ${metadata_dir}/MANIFEST.sha256.
Run scripts/install-dev.sh --record-pristine first, or this install cannot be rolled back."
  [[ -f "${build_dir}/${archive}" ]] || die "build.json names ${archive} but the file is missing"
  actual="$(file_hash "${build_dir}/${archive}")"
  [[ "${actual}" == "${recorded}" ]] || die "BUILD CORRUPT: ${build_dir}/${archive}
  build.json ${recorded}
  actual     ${actual}
Do not install this build."
  [[ -f "${dev_game_dir}/${archive}" ]] || die "the development profile has no ${archive}"
  approve_dev_path "${dev_game_dir}/${archive}" >/dev/null
done

timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
status=0
for row in "${install_rows[@]}"; do
  archive="${row%%$'\t'*}"
  recorded="${row##*$'\t'}"
  approved="$(approve_dev_path "${dev_game_dir}/${archive}")"
  cp -c "${build_dir}/${archive}" "${approved}"
  installed="$(file_hash "${approved}")"
  if [[ "${installed}" == "${recorded}" ]]; then
    echo "  ${archive} ${installed}  OK"
    printf '%s\t%s\t%s\t%s\t%s\n' \
      "${timestamp}" "${mod_id}" "${build_id}" "${archive}" "${installed}" \
      >> "${metadata_dir}/INSTALLS.tsv"
  else
    echo "  ${archive} ${installed}  MISMATCH (build.json ${recorded})" >&2
    status=1
  fi
done

echo
echo "== result =="
echo "  ${mod_id}/${build_id} is live in ${dev_root}"
echo "  install log: ${metadata_dir}/INSTALLS.tsv"
echo "  Roll back with: scripts/restore-dev.sh"
exit "${status}"
