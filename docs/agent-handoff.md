# Agent handoff — 2026-09-15

## Outcome and next move

This is a working macOS setup plus a native Rust asset/MPQ viewer and an experimental GameScript interpreter, **not** a native playable replacement. The best next bounded implementation task is [issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5): extend the VM just far enough to load one more engine-light module and classify unknown host calls. Do not jump straight to a full engine rewrite. The user's immediate question about difficulty is answered in [Difficulty and AI](difficulty-ai.md): vanilla has difficulty-gated strategic behavior, while GS5R3 adds tactical difficulty checks.

## Repository and local setup

- Repository: `https://github.com/jake-bliss/lords-of-magic-modding` (private); local checkout `/Users/jakebliss/personal-projects/lords-of-magic-modding`.
- Last code checkpoint before this handoff: `53f17d5b5e959bb65be5d237903f4cf684f3a19e` (`Start GameScript lexer and VM probe`). The handoff documentation was prepared in `Codex/difficulty-handoff`; verify `git status`, `git log -1`, and remote state before new work because the branch may already have been merged.
- Worktree policy: create code-change worktrees with `wt switch --create Codex/<task>`, inspect with `wt list`, and clean merged worktrees with `wt remove`. Preserve other worktrees and user edits.
- Three app profiles under `/Users/jakebliss/Applications/`: `Steambuild 32 64bit DXVK.app` (preserved recovery baseline), `Lords of Magic 3.02.app`, and `Lords of Magic GS5R3.app`. Their Windows installs are under each app's `Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English/`. Never modify the baseline for experiments. See [macOS runbook](macos-runbook.md).
- Original game archives and saves are user-owned, proprietary, and excluded from Git. Use the installed archives read-only, keep generated outputs under ignored `artifacts/` or a temporary directory, and publish only original code, metadata, analysis, or patches.
- The public archive name list is fetched with `scripts/fetch-lom-listfile.sh` into ignored `artifacts/reference-listfiles/lords-of-magic.txt`; a fresh worktree does not inherit it. See [native asset stage](native-asset-stage.md).

## What is complete versus open

| Track | Verified capability | Not yet proved |
| --- | --- | --- |
| macOS play | Baseline, 3.02, and GS5R3 launch-tested; CD prompt fixed by the 32-bit Steam registry key | Future OS/Wine compatibility and save portability |
| Stage 0 evidence | Three-profile archive comparison and mod/patch inventory | No claim of full engine behavior |
| Stage 1 asset layer | 9,804 core archive members classified; 1,377 PBMs decoded; 1,800 IMPs expanded; 26 tilesets and all 365 loose maps bounded; 16,628 dominant 49-byte map-object records parsed | IMP compositing/timing, 10 metadata mismatches + 4 catalog orphans, other map tails, reimport/repacking, portable packaging |
| Stage 2 VM probe | 4,692 GameScript members tokenized; baseline/3.02/GS5R3 vocabularies inventoried; 3.02 `standard.gs` loads and script-defined `min`/`max` execute | Host API, simulation, save semantics, complete module loader, playable engine |

Detailed facts and confidence labels live in [Stage 1](native-asset-stage.md), [map format](map-format.md), [GameScript probe](gamescript-format.md), and the [engine candidate plan](native-engine-plan.md). Do not treat Stage 1 coverage as evidence that Stage 2–5 time estimates are reduced proportionally.

## Reproduce before changing code

```sh
cd /Users/jakebliss/personal-projects/lords-of-magic-modding/spikes/asset-viewer
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

As of 2026-09-15, `cargo test` passed **40 library and 4 CLI/viewer tests** in a fresh worktree. The full proprietary corpus was last scanned on 2026-09-12; rerun that scan on the user's installed GS5R3 archives before claiming a new corpus validation. The exact read-only inventory and viewer commands are in the [tool README](../spikes/asset-viewer/README.md). Current `build.rs` assumes Homebrew StormLib/SDL3 under `/opt/homebrew/opt`; portable discovery is still open.

## Open work and safe order

1. [#5 GameScript VM](https://github.com/jake-bliss/lords-of-magic-modding/issues/5): next bounded, engine-light module load and host-call classification; stop with structured traces for unknown names.
2. [#4 Map variants](https://github.com/jake-bliss/lords-of-magic-modding/issues/4): 52-/53-byte tails, object-field semantics, and controlled Map Editor save diff. The save diff is parked because Wine-window automation was blocked by macOS accessibility controls; ask the user for a manual export if needed.
3. [#1 IMP presentation](https://github.com/jake-bliss/lords-of-magic-modding/issues/1), [#2 timing/direction](https://github.com/jake-bliss/lords-of-magic-modding/issues/2), and [#3 validation exceptions](https://github.com/jake-bliss/lords-of-magic-modding/issues/3): original-game comparison and lossless decoder exceptions.
4. [#6 Difficulty/AI gameplay proof](https://github.com/jake-bliss/lords-of-magic-modding/issues/6): static finding is documented, but controlled gameplay proof and the full GS5R3 call-path audit remain open.

For any new task, document the evidence class: **observed in a local binary/script**, **observed in gameplay**, **community claim**, or **inference**. Keep 3.02's focused bug fix distinct from GS5R3's broad replacement scripts. Run proportionate Rust tests and read-only corpus checks, then update the relevant documentation and GitHub issue. The user has previously asked to keep work pushed and merged to `main`; check current authorization and remote state before publishing a new branch.

## Practical pitfalls

- Archive path names are case-insensitive Windows names; some original members are anonymous until the public/internal listfiles are loaded.
- The IMP viewer's green/red raw channels are not a single universal chroma key. Clean preview, mask, and raw modes expose different evidence; exact compositing is unresolved.
- Generated `.h` IMP metadata is strong ground truth but not perfect: ten paired disagreements and four public-catalog orphans remain explicit failures.
- The GameScript VM is deliberately bounded. A successful utility module does not imply a tractable full host/simulation API.
- Keep the working baseline and its saves untouched. Use independent mod profiles for gameplay tests.
