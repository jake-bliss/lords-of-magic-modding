# Agent handoff — 2026-09-16 (engine operator table, arity, hotspot geometry)

## Outcome and next move

This is a working macOS setup plus a native Rust asset/MPQ viewer and an experimental GameScript interpreter, **not** a native playable replacement.

**[Issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1) is resolved: sprite placement is fully
settled.** The rule, both storage forms, the reserved hotspot record, the type numbers and the writer commands all
live in **[hotspots](hotspots.md)**, which is the single source of truth — link to it rather than restating it.
Its "Still open" section lists what is left, and the next engine run should batch those: the shadow blend, the
palette channel order, and the unit-anchor caveat. `map2screen` is now fully closed, including the y convention:
the drawn top is `output2 - 973.4` at slope 1, and the third input is the **interpolated mesh
height**, not `getelevation` — see [map projection](../tools/map_projection.py) and the research log.

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
| Stage 1 asset layer | 9,804 core archive members classified; 1,377 PBMs decoded; 1,800 IMPs expanded; 26 tilesets and all 365 loose maps bounded; 21,117 map-object records parsed across six record layouts in 365 of 365 maps | IMP shadow blend and frame timing, palette channel order, 10 metadata mismatches + 4 catalog orphans, what the record layouts' constant tail words mean, reimport/repacking, portable packaging (IMP **placement** is solved — see [hotspots](hotspots.md)) |
| Stage 2 VM probe | 4,692 GameScript members tokenized; baseline/3.02/GS5R3 vocabularies inventoried; 3.02 `standard.gs` loads and script-defined `min`/`max` execute | Host API, simulation, save semantics, complete module loader, playable engine |

Detailed facts and confidence labels live in [Stage 1](native-asset-stage.md), [map format](map-format.md), [GameScript probe](gamescript-format.md), and the [engine candidate plan](native-engine-plan.md). Do not treat Stage 1 coverage as evidence that Stage 2–5 time estimates are reduced proportionally.

## Reproduce before changing code

```sh
cd ~/personal-projects/lords-of-magic-modding/spikes/asset-viewer
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

As of 2026-09-17, `cargo test` passes **164 library and 54 CLI/viewer tests**, `python3 -m unittest discover -s tests` passes **157**, and `cargo clippy --all-targets -- -D warnings` is clean. `--validate-imp` on the GS5R3 `imp.mpq` reports 1,800 pairs, 1,795 validated, 5 value-pinned exceptions, 2 value-pinned orphan notes, 0 failures. `--map-roundtrip` on the installed `map/` directory reports 365 checked, 365 byte-identical, **21,117** records rebuilt from their typed fields, 0 failures — all six placed-object record layouts, which is also why `--map-place-sprite` and `--map-remove-sprite` now work on all 365 maps rather than 196. The `mapload` probe **ran on 2026-09-17 and all seven rungs passed**: the engine loads maps this project wrote, including edited, sprite-placed, created-from-nothing and **non-square** maps, and the two created-from-nothing maps re-saved byte-identically. It also settled the header word at `0x00`, showed tag bit `0x00800000` is not durable map data, and recovered `setterrain`'s transition ring for one background. See [engine acceptance](map-format.md#engine-acceptance-measured) and [the run sheet](mapload-run-sheet.md). The `terrainrings` probe **also ran on 2026-09-17**: `setterrain`'s transition ring is one offset table plus a per-background anchor (the anchors sit 48 apart, which is **not** a claim that the atlas is partitioned into 48-tile blocks — see the section for what that does and does not establish), `resetvisibility` is what clears cell bit `0x00800000`, and the engine's `terrainsprites` dict was dumped — 178 name-to-id pairs, so `--map-place-sprite` takes a name. That measurement is now understood as a **lookup into data the game ships**: the `.til` tilesets in `pic.mpq` declare, per atlas slot, the cell's own terrain and a constraint on all eight neighbours, and `src/tile.rs` had been parsing those files while discarding the eight constraint columns. `--map-paint-terrain IN X0 Y0 X1 Y1 TERRAIN OUT TILESET.til` now re-selects every disturbed cell from those constraints, which reproduces the engine on 2,084 of 2,084 deterministic cells in the captures and **works on shipped world maps**, where the previous version refused everywhere. It refuses a *partial* road or impassable rectangle -- which is the old ragged-road measurement re-derived from data -- an undeclared tile, and a missing tileset; a **newly painted** interior is a random draw the engine makes and no writer can reproduce, while a cell that already holds a valid tile keeps it and is reproducible. The map edge is read **closed**, without which a whole-map water paint writes a phantom coastline. The direction convention was **derived** against the saved captures (576/576 for the geometric reading, 0/576 mirrored), not chosen. Two limits found on the way: shipped world maps are only 94% consistent with their own tileset, and the 337 `.smp` battle maps do not use `tilesb01.til` at all -- which tileset they use is unidentified. See [the transition rings](map-format.md#setterrain-transition-tiles-one-offset-table-one-anchor-per-background), [the tileset blend rule](map-format.md#the-tileset-declares-the-whole-blend-rule) and [the sprite table](map-format.md#the-terrain-sprite-type-table). The full proprietary corpus was last scanned on 2026-09-12; rerun that scan on the user's installed GS5R3 archives before claiming a new corpus validation. The exact read-only inventory and viewer commands are in the [tool README](../spikes/asset-viewer/README.md). Current `build.rs` assumes Homebrew StormLib/SDL3 under `/opt/homebrew/opt`; portable discovery is still open.

## Open work and safe order

1. **[#1 IMP presentation](https://github.com/jake-bliss/lords-of-magic-modding/issues/1) — closed.** See [hotspots](hotspots.md); the residual questions are listed there, not here.
2. [#5 GameScript VM](https://github.com/jake-bliss/lords-of-magic-modding/issues/5): **mostly resolved.** The engine's operator tables are recovered (1,906 natives with entry points), arity is recovered by disassembly, and the VM classifies any name it stops on as operator / engine-constant / unresolved with a signature. What remains is loading a *second* engine-light module end to end.
3. **[#22 Oversized map header](https://github.com/jake-bliss/lords-of-magic-modding/issues/22) — closed 2026-09-17, refuted.** A 512x512 map was generated in the running engine and parsed: the four-byte word at `0x00` is present and holds the same `0x6f` as the 128 and 256 controls. See [map format](map-format.md#oversized-maps-keep-the-header).
4. [#4 Map variants](https://github.com/jake-bliss/lords-of-magic-modding/issues/4): **the save-diff half is done** — the `maptag` probe ran attended on 2026-09-17 and settled the cell tag, the storage order, the terrain table, the section layout and several record fields (see [map format](map-format.md)). **The record-layout half is also done** — offline, 2026-09-17: there are six layouts (47, 48, 49, 52, 53 bytes, two with a footer), all 365 maps decode, and the "18 unmatched tails" were nine of each of the two new procedure-tail and plain-tail sizes rather than a separate phenomenon. Still open: the meaning of tag bit `0x00800000`, what the layouts' constant tail words mean (which this corpus cannot answer — each is constant within its layout), the trailing footer and why four layouts have none, and the attribute field at `+24`. The header word at `0x00` is now known to partition the six layouts exactly and in order, which reads as a format version — Observed correlation, Inferred meaning.
5. [#2 timing/direction](https://github.com/jake-bliss/lords-of-magic-modding/issues/2) and [#3 validation exceptions](https://github.com/jake-bliss/lords-of-magic-modding/issues/3): original-game comparison and lossless decoder exceptions.
6. [#6 Difficulty/AI gameplay proof](https://github.com/jake-bliss/lords-of-magic-modding/issues/6): static side closed; controlled gameplay measurement holding `insane_mode?` constant remains open.

**Two methodological traps, both paid for today. Read these before trusting any new measurement.**

1. **Validate on a representative sample, not a convenient one.** The arity walk scored 23/24 against PostScript primitives and was shipped as "1,875 of 1,908 walks well formed". That was wrong: the primitives are mostly direct-branch, while the 1,804 game operators dispatch through jump tables the walk cannot follow. It reported `getarmydata` as pushing nothing, with `well-formed` confidence. Corrected in PR #16 — the real figure is **118** complete walks. The accuracy claim survived; the completeness claim did not.
2. **Derive a label in one place.** Merging PR #19 after #16 left two copies of the confidence mapping and only one knew about `indirect-branch`, so the same operator was reported two different ways by two commands. Fixed in PR #20 by `StackEffect::confidence()`.

**Known soft spot, still open:** `GameScriptDocument::DEFINITION_WINDOW` is 3 tokens, chosen to fit the observed forms rather than derived. The `X /dummy <dict> replace bind def` closure idiom — 7,472 uses of `replace` in GS5R3 — is not fully modelled by it. If the candidate vocabulary ever looks wrong, suspect this first.

Newly available leads, all from the 2026-09-16 survey:

- The oversized-map hypothesis moved out of #4 into its own tracked issue, **#22**, and is now **closed and refuted** — the header survives at 512x512. Getting there took finding that the shipped editor generates 32-1024 itself, so no sample map from the board was ever needed.
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

**Five probes exist.** `LOM_PROBE=ladder` (the default) is the four-rung compositing diagnostic that
settled the shadow blend and the palette order. `LOM_PROBE=elevation` surveys `getelevation` beside
`map2screen` called with `z = 0` and with `z =` the cell's own elevation, then places six sprites to
measure real anchors — that is the open half of the y convention. `LOM_PROBE=mapsize` generates and
saves a 128, a 256 and a 512 map for [issue #22](https://github.com/jake-bliss/lords-of-magic-modding/issues/22);
it places no sprites, destroys nothing, and is the only probe run from the **Map Editor** rather than
from a game. `LOM_PROBE=flatground` builds its own mesh to settle `map2screen`'s y convention.
`LOM_PROBE=maptag` builds a 64x64 map, writes chosen tiles, terrain types and sprites into it, and
saves it four ways for [issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4) —
**run 2026-09-17, and it refuted two documented claims**; see below.

**The mapsize probe writes outside the archives, and that needs care.** The game directory has a
loose `map/` folder holding 366 shipped files, and **no backup here covers it** — the manifest covers
`gs.mpq` and `imp.mpq` only.

Both scripts therefore work from an **exact list of names**, `engine_probe.generated_map_names()`,
and never from a glob. `map/zz*.scn` would have been a standing offer to delete somebody's own
`zzCustom.scn`, with nothing to restore it from. One source of truth means the cleanup list cannot
drift from what the probe writes, and a test asserts that every `map/` name in a probe body is on
that list — which is stricter than "starts with `z`", because `map/zURAK.scn` would satisfy the
loose rule and then never be cleaned up at all.

The restore script hashes each generated map **before** copying it and compares after. Comparing a
copy against the file it was just copied from proves nothing, and the original is about to be
deleted. If a probe ever needs to write elsewhere under `map/`, back that directory up first.

**Where `make_custom_random_map` comes from.** `START.GS:106` runs `gs\edit\TERREDIT5.gs`, whose
first lines are `userdict begin "gs/rmg.gs"run "gs/edit/mapgen.gs"run end`. So the generator is
defined in **userdict** at startup and is reachable from a hotkey anywhere, editor or not. The probe
does not call `clearmap` first — the editor does, but only to blank a map it already has loaded, and
`make_random_map` opens with `mw mh newmap` itself. Requiring a map to already exist would be a
precondition the probe does not need.

Its ladder is 128, 256, then 512, in that order and for a reason: 128 and 256 both exist in the
shipped corpus, so a generated one can be compared against a file the game itself wrote. If the
generated 128 does not match, the generator is not a faithful writer and nothing the 512 says can be
trusted. The control runs first; a test enforces the order.

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
- **Map locations are packed, not coordinate pairs.** `anythinglocation` and
  `getterrainspritelocation` each return one `y * map_width + x` value, and `findemptylocation`
  takes `(location, unittype)`. Reading one as a pair underflows the operand stack and the probe
  body dies silently part-way through. **Get operand order from shipped call sites, never from the
  recovered arity table** — the table undercounts, and this cost a whole attended run.
- **Clear `z*.bmp` and `zprobe.log` from the game directory before every run.** `screencapture`
  refuses to overwrite, so a stale capture survives and gets collected as if it were this run's
  output — and a plate diffed against itself reads exactly like "the sprite did not render".
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

### Build the map instead of surveying it — `LOM_PROBE=flatground`

Implemented 2026-09-17. This supersedes the "place sprites and survey the neighbourhood" approach
for the open `map2screen` y question.

The probe builds a 64x64 map with `newmap`, sets every elevation with `setelevation`, points the
camera with `centeron`, and photographs **the same six cells three times**:

| Phase | Mesh | What it gives |
| --- | --- | --- |
| A `flat` | every cell 0 | calibrates drawn y against output 2 with no z term |
| B `plateau` | a uniform block at 2.0 | cell elevation 2.0, neighbourhood also 2.0 |
| C `spike` | only the six cells at 2.0 | cell elevation 2.0, neighbourhood 0 |

**Run 2026-09-17, and it answered.** The renderer reads the interpolated mesh, not the cell: same
cell elevation of 2.0, uniform neighbourhood gave an effective height of 2.004 and a clamped 1.0-1.5
ring gave 1.395 — 12.6 pixels apart. The control sprite earned its place immediately: it showed the
camera moving 40 pixels between phases, which is also what proved `map2screen`'s output 2 already
contains the scroll. Results in the research log; the model is in `tools/map_projection.py`.

Two things for the next probe that places sprites. `setelevation` is **clamped against a slope
limit** — the spikes came back with a 1.0-1.5 ring rather than the 0 they were set to, which the
per-cell survey caught and which every number in the result depends on. And one placement, cell
(32, 32), the exact centre of the map, logged normally and drew nothing in all three phases; log
`enumterrainsprites` counts either side of each placement and that question answers itself.

**B against C is the experiment.** Identical `getelevation` at every placement cell, identical
screen x, and the only difference is what surrounds them. If the drawn y moves, the renderer reads
the mesh rather than the cell and the move measures it; if it does not, mesh interpolation is dead
and the missing term is something else. Either way it is an answer, which the earlier run could not
produce.

Three things the earlier run could not do, all free here: the neighbourhood is known before placing
rather than surveyed after; every placement has its own screen x, so no sprite is ever matched to a
cell by whichever assignment fits best; and the map is the probe's own creation, so nothing of the
game's is at risk at any point.

**A seventh sprite is the instrument that makes B-versus-C readable.** The camera cannot be moved
out of the plateau: framing the row pins `centeron` to within a cell or two of the row itself, so
the camera cell is inside any block that covers the placements with a margin. If `centeron` derives
its offset from terrain height the way the sprite renderer may, the whole viewport shifts about 41
pixels between B and C, every sprite moves with it, and `392 clearmap` leaves a uniform texture with
no landmark to notice it — a camera artefact would read as the mesh coefficient, or cancel a real
one into a false null. So a control sprite stands at (35, 41), six cells clear of the block and flat
in all three phases. Any movement in **its** drawn y is purely camera, and is subtracted from the
rest.

Each phase takes its own plate, because phases B and C move the ground and a shared plate would put
the whole changed mesh into the sprite difference. `rebuild3dmap` runs after every elevation change
or the render keeps the old heights and all three phases look alike. Each phase sweeps **twice** and
then counts survivors into the log: destroying during an enumeration may advance past an entry, and
a survivor is the same art on the same cell in the next phase's plate *and* shot, so it contributes
zero changed pixels and reads exactly like "the sprite did not render".

The tests assert **order**, not the presence of strings: three `rebuild3dmap` calls say nothing if
all three run before the elevations change, and three sweeps say nothing if they all run at the end.
They enforce the phase order, that each phase plates before it places, that each phase rebuilds and
re-points between its own elevation change and its own plate, that every phase places on all seven
cells exactly once, that the block loops use the declared bounds, that every placement's *drawn
frame* — not just its anchor — is on screen once the camera offset is accounted for, and — as a
*sequence*, because a substring search cannot tell one block write from another — that the spike
phase lowers the plateau before raising its cells.

### Write the map, then read it back — `LOM_PROBE=maptag`

Run 2026-09-17, attended, and it produced the map-format results that
[map format](map-format.md) now carries. The shape worth reusing: **choose the values before the
run so that the candidate readings disagree on them.**

The probe builds a 64x64 map, `392 clearmap`s it, forces seven tile slots spanning the whole atlas
range along one row, `setterrain`s all eleven terrain types along another, saves as `.scn` and
`.smp`, places three terrain sprites of a type it minted itself, saves, destroys them, and saves
again. Four files, each a diff against its neighbour.

What that bought, all **Observed in gameplay**:

- the low tag bits *are* the tile slot, byte-exact at both ends of `0..623`;
- `0x00800000` is **not** a forced-texture flag — 4,096 forced cells, none flagged;
- cells are packed `y × width + x`; the documented X-major reading is **refuted**;
- `forcetexture` writes one cell, `setterrain` also blends the 8-neighbourhood;
- the engine's terrain-type-to-tile table, both directions;
- `savescenariomap` and `savespecialmap` write **identical bytes**, so the `.scn`/`.smp` record-size
  split is a content difference, not a format one;
- the trailing section is `count`, records, `footer`, in that order;
- `sprite_type` at `+28` and the sequential `instance_id` at `+20`.

**The three sprite cells are the whole coordinate result, and they were chosen for it.** `(20,30)`,
`(21,30)` and `(20,31)` give six distinct numbers under the two candidate encodings, so the saved
records could only match one. A tidier choice on the diagonal would have proved nothing. The
byte data still cannot say which operand is *x* — operand order and storage order are exact
transposes — so that came from the screen capture: `map2screen` puts screen-x on `(x − y)`, so a run
varying the first operand travels down-**right**, and both painted bands do.

**Why this was not caught years ago, and the lesson to carry:** every shipped map is square, so
X-major and y-major are indistinguishable across the entire corpus, and the regression test that was
supposed to prevent a transposed render had been fitted to a square 2x2 fixture. The Rust tests now
use deliberately **non-square** synthetic maps. A fixture shaped like the corpus cannot catch a bug
the corpus hides.

Read the four files back with `--dump-map-cells` and `--diff-maps` in the asset viewer rather than by
eye; a 16 KiB tail read by hand is how a wrong record size gets believed.

### Captures are per-probe and per-run, and this was paid for

`ladder` and `elevation` both wrote `zp0`/`zs1`/`zs2`, and `scripts/restore-game-archives.sh`
collected them into one flat directory. **On 2026-09-17 that destroyed the elevation run's survey
log** — the raw data behind the `map2screen` decode — when the mapsize run reused `zprobe.log`.
The conclusions survive in this log; the raw file does not.

Two fixes, both in place. Every probe now owns a capture prefix (`zl`, `ze`, `zm`, `zf`, `zg`) and a test
fails if two probes share a name. And the restore script files each run in its own
`run-YYYYmmdd-HHMMSS/` directory, refusing to start if that directory already exists. An attended
run costs a human a game session; its output is not something to overwrite.

### The shipped precedent it was built from

`gs\generate.gs` defines `/generate_simple_game`, which builds a complete playable scenario from
script with no user input: `64 64 newmap`, `clearmap`, `paintelevation` at chosen cells, `addcapitol`,
`addunit` inside `unittypedict begin ... end`, `2 newgame`, `create3dmap`, `gamemode`. Every operand
order is in [GameScript](gamescript-format.md).

Why it matters: the residual y error was measured against a shipped map whose terrain mesh we did not
choose and could only survey afterwards, and two of six placements fell outside the surveyed square.
A map painted by `paintelevation` is one we specify — flat where we want flat, a known step where we
want a step, unit at a cell we picked, each sprite at its own screen x. Build the control instead of
reconstructing it.

Related, from the same sweep: `mapw` and `maph` are readable script names giving the live map's
dimensions, so a packed location can be unpacked without assuming a width.

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
- IMP compositing is keyed by palette **index**, not colour: slot 0 is transparency and slot 1 is the shadow, and **slot 1 draws the background at half brightness** (measured 2026-09-17). The RGB in those slots really is ignored — rewriting slot 1 to magenta rendered identically to the untouched copy. Palette entries are stored **blue, red, green, pad**; the decoder's old reversal swapped red and green.
- Community material is a source of hypotheses, never specification. Check it against the corpus before writing it down; the survey refuted several claims, including two from the mod's own author about his own code. Where a claim turns out to describe unshipped code rather than a mistake, say so — it is both more accurate and fairer.
- **Jake posted replies to both forum threads on 2026-09-16** — thread 2590 and thread 2176 (the hotspot mechanism thread, answering ozz on the 19-name constant list, HSType reaching 9, and the shipped `aura.gs` question). Replies are awaited; a reply from **ozz** would be the highest-value input available, especially on the two files with out-of-vocabulary hotspot types. The repo is still private pending that exchange; it is hygiene-ready apart from six home-path lines. That reply cited a **19-name constant list, which we have since corrected** — only 11 are IMP hotspot types, over nine distinct values; see [hotspots](hotspots.md#hotspot-types). Worth correcting on the board rather than leaving it standing. The earlier 2590 post covered the 12 files that crash the community parser, their unchecked `rle_decode_1` run loop, the type-57 gap, and our map-header measurements.
- Mantera's site is **HTTP-only with no TLS listener**, so any fetcher that force-upgrades to HTTPS fails with `ECONNREFUSED`. The forum rate-limits automated requests with a proof-of-work challenge; read it politely and do not attempt to defeat it.
- Generated `.h` IMP metadata is strong ground truth but not perfect: ten paired disagreements and four public-catalog orphans remain explicit failures.
- The GameScript VM is deliberately bounded. A successful utility module does not imply a tractable full host/simulation API.
- Keep the working baseline and its saves untouched. Use independent mod profiles for gameplay tests.
