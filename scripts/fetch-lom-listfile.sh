#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
destination_dir="${project_dir}/artifacts/reference-listfiles"
destination="${destination_dir}/lords-of-magic.txt"
source_url="http://www.zezula.net/download/listfiles.zip"
expected_zip_sha256="23e2a42b9f88b02852a14a1af381a572cf9a337fbcd0378b52cb2fedab6104ae"
expected_list_sha256="6b42e7578a06ee5dfbd8181972b66717b5e586df2f87c2137a3127bb70755188"

hash_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

if [[ -f "${destination}" ]]; then
  actual_list_sha256="$(hash_file "${destination}")"
  if [[ "${actual_list_sha256}" == "${expected_list_sha256}" ]]; then
    echo "Already verified ${destination}"
    exit 0
  fi
  echo "Refusing to overwrite unrecognized ${destination}" >&2
  exit 1
fi

temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/lom-listfile.XXXXXX")"
trap 'rm -rf "${temporary_dir}"' EXIT

archive="${temporary_dir}/listfiles.zip"
extracted="${temporary_dir}/lords-of-magic.txt"
curl --fail --location --silent --show-error "${source_url}" --output "${archive}"

actual_zip_sha256="$(hash_file "${archive}")"
if [[ "${actual_zip_sha256}" != "${expected_zip_sha256}" ]]; then
  echo "Downloaded listfiles.zip failed the pinned SHA-256 check" >&2
  echo "Expected: ${expected_zip_sha256}" >&2
  echo "Actual:   ${actual_zip_sha256}" >&2
  exit 1
fi

unzip -p "${archive}" 'Lords of Magic.txt' > "${extracted}"
actual_list_sha256="$(hash_file "${extracted}")"
if [[ "${actual_list_sha256}" != "${expected_list_sha256}" ]]; then
  echo "Extracted Lords of Magic listfile failed the pinned SHA-256 check" >&2
  exit 1
fi

mkdir -p "${destination_dir}"
mv "${extracted}" "${destination}"
echo "Verified and installed ${destination}"
