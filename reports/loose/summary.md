# Loose-file inventory

Produced by `lom-asset-viewer --loose-inventory INSTALL_ROOT PROFILE_LABEL`, one table per
installed profile, rooted at each profile's
`Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition`.
Every file under that root that is not inside an `.mpq` is a row. Measured 2026-09-18, regenerated
2026-09-19.

`magic` is decided by leading bytes alone; `probe_kind` is the verdict of the existing
`asset::probe`, which consults the extension for several formats. Both are recorded so that
disagreement is visible rather than reconciled. See `docs/loose-files.md` for what each
column establishes and what it does not.

A file that could not be read is a row carrying its error in `probe_error`, not a missing
row: the `gs5r3` profile is played while it is measured and its saves move underneath the
walk. There are currently no such rows.

The `development` profile moved too, between the two runs: its `lom.cfg` has a new digest and one
more help-panel flag cleared, with an mtime of 2026-09-19. Nothing in this repository writes inside
`~/Applications`; something ran that profile.

## Totals

| profile | app bundle | files | bytes |
| --- | --- | ---: | ---: |
| `baseline` | `Steambuild 32 64bit DXVK.app` | 467 | 581,035,931 |
| `development` | `Lords of Magic Development.app` | 467 | 581,035,931 |
| `gs5r3` | `Lords of Magic GS5R3.app` | 496 | 663,783,409 |
| `patch302` | `Lords of Magic 3.02.app` | 475 | 653,356,763 |

497 distinct relative paths across the four profiles; 467 of them appear in all four.

## By magic signature

| signature | `baseline` | `development` | `gs5r3` | `patch302` |
| --- | ---: | ---: | ---: | ---: |
| `ascii-text` | 21 | 21 | 29 | 23 |
| `asura-container` | 1 | 1 | 1 | 1 |
| `iff-pbm` | 1 | 1 | 1 | 1 |
| `lom-serialised` | 6 | 6 | 11 | 8 |
| `mpq-archive` | 5 | 5 | 7 | 7 |
| `ms-dos-executable` | 12 | 12 | 12 | 12 |
| `smacker-video` | 23 | 23 | 23 | 23 |
| `unrecognised` | 355 | 355 | 369 | 357 |
| `wave-audio` | 42 | 42 | 42 | 42 |
| `zip-archive` | 1 | 1 | 1 | 1 |

## By `asset::probe` verdict

| kind | `baseline` | `development` | `gs5r3` | `patch302` |
| --- | ---: | ---: | ---: | ---: |
| `asura-text` | 1 | 1 | 1 | 1 |
| `game-script` | 2 | 2 | 2 | 2 |
| `iff-pbm` | 1 | 1 | 1 | 1 |
| `legend-scenario` | 8 | 8 | 8 | 8 |
| `map-component` | 337 | 337 | 337 | 337 |
| `map-grid` | 1 | 1 | 1 | 1 |
| `map-scenario` | 8 | 8 | 20 | 8 |
| `mpq-archive` | 5 | 5 | 7 | 7 |
| `portable-executable` | 12 | 12 | 12 | 12 |
| `smacker-video` | 23 | 23 | 23 | 23 |
| `text` | 19 | 19 | 27 | 22 |
| `unknown` | 8 | 8 | 15 | 11 |
| `wave-audio` | 42 | 42 | 42 | 42 |

## What the existing probe could not classify

A bounded negative needs a count, not a silence. These are every row whose `probe_kind` is
`unknown`, listed rather than summarised because the list is short enough to read.

Two kinds came off this list on 2026-09-19: `.asr` now probes as `asura-text` and `.map` as
`map-grid`. `English/custldr/0templdr.ldr` stays on it -- the file is identified and its outer
container is read out of `savedefaultarmy`, but no parser is written for it. See
`docs/loose-files.md`.

| profile | path | extension | magic |
| --- | --- | --- | --- |
| `baseline` | `English/Shaders/shader-package.zip` | `zip` | `zip-archive` |
| `baseline` | `English/lom.cfg` | `cfg` | `unrecognised` |
| `baseline` | `English/savegame/combat.sav` | `sav` | `lom-serialised` |
| `baseline` | `English/savegame/experience.sav` | `sav` | `lom-serialised` |
| `baseline` | `English/savegame/magic.sav` | `sav` | `lom-serialised` |
| `baseline` | `English/savegame/merc.sav` | `sav` | `lom-serialised` |
| `baseline` | `English/savegame/quickstart` | `-` | `lom-serialised` |
| `baseline` | `English/savegame/temple.sav` | `sav` | `lom-serialised` |
| `development` | `English/Shaders/shader-package.zip` | `zip` | `zip-archive` |
| `development` | `English/lom.cfg` | `cfg` | `unrecognised` |
| `development` | `English/savegame/combat.sav` | `sav` | `lom-serialised` |
| `development` | `English/savegame/experience.sav` | `sav` | `lom-serialised` |
| `development` | `English/savegame/magic.sav` | `sav` | `lom-serialised` |
| `development` | `English/savegame/merc.sav` | `sav` | `lom-serialised` |
| `development` | `English/savegame/quickstart` | `-` | `lom-serialised` |
| `development` | `English/savegame/temple.sav` | `sav` | `lom-serialised` |
| `gs5r3` | `English/Shaders/shader-package.zip` | `zip` | `zip-archive` |
| `gs5r3` | `English/_vanilla_backup/lom.cfg` | `cfg` | `unrecognised` |
| `gs5r3` | `English/custldr/0templdr.ldr` | `ldr` | `unrecognised` |
| `gs5r3` | `English/lom.cfg` | `cfg` | `unrecognised` |
| `gs5r3` | `English/savegame/Merlin I` | `-` | `lom-serialised` |
| `gs5r3` | `English/savegame/combat.lom` | `lom` | `lom-serialised` |
| `gs5r3` | `English/savegame/combat.sav` | `sav` | `lom-serialised` |
| `gs5r3` | `English/savegame/endturn.lom` | `lom` | `lom-serialised` |
| `gs5r3` | `English/savegame/experience.sav` | `sav` | `lom-serialised` |
| `gs5r3` | `English/savegame/lastsave.lom` | `lom` | `lom-serialised` |
| `gs5r3` | `English/savegame/magic.sav` | `sav` | `lom-serialised` |
| `gs5r3` | `English/savegame/merc.sav` | `sav` | `lom-serialised` |
| `gs5r3` | `English/savegame/quickstart` | `-` | `lom-serialised` |
| `gs5r3` | `English/savegame/temple.lom` | `lom` | `lom-serialised` |
| `gs5r3` | `English/savegame/temple.sav` | `sav` | `lom-serialised` |
| `patch302` | `English/Shaders/shader-package.zip` | `zip` | `zip-archive` |
| `patch302` | `English/_vanilla_backup/lom.cfg` | `cfg` | `unrecognised` |
| `patch302` | `English/lom.cfg` | `cfg` | `unrecognised` |
| `patch302` | `English/savegame/Merlin I` | `-` | `lom-serialised` |
| `patch302` | `English/savegame/combat.sav` | `sav` | `lom-serialised` |
| `patch302` | `English/savegame/experience.sav` | `sav` | `lom-serialised` |
| `patch302` | `English/savegame/lastsave.lom` | `lom` | `lom-serialised` |
| `patch302` | `English/savegame/magic.sav` | `sav` | `lom-serialised` |
| `patch302` | `English/savegame/merc.sav` | `sav` | `lom-serialised` |
| `patch302` | `English/savegame/quickstart` | `-` | `lom-serialised` |
| `patch302` | `English/savegame/temple.sav` | `sav` | `lom-serialised` |

## Rows whose extension disagrees with their leading bytes

None, among the extensions this sweep has a magic rule for
(`.wav`, `.smk`, `.mpq`, `.zip`, `.exe`, `.dll`, `.snp`, `.lbm`).
Extensions with no such rule are counted as absences, not as conflicts;
`docs/loose-files.md` names the ones where extension and content pull apart anyway.

## Files present in only some profiles

| path | `baseline` | `development` | `gs5r3` | `patch302` |
| --- | :-: | :-: | :-: | :-: |
| `English/GS5R3 Contributors.txt` | - | - | yes | - |
| `English/GS5R3 README.txt` | - | - | yes | - |
| `English/_vanilla_backup/ddraw.ini` | - | - | yes | yes |
| `English/_vanilla_backup/gs.mpq` | - | - | yes | yes |
| `English/_vanilla_backup/lom.cfg` | - | - | yes | yes |
| `English/_vanilla_backup/pic.mpq` | - | - | yes | yes |
| `English/_vanilla_backup/settings.cfg` | - | - | yes | yes |
| `English/artifact.log` | - | - | yes | - |
| `English/combat.log` | - | - | yes | - |
| `English/custldr/0templdr.ldr` | - | - | yes | - |
| `English/gs5r.cfg` | - | - | yes | - |
| `English/lomse302.htm` | - | - | - | yes |
| `English/map/Avaeron II-128.scn` | - | - | yes | - |
| `English/map/Avaeron III-128.scn` | - | - | yes | - |
| `English/map/Avaeron IV-128.scn` | - | - | yes | - |
| `English/map/Avaeron128.scn` | - | - | yes | - |
| `English/map/Corlis.scn` | - | - | yes | - |
| `English/map/Denmor64.scn` | - | - | yes | - |
| `English/map/Giganta256.scn` | - | - | yes | - |
| `English/map/Kelmor32.scn` | - | - | yes | - |
| `English/map/Penara III-128.scn` | - | - | yes | - |
| `English/map/Penara.scn` | - | - | yes | - |
| `English/map/Roggow IV.scn` | - | - | yes | - |
| `English/map/Temora.scn` | - | - | yes | - |
| `English/savegame/Merlin I` | - | - | yes | yes |
| `English/savegame/combat.lom` | - | - | yes | - |
| `English/savegame/endturn.lom` | - | - | yes | - |
| `English/savegame/lastsave.lom` | - | - | yes | yes |
| `English/savegame/temple.lom` | - | - | yes | - |
| `English/thief.log` | - | - | yes | - |
