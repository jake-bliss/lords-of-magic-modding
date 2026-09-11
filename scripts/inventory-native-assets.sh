#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 GAME_DIRECTORY NEW_OUTPUT_DIRECTORY" >&2
  exit 2
fi

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
game_dir="$1"
output_dir="$2"
listfile="${project_dir}/artifacts/reference-listfiles/lords-of-magic.txt"
manifest="${project_dir}/spikes/asset-viewer/Cargo.toml"
tool="${project_dir}/spikes/asset-viewer/target/release/lom-asset-viewer"
archives=(pic special gs imp sndfx)

if [[ ! -d "${game_dir}" ]]; then
  echo "Game directory does not exist: ${game_dir}" >&2
  exit 1
fi
if [[ -e "${output_dir}" ]]; then
  echo "Output path already exists; choose a new directory: ${output_dir}" >&2
  exit 1
fi
if [[ ! -f "${listfile}" ]]; then
  echo "Reference listfile is missing. Run scripts/fetch-lom-listfile.sh first." >&2
  exit 1
fi
for archive_name in "${archives[@]}"; do
  if [[ ! -f "${game_dir}/${archive_name}.mpq" ]]; then
    echo "Required archive is missing: ${game_dir}/${archive_name}.mpq" >&2
    exit 1
  fi
done

cargo build --release --manifest-path "${manifest}"
mkdir -p "${output_dir}"

hash_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

printf 'archive\tbytes\tsha256\n' > "${output_dir}/archives.tsv"
for archive_name in "${archives[@]}"; do
  archive="${game_dir}/${archive_name}.mpq"
  bytes="$(wc -c < "${archive}" | tr -d ' ')"
  sha256="$(hash_file "${archive}")"
  printf '%s\t%s\t%s\n' "${archive_name}.mpq" "${bytes}" "${sha256}" \
    >> "${output_dir}/archives.tsv"
  "${tool}" --scan "${archive}" --listfile "${listfile}" \
    > "${output_dir}/${archive_name}-summary.tsv"
  "${tool}" --catalog "${archive}" --listfile "${listfile}" \
    > "${output_dir}/${archive_name}-catalog.tsv"
done

echo "Native asset inventory written to ${output_dir}"
