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
- **Observed:** the format contains 32-byte file headers, animation sequences/facings/frames, 256-entry BGRA palettes, six-byte padded hotspots, direct duplicate frames, and `0x04` shared-pixel frame records. (**Corrected 2026-09-17:** this entry said "repeated facings". There is no such thing — a facing's frame table is an ordinary array of 16-byte records, of which only the first may carry `0x04`. See the 2026-09-17 correction entry. The "BGRA palette" claim is separately corrected: entries are stored blue, red, green, pad.)
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
- **Observed:** one inspected creature frame uses palette index 0 for 8,576 background pixels and a separate index for a 1,651-pixel silhouette beneath the creature. (Those colour names were recorded through the old decoder, which swapped red and green. Corrected 2026-09-17: index 0 is pure **red** and index 1 is pure **green**.)
- **Observed:** an inspected 1-bit aura asset uses bright red and green as its two palette colors, confirming that blindly deleting both colors destroys meaningful mask data.
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
- **Observed:** the remaining IMP corpus contains 15,725 logical origin records and 64,432 six-byte hotspots across 28,771 frames. Origin ranges are X `-66..70`, Y `-207..77`; hotspot ranges are X `-115..123`, Y `-232..86`. (**Stale, 2026-09-17:** these totals were measured with the decoder bug corrected that day, which swallowed 29 frame records across five files. A spot re-measurement over the 1,798 stem-paired sprites gives 15,677 origins, 28,800 hotspot-bearing frames and 64,492 hotspot records — a different population from the one these figures were taken over, so the corpus totals are **pending re-measurement** rather than replaced. The coordinate ranges are unaffected in kind but unverified since.) **Blast radius, measured 2026-09-17 over all 1,800 sprites in the archive:** exactly **five files and 29 frame records** had a facing whose first record was `0x04` followed by a genuine one, and they are precisely the five that were failing validation — `aicr3b`, `chcr3b`, `chwmmb`, `ficr3b`, `ficr5b`. So the validator was a complete detector of this bug, the totals above are wrong by at most those 29 records, and the placement rule in [hotspots](hotspots.md) is untouched: it was measured on frames of sprites that are not among the five.
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
- **Observed, since corrected:** `lomse.exe` has 19 `*_HOTSPOT`-shaped names, but only 11 are IMP hotspot types (nine distinct values, 0-8); the eight `BOLT_HOTSPOT_*` names are bolt-record field indices. See the constant-table entry at the end of this log. The original observation was that there are more than the 10 quoted on the board from a
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

> **Naming corrected later the same day.** This entry measured hotspot type **0**, which the exe's constant table names `NO_HOTSPOT`, not `CURSOR_HOTSPOT` (which is type 1). Type 0 is the engine-reserved **draw placement**. The measurement is unaffected; see the constant-table entry at the end of this log.

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

## 2026-09-16 — `map2screen` decoded statically: a real 3D projection, and it already includes scroll

**Evidence class: observed in a local binary.** Disassembled `0x0046B0C0` (the `map2screen` native)
and the transform it calls at `0x00469AE0` in GS5R3 `lomse.exe`. No game launch was needed.

The recovered arity of 3 operands and 3 results is correct — but that was the least interesting part.

### Operands

Three pops, each coerced through the usual `0x004026A0` helper, so each operand may be written as an
int, an 8.8 fixed, or a float. The coercion's *float* output is the one used. Operands map to the
transform input vector in **written order**:

```
x y z map2screen
```

`x` becomes `in[0]`, `y` `in[1]`, `z` `in[2]`. A stack-empty pop raises error 6 and a non-numeric
operand raises error `0x0E`, both as elsewhere.

### Results

Three floats (tag 4) are pushed, and they are pushed **innermost-first**, so the stack reads
bottom-to-top as `out[2] out[1] out[0]`. That is, **screen X is on top** — the first thing a
following `exch`/`def` sees.

| Result | Meaning |
| --- | --- |
| `out[0]` (top of stack) | screen X, pixels, scroll included |
| `out[1]` | screen Y, pixels, scroll included |
| `out[2]` (deepest) | screen Y of the same `(x, y)` at **`z = 0`**, i.e. the ground point directly below — **scroll not applied** |

### The transform

`0x00469AE0` is a thiscall on the global camera object at `0x005876D0`. It builds the homogeneous
vector `{x, y, z, 1.0}` and runs it through two 4x4 matrix multiplies (`0x004A47C0`) using the
matrices at camera `+0x164` and camera `+0x64`, then maps to the viewport with the literals at
`0x0054D7D4`, `0x0054D7D8` and `0x0054D7DC`:

```
screen_x = ndc_x * 320.0 + 320.0        ; 54D7D4 = 320.0, 54D7D8 = -320.0
screen_y = 192.0 - ndc_y * 192.0        ; 54D7DC = 192.0
```

Then, back in `0x00469AE0`, the integer camera scroll at `+0x1A4` and `+0x1A8` is added to
`screen_x` and `screen_y` respectively.

Four consequences, all of which matter for [issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1):

1. **The map view is 640x384**, centred at `(320, 192)` — 480 minus 96 rows of interface chrome.
   The half-extents are baked in as literals, not read from a mode structure.
2. **World `+y` is up; screen `+y` is down.** The `fsubr` inverts it. Any hotspot sign conclusion has
   to state which space it is in, and the two differ in sign on the y axis.
3. **`map2screen` output is directly comparable to a screen capture.** Scroll is already folded in,
   so `observed_top_left - map2screen(cell)` needs no separate camera bookkeeping. This removes the
   largest remaining source of error in the planned hotspot measurement.
4. **`out[2]` is a free ground-level baseline.** The engine re-projects the same point with `z`
   forced to zero, which is exactly the quantity a shadow or a terrain footprint needs. Remember it
   does **not** have the scroll offset added, unlike `out[0]` and `out[1]`.

### Method note

This is the second operator whose real signature came out of the disassembly rather than the arity
walk, after `drawimpframe`. The walk is useful for finding candidates; it is not evidence about an
operator's contract. Read the entry point before designing an experiment around an operator.

## 2026-09-16 — The hotspot convention, measured in the running engine

**Evidence class: observed in gameplay.** Issue #1's last open question — the hotspot *sign* — is
answered. The game was modified with a checksum-verified backup and restored to a byte-identical
`gs.mpq` afterwards.

### The rule

```
top_left = anchor + hotspot - (w >> 1, h >> 1)
```

Equivalently: **the hotspot is the vector from the anchor point to the centre of the frame**, in
screen pixels with `+y` downward. The frame is drawn **centred**, and the hotspot is **added**.

### How it was measured

The earlier attempt failed because the subject was uncontrolled — an army banner whose sequence,
facing and cycle position were all unknown, segmented with a luminance threshold against unknown
terrain. This run removed frame identification entirely instead of improving it.

`tree2.gs` defines a `terrainsprites` dictionary whose entries resolve to `imp/<name><zoom>.imp`.
Every candidate is **one sequence, one facing, one frame** — there is nothing to identify. A single
injected hotkey then did, in one frame with nothing else moving:

1. `currentplayer { ... anythinglocation UNITTYPELAND findemptylocation } enumplayerarmies` to pick an
   empty on-screen cell, logged as `cell 63 70`.
2. `"plate.bmp" screencapture` — the cell with nothing on it.
3. For each of four subjects: `hsx hsy terrainsprites /<name> get addterrainsprite`,
   `rendermap refreshdirty`, capture, then `hsx hsy terrainspriteat destroyterrainsprite` and
   re-render. Self-cleaning, so the save was left as found.

Differencing each capture against the plate gives the **exact opaque silhouette** — no threshold, no
palette-remap assumption, dark pixels included.

Four subjects at one cell means the anchor is shared and unknown constants cancel, so the result does
not depend on `map2screen` being correct.

| Subject | Frame | Hotspot | Predicted top-left | Measured |
| --- | --- | --- | --- | --- |
| `orchard` | 60x70 | `(0, -35)` | (290, 110) | **(290, 110)** |
| `teeth` | 52x48 | `(0, -1)` | (294, 155) | (294, 156) |
| `palm1` | 53x53 | `(9, -20)` | (303, 134) | **(303, 134)** |
| `dtree` | 21x34 | `(-6, -12)` | (304, 151) | **(304, 151)** |

Recovered anchor: `(320, 180)`. Two free parameters fit against eight measurements.

**The `teeth` row.** Its predicted top row 155 measured zero changed pixels, and the per-row diff
profile tapers smoothly to zero at both ends (`... 4 2 4 2 2 2 1 0 0 0 0`), so the frame's outermost
rows simply happen to match the terrain beneath. This was checked rather than assumed: the frames were
decoded and **none of the four has any fully transparent border row or column**, which refutes the
first explanation offered for the shortfall. A coincidental colour match on one row is the surviving
one, and it is consistent with every other row.

**`floor`, not `ceil`.** Both odd-sized cases settle it. `palm1` is 53 wide and its silhouette measured
exactly 53 columns starting at 303; `ceil` predicts 302, which would have measured 54.

### Why this matters

- **The sign is plus.** Every shipped hotspot `y` is negative, which shifts art *upward* on screen —
  art grows up from where the object stands.
- **The convention is centre-relative, not corner-relative.** That `w >> 1` term explains why the
  board's crop-and-re-centre workaround kept almost working: re-cropping changes the implied hotspot
  by half the crop, so the error tracks the edit instead of staying fixed. Combined with the already
  established fact that `lomut` writes no hotspot array at all, this is what a correct writer needs.

### One honest negative

`map2screen(63, 70, 0)` returned `x = 320`, matching the recovered anchor x, but `y = 2027` against a
recovered anchor y of `180`. Its x agreed and its y did not, so the y/z input convention is wrong
somewhere — units, or a required elevation term. **`map2screen` is not validated by this run**, and
nothing above depends on it, because the four-subject differencing never used it. Left open.

## 2026-09-16 — A placement writer, and the two things it is not allowed to claim

**Evidence class: observed in a local binary.** With the draw rule measured, the tool can now write
placement back. `--set-imp-placement` edits a loose IMP in place and `--imp-placement-for` solves for
the value a re-cropped frame needs.

### The corpus splits in two, and the split matters

Frame record bytes `+8..+12` are **overloaded**. When the record's hotspot count byte is zero those
four bytes are the `origin_x`/`origin_y` pair; when it is non-zero they are a file offset to the
hotspot array. A frame therefore cannot carry both, and across GS5R3's `imp.mpq`:

| Placement form | Frames |
| --- | --- |
| origin pair | 15,725 (454 of them exactly zero) |
| hotspot records | 28,771 |
| neither (duplicate/back-reference frames) | 7,170 |

**Hotspot records are the majority form.** The rule measured earlier today was measured on terrain
sprites, which carry the *origin pair*. So the writer edits either form mechanically, but the
**semantic** claim covers the origin pair only.

### Which hotspot type is the draw anchor is still open

Types 0 and 7 appear on nearly every unit frame. Two things were tried and neither settles it:

- Regressing the offsets against frame height separates the types but identifies none of them as the
  anchor. Type 0 gives `y ~ -0.301h - 1.26` (residual 9.93 of 14.54) and type 7 gives
  `y ~ -0.430h - 2.97`; the origin pair itself gives `y ~ -0.365h + 4.19` with a *worse* residual
  than either. The four measured terrain sprites had `-y/h` ranging from 0.02 to 0.50, so no single
  ratio was expected, and none is found.
- **Refuted:** `units\imp\aicr2a.imp` has type 0 at `y = -33` on a 67-tall frame and `aicr2b.imp` has
  `-16` on 33, both exactly `-(h >> 1)`. That looked like a rule. Corpus-wide it holds for **1.7%** of
  frames. Two samples agreeing is not a rule; the check is kept in
  `examples/imp_placement_survey.rs` so the same idea is cheap to re-test rather than re-derive.

Settling this needs a second engine measurement against a sprite whose hotspot records we know,
in the way the terrain-sprite probe settled the origin pair.

### What the writer guarantees

- **Identity is byte-identical.** Writing a frame's existing placement back reproduces the input
  exactly, verified against shipped `palm1b.imp` and `units\imp\aicr2a.imp`.
- **Length never changes**, so every offset stored elsewhere in the file stays valid. A real edit
  touches only the bytes that actually differ — two, for the cases tested.
- **It re-parses before writing** and refuses to emit a file it cannot read back, or one whose
  placement does not read back as the value requested.
- **It refuses rather than guesses**: writing an origin to a hotspot-bearing frame, writing a hotspot
  type the frame does not carry (the error names the types it does carry), and writing a type that
  appears more than once.
- **It warns about aliasing, per path.** An origin lives in the frame record and a hotspot lives in
  the array the record points at, and the two share differently: `frames_sharing_record` covers
  records that back more than one logical frame, `frames_sharing_hotspots` covers distinct records
  storing one array pointer. (**Corrected 2026-09-17:** this said "repeated facings and `0x04`
  shared-pixel runs"; neither is a real structure. Records alias only when two facings point their
  frame tables at the same offset.) The second does not occur in any of 241 shipped unit sprites, but the
  writer advertises a guarantee, so it holds for files we did not author.
- **It refuses a duplicate frame's origin.** A `0x04`/`0x08` frame has no origin of its own.
- **It refuses to overwrite**, like every other output path in the tool.
- **It refuses arithmetic that would wrap** rather than printing a wrapped value.

### The demonstration that matters for the board

`palm1b.imp` frame 0 is 53x53 with origin `(9, -20)`. Pad the art by 4 pixels on every side, to
61x61, and ask what keeps it on screen:

```
$ lom-asset-viewer --imp-placement-for 61 61 320 180 303 134
placement	13	-16
```

`(9, -20)` becomes `(13, -16)` — each axis shifts by exactly half the added pixels. That is the
centre-relative convention stated as a recipe, and it is precisely the correction the board's
crop-and-re-centre workaround was missing.


### Review of the writer, and what each side caught

Both a Claude reviewer and a Codex reviewer ran over the same diff. **They disagreed, and the
disagreement is the interesting part.** Codex reported no defects; the Claude reviewer reported five,
four of which were confirmed here by reading the code and running the path:

| Finding | Verdict | Fix |
| --- | --- | --- |
| `set_imp_placement` used `fs::write`, which truncates, while every other output path in the tool is create-new | **Confirmed.** This is the only command that mutates game art. | create-new, plus a CLI test |
| `write_frame_origin` accepted a duplicate/shared-pixel frame, patching four bytes the parser reports as having no origin | **Confirmed** by reading the guard: the only check was `hotspot_offset.is_some()` | refuse by name, plus a regression test |
| Placement arithmetic overflowed on command-line coordinates | **Confirmed** | `checked_add`/`checked_sub`, plus a regression test |
| The aliasing warning used record sharing for the hotspot path, where the shared unit is the array | **Confirmed as a defect, but not reachable in shipped art** — 0 of 241 unit sprites have two records pointing at one hotspot array. Fixed anyway | `frames_sharing_hotspots` |
| No test covered the new CLI layer | Fair | five new tests |

**Why Codex missed the overflow is worth recording.** It probed the boundary with the *release*
binary, where the subtraction wraps silently and prints a plausible number, and read that as a pass.
The Claude reviewer ran a debug build and got `attempt to subtract with overflow`. Reproduced here
both ways. A boundary probe against an optimised build is not a boundary probe.

Tests after the fixes: **83 library and 11 CLI**, clippy clean.

## 2026-09-16 — Hotspot record 0 is reserved: the engine hides it from GameScript

**Evidence class: observed in a local binary, plus corpus measurement.** No game launch. This is the
first real progress on the open question from the placement writer — how the engine places the 28,771
frames that carry hotspot records instead of an origin pair.

### Both hotspot natives skip record 0

`getimphotspot` is at `0x0049BF90` and `enumimphotspots` at `0x0049C1D0`. Each walks the frame's
hotspot array, and **each starts at record index 1**:

```asm
; getimphotspot, 0x0049C13A
mov  cx,[edx]          ; frame record's first u16
shr  ecx,8             ; record count, from record byte +1
cmp  ecx,eax           ; eax = 1
jle  <fail>            ; count <= 1 means nothing to search
mov  ebx,[edx+8]       ; hotspot array pointer, the overloaded dword
lea  edx,[ebx+6]       ; *** start at record 1, not record 0 ***
cmp  di,[edx]          ; compare requested type
```

`enumimphotspots` does the same at `0x0049C35A`: loop counter initialised to `1`, byte cursor
initialised to `6`, and the same `count <= 1` bail-out.

So **no GameScript can read or enumerate record 0 by any means.** It is engine-internal.

This also confirms two format details directly from the code rather than by inference: record byte
`+1` really is a count (`shr ecx,8` of the first u16), and the dword at `+8` really is the array
pointer.

While reading it: **`getimphotspot` pops five operands**, not the one the arity walk reports. That is
the third operator whose real signature came from disassembly after `drawimpframe` and `map2screen`.
The walk finds candidates; it is not evidence about a contract.

### The corpus agrees that record 0 is special

Across GS5R3's 28,771 hotspot-bearing frames:

| Measurement | Result |
| --- | --- |
| record 0 has type 0 | 28,661 (99.62%) |
| type 0 appearing in any slot **other** than 0 | **0** |
| frames with only record 0 | 0 (minimum count is 2) |
| record-count histogram | 2:24,412  3:2,621  4:1,287  5:190  6:212  7:27  8:12  9:10 |

Type 0 and slot 0 are the same thing: type 0 never occurs anywhere else, and slot 0 is almost always
type 0. The engine's `count <= 1` bail-out is exactly what a reserved slot 0 plus optional typed
attach points would need.

### It also localises the out-of-vocabulary anomaly

All **110** frames whose record 0 is not type 0 are in one file, `units\imp\eacr5a.imp`, and all carry
type **136**. That is one of the two files already flagged as carrying hotspot types outside the
engine's 19-name vocabulary, and it is now pinned to the reserved slot rather than to an attach point.
This is directly relevant to the open question on board thread 2176.

### Where this leaves the custom-unit problem

The reading is that **record 0 plays the role for unit frames that the origin pair plays for terrain
frames** — the draw placement — while records 1 and up are the typed attach points the vocabulary
names. That is consistent with every measurement above, but it is still **inference**: nothing here
observes the draw code consuming record 0.

It is now a sharp, cheap hypothesis to test, and the test needs no frame identification at all:
**perturb record 0 of a unit sprite by a large known amount, inject, and look.** If record 0 is the
draw placement, every frame of that unit shifts by the perturbation. If it is not, nothing moves.
A single keypress settles it.

## 2026-09-16 — Hotspot record 0 *is* the draw placement, and the rule is the same one

**Evidence class: observed in gameplay.** The inference from the reserved-slot finding is now
measured. Both `gs.mpq` and `imp.mpq` were modified with checksum-verified backups and restored
byte-identical afterwards.

### The design: build the control instead of finding one

No shipped unit sprite has uniform frame sizes, so animation would reintroduce the frame
identification problem that sank the first attempt. Rather than search harder, the control was
manufactured:

1. Extract `units\imp\aicr2a.imp` and inject it back as `units\imp\zzprba.imp` — sprite **A**.
2. Take the same bytes, shift hotspot **record 0 of every frame** by exactly `(+60, +40)`, and inject
   that as `units\imp\zzprbb.imp` — sprite **B**. 105 distinct records were patched and all 105 were
   verified to have moved by exactly the delta before injection.
3. Leave record **7** untouched in both. If the engine placed by record 7, nothing would move — a
   control built into the experiment rather than argued for afterwards.

Shifting *every* frame is what makes the result frame-independent: whichever frame the engine draws,
it moves by the same amount.

A unit IMP cannot be placed as a unit from script, but `addterrainspritetype` takes an arbitrary
filename, so `["units/imp/zzprba.imp"]cvx addterrainspritetype` registers a unit sprite as a terrain
sprite type. Both types registered (ids 470 and 471). One hotkey then captured a plate, placed A,
captured, destroyed it, placed B, captured, destroyed it.

### The measurement

The naive plate difference gave bounding boxes of `110x191` and `129x151` — larger than any frame in
the file, because the map is live and an unrelated interface element was animating. Clustering the
changed pixels into connected components separates them cleanly:

| Capture | Component | Top-left | Size |
| --- | --- | --- | --- |
| A | sprite | (331, 136) | 49x67 |
| B | sprite | (391, 176) | 49x67 |
| both | interface noise at (411, 301) | — | 10x26, identical in both |

Both silhouettes are `49x67`, which is frame 0's exact size, so the same frame was drawn both times.
Differencing A against B directly yields the same two components and nothing else.

```
B - A = (+60, +40)        exactly the record-0 delta that was built in
```

And the absolute rule holds without modification. Solving `top_left = anchor + placement - (w>>1, h>>1)`
from A's record 0 of `(1, -33)` gives `anchor = (354, 202)`; feeding B's record 0 of `(61, 7)` through
the same anchor predicts `(391, 176)`, which is what was measured.

### What this settles

**Hotspot record 0 is the draw placement for record-bearing frames, and it obeys the same rule as the
origin pair**, added and centre-relative. Placement is therefore unified across the whole corpus:

| Frame form | Where placement lives |
| --- | --- |
| hotspot count 0 | the origin pair in the record's `+8` dword |
| hotspot count > 0 | hotspot **record 0**, which no script can read |

That closes the gap the placement writer had to leave open, and `--set-imp-placement --hotspot 0` is
the command that writes it.

### One caveat, stated rather than buried

This placed a unit IMP through the **terrain sprite** path. It proves the renderer reads record 0 from
the IMP frame and applies the measured rule; it does not prove the unit draw path computes its
*anchor* the same way. The sign, the centre-relative form and the choice of record 0 are settled; a
unit-specific constant offset in the anchor is not ruled out.

### A lead, not a result

The first run measured anchor `(320, 180)` at cell `(63, 70)`; this run measured `(354, 202)` at cell
`(64, 70)`. One cell of x apparently costs `(+34, +22)` of screen, which has the right shape for an
isometric step. **The two runs were separate games and the camera may not have matched**, so this is
recorded as a lead to test deliberately, not a measurement. It would also give a second, independent
route at `map2screen`'s still-wrong y convention.

## 2026-09-16 — The hotspot type numbers, read out of the exe, and two corrections

**Evidence class: observed in a local binary.** The engine keeps a name/value table of GameScript
constants as 8-byte `{char* name, int value}` pairs. Locating every `*_HOTSPOT` string, finding the
dword that points at it, and reading the next dword gives the numbers directly:

| Constant | Value |
| --- | --- |
| `NO_HOTSPOT` | **0** |
| `CURSOR_HOTSPOT` | **1** |
| `MISSILE_ORIGIN_HOTSPOT` | **1** |
| `SPELL_ORIGIN1_HOTSPOT` | 2 |
| `SPELL_ORIGIN2_HOTSPOT` | 3 |
| `SPELL_ORIGIN3_HOTSPOT` | 4 |
| `SPELL_ORIGIN4_HOTSPOT` | 5 |
| `FLAP_OFFSET_HOTSPOT` | 6 |
| `MISSILE_TARGET_HOTSPOT` | **7** |
| `SPELL_TARGET_HOTSPOT` | **7** |
| `STREAMER_HOTSPOT` | 8 |

The table is at `0x00560108` through `0x00560158`.

### Correction 1: the vocabulary is nine values, not nineteen names

The `BOLT_HOTSPOT_S0`–`S3` and `BOLT_HOTSPOT_D0`–`D3` names, which this repository has been counting
as part of the hotspot vocabulary since PR #12, are **not IMP hotspot types at all**. They live in a
different block at `0x005604B0` with values 15 through 22, immediately followed by:

```
0x5604f0  BOLT_SPELLDEF_ID  = 23
0x5604f8  BOLT_RESULT_PROC  = 24
```

They are **field indices into a bolt definition record**, not type tags. `MISSILE_HOTSPOT = 14` at
`0x00560580` is the same kind of thing. So the real IMP hotspot vocabulary is **eleven names mapping
to nine distinct values, 0 through 8**, with two aliased pairs (`CURSOR`/`MISSILE_ORIGIN` both 1,
`MISSILE_TARGET`/`SPELL_TARGET` both 7).

That fits the corpus exactly. The measured ids include every value 0 to 8 and all of them are common:
0 (28,661), 7 (28,183), 8 (1,409), 1 (1,130), 2 (1,125), 3 (966), 4 (890), 5 (834), 6 (132). The ids
that remain genuinely outside the vocabulary are 9, 10, 16, 106, 136, 138, 143 and 190.

### Correction 2: type 0 is `NO_HOTSPOT`, and this repository has been calling it the cursor hotspot

`CURSOR_HOTSPOT` is **1**, not 0. The "cursor hotspot is authored, not derivable" study earlier today
measured type **0**, so its subject was mislabelled throughout. The finding itself is unaffected and
in fact becomes more important, because type 0 is now known to be the **draw placement** — the study
was measuring the thing that actually matters, under the wrong name.

Better still, the number corroborates the reserved-slot result independently. Record 0 is the slot the
engine refuses to search or enumerate, and its type tag reads `NO_HOTSPOT` — literally "this is not a
typed attach point". The format is self-describing once the numbers are known.

### What the types mean, from the only scripts that use them

`getimphotspot` and `enumimphotspots` are **never called anywhere in the 4,692-member corpus**; they
are tool- and engine-facing. The one consumer is `addauratype` in `gs\aura.gs`, whose documented
operand order is `<hotspot> <looping_sound> <imp_filename_proc> ... addauratype`, across 71 calls.
No script ever passes a bare integer — always a named constant — and no script defines the numbers,
which is why they had to come from the exe.

| Type | Meaning, from usage |
| --- | --- |
| 0 `NO_HOTSPOT` | no anchor; the effect is drawn on the unit as a whole. All 8 elemental sphere auras, plus whole-body shields. Also the tag on the reserved draw-placement record. |
| 1 `CURSOR` / `MISSILE_ORIGIN` | launch anchor for attack and breath projectiles — the dragon-breath family sets `/missile_launch_hotspot MISSILE_ORIGIN_HOTSPOT def`. |
| 2–5 `SPELL_ORIGIN1..4` | caster-side emission points, hand or staff. `bolt_fury.gs` has all four commented out in sequence as alternative launch points, and the hydra auras use ORIGIN1 and ORIGIN2 for different heads. |
| 6 `FLAP_OFFSET` | never passed to a native in any script; name suggests a wing-flap offset for flyers. |
| 7 `MISSILE_TARGET` / `SPELL_TARGET` | the impact anchor on the *target*. The overwhelming default for per-spell auras — 54 of the 71 `addauratype` calls. |
| 8 `STREAMER` | never passed to a native; name suggests a trailing-streamer attach point. |

### The two out-of-vocabulary files, re-triaged

`units\imp\eacr5a.imp` uses 136 in record 0 for all 110 frames, and 106, 138 and 143 in later slots.
`units\imp\aiwm1b.imp` mixes 190 in with ordinary 0, 7 and 10.

Record 0's tag is **never read by the engine**, so an odd value there is harmless — that disposes of
the `eacr5a` record-0 anomaly and of `aiwm1b`'s, and its record-0 offsets are tightly clustered and
track frame size, i.e. ordinary placement data. What is **not** disposed of is `eacr5a` carrying 106,
138 and 143 in slots 1 and 2, which the engine *does* search. Those remain unexplained and are the
right thing to ask the board about.

### Shadow blend: measured the wrong sprite, and saying so

The plan was to recover the shadow blend from the unit captures already on disk. It did not work, for
a reason worth recording rather than retrying blindly. Of frame 0's 3,283 pixels, 1,022 are the colour
key and render fully transparent — confirmed, 100% of them leave the plate untouched — and **every
other pixel is an opaque palette colour**: zero pixels blend with the background.

The reason is that `aicr2a.imp` has `palette[1] = [255, 0, 0]` and never uses index 1 at all. Across
the corpus, index 1 *is* the shadow for most art — 1,223 of 1,800 files and 32,784 of 41,344 frames
use it, 6.6% of all pixels, and a typical `palette[1]` is `[8, 8, 8]`. The probe sprite was simply one
of the exceptions.

So the shadow blend needs one capture of a sprite that actually uses index 1, which is a game run
rather than an offline analysis. Not attempted here rather than guessed at.

A channel-order discrepancy also surfaced while comparing rendered pixels against decoded palette
entries: the two agree on every index where red equals green, and disagree where they differ. That is
the signature of a channel swap somewhere between our decoder and the capture, and it bears on the
"palette is BGRA, swapped to RGB" claim recorded in [Stage 1](native-asset-stage.md). It is **not**
resolved here — one frame against one background cannot separate a decoder bug from a BMP reader bug —
and it needs a deliberate test against a known colour.

## 2026-09-16 — An unattended probe does not work, and why

**Evidence class: observed in gameplay (negative result).** The two remaining offline-ish hotspot
questions — the shadow blend and the palette channel order — were prepared as a probe that needed no
human, on the theory that the harness's scripted map view (`{}gamemodeproc gamemode 128 128 newmap
default_edit_mode`) removes the only step macOS blocks. **It does not work from `START.GS`.**

What was tried: intro disabled, main menu suppressed by replacing `{newdlg opendialog}ifelse` with
`{}ifelse`, then the map-view idiom, a custom sprite type, a plate capture, nine placements along the
map diagonal, a second capture, and cleanup — all appended to the end of `START.GS`.

What happened: `lomse.exe` started and stayed resident, but **`combat.log` never grew**, so startup
never reached run position 93, and a screenshot showed **no game window at all**. No probe output was
produced.

The likely reason, stated as a hypothesis rather than a finding: this work runs at *script-load* time,
while the map view, rendering and `screencapture` all need the **game loop** to be running. The
harness note that recorded the idiom did not say which context it was exercised from, and every probe
that has actually worked here ran from a hotkey — that is, from inside the loop.

**Do not retry this by appending to `START.GS`.** If an unattended probe is wanted later, the thing to
investigate is a tick alarm: the corpus has `TICKALARM_END_OF_COMBAT` and `TICKALARM_RETURN_TO_WORLD`
constants and an `eventalarm` operator, which would fire from inside the loop without input. That is
unverified and should be treated as a lead.

The game was restored to byte-identical `gs.mpq` and `imp.mpq` afterwards.

### What is prepared, so the next attended run is one keypress

The measurement itself is ready and needs only the proven hotkey path:

- **Donor sprite**: `imp\tree4e.imp` — single frame, 72x76, 915 pixels of palette index 1, and
  `palette[1]` is the sentinel `[255, 0, 0]`. Single-frame means no animation to confound anything.
- **Authored palette** (`examples/author_palette.rs`): five entries rewritten as **raw bytes**, which
  is the point — comparing bytes written against pixels rendered settles the channel order without
  assuming our decoder's. Index 166 becomes raw `ff 00 00`, 141 `00 ff 00`, 200 `00 00 ff`, 135
  `ff ff ff`, and index 1 becomes raw `ff 00 ff`.
- **The shadow test falls out of the same capture.** If the index-1 region renders magenta, index 1 is
  an ordinary colour. If it renders as a darkened version of whatever is behind it, it is a blend, and
  the capture gives the blend function directly.
- A useful observation already: our decoder reports `palette[1] = [255, 0, 0]` for raw bytes
  `00 00 ff`, i.e. it reverses the triple. The capture will say whether that is right.

Because all of this places through a terrain sprite type, it runs on the **world map of a real game**,
which is where the unit-anchor check and the cell-to-screen fit also have to happen. One attended
session with one keypress can therefore settle all three.

## 2026-09-16 (later) — The attended run: a negative result, and two findings from its wreckage

**Evidence class: observed in gameplay.** The prepared probe ran on the world map of a real
single-player game. It did not answer the question it was built for, and it damaged the live map on
the way. Both archives were restored byte-identical afterwards and nothing was saved, so the damage
existed only in memory.

### What failed

`imp\zzpal.imp` — the donor with the authored palette — was injected, and
`["imp/zzpal.imp"]cvx addterrainspritetype` returned a type id. `addterrainsprite` then reported
success, and the cell it was placed on stayed occupied for the rest of the session, so **the sprite
object was really created**. But no capture contains it. `zc3.bmp` is pixel-for-pixel identical to
the plate, and `zc2.bmp` differs only by ordinary map animation; nothing anywhere is 72x76.

**The art failed to load while the placement succeeded.** The probe had no control that could say
why, which is the design fault worth recording: an invisible subject and a broken subject look the
same, and the run could not tell them apart.

### Two traps that were paid for here

**`anythingat?` does not see terrain sprites.** The guard `x y anythingat? not` reported the cell
empty while a village stood on it. Because the probe's own sprite was invisible,
`x y terrainspriteat` then returned *the village*, and `destroyterrainsprite` deleted it. The
captures show it exactly: a 34x38 building at screen (370,185), present in the plate and absent
afterwards.

> Never clean up a placed sprite by location. Match on `getterrainspritetype` and destroy only the
> type the probe itself registered. `enumterrainsprites` supports this directly, and `tree2.gs`'s
> `changeterrainspritetype` is the shipped example of the idiom.

**The hotkey auto-repeats.** Holding `z` for a moment ran the body **nine times**, registering nine
sprite types (470-478) and re-entering the placement logic eight times more than intended. Any probe
body needs a fire-once flag in `userdict`.

### Finding: `map2screen`'s third return value is the screen x coordinate

`map2screen` returns three values; the last one is screen x, and the isometric step falls straight
out of the log:

| cell | third value |
| --- | --- |
| (63,70) | 320.412 |
| (64,70) | 354.353 |
| (65,70) | 388.294 |
| (63,71) | 286.471 |
| (63,72) | 252.530 |
| (66,73) | 320.412 |

That is **+33.941 per cell of x and -33.941 per cell of y**, so `screen_x = x0 + 33.941*(dx - dy)`,
and the diagonal cell (66,73) returning the base value again confirms it. The remaining two values
move by 14.4 per isometric step and differ from each other by a constant 80, so they are in different
units and are not screen pixels. This is real progress on the y/z convention question left open by
PR #29, though the y half is still unmeasured — it needs a sprite that actually renders.

### Finding: `screencapture` writes R, G, B, not the BMP-standard B, G, R

Decoding the captures per the BMP standard makes the interface stone blue and the terrain purple.
Decoding the bytes in the order written makes the stone brown and the grass green. The interface
frame is a fixed asset that is not subject to lighting, so this is not ambiguous.

This matters out of proportion to its size. Every previous use of these captures was an *equality*
difference, which is blind to channel order — so the error survived undetected. The next measurement
scheduled to run through them is the **palette channel order**, where a reader that silently swaps
red and blue would have produced a confident, exactly-wrong answer. `screencapture` is now known to
be non-standard in two independent ways, since `bfOffBits` already reports 14 against a real offset
of 54.

`tools/probe_captures.py` is the reader that gets both right, and its tests pin them.

### The rebuilt probe

`tools/engine_probe.py` generates the replacement, `scripts/install-engine-probe.sh` installs it and
`scripts/restore-game-archives.sh` undoes it. The probe is now a **diagnostic ladder** rather than a
single measurement — four sprite types placed in one capture:

| Type | Sprite | What its absence would mean |
| --- | --- | --- |
| shipped type, shipped art | `terrainsprites /orchard get` | the capture or the cell logic is wrong |
| custom type, shipped art | `["imp/tree4e.imp"]cvx addterrainspritetype` | `addterrainspritetype` on a literal filename does not work |
| custom type, injected copy | `["imp/zzctl.imp"]cvx addterrainspritetype` | the engine cannot read an added archive member |
| custom type, authored palette | `["imp/zzpal.imp"]cvx addterrainspritetype` | the palette edit broke the file |

`zzctl.imp` is byte-identical to `imp\tree4e.imp`, so the third and fourth rungs differ only by the
twenty palette bytes. Whichever rung breaks names the cause, which is precisely what the failed run
could not do.

### A third cleanup trap, caught in review rather than in the game

The rebuilt probe's first version cleaned up by sprite **type**, which is what the village incident
seemed to teach. Cross-model review pointed out that rung 0's type is
`terrainsprites /orchard get` — the *shipped* orchard type, shared with every orchard on the map — so
the sweep would have destroyed all of them. The generated script confirms it: the id is looked up
from the shipped `terrainsprites` dictionary, and the sweep was unqualified.

The rule that actually holds is narrower than either version: **match on type and cell together, and
only treat a type id as safe to sweep on its own when the probe minted it via
`addterrainspritetype` during the same keypress.** A regression test asserts it and fails when the
bug is reintroduced.

Worth recording as a pattern: the fix for a destructive bug was itself destructive, in the same
direction, because it generalised from one incident instead of from the invariant. The invariant is
"destroy only what this keypress created", and neither location nor type alone expresses it.

### Corrected: map locations are packed, and the arity table cannot settle operand order

Cross-model review of the rebuilt probe found three operand-order errors in it, all the same
mistake. **`anythinglocation` and `getterrainspritelocation` each return one packed location**,
`y * map_width + x`, and **`findemptylocation` takes `(location, unittype)`** — two operands, not
three. The shipped corpus is unambiguous: `anythinglocation xy_to_x_y` appears 31 times (you do not
decompose an already decomposed pair), `unit_loc UNITTYPELAND findemptylocation` is the shipped
call form, and `/temple_loc temple_id getterrainspritelocation def` stores a single scalar.

**The failed run's own log confirms it and was misread at the time.** Its first line printed as
`army    9152  base  63   70` — an *empty* x beside `9152`. At map width 128, `9152` is cell
(64,71), one step from the `(63,70)` that `findemptylocation` returned. The x was empty because
reading the packed value as a pair underflowed the stack. That line was read as a successful army
lookup.

This sharpens the standing warning in [hotspots](hotspots.md#do-not). The recovered arity table
undercounts pops, and it also cannot express *what* the operands are. **Operand order comes from
shipped call sites; the table is only a hint.** The previous session's probe got this right by
copying `anythinglocation UNITTYPELAND findemptylocation` verbatim from working code — the rewrite
"improved" it into a stack underflow.

One more from the same review: `screencapture` refuses to overwrite, so **stale captures must be
cleared from the game directory before each run** or a second attempt silently produces nothing and
the old plate is collected as if it were fresh — which reads identically to "the sprite did not
render", the very conclusion under test.

## 2026-09-17 — The ladder run: the shadow blend and the palette channel order, both settled

**Evidence class: observed in gameplay.** One keypress on the world map of a real single-player
game. All four rungs rendered, `zs2.bmp` came back pixel-identical to the plate (so cleanup removed
exactly what the probe placed and nothing else), and both open compositing questions are answered.

Last night's failure was therefore **entirely** the stack underflow. The MPQ injection was fine and
the palette edit was fine — rungs 2 and 3 are injected members and both drew. Had the first probe
carried controls, that would have been visible immediately instead of costing a run.

### Palette index 1 draws the background at half brightness

| Copy | index-1 pixels that changed | exactly half | within one palette step | neither |
| --- | --- | --- | --- | --- |
| control, index 1 = shipped red | 903 | 122 (13.5%) | 781 (86.5%) | 0 |
| authored, index 1 = magenta | 889 | 129 (14.5%) | 760 (85.5%) | 0 |

**Not one pixel fell outside half-a-background.** The 86% that miss exact halving miss it by at most
one palette step, which is what an *indexed* framebuffer forces: the blend is a 256-entry remap
table, so the result snaps to the nearest available entry rather than being computed per pixel.

The two copies are the same art with one palette entry differing, and they rendered **identically**.
So "the RGB in slot 1 is incidental" is now a controlled result rather than an inference: the entry
was rewritten to bright magenta and the engine ignored it.

### Palette entries are stored blue, red, green, pad

Every index in the frame was paired with the pixel the engine painted at the corresponding screen
position. Fitting the six permutations of the stored triple:

| Permutation | Fits |
| --- | --- |
| `(p1, p2, p0)` | **14 / 14** |
| `(p2, p1, p0)` — the reversal we shipped | 4 / 14 |
| the other four | 1-2 / 14 |

The authored entries confirm it independently: raw `ff 00 00` rendered **blue**, `00 ff 00` rendered
**red**, `00 00 ff` rendered **green**.

`src/imp.rs` mapped `|bgra| [bgra[2], bgra[1], bgra[0], 255]`, a reversal, which **swaps red and
green and leaves blue correct**. That is precisely the symptom this log has carried since the first
capture — *"agree wherever red equals green and disagree where they differ"* — recorded accurately
and left unexplained. Corrected to `|brg| [brg[1], brg[2], brg[0], 255]`; against the engine capture
the old mapping scores 3/10 and the new one 10/10.

Two consequences worth stating. Every PNG the viewer has exported has red and green swapped. And the
community specification's "stored BGRA, swapped to RGB" is **refuted** — we had accepted it in
[Stage 1](native-asset-stage.md) as confirmation, so a wrong claim was used to close a question our
own evidence was already contradicting.

**The off-by-one alternative was ruled out, not assumed away.** "Entries are `[R,G,B,pad]` and our
palette offset is one byte early" predicts blue coming from the fourth byte. The fourth byte is zero
for all 256 entries, while index 228 stores `(82, 49, 0)` and rendered blue 80. Blue comes from the
first byte.

### The BMP byte order, settled numerically

`screencapture` writes pixels **R, G, B**, not the BMP-standard B, G, R. Judging this by eye is
unsound, so it was measured on materials whose hue is not in question — the carved stone interface,
its wooden portrait panel and parchment. Green is the middle byte under both candidate orders and
cannot discriminate; only the warm/cool axis can:

| Region | first byte dominant | last byte dominant |
| --- | --- | --- |
| interface stone, left of the portrait | 73.6% | 4.7% |
| interface stone, right panel | 90.8% | 0.8% |
| whole interface band | 62.1% | 2.4% |

Stone, wood and parchment are not blue. The first byte is red.

### Also confirmed, and one thing still open

`map2screen`'s **third return value is screen x** to under a pixel: predicted anchors 456.177,
252.530 and 184.648 against measured 456, 252 and 184. The placement rule held in a live game for
all three measurable sprites.

**Still open:** the measured anchor *y* is not linear in the cell — 156, 167, 230, 265 across cells
60, 62, 66, 68 of one row — while `map2screen`'s first two values are perfectly linear at 14.4 per
isometric step. The obvious candidate is terrain elevation, which the probe passed as `z = 0`. That
is the remaining piece of the y convention and it now has a testable shape: place the same sprite on
cells of known differing terrain height and see whether the residual tracks it.

## 2026-09-17 (later) — `map2screen` decoded completely; the drawn y is close but not explained

**Evidence class: observed in gameplay.** One keypress: an 81-cell survey calling `map2screen` twice
per cell — once with `z = 0`, once with `z =` that cell's `getelevation` — plus six sprite placements
to measure real anchors. Cleanup again left the screen pixel-identical to the plate.

### The operator, in full

Over all 81 cells, without exception:

```
map2screen(x, y, z) -> ( 14.4 * (x + y) + K ,           output 1, independent of z
                         output1 - 80 - 20.3625 * z ,   output 2  [see correction below]
                         33.941 * (x - y) + L )         output 3 = SCREEN X
```

- **Output 3 is screen x**, and only that. It is a function of `x - y` alone, never moved with `z`
  in any of the 81 cells, and predicted the drawn left edge exactly at 88, 190, 393 and 495 across
  the six placements — as it did for three placements in the earlier run.
- **Output 1 is a function of `x + y` alone**, stepping 14.4 per isometric step, and `z` never moved
  it in any cell.
- **Output 2 is exactly `output1 - 80`, less `20.3625` per unit of `z`.** *Corrected 2026-09-17: the
  `80` is this run's **camera scroll**, not a constant of the operator — see the last entry in this
  log, where a control sprite on unchanging ground moved it 40 pixels.* The z coefficient held
  between 20.3600 and 20.3680 across all 69 cells with non-zero elevation.

So **the third input is the elevation**, and `getelevation` is the operator that supplies it — which
answers the question this probe was built for. The constants are a plain isometric projection:
`33.941 = 24 * sqrt(2)` and `20.3625 ~ 14.4 * sqrt(2)`.

### The drawn y is *approximately* output 2, and that gap is unexplained

*Corrected 2026-09-17: both the slope and these residuals are artefacts of fitting across cells with
different neighbourhoods. The drawn top is `output2 - 973.4` at slope exactly 1 — see the last entry.*

Fitting the six measured tops against output 2 gives `top = 0.9652 * output2 - 1822.09`, and the
residuals are **up to 11 pixels**:

| # | cell | elevation | measured top | predicted | error |
| --- | --- | --- | --- | --- | --- |
| 1 | (60,70) | 1.0 | 73 | 73.19 | +0.19 |
| 3 | (70,71) | 0.75 | 228 | 230.99 | +2.99 |
| 5 | (64,68) | 2.0 | 88 | 81.33 | -6.67 |
| 0 | (58,71) | 0.5 | 62 | 69.12 | +7.12 |
| 4 | (64,74) | 2.0 | 157 | 164.72 | +7.72 |
| 2 | (67,71) | 1.75 | 181 | 169.64 | -11.36 |

Compare the x axis, which agrees to under a pixel. Eleven pixels is far outside that.

The decisive pair is 2 and 4: **the same `x + y`, elevations 1.75 and 2.0, and tops 24 pixels
apart.** Output 2 differs by only 5.09 between them, so no rescaling of output 2 can produce a
24-pixel separation. Whatever vertical term the renderer uses, it is not the anchor cell's
`getelevation` fed through output 2.

The obvious candidate is that the renderer interpolates height across the terrain mesh rather than
sampling the cell, and the three largest errors are indeed on the cells whose 3x3 neighbourhood
departs most from the cell's own value (2.0 against a neighbourhood mean of 1.25 at placement 5, for
instance). **But the signs do not line up** — placements 2 and 5 both sit above their neighbourhood
mean and their errors have opposite signs — so this is a hypothesis with a counter-example in hand,
not a finding.

**Two honest caveats.** Two pairs of sprites shared a screen x, and the assignment within each pair
was chosen by whichever permutation fit best; that is convenient reasoning, and a rerun should place
six sprites at six *distinct* screen x values instead. And placements 0 and 3 fell outside the
surveyed square, so their neighbourhoods are unknown.

### The experiment that would settle it

Place sprites only on cells whose 3x3 neighbourhood is **uniform**, which the survey can find before
choosing where to place. If the residuals collapse to about a pixel on flat ground, the renderer
interpolates and the y convention is closed; if they do not, the extra term is something else. Give
each sprite its own screen x so no assignment is ever inferred.

### Checking the community threads rather than relaying them

Four claims from impz threads 2012 and 2086 were tested here instead of being taken on trust. Three
survived in part, one did not, and one of our own statements needed narrowing.

| Claim | Source | Outcome |
| --- | --- | --- |
| The RLE algorithm, with the `+3` bias | snv, 2011 | **Confirmed** — matches `decode_rle_packet` line for line |
| `u1 Palette[256*4]; // RGBA palette` | snv, 2011 | Size confirmed, **order refuted**: stored blue, red, green, pad |
| "index 0xff is RLE special value" | snv, 2011 | **Refuted** as a palette claim: index 255 is ordinary pixel data in 164 of 1,800 files, 7,070 frames, 6.25M pixels, across every asset category |
| "Pure Red and Pure Green... have to be the first two colors" | Boaster, 2023 | **Order answered, requirement refuted** — see below |
| `LOM_Sprite_Tool` preserves and adjusts placement | Hexdragon, 2023 | **Unverified.** The tool has not been obtained or run; recorded as a community claim |

**Boaster's ordering, measured across all 1,800 shipped IMPs:** 1,542 files (85.7%) do hold pure red
at index 0 and pure green at index 1. The other 258 do not — 167 hold black and `(8,8,8)`, 56 hold
pure red and a cyan, 35 something else. They render correctly anyway, because the engine keys on the
**index** and ignores the colour, which the magenta test proved directly. So the convention is an
art-pipeline habit, not an engine constraint: repainting a palette must preserve those two *indices*,
not those two colours.

**IMP Studio carries the same red/green swap**, established by reading its code rather than its
documentation:

```js
return [p[i*4+2], p[i*4+1], p[i*4]];
```

The engine's layout requires `[p[i*4+1], p[i*4+2], p[i*4]]`. Every colour that tool has displayed or
exported has red and green transposed, exactly as ours did — and that is how the wrong order
survived scrutiny here. Our decoder and the community tool agreed with each other, and
**agreement between two implementations was mistaken for confirmation from evidence.** Neither had
been checked against the engine until now.


## 2026-09-17 (later still) — A board sweep, and the two shipped files it pointed at

Six pages of the LOMSE Modding board were listed (169 threads) and twelve read. Most threads carried
nothing testable. Two of them named shipped script files this project had never opened, and those
files answered a parked question and supplied a better probe harness than the one we built.

The threads themselves are **Documented** evidence at best. Everything below that is labelled
Observed was read out of the shipped GS5R3 `gs.mpq`, not out of a forum post.

### `gs\rmg.gs` and `gs\edit\mapgen.gs`: issue #22 is not blocked any more

Thread 2206 mentioned in passing that map generation is driven by "an rmg file in the gs folder".
There are two: `gs\rmg2.gs`, which only defines `make_big_random_map` at a fixed 128x128, and
`gs\rmg.gs`, which adds `make_custom_random_map` taking the dimensions as operands.

**Observed in the archive**, `gs\edit\mapgen.gs` line 306 is the only call site, and it settles the
operand order — width first:

```
newmapdict begin map_width 32 mul map_height 32 mul end make_custom_random_map
```

The same file shows the New Map dialog's range. `map_width` and `map_height` are held in units of
32 at map zoom (4 at dungeon zoom), and the three presets offered are:

| Preset | Dungeon zoom | Map zoom |
| --- | ---: | ---: |
| red | 4 | 32 |
| yellow | 64 | 512 |
| green | 128 | 1024 |

So the shipped GS5R3 editor already generates maps from **32 to 1024** in steps of 32. It also warns
about exactly the failure eyesodilated described, at `mapgen.gs` line 192:

> Do not attempt to generate maps for play with any other mod or unmodded version of the game with
> Random Dungeons turned 'ON' for maps with a dimension greater than 128, or else the game will
> experience errors when loading the map for game play.

That is independent support for the *phenomenon* behind the "4-byte header disappears" claim, and
says nothing about the mechanism. The mechanism is still ours to measure — and now we can, because
we no longer need someone else to hand us an oversized map.

`gs\hotkey.gs` line 562 supplies the other half. Saving takes one string operand:

```
mapfilename savescenariomap        ; .scn
mapfilename savespecialmap         ; .smp
```

Which makes the whole test one hotkey body and one keypress:

```
512 512 make_custom_random_map
"map/zz512.scn" savescenariomap pop
```

[Issue #22](https://github.com/jake-bliss/lords-of-magic-modding/issues/22) moves from blocked to
ready. See [map format](map-format.md) for what the resulting file is supposed to tell us.

### `gs\generate.gs`: a synthetic map beats a shipped one

`/generate_simple_game` builds an entire playable scenario from script with no user input:

```
64 64 newmap 392 clearmap 0 unknowndarken resetvisibility clearregions
0 63 63 0 markborder findregions
1 maxslope 20 43 1.5 33 32 5.0 tt_mountain ridge ...
.3 maxslope 16 16 tt_happy 1.0 paintelevation 24 20 tt_desert 0.75 paintelevation
16 16 LIFE LIFE addcapitol 48 48 DEATH DEATH addcapitol
unittypedict begin 20 16 liinf 1 -1 addunit 52 48 deinf 0 -1 addunit end
resetnetworking 2 newgame 1 1 setplayeraistatus ... create3dmap gamemode
```

This is the harness the open `map2screen` y question actually wants. The residual problem there is
that the drawn y depends on a terrain mesh we did not choose and can only survey after the fact. A
map built by `newmap` and painted by `paintelevation` is one we specify: flat where we want it flat,
stepped where we want a step, with a unit at a cell we picked. It removes the "placements 0 and 3
fell outside the surveyed square" caveat by construction.

Not yet run. Recorded as the preferred rig for the next elevation experiment.

### `addterrainsprite` takes three operands, and now for a reason

Our probe emits `x y ttype addterrainsprite`. It worked, but several shipped call sites read as
though the operator took four:

```
cx cy f terrainsprites begin keep_ttype end addterrainsprite
```

It does not. **Observed in the archive**, `gs\tree.gs` line 178:

```
/great_temple{/dummy exch get}/dummy great_temple_array replace bind def
```

`great_temple`, `keep_ttype` and `leader_ttype` are *procedures* inside the `terrainsprites` dict.
Each consumes the faith and returns one type id, so `f terrainsprites begin keep_ttype end` is a
single value by the time `addterrainsprite` sees it. The three-operand sites — `s_x s_y
terrainsprites /tower3 get addterrainsprite`, `x y esp03 addterrainsprite`, `xy_to_x_y
terrainsprites begin dirtpil end addterrainsprite` — are the same call. Arity is 3.

This is the rule from [the arity section](gamescript-format.md#operator-arity-recovered-from-the-code)
working as intended: the recovered table cannot say what operands *are*, call sites can, and a call
site that looks like a counter-example has to be read rather than counted.

### Elevation is a float, and the forum's "1.0 is now 10" is about a different program

The shipped generator passes fractional elevations throughout: `0.11`, `0.5`, `1.0`, `2.0`, `6.3`,
and `-1.0`. `paintelevation` takes `x y terrain elevation`, `setelevation` takes `x y elevation`, and
`getelevation` takes `x y`, all as coordinate **pairs** — not the packed locations that
`getterrainspritelocation` and `anythinglocation` return.

Thread 2334 says "the previous elevation of 1.0 is effective to 10". That is a GSZ **map editor** UI
change, made by Boaster in 2020, not engine behaviour. It must not be folded into the `map2screen`
z coefficient of 20.3625 per unit, which was measured against the shipped engine.

### What the rest of the board was worth

| Thread | Subject | Outcome |
| --- | --- | --- |
| 2033 | Game Script Manual | Paid PDF, $5.99. Its own contents list is unit, artifact and spell editing across 25 pages — no interpreter, no operator reference, no file formats. **Not worth buying**; we recovered 1,906 operators from the binary |
| 2542 | LOM source code search | No source exists; Rebellion holds the rights. Ghidra yields ~684,794 lines. Repeats the claim that "the mpq files are not fully decryptable", which our injection path refutes |
| 2316 | Decompile/recompile LOMSE.exe | No result. Poses one answerable question: `UNIT_WIZARD_MANA` is locked to -1 for non-wizards and is believed to live in the exe |
| 2426 | `gs\edit\radius_terrain` | The only `.txt` in `gs.mpq`. Boaster: "That may have been something I left in there" |
| 2428 | LOMSE Borderless | cnc-ddraw wrapper. No resolution or coordinate detail |
| 2014 | lomut | Format support only; no offsets, no hotspot detail |
| 2083 | `waicons.imp` fix | A colour improvement with no technical description |
| 2406 | Alternate web server | A download mirror at `lomse.ddns.net`; mod builds only, no documents |
| 2493 | "Corrupted" images in `pic.mpq` | Not corrupt. The reporter had no listfile. Repeats the red/green palette convention, for LBM |
| 2247 | Editing small_doodads | **A naming trap.** `small_doodad` is a roster-sprite override keyed on `code` (WM1, WM2, FIT, THF), resolved through `gs\graphics5.gs`. It has nothing to do with terrain doodads |
| 2222 | GSZ all map sizes | Announcement: 32x32 to "1024x1024, or perhaps larger". No implementation detail. The shipped editor already does this range |
| 2334 | Map editor updates | Elevation UI change above, plus a real engine note: `genericpanel` calls `copydoodad`, whose graphics never unload, so `maxgraphics` in `START.gs` had to be roughly doubled |

### The thing we should give back

Thread 2437, May 2023, someone building new unit sprites in Blender:

> When I add the pallet to the sprite images the Red shadow is getting blended into the sprite, same
> with transparent Green. I have solved this by compositing. The green is background, and the red
> shadow is foreground and all three images get put together after adjusting the pallet and then
> applying the pallet right after they are joined.

By their own account that workaround costs about 300% more processing. It is unnecessary. The engine
keys on the palette **index** and ignores the entry's colour — index 0 is transparent, index 1 draws
the background at half brightness — which the magenta rewrite proved directly on 2026-09-17. An
author needs the two indices right and may paint them anything at all.

## 2026-09-17 (later) — The oversized map, generated here: the header does not disappear

Issue #22 was parked for want of a map larger than 256x256. It did not need one from outside. The
shipped GS5R3 editor generates up to 1024, so the map was generated in the running engine, saved,
and parsed.

**Observed in gameplay**, one keypress in the Map Editor, `LOM_PROBE=mapsize`:

```
map size ladder start
gen begin 128   gen done 128  mapw 128  maph 128   save 128 name map/zz128.scn result -1
gen begin 256   gen done 256  mapw 256  maph 256   save 256 name map/zz256.scn result -1
gen begin 512   gen done 512  mapw 512  maph 512   save 512 name map/zz512.scn result -1
map size ladder done
```

`mapw` and `maph` report the engine's own view of the live map, so the engine accepted 512x512
without clamping. All three saves returned the same value as the 128 control, which is what makes
`-1` readable as success rather than as an error code.

### The result

| File | Bytes | `[0x00]` | Declared | `16 + w*h*8` | Trailing | Records | Footer |
| --- | ---: | ---: | --- | ---: | ---: | ---: | --- |
| `zz128.scn` | 132,272 | `0x6f` | 128x128x8 | 131,088 | 1,184 | 24 | `01000000` |
| `zz256.scn` | 525,488 | `0x6f` | 256x256x8 | 524,304 | 1,184 | 24 | `01000000` |
| `zz512.scn` | 2,098,352 | `0x6f` | 512x512x8 | 2,097,168 | 1,184 | 24 | `01000000` |

**The claim is refuted.** The four-byte word at `0x00` is present in a 512x512 map, holding the same
`0x6f` as the 128 and the 256. Every file is exactly `16 + width x height x 8 + 1,184` bytes, so the
16-byte prefix is intact at every size and nothing downstream is shifted. The native parser reads
all three unchanged, with no code path for an oversized map and none needed:

> Lords of Magic stores a 4-byte compression header in every map. It's basically just a version
> number and reserved space. However, when maps exceed the original maximum size, that header
> disappears.

Both halves of that are now answered. The "version number" half was refuted by measurement on
2026-09-16 — the word takes 20+ values across the corpus, independent of geometry. The "disappears
when oversized" half is refuted here, by the engine's own writer.

The underlying phenomenon is real and has a different cause. `gs\edit\mapgen.gs` line 192 warns that
maps over 128 in a dimension break **random dungeon placement** outside GS5R3, which is a script
concern, not a header one.

### What the controls bought

The 128 and 256 rungs are why this is a result rather than an anecdote. Both sizes exist in the
shipped corpus, so the generated files can be checked against what the game itself ships: `0x6f`
falls inside the `0x6c`-`0x6f` band world `.scn` files occupy, the 16-byte prefix matches, the
trailing section is the dominant 49-byte family with an 8-byte frame, and the footer value `1` is
one of the three observed. The generator writes what the game writes, so the 512 is evidence about
the format and not about the generator.

They also carry two findings of their own:

- **`0x6f` now appears at 512x512 as well as 32, 48, 64, 128 and 256.** One more size the word is
  indifferent to, and all three files here came from the same generator with the same tile set,
  which is what the tileset-selector hypothesis predicts.
- **X-major cell indexing holds at 512.** Every one of the 24 records in each file satisfies
  `cell_index = x * height + y` within bounds — record 0 of `zz512.scn` is cell 241,316 at
  (471, 164), and `471 * 512 + 164 = 241,316`. The convention was established on maps no larger
  than 256; it does not change when the index no longer fits in 16 bits.

The three files carry an identical 24-record placement set at every size — same instance ids 200
upward, same sprite-type sequence, only the cells differ. That is the random map generator placing a
keep, a leader and a great temple for each of eight faiths, and it makes the record layout
demonstrably independent of map size.

### Cost

One keypress. The 512 took about two minutes of single-threaded script; the run start to finish was
under five. Nothing was placed on a map that mattered, nothing was destroyed, and the three
generated files were collected into `artifacts/` and removed from the game directory, which is back
to its 366 shipped files.

## 2026-09-17 (last) — The drawn y, closed: the renderer reads the mesh, not the cell

**Evidence class: observed in gameplay.** One keypress. The probe built its own 64x64 map, wrote
every elevation, pointed the camera with `centeron`, and photographed the same cells three times:
flat, on a uniform raised plateau, and on single-cell spikes. Six captures and a 10 KB log.

Three things came out, and the first two were not what the probe was built to find.

### The `-80` in our own formula was never a constant

A control sprite stood at (35, 41) on ground that is flat in all three phases. Its `map2screen`
output 2, called with `z = 0` every time, moved anyway:

| Phase | control `output2` | control drawn top | difference |
| --- | ---: | ---: | ---: |
| flat | 1258.4 | 285 | -973.4 |
| plateau | 1298.4 | 325 | -973.4 |
| spike | 1278.4 | 305 | -973.4 |

Its elevation never changed and its output moved 40 pixels, so that term is the **camera's vertical
scroll**, and `map2screen` already contains it. The `- 80` recorded on 2026-09-17 as part of
`output2 = output1 - 80 - 20.3625 * z` was that run's scroll, not a property of the operator.

### The drawn y is output 2 exactly, at slope 1

The earlier fit was `top = 0.9652 * output2 - 1822.09`, with residuals to 11 pixels. Both the slope
and the residuals were artefacts. Measured here, `top - output2` is **-973.4 in all three phases**,
across a 40-pixel camera move, and on flat and plateau ground the eleven other observations run from
-973.2 to -974.1 — under a pixel, with no scaling term at all.

The old 0.9652 came from fitting a straight line across cells whose *neighbours* differed, which is
the next section.

### The renderer does not use `getelevation`

The experiment. Six cells, identical `getelevation` of 2.0, identical screen x, two different
surroundings. With the camera removed by subtracting the control:

| Phase | cell elevation | neighbourhood | displacement from flat | implied `z` |
| --- | ---: | --- | ---: | ---: |
| plateau | 2.0 | uniform 2.0 | 41 px | **2.004** |
| spike | 2.0 | 1.0-1.5 ring | 28.4 px | **1.395** |

Same cell, same elevation, **12.6 pixels apart**. Where the neighbourhood is uniform the effective
height *is* the cell's elevation, to three decimal places. Where it is not, it is not.

So the third input to `map2screen` is the **interpolated mesh height**, and feeding `getelevation`
into it is correct only on flat ground. That closes the y convention and explains the 11-pixel
residuals: they were measured on a shipped world map where no two placements shared a neighbourhood.

The five measurements were 29, 28, 28, 28, 29 pixels. The maximum of the cell's four corner means —
each corner being the mean of the four cells meeting there — is 1.375, which is 28.0 pixels: three
of five exactly, the other two within a pixel. The corner *mean*, 1.3125, predicts 26.7 and is
excluded by the smallest measurement. **That is one rival ruled out on one neighbourhood shape, not
a kernel established.** `tools/map_projection.py` carries the model with that caveat in its
docstring.

### `setelevation` is clamped, and the survey is why we know

The spikes were meant to sit at 2.0 in a ring of 0. They did not:

```
1.0 1.0 1.0
1.0 2.0 1.5      <- the ring the engine left behind
1.0 1.0 1.0
```

The engine enforces a slope limit. The experiment survived — two different neighbourhoods at one
cell elevation is still the comparison it needed — but the contrast was smaller than designed, and
every number above rests on the ring being 1.0-1.5 rather than 0.

**This is the probe's per-cell survey doing its job.** Without it the clamp would have been invisible
and the write-up would have attributed the displacement to a neighbourhood of zeros. A reviewer
flagged `maxslope` as something they could not settle from the corpus and noted that the survey
would expose it. It did.

### One sprite produced no pixels

Place 2, cell (32, 32), logged normally in all three phases and contributed **zero changed pixels**
every time. The other five and the control behaved identically to each other. (32, 32) is the exact
centre of a 64x64 map, so the likeliest explanation is that something already occupies that cell and
`addterrainsprite` declined — but that is a guess. The next probe to place sprites should log
`enumterrainsprites` counts either side of each placement, which would settle it for free.

Nothing else was anomalous: `cleanup left 0` after all three phases, so the doubled sweep worked and
no sprite survived into the next phase's plate.

### Cost

One keypress, well under a minute of game time. The map was the probe's own creation, nothing was
saved, and both archives verify against the manifest.

## 2026-09-17 (final) — IMP validation closed: what the generated headers actually describe

> **Partly corrected the same day.** Five of the ten exceptions below were a decoder bug of ours,
> and the section explaining them is refuted. The current counters are **1,795 exact matches, 5
> exceptions, 0 failures, 2 documented orphans**. Read this entry together with
> "2026-09-17 (correction)" at the end of the log; it is left standing because it was written in
> good faith and the way it went wrong is the useful part.

`--validate-imp` reported 1,788 exact matches, 10 failures and 4 orphans. It now reports **zero
failures**: 1,790 exact matches, 10 named value-pinned exceptions, 2 pairs recovered by a new
pairing fallback, and 2 documented orphans. Getting there turned on three findings.

### The validator was reporting one disagreement per file

**Corrected.** `ImpSprite::validate_against` used `?` on each comparison in a fixed order, so every
failing file reported only its *first* disagreement. Collecting all of them changed the shape of the
problem immediately: the five files filed as "duplicate frame count only" in fact also disagree on
raw-pixel, hotspot **and** stored-pixel bytes. Any explanation that addressed the duplicate tally
alone was addressing a third of the symptom.

### "Duplicate bitmaps found" counts the tool's input, not the file — **REFUTED 2026-09-17**

> **This subsection is wrong and is kept for the record.** Five of the ten exceptions were a bug in
> our own frame-table decoder, not an archive artefact; with it fixed, those five files match their
> headers on every statistic and there is no deduplication gap to explain. The reasoning below was
> fitted to numbers the bug produced, and — the part that matters — it was filed as **Observed**
> when it was an inference. See "2026-09-17 (correction)" at the end of this log. What follows is
> the entry as originally written.

**Observed in a local binary.** Across all 1,800 pairs, `binary_duplicates >= header_duplicates`
holds on 1,799 — the sole violation being `units\imp\orcr4b`, whose frame count independently
disagrees. **Inferred** from that: `orcr4b`'s header describes some other build. (**Corrected
2026-09-17:** this said the header "provably does not describe it". Nothing here proves that;
disagreeing statistics are consistent with a stale header *and* with a decoder fault, which is
exactly what the five files below turned out to be. Worse, excluding the one counterexample from
the population on the strength of the invariant it violates is circular — it is the only evidence
that could have falsified the invariant. The denominator was also wrong: 1,800 includes two pairs
made against a *foreign* header, where agreement is not evidence about a build tool's own output.
The measurement over the population it is meaningful for is 1,797 of 1,798 stem pairs.) The
statistic therefore counts duplicates among the build tool's *input* bitmaps; the written file
dedupes at least as much and never less.

Two things corroborate it rather than merely fitting it. The sequence and frame counts agree exactly
on all five affected files, so the header is not describing different art. And with
`header_distinct = frames - header_duplicates`, the header's hotspot-byte total is exactly
`binary_hotspot_bytes / binary_distinct * header_distinct` on four of five: `aicr3b` 840/35*36 = 864,
`ficr3b` likewise, `ficr5b` 720/45*46 = 736, `chwmmb` 640/40*65 = 1,040. `chcr3b` predicts 1,152
against an observed 1,136, because the one extra input bitmap carried a 16-byte hotspot array rather
than that file's usual 32. Every raw and stored total is larger in the header too, by 12 bytes on
`aicr3b` (one tiny extra bitmap) up to 12,865 on `chwmmb` (25 of them).

The scratch instrument built to count distinct records earned nothing and was deleted: the distinct
bitmap count is exactly `frame_count - duplicate_frame_count`, which `ImpSprite` already reported.
`ImpSprite::distinct_bitmap_count()` now names it. (**Deleted 2026-09-17:** it existed only to serve
the refuted hypothesis, it had no caller outside its own test, and that test could not fail — the
fixture has one frame and no duplicates, so both sides of its assertion read 1 under any mutation.)

### The archive's `.h` members are not reliably their own

**Observed in a local binary, and the finding that reframed the rest.** Of the 1,800 generated
headers, **602 declare a sequence name other than their stem**, and **388 of them fall into 115
groups of byte-identical files** — 17 `missile\*.h` members are one shared file declaring `spl01ap`,
21 share another. A `.h`
here is a build artefact that was freely copied. It cannot be assumed to describe the `.imp` beside
it.

That explains the remaining five. `missile\lsp01ap.h` claims 540,672 raw bytes, exactly
`33 * 128 * 128` — a uniform uncropped canvas — while the file's maximum frame size is 60x83 and its
33 frames are cropped; the structure otherwise agrees exactly (**Inferred:** the header predates the
crop pass). `lifitam`, `lifitbm` and `lifitfm` agree on structure exactly and differ only in pixel
totals; `lifitfm.h` is byte-identical to `lifitam.h` and declares sequence `LIFITAM` (**Observed**),
and the two `.imp` members measure identically, so `lifitfm` fails exactly as `lifitam` does.
`units\imp\orcr4b` is the one file whose structure itself disagrees: its 7 sequences, 92 frames, 0
duplicates and 1,472 hotspot bytes match its sibling `units\imp\orcr4a` *exactly*, while its
header's 86/50/576 signature matches **no** member in the archive (**Inferred:** rebuilt from the
`orcr4a` source, header never regenerated).

**Not proven.** No `.imp` in the archive measures 170,700, 49,014, 540,672 or `orcr4b`'s 22,957 raw
bytes. No member can be pointed at as the true owner of any of those four statistics, so "the header
is stale" stays an inference from structural agreement rather than a demonstrated copy — except for
`lifitfm`, where the byte-identical header is direct evidence.

### The four orphans were naming artefacts

**Observed in a local binary.** `1798 * 2 + 4 == 3600`, the archive's exact member count, so nothing
was missing. Pairing now falls back to the sequence name the header *declares* when the stem finds
no counterpart, and two of the four resolve and then validate on every statistic:
`imp\fleemark.h` declares `UNMRKA` and matches `imp\unmrka.imp`; `units\imp\chwmcbm.h` declares
`DEWMHB` and matches `units\imp\dewmhb.imp`. The fallback is deliberately consulted only for
members the stem left unmatched, because declared names are far from unique. (**Corrected
2026-09-17:** "far from unique" was right and the code did not act on it — it took the first
candidate. Measured: the 1,800 headers declare 1,370 distinct sequence names, 155 names are declared
by more than one header, covering 585 headers, and `deaura` is declared by 32. The sprite-basename
index was last-wins over 5 colliding `.imp` basenames. Both sides now keep every candidate, prefer
one in the member's own directory, and report ambiguity instead of guessing. The two pairs we make
are each uniquely supported, so the result is unchanged.)

The other two cannot pair and carry catalog notes instead. `aura\lsp01ea.h` declares `SPL01EA`,
which has no `.imp` in the archive, and is byte-identical to `aura\fsp03aa.h` whose `.imp` matches
its statistics exactly — a stray header copy. `imp\fleemarka.imp` measures identically to
`imp\unmrka.imp` and no header declares `FLEEMARKA` — an art copy shipped without a header.

### The exception mechanism

`IMP_VALIDATION_EXCEPTIONS` is a ten-row table in `spikes/asset-viewer/src/imp.rs` (**five rows
since the 2026-09-17 correction, and two classes rather than three**), each row naming
the member, one of three classes, a reason, and **the exact measured numbers**. An exception applies
only when the observed disagreements equal the recorded list — same statistics, same order, same
values. A decoder change that moves a number, drops a disagreement or adds one re-fails the member.
Nothing is loosened globally: every member is still fully parsed and every statistic still compared.
A unit test additionally holds each row to the falsifiable claim its class makes, so a
`HeaderPredatesArtRevision` row that starts waiving a frame count fails the suite.

### Refuted, retained

The earlier `0x08`-only duplicate hypothesis stays refuted: validating "Duplicate bitmaps found"
against back-references alone raises corpus failures from 10 to **112**. The statistic counts both
`0x04` and `0x08`. (**Re-measured 2026-09-17** after the frame-table fix, on the same archive: 0 to
**107**. The conclusion is unchanged on either decoder.)

## 2026-09-17 (correction) — five of the ten IMP "exceptions" were our own decoder bug

A cross-model review of the entry above asked why a build tool would deduplicate bitmaps *after*
writing its header. Checking the claim against the archive instead of against the model produced a
different answer: five of the ten exceptions were never archive artefacts. They were records our
decoder swallowed.

### The bug

```rust
let repeated_facing = source[frame_table_offset] & 0x04 != 0;
```

The decoder read the shared-pixel flag of a facing's **first** frame record and, when it was set,
treated the entire facing as a repetition of that one record: every frame slot pointed at the same
16 bytes. It also skipped the bounds check for the full `facing_frames * 16` table on that path, so
the records it never looked at were never even required to exist.

A facing's frame table is an ordinary array. Only the first record may carry `0x04`. Measured record
flags (**Observed in a local binary**): `units\imp\aicr3b` sequence 2 facing 0 is `[04 00]`;
`units\imp\chwmmb` sequence 5 facings 0-4 are each `[04 00 00 00 00 00]`. Every record after the
first was dropped along with its pixels and its hotspot array:

| Member | Swallowed records | Swallowed raw pixels | Swallowed hotspot bytes |
| --- | ---: | ---: | ---: |
| `units\imp\aicr3b` | 1 | 12 | 24 |
| `units\imp\ficr3b` | 1 | 12 | 24 |
| `units\imp\ficr5b` | 1 | 30 | 16 |
| `units\imp\chcr3b` | 1 | 1,443 | 16 |
| `units\imp\chwmmb` | 25 | 12,865 | 400 |

**Observed in a local binary.** Those are exactly the deltas the five `HeaderPredatesDeduplication`
exceptions waived — all three quantities, all five files. With `frame_offset` computed as
`frame_table_offset + frame_index * 16` unconditionally and the full-table bounds check always run,
the corpus went `validated` 1,790 -> 1,795, `validated_with_exception` 10 -> 5,
`validation_failures` 0 -> 0, with nothing else moving. All five now agree with their headers on
every statistic.

### Why the wrong explanation survived

It fit the numbers, and the arithmetic looked like corroboration. Both are weaker than they felt.

- **The inequality proved nothing.** `binary_duplicates >= header_duplicates` on 1,797 of 1,798 stem
  pairs is exactly what you also see when the headers are simply correct. It was read as evidence
  for a deduplication pass because a deduplication pass had already been assumed.
- **Three of the four hotspot "predictions" were arithmetically forced.** Where the gap is one
  bitmap and a file's hotspot arrays are uniformly sized,
  `binary_hotspot_bytes / binary_distinct * header_distinct` *must* land on the header's total. It
  cannot fail, so it cannot confirm. Only `chwmmb`'s 25-bitmap fit carried information — and the
  swallowed-record measurement explains that one too.
- **The counterexample was absorbed by an epicycle.** `chcr3b` missed by 16 bytes, and the entry
  explained it by asserting that the extra input bitmap "carried a 16-byte hotspot array rather than
  this file's usual 32" — a property of a bitmap that is not in the archive and cannot be examined.
  An unfalsifiable patch on a hypothesis is the signal to stop defending it. That line is deleted.
- **The circular exclusion.** `orcr4b`, the single pair violating the inequality, was set aside as
  "provably not described by its header" — using the invariant to dismiss the one measurement that
  could have falsified it.

### The failure that matters

Not the wrong hypothesis. Wrong hypotheses are the normal cost of doing this. The failure is that an
**inference was written down as Observed**, in a repository whose whole discipline is that those two
words mean different things. Once "the header counts the tool's input bitmaps" was recorded as a
measurement, nothing downstream re-examined it; it became the frame every later number was read in,
and the five exceptions it created were carried as settled archive facts for as long as the label
held.

The mechanical safeguards were all present and all passed: the exception table was value-pinned to
exact measurements, a unit test held every row to the falsifiable claim its class makes, and the
corpus run was green. None of them could help, because they all checked consistency with the
decoder's own output. Nothing compared the decoder against the format.

### What changed as well

- `ImpExceptionClass::HeaderPredatesDeduplication` and its five rows are deleted. Five exceptions
  remain: `missile\lsp01ap`, `units\imp\lifitam`, `units\imp\lifitbm`, `units\imp\lifitfm`
  (`HeaderPredatesArtRevision`) and `units\imp\orcr4b` (`HeaderDescribesAnotherBuild`).
- `ImpSprite::distinct_bitmap_count()` is deleted. It existed for the refuted hypothesis, had no
  caller outside its own test, and that test could not fail.
- Three tests encoded the wrong model and asserted the bug's behaviour. They are rewritten around a
  fixture whose facing holds records `[04 00]` with a genuinely distinct second frame, and a new
  test pins that a truncated frame table is rejected on that path. Both fail against the old
  decoder.
- Catalogued orphans are now **read, parsed and re-measured** against pinned values. A member was
  previously accepted on its *name*: a truncated replacement at a catalogued name exited 0.
- The declared-sequence-name fallback and the sprite-basename index required a **unique** match.
  Measured: 1,800 headers declare 1,370 distinct sequence names, 155 names are declared by more than
  one header covering 585 headers, `deaura` by 32; 5 `.imp` basenames collide in the listfile.
  Ambiguity is now reported, not guessed. `ambiguous_pairings` is 0 on the shipped archive.
- One unreadable orphan header used to abort the whole run with `?` and print nothing. It is a
  failure line now, like every other read error in the function.
- The duplicate-tally counters print a denominator (`dedup_compared_stem_pairs`) and exclude the two
  foreign-header fallback pairs (`dedup_foreign_header_pairs`), which had been counted as
  confirmations.
- `validate_imp_archive` had no test at all, which is why all of the above lived there unexercised.
  The pairing and verdict rules are now separated from archive I/O and tested against synthetic
  members: unique match, ambiguous match, reused partner, malformed orphan, exact exception, altered
  exception, unreadable member and unexplained disagreement.

### Final counters

```
candidate_stems	1802
matched_pairs	1800
paired_by_stem	1798
paired_by_declared_sequence	2
ambiguous_pairings	0
validated	1795
validated_with_exception	5
validation_failures	0
documented_orphans	2
orphan_entries	0
dedup_compared_stem_pairs	1798
dedup_at_least_header	1797
dedup_below_header	1
dedup_foreign_header_pairs	2
failures	0
```
