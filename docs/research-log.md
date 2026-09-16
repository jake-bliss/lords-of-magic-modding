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

## Evidence labels for future entries

Use these labels when recording findings:

- **Observed:** reproduced on the local machine or directly inspected in a file.
- **Documented:** stated in original or community documentation.
- **Inferred:** strongly suggested by evidence but not yet directly proven.
- **Unknown:** an open question requiring research or experiment.
