#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 APPLICATIONS_DIR OUTPUT_DIR" >&2
  echo "Example: $0 /Users/you/Applications artifacts/run-YYYYMMDD" >&2
  exit 2
fi

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
applications_dir="$(cd "$1" && pwd)"
output_dir="$2"

if [[ -e "${output_dir}" ]]; then
  echo "Output path already exists; choose a fresh directory: ${output_dir}" >&2
  exit 1
fi

game_subpath='Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English'
profile_labels=(vanilla patch302 gs5r3)
profile_apps=(
  'Steambuild 32 64bit DXVK.app'
  'Lords of Magic 3.02.app'
  'Lords of Magic GS5R3.app'
)

"${project_dir}/scripts/build-tools.sh"
mpq_tool="${project_dir}/.build/lom-mpq"

for index in "${!profile_labels[@]}"; do
  label="${profile_labels[$index]}"
  game_dir="${applications_dir}/${profile_apps[$index]}/${game_subpath}"
  for archive in gs pic; do
    source_mpq="${game_dir}/${archive}.mpq"
    if [[ ! -f "${source_mpq}" ]]; then
      echo "Missing archive: ${source_mpq}" >&2
      exit 1
    fi
    mkdir -p "${output_dir}/extracted/${label}/${archive}" "${output_dir}/inventory"
    "${mpq_tool}" list "${source_mpq}" > "${output_dir}/inventory/${label}-${archive}.tsv"
    "${mpq_tool}" extract "${source_mpq}" "${output_dir}/extracted/${label}/${archive}"
  done
done

python3 "${project_dir}/tools/compare_trees.py" \
  --base "vanilla=${output_dir}/extracted/vanilla/gs" \
  --variant "patch302=${output_dir}/extracted/patch302/gs" \
  --variant "gs5r3=${output_dir}/extracted/gs5r3/gs" \
  --output "${output_dir}/reports/gs"

python3 "${project_dir}/tools/compare_trees.py" \
  --base "vanilla=${output_dir}/extracted/vanilla/pic" \
  --variant "patch302=${output_dir}/extracted/patch302/pic" \
  --variant "gs5r3=${output_dir}/extracted/gs5r3/pic" \
  --output "${output_dir}/reports/pic"

echo "Inventory complete: ${output_dir}"
