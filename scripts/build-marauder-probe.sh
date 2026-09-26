#!/usr/bin/env bash
# Build the marauder probe and check offline everything that can be checked without the engine.
#
#   scripts/build-marauder-probe.sh
#
# Installs nothing and launches nothing. `docs/marauder-probe.md` is the run sheet for the attended
# part; this script produces the build it refers to, under artifacts/build/marauder-probe/, and an
# offline-checks report at artifacts/marauder-probe/offline-checks.txt.
#
# The mod tree is rebuilt from scratch on every run, for the reason scripts/build-cheat-keys-ladder.sh
# gives: mod-seed.sh refuses to overwrite a file already in a tree, so a build whose input depended
# on what a previous run left behind would not be reproducible.
#
# No separate no-op control mod is built. The repack control for this exact member and storage
# class is cheat-keys-noop, accepted by the engine on 2026-09-19 (docs/cheat-keys-ladder.md). The
# control this probe needs is a different one -- that the on-screen readout is connected -- and it
# is taken in-game by pressing the readout key before the takeover (see the run sheet).
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

MOD_ID=marauder-probe
HOTKEY_MEMBER='gs\hotkey.gs'
PLACEDNG_MEMBER='gs\placedng.gs'

case "${1:-}" in
  --help|-h) sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
  "") ;;
  *) die "unknown argument: $1" ;;
esac

prepare_tools
game_dir="$(profile_game_dir vanilla)"
out_dir="${artifacts_dir}/marauder-probe"
mkdir -p "${out_dir}"
work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

report="${out_dir}/offline-checks.txt"
: > "${report}"
failures=0

note() {
  echo "$@" | tee -a "${report}"
}

check() {
  # check "what was asserted" <command...>
  local label="$1"
  shift
  if "$@" >"${work_dir}/check.log" 2>&1; then
    note "  PASS  ${label}"
    sed 's/^/        /' "${work_dir}/check.log" | tee -a "${report}"
  else
    note "  FAIL  ${label}"
    sed 's/^/        /' "${work_dir}/check.log" | tee -a "${report}"
    failures=$(( failures + 1 ))
  fi
}

require() {
  # require "what was asserted" <command...> -- a check whose failure makes everything after it
  # meaningless. Recorded like any other check, then the run stops.
  local before="${failures}"
  check "$@"
  (( failures == before )) || { note; note "  STOPPED: a required check failed. Nothing to install."; exit 1; }
}

probe_tool() {
  PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/marauder_probe.py" "$@"
}

files_identical() {
  cmp -s "$1" "$2"
}

extract_member() {
  # extract_member ARCHIVE MEMBER OUTPUT -- the exporter refuses to overwrite, so the target is
  # removed by its exact computed path first.
  rm -f "$3"
  "${viewer_tool}" --extract "$1" "$2" "$3" >/dev/null
}

only_gs_in_build() {
  python3 -c '
import json, sys
build = json.load(open(sys.argv[1]))
archives = sorted(build["output_archive_digests"])
print("output archives:", ", ".join(archives))
sys.exit(0 if archives == ["gs.mpq"] else 1)
' "$1"
}

note "== marauder probe, offline checks =="
note "  project  ${project_dir}"
note "  baseline ${game_dir}"
note "  gs.mpq    sha256 $(file_hash "${game_dir}/gs.mpq")"
note "  lomse.exe sha256 $(file_hash "${game_dir}/lomse.exe")"
note

# --------------------------------------------------------------------------------------------
# Facts the run sheet's predictions rest on, read from the shipped files
# --------------------------------------------------------------------------------------------
note "== corpus facts =="
extract_member "${game_dir}/gs.mpq" "${HOTKEY_MEMBER}" "${work_dir}/hotkey.shipped.gs"
extract_member "${game_dir}/gs.mpq" "${PLACEDNG_MEMBER}" "${work_dir}/placedng.shipped.gs"
require "WANDERING_MONSTER_PLAYER in lomse.exe's constant table agrees with placedng.gs's literal slot" \
  probe_tool wmp "${game_dir}/lomse.exe" "${work_dir}/placedng.shipped.gs"
require "the probe keys (J N U H) are unbound in the shipped hotkey.gs, every addhotkey accounted for" \
  probe_tool keys "${work_dir}/hotkey.shipped.gs"
note

# --------------------------------------------------------------------------------------------
# The edit
# --------------------------------------------------------------------------------------------
note "== edit =="
# The path is computed from the project directory and the mod id, never globbed.
rm -rf "${project_dir}/mods/${MOD_ID}/archives"
"${project_dir}/scripts/mod-seed.sh" "${project_dir}/mods/${MOD_ID}" "gs.mpq:${HOTKEY_MEMBER}" >/dev/null
tree_file="${project_dir}/mods/${MOD_ID}/archives/gs.mpq/gs/hotkey.gs"

require "the seeded tree file is byte-identical to the shipped member" \
  files_identical "${tree_file}" "${work_dir}/hotkey.shipped.gs"

probe_tool apply "${work_dir}/hotkey.shipped.gs" "${work_dir}/hotkey.edited.gs" >/dev/null \
  || die "the probe edit could not be applied to the shipped member"
cp "${work_dir}/hotkey.edited.gs" "${tree_file}"

require "the tree file is the shipped member plus exactly the flag flip and the probe block" \
  probe_tool verify "${work_dir}/hotkey.shipped.gs" "${tree_file}"
note

# --------------------------------------------------------------------------------------------
# Build (mod-build.sh runs mod-validate.sh first: lexer, encoding and symbol checks)
# --------------------------------------------------------------------------------------------
note "== build =="
"${project_dir}/scripts/mod-build.sh" "${project_dir}/mods/${MOD_ID}" \
  --determinism-runs 2 --force | tee "${work_dir}/build.log"
build_id="$(awk '/^  build /{print $2}' "${work_dir}/build.log" | tail -1)"
[[ -n "${build_id}" ]] || die "could not read the build id"
build_dir="${artifacts_dir}/build/${MOD_ID}/${build_id}"
note "  build ${build_dir}"

extract_member "${build_dir}/gs.mpq" "${HOTKEY_MEMBER}" "${work_dir}/hotkey.packed.gs"
check "the member read back out of the packed archive is the edited source" \
  files_identical "${work_dir}/hotkey.packed.gs" "${tree_file}"
check "the member read back out of the packed archive verifies against the shipped member" \
  probe_tool verify "${work_dir}/hotkey.shipped.gs" "${work_dir}/hotkey.packed.gs"
require "the build changes gs.mpq and no other archive" \
  only_gs_in_build "${build_dir}/build.json"
cp "${work_dir}/hotkey.packed.gs" "${out_dir}/packed-hotkey.gs"
note "  archive sha256 $(file_hash "${build_dir}/gs.mpq")"
note "  build id ${build_id}"
note

note "== result =="
if (( failures > 0 )); then
  note "  ${failures} check(s) FAILED. See above."
  exit 1
fi
note "  all offline checks passed. Report: ${report}"
note "  install: scripts/install-dev.sh ${MOD_ID} ${build_id}"
