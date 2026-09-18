#!/usr/bin/env bash
# Repack one MPQ archive with replaced members, and refuse the result unless it
# still has the source archive's shape.
#
# The order here is the point: the archive is built somewhere disposable, its
# shape is checked against the source, and only a passing archive is even a
# candidate for installation. Installing is a separate, unimplemented step --
# see --install below. Following scripts/restore-game-archives.sh, this always
# ends by printing the hashes it produced.
#
# Usage:
#   scripts/repack-archive.sh SOURCE.mpq OUTPUT.mpq 'ARCHIVE\NAME=local/file' ...
#
# Options:
#   --listfile NAMES.txt   supply member names the source archive does not carry
#                          itself. Required for pic.mpq, imp.mpq, sndfx.mpq and
#                          special.mpq, none of which has a (listfile): without
#                          it every member lists under a File%08u.xxx
#                          pseudo-name, which cannot be written to. The same
#                          names are used for BOTH manifests, because a shape
#                          check that named one side and not the other would be
#                          comparing two different addressings of one archive.
#   --determinism-runs N   repack N extra times into throwaway paths and report
#                          whether every run produced the same bytes
#   --install PROFILE      refuses; archive installation is Phase 3
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  sed -n '2,25p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
}

determinism_runs=0
determinism_failed=0
listfile=""
positional=()
while (( $# )); do
  case "$1" in
    --listfile)
      [[ $# -ge 2 ]] || { echo "--listfile needs a path" >&2; exit 2; }
      listfile="$2"
      shift 2
      ;;
    --determinism-runs)
      [[ $# -ge 2 ]] || { echo "--determinism-runs needs a count" >&2; exit 2; }
      determinism_runs="$2"
      shift 2
      ;;
    --install)
      echo "refusing: installing a repacked archive into a game profile is Phase 3 work." >&2
      echo "This command builds and verifies an archive; it never writes to a profile." >&2
      exit 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      positional+=("$1")
      shift
      ;;
  esac
done

if (( ${#positional[@]} < 3 )); then
  usage
  exit 2
fi

source_archive="${positional[0]}"
output_archive="${positional[1]}"
replacements=("${positional[@]:2}")

if [[ ! -f "${source_archive}" ]]; then
  echo "source archive not found: ${source_archive}" >&2
  exit 1
fi
if [[ -e "${output_archive}" ]]; then
  echo "output archive already exists; choose a fresh path: ${output_archive}" >&2
  exit 1
fi

# The installed profiles under ~/Applications are the recovery baseline. Nothing
# in this pipeline may write there, so the check is on the path, before any work.
output_parent="$(cd "$(dirname "${output_archive}")" 2>/dev/null && pwd || true)"
if [[ -z "${output_parent}" ]]; then
  echo "output directory does not exist: $(dirname "${output_archive}")" >&2
  exit 1
fi
case "${output_parent}/" in
  "${HOME}/Applications/"*)
    echo "refusing to write inside ${HOME}/Applications: ${output_archive}" >&2
    exit 1
    ;;
esac

"${project_dir}/scripts/build-tools.sh" >/dev/null
mpq_tool="${project_dir}/.build/lom-mpq"

file_hash() {
  shasum -a 256 "$1" | cut -d' ' -f1
}

listfile_arguments=()
if [[ -n "${listfile}" ]]; then
  [[ -f "${listfile}" ]] || { echo "listfile not found: ${listfile}" >&2; exit 1; }
  listfile_arguments=(--listfile "${listfile}")
fi

repack_arguments=()
expectation_arguments=()
for replacement in "${replacements[@]}"; do
  repack_arguments+=(--replace "${replacement}")
  # The archive name is everything before the FIRST `=`, matching lom-mpq.
  expectation_arguments+=(--expect-changed "${replacement%%=*}")
done

source_manifest="${output_archive}.source-manifest.tsv"
output_manifest="${output_archive}.manifest.tsv"

echo "== source =="
echo "  ${source_archive}"
echo "  sha256 $(file_hash "${source_archive}")"
"${mpq_tool}" manifest "${source_archive}" "${listfile_arguments[@]}" \
  > "${source_manifest}"

echo "== repacking =="
"${mpq_tool}" repack "${source_archive}" "${output_archive}" \
  "${listfile_arguments[@]}" "${repack_arguments[@]}"
"${mpq_tool}" manifest "${output_archive}" "${listfile_arguments[@]}" \
  > "${output_manifest}"

echo "== shape check =="
if ! python3 "${project_dir}/tools/mpq_shape.py" \
  --source "${source_manifest}" \
  --output "${output_manifest}" \
  "${expectation_arguments[@]}"; then
  # A refused archive is deleted rather than left on disk, so no later step can
  # pick up an archive that failed its own check.
  rm -f "${output_archive}"
  echo "shape check failed; the repacked archive has been deleted." >&2
  echo "Nothing was written to any game profile." >&2
  exit 1
fi

if (( determinism_runs > 0 )); then
  echo "== determinism (${determinism_runs} extra run(s)) =="
  reference_hash="$(file_hash "${output_archive}")"
  echo "  run 1 ${reference_hash}"
  determinism_dir="$(mktemp -d)"
  trap 'rm -rf "${determinism_dir}"' EXIT
  identical=1
  for (( run = 2; run <= determinism_runs + 1; run++ )); do
    repeat_archive="${determinism_dir}/run-${run}.mpq"
    "${mpq_tool}" repack "${source_archive}" "${repeat_archive}" \
      "${listfile_arguments[@]}" "${repack_arguments[@]}" >/dev/null
    repeat_hash="$(file_hash "${repeat_archive}")"
    if [[ "${repeat_hash}" == "${reference_hash}" ]]; then
      echo "  run ${run} ${repeat_hash}"
    else
      echo "  run ${run} ${repeat_hash}  DIFFERS FROM RUN 1"
      identical=0
    fi
  done
  if (( identical )); then
    echo "  every run produced byte-identical output"
  else
    echo "  output is NOT byte-deterministic; see docs/repack.md" >&2
    determinism_failed=1
  fi
fi

echo "== result =="
echo "  ${output_archive}"
echo "  sha256 $(file_hash "${output_archive}")"
echo "  manifests: ${source_manifest} ${output_manifest}"
echo "  This archive has passed its shape check and has NOT been installed."

# A determinism check that was asked for and failed is a failure of the command,
# even though the archive itself passed its shape check.
exit "${determinism_failed}"
