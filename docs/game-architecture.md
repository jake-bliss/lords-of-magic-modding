# Game and Data Architecture

## Modding boundary map

| Component | Observed contents | Likely modding uses | Difficulty |
| --- | --- | --- | --- |
| `gs.mpq` | Gameplay scripts and definitions | Units, spells, artifacts, buildings, encounters, balance, UI text, scripted behavior | Low–medium after extraction |
| `pic.mpq` | Primarily paletted IFF PBM images plus a small number of BMP/unknown resources | Portraits, icons, panels, menus | Low–medium for inspection; repacking semantics remain unknown |
| `imp.mpq` | 1,800 IMP sprite/animation binaries paired with 1,800 `.H` companion headers in GS5R3 (**the engine parses these at load time** -- see [imp format](imp-format.md#-the-remap-is-parsed-from-the-h-companion-member-at-load-time); they are not build residue) | Units, buildings, effects, missiles, interface sprites, palettes, animation metadata | Medium–high |
| `sndfx.mpq` | 1,880 WAVE members in GS5R3 | Sound-effect replacement and remastering | Medium |
| `special.mpq` | 1,218 WAVE members in GS5R3 | Special Edition voice/audio replacement and remastering | Medium |
| `map/*.scn` | Custom worlds | Terrain, factions, starts, structures, encounters | Low–medium |
| `map/*.lgd` | Legends of Urak scenarios | Scripted campaign content | Medium |
| `map/*.smp` | Map/sprite components | Locations and encounter scenes | Medium |
| `Wav/` | Music and spoken audio | Remastering and replacement | Low–medium |
| `smk/` | Smacker cinematics | Replacement cinematics | Medium–high |
| `lomse.exe` | Closed-source engine | Rendering, input, pathfinding, hard-coded limits and mechanics | High–very high |

The shared native prefix and cell grid for all three map families is documented in the [map-format probe](map-format.md). Standard terrain tags now resolve through the original tile atlas, and the placed-object section of **every** installed map is structurally typed — six record layouts, 21,117 records — while unproven fields retain candidate names.

## Evidence for script-level moddability

The community 3.02 patch replaces `gs.mpq` and configuration data without replacing `lomse.exe`. Its notes attribute bug fixes, new hotkeys, expanded options, interface changes, and removal of CD checks to that data layer.

GS5R3 likewise replaces `gs.mpq`; its required PIC5R3 package replaces `pic.mpq`. Community documentation attributes extensive unit, spell, artifact, encounter, auto-combat, reporting, and balance changes to these packages.

This demonstrates that the script/data layer is broad enough for a substantial expansion without executable patching.

The first local extraction confirms that `.gs` files use a PostScript-like, stack-oriented language. The entry script loads other files with string-plus-`run` expressions, definitions use `/name ... def`, braces delimit executable blocks, and semicolons begin line comments. Original Steam scripts are often minified into a single line; community versions are usually formatted and commented.

See [MPQ inventory](mpq-inventory.md) for the measured archive contents and exact 3.02 change surface.

The native [asset tool](../spikes/asset-viewer/README.md) found 1,377 IFF `FORM PBM` members and 26 tile-set definitions in GS5R3 `pic.mpq` and decoded all of them. It also classifies all 9,804 members in the five core archives and pixel-decodes all 1,800 IMP sprite binaries. IMP uses indexed palettes, a custom packet RLE, 8/4/2/1-bit packed pixels, animation tables, hotspots, and multiple shared-frame conventions. Representative unit art renders recognizably. Pure-green IMP backgrounds (index 0) and separate pure-red silhouettes (index 1) (colours corrected 2026-09-23; see the [research log](research-log.md#2026-09-23--imp-palettes-are-bgr-after-all-the-capture-reader-swapped-red-and-green)) are two engine-level channels — transparency and a 50% translucency blend, both measured in the engine; the viewer exposes clean-preview, mask, and raw modes while keeping these display hypotheses separate from lossless decoding. See the [Stage 1 record](native-asset-stage.md) for current coverage.

## Known executable imports and runtime dependencies

The 32-bit `lomse.exe` imports legacy Windows APIs and libraries including:

- DirectDraw (`DDRAW.dll`)
- DirectPlay (`DPLAYX.dll`)
- DirectSound (`DSOUND.dll`)
- Blizzard/Sierra Storm archive support (`STORM.dll`)
- Smacker video (`smackw32.dll`)
- WinMM

It also imports registry and drive APIs such as `RegOpenKeyExA`, `RegQueryValueExA`, and `GetDriveTypeA`. This agrees with the observed CD-path behavior.

## Built-in creation tools

The game includes Map Editor and Lord Editor entry points. The original manual states that custom `.SCN` worlds are stored in the `LOM/MAP` folder; its terrain mode edits topography, while sprite mode places structures, objects, and encounters. Extracted editor scripts independently expose `setterrain`, `forcetexture`, `addterrainsprite`, and `setterrainspriteprocid`, providing vocabulary for the decoded map fields without yet proving every binary representation.

The editors provide a useful test surface, but they do not replace the need to understand `gs.mpq` scripts for new mechanics or richer scenario behavior.

## Asset-upscaling constraints

Runtime scaling is already handled by `cnc-ddraw`; Lanczos is the current shader. Asset replacement is possible, but original dimensions, palette/indexing, transparency, and coordinate assumptions may be hard-coded.

A safe asset workflow should:

1. Extract without altering the archive.
2. Record dimensions, color mode, palette, transparency, filename, and archive path.
3. Upscale or redraw from a lossless source.
4. Convert back to a game-compatible format.
5. Validate dimensions and palette constraints automatically.
6. Repack into a development-only archive.
7. Compare screenshots in a deterministic scene.

## Unknowns to resolve

- Purpose-level names for some generic or mismatched public-listfile entries, including four orphan IMP header/binary names.
- Full script grammar, built-in vocabulary, type behavior, and execution model inside `gs.mpq`.
- Which AI, pathfinding, diplomacy, and auto-combat behaviors are scripted versus hard-coded.
- Uncommon IMP shared-frame metadata variants, the shadow-index blend, palette/chroma-key transparency, and sequence timing. **Placement is settled** — see [hotspots](hotspots.md).
- Hard limits on units, artifacts, spells, maps, IDs, and string tables.
- Save-file compatibility rules after data changes.
- Multiplayer determinism requirements and checksum/version checks.
- ~~Whether loose files override MPQ members consistently.~~ **Settled 2026-09-16: they do not.**
  A three-arm controlled test (probe script, garbage file, shipped original) reached run position 93
  identically in every arm, while the same bytes executed from inside `gs.mpq`. Modding therefore
  requires archive write-back.
