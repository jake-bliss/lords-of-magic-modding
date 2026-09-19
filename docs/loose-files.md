# Loose files

Everything on disk in an installed profile that is **not** inside an `.mpq`.

This repository's coverage claim — that every file format outside the executable is decoded — rested
entirely on the archive work. It was never true of the loose tree. `English/Wav/`, `English/smk/`,
`English/Shaders/`, `English/Text/` and the two install-root `.cfg` files had never been enumerated,
and two of them had no parser and no recorded layout at all. This page closes that gap and says
plainly where it is still open.

**Updated 2026-09-19.** Four of the items this page listed as undecoded are now decoded:
`map/e3map2.map` (the map format without its version word), `Text/Menu/Menu_En.asr` (parser **and**
emitter, hash function included), what writes `settings.cfg`, and `lom.cfg`'s trailing word.
`custldr/0templdr.ldr` is identified and its container is read, but its interior is not decoded and
cannot be validated against a corpus of one file. So the loose tree's formats are **not** all
decoded, and the one that is not is named.

Every claim below carries its evidence class: **Observed in a local binary** (read out of
`lomse.exe` or `LOMLauncher.exe`), **Observed in the corpus** (read off the installed files or the
archives they ship), **Observed in gameplay**, **Documented**, **Inferred**, **Refuted**.

Measured **2026-09-18**, re-measured **2026-09-19**, on the four installed profiles. Reports:
[`reports/loose/summary.md`](../reports/loose/summary.md),
`reports/loose/inventory-{baseline,development,gs5r3,patch302}.tsv`,
[`reports/loose/config-fields.tsv`](../reports/loose/config-fields.tsv).

## Reproducing it

```sh
scripts/loose-file-reports.sh            # every report, then every corpus-gated check
```

That is the tracked command, and it is the one to run whenever the reports are regenerated or
`src/loose.rs`, `src/asura.rs` or `src/map.rs` changes. **Run it rather than the individual verbs.** Half the checks behind these
reports need the proprietary tree, so they are `#[ignore]`d and an ordinary `cargo test` skips
them; consistently permuting two `LomConfig` fields left `cargo test` green for exactly that
reason. An `#[ignore]`d guard that no tracked command invokes is not a guard.

The individual verbs, for one-off inspection:

```sh
ROOT="$HOME/Applications/Steambuild 32 64bit DXVK.app/Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition"
lom-asset-viewer --loose-inventory "$ROOT" baseline > reports/loose/inventory-baseline.tsv
lom-asset-viewer --loose-config "$ROOT/English/lom.cfg"
lom-asset-viewer --loose-config "$ROOT/English/settings.cfg"
lom-asset-viewer --describe-asura "$ROOT/English/Text/Menu/Menu_En.asr"
lom-asset-viewer --dump-map-cells "$ROOT/English/map/e3map2.map" 0 0 3 3
lom-asset-viewer --scan-map-dir "$ROOT/English/map"
```

Both `--loose-config` parsers re-encode; the CLI prints `round-trips` so a partial read is visible
rather than silent.

### A known flake in the mod-pipeline tests, and what is actually established

`tests/test_mod_pipeline.py` sometimes fails with `lomse.exe is running; quit the game first.`
while no game is running, and sometimes skips all 16 of its tests for the same reason. Both come
from one guard: `game_is_running()` in that file and `refuse_if_game_running` in
`scripts/lib-mod-pipeline.sh`, each `pgrep -f 'lomse.exe'`.

**Observed, with the command lines captured.** `-f` matches the whole command line of every
process, and it matched two processes that were not the game:

- the agent harness shell (`/bin/zsh -c ...`) running a command whose text happened to contain
  `lomse.exe`;
- a bare `python3 -c` whose source contained the literal, with nothing to do with this repository.

So the guard answers "is the game running?" with "does any live command line mention this string?".
Whether it fires depends on what else is running at that instant, which is why the suite oscillates
between *16 skipped* (the guard fired at module import, where the `skipIf` is evaluated once) and
*runs and fails* (it did not fire at import but did fire mid-test).

**Not established**: a minimal deterministic reproduction. Two instances were captured, not a rule
about when `pgrep` matches. It is recorded at that grade rather than written up as a mechanism.

**Not caused by this work**: no commit on this branch touches either file, and the captured
instances are a harness shell and an unrelated `python3 -c`.

The distinguishing signal for whoever fixes it: the real game runs as `d:\lomse.exe`, a Wine drive
path with a **backslash**, while this project's own tools use forward slashes -- so
`pgrep -f '\\lomse\.exe'` separates them. (`.` is an any-character wildcard in that pattern too,
so `lomseXexe` matches; minor by comparison.) **Deliberately not fixed here**: both files are being
edited on other branches, and a three-way collision on a test guard is how a real failure gets lost
in a merge.

Until it lands, run `scripts/loose-file-reports.sh` and the Python suite one after the other, and
re-run the Python suite before believing a red result from it.

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
session.

**Observed in the corpus, 2026-09-19.** `development` is no longer stable either: its `lom.cfg`
changed between the 2026-09-18 sweep and the 2026-09-19 regeneration — one more help-panel flag
cleared, index 2 going `1` -> `0`, with a new digest — and its mtime is 2026-09-19 15:13. That is
the monotone 1 -> 0 behaviour the help-panel vector is identified by, so something ran that profile.
It was not this work: nothing here writes inside `~/Applications`, and the inventory opens files
read-only. `baseline` and `patch302` are unchanged.

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

**Re-measured 2026-09-19: eight** in the baseline and development profiles, 11 in `patch302`, 15 in
`gs5r3`. Two came off the list in every profile — `English/Text/Menu/Menu_En.asr` and
`English/map/e3map2.map`, which now probe as `asura-text` and `map-grid`. The full list is in
`reports/loose/summary.md`; the baseline eight are:

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

Off this list since 2026-09-19, with their new `probe_kind`:

| path | kind |
| --- | --- |
| `English/Text/Menu/Menu_En.asr` | `asura-text` |
| `English/map/e3map2.map` | `map-grid` |

`English/custldr/0templdr.ldr` (GS5R3 only) stays on it: the file is identified and its container
is read, but no parser is written for it — see below.

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

### `English/Text/Menu/Menu_En.asr` — decoded, and now composable

375 bytes. **Decoded, with a parser and an emitter**, 2026-09-19. `--describe-asura` reads it;
`asura::AsuraText::to_bytes` writes it, recomputing every derived word rather than copying it.

#### It belongs to the launcher, and that is no longer an inference

This page previously recorded the `LOMLauncher.exe` attribution as **Inferred** from the file's
contents. **Observed in a local binary, 2026-09-19**: `English/Launcher/LOMLauncher.exe` contains
the format strings `Text\Menu\Menu_%s.asr` and `Text\%s_%s.asr` — it builds this exact path —
together with the literals `Asura   ` and `HTXT`, the RTTI names
`Asura_HashedLocalisedText_Page`, `Asura_ResourceSet`, `Asura_Handle_List` and
`Launcher_ResourceProtocol`, an embedded HTML page whose buttons are `[[LAUNCHER_PLAY]]` and
`[[LAUNCHER_SUPPORT]]`, and the build path
`C:\Source Code\ASURA_ROOT\FileTools\Launcher\LordsOfMagic\Workspace\Release\LOMLauncher.pdb`.
`lomse.exe` contains none of them.

#### The layout

**Observed in the corpus.** File offsets; little-endian throughout.

| offset | size | field |
| ---: | ---: | --- |
| `0x00` | 8 | magic `Asura   ` — eight bytes, **three** trailing spaces |
| `0x08` | 4 | chunk id, `HTXT` |
| `0x0c` | 4 | chunk payload bytes — `351`, and `375 - 24 = 351` |
| `0x10` | 4 | `3`. **Not determined**; carried verbatim |
| `0x14` | 4 | `0`. **Not determined**; carried verbatim |
| `0x18` | 4 | string count, `5` |
| `0x1c` | 4 | `0x0033155f` — **the hash of the page name `Menu`** |
| `0x20` | 4 | `180` — total UTF-16 bytes across the records, terminators included |
| `0x24` | 4 | `0`. **Not determined**; carried verbatim |
| `0x28` | … | five records: `u32` hash, `u32` unit count, then that many UTF-16LE units |
| `0x104` | 8 | the page name, `Menu` and four NULs |
| `0x10c` | 4 | key-table bytes, `87` |
| `0x110` | 87 | five NUL-terminated ASCII keys |
| `0x167` | 16 | NULs. **Not determined**: padding or a field |

Both length equations from the first draft still hold and are now the parser's own checks rather
than prose: the five unit counts are 5, 8, 31, 6 and 40 against texts of 4, 7, 30, 5 and 39
characters, so each count includes the terminator; and `14 + 17 + 20 + 21 + 15 = 87` for
`LAUNCHER_PLAY`, `LAUNCHER_SUPPORT`, `LAUNCHER_GAME_TITLE`, `LAUNCHER_ERROR_TITLE`,
`LAUNCHER_ERROR`.

#### The hash — the thing that was blocking composition

**Observed in a local binary.** `LOMLauncher.exe` `0x004018b0`:

```text
h = 0;  for each byte c of the key, terminator excluded:
    if ('A' <= c <= 'Z') c += 0x20        ; 004018c0-004018c8, case folded down
    else if (c == '\\')  c = '/'            ; 004018cd-004018d2, backslash folded to slash
    h = h * 31 + (int8_t)c                ; shl/sub at 004018d6, movsx at 004018db
```

The multiply is the `shl edx,5; sub edx,eax` idiom, which is why a search for `imul reg,reg,31`
finds nothing. Three details come from the instructions and **cannot** be shown by this corpus,
whose keys are plain upper-case ASCII: the terminator is not hashed, `\` folds to `/`, and the
byte is **sign-extended**, so a byte at or above `0x80` contributes a negative value.

**Observed in the corpus.** It reproduces all five record hash words from their keys and the
page-name word `0x0033155f` from `Menu`. Six for six, and that is asserted as an equation over the
installed file rather than as a transcribed table.

**Five of the six dimensions are corpus-pinned. The sign extension is not, and that is the weak
one.** Review tested the neighbours rather than taking the rule on trust: multiplier 33,
multiplier 37, hashing the terminator, and no case folding each **fail on all six words**, so those
four choices are forced by the data. But **unsigned byte extension reproduces all six
identically**, because every key in the one installed file is ASCII below `0x80`. So `movsx` rests
on a single reading of the instruction at `0x004018db` and on nothing else — no second instrument
has confirmed it. If it were wrong, every plain-ASCII key would still hash correctly and only a key
containing a high byte would diverge. Graded **Observed in a local binary, single source**; what
would settle it is a second disassembly of `0x004018b0`, or an `.asr` from another Rebellion title
whose keys are not pure ASCII.

#### Composability, and what has and has not been shown

- **Adding a key is possible**, and that is what the hash unblocked. Before it was recovered only
  *replacing* an existing string could work, because a new record's hash word could not be filled
  in. `AsuraText::push` computes it. **Observed in the corpus**, as a property of the format:
  `a_key_added_to_the_installed_asura_table_survives_a_round_trip` adds a key to the *installed*
  file, lengthens an existing string, re-encodes, and re-reads both back.
- **Lengths are free.** Every derived word — the chunk size, the record count, the total text
  length, each record's unit count, the page-name hash, the key-table length — is recomputed by
  `to_bytes`, not stored. This is why the round trip is a real check here and not the mathematical
  identity that `lom.cfg`'s is: a wrong length rule or a wrong hash makes the re-encoded file
  differ from the installed one.
- **Not established: that `LOMLauncher.exe` accepts a file this emitter wrote.** Nothing here has
  been loaded by the launcher, and the launcher has not been run. The claim is self-consistency by
  the format's own rules, which is weaker, and it is the one the tests assert.

**Still not determined**: the words at `0x10`, `0x14` and `0x24`; whether the eight bytes at `0x104`
are one NUL-padded name field or a four-byte name plus a zero word (the one corpus name is four
characters, so the two readings produce identical bytes); and whether the trailing 16 NULs are
padding or a field — the file is 375 bytes, so they are not alignment to any power of two. All five
are carried verbatim, so nothing is invented for them and the file still re-encodes exactly.

### `English/map/e3map2.map` — decoded: the map format without its version word

**Decoded 2026-09-19.** It was the only file in `English/map/` the map pass never offered to the
parser; it now parses, re-encodes byte for byte, and `--scan-map-dir` counts it.

**Observed in a local binary.** The engine has two map readers and they are **nested**, not
parallel. `0x004a52e0` reads a bare grid: `u32` width into map object `+0x5c`, `u32` height into
`+0x60`, `u32` bytes-per-cell into a stack local, then a loop that reads `local << 6` bytes at a
time into the cell array at `+0x54`, advancing 64 cells a pass. `0x004855c0` reads a scenario: one
`u32` into the scenario object's `+0x00` and then a **call to that same `0x004a52e0`** -- the
instruction is at `0x004855f8` -- on the embedded map at `+0x482c`, then the placed-sprite section
and a version-gated dword. The writers mirror it: `0x004a5440` writes the grid (emitting the third
header word from a local set to `8`, `c7 44 24 08 08 00 00 00` at `0x004a544b`), and `0x00485550`
writes four bytes from `0x0055b1b0` and then calls `0x004a5440` at `0x0048558a`.

Those four addresses were independently re-derived by an adversarial review that disassembled the
image itself, so this is two readings of one binary rather than one.

So `.smp`/`.scn`/`.lgd` **is** a version word in front of a `.map`, and the operators reach the two
separately -- `loadmap` (`0x004dfad0`) / `savemap` (`0x004dfbe0`) for the grid form,
`loadscenariomap` / `savescenariomap` / `loadspecialmap` / `savespecialmap` for the scenario form.
`loadmap`'s worker closes the file as soon as the grid is in, so the grid form has **no tail
section at all** -- which is a different statement from "a tail this project cannot read", and the
tools now say which.

| form | header | tail | corpus |
| --- | --- | --- | ---: |
| scenario | `version, width, height, bytes_per_cell` | record section (+ version-gated dword) | 353-365 per profile |
| grid | `width, height, bytes_per_cell` | none | 1 per profile |

**Observed in the corpus.** 32,780 bytes: `12 + 64 x 64 x 8`, three header words of `64, 64, 8` and
4,096 eight-byte cells, sourced as three terms rather than as a total. The nearest neighbour is
`chbldg01.smp`, the one 64x64 scenario file in the corpus, at **32,788** bytes — exactly eight
more, being the version word in front and the empty record section's zero count word behind. Their
grids hold different content, so that is a shape match and not a duplicate.

**The earlier reading was wrong in a way one file could not show, and the corpus settles it.**
Reading the header one word later — as a 16-byte scenario header — is what produced `unsupported
map cell depth 391`. That alignment is not merely unsupported, it is refuted: it yields a tile-index
field that is `0` in all 4,095 cells it can reach and elevations that are denormal floats around
`1e-45`. On the correct alignment every one of the 4,096 tile indexes is inside the atlas (`1..443`
against the corpus range `0..623`), the cell's upper 16-bit field is `0` in every cell, and all 42
distinct elevations — `0.0` to `10.25` in steps of `0.25` — are bit patterns that the scenario files
in the same directory also use. Zero strays on either check. Pinned by
`the_grid_form_map_uses_the_corpus_tile_and_elevation_vocabulary`.

**Observed in the corpus.** `.map` and `.smp` are passed to the *same argument slot* by a script:
`gs.mpq` member `File00000214.xxx` calls `startspecialcombat` with `"map/thanh.smp"` in one branch
and `"map/test.map"` in the other, each paired with `"til/cavetile.til"`. So the extension does not
choose the reader, and `lomse.exe` contains no `.map`, `.smp`, `.scn` or `.lgd` literal at all. The
parser therefore sniffs the header: each form is tested on its own terms and every file in every
profile's `English/map/` matches exactly one — 353 scenario + 1 grid in three profiles, 365 + 1 in
`gs5r3`. Pinned by `both_header_forms_are_mutually_exclusive_across_the_corpus`.

**The two structural tests are asymmetric, and that asymmetry bit immediately.** A grid file must
account for every byte; a scenario file only has to be *at least* header-plus-grid, since a tail
follows. So a grid file can accidentally satisfy the scenario test and not the reverse — reading a
grid file as a scenario takes its **cell-0 tag as the bytes-per-cell word**, and a cell whose tile
slot is `8` makes that word `8`. Found by adversarial review, on the installed corpus, not reasoned
about:

```text
slot 7 -> set-tile (0,0) tile:7
slot 8 -> error: refusing to write: the edited map no longer parses:
          map header is ambiguous: it reads as Scenario and Grid equally well
slot 9 -> set-tile (0,0) tile:9
```

Every other slot worked. The same refusal reached `--map-set-terrain`, `--map-fill-terrain`,
`--map-paint-terrain` and the editor server's save path whenever cell (0, 0) landed on slot 8. **No
data was at risk** — the writers re-parse before writing, and that guard is what caught it — but
the tool refused a legal file and blamed the header for the edit.

**The tie-break, and why it is not arbitrary.** When both fit, ask whether the *scenario* reading's
remainder is actually a placed-sprite section: a count word present, and a count that some decoded
layout accounts for to the byte. If not, the scenario reading has left bytes it cannot explain while
the grid reading explained all of them, so the file is grid-form. The slot-8 case fails by a mile —
28,668 unexplained bytes behind a 512-cell grid. If the remainder *is* section-shaped, both readings
are internally consistent, nothing in the bytes can choose, and `Scenario` wins as the conservative
answer; no corpus file can reach that branch, because the grid test would need a scenario `height`
word of 8 and none is. Pinned by three unit tests, and all three of the obvious wrong rules — always
`Scenario`, always `Grid`, and the old refuse-on-tie — are killed by them.

**Retracted, 2026-09-19: the corpus does not test the nesting claim.** This page said that
dropping a scenario file's first four bytes and re-reading the remainder as a grid "tests the
nesting claim against the corpus". It does not, and the reason is worth keeping. The scenario
reader takes its shape words from absolute offsets 4/8/12 and its cells from 16; the grid reader
applied to `bytes[4..]` takes them from the **same absolute offsets of the same buffer**. The
agreement is a *mathematical identity*: it holds for any file that parses as scenario form, whether
or not the engine nests the two readers. `a_scenario_file_is_a_version_word_in_front_of_a_grid_file`
is a consistency check, not evidence, and its docstring now says so.

**Measured, and this is the part that matters.** Swapping the `width` and `height` reads in
`parse_as` -- a real, different header-layout hypothesis -- leaves that test green **and all eleven
corpus-gated checks green**, `the_committed_inventory_reproduces` included. The corpus cannot see
it because every shipped map is square. This project has been bitten by exactly that blind spot
before (the transposed cell index, corrected 2026-09-17), and it was about to publish a test whose
docstring claimed falsifiability it did not have. The mutant *is* killed -- by 44 of the library's
own unit tests, which use non-square fixtures -- so the layout is pinned; the credit was in the
wrong place.

**The nesting is Observed in a local binary, and only there.** `call 0x004a52e0` at `0x004855f8` in
the scenario reader, `call 0x004a5440` at `0x0048558a` in its writer, and the third header word
emitted from a local set to `8` at `0x004a544b`. Independently re-derived by review.

**Corrected in the code, not newly learned.** `MapAsset`'s third header field was called
`bits_per_pixel`; `docs/map-format.md` has called it **bytes per cell** since 2026-09-17, on the
evidence that the reader multiplies it by 64 to size a 64-cell block and the writer emits a literal
`8`. The field is now `cell_bytes` and `--dump-map-cells` prints `bytes-per-cell:` instead of
`bpp:`. The corpus can never show the difference, because 8 bits per pixel and 8 bytes per cell are
the same number.

**Not determined.** Which of the first two words is width and which is height *in this file*: it is
64x64, so the corpus cannot separate them, and the assignment comes from the allocator at
`0x004a4f20`, which stores the first as the row stride. Also not determined: what `e3map2.map` is
*for*. The name suggests a combat map, and `startspecialcombat` takes a map path, but no script in
any of the three corpora names this file.

**Also not determined: whether any grid-form file other than this one exists.** One file is not a
format; what carries the format here is the engine, not the file.

### `English/custldr/0templdr.ldr` — the custom-leader scratch slot

3,982 bytes, present in the `gs5r3` profile only. **Partly decoded, 2026-09-19**: what it is, what
writes it and reads it, and the outer container. The interior is **not** decoded, and this section
says exactly where the line is.

#### What it is

**Observed in the corpus.** The custom-leader editor. `gs\dlg\CHAREDIT5.gs` in GS5R3's `gs.mpq`
(and `gs\dlg\charedit.gs` in the vanilla and 3.02 archives, so the feature is shipped, not a GS5R3
addition) holds:

```text
/save_lord{"custldr/""custldr/*.*""Save Custom Leader"{ ... savevirtualarmy ... }fileselector}def
/istempldrfile?{/dummy begin"0templdr.ldr"strcmp 0 eq end} ...
/load_lord{"custldr/""custldr/*.*""Load Custom Leader"{ ... loadvirtualarmy ... }fileselector}def
```

and, on the button that starts the game with the edited leader, `savedefaultarmy`. `load_lord`
explicitly refuses this one name: `load_filename istempldrfile? {false} {load_filename
loadvirtualarmy} ifelse`. So `0templdr.ldr` is the *hand-off* file the editor writes on its way into
a game, not one of the player's saved leaders — which is why it is the only `.ldr` in any profile
and why it appears in the profile that has been played.

**Observed in the corpus.** The file's four visible strings are `Knights`, `Crossbowmen`,
`Crossbowmen` and `Jerk Lawbind` — three unit types and a leader name.

#### Two `.ldr` forms, which is the trap

**Observed in a local binary.** Two operators write into `custldr/`, with **different layouts**:

| operator | worker | filename | writes |
| --- | --- | --- | --- |
| `savedefaultarmy` `0x0044c9a0` | `0x0044c8f0` | `sprintf("custldr/%dtempldr.ldr", …)` | `0x2a0` bytes from army `+0x58`; the army object through its vtable slot `+0x20`; then `+0x54`, `+0x4c`, `+0x50`, `+0x48` |
| `savevirtualarmy` `0x0044ca80` | `0x0044c510` | `sprintf("custldr/%s", name)` | the same `0x2a0`, **plus** `0x78` bytes from `+0x2f8` and the three words `+0x40`, `+0x44`, `+0x38`; then the vtable write; then `+0x54`, `+0x4c`, `+0x50`, `+0x48`, **plus** one word from global `0x5af0f4` |

Both open `"wb"` through `0x00539610` and close through `0x005394d0`; the readers,
`loaddefaultarmy` `0x0044c9b0` → `0x0044c7d0` and `loadvirtualarmy` `0x0044cb60` → `0x0044c630`,
mirror them `fread` for `fread`. `0x0044c510` and `0x0044c630` also call the import at `0x0054d0bc`
on the literal `"custldr"` first, which is how the directory comes to exist.

**So a file saved by "Save Custom Leader" is not the same format as `0templdr.ldr`**, despite the
same extension and the same directory. Anyone writing a tool for these has to know which operator
produced the file, and the file does not say.

#### The outer container of the `0templdr.ldr` form

**Observed in a local binary.** In `savedefaultarmy` order:

| bytes | source |
| ---: | --- |
| 672 (`0x2a0`) | army object `+0x58` |
| variable | the army object, written through vtable slot `+0x20` = `0x00412090` |
| 4 | army `+0x54` |
| 4 | army `+0x4c` |
| 4 | army `+0x50` |
| 4 | army `+0x48` |

Fixed part = 672 + 16 = **688**, so the variable block in the installed file is
`3,982 - 688 = 3,294` bytes.

**Observed in the corpus**, and weakly: the file's first non-zero byte is at offset **676**, which
is inside the first word the variable block writes (`+0x44`, which is `0`) — consistent with the
672-byte boundary. It is consistent, not probative: a run of zeros fits many boundaries. The
boundary itself comes from the instructions.

#### What is not determined, and what would settle it

- **The 3,294-byte army object.** `0x00412090` writes `+0x44`, `+0x48`, `+0x4c`, `+0x50`, then a
  loop of `[+0x4c]` sub-objects at stride `0x4c` through `0x00524c20`, then `+0x53c`, `+0x540`,
  `+0x544`, `+0x548`, a container at `+0x538` through `0x004279d0`, then `+0x54`, `+0x58`, `+0x64`,
  `0x58` bytes at `+0x68`, and a further run of words. Decoding it means walking those three
  callees and the unit sub-object's own writer. It is a real piece of work, not a gap that a
  paragraph closes.
- **It cannot be validated against a corpus.** There is exactly **one** `.ldr` file in all four
  profiles. One file is not a format, and a layout fitted to one file is a layout that cannot fail.
  What would change that is a second file — and the cheap way to get one is *not* to run the game
  but to note that `savevirtualarmy`'s own form is written by a different code path, so the two
  disagree structurally and each constrains the other.
- **The `%d` in the filename.** `0x0044c8f0` formats it from `[0x005a7d90 + ([0x005a7d8c] << 10)]`
  — a field of the current player's 1,024-byte record, the same indexed-global pattern
  `loadconfig` uses. Whether that is the player index is **not determined**; the one shipped file
  is `0`.

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
- **`English/custldr/0templdr.ldr`** (GS5R3 profile only) — 3,982 bytes. See
  [the `.ldr` files](#englishcustldr0templdrldr--the-custom-leader-scratch-slot) below; it is the
  custom-leader editor's scratch save, and it is **partly** decoded.

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

### Resolved: what writes it, and how

**Observed in the corpus, 2026-09-19.** It is `gs\dlg\opdlg.gs` — the options dialog — in the
**3.02 unofficial patch's** `gs.mpq`. The relevant code is a GameScript procedure:

```text
"settings.cfg" "w" file
dup "TOOL_TIP_TRANSLUCENCY " writestring
dup tooltiptrans 3 string cvs writestring
dup carriage_return
...
dup "USE_DIRECTX_BLIT " writestring
dup getuseddrawblt 3 string cvs writestring
dup carriage_return
dup "CENTER_MOVE " writestring
dup getcenteronmovement 3 string cvs writestring
dup carriage_return
closefile
```

That settles four things at once and closes three open questions on this page:

- **What writes it.** A script, not the engine — which is why the earlier search found nothing.
- **How.** `"w"`, so the file is **truncated and rewritten whole**, one record at a time. The
  equal-length coincidence the first draft nearly read this from was never evidence; the `"w"` is.
- **Why every record ends in a bare `CR`.** The separator the script writes is `carriage_return`.
- **Where the values come from.** Each is `cvs`'d from a script-level value or operator:
  `getbuildingspeechflag`, `getmusicvolume`, `getsoundfxvolume`, `getspeechvolume`,
  `getambientvolume`, `getuseddrawblt`, `getcenteronmovement`, `SCROLLINGMAP_SCREEN
  getmodeticktime`, `COMBAT_SCREEN getmodeticktime`, and dialog state for the rest.

The **matching reader** is in the same member: `/val_array 23 array def`, then a character loop that
ends a key on a space (`char 32 eq`) and a record on a bare `CR` (`char 13 eq`), parsing each value
with `string_cvi` into a three-character buffer. Twenty-three records, space-separated, bare-CR
terminated — exactly what `loose::SettingsConfig` implements, arrived at from the bytes.

**Observed in the corpus.** The 23 `writestring` key literals in the script are the 23 keys of the
installed file, **in the same order**. Two artifacts that know nothing about each other: one is a
script inside an archive, one is a text file on disk.

**Documented.** The patch's own readme, `English/lomse302.htm` in the `patch302` profile, agrees.
Its feature list says the modification "greatly expanded the options menu, allowing the user to set
options involving tooltips, combat display and selection, mouse and keyboard scroll speeds, as well
as the option of loading and saving custom user settings"; its uninstall instructions say to delete
`gs.mpq`, `pic.mpq`, `lomse302.htm`, **`settings.cfg`** and the `pic` directory — `settings.cfg`
listed among the files the modification produces, not among the ones to restore from backup. And:
"Do not delete or manually modify `settings.cfg`."

A lead chased and **refuted** along the way, kept because someone will chase it again:
`lomse.exe` contains a `"%s %d"` format string of exactly the right shape for these records, but
its single cross-reference at `0x0049b099` sits in an `sscanf("#define %s %d")` loop that reads IMP
`.H` headers. It is not the settings writer, and now we know nothing in `lomse.exe` is.

**Why the previous negative was published, and what was wrong with it.** The search covered the
**vanilla** `gs.mpq` and `special.mpq`, and the answer was in the *modded* one. The instrument was
sound and the negative it reported was true — re-verified here: the keys are absent from all 1,688
vanilla members and from GS5R3's 1,699 — but its reach was one archive short of the question. The
lesson is not "search harder"; it is that **a corpus of four profiles is four different games**, and
a negative measured on one of them is not a negative about the file that all four happen to hold.

**Observed in the corpus.** The three non-3.02 profiles hold a `settings.cfg` too, byte-identical
across all three, and their `gs.mpq` contains no writer for it. So their copy was *delivered*, not
written in place. **Not determined**: by what. It is identical to `patch302`'s own
`_vanilla_backup/settings.cfg` except for the two keyboard-scroll records, which is what a file
shipped by the patch and then edited in play looks like, but nothing here establishes how it reached
a profile with no writer.

**Observed in gameplay** (unchanged, and now explained). The `patch302` profile's live
`settings.cfg` differs from its own `_vanilla_backup/settings.cfg` in exactly two records —
`KB_MAP_SCROLL_SPEED` 100 → 50 and `KB_COMBAT_SCROLL_SPEED` 10 → 135 — and in nothing else. That is
one pass through the options dialog's Save button.

### Resolved: `USE_DIRECTX_BLIT` and `lom.cfg`'s trailing word are the same quantity

Open question, now closed. **Observed in the corpus**, from the writer above: `USE_DIRECTX_BLIT` is
written from `getuseddrawblt`. **Observed in a local binary**: `getuseddrawblt` (`0x004da350`) names
global `0x5d20bc` and nothing else, and `loadconfig`'s **last** `fread` before `fclose` reads four
bytes straight into `0x5d20bc` (`0x004874d3`). One global, two files.

They are still not *equal* across the corpus — the setting is `-1` everywhere and the `lom.cfg`
field is `1` in `baseline` and `development` — and that is no longer a puzzle: only `patch302` has
ever run the script that writes `settings.cfg`, so in the other three profiles the two files were
last written by different things at different times.

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

### The channel mapping, and exactly what grades it

The chain is: file word -> config-object slot -> one of `setlastaudiosettings`'s four calls -> a
`set*volume` helper. The two links grade differently, and this section has now been wrong in both
directions, so both are spelled out.

**The last link is Observed in the corpus.** Each `set*volume` operator's own row names both a
distinguishing call target and a sound-object write:

| operator | distinguishing call target | sound-object slot written |
| --- | --- | --- |
| `setmusicvolume` | `0x479a00` | `+0x1368` |
| `setsoundfxvolume` | `0x479b40` | `+0x136c` |
| `setspeechvolume` | `0x479c30` | `+0x1370` |
| `setambientvolume` | `0x479d10` | `+0x1374` |

(The targets those four share, `0x4d4550` and `0x5394a0`, are boilerplate.) So `0x479a00` is the
music helper, on committed evidence.

**The middle link is Observed in a local binary, and the committed tables cannot supply it.** A
previous version of this page called the call-target column and the field-access column "two
independent columns" agreeing. They are not independent: `operator-field-access.tsv` is produced by
*walking into the callees listed in* `operator-bodies.tsv`, so `setlastaudiosettings`'s four
recorded destination slots are just the union of what its four callees write. Both tables record
the **set** of four globals and the **set** of four call targets; neither records which global was
pushed before which call. Exchanging the loads of `0x5aa144` and `0x5aa148` would leave every
committed table byte-identical while swapping the meanings of `lom.cfg[0x00]` and `[0x04]`.

That hop is now pinned by
`each_audio_word_reaches_the_helper_its_channel_is_named_after`, which decodes
`setlastaudiosettings` from the installed image and asserts the ordered pairing, so it is
reproducible rather than resting on a hand disassembly. It is corpus-gated, and it fails if the
pairing is stated wrongly.

> **The rule this cost three attempts to learn.** Two reports derived from one extraction pass are
> **one instrument**, however many columns they have and however independent the columns look. This
> branch got it wrong three times: the `lom.cfg` reader and writer agreeing (forced -- the game
> would not load its own file otherwise), and then these two tables twice over. Before calling
> something corroborated, ask what *produced* each reading, not what each reading says.

**Not done, and worth doing.** The general fix is to make the extractor emit ordered call-site facts
-- argument source, call target, object base -- so this pairing becomes reproducible for every
operator rather than for the one this work needed. That touches a core analysis module whose output
several committed reports and test suites assert against, so it is recorded here as a
recommendation rather than attempted as a side effect of a file-format sweep.

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

**Confirmed directly, 2026-09-19.** The naming rule gave this; the reader now corroborates it
without going through the table at all. `loadconfig`'s **last** `fread` before `fclose`
(`push 0x5d20bc` at `0x004874d3`, call at `0x004874d8`) reads four bytes straight into that global,
and defaults it to `1` on a short read at `0x004874e4`. The trailing word of the file *is* the
used-DrawBlt global, by direct address rather than by inference.

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

Recorded rather than quietly edited away.

Closed from artifacts already committed in this repository, without touching the game again:

- **What the four `lom.cfg` head words hold.** They are the *last audio settings*, not the live
  volumes; the first attribution was refuted by `loadconfig`'s own recorded globals.
- **Why they read zero.** The `preset*volume` operators that populate the quad are mentioned zero
  times in two of the three script corpora. Zero is the expected value.
- **`lom.cfg`'s trailing word.** `getuseddrawblt`/`setuseddrawblt` name global `0x5d20bc` and
  nothing else; confirmed directly 2026-09-19 by `loadconfig`'s last `fread` at `0x004874d3`.
- **Which audio word is which channel.** `setlastaudiosettings`'s four call targets are the
  distinguishing targets of the four `set*volume` operators, in order.

Closed 2026-09-19, by reading the binaries and the **modded** archives:

- **`English/map/e3map2.map`.** The map format without its version word. Parsed, round-tripped,
  counted by `--scan-map-dir`, and checked against the corpus's own tile and elevation vocabulary.
- **`English/Text/Menu/Menu_En.asr`.** Parser *and* emitter, including the launcher's hash
  function, read out of `LOMLauncher.exe` at `0x004018b0`.
- **Who owns the Asura container.** `LOMLauncher.exe`, now Observed in a local binary rather than
  inferred from the file's contents.
- **What writes `settings.cfg`, and how.** `gs\dlg\opdlg.gs` in the 3.02 patch's `gs.mpq`,
  whole-file through `"w"`, terminating each record with `carriage_return`.
- **Whether `USE_DIRECTX_BLIT` and `lom.cfg`'s trailing word are the same quantity.** They are:
  the script writes the setting from `getuseddrawblt`, which names the global the loader fills.
- **What `English/custldr/0templdr.ldr` is**, what writes it, and its outer container. The interior
  is not decoded; see below.

## What is not determined

Collected, so that nothing here reads as settled when it is not.

1. **How `English/custldr/0templdr.ldr`'s 3,294-byte army object is laid out.** The container
   around it is read out of `savedefaultarmy`; the object itself is written through a vtable slot
   at `0x00412090` and three further callees. And it **cannot be validated against a corpus**:
   there is one `.ldr` file in four profiles.
2. **Whether `savevirtualarmy`'s `.ldr` form appears anywhere.** It has a different layout from
   `0templdr.ldr` and no installed file is in it.
3. **Test coverage, not format knowledge, for six of the eleven `lom.cfg` fields.** Their identity
   is established from `loadconfig`'s `fread` destinations. What is undetermined is only that no
   value-based check over this corpus verifies the parser's *ordering* of them, because the four
   audio words, the Balkoth counter and centre-on-movement all read `0` in all four profiles.
   Measured by mutation. An untried non-value instrument that would separate the Balkoth/centre pair
   is named above.
4. **Whether patching `lom.cfg`'s audio words changes the applied volume.** The path reaches the
   same mixer helper `setmusicvolume` uses; it has not been tried in a running engine.
5. **Nothing in the committed operator tables can express which argument reaches which call.** The
   `lom.cfg` audio pairing is pinned by a test that decodes the image directly, but the general
   capability is missing from the extractor and every similar question will hit it again.
6. **Why `lom.cfg` holds 26 help panels**, given 40 script mentions of `addhelppanel`. The obvious
   explanation is refuted and no replacement is offered.
7. **How `settings.cfg` reached the three profiles whose `gs.mpq` cannot write it.** Their copies
   are byte-identical and differ from `patch302`'s only in the two records a play session changed.
8. **Why the 3.02 profile regenerated its GUID**, given its file is the full 160 bytes and the
   observed regeneration path is a short read.
9. **Which of `e3map2.map`'s first two words is width.** It is 64x64; the assignment comes from the
   allocator at `0x004a4f20`, not from the file. Also: what the file is *for*. No script names it.
10. **Five fields of the Asura container**: the words at `0x10`, `0x14` and `0x24`, whether the
    eight bytes at `0x104` are one name field or a name plus a zero word, and whether the trailing
    16 NULs are padding or a field. All are carried verbatim, so the emitter is exact regardless.
11. **Whether `LOMLauncher.exe` accepts a `.asr` this repository wrote.** The emitter's output is
    self-consistent by the format's own rules and re-parses; the launcher has not been run.
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
