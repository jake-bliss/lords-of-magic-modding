#!/usr/bin/env bash
# Copy a member out of the base profile's archive and into a mod source tree.
#
#   scripts/mod-seed.sh mods/<mod-id> 'gs.mpq:units\orinf.gs' ...
#
# The bytes are written exactly as the archive holds them. Nothing is normalised, re-terminated or
# re-encoded on the way, because the first validate after a seed is a control: it should report an
# unchanged member and no findings, and it cannot do that if the seeding step already changed the
# file.
#
# Refuses to overwrite an existing file in the tree. Losing an edit to a re-seed is a silent way to
# undo work, and a mod tree holds the only copy of it.
set -euo pipefail

# shellcheck source=scripts/lib-mod-pipeline.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-mod-pipeline.sh"

(( $# >= 2 )) || die "usage: $0 mods/<mod-id> 'ARCHIVE:MEMBER' ..."
mod_dir="$(cd "$1" && pwd)" || die "not a directory: $1"
shift

prepare_tools

base_profile="$(PYTHONPATH="${project_dir}/tools" python3 -c \
  'import sys, mod_tree; print(mod_tree.load(sys.argv[1]).manifest.base_profile)' "${mod_dir}")"
game_dir="$(profile_game_dir "${base_profile}")"

echo "== seeding from ${base_profile} =="
for specification in "$@"; do
  archive="${specification%%:*}"
  member="${specification#*:}"
  [[ "${archive}" != "${specification}" ]] || die "expected ARCHIVE:MEMBER, got ${specification}"

  found=0
  for candidate in "${PIPELINE_ARCHIVES[@]}"; do
    [[ "${candidate}" == "${archive}" ]] && found=1
  done
  (( found )) || die "unsupported archive: ${archive}"

  # The member name becomes a path below archives/<archive>/, separators turned round.
  relative="${member//\\//}"
  target="${mod_dir}/archives/${archive}/${relative}"
  [[ -e "${target}" ]] && die "refusing to overwrite ${target}
Delete it yourself if you mean to discard the edits in it."

  mkdir -p "$(dirname "${target}")"
  "${viewer_tool}" --extract "${game_dir}/${archive}" "${member}" "${target}" >/dev/null \
    || die "could not extract ${member} from ${archive}"
  echo "  ${archive}  ${member}"
  echo "    -> ${target}"
  echo "    sha256 $(file_hash "${target}")  $(wc -c < "${target}" | tr -d ' ') bytes"
done

echo
echo "Run scripts/mod-validate.sh ${mod_dir} now, before editing anything."
echo "It should report an unchanged tree with no findings. That is the control."
