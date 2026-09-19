#!/usr/bin/env bash
# Regenerate the loose-file reports and run every corpus-gated check behind them.
#
# The checks in `spikes/asset-viewer/tests/loose.rs` come in two kinds. The ones that read the
# committed TSVs run in an ordinary `cargo test`. The ones that re-derive those TSVs from the
# installed game need the proprietary tree, so they are `#[ignore]`d -- and an `#[ignore]`d guard
# that no tracked command invokes is the same as no guard. Consistently permuting two fields of
# `LomConfig` left an ordinary `cargo test` green precisely because the re-derivation was skipped.
#
# So: this is the tracked command. Run it whenever the reports are regenerated, and whenever
# `src/loose.rs` changes. It is READ-ONLY on ~/Applications -- it only writes under reports/.
#
# Fine to run concurrently with `python3 -m unittest discover -s tests`. That used to not be true:
# the mod pipeline's guard was a bare `pgrep -f 'lomse.exe'`, which matched any live command line
# merely containing that string -- including this project's own tooling and this very script's
# shell -- so the Python suite could go red, or skip every test in the file, for reasons that had
# nothing to do with either script. The guard is anchored now; see `game_command_pattern` in
# `scripts/lib-mod-pipeline.sh`.
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
applications_dir="${1:-${HOME}/Applications}"
game_subpath='Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition'
viewer="${project_dir}/spikes/asset-viewer/target/release/lom-asset-viewer"
report_dir="${project_dir}/reports/loose"

# Labels are the ones the committed reports and `LOM_PROFILE` use; keep them in step with
# `PROFILES` in `spikes/asset-viewer/tests/loose.rs`.
profile_labels=(baseline development gs5r3 patch302)
profile_apps=(
  'Steambuild 32 64bit DXVK.app'
  'Lords of Magic Development.app'
  'Lords of Magic GS5R3.app'
  'Lords of Magic 3.02.app'
)

(cd "${project_dir}/spikes/asset-viewer" && cargo build --release)
[[ -x "${viewer}" ]] || { echo "viewer did not build: ${viewer}" >&2; exit 1; }
mkdir -p "${report_dir}"

for index in "${!profile_labels[@]}"; do
  label="${profile_labels[${index}]}"
  root="${applications_dir}/${profile_apps[${index}]}/${game_subpath}"
  if [[ ! -d "${root}" ]]; then
    echo "missing profile, skipping: ${root}" >&2
    continue
  fi
  echo "== ${label}"
  "${viewer}" --loose-inventory "${root}" "${label}" > "${report_dir}/inventory-${label}.tsv"
done

# The configuration report is one table for all profiles, so it is rebuilt in one pass. Field order
# follows `describe_loose_config`, which is what `the_committed_configuration_report_reproduces`
# compares against for exact equality.
{
  printf 'profile\tfile\tfield\tvalue\n'
  for index in "${!profile_labels[@]}"; do
    label="${profile_labels[${index}]}"
    english="${applications_dir}/${profile_apps[${index}]}/${game_subpath}/English"
    [[ -d "${english}" ]] || continue
    for file in lom.cfg settings.cfg; do
      "${viewer}" --loose-config "${english}/${file}" \
        | awk -F'\t' -v profile="${label}" -v file="${file}" '
            $1 == "file" { next }
            $1 == "setting" { print profile "\t" file "\tsetting:" $2 "\t" $3; next }
            { print profile "\t" file "\t" $1 "\t" $2 }'
    done
  done
} > "${report_dir}/config-fields.tsv"

# Now the guards, once per profile. `--ignored` runs exactly the corpus-gated set.
for index in "${!profile_labels[@]}"; do
  label="${profile_labels[${index}]}"
  root="${applications_dir}/${profile_apps[${index}]}/${game_subpath}"
  [[ -d "${root}" ]] || continue
  echo "== corpus-gated checks: ${label}"
  (
    cd "${project_dir}/spikes/asset-viewer"
    LOM_GAME_DIR="${root}/English" \
    LOM_INSTALL_ROOT="${root}" \
    LOM_PROFILE="${label}" \
      cargo test --release --test loose -- --ignored
  )
done

echo
echo "Reports regenerated and every corpus-gated check passed."
echo "Review 'git diff reports/loose/' before committing: the gs5r3 profile is played, so its"
echo "saves and logs move between runs and a diff there is not necessarily a regression."
