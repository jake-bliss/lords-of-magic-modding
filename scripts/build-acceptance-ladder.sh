#!/usr/bin/env bash
# Build every rung of the engine-acceptance ladder, and check offline everything that can be
# checked without the engine.
#
#   scripts/build-acceptance-ladder.sh [--rung N]...
#
# Installs nothing and launches nothing. `docs/engine-acceptance-ladder.md` is the run sheet for
# the attended part; this script produces what that sheet refers to: five builds under
# artifacts/build/, and an expected-value PNG per rung under artifacts/engine-acceptance-ladder/,
# exported from the PACKED archive by our own decoder so that the person at the keyboard compares a
# screen against a picture rather than against a sentence.
#
# Every rung's edit is re-derived from the installed baseline on every run. Nothing here reads a
# digest this project wrote down earlier and treats it as the truth: a recorded digest detects a
# change, it does not stand in for the bytes.
#
# The mod trees are rebuilt from scratch each run, because `scripts/mod-seed.sh` refuses to
# overwrite a file in a tree -- rightly, since a tree is the only copy of an edit -- and a ladder
# whose inputs depended on what a previous run left behind would not be reproducible.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

BASE_PROFILE=vanilla
CURSOR_MEMBER='iface\cursors.imp'
CURSOR_FRAME=111
ADDED_MEMBER='iface\ladder.imp'
MENU_MEMBER='lbm\newgame.lbm'
# The stripe, in the member's own pixel coordinates, half-open. Chosen inside the 640x480 image
# with a margin on all four sides so that every edge of the screen is an untouched control.
STRIPE=(260 60 380 420)
# Palette index 1 of `lbm\newgame.lbm` is pure red in that member's own CMAP, and the engine was
# observed rendering it as red from this member on 2026-09-18.
STRIPE_INDEX=1
# Disjoint palette-index swaps: eight body colours of the POINTER gauntlet exchanged with eight
# near-white entries the frame does not use. Disjoint, therefore a permutation, therefore
# length-preserving under the IMP run-length encoder. See tools/png_index_patch.py.
CURSOR_SWAPS=("56=2" "41=3" "99=4" "98=6" "120=7" "157=8" "139=9" "243=15")
# The audio rungs. `wav\welcome.wav` is held byte-identically by BOTH audio archives, so both are
# rewritten; the audible rung gives each a different frequency so that one listen names which one
# the engine opened. Three octaves apart, and neither is a sound the game contains.
# Two members, chosen for different reasons and both needed.
#
# `wav\welcome.wav` is the observable: it plays unattended as the main menu opens, it is 5.7
# seconds long, and it is impossible to miss. Its WAVE layout is the weakest in the archive --
# `fmt |data` with an EVEN data chunk and no ancillary chunk at all -- so it exercises no pad byte
# and carries nothing verbatim.
#
# `wav\button.wav` covers exactly what that misses: `fmt |data|LIST` with an ODD 915-byte data
# chunk, so a pad byte is written BETWEEN two chunks (not as a trailing byte that a reader could
# skip), and a 68-byte `LIST` that `--import-wave` carries through untouched. It is also the engine's
# default button sound, so it can be triggered on demand as many times as the observer likes.
#
# Measured 2026-09-18 over both archives: the only chunk that is ever odd-sized is `data` -- 1,294
# of 1,880 in sndfx.mpq and 754 of 1,218 in special.mpq -- NO member of either archive has an
# odd-sized ancillary chunk, and every one of those 2,048 pad bytes is 0x00.
AUDIO_MEMBERS=('wav\welcome.wav' 'wav\button.wav')
AUDIO_ARCHIVES=(sndfx.mpq special.mpq)
SNDFX_HERTZ=220
SPECIAL_HERTZ=1760
# Rung 9's liveness control: `wav\welcome.wav` gets this SAME tone in BOTH archives, distinct from
# both 220 and 1760 so it cannot be confused with a previous sitting.
WELCOME_HERTZ=440
TONE_MS=750

wanted_rungs=()
while (( $# )); do
  case "$1" in
    --rung) wanted_rungs+=("${2:?--rung needs a number}"); shift 2 ;;
    --help|-h) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
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
game_dir="$(profile_game_dir "${BASE_PROFILE}")"
out_dir="${artifacts_dir}/engine-acceptance-ladder"
mkdir -p "${out_dir}"
work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

report="${out_dir}/offline-checks.txt"
: > "${report}"
failures=0
audio_writer_ready=0

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

files_differ() {
  ! cmp -s "$1" "$2"
}

# The names an archive is read with. `listfile_override` exists for exactly one case: a member
# this ladder ADDED is in no recovered list, so reading it back out of the packed archive needs the
# recovered list plus that one name. It is a variable rather than an argument because every reader
# below takes the archive name in a different position.
listfile_override=""
listfile_for() {
  if [[ -n "${listfile_override}" ]]; then
    echo "${listfile_override}"
    return
  fi
  profile_listfile "${BASE_PROFILE}" "$1"
}

extract_member() {
  # extract_member ARCHIVE_PATH 'MEMBER' OUTPUT ARCHIVE_NAME
  local listfile options=()
  listfile="$(listfile_for "$4")"
  [[ -n "${listfile}" ]] && options=(--listfile "${listfile}")
  "${viewer_tool}" "${options[@]}" --extract "$1" "$2" "$3" >/dev/null
}

export_imp_frame() {
  # export_imp_frame ARCHIVE_PATH 'MEMBER' FRAME OUTPUT.png ARCHIVE_NAME
  local listfile options=()
  listfile="$(listfile_for "$5")"
  [[ -n "${listfile}" ]] && options=(--listfile "${listfile}")
  "${viewer_tool}" --export-imp-frame "$1" "$2" "$3" "$4" "${options[@]}" >/dev/null
}

export_pbm() {
  # export_pbm ARCHIVE_PATH 'MEMBER' OUTPUT.png ARCHIVE_NAME
  local listfile options=()
  listfile="$(listfile_for "$4")"
  [[ -n "${listfile}" ]] && options=(--listfile "${listfile}")
  "${viewer_tool}" --export-pbm "$1" "$2" "$3" "${options[@]}" >/dev/null
}

# tools/asset_validate.py is a library, not a command -- `scripts/mod-validate.sh` reaches it
# through mod_validate. Calling it directly here checks the loose file BEFORE it is packed, so a
# broken re-encode is named at the step that produced it rather than three steps later.
validate_image() {
  PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="${project_dir}/tools" python3 -c '
import sys
from asset_validate import ERROR, validate_image
findings, summary = validate_image(open(sys.argv[1], "rb").read())
for finding in findings:
    print(f"{finding.severity}\t{finding.kind}\t{finding.detail}")
if summary is not None:
    print(f"note\tsummary\t{summary.width}x{summary.height} planes={summary.planes} "
          f"compression={summary.compression} palette={summary.palette_entries} "
          f"pixels-checked={summary.pixels_checked}")
sys.exit(1 if any(f.severity == ERROR for f in findings) else 0)
' "$1"
}

png_compare() {
  PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/png_index_patch.py" "$1" --compare-to "$2"
}

export_wave() {
  # export_wave ARCHIVE_PATH 'MEMBER' OUTPUT.wav ARCHIVE_NAME
  local listfile options=()
  listfile="$(listfile_for "$4")"
  [[ -n "${listfile}" ]] && options=(--listfile "${listfile}")
  "${viewer_tool}" --export-wave "$1" "$2" "$3" "${options[@]}" >/dev/null
}

# Compare two WAVE files by their decoded SAMPLES rather than their bytes. Whether the expected
# value arrived is a question about audio; a byte comparison would also answer it, but it would
# answer it for a file that differed only in an ancillary chunk, and then it would be answering a
# different question without saying so.
#
# Deliberately narrow, not incomplete: this checks channels, sample width, frame rate, the
# DECLARED frame count and the decoded sample payload, and nothing else in the file. Every
# ancillary byte -- `fmt `'s own extra fields, a `LIST` chunk, the `data` chunk's odd-size pad --
# is a different question, already asked by `wav_ancillary_identical` below, which this function
# does not duplicate. A caller that needs both asks both; rung 9 does exactly that, pairing this
# check with `wav_ancillary_identical` rather than folding the two together.
#
# The declared frame count is NOT sufficient on its own, though: `wave.Wave_read.readframes`
# returns however many bytes the underlying `data` chunk actually has, silently short if the file
# is physically truncated below what its own header claims. Two files can therefore agree on
# `getnframes()` while disagreeing on how much of that many frames either one actually delivered --
# and `zip()` over the decoded payloads would then stop at the SHORTER one and report zero
# differences, reading a truncated file as matching an intact one wherever their common prefix
# happens to agree, which a truncation (a dropped tail, not a corrupted middle) always does. The
# payload lengths are therefore compared explicitly, before the per-sample comparison, rather than
# left for `zip` to silently paper over.
wave_samples_equal() {
  PYTHONDONTWRITEBYTECODE=1 python3 -c '
import sys, wave
def read(path):
    with wave.open(path, "rb") as handle:
        return (handle.getnchannels(), handle.getsampwidth(), handle.getframerate(),
                handle.getnframes(), handle.readframes(handle.getnframes()))
left, right = read(sys.argv[1]), read(sys.argv[2])
if left[:4] != right[:4]:
    print(f"format or length differs: {left[:4]} vs {right[:4]}")
    sys.exit(1)
if len(left[4]) != len(right[4]):
    print(f"declared {left[3]} frame(s) but the decoded payload is truncated: "
          f"{len(left[4])} vs {len(right[4])} byte(s) actually read back")
    sys.exit(1)
differing = sum(1 for a, b in zip(left[4], right[4]) if a != b)
print(f"compare\t{sys.argv[1]}\t{sys.argv[2]}\tframes={left[3]}\tdiffering-bytes={differing}")
sys.exit(0 if differing == 0 else 1)
' "$1" "$2"
}

# wav_data_is_uniform_byte WAVE_PATH BYTE -- every byte of the DATA chunk, as decoded by Python's
# own `wave` module rather than by assuming an offset, equals BYTE. Rung 9 uses this to prove
# `sndfx.mpq`'s `button.wav` is digital silence -- 0x80 for 8-bit unsigned PCM, the zero level, not
# an empty or truncated chunk.
wav_data_is_uniform_byte() {
  PYTHONDONTWRITEBYTECODE=1 python3 -c '
import sys, wave
path, expected = sys.argv[1], int(sys.argv[2], 0)
with wave.open(path, "rb") as handle:
    data = handle.readframes(handle.getnframes())
bad = sum(1 for b in data if b != expected)
print(f"{len(data)} sample byte(s), {bad} not at 0x{expected:02x}")
sys.exit(1 if (bad or not data) else 0)
' "$1" "$2"
}

# wav_ancillary_identical TEMPLATE CANDIDATE -- every RIFF chunk except `data` is byte-identical
# between the two files (chunk id, size and body), and `data` itself matches in size and, if its
# size is odd, in its pad byte. It says nothing about the DATA payload itself -- asserting that
# would be asserting the edit did nothing -- which is why rung 9 pairs it with a `files_differ`
# check on the member as a whole.
wav_ancillary_identical() {
  PYTHONDONTWRITEBYTECODE=1 python3 -c '
import struct, sys

def chunks(data):
    cursor = 12  # past "RIFF" size(4) "WAVE"
    out = []
    while cursor + 8 <= len(data):
        chunk_id = data[cursor:cursor + 4]
        (size,) = struct.unpack_from("<I", data, cursor + 4)
        payload_start = cursor + 8
        payload = data[payload_start:payload_start + size]
        pad = size & 1
        pad_byte = data[payload_start + size:payload_start + size + pad]
        out.append((chunk_id, size, payload, pad_byte))
        cursor = payload_start + size + pad
    return out

left, right = open(sys.argv[1], "rb").read(), open(sys.argv[2], "rb").read()
left_chunks, right_chunks = chunks(left), chunks(right)
left_ids, right_ids = [c[0] for c in left_chunks], [c[0] for c in right_chunks]
if left_ids != right_ids:
    print(f"chunk order/ids differ: {left_ids} vs {right_ids}")
    sys.exit(1)
mismatches = []
for (chunk_id, lsize, lpayload, lpad), (_, rsize, rpayload, rpad) in zip(left_chunks, right_chunks):
    name = chunk_id.decode("ascii", "replace")
    if chunk_id == b"data":
        if lsize != rsize:
            mismatches.append(f"data size {lsize} vs {rsize}")
        if lpad != rpad:
            mismatches.append(f"data pad byte {lpad!r} vs {rpad!r}")
        continue
    if lsize != rsize or lpayload != rpayload or lpad != rpad:
        mismatches.append(f"{name} chunk differs")
if mismatches:
    print("; ".join(mismatches))
    sys.exit(1)
print("ancillary chunks and data shape match")
' "$1" "$2"
}

# hertz_for ARCHIVE SNDFX_HZ SPECIAL_HZ -- takes the per-rung assignment as arguments rather than
# reading a global, because rung 8 assigns the opposite frequency to each archive from rung 7 and a
# global pair would silently apply to whichever rung ran second.
hertz_for() {
  case "$1" in
    sndfx.mpq) echo "$2" ;;
    special.mpq) echo "$3" ;;
    *) die "no tone frequency defined for $1" ;;
  esac
}

# Expected-value exports are rewritten on every run. The exporters refuse to overwrite -- rightly,
# since an expected value silently replaced is worse than none -- so the previous run's file is
# removed by an exact computed path first, never by a glob over the directory.
fresh_output() {
  rm -f "$1"
  echo "$1"
}

reset_tree() {
  # The path is computed from the project directory and the mod id, never globbed, and is under
  # mods/ by construction.
  local mod_id="$1"
  rm -rf "${project_dir}/mods/${mod_id}/archives"
}

build_mod() {
  local mod_id="$1"
  "${project_dir}/scripts/mod-build.sh" "${project_dir}/mods/${mod_id}" \
    --determinism-runs 2 --force | tee "${work_dir}/${mod_id}.build.log"
  build_id="$(awk '/^  build /{print $2}' "${work_dir}/${mod_id}.build.log" | tail -1)"
  [[ -n "${build_id}" ]] || die "could not read the build id for ${mod_id}"
  build_dir="${artifacts_dir}/build/${mod_id}/${build_id}"
}

# ------------------------------------------------------------------------------------------------
# Preamble: what the profiles look like before anything is built
# ------------------------------------------------------------------------------------------------
note "== engine-acceptance ladder, offline checks =="
note "  project  ${project_dir}"
note "  baseline ${game_dir}"
note

profile_state() {
  local label app profile_dir archive
  : > "$1"
  for label in vanilla patch302 gs5r3; do
    app="$(profile_app "${label}")"
    profile_dir="${applications_dir}/${app}/${game_subpath}"
    for archive in "${PIPELINE_ARCHIVES[@]}"; do
      if [[ -f "${profile_dir}/${archive}" ]]; then
        echo "${label} ${archive} $(file_hash "${profile_dir}/${archive}")" >> "$1"
      fi
    done
  done
}

profile_state "${work_dir}/profiles.before"
note "-- installed profiles, before --"
sed 's/^/  /' "${work_dir}/profiles.before" | tee -a "${report}"
note

# The rollback source, verified by comparing BYTES with the baseline rather than by re-reading a
# digest this pipeline recorded. A digest snapshot detects a change; it cannot undo one, and it
# cannot notice that the copy it describes was itself overwritten.
note "-- rollback sources in the development profile --"
dev_pristine="$(dev_metadata_dir)/pristine"
if [[ -d "${dev_pristine}" ]]; then
  # A profile exists, so every archive PIPELINE_ARCHIVES now names is a promise this profile
  # should be keeping. A missing one here used to print TODO and nothing else -- informational,
  # not a failed check -- which is exactly how a profile created before `imp.mpq`, `sndfx.mpq` and
  # `special.mpq` joined PIPELINE_ARCHIVES could sit unrecorded for those three indefinitely while
  # this report stayed green. `scripts/install-dev.sh` now refuses to install an archive with no
  # pristine line for exactly this reason; this check exists so the gap is visible before an
  # install is even attempted, not just at the moment it is refused.
  for archive in "${PIPELINE_ARCHIVES[@]}"; do
    if [[ -f "${dev_pristine}/${archive}" ]]; then
      if files_identical "${dev_pristine}/${archive}" "${game_dir}/${archive}"; then
        note "  PASS  pristine/${archive} is byte-identical to the baseline archive"
      else
        note "  FAIL  pristine/${archive} DIFFERS from the baseline archive"
        failures=$(( failures + 1 ))
      fi
    else
      note "  FAIL  pristine/${archive} is not recorded;" \
        "run scripts/install-dev.sh --record-pristine before installing any build that touches it"
      failures=$(( failures + 1 ))
    fi
  done
else
  # No development profile at all is the expected state before scripts/install-dev.sh
  # --create-profile has ever been run -- the run sheet's own first step -- so this stays
  # informational rather than a failure. It is not the same state as the one above: a profile
  # that EXISTS but is missing one archive's record is a promise the pipeline stopped keeping,
  # not a step nobody has reached yet.
  note "  TODO  no development profile yet;" \
    "run scripts/install-dev.sh --create-profile"
fi
note

# ------------------------------------------------------------------------------------------------
# Rungs 0 and 1: repack control, and the IMP encoder as a no-op
# ------------------------------------------------------------------------------------------------
if wants 0 || wants 1; then
  note "== rungs 0+1: imp-cursor-noop =="
  mod_dir="${project_dir}/mods/imp-cursor-noop"
  reset_tree imp-cursor-noop
  "${project_dir}/scripts/mod-seed.sh" "${mod_dir}" "imp.mpq:${CURSOR_MEMBER}" >/dev/null
  tree_file="${mod_dir}/archives/imp.mpq/iface/cursors.imp"
  cp "${tree_file}" "${work_dir}/cursors.pristine.imp"

  export_imp_frame "${game_dir}/imp.mpq" "${CURSOR_MEMBER}" "${CURSOR_FRAME}" \
    "${work_dir}/pointer.png" imp.mpq
  rm -f "${work_dir}/cursors.encoded.imp"
  "${viewer_tool}" --import-png-imp "${work_dir}/pointer.png" \
    "${work_dir}/cursors.pristine.imp" "${CURSOR_FRAME}" "${work_dir}/cursors.encoded.imp" \
    | tee "${work_dir}/rung1.import.log"
  cp "${work_dir}/cursors.encoded.imp" "${tree_file}"

  check "rung 1: our IMP encoder reproduced the shipped member byte for byte" \
    files_identical "${work_dir}/cursors.encoded.imp" "${work_dir}/cursors.pristine.imp"

  build_mod imp-cursor-noop
  note "  build ${build_dir}"
  extract_member "${build_dir}/imp.mpq" "${CURSOR_MEMBER}" "${work_dir}/packed-noop.imp" imp.mpq
  check "rung 0: the member read back out of the packed archive is the shipped member" \
    files_identical "${work_dir}/packed-noop.imp" "${work_dir}/cursors.pristine.imp"
  check "rung 0: every IMP in the packed archive still parses and validates" \
    "${viewer_tool}" --validate-imp "${build_dir}/imp.mpq" \
    --listfile "$(listfile_for imp.mpq)"
  cp "${work_dir}/pointer.png" "$(fresh_output "${out_dir}/rung0-1-pointer-expected.png")"
  note "  expected value: ${out_dir}/rung0-1-pointer-expected.png (the SHIPPED pointer)"
  note "  archive sha256 $(file_hash "${build_dir}/imp.mpq")"
  note "  build id ${build_id}"
  note
fi

# ------------------------------------------------------------------------------------------------
# Rung 2: an IMP pixel change, at identical length
# ------------------------------------------------------------------------------------------------
repaint_cursor() {
  # repaint_cursor TREE_FILE  -- leaves the repainted member in place and the PNG in work_dir
  local tree_file="$1"
  cp "${tree_file}" "${work_dir}/cursors.pristine.imp"
  rm -f "${work_dir}/pointer.png" "${work_dir}/pointer-white.png" \
    "${work_dir}/cursors.white.imp"
  export_imp_frame "${game_dir}/imp.mpq" "${CURSOR_MEMBER}" "${CURSOR_FRAME}" \
    "${work_dir}/pointer.png" imp.mpq
  local swap_options=()
  local swap
  for swap in "${CURSOR_SWAPS[@]}"; do
    swap_options+=(--swap "${swap}")
  done
  PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/png_index_patch.py" \
    "${work_dir}/pointer.png" "${work_dir}/pointer-white.png" "${swap_options[@]}"
  "${viewer_tool}" --import-png-imp "${work_dir}/pointer-white.png" \
    "${work_dir}/cursors.pristine.imp" "${CURSOR_FRAME}" "${work_dir}/cursors.white.imp" \
    | tee "${work_dir}/repaint.import.log"
  cp "${work_dir}/cursors.white.imp" "${tree_file}"
}

assert_length_preserving() {
  grep -q 'shift=0' "${work_dir}/repaint.import.log" \
    && grep -q 'stored=530->530' "${work_dir}/repaint.import.log"
}

if wants 2; then
  note "== rung 2: imp-cursor-repaint =="
  mod_dir="${project_dir}/mods/imp-cursor-repaint"
  reset_tree imp-cursor-repaint
  "${project_dir}/scripts/mod-seed.sh" "${mod_dir}" "imp.mpq:${CURSOR_MEMBER}" >/dev/null
  tree_file="${mod_dir}/archives/imp.mpq/iface/cursors.imp"
  repaint_cursor "${tree_file}"

  check "rung 2: the repaint moved no byte offset inside the sprite (shift=0, 530->530)" \
    assert_length_preserving
  check "rung 2: the member is still 90,000 bytes" \
    files_differ "${work_dir}/cursors.white.imp" "${work_dir}/cursors.pristine.imp"
  note "  member size $(wc -c < "${work_dir}/cursors.white.imp" | tr -d ' ')" \
    "bytes; shipped $(wc -c < "${work_dir}/cursors.pristine.imp" | tr -d ' ') bytes"

  build_mod imp-cursor-repaint
  note "  build ${build_dir}"
  export_imp_frame "${build_dir}/imp.mpq" "${CURSOR_MEMBER}" "${CURSOR_FRAME}" \
    "$(fresh_output "${out_dir}/rung2-pointer-expected.png")" imp.mpq
  check "rung 2: the frame read back out of the packed archive is the frame we drew" \
    png_compare "${out_dir}/rung2-pointer-expected.png" "${work_dir}/pointer-white.png"
  # The internal control, read from the archive rather than asserted: the neighbouring frame of the
  # same member, which the edit never named, must still be the shipped one.
  export_imp_frame "${game_dir}/imp.mpq" "${CURSOR_MEMBER}" 112 \
    "${work_dir}/pointer-invalid-base.png" imp.mpq
  export_imp_frame "${build_dir}/imp.mpq" "${CURSOR_MEMBER}" 112 \
    "${work_dir}/pointer-invalid-built.png" imp.mpq
  check "rung 2: frame 112 of the same member is untouched" \
    png_compare "${work_dir}/pointer-invalid-built.png" "${work_dir}/pointer-invalid-base.png"
  check "rung 2: every IMP in the packed archive still parses and validates" \
    "${viewer_tool}" --validate-imp "${build_dir}/imp.mpq" \
    --listfile "$(listfile_for imp.mpq)"
  note "  expected value: ${out_dir}/rung2-pointer-expected.png"
  note "  archive sha256 $(file_hash "${build_dir}/imp.mpq")"
  note "  build id ${build_id}"
  note
fi

# ------------------------------------------------------------------------------------------------
# Rung 3: the PBM encoder, pixels unchanged, length changed
# ------------------------------------------------------------------------------------------------
if wants 3; then
  note "== rung 3: pic-newgame-reencode =="
  mod_dir="${project_dir}/mods/pic-newgame-reencode"
  reset_tree pic-newgame-reencode
  "${project_dir}/scripts/mod-seed.sh" "${mod_dir}" "pic.mpq:${MENU_MEMBER}" >/dev/null
  tree_file="${mod_dir}/archives/pic.mpq/lbm/newgame.lbm"
  cp "${tree_file}" "${work_dir}/newgame.pristine.lbm"
  rm -f "${work_dir}/newgame.png" "${work_dir}/newgame.reencoded.lbm"
  export_pbm "${game_dir}/pic.mpq" "${MENU_MEMBER}" "${work_dir}/newgame.png" pic.mpq
  "${viewer_tool}" --import-png-pbm "${work_dir}/newgame.png" \
    "${work_dir}/newgame.pristine.lbm" "${work_dir}/newgame.reencoded.lbm"
  cp "${work_dir}/newgame.reencoded.lbm" "${tree_file}"

  note "  member size $(wc -c < "${work_dir}/newgame.reencoded.lbm" | tr -d ' ')" \
    "bytes; shipped $(wc -c < "${work_dir}/newgame.pristine.lbm" | tr -d ' ') bytes"
  check "rung 3: the re-encode really did change the member's bytes" \
    files_differ "${work_dir}/newgame.reencoded.lbm" "${work_dir}/newgame.pristine.lbm"
  check "rung 3: the image is structurally valid" \
    validate_image "${work_dir}/newgame.reencoded.lbm"

  build_mod pic-newgame-reencode
  note "  build ${build_dir}"
  export_pbm "${build_dir}/pic.mpq" "${MENU_MEMBER}" "$(fresh_output "${out_dir}/rung3-menu-expected.png")" pic.mpq
  check "rung 3: the image read back out of the packed archive has the SHIPPED pixels" \
    png_compare "${out_dir}/rung3-menu-expected.png" "${work_dir}/newgame.png"
  note "  expected value: ${out_dir}/rung3-menu-expected.png (identical to the shipped menu)"
  note "  archive sha256 $(file_hash "${build_dir}/pic.mpq")"
  note "  build id ${build_id}"
  note
fi

# ------------------------------------------------------------------------------------------------
# Rung 4: the PBM encoder with a visible change
# ------------------------------------------------------------------------------------------------
if wants 4; then
  note "== rung 4: pic-newgame-stripe =="
  mod_dir="${project_dir}/mods/pic-newgame-stripe"
  reset_tree pic-newgame-stripe
  "${project_dir}/scripts/mod-seed.sh" "${mod_dir}" "pic.mpq:${MENU_MEMBER}" >/dev/null
  tree_file="${mod_dir}/archives/pic.mpq/lbm/newgame.lbm"
  cp "${tree_file}" "${work_dir}/newgame.pristine.lbm"
  rm -f "${work_dir}/newgame.png" "${work_dir}/newgame-stripe.png" \
    "${work_dir}/newgame.stripe.lbm"
  export_pbm "${game_dir}/pic.mpq" "${MENU_MEMBER}" "${work_dir}/newgame.png" pic.mpq
  PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/png_index_patch.py" \
    "${work_dir}/newgame.png" "${work_dir}/newgame-stripe.png" \
    --fill "${STRIPE_INDEX}" --rect "${STRIPE[@]}"
  "${viewer_tool}" --import-png-pbm "${work_dir}/newgame-stripe.png" \
    "${work_dir}/newgame.pristine.lbm" "${work_dir}/newgame.stripe.lbm"
  cp "${work_dir}/newgame.stripe.lbm" "${tree_file}"

  note "  member size $(wc -c < "${work_dir}/newgame.stripe.lbm" | tr -d ' ')" \
    "bytes; shipped $(wc -c < "${work_dir}/newgame.pristine.lbm" | tr -d ' ') bytes"
  check "rung 4: the image is structurally valid" \
    validate_image "${work_dir}/newgame.stripe.lbm"

  build_mod pic-newgame-stripe
  note "  build ${build_dir}"
  export_pbm "${build_dir}/pic.mpq" "${MENU_MEMBER}" "$(fresh_output "${out_dir}/rung4-menu-expected.png")" pic.mpq
  check "rung 4: the image read back out of the packed archive is the image we drew" \
    png_compare "${out_dir}/rung4-menu-expected.png" "${work_dir}/newgame-stripe.png"
  note "  expected value: ${out_dir}/rung4-menu-expected.png"
  note "  archive sha256 $(file_hash "${build_dir}/pic.mpq")"
  note "  build id ${build_id}"
  note
fi

# ------------------------------------------------------------------------------------------------
# Rung 5: an added member
# ------------------------------------------------------------------------------------------------
if wants 5; then
  note "== rung 5: imp-added-member =="
  mod_dir="${project_dir}/mods/imp-added-member"
  reset_tree imp-added-member
  "${project_dir}/scripts/mod-seed.sh" "${mod_dir}" "imp.mpq:${CURSOR_MEMBER}" >/dev/null
  tree_file="${mod_dir}/archives/imp.mpq/iface/cursors.imp"
  repaint_cursor "${tree_file}"
  # The added member is a copy of the SHIPPED sprite, not of the repainted one, so that a look at
  # the archive can tell the two apart by digest alone.
  cp "${work_dir}/cursors.pristine.imp" "${mod_dir}/archives/imp.mpq/iface/ladder.imp"

  build_mod imp-added-member
  note "  build ${build_dir}"
  printf '%s\n' "${ADDED_MEMBER}" > "${work_dir}/added-name.txt"
  "${mpq_tool}" probe-names "${build_dir}/imp.mpq" "${work_dir}/added-name.txt" \
    > "${work_dir}/probe.tsv"
  sed 's/^/  /' "${work_dir}/probe.tsv" | tee -a "${report}"
  check "rung 5: the added member resolves BY NAME through the archive's own hash table" \
    grep -q "present" "${work_dir}/probe.tsv"
  cat "$(listfile_for imp.mpq)" "${work_dir}/added-name.txt" > "${work_dir}/imp-plus-added.txt"
  listfile_override="${work_dir}/imp-plus-added.txt"
  extract_member "${build_dir}/imp.mpq" "${ADDED_MEMBER}" "${work_dir}/ladder-readback.imp" imp.mpq
  listfile_override=""
  check "rung 5: the added member reads back as the bytes it was added from" \
    files_identical "${work_dir}/ladder-readback.imp" "${work_dir}/cursors.pristine.imp"
  export_imp_frame "${build_dir}/imp.mpq" "${CURSOR_MEMBER}" "${CURSOR_FRAME}" \
    "$(fresh_output "${out_dir}/rung5-pointer-expected.png")" imp.mpq
  check "rung 5: the visible signal is still rung 2's repainted pointer" \
    png_compare "${out_dir}/rung5-pointer-expected.png" "${work_dir}/pointer-white.png"
  check "rung 5: every IMP in the packed archive still parses and validates" \
    "${viewer_tool}" --validate-imp "${build_dir}/imp.mpq" \
    --listfile "$(listfile_for imp.mpq)"
  note "  expected value: ${out_dir}/rung5-pointer-expected.png"
  note "  archive sha256 $(file_hash "${build_dir}/imp.mpq")"
  note "  build id ${build_id}"
  note
fi

# ------------------------------------------------------------------------------------------------
# Rung 6: the STORED storage class, and the WAVE encoder as a no-op
# ------------------------------------------------------------------------------------------------

# A gate, not a warning. **Observed 2026-09-18** by review of `spikes/asset-viewer/src/wave.rs`:
# `rebuild()` -- which is the path `--import-wave` writes through -- emits 0 for every odd-chunk pad
# byte instead of the value `parse` preserved, so re-importing a file's own audio can change a byte
# and exit 0 about it.
#
# It cannot reach the shipped corpus: every pad byte in `sndfx.mpq` and `special.mpq` is already
# 0x00, measured over all 2,048 of them. That is a fact about this corpus and not about the writer,
# so it is not a reason to skip the check -- it is the reason the check has to be made on a fixture
# rather than on a member. A no-op control that "passed" because the corpus happened to agree with
# a bug would be a control that proved nothing.
wave_pad_preserved() {
  local fixture="${work_dir}/pad-fixture.wav" rebuilt="${work_dir}/pad-rebuilt.wav"
  rm -f "${fixture}" "${rebuilt}"
  PYTHONDONTWRITEBYTECODE=1 python3 -c '
import struct, sys
# 8-bit mono PCM, one odd-length data chunk, then an odd-length LIST whose pad byte is 0x20.
fmt = struct.pack("<HHIIHH", 1, 1, 11025, 11025, 1, 8)
data = bytes(range(9))                      # odd
info = b"INFOISFT" + struct.pack("<I", 7) + b"fixture"   # odd body
chunks = (
    b"fmt " + struct.pack("<I", len(fmt)) + fmt
    + b"data" + struct.pack("<I", len(data)) + data + b"\x00"
    + b"LIST" + struct.pack("<I", len(info)) + info + b"\x20"
)
body = b"WAVE" + chunks
open(sys.argv[1], "wb").write(b"RIFF" + struct.pack("<I", len(body)) + body)
' "${fixture}"
  "${viewer_tool}" --import-wave "${fixture}" "${fixture}" "${rebuilt}" >/dev/null || return 1
  cmp -s "${fixture}" "${rebuilt}"
}

if wants 6 || wants 7 || wants 8 || wants 9; then
  note "== audio rungs: is the writer fit to build them? =="
  if wave_pad_preserved; then
    note "  PASS  --import-wave preserves an odd-chunk pad byte; the audio rungs are buildable"
    audio_writer_ready=1
  else
    note "  FAIL  --import-wave does NOT preserve an odd-chunk pad byte (wave.rs rebuild())."
    note "        Rungs 6, 7, 8 and 9 are NOT built. Rebuild them from scratch once the fix lands;"
    note "        do not patch the artifacts a previous run left behind."
    audio_writer_ready=0
    failures=$(( failures + 1 ))
  fi
  note
fi

seed_audio_members() {
  # seed_audio_members MOD_DIR -- seeds both members from both archives and keeps the pristine bytes
  local mod_dir="$1" archive member leaf
  for archive in "${AUDIO_ARCHIVES[@]}"; do
    for member in "${AUDIO_MEMBERS[@]}"; do
      leaf="${member##*\\}"
      "${project_dir}/scripts/mod-seed.sh" "${mod_dir}" "${archive}:${member}" >/dev/null
      cp "${mod_dir}/archives/${archive}/wav/${leaf}" \
        "${work_dir}/${leaf}.${archive}.pristine.wav"
    done
  done
}

if wants 6 && (( audio_writer_ready )); then
  note "== rung 6: audio-welcome-noop =="
  mod_dir="${project_dir}/mods/audio-welcome-noop"
  reset_tree audio-welcome-noop
  seed_audio_members "${mod_dir}"
  for archive in "${AUDIO_ARCHIVES[@]}"; do
    for member in "${AUDIO_MEMBERS[@]}"; do
      leaf="${member##*\\}"
      tree_file="${mod_dir}/archives/${archive}/wav/${leaf}"
      rm -f "${work_dir}/${leaf}.${archive}.export.wav" "${work_dir}/${leaf}.${archive}.enc.wav"
      export_wave "${game_dir}/${archive}" "${member}" \
        "${work_dir}/${leaf}.${archive}.export.wav" "${archive}"
      "${viewer_tool}" --import-wave "${work_dir}/${leaf}.${archive}.export.wav" \
        "${work_dir}/${leaf}.${archive}.pristine.wav" "${work_dir}/${leaf}.${archive}.enc.wav" \
        >/dev/null
      cp "${work_dir}/${leaf}.${archive}.enc.wav" "${tree_file}"
      # Against the SEED's bytes, not against a digest recorded anywhere earlier.
      check "rung 6: the WAVE encoder reproduced ${archive}:${member} byte for byte" \
        files_identical "${work_dir}/${leaf}.${archive}.enc.wav" \
        "${work_dir}/${leaf}.${archive}.pristine.wav"
    done
  done
  # Both archives hold both members byte-identically. That is what forces both to be rewritten, so
  # it is asserted rather than left as a remark in the mod.toml.
  for member in "${AUDIO_MEMBERS[@]}"; do
    leaf="${member##*\\}"
    check "rung 6: both audio archives hold ${member} identically" \
      files_identical "${work_dir}/${leaf}.sndfx.mpq.pristine.wav" \
      "${work_dir}/${leaf}.special.mpq.pristine.wav"
  done

  build_mod audio-welcome-noop
  note "  build ${build_dir}"
  for archive in "${AUDIO_ARCHIVES[@]}"; do
    for member in "${AUDIO_MEMBERS[@]}"; do
      leaf="${member##*\\}"
      rm -f "${work_dir}/${leaf}.${archive}.packed.wav"
      export_wave "${build_dir}/${archive}" "${member}" \
        "${work_dir}/${leaf}.${archive}.packed.wav" "${archive}"
      check "rung 6: ${archive}:${member} read back out of the packed archive is the shipped member" \
        files_identical "${work_dir}/${leaf}.${archive}.packed.wav" \
        "${work_dir}/${leaf}.${archive}.pristine.wav"
    done
    note "  ${archive} sha256 $(file_hash "${build_dir}/${archive}")"
  done
  cp "${work_dir}/welcome.wav.sndfx.mpq.pristine.wav" \
    "$(fresh_output "${out_dir}/rung6-welcome-expected.wav")"
  cp "${work_dir}/button.wav.sndfx.mpq.pristine.wav" \
    "$(fresh_output "${out_dir}/rung6-button-expected.wav")"
  note "  expected values: ${out_dir}/rung6-welcome-expected.wav (the SHIPPED sound)"
  note "                   ${out_dir}/rung6-button-expected.wav (the SHIPPED click)"
  note "  build id ${build_id}"
  note
fi

# ------------------------------------------------------------------------------------------------
# Rungs 7 and 8: an audible replacement of identical length, different in each archive -- and,
# for rung 8, the same two frequencies with the archives EXCHANGED.
# ------------------------------------------------------------------------------------------------

# build_tone_rung RUNG_NUMBER MOD_ID SNDFX_HZ SPECIAL_HZ -- the body shared by rungs 7 and 8. Every
# check is identical between the two rungs except for the rung number in its label and the two
# frequencies, both of which are now arguments rather than the module-level SNDFX_HERTZ/
# SPECIAL_HERTZ pair, so a caller cannot forget which rung it is building. Work files are namespaced
# by rung number (`rung${rung}.*`) so that rungs 7 and 8 can both run in the same invocation without
# one overwriting the other's intermediate bytes -- which rung 8's swap-verification check below
# depends on being able to read back.
build_tone_rung() {
  local rung="$1" mod_id="$2" sndfx_hz="$3" special_hz="$4"
  local mod_dir archive hertz member leaf tree_file
  note "== rung ${rung}: ${mod_id} =="
  mod_dir="${project_dir}/mods/${mod_id}"
  reset_tree "${mod_id}"
  seed_audio_members "${mod_dir}"
  for archive in "${AUDIO_ARCHIVES[@]}"; do
    hertz="$(hertz_for "${archive}" "${sndfx_hz}" "${special_hz}")"
    for member in "${AUDIO_MEMBERS[@]}"; do
      leaf="${member##*\\}"
      tree_file="${mod_dir}/archives/${archive}/wav/${leaf}"
      rm -f "${work_dir}/rung${rung}.tone.${leaf}.${archive}.wav" \
        "${work_dir}/rung${rung}.${leaf}.${archive}.tone.wav"
      PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/wav_tone.py" \
        "${work_dir}/${leaf}.${archive}.pristine.wav" \
        "${work_dir}/rung${rung}.tone.${leaf}.${archive}.wav" \
        --hertz "${hertz}" --tone-ms "${TONE_MS}"
      "${viewer_tool}" --import-wave "${work_dir}/rung${rung}.tone.${leaf}.${archive}.wav" \
        "${work_dir}/${leaf}.${archive}.pristine.wav" \
        "${work_dir}/rung${rung}.${leaf}.${archive}.tone.wav" \
        >/dev/null
      cp "${work_dir}/rung${rung}.${leaf}.${archive}.tone.wav" "${tree_file}"
      check "rung ${rung}: ${archive}:${member} keeps the shipped member's byte count" \
        test "$(wc -c < "${work_dir}/rung${rung}.${leaf}.${archive}.tone.wav")" \
        -eq "$(wc -c < "${work_dir}/${leaf}.${archive}.pristine.wav")"
      check "rung ${rung}: ${archive}:${member} really differs from the shipped member" \
        files_differ "${work_dir}/rung${rung}.${leaf}.${archive}.tone.wav" \
        "${work_dir}/${leaf}.${archive}.pristine.wav"
    done
  done
  # The whole point of two frequencies is that the two archives now differ. If they did not, the
  # listen could not name which archive the engine opened.
  for member in "${AUDIO_MEMBERS[@]}"; do
    leaf="${member##*\\}"
    check "rung ${rung}: the two archives' replacements of ${member} are different sounds" \
      files_differ "${work_dir}/rung${rung}.${leaf}.sndfx.mpq.tone.wav" \
      "${work_dir}/rung${rung}.${leaf}.special.mpq.tone.wav"
  done

  build_mod "${mod_id}"
  note "  build ${build_dir}"
  for archive in "${AUDIO_ARCHIVES[@]}"; do
    for member in "${AUDIO_MEMBERS[@]}"; do
      leaf="${member##*\\}"
      rm -f "${work_dir}/rung${rung}.${leaf}.${archive}.packed.wav"
      export_wave "${build_dir}/${archive}" "${member}" \
        "${work_dir}/rung${rung}.${leaf}.${archive}.packed.wav" "${archive}"
      check "rung ${rung}: ${archive}:${member} read back out of the packed archive is the tone we wrote" \
        wave_samples_equal "${work_dir}/rung${rung}.${leaf}.${archive}.packed.wav" \
        "${work_dir}/rung${rung}.tone.${leaf}.${archive}.wav"
      cp "${work_dir}/rung${rung}.${leaf}.${archive}.packed.wav" \
        "$(fresh_output "${out_dir}/rung${rung}-${leaf%.wav}-expected-${archive%.mpq}.wav")"
    done
    note "  ${archive} sha256 $(file_hash "${build_dir}/${archive}")"
  done
  note "  expected values: ${out_dir}/rung${rung}-welcome-expected-sndfx.wav ($(hertz_for sndfx.mpq "${sndfx_hz}" "${special_hz}") Hz)"
  note "                   ${out_dir}/rung${rung}-welcome-expected-special.wav ($(hertz_for special.mpq "${sndfx_hz}" "${special_hz}") Hz)"
  note "                   ${out_dir}/rung${rung}-button-expected-sndfx.wav, -special.wav"
  note "  build id ${build_id}"
  note
}

# reconstruct_tone ARCHIVE HERTZ LEAF OUTPUT [RAW_OUTPUT] -- regenerates, from the ARCHIVE's own
# pristine bytes (already seeded into work_dir by whichever rung most recently ran
# seed_audio_members), the exact tone.wav that a rung assigning HERTZ to ARCHIVE would have
# produced. It exists so that rung 8's swap check does not depend on rung 7 having actually run in
# this invocation: rung 8 seeds both archives' pristines itself, so it can reconstruct "what rung 7
# would have written" locally instead of reading a file rung 7 may never have left behind.
#
# OUTPUT is the file after `--import-wave` (what gets copied into the mod tree). RAW_OUTPUT, if
# given, also receives a copy of the file BEFORE `--import-wave` -- the independently generated
# tone a packed readback must be checked against, not the pipeline's own intermediate.
reconstruct_tone() {
  local archive="$1" hertz="$2" leaf="$3" output="$4" raw_output="${5:-}"
  local raw="${work_dir}/reconstruct.${leaf}.${archive}.${hertz}hz.wav"
  rm -f "${raw}" "${output}"
  PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/wav_tone.py" \
    "${work_dir}/${leaf}.${archive}.pristine.wav" "${raw}" \
    --hertz "${hertz}" --tone-ms "${TONE_MS}"
  "${viewer_tool}" --import-wave "${raw}" "${work_dir}/${leaf}.${archive}.pristine.wav" \
    "${output}" >/dev/null
  if [[ -n "${raw_output}" ]]; then
    cp "${raw}" "${raw_output}"
  fi
}

if wants 7 && (( audio_writer_ready )); then
  build_tone_rung 7 audio-welcome-tone "${SNDFX_HERTZ}" "${SPECIAL_HERTZ}"
fi

if wants 8 && (( audio_writer_ready )); then
  build_tone_rung 8 audio-tone-swapped "${SPECIAL_HERTZ}" "${SNDFX_HERTZ}"

  # The check rung 7 does not have: the swap must be a genuine exchange, not a second arbitrary
  # build. Rung 8's sndfx.mpq now carries the frequency rung 7 gave to special.mpq (1760 Hz) and
  # vice versa, so the SAME tone must land in the OPPOSITE archive. Reconstructed locally (see
  # reconstruct_tone above) rather than by reading rung 7's work files, so this holds even when
  # `--rung 8` is run alone and rung 7 never built anything in this invocation.
  for member in "${AUDIO_MEMBERS[@]}"; do
    leaf="${member##*\\}"
    reconstruct_tone special.mpq "${SPECIAL_HERTZ}" "${leaf}" \
      "${work_dir}/reconstructed.${leaf}.rung7-special-equivalent.wav"
    check "rung 8: sndfx.mpq's ${member} tone is byte-identical to rung 7's special.mpq tone (same ${SPECIAL_HERTZ} Hz, opposite archive)" \
      files_identical "${work_dir}/rung8.${leaf}.sndfx.mpq.tone.wav" \
      "${work_dir}/reconstructed.${leaf}.rung7-special-equivalent.wav"

    reconstruct_tone sndfx.mpq "${SNDFX_HERTZ}" "${leaf}" \
      "${work_dir}/reconstructed.${leaf}.rung7-sndfx-equivalent.wav"
    check "rung 8: special.mpq's ${member} tone is byte-identical to rung 7's sndfx.mpq tone (same ${SNDFX_HERTZ} Hz, opposite archive)" \
      files_identical "${work_dir}/rung8.${leaf}.special.mpq.tone.wav" \
      "${work_dir}/reconstructed.${leaf}.rung7-sndfx-equivalent.wav"
  done
  note
fi

# ------------------------------------------------------------------------------------------------
# Rung 9: a presence judgement instead of a pitch judgement
# ------------------------------------------------------------------------------------------------
# Rung 8 flipped `welcome.wav` exactly as predicted (low on rung 7, high on rung 8), so
# `wav\welcome.wav` is served from `sndfx.mpq` -- measured, not inferred. `button.wav` did not read:
# at 220 Hz its 41 ms is nine cycles, which the ear receives as a click with no perceptible pitch,
# and rung 7's tone played moments after 5.67 s of the OTHER frequency, so "higher" may have been
# relative to what had just stopped. Rung 7's split conclusion is withdrawn for `button.wav`.
#
# Rung 9 replaces the pitch judgement with a presence judgement. `button.wav` in `sndfx.mpq` becomes
# digital silence -- 0x80 for every 8-bit sample, the zero level, not zero bytes -- while
# `special.mpq` keeps rung 7's 1760 Hz tone there unchanged; this rung only moves `sndfx.mpq`.
# `special.mpq` is tied to rung 7 rather than rung 8 deliberately: rung 8's special.mpq value is
# 220 Hz, and this rung's whole observation is presence versus absence on a 41 ms member, where
# 220 Hz is the same nine-cycle shortness that made rung 8 unreadable -- now carrying the entire
# result, and a hit too faint to register would read as silence with no symptom of the mistake.
# 1760 Hz gives ~75 cycles in the same 41 ms and is a sound the engine has already demonstrably
# played, so the audible side is made as hard to miss as 41 ms allows; the silent side stays the
# ambiguous reading (a listener who hears nothing is certifying an absence, always the weaker
# report), so the only side worth strengthening is the one that already carries an unambiguous
# signal.
# `welcome.wav` gets the SAME tone, 440 Hz, in BOTH archives: distinct from 220 and 1760 so it
# cannot be confused with a previous sitting, and identical between archives so it fires
# unconditionally as the liveness control -- a silent button click is then attributable to which
# archive the engine read, not to the build having failed. Click a main-menu button: sound means
# `special.mpq`, silence means `sndfx.mpq` and both members share an archive. Neither answer needs a
# pitch judgement.
#
# This does not reuse build_tone_rung: that helper's shape is one hertz per archive, applied
# identically to every member, and rung 9 needs three different treatments in one mod (one archive's
# button silenced, the other archive's button copied unchanged from rung 8, and welcome.wav alone
# given the same new tone everywhere). Forcing that through build_tone_rung would have meant
# weakening its per-member symmetry or its own checks to make room; duplicating the shared steps
# (seeding, the byte-count and differs checks, the packed readback) here instead keeps rung 7 and 8
# exactly as they were.
if wants 9 && (( audio_writer_ready )); then
  note "== rung 9: audio-button-silence =="
  mod_id=audio-button-silence
  mod_dir="${project_dir}/mods/${mod_id}"
  reset_tree "${mod_id}"
  seed_audio_members "${mod_dir}"

  # welcome.wav: the SAME 440 Hz tone in both archives.
  leaf=welcome.wav
  for archive in "${AUDIO_ARCHIVES[@]}"; do
    tree_file="${mod_dir}/archives/${archive}/wav/${leaf}"
    rm -f "${work_dir}/rung9.tone.${leaf}.${archive}.wav" "${work_dir}/rung9.${leaf}.${archive}.tone.wav"
    PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/wav_tone.py" \
      "${work_dir}/${leaf}.${archive}.pristine.wav" "${work_dir}/rung9.tone.${leaf}.${archive}.wav" \
      --hertz "${WELCOME_HERTZ}" --tone-ms "${TONE_MS}"
    "${viewer_tool}" --import-wave "${work_dir}/rung9.tone.${leaf}.${archive}.wav" \
      "${work_dir}/${leaf}.${archive}.pristine.wav" "${work_dir}/rung9.${leaf}.${archive}.tone.wav" \
      >/dev/null
    cp "${work_dir}/rung9.${leaf}.${archive}.tone.wav" "${tree_file}"
    check "rung 9: ${archive}:wav\\${leaf} keeps the shipped member's byte count" \
      test "$(wc -c < "${work_dir}/rung9.${leaf}.${archive}.tone.wav")" \
      -eq "$(wc -c < "${work_dir}/${leaf}.${archive}.pristine.wav")"
    check "rung 9: ${archive}:wav\\${leaf} really differs from the shipped member" \
      files_differ "${work_dir}/rung9.${leaf}.${archive}.tone.wav" \
      "${work_dir}/${leaf}.${archive}.pristine.wav"
  done
  # Check 4: the liveness control shares one variable across archives. If the two archives'
  # welcome.wav differed, the rung would have two things changing and a silent click would not
  # name which one moved.
  check "rung 9: the two archives' wav\\welcome.wav are byte-identical (the liveness control shares one variable)" \
    files_identical "${work_dir}/rung9.${leaf}.sndfx.mpq.tone.wav" \
    "${work_dir}/rung9.${leaf}.special.mpq.tone.wav"

  # button.wav: sndfx.mpq -> digital silence. --tone-ms 0 leaves every frame inside wav_tone.py's
  # own "no tone" branch, which for 8-bit unsigned PCM writes the format's silence level, 0x80, to
  # every sample -- exactly the zero level this rung wants, not a shortened or zeroed-bytes member.
  # The frequency argument is required but unused when there are zero tone frames.
  leaf=button.wav
  tree_file="${mod_dir}/archives/sndfx.mpq/wav/${leaf}"
  rm -f "${work_dir}/rung9.tone.${leaf}.sndfx.mpq.wav" "${work_dir}/rung9.${leaf}.sndfx.mpq.tone.wav"
  PYTHONDONTWRITEBYTECODE=1 python3 "${project_dir}/tools/wav_tone.py" \
    "${work_dir}/${leaf}.sndfx.mpq.pristine.wav" "${work_dir}/rung9.tone.${leaf}.sndfx.mpq.wav" \
    --hertz 0 --tone-ms 0
  "${viewer_tool}" --import-wave "${work_dir}/rung9.tone.${leaf}.sndfx.mpq.wav" \
    "${work_dir}/${leaf}.sndfx.mpq.pristine.wav" "${work_dir}/rung9.${leaf}.sndfx.mpq.tone.wav" \
    >/dev/null
  cp "${work_dir}/rung9.${leaf}.sndfx.mpq.tone.wav" "${tree_file}"
  check "rung 9: sndfx.mpq:wav\\${leaf} keeps the shipped member's byte count" \
    test "$(wc -c < "${work_dir}/rung9.${leaf}.sndfx.mpq.tone.wav")" \
    -eq "$(wc -c < "${work_dir}/${leaf}.sndfx.mpq.pristine.wav")"
  check "rung 9: sndfx.mpq:wav\\${leaf} really differs from the shipped member" \
    files_differ "${work_dir}/rung9.${leaf}.sndfx.mpq.tone.wav" \
    "${work_dir}/${leaf}.sndfx.mpq.pristine.wav"

  # special.mpq: rung 7's own special.mpq button tone (1760 Hz), unchanged -- reconstructed from
  # this rung's own seeded pristine (see reconstruct_tone), so it holds even when `--rung 9` runs
  # without rung 7 in the same invocation. Tied to rung 7 rather than rung 8 deliberately: rung 9's
  # entire observation is presence versus absence on a 41 ms member, and at 220 Hz (rung 8's
  # special.mpq value) that is nine cycles -- the same shortness that made rung 8 unreadable, now
  # carrying the whole result. A special.mpq hit too faint to register reads as silence and the
  # rung concludes sndfx.mpq with no symptom of the mistake. 1760 Hz gives ~75 cycles in the same
  # 41 ms and is a sound the engine has already demonstrably played (rung 7's listen), so the
  # audible side is made as hard to miss as 41 ms allows.
  reconstruct_tone special.mpq "${SPECIAL_HERTZ}" "${leaf}" \
    "${work_dir}/rung9.${leaf}.special.mpq.tone.wav" \
    "${work_dir}/rung9.tone.${leaf}.special.mpq.wav"
  cp "${work_dir}/rung9.${leaf}.special.mpq.tone.wav" "${mod_dir}/archives/special.mpq/wav/${leaf}"
  check "rung 9: special.mpq:wav\\${leaf} keeps the shipped member's byte count" \
    test "$(wc -c < "${work_dir}/rung9.${leaf}.special.mpq.tone.wav")" \
    -eq "$(wc -c < "${work_dir}/${leaf}.special.mpq.pristine.wav")"
  check "rung 9: special.mpq:wav\\${leaf} really differs from the shipped member" \
    files_differ "${work_dir}/rung9.${leaf}.special.mpq.tone.wav" \
    "${work_dir}/${leaf}.special.mpq.pristine.wav"

  # Check 5: the rung's entire discriminating power is that the two archives' button.wav differ.
  check "rung 9: the two archives' wav\\${leaf} differ from each other (the rung's entire discriminating power)" \
    files_differ "${work_dir}/rung9.${leaf}.sndfx.mpq.tone.wav" \
    "${work_dir}/rung9.${leaf}.special.mpq.tone.wav"

  build_mod "${mod_id}"
  note "  build ${build_dir}"
  for archive in "${AUDIO_ARCHIVES[@]}"; do
    for member in "${AUDIO_MEMBERS[@]}"; do
      leaf="${member##*\\}"
      rm -f "${work_dir}/rung9.${leaf}.${archive}.packed.wav"
      export_wave "${build_dir}/${archive}" "${member}" \
        "${work_dir}/rung9.${leaf}.${archive}.packed.wav" "${archive}"
      check "rung 9: ${archive}:${member} read back out of the packed archive is the member we wrote" \
        wave_samples_equal "${work_dir}/rung9.${leaf}.${archive}.packed.wav" \
        "${work_dir}/rung9.tone.${leaf}.${archive}.wav"
      cp "${work_dir}/rung9.${leaf}.${archive}.packed.wav" \
        "$(fresh_output "${out_dir}/rung9-${leaf%.wav}-expected-${archive%.mpq}.wav")"
    done
    note "  ${archive} sha256 $(file_hash "${build_dir}/${archive}")"
  done

  # Check 1: the DATA CHUNK sndfx.mpq now serves for button.wav is uniformly the zero level, read
  # back out of the PACKED archive -- not the work file that was staged into the tree.
  check "rung 9: sndfx.mpq's wav\\button.wav data chunk read back out of the packed archive is uniformly the zero level (0x80)" \
    wav_data_is_uniform_byte "${work_dir}/rung9.button.wav.sndfx.mpq.packed.wav" 0x80

  # Check 2: silence kept the shipped byte length (already checked above) and every ancillary chunk
  # verbatim; only the data payload was allowed to change. Checked against the PACKED readback, not
  # the work file, for the same reason as check 1.
  check "rung 9: sndfx.mpq's wav\\button.wav keeps every ancillary chunk byte-for-byte against the shipped member; only the data payload changed" \
    wav_ancillary_identical "${work_dir}/button.wav.sndfx.mpq.pristine.wav" \
    "${work_dir}/rung9.button.wav.sndfx.mpq.packed.wav"

  # Check 3: special.mpq's button.wav, read back out of the packed archive, is rung 7's special.mpq
  # button tone.
  check "rung 9: special.mpq's wav\\button.wav read back out of the packed archive is byte-identical to rung 7's special.mpq button tone" \
    files_identical "${work_dir}/rung9.button.wav.special.mpq.packed.wav" \
    "${work_dir}/rung9.button.wav.special.mpq.tone.wav"

  note "  expected values: ${out_dir}/rung9-welcome-expected-sndfx.wav, -special.wav (${WELCOME_HERTZ} Hz, both archives)"
  note "                   ${out_dir}/rung9-button-expected-sndfx.wav (digital silence)"
  note "                   ${out_dir}/rung9-button-expected-special.wav (rung 7's special.mpq tone, unchanged, ${SPECIAL_HERTZ} Hz)"
  note "  build id ${build_id}"
  note
fi

# ------------------------------------------------------------------------------------------------
# Postamble
# ------------------------------------------------------------------------------------------------
profile_state "${work_dir}/profiles.after"
note "-- installed profiles, after --"
if files_identical "${work_dir}/profiles.before" "${work_dir}/profiles.after"; then
  note "  PASS  every installed profile's archives are byte-for-byte what they were"
else
  note "  FAIL  an installed profile changed during this run:"
  diff "${work_dir}/profiles.before" "${work_dir}/profiles.after" | sed 's/^/        /' \
    | tee -a "${report}"
  failures=$(( failures + 1 ))
fi
note

note "== result =="
note "  ${failures} failed check(s)"
note "  report ${report}"
note "  Nothing was installed and nothing was launched."
note "  The attended half is docs/engine-acceptance-ladder.md."
exit $(( failures > 0 ))
