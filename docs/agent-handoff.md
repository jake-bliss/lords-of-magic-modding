# Agent handoff — 2026-09-16 (engine operator table, arity, hotspot geometry)

## Outcome and next move

This is a working macOS setup plus a native Rust asset/MPQ viewer and an experimental GameScript interpreter, **not** a native playable replacement.

**[Issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1) is resolved: sprite placement is fully
settled.** The rule, both storage forms, the reserved hotspot record, the type numbers and the writer commands all
live in **[hotspots](hotspots.md)**, which is the single source of truth — link to it rather than restating it.
Its "Still open" section lists what is left, and the next engine run should batch those: the shadow blend, the
palette channel order, and the unit-anchor caveat. Beyond hotspots, `map2screen`'s y/z input convention is still
wrong (its static contract is otherwise decoded — see the research log).

A community research survey was completed on 2026-09-16 — see [community research](community-research.md). The surviving modding community is **live**, has a 2011 IMP specification that matches our decoder, and has a 2026 toolchain covering much of our Stage 1 scope. Read that document before trusting any community claim: two headline claims by the mod's own author about his own code were refuted by our corpus.

## Repository and local setup

- Repository: `https://github.com/jake-bliss/lords-of-magic-modding` (private); local checkout `~/personal-projects/lords-of-magic-modding`.
- Last checkpoint: PR #34 (`30bb2c2`). `main` is clean and synchronized; PRs #7 through #34 are all merged. The five `Codex/*` worktrees predate this work and are all behind `main`; leave them alone.
- Worktree policy: create code-change worktrees with `wt switch --create Codex/<task>`, inspect with `wt list`, and clean merged worktrees with `wt remove`. Preserve other worktrees and user edits.
- Three app profiles under `~/Applications/`: `Steambuild 32 64bit DXVK.app` (preserved recovery baseline), `Lords of Magic 3.02.app`, and `Lords of Magic GS5R3.app`. Their Windows installs are under each app's `Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English/`. Never modify the baseline for experiments. See [macOS runbook](macos-runbook.md).
- Original game archives and saves are user-owned, proprietary, and excluded from Git. Use the installed archives read-only, keep generated outputs under ignored `artifacts/` or a temporary directory, and publish only original code, metadata, analysis, or patches.
- The public archive name list is fetched with `scripts/fetch-lom-listfile.sh` into ignored `artifacts/reference-listfiles/lords-of-magic.txt`; a fresh worktree does not inherit it. See [native asset stage](native-asset-stage.md).

## What is complete versus open

| Track | Verified capability | Not yet proved |
| --- | --- | --- |
| macOS play | Baseline, 3.02, and GS5R3 launch-tested; CD prompt fixed by the 32-bit Steam registry key | Future OS/Wine compatibility and save portability |
| Stage 0 evidence | Three-profile archive comparison and mod/patch inventory | No claim of full engine behavior |
| Stage 1 asset layer | 9,804 core archive members classified; 1,377 PBMs decoded; 1,800 IMPs expanded; 26 tilesets and all 365 loose maps bounded; 16,628 dominant 49-byte map-object records parsed | IMP shadow blend and frame timing, palette channel order, 10 metadata mismatches + 4 catalog orphans, other map tails, reimport/repacking, portable packaging (IMP **placement** is solved — see [hotspots](hotspots.md)) |
| Stage 2 VM probe | 4,692 GameScript members tokenized; baseline/3.02/GS5R3 vocabularies inventoried; 3.02 `standard.gs` loads and script-defined `min`/`max` execute | Host API, simulation, save semantics, complete module loader, playable engine |

Detailed facts and confidence labels live in [Stage 1](native-asset-stage.md), [map format](map-format.md), [GameScript probe](gamescript-format.md), and the [engine candidate plan](native-engine-plan.md). Do not treat Stage 1 coverage as evidence that Stage 2–5 time estimates are reduced proportionally.

## Reproduce before changing code

```sh
cd ~/personal-projects/lords-of-magic-modding/spikes/asset-viewer
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

As of 2026-09-16, `cargo test` passes **83 library and 11 CLI/viewer tests**, `python3 -m unittest discover -s tests` passes 21, and `cargo clippy --all-targets -- -D warnings` is clean. `--validate-imp` on the GS5R3 `imp.mpq` reports 1,798 pairs, 1,788 validated, 10 failures, 4 orphans — the known bounded mismatches. The full proprietary corpus was last scanned on 2026-09-12; rerun that scan on the user's installed GS5R3 archives before claiming a new corpus validation. The exact read-only inventory and viewer commands are in the [tool README](../spikes/asset-viewer/README.md). Current `build.rs` assumes Homebrew StormLib/SDL3 under `/opt/homebrew/opt`; portable discovery is still open.

## Open work and safe order

1. **[#1 IMP presentation](https://github.com/jake-bliss/lords-of-magic-modding/issues/1) — closed.** See [hotspots](hotspots.md); the residual questions are listed there, not here.
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
- Issues #1 and #2 gained palette index semantics and the facing/clockwise naming, both now applied in code. A `0x08`-only duplicate-count hypothesis for issue #3 was tested and **refuted** (10 failures becomes 112) — do not retry it. **Issue #1 is now closed**; everything it covered, including the hotspot mechanism from board thread 2176 and the placement measurements, is consolidated in [hotspots](hotspots.md). The board's crop-and-re-centre workaround never converged because placement is authored per frame and the convention is centre-relative — both now established.
- **Issue #6's static side is effectively closed.** The GS5R3 AI-bonus path is located, and the `extra_strong?` question is settled by execution: the shipped body is `false` at every difficulty, while the body quoted on the forum is `true` on Hard in single-player. What remains is controlled *gameplay* measurement, holding `insane_mode?` constant.

For any new task, document the evidence class: **observed in a local binary/script**, **observed in gameplay**, **community claim**, or **inference**. Keep 3.02's focused bug fix distinct from GS5R3's broad replacement scripts. Run proportionate Rust tests and read-only corpus checks, then update the relevant documentation and GitHub issue. The user has previously asked to keep work pushed and merged to `main`; check current authorization and remote state before publishing a new branch.

## Engine experiments — what is settled, and how to run one

**Sprite placement is solved; the facts live in [hotspots](hotspots.md), which is the single source
of truth.** Do not restate the rule in other documents — link to it. What remains on the engine side
is listed in that file's "Still open" section.

The rest of this section is the reusable machinery: what was refuted, how to get code into the
running engine, and the traps that have produced wrong conclusions here before.

### Loose-file precedence: refuted, do not retry it

The earlier hypothesis — that GS5R3's loose on-disk `.gs` files override their `gs.mpq` namesakes,
letting the experiment skip archive write-back — was tested in the running game and is **false**.
Full three-arm write-up in the [research log](research-log.md). The short version:

- `gs/dlg/comb_dlg.gs` is a **dead vanilla leftover**: `START.GS` runs `COMB_DLG5.gs` instead, and
  nothing in the 1,471 extractable scripts names it. Only `scroldlg.gs` is actually loaded.
- `scroldlg.gs` loads at position **53** of 94 `run` statements; `gs/logs/makelogs.gs` loads at
  **93** and appends a line to `combat.log` every start. That ordering is the control that makes a
  null result readable — without it, "no effect" and "never reached" look the same.
- A loose file carrying a sentinel that writes its own log file produced nothing, while the control
  line appeared. A loose file of pure garbage changed nothing either.

**The methodological trap here cost a wrong conclusion.** The garbage arm *did* fail to launch the
first time, which looked like proof of precedence. The cause was `wineserver` still shutting down 3
seconds after the previous instance was killed. Re-run against a quiesced prefix, it started fine.
Always wait for `lomse.exe` to disappear *and* add a fixed delay before judging a launch.

### The injection path, proved end to end

Archive write-back works. The encryption worry was misplaced: members are plain
`MPQ_FILE_IMPLODE | MPQ_FILE_EXISTS` (`0x80000100`), only `(listfile)` is encrypted.

- `SFileAddFileEx` round-trips: member replaced, reads back byte-identical, member count unchanged at
  1,700, flags preserved. The archive keeps its original shape — format `0`, sector shift `3`, 4,096
  hash entries, 1,700 block entries; only `archive_size` and the two table offsets move.
- A control archive rewritten with the member's *original* bytes starts normally (position 93 in
  147 s), so the rewrite itself is sound.
- An archive carrying an injected statement **executed it**: the probe created its own log file
  145 s after launch, and startup still completed.

**The output channel is the engine's own file operators.** `"name" "w" file`, then
`dup <string> writestring`, `dup carriage_return`, `closefile` — `writestring` is defined in
`gs/standard.gs`, which runs at position 4, so it is available to anything loaded later. This is how
`gs/logs/makelogs.gs` writes `combat.log`, and it is the cheapest way to get a value out of the
running interpreter without reading the screen.

A prototype writer lives in `spikes/asset-viewer/examples/mpq_replace.rs`. Promote it to a CLI flag
when the next experiment needs it. **Always back the archive up first and verify the restore by
checksum** — `artifacts/experiment-backups/` holds the manifest pattern used on 2026-09-16.

### Engine probe harness — this is the reusable part

**Run it with the two scripts, not by hand.** `scripts/install-engine-probe.sh` verifies both
archives against the recorded originals, injects the sprites and the generated hotkey, disables the
intro movies and checks every write reads back byte-identical.
`scripts/restore-game-archives.sh` collects the captures, restores, and prints the two hashes. A
worktree has no `artifacts/`, so set `LOM_ARTIFACTS_DIR` to the main checkout's copy when running
from one. The hotkey body itself is generated by `tools/engine_probe.py` and unit-tested.

**Four traps, each one having cost a run:**

- **`anythingat?` does not see terrain sprites.** A cell can read empty and still hold a village.
  Never clean up by location — `terrainspriteat` will happily hand back somebody else's building and
  `destroyterrainsprite` will delete it.
- **Do not clean up by sprite *type* either, unless the probe minted the type id itself.** A shipped
  type such as `terrainsprites /orchard get` is shared with every orchard the map generator placed,
  so a type-only sweep deletes all of them. This was caught in review rather than in the game, but it
  is the same defect as the village, only wider. **Require type *and* cell**, and reserve the
  type-only sweep for ids returned by `addterrainspritetype` during the same keypress.
- **The hotkey auto-repeats while the key is held.** One press ran a probe body nine times. Guard it
  with a flag in `userdict`.
- **`screencapture` writes R, G, B, not the BMP-standard B, G, R** — and its `bfOffBits` says 14
  against a real pixel offset of 54. Equality differencing is blind to the first, so it went
  unnoticed for several runs; any colour conclusion drawn through a standard reader is wrong. Use
  `tools/probe_captures.py`.
- **Capture the plate after a `rendermap refreshdirty`**, not before. A plate taken on the bare
  keypress is missing the terrain sprite layer, and the whole layer then shows up in the difference.

**Build a ladder, not a measurement.** The 2026-09-16 run placed one custom sprite that never
rendered, and nothing in the run could distinguish "the operator does not work", "the injected member
is unreadable" and "our edit broke the file". Placing a shipped control, an unmodified copy and the
edited subject in the same capture costs nothing extra and answers that directly.

- **Hotkey**: insert before the final `end` of `gs/hotkey.gs`:
  `ASCII_VAL"z"0 get{ ... }addhotkey`. Free keys: `e f g i j n o r u v w x z` and backtick. F1-F9 are
  taken by the game, F10-F12 by macOS.
- **Capture**: `"name.bmp" screencapture` writes a true 640x480 24-bit BMP.
  **Its `bfOffBits` says 14 while pixels start at 54** — compute the offset; ImageMagick rejects the
  file outright, so use the small reader in the research log's approach.
  **`screencapture` will not overwrite an existing file**, so a fixed filename captures exactly once.
  Use the counter idiom, which works:
  `dest{"shot"n".bmp"}build_statement_ns strcpy` then `dest screencapture`.
- **Fast startup**: set the `true{...}if` guarding `imptitle.smk`/`intro.smk` in `START.GS` to
  `false`. Over 150 s becomes about 6 s.
- **Live map with no user input**: `{}gamemodeproc gamemode 128 128 newmap default_edit_mode`.
- **Reaching a real game needs a human** — macOS blocks synthetic input to Wine, so menu navigation
  has to be asked for. The user has been willing; make each run count.
- **Check for the files a mechanism would produce before declaring it broken.** 36 captures sat
  unnoticed because only `shot1.bmp` was checked; a whole probe was rebuilt on that bad inference.
- **Live map with no user input**: `{}gamemodeproc gamemode 128 128 newmap default_edit_mode`, lifted
  from the Map Editor button in `gs/dlg/NEWDLG5.gs`. Renders a 128x128 map in about 8 s. Replacing
  `{newdlg opendialog}ifelse` with `{}ifelse` suppresses the main menu when a clean screen is wanted.
- **`refreshdirty` repaints dialogs over direct draws**, so settle the screen, capture a control, then
  draw and capture again and diff.
- **The map is live during a capture.** Unrelated interface elements animate between shots, so a
  plate difference can bound far more than the sprite. Cluster the changed pixels into connected
  components instead of taking a bounding box — that turned a spurious `110x191` box into the
  correct `49x67`.

### Read this before judging any launch

`START.GS` plays `smk/imptitle.smk` then `smk/intro.smk` with `MODAL playvideo`, **before** the 94
`run` statements. `intro.smk` is a multi-minute cinematic and how much of it plays varies run to run:
a measured pristine startup took over two minutes to reach position 93, while other runs took 12
seconds.

- **`combat.log` growing by 36 bytes is proof** that execution reached position 93.
- **`combat.log` not growing inside a window proves nothing.** Hang and "intro still playing" look
  identical, and a screenshot taken between the two movies shows a plain black window.
- Also wait for `lomse.exe` to actually disappear after `pkill`, plus a fixed delay, before
  relaunching. A 3-second wait left `wineserver` mid-shutdown and produced a launch failure that
  looked like a real result.

This trap produced a wrong conclusion once already during the loose-file work. Judge by the positive
signal or by a screenshot showing recognisable game content — never by a timeout.

### Permissions, as they now stand

The user granted standing permission to modify game files **conditional on a checksum-verified backup
and a verified restore**. Every run so far has restored `gs.mpq` and `imp.mpq` byte-identical, and
`artifacts/experiment-backups/gs5r3-20260916/MANIFEST.sha256` records the originals. Keep it that way:

- Use `Lords of Magic GS5R3.app`. **Never** touch the `Steambuild 32 64bit DXVK.app` baseline.
- Back up before the first modification of any archive, and verify the restore by hash afterwards.
- Remove probe output files (`*.bmp`, `*.log`) from the game directory when done.
- **Reaching a real game needs a human** — macOS blocks synthetic input to Wine, so menu navigation
  has to be requested. Make each run count.

## Practical pitfalls

- Archive path names are case-insensitive Windows names; some original members are anonymous until the public/internal listfiles are loaded.
- IMP compositing is keyed by palette **index**, not colour: slot 0 is transparency and slot 1 is the shadow. The green and red RGB values in those slots are incidental. Clean preview, mask, and raw modes still expose different evidence, and blending behavior is unresolved.
- Community material is a source of hypotheses, never specification. Check it against the corpus before writing it down; the survey refuted several claims, including two from the mod's own author about his own code. Where a claim turns out to describe unshipped code rather than a mistake, say so — it is both more accurate and fairer.
- **Jake posted replies to both forum threads on 2026-09-16** — thread 2590 and thread 2176 (the hotspot mechanism thread, answering ozz on the 19-name constant list, HSType reaching 9, and the shipped `aura.gs` question). Replies are awaited; a reply from **ozz** would be the highest-value input available, especially on the two files with out-of-vocabulary hotspot types. The repo is still private pending that exchange; it is hygiene-ready apart from six home-path lines. That reply cited a **19-name constant list, which we have since corrected** — only 11 are IMP hotspot types, over nine distinct values; see [hotspots](hotspots.md#hotspot-types). Worth correcting on the board rather than leaving it standing. The earlier 2590 post covered the 12 files that crash the community parser, their unchecked `rle_decode_1` run loop, the type-57 gap, and our map-header measurements.
- Mantera's site is **HTTP-only with no TLS listener**, so any fetcher that force-upgrades to HTTPS fails with `ECONNREFUSED`. The forum rate-limits automated requests with a proof-of-work challenge; read it politely and do not attempt to defeat it.
- Generated `.h` IMP metadata is strong ground truth but not perfect: ten paired disagreements and four public-catalog orphans remain explicit failures.
- The GameScript VM is deliberately bounded. A successful utility module does not imply a tractable full host/simulation API.
- Keep the working baseline and its saves untouched. Use independent mod profiles for gameplay tests.
