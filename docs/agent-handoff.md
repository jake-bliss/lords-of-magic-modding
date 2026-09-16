# Agent handoff — 2026-09-16 (engine operator table, arity, hotspot geometry)

## Outcome and next move

This is a working macOS setup plus a native Rust asset/MPQ viewer and an experimental GameScript interpreter, **not** a native playable replacement.

**The immediate next task is a controlled experiment on [issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1), and it is waiting on user permission — do not start it without asking.** See "The pending issue #1 experiment" below. Do not jump straight to a full engine rewrite. The user's immediate question about difficulty is answered in [Difficulty and AI](difficulty-ai.md): vanilla has difficulty-gated strategic behavior, while GS5R3 adds tactical difficulty checks and difficulty-scaled AI stat bonuses.

A community research survey was completed on 2026-09-16 — see [community research](community-research.md). The surviving modding community is **live**, has a 2011 IMP specification that matches our decoder, and has a 2026 toolchain covering much of our Stage 1 scope. Read that document before trusting any community claim: two headline claims by the mod's own author about his own code were refuted by our corpus.

## Repository and local setup

- Repository: `https://github.com/jake-bliss/lords-of-magic-modding` (private); local checkout `/Users/jakebliss/personal-projects/lords-of-magic-modding`.
- Last checkpoint: PR #21 (`2727572`). `main` is clean and synchronized; PRs #7 through #21 all merged on 2026-09-16. The five `Codex/*` worktrees predate this work and are all behind `main`; leave them alone.
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

As of 2026-09-16, `cargo test` passes **70 library and 8 CLI/viewer tests**, `python3 -m unittest discover -s tests` passes 21, and `cargo clippy --all-targets -- -D warnings` is clean. `--validate-imp` on the GS5R3 `imp.mpq` reports 1,798 pairs, 1,788 validated, 10 failures, 4 orphans — the known bounded mismatches. The full proprietary corpus was last scanned on 2026-09-12; rerun that scan on the user's installed GS5R3 archives before claiming a new corpus validation. The exact read-only inventory and viewer commands are in the [tool README](../spikes/asset-viewer/README.md). Current `build.rs` assumes Homebrew StormLib/SDL3 under `/opt/homebrew/opt`; portable discovery is still open.

## Open work and safe order

1. **[#1 IMP presentation](https://github.com/jake-bliss/lords-of-magic-modding/issues/1)** — the designed experiment above, pending permission. Everything static is done.
2. [#5 GameScript VM](https://github.com/jake-bliss/lords-of-magic-modding/issues/5): **mostly resolved.** The engine's operator tables are recovered (1,906 natives with entry points), arity is recovered by disassembly, and the VM classifies any name it stops on as operator / engine-constant / unresolved with a signature. What remains is loading a *second* engine-light module end to end.
3. [#22 Oversized map header](https://github.com/jake-bliss/lords-of-magic-modding/issues/22): **blocked**, needs one map larger than 256x256. Ask the board; eyesodilated's MapEditor likely has samples.
4. [#4 Map variants](https://github.com/jake-bliss/lords-of-magic-modding/issues/4): 52-/53-byte tails and object-field semantics. The Map Editor save diff is parked because Wine-window automation was blocked by macOS accessibility controls; ask the user for a manual export if needed.
5. [#2 timing/direction](https://github.com/jake-bliss/lords-of-magic-modding/issues/2) and [#3 validation exceptions](https://github.com/jake-bliss/lords-of-magic-modding/issues/3): original-game comparison and lossless decoder exceptions.
6. [#6 Difficulty/AI gameplay proof](https://github.com/jake-bliss/lords-of-magic-modding/issues/6): static side closed; controlled gameplay measurement holding `insane_mode?` constant remains open.

**Two methodological traps, both paid for today. Read these before trusting any new measurement.**

1. **Validate on a representative sample, not a convenient one.** The arity walk scored 23/24 against PostScript primitives and was shipped as "1,875 of 1,908 walks well formed". That was wrong: the primitives are mostly direct-branch, while the 1,804 game operators dispatch through jump tables the walk cannot follow. It reported `getarmydata` as pushing nothing, with `well-formed` confidence. Corrected in PR #16 — the real figure is **118** complete walks. The accuracy claim survived; the completeness claim did not.
2. **Derive a label in one place.** Merging PR #19 after #16 left two copies of the confidence mapping and only one knew about `indirect-branch`, so the same operator was reported two different ways by two commands. Fixed in PR #20 by `StackEffect::confidence()`.

**Known soft spot, still open:** `GameScriptDocument::DEFINITION_WINDOW` is 3 tokens, chosen to fit the observed forms rather than derived. The `X /dummy <dict> replace bind def` closure idiom — 7,472 uses of `replace` in GS5R3 — is not fully modelled by it. If the candidate vocabulary ever looks wrong, suspect this first.

Newly available leads, all from the 2026-09-16 survey:

- The oversized-map hypothesis moved out of #4 into its own tracked issue, **#22**, labelled blocked. It needs one map larger than 256x256, which nothing locally provides.
- Issue #5's classifier correction is **done**; roughly 30 confirmed native host names and six recommended first stubs are listed in the issue and in [GameScript](gamescript-format.md).
- Issues #1 and #2 gained palette index semantics and the facing/clockwise naming, both now applied in code. A `0x08`-only duplicate-count hypothesis for issue #3 was tested and **refuted** (10 failures becomes 112) — do not retry it. Issue #1 is no longer a lead but a stated mechanism: board thread 2176 establishes that `lomut` writes no hotspot data at all, which is the real cause of the "512x512 hotspot" problem, and the hotspot ID is a type tag from a 19-constant engine vocabulary in `lomse.exe`. Both were confirmed here by corpus measurement and independent hex parse. What remains on #1 is the sign convention and shadow blend, which need the running engine — see the experiment section above. **The cursor hotspot has since been shown to be authored, not derivable**: across 28,447 unit frames, x is independent of frame width (median exactly 0, residual 8.84 px of 8.87 raw) and height explains only about half of y (residual 10.03 of 14.55). That is why the board's crop-and-re-centre workaround never converged, and why `lomut` dropping the hotspot array is unrecoverable rather than inconvenient. Two files, `eacr5a.imp` and `aiwm1b.imp`, still carry hotspot types outside the engine's 19-name vocabulary; the bytes are verified real by hex. See [community research](community-research.md#the-hotspot-mechanism-thread-2176).
- **Issue #6's static side is effectively closed.** The GS5R3 AI-bonus path is located, and the `extra_strong?` question is settled by execution: the shipped body is `false` at every difficulty, while the body quoted on the forum is `true` on Hard in single-player. What remains is controlled *gameplay* measurement, holding `insane_mode?` constant.

For any new task, document the evidence class: **observed in a local binary/script**, **observed in gameplay**, **community claim**, or **inference**. Keep 3.02's focused bug fix distinct from GS5R3's broad replacement scripts. Run proportionate Rust tests and read-only corpus checks, then update the relevant documentation and GitHub issue. The user has previously asked to keep work pushed and merged to `main`; check current authorization and remote state before publishing a new branch.

## The pending issue #1 experiment — ask before starting

Everything the *files* can say about hotspots has been said. What remains on
[issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1) is how the engine
*consumes* a hotspot: the sign convention (is the sprite drawn at `position - hotspot` or
`position + hotspot`, and does `+y` mean up or down?) and how the shadow index is blended. Both live
in the drawing code, so no amount of file measurement settles them.

The designed experiment, agreed with the user but **not yet authorised to run**:

1. **Confirm loose-file precedence** — cheapest falsifiable step, do this first.
2. Write a `.gs` calling **`drawimpframe`** (5 operands, from the operator table) to draw a known
   frame at known screen coordinates — one whose hotspot we have already decoded.
3. Launch the profile, screenshot, measure where the sprite actually landed.
4. The offset between commanded and observed position *is* the sign convention. The shadow blend is
   visible in the same capture.

**Why `getimphotspot` alone is not the answer.** It returns the hotspot the engine read *from the
file* — the same number our decoder already reports. The convention lives in the consumer, not the
accessor. Querying it only confirms both sides read the same bytes. `drawimpframe` is the native
that matters.

### The loose-file finding this depends on

GS5R3 ships files on disk that shadow archive members and **differ from them**:

```
gs/dlg/comb_dlg.gs     4,951 bytes   loose on disk
gs\dlg\comb_dlg.gs     6,293 bytes   inside gs.mpq
```

Two `.gs` and one `.lbm` are shipped this way. That is the shape of a patch mechanism, and if the
engine really does prefer disk over archive then the experiment needs **no MPQ write-back at all** —
which would otherwise mean implementing archive writing plus the community's finicky
Implode+Encrypt(`0x00010100`) repack ruleset.

**This is strong evidence, not proof.** The loose files could be dead leftovers. Confirm before
building anything on it: place a deliberately malformed loose file and see whether the game
complains. If it does, precedence is real.

### Permissions required

The user's standing rule is that game files are strictly read-only, so both of these need explicit
confirmation each time:

- adding a file to a game profile — use `Lords of Magic GS5R3.app`, **never** the
  `Steambuild 32 64bit DXVK.app` baseline, add rather than modify, and back up anything touched;
- launching the game under Wine and capturing screenshots.

## Practical pitfalls

- Archive path names are case-insensitive Windows names; some original members are anonymous until the public/internal listfiles are loaded.
- IMP compositing is keyed by palette **index**, not colour: slot 0 is transparency and slot 1 is the shadow. The green and red RGB values in those slots are incidental. Clean preview, mask, and raw modes still expose different evidence, and blending behavior is unresolved.
- Community material is a source of hypotheses, never specification. Check it against the corpus before writing it down; the survey refuted several claims, including two from the mod's own author about his own code. Where a claim turns out to describe unshipped code rather than a mistake, say so — it is both more accurate and fairer.
- **Jake posted replies to both forum threads on 2026-09-16** — thread 2590 and thread 2176 (the hotspot mechanism thread, answering ozz on the 19-name constant list, HSType reaching 9, and the shipped `aura.gs` question). Replies are awaited; a reply from **ozz** would be the highest-value input available, especially on the two files with out-of-vocabulary hotspot types. The repo is still private pending that exchange; it is hygiene-ready apart from six home-path lines. Earlier context: Jake posted our findings to forum thread 2590 on 2026-09-16 (the 12 files that crash the community parser, their unchecked `rle_decode_1` run loop, the type-57 gap, and our map-header measurements). If a reply has arrived, it is the highest-value input available. The repo is still private pending that exchange; it is hygiene-ready apart from six home-path lines.
- Mantera's site is **HTTP-only with no TLS listener**, so any fetcher that force-upgrades to HTTPS fails with `ECONNREFUSED`. The forum rate-limits automated requests with a proof-of-work challenge; read it politely and do not attempt to defeat it.
- Generated `.h` IMP metadata is strong ground truth but not perfect: ten paired disagreements and four public-catalog orphans remain explicit failures.
- The GameScript VM is deliberately bounded. A successful utility module does not imply a tractable full host/simulation API.
- Keep the working baseline and its saves untouched. Use independent mod profiles for gameplay tests.
