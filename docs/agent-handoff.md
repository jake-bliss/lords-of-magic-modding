# Agent handoff — 2026-09-16 (community survey + VM slice)

## Outcome and next move

This is a working macOS setup plus a native Rust asset/MPQ viewer and an experimental GameScript interpreter, **not** a native playable replacement. The best next bounded implementation task is [issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5): extend the VM just far enough to load one more engine-light module and classify unknown host calls. Do not jump straight to a full engine rewrite. The user's immediate question about difficulty is answered in [Difficulty and AI](difficulty-ai.md): vanilla has difficulty-gated strategic behavior, while GS5R3 adds tactical difficulty checks and difficulty-scaled AI stat bonuses.

A community research survey was completed on 2026-09-16 — see [community research](community-research.md). The surviving modding community is **live**, has a 2011 IMP specification that matches our decoder, and has a 2026 toolchain covering much of our Stage 1 scope. Read that document before trusting any community claim: two headline claims by the mod's own author about his own code were refuted by our corpus.

## Repository and local setup

- Repository: `https://github.com/jake-bliss/lords-of-magic-modding` (private); local checkout `/Users/jakebliss/personal-projects/lords-of-magic-modding`.
- Last checkpoint: PR #9 (`9159e93`). `main` is clean and synchronized; PRs #7, #8 and #9 all merged on 2026-09-16. The five `Codex/*` worktrees predate this work and are all behind `main`; leave them alone.
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

As of 2026-09-16, `cargo test` passes **47 library and 4 CLI/viewer tests**, `python3 -m unittest discover -s tests` passes 3, and `cargo clippy --all-targets -- -D warnings` is clean. `--validate-imp` on the GS5R3 `imp.mpq` reports 1,798 pairs, 1,788 validated, 10 failures, 4 orphans — the known bounded mismatches. The full proprietary corpus was last scanned on 2026-09-12; rerun that scan on the user's installed GS5R3 archives before claiming a new corpus validation. The exact read-only inventory and viewer commands are in the [tool README](../spikes/asset-viewer/README.md). Current `build.rs` assumes Homebrew StormLib/SDL3 under `/opt/homebrew/opt`; portable discovery is still open.

## Open work and safe order

1. [#5 GameScript VM](https://github.com/jake-bliss/lords-of-magic-modding/issues/5): **partly landed in PR #9.** The definition-shape classifier, `--stub NAME=VALUE` native state reads, and structured unknown-name traces all exist now. What remains is loading a *second* engine-light module end to end and turning the 2,151-name candidate vocabulary into an actual classification. Start by probing a real module and following the `unknown-native-name` traces outward.
2. [#4 Map variants](https://github.com/jake-bliss/lords-of-magic-modding/issues/4): 52-/53-byte tails, object-field semantics, and controlled Map Editor save diff. The save diff is parked because Wine-window automation was blocked by macOS accessibility controls; ask the user for a manual export if needed.
3. [#1 IMP presentation](https://github.com/jake-bliss/lords-of-magic-modding/issues/1), [#2 timing/direction](https://github.com/jake-bliss/lords-of-magic-modding/issues/2), and [#3 validation exceptions](https://github.com/jake-bliss/lords-of-magic-modding/issues/3): original-game comparison and lossless decoder exceptions.
4. [#6 Difficulty/AI gameplay proof](https://github.com/jake-bliss/lords-of-magic-modding/issues/6): the static finding is now stronger — GS5R3's AI stat-bonus path is located and a competing community claim refuted — but controlled gameplay proof remains open.

**Known soft spot in what just landed:** `GameScriptDocument::DEFINITION_WINDOW` is 3 tokens, chosen to fit the observed forms rather than derived. The `X /dummy <dict> replace bind def` closure idiom — 7,472 uses of `replace` in GS5R3 — is not fully modelled by it. If the candidate vocabulary ever looks wrong, suspect this first.

Newly available leads, all from the 2026-09-16 survey:

- Issue #4 gains a falsifiable hypothesis: the unknown 4-byte map field at offset `0x00` is described by a community tool author as a compression header that **disappears** on oversized maps, corrupting maps and saves. Oversized custom maps should therefore shift every later offset by four bytes.
- Issue #5's classifier correction is **done**; roughly 30 confirmed native host names and six recommended first stubs are listed in the issue and in [GameScript](gamescript-format.md).
- Issues #1 and #2 gained palette index semantics and the facing/clockwise naming, both now applied in code. A `0x08`-only duplicate-count hypothesis for issue #3 was tested and **refuted** (10 failures becomes 112) — do not retry it. Issue #1 is no longer a lead but a stated mechanism: board thread 2176 establishes that `lomut` writes no hotspot data at all, which is the real cause of the "512x512 hotspot" problem, and the hotspot ID is a type tag from a 19-constant engine vocabulary in `lomse.exe`. Both were confirmed here by corpus measurement and independent hex parse. What remains on #1 is placement semantics — how a `(type, x, y)` triple becomes a screen position — plus two files whose hotspot types fall outside the engine vocabulary. See [community research](community-research.md#the-hotspot-mechanism-thread-2176).
- **Issue #6's static side is effectively closed.** The GS5R3 AI-bonus path is located, and the `extra_strong?` question is settled by execution: the shipped body is `false` at every difficulty, while the body quoted on the forum is `true` on Hard in single-player. What remains is controlled *gameplay* measurement, holding `insane_mode?` constant.

For any new task, document the evidence class: **observed in a local binary/script**, **observed in gameplay**, **community claim**, or **inference**. Keep 3.02's focused bug fix distinct from GS5R3's broad replacement scripts. Run proportionate Rust tests and read-only corpus checks, then update the relevant documentation and GitHub issue. The user has previously asked to keep work pushed and merged to `main`; check current authorization and remote state before publishing a new branch.

## Practical pitfalls

- Archive path names are case-insensitive Windows names; some original members are anonymous until the public/internal listfiles are loaded.
- IMP compositing is keyed by palette **index**, not colour: slot 0 is transparency and slot 1 is the shadow. The green and red RGB values in those slots are incidental. Clean preview, mask, and raw modes still expose different evidence, and blending behavior is unresolved.
- Community material is a source of hypotheses, never specification. Check it against the corpus before writing it down; the survey refuted several claims, including two from the mod's own author about his own code. Where a claim turns out to describe unshipped code rather than a mistake, say so — it is both more accurate and fairer.
- A pending outreach exists: Jake posted our findings to forum thread 2590 on 2026-09-16 (the 12 files that crash the community parser, their unchecked `rle_decode_1` run loop, the type-57 gap, and our map-header measurements). If a reply has arrived, it is the highest-value input available. The repo is still private pending that exchange; it is hygiene-ready apart from six home-path lines.
- Mantera's site is **HTTP-only with no TLS listener**, so any fetcher that force-upgrades to HTTPS fails with `ECONNREFUSED`. The forum rate-limits automated requests with a proof-of-work challenge; read it politely and do not attempt to defeat it.
- Generated `.h` IMP metadata is strong ground truth but not perfect: ten paired disagreements and four public-catalog orphans remain explicit failures.
- The GameScript VM is deliberately bounded. A successful utility module does not imply a tractable full host/simulation API.
- Keep the working baseline and its saves untouched. Use independent mod profiles for gameplay tests.
