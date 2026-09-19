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

# Refuse while the game is running. Copied in spirit from scripts/restore-game-archives.sh: a false
# NEGATIVE here -- the guard staying quiet while the game is actually up -- is what lets an archive
# get swapped under a live process, a corruption no checksum afterwards can undo. A false POSITIVE
# only costs a loud refusal and a re-run. That asymmetry is why this guard is written, tested and
# re-widened to fail CLOSED: every widening below trades a cheap false positive for closing a false
# negative, on purpose, and says so. (`scripts/restore-game-archives.sh` had this backwards in an
# earlier draft of this branch; fixed there too -- see that file's own comment.)
#
# The DOS command line the game presents, as an extended regular expression anchored at the start.
#
# **The profiles do not agree on it.** Observed in gameplay, both live on 2026-09-19:
#
#   Development  c:\program files (x86)\steam\steamapps\common\lords of magic special edition\english\lomse.exe /* MVK_CONFIG_FULL_IMAGE_VIEW_SWIZZLE=1   (PID 77245)
#   3.02         d:\lomse.exe /*                                                                                                                          (PID 47723)
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
# **This pipeline manages FOUR profiles -- vanilla, 3.02 and GS5R3 via `profile_app` below, plus
# Development via `dev_profile_root` -- and every
# observation of a real command line anywhere in this repository (both bullets above, this file's
# own comments, `tests/test_mod_pipeline.py`, `docs/loose-files.md`) is Development and 3.02 ONLY.**
# Evidence class **Observed in gameplay**, and only for those two. Vanilla and GS5R3 are
# **Not established**: nobody has run `ps -o command=` against either while it was live, and
# `scripts/restore-game-archives.sh` -- the other script this guard now protects -- targets GS5R3
# specifically. This is not a hypothetical gap: it is the exact position the `game_subpath` bullet
# above describes -- "somebody watched a real profile [and] applied it to a profile it had never
# been measured against" -- for the two profiles this branch has not looked at. An attended
# measurement against a live GS5R3 and a live vanilla process is on the run sheet; until it lands,
# this pattern trusts that both use the same argv shape as Development/3.02 (a DOS path, at the
# start of the command line) without having checked. The anchor at `^` is only safe on that
# assumption -- see the anchor's own warrant a few lines down for why the assumption is being made
# rather than replaced with `.*`.
#
# So the directory is no longer named at all. The only property shared by every observed and every
# plausible layout is that the command line is a DOS path ending in `lomse.exe`:
#
#   ^"?([A-Za-z]:[\\/]|[\\][\\])(.*[\\/])?lomse[.]exe(["[:space:]]|$)
#
# This closes the false-negative class above at the cost of a new false-positive class: a Windows
# process running inside Wine that merely NAMES `lomse.exe` in its own arguments --
# `c:\windows\system32\cmd.exe /c dir c:\games\lomse.exe` -- now matches too, and so does a real
# `lomse.exe` sitting under any directory, e.g. `c:\games\lomse.exe`. That trade is deliberate, not
# an oversight: a false positive here REFUSES LOUDLY -- `install-dev.sh`/`restore-dev.sh` print
# "lomse.exe is running; quit the game first." and exit -- while a false negative swaps archives
# under a live process. See `tests/test_mod_pipeline.py`'s `ACCEPTED_FALSE_POSITIVES`, which pins
# the trade with a test asserting the cmd.exe case DOES match, on purpose.
#
# **Three more forms admitted, all at no cost to the false-positive class above** -- each keeps the
# match anchored to position zero (mod an optional leading quote), so none of them opens the
# "mentioned somewhere in an argument" class the anchor exists to close:
#
#   * `[\\/]` in place of `[\\]` for every separator: some Wine builds hand back a forward-slash
#     path for a drive letter (`Z:/home/...`), and nothing about a DOS drive letter requires the
#     backslash specifically.
#   * `[\\][\\]` as an alternative to `[A-Za-z]:` for the whole prefix: a UNC path
#     (`\\server\share\...\lomse.exe`) names no drive letter at all, and Wine can map a drive to a
#     network share.
#   * An optional leading `"` (`^"?`), and `"` added to the trailing boundary class alongside
#     whitespace and end-of-line: a path containing a space is often passed to `CreateProcess`
#     quoted (`"C:\Program Files\...\lomse.exe" /*`), and quoting the argument does not change
#     what a human would call "the game running" or what a corruption-safety guard needs to catch.
#     Checked directly against every fixture in `NOT_THE_GAME`: none of them starts with `"` or
#     ends `lomse.exe"`, so this costs nothing there.
#
# **Deliberately NOT widened for a wrapper executable appearing before the DOS path** -- e.g. a
# visible `wine64-preloader`, a Wineskin launcher's own `.../Wineskin.framework/bin/wine64 <path>`
# invocation, or a Proton `waitforexitandrun <native path>` line. Structurally, "some prefix, then
# whitespace, then the DOS path" cannot be told apart from "some unrelated command whose own
# arguments happen to quote the DOS path" -- that is exactly the `grep --fixed-strings ...` decoy,
# and the ORIGINAL bare-name defect this guard's whole history is about (see the anchor's warrant
# below). Covering the wrapper case would reopen that class for every one of this project's own
# tools that already take `lomse.exe`'s path as an argument. Not covering it is a real gap for a
# machine where the visible process IS the wrapper rather than the emulated Windows process, but
# there is first-party evidence it is not a gap on the machine this repository targets: `README.md`
# states this project runs ONLY on Apple Silicon macOS via locally-built Wineskin wrappers, with no
# Linux, SteamOS, Proton or network-share install anywhere in its scope; and both Development/3.02
# command lines this guard has ever actually observed live (PIDs 77245 and 47723, both 2026-09-19)
# show the DOS path itself as the process's own command line, with no preloader or launcher token in
# front of it -- meaning that on the Wine build this project actually runs, the OS-visible process
# already IS the emulated Windows one by the time a human or this guard would check. That evidence
# does not extend to GS5R3 or vanilla (see the four-profiles note above). If this guard is ever
# reused to protect a Linux/Proton or Steam Deck install, this decision needs revisiting; it is not
# revisited here for a machine that does not exist in this repository's scope.
game_command_pattern() {
  # Anchored at the drive letter, the UNC `\\` prefix, or an opening quote ahead of either, and
  # terminated by a closing quote, whitespace or end of line, so a command line that merely
  # MENTIONS the executable somewhere later in its own arguments -- as opposed to BEGINNING with a
  # path ending in it -- still cannot match; the `grep --fixed-strings ...` and bare-name-prefixed
  # decoys in the test file exist for exactly that boundary. The directory group is OPTIONAL, for
  # the drive-root profile, and unstructured (`.*`) rather than a specific path, for every profile
  # that is not.
  printf '^"?([A-Za-z]:[\\\\/]|[\\\\][\\\\])(.*[\\\\/])?lomse[.]exe(["[:space:]]|$)'
}

refuse_if_game_running() {
  # `pgrep -f` matches ANY live command line containing the pattern -- including this script's own
  # shell and anything that merely mentions the name. The game runs under Wine and its command line
  # BEGINS with a DOS drive path, so the pattern is anchored to the start.
  #
  # **The anchor's warrant** -- this is not a theoretical risk, it has actually happened, twice, to
  # this exact guard, on this exact machine, while this exact file was being edited: an unanchored
  # `pgrep -f 'lomse.exe'` matched the agent harness shell (`/bin/zsh -c ...`) running a command
  # whose own text happened to contain the string `lomse.exe`, and separately matched a bare
  # `python3 -c` invocation whose source contained the literal, with nothing to do with the game.
  # Three agents tripped it in one day, and writing the fix tripped it again: a comment in the patch
  # command itself contained `d:\lomse.exe`. The anchor is what makes those quiet instead of a false
  # refusal, and it depends on one fact: that the game's OWN command line, as `ps`/`pgrep` see it,
  # BEGINS with the DOS path rather than naming it somewhere inside a wrapper's arguments. That fact
  # is Observed in gameplay for Development and 3.02 only -- see `game_command_pattern`'s "four
  # profiles" note above for GS5R3 and vanilla, which have not been checked.
  #
  # Observed in gameplay, 2026-09-19, against the live process, PID 77245:
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
  #
  # **The exit status is branched on explicitly, and `2>&1` is no longer thrown away.** `pgrep`
  # returns 0 for a match, 1 for no match, 2 for a malformed pattern (a syntax error in the ERE
  # `game_command_pattern` produces) and 3 for an internal error (`man pgrep`, EXIT STATUS); a
  # missing or non-executable `pgrep` gives bash's own 127. `if pgrep ...; then die; fi` treats
  # every one of those non-zero codes identically to "no match" and proceeds -- so a future edit
  # that makes the pattern invalid, or a `pgrep` that cannot run at all, would silently turn this
  # guard OFF rather than error, and nothing would notice. Only 1 means "proceed"; everything else
  # that is not 0 is a broken guard and has to `die` loudly, with the exit code and pgrep's own
  # stderr, rather than being folded into "the game is not running".
  local pgrep_output pgrep_status
  if pgrep_output="$(pgrep -if "$(game_command_pattern)" 2>&1)"; then
    pgrep_status=0
  else
    pgrep_status=$?
  fi
  case "${pgrep_status}" in
    0)
      die "lomse.exe is running; quit the game first."
      ;;
    1)
      return 0
      ;;
    *)
      die "pgrep failed while checking whether lomse.exe is running (exit ${pgrep_status}): ${pgrep_output}"
      ;;
  esac
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
