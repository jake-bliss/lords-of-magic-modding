# Research Log

## 2026-09-11 — Apple Silicon installation

### Environment

- M4 Max Mac
- macOS 26.5.2
- Porting Kit 6.7.0
- `Steambuild 32/64bit DXVK` Wineskin wrapper
- Steam App ID `404040`

### Installation findings

1. Native macOS Steam correctly showed ownership but would not download the Windows-only game.
2. Windows Steam inside Porting Kit successfully downloaded the complete depot.
3. The initial direct game launch exited during its legacy fullscreen transition.
4. Wine debug logs showed a failed display-mode change and invalid OpenGL framebuffer operation.
5. Installing `cnc-ddraw` and forcing its native `ddraw.dll` enabled a stable windowed launch.
6. `DirectDrawRenderer=gdi` and `UseGLSL=disabled` were retained as older Wine compatibility settings, while `cnc-ddraw` itself uses its OpenGL renderer.
7. Catmull-Rom scaling worked; Lanczos2-sharp was later selected for a crisper image.

### CD-ROM prompt investigation

Attempted fixes and outcomes:

| Attempt | Result |
| --- | --- |
| Start Steam's `LOMLauncher.exe` instead of `lomse.exe` | Did not remove prompt |
| Add `HKLM\Software\Sierra Online\Setup\LOMSE\CDPath` | Did not remove prompt |
| Add a Wine `D:` drive typed as CD-ROM | Did not remove prompt |
| Add CDPath under `HKLM\Software\Wow6432Node\...` | **Prompt removed** |

Conclusion: the 32-bit process was reading the redirected registry view. The Steam installation script's logical value was correct, but it was missing from the view visible to `lomse.exe` in this Wine configuration.

### Performance findings

- The native `cnc-ddraw` DLL was observed loaded from the game directory.
- Wine OpenGL and Apple's Metal-backed OpenGL driver were active.
- The process used approximately one CPU core continuously.
- The renderer was capped at 60 FPS; GPU performance was not a bottleneck.

## 2026-09-11 — Native archive and asset layer

### Archive inventory

- **Observed:** a native 64-bit Rust tool opened the five GS5R3 core MPQs read-only through a narrow StormLib wrapper.
- **Observed:** all 9,804 members were readable and content-classified with no probe failures.
- **Observed:** the corpus contains 1,377 IFF `FORM PBM` images, 1,800 IMP sprite binaries paired with 1,800 generated C headers, and 3,098 WAVE members.
- **Observed:** all 1,377 PBMs decode, including a ByteRun1 packet that crosses a scanline boundary.

### IMP format findings

- **Observed:** all 1,800 IMP binaries pass bounded table, palette, frame-reference, and packed-pixel decoding, including RLE expansion where applicable.
- **Observed:** the format contains 32-byte file headers, animation sequences/facings/frames, 256-entry BGRA palettes, six-byte padded hotspots, direct duplicate frames, and repeated facings.
- **Observed:** the custom RLE uses controls below `0x80` for repeated runs and controls at or above `0x80` for literal runs.
- **Inferred:** file-flag bits `0x30` select 8-, 1-, 2-, or 4-bit indexed storage, and sub-byte indices are packed most-significant-bit first.
- **Observed:** generated-header comparison matches 1,784 of 1,798 paired stems exactly. Fourteen bounded metadata disagreements and four orphan names remain explicit validation failures.
- **Unknown:** the remaining disagreements may represent additional frame-sharing rules or differences between generated statistics and runtime structures.

## 2026-09-11 — Community profiles

### 3.02

- Retrieved community 3.02 build 0014.3 from an archived Sierra Help copy.
- Inspected the archive and its included documentation.
- Confirmed that it replaces `gs.mpq` and configuration files, not `lomse.exe`.
- Installed it into a dedicated APFS clone with vanilla backups.
- Launch-tested successfully.

### GS5R3

- Retrieved the original GS5R3 and required PIC5R3 archives from ManTerA's surviving host.
- Verified both as RAR4 archives and inspected their contents.
- Confirmed that the payload is MPQ data, maps, documentation, and simple batch installers—no replacement executable.
- Installed GS5R3 and PIC5R3 into a second dedicated APFS clone.
- Copied all twelve supplied custom maps into the game's `map/` directory.
- Launch-tested successfully.

## 2026-09-12 — IMP presentation channels

- **Observed:** native SDL3 presentation produces recognizable 8-bit unit art and stable frame scaling.
- **Observed:** one inspected creature frame uses green palette index 0 for 8,576 background pixels and a separate pure-red index for a 1,651-pixel silhouette beneath the creature.
- **Observed:** an inspected 1-bit aura asset uses bright green and red as its two palette colors, confirming that blindly deleting both colors destroys meaningful mask data.
- **Inferred:** green is a background channel in the inspected frames, while red is a separate shadow, translucency, recoloring, or other compositor input.
- **Implemented:** the viewer defaults to a clean preview while `C` facings through visible-mask and untouched raw-palette modes. The decoder itself preserves every source index and palette color.
- **Corrected:** sampling the top-left pixel as a chroma key failed when frame 156 of `chcr5a.imp` touched that corner and removed gold artwork. Viewer channels are now selected by palette index 0 (background) and index 1 (secondary mask), preserving the same colors when they occur at other indices.
- **Unknown:** the original engine's exact mask blend, origins, hotspot behavior, sequence boundaries, and timing still require controlled comparison.

## 2026-09-12 — IMP actions, facings, export, and FFI safety

- **Observed:** all 1,800 IMP binaries expose bounded sequence-to-facing and facing-to-frame ranges under the current parser.
- **Observed:** generated C-header labels cover 4,649 of 4,666 declared sequence slots; aliases are retained, and 1,799 of 1,800 headers provide at least one label.
- **Observed:** `units\imp\chcr5a.imp` contains seven named actions, five facings per action, and 170 logical frames. The action names are `MOVE`, `STAND`, `DEFEND`, `GET_HIT`, `DIE`, `CORPSE`, and `MELEE_ATTACK`.
- **Inferred:** the five facings in each inspected creature action are directional views. Raw sequence and facing metadata remain preserved but uninterpreted.
- **Implemented:** the viewer navigates frames within a facing, facings within an action, and named actions; autoplay wraps within the selected facing.
- **Implemented:** the CLI exports a resolved logical frame as an 8-bit indexed PNG while preserving palette indices and RGB palette entries. Synthetic decode-back verifies exact bytes, and output creation refuses overwrite.
- **Corrected:** the manual StormLib binding used Windows' 260-byte `MAX_PATH` inside `SFILE_FIND_DATA`, but StormLib's macOS portability header uses 1,024 bytes. Enumeration consequently overwrote adjacent stack memory. The binding now selects the platform ABI, a layout regression test matches the installed C header's 1,064-byte structure, and the full five-archive scan still succeeds.
- **Observed:** after the ABI correction, a real frame-155 export reports the requested index and is independently identified as a 165×127, 8-bit indexed PNG.

## 2026-09-12 — IMP placement records and native map grid

- **Corrected:** frame flag `0x04` is a shared-pixel reference even when it occurs after the first record in a facing. Applying it consistently removed 27 false origin records and increased exact generated-header matches from 1,784 to 1,788 of 1,798 pairs.
- **Observed:** the remaining IMP corpus contains 15,725 logical origin records and 64,432 six-byte hotspots across 28,771 frames. Origin ranges are X `-66..70`, Y `-207..77`; hotspot ranges are X `-115..123`, Y `-232..86`.
- **Inferred:** hotspot records are `u16 id, i16 x, i16 y`. All six bytes remain preserved while original-engine placement behavior is tracked in issue #1.
- **Corrected:** generated-header action aliases can share a sequence number (`MOVE` and `STAND` in `aicr2a.h`), so the parser now retains every alias instead of overwriting the earlier name.
- **Observed:** the installed profile contains 20 `.scn`, 337 `.smp`, and eight `.lgd` files; all 365 share a bounded 16-byte header and `width × height × 8` cell grid.
- **Observed:** every candidate second cell word is a finite little-endian float from 0 to 20. Its grayscale view produces coherent geographic relief for `URAK.scn`.
- **Inferred:** the second cell word is elevation. The first word is likely a terrain tile identifier plus possible flags.
- **Observed:** trailing data falls into candidate 49-, 52-, and 53-byte record families, with 18 unknown layouts. Field decoding is parked in issue #4.

## 2026-09-12 — Terrain atlas and placed-sprite records

- **Observed:** all 26 recovered `.til` members parse as text definitions of an LBM atlas, grid dimensions, 32×32 tiles, terrain types, and tile-to-terrain relationships.
- **Observed:** `tilesb01.til` declares a 16×39 atlas with 624 slots. After masking `0x00800000`, every map-cell tag falls in `0..623`; 603 distinct indices occur across 1,258,496 cells.
- **Observed:** indexing `tilesb01.lbm` with `URAK.scn` produces a coherent, correctly oriented map with connected ocean, snow, forest/grass, and desert regions.
- **Observed:** map cells are X-major (`index = x × height + y`). This agrees with placed-sprite cell coordinates and Map Editor script iteration; the diagnostic viewer's former transposition is corrected and regression-tested.
- **Inferred:** tag bit `0x00800000` is the binary form of the editor's `forcetexture` operation. It occurs only in `.smp` files, and many 48×48 components flag exactly their 188 perimeter cells.
- **Observed:** the exact 49-byte tail family covers 196 files and 16,628 records. Every record has bounded, per-file-unique cell and instance identifiers; fixed words are stable across the full corpus.
- **Inferred:** record offsets `+20`, `+28`, and `+34` are instance ID, terrain-sprite type, and procedure ID. Editor script names and default-world coordinate correlations support these names, but controlled editor save diffs are still required.
- **Unknown:** the record attribute nibble and footer; header-to-tileset selection; 52-/53-byte record layouts; and exact runtime behavior of candidate sprite/procedure fields. These remain parked in issue #4.
- **Implemented:** the native tool describes 49-byte records, displays original-art terrain, and exports a non-overwriting RGBA map preview. A real `URAK.scn` export is 1024×1024.

## 2026-09-12 — GameScript lexical and vocabulary probe

- **Observed:** a bounded Rust lexer tokenizes all 1,315 baseline, 1,681 community-3.02, and 1,696 GS5R3 `.gs` members with zero fatal failures.
- **Observed:** the three corpora contain 557,649, 633,525, and 1,149,348 tokens respectively; maximum procedure nesting is 19, 19, and 20.
- **Corrected:** square brackets and `<< >>` cannot be validated as balanced source containers because they are runtime operators in shipped scripts. Backslash is an ordinary string byte rather than a C-style escape.
- **Observed:** four baseline, six 3.02, and one GS5R3 unmatched-procedure diagnostics are confined to a few shipped fragments and are reported without discarding the rest of each corpus.
- **Observed:** static adjacent-string `run` references total 650, 789, and 1,081 edges. GS5R3's large unresolved set is dominated by an optional dungeon-path catalog and is not treated as a load failure.
- **Inferred:** exact matches between executable script names and printable strings in the corresponding `lomse.exe`, after excluding literal definitions, yield about 2,100 native-host candidates per profile. This deliberately over-approximates the built-in surface.
- **Decision:** proceed to a bounded stack/dictionary interpreter for utility code; do not revise the full preservation-engine estimate until host calls are classified and representative traces execute.
- **Implemented:** the first bounded interpreter supports core values, operand/dictionary stacks, definitions, procedures, arrays/dictionaries, conditionals, arithmetic/comparison, a step limit, and structured unknown-name call traces.
- **Observed:** the shipped 3.02 `gs\standard.gs` loads in 339 VM steps, leaves an empty operand stack, defines 36 names, and its `min`/`max` procedures return `3`/`5` for the probe expression `3 5 min 3 5 max`.

## 2026-09-16 — Community research survey and corpus cross-check

- **Documented:** Mantera's fan site and the `impz.proboards.com` LOMSE Modding board are both live;
  the board has 169 threads and tool releases as recent as July 2026. Sources and per-claim verdicts
  are in [community research](community-research.md).
- **Documented:** a 2011 community IMP specification agrees with our header offsets, record sizes,
  and RLE algorithm **exactly**, including the `control + 3` positive-branch bias.
- **Corrected:** palette index 1 is the **shadow**, and compositing is keyed by palette index rather
  than by colour. This retires the "not a single universal chroma key" framing and resolves the
  secondary-mask question. The viewer's `secondary_mask` is renamed `shadow`.
- **Corrected:** exported indexed PNGs never wrote a `tRNS` chunk, so index-0 transparency was
  silently lost on export although the interactive viewer honoured it. Fixed and covered by a test.
- **Observed:** validating the generated header's "Duplicate bitmaps found" statistic against a
  `0x08`-only back-reference count raises corpus failures from 10 to 112. That statistic counts
  `0x04` shared-pixel frames too, so our existing conflation is correct. Hypothesis refuted; the
  separate tally is retained as `back_reference_frame_count`.
- **Refuted:** the mod author's claim that `extra_strong?` controls difficulty-scaled AI bonuses. The
  shipped body in `gs\scenario\default.gs` is `[false false false]getdifficultylevel get` — false on
  every difficulty — and the name appears nowhere in `gs\LEVLMODS5.gs`, which gates on `insane_mode?`.
  The phenomenon is real; the named control is vestigial.
- **Refuted:** that a definition terminates with `;`. Definitions end with `def`; `;` is the line
  comment. Our lexer was already correct.
- **Refuted:** that `gs5_globals.gs` supplements EXE variables in GS5R3. It ships, but its `run` is
  commented out at `START.GS:34`.
- **Observed:** `gs\artifact\_custom\misc\ai_stat_bonus.gs` is a second, live difficulty-to-AI-stat
  path in GS5R3 that no community source mentions.
- **Observed:** roughly 30 native host names are now confirmed rather than merely candidates, by
  intersecting never-script-defined executables with `lomse.exe` strings.
- **Corrected:** a name appearing as `/literal` does not prove it is script-defined — `/invoke_spell
  cvx` defers a native call — so the issue #5 classifier must match definition **shape**.
- **Documented:** a 2026 community toolchain already reads MPQ, IMP, and map files and claims to
  repair the map/save corruption we have not yet characterised. We are not the only party decoding
  these formats.
- **Observed:** eight community maps parse with zero failures, including a **160x160** scenario whose
  dimension appears in no installed profile and outside the previously documented 32/48/64/128/256
  set. The parser accepted it unchanged, so its bounds are data-driven rather than fitted.
- **Refuted:** that the unknown 4-byte map field at offset `0x00` is a version number. It takes 20+
  distinct values in `0x3f`-`0x6f` across 365 installed maps, is independent of dimensions and record
  counts, and clusters by file family. A tileset or terrain-set selector is the better hypothesis,
  which would also close the separate header-to-tileset unknown.
- **Observed:** the community `impstudio.py` round-trips all 1,800 IMP members byte-identically, which
  independently confirms the shared header byte layout. The test re-packs values over a copy of the
  original bytes, so it proves layout, not semantics.
- **Observed:** the same tool decodes 37,666 of 41,142 non-duplicate frames (91.6%). It reaches 100%
  on the three types it implements — exact agreement with our decoder — and 0% on type 57 (4-bit RLE,
  188 files, 3,388 frames) plus 12 files that crash its parser. Our decoder handles all 1,800.
- **Decision:** treat all community material as hypotheses with named sources. Two headline claims by
  the mod's own author about his own code were wrong; both would have propagated into our docs
  unchecked.

## 2026-09-16 — Facing rename, definition-shape classifier, and native stubs

- **Corrected:** IMP "cycles" are renamed **facings** throughout the code, docs, and
  `--describe-imp` output, matching the community specification and what the records actually are.
  Behaviour is unchanged.
- **Corrected:** the native-candidate heuristic excluded any name appearing as a `/literal`
  anywhere, which hid genuine host calls because the corpus defers native calls by pushing the name
  (`/invoke_spell cvx`). It now requires a definition **shape**.
- **Observed:** GS5R3 has 17,641 distinct literal names but only 13,609 definitions — 4,032 literals
  are not definitions. The native-candidate count rises from 2,091 to 2,151.
- **Implemented:** native host stubs (`--stub NAME=VALUE`) for pure state reads, with call counts as
  classification evidence, plus structured unknown-name traces carrying the name, VM step, and call
  stack at the point of failure. Unknown names still stop execution rather than being guessed.
- **Observed:** the real GS5R3 difficulty idiom `[25 50 75]getdifficultylevel get` executes and
  returns 25, 50, and 75 for Easy, Medium, and Hard.
- **Observed:** the shipped `extra_strong?` body evaluates to `false` at every difficulty and in both
  multiplayer states; the body quoted on the forum evaluates to `true` on Hard in single-player and
  `false` in multiplayer. The author's description matched code that did not ship. This upgrades the
  difficulty finding from a reading of the source to an execution under declared inputs.

## 2026-09-16 — Sprite hotspot mechanism, from board thread 2176

- **Documented:** `lomut` never writes hotspot data. Recompiled IMPs come back with `HSType = 0`, so
  the engine reads a packed XY displacement from a field holding a stale pointer. This, not frame
  sizing, is the root cause of the "512x512 hotspot" problem, and of the wobble, health-bar and
  depth-sorting symptoms that chased each other for nine years on the board. Source: ozz, 2023.
- **Documented:** snv's `imp.c` hotspot struct hardcodes two hotspot sets; the count is variable.
  Every tool ported from that source inherits the defect.
- **Corrected:** frame byte `+1` is a **count** of hotspot records, not a type tag. Our reading was
  already the count; the open question recorded in community research is now closed.
- **Observed:** across all 1,800 IMP members, hotspot records per frame range from 2 (24,412 frames)
  to 9 (10 frames). `imp.c` assumes 2; ozz had observed a maximum of 5.
- **Observed:** `lomse.exe` defines 19 `*_HOTSPOT` constants, not the 10 quoted on the board from a
  code comment. The extra names account for the observed types 10 and 16. The engine also exports
  the natives `getimphotspot` and `enumimphotspots`, so hotspots are reachable from GameScript.
- **Observed:** shipped `gs\aura.gs` carries no `;` comments at all, but does use `NO_HOTSPOT`,
  `SPELL_TARGET_HOTSPOT`, `SPELL_ORIGIN1_HOTSPOT` and `SPELL_ORIGIN2_HOTSPOT` as live identifiers
  passed to `addauratype`. The board's list is genuine engine vocabulary quoted from a comment the
  shipped scripts do not contain.
- **Observed:** sequence-record byte 1 is a mirror flag. Across 4,629 sequences, no unmirrored
  sequence has more than two facings, and all 28 of the 33-facing sequences are flagged mirrored,
  matching the account of arrows storing 33 facings and mirroring to 64 directions. Byte 3 is `0x01`
  in 4,623 of 4,629; byte 4 is `0xff` in all 4,629.
- **Unknown:** `units\imp\eacr5a.imp` uses hotspot types 106, 136, 138 and 143 on all 110 frames
  and carries neither type 0 nor type 7, which every other unit has. `units\imp\aiwm1b.imp` uses
  type 190 on 25 frames. An independent hex parse confirms the bytes are genuinely present and
  regular, and both files pass `--validate-imp`, so this is not a decoder defect. The meaning of the
  values is open.

## 2026-09-16 — The engine's GameScript operator tables

- **Observed:** `lomse.exe` registers native operators in two tables of eight-byte
  `(name pointer, implementation pointer)` records: 104 interpreter primitives at file offset
  `0x15bd20` and 1,804 game operators at `0x15f120`. 1,908 records, 1,906 distinct names, each with
  an entry-point address. This is the host API itself rather than a bound on it.
- **Observed:** reconciling the 2,151-name candidate vocabulary against the tables gives 1,445
  confirmed operators, 671 SCREAMING_CASE engine constants, and a 35-name remainder.
- **Inferred:** SCREAMING_CASE candidates are constants pushed by name, not operators, so their
  absence from the tables is structural rather than an error. That accounts for 95% of the names the
  tables do not confirm.
- **Corrected:** the candidate heuristic's documented risk of "false positives from unrelated binary
  strings" is now measured instead of assumed. Its real error bar is the 35-name remainder — 19
  `Type_*` engine type tags plus 16 short, low-use names that look like dictionary keys the
  definition-shape classifier misses. That is roughly 0.7% of candidates, and it is a precision limit
  of the classifier rather than engine surface.
- **Observed:** 465 operators are never called by any GS5R3 script, including `addfollower`,
  `addbuilding`, `aimedattack`, `animatearmy` and `addspelleffect`. This is engine capability the
  shipped mod does not reach.
- **Observed:** 83 diagnostic strings of the form `operator - message` cover 57 operators and name
  their parameters (`data_id`, `unit_num`, `player reference`, `owner`, `location`, `num_units`,
  `drawn_unit`), giving a partial field vocabulary for the core data accessors without running the
  game.
- **Rejected:** comparing the tables against the raw called-but-never-defined set (5,548 names)
  rather than the candidate vocabulary. That set is dominated by names whose definition site the
  classifier does not recognise, so the comparison measures classifier recall, not engine surface.
  The scan reuses the established candidate rule instead.

## 2026-09-16 — VM stops are now classified against the operator tables

- **Implemented:** `--probe-gamescript --exe` classifies any name the VM stops on as `operator`
  (reporting its entry point), `engine-constant`, or `unresolved`, and prints the corresponding
  remedy. Verified on all three paths: `getdifficultylevel` resolves to an operator at `0x00485e90`,
  `SD_MANA` to a constant, `give_level_exp` to unresolved.
- **Unknown:** the `unresolved` class does not distinguish "our definition-shape classifier missed a
  definition" from "the definition is in a module this run has not loaded". During single-module runs
  the second is far more common, so the class must not be read as evidence of engine surface.

## 2026-09-16 — Operator arity recovered by disassembly

- **Observed:** the interpreter context carries the operand array at `+0x50`, a stack index at
  `+0x54` that counts **down** as values are pushed, and a limit at `+0x58`. A pop increments the
  index and stores it back; a push decrements it and stores it back. Entries are eight-byte
  `(tag, value)` pairs.
- **Observed:** `0x0041d1d0` is the shared push-one-operand helper, taking `(tag, value)` with the
  context in `ecx`. Most operators push their result through it rather than inline.
- **Implemented:** `--scan-natives` walks each operator from its entry point with `iced-x86` and
  reports `pops`, `pushes` and a confidence column.
- **Corrected:** the first version of that walk reported 1,875 of 1,908 walks as well formed. It
  treated an unfollowable computed jump as an ordinary end of block, so walks that had silently
  given up counted as complete. `getarmydata` was reported `well-formed` with a missing push. With
  indirect branches detected and inherited from followed callees, the real figures are **118 well
  formed, 1,781 stopped at a computed jump, 7 with an unexplained store**. Most operators dispatch
  on operand type through a jump table, so the low figure is the honest one.
- **Observed:** the completeness flag is conservative, not a predictor of error. All 24 validation
  operators are flagged `indirect-branch` and 23 of them are still correct.
- **Observed:** measured against 24 operators whose arity follows from PostScript semantics,
  **23 agree**.
- **Corrected, three times, each by a known answer disagreeing:** the adjustment is not adjacent to
  the commit (the shipped `pop` interleaves an error-slot store); the compiler also spells the
  adjustment `lea ecx,[eax+1]`, which made the comparison operators look one-operand; and results
  are usually pushed by a helper, which made `add` and `sub` look like they pushed nothing. Each fix
  is structural rather than special-cased.
- **Unknown:** `mul` is reported as pushing twice. It has two push sites on mutually exclusive type
  paths — the helper at `0x004cae73` and an inline commit at `0x004cae98` — each pushing one result.
  The counts are therefore **site counts**, equal to arity only when every commit lies on one path,
  and a sound upper bound otherwise.

## 2026-09-16 — Operator table order is meaningful, but not along the obvious axis

- **Observed:** adjacent entries in the engine's operator table share callers far more than chance.
  Mean caller-set Jaccard is 0.3145 against a shuffled baseline of 0.0133, a **23.6x** ratio, and
  58.5% of adjacent pairs share at least one calling script against 10.5% for random pairs.
- **Observed:** adjacent entries share a name stem 12.87% of the time against a shuffled baseline of
  0.021% — a **611x** ratio. Eleven runs of three or more consecutive same-stem operators exist on
  exact stem equality alone, which is a conservative floor.
- **Refuted:** that the subsystem label can be read from where the calling scripts live. Mean run
  length of dominant caller directory is 1.45 against a shuffled 1.10, only **1.33x**. GS5R3's
  script tree is organised for the mod's authors and two directories dominate it, so the label cuts
  across the engine's seams rather than along them. Inferring subsystems that way yields a
  plausible map with no way to tell where it is wrong.
- **Implemented:** `tools/operator_groups.py` performs all three comparisons against shuffled
  baselines. It tokenises with the project lexer, so a name appearing only inside a `;` comment is
  not counted as a call site.

## 2026-09-16 — The cursor hotspot is authored, not derived

- **Observed:** across all 28,447 unit frames carrying a type-0 `CURSOR_HOTSPOT`, fitting the
  hotspot against frame size leaves most of the spread standing. On x the slope against width is
  `-0.021` with a median of exactly 0 and the residual spread is 8.84 px against a raw 8.87 px — the
  fit explains essentially nothing. On y the slope against height is `-0.298` and the residual is
  10.03 px against a raw 14.55 px, about half the variance.
- **Inferred:** the anchor is per-frame authored data — where the artist placed that sprite's feet in
  that pose — with only a horizontal centring convention derivable from the frame box.
- **Refuted, with consequences:** that a cropped frame can have its anchor reconstructed by
  re-centring frames against each other. That is the board's standing workaround for the
  "512x512 hotspot" problem, and it explains mechanically why it kept producing wobble and a
  drifting health bar over nine years: it reconstructs a value that is not reconstructible. It also
  makes `lomut` omitting the hotspot array unrecoverable rather than merely inconvenient.
- **Observed:** the missile-target hotspot sits `(+2.0, -10.7)` from the cursor hotspot on average
  across 28,159 frames, so projectiles are aimed at the body rather than the feet.

## 2026-09-16 — Loose files on disk do not override archive members

The [pending issue #1 experiment](https://github.com/jake-bliss/lords-of-magic-modding/issues/1)
rested on a hypothesis recorded the same day: that GS5R3 ships two `.gs` files and one `.lbm` loose
on disk which differ from their `gs.mpq` namesakes, and that the engine therefore prefers disk over
archive. It was labelled strong evidence, not proof. It is now **refuted**, in the running game.

### What the loose files actually are

- **Observed:** GS5R3's `START.GS` runs `gs/dlg/COMB_DLG5.gs`, not `gs/dlg/comb_dlg.gs`. The loose
  `comb_dlg.gs` is not named anywhere in the 1,471 extractable scripts, so it could never have been
  loaded regardless of precedence. It is a vanilla-era leftover.
- **Observed:** `gs/dlg/scroldlg.gs` *is* run, at position 53 of the 94 `run` statements in
  `START.GS`. That makes it the only usable probe of the two.
- **Observed:** the loose `scroldlg.gs` is the older vanilla text. It sets `1 1 finescrollpixels`
  where the archived copy sets `world_scrolling dup finescrollpixels`, and `START.GS` defines
  `/world_scrolling 8 def`. Had the loose copy ever won, GS5R3's world map would scroll at 1 px
  instead of 8.

### The control that makes the result readable

`gs/logs/makelogs.gs` runs at position **93** and appends `Lords of Magic has been launched.` to
`combat.log` on every start. Because 93 is after 53, a grown `combat.log` proves execution passed
the point where `scroldlg.gs` is loaded. Without that ordering fact, "no effect" and "never reached"
are indistinguishable, and the experiment says nothing.

### Three arms, `gs.mpq` untouched throughout

| Arm | Loose `gs/dlg/scroldlg.gs` | Reached position 93 | Probe fired |
| --- | --- | --- | --- |
| A | archived bytes **plus** a statement writing `precedence.log` | yes | **no** |
| B | not GameScript at all — a line of garbage | yes | n/a |
| C | the shipped vanilla file, restored | yes | n/a |

- **Observed:** in arm A the sentinel used the engine's own logging idiom
  (`"name" "w" file dup <string> writestring dup carriage_return closefile`, all reachable since
  `gs/standard.gs` runs at position 4). `precedence.log` was never created, while `combat.log` grew.
- **Observed:** in arm B a file that cannot parse as GameScript changed nothing. Startup completed
  normally.
- **Refuted:** loose-file precedence for `.gs` members. The engine reads `gs\dlg\scroldlg.gs` from
  `gs.mpq` and ignores the file of the same name on disk.

### The trap in arm B, and why the first run of it was thrown away

The first garbage run *did* fail to start, which looked like confirmation of precedence. It was an
artifact: the game had been killed 3 seconds earlier and `wineserver` had not finished shutting
down. Re-run after waiting for the process to disappear plus a fixed delay, the same garbage file
started normally. **A launch failure is only evidence if the launcher was given a quiesced prefix.**

### The timing trap that nearly produced a second wrong conclusion

`START.GS` plays `smk/imptitle.smk` and `smk/intro.smk` with `MODAL playvideo` at byte 1183,
**before** the 94 `run` statements. `intro.smk` is a multi-minute narrated cinematic, and how much of
it plays varies between launches. A measured pristine startup reached position 93 only after well
over two minutes, where earlier runs had reached it in 12 to 16 seconds.

That asymmetry is the whole lesson:

- **`combat.log` growing is proof.** It can only happen if execution reached position 93, whatever
  the elapsed time. Every conclusion above rests on a growth event, so none of them are affected.
- **`combat.log` not growing inside a fixed window proves nothing.** It is equally consistent with a
  hang and with the intro still playing.

A screenshot taken during a gap between the two movies shows a black window, which looks exactly
like a hang. On that basis a rewritten `gs.mpq` was briefly recorded here as unreadable by the
engine. **That was wrong — the rewritten archive works, as recorded below — and the claim was
withdrawn before it left this file.** Judge a launch by
the positive signal, or by a screenshot that shows recognisable game content, never by a timeout.

### Consequence: archive write-back works, and it is now the injection path

The experiment needs a modified member inside `gs.mpq`. The encryption worry was misplaced: members
are plain `MPQ_FILE_IMPLODE | MPQ_FILE_EXISTS` (`0x80000100`) and only `(listfile)` is encrypted, so
there is no Implode+Encrypt ruleset to reproduce.

- **Observed:** `SFileAddFileEx` round-trips correctly. The member reads back byte-identical through
  our own reader, the member count is unchanged at 1,700, and the flags are preserved.
- **Observed:** the rewritten archive keeps the original shape — format `0`, sector shift `3`, hash
  table 4,096 entries, block table 1,700 entries. Only `archive_size` and the two table offsets move,
  which is what compaction is expected to do.
- **Observed:** a control archive, rewritten by the same tool but with the member's *original* bytes,
  starts normally and reaches position 93 in 147 s.
- **Observed:** the archive carrying the probe **executes it**. `precedence.log` was created with the
  expected contents 145 s after launch, and `combat.log` still grew, so startup completed normally
  afterwards.

That last pair is the positive control the loose-file arms needed. **The same bytes execute from
inside `gs.mpq` and do nothing at all on disk.** The loose file is not merely ineffective; it is
never read. It also confirms the probe itself was valid GameScript, which the null result on disk
could not establish by itself.

**GameScript can now be injected into the running engine and observed from outside**, using the
engine's own file operators as the output channel. A prototype writer lives in
`spikes/asset-viewer/examples/mpq_replace.rs`.

### Two incidental findings from `lomse.exe`

- **Observed:** the five archives open through one wrapper at `0x004FEB10`, with `gs.mpq` opened
  **last**.
- **Observed:** the empty `custldr` directory is created by a plain
  `CreateDirectoryA("custldr", NULL)` at `0x004FF441`. It is not a loader hook.
- **Observed:** the numeric coercion helper at `0x004026A0` multiplies by `256.0` and `0.00390625`,
  so the interpreter's non-integer numbers are **8.8 fixed point**, and it returns a number in three
  forms at once — integer, raw fixed, and float.
- **Observed:** `drawimpframe` at `0x0049C500` pops **six** operands, not the five our arity walk
  reports: four through the inline pop sequence and two more through the shared pop helper at
  `0x0040ADB0`. The last one popped — so the first written in a script — is the IMP handle, which is
  looked up in a registry at `0x5A7B50` and rejected with the string `drawimpframe - no such imp`.
  The undercount is worth chasing in `operator_arity`.

## 2026-09-16 — Step 3: a working experiment harness, and `drawimpframe` refusing to draw

With injection proved, the next step was to call `drawimpframe` at known coordinates and measure
where the sprite landed. **The measurement was not obtained.** What was built along the way is
reusable, and the failure is bounded and specific.

### The operator signature, recovered and partly confirmed

`drawimpframe` at `0x0049C500` takes **six** operands, not the five the arity walk reports:

```
<imp> <sequence> <facing> <frame> <x> <y> drawimpframe
```

- **Observed:** four operands arrive through the inline pop sequence and two more through the shared
  pop helper at `0x0040ADB0`. The last popped — the first written in a script — is the IMP handle,
  looked up in the registry at `0x5A7B50` and rejected with `drawimpframe - no such imp`.
- **Observed:** the setup call `0x004F31F0` stores the imp data then calls `0x004F2FC0`, which
  indexes a **16-byte** record table by the first int (clamped against a count at `[table+0x1A]`),
  from which `0x004F2F70` indexes an **8-byte** table by the second, and the draw then computes
  `frame = [facing+4] + index*16`. Those record sizes are exactly our decoded Sequence (16),
  Facing (8) and Frame (16) records, which is what fixes the operand roles.
- **Observed in the running engine:** the interpreter accepted all six operands with no stack or type
  error, in this order, across several runs. That is a real confirmation of the arity and types, even
  though no pixels resulted.
- `imp` expects a string (`cmp cl,8`) and registers the object in `0x5A7B50`; `flagimp` has the same
  shape. Both were tried.

### `0x584AB8` is a clip rectangle, not a render target

The forwarder at `0x004F2F50` pushes `0x584AB8` before calling the real draw. That address is **not**
a surface: `0x0049AA90` initialises it as a `RECT{0, 0, 0x27F, 0x17F}` = `{0,0,639,383}`. 383 is the
map viewport height, below which the editor and game panels sit. An uninitialised (all-zero) rect
would clip everything away, which was the first hypothesis for the silent no-op — and it was wrong,
see below.

### What was tried, and the bounded negative result

- **Refuted:** that the draw was merely unpresented. Adding `refreshdirty` does present, but it
  repaints dialogs over anything drawn directly, and without it nothing appears either.
- **Refuted:** that the clip rect was uninitialised. The probe was re-run inside a **live map editor
  session** — clip rect initialised, map rendered, viewport active — and the captures were still
  byte-identical.
- **Observed:** across menu, no-menu, and live-map-editor contexts, with both `imp` and `flagimp`,
  `drawimpframe` changed **zero pixels** while never raising an error.

**Inferred:** there is a further precondition on the imp-player state that neither loader satisfies
from a bare script — most likely `[imp+0]` (the inner data pointer) is null because the loaders are
lazy, so `0x004F31F0` takes its `test eax,eax / je` exit and the draw silently does nothing. The next
attempt should confirm that by logging `getimpmemory`, which was the one diagnostic that did not run.

### Follow-up: the lazy-loader inference is refuted, and `drawimpframe` looks vestigial

The diagnostic that had not run was run. Using the engine's own accessors on the handle returned by
`imp`:

```
filename= iface/ordragb.imp
memory= 42964
```

- **Refuted:** that the imp was unloaded and `0x004F31F0` was taking its `test eax,eax / je` exit.
  The file is loaded, correctly named, and occupying 42,964 bytes.
- **Observed:** `screencapture` operates on the object at `0x584AE8`, reading `[obj+0x684]`, and
  `drawimpframe` calls `0x004753D0` on that **same object** immediately after drawing. The draw
  target and the capture source are the same surface, so a surface mismatch does not explain the
  silence either.
- **Observed:** a sweep of **20 calls** covering both sequences, all five facings, two frame indices
  each, at twenty distinct screen positions inside the clip rect, changed **zero pixels** in a live
  map editor session. The result is independent of the parameters.

**Inferred, and now the leading explanation:** `drawimpframe` is **vestigial** in the shipped build.
It is present in the dispatch table, it parses and type-checks its six operands correctly, it looks
up the imp and would report `drawimpframe - no such imp` for a bad handle — and it paints nothing.
It also has **zero call sites across all 1,471 extractable scripts**, which is consistent: nothing in
the shipped game uses it, so nothing would have caught its rotting.

That makes it the wrong instrument for issue #1. The engine's *working* sprite path is the one that
draws units on the map — visible in every editor capture — and that is where the convention should be
measured instead. `getspritescreenx` / `getspritescreeny` (each 1 operand, 1 result) expose a live
sprite's screen position, which is exactly the commanded-versus-observed pair this experiment needs.

### The harness, which is the durable part

Three things make future engine experiments cheap, and all are in `START.GS`:

- **Disable the intro**: the `true{...}if` guarding `imptitle.smk` and `intro.smk` becomes `false`.
  Startup to the end of the script drops from **over 150 s to about 6 s**. This alone changes what is
  practical to iterate on.
- **Reach a live map view with no user input**: `{}gamemodeproc gamemode 128 128 newmap
  default_edit_mode`, lifted from the Map Editor button in `gs/dlg/NEWDLG5.gs`. It reaches a rendered
  128x128 map in about 8 s. Replacing `{newdlg opendialog}ifelse` with `{}ifelse` suppresses the main
  menu when a clean screen is wanted.
- **Capture pixels**: `"name.bmp" screencapture` writes a 640x480 24-bit BMP. **Gotcha:** the file's
  `bfOffBits` field says `14`, but the pixel data actually starts at the normal `54`. Trust the
  computed offset, not the header field. `refreshdirty` is required before a capture to present
  anything, and it repaints dialogs, so capture a control frame *after* settling and diff the pair.

## 2026-09-16 — The engine applies the hotspot; the sign is still open

With `drawimpframe` established as vestigial, the measurement moved to the engine's *working* sprite
path: a stationary army on the world map, whose banner animates in place. A hotkey injected into
`gs/hotkey.gs` dumps a true 640x480 BMP on each press, so the sprite can be measured in game pixels
rather than through a scaled, filtered window capture.

**48 captures** of one stationary army were taken across two sessions.

### The result that needs no frame identification

Segmenting the banner cloth by luminance in a fixed window:

| Quantity | Across all 48 captures |
| --- | --- |
| cloth **right** edge | `x = 323` — every capture |
| cloth **top** edge | `y = 155` — every capture |
| cloth **left** edge | varies, `306..315` |
| cloth **width** | varies, `9..18` |

- **Observed:** the drawn sprite grows and shrinks **leftward and downward from a pinned top-right
  corner**, over a 10 px range of widths, with no movement of the anchor.
- **Inferred, strongly:** the engine does **not** draw a frame at its raw top-left. In
  `iface/orflagb.imp` the cloth begins flush with the frame box's left edge (`cx0 = 0`) in twelve of
  the fourteen facings, so a renderer that ignored the hotspot and drew each frame box at a fixed
  screen position would pin the **left** edge and let the right edge move with the width. The
  opposite is observed. **The hotspot is consumed by the drawing code.**

That answers the first half of [issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1):
the per-frame anchor is real and is applied at draw time, not merely stored.

### What was not settled, and why

**The sign is still unknown.** Deciding between `position - hotspot` and `position + hotspot` needs
each capture matched to a specific frame, and that matching was not good enough to build on:

- Silhouette matching of the segmented cloth against all 112 decoded frames (and their mirrors)
  returned best-fit IoU of only about **0.45**, and the frames it chose — 98 to 103 — render as thin
  wisps that look nothing like the banner on screen. The match is wrong.
- The observed cloth widths span `9..18`, but **no single facing** in `orflagb.imp` covers that range;
  the widest facings run 14..21 and the narrowest 3..9. So either the segmentation width is not a
  faithful measure of frame width, or the displayed sequence is not the one assumed.
- An anchor-constancy test run per facing gives contradictory answers: facing 0 favours
  `pos - hotspot` (spread 2 vs 13) while facings 9 and 10 favour `pos + hotspot` (spread 8 vs 2).
  Without knowing which facing is on screen, that test decides nothing.

A provisional calculation using the bad matches favoured `pos + hotspot` by a spread of 4 against 12.
**It is recorded here only to be dismissed**: it rests on frame identifications that are demonstrably
wrong, and it should not be cited.

### What would settle it

Identify the displayed frame independently of shape. The cleanest route is to capture a *complete*
animation cycle at a known tick rate and use the cycle's frame count and ordering to index frames,
rather than trying to recognise each one. Alternatively, segment against a true background plate —
obtainable by capturing the same tile with the army moved away — which would yield the sprite's full
opaque silhouette including the dark pole, instead of a luminance-thresholded fragment.

### Harness facts, including one self-inflicted detour

- Hotkeys can be added by inserting before the final `end` of `gs/hotkey.gs`:
  `ASCII_VAL"z"0 get{ ... }addhotkey`. Keys `e f g i j n o r u v w x z` and backtick are unbound;
  F1-F9 are taken by the game and F10-F12 by macOS.
- The counter-and-filename idiom **works**:
  `dest{"shot"n".bmp"}build_statement_ns strcpy` then `dest screencapture`. It produced
  `shot1.bmp` through `shot37.bmp`.
- **`screencapture` will not overwrite an existing file.** A probe writing one fixed filename
  captures exactly once and then silently does nothing.
- **The detour:** those 36 extra captures existed for most of the session and went unnoticed, because
  only `shot1.bmp` was ever checked. From that absence it was concluded that the counter had failed,
  and a second, simpler probe was built on the bad inference — costing a game restart and a repeat of
  the user's navigation. *Check for the files a mechanism would actually produce before concluding the
  mechanism is broken.*

## Evidence labels for future entries

Use these labels when recording findings:

- **Observed:** reproduced on the local machine or directly inspected in a file.
- **Documented:** stated in original or community documentation.
- **Inferred:** strongly suggested by evidence but not yet directly proven.
- **Unknown:** an open question requiring research or experiment.
