# Agent handoff — 2026-09-16 (engine operator table, arity, hotspot geometry)

## Outcome and next move

This is a working macOS setup plus a native Rust asset/MPQ viewer and an experimental GameScript interpreter, **not** a native playable replacement.

**[Issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1) is resolved, and sprite placement is now fully settled.** Every frame obeys

```
top_left = anchor + placement - (w >> 1, h >> 1)
```

so the stored pair is the vector from the anchor to the **centre** of the frame, in screen pixels with `+y` down, and it is **added**. Where that pair lives depends on the frame: with a zero hotspot count it is the origin pair in the record's `+8` dword; with hotspot records it is **record 0**, which is engine-reserved and unreadable from script — `getimphotspot` (`0x0049BF90`) and `enumimphotspots` (`0x0049C1D0`) both begin their walk at record 1. Both halves were measured in the running engine on 2026-09-16: four terrain sprites with differing origins for the first, and two copies of one unit sprite differing only by a `(+60, +40)` shift of record 0 across all 105 records for the second, which landed exactly `(+60, +40)` apart. **Caveat:** the record-0 run went through the terrain-sprite draw path, so a unit-specific constant in the *anchor* is not ruled out; the sign, the centre-relative form and the choice of record 0 are settled. `drawimpframe` is vestigial and must not be used. The writer is `--set-imp-placement` (add `--hotspot 0` for the record form) and `--imp-placement-for` solves for a re-cropped frame. What stays open is `map2screen`'s y/z convention.

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

As of 2026-09-16, `cargo test` passes **83 library and 11 CLI/viewer tests**, `python3 -m unittest discover -s tests` passes 21, and `cargo clippy --all-targets -- -D warnings` is clean. `--validate-imp` on the GS5R3 `imp.mpq` reports 1,798 pairs, 1,788 validated, 10 failures, 4 orphans — the known bounded mismatches. The full proprietary corpus was last scanned on 2026-09-12; rerun that scan on the user's installed GS5R3 archives before claiming a new corpus validation. The exact read-only inventory and viewer commands are in the [tool README](../spikes/asset-viewer/README.md). Current `build.rs` assumes Homebrew StormLib/SDL3 under `/opt/homebrew/opt`; portable discovery is still open.

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

## The issue #1 experiment — the hotspot is applied; the sign is still open

Everything the *files* can say about hotspots has been said. What remains on
[issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1) is how the engine
*consumes* a hotspot: the sign convention (is the sprite drawn at `position - hotspot` or
`position + hotspot`, and does `+y` mean up or down?) and how the shadow index is blended. Both live
in the drawing code, so no amount of file measurement settles them.

The plan, with current status:

1. ~~Confirm loose-file precedence.~~ **Done 2026-09-16, and it is refuted** — see below.
2. ~~Get a modified `.gs` into `gs.mpq` in a form the shipped `storm.dll` will read.~~ **Done.**
   StormLib writes an archive the engine reads, and an injected statement was observed executing.
3. **Attempted, blocked.** A `.gs` calling **`drawimpframe`** runs without error but draws **zero
   pixels**, in every context tried. See "Why step 3 stalled" below.
4. Still open: measure where the sprite lands. The offset between commanded and observed position
   *is* the sign convention; the shadow blend is visible in the same capture.

**Why `getimphotspot` alone is not the answer.** It returns the hotspot the engine read *from the
file* — the same number our decoder already reports. The convention lives in the consumer, not the
accessor. Querying it only confirms both sides read the same bytes. `drawimpframe` is the native
that matters.

**`drawimpframe` takes six operands, not five.** Read at `0x0049C500`: four arrive through the
inline pop sequence and two more through the shared pop helper at `0x0040ADB0`, which the arity walk
undercounts. The last operand popped — the *first* written in a script — is the IMP handle; a bad one
is rejected with `drawimpframe - no such imp`. Non-integer numbers in the interpreter are **8.8 fixed
point** (the coercion helper at `0x004026A0` multiplies by `256.0` and `0.00390625`).

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

### The experiment harness — use this, it makes runs cheap

All three live in `START.GS`, which we can replace via the injection path:

- **Disable the intro.** Change the `true{...}if` guarding `imptitle.smk`/`intro.smk` to `false`.
  Startup drops from **over 150 s to about 6 s**. Do this first in any probe.
- **Reach a live map view with no user input**:
  `{}gamemodeproc gamemode 128 128 newmap default_edit_mode` (lifted from the Map Editor button in
  `gs/dlg/NEWDLG5.gs`). Rendered 128x128 map in ~8 s. Replace `{newdlg opendialog}ifelse` with
  `{}ifelse` to suppress the main menu when a clean screen is wanted.
- **Capture pixels**: `"name.bmp" screencapture` writes a 640x480 24-bit BMP. **Its `bfOffBits` field
  says 14 but the pixels really start at 54** — compute the offset, do not trust the header.
  `refreshdirty` is needed to present anything, and it repaints dialogs over direct draws, so settle
  the screen, capture a control, then draw and capture again and diff.

### Where issue #1 actually stands

**Settled: the engine consumes the hotspot at draw time.** Across **48 true 640x480 engine captures**
of one stationary army, the banner's cloth right edge sits at `x = 323` and its top at `y = 155` in
*every* capture, while the left edge ranges `306..315` and the width `9..18`. The sprite grows
leftward from a pinned top-right corner. In `iface/orflagb.imp` the cloth starts flush with the frame
box's left edge (`cx0 = 0`) in twelve of fourteen facings, so a renderer ignoring the hotspot would
pin the **left** edge. It does not. The anchor is real and applied.

**Not settled: the sign.** `position - hotspot` versus `position + hotspot` needs each capture matched
to a specific frame, and the matching failed. Silhouette matching reached only ~0.45 IoU and selected
frames 98-103, which render as thin wisps unlike the on-screen banner. The observed width range
`9..18` matches no single facing. A per-facing anchor-constancy test contradicts itself: facing 0
favours `pos - hotspot`, facings 9 and 10 favour `pos + hotspot`. A provisional number favouring
`pos + hotspot` exists in the research log and is **explicitly dismissed** there — do not cite it.

**To finish:** identify the displayed frame without relying on shape. Either capture a complete
animation cycle and index frames by position in the cycle, or build a true background plate by
capturing the same tile with the army moved away, which gives the full opaque silhouette (pole
included) instead of a luminance-thresholded fragment.

`drawimpframe` remains **vestigial** — it type-checks its six operands, looks up the imp, and paints
nothing, with zero call sites in 1,471 scripts. Do not reach for it again.

### The next experiment, designed — a controlled sprite with a background plate

The last attempt failed on **frame identification**, not on capture quality. It used an uncontrolled
subject (an army banner whose sequence, facing and cycle position were all unknown) and segmented it
by luminance threshold against unknown terrain, which throws away the dark pole and is perturbed by
the player-colour remap. Fix both by choosing the subject and by subtracting a true background.

**The operators needed all exist and their idioms are confirmed in shipped scripts:**

| Operator | Idiom (from the corpus) |
| --- | --- |
| `addterrainspritetype` | `[ ... ]cvx addterrainspritetype` → returns a type id |
| `addterrainsprite` | `<x> <y> <typeid> addterrainsprite` |
| `terrainspriteat` | `<x> <y> terrainspriteat` → sprite handle |
| `getterrainspritelocation` | `<sprite> getterrainspritelocation` → packed location (`xy_to_x_y` unpacks) |
| `destroyterrainsprite` | `<sprite> destroyterrainsprite` |
| `map2screen` | `<x> <y> <z> map2screen` -> three floats; **top of stack is screen X**, then screen Y, then the `z = 0` ground screen Y. Verified statically 2026-09-16 |

**This experiment was run on 2026-09-16 and it worked.** The result is
`top_left = anchor + hotspot - (w >> 1, h >> 1)`, fitted exactly against four subjects at one cell;
the derivation, the measured table and one honest negative about `map2screen` are in the research log.
Two refinements over the design as written, both worth keeping for any future engine probe:

- **Use `terrainsprites`, not `addterrainspritetype`.** `tree2.gs` already defines the dictionary and
  every entry resolves to `imp/<name><zoom>.imp` with **one sequence, one facing, one frame**. So
  `terrainsprites /orchard get` is enough, and frame identification disappears rather than shrinking.
- **Place four subjects at one cell, not one subject at several cells.** A shared anchor cancels every
  unknown constant, so the result stops depending on `map2screen` — which turned out to matter,
  because `map2screen`'s y did not agree.

The procedure as run, all from one injected hotkey so it happens in a single frame with nothing else
moving:

1. `"plate.bmp" screencapture` — the tile with **no** sprite on it.
2. `x y typeid addterrainsprite`, then `rendermap refreshdirty`.
3. `"sprite.bmp" screencapture`.
4. `x y terrainspriteat destroyterrainsprite`, `rendermap refreshdirty` — leaves the map as found.

`sprite.bmp - plate.bmp` is then the sprite's **exact opaque silhouette**, dark pole included, with no
threshold and no palette assumptions. Because we chose the sprite type, its IMP and frame are known,
so its decoded hotspot is known. The remaining unknown is the commanded position, which comes from
`getterrainspritelocation` plus `map2screen` — log both through the file operators.

`observed_top_left - commanded_screen_position` is then the answer, and its sign is the result.

**Why this is strictly better than what was tried:**

- No frame identification at all — we place a sprite whose frame we chose.
- Exact silhouette, so the measured box is the real frame box rather than a bright fragment.
- Repeatable at several map cells in one run, which turns a single reading into a fitted line and
  exposes any constant offset in `map2screen`.
- Self-cleaning: the sprite is destroyed, so the save is untouched.

**Fallbacks if terrain sprites prove awkward:**

1. **Plate by displacement.** Keep the army subject but ask the user to move it one tile, capture the
   vacated tile as the plate, and difference against an earlier capture of the same tile. Recovers the
   exact silhouette without any new operators.
2. **Index frames by cycle rather than shape.** Hold the capture key so auto-repeat samples densely,
   then recover the animation period from the repeating sequence of silhouettes and index frames by
   position in the cycle instead of recognising them.

**`map2screen` is now pinned, statically — done 2026-09-16.** Disassembly of `0x0046B0C0` and the
camera transform at `0x00469AE0` gives the full contract; see the research log entry for the
derivation. The short version, and why it helps:

- Operands are `<x> <y> <z> map2screen` in written order, each accepted as int, 8.8 fixed or float.
- Results are three floats with **screen X on top**, then screen Y, then the same point re-projected
  at `z = 0` (a ground-level baseline, useful for shadows).
- The viewport is `640x384` centred at `(320, 192)`: `screen_x = ndc_x * 320 + 320` and
  `screen_y = 192 - ndc_y * 192`, from literals at `0x0054D7D4`/`0x0054D7D8`/`0x0054D7DC`.
- **World `+y` is up, screen `+y` is down.** State which space any hotspot sign result is in.
- **The camera scroll at `+0x1A4`/`+0x1A8` is already added** to screen X and Y (but not to the third
  result). So `observed_top_left - map2screen(cell)` is directly comparable to a screen capture with
  no separate scroll bookkeeping — this removes the largest error source in the measurement below.

That is two operators now whose real contract came from disassembly rather than the arity walk. The
walk finds candidates; it is not evidence about a contract, because it undercounts operators that pop
through the shared helper at `0x0040ADB0` — exactly how `drawimpframe` hid two operands. Read the
entry point before designing an experiment around an operator.

### Engine probe harness — this is the reusable part

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
