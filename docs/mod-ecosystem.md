# Mod Ecosystem

## Installed profiles

### Community 3.02 build 0014.3

Purpose: a mostly vanilla game with stability fixes and quality-of-life improvements.

Installed payload:

- Community `gs.mpq`
- `lom.cfg`
- `settings.cfg`
- `lomse302.htm`
- Optional `pic/lbm/newgame2.lbm` loose asset

Documented changes include:

- Correcting the level-one Chaos encounter typo that spawned Pegasi instead of Brownies.
- Removing CD checks.
- Adding Page Up/Page Down combat-map rotation.
- Adding and expanding hotkey, unit-information, game-menu, and options interfaces.
- Removing several menu/music/credits problems.

The archive identifies itself as **Build 0014.3, 11 December 2007**.

### ManTerA GS5R3

Purpose: a substantial balance and content overhaul.

Installed payload:

- `GS5R3.mpq` installed as `gs.mpq`
- Required `PIC5R3.mpq` installed as `pic.mpq`
- Twelve included `.scn` custom maps copied into `map/`
- The original GS5R3 readme and contributor list

The GS5R3 readme says the mod already handles the CD requirement and does not require official 3.01 first. Community reports attribute the following to GS5R3:

- Broad unit, artifact, spell, and economic rebalancing.
- Improved auto-combat estimates and results.
- Expanded faction reports and spell descriptions.
- More artifacts and custom-start choices.
- More informative encounter and army interactions.
- Changes intended to reduce combat exploits.
- Multiplayer-compatible scripted changes.

## Compatibility matrix

| Package | Baseline | 3.02 | GS5R3 |
| --- | ---: | ---: | ---: |
| `cnc-ddraw` rendering wrapper | Yes | Yes | Yes |
| Lanczos runtime shader | Yes | Yes | Yes |
| Community 3.02 `gs.mpq` | Optional | Installed | **No** |
| GS5R3 `gs.mpq` | Optional | **No** | Installed |
| PIC5R3 `pic.mpq` | Optional | Not installed | Required/installed |
| High-quality music replacement | Likely | Likely | Likely |

The 3.02 documentation explicitly says it cannot be used in conjunction with GS5. Both replace `gs.mpq`, so the separate-app design is intentional.

## Other packages worth evaluating

### High-quality music fix

A 2023 package replaces the clipped, noisy 8-bit WAV music with compatible 16-bit remasters. It appears independent of gameplay archives, but it should still be tested in one profile before applying elsewhere.

### LoMSE Update Mod

A 2024 community project built on 3.02. It focuses on artifact and spell-sound errors, modest unit/artifact/spell adjustments, and more varied dungeon encounters. This is a possible third gameplay profile, not something to stack blindly onto GS5R3.

### GSZero+ / GS5R3+

A later GS5-derived variant that changes AI lord classes and further increases difficulty. It is less established than the original GS5R3 and should be treated as a separate profile.

## Save policy

- Start new campaigns after changing `gs.mpq` or `pic.mpq`.
- Do not move saves between profiles unless compatibility is explicitly tested.
- Keep a known save at a reproducible scene for smoke testing, but do not treat it as a gameplay save.
