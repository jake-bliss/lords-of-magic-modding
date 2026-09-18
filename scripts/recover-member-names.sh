#!/usr/bin/env bash
# Recover MPQ member names by pooling every catalogue we have and asking each
# target archive, directly, which of those names it will open.
#
# Nothing here writes to a game profile. Every archive is opened read-only.
#
#   scripts/recover-member-names.sh ~/Applications artifacts/names-20260918
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 APPLICATIONS_DIR OUTPUT_DIR" >&2
  exit 2
fi

applications_dir="$1"
output_dir="$2"

# Everything below opens archives read-only, and the output never goes near an
# installed profile.
case "${output_dir}" in
  "${HOME}"/Applications/*)
    echo "Refusing to write under ~/Applications: ${output_dir}" >&2
    exit 1
    ;;
esac

if [[ -e "${output_dir}" ]]; then
  echo "Refusing to reuse an existing output directory: ${output_dir}" >&2
  exit 1
fi

tool="${project_dir}/.build/lom-mpq"
"${project_dir}/scripts/build-tools.sh" >/dev/null

mkdir -p "${output_dir}/manifests" "${output_dir}/catalogues" "${output_dir}/probes" \
  "${output_dir}/resolutions" "${output_dir}/listfiles"

profiles=(
  "vanilla:Steambuild 32 64bit DXVK"
  "302:Lords of Magic 3.02"
  "gs5r3:Lords of Magic GS5R3"
)
archives=(gs pic imp sndfx special)

profile_dir() {
  echo "${applications_dir}/$1.app/Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English"
}

# 1. Manifest every archive, addressed by block index.
for profile in "${profiles[@]}"; do
  tag="${profile%%:*}"
  directory="$(profile_dir "${profile#*:}")"
  for archive in "${archives[@]}"; do
    "${tool}" manifest "${directory}/${archive}.mpq" \
      > "${output_dir}/manifests/${tag}-${archive}.tsv"
  done
done

# 2. Every archive's own catalogue becomes a candidate source for every other
#    archive. Names are archive-independent; block slots are not, so the
#    `File%08u.xxx` placeholders are dropped here and never travel.
source_arguments=()
for profile in "${profiles[@]}"; do
  tag="${profile%%:*}"
  for archive in "${archives[@]}"; do
    # Pseudo-names are dropped by tools/member_names.py, which owns the rule.
    # This only strips the header and the control characters that would break a
    # TSV row.
    catalogue="${output_dir}/catalogues/internal-${tag}-${archive}.txt"
    awk -F'\t' 'NR > 1 { print $1 }' \
      "${output_dir}/manifests/${tag}-${archive}.tsv" \
      | LC_ALL=C grep -v '[[:cntrl:]]' | sort -u > "${catalogue}"
    if [[ -s "${catalogue}" ]]; then
      source_arguments+=(--source "internal-${tag}-${archive}=${catalogue}")
    else
      rm -f "${catalogue}"
    fi
  done
done

# 3. The external catalogue, verified by pinned digest before it is trusted.
"${project_dir}/scripts/fetch-lom-listfile.sh" >/dev/null
# The published catalogue is CRLF. A stray carriage return inside a candidate
# name is not a harmless formatting detail: it splits the probe's TSV row when a
# reader applies universal newlines, and the name it probes is not the name it
# reports. Strip it here, at the one place that reads the file.
external="${output_dir}/catalogues/catalogue-zezula.txt"
tr -d '\r' < "${project_dir}/artifacts/reference-listfiles/lords-of-magic.txt" \
  | sort -u > "${external}"
source_arguments+=(--source "catalogue-zezula=${external}")

# 4. Pool the catalogues and derive the negative controls from the pool, both in
#    the tool that owns the pseudo-name rule.
pool="${output_dir}/candidate-names.txt"
controls="${output_dir}/negative-controls.txt"
python3 "${project_dir}/tools/member_names.py" \
  --emit-pool "${pool}" --emit-controls "${controls}" "${source_arguments[@]}"

probe_input="${output_dir}/probe-input.txt"
cat "${pool}" "${controls}" > "${probe_input}"

# 5. Ask each archive, directly. `probe-names` does not load the target's own
#    listfile, so a hit is a property of the archive's hash table.
for profile in "${profiles[@]}"; do
  tag="${profile%%:*}"
  directory="$(profile_dir "${profile#*:}")"
  for archive in "${archives[@]}"; do
    "${tool}" probe-names "${directory}/${archive}.mpq" "${probe_input}" \
      > "${output_dir}/probes/${tag}-${archive}.tsv"
    echo "=== ${tag} ${archive}.mpq"
    python3 "${project_dir}/tools/member_names.py" \
      --manifest "${output_dir}/manifests/${tag}-${archive}.tsv" \
      --probe "${output_dir}/probes/${tag}-${archive}.tsv" \
      --controls "${controls}" \
      --resolution "${output_dir}/resolutions/${tag}-${archive}.tsv" \
      --listfile "${output_dir}/listfiles/${tag}-${archive}.txt" \
      "${source_arguments[@]}"
  done
done

echo "Wrote ${output_dir}"
