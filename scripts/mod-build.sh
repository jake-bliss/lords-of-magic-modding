#!/usr/bin/env bash
# Build a mod: validate, repack, and record what changed. Installs nothing.
#
# Output lands in artifacts/build/<mod-id>/<build-id>/ and contains the repacked archives, a
# build.json, and a change report. The build id is a digest of the mod tree, the base archives and
# the tools -- not a timestamp -- so building the same inputs twice lands in the same directory
# with the same bytes.
#
# The packing is scripts/repack-archive.sh, which refuses to hand on an archive that lost the
# source archive's shape. Nothing here re-implements it.
#
# Usage:
#   scripts/mod-build.sh mods/<mod-id> [--determinism-runs N] [--force]
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

determinism_runs=0
force=0
positional=()
while (( $# )); do
  case "$1" in
    --determinism-runs) determinism_runs="${2:?--determinism-runs needs a count}"; shift 2 ;;
    --force) force=1; shift ;;
    --help|-h) sed -n '2,15p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) positional+=("$1"); shift ;;
  esac
done
(( ${#positional[@]} == 1 )) || die "usage: $0 mods/<mod-id> [--determinism-runs N] [--force]"
mod_dir="$(cd "${positional[0]}" && pwd)" || die "not a directory: ${positional[0]}"

prepare_tools

mod_id="$(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import sys, mod_tree; print(mod_tree.load(sys.argv[1]).manifest.id)' "${mod_dir}")"
base_profile="$(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import sys, mod_tree; print(mod_tree.load(sys.argv[1]).manifest.base_profile)' "${mod_dir}")"
game_dir="$(profile_game_dir "${base_profile}")"

# Validation is a gate, not a report. A build cannot be produced from a tree that failed it.
echo "== validate =="
"${project_dir}/scripts/mod-validate.sh" "${mod_dir}" \
  || die "validation failed; nothing was packed."
echo

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

manifest_arguments=()
digest_arguments=()
for archive in "${PIPELINE_ARCHIVES[@]}"; do
  source_archive="${game_dir}/${archive}"
  [[ -f "${source_archive}" ]] || die "base profile ${base_profile} has no ${archive}"
  "${mpq_tool}" manifest "${source_archive}" > "${work_dir}/${archive}.tsv"
  manifest_arguments+=(--base-manifest "${archive}=${work_dir}/${archive}.tsv")
  digest_arguments+=(--base-digest "${archive}=$(file_hash "${source_archive}")")
done

tool_arguments=(
  --tool-digest "lom-mpq=${mpq_tool_sha}"
  --tool-digest "lom-asset-viewer=${viewer_tool_sha}"
)

build_id="$(PYTHONPATH="${project_dir}/tools" python3 "${project_dir}/tools/mod_build.py" plan \
  "${mod_dir}" "${manifest_arguments[@]}" "${digest_arguments[@]}" "${tool_arguments[@]}" \
  --output "${work_dir}/plan.json")"

output_dir="${artifacts_dir}/build/${mod_id}/${build_id}"
if [[ -d "${output_dir}" ]]; then
  if (( force )); then
    # Deleted by an exact path this script computed, never by a glob, and only under artifacts/.
    echo "== replacing the existing build ${build_id} (--force) =="
    rm -rf "${output_dir}"
  else
    die "build ${build_id} already exists: ${output_dir}
The build id is a digest of the inputs, so an identical id means identical inputs.
Pass --force to rebuild it, or change the mod tree."
  fi
fi
mkdir -p "${output_dir}"

echo "== repack =="
output_digest_arguments=()
mapfile -t built_archives < <(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["archives"]))' \
  "${work_dir}/plan.json")

for archive in "${built_archives[@]}"; do
  replacements=()
  while IFS=$'\t' read -r member file; do
    replacements+=("${member}=${file}")
  done < <(PYTHONPATH="${project_dir}/tools" python3 -c '
import json, sys
plan = json.load(open(sys.argv[1]))
for entry in plan["archives"][sys.argv[2]]:
    print(entry["member"], entry["file"], sep="\t")
' "${work_dir}/plan.json" "${archive}")

  repack_options=()
  (( determinism_runs > 0 )) && repack_options+=(--determinism-runs "${determinism_runs}")
  "${project_dir}/scripts/repack-archive.sh" \
    "${game_dir}/${archive}" \
    "${output_dir}/${archive}" \
    "${repack_options[@]}" \
    "${replacements[@]}"
  output_digest_arguments+=(--output-digest "${archive}=$(file_hash "${output_dir}/${archive}")")
done
echo

# The base member's own bytes, for the change report's token comparison. Extracted to a temporary
# directory; the build output holds archives and reports, never loose game content.
base_file_arguments=()
while IFS=$'\t' read -r relative archive member; do
  [[ "${member}" == *.gs || "${member}" == *.GS ]] || continue
  target="${work_dir}/base/${relative}"
  mkdir -p "$(dirname "${target}")"
  if "${viewer_tool}" --extract "${game_dir}/${archive}" "${member}" "${target}" >/dev/null 2>&1; then
    base_file_arguments+=(--base-file "${relative}=${target}")
  fi
done < <(PYTHONPATH="${project_dir}/tools" python3 -c '
import json, sys
plan = json.load(open(sys.argv[1]))
for archive, entries in plan["archives"].items():
    for entry in entries:
        print(entry["relative"], archive, entry["member"], sep="\t")
' "${work_dir}/plan.json")

: > "${work_dir}/base-facts.jsonl"
for archive in "${PIPELINE_ARCHIVES[@]}"; do
  cat "$(base_gs_facts "${game_dir}/${archive}")" >> "${work_dir}/base-facts.jsonl"
done
"${viewer_tool}" --gs-facts "${mod_dir}" > "${work_dir}/mod-facts.jsonl"

PYTHONPATH="${project_dir}/tools" python3 "${project_dir}/tools/mod_build.py" report \
  "${mod_dir}" \
  "${manifest_arguments[@]}" \
  "${digest_arguments[@]}" \
  "${tool_arguments[@]}" \
  "${output_digest_arguments[@]}" \
  "${base_file_arguments[@]}" \
  --base-gs-facts "${work_dir}/base-facts.jsonl" \
  --mod-gs-facts "${work_dir}/mod-facts.jsonl" \
  --symbols "${project_dir}/reports/gameplay/symbols.tsv" \
  --build-id "${build_id}" \
  --output-dir "${output_dir}"

echo
echo "== result =="
echo "  build ${build_id}"
echo "  ${output_dir}"
for archive in "${built_archives[@]}"; do
  echo "  ${archive} sha256 $(file_hash "${output_dir}/${archive}")"
done
echo "  This build has passed validation and its shape check. It has NOT been installed."
echo "  Install it with: scripts/install-dev.sh ${mod_id} ${build_id}"
