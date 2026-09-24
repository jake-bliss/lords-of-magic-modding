#!/usr/bin/env bash
# Install a patched lomse.exe into the development profile -- and into nothing else, ever.
#
#   scripts/install-dev-exe.sh tools/exe_patches/A.toml [tools/exe_patches/B.toml ...]
#   scripts/install-dev-exe.sh --restore
#
# The binary is always built from the development profile's RECORDED pristine copy
# (.lom-pipeline/pristine/lomse.exe), never from whatever lomse.exe is installed at the moment, so
# sets never stack on an earlier patch and a rung's binary is exactly the sets it names. The
# pristine copy is recorded on first use, and only if the installed exe is the GS5R3 binary the
# patch sets target; after that it is verified against that hash every time.
#
# Every write goes through tools/install_guard.py, like scripts/install-dev.sh.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

# The binary every set in tools/exe_patches/ was derived from.
PRISTINE_SHA=a505f399d5be73fe0a2215633f663717f28daeb3075bbcc05b47d40653669052

restore=0
sets=()
while (( $# )); do
  case "$1" in
    --restore) restore=1; shift ;;
    --help|-h) sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 0 ;;
    *) sets+=("$1"); shift ;;
  esac
done
(( restore )) || (( ${#sets[@]} )) || die "name at least one patch set, or --restore"
(( restore )) && (( ${#sets[@]} )) && die "--restore takes no patch sets"

refuse_if_game_running
dev_root="$(dev_profile_root)"
metadata_dir="$(dev_metadata_dir)"
dev_game_dir="${dev_root}/${game_subpath}"
[[ -d "${metadata_dir}" ]] || die "no development profile at ${dev_root}"
[[ -f "${dev_game_dir}/lomse.exe" ]] || die "the development profile has no lomse.exe"
pristine="${metadata_dir}/pristine/lomse.exe"

if [[ ! -f "${pristine}" ]]; then
  current="$(file_hash "${dev_game_dir}/lomse.exe")"
  [[ "${current}" == "${PRISTINE_SHA}" ]] || die "no pristine lomse.exe recorded, and the installed one
  is ${current}, not the GS5R3 binary ${PRISTINE_SHA}.
Refusing to record a binary of unknown provenance as pristine."
  approved="$(approve_dev_path "${pristine}")"
  cp -c "${dev_game_dir}/lomse.exe" "${approved}"
  echo "  recorded pristine lomse.exe ${current}"
fi
[[ "$(file_hash "${pristine}")" == "${PRISTINE_SHA}" ]] \
  || die "${pristine} is not ${PRISTINE_SHA}; the recorded pristine copy has changed"

if (( restore )); then
  source_exe="${pristine}"
  label="-"
  build_id="pristine"
else
  build_root="${project_dir}/artifacts/build/exe"
  mkdir -p "${build_root}"
  staging="$(mktemp "${build_root}/lomse.XXXXXX")"
  set_args=()
  for s in "${sets[@]}"; do set_args+=(--set "${s}"); done
  python3 "${project_dir}/tools/exe_patch.py" build "${pristine}" "${staging}" "${set_args[@]}" \
    || { rm -f "${staging}"; die "patch sets did not apply"; }
  digest="$(file_hash "${staging}")"
  mkdir -p "${build_root}/${digest:0:12}"
  source_exe="${build_root}/${digest:0:12}/lomse.exe"
  mv "${staging}" "${source_exe}"
  label="exe:$(for s in "${sets[@]}"; do basename "${s}" .toml; done | paste -sd+ -)"
  build_id="${digest:0:12}"
fi

expected="$(file_hash "${source_exe}")"
approved="$(approve_dev_path "${dev_game_dir}/lomse.exe")"
cp "${source_exe}" "${approved}"
installed="$(file_hash "${approved}")"
[[ "${installed}" == "${expected}" ]] || die "installed lomse.exe is ${installed}, expected ${expected}"
printf '%s\t%s\t%s\t%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "${label}" "${build_id}" \
  lomse.exe "${installed}" >> "${metadata_dir}/INSTALLS.tsv"
echo "  lomse.exe ${installed}  OK (${label} ${build_id})"
