# Savegame Format (`.sav`, `.lom`, and extensionless)

## Status

**The container is solved and all nine sections now decode**: `LS_VER_`, `LS_MULT`, `LS_MAP_`,
`LS_USER`, `LS_GAME`, `LS_PLR_`, `LS_REGN`, `LS_ALRM` and — new later on 2026-09-18 — `LS_SPR_`.
A native Rust parser (`spikes/asset-viewer/src/save.rs`) reads every save in every install on this
machine without modifying it.

**`LS_SPR_` was the last one, and it went the same way the three before it did: through the
writer, and then through the ten class readers the reader's jump table dispatches to.** Its
records are polymorphic and variable-length, three of the eight real classes embed a whole
class-0 record inside themselves, and one embeds it recursively through an array. The section's
*structure* is now decoded end to end; the *meaning* of its fields is not, and those bytes are
carried verbatim. See [`LS_SPR_`](#ls_spr_--units-armies-and-heroes) and
[What is not determined](#what-is-not-determined).

**What carries the claim, and what does not.** The evidence for the record layouts is the
**disassembly**, corroborated by a **version sweep against an independently written fixture** —
a second transcription of the same readers with literal gate values, so parser and fixture can
genuinely disagree. A **byte account** over the corpus corroborates the aggregate extents:
31 of 31 files, zero slack.

`LS_SPR_` also re-emits byte-identically from its decoded records in 31 of 31 files, and the
whole file reassembles byte-identically with `LS_SPR_` regenerated in 31 of 31. **Corrected,
2026-09-18: an earlier draft of this page called that "the check that makes the record model
falsifiable". It is not.** Once `parse` succeeds the re-encode is an *identity*, so it proves
**lossless preservation and correct container splicing** and nothing more. It does not prove any
record's internal field boundaries and does not exclude compensating errors — see
[what the round trip proves](#what-the-round-trip-proves-and-what-it-cannot). There is still no
savegame writer; the other eight payloads are copied through unchanged.

### The three new sections were decoded from the writer, not from the files

This pass did **not** infer structure from the save bytes. Five shipped scenarios cannot support
structural inference and the previous pass said so in as many words — "three observed lengths
(8,998 / 9,389 / 9,780) and no known structure" is what corpus arithmetic had to offer after
exhausting itself.

The writer at `0x00482AF0` was walked instead, section by section, into the per-record writers it
calls, and each recovered model was then **checked against the files as a byte account**: parse the
section with the model and assert the cursor lands exactly on the section's end. That check has
nothing to tune. Every length in these three sections is either a constant in the instruction
stream or a count the file itself stores, so a model that is merely plausible stops short or runs
off the end. All three land exactly, in every file, with zero slack.

The corpus then confirmed things the disassembly had already decided:

- `LS_REGN`'s three tail lengths differ by **exactly 391 bytes**, which is the fixed part of the
  region record read off `0x004C5840`. The previous pass was looking at one record's width three
  times without a record to compare it to.
- `LS_ALRM`'s "header word 7 = 16" is the **length of the string `monstergenerator`**.
- `LS_PLR_`'s version-108 records are **exactly 12 bytes shorter** than version 111's, which is the
  width of the three fields the reader gates at `0x004BCD9F` and `0x004BCDD0`.

### The corpus is eleven game states across 31 files, not ten across 24

**Corrected twice on 2026-09-18, and the two corrections have different causes.** The first pass
counted seven states across twenty files and had missed an install. The second pass found
`Lords of Magic GS5R3.app` and wrote "ten states across 24 files" — but its own table's rows sum
to **30**, not 24, so that headline was an arithmetic slip independent of any search. Counted
from the directories at 21:57:

| install | files | distinct states |
| --- | ---: | ---: |
| Steambuild 32 64bit DXVK | 6 | 6 |
| Lords of Magic Development | 6 | 6 (the same six) |
| Lords of Magic 3.02 | 8 | 7 |
| Lords of Magic GS5R3 | 11 | 10 |
| **union** | **31** | **11** |

`save_survey` reports **0 failures on all 31 files**, and every check in this document was
re-measured across all eleven states in that run rather than carried over from the previous one.

**And the corpus is a live directory, which is the more interesting half of this.** The five
`.lom`-family files under GS5R3 — `combat.lom`, `endturn.lom`, `temple.lom`, `lastsave.lom` and
`Merlin I` — all carry mtimes between **20:39 and 20:47 on 2026-09-18**, which is after some of
this document was drafted. `temple.lom` is not a file the previous pass overlooked so much as one
that may not have existed when it looked, and the other four may have been re-saved under it.

So there are two lessons here, not one:

1. **A headline count typed into prose goes stale.** Twice now it has been wrong, in opposite
   directions — once by missing an install, once by not adding up its own table. Neither changed
   a structural conclusion, because a check that holds in 24 files holds in 31, but both
   misreported how much corroboration a claim had. `save_survey` prints the count it measured, so
   a reader never has to trust a number typed here.
2. **This corpus is the user's live savegame directory, not a frozen fixture.** It grows and
   changes between runs, and a per-file measurement quoted in this document is a measurement of
   whatever that path held at the time. Anything that must not move belongs in a synthetic
   fixture — which is where every test already is.

**The caution the previous passes wrote still stands.** Six of the eleven states are authored demo
scenarios shipped together and may share a generator; their agreement is weaker evidence than its
count suggests. Five states are genuine play — the turn-315 state under 3.02 and the four GS5R3
ones — and where an invariant holds, it holding *there* is what matters. **No original save data is
stored in Git.** The survey takes a directory at runtime; every test uses synthetic fixtures built
in code.

### Structural requirements versus corpus regularities

The survey reports two kinds of check and treats them differently, because they are different
claims.

**Structural checks** are the format's own requirements — the `LS_MULT` length arithmetic, the
`LS_MAP_` accounting, `LS_USER` being exactly eight records, the `LS_PLR_` terminator. A violation
means the file is malformed or this project's model is wrong. These **fail the run**.

**Corpus regularities** are things every inspected save happens to do that nothing in the format
requires: `N - live_count == 71`, visibility taking only three values, the `LS_REGN` tail being one
of three lengths, the three turn fields agreeing. **A real save that breaks one of these is a
discovery, not a bad file** — a save with a surplus of 72 would be the most interesting file in the
corpus. These are reported with their measured values and **do not fail the run**. Reporting them as
errors would train a reader to ignore exactly the signal worth acting on.

The structural checks are also **computed from the raw bytes on the container, not from the parsed
structs, and they run even when parsing fails.** An invariant read off a struct that `parse` already
validated cannot report a failure — `parse` rejected the file before the caller saw it, which is the
same defect as a test that cannot fail. Re-deriving them from the bytes makes them a second
implementation that can genuinely disagree with the first, and hanging them off the container is
what lets them say *which* section is malformed on a file the parser refuses.

---

## The container

**Observed in a local binary, 2026-09-18.** A save is a bare concatenation of nine sections. Each
section is an 8-byte NUL-padded ASCII tag followed immediately by its payload:

```text
section := tag[8] ++ payload
```

**There is no length word and no count word between a tag and its payload.** The `u32` that follows
a tag is already the first field of that section's own structure, and it means something different
in every section. A section's extent runs to the next tag, or to end of file for the last one. Only
`LS_MULT` happens to store a real byte length in that position.

The nine tags live as one contiguous array of nine `char[8]` in the binary, immediately followed by
the dword `111` which serves both as the loop's sentinel and as the build's format-version constant:

| VA | tag |
| --- | --- |
| `0x0055B168` | `LS_MAP_` |
| `0x0055B170` | `LS_SPR_` |
| `0x0055B178` | `LS_USER` |
| `0x0055B180` | `LS_GAME` |
| `0x0055B188` | `LS_PLR_` |
| `0x0055B190` | `LS_VER_` |
| `0x0055B198` | `LS_REGN` |
| `0x0055B1A0` | `LS_ALRM` |
| `0x0055B1A8` | `LS_MULT` |
| `0x0055B1B0` | dword `111` — sentinel and version constant |

### The reader dispatches on the tag; order is meaningless

**Observed in a local binary, 2026-09-18.** The loader at `0x0048322A` is a **tag-dispatch loop**,
not a fixed sequence: it reads eight bytes, `memcmp`s the first **seven** of them — the eighth byte
is never examined — and jumps through the 9-entry table at `0x00483970`. An unrecognised tag aborts
the load.

So the reader **structurally accepts any order** (**Observed in a local binary**). Every file
inspected here happens to store VER, MULT, MAP, SPR, USER, GAME, PLR, REGN, ALRM in that order, and
the parser depends on none of it — the tests permute the order and assert the decoded content is
unchanged.

**But structural order-freedom is not semantic order-independence, and the earlier draft of this
section overstated it.** The nine handlers share the singleton at `0x005AA12C`, and at least one of
them reads a field another writes: **`LS_MULT`'s handler consults the version that `LS_VER_`'s
handler stores** (**Observed in a local binary** — it is the gate at
[`MULTIPLAYER_SLOTS_MIN_VERSION`](#ls_ver_--the-format-version) that decides whether the 576-byte
slot block is present). Put `LS_MULT` before `LS_VER_` in a pre-99 save and the `LS_MULT` handler
reads whatever version the singleton happened to hold.

So, precisely:

| claim | class |
| --- | --- |
| the reader structurally accepts the nine sections in any order | **Observed in a local binary** |
| `LS_VER_` -> `LS_MULT` is a real semantic ordering dependency through the shared singleton | **Observed in a local binary** |
| full semantic order-independence | **not established** — the other handlers' singleton reads have not been audited |

This parser reproduces the one known dependency explicitly: `SaveFile::parse` parses `LS_VER_` first
and passes it into `MultiplayerSection::parse`.

### Sections are not skippable, which is why this parser scans

Because there is no length word, the only way to find the end of a section is to decode it. Two
sections cannot be decoded today. So the parser does **not** reimplement the engine's loop: it scans
the whole file for the nine tag byte-strings and takes each section's extent as running to the next
one found.

**That shortcut is only honest with a census.** A tag byte-string could in principle appear inside
payload data, and nothing in the format forbids it. `SaveContainer::locate` therefore requires
**exactly nine distinct tags, each occurring exactly once**, and refuses with a structured
`SaveError::TagCensusFailed` carrying the full per-tag counts otherwise. `TagCensus::take` is public
so a caller diagnosing a refused file can see the counts without a parse. A test plants a duplicate
`LS_GAME` tag inside `LS_REGN`'s payload and asserts the refusal.

Measured on the corpus: **9/9 in all 31 files**, so no payload in any of them accidentally contains
a tag.

**What the census does and does not prove.** It bounds *accidental misparsing*; it is **not integrity
checking**, and the distinction matters in both directions:

- **A count of one does not prove the located occurrence is the real tag.** A tag sequence hidden
  inside the opaque `LS_REGN` tail, combined with a corrupted real tag, still yields a census of one
  each — and the parser would then happily carve the file at the wrong offset.
- **A perfectly valid save is rejected if its payload happens to contain a seven-byte tag
  sequence.** Nothing in the format forbids it. This is a false positive the design accepts
  deliberately, because carving at a wrong offset silently is worse than refusing loudly.

The guard is still the right guard — it correctly refused a `.DS_Store` when the survey was pointed
at the wrong directory — but it is a sanity bound, not a checksum. The format has no checksum.

### Three functions own the format

**Observed in a local binary, 2026-09-18.**

| VA | role |
| --- | --- |
| `0x00482AF0` | writer |
| `0x00483120` | full loader |
| `0x00483010` | header-peek reader: reads `LS_VER_` and `LS_MULT` only, then closes the file |

**The writer is the map of the format, and it is worth reading before anything else.** It emits the
nine tags in a fixed order and, between them, calls one routine per owned sub-object. Every section
below whose structure is known was recovered by following those calls:

| tag pushed at | routine | owns |
| --- | --- | --- |
| `0x00482B69` `LS_VER_` | inline | the version dword at `0x0055B1B0` |
| `0x00482BA0` `LS_MULT` | `0x004839A0` | setup block + sixteen slots |
| `0x00482C63` `LS_MAP_` | `0x004A5440`, `0x004A54E0`, `0x004C8FC0` | the map |
| `0x00482C97` `LS_SPR_` | `0x004F6BC0` | the sprite table |
| `0x00482CB5` `LS_USER` | `0x0052D040` | eight user records |
| `0x00482CE5` `LS_GAME` | inline + `0x0052B3B0` | turn and the record table |
| `0x00482E22` `LS_PLR_` | `0x004BCE20` per player | per-player state |
| `0x00482F3A` `LS_REGN` | `0x004C7390` | grid and region table |
| `0x00482F6A` `LS_ALRM` | six calls, `0x0040B7D0` .. `0x0040F600` | six alarm queues |

`this` is the static singleton at `0x005AA12C`. The header-peek reader backs a pre-load "who was in
this game" screen, which is why `LS_MULT` is the one section with a genuine forward-compatible
length.

### No compression and no encryption

**Observed in a local binary, 2026-09-18**, established four independent ways, including raw
`fwrite` of struct memory in the writer. `combat.sav` measures 3.01 bits/byte entropy with 56% zero
bytes and plaintext unit names. A caveat previously attached to `LS_REGN` alone is
[withdrawn](#corrected-0x0054d0d8--0x0054d0dc-is-a-lock-not-a-transform): the two imports bracketing
its I/O are a lock pair, and its bytes decode with no transform applied.

---

## Per-section table

| tag | payload | state | leading `u32` means |
| --- | ---: | --- | --- |
| `LS_VER_` | 4 | decoded | the format version |
| `LS_MULT` | 744 | decoded | `sizeof` of the setup block (164) — **the only real length in the format** |
| `LS_MAP_` | 196,628 | decoded | map width |
| `LS_SPR_` | varies | decoded | live record count |
| `LS_USER` | 6,272 | decoded | record 0's own index (`0`) |
| `LS_GAME` | varies | decoded | the turn number |
| `LS_PLR_` | varies | decoded | the first record's slot index |
| `LS_REGN` | varies | decoded | region-grid width |
| `LS_ALRM` | to EOF | decoded | the record count of alarm queue 0 |

---

### `LS_VER_` — the format version

**Observed in a local binary, 2026-09-18.** The payload is exactly four bytes. `111` in 27 of the 31 files and `108` in the four copies of `quickstart`.

**The engine's version handling is a monotone feature gate that never rejects.** The handler's only
error path is a short read. Every subsequent test against `[0x005AA12C]` is `jl` or `jge`; there is
**not one** `jg`, `ja` or `jne` on that field anywhere in the binary. There is no floor and no
ceiling. 40 distinct version constants appear, spanning 50..111.

The parser therefore imposes no version range either — a version is data, not a gate. Tests assert
parsing succeeds at 0, 50, 108, 111, 9999 and `u32::MAX`.

One version-gated behaviour is known: **below version 99 the reader synthesizes `LS_MULT`'s
576-byte slot block from memory instead of reading it** (`MULTIPLAYER_SLOTS_MIN_VERSION`).

---

### `LS_MULT` — game setup and the sixteen lord slots

**Observed in a local binary, 2026-09-18.** 744 bytes in all 31 files, accounting exactly:

```text
  u32 = 164          a hardcoded sizeof; the reader USES it as the read length
  164 bytes          game-setup struct, copied from [gameobj+0x520]; fields Unknown
  576 bytes          16 x { u32 lord_code; char name[32] }   (36 bytes each)
```

`4 + 164 + 576 = 744`. This is the one forward-compatible section, because the stored 164 is used as
an actual length — so the parser reads the block at its *declared* length rather than at the
constant, and a test drives that with declared lengths of 0, 100, 164 and 300.

Slots 0..8 carry a code and slots 8..16 carry `0xFFFFFFFF` in every file. **Occupancy is the code,
never the name**: in both turn-315 files, slots 1 and 4 carry live codes `0x43` and `0x35` with an
**empty** name. Treating an empty name as an absent player drops two live players.

Slots 8..16 are never presented as data.

#### The name padding is uninitialised process memory

**Observed in a local binary, 2026-09-18.** The engine `strcpy`s a name into the 32-byte field from
`[player_i + 0x50AC]` with no preceding `memset`, so whatever the buffer held goes to disk.

This was measured, not inferred. `lastsave.lom` and `Merlin I` have different md5s. Diffed section
by section they are **the same saved game state**:

| section | bytes | differing bytes |
| --- | ---: | ---: |
| `LS_VER_` | 4 | 0 |
| `LS_MULT` | 744 | **356** |
| `LS_MAP_` | 196,628 | 0 |
| `LS_SPR_` | 306,853 | 0 |
| `LS_USER` | 6,272 | 0 |
| `LS_GAME` | 32,316 | 0 |
| `LS_PLR_` | 42,414 | 0 |
| `LS_REGN` | 107,701 | 0 |
| `LS_ALRM` | 7,939 | 0 |

All 356 differing bytes lie in `LS_MULT`, and **every one of them is strictly past a name's NUL
terminator** — checked byte by byte: 356 past the terminator, 0 elsewhere. The first differing byte
in the whole file is payload offset 179, which is slot 0's name field byte 11, exactly one past the
NUL that terminates `"Merlin"`. The leaked bytes are recognisable Win32 stack and heap pointers:
`0x004d3756`, `0x01bfbc70`, `0x02fc101c`, `0x04537e44`.

Three consequences:

1. **A `.sav` is not a pure function of game state.** Two saves of one state differ on disk. No test
   and no invariant may assume save bytes are reproducible.
2. **Any save-diffing tool must mask the padding**, from `NUL+1` to `+35` in each of the sixteen
   slot records, or it reports two identical states as different. This nearly produced a wrong
   conclusion during this work: the md5 difference is real and the inference from it — "two
   independent player saves" — was not. The survey therefore groups files by a digest computed over
   *decoded* content with the padding excluded, and reports **7 distinct game states across 14
   files**.
3. **Saves carry fragments of process memory.** Benign in this corpus — pointers, not user data —
   but worth stating plainly in a project whose point is people sharing and modding these files.

`LordSlot::name()` stops at the terminator and `LordSlot::name_padding()` exposes the remainder
separately, so a caller must opt in to the leak rather than receive it silently.

---

### `LS_MAP_` — the world map

**Observed in a local binary, 2026-09-18.** 196,628 bytes in all 31 files, accounting exactly
with no slack:

```text
  u32 width            = 128
  u32 height           = 128
  u32 bytes_per_cell   = 8
  width*height*8       = 131,072    the cell grid
  u32 count            = 16,384     ( == width*height )
  count * u32          = 65,536     a second, parallel per-cell plane; meaning Unknown
  u32                  = 1          trailer; meaning Unknown
```

`4 + 4 + 4 + 131072 + 4 + 65536 + 4 = 196,628`.

This is the standalone map format **minus its leading `metadata` word**: `src/map.rs` reads
`metadata, width, height, bpp`, and the save has `width, height, bpp` only. The parser re-attaches a
zero `metadata` word and hands the bytes to `MapAsset::parse`, so cell decoding is never duplicated
and the save path cannot drift from the `.scn`/`.smp` path.

Cell facts inherited from `docs/map-format.md` and not re-derived here: cells are packed
`y*width + x`; the cell's first word is two `u16` fields.

#### Corrected: "128*128*12 + 16" was a false arithmetic fit

**Corrected, 2026-09-18.** An earlier reading described this section's span as `128*128*12 + 16`,
a 12-byte cell in a single array. **There is no 12-byte cell.** There are 8-byte cells in one array
and 4-byte words in a *separate, separately counted* array, with a count word between them and a
trailer after.

The arithmetic is worth writing out, because it shows precisely how the reading survived:

| term | false fit | truth |
| --- | ---: | ---: |
| bulk | `16384 * 12` = **196,608** | `16384 * 8` + `16384 * 4` = **196,608** |
| scaffolding | `+16` | `12` header `+ 4` count `+ 4` trailer = `+20` |
| total | 196,624 | **196,628** |

**The bulk term is exact.** `8 + 4` really is `12` bytes of per-cell data, so any regrouping of the
two arrays into one produces the identical figure, and the only thing separating the two readings in
the total is a 4-byte discrepancy in the scaffolding — small enough to be waved through as an
off-by-one in the header.

The lesson is not "check your arithmetic". It is that **an accounting check comparing only a sum
cannot fail on a regrouping of its terms**, and here the terms regrouped exactly. The parser
therefore checks the *structure* as well as the total: `MapSection::plane_covers_every_cell` asserts
the stored plane count equals the cell count, which no regrouping can satisfy, because a single
12-byte-cell array has no second count word to agree with. A test halves the stored plane count and
confirms the structural check fails independently of the byte total.

#### The `+2` field: visibility

**Observed in a local binary, 2026-09-18.** Across all cells of all 31 files, the `+2` field
takes **exactly three values and no others: 0, 63 and 128.**

**Inferred, 2026-09-18: these are visibility levels — unexplored, dim, fully visible.** The evidence
is a distribution, not a measurement:

| file | 0 | 63 | 128 |
| --- | ---: | ---: | ---: |
| `lastsave.lom` / `Merlin I` (turn 315, real play) | 7,840 | 7,403 | 1,141 |
| `combat.sav` | 196 | 990 | 15,198 |
| `experience.sav` | 196 | 1,105 | 15,083 |
| `magic.sav` | 126 | 68 | 16,190 |
| `merc.sav` | 124 | 72 | 16,188 |
| `temple.sav` | 190 | 204 | 15,990 |
| `quickstart` | 151 | 0 | 16,233 |

The one genuine mid-game state is the only one that looks like a partly-explored map; the authored
demo saves are almost entirely `128`. It agrees with the dimming expression `(0x80 - field) * k >> 7`
at `0x00519ced`.

**What is Observed is only that the field takes exactly three values.** The full `0..128` range and
any saturation behaviour are **not** observed, and nothing in this project asserts them.

The three-value set *is* asserted — `OBSERVED_VISIBILITY_LEVELS`, and the survey reports a violation
— but **as a corpus regularity, not as a format requirement**, so a save with a fourth level is
reported as a finding and does not fail the run. An earlier draft of this paragraph claimed the
assertion did not exist at all, which was simply wrong: it exists, and what matters is which class
it sits in.

#### Refuted: `0x00800000` is not a flag

**Refuted, carried forward from `docs/map-format.md`.** Cell word 0 is two `u16` fields:
`tile_index = tag & 0xffff`, and the `+2` field, which is a **signed scalar and not a bitfield** —
all eleven readers in the binary are `movsx` and none masks. `0x00800000` set is simply a `+2` field
holding `128`, a maxed-out *number*. `cell_visibility()` and `cell_tile_index()` in `save.rs`
implement this reading.

**A divergence worth flagging, left unfixed as out of scope.** `MapCell::tile_index()` in
`src/map.rs` masks only `0x00800000` (`tag & !CELL_TAG_HIGH_FLAG`), not `tag & 0xffff`. On the
standalone-map corpus those agree, because bits 16..23 are otherwise zero there. They are not the
same function, and the `save.rs` accessors deliberately do not route through the `map.rs` one.
Reconciling `map.rs` is a separate change with its own corpus to re-verify against.

---

### `LS_SPR_` — units, armies and heroes

**Observed in a local binary, 2026-09-18.** `u32 count`, then `count` polymorphic records:

```text
  u32 count
  count * { u32 class_id ; the class's own record }
```

The reader is at **`0x004F7122`**. It reads the count, then per record reads a `u32 class_id`,
bounds it with `cmp eax,9 / ja` and jumps through the ten-entry table at **`0x004F73B8`**. Each arm
allocates an object of a class-specific size, runs that class's constructor, stores the owning
table at `object+0x40`, and calls the object's reader at **`vtable+0x24`**. The writer at
`0x004F6BC0` is the mirror image and calls `vtable+0x20`.

#### Corrected: the table at `0x004F73B8` belongs to the reader, and the reader's slot is `+0x24`

The previous pass recorded the dispatch as "`call dword [eax+0x20]`, dispatched through a 10-entry
jump table at `0x004F73B8`". Those are two different call sites. `[eax+0x20]` is the **writer**'s
virtual call, at `0x004F6C47`, and it is not dispatched through a jump table at all — the writer
never switches on the class id, it just writes it and calls the object's writer. The jump table is
the **reader**'s `switch (class_id)` at `0x004F71A9`, and what it selects is which class to
*construct*; the read itself then goes through `[edx+0x24]` at `0x004F731A`. The address was right
and what it dispatches was not.

#### The ten entries

**Observed in a local binary, 2026-09-18**, read out of `lomse.exe` under
`Lords of Magic Development.app`. All four installs ship a byte-identical `lomse.exe`.

| id | jump target | allocation | constructor | vtable | writer `+0x20` | reader `+0x24` | on-disk reader |
| ---: | --- | ---: | --- | --- | --- | --- | --- |
| 0 | `0x004F71B0` | 1500 | `0x00411610` | `0x0054D3A8` | `0x00412090` | `0x00412590` | `0x004122A0` |
| 1 | `0x004F71E1` | 96 | `0x0050C0B0` | `0x0054E910` | `0x0050D930` | `0x0050DD70` | `0x0050DA70` |
| 2 | `0x004F7213` | 148 | `0x0043B930` | `0x0054D4D8` | `0x0043D000` | `0x0043D1A0` | `0x0043D0A0` |
| 3 | `0x004F7248` | 844 | `0x0044E850` | `0x0054D630` | `0x00451610` | `0x004517A0` | `0x004516A0` |
| 4 | `0x004F727D` | 120 | `0x004EF6F0` | `0x0054DD88` | `0x004F0C80` | `0x004F0D80` | `0x004F0D80` |
| 5 | `0x004F73A3` | **raises** | — | — | — | — | — |
| 6 | `0x004F73A3` | **raises** | — | — | — | — | — |
| 7 | `0x004F72A8` | 196, pooled | `0x0047CA70` | `0x0054D968` | `0x0047CCD0` | `0x0047CDA0` | `0x0047CDD0` |
| 8 | `0x004F72AF` | 88 | `0x004BA150` | `0x0054DC68` | `0x004F6A80` | `0x004F6B00` | `0x004F6B00` |
| 9 | `0x004F72DA` | 328 | `0x004ACF00` | `0x0054DAF8` | `0x004AD910` | `0x004ADB40` | `0x004ADA10` |

"Allocation" is the size in **RAM**. It is not the on-disk record length and is not close to it —
class 0's object is 1500 bytes and its smallest possible record is 202. Classes 4 and 8 have no
separate on-disk reader — the `vtable+0x24` entry point *is* the reader, with no registration
wrapper in front of it.

Three things in that table are worth stating outright:

- **Ids 5 and 6 are not classes.** They share one target that pushes a string and raises. A save
  carrying either is rejected by the engine, and this parser rejects it too.
- **Id 7 is not allocated.** Its arm calls a pooled factory at `0x0047CBD0`, which either recycles
  a freed block or allocates 196 bytes at `0x0047E9E8`. It is also the one arm that skips the
  exception-state store the other eight perform, because there is no constructor call to unwind.
- **Id 8 is the base class itself.** Its vtable's writer and reader slots hold `0x004F6A80` and
  `0x004F6B00` — the base implementations, unchanged. That is why its record is the base block and
  nothing else.

There is **no RTTI in the binary** (the word before each vtable is float data, not a locator), so
none of these classes can be named from the executable. They are referred to by id throughout.

#### Every record opens with its class id twice

**Observed in a local binary, 2026-09-18.** The base reader at `0x004F6B00` reads six dwords, with
no version gate anywhere in it, into `this+4`, `+0x1C`, `+0x20`, `+0x24`, `+0x28` and `+0x30` — 24
bytes, always. `this+4` is where the class id lives in the object, so the base reader reads the
class id **a second time**, the container's dispatch loop having already consumed one.

**Corrected, 2026-09-18: this is a deduction, not a prediction, and an earlier draft of this page
claimed more for it than it can carry.** That draft said it was "a prediction of the disassembly
and not a pattern noticed in the files" which "could have failed and did not". It could not have
failed. The **writer** puts the same field on disk twice — the outer writer reads `[object+4]` at
`0x004F6C25` and the base writer writes `[this+4]` at `0x004F6A8B` — so every save this engine
produces carries the equality *by construction*, whatever is right or wrong about any record
boundary downstream. This is the repository's own "agreement is not confirmation" lesson: two
writers writing one field always agree.

**What measuring it buys is one-way, and the first draft of this paragraph got even that wrong.**
It claimed agreement "confirms per record that a file is aligned, uncorrupted, and written by the
expected writer". It does not. Flip any byte of a record's class-specific **body** and leave the
two class-id dwords alone: the check still reports zero disagreements, `regularities()` still
passes, and the survey still counts the file clean while it is corrupt. The implication runs one
way only:

- **Disagreement** proves something is wrong — corruption, a misaligned parse, or another writer.
- **Agreement** proves only that those two dwords match. It is not an integrity check on the file,
  on the record, or on anything else.

On a format with no checksum a one-way detector is still worth having, and `save_survey` evaluates
it per file with `SaveFile::regularities` carrying it, so the count below is reproducible rather
than asserted and a future disagreement is reported as a **discovery about the file** instead of
passing in silence.

**Observed in the corpus, 2026-09-18:** 0 disagreements in 31 of 31 files, across all 25,000-odd
records — which, per the above, means no file *failed* the detector, not that any file is sound.
It is **not** evidence about the layouts, and in particular it cannot detect a wrong split of the
24-byte base block.

```text
base := u32 class_id_echo ; u32 +0x1C ; u32 +0x20 ; u32 +0x24 ; u32 +0x28 ; u32 +0x30
```

So the minimum record is **28 bytes**: the dispatch word plus the base block. That is exactly a
class-8 record.

**Every byte count on this page includes the four-byte dispatch word**, so that "record" means the
same thing in every sentence. Class 0's smallest possible record at version 111 is **202 bytes**
on that convention, and 180 for a pre-`0x33` save.

#### The eight class record layouts

Written for format version 111. Every gate is a signed `jl`/`jge` against `[0x005AA12C]`, as
everywhere else in this format. `V` below is the save's `LS_VER_` version.

**Class 0** (`0x004122A0`) — the big one, and the one three other classes embed:

```text
  base
  u32 +0x44 ; u32 +0x48 ; u32 n_slots ; u32 +0x50
  n_slots * slot                      ( see below )
  u32 +0x53C ; u32 +0x540 ; u32 +0x544 ; u32 +0x548
  u32 len ; len bytes                 ( 0x00427A40, see "not a string" below )
  V <  0x3F : four dwords read and discarded
  u32 +0x54 ; u32 +0x58 ; u32 +0x64
  88 bytes -> +0x68
  V >= 0x33 : u32 +0x54C
  V >= 0x38 : u32 +0x550
  V >= 0x3D : six dwords +0x5B0 .. +0x5C4
  V >= 0x42 : ( V >= 0x5D ? 4 : 1 ) bytes -> +0x5CC ; 1 byte -> +0x5D0
  0x59 <= V <= 0x5B : u32, whose truth sets bit 3 of +0x30
  V >= 0x6A : 1 byte -> +0x5C8
```

**The slot** (`0x00524D70`), one of class 0's `n_slots`:

```text
  W bytes                             ( W from the ladder below )
  if the word at blob+0x14 is non-zero, and V >= 0x37:
      u32 nested_type_id              ( bounded 0..=3 by the factory at 0x0044B4F0 )
      the nested class's reader
  u32 n_a ; n_a * item_a
  V >= 0x3E : u32 n_b ; n_b * item_b
```

`W` is a five-rung ladder on the version, `0x00524E3C`: **32** below `0x48`, **36** at `0x48`,
**40** below `0x65`, **72** below `0x67`, **76** at and above. The nested-object gate is a field
**inside the blob the slot has just read** — one of the few places in this format where a read
length depends on data rather than on the version.

**The two list items**, both fixed-shape:

| reader | v111 length | shape |
| --- | ---: | --- |
| `item_a`, `0x00526C20` | 28 | four dwords; `V >= 0x43` adds two; `V >= 0x51` adds one |
| `item_b`, `0x00427FA0` | 20 | three dwords; `V >= 0x4B` adds one; `V >= 0x58` adds one |

**The four nested classes** behind the factory at `0x0044B4F0` (allocations 40, 20, 20, 72;
constructors `0x0044B730`, `0x0044B8E0`, `0x0044B950`, `0x0044B970`). Their reader is
`vtable+0x08`, not `+0x24` — a different, smaller class family. All four begin with the shared
base at `0x0044B660`, **12 bytes at v111**:

| type | reader | v111 length | shape |
| ---: | --- | ---: | --- |
| 0 | `0x0044B810` | 32 | base; `V < 0x3C` adds 16; one dword; `V >= 0x4F` adds three; one dword |
| 1 | `0x0044B920` | 12 | base; `V < 0x3C` adds one dword |
| 2 | `0x0044B920` | 12 | identical reader to type 1, reached through a distinct vtable |
| 3 | `0x0044BBC0` | variable | base; `V >= 0x55 ? u32 groups : 6`; per group `u32`, `u32 k`, `k * item_a` |

**Classes 1, 2 and 3 embed a whole class-0 record**, base block included, behind a `u32` flag.
Class 1 embeds it twice over: once directly and once per element of a `u16`-counted array.

```text
class 1   0x0050DA70
  base
  u32
  V >= 0x62 ? 2 bytes : 8 bytes
  V >= 0x36 : u32 ; u32 flag ; if flag: an embedded class-0 record
  V >= 0x4A : u32
  V >= 0x60 : 1 byte
  V >= 0x66 : u16 n ; n * { 8 bytes ; u32 flag ; if flag: an embedded class-0 record }

class 2   0x0043D0A0
  base ; u32 len ; len bytes ; u32
  V >= 0x3B : u32 flag ; if flag: an embedded class-0 record
  V >= 0x5A : u32

class 3   0x004516A0
  base
  V >= 0x34 ? ( u32 len ; len bytes ) : 700 bytes with no stored length
  u32 flag ; if flag: an embedded class-0 record
  V >= 0x53 : u32 n ; n * 36 bytes      ( items at 0x00452CE0, no gates )

class 4   0x004F0D80   base + eleven dwords            = 68 bytes, no gates
class 7   0x0047CDD0   base + eleven dwords            = 68 bytes, no gates
class 8   0x004F6B00   base                            = 24 bytes, no gates
class 9   0x004ADA10   base + 92 bytes + ten dwords    = 156 bytes, no gates
```

Class 1's array count is a **`u16`**, not a `u32`: `fread(this+0x4E, 2, 1)` at `0x0050DBD1`, and
the loop bound is re-read as a signed word at `0x0050DD35`. Reading four bytes there consumes two
too many and derails the rest of the section.

#### Counts in this section are signed, and a non-positive one skips its loop

**Observed in a local binary, 2026-09-18.** Six counts are read into a register and then tested
with a **signed** branch before their loop is entered. A non-positive value is not an error to the
engine — it skips the loop, and in the counted array's case skips the read as well as the
allocation:

| count | guard | note |
| --- | --- | --- |
| the **top-level record count** | `0x004F717F` `cmp eax,0 / jle` | continues with `jl` at `0x004F7336` |
| class 0's slot count | `0x004122FC` `test eax,eax / jle` | |
| the counted byte array's length | `0x00427A5E` `test eax,eax / jle` | skips the allocation *and* the read |
| class 1's array count | `0x0050DC81` `cmp word,0 / jle` | a **word**; `movsx` at `0x0050DD35` |
| nested type 3's group count | `0x0044BCC7` `test eax,eax / jle` | |
| class 3's tail count | `0x00452EBF` `cmp eax,0 / jle` | |

This matters more here than it would elsewhere, because `LS_SPR_` can now **fail**. When the
section decoded to its count only it could not refuse anything; now a modelling error in it
refuses the **whole file**. A save whose entire `LS_SPR_` payload is `FF FF FF FF` is one the
engine loads and consumes nothing from, and a parser that read the count unsigned would attempt
four billion records and reject it. No corpus file does this today — which is exactly the
reachability argument a new save invalidates.

**Three counts are not guarded this way and are deliberately not treated as if they were.** The
list counts inside a class-0 slot (`0x00524FC6`, `0x0052503B`, `0x0044BD07`) are `test / je`
followed by a decrement — a `do { } while (--n)` loop with no signed test, so a negative value
does not skip, it runs away. There is no correct behaviour to mirror. This parser reads them
unsigned and refuses, **a refusal where the engine would misbehave**; that is the right direction
to differ in, but it is a difference, and `spr_unguarded_count` exists so that it is visible at
each of the three call sites rather than hidden behind a cast that looks like the guarded case.

**And two more are lengths, not counts.** Class 2's blob length reaches the `fread` at
`0x0043D0E3` and class 3's the one at `0x004516F7`, in both cases as the `size` argument with no
compare, branch or clamp anywhere in between. They are a third shape, they go through
`spr_unguarded_length`, and the only reason that marker exists is that they sat silently outside
the classification until a review found them — behaviour right, marker missing, which is how a
convention rots. Every file-declared number in the section now goes through exactly one of the
three markers, or is on a short, exactly matched list of reads that are not quantities at all
(the nested factory's type id is the only member); a test enforces it.

#### Refuted: the counted byte array in class 0 is not a string

The previous pass called `0x00427A40` a length-prefixed **string** reader, and
`docs/save-format.md` cited "length-prefixed strings at irregular offsets" as file-side evidence
for variable-length records. The variable length is real; the strings are not. **Observed in the
corpus:** decoded at its true offset, the payload in every class-0 record is a run of small
integers in `0..=7` closed by a larger byte — 66 of them in one file, of which 2 are non-empty;
217 in another, of which 43 are. Nothing in it is text. The engine parks the bytes in a pool and
never treats them as characters. A run of 0..7 terminated by a sentinel **looks** like a queued
movement path over an eight-direction grid, and that reading is **Inferred** only; the bytes are
carried verbatim and no field is minted from it.

This one nearly became a wrong field. The previous pass's `LS_SPR_` entry already carries a
`Refuted` note about a first-hero name at a fixed offset; this is the same mistake reached from
the other direction, and only decoding the record stopped it.

#### How the model was checked

Three instruments, and they are not equally strong. Stating which is which is the point of this
section — an earlier draft of this page put the weakest one first.

**1. The disassembly.** Every length in this section is either a constant in the instruction
stream or a count the file itself stores. That is where the layouts come from, and it is what any
correction has to argue with.

**2. The version sweep against an independently written fixture.** The test fixture emitters are a
**second transcription** of the same readers, written from the disassembly with **literal** gate
values rather than the parser's constants. Driving the whole record set at every version from
`0x30` to `0x80`, plus 0, 1, 50, 108, 111, 200, 9999 and `u32::MAX`, makes parser and fixture
genuinely able to disagree: if any gate differs between them the record lengths differ and the
section either overruns or leaves bytes over. `tools/mutate_save_constants.py` turns that into a
number — **73 of 76 single-step mutations of this section's constants are killed, in both
directions**. The harness is in the repository rather than in a transcript, because a sweep
nobody can re-run is a number nobody can check.

**A mutation harness fails open, and this one was found failing open three ways.** The number it
prints is worth only the harness's own honesty, so:

- **It verifies its baseline before it mutates anything** and aborts nonzero if the suite is not
  green, printing what it measured against. The first version inferred "caught" from the *absence*
  of `test result: ok` in stdout, so a suite that could not build, could not take the target lock,
  or failed for an unrelated reason would mark **every** mutant caught and exit 0 — total failure
  and total success producing the same output. Success is now `returncode == 0`, never a
  substring. A sister branch lost three commits to a red baseline making one survivor look killed;
  this polarity is worse. Both red-baseline modes were checked by hand: a failing test and a
  crate that will not compile each abort with exit 2.
- **Its expected-survivor list is matched by equality, in both directions.** Substring matching
  would excuse a future gate named `SPR_NESTED_LAST_WORD_MIN_EXTRA` in silence, and an exemption
  that has stopped excusing anything must be deleted rather than left standing.
- **A mutation whose pattern stops matching is `SKIPPED`, not a survivor,** and always fails the
  run. Folding skips into the survivor list let an exempt mutation quietly stop being run at all.
- **A mutant that does not compile is counted separately from one the tests killed.** Both are
  "not survived"; only the second is evidence that a test can see the constant.

`tests/test_mutate_save_constants.py` pins all of that, plus the exhaustiveness of the count
taxonomy, so coverage cannot shrink while the headline number stays put. The three survivors are all expected and all named by the harness:
`SPR_NESTED_LAST_WORD_MIN` moved either way, which guards a branch
[this build cannot reach](#4-the-pre-version-99-ls_mult-layout-is-implemented-but-unexercised),
and one deliberate compensating pair described below.

**3. The byte account over the corpus.** Parse the records and require the cursor to land
**exactly** on the section end. **Observed in the corpus, 2026-09-18:** zero slack in **31 of 31
files** across both format versions present. This corroborates the *aggregate* extents of the
paths those files exercise. It cannot see inside a record.

#### What the round trip proves, and what it cannot

**Corrected, 2026-09-18.** `LS_SPR_` re-emits byte-identically from its decoded records in 31 of
31 files, and the whole file reassembles byte-identically with `LS_SPR_` regenerated in 31 of 31.
An earlier draft of this page presented that as the falsifier — "get any record's extent wrong and
the re-emitted section is a different length". **That is wrong, and it is wrong in a way this
repository has been caught by before.**

`class_id` and the six base dwords are read little-endian and written back little-endian, and each
record's body is an exact contiguous slice. So once `parse` has succeeded, re-encoding is an
**identity**: it *necessarily* equals the payload. The whole-file version then copies the other
eight payloads through, so it is the same identity in a larger wrapper.

The worked counter-example, which the mutation harness **runs** rather than merely describing:
change class 0's two consecutive fixed reads from `12 + 88` to the compensating `16 + 84`. Every
field boundary in that record is then wrong. The aggregate size is unchanged, so `parse` still
lands exactly on the section end, the body captures the same bytes, and **both round trips still
match byte for byte**. `mutate_save_constants.py` lists it as a survivor by design, so the
limitation is demonstrated in the test suite instead of being a caveat in prose.

So, precisely:

| claim | what supports it |
| --- | --- |
| the records tile the payload contiguously, in order, with no gap or overlap | the round trip, and equivalently the zero-slack check |
| every byte is preserved losslessly, and the container splices back together | the round trip |
| the aggregate extent of each record on an exercised path | the byte account over 31 files |
| **each individual field boundary** | **the disassembly, corroborated by the version sweep** |
| that no compensating pair of errors exists | **nothing here.** Only re-reading the instruction stream |

#### The class census

Per distinct game state rather than per file, and with the five genuine-play states shown, because
the six shipped demo scenarios [may share a generator](#corpus-limitations) and a table built only
from them is weaker than its row count suggests.

| state | records | class 0 | 1 | 2 | 3 | 7 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `combat.sav` (demo) | 829 | 67 | 673 | 46 | 8 | 35 |
| `experience.sav` (demo) | 851 | 68 | 673 | 46 | 8 | 56 |
| `magic.sav` (demo) | 491 | 27 | 377 | 48 | 8 | 31 |
| `merc.sav` (demo) | 488 | 25 | 377 | 48 | 8 | 30 |
| `temple.sav` (demo) | 496 | 33 | 377 | 48 | 8 | 30 |
| `quickstart` (demo, v108) | 877 | 30 | 703 | 46 | 8 | 90 |
| 3.02 `Merlin I` / `lastsave.lom` — **turn 315, real play** | 836 | 15 | 702 | 73 | 8 | 38 |
| GS5R3 `Merlin I` / `lastsave.lom` — **real play** | 833 | 35 | 702 | 51 | 8 | 37 |
| GS5R3 `combat.lom` — **real play** | 838 | 36 | 702 | 51 | 8 | 41 |
| GS5R3 `endturn.lom` — **real play** | 848 | 39 | 702 | 51 | 8 | 48 |
| GS5R3 `temple.lom` — **real play** | 838 | 36 | 702 | 51 | 8 | 41 |

**Class 3 is exactly 8 records in every state, demo and real play alike**, which is a corpus
regularity and not a structural requirement — nothing in the reader fixes it. **Class-id echo
disagreements: 0 in every record of every file**, which is the integrity reading
[above](#every-record-opens-with-its-class-id-twice) and not a layout check.

The no-fixed-stride argument the previous pass made from the files is now **explained** rather than
merely corroborated: the survey still computes it, and the intersection is still empty, because
three of the five classes present are genuinely variable-length.

#### What the corpus does and does not exercise

**Observed in the corpus, 2026-09-18**, measured branch by branch rather than assumed:

| branch | exercised |
| --- | --- |
| class ids 0, 1, 2, 3, 7 | yes, in every file |
| **class ids 4, 8 and 9** | **never, in any file** |
| nested factory types 0, 1, 2, 3 | all four, 393 / 399 / 356 / 3,467 times |
| class 1 / 2 / 3 embedded class-0, present and absent | both, for all three |
| slot `item_a` and `item_b` lists, empty and non-empty | both |

So **classes 4, 8 and 9 are read from the instruction stream and have never met a real record.**
Each is short, fixed-length and gate-free, which is the best case for a transcription — but a
wrong reading of any of them would parse every save on this machine perfectly, exactly like the
two empty alarm queues.

**Partly narrowed by a second reader, 2026-09-18.** An independent review disassembled `lomse.exe`
itself and re-derived classes **4 and 9** from their instruction streams, agreeing with the
transcriptions here — class 9's reader at `0x004ADA10` is `fread(this+0x44, 1, 0x5C)` followed by
ten dword reads into `+0xA0`..`+0xC4`, which is the `92 + 40 + 24 = 156` stated above. That is a
second transcription, not corpus corroboration: both readers read the same bytes of the same
binary, so a shared misreading would survive it. **Class 8 has neither.** It is the base reader
verbatim, which is the least likely of the three to be wrong and still the one with no second
opinion.

The same review reproduced the corpus numbers on its own machine — 31 files, 11 states, zero
slack, both round trips — which is the first second-party check of any corpus figure on this
page. `save_survey` takes a directory at runtime precisely so that this is possible.

### `LS_USER` — eight per-player records

**Observed in a local binary, 2026-09-18.** 6,272 bytes in all 31 files, which is `8 × 784`
with zero remainder. Block *i* begins at `payload + 784*i` and its first `u32` is exactly *i*. The
writer does `push 0x310` (784) and `fwrite` eight times; note its **in-memory** stride is `0x400`,
so the on-disk record is the struct's first 784 bytes and not the whole struct.

Known fields, all with **Unknown** meaning apart from the index:

| offset | value in every inspected record |
| ---: | --- |
| `+0` | the record's own index |
| `+4` | `0xFFFFFFFF` |
| `+8` | `0` |
| `+12` | `0x3F800000` — the bits of `1.0f` |
| `+16` | `0` |
| `+20` | an id: a small value in record 0, `0xFFFFFFFF` in records 1..8 |

Most of the remainder is a repeating `(-1, -1, 0)` 12-byte pattern.

---

### `LS_GAME` — the turn counter and a record table

**Observed in a local binary, 2026-09-18.** Holds in all 31 files with zero exceptions:

```text
  u32 turn
  u32 unknown_4
  u32 0
  u32 live_count
  u32 12                  <- literally the record size, stored
  N * 12 bytes            where N = (payload_len - 24) / 12
  u32 trailer
```

`(payload_len - 24) % 12 == 0` in all 31, and **`N - live_count == 71` in all 31**:

| file | N | live_count | surplus |
| --- | ---: | ---: | ---: |
| `combat.sav` | 1528 | 1457 | 71 |
| `experience.sav` | 1653 | 1582 | 71 |
| `magic.sav` | 269 | 198 | 71 |
| `merc.sav` | 269 | 198 | 71 |
| `temple.sav` | 424 | 353 | 71 |
| `quickstart` | 191 | 120 | 71 |
| `lastsave.lom` / `Merlin I` (3.02) | 2691 | 2620 | 71 |
| `combat.lom` / `lastsave.lom` / `Merlin I` (GS5R3) | 574 | 503 | 71 |
| `endturn.lom` (GS5R3) | 470 | 399 | 71 |

The constant 71 is Observed — now over **ten** distinct game states, including three the previous
pass did not have — and **unexplained**. The surplus is computed as a signed value and
reported as a number, never as a boolean.

The record is `i32 id; i32 a; i32 b`. Ids descend by one in long runs and then jump, which is what a
free list looks like — that reading is **Inferred**.

The trailer varies (1, 319, 801 observed) and its meaning is **Unknown**.

---

### `LS_PLR_` — per-player state; **decoded**

**Observed in a local binary, 2026-09-18.** The section is `{ u32 slot_index; record }*` terminated
by `u32 -1`, then **eight `u32` lord codes** matching `LS_MULT` slots 0..8 in order
(`0x00482E1D` .. `0x00482F33`). The writer emits a record for slot *i* only when
`[player_i + 0x6640]` has bit 0 or bit 7 set; the reader validates `0 <= slot_index < 16`.

The record writer is **`0x004BCE20`** and the record reader is **`0x004BCBD0`**. Walked, the record
is:

```text
  u32   +0x15a8, +0x15ac, +0x15b0
  u32   queue_len                       linked list at +0xd24, next at +0x20
  queue_len x 24 bytes                  0x0050B800: six u32, on-disk order +0,+4,+8,+0x1c,+0xc,+0x10
  16 x army                             0x0050A360, RAM stride 0x88
  u32   +0x15e4                         0x00462FF0
  roster                                0x0051C6C0
  -- version-gated from here on --
  15 x u32                              >= 57;  three 5-word arrays interleaved a[i],b[i],c[i]
  u32   +0x3c                           >= 68
  bitset on +0x34                       always
  31 bytes +0x44                        >= 76;  the lord's name, NUL-terminated inside the field
  u32   +0x15d8                         >= 86
  3200 bytes +0x68                      >= 104
  u32   +0x15b4, +0x15b8                >= 110
  u32   +0x40                           >= 111
```

where

```text
  army   := u32 +0,+4,+8
          ; u32 unit_count               linked list at +0x84, next at +0x14
          ; unit_count x 20 bytes        0x00509FC0: five u32
          ; 100 bytes                    0x0049F2A0: 0x28 from +0, 0x34 from +0x28, +0x5c, +0x60
          ; bitset on +0x7c
          ; u32 +0xc, +0x14

  roster := u32 +0x68, +0x6c
          ; u32 entry_count              0x004BD210 writes NOTHING per entry
          ; u32 slot_count               the writer pushes a literal 22
          ; slot_count x u32 from +0x10
          ; 64 bytes from +0x78

  bitset := u32 bit_count ; ceil(bit_count / 32) u32      0x004BF0A0 / 0x004BF100
```

Every field above is **Unknown in meaning** apart from the slot index and the name. What is
established is the *shape*, and the shape is what the previous pass was missing.

#### Refuted: "the record size is not established for format version 111"

**Refuted, 2026-09-18 — the premise, not just the answer. There is no record size, at any
version.** A record is a tree of counted lists: a unit queue, sixteen armies each with its own unit
list, and three bit-counted sets whose widths are stored. Two players *in the same file* differ in
length. Measured across the eleven states a record runs **6,303 to 7,983 bytes**, and `combat.sav`
alone holds eight records of eight different lengths.

So two things were being sought that do not exist, which is exactly why "generalising it to version
111 failed on four of six files". The version-108 file's clean `9 × 6223` is **not a stride**: it is
a turn-1 scenario in which every player happens to have an empty queue, sixteen empty armies and
equal-width bitsets. A fixture that agrees with a wrong reading is the failure mode this document
already names twice; this is a third instance of it, in a save file rather than in a test.

`PlayerSection` therefore offers `record_lengths()` — a list — and no `record_size`.

#### Corrected: version and game-age were **not** confounded here

The previous pass recorded that the only version-108 file is also the only turn-1 file, so a
108-versus-111 difference "might equally be a difference between a freshly-started game and a
played one", and that resolving it needed a new sample.

**It did not.** The reader's own gates settle it, and they are eight `cmp dword [0x5AA12C], n / jl`
instructions:

| gate VA | minimum version | field |
| --- | ---: | --- |
| `0x004BCCDE` | 57 | the fifteen interleaved words |
| `0x004BCD28` | 68 | `+0x3c` |
| — | always | the `+0x34` bitset |
| `0x004BCD4B` | 76 | the 31-byte name |
| `0x004BCD65` | 86 | `+0x15d8` |
| `0x004BCD82` | 104 | the 3,200-byte block |
| `0x004BCD9F` | 110 | `+0x15b4` and `+0x15b8` |
| `0x004BCDD0` | 111 | `+0x40` |

Version 108 clears the 104 gate and not the 110 or 111 gates, so it stores **12 bytes less per
record** than 111 does — exactly the difference the corpus shows. **Observed in a local binary** and
**Observed in the corpus**, independently.

**The writer has no gates at all** (`0x004BCE20` writes every field unconditionally). A save is
therefore readable by the build that wrote it and by every later build, and the ladder exists only
to read older files. That asymmetry is also why a record must be parsed against `LS_VER_` rather
than against its own length.

#### A second semantic ordering dependency

`LS_VER_` → `LS_MULT` was the one known dependency between handlers. There is now a second:
**`LS_PLR_`'s record reader consults the same version singleton**, at the eight addresses above.
`SaveFile::parse` threads `LS_VER_` into `PlayerSection::parse` for this reason. "Full semantic
order-independence" remains **not established**, and is now known to be false for two sections
rather than one.

#### Observed in the corpus

Parsing with the model above lands exactly on the `-1` sentinel in **all 31 files / 11 distinct
states**, with zero slack, and the version-gated name field yields the lords' names — `Merlin`,
`Balkoth`, `Amazon Princess`, `Witchqueen`, `Lylendnar`. Slot 15 is present in every file and is
the neutral/unowned pseudo-player; its name field is empty.

---

### `LS_REGN` — region grid and region table; **decoded**

**Observed in a local binary, 2026-09-18.** Writer `0x004C7390`, reader `0x004C7450`.

```text
  u32 width
  u32 height
  width*height*6 bytes         the region grid; the six-byte cell's layout is still Unknown
  u32 array_count              [this+0x1b0]
  (array_count + 1) records    0x004C5840 each
```

The trailing `+1` is not an off-by-one. After the counted array at `[this+0x1ac]` (RAM stride
`0x19c`) the writer calls the **same** record writer once more, at `0x004C742E`, on the object
embedded at `[this+0x10]`.

A region record is:

```text
  u8  +0x08
  u8  +0x09
  u8  name_len                 strlen(name) + 1, or 0 when the engine's pointer is null
  name_len bytes               the name INCLUDING its NUL
  u32 +0x10
  6 x 64 bytes                 +0x18, +0x58, +0x98, +0xd8, +0x118, +0x158
```

so **391 bytes plus its name**. The length byte is a `u8`, so a region name longer than 254
characters cannot be written at all.

#### The three tail lengths were one record's width, seen three times

**Observed in the corpus.** The previous pass could only list 8,998 / 9,389 / 9,780 and say "no
known structure". The differences between them are **391 and 391** — one nameless region record,
twice. With the record size in hand:

| tail | `4 + n × 391 + name bytes` | regions |
| ---: | --- | ---: |
| 8,998 | `4 + 23×391 + 1` | 23 |
| 9,389 | `4 + 24×391 + 1` | 24 |
| 9,780 | `4 + 25×391 + 1` | 25 |

and the single name byte is the final embedded region carrying an **empty** name — a lone NUL,
which is what a non-null pointer to an empty string produces. Every other region in every file has
a null name pointer. The table accounts to the byte in **all 31 files**.

`+0x08` and `+0x09` always hold the same value as each other, and across the corpus those values are
the eight powers of two 1..128. That is **Observed in the corpus** and unexplained; "a bitmask
index" is **Inferred** and nothing here asserts it.

#### Corrected: `[0x0054D0D8]` / `[0x0054D0DC]` is a lock, not a transform

**Corrected, 2026-09-18.** The previous pass left an asterisk on the no-encryption finding for this
section because two unresolved imports bracket its I/O. They are a lock pair, and the asterisk comes
off:

- **Shape.** Both take a pointer to `[this+0x1bc]`, an object field, as their **only** argument
  (`0x004C739D`, `0x004C7438`), and neither return value is used. A codec would need the buffer and
  a length, and would have to be applied to the bytes rather than to a field of the object.
- **A cross-check that does not share the mechanism.** The bytes between them decode with **no
  transform applied at all**: 391-byte records, plaintext structure, and an exact byte account
  across 31 files. A cipher that leaves all of that in place is not a cipher.

The six-byte grid cell's field layout remains **Unknown**. That is a real gap and none of the above
touches it.

---

### `LS_ALRM` — pending GameScript callbacks; **decoded**

Runs to end of file.

**Observed in a local binary, 2026-09-18.** `0x00482F77` .. `0x00482FBA` writes **six independent
linked lists**, one after another, each as `u32 count` followed by that many records:

| queue | list writer | element writer | fields before the arguments |
| ---: | --- | --- | --- |
| 0 | `0x0040B7D0` | `0x0040B180` | `u32 × 4`, name |
| 1 | `0x0040C230` | `0x0040BBE0` | `u32 × 5`, name |
| 2 | `0x0040CCF0` | `0x0040C650` | name |
| 3 | `0x0040D9F0` | `0x0040D3B0` | name, `u32`, name |
| 4 | `0x0040E4E0` | `0x0040DDA0` | `u32 × 3`, name |
| 5 | `0x0040F600` | `0x0040EC50` | `u32 × 4`, name |

Every record then ends the same way:

```text
  u32 argument_count
  argument_count x u32
  u32 trailer
```

and a `name` is `u32 len` + `len` bytes with **no terminator**, written by the shared string writer
at `0x004D5F20`, which `strlen`s the callback name out of a table reached through `[0x005A7B78]`.

**The six queues are unnamed.** Nothing in the binary names them, and this document will not invent
names for six things it can only tell apart by an address and a field schedule. What each queue is
*for* is **Unknown**; what goes in it is decoded.

#### Refuted: the eight-word header

**Refuted, 2026-09-18.** There are no header words. What two earlier passes read as a header is:

| payload word | what it actually is |
| ---: | --- |
| 0 | `count` of queue 0 — **`0` in every corpus file** |
| 1 | `count` of queue 1 |
| 2 | queue 1's first record, word 0 — the turn |
| 3 | word 1 — `15` |
| 4 | word 2 — `1` |
| 5 | word 3 — `1000001 - turn` |
| 6 | word 4 — `0` |
| 7 | the **string length** of `monstergenerator`, which is 16 |

The turn really does sit at payload word 2 in every file inspected — **because queue 0 is empty in
every file inspected**. A save with a single queue-0 alarm moves it. The constant `16` was never a
field at all.

This is the same failure the previous `Corrected` note diagnosed, one level up. That note moved the
turn from index 1 to index 2 and kept the frame that produced the error: *indexing into a payload is
not a structure*. The turn-agreement check passed either way, because a check that asks "does some
word equal the turn" cannot tell a field from a coincidence. The parser now reads the turn out of a
**record**, and the test that guards it uses a fixture whose queue 0 is **occupied**, so a reader
that goes back to indexing the payload fails whichever index it picks.

#### Refuted: records end with their callback name

**Refuted.** The name is followed by the argument vector and a trailer. The previous pass's
observation that "the number of fixed `u32` fields before the trailing length word varies between
records — 2 in one case, 10 in another" was the **argument vector of the preceding record** being
counted as the fixed fields of the next.

#### Observed in the corpus

The six queues account for the payload to the byte in all 31 files. Occupancy, and the callback
names recovered:

| file | queue counts 0..5 | callbacks |
| --- | --- | --- |
| `combat.sav` | `0, 1, 212, 0, 0, 181` | `experience_attack_callback` ×208, `village_security_brain` ×53, `brain` ×32, `security_brain` ×28, `explore_brain` ×18, `spy_brain` ×15, `primary_brain` ×10, `research_brain` ×8, `antispy_brain` ×5, `dpw_brain` ×5, `great_temple_security_brain` ×4, `thief_steal_from_enemy_event` ×4, `train_brain` ×2, `monstergenerator` ×1, `revenge_brain` ×1 |
| `temple.sav` | `0, 7, 1, 0, 0, 127` | adds `unmodify_champion` |
| `lastsave.lom` (turn 315) | `0, 1, 19, 0, 0, 171` | adds `engage_special_building_brain`, `cr5_brain` |
| `magic` / `merc` / `quickstart` | `0, 1, 0, 0, 0, 95..119` | brains only |

**Queues 3 and 4 are empty in all 31 files**, so their schedules are **Observed in a local binary
only** — the corpus cannot corroborate them, and a wrong reading of either would be invisible here.
The argument for them is the instruction stream alone; a queue-3 or queue-4 record has never been
seen on disk.

#### The turn reading, restated

The turn still appears three times per file — `LS_GAME`'s first word, queue 1's turn record word 0,
and `1000001 -` that record's word 3 — agreeing in all 31 files. What changed is that two of the
three are now read from a located field instead of from a payload index, and the accessor returns
`Option<u32>`: an empty turn queue is a well-formed save, and the reading must go absent rather than
invent a turn.

---

## What is not determined

Four gaps, stated as gaps. "Not determined" is the honest answer for each; a confident wrong answer
would cost far more. **The previous pass's first gap — `LS_SPR_`'s record layouts — is closed**,
and what replaces it is a narrower gap of a different kind: the layouts are decoded and the field
meanings are not.

### 1. What every `LS_SPR_` field *means*

**The section's structure is decoded; its semantics are not.** The class readers name offsets into
an object, not meanings, and the binary carries no RTTI, so not one of the eight classes can even
be named — they are "class 0" and "class 7" because that is what the jump table calls them.
Everything past each record's class id is carried verbatim in `SpriteRecord::body` rather than
parsed into invented field names.

Three specific holes inside that:

- **Classes 4, 8 and 9 have never been seen in a save.** Their layouts are short, fixed and
  gate-free, which is the easiest case to transcribe correctly, but no file on this machine
  exercises any of them. This is the same shape of gap as alarm queues 3 and 4 below. Classes 4
  and 9 have since been re-derived from the instruction stream by an independent reader and
  agree; **class 8 has no second opinion and no corpus coverage.** A second transcription is not
  corpus corroboration — both readers read the same binary.
- **The counted byte array in class 0** is a run of small integers with a larger closing byte. The
  movement-path reading is **Inferred** and nothing rests on it.
- **The version gates below 108 are transcribed, never exercised.** The corpus holds only 108 and
  111. The gate sweep drives every rung of the ladder against an independently written fixture and
  `tools/mutate_save_constants.py` reports **73 of 76** single-step mutations killed in both
  directions — but a fixture transcribed from the same reading of the same binary cannot confirm
  that the engine's own pre-108 writer produced it. Two of the three survivors are a single gate,
  `0x0044B6E0`, which is **unreachable in this build** because a second test against the build
  constant at `0x0055B1B0` decides it at compile time; the third is the deliberate compensating
  pair that demonstrates the round trip's blind spot.
- **Nothing here excludes a compensating pair of errors** inside a record — two boundaries wrong
  in opposite directions by the same number of bytes. Neither the byte account nor the round trip
  can see one, and the version sweep only sees one that a *gate* would move. Only re-reading the
  instruction stream can, which is why classes 4 and 9 having a second reader matters and class 8
  not having one is listed above.

### 2. The six-byte `LS_REGN` grid cell

The region **table** is decoded; the **grid** is not. Cells are carried as opaque 6-byte arrays.
The writer emits the whole grid with one `fwrite` of `count * 6` bytes (`0x004C73E1`), so the writer
says nothing about the fields inside a cell — the next instrument is whatever reads a cell at
runtime, not the save path.

### 3. Alarm queues 3 and 4 have never been seen occupied

Their schedules are **Observed in a local binary** and have **no corpus corroboration at all**: both
are empty in all 31 files. A wrong reading of either would parse every available save perfectly.
What would settle it is one save with a queue-3 or queue-4 alarm, or the code that *pushes* onto
those two lists, which would name them as well as confirm their shape.

### 4. The pre-version-99 `LS_MULT` layout is implemented but unexercised

Below version 99 the reader synthesizes the 576-byte slot block from memory instead of reading it,
so a pre-99 `LS_MULT` payload is `4 + declared_setup_len` and stops. The parser implements this and
threads `LS_VER_` into `LS_MULT` to decide, and there are tests for both branches.

**No sample exercises it.** The corpus contains exactly two format versions, 108 and 111, and both
store the block. This path is written **from the disassembly alone** and has never met a real pre-99
file. It is also the one place where a wrong reading would be invisible: a pre-99 save would parse
"successfully" with a plausible-looking setup block and no slots, and nothing here could tell that
from correct behaviour.

**The same caution now applies to `LS_PLR_`'s lower version gates.** The ladder runs from 57, but the
corpus contains only 108 and 111, so gates 57, 68, 76, 86 and 104 are implemented from the
instruction stream and **never exercised by a real file**. The unit tests sweep every gate and the
version immediately below it, which is what a mutation sweep demanded (see below), but a synthetic
fixture cannot confirm that the engine's own pre-104 writer produced what this reader expects.

---

## Corpus limitations

Every claim above is bounded by what these files can show. The limits are sharp and some of them
have already produced wrong readings.

**31 files, but only 11 distinct game states.** The six shipped demo saves are byte-identical
across all four installs, so the Steam, Development, 3.02 and GS5R3 copies are the same six states
counted four times. The survey reports this by grouping on **normalized decoded bytes** — every
decoded field, with the leaked name padding excluded — compared directly rather than hashed, since
the group count is a headline number.

**Corrected, 2026-09-18: the previous "20 files / 7 states" undercounted, because an install was
missed.** There are four installs on this machine, not three, and `Lords of Magic GS5R3.app` carries
**five files no other install has**: `combat.lom`, `endturn.lom`, `temple.lom`, and its own
`lastsave.lom` and `Merlin I`, which do **not** match the identically-named files under
`Lords of Magic 3.02.app`. That is four further distinct states, all of them genuine play. The
correction is worth stating because it cuts the other way from the usual one: the pass before it
was rightly suspicious of an inflated file count and, in guarding against that, stopped looking
for more states. **All five of those files were last written at 20:39--20:47 on 2026-09-18**, so
part of this growth is the user playing the game rather than anyone searching harder --- see
[the corpus section](#the-corpus-is-eleven-game-states-across-31-files-not-ten-across-24).

**Within one install, `lastsave.lom` and `Merlin I` are ONE state, not two.** They have different md5s, which is real,
and eight of their nine sections are byte-identical, which is also real. Their entire difference is
leaked name padding. Counting them as two independent samples would inflate every invariant by one,
and an earlier pass did exactly that — the retraction is recorded here rather than quietly dropped,
because the md5 difference is a genuinely convincing-looking piece of evidence for a false
conclusion.

**Five of the eleven states are genuine mid-game player saves** — the turn-315 state under 3.02
and the four under GS5R3. The turn-315 one remains the single most informative sample: it is the only
one that exercises partial map visibility, a long game record table, and a `LS_REGN` tail length the
demo saves never produce. Where an invariant holds, it holding there matters more than the six demo
states combined. Where the six agree with each other and it does not, suspect a shared generator.

**Six of the eleven may share a generator.** They are authored demo scenarios shipped together. Their
agreement on any structural property is weaker evidence than it looks.

**Every save is 128×128.** Both `LS_MAP_` and `LS_REGN`. So the `y*width + x` cell packing is
**inherited from `docs/map-format.md` and is NOT confirmed here** — a square corpus cannot separate
it from `x*height + y`, which is precisely how the X-major reading survived for months in the map
work. The parser's tests use a deliberately non-square **96×64** fixture for both grids, and a
mutation that computes the cell count as `width*width` is caught by them.

**Only one game state is version 108, and it is also the only turn-1 state.** Version and game-age remain
perfectly confounded *in the corpus*. The `LS_PLR_` reading that depended on separating them no
longer does: the reader's version gates settle it from the instruction stream, so the confound is
now a limitation of the files rather than of the conclusion. Any *other* 108-versus-111 difference
found by comparing files still carries it.

**No multiplayer save exists anywhere on this machine.** All four installs' `Multisav/` directories
are empty. `LS_MULT` is decoded from single-player saves only, and the multiplayer save path — the
one place its 164-byte setup block and its 16 slots would actually be exercised — is **entirely
unexercised by this corpus.** Slots 8..15 are never populated in anything available here.

**The version ladder is barely sampled.** 40 version constants appear in the binary spanning 50..111,
and the corpus contains exactly two of them. `LS_PLR_`'s reader alone gates on five versions the
corpus never shows (57, 68, 76, 86, 104).

**Two of the six alarm queues are empty in every file.** Queues 3 and 4 have no corpus support at
all. See [gap 3](#3-alarm-queues-3-and-4-have-never-been-seen-occupied).

---

## Testing notes

All fixtures are synthetic and built in code; **no save data is committed**.

Fixtures are deliberately **unlike the corpus** in every way the corpus is uniform: a non-square
96×64 map and region grid, a permuted section order, an empty sprite table, versions far outside the
observed range, **an occupied alarm queue 0**, **regions that carry names where the corpus's do
not**, and **three `LS_PLR_` records of three different lengths**. Parametrised tests cover a
missing section, a tag duplicated inside a payload, truncated payloads, five region-table shapes,
a player section with no records at all, and thirteen format versions.

The default fixture **deliberately breaks three corpus regularities** — the `LS_REGN` tail length,
the "exactly one named region" pattern, and the empty alarm queue 0 — and a test asserts exactly
which three break. That is the point of separating the two check classes: those three are things
the shipped scenarios happen to do, and a fixture that copied them could not fail on a reader that
depended on them. A separate test builds the corpus's own shape and shows that an empty queue 0 is
the *only* reason the turn lands at payload word 2.

**Assertions are structural, not literals lifted from the corpus.** `N - live_count` is computed and
compared; the stride search is asserted to be the arithmetic it claims rather than a table of past
results; the plane-coverage check is independent of the byte total.

**Every test was mutation-checked, and the first numbers reported were wrong.**

The 2026-09-18 sweep over the three newly decoded sections is **53 mutations, 53 caught, 0
survivors, 0 unapplied**, every constant tried in **both** directions. It is recorded here with its
survivors, because the first run of it had three:

| survivor | direction | why it was invisible |
| --- | --- | --- |
| `NAME` gate 76 | **down** to 75 | no fixture version stood between 75 and 76 |
| `BLOCK_68` gate 104 | **down** to 103 | no fixture version stood between 103 and 104 |
| `UNKNOWN_15B4_15B8` gate 110 | **down** to 109 | no fixture version stood between 109 and 110 |

All three were caught in the *up* direction and all three survived the *down* one, which is the
asymmetry this document already recorded for the visibility level `63` and then walked into again.
**A version sweep that samples only the gates cannot fail on a gate moved down.** The fix is to
sweep every gate *and the version immediately below it* — 56, 57, 67, 68, 75, 76, 85, 86, 103, 104,
109, 110, 111 — and to assert the per-step size deltas including the **zeros**, since a zero is what
a gate moving down destroys.

Mutations tried in both directions and caught: the region record's block count and block width, its
three-byte prefix, the region table's `count + 1`, the raw-bytes region walker's `count + 1`, the
army count, the name width, the 3,200-byte block, the army stats block, every version gate, the
bitset's `div_ceil(32)`, the slot-index bound, every alarm queue's field schedule, the turn and
countdown word positions, the turn queue's identity, `AlarmQueue::ALL`'s order, `COUNTDOWN_BASE`,
the counted-string length, the per-record word counts, and `expect_exhausted` stubbed to `Ok`.

Two mutations target the **raw-bytes structural walkers** specifically. Those walkers are a second
implementation on purpose — they count bytes and never build a record, and they run on files
`SaveFile::parse` refuses — so a mutation of one that the other does not catch would mean they had
collapsed into one implementation. Neither survived.

The earlier figure for the rest of the module was **43 mutations, 43 caught, 0 survivors, 0
unapplied.** The previously reported
"31 mutations, 0 survivors" was true of the mutations chosen and **not of the behaviour** — an
independent review then mutated `UserSection::RECORD_COUNT` from 8 to 7 and it survived the entire
suite. Two more survived the same way: `PlayerSection::SENTINEL`, and the visibility level `63` in
the `63 -> 62` direction.

All three were the **same defect, and it is the one this repository keeps re-learning**: a fixture
generated from the constant it is testing moves with that constant, so both sides of the assertion
shift together and the mutation is invisible. The fixture built its user records from
`RECORD_COUNT`, its terminator from `SENTINEL`, and its cell visibility by indexing
`OBSERVED_VISIBILITY_LEVELS`. All three now use literals, with a comment at each site saying why.

Note the asymmetry that hid the visibility one: mutating `63 -> 64` **was** caught, because a
separate test plants the value 64 and asserts it is rejected. Only the `63 -> 62` direction
survived. A mutation set that tries one direction per constant will report a clean sweep it has not
earned.

**The real-corpus survey does catch the `RECORD_COUNT` mutation** — `7 * 784 != 6272` — so the
regression was never reachable on real files. That is worth stating rather than hiding behind the
fix: the unit tests were the layer that failed, and the corpus check was the layer that would have
caught it.

Four mutations target the **structural checks specifically**, because those are a deliberate second
implementation and a second implementation that merely restates the first is worthless. One of them
— stubbing the `LS_PLR_` terminator check to `true` — survived until a test was added that runs the
container-level checks on a file `SaveFile::parse` **refuses**, which is the case those checks exist
for.

Two earlier "survivors" were **no-op mutations** that could not change behaviour (`& 0xffff` versus
`& 0xffffff` under an `as u16`; swapping two locals that feed only symmetric arithmetic). They were
replaced with genuine ones rather than counted either way.

### A decode path that killed the process

The review also found that the survey **panicked** on a save with an empty `LS_USER`: zero is a
multiple of 784, so the divisibility check accepted it, `records` came back empty, and printing
`records[0]` aborted. This repository has already shipped one decode panic that killed a process, so
the fix is at the parse rather than at the caller: `LS_USER` must be **exactly** eight records,
which is what the writer emits unconditionally. The survey additionally uses `.first()` rather than
`[0]`. Both branches have tests, built from the proof-of-concept file's shape.
