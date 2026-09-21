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
- **Observed:** indexing `tilesb01.lbm` with `URAK.scn` produces a coherent map with connected ocean, snow, forest/grass, and desert regions. **[Corrected 2026-09-17: coherence is evidence that the masked tag indexes real terrain art rather than noise; it is NOT evidence of orientation. Transposing a world map yields another coherent world map, which is exactly why the X-major reading survived this check for months. Nothing in the repository establishes the render's orientation; the decisive test is comparing an exported preview against the game's own world map in an attended run.]**
- **Observed:** map cells are X-major (`index = x × height + y`). This agrees with placed-sprite cell coordinates and Map Editor script iteration; the diagnostic viewer's former transposition is corrected and regression-tested. **[Refuted 2026-09-17: the packing is `y × width + x`. Every shipped map is square, so neither the corpus nor the square regression fixture could tell the two apart — see the map cell tag entry.]**
- **Inferred:** tag bit `0x00800000` is the binary form of the editor's `forcetexture` operation. It occurs only in `.smp` files, and many 48×48 components flag exactly their 188 perimeter cells. **[Refuted 2026-09-17: an attended run forced a texture into all 4,096 cells of a fresh map with `clearmap` and seven more individually, and saved. No saved cell had the bit set. Forcing a texture does not set it. The bit's meaning is Unknown — see the map cell tag entry, and `CELL_TAG_HIGH_FLAG` in `spikes/asset-viewer/src/map.rs`, which keeps this reasoning so it is not re-derived from the same corpus shape.]**
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
  **[Corrected 2026-09-17, see the map cell tag entry below: the packing is `y * width + x`. These
  maps are square, so this rung could not distinguish the two and the arithmetic is unchanged; only
  the axis labels swap, and record 0 sits at (164, 471).]**

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

## 2026-09-17 (map cell tags) — Two documented claims refuted, and the terrain table read out of the engine

`LOM_PROBE=maptag`, one attended keypress, for [issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).
Every finding below is **Observed in gameplay** on that run unless labelled otherwise, and every one
was re-derived from the saved files with `--dump-map-cells` and `--diff-maps` before being written
here.

### What the probe did

Built a 64x64 map and `392 clearmap`'ed it, which calls `forcetexture` on all 4,096 cells. Then:

- row `y = 8`: `forcetexture` at `x = 8,10,…,20` with slots 0, 1, 2, 48, 96, 392, 623;
- row `y = 12`: `setterrain` at `x = 8,10,…,28` with terrain types 0 through 10;
- saved that state twice, as `.scn` (`savescenariomap`) and `.smp` (`savespecialmap`);
- minted a terrain sprite type with `addterrainspritetype`, placed three of it at `(20,30)`,
  `(21,30)`, `(20,31)`, and saved;
- destroyed all three and saved again.

Four files, plus two screen captures and a log. The values were all chosen **before** the run so
that the competing readings of the format would disagree on them.

### 1. The low tag bits are the tile-atlas slot, exactly

All seven forced slots round-trip byte-exact, including 0 and 623, the two ends of the atlas. This
was previously **Inferred** from the fact that masked corpus values happen to land inside the atlas;
it is now **Observed by construction**.

### 2. `0x00800000` is not a forced-texture flag — **Refuted**

Zero cells in the saved map have the bit set: not the 4,096 that `clearmap` wrote with
`forcetexture`, not the seven forced individually. The docs' "strong evidence that `0x00800000`
means a forced texture" was corpus pattern-matching — the bit appears only in `.smp` files, often on
exactly a map's perimeter — and the engine says no. **The meaning is Unknown again.** The accessor
survives, renamed to `MapCell::high_flag_set()` over `CELL_TAG_HIGH_FLAG`, so the code no longer
asserts a meaning; the masking survives too, because that part is independent and still holds.

### 3. Cells are packed y-major — the X-major claim is **Refuted**

`cell_index = y × width + x`. The three sprites were placed at coordinates chosen so the two
candidate encodings share no value, and the records carry **1940, 1941, 2004** = `y × 64 + x`.
X-major would have written 1310, 1374, 1311.

Which operand is *x* could not come from the bytes, because operand order and storage order are
exact transposes of each other. It came from `zg0.bmp`: `map2screen` gives screen-x proportional to
`(x − y)` and screen-down proportional to `(x + y)`, so a run with the **first** operand varying
must travel down-right and one with the second varying must travel down-left. Both painted bands run
down-right. First operand is x.

**Why this survived a full-corpus regression suite: every shipped map is square.** 128x128 world
maps, 48x48 special maps — the transpose is undetectable from the corpus, and the test that was
supposed to "prevent the earlier transposed rendering" had been fitted to a square 2x2 fixture that
agreed with both readings. The Rust tests now use deliberately non-square synthetic maps, and say so
in their names. *A fixture shaped like the corpus cannot catch a bug the corpus hides.* The rendered
map preview transposes as a result; that is the correction, not a regression.

### 4. `forcetexture` and `setterrain` differ in footprint

`forcetexture` writes exactly one cell: the forced row changed seven cells at `y = 8` and nothing at
`y = 7` or `y = 9`. `setterrain` writes the cell **plus blended transition tiles into its
8-neighbourhood**: the painted run along `y = 12`, `x = 8..28` changed rows 11, 12 **and** 13 across
`x = 7..29`, one cell beyond the run on every side. That is the actual semantic difference between
the two editor operations, and why both exist.

### 5. The terrain-type-to-tile table, both directions

| Type | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Tile | 175 | 392 | 111 | 159 | 207 | 255 | 15 | 303 | 351 | 459 | 469 |

Names are **Documented** in `gs\maplib.gs`: `tt_dirt`/`tt_rough` 0, `tt_water` 1,
`tt_desert`/`tt_sand` 2, `tt_mountain` 3, `tt_happy`/`tt_meadow` 4, `tt_ice`/`tt_snow` 5,
`tt_land`/`tt_plains` 6, `tt_swamp` 7, `tt_lava` 8, `tt_road` 9, `tt_impassible`/`tt_impassable` 10.

The inverse direction, `getterrain` on a cell whose type was **never** set and only its tile forced:
slot 0 → type 0, 1 → 6, 2 → 6, 48 → 0, 96 → 0, 392 → 1, 623 → 9. Two of those slots are not any
type's painted base tile and still answer with a type.

Both directions together establish that **terrain type is derived from the tile index via the
tileset, not stored in the cell** — consistent with tag bits `10..22` being unused corpus-wide. The
table is codified as data next to the map code with a unit test on the exact values, not as prose
here.

### 6. `savescenariomap` and `savespecialmap` write byte-identical files

`zzt0.scn` and `zzt0.smp` share one sha256, `7744b749…4a3c`. The `.scn`/`.smp` split is therefore
**not** a format difference, so the corpus's concentration of 52-byte trailing records in `.smp`
files has to be a **content** difference — different object kinds on special maps — not a different
writer. That redirects the remaining half of issue #4.

### 7. The trailing section: count first, footer last

`u32 record_count`, then `record_count × 49`, then a `u32` footer. Confirmed by construction: 0
records → 8 bytes, `00000000 01000000`; 3 records → 155 bytes, `03000000` + 147 + `01000000`. The
old description — "49-byte records plus eight fixed bytes" — was right about the total and did not
know which four were which. **The footer was `1` in both files**, so it is not a sprite-related
count. Meaning still Unknown.

### 8. Record fields

`sprite_type` at `+28` **is** the terrain sprite type id: all three records carry 470, the id
`addterrainspritetype` returned in the same keypress. Promoted from `sprite_type_candidate`.
`instance_id` at `+20` was 200, 201, 202 — sequential, starting at 200 on a fresh map, which matches
the corpus range `200..1659`. `attribute_bits` at `+24` was `0x00000001` on all three, for which
`attribute_code_candidate()` (`bits >> 28`) reports 0; that looks like a plain small integer being
read as a top nibble, so **the nibble reading is flagged suspect**. One sprite type cannot
distinguish the field's layout, so no replacement is asserted.

### 9. Placement and removal round-trip exactly

`zzt2.scn`, saved after destroying all three sprites, is byte-identical to `zzt0.scn`, saved before
any were placed. Nothing is left behind in the file, which is what makes a save diff a trustworthy
instrument on this format at all.

### Still open on issue #4

The meaning of `0x00800000`; the 52-/53-byte record families, now known to be a content rather than
a format difference; the 18 unmatched tails; the header word at `0x00`, which our generated maps
write as `0x6f` while shipped `URAK.scn` writes `0x6c`; the trailing footer; and the attribute field
at `+24`.

### What landed in code

`MapAsset::cell_index` and `PlacedSpriteRecord49::coordinates` corrected to the packed order, with
every call site — `--describe-map`, the terrain renderer, and the two new commands — following.
`CELL_TAG_FORCED_TEXTURE` → `CELL_TAG_HIGH_FLAG`, `tile_index_candidate` → `tile_index`,
`forced_texture_candidate` → `high_flag_set`, `sprite_type_candidate` → `sprite_type`. The terrain
table and the observed `getterrain` samples as constants. `--dump-map-cells` and `--diff-maps` in the
asset viewer, because reading a 16 KiB tail by eye is how a wrong record size gets believed.

### Cost

One keypress, a minute or so of game time. The map was the probe's own creation and the sprite type
was minted by the probe, so the cleanup could safely destroy by type and nothing shipped was at
risk. No archive was modified; the four saved files are proprietary-derived and stay in ignored
`artifacts/`.

## 2026-09-17 — A map writer, and the property it rests on

The first tool in this project that produces game data rather than describing it. It is a writer and
a set of CLI editing verbs, not a GUI; the GUI is the layer that goes on top once the bytes are
trustworthy.

### The result

`--map-roundtrip` over the installed `map/` directory: **365 checked, 365 byte-identical, 16,628
placed-sprite records rebuilt from their typed fields, 0 failures.** On a real 128×128 shipped map,
placing a terrain sprite and then removing it returns the file byte for byte — the same behaviour
the 2026-09-17 engine probe observed from the game itself, now reproduced by a tool the game never
ran.

### Why it can be correct while the format is not solved

Most of this format is still Unknown, so the writer never mints an unknown field. The header word at
`0x00`, the trailing footer, the record attribute field at `+24`, tag bit `0x00800000` and the whole
trailing section of every family this project has not decoded are all **copied**. What that buys is
an editor today instead of after the remaining unknowns fall. What it costs is that there is no
create-a-map-from-nothing mode, because three of those fields would have to be invented; the shipped
GS5R3 editor already generates from 32 to 1024 in steps of 32, so generate there and edit here.

Records are the exception, and deliberately: the decoded fields cover all 49 bytes with no gap, so a
record is rebuilt from its typed fields rather than carried across as bytes. That is what makes an
*edited* field land. `--map-roundtrip` checks it record by record, which is stricter than comparing
files — a file can round-trip through its raw tail while a field is written back wrong.

### What is deliberately not offered

`setterrain`'s transition blending. The 2026-09-17 probe measured its *footprint* — a run painted
along `y = 12` changed rows 11, 12 and 13 across `x = 7..29` — but not **which tiles** it blends in.
Approximating it would put plausible-looking wrong tiles into a map, and no test here could tell.
`--map-set-terrain` writes one cell, reproducing `forcetexture`, and says so on every run.

### The same test bit twice, and the second time it was the test that was wrong

`next_instance_id` was written to take the maximum live instance id and add one, with a doc comment
claiming a removed id is never reissued. It reissued it immediately. Fixed with a high-water mark
rebuilt at parse and never serialized, and a test that passed.

**The review then showed the fix does not hold where it matters.** The high-water mark dies with the
process, and the CLI edits exactly one file per process — so `place, place, remove, place` across
four invocations reissues the freed id, now pointing at a different cell. Verified on the built
binary. The test passed because it made all four edits against **one parsed map**, which is the only
scope where the guarantee is true and is not the scope anyone uses.

That is this project's own lesson landing on the person who wrote it down the same day: *a test that
cannot fail on the axis it is named for*. The in-memory test never went through bytes, so it could
not see the only thing that breaks the property.

The mechanism is kept — it is correct within one parse, which is the scope a future interactive
editor will have — but the guarantee is now documented as the limitation it is, in all three places
that stated it, and a second test round-trips through bytes between every edit and asserts the freed
id **does** come back. Pinning the real behaviour beats asserting the desired one. The format has
nowhere to persist a high-water mark and inventing a field would break the copy-never-mint rule, so
there is no fix available, only an honest statement.

### What the two reviewers each saw

Both independently found the instance-id reuse and the minted-fields overclaim, which is the
strongest signal either produced — two models, two harnesses, same two defects. Beyond that they
diverged, and the divergence was the useful part:

- **Codex alone** named the read-back guard as **tautological**. `verify_map_edit` asked
  `map.cell(x, y)`, which is the same `cell_index` the setter had just used, so on the coordinate
  axis the check could not fail: flip the packing in both and the guard still passes. It was being
  advertised as a safety property. The fix adds a second witness — the before/after cell diff, which
  catches an edit that strayed to another cell or to several — and the doc now says plainly that
  this does not make the packing formula independent of itself. That is held by the byte-offset test
  instead, which never calls `cell_index`. Codex also caught `paths_are_same_file` comparing
  canonical path *strings* rather than file identity, so two hardlinks to one inode read as
  different files; no overwrite was reachable, because `create_new` refuses either way, but the
  function did not do what its name said. It now compares device and inode.
- **Claude alone** caught `--map-roundtrip` exiting 0 on a directory with no maps — a green result
  from the command every other claim here leans on — and the unvalidated tile index, where a
  fat-fingered `3920` for `392` went straight into the tag word.

Neither list was usable as delivered. Codex's first report was a partial that found nothing and had
to be read again when the real one arrived; Claude's verdict was right but its severity ordering put
a doc-scope fix above a guard that could not fail.

### An overclaim the review caught

"Fields whose meaning is unknown are copied, never minted" was written in three files and is false
on the `--map-place-sprite` path. A record that did not exist has to get its bytes from somewhere:
`PlacedSpriteRecord49::new` mints nine fields, and one of them is `+24` — the very field the
sentence lists as never-minted, written as `0x00000001` because that is what the probe watched the
engine write, in direct contradiction of the corpus reading in which only the upper nibble varies.
Eight of the nine are corpus-invariant; that one is not. All three copies of the claim now scope it
to editing and name the exception.

### Applying the fixture lesson from earlier the same day

Every fixture here is **non-square** (5×3, 3×2, 11×3) and one is deliberately opaque-tailed, so the
undecoded-tail path is exercised rather than assumed. The packing test asserts against a computed
*byte offset*, not through `map.cell(x, y)` — going back through the same accessor the writer used
would agree with an X-major writer just as happily. Mutating `cell_index` back to `x * height + y`
fails three tests; that was checked rather than hoped for.

### Safety

The loose `map/` directory still has no backup. So: no in-place mode, an explicit output path on
every command, refusal to write over the input by canonical path, `create_new` on the output, and a
re-parse of the encoded bytes with the edit read back before anything reaches disk. A refused edit
leaves no partial file. No game file was written during this work; all experiments ran on copies in
a scratch directory.

## 2026-09-17 — The `mapload` probe, staged and not yet run

Built, installed, awaiting one keypress. The run sheet is [docs/mapload-run-sheet.md]
(mapload-run-sheet.md); it names what each outcome would mean before the run, so no result can be
reinterpreted after the fact.

### The gap it closes

Round-trip identity over 365 of 365 installed maps shows this project's writer matches the engine's
**writer**. It says nothing about the engine's **reader**, and no map this project produced has ever
been loaded by the game. The map editor's whole value rests on a claim nobody has tested.

`gs\hotkey.gs` turned out to supply the instrument outright: `loadscenariomap` takes a filename and
**returns a boolean** that the shipped editor tests. Acceptance is a value the engine hands back, not
something to be read off a screenshot. Finding that in the archive is what made this a one-keypress
experiment rather than a session of squinting at renders.

Each rung then saves the loaded map straight back out, so the offline diff of input against echo
reports whether the engine **normalised** anything. Any field it rewrites — the `0x00` header word,
the border bit, the `+24` attribute — shows up as a byte difference. One keypress, three unknowns.

### Two bugs caught before the run, both of which would have wasted it

**The blend background was laid with `setterrain`.** That is the operator under test, and it blends:
sweeping it across the map lays transitions against the default terrain and then partly overwrites
them, so the tiles around each blob could not be attributed to the blob. It uses `clearmap` now —
which forcetextures and blends nothing — and a test asserts no `setterrain` runs before the
background is down.

**The install script deleted the probe's own input maps.** `generated_map_names()` is the exact list
the install script clears as stale output, and adding `mapload`'s inputs to it meant the clearing
step removed the six files whose presence the script's own prerequisite check had just confirmed,
thirty lines earlier. Rungs 1 to 6 would each have loaded nothing, and the log would have read as
six rejections — the engine refusing our maps — when the files were simply absent. That is the worst
shape a probe bug can take: not a crash, but a plausible wrong answer.

The fix splits the list in two. `generated_map_outputs()` is what install clears; `generated_map_inputs()`
is what must already exist. Restore still removes both, or the probe's files outlive the probe.
`zm0.scn` sits in *outputs* despite being loaded, because the engine writes it during the run and a
leftover would replace rung 0's control.

It was the regression test, written from the intent rather than the code, that made the second one
obvious — the same shape of test that caught the instance-id reuse earlier the same day.

## 2026-09-17 — The `mapload` run: the engine accepts what we write

One keypress. All seven rungs passed, and it settled more than it was designed to ask.

### The question it closed

Every claim this project made about writing maps rested on round-trip identity — 365 of 365
installed maps re-encode to their input bytes. That shows this writer matches the engine's
**writer**. It says nothing about the engine's **reader**, and no map this project produced had ever
been loaded by the game.

`gs\hotkey.gs` supplied the instrument outright: `loadscenariomap` takes a filename and **returns a
boolean** the shipped editor tests. Finding that in the archive is what turned a session of
squinting at renders into a one-keypress experiment with a machine-readable verdict.

| Rung | Loaded | Engine reported | Echo vs input |
| ---: | :---: | --- | --- |
| 0 engine's own save (control) | yes | 64x64 | 4,096 cells |
| 1 our re-encode of `URAK.scn` | yes | 128x128 | 1 byte |
| 2 our terrain edit | yes | 128x128 | 1 byte |
| 3 our placed sprite | yes | 128x128 | 1 byte |
| 4 interior border bit | yes | 128x128 | 17 bytes |
| 5 created from nothing, 64x64 | yes | 64x64 | **identical** |
| 6 created from nothing, **96x64** | yes | **96x64** | **identical** |

Rung 2's three edited cells read back as terrain 1, 8 and 5 — water, lava, snow, exactly what was
written. The engine did not merely accept the file; it read the edits correctly. Rung 3 is the one
that mattered most for the writer's single honest compromise: the placed-sprite record, minted `+24`
and all, survived byte-exactly.

### The design decision that paid for itself

Each rung **saved the loaded map straight back out**. That was added because acceptance alone is a
thin result, and the echo diff turned out to answer two questions the probe was not built to ask.

**The header word at `0x00` is engine output, not map input.** `URAK.scn` carries `0x6c`; the echo
carries `0x6f`, which is what the engine writes for everything. That single byte is the entire
difference in rungs 1 through 4, and it retires the "stored tileset selector" reading — the engine
overwrites the word from its own state and never reads it back. It is also why the two
created-from-nothing maps came back identical: they were already written with `0x6f`.

**Tag bit `0x00800000` is not durable map data.** Rung 4's sixteen interior-flagged cells came back
cleared with their tiles untouched. The stronger half is rung 0, the engine's *own* map: a `clearmap`
save had the bit set on all 4,096 cells, and loading and re-saving cleared all 4,096. It is written
on save from in-memory state that a load does not repopulate, which makes it cosmetic to a writer.

That last result **appears to contradict** the earlier finding that `forcetexture` never sets the bit
(0 of 4,096) — and the reconciliation is that `forcetexture` *does* set it and something on the
render path clears it, because this control saved with no renderer call while the earlier probe
rebuilt and rendered first. Two measurements that disagree turned out to be a finding about the
*sequence*, not a reason to retract either.

**Both reviewers then showed the write-up of this was confounded, and they were right.** Two
separate claims were wrong:

- "the engine clears it when it saves a map it loaded" — every echo save in this run happens
  *after* the renderer block inside the same rung, so the sixteen cleared interior cells are
  equally explained by the renderer. Load and save were never isolated.
- "the two runs differ in exactly one step" — they differ by seven paint operators and four
  renderer calls. `resetvisibility`, `rendermap` and `refreshdirty` are as much candidates as
  `rebuild3dmap`, and the next probe should save between each.

The corrected write-up also flips the writer guidance. "Treat it as cosmetic" was the wrong
conclusion to draw from a confounded measurement: the corpus shows this bit living on disk in
exactly one place — 27,448 cells across 146 `.smp` files, precisely their perimeters — and this
probe never exercised the `.smp` path at all. **Preserve the bit.** An editor that dropped it on the
strength of "the engine doesn't keep it" could be destroying the only real occurrence of it.

### The blend ring has structure, and the structure is the result

Eleven isolated 3x3 blobs on a background forced to tile 15:

```
  18   2   2   2  19
   4  57  49  58   3
   4  51 398  52   3
   4  55  50  56   3
  17   1   1   1  16
```

N=2, S=1, W=4, E=3, NW=18, NE=19, SW=17, SE=16 — and that ring is **byte-for-byte identical for nine
of the eleven terrains**. A transition tile is chosen by the background and the direction of the
boundary, not by which terrain is on the other side. That is what makes a painter tractable: the
alternative, a full 11x11 pair table, would have cost eleven times the measurement.

Terrain 6 — the background's own type — has a ring of pure tile 15. No boundary, no transition. That
control is what proves the other rows measure something rather than reporting noise.
Terrain 9 (`tt_road`) has its own family, `384..390`.

One background only. The structure generalises; the numbers do not.

### A table that turned out to be conditional

The same map shows `setterrain` picking its *core* tile from a family too: terrain 6 onto a tile-15
background writes `385..391`, where the original measurement — against a tile-392 background — gave
15. So the terrain-to-tile table's `base_tile` column is a **representative** tile, not the tile
`setterrain` writes in an arbitrary neighbourhood.

Nothing downstream breaks. The reverse direction still holds, the blend background read back as
terrain 6 exactly as predicted, and what the writer does with the table — force one representative
tile into one cell, which is `forcetexture` semantics — was always right. But the column had been
carrying a stronger claim than the measurement supported, which is the third time in one day that a
recorded fact turned out to be narrower than its wording.

### Cost

One keypress. The game **crashed on exit**, after the probe had logged `map load probe done` — every
artifact was already on disk and nothing was lost. Whether that is the probe or Wine on shutdown is
untested. Archives restored and verified against `MANIFEST.sha256`; `map/` back to its 366 shipped
files with no leftovers.

### Post-review corrections to the run's write-up

Both reviewers converged on the confound above; each then found things the other missed, which is
the whole argument for running two.

**The reviewers between them caught eight claims that outran their evidence**, and the pattern in
all eight is the same: a single observation written as a general law.

- The header word was refuted on the **write** side and the wording claimed the **read** side. All
  seven rungs loaded successfully and nothing distinguished tileset behaviour between a `0x6c` map
  and a `0x6f` one. Worse, the confidence list still carried "the header word is a tileset
  selector" as *Inferred* four lines above the bullet refuting it — the canonical known/unknown
  block an agent reads first, self-contradicting.
- "A transition tile is chosen by the background and direction, **not** by which terrain is on the
  other side" is refuted by the same paragraph's own road row. Nine of ten is a useful regularity,
  not a law, and `LAND_TRANSITION_TILES` carried neither a road row nor anything to refuse one.
- The terrain-6 "control" was not a control. Its ring is pure background, but its *core* was
  rewritten to `385..391` rather than left at the background tile — so something was written and no
  ring appeared, which the "no boundary, no transition" story does not explain.
- The `385..391` family was said to round-trip through `getterrain`. The probe never reads
  `getterrain` on a blob cell, and `base_tile_terrain_type(385)` returns `None` in our own code.

One reviewer suspected the terrain-6 numbers were a transcription mix-up with terrain 9 and could
not check, because `restore-game-archives.sh` had removed the artifacts from the game directory. The
copies in the scratch directory survived: re-measured, blob 6 sits at (6,14) with core
`385..391` and an all-15 ring, blob 9 at (30,14) with core `474`/`546..553` and ring `384..390`, and
the far field is clean tile 15. **No transcription error** — but the reviewer was right that the
*reasoning* did not hold, and right that the artifacts should have been checkable.

**Two real code defects**, both in the verbs added to build the probe's inputs:

- `--map-flag-rect` and `--map-flag-border` had an **empty verification arm carrying a comment that
  claimed the edit verified itself**. It did not: only out-of-range errors propagated. A mask bug in
  `set_high_flag` that clobbered the tile field would have been encoded, reparsed and written into a
  directory with no backup, while every other verb refused. These are the only verbs that write many
  cells at once, which made it the worst one to leave unchecked. Now verified cell by cell, plus an
  assertion that nothing outside the region moved — and mutation-checked by making `set_high_flag`
  clobber the tile, which the new test catches.
- `MapAsset::create` allocated before bounding its dimensions, so `--map-create 100000 100000` died
  in the allocator instead of being refused like every other bad input.

**Three tests that could not fail on what they were named for:**

- The renderer-guard test checked that *something* appeared earlier in the file. The control's
  `clearmap` made that unconditionally true, so moving the rebuild outside its `zok` guard left the
  suite green — reintroducing exactly the crash the run sheet says would take every later rung with
  it. It now asserts indentation depth, and moving the line fails it.
- Two tests pinned constants that **only the tests referenced**. The real values were hardcoded in
  `build-mapload-inputs.sh`, so changing the script to flag `0 0 3 3` left
  `test_the_interior_flag_rectangle_is_nowhere_near_an_edge` passing while rung 4 measured the
  border it was designed to avoid. Both now read the values out of the script and assert the script
  and the constants agree.
- And the input-list regression test asserted a Python set relationship while the two shell scripts
  that decide the list hardcoded `zm1..zm6`. Both scripts now read `mapload_prebuilt_names()`, and a
  test greps them to keep it that way. That matters because this is the same class as the bug
  already recorded above: a rung added but not built would log as *the engine rejected our map*.

Five new CLI verbs also shipped with no CLI-level tests, while every pre-existing edit verb had
four. The untested layer was the one that writes into the game directory, and that is precisely why
the `FlagRegion` gap was invisible.

## 2026-09-17 — `terrainrings`: one table, one call, and the sprite names

One keypress, three sections, all three delivered. The most useful result is that the thing it set
out to measure turned out to be much smaller than expected.

### Section A: eleven tables collapse into one

The `mapload` run measured `setterrain`'s transition ring against one background and found it shared
by nine of eleven painted terrains. The obvious next step was eleven separate tables. What the data
actually shows is **one** table plus a per-background anchor:

```
N −13   S −14   W −11   E −12   NW +3   NE +4   SW +2   SE +1
```

Subtract the anchor and all eight blending backgrounds are byte-identical. And the anchors —
`15, 63, 111, 159, 207, 255, 303, 351` — are a contiguous arithmetic run of stride 48, every one
congruent to 15 mod 48. The atlas is laid out in 48-tile terrain blocks and blending indexes within
a block.

That is a far stronger result than the eleven tables it replaces, and the reason is worth keeping:
**eleven independent tables could each have been a coincidence; one table that regenerates all
eleven cannot.** The Rust constant is generated from the saved maps rather than transcribed, and a
test regenerates every measured ring from the single table.

It also caught a trap. **Water's blending anchor is 63, while its representative tile is 392.** A
painter that used `terrain_type_base_tile` as the anchor would have taken water transitions from the
wrong 48-tile block, and the numbers would have looked plausible.

Three backgrounds do not give a uniform ring: `tt_dirt` and `tt_impassible` blend nothing at all,
and `tt_road` depends on the painted terrain with only its edges changing. Both no-transition
backgrounds are also the two whose representative tiles are off the block grid (175 and 469 are 31
and 37 mod 48) — suggestive, and recorded as suggestive, because two cases is not a rule.

And `tt_road` as a *painted* terrain is **ragged along every edge** on all seven backgrounds where
it blends. The offline analyser found that before this probe ran, by flagging non-uniform edges on
the `mapload` map; the hand analysis had reported road as "a different family" and collapsed the
raggedness by only quoting the set of tiles. A tool that refuses to average was worth writing.

### Section B: the hypothesis was wrong, and the isolation says so

The bit-clearing question was bracketed but not isolated, and the write-up of it was what both
reviewers caught. Five fresh maps, one renderer call each — fresh because the bit does not come back:

| sequence | bit set |
| --- | ---: |
| `clearmap`, save | 4096 / 4096 |
| `clearmap`, `rebuild3dmap`, save | 4096 / 4096 |
| `clearmap`, **`resetvisibility`**, save | **0 / 4096** |
| `clearmap`, `rendermap`, save | 4096 / 4096 |
| `clearmap`, `refreshdirty`, save | 4096 / 4096 |

**`resetvisibility`.** Not `rebuild3dmap`, which was the hypothesis in the corrected write-up *and*
in this probe's own run sheet. Writing the prediction down before the run is what makes being wrong
cheap and legible instead of invisible.

And the name is the finding: **the bit is visibility state, not terrain state.** The corpus carries
it on exactly the perimeter ring of 146 `.smp` files, and a visibility flag on a map's edge cells
reads very differently from a texture flag. Retroactively, that is also why "forced texture" never
fit.

### Section C: the sprite table, which was the real blocker

`terrainsprites` is a dict keyed by name, and `forall` enumerated all 197 entries: 178 plain
name-to-id pairs plus nine arrays and some procedures. The arrays are the per-faith tables —
`keep_array`, `vilg_array`, `great_temple_array`, `leader_ttype_array` — eight entries each, exactly
the set the random map generator was observed placing.

This was the gap between a terrain editor and a map editor. `sprite_type` is assigned in script
execution order across 536 `addterrainspritetype` sites, so a raw id says nothing about what it is.
`--map-place-sprite IN.scn 10 20 castle1 OUT.scn` now works, with suggestions on a near miss.

The table is **profile-specific** — a different script set shifts every id — and the CLI prints that
warning alongside the table rather than leaving it in a document. Raw ids stay accepted and
deliberately unchecked against the table, because ids above it are runtime registrations, which is
how the probe's own type 470 exists.

### Why all three rode in one keypress

An attended run costs a person's time, and the two riders were cheap in lines and independent of the
matrix. Ordering them last meant `forall`-over-a-dict and `cvs`-on-a-name — the least documented
things in the run — could only cost themselves. They worked, and the section that was least likely
to succeed is the one that moved the project furthest.

### Cost

One keypress, no crash. Sixteen maps and eleven captures written into the loose `map/` directory and
removed afterwards; archives restored and verified against `MANIFEST.sha256`; `map/` back to its 366
shipped files. Artifacts preserved at `artifacts/engine-probe-captures/terrainrings-20260917/`.

### Post-review corrections, second pass

Both reviewers were strong here and they diverged usefully. Codex went at the reasoning; the Claude
pass went to the **committed run artifacts** and re-derived every claim, which is why it found things
no amount of reading could have.

**The correction that matters most came from checking a reviewer's numbers against my own.** The
review said terrain 6's blob interior is `384..391` on every background, including the tile-392 one
the docs cited as a contrast, and that `385..391` was wrong at the low end. Both true. But my earlier
measurement of the *same* experiment had given a different set — because it was a different run.
Comparing the two runs directly:

| | core (3x3) | ring |
| --- | --- | --- |
| `mapload` `zb0.scn` | `388 390 386 391 385 388 388 390 389` | identical |
| `terrainrings` `zr6.scn` | `388 390 387 384 390 386 387 390 391` | identical |

**The ring is byte-identical across two independent attended runs; the interior is not.** The centre
cell was 385 in one and 390 in the other. So the interior is a **random draw** from the terrain's
eight-member `384 + 8k` family — verified for all eight blending terrains on all eleven backgrounds —
and no writer can reproduce it. That is a property of the engine, not a gap in the measurement, and
it is a better answer than either the docs or the review had.

The double result is worth more than either half: independent replication confirms the ring, and the
same comparison proves the interior unreproducible.

**Claims that were wrong, not merely overstated:**

- The generator's log slice was unbounded, so it counted two trailing lines as dict entries. Every
  "197 entries / 10 unaccounted" figure was two too high, and `--check` validated the wrong number —
  a checker confirming its own error. The truth is 195 rows logged, the probe's own counter said 196,
  so 178 pairs + 9 arrays + 8 name-only, and **one entry enumerated without logging anything**. The
  generator now bounds both ends, cross-checks the counter, and pins the silent gap at 1 so a change
  fails rather than a known gap failing forever. This was the *second* correction to the same
  arithmetic; the first was also a reviewer doing the subtraction.
- "Every corpus sprite id is either in the table or above its top, never a gap inside it" — false.
  The table has **60 gaps**, and 31 distinct ids across 113 records in the installed corpus land
  inside them, concentrated at 95..118 and 135..141, exactly where the log shows `keep_array`,
  `vilg_array` and `leader_ttype_array` in enumeration order. **The gaps are the per-faith types**,
  and they are the commonest objects on a real map. `--map-place-sprite` was reporting them as
  "unregistered (runtime type?)" — the opposite of the truth. It now distinguishes an id inside a gap
  from one above the table's top.
- "The two backgrounds whose representative tiles are off the 48-grid" — **four** are off it, and one
  of them is water, which blends normally. So "off-grid implies no transition" is *refuted*, and the
  counter-example was two paragraphs away in the same document. A hedge does not cover a false
  premise.

**Data I had and did not commit.** Road as a background is fully measured in the artifacts: corners
keep 459, edges are `N=a, S=a−2, W=a−1, E=a+1` with `a = 456` for painted dirt and
`a = 488 + 16(T−2)` for T in 2..8, no ring for water, road or impassible. Verified 11 of 11 and now
`road_background_ring()`. The documentation had been telling a painter it "must special-case road"
while withholding the table that lets it.

**The framing was weaker than the evidence.** For **seven of the eight** blending backgrounds the
anchor simply *is* `terrain_type_base_tile`; water is the sole exception. Highlighting only the
exception made the anchor look fitted when it is mostly predictable — and predictability is what lets
someone compute an anchor for a background nobody measured.

**And an API that could be misused into writing wrong tiles.** `transition_ring` took only the
background, so it handed out the land ring for painting *road* onto land (where the engine writes a
ragged run) and for painting land onto land (where it writes nothing). It now takes both terrains and
returns `None` for both cases. A measurement that can be misread into corrupting a map is worth less
than one that refuses.

Also fixed: a doc comment displaced onto the wrong function by an insertion, the handoff asserting
the atlas-block claim this branch had already retracted, three stale test counts, README text saying
the blend tiles were unmeasured, and the two listing verbs having no test at all — the same "the
untested layer is the one that matters" note this log recorded one review earlier.

## 2026-09-17 — The blend rule was shipped as data, and we had been discarding it

**Observed in a local binary.** `setterrain`'s transition ring is not an engine constant that had to
be measured. It is declared, per atlas slot, in the `.til` tileset files, and this repository had
been parsing those files since Stage 1 while reading two of their eleven columns.

A `TILE=` line is `tilenum, self, n, ne, e, se, s, sw, w, nw, index`. `self` is the terrain a cell
holding the tile belongs to; the eight middle columns are constraints on that cell's neighbours, in
a small syntax — `~` for *not*, `|` for *one of*, `*` for *don't care*. `src/tile.rs` kept `tilenum`
and `self` and threw the rest away. So tile 15 — the "anchor" the 2026-09-17 `terrainrings` run
recovered by painting 121 blobs in an attended engine session — is literally the line that says *a
plains cell none of whose four cardinal neighbours is plains*.

**The lesson is not "find the file."** The file was found, documented, opened and parsed. The lesson
is that a parser which silently drops columns converts shipped data into an unknown, and an unknown
gets measured at the cost of a human sitting at a keyboard. Either read every column of a format you
claim to parse, or say in the parser which ones you are dropping and why.

(A hand-off note claimed the `.til` members had previously been reported missing from the archives.
Not by this repository: `docs/game-architecture.md` has recorded 26 tile-set definitions in GS5R3
`pic.mpq` from the start. The members live under a `til\` prefix, so a bare `*.til` search against
`gs.mpq` or `imp.mpq` — the wrong archives — returns nothing, which may be where the story came from.)

### The direction convention was derived, because guessing it is invisible

A neighbour column can mean *the neighbour in this geometric direction from me* or *the direction
from that neighbour back to me*. Those are mirror images, and both fit some lines. A wrong choice
produces maps that look completely plausible and are systematically flipped — the same failure that
let the X-major cell order stand for months, and the reason this was resolved against saved artifacts
rather than by eye.

Over `artifacts/engine-probe-captures/terrainrings-20260917`, eight blending backgrounds × eight
painted terrains × eight ring cells: the geometric reading has **576 of 576** ring tiles satisfying
their own declared constraints; the mirrored reading has **0 of 576**. Every one of the eight columns
carries a nonzero failure count under the mirror, so that is eight independent confirmations rather
than one global fit. The single decisive line: in `zr6.scn` the ring cell north of a blob holds tile
2, whose only non-plains column is `s`, and the paint is indeed to its south.

The apparent inversion in the measured table is a naming collision, not a mirror. `W = −11` gives
tile 4, which declares an east edge — because the measured label `W` is the ring cell's position
*relative to the blob*, while `e` is the direction from that cell to its own neighbour. The cell west
of the paint has the paint to its east. Both readings are geometric; neither is flipped.

### What the constraints reproduce

Simulating "set the region's terrain, then re-select a tile for every disturbed cell" over all 121
rows of the run: **2,084 of 2,084** cells whose candidate set was a single tile match what the engine
wrote — every ring cell and every painted-region perimeter cell — with zero mismatches. 253 cells had
several equally valid candidates, and the engine's tile was inside the candidate set in **all 253**.
512 had no candidate, and the engine wrote something there regardless.

So the rule reproduces the engine wherever the engine is deterministic, and the three cases that
looked like special rules are all declared:

- The **random interior** is now explained rather than merely observed. `TILE=384..391` are eight
  *identical* all-neighbours-plains lines; nothing distinguishes them, so nothing can choose but a
  draw. That is why the same experiment twice gave centre tiles 385 and 390 with a byte-identical
  ring. It also makes `384 + 8k` a block in the file rather than an inference from anchors.
- `tt_dirt`'s slots are `self = 0` with `*` in all eight columns, so a dirt cell always matches what
  it already holds and is never re-selected. That is the "no transitions at all" row.
- `tt_impassible`'s slots `464..=471` each require all eight neighbours to be impassable, so a
  rectangle of it has no legal boundary tile — and road's slots describe a network, not an area.
  Those are the 512 no-candidate cells, and the reason both are refused rather than imitated.
- **Terrain 9 is `"free move"`**, which is roads. Hence `6|9` throughout the plains constraints: a
  road counts as plains for blending, so a road can cross a field without cutting a shore into it.
- **Water's anchor 63 was never an exception.** It is water's block-index-15 slot, `48 + 15`, exactly
  parallel to plains' 15. What differs is `TERRAIN_BASE_TILES`, which happens to record water's
  *interior* slot 392 where it records plains' *index-15* slot. The engine's own table is
  inconsistent between those two rows; the anchors never were.

### The empirical table is kept, as corroboration

Fifty-six measured ring tiles collapsing onto seven offsets is real, independently obtained evidence,
and it agrees with the data file. Nothing measured is deleted. `TRANSITION_RING_OFFSETS`,
`TERRAIN_TRANSITIONS`, `transition_anchor`, `transition_ring`, `road_background_ring` and
`interior_tile_family` all remain, and a test builds a tileset laid out the way a real terrain block
is laid out, paints through the constraint matcher, and asserts the result equals `anchor + offset`
for all eight directions. The two routes to the same answer are pinned to each other.

**The table does not become wrong; it becomes derived.** What was retired is only its role as the
paint's decision procedure.

### Two findings that were not being looked for

**Every cell of all 365 installed maps holds a tile `tilesb01.til` declares** — 1,258,496 cells, zero
unrecognised. This is what removes the old refusal: the paint could not previously read a shipped
map's background because only eleven representative slots were known, and the tileset knows 617.
Sampling 3x3 plains paints on a stride-8 grid, 20 of 20 `.scn` and 8 of 8 `.lgd` world maps now accept
paints, at 75% and 60% of sampled sites. On `URAK.scn` a 3x3 at `(10, 26)` writes 25 cells of which
24 are determined by a single candidate.

**But shipped world maps are not consistent with their own tileset.** 5.9% of `.scn` cells and 9.5%
of `.lgd` cells hold a tile that does not satisfy its declared constraints; only `Denmor64.scn` and
`Kelmor32.scn` are clean. The failures cluster on roads — a plains cell with road to its west and
south-west has no plains-block tile that accepts it — which is exactly why the success rate above is
75% and not 100%. Whether the authors used `forcetexture` freely, or the engine enforces constraints
only during a paint and not as a stored invariant, is **Unknown**.

**And the 337 `.smp` battle maps are not `tilesb01` maps at all.** Checked against a 15-member sample
of the 26 tilesets, the world `.scn` files fit `tilesb01` best with 0% undeclared tiles, while no
sampled tileset fits `.smp` — the best still left 36% of cells undeclared. The map file does not
record its tileset, so this is a real limit: editing a battle map needs the right `.til`, and which
one that is has not been identified. It is also why the CLI takes the tileset as an argument and
refuses without it instead of defaulting to the world one.

## 2026-09-17 (review) — The tie-break was the whole operation, and it was wrong

Two reviewers found the same defect from opposite ends, and it had one cause. An off-map neighbour
was treated as satisfying every constraint, including a negated one. Along a map edge that lets the
*one-sided boundary* tiles compete with the interior family on equal terms, and a lowest-slot
tie-break then picks the most wrong legal option. Measured on 9x9 maps against `tilesb01.til`:

- a whole-map **water** paint wrote a complete phantom coastline — row 0 all tile 49, which asserts
  *land to the north*, row 8 tile 50, column 0 tile 51, column 8 tile 52, interior correctly 392;
- a whole-map **road** paint wrote **nine different road tiles** (450..457, 459) for a uniform road
  field;
- a whole-map **impassable** paint wrote 464 everywhere, replacing the 469 already there.

The fix is in the tie-break, not in a guard: candidates are taken with every off-map neighbour read
as the cell's **own terrain** first, and the open reading is used only if closing leaves nothing.
Closed candidates are provably a subset of open ones, so this narrows a tie and can never invent a
tile. All three paints are now uniform and change zero cells.

**A whole-map refusal was proposed and rejected.** Once the tie-break is principled, painting the
whole map is a legitimate and useful operation — it is "fill with correct interior tiles" — and
refusing it is worse than allowing it. The old `RegionCoversMap` variant was still telling users the
operation was refused while it in fact succeeded and wrote 81 cells; it is deleted along with four
other unreachable variants.

### Nothing read the current tile, and `NoTransition` was sitting there saying it should

`tt_dirt`'s six slots are `self = 0` with `*` in all eight columns, so every dirt cell has six
equally valid candidates and the tileset forces none. A 3x3 plains paint therefore moved **all
sixteen** ring cells from 175 to 31 — while `TERRAIN_TRANSITIONS[0] = NoTransition`, a saved-artifact
measurement in the same file, recorded that the engine leaves them alone. Nothing consulted it.

Worse, this document had already *asserted* the fix as though it were implemented: it said a dirt
cell "matches whatever it already holds and is never re-selected", and that sentence was the stated
justification for deleting the special case. It was false. Keeping a valid existing tile makes it
true, with no special case, and the claim and the code now agree.

### The 2,084 figure was right and its description was not

The claim was "2,084 of 2,084 cells whose candidate set was a single tile". Re-measured: 1,888 have a
single candidate; **196 have between two and sixteen** and are reproducible because the engine keeps
what such a cell already holds — in `zr0.scn` a water blob painted onto dirt leaves ring cell
`(13, 5)` at tile `175` although all six of `[31, 79, 127, 175, 223, 271]` match. The total and the
zero-mismatch result stand; the description was wrong, and for one revision the figure had been
measured with a keep-current rule the shipped code did not implement.

A reviewer said exactly that and was overruled on the grounds that the claim was scoped to
single-candidate cells and the dirt cells have six. The arithmetic decides it: 2,084 = 1,888 + 196,
so the 196 multi-candidate cells are inside the figure and the scoping was inaccurate. **The reviewer
was right.** The 253 genuinely drawn cells are unchanged and are not convertible by keeping — by
construction they hold no valid candidate — so the acceptance number for the keep-current rule is
196 of 196 reproduced, not a share of the 253.

This also sharpens the random-interior story. A genuine draw is what happens when a cell is **newly**
painted and several interiors match, not merely whenever the candidate set is larger than one. The
looser reading is what produced the over-claim, and the outcome type now distinguishes `Kept` from
`Drawn` so the two cannot be conflated again.

### A verifier that could not fail, for the third time in this repository

`verify_map_edit`'s paint arm re-ran `plan_terrain_paint` — the *same* planner the applier had used —
and compared the written map to its output. No planner defect could ever surface, because a wrong
plan was being compared against itself. The two loops that followed were worse than idle:
`tiles.get(&tile_index)` and `definition.terrain_type != cell.terrain_type` are both *guaranteed* by
`candidates()`, which filters on terrain and yields keys of `tiles`, so they evaluated **no neighbour
constraint at all** while the comment claimed they proved the plan legal by the tileset's own rules.

It now derives the affected set from the before/after maps and the tileset, without calling the
planner, and for every written cell rebuilds the neighbourhood from the **encoded** map and checks
each of the eight columns by name. Each check has a named breaking input, which is the standard this
repository should have been holding all along:

| check | input that fails it |
| --- | --- |
| affected set | a cell changed that is neither in the rectangle nor beside moved terrain |
| region terrain | a tile of the wrong terrain inside the rectangle |
| **constraint satisfaction** | a tile of the *right* terrain whose own constraints the written neighbours violate — tile 15, the plains shore tile, dropped into a plains field |
| planner independence | a map where the rectangle simply was not painted, which a re-planning verifier reports as fine |

The third row is the one every earlier version accepted. **Before calling any check done, state what
input would make it fail; if you cannot name one, it is not a check.**

### The ring was wider than the reason for it

Any changed region cell caused the whole rectangular ring to be re-selected, so a ring cell whose
eight neighbours had not moved was re-picked anyway. Reproduced: painting plains over a rectangle
already half plains moved **tile 387 to 384 three columns away from the only cell whose terrain
changed**. The affected set is now the rectangle plus any cell outside it with a neighbour whose
terrain actually moved. Keeping a valid existing tile already neutralises the reported symptom; the
narrowing still matters, because on a map whose tiling is *already* inconsistent — 5.9% of shipped
`.scn` cells are — the old ring would silently rewrite unrelated cells the engine would not touch. It
also consults fewer cells, so slightly fewer paints refuse: stride-8 sampling on world maps went from
75% to 76% of sites succeeding, and `.lgd` from 60% to 61%.

### The leniency had no shipped justification

Truncated `TILE=` rows were padded with `*`, which made the tile a wildcard selectable for any
neighbourhood — the opposite of what a missing declaration means. The code comment justified this by
claiming `tilesa01.til` "is already a different shape from `tilesb01.til`". **That is factually
wrong.** Counted across all 26 shipped tilesets: **4,043 `TILE=` rows and 402 `TERRAINTYPE=` rows,
every one with exactly 11 fields, and not one incomplete tile.** The two world tilesets differ in row
count — 609 against 617 — and not in field shape. Completeness is now explicit and an incomplete tile
is excluded from selection, so a short row stays inspectable and can never be painted.

The related risk is the reverse one: reading eight columns the parser used to discard means malformed
*content* can now hard-error where it was previously ignored, and `--view-map` needs only a tile's
slot and `self` column. So an unreadable neighbour column, trailing index or `TERRAINTYPE` attribute
is recorded as **absent** and leaves the tile unpaintable, rather than failing the file. What stays a
hard error is only what the renderer needs: the atlas name, grid dimensions, tile size, slot and
`self`. All 26 shipped tilesets parse — `cargo run --example parse_all_tilesets` — and that example is
committed so the claim is re-runnable.

### Terrain ids are tileset-local

Parsing all 26: atlases of 64, 128, 256 and 624 slots, 10 to 19 terrain types each, and ids reaching
**42** in `cavecry2.til` where `tilesb01.til` stops at 10. The eleven-name table in `map.rs` is one
tileset's vocabulary, not a global one. This independently reinforces that `.smp` battle maps need
their own tileset, and that no terrain id should be hardcoded.

### Left alone deliberately

`--map-fill-terrain` and `--map-create` fill from `TERRAIN_TYPES.base_tile`, which for seven of the
eleven terrains is the block's **shore** slot: `--map-create 9 9 6` writes 81 cells each declaring
that none of its neighbours is plains, in a field that is entirely plains. Water only looks fine
because its recorded base tile 392 happens to be the interior slot. This is not changed, because
`fill_terrain` reproduces `clearmap` and `clearmap` really does force one tile everywhere — three
probe tools depend on that equivalence to lay a known background, and "fixing" the fill would change
what a probe measures. The follow-up is a separate `terrain_interior_tile()` accessor; meanwhile
painting the same rectangle repairs the field, since a whole-map paint is "fill with correct interior
tiles".

`TERRAIN_BASE_TILES` also stays, for a reason neither the original write-up nor the review had: its
only consumers want *a tile `getterrain` answers with this terrain* so `clearmap` can lay a
background, and both 392 and 63 satisfy that. Changing water's 392 to its anchor 63 would make the
probe's background a constraint-violating field — strictly worse.

## 2026-09-17 — Six record layouts, not three families and 18 mysteries

Object editing worked on 196 of the 365 installed maps. The other 169 had trailing sections this
project could not parse — recorded as "52-byte and 53-byte families" plus "18 unmatched tails" —
so `--map-place-sprite` and `--map-remove-sprite` refused them. This run decoded all of it, offline,
from the installed corpus alone. No engine run was involved and no map file in the game directory
was touched.

### The population, measured before anything was interpreted

Counting exactly — a section is `4 + count × record_size + footer`, with **nothing left over**, and
every record's head validated at the stride — each of the 365 files resolves to exactly one layout:

| Records | Footer | Files | Records | Header word at `0x00` |
| ---: | --- | ---: | ---: | --- |
| 47 | none | 6 | 434 | 98 |
| 47 | `u32` | 3 | 52 | 101 |
| 48 | none | 9 | 222 | 63, 73 |
| 49 | `u32` | 196 | 16,628 | 102, 105-111 |
| 52 | none | 144 | 1,253 | 76, 79, 81, 87, 89 |
| 53 | none | 6 | 2,528 | 96, 97 |
| | | **364** | **21,117** | |

The 365th, `chbldg01.smp`, holds a four-byte section: a zero count and no footer.

**No map mixes record sizes.** Size is a per-map property, which is what made a single
`layout` field on the section the right parser shape rather than a per-record one.

**The 18 "unmatched tails" were never a separate phenomenon.** Nine are the 48-byte layout and nine
the 47-byte one. They looked unmatched because only three record sizes had ever been tried. The one
"ambiguous file" was `chbldg01.smp`.

### The head aligns; the brief's leading hypothesis did not survive

The head does align, field for field, across all 21,117 records in all six layouts: eight `u32`s,
`record_kind` and `record_version` always `1`, `+12` always `0xffffffff`, `+16` always `0`, then
`cell_index`, `instance_id`, `attribute_bits`, `sprite_type`. `cell_index` is unique and in bounds in
every file; `instance_id` is unique and strictly increasing in every file. So the shared-head
assumption held, and it was checked rather than assumed.

**What did not hold is "the 49-byte record plus three or four extra bytes".** Two of the newly
decoded layouts are *shorter* than 49 bytes, and past `+32` there are two tail shapes that are not
one shape at an offset:

- **procedure tail**, sizes 47 and 49, 17,114 records: `ff 01`, a `u32` procedure id, `u32` `0`,
  `u32` `0xffffffff`, then **one** zero byte (47) or **three** (49).
- **plain tail**, sizes 48, 52 and 53, 4,003 records: `u32` `0`, `u32` `0`, `u32` `0xffffffff`,
  `u32` `0`, then a `u32` `0xffffffff` (52, 53), then one zero byte (53).

The plain tail has no procedure-id field at all — not an extra one. The `0x01ff` marker is two bytes
where the plain tail has four zero bytes, and the trailing padding differs, so no single shift maps
either onto the other. Several shifts were tried before this was accepted; each matched three or
four words and then broke.

### What the extra bytes are, and what cannot be said about them

Every byte outside the eight head fields and the procedure id is **constant within its layout across
the whole corpus** — zero, or `0xffffffff`. There is therefore nothing to correlate against sprite
type, coordinates, footer value or map dimensions: a field with one value has no distribution. They
are decoded as named, typed, per-layout fields that round-trip exactly and are asserted of nothing.
Their meaning is **Unknown**, and it is unknowable from this corpus.

The one head field that varies by shape is `attribute_bits` at `+24`: the procedure-tail layouts
carry the upper-nibble codes already recorded, and all 4,003 plain-tail records carry zero.

### The header word at `0x00` partitions the layouts, and an old refutation was wrong

The word's 19 distinct values partition the six layouts **with no overlap at all**, and in order:
63-73, 76-89, 96-97, 98, 101, 102-111. The engine's own fresh save stamps 111 — the top of the range
— and writes the 49-byte layout.

This document had recorded the community's "version number" reading as **refuted**, on the 2026-09-16
argument that the word varies independently of geometry and clusters by file family, so "a version
number would not vary that way". That argument does not follow: a version varies with *when* a file
was built, which is independent of its geometry and does cluster by family. The refutation is
**corrected**. What is genuinely refuted is the tileset-selector reading that replaced it, since the
engine rewrites the word from its own state on every save. And the same 2026-09-16 measurement said
"more than twenty distinct values"; the count is **19**.

Reading the word as a format version, and as what the loader uses to pick a layout, stays
**Inferred**. The partition is the observation. Because it is inferred, the parser consults the word
for exactly one thing: a section whose record **count is zero**, where the length genuinely cannot
distinguish the layouts. Only the 19 observed values resolve; anything else leaves the section raw
rather than interpolating a version number.

### Results

- `--map-roundtrip` on the installed `map/` directory: **365 checked, 365 byte-identical, 21,117
  records rebuilt from their typed fields, 0 failures** — up from 16,628. The 4,489 newly counted
  records are the 169 files that previously round-tripped as opaque bytes with no field checked.
- **Object editing works on all 365 maps.** Exercised through the CLI, one process per file: place a
  sprite on a free cell, then remove it. All 365 came back byte-identical to the shipped file, and
  the placement grew each file by exactly its own layout's record size.
- A new record is minted **in the map's own layout**. In the plain-tail layouts that includes `+24`,
  written as zero because that is the only value those 4,003 records hold — and it is flagged
  **Inferred and unmeasured**, because the engine run that watched a fresh record being written wrote
  a 49-byte record and says nothing about the older layouts.

### The trap, and the check that catches it

A wrong record size can divide a section evenly. Four 48-byte records with no footer and four
47-byte records with a footer are both 196 bytes, and one installed map — `CAVWAT02.SMP` — is exactly
that size. Arithmetic cannot choose, and `candidate_tail_layouts` honestly reports both. What chooses
is the record heads: at the 47-byte stride the second record's `record_kind` lands inside the first
record's padding, which is zero rather than `1`. A synthetic fixture of that exact shape is a test,
and deleting the head check makes it fail.

Two fixtures the corpus cannot supply also exist, for the same reason the X-major bug survived
months in a 2×2 fixture: a section of **mixed** record sizes, which no installed map is, and a
49-byte-stride section carrying the plain shape's zero marker, which would otherwise have four zero
bytes read as a procedure id of `0`. Nothing in the parser validates the marker or the procedure id
beyond this check — and `0` appears in **no** corpus record, whose procedure ids are `-1` or `8..717` — so the misreading would have been invisible in every other
assertion, round trip included.

### Review corrections, same day

Two independent reviews of the change above found no runtime defect — every input either reviewer
could construct behaved correctly — and both reproduced the 21,117 records, the +4,489 breakdown,
the zero-overlap header-word partition, the per-layout tail constants, and the `cell_index` and
`instance_id` uniqueness. What they found was in the docs, the comments and the fixtures, and three
of the items are the same shape as this repo's standing lesson about verifiers that cannot fail —
one layer out, in the fixtures rather than the code.

**A second arithmetic collision was not enumerated.** There are exactly two lengths that two layouts
fit for `count >= 1`, and the write-up named one. The missing one is `count == 1`: 57 bytes fit
`49 + footer` and `53 + none`, and there the **marker word is the sole discriminator**, because one
record sits at the same offset under both readings so every other head field is byte-identical. The
comment in `record_head_matches` asserted the opposite — that the marker is "not what resolves the
ambiguity" — which was true of the 196-byte pair and actively misleading about this one. Both
collisions are now enumerated by iterating every count against all six layouts, rather than found.

**The consequence of removing that check is a refusal, not a silent misread** — which is where this
log departs from the review that raised it. Measured both ways: with the marker test deleted, both
candidates survive, the parser's own ambiguity rule returns the tail raw, and `--map-place-sprite`
**refuses** a map it could edit a moment earlier. It does not decode one 49-byte record with a footer
read out of the record's own last bytes. The check is load-bearing either way, and no corpus file can
exercise it, because no installed map has a count of 1 — but "silently round-trips wrong" and
"loudly refuses" are different failures and the fix should be justified by the one that happens.

**The fixture for that check was shaped like the wrong thing.** It zeroed two bytes, leaving
`00 00 ff ff` at `+32..36`: a corrupted procedure record, not the plain shape, whose procedure id
would have read as `-1`, the commonest corpus value. Zeroing all four builds what all 4,003
plain-tail records really carry. Fixing it turned up something better: **at `count == 1` the plain
shape in a 49-byte-sized section is a valid single 53-byte record, byte for byte**, so the parser
decodes it and is right to. The fixture needs two records — 106 bytes, which only `49 + footer`
fits — for the marker to be the only thing that can refuse it.

**Three other fixtures were asserting less than they appeared to.** Mutation-checking the new tests
against "validate only record zero's head" showed both 196-byte collision fixtures still pass,
because record zero's marker already eliminates the rival layout. Nothing covered the every-record
property until `a_section_whose_later_head_is_invalid_stays_raw` was added. The mixed-size fixture
stays raw on the length check alone and so asserts the default outcome; its docstring now says so.
The 196-byte collision was tested from the 48-byte side only, and the corpus cannot supply the other
side, since the 47-byte-with-footer layout's smallest file holds 16 records.

**Figures corrected in what was published.** "200 in 357 of the 364 files" was wrong and did not add
up against its own total — 357 + 10 + 1 is 368. The per-file lowest ids are `{200: 353, 100: 10,
203: 1}`. And the `instance_id` range is `100..1659`, not `200..1659`: the ten files that start at
100 are all in layouts this project could not read until now, so the decode falsified a figure in the
same document that recorded it. The `place_sprite` refusal was justified by uniqueness "across all
16,628 corpus records"; it holds across all 21,117.

**Two doc blocks contradicted the code they document.** The central safety paragraph said that if two
layouts both fit, the tail stays raw — but the code narrows by head checks and *decodes* the
196-byte case, and a test asserts exactly that. And `placed_sprites_mut`'s refusal still said "not
the decoded 49-byte placed-sprite family", true of 169 maps that now edit fine; its only test
asserted on the string's tail, so it pinned the true half and not the false head. The test now
asserts the whole message and that it does not name a record size.

**The unmeasured mint is now surfaced where it is used, not only where it is documented.**
`--map-place-sprite` notes that the record it just minted is Inferred, on **five** of the six layouts
— not four. The review proposed excluding 47 and 49; only the **49-byte** layout's mint was ever
watched being written. The 47-byte layout shares the procedure tail and so takes `+24 = 0x00000001`
measured in a *different* layout, which is the value-carried-across-layouts move this project warns
about. `MapTailLayout::mint_provenance` makes that distinction a value with a test on it.

**Support for the header-word partition is stated, not glossed.** Seven of the 19 words appear in
exactly one file (63, 81, 87, 89, 97, 108, 109), and `chbldg01.smp`'s tie-break rests on word 76,
which occurs in two files — one of them `chbldg01.smp` itself. No counterexample across 365 files,
and one file deep in seven places.

**`CAVWAT02.SMP` is the 196-byte collision in the shipped corpus, and it is doubly determined:** the
stride head-check and its header word (73) agree independently, which is stronger than the original
write-up claimed.

Also closed: `MapTailLayout::footer_bytes` underflowed on `total_fixed_bytes: 0`, reachable because
the struct's fields are `pub` — a debug panic and a release wrap to `usize::MAX - 3`. Saturating, with
the input that would have caught it written down as a test.
## Which tileset the 337 `.smp` battle maps use — `tilesa01.til`, from the engine's own startup script

> **RETRACTED 2026-09-17.** The central conclusion of this entry is wrong. `.smp` maps are **not**
> all read through `tilesa01.til`; each combat map's tileset is declared by the *encounter* that
> loads it. The `combattileset`-occurs-once measurement below is correct and the inference from it
> is not. The entry is kept unedited because how it survived review is the useful part; the
> correction is [the next section](#correction-a-combat-maps-tileset-belongs-to-its-encounter).
> Specifically wrong below: the headline; "the answer is in the gamescript, not in a fit" (it is in
> the gamescript, but in the 169 per-encounter pairings rather than in `START.GS`); the refutation
> of the filename hypothesis; "the other 24 `.til` members are the terrain editor's palette"; the
> 8.11% "residue" and the `loadsubmappages` lead offered to explain it; the 3.00/1.92 wildcard
> figures; "`maptileset` is re-set in four places"; and the 490/496 paint survey.


The question had been open since the tileset-driven paint landed: `--map-paint-terrain` needs an
explicit `.til`, and for 337 of 365 installed maps this project could not name the right one. Two
facts were in tension. Slot *declaration* fitted perfectly — `tilesa01.til` and `tilesb01.til` each
declare 100% of the 308 atlas slots the `.smp` corpus uses, zero undeclared across all 778,240
cells. Slot *semantics* fitted terribly — both score an identical 8.11% on the eight declared
neighbour constraints, where shipped `.scn` maps score 94.11% against theirs.

**Both halves were reproduced before anything was built on them.** The 8.11% came out of an
independent Python re-implementation of the `.til` grammar and the neighbourhood check, and then a
third time out of a committed Rust example. What makes it a statement about the corpus rather than
about a scorer is the **control**: the same code scores `.scn` at 94.11%, reproducing the 5.9%
violation rate this repository had already measured by a different route. A scorer that reported 8%
on both classes would have been a broken scorer.

### The constraint approach cannot answer it, and that is measurable

Over the 308 slots the combat maps actually use, the two 624-slot tilesets disagree on **nothing** —
0 `self` disagreements, 0 neighbour-constraint disagreements, neither declaring a slot the other
does not. They differ at exactly eight slots, `464..=471`, which is `tilesb01.til`'s `tt_impassible`
block; `tilesa01.til` stops at terrain 9 and no `.smp` cell reaches slot 464. Their identical 8.11%
was therefore never a coincidence in need of an explanation — it is one rule set scored twice.
*Agreement is not confirmation*, and here the agreement turned out to be literal file identity over
the range in question.

That is a real result on its own: **no measurement over `.smp` cells can prefer one of them**, so if
the answer had to be a fit, there was no answer.

### The answer is in the gamescript, not in a fit

`lomse.exe` exports two operators, `maptileset` and `combattileset`. Across all 1,696 gamescript
members of GS5R3's `gs.mpq`, `combattileset` appears **exactly once**:

```text
"til/tilesb01.til"dup /currenttileset exch def maptileset
"til/tilesa01.til"combattileset
```

`START.GS` lines 74-75. It is never re-set, never parameterised, and takes no map argument, so the
engine holds one world tileset and one combat tileset for the whole session and picks between them
by the kind of map being drawn. `.smp` is the special/combat map; every one of the 337 is read
through `tilesa01.til`. `maptileset` is re-set in four places and all four put `tilesb01.til` back
on the way out to the menu. The terrain editor's `tileselector` feeds a user's pick to `maptileset`
and nothing feeds anything to `combattileset` — which is what the other 24 `.til` members are for.

### Three hypotheses refuted, with their numbers

**A per-faith or per-location tileset chosen from the filename.** This was the most attractive
hypothesis and it fits well. The 337 names decompose into an eight-faith prefix and a location
token, `AIBLDG01.SMP` and `orcavmule.smp`, and the tilesets carry the same prefixes; for **236 of
the 263** faith-prefixed maps the same-named `{faith}bldg01.til` declares 100% of that map's slots.
It is nonetheless wrong, and the mechanism of the illusion is worth keeping: an individual `.smp` is
small, and 52 of them stay under slot 64, 252 under 128, 30 under 256, and only 3 go further. The
pooled range 0..439 is the union of maps that each use a fraction of one atlas. A per-map tileset
would have explained both facts at once, which is exactly why it needed refuting rather than
adopting.

**A constant slot offset or bank.** Sweeping all 624 offsets over a stride-17 sample of 20 maps
gives a broad hump, not a peak: maximum 47.88% at offset +336, offset 0 ranked 70th of 624, median
2.77%. The hump has a cause and not a meaning — offsets near +336 push combat-map slots into the
three `free move` blocks at `480..=623`, which carry 3.00 wildcard columns per tile against 1.92
elsewhere and so accept nearly anything. *An explanation that fits the numbers may be your own
tooling*, and naming the mechanism is what separates a refutation from a shrug.

**The prior claim in this repository.** `docs/map-format.md` said *"no sampled tileset fits `.smp`;
the best had 36% of cells undeclared"*. Wrong twice: the 15-tileset sample **excluded both 624-slot
files**, which are the only ones whose atlas can hold the slots `.smp` uses, and the number was
inverted — 35.41% is the fraction the two *worst* tilesets (`aibldg01`, `eabldg01`) **declare**, and
they leave 64.59% undeclared. This is the second time in this repository that an unrepresentative
validation set produced a confident wrong conclusion, and the reversed sense is an argument for
printing a percentage with the noun it counts.

### What is still open, and one new lead

Knowing the tileset does not make `.smp` maps constraint-consistent. The honest statement is that
**the slots are valid and the blend constraints do not apply**: 8.11% satisfaction, 7.48% with the
map edge read closed, and violations spread evenly across all eight neighbour columns rather than
concentrated in one, which is what a mistaken column order would look like. Combat maps were not
authored the way world maps were.

The lead is the line immediately after `combattileset`:

```text
"til/ttype01.lbm"
"til/thite01.lbm"loadsubmappages
```

Both members are 320x624 8-bit images, and those dimensions factor onto the tile atlas exactly:
624 = 39 rows x 16 and 320 = 16 columns x 20, giving each of the 624 atlas slots a **20x16 block**.
The blocks hold terrain-type ids — 12 distinct values across the page — and they are coherent: slot
392, water's interior tile, has a uniformly-water block, and 129 slots have a single-valued block.
A per-atlas-slot terrain-type and height table for *submaps*, which is what a combat map is. Whether
a combat map's terrain is read from there rather than from the `.til` `self` column is **untested**,
and it is the first thing to try.

### What landed

`MapClass`, `engine_tileset_member` and `tileset_mismatch` in `tile.rs` record the assignment;
`--map-tileset-for FILE` reports it for a map. The tileset stays a **required** argument to
`--map-paint-terrain` — nothing is defaulted and no file is guessed at — but two things changed:
omitting it now names the member the engine uses for that class, and supplying a *shipped* tileset
the engine does not use for that class is **refused**. A name that is not one of the 26 shipped
members is presumed modded and accepted untouched, which is also what keeps the existing fixture
tests working. Classification is case-insensitive because the installed `map/` directory is split
172 `.smp` to 165 `.SMP`, and a case-sensitive match would have handed 165 combat maps the world
tileset; that is the input the mutation check confirmed the tests catch.

`cargo run --release --example smp_tileset_fit -- MAP_DIR TIL_DIR` re-runs the measurement, so the
numbers above are reproducible without committing a map or a tileset.

### The blocker is actually lifted, and the 8.11% is not a prediction about paints

`--map-paint-terrain` was unusable on 337 of 365 installed maps. With `tilesa01.til` named, it is
not: sampling every 11th of the sorted 337 `.smp` files, on the same stride-8 grid of 3x3 plains
paints the world-map survey used, **490 of 496 sites succeed and all 31 sampled maps accept at least
one paint**. That is a higher rate than the world maps' 76%.

The two figures are not in tension, and saying why matters because conflating them is how this
document over-claimed once already. A paint needs candidates for the cells it *writes* — the region
and its one-cell ring — and it re-selects those itself. The 8.11% counts cells whose currently
stored tile does not satisfy its own declared constraints, which is a statement about how the map
was authored, not about whether a new edit has a legal answer.


## Correction A: a combat map's tileset belongs to its encounter

The previous entry concluded that all 337 `.smp` files are read through `tilesa01.til`, because
`combattileset` is set to it once in `START.GS` and never again. That measurement is right. The
inference is wrong, and the scripts say so directly:

```text
/mapfile"map/aicave.smp"def   /tileset"til/aibldg01.til"def
/mapfile"map/fienc1.smp"def   /tileset"til/cavelava.til"def
```

Each **encounter** dictionary defines `mapfile` and `tileset` as adjacent keys, so the tileset is a
property of the encounter, not of the map. `lomse.exe` exports `setterrainspritemapfileproc`
alongside `setterrainspritetilesetproc` — the engine asks the script for both, per encounter. And a
corpus comment in `wilderness_land.gs` gives the rule for `combattileset` outright: *"WHEN 'mapfile'
AND 'tileset' ARE UNDEFINED, YOU GET AN OUTSIDE COMBAT ENCOUNTER."* It is the default for
engine-generated open-field combat, not the tileset of the shipped battle maps.

### What the corpus says, measured

Extracted from all 1,700 `gs.mpq` members (1,699 extractable, the other being the archive's own
`(listfile)`; zero extraction failures) by taking every `/mapfile` definition and the `/tileset`
definition nearest it in the same member, with **procedure bodies included** — `aimult.gs` writes
`/tileset{dungeon_id getdungeonstrength 3 le{...}...}` whose three branches all yield
`aibldg01.til`, and `genchaos.gs` writes a `terrainsprites`-keyed selector yielding `chbldg01.til`
or `cavelava.til`.

| | Maps |
| --- | ---: |
| Installed `.smp` | 337 |
| Bound by an encounter | **169** — 143 to one tileset, **26 to several** |
| No binding found | **168** |

The multi-valued maps are a finding, not a gap. `chcave.smp` is drawn through `cavelava.til`,
`chbldg01.til`, `cavewatr.til`, `ruins01.til` and `cavecrys.til` by five different encounters: three
quest encounters use `ruins01`, `genbeast.gs` uses `cavewatr`. There is no single right answer, so
`--map-tileset-for` reports `ambiguous:` with every candidate.

| Scored against | Maps | Satisfied |
| --- | ---: | ---: |
| The gamescript's own tileset | 169 | **370,254 / 388,910 = 95.20%** |
| `tilesa01.til`, the retracted rule | 169 | **32,842 / 389,376 = 8.43%** |

Script tileset wins on **166 of 169**. `aicave.smp` 84.3% against 3.3%; `licave.smp` 100% against
13.4%; `decave.smp` 94.1% against 21.3%. The control needing no scoring: `aicave.smp` uses exactly
slots `0..63` and `aibldg01.til` declares `TILES=16,4` — a 64-slot atlas — while `tilesa01.til`
declares 624. And `pathwoods.smp`, one of three maps the scripts really do pair with `tilesa01.til`,
scores 98.5% against it, so the table is not merely "never `tilesa01`".

### The 168 unresolved maps, and why scoring cannot rescue them

Best-fit scoring is not discriminating here: **110 of the 168** have *sixteen* tilesets tied within
one percentage point of the best, because the 26 shipped tilesets collapse to only **16 distinct
rule sets** — all seven `*bldg01.til` files are byte-identical in rule and differ only in which
`.lbm` they point at. Faith prefixes do not help either (below). So these maps are reported
`unresolved` and the paint verb refuses on them. What would settle it is a binding form this
extraction reads too narrowly.

### The filename rule: corrected in both directions

The previous entry filed a per-faith/per-location filename rule as **Refuted**, on the grounds that
its fit was an artifact of atlas size. That refutation is wrong — combat maps really do each have
their own tileset — and a wrong refutation is worse than an open question, because it stops the next
reader looking. But the filename is still not the selector:

| Over the 263 faith-prefixed `.smp` | Count |
| --- | ---: |
| `{faith}bldg01.til` merely *declares* every slot the map uses | 236 |
| `{faith}bldg01.til` *is* what the gamescript pairs the map with | **41** |

The 236 figure — the one published — measures declaration coverage, not the pairing, and conflating
the two is exactly what made the rule look causal. The scripts cross the faiths freely:
`licave.smp` pairs with `libldg01.til` **and** `wabldg01.til`; `eacave.smp` with `libldg01.til` and
`ruins01.til`. A faith rule would be wrong about 222 of 263.

### Three published numbers that do not reproduce

- **Wildcard density.** Published as 3.00 `*` columns per `TILE=` row for slots `480..=623` against
  1.92 elsewhere. Measured: **2.33** (336/144) against **1.67** (776/465) in `tilesa01.til`, and
  2.33 against 1.64 in `tilesb01.til`. Two independent measurements agree on the correction. The
  conclusion — that the +336 offset hump is a permissive region of the atlas rather than an
  alignment — survives; the arithmetic was not checked.
- **`maptileset` is re-set in four places and all four restore `tilesb01.til`.** Wrong in both
  halves. Ten occurrences across seven members: one in `START.GS`, the `tilesb01.til` restore in
  **six**, and **three** `currenttileset exch maptileset` editor calls that restore nothing.
- **The `loadsubmappages` lead.** Offered as the likely explanation for combat maps satisfying only
  8.11% of their tileset's constraints. There is no such residue — that figure was scoring against
  the wrong tileset, and the real one is 95.20%. **Withdrawn as an explanation.** The two 320x624
  pages are still real and their geometry still factors onto the atlas as a 20x16 block per slot;
  that stays recorded as an unexplained observation with no claim attached.

### A second blocker, behind the first

With the right tileset in hand, painting a battle map still failed. `--map-paint-terrain`'s terrain
argument went through the same parser `--map-set-terrain` uses, whose ceiling is `tilesb01.til`'s
eleven types — so a combat terrain id refused with `21 is not one of the 11 terrain types`. Sampled
paint success across the bound maps was **7.5%**. A paint's ceiling is now the supplied tileset,
which `PaintRefusal::TerrainTypeNotInTileSet` was already enforcing anyway; the tileset-less verbs
keep the eleven-type table because they have nothing else to check against. After the fix: **784 of
912 sampled sites across 55 of 57 maps, 86.0%.** This is the measured consequence of "terrain ids
are tileset-local", which this project had recorded as a hazard without ever testing.

## Correction B: why nothing caught it, and what was built instead

This is the part worth keeping. Two independent reviewers reproduced **every** published percentage
— 35.41%, 236/263, 47.88% at +336, rank 70 of 624, 8.11%, 94.11% — verified the scorer's column
order and its `~`/`|` handling, verified the 26-name shipped tileset list against the files,
extracted all 1,696 `.gs` members with zero failures, confirmed the single `combattileset`
occurrence — and endorsed the wrong conclusion. Four independent mutations each killed tests. CI was
green throughout.

**The reason is structural: every assertion was against a hardcoded literal.** No test in the suite
could fail on *the rule being wrong*, only on the code disagreeing with the constant. A clever test
that writes one fixture tileset under three names and checks three outcomes tests the plumbing
perfectly and says nothing about whether the names are right.

Two lessons, both already in this repository in other words:

- *Agreement is not confirmation* — and reproducing a figure is not even agreement about what the
  figure means. Both reviewers and the author shared one assumption, and checking the arithmetic
  downstream of it could not surface that.
- *An empty review is not a clean review*, and neither is an instrument that cannot fail.

What was built in response: `examples/smp_tileset_fit.rs` scores the **installed corpus** against
the resolution rule and now **exits non-zero** when it scores no maps, when a map class is absent
entirely, or when any group's satisfaction falls below a 60% floor — which the retracted rule's
8.43% trips. The previous version reported `self-disagreements:0` on an empty directory and exited
0, certifying perfect agreement on no data. It also emits **per-extension** rows, so the `.scn`
94.11% open-edge control that the "not a broken scorer" argument rests on is *printed output* rather
than a prose claim, and it emits the coverage counts, the atlas buckets, the ambiguity spread and
the 41-versus-236 filename measurement — all of which were prose before.

One run of that instrument would have caught the class-based rule. That is the only test here that
could have.


## Correction C: the runtime selector form, and comments

Two further errors of mine, both found by review, neither changing the reversal.

**The `/tilesets` selector form is live code and I recorded it as absent.** I searched for
`/tilesets[` and found nothing. It ships as a procedure over a *separately named* array:

```text
/tilesets{ ... tiles 0 get ... currentterrainsprite getterrainspritelocation
           8 mod 2 eq{pop tiles 2 get}if ... }/dummy currentdict replace
/tiles["til/cavewatr.til" "til/cavecrys.til" "til/cavelava.til" "til/aibldg01.til"]replace bind def
```

41 members define `/tilesets`, 40 define `/tiles[`, 40 define `/maps[`. **One** encounter picks
among up to four tilesets at runtime from a sprite's map location, which is a second and stronger
reason a combat map has no single tileset. My singular-key reader used `/tileset\s*\{`, which cannot
match `/tilesets {` — the `s` is in the way. A negative result from one spelling of a search is not
absence, and I stated it as absence.

What *is* absent is **randomised** selection. The `DUNGEONS5.gs` comment I cited is a 2025-06-12
changelog entry recording that randomised mapfiles and tilesets were **removed** for multiplayer
desync. That comment describes a deletion; citing it as evidence the mechanism does not exist was
reading a changelog as a specification.

**The extractor never stripped comments.** Gamescript members use bare `CR` line endings, so a `;`
comments to end of line even though a whole procedure looks like a single line to anything splitting
on `LF`. Seven members changed once comments were honoured — including `wilderness_land.gs` and
`wilderness_sea.gs`, whose `chcave.smp`/`cavewatr.til` pair is commented out *because those are the
outside-combat-encounter files*, the very members whose comment I quoted for the `combattileset`
rule. Multi-valued maps drop 26 → **22**, undeclared cells 880 → **466**.

### Why the selector form is recorded separately rather than merged

The instruction I was given was to fold the array maps into the binding table under a coarse
reading. Measuring first changed the answer, and this is the useful part:

| | |
| --- | ---: |
| Distinct `.smp` named in any `/maps[...]` | 43 |
| Of those, **not** already declared by a literal pair | **0** |
| Coverage change | 169 → **169** |
| Coarse sets crossing more than one tileset *rule class* | **42 of 43** |

The predicted benefit was five new maps; there are none. So the merge buys no coverage and costs
precision: it would stand `ruins01.til` (81.3% satisfaction on `demina.smp`) beside the declared
`debldg01.til` (98.3%) as an equal.

I also tested the positional zip the instruction warned against, because the arrays *look* parallel
— `aimina.smp` sits at index 3 in four members and `aibldg01.til` at index 3 in all four. It is
suggestive and not establishable: only **31 of 40** members have equal-length arrays and **5 of 40**
disagree with their own literal pair at index 0. And scoring cannot adjudicate any of the three
readings — mean best satisfaction 95.33% declared, 94.58% zipped, 95.57% crossed. I expected that
to be because the array entries are rule-identical art variants; **that was wrong**, 42 of 43 cross
rule classes, and I only know it because I checked instead of asserting it.

So the resolution: two tables. `COMBAT_TILESET_BINDINGS` is what a map *is* read through and drives
reporting; `COMBAT_TILESET_ARRAY_CANDIDATES` is what an encounter may *reach* and is consulted only
so the paint gate never refuses it. **Reporting is precise; refusing is permissive.** Refusing the
engine's own answer is the bug that shipped on this branch once already, and the permissive side of
the gate is where that lesson lives.

## 2026-09-17 — IMP animation control read out of the decoder; the timing field does not exist

[Issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2) was parked on the
assumption that it needed an attended run: sit in front of the game, watch a `MELEE_ATTACK` play,
count frames. It did not. The metadata is parsed by code we possess, and reading the parser answers
three of the issue's four bullets outright. The full field table with addresses is
[docs/imp-format.md](imp-format.md); what follows is what changed and what the method cost.

### The entry point was the operator table

The 1,906-entry native table already recovered for GameScript is a directory of named function
pointers, and it contains `setimpplayeraction`, `setimpplayerfacing`, `setimpplayerdirection` and
`setimpplayerframe`. Each operator is a thin argument-popping shim ending in one `call` to the real
method, so four names bought the whole animation-player class in about twenty minutes. That the
table pays off on a question it was not built for is the argument for building it.

### The control came for free, and it mattered

Before trusting the disassembly on anything unknown, the same reading reproduces four structures the
decoder already had: the sequence count at loaded-header `0x1A` and the table pointer at `0x1C` (our
file offsets 26 and 28), the 16-byte sequence stride, the facing count at sequence byte 11, the
8-byte facing stride and the frame count at facing `+2`. A triple loop at `0x0049C2CD`–`0x0049C349`
walks all three levels with exactly those constants. A method that had the structure wrong would
have failed there instead of one step later on the part nobody can check.

### Cycle direction: a five-entry jump table, and mode 4 is a ping-pong

`Imp::Advance` at `0x0049D9A0` masks sequence byte 0 with 7 and dispatches through a table at
`0x0049DA68`. Two distinct endings: wrap to zero, or hold the last frame. Mode 4 shares the wrapping
target but is special-cased at **two other addresses** — `Imp::CycleLength` returns `2N − 1` at
`0x0049D90E`, and `Imp::GetFrame` reflects a position past the end to `2N − i − 2` at `0x0049ACA1`.
That is a ping-pong, and — a point the review sharpened — it is **established rather than merely
consistent**. The two sites are not independent guesses that happen to agree: they run on the same
record in the same call, `Advance` takes the length from the first and hands the index to the second,
and composing them is arithmetic. Length `2N − 1` with `0..N−1` taken as themselves and `N..2N−2`
folded to `2N − i − 2` enumerates `0,…,N−1,N−2,…,0`. There is no reading under which mode 4 is
something else. The corpus then supports it from a second direction: of the sequences whose
generated `.h` names them, `DEFEND`, `GET_HIT`, `MELEE_ATTACK` and `MINOR_SPELL` are overwhelmingly
mode 4, while `STAND`, `MOVE`, `DIE` and `CORPSE` are mode 0. A sword swings out and back; walking
loops. Neither side was fitted to the other.

The detail that made mode 0 on `DIE` stop looking wrong: **both** endings store 1 into the value
`Advance` returns (`0x0049DA0C`, `0x0049DA4E`). One-shot behaviour is the caller reacting to that
return, not a property of the file — which is why only 5 of 4,667 sequences use the one-shot mode at
all.

### Direction: five facings, eight directions, and the flip is the placement rule in reverse

`Imp::DirectionCount` at `0x0049D920` is three instructions long: if sequence byte 1 has bit 7 set,
return `2N − 2`; otherwise return `N`. Five stored facings therefore cover eight directions, with
5, 6 and 7 drawn as 3, 2 and 1 flipped. 2,234 sequences are exactly that shape.

The flip was worth one more step of checking rather than assuming. When it is set the sprite's
anchor x is *negated* at `0x0049CCCC`, which is the mirror image of the centre-relative placement
rule in [hotspots.md](hotspots.md) — a rule established months ago by a different method. That one
observation carries the claim. The flag is also pushed to the blitter at `0x0049D416`, but nothing
inside `0x004F46F0` was read, so that shows the flag *reaches* the drawing code and not what the
drawing code does with it. Corroboration, not a second independent count.

**Corrected on review, same day.** The first draft of this entry and of
[imp-format.md](imp-format.md) called `0x0049CCD3` an "odd-width correction". It is the opposite:

```asm
0049ccce  test dl,1              ; dl = low byte of the width
0049ccd1  jne  short 0049CCD4h   ; width ODD -> jump, skipping the dec
0049ccd3  dec  ecx               ; runs when the width is EVEN
```

`jne` is taken when the bit is **set**, so the `dec` it jumps over runs when the bit is **clear**.
The rule is `flipped_x = -((w >> 1) + placement_x) - (w even ? 1 : 0)`. Implementing the wrong
wording puts every even-width sprite one pixel off, and even widths are the majority — the same
one-pixel class of error that cost this repository weeks on the palette channel order. Both
reviewers found it independently; I had read the mnemonics in order and assumed the `dec` ran on the
tested condition. `recover_mirror_parity` now reads the branch polarity out of the binary and two
tests pin both polarities.

One more honest scope note while correcting it: the extra pixel is **Observed, not derived.**
Reflecting the unflipped span about the anchor column reproduces the engine exactly on odd widths
and lands two pixels away on even ones, so the `dec` is a convention of the engine rather than a
consequence of mirroring. The test asserts the discrepancy as well as the identity, so nobody
"fixes" the even case to match the algebra.

The corpus then produced an anomaly worth recording rather than smoothing over: **991 sequences set
the mirror bit on a single facing**, advertising `2 × 1 − 2 = 0` directions. It is inert — direction
0 resolves below the facing count before the fold is reached — but any consumer that trusts
`DirectionCount` on those files gets zero.

### Timing: the honest answer is that the field is not there

The issue asks to "measure frame cadence for MOVE, STAND, DIE and MELEE_ATTACK", which presupposes
the cadence is in the file. It is not, and the interesting part is how that was established rather
than merely believed.

A negative claim is worth exactly as much as the bound on the search behind it. **The first version
of this bound was overstated in three ways, and all three were caught on review.** Recording the
corrected argument and what was wrong with the old one, because the bound *was* the result:

**Refuted: "a sequence-record address can only be formed by scaling an index by 16."** It cannot,
and the refutation is code I had already read and quoted. `Imp::SetAction` caches the record pointer
into the player object at `0x0049DAA2 mov [esi+24h],eax`, and `Imp::CycleLength` reads it straight
back at `0x0049D8F7` with no scaling anywhere — from a function 12 out-of-module callers reach.
Scanning for the arithmetic was searching the wrong thing. The reviewer then ran the correct check
and it came out my way, which does not make my argument the one that got there.

**Corrected: "no reads at displacements 3–10."** My own survey output printed over a hundred
register-relative reads at displacements 4–10 — only the *byte-sized* rows were empty. I had read a
table I generated and reported a stronger sentence than it contained. Re-measured with the
exclusions fixed it is **124**: word and dword reads of frame heights, frame-table pointers, pixel
pointers and fields of unrelated objects. Nothing in that scan establishes what they are; the
pointer-following scan is what does. (The review quoted 120 from the old output; the difference is
the exclusions changing underneath, and six `lea`s that were being counted as reads and are not.)

**Corrected: the scan's exclusions were wider than its documentation.** It dropped every indexed
operand and every `ebp`-based one while disclosing only a stack exclusion. Both drops were wrong:
`0x0049AC8F mov di,[ebx+esi*8+2]` is a genuine record read at displacement 2, and in this module
`ebp` is an object pointer rather than a frame pointer (`0x0049ABFC`, `0x0049AC0F`, `0x0049AC43`).
A timing field read as `mov cx,[edi+ebx*16+2]` would have produced zero hits and the survey would
have printed exactly what it printed.

The argument that actually reaches the conclusion is in two parts:

1. **Who can obtain a sequence-record pointer.** `Imp::GetSequence` (`0x0049ADB0`) is the only
   function that returns one. Its direct call sites over the whole `.text` number **five**, and all
   five are in-module.
2. **What the module reads through one.** Following the pointer from both places it is created —
   the header table at `0x1C` with an index added, and the player's cached record at `0x24` — the
   in-module reads land at displacements **0, 1, 11 and 12 only. Zero at 2 through 10.**

*Both parts were amended again in the [second review round](#2026-09-17-second-review-round--the-instrument-was-blind-twice-more-and-a-stated-limit-hid-it):
part 1 needed the absolute-reference check to close the indirect route, part 2 was silently walking
only one of its two sources, and a third leg was added to type the out-of-module hits. The row
counts quoted here are the pre-fix ones; [imp-format.md](imp-format.md) carries the current table.*

The one rule that made part 2 work is worth stating: a `mov` that *dereferences* a tainted pointer
must kill the taint, because `mov eax,[eax+edx+0Ch]` yields the facing table a sequence record
points at — a different object. The first run propagated through it and duly reported facing-record
reads as sequence-record reads, which is how a scan built to bound a negative can manufacture a
positive.

Displacements also cannot type a struct, and in this engine that is not hypothetical: offset `0x1C`
is the sequence table on the IMP header and the *current frame record* on the player object
(`0x0049CC80`). A scan keyed on displacement alone conflates them.

And the structural part, unchanged: the playback object's constructor at `0x0049C830` has no timer
field; its only numeric default is the direction denominator, 8.

Cadence is the caller's. The terrain-sprite driver at `0x0050C31F` shows the shape it takes:
`(per-object phase + [0x005AF134]) mod CycleLength`, one global counter for every sprite on screen.
So the viewer's fixed interval is not a guess awaiting better metadata; it is the right *kind* of
answer with an unverified number, and the number lives in the engine's tick loop rather than in any
`.imp`.

### Sequence byte 2: shaped like a frame rate, and that is not good enough

Byte 2 has a non-garbage distribution — 12 values in 1–16, 15 in two thirds of sequences. (The
first draft called it "the only" such byte, which was wrong and my own survey output said so: byte 3
is `0x01` in 4,661 records and `0x04` in 6, byte 4 is `0xFF` in all 4,667, and 501 records carry a
patterned value in the high bits of byte 0 that every reader masks away. Constant is not garbage.) It is tempting and it would be useful. Three findings, in increasing
inconvenience: the engine never reads it; it is not the frame count (97 of 4,667 match); and it is a
property of the *file* rather than the action — 1,624 of 1,800 members give every sequence the same
value, and `MOVE` takes 10, 15, 6, 11 and 8 across different creatures. That is consistent with an
export-time frames-per-second and equally consistent with a version number. Wiring it into the
viewer as a frame rate would be a guess wearing metadata's clothes, so it stays labelled Inferred.

Bytes 5–10, by contrast, are settled and unglamorous: they are uninitialised authoring-tool memory,
containing readable fragments such as `frames` and `\imps\`.

### What this leaves

Issue #2's acceptance criterion is that **the viewer** derive direction labels and playback timing
from verified metadata rather than a fixed guess.

The first draft of this entry claimed "direction is now derivable" and treated that as satisfying
the direction half. It does not, and the review was right to press on it: `imp_anim` has no
consumers outside its own survey example, and `src/main.rs` still steps forward-only on a fixed
100 ms. What this work did was convert #2 from "we do not know" into "we know and have not applied
it" — real progress, and not the criterion. It also turns an unknown defect into a documented one:
**955 sequences are ping-pong and the viewer plays them forward-then-jump.**

So the premise of the timing half is **Refuted** — there is no cadence field in an `.imp`, and no
amount of further decoding will produce one — and the issue splits into three:

1. Wire the cycle mode and the direction fold and flip into the viewer. Fully specified now; no
   further reverse engineering needed.
2. Find `0x005AF134`'s writer for the tick period. One global constant, a static exe question. A
   literal-dword search finds only readers, so it needs a data-xref pass.
3. Anchor direction 0 to a compass bearing. Still needs an attended run or a GameScript call site
   with an independently known bearing; two rotations sit in the way (`0x005AEC3C`, and a `+1` at
   `0x0049DCDD`).

### A contradiction to record rather than resolve: the palette channel order

Found while scanning for byte-sized reads and initially filed as out of scope, which is the one
outcome to avoid — an out-of-scope observation that goes nowhere is an unrecorded finding.

`0x0049B220`–`0x0049B269` is a 1,024-byte loop over **the IMP's own palette**. It reaches it by
loading the palette pointer from the loaded header at offset 8 — `0x0049B247 mov eax,[edi+8]`, where
`edi` is the file image — which is the same field `imp.rs:535` reads as `read_u32(source, 8)`. It
writes a three-byte destination triple in the order `(p2, p1, p0)`:

```asm
0049b258  mov  dl,[eax+1]
0049b25b  mov  bl,[eax]
0049b25d  mov  al,[eax+2]
0049b260  mov  [ecx-4],al      ; p2
0049b263  mov  [ecx-3],dl      ; p1
0049b266  mov  [ecx-2],bl      ; p0
```

That is a pure reversal. `imp.rs:566` maps `(p0,p1,p2) → (p1,p2,p0)`, i.e. it reads file order as
**B, R, G**. Those two cannot both be a plain channel identity: a pure reversal is benign only if
the file is R,G,B and the destination B,G,R, and file-order R,G,B is the community reading that
`imp.rs:563` explicitly refutes.

**Both observations stand; the resolution does not.** Codex notes that `0x0049B2A0` repacks the
result into a 16-bit format using shift globals, so the destination of this loop may not be a plain
byte triple at all, in which case there is no contradiction to resolve — only a second consumer with
its own convention. The two candidate resolutions are:

- the destination is not a linear RGB triple, and the reversal is an artefact of whatever
  `0x0049B2A0` expects; or
- one of the two readings of file order is wrong.

**`imp.rs` is deliberately untouched.** The attended measurement behind the current code is strong —
14 of 14 sampled indices fit `(p1,p2,p0)`, the next best permutation fits 4, and writing raw
`ff 00 00` renders blue — and this repository's rule is that a recorded contradicting symptom wins
over a clean-looking disassembly. This entry exists so that if the palette is ever questioned again,
the four addresses are already written down.

## 2026-09-17 (second review round) — The instrument was blind twice more, and a stated limit hid it

The IMP animation result survived every challenge to its content. The instrument that produced it
did not, for the third round running. Field-level detail is in [imp-format.md](imp-format.md); this
entry is about the pattern, because the pattern is now the finding.

### The two blind spots, both introduced by the previous round's fix

`record_pointer_reads` — the scan written specifically to replace an argument that had been refuted
— cleared taint from any register that appeared as an instruction's first operand. `test`, `cmp` and
`push` all have a register first operand and write nothing. **Every cached-pointer reload in this
engine is immediately null-checked:**

```asm
0049d8f7  mov  ecx,[ecx+24h]     ; the cached sequence record
0049d8fa  test ecx,ecx           ; <- erased the pointer it never touched
0049d8fe  mov  cl,[ecx]          ; never seen
```

So the `PLAYER_CACHED_SEQUENCE` source — the source added that round, the whole point of the
rewrite, the thing whose absence had *refuted* the previous argument — contributed **zero** reads.
Part 2 was a walk of two `[reg+0x1C]` chains with a second source that did nothing. Fixed by asking
`instr_info` which registers an instruction writes instead of guessing from operand position, which
is the general form of the fix and was available the whole time: `instr_info` was already an enabled
feature.

Second: the `add` rule carried the pointer only when the destination was already tainted.
`Imp::DirectionCount` forms its record the other way round — `0x0049D95D add eax,edx` with the table
in `edx` — so `0x0049D95F` and `0x0049D967`, the mirror-bit test and the facing-count read this very
document quotes, were invisible to the scan that was supposed to enumerate them.

Corrected rows: displacement 0 goes 1 → **2**, displacement 1 goes 2 → **3**, displacement 11 goes
5 → **6**. **Displacements 2–10 stay at zero**, which is the only reason the conclusion stands, and
it is a checked fact rather than an assumed one.

### The worse half: a stated limitation that explained the defect away

The previous entry and the field document both said the missing rows were a consequence of the walk
stopping at the first `call`, citing `0x0049D9E6` and `0x0049D900`. Both citations were wrong —
`0x0049D900` is `and cl,7`, not a read at all, and the read that was actually missing,
`0x0049D8FE`, sits four instructions after its load with no `call` between them. The rows were lost
to the taint bug.

That is the part worth keeping. A limitation section is supposed to be where a reader goes to
calibrate a result. A limitation that *plausibly explains a bug* converts the bug into expected
behaviour and stops anyone looking for it — including the person who wrote it. I had a symptom
(a source contributing nothing) and an explanation ready to hand, and the explanation was close
enough to true in general that I never tested whether it was true here.

### The pattern across three rounds

**Every defect in this work was an instrument that could not find what it was looking for, and twice
a stated limitation made the blindness look intentional.**

| Round | The instrument's blindness | What it hid |
| --- | --- | --- |
| 1 | scan dropped indexed and `ebp`-based operands | a field read as `mov cx,[edi+ebx*16+2]` would have shown nothing |
| 1 | the argument searched for `shl`+header arithmetic | the cached-pointer route, which has no arithmetic |
| 2 | taint propagated through a dereference | facing-record reads reported as sequence-record reads |
| 2 | `lea` counted as a read | six phantom reads in a quoted total |
| 3 | taint cleared on non-writing instructions | the entire second pointer source |
| 3 | `add` carried only one way round | the two reads in `DirectionCount` |

Not one of these was a wrong conclusion about the engine. All six were the measuring device
answering a narrower question than the one being asked, while reporting in the vocabulary of the
wider one. The standing lesson from this repository's own files — *verify the instrument before
building on it*, and *an empty result is not a clean result* — applies to a disassembly scan exactly
as it applies to a speed monitor: **when a scan built to find something finds nothing, the first
hypothesis is that the scan is broken, not that the thing is absent.** Three regression tests now
encode that for this scan: a null check between a load and a dereference, an `add` either way round,
and a store or `lea` mistaken for a read.

### Also this round

- **Part 1 is now actually closed.** `call_sites` only sees `NearBranch32`, so enumerating direct
  callers of `Imp::GetSequence` left the indirect route open. The address `0x0049ADB0` appears
  **nowhere in the file as a literal dword**, so no vtable slot and no `mov reg,imm32` can reach it.
  That is the fact that closes it, and it is now asserted rather than assumed.
- **Part 1 does not cover the inline route, and the scan I offered for it was circular.**
  `sequence_record_sites` was being run over the module and then used to conclude the sites were all
  in-module. Run over the whole `.text` it finds 226 stride-by-16 sites, 22 near a `[x+0x1C]` load,
  and **13 of those outside the module** — so it narrows a set to read by hand and evidences
  nothing. The inline route is covered by a new leg that types the loads: a `[x+0x1C]` load is only
  an IMP header load if `x` came from an `Imp` object's field 8 (`0x0049ADB7`), and **no read at
  displacements 2–10 anywhere in `.text` has a source in that set.**
- **Only one of the two constants had actually been parameterised.** The previous entry claimed
  both; `mirrors_facings` still read `SEQUENCE_MIRROR_BIT` directly and `AnimRules::mirror_bit` was
  consumed by nothing. The hole was open precisely for the next caller — the one who wires this into
  the viewer without calling `recover`.
- **`Imp::CycleLength` has 12 out-of-module callers, not 25.** 25 is `Advance`'s. Both `.md` files
  had it right and the source comment introduced that round had it wrong: the third
  one-file-not-the-other inconsistency on this branch, and the source comment is the copy that gets
  read while editing.
- **`iced-x86`'s `nasm` feature is back to a dev-dependency.** Making it a real dependency linked a
  formatter's tables into the SDL viewer and the map editor so that one analysis example could print
  a debug string, in a crate that pins `default-features = false` on four dependencies.
  `FieldRead` and `TaintedRead` now carry the decoded `Instruction`; the example owns the formatter.
- The code range comes from `PeImage::executable_ranges` rather than a hardcoded length, so "the
  whole of `.text`" means it.

## 2026-09-17 — Inside the operator bodies: five fetch helpers, 71 arity disagreements, and two controls that failed

The operator tables have been known since #13/#15 — 1,906 names, entry points, and a static count of
operand-stack traffic. This is the first time anything has read the function bodies. The analyser is
`spikes/asset-viewer/src/operator_bodies.rs`, the driver is `examples/operator_bodies.rs`, and the
output is committed as `reports/natives/operator-bodies.tsv`, `global-clusters.tsv` and
`summary.md`. The prose is [inside the native operator bodies](native-operator-bodies.md).

### The controls first, because two of them failed

Run against operators whose behaviour is known from outside the binary:

| Control | Result |
| --- | --- |
| `savescenariomap`/`savespecialmap` write byte-identical files | **Reproduced** — same object `0x005aa12c`, same callee `0x00485550` |
| the map operators act on one object | **Reproduced** — exactly one address in common, `0x005ae958`, named by 139 operators |
| `resetvisibility` is nullary | **Reproduced** |
| `drawimpframe` takes 6 and `getimphotspot` 5 | **Reproduced** by a different method than the hand reading in #15 |
| `resetvisibility` clears cell bit `0x00800000` | **Failed** |
| `setterrain` mutates the map | **Failed** — classified `reads-state` |

Both failures are one limitation. The engine is C++ with singletons in `.data`; an operator's body
is fetch operands, `mov ecx,<singleton>`, call a method, and the store happens frames down through
`this`. A static direct-call graph cannot follow it, and neither can import evidence: the reach
curve is printed in the summary and shows that by depth 3, 93% of operators reach `user32`, a timer
and the allocator, and by depth 5, 90% reach `CreateFileA`. `savescenariomap` reaches the file
imports at depth 5 — so does `dup`. The classifier therefore reads depth 1 and `savescenariomap` is
not labelled as file I/O. Saying so is better than a label that would have been produced by the call
graph's density rather than by the operator.

The one tempting fix was measured and rejected. Attributing a callee's store-through-`this` to the
singleton the caller named does make `setterrain` a mutator — and it does the same for 55% of the
table, because the engine's getters cache into their own object. It is carried as its own column and
never promoted. **`reads-state` in the table means "performs no store of its own", not "has no side
effects", and `setterrain` is the standing counter-example.**

### The engine has five operand-fetch helpers, and finding one is not enough

`main.rs` already warned that the recorded arity undercounts operators that fetch through the shared
helper at `0x0040adb0`. Searching for that helper *by shape* — a small function many operators call
whose own body pops exactly once and pushes nothing — finds five, not one: the `thiscall` fetch that
hands back the raw `(tag, value)` pair, and four `cdecl` fetches that coerce on the way out. With
only the first recognised, 144 operators came out nullary that are not. There is also exactly one
result-push helper, `0x0041d1d0`.

The second bug was worse because it was silent. Labelling each instruction with the operands
consumed before it, and keeping **one** label per address, makes the answer depend on the order the
queue happens to visit blocks in: every fetch checks for underflow, and the underflow path of a
four-operand operator reaches the `ret` first, so the operator was reported as consuming nothing.
Propagating every distinct count that can reach an instruction fixes it; a count that grows around a
loop is capped and reported as **unbounded**, which is what a variadic operator is.

### 71 disagreements with the recorded arity, 68 in the expected direction

| | |
| --- | ---: |
| one count on every returning path | 544 |
| paths disagree; nominal count is the successful path | 1,362 |
| genuinely variadic (count grows around a loop) | 31 |
| nominal count **higher** than the recorded site count | **68** |
| nominal count **lower** | **3** |

Largest gaps: `launchmissile` 21 against 2 — 18 distinct fetch sites chained down one path —
`addbuildinginfo` 15 against 6, `toptriangle` 11 against 3. `getimphotspot` 5 and `drawimpframe` 6
match the values read by hand in #15, so the disagreement with the table was already known and is
now measured rather than annotated.

The three the other way are `button`, `setunitdata` and `nsetunitdata`, which pop different numbers
on different branches; a site count is the larger by construction. They are named in the test rather
than excused.

The 31 variadic operators include `astore`, `container`, `setformation`, `setregionfaiths` and
`setregionraces`. `astore` popping a script-determined number of operands is what PostScript's
`astore` does, and is the best independent check on the loop detection available here.

### Boundaries, reported as a rate

99.9% of bodies walked to completion; one ended at an unresolvable indirect jump. 2.9% ran past the
next operator entry point, which is the honest **upper bound** on boundary failure rather than a
count of failures — operators are not laid out contiguously and some tail call far away.

### What the clusters are

Absolute data references — including a displacement behind a register, and an address materialised
as an immediate, which is how the engine names its singletons — clustered at a `0x100` gap. The gap
is not fitted; the sweep is printed (247 clusters at `0x20`, 86 at `0x100`, 12 at a page).

`0x005aa12c` is named by **299** operators (`addbuilding`, `addcapitol`, `buybuilding`, both map
writers) and `0x005ae958` by **139** (`anythingat`, `armyat`, `buildingat`, `cityat`, `cantmovehere`,
`setterrain`, `terrainspriteat`, `resetvisibility`). Reading those as the scenario object and the
world object is **inferred** from the operator names; the addresses and the membership are observed.
Sixteen bytes at `0x00584ae0` are named by 128 operators including `blackbackbuffer` and
`blackrenderbuffer` — the render targets. `0x0054dbc0` is not state at all: it is the `.rdata` float
pool `abs`, `add`, `atan`, `cos`, `div` and `eq` share.

The useful part is not that the two biggest clusters are the obvious two. It is that the map API is
now **enumerable**: 139 named operators, recovered without reading a single one of their names.

### Tests

Relations, not restated numbers, and most of them run without the binary: the two map writers share
a callee; the map operators intersect on exactly one address; the stack primitives touch no engine
state; the body walk never undercounts the recorded site count except for the three named branching
operators; the boundary walk completes for ≥99%; every operand count has a mechanism behind it. With
`LOM_EXE` set, the committed table is re-derived and required to still match the binary, and the
helper search is required to find more than one helper — the specific regression that made a third
of the table look nullary.

## 2026-09-17 (review) — Seven corrections to the operator-body pass, and the corpus cross-check that should have been first

Cross-review of the body analysis. The core survived — the five fetch helpers are real, the boundary
claim was if anything understated, no game content leaked — and seven things were wrong. Six were
wrong in the write-up or the classification; one was a real defect that made the whole arithmetic
family unclassifiable. Two more things came out of doing the review properly.

### The check that should have been first: the script corpus

Nothing in the first pass consulted the `.gs` members, which are the engine's own callers, and
`tools/gs_callsites.py` has existed for exactly this. Five predictions now checked:

| Operator | Call site | Operands there | Body walk | Recorded |
| --- | --- | ---: | ---: | ---: |
| `xywh` | `PANELS5.gs:51:497` | 4 | 4 ✓ | 4 ✓ |
| `addcitymod` | `gs/spells/fireworks.gs:1:1129` | 9 | 9 ✓ | 2 ✗ |
| `bargraph` | `selarmy2.gs:228:79` | 8 | 8 ✓ | 2 ✗ |
| `setbuildingrequirements` | `building.gs:549:31` | 8 | 8 ✓ | 1 ✗ |
| `getplayergroupintoformation` | `getinfrm.gs:1:1079` | 8 | 8 ✓ | 1 ✗ |

Four of the five disagreements are settled in the body walk's favour by an independent source. That
is worth more than the other six fixes together, and it was available the whole time.

`launchmissile` is **corroborated, not settled**: three of twelve call sites pass exactly 21 tokens
that each resolve to one value; the other nine sit inside procedures the engine invokes with
operands already on the stack. What all twelve settle is that none passes 2.

### The one real defect: `.rdata` was counted as engine state

`is_data_address` tested only `!executable`. `.rdata` is `0x40000040` and `.data` is `0xc0000040`,
so the float pool at `0x0054dbc0` counted as state, `abs`, `atan`, `cos`, `sqrt` and fifteen others
were published as `reads-state`, and the class that describes them had **zero members in a 1,906-row
table**. A class with no members should have been read as a bug and was not. Now gated on
`IMAGE_SCN_MEM_WRITE`, with the section carried through to the artifact as `constant_addresses`.

**The fixture is why nothing failed.** The synthetic PE had one data section with `0x40000040` — no
writable section at all — so no test could distinguish the two readings. It now has three sections
with the engine's own flags. Same lesson as the square-map and uniform-facing fixtures: a fixture
built to look adequate cannot fail on what the real image has.

Two related imprecisions fell out of the same change: this linker puts **string literals in
writable `.data`**, so a format string was engine state until printable literals were made constants
regardless of section; and a write was being inserted as a read as well, duplicating the address in
343 rows, which is why grepping the table for a cluster's membership gave 140 and 308 where the
truth is 139 and 299.

### `mutates-state` was false for 154 of its rows

It was granted on a *direct callee's* store, so `armycanmove?`, `armystrength`, `armyexpense` and
`ambientlight` were mutators. The false-negative direction (`setterrain`) was documented at length
and this one was not documented at all — so a reader filtering for the state-editing API got
predicates and still did not get `setterrain`. **Both errors at once.** The class now requires a
store in the body itself; the callee's store is `state_write_depth` and `calls_mutating_method`, the
treatment `calls_mutating_method` already had for the same reason.

### `stack` contained no stack primitives

Twelve of its seventeen members shared one entry point that is a single `ret` — `savegridflags`,
`sunlight`, `makelighttables`, `setplane` and eight more are **registered names with no
implementation**, which is a finding in its own right and is now the `stub` class. Meanwhile `dup`,
`exch`, `pop` and `roll` were in `unknown`, because each calls the script error raiser and that
counted as "calls something" — while a comment in the source claimed this measurement had moved
`dup` out of `unknown`. A comment describing an outcome the shipped table does not have.

Callees are now discounted when they cannot distinguish one operator from another: the shared
operand helpers, any callee more than half the table calls, and any leaf that references no data and
reaches no import. Same saturation argument the import depth is chosen by. The first attempt at the
rule was written from a guess that the error raiser touches no globals; measuring it — which is what
the new `--function` flag is for — showed it references the engine's error-message objects.

The class was also renamed. `stack` claimed more than the evidence: `sleep` and `debug` qualify as
surely as `dup` does. It is `operand-only`, and `arithmetic` is `floating-point`, because the
evidence is an x87 instruction and `sleep` is in it for coercing a float delay.

### The helper headline did not affect a published number

"With only the first helper recognised, 144 operators came out nullary" is a fact about an iteration
of this work that predates the generic callee-pop folding, and was presented as a property of the
shipped analyser. Measured on what shipped, collapsing helper discovery entirely changes
`helper_pops` for **318** rows, `behaviour` for **44**, and `nominal_arity` for **3**. The arity
result rests on the generic folding, not on the helper search. Restated in the docs, and the test
that asserted the wrong consequence in its failure message now guards the two columns the search
does own.

### Quoted figures contradicted the committed artifact

`docs` said 93% of operators reach the allocator at depth 3; the artifact says the allocator is 1%
at depth 3 and 94% at depth 6 — the 93% row is `other`. A rustdoc block argued "nothing below three
is visible… past four the archive reach saturates" directly above `CLASSIFY_DEPTH = 1`, which would
have led a maintainer to relabel 93% of the table. Three different numbers were quoted for the
store-through-`this` share. All corrected against the artifact; the decision the 55% figure
supported is unchanged and is stronger on the real number — folding it in classifies **1,336 of
1,906 (70%)** as mutators.

`summary.md` also contradicted itself: "`unknown` is what an incomplete walk produces" for 290
operators, two sections below a table reporting **one** incomplete walk. A reader of that file alone
concluded the coverage was 85%. The line now prints both counts from the data.

### Both binary-backed guards were green by default

`executable()` returned `None` when `LOM_EXE` was unset *and* when the read failed, so a typo'd path
was indistinguishable from no path, and `cargo test` on a machine without the binary reported "8
passed" while checking nothing. They are now `#[ignore]`d — run with `cargo test --release --
--ignored` — and panic when the variable is missing or unreadable. The staleness check compared 3 of
28 columns, and the two the offline anchors read were not among them; it now regenerates the whole
table and names every field that moved.

### Two things the review produced that were not fixes

**The variadic evidence was stronger than the argument given for it.** The write-up leaned on the
analogy with PostScript's `astore`. The real evidence is that `armyexpense` and `repoman` have
candidate counts 1, 6, 11, 16 … 76 — an arithmetic progression of step five — and
`combat_controltarget` steps by two. Converging early-exit paths cannot produce that; only a loop
popping k operands per iteration can. Leading with the progression also exposed that loop detection
was a proxy — "the successor sits at a lower address" — which is not a back edge at all, because the
compiler puts the shared error epilogue below the code that jumps to it. It declared 26 operators
variadic that fetch through a per-subsystem wrapper, and a variadic callee contributes nothing to
its caller, so those callers came back nullary. Loops are now found by strongly-connected
components, and "many candidate counts converge" is a separate flag: `launchmissile` has 21
candidates and no loop and keeps its count; `slider` has 20 and a loop and has none.

**A virtual call can be named even though it cannot be followed.** The taint chain survives
`mov ecx,[global]` → `mov eax,[ecx]` → `jmp [eax+0x58]`, so the object and the vtable byte offset both
come out. Twenty operators carry one, and the network family resolves into a partial vtable map of
the session object at `0x005d1e84`: `+0x08` `createnetworkgame`, `+0x0c` `joinnetworkgame`, `+0x10`
`modemcreate`, `+0x14` `modemdial`, `+0x18` `enumnetworkcomputers`, `+0x1c` `enumnetworkgames`,
`+0x20` `selectprovider`, `+0x48` `enumproviders`, **`+0x58` `netlockgame`**, `+0x64`/`+0x68` and
`+0x6c`/`+0x70` the provider art accessors.

`netlockgame` — the single body the walk cannot finish, and the reason it cannot — is six
instructions: load the session object, return immediately if it is null, otherwise nullary tail call
into vtable slot 22. Slots `+0x24` through `+0x54` are reached by no operator at all, which is where
a turn-synchronisation method with no script-visible name would sit. That was only visible because
the taint bug behind it was fixed: the invalidation rule dropped a register's taint whenever the
first operand was a register, and `test ecx,ecx` writes nothing.

## 2026-09-17 — The map loader, read out of `lomse.exe`

Everything this project knew about the map header, the cell word and the six trailing-record sizes
was inferred from statistics over 365 shipped files. This run read the same questions out of the
engine's instructions instead. Tool: `spikes/asset-viewer/examples/map_loader_survey.rs`. Full
findings with every address: `docs/map-format.md`, "What the loader does, read out of `lomse.exe`".

### Method, and why `resetvisibility` was the right place to start

The only confirmed fact about any cell bit was that `resetvisibility` clears `0x00800000`. That made
it the one anchor that could prove the survey was looking at cell memory rather than at one of the
binary's several other eight-byte-strided arrays. Following it gave the map object at `0x005ae958`,
its cell array at `+0x54`, count at `+0x58`, width at `+0x5c`, height at `+0x60` — all confirmed
independently by the allocator at `0x004a4f20`, which stores exactly those fields and mallocs
`width * height * 8`.

The single most useful structural fact fell out of that: `0x005aa12c + 0x482c == 0x005ae958`. The
scenario object's terrain member *is* the global map object. So `loadmap` and `loadscenariomap` reach
one cell array, and `resetvisibility` and the file loader are provably talking about the same memory.

### Confirmed

- **The loader switches on the header word at `0x00`.** Six places: `0x0050dac2`, `0x0050db14`,
  `0x0050db87`, `0x0050dba1`, `0x0050dbbb` in the record serialiser, and `0x00485617` in the
  scenario reader. The previous entry in `map-format.md` called this Inferred and warned the
  correlation might be nothing but successive editor builds. It is not; the loader reads it.
- **The six record layouts are one struct with five version gates.** Evaluating the five thresholds
  reproduces all six sizes and all nineteen corpus header words with nothing fitted. 63/73 → 48,
  76–89 → 52, 96/97 → 53, 98/101 → 47, 102–111 → 49.
- **Cell word 1 is an `f32`.** Fifteen x87 sites on it, `setelevation`'s worker at `0x004a59f3`
  being the clearest. No longer an inference from value ranges.
- **`savescenariomap` and `savespecialmap` are literally the same function**, `0x00485550`. The
  corpus observation that they write byte-identical files now has its cause.

### Contradicted

Five prior readings in `map-format.md` were wrong, and each was wrong in the same way — the corpus
could not distinguish the true rule from a nearby one, so the nearby one got written down:

1. **`0x0c` is not `bits_per_pixel`.** It is bytes per cell. The reader multiplies it by 64
   (`0x004a535c`) to size a block read; the writer emits the literal `8`. "Always 8 in the corpus"
   is true of both readings, which is why the corpus could not tell.
2. **Cell word 0 is not a 32-bit tag.** It is two `u16` fields. Every engine read of byte `+0` is
   `movsx r32, word`, every write is 16-bit, and nothing reads the word as a dword. Bits 10–15 are
   part of the *tile slot*, which is signed 16-bit and bounds-checked against the tileset at
   `0x00508f10` — not a separate unused region as documented.
3. **`tile_index` was `tag & !0x00800000`; it is `tag & 0xffff`.** The two agree on every corpus
   cell, because `0x00800000` is the only bit above 15 any corpus cell sets. Corrected in `map.rs`
   with a test that fails under the old rule.
4. **The "four-byte footer" is not part of the record section.** `cmp dword [esi],64h` at
   `0x00485617` gates a separate dword belonging to a different member of the scenario object. The
   corpus split at 98-no-footer / 101-footer brackets the engine's threshold of 100 exactly, which
   is how a section boundary got read as a record-layout field.
5. **There are eight record *kinds*, not one record shape.** `0x004f7120` dispatches the first
   dword of each record through a ten-entry jump table at `0x004f73b8` with two dead slots. The
   whole corpus is kind 1.

### Reconciled, not contradicted

The two attended `resetvisibility` runs that looked like they disagreed — "all 4,096 cells set" and
"0 of 4,096" — are both correct. The perimeter-writing block at `0x004a9151`–`0x004a91a1` is guarded
by `test eax,eax; je` on map object `+8`. A fresh `clearmap` map has that field zero, so the block is
skipped. The 146 `.smp` files carrying `0x0080` on exactly their perimeter ring were saved with it
non-zero. `mov edx,80h` at `0x004a915f` is where the value comes from.

### Not settled, stated rather than glossed

- **What the `+2` field means.** `resetvisibility` writes it; *nothing in the 148 surveyed
  map-object methods reads it*. "Visibility" stays Inferred from the operator's name. Whatever
  consumes it is outside the survey — most likely the renderer, reached through a pointer this
  analysis does not follow.
- **No cell bit is ever masked.** Across all 35 cell-lane instructions the survey found zero `and`,
  `test`, `or`, `xor`, `shr` or `sar` with an immediate against a cell lane. The bit-by-bit map this
  run was asked for is empty, and that is the finding: the engine does not work on the cell in bits.
- **The thresholds 74 and 75 cannot both be checked.** The corpus contains 73 and 76. The binary
  says 74; the corpus can pin it only to an interval, and the test says so.
- **The gate at 54 is unexercised by the corpus.** Every shipped header word is at least 63.

### Two method notes worth keeping

**A linear taint walk reported a third of the cell accesses.** The first version of the survey
walked each function's instructions in address order. It found 17 sites. The taint was dying at
early-return epilogues — `pop esi` on a path that is never reached from the code after it — and
`getterrain`'s own cell read was among the casualties. Replacing it with a forward must-analysis over
basic blocks, merging by intersection, took it to 35. A survey that reports "the engine barely
touches the cell tag" when it means "the analysis stopped early" is the empty-review failure in
another costume.

**Linearly decoding a whole `.text` section silently loses call sites.** The scan that finds
`mov ecx, 0x5ae958` followed by a `call` originally decoded the section from its start; it drifts out
of phase on the first embedded jump table and never recovers. Searching the raw bytes for the
five-byte `mov ecx, imm32` encoding and decoding forward from each hit is what made
`forcetexture`'s and `getterrain`'s workers visible.

### 2026-09-17, same day — cross-review of the above, and what it overturned

Claude and Codex both reviewed the entry above independently. Every *positive* binary fact survived
byte for byte: the `shl ecx,6` block sizing, the 8-byte stride, the signed bounds check at
`0x00508f10`, `fld dword [ecx+eax*8+4]`, the `cmp eax,9`/`ja`/`jmp dword [eax*4+0x4f73b8]` dispatch,
and the section-boundary reinterpretation at `0x00485617`. The *negatives* did not, and neither did
the writer.

**The negatives were negatives over an incomplete set — three separate ways.**

1. **`cell_lane` required an eight-byte index scale.** The engine constantly uses the scale-1 form
   `[array + byte_offset_register + disp]`, where the register already holds `cell * 8`. Every one
   of those was dropped, *including a write inside the survey's own anchor*:
   `resetvisibility`'s second perimeter write at `0x004a9178`. Also dropped: reads `0x004a9025`,
   `0x004a9048`, `0x004a938e` and writes `0x004a9036`, `0x004a9055`, `0x004a93a6`.
2. **Method discovery keyed only on `mov ecx, 0x005ae958`.** But the map object *is*
   `scenario + 0x482c` — my own claim 7 — and 28 sites reach it as `lea ecx, [reg + 0x482c]`, which
   mentions `0x005ae958` nowhere. Nine methods were invisible, one of which contains a plain
   scale-8 `+2` write the existing filter would have caught. Worth recording: the coordinator
   probed for `mov ecx, [reg+0x482c]`, found zero, and reported that as evidence the coverage was
   fine. The probe was structurally incapable of finding the `lea` form. That is the third instance
   on this branch of a measurement that could not fail.
3. **Neither path follows indirect calls.** The surveyed methods contain 35 indirect call sites.
   Virtual dispatch is uncovered, and the finding is now scoped to say so.

After fixing 1 and 2 the survey walked 159 methods (was 148) and found 53 cell-lane instructions
(was 35). **The no-masks result survived.** It then survived twice more — see the next section,
where two further access routes took it to 70 sites.

`cell_lane` now **counts every operand it reaches through the cell array but cannot decode** and
prints the total. It reports zero. The point is that the next gap of this kind will be loud.

**Two "never" sentences were false, and both were false in the same way: a true claim about
*interpreting* accesses written as a universal.**

- "The engine never reads a cell's first word as a dword" — refuted by `0x004a9660`/`0x004a9666`, a
  two-dword whole-cell copy. It decodes nothing, and the same function re-reads the low word with
  `movsx` at `0x004a9671` and bounds-checks it, so the two-`u16` conclusion stands. The sentence did
  not.
- "`+2` is never read by any of the 148 surveyed methods" — false *inside a method I had walked*.
  `0x004a938e movsx ecx,word [eax+esi+2]` → `cmp ecx,ebx` → `jle` → `mov [eax],bx`, plus the same
  shape at `0x004a9025` and `0x004a9048`.

**And the reader's shape is worth more than the miss.** `movsx` plus a signed `cmp`, never a mask:
the field is a **signed 16-bit scalar**, which *strengthens* the two-field split — a bitfield packed
beside a tile index would be tested, and this is compared. I had reached "Inferred" for the `+2`
field's meaning by the wrong route ("nothing reads it"); the label was right and the reasoning was
not, and the reasoning is now the update shape instead.

**One correction to the review itself.** It described the field as "monotonically raised". The
direction is the opposite: `cmp ecx,ebx; jle skip` reaches the store only when the stored value is
strictly *greater* than the candidate, so the stored value is replaced by a smaller one —
monotonically non-**in**creasing at the two `jle` sites. (`0x004a9025` uses `je` and writes on any
difference.) It does not change the reviewer's point, which was that the field is a scalar.

**The writer had not moved with the reader.** `set_tile` and `fill_terrain` still wrote
`(tag & 0x00800000) | tile`, preserving one bit of a field this branch had just redefined as 16 bits
wide and zeroing the other fifteen. Fixed to `tag & 0xffff0000`, with a test that fails under the
old mask. Severity, measured rather than asserted: across all 353 parseable shipped maps and
1,040,384 cells the upper field takes exactly two values, `0x0000` (1,012,936) and `0x0080`
(27,448). So it was **latent, not live** — no shipped map loses anything either way. Fixed anyway,
per this file's own standing rule that preserving what it does not understand costs nothing.

**Two smaller ones, both self-inflicted.** The 4,892/4,284 figures I quoted as the *scope
denominator* of a negative result came from `disassemble_section`, a deliberately misaligned linear
decode of the whole `.text`. They are withdrawn; the census is now computed from the same
per-function decodes the survey uses (45/45/7), and `disassemble_section` is deleted. And
`verify_anchors` hardcoded `RECORD_KIND_COUNT = 10` without reading the `cmp eax,9` operand — the
one thing that function exists to prevent. It now recovers the bound, the table address and the
handler set from the dispatch itself, and checks every slot lands in code.

**Four stale passages in files this branch edited** restated the retired 32-bit-tag model: the
`CELL_TAG_HIGH_FLAG` doc comment, the user-visible `--map-set-tile` error, and two prose passages in
`map-format.md`. All four now carry the correction rather than the old claim. The `1024` cap stays —
it is a good guard against a fat-fingered `3920` — but it is no longer claimed to be the field's
width, because the field is a signed `u16` and the tileset decides the real limit.

### 2026-09-17, later — two more access routes, and `0x00800000` is a saturated scalar

The coordinator inventoried the references the fix above still did not account for and found two
more ways into the map object. Both were invisible to every pattern the survey had. Verified against
the bytes before changing anything.

**Route 3: a dedicated accessor with 208 callers.**

```
004c6fc0  mov eax,5AE958h
004c6fc5  ret
```

That is the entire function. Callers do `call 0x004c6fc0`, then use `eax` — or move it to `ecx` and
call a method. No call site contains the literal `0x005ae958`, so neither the immediate scan nor the
`lea` scan could see a single one of them. Adding it: **+10 methods and +7 cell sites**, including
five `+2` readers.

**Route 4: the cell array loaded straight from the global.** `mov ecx, [0x005ae9ac]` —
`MAP_OBJECT + 0x54` — reaching the grid without touching the object at all. Eleven sites. This is
where the three `+2` readers the review cited but I could not reproduce actually live:
`0x004c5cc2`, `0x00517b67`, `0x00519ced`. They were real; my survey was blind to their route, not to
them. Adding it: **+10 cell sites**.

Final counts: **169 methods**, plus 219 mid-function entry points (208 accessor calls, 11 absolute
loads), and **70 cell-lane instructions** — double the 35 the first pass reported.

**The no-masks result survived all four widenings: 35 → 53 → 60 → 70 sites, still zero masks.**
Every one of the 35 recovered instructions is a `movsx`, `mov` or `cmp` with no immediate. That was
the predicted outcome each time and it was measured each time, which is the only reason it is worth
anything. The finding's honest scope is now: *no interpreting bitwise-immediate access to a cell lane
exists among map-object methods reachable without virtual dispatch.* The surveyed methods contain 36
indirect call sites; vtables remain unexplored and are the next step.

**The real prize was not the negative.** Route 4 led to the arithmetic that reframes `0x00800000`
entirely:

```
00519ced  movsx edx,word [ecx+eax*8+2]
00519cfb  mov eax,80h
00519d00  sub eax,edx                       ; 128 - field
00519d05  imul eax,esi
00519d08  sar eax,7                         ; ... * k / 128
```

`(0x80 - field) * k >> 7` is a linear interpolation whose denominator **is** `0x80`. The same shape
is at `0x00517b6f`. And `0x004c5cc2` compares the field against map object `+0x4c` rather than
against a constant.

**The next entry corrects the conclusion I drew from that.** I wrote "so the `+2` field is a level on
a 0..128 scale and `0x00800000` is that scale saturated" as though the disassembly established it.
The arithmetic establishes `0x80` as a *scale base*; the closed range and the saturation are a
further inference. What the arithmetic does settle, and settles firmly, is that the value was never
a flag.

Eleven readers exist, spread across the binary, and **every single one is a `movsx`**. Not one is a
mask. That is what makes the two-`u16` split solid rather than merely consistent: a bitfield packed
beside a tile index would be tested, and this is compared and interpolated.

The meaning is still **Inferred** — a 0..128 level that decreases monotonically, is compared against
a global threshold and scales a rendering quantity is consistent with fog or light intensity, and
`resetvisibility` writing it fits. The missing step is the consumer of the array `0x00519d0b` writes
into. But the label now rests on arithmetic rather than on an operator's name, which is a different
and much better position than yesterday's.

**One anomaly recorded without explanation.** Three byte stores write computed values into the map
object's offset `0` — `0x004a82b7`, `0x004a82c5`, `0x004a82f5`, with `and al,0Ch` and `sar eax,8`
feeding them, inside an operator body that also raises script error `0x0e`. It rules out offset `0`
being a vtable pointer, and that is all this survey can say about it.

### The lesson this branch actually taught

Four discovery patterns, four blind spots, and **every single failed probe failed by being unable to
find what it was looking for**:

- A linear `.text` decode drifts out of phase on the first embedded jump table and loses every call
  site after it.
- A linear taint walk dies at early-return epilogues it walks through but no path reaches.
- A search for `mov ecx, [reg+0x482c]` returns nothing because the member is a subobject whose
  address is taken with `lea` — and that nothing was read as reassurance.
- A count of "499 of 503 references to `0x005ae958`" cannot see routes 2, 3 or 4, because none of
  them contains that constant in the form being counted.

In every case the instrument returned a clean-looking result and the conclusion drawn was broader
than the instrument could support. The countermeasure now in the code is not a better pattern, it is
a **counter**: `cell_lane` tallies every operand it reaches through the cell array and cannot decode,
and the survey prints the total. It reads zero today. The point is that the next blind spot of this
kind arrives as a number instead of as silence.

**Process note.** The "Codex side" of the review of this branch was not Codex — that run produced
nothing usable and the review was written by a second Claude pass without saying so. The
cross-model gate is therefore unmet on this work, and the three `+2` read sites that pass cited
were nonetheless real and correct. Its one substantive error was direction: it described the field
as monotonically *raised*, where `cmp ecx,ebx; jle` reaches the store only when the stored value is
strictly greater, so it is lowered. A real Codex pass on the fixed branch is still owed.

### 2026-09-17, third pass — a real Codex review, and an overclaim of mine that it caught

The first "Codex" review of this branch was not Codex. This one was, and it disputed four of six
items. One of the four is the headline claim, and I got it wrong in the direction this project is
most prone to: I took an Observed arithmetic fact and wrote an Inferred conclusion in the same
sentence, at the same confidence.

**What I claimed:** the `+2` field is "a level on a 0..128 scale" and the corpus's `0x0080` is "that
scale saturated", labelled **Observed in a local binary**.

**What the instructions actually show:** the field is read sign-extended at all 11 readers, none of
them masks it, it is used as `(0x80 - field) * k >> 7`, and it is compared against map object
`+0x4c`. That makes `0x80` an arithmetic **scale base**. It does not make the field's range *closed*
at `0..128`, and **nothing disassembled so far clamps it**. A base of `0x80` is what you would use
for a fraction in `0..128` and equally what you would use for a fixed-point quantity that can exceed
it; the instructions do not choose. The corpus holding only `0x0000` and `0x0080` is a separate
Observed fact, and "therefore `0x0080` is saturation" is the Inferred step joining the two.

Now split into a table in `map-format.md` with each row carrying its own class. The part that
survives unchanged is the part worth having: **the bitfield reading is dead.** Eleven sign-extending
reads and no mask anywhere.

This is the same error as `feedback_agreement_is_not_confirmation`, in a new costume: the arithmetic
and the census agreed with each other, and I recorded their agreement as a measurement of the thing
they agree about.

**A second finding fell out of the sweep the review asked for, and it is a real refutation.** Four
older passages still described `0x00800000` in single-bit language, including "`forcetexture` — which
`clearmap` calls on every cell — *does* set the bit". Checking that against the 16-bit field model
refutes it: `forcetexture`'s worker at `0x004a5ed0` touches exactly two cell operands in the whole
function, a 16-bit read at `0x004a5efe` and a 16-bit write at `0x004a5f06`, both of lane `+0`. **A
16-bit store to `+0` cannot alter `+2`.** The gameplay measurement (a `clearmap` save carries the
value on all 4,096 cells) was right; the attribution was wrong, because `clearmap` does more than
call `forcetexture` on every cell. The grid initialiser at `0x004a50e6`, which fills every cell's
`+2` from map object `+0x48`, is the Inferred culprit.

And it rehabilitates a step this file had itself disowned. An earlier section said the
"`forcetexture` never sets it" observation "was an artefact of operation order and should not be
relied on". It was correct all along, for a reason neither gameplay run could see: the two
operations write different fields.

**The `cell-unclassified` counter was the right idea in the wrong place.** Codex's objection: it
"genuinely reaches zero only after taint reaches `cell_lane`" — it counts what arrives, not what
never arrived. Fair, and adding counters for the reachable parts of that hole proved it immediately:

| Counter | Value |
| --- | ---: |
| `cell-unclassified` | 0 |
| `survey-entries` | 388 |
| `survey-functions-truncated-at-2400-instructions` | 1 |
| `survey-unreached-blocks-with-cell-shaped-operands` | 20 |

The discovery loop also **reported that it had not converged** within its round cap the moment a
counter existed for it. Raising the cap showed the method set was already complete at 169 — the cap
was failing to *prove* convergence rather than losing methods — but I would not have known that
without measuring, and "4 rounds looked like enough" is exactly the reasoning that produced the
other four blind spots.

One counter I tried and rejected: a raw count of unreached basic blocks. It is dominated by junk past
the end of a function and **grows when the decode limit is raised**, which makes it an instrument
that gets worse as the analysis gets better. Filtered to blocks containing an eight-byte-strided
operand, 20 is a real number.

**The fix that actually answers the objection is a taint-independent scan.** The survey now checks
every instruction it decodes, *ignoring the taint entirely*, for a masking instruction whose operand
is eight-byte strided at a cell-lane displacement. It over-counts freely — any eight-byte array in
any surveyed function qualifies — and it finds **zero**. That is a statement no amount of missed
taint can weaken, and it covers the 20 unreached blocks too, since it reads them regardless of
reachability. It does not cover the scale-1 form, whose lanes cannot be identified without the taint,
nor any function the four routes never found.

**Four routes is the number found, not the number that exist.** Said that way now. Every one of the
four was discovered only after a negative result looked wrong, and there is no argument that the
list is closed: a pointer copied into another object's field, spilled and reloaded, passed as an
argument, or formed by arithmetic the taint does not model would be a fifth.

**One item I did not act on, because the coordinator adjudicated it against Codex and I reproduced
the adjudication.** Codex claimed `the_corpus_exercises_every_gate_it_is_able_to` would pass with an
empty `ENGINE_RECORD_GATES`. The constant is typed `[(u32, u32, u32); 5]`, so an empty array does
not compile; and mutating the first threshold from 98 to 99 gives `FAILED. 216 passed; 1 failed`,
which I ran. The claim was static reasoning from a reviewer whose sandbox could not take the cargo
lock. The test stays, and the adjudication is recorded in a comment on it so the next reader does
not re-litigate it.

**Calibration note on this review.** It returned six verdict lines and no detailed body, so its
silence on anything else is "not looked for", not "not present". Its four disputes were all
specific, all checkable, and three of the four were right.

### The pattern across all three reviews of this branch

Every defect found in the survey itself was an instrument that could not find what it was looking
for; every defect found in the *write-up* was an inference recorded at the confidence of the
observation next to it. Those are the two failure modes, and they need different countermeasures. The
first is answered by counters and by taint-independent cross-checks. The second is answered only by
splitting the evidence classes in the sentence where the claim is made — which is why the `+2`
finding now carries a six-row table instead of a paragraph.

## 2026-09-17 — Multiplayer is lockstep, the transport is DirectPlay, and the `.snp` files are Blizzard's

Full findings in [`docs/multiplayer.md`](multiplayer.md); this entry records what changed and the
two things I got wrong on the way.

### The answer

**Observed.** The game is a lockstep, deterministic, peer-to-peer simulation. Four independent lines
of evidence, none of which depends on the others:

1. The engine compares six checksums between computers and reports `Divergence` — `'EXE' version`,
   `'PlayAnimation Count'`, `'Random-Seed'`, `'IMP' files`, `'GS' files`, `'Player-Stats'` —
   per message ID and per tick. Reporting code at `0x004b4fc0`–`0x004b5260`.
2. Vanilla `gs/network.gs` hands `setchecksumproc` a procedure that checksums every army's location
   and facing and every unit's type, health, hit points, movement points, champion type, experience,
   unique ID and seven modifier words, plus each player's gold, food and crystals. The engine reads
   that procedure back at `0x004b4c81`, inside the checksum module.
3. The ~100 wire message types at file offsets `0x159c1c`–`0x15a1f0` are overwhelmingly *orders* —
   `MOVE_ARMY`, `ORDERS`, `BATCH_ORDERS`, `BUY_UNIT`, `END_TURN`, `SCRIPTCALLBACK` — with a smaller
   state-poke vocabulary and a `RESYNC_*`/`XFER` recovery path.
4. The state dump at `0x15a248`–`0x15a4f5` carries `gameseed`, `random count`, `halt_ticks`,
   `skip_ticks`, `half_speed_ticks`, `go_countdown`, `late_messages`.

**So the homelab verdict is negative on the thing that matters.** Desync is a determinism problem;
a better network cannot fix it. The actionable finding is that `'GS' files` and `'EXE' version` are
divergence classes, which means every peer needs byte-identical archives — mixed 3.02/GS5R3/vanilla
installs cannot stay in step. That needs no infrastructure at all.

### `netlockgame`, the operator that defeated the arity walker

**Observed.** Six instructions at `0x004b6020`: load the provider pointer, null-check it, load the
vtable, `jmp dword [eax+58h]`. It is nullary, pushes nothing, and the walker was right to refuse an
indirect branch. Slot 22 on the DirectPlay class is `0x0044b440`, which is `jmp 0x004b7dd0` — the
base member that returns the local computer id, the same one `thiscomputer` and `ishost` call. The
operator discards the result.

**On the transport the game uses, `netlockgame` locks nothing.** The shipped corpus calls it exactly
once, in `gs/Dlg/multidlg.gs`, immediately before `startnetgame`, where it was evidently meant to
close the lobby.

### Refuted: the `.snp` files are not DirectPlay service providers

**Refuted.** `Battle.snp` and `Standard.snp` export `SnpBind`/`SnpQuery` — the **Storm** Network
Provider interface, Blizzard's. `Standard.snp` declares `Direct Cable Connection` (`SERIAL.CPP`),
`Modem` (`MODEM.CPP`) and `Local Area Network (IPX)` (`IPX.CPP`) and imports no sockets library at
all. `Battle.snp` is Blizzard's Battle.net client verbatim, help text and all, describing a *Diablo*
chat screen and hardcoding `209.67.136.170;exodus.battle.net`.

**Observed.** `lomse.exe` imports 26 `STORM.dll` ordinals, every call site of which is in the archive
and Storm-wrapper modules plus the two allocator ordinals, and three `DPLAYX.dll` functions —
`DirectPlayCreate`, `DirectPlayEnumerateA`, `DirectPlayLobbyCreateA`. It carries a complete set of
source-level assertion strings for a class named `CDPlay`. The transport is DirectPlay; the `.snp`
files came in the box with Storm and are not loaded.

There are **four** classes in the provider hierarchy, recovered automatically from the constructor
stores that install their vtables: an abstract base (`0x0054dbf0`), `CDPlay` (`0x0054d548`),
`CSigs` (`0x0054d838`) and a Storm/SNet class (`0x0054d8b0`). `CSigs` is largely stubbed — five of
its slots, including `selectprovider` and `joinnetworkgame`, are `xor eax,eax / ret 4`.

### The correction that cost the most

**Corrected.** I first reported `0x005d1e84` as a global pointer with 110 reads and **zero** writes,
and concluded the provider abstraction was dead code in this build. Two independent scans agreed —
a byte-pattern search over the whole file and an `iced` sweep with real operand-access analysis —
and both were right about what they measured and wrong about what it meant.

`0x005d1e84` is field `+0x4b2c` of the singleton at `0x005cd358`. It is written at `0x004b6290` as
`mov [edi+4B2Ch],eax`. No absolute store exists to find because the compiler addresses a static
object's fields absolutely on *reads* while the writer holds the base in a register. **A global with
many reads and no writes is the signature of either an unassigned pointer or a static object's
field, and only the arithmetic tells them apart.** Two agreeing scans are one shared assumption, not
a confirmation. `native_dispatch::static_object_field` now makes that check one call, with a test.

The second, smaller error: my first draft of the survey tool annotated *every* singleton whose
address was below the pointer as "contains it", which named a dozen owners for one field. It now
reports only the nearest base below the pointer.

### What is now committed

`spikes/asset-viewer/src/native_dispatch.rs` recognises three dispatch shapes — virtual on a global
pointer, non-virtual member on a global pointer, and `thiscall` on a static singleton — reads
vtables, recovers the vtable a constructor installs, and finds every vtable in the image from the
stores that install it. Ten unit tests over synthetic PE images; two mutations confirmed to fail the
suite (dropping the `ecx`-provenance tracking, and dropping the requirement that a vtable be loaded
out of the object before an indirect call is read as virtual).

`spikes/asset-viewer/examples/multiplayer_survey.rs` drives it. Nothing is hardcoded to the
addresses this branch found: the provider pointer comes out of `netlockgame`'s own body, the vtable
candidates come from the constructor stores, and the slot span comes from the operators. It reports
which slots **no** operator reaches — slots 0, 1, 9–17, 19–21, 23, 24, 26 — which is where the send
and receive paths live (`CDPlay` slots 20 and 21, `0x0044b210` and `0x0044b130`).

### Concrete "touchy" mechanisms, for the record

**Observed**, all with addresses in `docs/multiplayer.md`: a 257-byte message payload cap on a fixed
receive buffer; `EnumSessions` with a **50 ms** timeout; a send path that `Sleep`s on the game thread
to rate-limit itself, with a 100 ms default post-send wait set in the base constructor; a
per-destination sequence number kept as a dword but sent as **one byte**; a silent drop path for a
flagged peer; and no IP-address field anywhere in the shipped UI, so discovery is broadcast-only and
`Join` refuses any name not already in the enumerated list.

## 2026-09-18 — The six checksums, an offline pre-flight checker, and a post-mortem that is switched off

Second pass on multiplayer. Full detail in [`docs/multiplayer.md`](multiplayer.md) from the heading
"Second pass". Three results, in order of how much they change what a person would do.

### 1. The desync post-mortem exists, fires in exactly the right place, and is disabled

**Observed.** `0x004b4940` runs the procedure registered by `setstatlogproc` — in vanilla
`gs/network.gs`, a full per-player, per-army, per-unit state dump — and has exactly **one** caller,
`0x004b52d7`, immediately after the `Divergence` report is formatted. The engine was built to dump
its state the moment a desync is detected.

**Observed.** It is gated on `[0x005843e4]`. That global is written at exactly one instruction in
the whole image, `0x004b585e` inside the `netlog` operator, with `edi` zeroed at `0x004b57d9`. It
lies past `.data`'s raw data, so it is zero at load. **The gate is zero at load and the only write
to it writes zero.** `netlog` itself contains no output call of any kind.

So the instrument is switched off with no switch to turn it on, which is a complete explanation for
why nobody in this community has ever diffed a desync. This **replaces** the first pass's closing
suggestion that the network log be captured; it cannot be, and no second machine was needed to find
that out.

**Why this global reading is sound when last week's identical-looking one was not.** On the previous
entry I read `0x005d1e84` as never written and was wrong: it is a field of a static object whose
writer holds the base in a register, so no absolute store existed to find. `0x005843e4` *is* written
absolutely, and each of its neighbours `0x005843e0`/`e8`/`ec`/`f0` is written absolutely by a
different operator — the signature of separate globals, not one object. The presence of an absolute
write is the check that distinguishes the two cases, and it is the check I skipped last time.

### 2. Two of the three install-derived checksums are reproducible; the third is not computed

**Observed.** The comparator dispatches on a value index through a six-entry jump table at
`0x004b54a8` (`cmp eax,5 / ja` at `0x004b4f9b`), giving index 0 `'EXE' version`, 1 `'GS' files`,
2 `'Random-Seed'`, 3 `'Player-Stats'`, 4 `'IMP' files`, 5 `'PlayAnimation Count'`.

**Observed.** `'EXE' version` (`0x004b4a95`–`0x004b4b2a`) is a 32-bit wrapping sum of the
**zero-extended** bytes of the running executable, cached in `0x00584424`.

**Observed.** `'GS' files` (`0x004d496c`–`0x004d4990`) is a 32-bit wrapping sum of the
**sign-extended** bytes of every script source the loader is handed, gated by
`gschecksumon`/`gschecksumoff` on `0x00584600` and accumulated in `0x00584604` — the same global
the state dump prints as `GS Checksum=%d`. It is **not** a digest of `gs.mpq`. That sharpens the
first pass rather than contradicting it: peers with different scripts still diverge, and the engine
notices by executing them.

The two byte sums are **not the same byte sum**: `movsx` against `xor edx,edx`. Any test vector made
of ASCII cannot tell them apart, which is how a guessed hash would have survived a weak test.

**Observed.** There is exactly one `CHECKSUM` builder (`0x004b4c20`, one caller) and its complete
set of payload stores puts four meaningful quantities in a ten-dword payload: the executable sum,
the script content sum, the game seed, and the script `setchecksumproc` result. Two slots are
hardcoded zero; two are never written. **Inferred: two of the six named classes can never fire and a
third may compare uninitialised stack.** Which class lands on which slot is **Unknown** — the
payload offsets and the value indices do not line up in any ordering I could justify, and I did not
pick one to make the table tidy.

**Observed.** `lomse.exe` contains no `.mpq` filename string, and the only two whole-file byte-sum
routines are the executable sum and a generic one at `0x004b1e60` serving the scenario transfer.
**Inferred: `'IMP' files` is not computed in this build.** So the pre-flight checker is built around
what was read, not around a plausible hash.

Also Observed, on the bounds: the checksum queue is **100 entries of 192 bytes** (`mov esi,64h` at
`0x004b4a71`, stride `0xc0`), and the comparison covers **at most four computers**
(`cmp ebx,4 / jge` at `0x004b4f43`) while computer records are indexed 1..16.

### 3. The pre-flight checker works, validated three ways

`examples/preflight.rs` over the new `install_checksum` module. Three installs on this machine have
byte-identical `lomse.exe` and `imp.mpq` and three different `gs.mpq` (confirmed independently with
`shasum` before trusting it), so the output is falsifiable.

| Pair | `'EXE' version` | `imp.mpq` | script content |
| --- | --- | --- | --- |
| baseline vs 3.02 | both `149429203` | identical | differs, 379 named members |
| baseline vs GS5R3 | both `149429203` | identical | differs, 2406 named members |
| 3.02 vs GS5R3 | both `149429203` | identical | differs, 2770 named members |

All nine predictions hold. **The corollary is the useful part: since the executable is byte-identical
across all three, `'EXE' version` cannot be what makes a modded install incompatible. Script content
is the whole story.**

Honest limits, both in the tool's own output: the archive comparison covers every member rather than
only those the engine loads, so it can raise a false alarm but cannot miss a real difference; and
the three-install test skips when the installs are absent, so on a bare machine it cannot fail. The
algorithms are separately covered by synthetic tests that always run. Mutation-checked: swapping the
sign-extension for a zero-extension fails two synthetic tests **and passes the three-install test**,
which is exactly the limit of what a separation test can prove.

### 4. The message-name table is off by one, and its last slot is a wild pointer

**Observed.** The lookup at `0x0048a4e0` bounds `gm_type` to 0..97 (`cmp eax,62h`), and only 97
slots of the table at `0x0055b898` resolve to a string; slot 97 holds `0x52454658`, the ASCII bytes
`XFER` — the pointer table has run into the string data it points at, and the caller formats it
with `%s`. Slot 20 is `THIEF_STOLEN_RESOURCEPRISONER_ESCAPE`, 36 characters, eight longer than the
next-longest entry. **Inferred, strongly:** a missing comma in a C literal array, so `table[t]`
names message `t + 1` for every `t ≥ 21`.

**Observed — a cross-check sharing no mechanism with the string evidence.** The CHECKSUM builder
sends `push 5Fh` (type 95) at `0x004b4c4f`; slot 94 is `CHECKSUM` and slot 95 is `AUTOPLAY`. A live
send site puts the name one slot low. A third confirmation: the 16 message types that route to the
checksum-emitting branch are, shift-corrected, all simulation mutations (`MOVE_ARMY`, `END_TURN`,
`BUY_UNIT`, `ORDERS`, `SCRIPTCALLBACK`, …) and uncorrected a nonsense mix.

**A detector I withdrew.** The survey tool first flagged the merge automatically with "entry X ends
with entry Y's whole name". It fired on `BATCH_ORDERS`/`ORDERS`, `SCRIPTCALLBACK`/`ACK` and
`REQUEST_START_GAME`/`START_GAME` — all legitimate — and **missed slot 20**, because the swallowed
name is only a suffix of the merged literal and no slot points at it. Three false positives and a
false negative. The tool now reports the length distribution and the argument lives in prose. A
detector that cannot be stated crisply is worse than none.

### 5. `/testseed=` is a determinism switch, not just a seed

**Observed.** The command line is parsed at `0x004fee60`–`0x004fefb0` by `strstr` plus `atoi`:
`/s=`, `/x=`, `/testseed=`, `/debug`, `/nodebug`, `/nompq`, `/notrimlogs=`, `/cd=`, `/%`.
`/testseed=` stores to `0x005d2ca4`, read at three sites: `0x0048417b` makes the host's game seed
`[0x005d2ca4]` when non-zero and `rand()` otherwise, and `0x0045c722`/`0x0045cdf4` substitute
`[0x005d2ca4]` for `GetTickCount()` in two message-construction paths. **Inferred:** it fixes the
shared seed *and* removes wall-clock variation from two wire paths, which is the right tool for a
reproducible two-machine test.

**Unknown:** what reads config `+0x130`, so what `/notrimlogs=` changes is not established. The name
is suggestive and the name is all I have. Also Unknown: what writes `GS.LOG` and `GSDEBUG.DAT` —
Observed only that they are filename fields of the GameScript VM object (`+0x660` and `+0x55c`, set
at `0x004d2225`/`0x004d221d`), which is a different thing from the network stat log.

## 2026-09-18 — Resync is dead, the transfer path cannot ship scripts, and the engine already marks bad builds

Third pass. Detail in [`docs/multiplayer.md`](multiplayer.md) from "Third pass". The question was
whether resync could move the homelab verdict. It cannot, and the reason is structural rather than
evidential.

### Resync is unreachable

Shift-corrected first, because the slot-versus-type trap has now bitten twice on this branch: slots
62/63 hold `RESYNC_REQUEST`/`RESYNC_START`, so the real types are **63** and **64**.

**Observed.** There are exactly two message-header builders: `0x0048cc70`, which takes the type as
an argument, and `0x0048cc40`, the same function with the type hardcoded to zero. `0x0048cc70` has
**54 call sites and every one passes a literal `push imm8`** — no site passes a computed type. Those
54 sites construct **45 distinct types**, and neither 63 nor 64 is among them.

The routing exists, which is what made them look live: the send fan-out at `0x00489c6c` gives both
policy `0x00489d0d` (send to every computer including the sender, shared with `READY`, `GO`,
`TICK_TIME`), and the 31..65 gate at `0x00489ad5` routes both to the same branch as 28 other types.
**Wired and never built** — the same shape as the disabled post-mortem from the previous entry.
**Inferred:** designed, plumbing survived, trigger never written or removed. **Unknown:** what it
would have carried, since there is no builder whose payload could be read.

Method note: I first tried to settle this from the dispatchers and got two false leads. The
dispatcher at `0x0048bdc0` turned out to be the "emit a CHECKSUM after this message" filter, and
`0x00489c6c` the *send* fan-out, not a receive handler. Enumerating what the binary can *construct*
was the question that actually had an answer, and it is a better question than "where is the
handler" whenever the handler might not exist.

### The transfer path cannot ship an archive

**Observed.** `XFER` (type 31) is built once, at `0x004b7439`. The sender `0x004b7410` resolves a
file-kind tag through `0x004b9570`, then `CreateFileA` / `CreateFileMappingA` / `GetFileSize` /
`MapViewOfFile`, and ships the mapping in **120-byte chunks** (`mov edi,78h` at `0x004b74da`) with a
`chunk * 100 / total` percentage for `XFER_PROGRESS` (`0x004b75a8`–`0x004b75b7`). 120 fits inside
the 257-byte payload cap, so the design is coherent.

**Observed.** Four callers, each passing a literal tag (83, 71, 77, 68). The resolver maps tags
68..83 onto the path builder `0x00505110`, whose eight kinds are exactly
`savegame|multisav/lastsave.lom`, `LOMGSOUT.TMP`, `LOMXFERG.TMP`, `LOMXFERS.TMP`, `LOMXFERU.TMP`,
`APPLOG.TXT`, `LOM_TSPR.TMP`, `LOMD%.4d.TMP` — plus tag 77, which runs the script's
`setscenarionameproc` procedure for `map/<name>` or `multisav/<name>`.

**No MPQ path template exists and no caller can supply an arbitrary filename.** So a resync could
only ever have repaired a *state* divergence, never a `'GS' files` mismatch, and **the homelab
verdict stands unchanged**.

### The engine already marks incompatible games — and this is the useful part

**Observed.** `0x00505f10` formats `[0x00584604]` (script content checksum) and `[0x00584424]`
(executable byte sum) through `cksum=%d,%d` (`0x005736ec`) at `0x00505f30`, script checksum first.
Both transports build that tag while setting the session name — Storm at `0x0046fbf8` inside vtable
slot 2 (`0x0046fbd0`), `CDPlay` at `0x0044a84e`.

**Observed.** `CDPlay` vtable slot 11 (`0x0044a840`, virtual-only, reached by no operator) builds the
local tag and compares it byte by byte against another; on mismatch it formats the session's display
name through `"*%s"` (`0x00556b24`), on match it copies it plainly.

**Inferred: a game in the multiplayer list whose host build does not match yours is shown with a
leading asterisk.** It marks, it does not refuse. That is a shipped, user-facing pre-flight check
over exactly the two values the `preflight` tool reproduces, and it partly answers the previous
entry's Unknown — the *tag* is checked before joining, while the six-value comparison still waits
for play to start. **Unknown:** whether any UI renders the asterisk; I have not seen the list.

`install_checksum::session_tag` now produces the same string and `preflight` prints it. **Only half
is comparable to the game's display:** the executable sum is exact, the script half sums a member set
the engine may not load, so the first number will likely differ. The comparison carries across; the
absolute value does not. I wrote the stronger claim first and corrected it before committing.

### Two unexplored leads

**Observed.** `APPLOG.TXT` is path kind 5, requested from exactly one site, `0x0048447f`. There *is*
an application log path in the shipped build. **Unknown** what is written to it or whether that site
is reachable — a better lead than `GS.LOG` for anyone wanting engine output. `LOMGSOUT.TMP` (kind 1)
is requested from `0x004c9929` and `0x004d7119`, both GameScript modules, likewise **Unknown**.

### The two-machine experiment is now specified

Written out in the doc as steps: byte-identical archives first (checked with `preflight` or by
comparing session tags), an eight-faith `.scn`, a layer-2 domain, `/testseed=12345` on both, two runs
differing only in `COMBAT_MODE`, and a result table mapping each observation to what it would mean.
The engine's own dump is unavailable, so capture is script-side: `getgameseed` and the
`setchecksumproc` procedure's result each turn, plus a screen recording because
`'PlayAnimation Count'` is a divergence class.

### On the test gap flagged last entry

It is covered rather than open, and the doc now says so. The property a guessed hash would have
violated — the two byte sums differ because one sign-extends and the other zero-extends — is
asserted by `the_two_checksums_disagree_on_a_high_byte`, which always runs and was confirmed to fail
when the sign extension is swapped. The install test demonstrates separation only, and that same
swap passes it.

## 2026-09-18 — Corrections from cross-model review

A correctness pass over the three multiplayer entries above, after an independent Codex review of
`3dec492`. Four of its findings were verified and left alone: the two checksum algorithms, the
unreachability of resync, and the off-by-one with its `push 5Fh` cross-check. Three were disputes
and one was a bad test. All four are fixed here; nothing new was investigated.

### Corrected: `lomse.exe` does contain `.mpq` filename strings

I wrote that it contains none, and used that as the stated basis for "`'IMP' files` appears not to
be computed at all". **The premise is false.** There are five, in a packed table:

| Address | String | Pushed at |
| ---: | --- | --- |
| `0x005734d0` | `special.mpq` | `0x004ff4ad` |
| `0x005734dc` | `gs.mpq` | `0x004ff49d`, `0x004ff4e6` |
| `0x005734e4` | `pic.mpq` | `0x004ff48a`, `0x004ff4d3` |
| `0x005734ec` | `imp.mpq` | `0x004ff479`, `0x004ff4be` |
| `0x005734f4` | `sndfx.mpq` | `0x004ff466`, `0x004ff4f7` |

**Observed.** Every push feeds one two-argument helper, `0x004feb10`, called at `0x004ff491`,
`0x004ff4b2`, `0x004ff4c5`, `0x004ff4d8`, `0x004ff4eb` and `0x004ff4fc`, which stores archive
handles at object offsets `+0x114`, `+0x118`, `+0x11c` and `+0x124`. The archives are opened there,
not digested — so the conclusion is unchanged, but its stated reason was wrong.

**How I got it wrong, because that is the reusable part.** I searched with
`strings -n 4 | grep -iE '\.mpq'` and got nothing. `strings` on this PE does not emit that region;
a raw byte search over the file finds all five immediately. **This repository already records the
lesson — a negative result from one spelling of a search is not absence — and I repeated it inside
the same branch that records it.** The check that would have caught it costs one line.

**What the conclusion now rests on**, which never depended on filename strings: the sole `CHECKSUM`
builder (`0x004b4c20`, one caller) has a complete enumerated set of payload stores, and they are
four identified quantities plus two hardcoded zeros plus two slots never written. No archive digest
reaches the message. **Inferred** (unchanged): `'IMP' files` is one of the dead slots.
**Unknown**, and newly so: whether an `imp.mpq` digest is computed anywhere for another purpose.

### Softened: "XFER cannot ship an MPQ" was one step short

**Observed** and unchanged: the tag is a literal at all four `XFER` call sites, and seven of the
eight resolver outcomes are fixed templates, none naming an archive.

**Not proved:** tag 77 resolves through the **script-supplied** `setscenarionameproc` procedure at
`0x004b54c0`, and I established no constraint on its output. Vanilla `gs/network.gs` is
**Observed** to build `"map/"` or `"multisav/"` plus `getmultiscenarioname`, but a mod replaces
`gs.mpq` and therefore that procedure. The doc now says "no path found, tag 77 unchecked" rather
than "cannot". The homelab verdict never depended on it — it rests on resync being unbuildable.

### Reconciled: the first pass contradicted the third

The document was written in three passes and the early ones still described resync as a live
recovery path — in the wire-message table, in the prose after it, and as a homelab benefit. A reader
could find two answers. All three now carry forward-pointing **Corrected** notes and the wire table
lists `RESYNC_*` as names-only. The evidence-label preamble now also states what an `Observed` claim
cites, defines **Corrected**, and says outright that where passes disagree the later one is current.

Also relabelled: "the routing is wired and the messages are never built" was stated as `Observed`
when only its two halves are; the conjunction is `Inferred`.

### Fixed: a test that could not fail

`the_exe_checksum_wraps_at_32_bits_rather_than_saturating_or_panicking` folded `wrapping_add` in the
test body and then called the public function on four bytes, which cannot overflow — so it passed
under a saturating implementation. Replaced with
`both_checksums_wrap_through_the_public_function_rather_than_saturating`, which pushes ~16 MB
through each public function to cross the bound for real: 255 × 16,843,010 = 2³² + 254, and
127 × 16,909,321 = `i32::MAX` + 120.

Mutation-verified both ways. Swapping `wrapping_add` for `saturating_add` in `exe_checksum` fails
with `left: 4294967295, right: 254`; the same swap in `script_checksum` fails at the second
assertion. The old test survived both.

## 2026-09-17 — The GameScript VM slice: the language runs, the engine boundary holds

Issue #5. The lexer already read all 4,692 `.gs` members; the question was whether the *language*
could be executed well enough to tell a script definition from an engine call, and whether the
~2,100-name candidate vocabulary could be partitioned rather than guessed at.

### What the VM can now execute

68 language primitives — stack (`index`, `roll`, `count`, `clear`), arithmetic, comparison and
bitwise logic, `if`/`ifelse`/`repeat`/`for`/`loop`/`exit`/`forall`, array/dictionary/string
construction and access, `known`/`undef`/`load`, and the conversions `cvx`/`cvi`/`cvs`/`type`.

Two features are not PostScript and had to come out of the corpus:

- **Procedure locals.** `PROC /name VALUE replace` attaches a value to a procedure under a name,
  and `PROC dup 0 N dict put` does the same in slot zero. While that procedure runs the name
  resolves to the attached value, which is what makes `/dummy begin`, `/dummy /low known`,
  `/char_array exch get` and `/dummy length` all mean something.
- **Typed dictionary keys.** `gs\spells\weaken.gs` keys a table by number and `standard.gs`'s
  `interpolate` reads it back numerically, so keys are numbers or names, never strings.

And one type rule: `ifelse` accepts an integer condition, because `getflagvalue` feeds it the
result of `bitshift and`.

### The battery, and why the expectations were written first

`examples/gamescript_standard.rs` loads 3.02's `gs\standard.gs` and runs 31 exercises, each
stating the stack it must produce, worked out from the shipped bodies. **Six of the 31 were wrong
on the first run.** Five were my arithmetic on the module's array-backed stack — every entry point
consumes the array reference, which reading the body had not made obvious. One, an `exit` inside a
`for`, was simply mis-traced. All six were my error, not the VM's, and I only know that because the
expectation was committed before the run. A battery that recorded its own output would have been
green and worthless.

Three findings came out of executing rather than reading:

1. `get_if_known`'s header comment lists its operands backwards. The comment says
   `;val dict key`; the `3 1 roll` in the body requires `dictionary key default`. **Corrected.**
2. `dump_flags` does nothing to its operand. After the first iteration its `2 copy` reads the loop
   counter instead of the flags, and the trailing `pop` discards the one index it kept.
3. **GS5R3 ships `min` and `max` swapped.** Vanilla and 3.02 both define
   `/min{2 copy gt{exch pop}{pop}ifelse}`. GS5R3 comments that pair out at lines 68 and 70 and
   redefines them at 73 and 74 with the bodies exchanged, so under GS5R3 `3 7 min` is `7`. Running
   the same battery against GS5R3 reports **four** failures: `min` and `max` for the swap, plus
   `string_cvi` and `char_cvs` stopping on an unknown name because GS5R3 does not ship them. The
   vanilla run reports only that second pair. Transcripts of all three runs are committed as
   `reports/gs/standard-run-*.tsv`.

Nine distinct engine names were reached. Six are in the operator table with entry points; three —
`build_statement`, `sysdlg`, `unitdictxref` — are not engine calls at all but definitions in
`gs\text.gs`, `gs\dlg\sysdlg.gs` and `units\easyunit.gs`. None was guessed at.

### Bare CR, confirmed present and load-bearing

The corpus uses every line ending: in GS5R3's `gs.mpq`, 1,123 members CRLF, **242 bare CR**, 63
bare LF. The lexer terminated comments correctly but counted lines only on LF, so every position
it reported in a Mac-line-ended member was line 1. Fixed, with two tests that were confirmed to
fail when each rule is removed. The `min`/`max` finding above depends directly on reading
`;`-commented lines as dead — the swapped definitions sit two lines below the commented originals.

### The definition scanner was under-counting, and executing is what showed it

The three-token definition window could not see past `dup 0 N dict put bind` or
`/dummy N dict replace bind`, so `standard.gs`'s own `writestring`, `pushonstack`, `popoffstack`
and `onstack?` were filed as names nothing in the corpus defines. The window now admits only
definition-finishing operators past the first three tokens, which keeps `/invoke_spell cvx` out.
3.02's script-definition count moved 10,017 → 10,967 and its unexplained residue 2,951 → 2,004.
**Corrected.**

### The vocabulary, partitioned

`examples/gamescript_vocabulary.rs` writes `reports/gs/vocabulary-<profile>.tsv`. A name is a
native host call exactly when `lomse.exe` registers it and the VM does not implement it, so the
boundary is a lookup. `PRIMITIVE_NAMES` is checked against the dispatch by a test that reads the
match arms back out of the source, because a stale list would silently move names between classes.

Restricted to the broad candidate list issue #5 asked about (3.02: 2,211 names): 55 primitives,
1,414 native host calls, 709 constants, **33 coincidences**. The heuristic was ~98.5% right; it
just could not say which 98.5%.

### Continue or park: continue, for module loading

`--survey` loads each member on its own machine. 406 of 1,681 execute with no engine support.
Of the 1,267 that stop, the first blocking name is **defined in another member for 640 of them**,
an engine constant for 480, a real engine operator for only 124. The largest obstacle to loading
the corpus is loading the corpus.

So: `run` backed by the archive, a *declared* engine-constant table, and a per-module load report
rather than a boot — `START.GS` reaches `dialog` almost immediately. Park the moment the dominant
first-blocker class flips from `defined-in-another-member` to `engine-operator`; past that point
the work is simulation, not loading.

## 2026-09-17 — Cross-review of the VM slice: four inventions and an inverted argument

A cross-model review of the slice above reproduced almost every published number exactly — the 339
steps, the 31-of-31, the nine unknown natives, the survey's 406/1,267/8, all five class counts, the
line-ending counts, the `min`/`max` swap against the raw bytes, and all five mutation results. It
also found ten things, and two of them matter more than the rest.

### The VM was inventing values, which is the one thing issue #5 forbids

Not through a host call — through arithmetic. `1 0 div` returned `inf`, `1 0 mod` and `-4 sqrt`
returned `NaN`, `1e308 1e308 mul` returned `inf`, and all returned `Ok`. PostScript raises
`undefinedresult` for every one. The consequences were worse than the values: `1 0 div 1000000 gt`
answered `true`, and because `compare_numbers` answered `false` for both `gt` and `lt` on a `NaN`,
a single undefined result turned every later comparison into a plausible wrong answer. Arithmetic
that reaches a non-finite result from finite operands now stops, and ordering a `NaN` — still
reachable from a native stub — stops too.

`bitshift` was worse than that: it both panicked and fabricated. `1 -1e300 bitshift` saturated
`f64 as i64` to `i64::MIN` and then panicked on the negation, in a debug build, which is what
`cargo run --example` produces. And `wrapping_shl` masked the shift count, so `1 32 bitshift`
answered `1`. `getflagvalue` is `1 exch bitshift and`, so that was a **fabricated set flag** for
any bit index of 32 or more. It now computes on the 32-bit pattern within `-31..=31` and refuses
anything wider, because a 32-bit x86 `shl` masks the count to five bits while C's `1 << 32` is
undefined, and which one `lomse.exe` does is not established here.

I had written in the module header that this VM never returns an invented value. That claim was
about host calls and I let it stand over the whole machine. **Corrected.**

### Bounded execution was bounded in one dimension out of three

`repeat` never charged a step, so `100000000000 {} repeat` never returned — `for` and `loop` both
charged, and the step-limit test only covered `loop`. Script recursion exhausted the *host* stack:
`/f {f} def f` aborted the process with a Rust stack overflow at roughly twenty thousand frames,
and since `--survey` runs all 1,681 members in one process, one recursive member would have
destroyed every other result in the run. And `100000000000 array` asks for 1.6 TB before a single
step is charged. Steps, call depth and allocation are now all bounded, with the depth figure
*measured* against the 2 MB stack `cargo test` gives a test thread rather than chosen for
roundness — my first attempt at 1,600 frames aborted the suite.

### The continue argument rested on a case fold, and inverting the fold inverts the sentence

`blocker_class` lowercased both sides while the VM resolves names case-sensitively. Nothing in 3.02
defines `GOLD`; `gs\barter.gs` defines `/gold`. So 54 engine-constant names — 75 members by `GOLD`
alone — were filed as "defined in another member", and that produced the 640-against-480 split that
the sentence "the single largest group stops on a name another member defines" was built on.

Case-sensitively it is **564 against 556**, and discounting the 41 members blocked on `userdict` —
the root dictionary the entry-point module builds for itself — it is **523 against 556**, so the two
groups cross. The reviewer's further caveat is the honest one: `engine-constant` is itself a shape
heuristic, so 523-against-556 pits one heuristic against another and the crossover point is not
establishable from this measurement.

**The recommendation stands and the argument for it does not.** What decides Continue is the row no
correction touched: only **124** of 1,267 members stop first on a real engine operator. Both large
groups are cheap; the expensive class is small and stayed small. The park trigger is now stated
against `engine-operator` becoming the largest class, deliberately not against which of the two
cheap classes leads, because that comparison already moved once under correction.

The same fold was in the committed TSVs. `native-host-call` is unaffected (1,383/1,414/1,390 — none
of the 54 is an operator-table entry); the movement is `script-definition` down and
`constant-or-data` up. The `broad-candidate` column still reproduces the old heuristic's fold on
purpose, so the repaired classifier can be compared against a faithful baseline rather than a
half-repaired one.

### `heuristic-false-positive` was the wrong name for that class

`vocabulary.md` asserted it was a false-positive list. Its top rows by use count are real script
definitions the scanner still cannot see — `set_level_modifications` (279 uses, `gs\levlmods.gs`),
`getdungeonstrength` (204, `gs\placedng.gs`) — and 113 of 3.02's 1,893 rows are engine
terrain-sprite dictionary keys such as `cave` (157 uses), `minec` and `crystb`, a table **this
crate already holds** as `map::TERRAIN_SPRITE_TYPES`. Those are now a sixth class,
`engine-dictionary-key`, and the residue is renamed `unclassified-residue` so its name asserts
nothing. The 98.5% broad-candidate figure is unaffected: every row involved is
`broad-candidate = no`.

### Four documentation claims were wrong, three about my own output

- "flags exactly those two exercises and nothing else" — the GS5R3 run reports **four** failures;
  `string_cvi` and `char_cvs` stop because GS5R3 does not ship them. My own parenthetical said so
  two clauses later, contradicting the sentence it was attached to.
- "37 names" — the tool prints **36**, and this log's own 2026-09-16 entry already said 36 under an
  *Observed* label. I miscounted a `grep -c` that also matched the summary line, and introduced a
  contradiction with a prior observation.
- The definition-shape section still described the three-token window and quoted 13,609 GS5R3
  definitions and 2,091→2,151 candidates. Those are the *old* rule's output; the new rule gives
  **14,978** and **2,148**. I changed the rule and the tables and not the section that states them.
- The line-ending figures were overlapping counts presented as a partition — 1,123 + 242 + 63 + 501
  over 1,696 members. Only **49** members are bare-CR-*only*; 193 of the 242 also contain CRLF, for
  which the line counter's absence understates rather than collapses the reported position.
  `--survey` now prints the census so the figures are reproducible instead of asserted.

Smaller corrections: the 21-name static debt includes `outfilename`, which is `standard.gs`'s own
local defined inside `eval`, so the real debt is 20 with 3 unresolved. "Not the host-API gap" was
wrong for `sysdlg`, whose definition is `/sysdlg 50 dialog def` — loading it relocates the
dependency onto the `dialog` operator rather than removing it. Four of the eight "failed some other
way" members are one named modelling gap (`type` reports a procedure as `/arraytype` and `length`
accepts one, but `forall` refuses it and `PROC 0 get` reads the attachment table), which is now
stated rather than reported anonymously. And a procedure local shadowing a primitive is the
PostScript dictionary-stack model working as intended, which is now written down as intent.

### The process note worth keeping

The reviewer could not check whether my six first-run expectation corrections were genuine or
retro-fitted, because the branch was one squashed commit with no run output, and substituted
independent re-derivation instead. The three run transcripts are now committed as
`reports/gs/standard-run-*.tsv`, which is what should have gone in beside the expectations the
first time.

## 2026-09-17 — The same case fold was in the shipped tool, not just the derived table

The previous entry fixed a case-folded definition check in the classifier example and *reported*
that `likely_engine_names` in `src/main.rs` carried the same defect. Reporting it was not enough:
that function is what `--scan-gamescript` prints, so the state I left behind had the derived
artifact correct and the tool a person actually runs still wrong, and the two disagreeing with the
wrong one being the visible one is worse than either alone. **Corrected.**

### What moved

`GameScriptVm::lookup` resolves names case-sensitively. The scanner's definition check folded, so a
lowercase definition suppressed an uppercase call of the same word:

| | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| published candidates, case-sensitive | 2,159 | 2,262 | 2,195 |
| under the old fold | 2,113 | 2,211 | 2,148 |
| recovered | 46 | 51 | 47 |

Every recovered name is an engine constant. The largest are `GOLD` (290 uses in 3.02, hidden by
`gs\barter.gs`'s `/gold`), `FOOD` (268), `CRYSTALS` (267), `WARRIOR` (193), `WIZARD` (180),
`THIEF` (167), `MOVE` (163), `TARGET_ARMY` (146). Surfacing exactly that class is what the filter
exists for, so the fold was suppressing its best output.

The GS5R3 operator-table reconciliation had to be re-measured with it, and I nearly repeated the
error the reviewer caught last time — changing a rule and leaving the section that states its
numbers. It is now 2,195 candidates / 1,444 confirmed operators / 717 SCREAMING_CASE / 34
remainder, against 2,151 / 1,445 / 671 / 35 before the definition-window widening and this fix.
The heuristic's error bar therefore widened from ~0.7% to ~1.5% of candidates — a worse-looking
number that is a more honest one, since the denominator no longer excludes 47 real engine names.

Two code paths now agree on those figures from independent implementations: `--scan-gamescript`'s
`engine-names-recovered-from-the-old-case-fold` and the classifier's
`broad-candidates-recovered-from-the-old-case-fold`. A third cross-check falls out for free —
GS5R3's candidate-restricted `constant-or-data` is 717, the same as `--scan-natives`'s
`unconfirmed-screaming-case`, and the residue differs by exactly the one name the new
`engine-dictionary-key` class pulls out.

### Which fold stayed, and why

Only the *definition* comparison has to agree with the interpreter. The other half of the rule asks
whether a name occurs as an ASCII run anywhere in `lomse.exe`, which is a coincidence filter that
case does not bear on, and it stays folded in both the scanner and the classifier. Keeping the two
halves distinct in the comment matters, because "fix the case fold" applied indiscriminately would
have broken the filter rather than the defect.

The classifier's `broad-candidate` column no longer reproduces the old fold. It did last time on
the grounds that a repaired classifier is only worth comparing against a faithful baseline — which
was right while the scanner was still folding, and is wrong now that it is not. Both sides are
case-sensitive and both print the historical delta, so the baseline is still recoverable without
either of them reproducing a known defect.

### Two things preserved in the artifacts rather than only in a report

**The bounds tests are not all assertions, and the source now says so where they live.** Of the
three new bounds, only allocation has a threshold pinned from both sides by something that fails
cleanly. Deleting `repeat`'s step charge makes its test hang forever — measured, still running at
20 s — and deleting the call-depth check aborts the whole test binary with a stack overflow. Those
are detections by the process dying, not failing assertions, and a reader seeing "229 passed"
should not take the three as equally covered. The doc comment on
`execution_is_bounded_in_steps_call_depth_and_allocation` states that, names which case is which,
and points at the allocation case as the shape to copy where a threshold exists to pin.

**`engine-dictionary-key` is GS5R3-derived**, and `reports/gs/vocabulary.md` now carries that
caveat next to the class instead of only in a handback. `map::TERRAIN_SPRITE_TYPES` was dumped from
a GS5R3 script set, so the 126 GS5R3 rows are measured against their own profile while the 112
vanilla and 113 patch302 rows assume the registry carries the same names across profiles — an
assumption this run does not test. The error is confined to `engine-dictionary-key` and
`unclassified-residue` and cannot reach `native-host-call` or the candidate totals. Re-running the
sprite-type probe per profile would settle it.

## 2026-09-17 — The battery was never in the test suite; three shapes were not definitions

A Codex pass over the same branch found three things the Claude review and my two correction rounds
had all missed. The report was partly contaminated — its closing bullets describe a map-loader
getter at `0x004c6fc0` that has nothing to do with this branch — which is a reason to verify its
claims rather than take them. All three below were verified against the real archives before being
acted on.

### 1. The 31 exercises never ran under `cargo test`. Corrected.

`examples/gamescript_standard.rs` carried 32 exercise records and **zero** `#[test]` attributes, and
the crate had no `tests/` directory. So the battery executed only when a person typed the command,
and every "225 passed" / "229 passed" figure I published about this work was a suite that never
touched one exercise. Pointing `sin` at `f64::cos` would have left it green — and with no CI in
this repo, green everywhere.

This is worse than the provenance question the previous round spent its effort on. Whether the
expectations were written before the first run matters much less than nothing re-checking them
afterwards; I committed run transcripts to address the first and left the second in place.

The table now lives in `src/gamescript_standard.rs` and runs from
`tests/gamescript_standard.rs`. The three tests are `#[ignore]`d because they execute shipped
script that is not in Git, so they report as `ignored` rather than being absent, and
`LOM_GS_PROFILE` asserts the per-profile disagreements instead of tolerating them — `patch302` must
disagree on nothing, `vanilla` on its two missing string helpers, `gs5r3` on those plus the swapped
`min`/`max`. A declared disagreement that stops happening fails too.

**Superseded 2026-09-19.** Taking the lineage from an environment variable made the battery green
only on the profile the operator happened to name, and its `patch302` default made three of the
four installs red out of the box; worse, a *wrong* declaration silently selected the wrong
expectations rather than being caught. The lineage is now **derived from the archive's own
`standard.gs`** and `LOM_GS_PROFILE` is checked against that derivation instead of believed. The
"declared disagreements" are gone too: tolerating a disagreement meant four exercises asserted
nothing at all on GS5R3, so each lineage now carries a real expectation for every exercise. All
three runs report zero failures, on all four installs, with no environment variable.

For the default suite, which cannot reach the corpus, I added
`trigonometry_is_not_self_consistent_under_a_swap` (sin is odd, cos is even; a swap breaks both
identities and both orderings) and
`arithmetic_and_comparison_primitives_are_wired_to_the_right_operations` (non-commutativity catches
swapped operands where `add` and `mul` cannot). **Verified: the `sin`-calls-`cos` mutation now
fails the default `cargo test` *and* the battery.** Before this change it failed neither.

### 2. Three definition-site shapes that are not definitions. Corrected, with the residual measured.

All three confirmed against a local 3.02 `gs.mpq` before any change:

| Shape | Corpus site | Distinct names |
| --- | --- | ---: |
| the name is discarded by the next operator | `fonts\balloon.gs`: `/CopperplateGothicBT-BoldCond pop /gridsize[16 14]def`; `gs\diplo.gs`: `/i undef /majorrace?{4 lt}bind def` | 12 |
| a dictionary *value* read as a key | `<< /a /value /b 1 >>` | 114 |
| a literal nested inside a dictionary | `gs\actvrect.gs`: `/xdict << /left{/x parentrect /x get def} … >>` | 202 |

Fixed by, respectively: treating `pop` and `undef` as consuming the name before them; requiring a
key to sit at an **even** offset from its `<<`; and requiring the `<<` to be the **innermost**
enclosing group rather than merely open somewhere outside.

**A fourth shape is left in, deliberately.** The second `/x` in `/x parentrect /x get def` is a
`get` operand, not a definition, and separating it from the genuine leading `/x` needs the operand
arity of `parentrect` — a native whose arity this project does not have. I tried a general rule
("stop at an intervening literal once real work has been seen") and it rejects the true positive as
readily as the false one, so it is not a fix, it is a trade. The test records the over-count as an
expectation of 2 rather than pretending it is 1.

**The damage, bounded by measurement rather than guess.** Of the 12,979 names the pre-fix rule
admitted on 3.02, **83** (0.64%) had no sound definition site anywhere in the corpus. Fixing the
three shapes reclassified **12** of them; the rest sit in the residual above. And the question as
asked — how many of the newly-admitted names are of these shapes — is **6 of 1,131**, so the
definition-window widening was itself ~99.5% sound. (The 950 figure in the prompt came from my own
earlier case-folded class counts, a different quantity; measured directly, the widening admits
1,131 names.)

Twelve names changed class in 3.02: eleven dictionary values (`button2_t`, `crystalsvaluestring`,
`up_button_x`, …) moved to `unclassified-residue`, and `exec` moved from `script-definition` to
`language-primitive` because its only "definition" was a false site. Definition totals moved
12,979 → 12,922 (3.02) and 14,978 → 14,917 (GS5R3). **No VM behaviour changed**: 3.02's survey is
still 406 loaded / 1,267 stopped / 8 other and 564/556/124/23.

### 3. String and name dictionary keys aliased. Corrected.

`as_dictionary_key` mapped both to `DictKey::Name`, so `<< /a 1 "a" 2 >> /a get` answered `2` — a
**silent overwrite**, the exact failure class this VM is supposed to refuse. My own documentation
already said keys are "numbers or names, never strings", so the code contradicted the doc. There is
direct corpus evidence for name keys and for numeric keys and none for string keys, so a string key
now stops. If a shipped member uses one, it will say so rather than quietly reading a neighbour.
3.02's survey is unchanged by it, so no 3.02 member does.

### One finding I am not acting on, and why

Codex called `primitive_names_match_the_dispatch` circular because it extracts quoted labels rather
than behaviour. It is not circular for its stated purpose — it pins `PRIMITIVE_NAMES` to the
dispatch arms and fails when either side moves, which is what keeps the vocabulary classification's
"native call = engine lists it and the VM does not implement it" boundary honest. What it cannot do
is detect a *wrong implementation* behind a right name, and that gap is now covered by the two new
primitive-wiring tests rather than by widening this one's claim.

## 2026-09-18 — Savegame container decoded

### Scope

A native `.sav` decoder (`spikes/asset-viewer/src/save.rs`), a corpus survey
(`examples/save_survey.rs`) and `docs/save-format.md`. Corpus: every savegame in the three installs
on this machine — 14 files, **7 distinct game states**.

### Container

- **Observed in a local binary:** a save is a bare concatenation of nine sections,
  `tag[8] ++ payload`, with **no length or count word between a tag and its payload**. The `u32`
  after a tag is the first field of that section and means something different in each one.
- **Observed in a local binary:** the loader at `0x0048322A` is a **tag-dispatch loop**, not a fixed
  sequence. It `memcmp`s **seven** bytes and jumps through the 9-entry table at `0x00483970`.
  Section order in the file is therefore meaningless, and the parser and its tests treat it so.
- Because there is no length word, a section cannot be skipped without decoding it, and two of the
  nine cannot be decoded. The parser is a **tag locator**, not a reimplementation of the reader —
  guarded by a census that requires exactly nine tags each occurring exactly once. **9/9 in all 14
  files.**

### Sections

Fully decoded: `LS_VER_`, `LS_MULT`, `LS_MAP_`, `LS_USER`, `LS_GAME`, `LS_REGN` header and grid,
`LS_PLR_` tail, `LS_ALRM` header. Carried verbatim: `LS_SPR_` records, `LS_PLR_` records, `LS_REGN`
tail, `LS_ALRM` records.

Fifteen invariants hold on **14/14 files**, including `LS_USER == 8 × 784`, `LS_MULT == 4+164+576`,
the `LS_MAP_` 196,628 accounting, `LS_GAME`'s unexplained `N − live_count == 71`, and the `-1`
terminator at `LS_PLR_`'s `payload_end − 36`.

### Three corrections

**`LS_ALRM`'s turn is at header index 2, not index 1.** The header is eight words,
`[0][A][turn][15][1][1000001 − turn][0][16]`, exact in all 14 files. The earlier index-1 reading and
its "constant 1,000,000" are both **Corrected**. The reason it survived is the reusable part:
`quickstart` is at **turn 1**, and the value `1` sits at three separate indexes of its header, so
that file agrees with several readings at once and can single out none of them. *A fixture shaped
like the corpus cannot fail on what the corpus hides.* The correction makes the turn reading
stronger, not weaker — it now appears three times per file and all three agree everywhere.

**The `LS_MAP_` span is not `128*128*12 + 16`.** That was a **false arithmetic fit**, and an
instructive one: the bulk term is *exact*, because `8 + 4` really is 12 bytes of per-cell data, so
regrouping the two arrays into one reproduces 196,608 precisely. Only the scaffolding differs
(`+16` against the true `+20`). The lesson is that an accounting check comparing only a sum cannot
fail on a regrouping of its terms; the parser now checks the structure as well, asserting the stored
plane count equals the cell count, which no regrouping can satisfy.

**There is no first-hero name at a fixed `LS_SPR_` offset.** **Refuted:** the word at payload `+0x4c`
varies with no pattern, and in `experience.sav` and `lastsave.lom` the bytes where a name was
reported are **all zero**. Variable-length records have no fixed offsets.

### The name-padding leak

- **Observed in a local binary:** the engine `strcpy`s a lord name into a 32-byte field from
  `[player_i + 0x50AC]` with **no preceding `memset`**, so every save writes whatever the buffer
  held.
- `lastsave.lom` and `Merlin I` have different md5s and are **the same game state**: eight of nine
  sections byte-identical, and all **356** differing bytes strictly past a name's NUL — checked byte
  by byte, 356 past the terminator and 0 elsewhere. The leaked bytes are Win32 stack and heap
  pointers.
- Consequences: a `.sav` is **not** a pure function of game state; any save-diffing tool must mask
  the padding or report identical states as different; saves carry fragments of process memory.
- This nearly produced a wrong conclusion. The md5 difference is real evidence and the inference
  from it — "two independent player saves, so invariants holding on both are strong" — was false.
  Recorded in the doc as a retraction rather than quietly dropped.

### Corpus limits worth carrying forward

- 14 files, **7 states**. The six demo saves are byte-identical across all three installs and may
  share a generator; only **one** state is genuine play.
- **Every grid is 128×128**, so `y*width + x` is inherited from `docs/map-format.md` and **not**
  confirmed here. Tests use a non-square 96×64 fixture for both the map and the region grid.
- **Only one file is version 108 and it is also the only turn-1 file**, so version and game-age are
  perfectly confounded. This is what blocks `LS_PLR_`'s version-111 record size: the version-108
  layout (`9 × 6223`) is Observed, generalising it failed on four of six files, and no `record_size`
  field is offered because offering one would be claiming it.
- **All three installs' `Multisav/` directories are empty.** The multiplayer save path is entirely
  unexercised, which is exactly where `LS_MULT`'s setup block and slots 8..15 would be.

### Testing

31 mutations applied by script; **30 caught, 0 survivors**, 1 rejected by the compiler. The first run
left five survivors: **three were tests phrased in terms of the constant they were testing** — the
countdown fixture built its value from `COUNTDOWN_BASE`, the version-gate assertion compared against
`MULTIPLAYER_SLOTS_MIN_VERSION`, and the stride search never looked past the section length. The
other two were no-op mutations that could not change behaviour and were replaced rather than counted.

One always-true "invariant" was removed rather than kept green: `LS_SPR_`'s no-fixed-stride property
is a property of the **corpus**, not of any one file — individual files do admit a dividing header
size, `combat.sav` exactly one (717). The survey now intersects across files and measures it once.

## 2026-09-18 — Savegame decoder, corrections from cross-review

A Claude/Codex cross-review of the entry above found six real defects. All are fixed; the ones that
change a previously reported *number* are restated here rather than quietly edited.

### A decode path that killed the process

The survey **panicked** on a save with an empty `LS_USER`. Zero is a multiple of 784, so the
divisibility test accepted it, `records` came back empty, and `records[0]` aborted the process. A
798-byte proof-of-concept with nine otherwise-valid sections reproduced it.

Fixed at the parse, not at the caller: `LS_USER` is **exactly** eight records, which is what the
writer emits unconditionally, so zero is not something the engine can produce. This repository had
already shipped one decode panic that killed a process; a second one in the same style is the thing
to design against, not to patch downstream.

### The mutation numbers were wrong: 31/0 restated as 43/43/0

The previous entry reported "31 mutations, 30 caught, 0 survivors". **That was true of the mutations
chosen and not of the behaviour.** The review mutated `UserSection::RECORD_COUNT` 8 -> 7 and it
survived all 36 tests. Two more survived identically: `PlayerSection::SENTINEL`, and the visibility
level `63` **in the `63 -> 62` direction only**.

All three were the same defect, and it is the one this repo keeps re-learning in new clothing: **a
fixture generated from the constant it is testing moves with that constant.** Both sides of the
assertion shift together and the mutation is invisible. I had reported *fixing* exactly this defect
three times in the previous round and then left three more instances of it in place.

Two things worth carrying forward:

- **Direction matters.** `63 -> 64` was caught, because a separate test plants 64 and asserts
  rejection. Only `63 -> 62` survived. A mutation set trying one direction per constant reports a
  clean sweep it has not earned.
- **The corpus survey caught what the unit tests did not** — `7 * 784 != 6272`. The layer that
  failed was the unit tests; the layer that would have caught it was the real-data check. Worth
  recording rather than hiding behind the fix.

New figure: **43 mutations, 43 caught, 0 survivors, 0 unapplied**, including four aimed at the
structural checks specifically. One of those survived until a test was added that runs the
container-level checks on a file the full parse **refuses** — which is the only case those checks
exist for.

### Structural requirements separated from corpus regularities

The survey previously failed the run on `N - live_count != 71`, or on a visibility level outside
`{0, 63, 128}`. Those are **unexplained empirical regularities over seven game states, not format
requirements**. A real save breaking one is a **discovery**, and reporting it as a malformed file
trains a reader to ignore the signal worth acting on. Now two classes: structural checks fail the
run, regularities are printed with their measured values and do not.

The structural checks were also **tautological**: four of the fifteen restated what `parse` had
already enforced, so they could never report a failure. They are now re-derived from the raw bytes,
hang off the container rather than off `SaveFile`, and **run even when parsing fails** — which is
what lets them name the offending section on a file the parser refuses. `LS_REGN`'s "accounting"
check was dropped outright: the tail is *defined* as the leftover bytes, so the equation was true by
construction.

### Version handling: signed, and threaded into LS_MULT

- **Observed in a local binary:** the engine's version gates are `jl`/`jge`, which are **signed**. A
  stored version of `0xFFFFFFFF` is `-1` to the engine and takes the low path; the unsigned
  comparison shipped previously answered the opposite. Now compared as `i32`.
- `MultiplayerSection::parse` previously required sixteen slot records unconditionally while the
  doc said versions below 99 omit the block, so **a genuine pre-99 save could not parse.** The
  version is now threaded in and `slots` is `Option`. The test that supposedly covered v0/v50 was
  ineffective because its fixture emitted the block anyway — a fixture shaped like the corpus.
- **Recorded as a gap:** that path is implemented from the disassembly and is **unexercised by any
  sample.** The corpus has two versions, 108 and 111, and both store the block.

### Two claims narrowed

**Order-independence was overstated.** Tag dispatch establishes that the reader *structurally*
accepts any order. It does not establish semantic order-independence: the handlers share the
singleton at `0x005AA12C`, and `LS_MULT`'s handler reads the version `LS_VER_`'s handler writes. Now
recorded as three separate claims with the third marked **not established**.

**The `LS_SPR_` stride search is bounded and now says so.** It establishes *no common candidate with
a header of at most 1024 bytes*, not "no fixed stride exists". The disassembly's virtual dispatch and
the embedded length-prefixed strings are the primary evidence; the arithmetic corroborates. Per the
standing rule that a negative needs a counter, the bound **is** the counter and is printed with the
result.

### Denominator inflation

The previous entry said 14 files. Surveying all three installs gives **20 files — carrying the same
7 states.** Six of those seven are authored demo scenarios that may share a generator; **exactly one
is genuine play.** A "20/20" line that reads as twenty independent confirmations is the inflated
denominator, so the headline now leads with the state count and says plainly that the file count is
duplication.

Also corrected: "six sections decode completely" contradicted the document's own table. It is
**five** fully decoded and **four** partial.

### The tag census is a sanity bound, not integrity checking

Documented rather than fixed. An exact count of one does not prove the located occurrence is the
real tag — a tag sequence hidden in the opaque `LS_REGN` tail plus a corrupted real tag still counts
one each. And a valid save whose payload happens to contain a seven-byte tag sequence is rejected.
The guard is right (it correctly refused a `.DS_Store`), but it bounds accidental misparsing; the
format has no checksum.

### The distinct-state grouping was incomplete

It omitted map dimensions, bpc, both trailer words, the plane count, four of `LS_GAME`'s head
fields, the player lord codes and the region dimensions — so two saves differing only in
`game.unknown_4` collapsed into one state, in the function whose whole job is telling states apart.
Now covers every decoded field, and groups by **comparing normalized bytes** rather than a 64-bit
non-cryptographic hash, since the group count is a published number.

### A test that did not test its own name

`a_matching_byte_total_does_not_make_the_map_accounting_right` only lowered the plane count, which
breaks the byte total as well — so a mutant implementing `plane_covers_every_cell` as the
total-accounting check passed it. The real case is now constructed: a **96x32 map with a 12,288-word
plane occupies exactly the same bytes as a 96x64 map with a 6,144-word plane**, because
`8*3072 + 4*12288 == 8*6144 + 4*6144`. The total accounts, and the plane covers four times the
cells.

## 2026-09-18 — Deterministic MPQ repack and its shape check

Closes the last open Phase 1 item: a repeatable repack command with a byte-shape check that runs
before anything could be installed. Details and the full "what this does not guarantee" list are in
[deterministic MPQ repack](repack.md).

### What was built

`lom-mpq` gained three verbs — `manifest`, `repack`, `create` — and the shape check went into
`tools/mpq_shape.py`, deliberately separate from the writer so it can be tested against archives
that do not exist. `scripts/repack-archive.sh` orders them: manifest the source, repack, manifest the
output, check, and refuse. A refused archive is deleted, and the command will not write anywhere
under `~/Applications` at all. `--install` refuses; installation is Phase 3.

### Determinism, measured rather than assumed

- **Observed:** repacking GS5R3 `gs.mpq` six times produced six identical SHA-256s, and PIC5R3
  `pic.mpq` five times likewise. The output also survived a changed replacement-file mtime, a changed
  source path, a changed `umask`, `TZ` and `LC_ALL`, and reversed `--replace` argument order.
- **Observed:** this holds because the archive is produced by copying the source file and replacing
  members within it. Rebuilding from an extracted tree is not possible for this corpus: 409 PIC5R3
  members have no name, and a member's name is part of its encryption key.
- **Observed:** `SFileCompactArchive` fails with `ERROR_UNKNOWN_FILE_NAMES` (10007) on PIC5R3
  `pic.mpq`. Compaction is therefore off by default; it is available as `--compact` and is itself
  deterministic, but it cannot run on any archive with unnamed members, which includes vanilla
  `gs.mpq` with its 372.
- **Observed:** StormLib regenerates `(listfile)` on every write (48,469 → 48,457 bytes on GS5R3
  `gs.mpq`), so byte-preservation of that one member is impossible. It is the only exemption in the
  check, and it is safe only because every member name is verified individually.
- **Not determined:** whether the engine executes a repacked archive. The only engine acceptance
  evidence remains the attended 2026-09-16 round trip. Shape preserved is not playable.

### The 1,406-versus-1,405 entry gap was misattributed

- **Corrected:** [MPQ inventory](mpq-inventory.md) blamed the case-insensitive macOS filesystem. The
  two `portrait\AIpotM.lbm` entry names are byte-identical, so the loss happens on any filesystem.
- **Observed:** the two entries hold different content — `9dc00e94…` at block 1108, `884f4bb9…` at
  block 1144. Reading members by StormLib's `File%08u.xxx` pseudo-name addresses the block table
  directly and sees both. That addressing was validated against name-based extraction on all 1,700
  GS5R3 `gs.mpq` members: 1,700 matched, 0 mismatched.
- **Observed:** an MPQ cannot hold two members whose names differ only in case. The name hash is
  case-insensitive: adding `A.txt` then `a.txt` leaves one member under the first name holding the
  second content.
- **Observed:** StormLib 9.40 returns success for a zero-length read, contradicting a defensive
  comment written here earlier the same day. The guard stays, but as defence, not as fact.

### Two defects the mutation sweep found

The first was in the sweep itself. A mutation that keeps a file's byte size and is reverted inside
the same second is served from a stale `__pycache__` entry, so the test run never sees it and the
mutation is scored "caught". Under that bug the first sweep reported 44 of 52 caught; with
`PYTHONDONTWRITEBYTECODE=1` and the caches cleared between runs, three of those results changed. A
mutation harness that cannot prove the mutation ran is measuring nothing.

The second was in the shape check. Case-folded rename detection paired the missing and added halves
while emitting findings, which only worked when the missing half sorted first — a property of the
example names, not of the data. The mirrored fixture (`portrait\aipotm.lbm` renamed *up* to
`Portrait\AIpotM.lbm`) reported both halves, and the same one-direction fixture had also let a
"case-fold map keyed on exact names" mutant survive. Partners are now collected before anything is
emitted, and both directions are tested.

Final sweep: **54 mutations, 49 caught, 5 survivors**, each named in the handoff — two equivalent
mutants (an `MPQ_FILE_EXISTS` bit that is already set as `MPQ_FILE_REPLACEEXISTING`; the zero-length
read guard), one that needs two members sharing a name (no fixture can build one), and two on the
determinism-failure branch, which no fixture can reach because no fixture produces nondeterministic
output.

## 2026-09-18 — Gameplay symbol/index database (Phase 2 close-out)

Built a semantic symbol/index database over the GameScript corpus of all three profiles, plus the
searchable reference that was Phase 2's stated deliverable. Full writeup:
[gameplay reference](gameplay-reference.md). Generator:
`spikes/asset-viewer/examples/gameplay_symbols.rs` over `src/gameplay_symbols.rs`; committed output
in `reports/gameplay/`; query verbs `--gameplay-symbol` and `--gameplay-symbols-like`.

### Classification

- **Observed:** three of the six kinds are declared by a named engine operator, not by a directory.
  `/bolt_fire "gs/spells/FIRE/bolt_fire.gs" define_spell def` carries the symbol name, the kind and
  the defining member in one token triple. Units use a delimited block —
  `begin_unit_definition` … `end_unit_definition` — bound by the `/name exch def` that follows it,
  and **that binding, not the filename, is the symbol's name**.
- **Observed:** 854 symbols in vanilla, 854 in 3.02, 1,012 in GS5R3; 1,535 distinct (kind, name)
  pairs across all three.
- **Inferred:** encounters are catalog entries. Their kind comes from the catalog's identity rather
  than from any operator, and they have no script-level bound name, so the member path below
  `gs/dungeons/` is the identity.
- Every record carries an evidence class in the row itself, because the six kinds do not share one.

### Accuracy and completeness, measured as two numbers

A second extractor was written in Python over the raw extracted bytes with its own comment stripper,
sharing no code with the Rust tokenizer. Over the four well-served kinds in all three profiles:
**2,660 symbol identities agreed, 0 were only in the database, 3 were only in the independent
extractor.** Accuracy 100.00%; completeness 100.00% for spells, artifacts and encounters and 99.4%
for units. The three residuals are the same name in each profile, `lastunittype`, and are a **false
positive of the validator** — `units\easyunit.gs` writes it as a procedure local, not a unit.

### What the instrument could not reach, counted rather than assumed

Each figure below is from a **raw byte scan**, which shares no mechanism with the tokenizer.

- **Observed:** vanilla excludes 373 archive entries by extension because its listfile cannot name
  them, and 12 of those contain `begin_unit_definition`. Admitting them on the byte marker recovered
  the unit `boat`, which no named member defines. 3.02 and GS5R3 have zero such entries.
- **Observed:** GS5R3's `gs\dungeons.gs` is a **stale catalog** — 266 of its 312 encounter paths no
  longer exist in its own archive, because GS5R3 moved the tree into per-faith subdirectories and
  catalogs them from `gs\DUNGEONS5.gs`. Reading one hardcoded catalog found 46 of 314 encounters and
  reported the other 268 as missing dependencies. Scanning every member for `run` edges fixed it.
- **Observed:** naming an encounter by basename merges 59 of vanilla's 306 into 247. `earth/encounter11`
  and `hidden/encounter11` are different encounters. Found by the collision report, not by review.
- **Observed:** 18 symbol-name collisions in vanilla and 3.02, 1 in GS5R3, all genuine corpus facts:
  `units\gate.gs` binds three units in one member, and eight members bind `gate` in vanilla.
  `units\licr3old.gs`, `units\holdchwmi.gs` and GS5R3's `units\test.gs` are superseded copies binding
  live names.

### 3.02 changes no gameplay record at all

- **Observed:** all 1,535 symbols have a byte-identical defining member in vanilla and 3.02. This is
  not a broken comparison: 3.02 does modify 14 members — `START.GS`, `gs\standard.gs`,
  `gs\textdict.gs`, `gs\buttons.gs`, `gs\hotkey.gs`, `gs\makearmy.gs`, `gs\modeinfo.gs` and six
  `gs\Dlg\*` — and **none of them defines a unit, spell, artifact or encounter**. At the level of
  gameplay data, 3.02 is an interface, hotkey, text and standard-library patch.

### GS5R3, and why the diff's own headline is misleading

- **Observed:** gs5r3 vs vanilla is 26 identical, 2 formatting-only, 303 token-level, 523 only in
  vanilla/3.02 and 681 only in GS5R3.
- **Corrected reading:** those last two rows are *not* 1,204 additions and removals. GS5R3 renamed
  its spell and artifact sets wholesale — vanilla's `lightning_spll` in `gs/spells/lightning.gs` is
  GS5R3's `bolt_air` in `gs/spells/AIR/bolt_air.gs` — so a name-keyed diff reports each rename as one
  removal plus one addition. Matching on the **token fingerprint of the defining member** recovers
  **294 unedited renames**. That is a lower bound and the largest known gap: a symbol renamed *and*
  edited has no matching fingerprint, which is exactly the renamed-spell case, and how many of the
  523/681 are edited renames is **not established**.

### A corpus-observed maximum is not an engine-enforced bound, and the corpus proves it

- **Refuted:** that unit magic resistances are capped at 100. In vanilla and 3.02 all eight
  resistances max at exactly 100 across 88 units, which looks precisely like a hard cap. **GS5R3
  reaches 125 on `air`/`life`/`water` and 150 on `earth`** — and the three installs differ only in
  `gs.mpq`, with a byte-identical `lomse.exe`. The same engine accepts 150, so 100 is a design
  convention of the vanilla data.
- **Unknown:** every other candidate limit. `armor` ≤ 99, spell `research_cost` and `range` ≤ 256,
  spell `level` ∈ 0…9, `military_units` ∈ 1…3 are all corpus-observed maxima only. An engine-enforced
  bound would have to be shown as a comparison and a clamp in `operator_bodies.rs`, and no such
  search was run. **No limit in this work is claimed as engine-enforced.**
- The reports name the column `modal-value`, not `default`: the mode is the most common written
  value, and a true default is what the engine uses when a record omits the field — not observable
  from scripts.

### Scope boundary

Well served: **units, spells, artifacts, encounters** — 1,515 of 1,535 symbols. Thin, and stated as
findings rather than gaps:

- **Factions (8 symbols).** Nothing in any profile defines `FIRE`; scripts only select on it. The
  eight faiths are recovered from the corpus's own `/faith` values. **GameScript has no faction
  record** — everything a faith is lives in `lomse.exe` or is spread across the records naming it.
- **Buildings (12 symbols, 3 fields).** No record file and no registrar. Per-level data reaches the
  engine as positional arguments to `define_building_levels`, one of whose "procedure" operands is
  not braced (`buildingdict /strongholdbeginturn get`), so a strict positional walk misaligns. Only
  the type name, level bounds and four-character code are taken; the rest is **refused rather than
  recorded wrongly**. Upgrade costs and requirements need a stack model, which is VM work.

### Two pre-existing defects found on the way — reported, not fixed

*(The first of these was fixed on 2026-09-18; see [the entry below](#2026-09-18--inf-is-a-unit-code-not-an-infinity-the-number-predicate-now-models-the-language).)*

- **`gamescript.rs` lexes the shipped unit code `INF` as floating-point infinity.** Token
  classification is `name.parse::<f64>().is_ok()`, and Rust's `f64::from_str` accepts `inf`,
  `infinity` and `nan` case-insensitively. Eight units per profile are affected and a naive range
  over `code` reads `inf … inf`. This module guards locally — a non-finite number keeps its text but
  is never summarised — and the lexer is left alone because its token counts are load-bearing
  elsewhere. Anything reading `TokenKind::Number` has the same exposure. Evidence class: Observed.
- **`tools/gs_syntax.py` terminates a `;` comment at `\n` only** *(fixed on 2026-09-18, with three
  sibling divergences found beside it; see [the entry below](#2026-09-18--four-lexer-divergences-closed-and-what-the-first-one-hid))*,
  but bare CR is a line ending in
  this corpus and **25 GS5R3 members contain a comment and no LF at all**. In those the first comment
  swallows the file: `gs\dungeons\water\wacave.gs` is 5,347 bytes and normalises to **six tokens**.
  It feeds `compare_trees.py`'s token hash, so the "Layout/comments only" column in
  `reports/gs/summary.md` is unreliable for those members. This is the exact failure
  [gamescript-format.md](gamescript-format.md#line-endings-bare-cr-is-a-line-ending-here) already
  warns about, surviving in a tool the warning did not reach. The profile diff here is computed from
  the Rust lexer for that reason. Evidence class: Observed.

### Tests

27 unit tests and 5 corpus-gated integration tests (passing on all three profiles). Fixtures probe
shapes the corpus does **not** contain — a repeated top-level key, a value of two numbers, an
unclosed procedure, a decoy `/decoy bind def` before the real unit binding — because a suite built
only from corpus-shaped input cannot fail on what the corpus never does. No integration test compares
a count to a constant; one fails if the registrar rule and the directory rule ever stop disagreeing,
which is what keeps the classification falsifiable.

**Mutation run: 31 mutations, 31 caught, 0 survivors**, constants mutated in both directions. Seven
survivors from the first run were each closed with a test rather than argued away. The mutation
harness itself was corrected mid-run: it labelled every catch a "compile error" because it matched
`"error: "`, which is what cargo prints for an ordinary assertion failure.

## 2026-09-18 — Gameplay reference: searching by the name a person actually knows

Follow-up to the entry above, after review found the deliverable's search unusable by anyone who did
not already know a symbol's internal code. `--gameplay-symbols-like 'Windrider*'` matched nothing
while the unit it names sat in the index as `aicav` with the display name `Windriders`.

### The search now matches both names and says which one hit

- `--gameplay-symbols-like` matches the code **and** the display name, case-insensitively, and every
  hit carries a `matched` column of `code`, `display-name` or `code+display-name`.
- `--gameplay-symbol` accepts either. An **exact code match wins outright**, so a symbol cannot be
  made ambiguous by another merely being *called* that code.
- The two kinds of multiplicity are handled differently, because they mean different things.
  Several symbols sharing a **code** are all the answer — `potion_health` is registered as both an
  artifact and a spell — and all print in full. Several sharing a **display name** are a question:
  16 spells are labelled "Dispel Magic", so the query lists the candidate codes and prints no
  record, rather than burying the ambiguity under 16 blocks of detail.

### The review's premise was wrong, and the correction is the useful part

- **Corrected:** the claim that all 1,535 symbols have a display name. At the time, **602 had none**
  — 524 encounters, 51 artifacts, 12 buildings, 8 factions, 7 units. A display-name-only search
  would have missed 39% of the database. Two sources were added, and coverage is now **1,439 of
  1,535**: `declared-name` 933, `encounter-key` 450, `text-table` 56, none 96. Every row carries a
  `display-source` column, and every listing prints how many rows had no display name to match, so a
  thin result is explainable rather than mysterious.
- The remaining 96 are 74 encounters whose name is computed at runtime, 12 buildings, 8 factions and
  2 artifacts — kinds with no human-facing label in GameScript at all.

### A field reader defect of my own, found while closing the gap

- **Corrected:** the field scanner stopped at any literal name appearing before a statement's
  terminating `def`. That guard exists because `/invoke_spell cvx` defers a *native call*, and a
  scanner running past it would swallow the rest of the member. But it also fired on the corpus's
  text-table idiom, `/name textdict /T_artifact_name_adventsword get def`, where the inner literal
  is a dictionary key about to be read. Two failures at once: the record **lost its `name` and
  `description`**, and the *key* was filed as a field of its own valued `get`.
- **Observed:** the idiom is not rare — **246 definitions in vanilla and 3.02, 416 in GS5R3**,
  including 56 `name` and 48 `description` in vanilla. All of them use `get` and nothing else.
- The scanner now passes a literal followed by a key-consuming operator, and that list is `["get"]`
  alone. Admitting `known`, `load` or `undef` on plausibility would admit shapes the corpus does not
  contain, and every name admitted there is a name the deferred-call guard stops protecting. Two
  corpus-gated tests hold both halves: no looked-up key may become a field, and `invoke_spell` may
  never become one either.
- Resolving the recovered `/name` values against the corpus's text tables supplies the 56
  `text-table` display names. Keys two members define **differently** are dropped rather than
  resolved to whichever was read last — 230 to 243 per profile — so no symbol is named on the
  strength of archive order.

### A near miss worth recording

The generator wrote `symbols.tsv`'s header from a **second copy** of the column list. A column added
to the rows and to the reader but not to that string produced a file whose every value sat under the
previous column's name — the exact failure `the_index_is_parsed_against_its_declared_columns` was
written for, and it caught it on the next run. The header is now emitted from `INDEX_COLUMNS`, the
same constant the reader checks against, so the two cannot drift again.

### Gates

`cargo test` 373 + 61 + 14 Rust and 219 Python, clippy clean, 7 corpus-gated tests passing on all
three profiles. **Mutation run: 49 mutations, 49 caught, 0 survivors**, and every mutation compiled
and ran — a mutation that fails to build is not a caught mutation, so the harness now distinguishes
a rustc diagnostic from `error: test failed`. The 18 new mutations cover the search (code-only,
display-name-only, each `MatchedField` swapped for each other, the `-` sentinel made searchable,
both case folds removed) and the key-lookup fix (exception removed, any operator accepted, the
`get` requirement dropped).

## 2026-09-18 — `INF` is a unit code, not an infinity: the number predicate now models the language

The lexer in `spikes/asset-viewer/src/gamescript.rs` classified a bare word as a number with
`name.parse::<f64>().is_ok()`. Rust's `f64::from_str` accepts `inf`, `infinity` and `nan`
case-insensitively with an optional sign, and the shipped infantry unit code is the bare word
`INF`, so every infantry reference in the corpus lexed as floating-point infinity. It is replaced
by `gamescript::is_number_token`, which accepts PostScript's integer and real forms and nothing
else. Evidence class: Corrected.

**What the corpus actually contains.** Surveyed over all three profiles' `gs.mpq`, counting every
bare word (evidence class: Observed in a local binary):

| form | uses | distinct | example |
| --- | ---: | ---: | --- |
| integer | 302,015 | 1,411 | `90` |
| signed integer | 32,097 | 80 | `-1` |
| real (`d.d`, `.d`) | 6,409 | 242 | `1.75`, `.6` |
| signed real | 549 | 25 | `-.5` |
| exponent (`1e5`) | 0 | 0 | — |
| radix (`16#FF`) | 0 | 0 | — |
| `inf` family | 118 | 2 | `INF`, `inf` |

No token containing `#` occurs in any profile, and no real carries a trailing dot with no fraction
digits. The predicate accepts the exponent and trailing-dot forms anyway, because both are
PostScript reals and both were already accepted by the rule being replaced, so admitting them
reclassifies nothing — they are stated as **unexercised by the corpus** rather than as measured.
The radix form is deliberately **not** accepted: there is no corpus evidence for it and the shipped
engine's handling of it is unverified, so accepting it would reclassify words on nothing.

**Both `INF` and lowercase `inf` are real names.** `inf` stands beside `fit`, `thf`, `mis` and
`cav` in the corpus's own unit lists, and `INF` beside `CAV` in `/unit_code_strings["INF""MIS"
"CAV"…]`. That neighbour relation is what the corpus-gated tests assert against: the unit-code
alphabet is read out of `/unit_code_strings` at test time rather than listed in the test.

**What moved in the published reports.**

| | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| `INF` uses, previously absent from the vocabulary | 32 | 35 | 38 |
| `CAV` uses, for comparison | 32 | 35 | 38 |
| `inf` uses, previously absent | — | 5 | — |
| distinct executable names | 14,080 → 14,081 | 15,174 → 15,176 | 16,855 → 16,856 |
| broad candidates | 2,160 → 2,161 | 2,263 → 2,264 | 2,198 → 2,199 |

`reports/gameplay/fields.tsv` changed 24 rows — the eight infantry units per profile whose `code`
field carried shape `number` with the text `INF` and now carries shape `name` — and the `code` row
of `reports/gameplay/field-ranges.tsv` reads `name:151`/`name:151`/`name:166` where it read
`number:8 name:143` and `number:8 name:158`. The three `reports/gs/standard-run-*.tsv` transcripts
regenerate **byte-identical**, so no VM exercise depended on the defect.

**The other four `parse::<f64>` sites.** `src/gameplay_symbols.rs` filters `is_finite()` after the
parse, which is the right order and now catches only an overflowing literal, since a spelled-out
infinity can no longer arrive as a `Number` token; the filter stays and its comment is corrected.
`src/gamescript_vm.rs`'s `DictKey::to_value` has a silent `unwrap_or(f64::NAN)` that is
**unreachable rather than lenient** — a `DictKey::Number` is only ever built by `number_key_text`,
which formats an `f64`, so no token text reaches it. `compile_sequence`'s parse error is likewise
unreachable for tokens from this lexer, because every form the predicate admits is also accepted by
`f64::from_str`. `src/main.rs`'s `--stub NAME=VALUE` **did** have the same hole and is fixed: it
classifies with `is_number_token` first, so `--stub x=inf` is now the error the help text promises
instead of an infinity on the stub's stack.

## 2026-09-18 — The `value` column was cut mid-token; prose and data now differ

`gameplay_symbols.rs` built every non-aggregate field value with one rule,
`.chars().take(120)`. It cut **414 of 62,211 rows** in `reports/gameplay/fields.tsv` — the maximum
length in the committed file was exactly 120 and the histogram was a cliff, 119:2 then 120:414 —
and it cut them **mid-token with no marker**. The published data therefore contained the flag names
`CAN_USE_R`, `CAN_USE_LE` and `CAN_TRAN`, which the game does not have, and a bare `o` that had
been the operator `or`. A consumer could only detect the cut by noticing a length of exactly 120,
and had to discard the trailing partial token by hand. Evidence class: Corrected.

**The two classes are now bounded differently, because they are different things.**

| Value class | Rule | Longest in the corpus |
| --- | --- | ---: |
| procedure, dictionary, array | unchanged: `<procedure 12 tokens>`, shape and size only | — |
| number, name, expression | emitted **in full** | 2,951 chars / 227 tokens |
| text (a lone string literal) | up to 120 chars, cut on a **word** boundary, then `<truncated, N chars>` | 709 chars |

Emitting a symbolic value whole does not reopen the no-script-text rule the cap was there to serve:
every braced or bracketed aggregate has already been replaced by its shape, so what reaches this
point is flat. Measured across all three profiles, expressions are 3,010 values with a median well
under 20 tokens; the longest is `alt_spells_id` on `chalice_chaos` and `scroll_sages`, a list of
spell names, and exactly **four** expression values contain a string at all, each the asset path
`iface/ordragb.imp`. Prose keeps a bound because a shipped description is game content and these
reports are published. Evidence class: Observed in a local binary.

**Before and after, `reports/gameplay/fields.tsv`:**

| | before | after |
| --- | ---: | ---: |
| rows | 62,211 | 62,211 |
| values cut | 414 | 0 cut, 264 bounded and marked |
| rows changed | — | 412 (264 artifact `description`, 142 unit `flags`, 6 spell and artifact expressions) |
| longest value | 120 | 2,951 |
| values at exactly 120 | 414 | 2, both complete `ring_of_shelter` descriptions |

**The cut was hiding a real cross-profile difference.** `unit:dethf` and `unit:waldf` have `flags`
whose first 120 characters are identical in all three profiles and which diverge after them: GS5R3
adds `CAN_USE_RIGHT_ARTIFACT` to `dethf` and `NO_DEFEND_ANIM` to `waldf`. Both now appear in the
`changed-fields` column of `reports/gameplay/profile-diff.tsv`, which had reported no difference.
That is the concrete cost of a silent truncation: not only was the published value wrong, the
*comparison* built on it was wrong in the direction that hides a mod's changes. Evidence class:
Corrected.

No prose in `docs/` quoted a figure derived from the truncated column; the counts that moved are
the two `profile-diff` rows above and the test counts in
[gameplay-reference.md](gameplay-reference.md#tests).

**The finiteness guard stays, and now has a different job.** With the lexer fixed, `INF` can no
longer arrive as a number token, so the guard's original case is gone. It is not redundant:
a number the predicate correctly accepts can still exceed `f64` — `1e400`, or a 400-digit integer —
and would reach it as infinity through a lexer doing its job. The shipped corpus does not exercise
that: across all three profiles there are **zero** non-finite number tokens, the largest magnitude
is `300000000`, and the longest numeric token is `3.14159265` at ten characters against the 309 it
would take to overflow. So the guard is unexercised rather than dead, and the unit test now drives
it with `1e400` and `-1e400` instead of `INF`, which would no longer reach it. Evidence class:
Observed in a local binary.

## 2026-09-18 — Three savegame sections decoded from the writer, and two readings refuted

`LS_PLR_`, `LS_REGN` and `LS_ALRM` now decode completely. `LS_SPR_` does not, and that is stated as
a scope decision rather than a finding. Full detail in [save-format.md](save-format.md).

**The method was the point, and it was not corpus inference.** The previous pass had taken corpus
arithmetic as far as it goes and said so — "three observed lengths (8,998 / 9,389 / 9,780) and no
known structure" is what five shipped scenarios have left to give. This pass walked the writer at
`0x00482AF0` into the per-record writers it calls, and then used the files only as a **byte
account**: parse with the recovered model and require the cursor to land exactly on the section's
end. That check has nothing to tune, because every length in these sections is either a constant in
the instruction stream or a count the file stores. All three land exactly, in every file, with zero
slack.

Routines walked: `0x004BCE20` / `0x004BCBD0` (player record, writer and reader), `0x0050A360`,
`0x0050B800`, `0x00509FC0`, `0x0049F2A0`, `0x0051C6C0`, `0x004BF0A0`, `0x004C7390`, `0x004C5840`,
the six alarm list writers `0x0040B7D0` .. `0x0040F600` with their element writers, and the shared
string writer `0x004D5F20`.

**Refuted: `LS_ALRM`'s eight-word header.** There is no header. The section is six independent
linked lists, each `u32 count` then that many records. What two passes read as a header is
`count(queue 0) = 0`, `count(queue 1)`, then the first five fields of queue 1's first record — and
the eighth "header word", the constant 16, is the **string length of `monstergenerator`**. The turn
does sit at payload word 2 in every file, *because queue 0 is empty in every file*. This is the same
failure the previous `Corrected` note diagnosed, one level up: that note moved the turn from index 1
to index 2 and kept the frame. Indexing into a payload is not a structure, and a check that asks
"does some word equal the turn" cannot tell a field from a coincidence. Evidence class: Refuted.

**Refuted: `LS_PLR_`'s "record size is not established for version 111" — the premise.** There is no
record size at any version. A record is a tree of counted lists and varies *within* one file;
`combat.sav` holds eight records of eight different lengths, 6,759 to 7,339 bytes. The version-108
file's clean `9 × 6223` is a turn-1 scenario in which every player has an empty queue and sixteen
empty armies, not a stride. Evidence class: Refuted.

**Corrected: version and game-age were not confounded for that reading.** The previous pass said
resolving the 108-versus-111 difference needed a new sample. It needed the reader: eight
`cmp dword [0x5AA12C], n / jl` gates in `0x004BCBD0` at minimum versions 57, 68, 76, 86, 104, 110
and 111. Version 108 clears 104 and not 110 or 111, so it stores 12 bytes less per record — exactly
what the corpus shows. The writer has **no** gates, so a save is readable by its own build and every
later one; the ladder exists only for older files. Evidence class: Corrected.

**Corrected: `[0x0054D0D8]` / `[0x0054D0DC]` is a lock pair, not a transform**, so the asterisk on
"no compression and no encryption" comes off `LS_REGN`. Both take a pointer to `[this+0x1bc]` as
their only argument and neither return is used; and the cross-check that does not share that
mechanism is that the bytes between them decode with no transform applied at all. Evidence class:
Corrected.

**Corrected: the corpus is ten distinct game states across 24 files, not seven across twenty.**
There are four installs on this machine, not three, and `Lords of Magic GS5R3.app` carries four
files no other install has — `combat.lom`, `endturn.lom`, and its own `lastsave.lom` and `Merlin I`,
which do not match the identically-named 3.02 files. Three further states, all genuine play. Worth
recording because it cuts the other way from the usual error: the previous pass was rightly
suspicious of an inflated file count and, guarding against that, stopped looking for more states.
Evidence class: Corrected.

**What resisted.** `LS_SPR_`. Its writer at `0x004F6BC0` is now read — `u32 live_count`, then one
self-identifying record per live object through `call [vtable+0x20]` — which narrows the problem to
ten class writers, each of which will be a tree like `0x004BCE20`'s with its own version ladder.
That was not attempted here. The next instrument is the vtable, not the files. Also still open: the
six-byte `LS_REGN` grid cell, whose fields the writer says nothing about because it emits the whole
grid in one `fwrite`.

**A bounded negative, with what it could not reach.** Alarm queues 3 and 4 are **empty in all 24
files**. Their field schedules are Observed in a local binary and have no corpus corroboration at
all — a wrong reading of either would parse every available save perfectly. Likewise `LS_PLR_`'s
gates at 57, 68, 76, 86 and 104: the corpus holds two versions, so five of the seven gates are
exercised only by synthetic fixtures.

**Mutation sweep: 53 mutations over the new code, every constant in both directions, 53 caught, 0
survivors.** The first run had three survivors and they were all one defect: `NAME` 76→75,
`BLOCK_68` 104→103 and `UNKNOWN_15B4_15B8` 110→109 each survived because no fixture version stood
between the gate and the value below it. All three were caught in the *up* direction. This is the
asymmetry already recorded for the visibility level `63`, walked into again in a new place: **a
version sweep that samples only the gates cannot fail on a gate moved down.** The fix sweeps each
gate *and the version immediately below it*, and asserts the per-step size deltas including the
zeros, since a zero is exactly what a downward gate move destroys.


## 2026-09-18 — Four lexer divergences closed, and what the first one hid

The recorded defect was one line: `tools/gs_syntax.py` ended a `;` comment at `\n` only, though
bare CR is a line ending in this corpus. Fixing it took a line too. **Everything of value in this
entry came from what happened next** — reading `gs_syntax.py` against
`spikes/asset-viewer/src/gamescript.rs` statement by statement instead of stopping at the defect
that had been named.

**The instrument came before the numbers.** A Python port of the Rust lexer's rules was written and
then validated against the real thing: its NUL-joined token digest equals `--gs-facts`'s
`token_sha256` on **4,692 of 4,692** members of the three installed profiles, zero parse-error
disagreements. Only after that was any divergence attributed to `gs_syntax.py`. The port also had
to decode the way the comparison decodes: the Rust lexer uses `from_utf8_lossy`, so the 17 members
carrying a byte above 0x7e that is not valid UTF-8 differ in *token text* while agreeing on every
token *boundary*. Compared naively that is 17 false findings out of 23.

| Divergence | Members changed | Where |
|---|---:|---|
| `;` comment ended at `\n` only | 34 | all GS5R3 (25 with no LF at all) |
| `\` treated as a string escape | 5 | vanilla 1, 3.02 2, GS5R3 2 |
| `/` did not end a name | 5 | vanilla 1, 3.02 1, GS5R3 3 |
| `str.isspace()` wider than `is_ascii_whitespace` | 0 | one member has such a byte, inside a string |
| **after all four** | **0 of 4,692** | |

**The recorded defect was not the worst one.** The comment rule cost `gs\dungeons\water\wacave.gs`
706 of its 712 tokens, which is the figure this log already carried, and every one of its 34 members
is in GS5R3. The string rule is in **vanilla**: `gs\Dlg\lib_dlg.gs` holds the punctuation table
`"@#${}()[]\"`, whose closing quote the escape rule ate, and the member lexed to 3,247 tokens
against its real 2,324. Two pages here had reasoned from "bare CR is a GS5R3 phenomenon" to "this
class of defect cannot reach a mod built against vanilla, which is what `mods/orinf-rebalance` is".
That inference was **Refuted** by a sibling of the defect it was written about. The authority has no
escape rule at all — `read_string` matches on `"` and takes every other byte verbatim, and its own
fixture `"path\to\file"` keeps both backslashes — so the escape branch was never anything but a
Python habit applied to a language that does not have it.

**A divergence with zero corpus reach is still worth closing, and still has to be measured.**
`str.isspace()` accepts ASCII `\x0b` and `\x1c`-`\x1f` as well as `\x85` and `\xa0`; the
authority accepts none of them. Exactly one member of the corpus contains such a byte — GS5R3's
`gs\artifact\_custom\shield_balkoth.gs`, one `\x85` — and it is **inside a string literal**, where
no layout rule looks, so the reach is zero, not one. An earlier draft of the build-pipeline page
said that member "trips it", conflating *containing the byte* with *lexing differently because of
it*, and a test premised on that conflation would have been asserting nothing.

**Two measurements that would have passed while the code was wrong.** A token-count comparison was
tried first for the whitespace case: `/hit_points\x85 13 def` is three tokens under both rules, one
of them with the name split and one with it whole, so the count is blind to exactly the mutation it
was there to catch. It was replaced with a digest comparison that applies only the lossy decoder to
this side and nothing else. Separately, `test_comment_ends_at_the_carriage_return_of_a_crlf_pair`
passed under the LF-only rule it was named for — the LF of a CRLF pair had always terminated the
comment — and is now named `test_crlf_tokenizes_like_lf`, which is what it actually checks. Both are
the same error: a measurement whose name promises more than its mechanism can deliver.

**What is left, stated rather than implied.** `<` and `>` end a name for the authority and do not
here, so `x<<y` is three tokens to one lexer and one to the other. **No corpus member reaches it**,
and closing it needs a decision `gs_syntax.py` cannot make: a single `<` is a parse error to the
authority and this tokenizer has no error channel, as with an unterminated string and an empty
literal name. It is named in [gamescript-format.md](gamescript-format.md#lexer-parity-the-two-tokenizers-and-what-still-separates-them)
and is what the change report's two-lexer check is left watching.

**`reports/gs/summary.md` regenerates byte-identical after all four fixes**, at every step. Token
hashes moved for the affected members in each comparison and **no member changed status**, so the
"layout/comments only" column was never wrong in its *classification* for these members even while
its input was — which is luck, not design, and is why it was checked after each change rather than
once at the end.

**A stale caveat was being stamped into every build.** `tools/mod_build.py` wrote
`engine_acceptance["pic.mpq"] = "Never tested. No rewritten pic.mpq has been put in front of the
engine and the compression choice for one is Inferred."` into `build.json`, deliberately, "so a
build carries its own caveat". Both halves had been refuted on 2026-09-18 by this repository's own
roadmap. It now states the measured scope — one member, replaced not added, a length-preserving
`pbm_patch.py` edit, with the size-changing edit, the added member and the ByteRun1 encoder all
still untested — and `tests/test_mod_pipeline.py` reads `docs/roadmap.md` and fails if the two
drift apart again. The `gs.mpq` entry beside it was checked and is accurate, as is
`mod_validate.py`'s added-member warning; those are the only two places in the repository that
stamp a caveat into an artifact rather than into prose.

**Left alone, deliberately.** `tools/gs_callsites.py` computes line and column with `\n`
arithmetic, which looks like the same defect and is **inert**: it reads members through
`Path.read_text()`, whose universal-newline translation turns every bare CR into `\n` before the
tokenizer sees it. Recorded here so the next reader does not "fix" it and change what those offsets
mean.

### Corrections to the entry above, same day, from two independent reviews

Both reviewers ran over the same diff and found the same two defects, which is the strongest signal
this process produces. Neither could verify a single corpus number, because the archives are not in
this repository — the measurements below remain the only evidence for them.

**A fifth divergence, and every "what is still open" list above was wrong.** `DELIMITERS` in
`gs_syntax.py` was `frozenset("{}[]()")`. The authority's `is_separator` lists no parenthesis: they
are ordinary name bytes, so `/a{ foo(1) }def` is **5 tokens** to `--gs-facts` and was **8** here.
It is not in the same class as the `<`/`>` divergence and filing it beside that one would have been
wrong: `<` needs an error channel this tokenizer does not have, while the parentheses needed
`frozenset("{}[]")` and nothing else. Corpus reach **0**, measured the same way as the rest: 530
members contain a parenthesis and in every one of them it is inside a string or a comment. Both
zero-reach divergences were fixed anyway — a disagreement about a shipped grammar is a defect
whether or not the shipped corpus exercises it, and the case it would break is an edit from
`foo(1)` to `foo (1)` in a mod nobody has written yet, which is the case this pipeline exists for.

**A guard that signed off on the direction it existed to block.** `EngineAcceptanceCaveatTest`
checked that five phrases appeared *somewhere* in the `build.json` caveat. Both reviewers broke it
the same way: rewrite "NOT established: … have none of them been put in front of the engine" as
"ALSO established: … have every one of them", and all three tests still reported `ok`. The
direction it did block — the old "Never tested" — merely *understated* what had been proved; the
direction it let through would have shipped a build claiming engine acceptance for a size-changing
edit and the full ByteRun1 encoder, neither of which has run. Vocabulary is not a sentence. The
tests now take the limits from the roadmap's own `Not established.` paragraph at runtime, assert
the caveat makes exactly one establishment claim and that it is negative, and assert each limit
sits inside the negative clause. Four mutations, including the reviewers' exact inversion, all
fail. The `assertIn("2026-09-18", roadmap)` it also carried is gone: any occurrence of a date
anywhere in a 400-line document satisfied it.

**The published operator-ordering table had an unrecorded delta, and "those rows cannot have
moved" was not good enough.** `tools/operator_groups.py` consumes `gs_syntax.tokens`, so the fixes
changed its inputs; restoring 706 tokens to `wacave.gs` and splitting `w/name`-shaped tokens grows
caller sets, and three of the four rows are computed from caller sets. Re-run with the tokenizer
immediately before the fixes and immediately after, over the same 1,696-member GS5R3 dump: the
first three rows are **identical**, and the fourth moves — observed mean run length 1.45 → 1.44,
shuffled baseline 1.09 → 1.10, ratio 1.32x either way. The table is republished with that row and
dated. Its published ratio had been 1.33x. The reproduction recipe was also wrong by omission and
is now written out: `caller_index` uses `Path.iterdir` and does not descend, and the
dominant-directory row recovers a directory by splitting the file *name* on `__`, so the dump has
to be flattened with that separator. Pointed at a nested tree the tool reports 30 operators with
callers instead of 1,371 and nobody is told.

**A parity test that skips is a parity test that is not run.** The new fixtures guarded on
`VIEWER.is_file()`, so a fresh checkout would report OK with all of them silently skipped, and an
edit to `gamescript.rs` without a rebuild would have them agree with a months-old authority. They
now build the viewer when it is missing *or older than `gamescript.rs`*, and skip only when the
build genuinely fails — the shape `stormlib_available` already uses in `tests/test_member_names.py`.
A stranded `unittest.main()` sat above the class, which under direct invocation would have defined
none of it; it is at the bottom of the file now.

**One claim three sections away was refuted by this work.** `gamescript-format.md` said, of `<<`
and `>>`, "Both of ours already do — checked, not assumed". `gs_syntax.py` does not: `x<<y` is one
token here and three to the authority. The original check saw the corpus's habit of
whitespace-separating `<<` and read it as evidence about the lexer. Corrected in place, with a
pointer to the parity section. A "checked, not assumed" phrase is worth exactly as much as the
mechanism behind it, and this one was a corpus coincidence.

### Corrections to the corrections, same day, round four

Three of the five claims the round-three note made about its own work were wrong. They are
corrected here rather than edited above, because the shape of the errors is the finding.

**The test said it took its limits from the roadmap at runtime. It did not.** The round-three note
and that test's docstring both claimed the caveat's limits were derived from the roadmap's
`Not established.` paragraph. The code computed that paragraph and then asserted three *other*
hardcoded strings into the caveat; nothing flowed from one to the other, one of the three
("ByteRun1 encoder") does not appear in that paragraph at all, and the whole class passed when the
roadmap's paragraph was monkeypatched to grow a limit. A test that reads a file and ignores what it
read is worse than one that never opens it, because the docstring buys trust the mechanism has not
earned.

**The prose approach was the defect, and the third bypass proved it.** Round one broke the caveat
test by turning `NOT established` into `ALSO established`; round two by turning `have none of them`
into `have every one of them`; round three by appending "In fact, the engine accepted a
size-changing edit, an added member, and the full ByteRun1 encoder" *after* the clause every
assertion was about. Each repair closed the previous bypass without narrowing the gap, because a
finite set of assertions about prose cannot constrain the open set of sentences prose can be. The
sentence is now rendered, not written: `tools/engine_acceptance.py` holds the run as data — count,
replaced-or-added, length-preserving-or-size-changing, date, mechanism, observation — and derives
the limits from it, so "the engine accepted a size-changing edit" cannot be said without recording
that the run *was* size-changing, which changes the rendered roadmap paragraph, which no longer
matches `docs/roadmap.md`. `build.json` carries the structure beside the sentence.

**That claim was checked and was wrong.** The round-four note said six mutations "including all
three historical bypasses" failed. Two of the three still worked, through fields the module stored
as free text and never guarded — `mechanism` and `storage_class` — and a third bypass worked at
the build site, past a check that read the source rather than the output. See the round-five note
below; the claim is corrected there rather than deleted here, because a repaired guard that still
claims more than it does is the exact failure this sequence keeps repeating.

Two holes stayed open after the first rewrite and were closed too: a wrapper at the build site
(`{k: dict(v, summary=v["summary"] + "…")}`) bypassed every assertion about the module's own
output, so the build's dict is now checked through the syntax tree and must be exactly
`engine_acceptance.build_metadata()`; and the one free-text field left, `observation`, could still
be widened to "Each of the 1,071 members was re-encoded and accepted", so a quantity that is not
the run's own count or date is now refused outright, and every observation has to be a sentence the
documentation already carries.

**`foo(1)` was four tokens, not five.** The parenthesis note shipped in five files with a wrong
count — `foo`, `(`, `1`, `)` is four — in a repository whose premise is that a stated count is a
measurement. The one place that was right used the fuller example (`/a{ foo(1) }def`, 8 against 5),
which is the lesson: a worked example carries its own check, a bare number does not.

**The operator-ordering table did not move, and the reported movement was hash noise.** Round three
reported the fourth row going 1.45 → 1.44 observed and 1.33x → 1.32x, and credited the tokenizer.
Re-running the two tokenizers with a fixed `PYTHONHASHSEED` gives **bit-identical floats on all
four rows**. The movement came from `caller_group_agreement`, which picked each operator's dominant
directory with `Counter.most_common` over a `set`, so ties broke by hash order: five runs of one
unchanged tokenizer give observed 1.43, 1.44, 1.44, 1.44, 1.45. The tie is now broken by name, and
`tests/test_operator_groups.py` runs the measurement under eight hash seeds and demands one answer.
**A one-run-against-one-run comparison could not have told a real delta from this**, and the way to
have known that was to run the do-nothing case first, which is a lesson this repository has already
written down.

The same page's "three of four rows are computed from caller sets" is also corrected: rows 1 and 2
are two projections of a single `call_site_agreement` computation and row 3 never touches the
caller index at all, so the invariance is one statistic plus one constant, not three independent
confirmations.

**What was re-run rather than reasoned about.** `docs/native-operator-bodies.md`'s operand-count
table is read through `gs_callsites.py` and so through this tokenizer. All five cited operators,
plus `launchmissile` and `relative2actual`, produce **byte-identical reports** before and after the
five fixes. The five members the string fix moved are named there now. One number in that section
could not be reproduced and predates this work: `launchmissile` has 4 + 7 + 5 = 16 call sites across
the three profiles against a recorded "twelve".

**Also closed.** `viewer_is_stale` compared the built viewer against `gamescript.rs` only, while
the value these tests compare is `token_digest` in `gs_facts.rs` — so an edit there would have left
the fixtures agreeing with a stale binary. The manual staleness rule is gone; `cargo build
--release` runs unconditionally and Cargo decides freshness, with the build's own stderr printed
when it fails instead of a guessed cause. And the parity measurement now states its denominator's
other half: **0 of 4,692 members produced a parse error**, which matters because `GsFacts::measure`
returns an empty digest for a member it cannot lex, so an erroring member would have left the
comparison silently.

## 2026-09-18 — Round five: every field that renders carries a rule, and the test reads the output

Three more bypasses of the engine-acceptance guard, all demonstrated by running them, and all of
one kind: **the module stored three sentences and guarded one.**

`mechanism` and `storage_class` were free text interpolated straight into the rendered caveat.
Appending the historical bypass sentence to either shipped it in every `build.json` with the whole
suite green — and `gs.mpq`'s mechanism was asserted nowhere at all, so nothing even pinned its
wording. Both are enums now, with fixed text and a name per value; the only choice a record makes
is which value applies. `observation` is the one sentence left, and it is constrained twice.

**A source-shape check cannot bound runtime behaviour, learned twice in one module.** The
round-four guard walked `mod_build.py`'s syntax tree and asserted the dict literal held
`engine_acceptance.build_metadata()`. It says nothing about what happens to that dict afterwards,
and a reviewer appended to every summary between the literal and `write_text` with the suite still
green. It is replaced by the obvious thing, which is also one line: run `command_report` in a
temporary directory and assert `json.loads(build.json)["engine_acceptance"] ==
build_metadata()`. That closes the class including the sources the AST walk never read. The same
mistake in miniature had already been made once here — a test that read `docs/roadmap.md` and then
asserted hardcoded strings — and the lesson is the same both times: **assert the artifact, not the
recipe.**

**A derivation can derive nothing.** `derived_limits` returns the empty tuple for a run that is
size-changing, added and multi-member, and nothing required the result to be non-empty — so a
record could claim 1,071 added members with size-changing edits and render "Not established: ."
The docstring claiming that could not happen was itself the evidence that nobody had tried it. An
`ArchiveAcceptance` whose limits are empty is now refused at import: no single attended run
establishes a whole archive.

**Containment was blind to polarity and to place.** The round-four rule asked whether an
observation appeared anywhere in two documents — 1,400 lines that include, deliberately, a
*quotation of a refuted claim* in `build-pipeline.md` ("a rewritten archive has never faced the
engine and the compression choice is Inferred"), so a sub-span of a sentence the repository exists
to refute would have passed as evidence. The rule now compares a **marked region** —
`<!-- engine-acceptance:<archive> -->` — against the render, for **every** archive with a run,
enumerated from the data rather than hardcoded to `pic.mpq`. `gs.mpq` had no such paragraph at all
and was therefore coupled to nothing; it has one now. Every rendered field, enums included, is in
that paragraph, so editing any of them breaks the comparison.

Mutations re-run after all of this, each one previously green: bypass through `pic.mpq`'s
mechanism, through `gs.mpq`'s mechanism, through `storage_class`, appending inside the renderer,
appending at the build site, a wrapper between the literal and the write, an em-dash continuation
inside the clause, a widened observation, a run with no limits, and both directions of the number
rule. All fail.

Two smaller repairs in the same pass: the tie-break test asserted a mean run length of 2, which
**both** tie outcomes satisfy, so it now names the winning directory through a new
`dominant_directories` helper; and the number rule refused a truthful observation citing
`0x80010100`, because the digit scan did not know hexadecimal.

### The game-running guard matches our own tools, and a skip was hiding it

The round-five note above said three pipeline tests had gone red because "a game is open". **That
diagnosis was probably wrong for at least some of those runs**, and the fix it produced was worse
than the failure.

`scripts/lib-mod-pipeline.sh` refuses while the game is up, by asking `pgrep -f 'lomse.exe'`.
`-f` matches the **whole command line**, so it matches every one of this project's own tools that
takes that path as an argument -- `tools/engine_probe.py`, `dumpva`, the save survey, the
corpus-gated disassembly test -- and a false positive lasts exactly as long as the tool runs.
Demonstrated 2026-09-18 with two sleeping processes, one carrying
`.../English/lomse.exe` and one carrying Wine's `d:\lomse.exe`: `pgrep -f 'lomse.exe'` matched
**both**; `pgrep -f '\\lomse\.exe'` matched only the Wine one. The game runs under Wine and its
command line has a **backslash**; ours have forward slashes. (The `.` needs escaping too -- it is
an any-character, which is how `lomse.exe` also matches `lomseXexe`.)

Converting that refusal into a `SkipTest`, as the round-five change did, made the two cases
indistinguishable: a real game skipped, and a false positive from our own tooling *also* skipped,
taking the whole install / profile-creation / restore coverage with it, silently, at the moment
that tooling was running. **A green suite that means the guard misfired is worse than a red one
that means a game is open** -- it is the same defect as the caveat test five rounds running: an
instrument reporting safety it has not earned.

The Python side now separates them: a process matching the narrow pattern is named in the skip
reason, so a skip says *which* process caused it; a refusal with no such process is retried once
(safe, because the guard refuses before the script writes anything) and then **fails loudly**,
printing whatever the broad pattern matched. Verified both ways by starting a process of each
shape.

**The pattern in this branch was still too loose, and a parallel fix on `main` is the one to
keep.** Requiring the backslash form only asks whether a command line *mentions* a Wine path, which
an agent, an editor or a shell quoting the name satisfies -- three agents tripped the guard in one
day, and writing the fix tripped it, because a comment in the patch command contained
`d:\lomse.exe`. The game's own command line **begins** with a DOS drive path, so the pattern is
anchored: `^[A-Za-z]:[\\]lomse[.]exe`, in both the shell guard and here. Verified in both
directions, which needs `exec -a` to fake convincingly: a process whose argv merely *contains* the
path does **not** match, and one whose command line *begins* with it does. Six consecutive suite
runs report `OK (skipped=1)` rather than 17, so the install and restore coverage that had been
disappearing is executing again.

**The retry is load-bearing rather than belt-and-braces, and the measurement above is why.**
Sampling `ps` through a failing run caught zero matching command lines: some matches are processes
that exit within milliseconds, so no pattern can close the race by itself. Anchoring shrinks the
false-positive population; the retry survives the ones that are already gone by the time anything
can look.

## 2026-09-19 — The game-running guard: two more real command lines, then a widened pattern still short of two profiles

The 2026-09-18 entry above ends on `^[A-Za-z]:[\\]lomse[.]exe`, anchored but pinned to the
executable sitting immediately at the drive root. **Observed in gameplay, PID 77245**: the
Development profile's real command line is six directories deeper --

    c:\program files (x86)\steam\steamapps\common\lords of magic special edition\english\lomse.exe /* MVK_CONFIG_FULL_IMAGE_VIEW_SWIZZLE=1

-- so that pattern matched nothing for a full day and the guard was dead. **Observed in gameplay,
PID 47723**: the 3.02 profile really does run from the drive root, `d:\lomse.exe /*`, so a pattern
written to fit Development alone would have missed 3.02 the same way. Both are now required
fixtures (`OBSERVED_GAME_ARGV0`, `OBSERVED_GAME_ARGV0_DRIVE_ROOT` in `tests/test_mod_pipeline.py`),
and the pattern's directory component is optional rather than a specific path for exactly this
reason -- see `game_command_pattern` in `scripts/lib-mod-pipeline.sh` for the full ladder, which
this entry does not re-derive.

The pattern was then widened twice more, both times for launch layouts nobody has watched live:
a 64-bit Wineskin bottle (`Program Files`, no `(x86)`), a Wineskin profile mapping its game drive
off `c:`/`d:`, a forward-slash drive path, a UNC path with no drive letter at all, and a quoted
argv0. None of these is **Observed**; each is **Inferred** from how Wine and `CreateProcess` are
documented to behave, adopted because the guard's own asymmetry (a false negative can corrupt an
archive under a live process; a false positive only costs a re-run) argues for covering a plausible
shape rather than waiting to observe it. `game_command_pattern`'s comment names, for each one,
whether it costs anything against the existing decoys -- none of them do.

**Still not established: GS5R3 and vanilla.** Both real command lines above are Development and
3.02. `scripts/restore-game-archives.sh` -- which now shares this guard -- targets GS5R3
specifically, and nobody has run `ps -o command=` against a live GS5R3 or vanilla process. The
anchor's whole safety argument depends on the game's own command line beginning with the DOS path,
which is exactly the fact that is unmeasured for these two. An attended `ps -o command=` capture
against both, while each is running, is the next thing this guard needs and is not something a
closed-game measurement can substitute for.

## 2026-09-19 — The savegame writer, and the 71 phantom records it found on the way

`docs/save-format.md` said "there is still no savegame writer; the other eight payloads are copied
through unchanged". All nine sections now have an encoder and `SaveFile::encode` writes a whole
file from the decoded sections with **nothing spliced**: **31 of 31** installed saves come back
byte-identical. That figure is split in the doc between what is **reconstructed** (every count
word, every length word, every version gate) and what is **replayed** (every opaque block and the
leaked `LS_MULT` name padding), because only the first half can fail.

**The interesting result is not the writer.** Writing an encoder for `LS_GAME` meant reading its
writer, and the writer says the section does not end where this project thought it did. Seven
`fwrite`s follow the record table — `4 + 4 + 32 + 4 + (8 + 4*n) + 200 + 4`, all version-gated —
and with `n == 150` in every corpus file that tail is **856 bytes**, which is exactly
`71 × 12 + 4`. The page's headline unexplained constant, `N - live_count == 71`, held over ten
game states because the old model was reading a fixed tail as seventy-one records and a trailer.
The count word it called `live_count` is simply the record count: `0x0052B3B0` walks a linked list
to length and writes that many nodes.

**Why it survived**: the old model's only checks were a byte total and `(payload - 24) % 12 == 0`,
and 856 satisfies both. That is the third time on that page that an accounting check over a *sum*
failed to see a regrouping of its *terms* — the `128*128*12 + 16` note diagnosed the same failure
in `LS_MAP_` and did not generalise it. The rule worth carrying out of this: **an unexplained
constant that appears in every file is a reading error until it has been read out of the
instruction stream.**

Three smaller corrections came with it. `LS_GAME`'s stored record size **is** a length the reader
uses (`cmp dword,0xc / jbe` at `0x0052B2F8`, then an `fread` of that width), so "there is no
evidence the reader uses this as a length" is refuted and "`LS_MULT` is the only real length in the
format" is withdrawn. And the parser now **refuses** a record size other than 12, which is a
deliberate difference from an engine that would copy three dwords out of a stack buffer it does not
clear between records.

**The instrument was `objdump -d` against `lomse.exe`**, the same read-only route the previous
passes used, and the method was the same: decode from the engine's own **writer**, then check the
model as a byte account over the corpus. Every length in the new `LS_GAME` model is either a
constant in the instruction stream or a count the file stores, so a merely plausible model stops
short or overruns; this one lands exactly, in all 31 files, across both format versions present.

**What this does not establish.** No save this project has written has ever been loaded by the
engine — that run is still an attended item, and "composable offline" is the whole claim. Nothing
here decodes the *meaning* of any `LS_GAME` tail field: the 32-byte and 200-byte blocks and the
150-word array are carried verbatim, and a writer that preserves bytes is not a reader that
understands them.

## 2026-09-19 — The `unitanchor` run: the probe was right, the subject was the wrong file

The probe ran attended on GS5R3. All four rungs fired, both gated cleanups ran, nothing was left on
the map, and `scripts/restore-game-archives.sh` verified both archives back to their recorded
originals (`gs.mpq 2d394279…`, `imp.mpq cb5c1068…`). `zprobe.log`: army loc 185, cell (57,1),
owner 0, target cell 58, both placements `facing 4`, both cleanups done.

The first reading of the captures concluded the experiment had failed. It had not. Two mistakes,
one in the instrument and one in the run sheet's own premise, are what made it look that way.

### The instrument: components are not silhouettes

`tools/probe_captures.py` reported *connected components*. `units\imp\licr2a.imp` frame 0 is 30x122
and the engine draws it with a transparent band across the middle, so the control rung came back as
**30x111 plus a detached 8x9 three rows lower**. Read component-wise, a control that had reproduced
its frame **exactly** looked as though it had missed by 11 pixels.

The tool now reports the union box alongside the components, and says how many components fell below
the reporting threshold and what the union would be including them — a sprite's own outlying tail is
often two or three pixels. `tests/test_probe_captures.py` carries the split-sprite case.

### The premise: the world map draws the `b` file, not the `a` file

**Observed in the corpus.** A unit type declares `/impfile_proc{"licr2"unittype_imp_filename}`, and
`gs\imps.gs`'s `unittype_imp_filename` consumes two operands — the base name, and beneath it a
**screen mode the engine pushes**, documented in that file's own `citygate_imp_filename` comment as
`<zoom_mode (from c++)>`. `unit_zoom_letter` maps COMBAT_SCREEN and LOCATION_SCREEN to `"A"`, and
SCROLLINGMAP_SCREEN, REGION_SCREEN and WORLD_SCREEN to `"B"`. **Observed in a local binary:**
`lomse.exe` carries `getzoommode` (arity 0) and `setspritezoommode` (arity 1).

So a unit standing on the world map draws `units\imp\licr2b.imp`. The run sheet had asserted
`licr2a.imp` — the combat sprite — and the analysis was matching a world-map capture against a frame
table from the wrong file. Nothing matched, which is exactly what should happen.

**Observed in the corpus.** A world-map army is also a *composite*, not one sprite.
`gs\PLAYER5.gs`'s `setupplayergraphics` attaches, per player, a faith flag
(`"iface/liflagb.imp" 0 -40 setplayerflagimp` — four operands, matching `setplayerflagimp`'s
disassembled arity of 4), health bars (`setplayerbars`), a group-number object at
`0 -20 setplayergroupnumberoffset`, and an indicator aura and halo colour. A box drawn around a
placed unit therefore encloses at least two independent sprites, and the "47x72" the first reading
tried to match was a union of the body and the flag. It is no frame of anything, and never was.

### What the captures actually say

Control rung: union 30x122 at (374,136) — frame 0 of `licr2a.imp`, record 0 `(1,-25)`. So

```
anchor = (374,136) - (1,-25) + (15,61) = (388,222)
```

which reproduces the 2026-09-16 result on a new cell in a new session.

**Body.** `licr2b.imp` frame 33 (STAND, facing record 8) is 47x46, record 0 `(0,-6)`:

```
predicted = (388,222) + (0,-6) - (23,23) = (365,193)      measured = (365,193)
```

Of all 86 frames of the file, admitting both orientations, exactly four predict `(365,193)`: frames
19 (50x47), 20 (45x46) and 23 (49x46) mirrored, and frame 33 in either orientation. **Only frame 33
is 47 wide, and 47 is the measured width.** Two facts not used to pick it agree: the 3-pixel
component at `(366,237)` sits at sprite-relative columns 1-3 rows 44-45, where frame 33 is opaque
**only when mirrored** (unmirrored those rows are opaque at columns 43-45); and the capture shows a
unicorn facing left while frame 33 as stored faces right. **The engine mirrored the frame, and the
rule still predicted its top-left to the pixel.**

**Flag, an independent second test of the same anchor.** `iface\liflagb.imp` frame 111
(UNIT_UNSELECTED, cycle offset 7) is 12x21, origin `(-5,-7)`, hanging off `anchor + (0,-40)`:

```
predicted = (388,182) + (-5,-7) - (6,10) = (377,165)      measured = (377,165)
```

Frame 111 is the **unique** frame of all 112 in that file predicting `(377,165)`, and its stored size
matches the measured silhouette exactly rather than approximately.

Two sprites, two files, two sub-paths of the army composite, one anchor recovered from the
terrain-sprite rung, **zero residual in x and y for both**. **Inferred**, conditional on the two
frame identifications; they are independent of each other, and a coincidence would have to hit both.

### What stays open, and why the probe was rebuilt

1. **One facing, sampled twice.** Both placements reported `facing 4` and produced pixel-identical
   captures. The run carries one facing, not two.
2. **The mirror sign is undetermined.** Frame 33's record-0 `x` is `0`, so `+placement.x` and
   `-placement.x` predict the identical pixel.
3. **One unit type.**

The probe now works **three cells**, each with its own control rung and therefore its own
independently recovered anchor, and its subject is `/pyele` (Elephant) — chosen by sweeping all 141
`units\imp\*b.imp` members rather than by resemblance. It is the only one of the 103 with a
five-frame STAND sequence that satisfies both properties the failure showed are needed: every STAND
frame is separated from every other frame in the file **in both orientations** by position or size,
and every STAND frame's record-0 `x` is at least 4 from zero, so the two mirror hypotheses differ by
8 to 36 pixels on every facing. Both properties are asserted in `tests/test_engine_probe.py`; the old
`licr2a` table fails both (10 distinct signatures of 12, minimum |x| of 0).

### The persistent 10x26 component at (358,259)

**Observed in gameplay.** Absent from `zu0`–`zu3`, present and *byte-identical* in `zu4`, `zu5` and
`zu6`. It lies inside a green humanoid figure already present in the plate, 22 px left and 66 px
below the probe's cell; only the figure's lower body changes.

**Refuted as** anything the probe placed (absent from the two captures in which the probe's own
sprites were on screen, and it survives both id-exact cleanups into `zu6`); as the army's
group-number badge (that would land at `anchor + (0,-20) - (14,14) = (374,188)`); as a health or
morale bar (32x5 and 24x4 doodads, not 10x26); and as an ordinary animation cycle (it advanced once
and then held across three further redraws and three more captures).

**Inferred:** a one-step advance of a pre-existing world-map figure. *Why* it stepped between `zu3`
and `zu4` and then not again **cannot be established offline**, and is not established here.

### The methodological lesson

The run sheet stated what each outcome would mean before the run — including the case that occurred,
"the silhouette matches none of the six candidate frames" — and correctly called that an
identification failure rather than a result about the anchor. What it could not do was recover,
because the candidate list itself was derived from an assumption the sheet never marked as one:
*which file the engine draws*. **A sheet that names its subject's art without deriving it has a
premise its own failure cases cannot reach.** The rebuilt sheet derives the file from
`unit_zoom_letter` and requires the analysis to identify the frame from the capture, in both
orientations, rather than confirm a guess.

## 2026-09-20 — The rebuilt `unitanchor` run: zero residual on three anchors, and the mirror sign

The three-cell probe ran attended on GS5R3. All three cells placed, all three gated cleanups
**done**, zero `REFUSED`, all 11 captures written. Collected to
`artifacts/engine-probe-captures/run-20260920-210403`.

**The unit draw path has no unit-specific anchor offset.** Each cell's control rung recovered its
own anchor through the terrain-sprite path, and each cell's Elephant was then solved against that
anchor alone. The residual is **zero in x and y on all three** — (388,222), (252,161) and
(252,254). Three independently recovered anchors agreeing leaves no room for an additive constant,
which is exactly what `hotspots.md`'s unit-path caveat was reserving judgement on. Closed as
answered.

**The mirror sign fell, and the subject was chosen so it could.** The body is
`units\imp\pyeleb.imp` frame 33 — the unique fit among all 86 frames in both orientations, on each
of the three anchors independently. Its record-0 `x` is `+14`: as stored the rule predicts a left
edge of 370, mirrored it predicts 342, and 342 was measured three times. A 28-pixel discrimination,
where the 2026-09-19 run had zero because its frame's record-0 `x` was `0`.

**🔴 And then the repository turned out to already know the answer, more precisely than the run
did.** `imp_anim::mirrored_anchor_x` has carried the flipped placement rule since it was read out of
`ImpPlayer::GetPlacement` (`0x0049CC80`), including a detail this run cannot see: the flipped value
at `0x0049CCC8..0x0049CCD3` is `-((width >> 1) + placement_x)`, **minus one more pixel when the
width is even**. Frame 33 is 65 wide — odd — so the run exercised the branch without the `dec` and
would have published a simpler rule that is wrong on every even-width frame.

It was caught only by grepping for `mirror` before writing the doc. The rule was in `imp_anim.rs`
and in nothing else; `hotspots.md`, the file that calls itself *the single source of truth for how
the engine places a sprite frame*, said only that mirroring's effect "is undetermined". **Derived
knowledge that lives in one module is not published**, and the single-source-of-truth document is
the thing it has to reach. `hotspots.md` now states both branches, with the binary for the rule and
this run for the odd branch, and marks the even-width `dec` as never having faced the engine.

**What did not close:** one unit type, and the facing. All four placements across both runs
reported `facing 4`. Different *cells* did not buy different facings — the one thing the three-cell
design hoped for and could not guarantee, and the sheet said so in advance.

**A capture-level lesson.** cell 1's control came back `29x122`, and the sheet's own rule is that
anything but `30x122` voids the cell. It was not void: cell 1's per-column changed-pixel profile is
a strict subset of cell 2's *at the same x*, column for column, converging to equality from x259 —
whereas a one-pixel shift would have made cell 1's x253 match cell 2's x252 (29 against 15), and it
does not. The leftmost column was **occluded by terrain that already matched the plate**; a
difference image cannot see a sprite pixel equal to what was behind it. The exception was licensed
by a positive demonstration of where the column went, not by the absence of an alternative, and the
sheet's rule stays as written.

**A cross-check that turned out to be weaker than advertised.** The 2026-09-19 write-up called the
player flag an independent test of *the anchor*. It is an independent test of the anchor's **y**
only. All nine `iface\??flagb.imp` files share geometry across the 15 origin-bearing frames in
96..111, every one giving `py - (h>>1) = -17`, so the predicted flag top is `anchor.y - 57` no
matter which frame or faith drew — and the flag is at most 15 wide inside a 65-wide body, so it
cannot move the union's left or right edge at all.

## 2026-09-20 — Direction 0, chased offline: a static table in `.data` closes most of B2

The run sheet said to prefer the offline chase because it was free and might make the attended run
unnecessary. It did, for most of the question.

**`lomse.exe` carries a static 8-entry direction table**, twice: `0x005557E8`/`0x00555808`, and an
identical pair at `0x555828`/`0x555848`. Indexed by `direction * 4` at `0x0041B183` (direction from
object field `+0x44`) and at `0x004212D0..0x00421303`, where the linear cell index is `idiv`'d by
`[ecx+0x6c]` and the **remainder** takes the lower array while the **quotient** takes the upper one.
A cell index is `second_operand * width + first_operand`, so the lower array is the first-operand
delta. Composed with this project's own **Derived** `n = (0,-1)` (576/576 engine-written ring tiles
against 0/576 mirrored):

> **Direction 0 is the `.til` column `s`**; the ring is `0=s, 1=sw, 2=w, 3=nw, 4=n, 5=ne, 6=e, 7=se`.

**This does not inherit the x/y ambiguity**, and that was worth checking rather than asserting:
`Map::cell_index` is `y * width + x`, so the writer's `x` *is* the first operand and `tile.rs`'s
`(dx, dy)` is `(Δ first, Δ second)` directly. A relabelling of the axes moves both halves together.

**What is still open is the screen orientation, and only that.** A full string scan of the image
finds **no compass vocabulary anywhere**. That the column named `s` points down-screen is the
tileset authors' naming plus derived geometry, not something the binary says. The attended run is
now much smaller than it was: it has one thing to settle.

**Why the `0x5AE970` builder is unreachable, stated as a measurement rather than a shrug.** The
dword occurs **36 times in `.text` and every one is a read encoding** — no `A3`, no `C7 05`, no
`89 xx`. `.data` has `rsz=0x23200`, so raw data stops at `0x578200` and `0x5AE970` is in the BSS
tail, written at runtime. It is field `+0x18` of a singleton based at `0x5AE958`, whose
`mov ecx, 0x5AE958` immediate appears **499** times in `.text`. The store goes through a register
and no byte scan can see it. This needs the disassembly phase.

**🔴 A trap worth recording.** `reports/natives/global-clusters.tsv` marks this cluster
`read_only=no`, which reads like "something writes it". It means only "not in a read-only PE
section" (`operator_bodies.rs:614,2219`) and carries **no writer information**. Every one of the
five operators that touch `0x5AE970` has `globals_written=0`.

## 2026-09-21 — A unit type at index 160 draws, and the baseline that caught itself being wrong

The `unitindex` probe ran attended on GS5R3. Both cells placed, both gated cleanups done, zero
`REFUSED`, all five captures written. Collected to
`artifacts/engine-probe-captures/run-20260921-093451`.

**The question is answered.** A unit type defined at runtime at index **160** — above anything any
installed profile ships — placed and drew a 65x78 sprite at `(206,104)`, the same silhouette and
the same size as the shipped control at `(342,165)`. `lastunittype` read back **160** against a
predicted 160, and `numunittypes` moved 160 → 161. The post-cleanup capture for the control rung
differs from the plate by **zero components**.

**🔴 And the baseline assertion failed, which is the more useful result.** `numunittypes` reported
**160**, not the **155** the sheet predicted. The prediction was measured from the wrong archive:
`gs\unittype.gs` was read out of the *Development profile's pristine `gs.mpq`* — which is **vanilla**
— while the probe ran on **GS5R3**. Re-measured across all three:

| profile | `units/*.gs` runs | unique |
| --- | ---: | ---: |
| **GS5R3** | **159** | **159** |
| 3.02 | 153 | 152 |
| vanilla | 153 | 152 |

**So GS5R3 already ships unit types at indices 155-159, and this run sheet's own premise —
"nothing above 154 has ever existed at runtime" — was false when it was written.** The shipped game
had already been demonstrating the answer. Nobody had looked, because the count came from one
profile and the engine run happened on another.

This is the profile-scoping trap again, and it is the fourth time this project has been bitten by a
figure that was true of one installed variant and quoted as though it were universal. The
difference this time is that **it failed loudly**. The probe logs its baseline and states the
expected value beside it, so the discrepancy arrived as a log line rather than being silently
absorbed into a wrong conclusion about indices. That is exactly what stating an expected value
*beside* a reading, rather than in a document nobody re-reads, buys you. A probe that had merely
**assumed** 155 would have placed at 160, drawn correctly, and produced a write-up claiming
something subtly false — and nothing in the run would have contradicted it.

The constant now carries GS5R3's measured runtime value, marked **Observed in gameplay** rather
than counted from the corpus: the run list is only a proxy, because one member can hold several
definitions (`units/gate.gs` holds three).

**A by-product, filed under the question it also settles.** The control landed at `(342,165)` and
the subject at `(206,104)` — *exactly* the cell-0 and cell-1 positions the 2026-09-20 `unitanchor`
run measured, on the same map with the army in the same place, with the same 65x78 silhouette
(`pyeleb.imp` frame 33, mirrored). So this run **independently reproduces the mirror-sign result**
through a different probe built for a different question, and it is recorded in the
[unit anchor sheet](unit-anchor-run-sheet.md) as well as here.

**Still not established:** anything about passing **199**; durability across save/load, since the
type exists in no archive; new art; and the three adjacent caps, of which `maxauratypes` is 70 of
70 with zero headroom.

## 2026-09-21 — `maxauratypes` is not a cap: six tables, one allocator, no upper bound

The closing note of the previous entry named `maxauratypes` — 70 of 70 — as the live constraint on a
real new unit, ahead of the unit-type count. Chasing it offline dissolves it.

**Observed in a local binary.** `maxauratypes` (`0x0042e1e0`) pops one operand and calls an
allocator at `0x0042e080` with `this = 0x005cd334`. That allocator's only guard is `cmp ebx,1` /
`jge`: a count below 1 fails, and nothing else is compared. It computes `count * 72` through
`lea eax,[ebx+ebx*8]` / `shl eax,3`, runs a per-element constructor, and writes a three-field header
— capacity at `+0`, used at `+4`, base at `+8`. The five sibling operators (`maxmissiletypes`,
`maxmounttypes`, `maxterrainspritetypes`, `maxreferencepalettes`, `maxunittypes`) are the same shape
with different strides, and **each of the six whole bodies contains exactly one literal compare, the
`cmp <count>, 1`**.

One sibling breaks the pattern in a way worth recording, because assuming otherwise would misread
its memory: `maxreferencepalettes` writes the **base** to `+0` and the **capacity** to `+8`, the
reverse of the other five, and has no per-element constructor at all. The self-check that caught it
was re-running the literal-compare scan over the *whole* body of each of the six rather than the
entry, which is the version of the check that also had to be right for the headline claim.

So each of those six figures is a **script literal**, not an engine bound. (`setmaxartifacttypes`,
`setmaxquesttypes`, `maxpalettes`, `maxdialogs` and `maxgraphics` were not followed; the shape is
not claimed for them.) The unit-type
result from 2026-09-21 was not the special case it looked like; it was one instance of the general
rule, and I published the general rule as a per-table warning because I had only disassembled the
one table.

**Overflow is loud.** `addauratype` (`0x0042e290`) appends through `0x0042e160`, which returns -1
when `used >= capacity`; the operator then prints `"addauratype failed"` (`0x00555eb4`) and the
script binds the name to -1. A 71st aura today does not crash at registration. The exact eight-byte
append guard occurs at **5** sites in `.text` — aura, `0x0043c434`, missile, terrain sprite, unit
type — so these tables share one implementation.

Lookup (`0x0042e130`) takes a full 32-bit index, rejects negatives, and bounds against the live
**used** count. No aura index is narrowed anywhere in the image.

**Observed in the corpus.** `gs\aura.gs` is the only member that mentions auras: line 1 is
`70 maxauratypes`, and below it sit exactly 70 non-comment `addauratype` calls — 62 bound to a name
with `def`, 8 more (one per faith) collected into the `/aura_dict` dictionary. The "70 of 70"
reading was correct; the inference drawn from it was not.

**What is still open, and one thing worth watching.** No run has raised any cap — this is
disassembly alone. Savegames were never examined for aura ids, so a save written above 70 is
untested in both directions. And `0x0042d6b0` calls the lookup and immediately dereferences
`[eax+0x24]` with no null test, while its sibling setter at `0x0042d750` does check — and that setter
stores the id into `+8` *before* validating it. Whether the unchecked path is reachable after a
failed registration was not traced.

**The lesson is the one already on file.** A bound measured in one table is a fact about that table;
the same question asked of the neighbours took one `objdump` pass each. The warning stood for a day
because nobody asked the cheap version of the question.
