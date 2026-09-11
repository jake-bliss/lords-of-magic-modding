# Game and Data Architecture

## Modding boundary map

| Component | Observed contents | Likely modding uses | Difficulty |
| --- | --- | --- | --- |
| `gs.mpq` | Gameplay scripts and definitions | Units, spells, artifacts, buildings, encounters, balance, UI text, scripted behavior | Low–medium after extraction |
| `pic.mpq` | Primarily paletted IFF PBM images plus a small number of BMP/unknown resources | Portraits, icons, panels, menus | Low–medium for inspection; repacking semantics remain unknown |
| `imp.mpq` | Core game data | Engine-facing resources; exact scope not yet catalogued | Medium–high |
| `sndfx.mpq` | Sound effects | Replacement and remastering | Medium |
| `special.mpq` | Special Edition data | Expansion-specific resources; exact scope not yet catalogued | Medium–high |
| `map/*.scn` | Custom worlds | Terrain, factions, starts, structures, encounters | Low–medium |
| `map/*.lgd` | Legends of Urak scenarios | Scripted campaign content | Medium |
| `map/*.smp` | Map/sprite components | Locations and encounter scenes | Medium |
| `Wav/` | Music and spoken audio | Remastering and replacement | Low–medium |
| `smk/` | Smacker cinematics | Replacement cinematics | Medium–high |
| `lomse.exe` | Closed-source engine | Rendering, input, pathfinding, hard-coded limits and mechanics | High–very high |

## Evidence for script-level moddability

The community 3.02 patch replaces `gs.mpq` and configuration data without replacing `lomse.exe`. Its notes attribute bug fixes, new hotkeys, expanded options, interface changes, and removal of CD checks to that data layer.

GS5R3 likewise replaces `gs.mpq`; its required PIC5R3 package replaces `pic.mpq`. Community documentation attributes extensive unit, spell, artifact, encounter, auto-combat, reporting, and balance changes to these packages.

This demonstrates that the script/data layer is broad enough for a substantial expansion without executable patching.

The first local extraction confirms that `.gs` files use a PostScript-like, stack-oriented language. The entry script loads other files with string-plus-`run` expressions, definitions use `/name ... def`, braces delimit executable blocks, and semicolons begin line comments. Original Steam scripts are often minified into a single line; community versions are usually formatted and commented.

See [MPQ inventory](mpq-inventory.md) for the measured archive contents and exact 3.02 change surface.

The first native [asset-viewer spike](../spikes/asset-viewer/README.md) found 1,377 IFF `FORM PBM` members in GS5R3 `pic.mpq` and decoded all of them. They use indexed palettes and are either uncompressed or ByteRun1-compressed. At least one UI atlas visibly uses bright green as a likely engine-level chroma key while declaring no standard PBM mask, so format decoding and game compositing must remain separate concerns.

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

The game includes Map Editor and Lord Editor entry points. The original manual states that custom `.SCN` worlds are stored in the `LOM/MAP` folder and can define terrain, encounters, capitals, temples, and starting positions.

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

- Complete internal file lists for `imp.mpq`, `sndfx.mpq`, and `special.mpq`; some `pic.mpq` entries also lack catalogued names.
- Full script grammar, built-in vocabulary, type behavior, and execution model inside `gs.mpq`.
- Which AI, pathfinding, diplomacy, and auto-combat behaviors are scripted versus hard-coded.
- File formats and composition rules used for sprites, animations, interface layouts, and chroma-key transparency beyond the now-identified PBM pictures.
- Hard limits on units, artifacts, spells, maps, IDs, and string tables.
- Save-file compatibility rules after data changes.
- Multiplayer determinism requirements and checksum/version checks.
- Whether loose files override MPQ members consistently.
