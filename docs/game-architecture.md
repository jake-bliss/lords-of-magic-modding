# Game and Data Architecture

## Modding boundary map

| Component | Observed contents | Likely modding uses | Difficulty |
| --- | --- | --- | --- |
| `gs.mpq` | Gameplay scripts and definitions | Units, spells, artifacts, buildings, encounters, balance, UI text, scripted behavior | Low–medium after extraction |
| `pic.mpq` | Images and interface resources | Portraits, icons, panels, menus | Medium |
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

- Complete internal file lists for all five MPQ archives.
- Script grammar and execution model inside `gs.mpq`.
- Which AI, pathfinding, diplomacy, and auto-combat behaviors are scripted versus hard-coded.
- File formats used for sprites, animations, palettes, and interface layouts.
- Hard limits on units, artifacts, spells, maps, IDs, and string tables.
- Save-file compatibility rules after data changes.
- Multiplayer determinism requirements and checksum/version checks.
- Whether loose files override MPQ members consistently.
