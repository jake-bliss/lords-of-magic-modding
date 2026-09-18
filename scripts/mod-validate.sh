#!/usr/bin/env bash
# Statically validate a mod source tree. Packs nothing and installs nothing.
#
# Reads the base profile's archives, read-only, to get their manifests and their GameScript facts.
# It never opens a game file for writing and it never runs StormLib's writer.
#
# Usage:
#   scripts/mod-validate.sh mods/<mod-id>
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

[[ $# -eq 1 ]] || die "usage: $0 mods/<mod-id>"
mod_dir="$(cd "$1" && pwd)" || die "not a directory: $1"

prepare_tools

base_profile="$(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import sys, mod_tree; print(mod_tree.load(sys.argv[1]).manifest.base_profile)' "${mod_dir}")"
game_dir="$(profile_game_dir "${base_profile}")"

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

manifest_arguments=()
for archive in "${PIPELINE_ARCHIVES[@]}"; do
  source_archive="${game_dir}/${archive}"
  [[ -f "${source_archive}" ]] || die "base profile ${base_profile} has no ${archive}: ${source_archive}"
  archive_listfile="$(profile_listfile "${base_profile}" "${archive}")"
  listfile_options=()
  [[ -n "${archive_listfile}" ]] && listfile_options=(--listfile "${archive_listfile}")
  "${mpq_tool}" manifest "${source_archive}" "${listfile_options[@]}" \
    > "${work_dir}/${archive}.tsv"
  manifest_arguments+=(--base-manifest "${archive}=${work_dir}/${archive}.tsv")
done

# Only gs.mpq holds GameScript. pic.mpq is scanned too so that a `.gs` member appearing there
# would be seen rather than assumed away.
: > "${work_dir}/base-facts.jsonl"
for archive in "${PIPELINE_ARCHIVES[@]}"; do
  cat "$(base_gs_facts "${game_dir}/${archive}")" >> "${work_dir}/base-facts.jsonl"
done

"${viewer_tool}" --gs-facts "${mod_dir}" > "${work_dir}/mod-facts.jsonl"

echo "== base profile =="
echo "  ${base_profile}  ${game_dir}"
for archive in "${PIPELINE_ARCHIVES[@]}"; do
  echo "  ${archive} sha256 $(file_hash "${game_dir}/${archive}")"
done
echo

PYTHONPATH="${project_dir}/tools" python3 "${project_dir}/tools/mod_validate.py" \
  "${mod_dir}" \
  "${manifest_arguments[@]}" \
  --base-gs-facts "${work_dir}/base-facts.jsonl" \
  --mod-gs-facts "${work_dir}/mod-facts.jsonl" \
  --vocabulary "${project_dir}/reports/gs/vocabulary-${base_profile}.tsv"
