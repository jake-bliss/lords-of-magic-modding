#!/usr/bin/env bash
# Build the two-rung cheat-keys ladder, and check offline everything that can be checked without
# the engine.
#
#   scripts/build-cheat-keys-ladder.sh [--rung A|B]...
#
# Installs nothing and launches nothing. `docs/cheat-keys-ladder.md` is the run sheet for the
# attended part; this script produces what that sheet refers to: two builds under artifacts/build/
# and an offline-checks report under artifacts/cheat-keys-ladder/.
#
# Rung A (cheat-keys-noop) re-emits `gs\hotkey.gs` byte-identically -- the repack control. Rung B
# (cheat-keys-true) flips the single `/cheat_keys false def` token to `true` and nothing else.
# Both mod trees are rebuilt from scratch on every run, for the same reason
# scripts/build-acceptance-ladder.sh gives: mod-seed.sh refuses to overwrite a file already in a
# tree, so a ladder whose inputs depended on what a previous run left behind would not be
# reproducible.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

HOTKEY_MEMBER='gs\hotkey.gs'
FLAG_BEFORE='/cheat_keys false def'
FLAG_AFTER='/cheat_keys true def'

wanted_rungs=()
while (( $# )); do
  case "$1" in
    --rung) wanted_rungs+=("${2:?--rung needs A or B}"); shift 2 ;;
    --help|-h) sed -n '2,15p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

wants() {
  (( ${#wanted_rungs[@]} == 0 )) && return 0
  local rung
  for rung in "${wanted_rungs[@]}"; do
    [[ "${rung}" == "$1" ]] && return 0
  done
  return 1
}

prepare_tools
game_dir="$(profile_game_dir vanilla)"
out_dir="${artifacts_dir}/cheat-keys-ladder"
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
  else
    note "  FAIL  ${label}"
    sed 's/^/        /' "${work_dir}/check.log" | tee -a "${report}"
    failures=$(( failures + 1 ))
  fi
}

files_identical() {
  cmp -s "$1" "$2"
}

reset_tree() {
  # The path is computed from the project directory and the mod id, never globbed, and is under
  # mods/ by construction.
  rm -rf "${project_dir}/mods/$1/archives"
}

seed_mod() {
  "${project_dir}/scripts/mod-seed.sh" "${project_dir}/mods/$1" "gs.mpq:${HOTKEY_MEMBER}" >/dev/null
}

build_mod() {
  local mod_id="$1"
  "${project_dir}/scripts/mod-build.sh" "${project_dir}/mods/${mod_id}" \
    --determinism-runs 2 --force | tee "${work_dir}/${mod_id}.build.log"
  build_id="$(awk '/^  build /{print $2}' "${work_dir}/${mod_id}.build.log" | tail -1)"
  [[ -n "${build_id}" ]] || die "could not read the build id for ${mod_id}"
  build_dir="${artifacts_dir}/build/${mod_id}/${build_id}"
}

extract_hotkey() {
  # extract_hotkey ARCHIVE_PATH OUTPUT -- gs.mpq carries its own (listfile), so no override is
  # needed (see profile_listfile in lib-mod-pipeline.sh: gs.mpq is deliberately left alone).
  # The exporter refuses to overwrite, and this helper is called once per rung against the same
  # work_dir, so the previous rung's file (if any) is removed by its exact computed path first.
  rm -f "$2"
  "${viewer_tool}" --extract "$1" "${HOTKEY_MEMBER}" "$2" >/dev/null
}

# token_only_diff BEFORE AFTER NEEDLE REPLACEMENT -- asserts that AFTER equals BEFORE with every
# occurrence of NEEDLE (there must be exactly one) replaced by REPLACEMENT, and nothing else
# different. This is the claim the brief actually asks for -- "differ only in that token" -- which
# a positional `cmp` cannot state once the edit changes the file's length: every byte from the
# token onward shifts by len(REPLACEMENT)-len(NEEDLE) and a byte-for-byte compare reports all of
# them as different even though only the token was touched.
token_only_diff() {
  PYTHONDONTWRITEBYTECODE=1 python3 -c '
import sys
before_path, after_path, needle, replacement = sys.argv[1:5]
before = open(before_path, "rb").read()
after = open(after_path, "rb").read()
needle_b = needle.encode("ascii")
replacement_b = replacement.encode("ascii")
count = before.count(needle_b)
if count != 1:
    print(f"expected exactly one occurrence of {needle!r} in the BEFORE member, found {count}")
    sys.exit(1)
idx = before.index(needle_b)
expected = before[:idx] + replacement_b + before[idx + len(needle_b):]
if after != expected:
    # Report the first differing byte to name the actual mismatch rather than just failing.
    shortest = min(len(after), len(expected))
    first_diff = next((i for i in range(shortest) if after[i] != expected[i]), shortest)
    print(f"AFTER does not equal BEFORE with the token swapped; first difference at byte {first_diff}")
    print(f"  before length {len(before)}  after length {len(after)}  expected length {len(expected)}")
    sys.exit(1)
before_prefix = before[:idx]
after_prefix = after[:idx]
before_suffix = before[idx + len(needle_b):]
after_suffix = after[idx + len(replacement_b):]
print(f"token replaced at byte offset {idx}: {needle!r} -> {replacement!r}")
print(f"prefix (0..{idx}) identical: {before_prefix == after_prefix}")
print(f"suffix ({len(before) - len(before_suffix)}..end before / "
      f"{len(after) - len(after_suffix)}..end after) identical: {before_suffix == after_suffix}")
print(f"before {len(before)} bytes, after {len(after)} bytes, "
      f"delta {len(after) - len(before)} (len({replacement!r})-len({needle!r})={len(replacement_b)-len(needle_b)})")
' "$1" "$2" "$3" "$4"
}

note "== cheat-keys ladder, offline checks =="
note "  project  ${project_dir}"
note "  baseline ${game_dir}"
note "  gs.mpq sha256 $(file_hash "${game_dir}/gs.mpq")"
note

# --------------------------------------------------------------------------------------------
# Rung A: cheat-keys-noop -- the repack control
# --------------------------------------------------------------------------------------------
if wants A; then
  note "== rung A: cheat-keys-noop =="
  reset_tree cheat-keys-noop
  seed_mod cheat-keys-noop
  tree_file="${project_dir}/mods/cheat-keys-noop/archives/gs.mpq/gs/hotkey.gs"

  # Extract the shipped member directly for comparison, independent of what mod-seed.sh wrote,
  # so a seeding bug could not make this control agree with itself.
  extract_hotkey "${game_dir}/gs.mpq" "${work_dir}/hotkey.shipped.gs"
  check "rung A: the seeded tree file is byte-identical to the shipped member" \
    files_identical "${tree_file}" "${work_dir}/hotkey.shipped.gs"

  build_mod cheat-keys-noop
  note "  build ${build_dir}"
  extract_hotkey "${build_dir}/gs.mpq" "${work_dir}/hotkey.rungA.gs"
  check "rung A: the member read back out of the packed archive is the shipped member" \
    files_identical "${work_dir}/hotkey.rungA.gs" "${work_dir}/hotkey.shipped.gs"
  cp "${work_dir}/hotkey.rungA.gs" "${out_dir}/rungA-hotkey.gs"
  note "  archive sha256 $(file_hash "${build_dir}/gs.mpq")"
  note "  build id ${build_id}"
  note
fi

# --------------------------------------------------------------------------------------------
# Rung B: cheat-keys-true -- the one-token flip
# --------------------------------------------------------------------------------------------
if wants B; then
  note "== rung B: cheat-keys-true =="
  reset_tree cheat-keys-true
  seed_mod cheat-keys-true
  tree_file="${project_dir}/mods/cheat-keys-true/archives/gs.mpq/gs/hotkey.gs"

  extract_hotkey "${game_dir}/gs.mpq" "${work_dir}/hotkey.shipped.gs"
  [[ -f "${out_dir}/rungA-hotkey.gs" ]] || extract_hotkey "${game_dir}/gs.mpq" "${out_dir}/rungA-hotkey.gs"

  occurrences="$(PYTHONDONTWRITEBYTECODE=1 python3 -c '
import sys
data = open(sys.argv[1], "rb").read()
print(data.count(sys.argv[2].encode("ascii")))
' "${tree_file}" "${FLAG_BEFORE}")"
  [[ "${occurrences}" == "1" ]] \
    || die "expected exactly one occurrence of '${FLAG_BEFORE}' in the seeded member, found ${occurrences}"

  PYTHONDONTWRITEBYTECODE=1 python3 -c '
import sys
path, needle, replacement = sys.argv[1:4]
data = open(path, "rb").read()
data = data.replace(needle.encode("ascii"), replacement.encode("ascii"))
open(path, "wb").write(data)
' "${tree_file}" "${FLAG_BEFORE}" "${FLAG_AFTER}"

  cp "${tree_file}" "${work_dir}/hotkey.edited.gs"
  check "rung B: the edited tree file differs from the shipped member only in the cheat_keys token" \
    token_only_diff "${work_dir}/hotkey.shipped.gs" "${work_dir}/hotkey.edited.gs" \
    "${FLAG_BEFORE}" "${FLAG_AFTER}"

  build_mod cheat-keys-true
  note "  build ${build_dir}"
  extract_hotkey "${build_dir}/gs.mpq" "${work_dir}/hotkey.rungB.gs"
  check "rung B: the member read back out of the packed archive matches the edited source" \
    files_identical "${work_dir}/hotkey.rungB.gs" "${work_dir}/hotkey.edited.gs"
  check "rung B: the packed member differs from rung A's packed member only in the cheat_keys token" \
    token_only_diff "${out_dir}/rungA-hotkey.gs" "${work_dir}/hotkey.rungB.gs" \
    "${FLAG_BEFORE}" "${FLAG_AFTER}"
  cp "${work_dir}/hotkey.rungB.gs" "${out_dir}/rungB-hotkey.gs"
  note "  archive sha256 $(file_hash "${build_dir}/gs.mpq")"
  note "  build id ${build_id}"
  note
fi

note "== result =="
if (( failures > 0 )); then
  note "  ${failures} check(s) FAILED. See above."
  exit 1
fi
note "  all offline checks passed. Report: ${report}"
