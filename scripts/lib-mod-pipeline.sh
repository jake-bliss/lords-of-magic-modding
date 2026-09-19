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
PIPELINE_ARCHIVES=(gs.mpq pic.mpq)

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
profile_listfile() {
  local profile="$1" archive="$2" names="${project_dir}/reports/member-names"
  case "${archive}" in
    pic.mpq)
      case "${profile}" in
        vanilla|patch302) echo "${names}/vanilla-and-302-pic-recovered.txt" ;;
        gs5r3) echo "${names}/gs5r3-pic-recovered.txt" ;;
      esac
      ;;
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
# The DOS command line the game presents, as an extended regular expression anchored at the start.
#
# **The profiles do not agree on it.** Observed 2026-09-19, both live:
#
#   Development  c:\program files (x86)\steam\steamapps\common\lords of magic special edition\english\lomse.exe /* MVK_CONFIG_FULL_IMAGE_VIEW_SWIZZLE=1
#   3.02         d:\lomse.exe /*
#
# 3.02 runs from the DRIVE ROOT. So the directory part is optional, and a pattern written from
# either profile alone misses the other -- which is the entire history of this guard:
#
#   * `^[A-Za-z]:[\]lomse[.]exe` matched 3.02 and missed Development. It was not invented from a
#     bad guess; somebody watched a real profile. It was then applied to a profile it had never
#     been measured against.
#   * `^[A-Za-z]:[\].*lomse[.]exe` matched both, and also `c:\tools\notlomse.exe` and any command
#     line mentioning the path in an argument.
#   * The full path with no optional part matched Development and missed 3.02 -- the first defect
#     again, with the profiles swapped.
#   * A version derived from `game_subpath` -- the path this pipeline INSTALLS TO -- closed both of
#     the above by making the directory optional but pinned to that exact path. **That is still the
#     wrong source.** `game_subpath` names where the pipeline writes; it says nothing about where a
#     running game was LAUNCHED FROM, and nothing requires those to be the same tree. A 64-bit
#     Wineskin bottle (`Program Files`, no `(x86)`) and a Wineskin profile mapping its game drive to
#     something other than `c:`/`d:` are two more real layouts a game can launch from that the
#     installer never names -- a pattern built from the install path is one unlisted layout behind
#     by construction, which is exactly the defect class the profile-vs-profile bullets above are
#     made of, recurring one level up.
#
# So the directory is no longer named at all. The only property shared by every observed and every
# plausible layout is that the command line is a DOS path ending in `lomse.exe`:
#
#   ^[A-Za-z]:[\\](.*[\\])?lomse[.]exe([[:space:]]|$)
#
# This closes the false-negative class above at the cost of a new false-positive class: a Windows
# process running inside Wine that merely NAMES `lomse.exe` in its own arguments --
# `c:\windows\system32\cmd.exe /c dir c:\games\lomse.exe` -- now matches too, and so does a real
# `lomse.exe` sitting under any directory, e.g. `c:\games\lomse.exe`. That trade is deliberate, not
# an oversight, and it is asymmetric on purpose: a false positive here REFUSES LOUDLY --
# `install-dev.sh`/`restore-dev.sh` print "lomse.exe is running; quit the game first." and exit --
# while a false negative swaps archives under a live process, a corruption no checksum afterwards
# can undo. See `tests/test_mod_pipeline.py`'s `ACCEPTED_FALSE_POSITIVES`, which pins the trade with
# a test asserting the cmd.exe case DOES match, on purpose.
game_command_pattern() {
  # Anchored at the drive letter, and terminated by whitespace or end of line, so a command line
  # that merely MENTIONS the executable somewhere later in its own arguments -- as opposed to
  # BEGINNING with a path ending in it -- still cannot match; the `grep --fixed-strings ...` and
  # bare-name-prefixed decoys in the test file exist for exactly that boundary. The directory group
  # is OPTIONAL, for the drive-root profile, and unstructured (`.*`) rather than a specific path,
  # for every profile that is not.
  printf '^[A-Za-z]:[\\\\](.*[\\\\])?lomse[.]exe([[:space:]]|$)'
}

refuse_if_game_running() {
  # `pgrep -f` matches ANY live command line containing the pattern -- including this script's own
  # shell and anything that merely mentions the name. The game runs under Wine and its command line
  # BEGINS with a DOS drive path, so the pattern is anchored to the start.
  #
  # Observed 2026-09-19 against the live process, PID 77245:
  #
  #   c:\program files (x86)\steam\steamapps\common\lords of magic special edition\english\lomse.exe /* MVK_CONFIG_FULL_IMAGE_VIEW_SWIZZLE=1
  #
  # `game_command_pattern` above has its own history of failures; see it for the full ladder. The
  # short version is that a bare drive-root pattern went dead for a day, and the first fix
  # (`^[A-Za-z]:[\\].*lomse[.]exe`) reopened the false-positive class the anchor exists to close.
  #
  # **`-i` re-measured 2026-09-19 against the pattern actually shipped here.** The earlier
  # justification was narrower than "Wine hands back lower case" but WAS load-bearing for more
  # than the uppercase fixture: that earlier pattern was derived from `game_subpath` and so
  # embedded literal, mixed-case path components (`Program Files`, `Steam`); without `-i`, the
  # observed Development command line -- itself all lower-case -- failed to match THAT literal
  # text, while `d:\lomse.exe` still matched. That reason is gone now that the directory is `.*`:
  # there is no literal mixed-case text left to fail against. Measured directly against the
  # pattern above: with `-i` dropped, both `d:\lomse.exe` and the observed Development command
  # line still match (their own text is already lower-case), and only an upper-case profile like
  # `E:\LOMSE.EXE` stops matching. So `-i` is kept for that one case alone -- Windows paths and
  # executable names are case-INSENSITIVE, and a Wineskin profile presenting an upper-case command
  # line is equally valid -- not for the reason the previous comment gave.
  # `tests/test_mod_pipeline.py`'s `UPPERCASE_OTHER_DRIVE_ARGV0` is the one fixture pinning it.
  if pgrep -if "$(game_command_pattern)" >/dev/null 2>&1; then
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
