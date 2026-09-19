# Loose files

Everything on disk in an installed profile that is **not** inside an `.mpq`.

This repository's coverage claim — that every file format outside the executable is decoded — rested
entirely on the archive work. It was never true of the loose tree. `English/Wav/`, `English/smk/`,
`English/Shaders/`, `English/Text/` and the two install-root `.cfg` files had never been enumerated,
and two of them had no parser and no recorded layout at all. This page closes that gap and says
plainly where it is still open.

Every claim below carries its evidence class: **Observed in a local binary** (read out of
`lomse.exe`), **Observed in the corpus** (read off the installed files), **Observed in gameplay**,
**Documented**, **Inferred**, **Refuted**.

Measured **2026-09-18** on the four installed profiles. Reports:
[`reports/loose/summary.md`](../reports/loose/summary.md),
`reports/loose/inventory-{baseline,development,gs5r3,patch302}.tsv`,
[`reports/loose/config-fields.tsv`](../reports/loose/config-fields.tsv).

## Reproducing it

```sh
ROOT="$HOME/Applications/Steambuild 32 64bit DXVK.app/Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition"
lom-asset-viewer --loose-inventory "$ROOT" baseline > reports/loose/inventory-baseline.tsv
lom-asset-viewer --loose-config "$ROOT/English/lom.cfg"
lom-asset-viewer --loose-config "$ROOT/English/settings.cfg"
```

Both `--loose-config` parsers re-encode; the CLI prints `round-trips` so a partial read is visible
rather than silent.

## What the sweep covers, and what it does not

**The boundary is deliberate and it is not the whole app bundle.** The walk is rooted at each
profile's `steamapps/common/Lords of Magic Special Edition`. Counted and excluded above that root:
roughly **16,760 files per profile** belonging to the Wineskin wrapper, the Wine prefix
(`windows/`, `users/`, `ProgramData/`) and the Steam client itself — 17,230 files in the baseline
bundle against 467 in the game tree. Those are not game data and were never the gap; saying so with
a number is the difference between a boundary and an omission.

| profile | app bundle | files | bytes |
| --- | --- | ---: | ---: |
| `baseline` | `Steambuild 32 64bit DXVK.app` | 467 | 581,035,931 |
| `development` | `Lords of Magic Development.app` | 467 | 581,035,931 |
| `gs5r3` | `Lords of Magic GS5R3.app` | 496 | 663,783,409 |
| `patch302` | `Lords of Magic 3.02.app` | 475 | 653,356,763 |

497 distinct relative paths across the four; 467 shared by all four.

**Observed in the corpus.** The `gs5r3` profile was being played while this was measured — its
`combat.log`, `artifact.log` and `savegame/*.lom` carry mtimes inside the measurement window. Its
rows are a snapshot of live state, not of a shipped tree, and they will not reproduce after another
session. The other three profiles are stable.

## Two classifiers, recorded side by side

Each row carries a `magic` column decided by leading bytes alone and a `probe_kind` column from the
existing `asset::probe`, which consults the file extension for `.gs`, `.imp`, `.til`, `.scn`,
`.lgd`, `.smp`, `.txt` and `.url`. Inside an archive whose members were named from a recovered
listfile that is reasonable. A sweep looking for what nobody named cannot let the extension decide
the answer, so the two verdicts are recorded separately and never reconciled in the data.

| signature | baseline | development | gs5r3 | patch302 |
| --- | ---: | ---: | ---: | ---: |
| `unrecognised` | 355 | 355 | 369 | 357 |
| `wave-audio` | 42 | 42 | 42 | 42 |
| `smacker-video` | 23 | 23 | 23 | 23 |
| `ascii-text` | 21 | 21 | 29 | 23 |
| `ms-dos-executable` | 12 | 12 | 12 | 12 |
| `lom-serialised` | 6 | 6 | 11 | 8 |
| `mpq-archive` | 5 | 5 | 7 | 7 |
| `asura-container` | 1 | 1 | 1 | 1 |
| `iff-pbm` | 1 | 1 | 1 | 1 |
| `zip-archive` | 1 | 1 | 1 | 1 |

The 355 `unrecognised` rows in the baseline are not a mystery, and the breakdown says so: 337 `.smp`
map components, 8 `.scn` scenarios, 8 `.lgd` legend scenarios, `lom.cfg`, and `map/e3map2.map`.
Those formats open with a bare `u32` and have no magic to find; the first three are decoded
elsewhere in this repository. **Observed in the corpus.**

### Files the existing probe cannot classify — the count

Ten in the baseline and development profiles, 13 in `patch302`, 17 in `gs5r3`. The full list is in
`reports/loose/summary.md`; the baseline ten are:

| path | why |
| --- | --- |
| `English/savegame/combat.sav` | `LS_VER_` serialisation; `probe` has no rule |
| `English/savegame/experience.sav` | same |
| `English/savegame/magic.sav` | same |
| `English/savegame/merc.sav` | same |
| `English/savegame/temple.sav` | same |
| `English/savegame/quickstart` | same, and no extension at all |
| `English/lom.cfg` | now parsed — by `loose::LomConfig`, not by `probe` |
| `English/Shaders/shader-package.zip` | zip; `probe` has no rule |
| `English/Text/Menu/Menu_En.asr` | Asura container; `probe` has no rule |
| `English/map/e3map2.map` | see below |

### Extension against content

**Observed in the corpus.** Zero rows disagree among the extensions the sweep has a magic rule for
(`.wav`, `.smk`, `.mpq`, `.zip`, `.exe`, `.dll`, `.snp`, `.lbm`). That is a narrow negative, so the
cases where extension and content genuinely pull apart are named here instead of being counted as
conflicts:

- **`English/gs5r.cfg`** (GS5R3 only) is not configuration in the sense the other `.cfg` files are.
  It is GameScript source — `/combat_scrolling 4 def`, `/logs_enabled? true def`, with `;` comments
  and CR line ends. **Observed in the corpus.**
- **`English/savegame/quickstart`** and **`English/savegame/Merlin I`** have no extension and are
  full `LS_VER_` game states. **Observed in the corpus.**
- **`.sav` and `.lom` are the same format.** The expectation going in was that `LS_VER_` would also
  head the `.lgd` legend scenarios, which would have made the signature useless for telling shipped
  content from saved state. **Refuted in the corpus**: all eight `.lgd` files open with a bare
  `u32`. The header appears only on the six shipped starting states in `savegame/` and on the `.lom`
  files a session writes — so shipped starting state and player save really are one format, which
  `docs/save-format.md` already treats them as.

## Things this repository had not recorded

Each was absent from `docs/`, `reports/`, `tools/` and `README.md` before this sweep.

### `English/Text/Menu/Menu_En.asr` — an Asura container

375 bytes, magic `Asura   ` (eight bytes, three trailing spaces) followed by the block tag `HTXT`.
Asura is Rebellion's engine container format; this file belongs to the Steam-era `LOMLauncher.exe`,
not to `lomse.exe`. It holds the launcher's five UI strings.

**Observed in the corpus**, by two exact length equations rather than by eye:

- Five string records follow a `u32` count of 5. Each record is `u32` hash, `u32` character count,
  then UTF-16LE text. The five counts are 5, 8, 31, 6, 40 and the five strings are `Play`,
  `Support`, `Lords of Magic Special Edition`, `Error` and
  `Error launching the game: (0x%08x) - %s` — 4, 7, 30, 5 and 39 characters. Every count is the
  character count **including** the terminator. Five for five.
- A `Menu` block follows, with a `u32` of 87 and then 87 bytes of NUL-separated ASCII keys:
  `LAUNCHER_PLAY`, `LAUNCHER_SUPPORT`, `LAUNCHER_GAME_TITLE`, `LAUNCHER_ERROR_TITLE`,
  `LAUNCHER_ERROR`. Their lengths with terminators are 14 + 17 + 20 + 21 + 15 = 87. Exact.

**Not determined**: the meaning of the `u32`s at payload offsets `0x20` (`0x0033155f`) and `0x24`
(`0xb4`), the hash function, and whether the trailing NUL run is alignment padding or a field. No
parser is written for this; it is documented, not implemented.

### `English/map/e3map2.map` — a map component the map pass never saw

**Observed in the corpus.** 32,780 bytes. `--describe-map` refuses it (`unsupported map cell depth
391`), and `--scan-map-dir` skips it because it dispatches on the `.smp`/`.scn`/`.lgd` extensions.
It is therefore outside the "all 365 installed map/scenario/component files parse" claim, which
counts 337 `.smp` + 20 `.scn` + 8 `.lgd` in the GS5R3 profile.

Its first three words are `64`, `64`, `8`. A `.smp` opens with a **version** word (`0x6f` in
`91gauntlet.smp`) and then width, height, depth — the same three fields, one word later.
`12 + 64 × 64 × 8 = 32,780` accounts for the file exactly, sourced as three terms and not as a
total: a 12-byte header of three words, 4,096 cells, 8 bytes per cell.

**Inferred**, not observed: that this is an earlier or variant map component that predates the
version word. One file is not a format. It is not decoded further here.

### `English/Wav/` — 42 files, 233 MB

**Observed in the corpus.** All 42 are RIFF/WAVE, and all 42 are the *same* format: PCM
(`encoding=1`), 2 channels, 22,050 Hz, 8 bits per sample. Three groups:

- `Menu_Music.wav`, `Valkyries.wav` at the top level;
- `Music/` — eight faith themes (`M_Air`, `M_Chaos`, `M_Death`, `M_Earth`, `M_Fire`, `M_Life`,
  `M_Order`, `M_Water`) plus 29 win/lose/neutral stings;
- `lou_scenarios/` — 11 scenario intro and description tracks.

Worth flagging: `lou_scenarios/` contains **`German_Intro.wav` and `german_desc.wav`** in an English
install. **Not determined** whether anything loads them.

### `English/smk/` — 23 files, 177 MB

**Observed in the corpus.** All 23 are Smacker `SMK2`. `Intro.smk`, `Credits.smk`, `IMPTITLE.SMK`,
`legend/legends.smk`; `Balkoth/` (2); `PlayerLose/` (7 — Air, Chaos, Earth, Fire, Life, Order,
Water, with no Death); `legend/Win_Lose/` (10 — Death, Earth, Fire, Hid, Order, each win and lose).
The asymmetry between the seven `PlayerLose` faiths and the five `legend/Win_Lose` faiths is
recorded, not explained.

### Install-root files never mentioned

- **`English/LANGUAGE.INF`** — 9 bytes, the literal `[Ident]\r\n` and nothing else.
- **`English/Sierra.inf`** — the 1998 Sierra installer manifest: `ProductID=70582`,
  `Version=3.0.0.0`, `PatchVersion=3.0.0.1`, `ShortTitle=LOMSE`, `UpdateAfter=10-1-1998`, a
  `[System Test]` section asking for a Pentium-60 and 640×480, and a `[Demos]` section offering
  Caesar 3. Inert, but it is the only place the shipped product and patch version numbers are
  written down.
- **`installscript.vdf`** — the Steam install script, at the install root rather than under
  `English/`. The only file outside `English/` in any profile.
- **`English/Shaders/`** — 11 `.glsl`, two `.glsl.pass1`, `readme.txt` and `shader-package.zip`.
  These belong to `ddraw.dll` (cnc-ddraw), not to the game.
- **`English/Launcher/dbghelp.dll`** — shipped beside `LOMLauncher.exe`.
- **`English/custldr/0templdr.ldr`** (GS5R3 only) — 3,982 bytes, binary, 2,738 of them zero and 834
  of them `0xFF`. **Not determined**: anything about its structure.

## `settings.cfg`

**Observed in the corpus**, all four profiles. 513 bytes, ASCII, 23 records.

- One `KEY VALUE` record per line. Key, one `0x20`, value.
- Each record is terminated by a bare **`CR` (`0x0d`)**. Not `CRLF`. There is no trailing `LF` and
  no final newline beyond the last record's `CR`.
- All 23 observed values are decimal integers, two of them negative (`TOOL_TIP_TRANSLUCENCY -1`
  and `USE_DIRECTX_BLIT -1`).

The keys, in file order: `TOOL_TIP_TRANSLUCENCY`, `TOOL_TIP_DELAY`, `LOCAL_MOUSE_SCROLL_SPEED`,
`REGION_MOUSE_SCROLL_SPEED`, `WORLD_MOUSE_SCROLL_SPEED`, `COMBAT_MOUSE_SCROLL_SPEED`,
`KB_MAP_SCROLL_SPEED`, `KB_COMBAT_SCROLL_SPEED`, `COMBAT_DISPLAY_HEALTH_BAR`,
`COMBAT_DISPLAY_SMALL_HEALTH_BAR`, `COMBAT_DISPLAY_HALO`, `COMBAT_DISPLAY_FLAG`,
`COMBAT_DISPLAY_AURA`, `MILITARY_SELECTION_MODE`, `GAME_SPEED`, `COMBAT_SPEED`, `BUILDING_SPEECH`,
`MUSIC_VOLUME`, `SOUND_FX_VOLUME`, `SPEECH_VOLUME`, `AMBIENT_VOLUME`, `USE_DIRECTX_BLIT`,
`CENTER_MOVE`.

`loose::SettingsConfig` parses and re-encodes all four installed files byte for byte, with no
unparsed records.

### It is written by something, during play

**Observed in gameplay.** The `patch302` profile's live `settings.cfg` differs from that profile's
own `_vanilla_backup/settings.cfg` in exactly two records — `KB_MAP_SCROLL_SPEED` 100 → 50 and
`KB_COMBAT_SCROLL_SPEED` 10 → 135 — and in nothing else. GS5R3 -- the only other profile with a
`_vanilla_backup/` -- still matches its own backup byte for byte; `baseline` and `development` have
no backup to compare against. So the running game does write this file, and it writes the whole file rather than patching
a line: the two edits happen to cancel in length (`100`→`50` loses a character, `10`→`135` gains
one), which is why the size is still 513.

### What is not determined: which component writes it

The literal `settings.cfg` and all 23 key names are **absent** from:

- all 467 loose files of the baseline install other than `settings.cfg` itself — including
  `lomse.exe`, `LOMLauncher.exe`, `ddraw.dll`, `storm.dll`, `goggame.dll` and both `.snp` providers;
- all 1,688 `gs.mpq` members, extracted and searched;
- all 1,218 `special.mpq` members, extracted and searched.

That is 3,373 objects searched. **What the search could not reach**, stated rather than implied:
the 1,071 `pic.mpq` members, the 3,600 `imp.mpq` members and the 1,880 `sndfx.mpq` members (media
archives, not searched); the Wine prefix's own DLLs; and any filename or key assembled at run time
from pieces, which no substring search can find.

One cross-check that does not share that mechanism: the committed GameScript vocabulary
(`reports/gs/vocabulary-vanilla.tsv`, 14,082 names, built by lexing the script corpus rather than by
byte search) contains none of the 23 keys either. That narrows it but does not close it, because the
vocabulary indexes *names* and these keys would appear as string literals.

A second lead was chased and **refuted**: `lomse.exe` contains a `"%s %d"` format string, the right
shape for these records, but its single cross-reference at `0x0049b099` sits in an
`sscanf("#define %s %d")` loop that reads IMP `.H` headers. It is not the settings writer.

## `lom.cfg`

**Observed in a local binary** (`lomse.exe`, baseline profile). 160 bytes, binary, little-endian,
one record, variable length.

The GameScript operators `saveconfig` (`0x00487570`) and `loadconfig` (`0x00487580`) are
two-instruction thunks: `mov ecx, 0x5aa12c` and a tail jump to `0x00487220` and `0x00487360`. Those
two functions are the only code in the image that names the string `"lom.cfg"`. One opens it `"wb"`
and issues a fixed sequence of `fwrite` calls; the other opens it `"rb"` and issues the matching
`fread` calls. The layout below is that sequence, and reader and writer agree field for field —
**two instruments, not one instrument read twice**.

The *names* come from a third place that never looked at this file: the repository's own
`reports/natives/operator-bodies.tsv`, recovered by walking operator bodies. Every global the writer
stores is referenced by an operator whose name states what it holds.

| offset | size | field | named by |
| ---: | ---: | --- | --- |
| `0x00` | 4 | music volume | global `0x586770`; `getmusicvolume`, `presetmusicvolume` |
| `0x04` | 4 | sound-fx volume | global `0x586774`; `getsoundfxvolume`, `presetsoundfxvolume` |
| `0x08` | 4 | speech volume | global `0x586778`; `getspeechvolume`, `presetspeechvolume` |
| `0x0c` | 4 | ambient volume | global `0x58677c`; `getambientvolume`, `presetambientvolume` |
| `0x10` | 4 | help-panel count *N* | list object `0x5aa1f0`; `maxhelppanels` |
| `0x14` | 4×*N* | help-panel check flags | `addhelppanel`, `togglehelpcheck`, `getcheckmarkstate`, `uncheckallhelppanels` |
| `0x14+4N` | 4 | Balkoth kill counter | global `0x5aa154`; `balkothdeathcounter`, `cheatbalkoth`, `getbalkothkillcounter` |
| +4 | 4 | centre-on-movement | global `0x5aa254`; `setcenteronmovement` |
| +4 | 16 | install GUID | `ole32!CoCreateGuid` |
| +16 | 4 | building-speech flag | global `0x586784`; `getbuildingspeechflag`, `setbuildingspeechflag` |
| +4 | 4 | show-completed-quests | global `0x5aa258`; `get_show_completed_quests`, `set_show_completed_quests` |
| +4 | 4 | **unnamed** | global `0x5d20bc` |

With *N* = 26 that is 20 + 104 + 36 = 160, every term sourced as a field the writer emits.

### The help-panel vector

**Observed in a local binary.** The container at `0x5aa1f0` is `{ capacity, count, records }` with
**12-byte** records. `addhelppanel` appends one, setting `[record+0]` to the id the script passed
and `[record+4]` to 1. Only `[record+4]` is persisted. `uncheckallhelppanels` sets exactly that word
to 1 across every record, and the reader sets it to 1 for every record the file does not cover — so
1 is the default and the stored value is the panel's check state.

**Observed in the corpus**, and this is where the identification earns its keep. The pristine
`_vanilla_backup/lom.cfg` in both modded profiles is byte-identical and has all 26 flags at 1 except
index 0. After play, flags only ever go 1 → 0:

| profile | flags | state |
| --- | --- | --- |
| pristine (`_vanilla_backup`, both modded profiles) | `0,1,1,1,…,1` | as shipped |
| `baseline`, `development` | `0,1,1,1,…,1` | unchanged from pristine |
| `gs5r3` | `0,0,1,0,1,1,…,1` | two more cleared |
| `patch302` | `0,0,0,0,1,0,0,0,0,0,0,0,0,0,0,1,1,0,1,0,0,0,0,1,1,1` | most cleared |

Monotone 1 → 0 with play is what a "don't show this again" checkbox does.

**Not determined: why the count is 26.** Stated before looking: if the count were simply the number
of `addhelppanel` calls in the scripts, the vanilla corpus should mention `addhelppanel` 26 times.
It mentions it **40** times (`reports/gs/vocabulary-vanilla.tsv`), and GS5R3's fork mentions it 26.
The simple explanation is therefore **refuted**; the count is the runtime length of the list, and
what prunes it is not established.

### The GUID

**Observed in a local binary.** When the 16-byte read comes up short, the reader calls the
`ole32.dll!CoCreateGuid` import at `0x0054d34c` on that address and immediately saves. The bytes are
a locally generated GUID, not a shipped constant.

**Observed in the corpus**, with the expected values stated first. If the GUID were shipped, all
four profiles would carry the same one; if generated per prefix, the three copy-on-write clones of
one prefix would share and the odd one out would differ. `baseline`, `development` and `gs5r3` all
carry `519A38B2-5C54-444F-9F8E-E7B9C36C9002` — which is also what *both* pristine backups carry —
and `patch302` carries `3B0CD6D2-2612-408F-85DD-8B34C4C8D39F`. **Not determined**: what made the
3.02 profile regenerate, given its file is the full 160 bytes and the observed regeneration path is
a short read.

**Bounded negative.** Both GUIDs were searched for across **all 467 files** of the baseline tree.
`519A38B2-...` occurs in exactly one place -- `English/lom.cfg` itself -- and `3B0CD6D2-...` occurs
nowhere, which is consistent with generation rather than shipping. What that search cannot reach:
the members of the five archives (9,369 of them), the Wine prefix, and any value the code builds
rather than stores. The cross-check that does not share its mechanism is the `CoCreateGuid` call
site itself, which is a fact about code rather than about bytes on disk.

### The unnamed trailing word

Global `0x5d20bc`. No operator in the recovered table names it; `loadconfig` and `saveconfig`
reference it only as part of this file. Both default it to 1 on a short read. Observed values: `1`
in `baseline` and `development`, `-1` (`0xffffffff`) in `gs5r3` and `patch302`.

`-1` is how this codebase spells true elsewhere — `settings.cfg` ships `TOOL_TIP_TRANSLUCENCY -1`
and `USE_DIRECTX_BLIT -1`. That is a shape, not a name, so the field is carried verbatim as
`LomConfig::unnamed_trailing_word` and given no meaning.

### The volumes do not agree with `settings.cfg`

**Observed in the corpus.** All four volume words are `0` in all four profiles, while every
profile's `settings.cfg` simultaneously records `MUSIC_VOLUME 100`, `SOUND_FX_VOLUME 100`,
`SPEECH_VOLUME 100`, `AMBIENT_VOLUME 100`. The operators that write these globals are named
`preset*` and the ones that read them `get*`, so the two files are evidently not storing the same
quantity. **Not determined**: which quantity either stores.

## What is not determined

Collected, so that nothing here reads as settled when it is not.

1. **What writes `settings.cfg`.** Established that something does, during play. Not established
   what, after searching 3,373 objects; three media archives and the prefix DLLs were out of reach.
2. **The meaning of `lom.cfg`'s trailing word** (global `0x5d20bc`).
3. **Why `lom.cfg` holds 26 help panels**, given 40 script mentions of `addhelppanel`.
4. **Why the 3.02 profile regenerated its GUID.**
5. **What the four `lom.cfg` volume words hold**, given they disagree with `settings.cfg`.
6. **`English/map/e3map2.map`** beyond its size equation. Not decoded, and one file is not a format.
7. **`English/custldr/0templdr.ldr`** (GS5R3) — nothing at all beyond its byte histogram.
8. **The Asura container** beyond two exact length equations: two header words, the hash function,
   and the trailing NUL run are all unexplained, and no parser exists.
9. **Whether the German audio in `Wav/lou_scenarios/` is reachable** from an English install.
10. **Anything above the game-tree root.** ~16,760 files per bundle were excluded by boundary, not
    examined. If the game writes state into the Wine prefix — registry hives, `users/` — this sweep
    did not look.

## Safety

The sweep is read-only and this work never wrote inside `~/Applications`. `English/map/` has no
backup; `loose::inventory` opens files for reading only and follows no symlinks out of the tree
(`symlink_metadata`, not `metadata`).
