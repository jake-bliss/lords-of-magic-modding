#!/usr/bin/env bash
# Shared plumbing for validate / build / install-dev / restore-dev.
#
# Three rules are enforced here rather than repeated in four scripts:
#
#   1. No step runs against a stale tool. A stale `.build/lom-mpq` once produced 17 confusing
#      Python test failures, so both tools are rebuilt before every run and their digests are
#      recorded in the build. `scripts/build-tools.sh` recompiles unconditionally and `cargo build`
#      is incremental, so this costs a second, not a rebuild.
#   2. No step discovers its inputs. A profile is named, never searched for.
#   3. Nothing writes inside ~/Applications except through tools/install_guard.py.

# shellcheck disable=SC2034  # these are consumed by the scripts that source this file.

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
artifacts_dir="${LOM_ARTIFACTS_DIR:-${project_dir}/artifacts}"
applications_dir="${LOM_APPLICATIONS_DIR:-${HOME}/Applications}"
game_subpath='Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English'

# The archives this pipeline packs. Kept in step with tools/mod_tree.py's SUPPORTED_ARCHIVES.
PIPELINE_ARCHIVES=(gs.mpq pic.mpq imp.mpq sndfx.mpq special.mpq)

file_hash() {
  shasum -a 256 "$1" | cut -d' ' -f1
}

die() {
  echo "$@" >&2
  exit 1
}

# The app bundle for a profile label. The table is duplicated from tools/mod_tree.py on purpose:
# bash and Python each need it, and a generated third copy would be one more thing to drift.
# tests/test_mod_pipeline.py asserts the two agree.
profile_app() {
  case "$1" in
    vanilla) echo 'Steambuild 32 64bit DXVK.app' ;;
    patch302) echo 'Lords of Magic 3.02.app' ;;
    gs5r3) echo 'Lords of Magic GS5R3.app' ;;
    *) die "unknown profile label: $1 (expected vanilla, patch302 or gs5r3)" ;;
  esac
}

profile_game_dir() {
  echo "${applications_dir}/$(profile_app "$1")/${game_subpath}"
}

# The recovered name list for a profile's archive, or empty when the archive names itself.
#
# Only archives that carry NO `(listfile)` get one. `gs.mpq` does carry one, and supplying the
# recovered names for its 372 unnamed entries would re-address those entries from an unnamed
# multiset to per-block named members inside the shape check -- a change to the exact path Phase 4
# proved against the engine on 2026-09-18. There is no evidence that would be an improvement and
# there is evidence the current path works, so `gs.mpq` is left alone.
#
# `pic.mpq` has no `(listfile)` and no self-named member at all, so without this every member lists
# under a `File%08u.xxx` pseudo-name and cannot be written: **Observed 2026-09-18**, StormLib
# refuses SFileAddFileEx on a pseudo-name with error 22, and a real name is refused earlier for not
# being in the archive's own catalogue.
# `imp.mpq`, `sndfx.mpq` and `special.mpq` are all in the same position as `pic.mpq`: no
# `(listfile)`, no self-named member, every entry listing as `File%08u.xxx` without this. Measured
# 2026-09-18, the recovered lists name every entry of each -- 3,600 of 3,600, 1,880 of 1,880 and
# 1,218 of 1,218 -- and all three archives are byte-identical across the installed profiles, so one
# file serves each profile.
profile_listfile() {
  local profile="$1" archive="$2" names="${project_dir}/reports/member-names"
  case "${archive}" in
    pic.mpq)
      case "${profile}" in
        vanilla|patch302) echo "${names}/vanilla-and-302-pic-recovered.txt" ;;
        gs5r3) echo "${names}/gs5r3-pic-recovered.txt" ;;
      esac
      ;;
    imp.mpq) echo "${names}/all-profiles-imp-recovered.txt" ;;
    sndfx.mpq) echo "${names}/all-profiles-sndfx-recovered.txt" ;;
    special.mpq) echo "${names}/all-profiles-special-recovered.txt" ;;
    *) echo "" ;;
  esac
}

# Rebuild both tools and export their paths and digests.
#
# The digests go into build.json so a build that behaves differently from another can be traced to
# the tool rather than argued about.
prepare_tools() {
  "${project_dir}/scripts/build-tools.sh" >/dev/null || die "could not build .build/lom-mpq"
  mpq_tool="${project_dir}/.build/lom-mpq"
  [[ -x "${mpq_tool}" ]] || die "missing ${mpq_tool}"

  ( cd "${project_dir}/spikes/asset-viewer" && cargo build --quiet ) \
    || die "could not build lom-asset-viewer"
  viewer_tool="${project_dir}/spikes/asset-viewer/target/debug/lom-asset-viewer"
  [[ -x "${viewer_tool}" ]] || die "missing ${viewer_tool}"

  mpq_tool_sha="$(file_hash "${mpq_tool}")"
  viewer_tool_sha="$(file_hash "${viewer_tool}")"
}

# `--gs-facts` for a base archive, cached under a key that includes the archive's own digest.
#
# Keying the cache on the archive's content rather than on its path is what makes a stale cache
# impossible: a different archive is a different key, so there is no cache to invalidate and no
# window in which the pipeline reads facts about bytes it is not packing.
base_gs_facts() {
  local archive_path="$1" archive_sha cache
  archive_sha="$(file_hash "${archive_path}")"
  cache="${artifacts_dir}/base-facts/${archive_sha}.jsonl"
  if [[ ! -s "${cache}" ]]; then
    mkdir -p "$(dirname "${cache}")"
    "${viewer_tool}" --gs-facts "${archive_path}" > "${cache}.partial" 2>/dev/null \
      || { rm -f "${cache}.partial"; die "could not read GameScript facts from ${archive_path}"; }
    mv "${cache}.partial" "${cache}"
  fi
  echo "${cache}"
}

# Refuse while the game is running. Copied in spirit from scripts/restore-game-archives.sh: an
# archive swapped under a live process is a class of corruption no checksum afterwards can undo.
refuse_if_game_running() {
  if pgrep -f 'lomse.exe' >/dev/null 2>&1; then
    die "lomse.exe is running; quit the game first."
  fi
}

# Approve a path against the one-entry allowlist, or exit.
#
# The guard lives in Python so one implementation serves both languages and so the refusal tests
# exercise the same code the installer calls.
approve_dev_path() {
  PYTHONPATH="${project_dir}/tools" python3 "${project_dir}/tools/install_guard.py" \
    --applications-dir "${applications_dir}" "$1" \
    || die "refused by the install allowlist"
}

dev_profile_root() {
  echo "${applications_dir}/Lords of Magic Development.app"
}

dev_metadata_dir() {
  echo "$(dev_profile_root)/.lom-pipeline"
}
