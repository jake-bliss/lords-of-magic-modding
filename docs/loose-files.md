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

The count is profile-dependent and the qualification has to say which: GS5R3's `English/map/` holds
**366** files, and the other three profiles hold **354** (they have 8 `.scn` rather than 20). The
profile-independent statement is the useful one -- **every profile holds exactly one file in
`English/map/` that the map pass never offers to the parser**, and it is this one.

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

### The bare-CR trap, which nearly shipped in the parser

Worth recording because it is the **third** format in this work where a bare `CR` was the hazard —
after `gs5r.cfg` and the `ddraw.ini` rewrite already noted in the macOS runbook.

Open `settings.cfg` in any Windows editor and it comes back `CRLF`. Splitting on `CR` then leaves a
stray `LF` at the head of every record but the first, so `CENTER_MOVE 0` arrives as the key
`"\nCENTER_MOVE"`. The first version of this parser accepted that silently: `get` returned `None`
for 22 of the 23 keys with no error, and — because the mangling is **lossless** — the round-trip
check still reported success. A detector blind to the failure it was meant to catch.

The remaining detail is what makes it nasty. **Exactly one key still resolves**, the first, because
only it has no `LF` in front of it. Anyone spot-checking `TOOL_TIP_TRANSLUCENCY` sees a working
parser.

The parser now requires a key and value to contain no control characters, so a `CRLF` file surfaces
as `unparsed` records instead of as silently missing keys. `--loose-config` refuses such a file
outright. Pinned as a test.

### It is written by something, during play

**Observed in gameplay.** The `patch302` profile's live `settings.cfg` differs from that profile's
own `_vanilla_backup/settings.cfg` in exactly two records — `KB_MAP_SCROLL_SPEED` 100 → 50 and
`KB_COMBAT_SCROLL_SPEED` 10 → 135 — and in nothing else. GS5R3 — the only other profile with a
`_vanilla_backup/` — still matches its own backup byte for byte; `baseline` and `development` have
no backup to compare against. So the running game does write this file.

The file is still 513 bytes because the two edits happen to cancel in length (`100`→`50` loses a
character, `10`→`135` gains one). An earlier version of this page read that as evidence that the
writer rewrites the whole file rather than patching a line. It is not: two changed values establish
the resulting bytes and nothing about how they got there. **The write strategy is not determined.**

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

The extraction behind that was re-verified rather than trusted: 1,688 members listed, 1,688
extracted, 0 failures, 4,965,502 bytes. An earlier run left a stale scratch directory and reported a
member count that did not match what it had extracted, which is exactly how a negative gets
published from a broken instrument.

Three cross-checks that do not share the byte-search mechanism:

1. **The GameScript vocabulary** (`reports/gs/vocabulary-vanilla.tsv`, 14,082 names, built by lexing
   rather than by byte search) contains none of the 23 keys. It indexes *names*, though, and these
   would be string literals, so this narrows without closing.
2. **The recovered operator table**, which a substring search structurally cannot exploit. Every one
   of the 23 keys has a script-callable counterpart: `settooltipdelay`, `setcombatscrollpixels`,
   `coarsescrollpixels`/`finescrollpixels`/`limitscrollinginpixels`, `setmilitaryselectionmode`,
   `setmusicvolume`/`setsoundfxvolume`/`setspeechvolume`/`setambientvolume`,
   `setbuildingspeechflag`, `setcenteronmovement`. So these settings **are** script-reachable, and a
   script-level writer is mechanically possible: there is a generic `file` operator (`0x004cc0f0`)
   that calls `fopen`, and `savedefaultarmy` and `trace` demonstrate script-driven file writing.
3. **The scripts use a different vocabulary for the same options.** The member holding
   `next_combat_display_option` spells its constants `OPTION_HEALTH_BAR_ALWAYS`,
   `OPTION_SMALL_HEALTH_BAR_WHEN_ENEMY`, `OPTION_PLAYER_HALO_WHEN_SELECTED` -- not
   `COMBAT_DISPLAY_HEALTH_BAR`. And the substring `.cfg` appears in **no** `gs.mpq` member at all,
   so no script names any configuration file.

The lead was followed rather than filed as "not pursued", and it did not close the question. What it
establishes is narrower and still worth having: the settings are script-reachable, script-level file
writing exists, and the key strings are in neither the engine image nor the script corpus.

A further lead was chased and **refuted**: `lomse.exe` contains a `"%s %d"` format string, the right
shape for these records, but its single cross-reference at `0x0049b099` sits in an
`sscanf("#define %s %d")` loop that reads IMP `.H` headers. It is not the settings writer.

## `lom.cfg`

**Observed in a local binary** (`lomse.exe`, baseline profile). Binary, little-endian, one record,
variable length; 160 bytes in all four installed profiles, though the recovered writer can also emit
a shorter form (below).

The GameScript operators `saveconfig` (`0x00487570`) and `loadconfig` (`0x00487580`) are
two-instruction thunks: `mov ecx, 0x5aa12c` and a tail jump to `0x00487220` and `0x00487360`. Those
two functions are the only code in the image that names the string `"lom.cfg"`. One opens it `"wb"`
and issues a fixed sequence of `fwrite` calls; the other opens it `"rb"` and issues the matching
`fread` calls. The layout below is that sequence.

**Reader/writer agreement is not confirmation, and an earlier version of this page called it "two
instruments".** It is not: an `fread` sequence and its matching `fwrite` in the same binary *must*
agree or the game would not load its own file. The agreement establishes the byte partition --
widths, order, no gaps -- and says nothing whatever about which slot means what. It is also not
fully independent of my reading: `saveconfig`'s rows in the committed field-access table cover only
four of the eleven fields (`+0x08`, `+0x28`, `+0xc8`, `+0xcc`), so the writer side rests on a local
disassembly the repository's own extractor has not reproduced.

The *names* come from somewhere that never looked at this file: the repository's own
`reports/natives/operator-bodies.tsv` and `reports/natives/state/operator-field-access.tsv`,
recovered by walking operator bodies. **The naming rule is: a field is named after the operator that
reaches the slot the engine's own *reader* writes it to.** Getting that rule slightly wrong --
naming a field after the address the *writer* reads it *from* -- is what produced the retracted
volume attribution below, so it is stated explicitly.

| offset | size | field | slot the reader fills | named by |
| ---: | ---: | --- | --- | --- |
| `0x00` | 4 | last music volume | `+0x18` = `0x5aa144` | `setlastaudiosettings` |
| `0x04` | 4 | last sound-fx volume | `+0x1c` = `0x5aa148` | `setlastaudiosettings` |
| `0x08` | 4 | last speech volume | `+0x20` = `0x5aa14c` | `setlastaudiosettings` |
| `0x0c` | 4 | last ambient volume | `+0x24` = `0x5aa150` | `setlastaudiosettings` |
| `0x10` | 4 | help-panel count *N* | `+0xc8` = `0x5aa1f4` | list object `0x5aa1f0`; `maxhelppanels` |
| `0x14` | 4x*N* | help-panel check flags | `+0xcc` records | `addhelppanel`, `togglehelpcheck`, `getcheckmarkstate`, `uncheckallhelppanels`, `onetimeonly` |
| `0x14+4N` | 4 | Balkoth kill counter | `+0x28` = `0x5aa154` | `balkothdeathcounter`, `cheatbalkoth`, `getbalkothkillcounter` |
| +4 | 4 | centre-on-movement | `+0x128` = `0x5aa254` | `setcenteronmovement` |
| +4 | 16 | install GUID | `+0x08` = `0x5aa134` | `ole32!CoCreateGuid` |
| +16 | 4 | building-speech flag | global `0x586784` | `getbuildingspeechflag`, `setbuildingspeechflag` |
| +4 | 4 | show-completed-quests | `+0x12c` = `0x5aa258` | `get_show_completed_quests`, `set_show_completed_quests` |
| +4 | 4 | used-DrawBlt flag | global `0x5d20bc` | `getuseddrawblt`, `setuseddrawblt` |

With *N* = 26 that is 20 + 104 + 36 = 160, every term sourced as a field the writer emits.

`loadconfig`'s ten recorded field accesses account for that layout exactly: `+0x08`, `+0x18`,
`+0x1c`, `+0x20`, `+0x24`, `+0x28`, `+0xc8`, `+0xcc`, `+0x128`, `+0x12c`.

### A second form, which the corpus does not contain

**Observed in a local binary.** `saveconfig` branches on the help-panel records pointer at `+0xcc`
(`test eax,eax; je` at `0x00487291`). When it is null it writes the four head words and then jumps
straight to the suffix, skipping *both* the count word and the vector -- a **52-byte** file.
`loadconfig` branches on the same pointer at `0x004873d6` and reads the shorter form back.

**Not observed in the corpus**: all four installed files carry the vector. `LomConfig` accepts both
and represents the absent vector as `None`, never as an empty one, because a file with no count word
(52 bytes) and a file whose count word is zero (56 bytes) are different files.

Worth recording as a fragility rather than as a feature: **which form the engine writes depends on
runtime state, not on anything in the file.** Nothing in a `lom.cfg` says which shape it is. The
parser can only tell them apart because `20 + 4N + 36 = 52` has no non-negative solution.

### Retracted: the four head words are not the live volumes

This page first named `0x00`-`0x0f` the music, sound-effects, speech and ambient **volumes**, on the
grounds that `saveconfig` emits them from `0x586770`-`0x58677c` and the operator table names those
globals `getmusicvolume` and friends. **Refuted by the committed tables.** `loadconfig`'s recorded
globals are `0x5572ec, 0x586784, 0x5a7d8c, 0x5a8080, 0x5aa12c, 0x5d20bc` -- `0x586770`-`0x58677c` do
not appear, and the extractor plainly does see direct statics in that body, since it caught
`0x586784` and `0x5d20bc` in the same 163 instructions. What `loadconfig` does read is config-object
`+0x18`-`+0x24`, and the one other operator among 1,906 that touches `0x5aa144`-`0x5aa150` is
`setlastaudiosettings`, which copies them into four consecutive sound-object slots at
`+0x1368`-`+0x1374`.

So the file stores the **last audio settings** -- a restore-from slot -- and the live volume globals
are only the writer's *source*.

### What that does and does not mean for a modder

The first version of this retraction said a modder patching `lom.cfg[0x00]` to change music volume
"changes nothing". **That is false, and it was the one sentence here anybody would have acted on.**
It also carried no evidence grade while every claim around it did. A wrong "changes nothing" is
worse than the wrong attribution it replaced, because the attribution was falsifiable by trying it
and the "changes nothing" tells you not to.

Scoped and graded properly:

- **Observed in the corpus.** Patching `lom.cfg[0x00]` does not change what `getmusicvolume`
  returns. That operator reads `0x586770`, and the loader never writes it -- `loadconfig`'s recorded
  globals do not include it.
- **Not determined.** Whether it changes the *applied* volume. `setlastaudiosettings` passes
  `0x5aa144` to `0x479a00`, which is the same helper `setmusicvolume` calls on the same sound
  object, writing the same slot `+0x1368`. So the path plausibly reaches the mixer, and
  `setlastaudiosettings` is mentioned once in each of the three script corpora, so it is reachable.
  This has **not** been tested in a running engine, and it is cheap to test.

### The channel mapping is observed, not inferred

An earlier version of this section retracted this too far, calling the slot-to-channel mapping an
inference from two agreeing orderings. It is read off names, and the evidence was already committed:

| operator | distinguishing call target | sound-object slot written |
| --- | --- | --- |
| `setmusicvolume` | `0x479a00` | `+0x1368` |
| `setsoundfxvolume` | `0x479b40` | `+0x136c` |
| `setspeechvolume` | `0x479c30` | `+0x1370` |
| `setambientvolume` | `0x479d10` | `+0x1374` |
| `setlastaudiosettings` | all four, in that order | all four |

The other targets those four operators share (`0x4d4550`, `0x5394a0`) are boilerplate. Two
independent columns agree, and both are names rather than orderings. **Observed in the corpus.**

What does remain local-disassembly-only is the narrow pairing: which of `0x5aa144`--`0x5aa150` feeds
which of those four calls. That comes from reading `setlastaudiosettings` by hand and has not been
reproduced by this repository's extractor.

### Resolved: why the audio words are zero

This was listed as an open question: the four words read `0` while `settings.cfg` says
`MUSIC_VOLUME 100`. The committed vocabulary answers it. **Observed in the corpus:**

| operator | vanilla | 3.02 | GS5R3 |
| --- | ---: | ---: | ---: |
| `presetmusicvolume` | 0 | 1 | 0 |
| `presetsoundfxvolume` | 0 | 1 | 0 |
| `presetspeechvolume` | 0 | 1 | 0 |
| `presetambientvolume` | 0 | 1 | 0 |
| `setlastaudiosettings` | 1 | 1 | 1 |

The operators that populate this quad are mentioned **zero** times in the vanilla and GS5R3 script
corpora and once each in 3.02; the operator that consumes it is mentioned once everywhere. A pathway
the scripts essentially never take leaves its slot at its initial value, so **zero is the expected
reading**, and the disagreement with `settings.cfg` is not a contradiction -- the two files were
never storing the same quantity. A mention is not an execution, so 3.02 reading zero despite its
four mentions is consistent with this too.

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
The simple explanation is therefore **refuted**, and no replacement is offered: "it is the runtime
length of the list" would have to explain 26 under two corpora that mention the operator 40 and 26
times, so that is not an explanation either.

What survives the refutation is the **layout**, which never depended on it. The count word is
`+0xc8` and the vector is the `+0xcc` records, established from the container's own
`{ capacity, count, records }` shape; the check-state reading rests on the monotone 1 -> 0 behaviour
above, not on the number 26.

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

### The trailing word: the used-DrawBlt flag

Also first recorded here as unnamed, and also resolved from artifacts already in the repository.
Global `0x5d20bc` is named by exactly two operators, `getuseddrawblt` (`0x004da350`) and
`setuseddrawblt` (`0x004da2a0`), and each lists that address as its **only** global -- which is as
clean as this naming rule gets. Both are called in all three script corpora (2/5/4 times).

Observed values: `1` in `baseline` and `development`, `-1` (`0xffffffff`) in `gs5r3` and
`patch302`. `-1` is how this codebase spells true elsewhere, and `settings.cfg` ships
`USE_DIRECTX_BLIT -1`. **Not determined**: whether the `lom.cfg` flag and that setting are the same
quantity. They are not simply equal -- the setting is `-1` in all four profiles and this field is
not -- so the resemblance is recorded and not acted on. The loader defaults the field to 1 after a
short read; the writer has no short-input path to be symmetric with.

### The method, since it worked twice

Both fields this page originally left unnamed were named without touching the game again, by joining
the file layout against `reports/natives/operator-bodies.tsv` on the global address. The file itself
can never say what a slot means; the operator table can, because someone already paid the cost of
walking 1,906 bodies. Where the join returns nothing, the field stays unnamed -- that is the rule
working, not the rule failing.

### Where `lom.cfg` and `settings.cfg` agree, and what that can and cannot prove

The volume disagreement is resolved above. Its other half was missing from this page: two settings
appear in **both** files, and in both cases they agree across all four profiles.

| `lom.cfg` field | `settings.cfg` key | all four profiles |
| --- | --- | --- |
| building-speech flag | `BUILDING_SPEECH` | `1` |
| centre-on-movement | `CENTER_MOVE` | `0` |

That agreement is the only check in this work bearing on **which slot holds what** rather than on
how the bytes divide up: one side is named by an operator recovered from the binary, the other by a
literal in a text file that knows nothing about the binary. It is pinned as a test.

**It is weak, and the limit is worth stating precisely.** Both pairs are constant across the corpus,
so each offers exactly one value. More generally, **six of the eleven `lom.cfg` fields read `0` in
all four profiles** -- the four audio words, the Balkoth counter and centre-on-movement -- and no
value-based check over this corpus can distinguish any of them from any other. That was measured,
not assumed, and the attribution matters:

- Swapping the used-DrawBlt word with `show_completed_quests` is caught by
  `the_committed_configuration_report_reproduces`, which re-derives every field from the installed
  file and names the one that moved (verified on `patch302`). An earlier version of this page
  credited the report-backed identity checks instead; those only notice once the report has been
  regenerated from the mutated parser, so the credit was misplaced.
- Swapping `balkoth_kill_counter` with `center_on_movement` is caught by **nothing**, and cannot be
  by any value comparison: both read `0` in all four profiles, so the re-derivation is `0 == 0`.

There is an untried instrument that would separate that pair, and it is not a value comparison. The
reader's two `fread`s are asymmetric: the Balkoth one is followed by a zero-on-short-read fixup at
`0x487455`, and the centre-on-movement one by a mirror write to the indexed global
`0x5a8080 + ([0x5a7d8c] << 10)` at `0x487480`. The writer is asymmetric the same way. That
distinguishes the two slots structurally rather than by what they happen to hold, and it is not
pinned by any test here.

Round-tripping cannot help here either. For `lom.cfg` the round-trip is a mathematical identity --
`to_bytes` emits in exactly the order `parse` reads -- so it holds for any input `parse` accepts,
including noise with a plausible count planted at offset `0x10`. It proves the byte partition is
total and ordered; it is blind to a permutation of names over equal-width slots, which is exactly
the defect that produced the retracted volume attribution. `SettingsConfig::round_trips` is a
different matter and can genuinely fail, because its encoder has to reconstruct separators.

## Resolved since the first draft

Recorded rather than quietly edited away. All three were closed from artifacts already committed in
this repository, without touching the game again, and the method is reusable.

- **What the four head words hold.** They are the *last audio settings*, not the live volumes; the
  first attribution was refuted by `loadconfig`'s own recorded globals.
- **Why they read zero.** The `preset*volume` operators that populate the quad are mentioned zero
  times in two of the three script corpora. Zero is the expected value.
- **The trailing word.** `getuseddrawblt`/`setuseddrawblt` name global `0x5d20bc` and nothing else.
- **Which audio word is which channel.** `setlastaudiosettings`'s four call targets are the
  distinguishing targets of the four `set*volume` operators, in order, and the sound-object slots
  they write corroborate it. Filed as an open question for one commit by over-retracting; it was
  answerable from the committed tables all along.

## What is not determined

Collected, so that nothing here reads as settled when it is not.

1. **What writes `settings.cfg`.** Established that something does, during play; that all 23 keys
   have script-callable operator counterparts; and that script-level file writing exists. Not
   established what writes it, after searching 3,373 objects by bytes and cross-checking with the
   vocabulary and the operator table. Three media archives and the prefix DLLs remain out of reach,
   as does any string assembled at run time.
2. **How `settings.cfg` is written** — whole-file or in place. The size coincidence says nothing.
3. **Test coverage, not format knowledge, for six of the eleven `lom.cfg` fields.** Their identity
   is established from `loadconfig`'s `fread` destinations. What is undetermined is only that no
   value-based check over this corpus verifies the parser's *ordering* of them, because the four
   audio words, the Balkoth counter and centre-on-movement all read `0` in all four profiles.
   Measured by mutation. An untried non-value instrument that would separate the Balkoth/centre pair
   is named above.
4. **Whether patching `lom.cfg`'s audio words changes the applied volume.** The path reaches the
   same mixer helper `setmusicvolume` uses; it has not been tried in a running engine.
5. **Which of `0x5aa144`--`0x5aa150` feeds which of `setlastaudiosettings`'s four calls** -- local
   disassembly only, not reproduced by this repository's extractor.
6. **Why `lom.cfg` holds 26 help panels**, given 40 script mentions of `addhelppanel`. The obvious
   explanation is refuted and no replacement is offered.
7. **Whether the used-DrawBlt flag and `settings.cfg`'s `USE_DIRECTX_BLIT` are the same quantity.**
   They are not equal across the corpus.
8. **Why the 3.02 profile regenerated its GUID**, given its file is the full 160 bytes and the
   observed regeneration path is a short read.
9. **`English/map/e3map2.map`** beyond its size equation. Not decoded, and one file is not a format.
10. **`English/custldr/0templdr.ldr`** (GS5R3) — nothing at all beyond its byte histogram.
11. **The Asura container** beyond its magic and two length equations: two header words, the hash
    function, and the trailing NUL run are all unexplained, and no parser exists. The
    `LOMLauncher.exe` attribution is inferred from contents, not from reading that binary.
12. **Whether the German audio in `Wav/lou_scenarios/` is reachable** from an English install.
13. **Anything above the game-tree root.** ~16,760 files per bundle were excluded by boundary, not
    examined. If the game writes state into the Wine prefix — registry hives, `users/` — this sweep
    did not look.

## Safety

The sweep is read-only and this work never wrote inside `~/Applications`. `English/map/` has no
backup; `loose::inventory` opens files for reading only and follows no symlinks out of the tree
(`symlink_metadata`, not `metadata`).

It also tolerates a tree that moves underneath it. The GS5R3 profile is played while it is measured,
so its saves and logs can vanish between the directory walk and the read; an unreadable file becomes
a row carrying its error, and a vanished directory entry is skipped, rather than aborting a sweep
that had already classified 495 files. Any other I/O error still stops the run, because a sweep that
silently walks half a tree and reports a total is worse than one that fails.
