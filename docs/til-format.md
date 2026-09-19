# Tileset definitions (`.til`): what can be written back, and what cannot

**What this file covers:** the write path for `.til` tileset definitions — the format's shape, what
an edit may safely change, what this writer refuses and why, and what about the format is still
undetermined. Reading a `.til` and the terrain-blend rule it carries are covered by
[map-format.md](map-format.md); this file does not restate them.

The corpus is the 26 `.til` members of `pic.mpq`. The baseline archive is
SHA-256 `d0df8b9254bf03c6cb7b2c1db733d8ee1b735e20d16ed1fa192a99066cc62ca8` (vanilla profile),
315,358 bytes of tileset text in total. **All 26 files are byte-identical across all four installed
profiles** — vanilla, 3.02, GS5R3 and the development clone — so neither shipped mod changes a
tileset, and every measurement below is one measurement, not four.

Reproduce everything here with:

```
lom-asset-viewer --til-roundtrip pic.mpq --listfile artifacts/reference-listfiles/lords-of-magic.txt
lom-asset-viewer --describe-til extracted/til/tilesb01.til
```

`pic.mpq` carries **no internal listfile**, so `--listfile` is what names the members; without it
the sweep checks nothing and says so rather than exiting green.

## The format

A `.til` is a hand-written text file, not a binary one. That single fact decides the shape of
everything below.

| Property | Value | Evidence class |
| --- | --- | --- |
| Encoding | UTF-8 (in fact 7-bit ASCII) in all 26 | Observed in the corpus |
| Line endings | CRLF in all 26: 6,001 CRLF, **0** bare LF, **0** bare CR | Observed in the corpus |
| Final newline | present in all 26 | Observed in the corpus |
| Comments | everything from the first `;` on a line | Observed in the corpus |
| Keys | exactly five: `LBM`, `TILES`, `TILESIZE`, `TERRAINTYPE`, `TILE` | Observed in the corpus |
| Records | 26 `LBM`, 26 `TILES`, 26 `TILESIZE`, 402 `TERRAINTYPE`, 4,043 `TILE` | Observed in the corpus |
| Row width | **every** `TERRAINTYPE=` and `TILE=` row has exactly 11 comma-separated fields | Observed in the corpus |
| Repeated keys | none: no file writes any of the five keys twice | Observed in the corpus |

Column alignment is by hand and is not derivable: one row pads with a tab, the next with seven
spaces, and `tilesb01.til` writes `"happy plains" ,` with a space before the comma. Nothing in this
writer tries to regenerate that layout.

### The 26 tilesets

| file | atlas | grid | capacity | terrain types | tiles |
|---|---|---|---|---|---|
| `aibldg01.til` | `aibldg01.lbm` | 16x4 | 64 | 19 | 63 |
| `cavecry2.til` | `cavecrys.lbm` | 16x8 | 128 | 14 | 117 |
| `cavecrys.til` | `cavecrys.lbm` | 16x8 | 128 | 14 | 117 |
| `cavelava.til` | `cavelava.lbm` | 16x8 | 128 | 13 | 117 |
| `cavewatr.til` | `cavewatr.lbm` | 16x8 | 128 | 14 | 117 |
| `chbldg01.til` | `chbldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `chbldg02.til` | `chbldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `chbldg0x.til` | `chbldg0x.lbm` | 16x8 | 128 | 16 | 117 |
| `debldg01.til` | `debldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `debldg02.til` | `debldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `eabldg01.til` | `eabldg01.lbm` | 16x4 | 64 | 15 | 63 |
| `fibldg01.til` | `fibldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `fibldg02.til` | `fibldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `fibldg0x.til` | `fibldg0x.lbm` | 16x8 | 128 | 16 | 117 |
| `jeff01.til` | `jeff01.lbm` | 16x16 | 256 | 18 | 234 |
| `libldg01.til` | `libldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `libldg0x.til` | `libldg0x.lbm` | 16x8 | 128 | 16 | 117 |
| `orbldg01.til` | `orbldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `orbldg02.til` | `orbldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `orbldg0x.til` | `orbldg0x.lbm` | 16x8 | 128 | 16 | 117 |
| `ruins01.til` | `ruins01.lbm` | 16x8 | 128 | 17 | 117 |
| `ruins0x.til` | `ruins0x.lbm` | 16x8 | 128 | 17 | 117 |
| `tilesa01.til` | `tilesb01.lbm` | 16x39 | 624 | 10 | 609 |
| `tilesb01.til` | `tilesb01.lbm` | 16x39 | 624 | 11 | 617 |
| `wabldg01.til` | `wabldg01.lbm` | 16x8 | 128 | 16 | 117 |
| `wabldg02.til` | `wabldg01.lbm` | 16x8 | 128 | 16 | 117 |

Two things in that table are worth naming, both **Observed in the corpus**:

- **Every grid is 16 columns wide.** All the variation is in rows. A slot's picture is
  `(index % columns, index / columns)`, so this is why the writer treats a change to `columns` as a
  different kind of edit from a change to `rows`.
- **Seven atlases are shared by two tilesets each** — `cavecrys.lbm`, `chbldg01.lbm`,
  `debldg01.lbm`, `fibldg01.lbm`, `orbldg01.lbm`, `tilesb01.lbm`, `wabldg01.lbm` — so the 26 files
  reference 19 distinct atlas images. `tilesa01.til` names `tilesb01.lbm`, not a `tilesa01.lbm`;
  there is no such member. Editing one of a pair does not disturb the other, but repainting the
  shared `.lbm` disturbs both, which is an `--import-png-pbm` concern rather than a `.til` one.

## How the round trip is measured, and how strong each half is

`--til-roundtrip` prints three counts. **They are not equally strong and the report should not be
read as though they were.**

| Count | Over all 26, all four profiles | What it actually shows |
| --- | --- | --- |
| `byte-identical` | **26 / 26** | The line splitter and its terminators are exact. **Near-tautological**: the writer keeps each line's bytes and re-emits them, so an unedited file can only differ if a terminator was mis-recorded or a final line without a newline was given one. |
| `values-rebuilt` | **46,989 / 46,989** | Each integer and each neighbour column was **regenerated from the value the parser read** and compared with the file's own characters. This can fail, and does on inputs the corpus does not contain. |
| `no-op-edits-byte-identical` | **740 / 740** | Every span-replacing edit the writer offers, per file, set to the value the file already holds: the atlas name, the grid, both ends of the tile table, a terrain colour, **all 402 terrain descriptions**, and **all eight** neighbour columns of each file's first complete tile. Each goes through the real span replacement, re-parse and verification. A span located one character off, a trimmed field, or a re-quoted description shows up here and nowhere else. |

**Do not read 26/26 as the evidence.** It is reported for completeness. The measurement that can
fail is `values-rebuilt`, and the measurement that tests the *edit* path is `no-op-edits`.

The no-op set covers **every** description rather than one per file, and all eight neighbour
columns rather than one, because the awkward case is never the first: `tilesb01.til`'s terrain 4 is
written `"happy plains" ,` with a space between the closing quote and the comma, and a span offset
that is wrong only for column `nw` is caught by column `nw` and by nothing else. Both are
byte-identical after a no-op; the space survives.

### What is genuinely reconstructed, and what is carried verbatim

| Fields | Count over the corpus | Treatment |
| --- | --- | --- |
| `TILE=` — slot, `self`, the eight neighbour columns, pattern id | 4,043 × 11 = 44,473 | **Reconstructed** from the typed value |
| `TERRAINTYPE=` — index, colour, `a`, `b`, `c`, `f` | 402 × 6 = 2,412 | **Reconstructed** from the typed value |
| `TILES=` and `TILESIZE=` | 26 × 2 + 26 × 2 = 104 | **Reconstructed** from the typed value |
| `LBM=` value | 26 | **Carried** — it is text |
| `TERRAINTYPE=` description and columns `d`, `e`, `g`, `h` | 402 × 5 = 2,010 | **Carried** — text, or a column with no sourced meaning |
| Comments, blank lines, alignment whitespace, field quoting, line order, trailing comments on record lines | all of it | **Carried** byte for byte |

Totals: 46,989 reconstructed, 2,036 carried as text. Comparing a carried text field with itself
would prove nothing, so those are counted separately and are deliberately not in the rebuilt figure.

That the 46,989 can fail is not a claim; it is tested. A column written `9|6`, `1|1` or `007`
parses to the same value as `6|9`, `1` and `7` and rebuilds differently, and
`the_field_audit_names_columns_whose_spelling_the_value_model_loses` asserts exactly those three
mismatches on a fixture built to contain them. **No shipped column is spelled that way**, which is
what makes the corpus figure 46,989 of 46,989 rather than a smaller number.

## What an edit can change

Every edit writes a **new** file. Nothing is written in place, nothing overwrites an existing
output, and naming the input as the output is refused by its own message rather than by
`create_new`'s "file exists".

| Command | Changes | Sourced by |
| --- | --- | --- |
| `--til-set-atlas IN NAME OUT` | the `LBM=` value | Observed in the corpus: all 26 name a `.lbm` and the renderer resolves the atlas through it |
| `--til-set-grid IN COLUMNS ROWS OUT` | `TILES=` | Observed in the corpus; see the refusals below, which is where most of this verb lives |
| `--til-set-tile-terrain IN TILE TERRAIN OUT` | a tile's `self` column | Observed in the corpus: `self` is how all 1,258,496 cells of 365 maps resolve to a terrain |
| `--til-set-tile-neighbour IN TILE n\|ne\|e\|se\|s\|sw\|w\|nw CONSTRAINT OUT` | one of the eight neighbour columns | Derived 2026-09-17 against the `terrainrings` captures: 576 of 576 ring tiles satisfy the geometric reading, 0 of 576 the mirrored one |
| `--til-set-terrain IN TERRAIN color VALUE OUT` | `TERRAINTYPE=` column 1 | Documented by the files' own header, `color`, in 26 of 26 |
| `--til-set-terrain IN TERRAIN description TEXT OUT` | `TERRAINTYPE=` column 2 | Documented by the files' own header in 26 of 26 |
| `--til-set-terrain IN TERRAIN passability VALUE OUT` | column `a` | Documented: 26 of 26 headers call it flags; see the vocabulary split below |
| `--til-set-terrain IN TERRAIN min-elevation VALUE OUT` | column `b` | Documented: 26 of 26 say `minimum elevation (b=1000 = 1.0 in map model)` |
| `--til-set-terrain IN TERRAIN max-elevation VALUE OUT` | column `c` | Documented: 26 of 26 say `maximum elevation` |
| `--til-set-terrain IN TERRAIN movement-cost VALUE OUT` | column `f` | Documented: 26 of 26 say `movement cost` |

## What it refuses, by name

The precedent is the IMP writer, which refuses a frame whose pixels another frame reads and **names
the frames** rather than silently repainting art nobody asked about. Each refusal below names the
record and the reason.

| Refusal | Reason |
| --- | --- |
| A `TILE=` row the file does not declare | There is nothing to edit and **nothing is minted**: a minted row needs eight neighbour columns that nothing in the file sources. The message gives the declared count and range. |
| A `TERRAINTYPE=` row the file does not declare | Same; the message lists the declared indices. |
| A `self` or constraint naming an undeclared terrain type | Such a tile can never be selected and its cells read as unknown terrain. The undeclared types are named. |
| Moving the **last** tile out of a terrain type | Every map cell holding one of that type's tiles resolves through it; emptying the type makes those cells unreadable. The terrain and its description are named. |
| A neighbour column of a row that did not declare all eight readably | The tile is already unpaintable; writing one column makes an incomplete row *look* complete. The first missing column is named. |
| A column the row stops before | Extending a row means inventing values for the columns in between. The field count is named. No shipped row is short. |
| Changing `columns` while any tile is declared | A slot's picture is `(index % columns, index / columns)`, so re-columning repaints **every** declared tile with a different image — art the caller did not name. The count is in the message, and it says that changing `rows` alone is safe. |
| A grid that would orphan declared tiles | The parser rejects a tile at or beyond capacity, so the file would stop loading. The orphaned tiles are listed. |
| Columns `d`, `e`, `g`, `h` | **No sourced meaning.** 25 of 26 headers call `d` food and `e` ore; `tilesb01.til` calls both unused; all 26 call `g` and `h` unused. Writing into a column whose meaning is a disagreement is minting a field. |
| `TILESIZE=`, any value — **including `32, 32`** | Changing it would **invent a capability**: nothing has been observed reading the line, so nothing says a different value would be honoured rather than ignored or crashed on. The refusal is about the field, not about the value written, which is why the same-value case is refused too. Note the caveat below: the reason is *not* "unobserved", or it would apply to four accepted columns as well. Library-only; there is no command for it. |
| The trailing `index` (pattern) column of a `TILE=` row | The column's rule is not known and is demonstrably unreliable — `tilesb01.til`'s tile 2 carries 6 where its pattern is plainly 2, and its tile 50 carries 1. Library-only. |
| An atlas name that is not a `.lbm`, is empty, or carries `,` `;` `"` `=` or whitespace | The first is unsourced; the rest would change how the line parses. |
| A passability above 2 | Outside both shipped vocabularies. |
| A palette index above 255 | The palettes hold 256 entries; every shipped terrain colour is in `90..=158`. |
| An inverted elevation range | No shipped row writes one and nothing says what the engine would do with it. |
| A description containing `,`, `;`, `"` or a control character, or an empty one | Would change how the line parses, or would leave the terrain unnamed. |

A refused edit **leaves no output file** and leaves the in-memory document byte-identical. Both are
asserted by tests, and the second needs a refusal that gets far enough to matter: most refusals
return before any byte is touched, so the restore is driven by
`an_edit_that_does_not_read_back_is_refused_and_the_line_is_restored`, which edits the line, has the
change rejected by the reader, and then asserts both the bytes and that a **subsequent** edit still
lands on the right offsets.

### The edit is verified through the parser, not through its own intention

Every setter replaces a span, re-encodes the whole file, and **re-parses those bytes**. If the
result does not parse, or parses to something other than what was asked, the original line is
restored and the caller is refused. A writer that reported success from having intended a change is
the tautology this repository has had to delete tests for.

The reachable case is a value the reader normalises: a description of `"  padded  "` passes every
guard, is written, and comes back from the reader as `padded`. Without the read-back check the
caller would be told the edit succeeded while the file says something else.

### A caveat on that refusal, stated rather than buried

"No observation says the engine reads this line" is true of `TILESIZE=` **and equally true of the
four `TERRAINTYPE=` columns this writer accepts** — `a`, `b`, `c` and `f` are sourced by the files'
own headers, which are the authoring tool documenting itself, not evidence about `lomse.exe`. If
unobservedness alone were the bar, those four would be refused too. `TILESIZE= 32, 32` is arguably
*better* corroborated than `movement-cost`, since the renderer and `tools/map_projection.py`
independently use a 32x32 tile.

The refusal stands anyway, on the narrower ground that every shipped file agrees on one value and
changing it would be claiming a capability nothing demonstrates. The four accepted columns are
accepted on the ground that they change a *value* within a field the format plainly carries. That
is a real distinction, and it is also a judgement call rather than a measurement — recorded here so
it can be overturned by an engine run rather than rediscovered as an inconsistency.

### Line endings, and one refusal that is not a gap

A `.til` whose records are separated by **bare CR** collapses to a single physical line and is
refused. That is deliberate, not an omission. This game does ship a bare-CR text format —
`settings.cfg` — but no `.til` uses one and nothing establishes that `lomse.exe` would read one, so
accepting it would be minting a capability at the level of line structure. It is also explicitly
*not* the `settings.cfg` failure mode, where a file parsed "successfully" while silently yielding
nothing for 22 of 23 keys: here it fails loudly.

What was wrong, and is fixed, is the *message*. Such a file used to be refused with "tile definition
has no TILES dimensions" — naming a key that is present, on a file whose problem is two lines up.
It now names the line endings and the count of records affected.

One splitter serves both the value model and the line model, so they cannot disagree about where a
line ends.

### A bound on one allocation sized from the file

A record line's field count comes from counting commas, so `MAX_RECORD_FIELDS` (64, against a
corpus that is uniformly 11) bounds it. Without it a row of ten million commas becomes ten million
allocations and `--describe-til` dies in the allocator instead of refusing. The bound is checked as
the fields are produced, not after.

## What is not determined

- **No written `.til` has ever been in front of the engine.** Not one. Every result here is about
  this project's reader and writer agreeing with the shipped bytes. An engine-acceptance ladder is
  being built separately, and a hand-edited tileset belongs on it; until it runs, "the engine loads
  it" is unevidenced.
- **Whether the engine reads `TERRAINTYPE=` at all**, and which columns it reads. The header
  comments are the *authoring tool's* documentation of its own file, not evidence about `lomse.exe`.
  No operator body has been traced to any of these columns. `self` and the eight neighbour columns
  are different: those are corroborated by 576 of 576 engine-saved ring tiles.
- **Whether `TILESIZE=` is read.** See above; it is refused for this reason.
- **What columns `d`, `e`, `g` and `h` mean.** The corpus disagrees with itself.
- **The pattern column's rule.** It is shared across terrain blocks and is inconsistent with any
  simple reading.
- **What the engine reads past a map edge**, which the neighbour columns interact with. Every
  `terrainrings` capture is interior, so no saved artifact says.
- **Whether a tileset may declare a grid other than 16 columns wide.** All 26 are 16; nothing says
  whether that is a constraint or a habit. The writer refuses a `columns` change for a different
  reason — the repaint — not because 16 is believed to be required.
- **Whether adding a `TILE=` or `TERRAINTYPE=` row is safe.** This writer never appends one, so the
  question is open rather than answered. Appending is the obvious next capability and it needs
  either an engine run or a source for the eight neighbour columns of a new tile.
- **Whether `MAX_ATLAS_CAPACITY` (1,024) reflects anything in the engine.** It does not; it is a
  conservative guard matched to the map writer's own tile-index guard. The largest shipped tileset
  declares 624.

## Reproducing

```
# build the MPQ helper and extract the 26 members
scripts/build-tools.sh
.build/lom-mpq extract PIC.MPQ OUTDIR --listfile artifacts/reference-listfiles/lords-of-magic.txt

# the corpus sweep, straight out of the archive
lom-asset-viewer --til-roundtrip PIC.MPQ --listfile artifacts/reference-listfiles/lords-of-magic.txt

# one file
lom-asset-viewer --describe-til OUTDIR/til/tilesb01.til
```

Game data is read-only and none of it is committed. The sweep reads `pic.mpq` in place and writes
nothing.
