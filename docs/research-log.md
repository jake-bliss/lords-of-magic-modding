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
- **Observed:** the format contains 32-byte file headers, animation sequences/cycles/frames, 256-entry BGRA palettes, six-byte padded hotspots, direct duplicate frames, and repeated cycles.
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
- **Implemented:** the viewer defaults to a clean preview while `C` cycles through visible-mask and untouched raw-palette modes. The decoder itself preserves every source index and palette color.
- **Corrected:** sampling the top-left pixel as a chroma key failed when frame 156 of `chcr5a.imp` touched that corner and removed gold artwork. Viewer channels are now selected by palette index 0 (background) and index 1 (secondary mask), preserving the same colors when they occur at other indices.
- **Unknown:** the original engine's exact mask blend, origins, hotspot behavior, sequence boundaries, and timing still require controlled comparison.

## 2026-09-12 — IMP actions, cycles, export, and FFI safety

- **Observed:** all 1,800 IMP binaries expose bounded sequence-to-cycle and cycle-to-frame ranges under the current parser.
- **Observed:** generated C-header labels cover 4,649 of 4,666 declared sequence slots; aliases are retained, and 1,799 of 1,800 headers provide at least one label.
- **Observed:** `units\imp\chcr5a.imp` contains seven named actions, five cycles per action, and 170 logical frames. The action names are `MOVE`, `STAND`, `DEFEND`, `GET_HIT`, `DIE`, `CORPSE`, and `MELEE_ATTACK`.
- **Inferred:** the five cycles in each inspected creature action are directional views. Raw sequence and cycle metadata remain preserved but uninterpreted.
- **Implemented:** the viewer navigates frames within a cycle, cycles within an action, and named actions; autoplay wraps within the selected cycle.
- **Implemented:** the CLI exports a resolved logical frame as an 8-bit indexed PNG while preserving palette indices and RGB palette entries. Synthetic decode-back verifies exact bytes, and output creation refuses overwrite.
- **Corrected:** the manual StormLib binding used Windows' 260-byte `MAX_PATH` inside `SFILE_FIND_DATA`, but StormLib's macOS portability header uses 1,024 bytes. Enumeration consequently overwrote adjacent stack memory. The binding now selects the platform ABI, a layout regression test matches the installed C header's 1,064-byte structure, and the full five-archive scan still succeeds.
- **Observed:** after the ABI correction, a real frame-155 export reports the requested index and is independently identified as a 165×127, 8-bit indexed PNG.

## 2026-09-12 — IMP placement records and native map grid

- **Corrected:** frame flag `0x04` is a shared-pixel reference even when it occurs after the first record in a cycle. Applying it consistently removed 27 false origin records and increased exact generated-header matches from 1,784 to 1,788 of 1,798 pairs.
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

## Evidence labels for future entries

Use these labels when recording findings:

- **Observed:** reproduced on the local machine or directly inspected in a file.
- **Documented:** stated in original or community documentation.
- **Inferred:** strongly suggested by evidence but not yet directly proven.
- **Unknown:** an open question requiring research or experiment.
