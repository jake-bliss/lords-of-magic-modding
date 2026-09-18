# Savegame Format (`.sav`, `.lom`, and extensionless)

## Status

**The container is solved. Five of the nine sections decode completely: `LS_VER_`, `LS_MULT`,
`LS_MAP_`, `LS_USER` and `LS_GAME`.** A native Rust parser (`spikes/asset-viewer/src/save.rs`) reads
every save in every install on this machine without modifying it, and `examples/save_survey.rs`
checks nine structural requirements and nine corpus regularities per file.

**Four sections decode only in part, and the undecoded bytes are carried verbatim rather than
guessed at**: `LS_SPR_` yields its record count only, because the records are polymorphic and
variable-length; `LS_PLR_` yields only its tail, because the record size is not established for
format version 111; `LS_REGN` yields its header and grid but not its tail; `LS_ALRM` yields its
header but not its records. See [What is not determined](#what-is-not-determined).

### The corpus is seven game states, not twenty files

Current result: **20 files, 20/20 on all 18 checks — but those 20 files carry only SEVEN distinct
game states.** The six shipped demo saves are byte-identical across all three installs, and the two
turn-315 files are one state written twice. **The file count is duplication, not corroboration**, and
a 20/20 line that reads as twenty independent confirmations is exactly the inflated-denominator
trap.

Worse, **six of the seven states are authored demo scenarios shipped together**, so they may share a
generator; their agreement on any structural property is weaker evidence than it looks. **Exactly one
state — the turn-315 save — is genuine play.** Where an invariant holds, it holding *there* matters
more than the other six combined. Read every "holds in all N files" claim below through this.

**No original save data is stored in Git.** The survey takes a directory at runtime; every test uses
synthetic fixtures built in code.

Evidence classes used throughout, as elsewhere in this repository: **Observed in a local binary**,
**Observed in gameplay**, **Documented**, **Inferred**, **Refuted**, **Corrected**.

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

Measured on the corpus: **9/9 in all 20 files**, so no payload in any of them accidentally contains
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

`this` is the static singleton at `0x005AA12C`. The header-peek reader backs a pre-load "who was in
this game" screen, which is why `LS_MULT` is the one section with a genuine forward-compatible
length.

### No compression and no encryption

**Observed in a local binary, 2026-09-18**, established four independent ways, including raw
`fwrite` of struct memory in the writer. `combat.sav` measures 3.01 bits/byte entropy with 56% zero
bytes and plaintext unit names. One caveat applies to `LS_REGN` alone and is recorded
[below](#ls_regn--region-grid-and-an-undecoded-tail).

---

## Per-section table

| tag | payload | state | leading `u32` means |
| --- | ---: | --- | --- |
| `LS_VER_` | 4 | decoded | the format version |
| `LS_MULT` | 744 | decoded | `sizeof` of the setup block (164) — **the only real length in the format** |
| `LS_MAP_` | 196,628 | decoded | map width |
| `LS_SPR_` | varies | **count only** | record count |
| `LS_USER` | 6,272 | decoded | record 0's own index (`0`) |
| `LS_GAME` | varies | decoded | the turn number |
| `LS_PLR_` | varies | **tail only** | the first record's slot index |
| `LS_REGN` | varies | **header + grid only** | region-grid width |
| `LS_ALRM` | to EOF | **header only** | `0` |

---

### `LS_VER_` — the format version

**Observed in a local binary, 2026-09-18.** The payload is exactly four bytes. `111` in nineteen of the twenty files and `108` in `quickstart`.

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

**Observed in a local binary, 2026-09-18.** 744 bytes in all twenty files, accounting exactly:

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

**Observed in a local binary, 2026-09-18.** 196,628 bytes in all twenty files, accounting exactly
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

**Observed in a local binary, 2026-09-18.** Across all cells of all twenty files, the `+2` field
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

### `LS_SPR_` — units, armies and heroes; **count only**

**Observed in a local binary, 2026-09-18.** `u32 count`, then `count` records. **The records are
polymorphic and variable-length.** Each begins with a `u32 class_id`, and the reader makes a virtual
call `call dword [eax+0x20]`, dispatched through a 10-entry jump table at `0x004F73B8` bounded by
`cmp eax,9 / ja`. Class ids 5 and 6 abort as invalid.

In-memory object sizes per class — **sizes in RAM, not on disk**:

| class | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- | --- | ---: | ---: |
| bytes | 1500 | 96 | 148 | 844 | 120 | invalid | invalid | factory `0x0047CBD0` | 88 | 328 |

**The primary evidence for variable-length records is the disassembly**: virtual dispatch through
the 10-entry table above, with each class reader consuming what its own structure needs. The section
also contains **length-prefixed strings** (`u32 len` then `len` raw bytes, **no NUL**) at irregular
offsets, so variable length is directly visible in the bytes without any arithmetic at all.

A file-side search corroborates this, and **its bound is part of the result.** For every candidate
header size `0..=1024`, the set of sizes for which `(payload_len - header) % count == 0` has an
**empty intersection** across the corpus. Stated exactly: *no common fixed stride exists with a
per-section header of at most 1024 bytes.* A larger header was not tried, so this is a bounded
search and not a proof that no stride exists. The survey prints the bound alongside the result
rather than reporting "no stride exists".

Two further qualifications, because the arithmetic is weaker than it first looks:

- **The no-stride property belongs to the corpus, not to any one file.** Individual files do admit a
  dividing header size — `combat.sav` exactly one (717), `quickstart` one, `magic` two, `merc`
  three. Only the intersection is empty, so the survey computes it once at the end instead of
  reporting a per-file pass.
- **Seven states is a small intersection to rest on.** With more files the intersection can only
  shrink, but with seven it could in principle have been empty by chance. That is why the
  disassembly leads and the arithmetic corroborates, rather than the other way round.

**So: parse the count and stop.** `SpriteSection.raw` carries the rest verbatim.

**Refuted, 2026-09-18: there is no first-hero name at a fixed offset.** An earlier reading placed
`u32 = 8` at payload `+0x4c` and a 32-byte name at `+0x50`. Measured across the corpus, the word at
`+0x4c` is `0` in `combat` and `lastsave`, `47793108` in `magic`, `46767388` in `merc`; a name does
appear near `+0x58` in `combat`, `magic`, `merc`, `temple` and `quickstart` — but in
`experience.sav` and `lastsave.lom` those bytes are **all zero and there is no name there at all.**
Variable-length records have no fixed offsets; the apparent ones are a coincidence of the files that
happened to be checked.

---

### `LS_USER` — eight per-player records

**Observed in a local binary, 2026-09-18.** 6,272 bytes in all twenty files, which is `8 × 784`
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

**Observed in a local binary, 2026-09-18.** Holds in all twenty files with zero exceptions:

```text
  u32 turn
  u32 unknown_4
  u32 0
  u32 live_count
  u32 12                  <- literally the record size, stored
  N * 12 bytes            where N = (payload_len - 24) / 12
  u32 trailer
```

`(payload_len - 24) % 12 == 0` in all twenty, and **`N - live_count == 71` in all twenty**:

| file | N | live_count | surplus |
| --- | ---: | ---: | ---: |
| `combat.sav` | 1528 | 1457 | 71 |
| `experience.sav` | 1653 | 1582 | 71 |
| `magic.sav` | 269 | 198 | 71 |
| `merc.sav` | 269 | 198 | 71 |
| `temple.sav` | 424 | 353 | 71 |
| `quickstart` | 191 | 120 | 71 |
| `lastsave.lom` / `Merlin I` | 2691 | 2620 | 71 |

The constant 71 is Observed and **unexplained**. The surplus is computed as a signed value and
reported as a number, never as a boolean.

The record is `i32 id; i32 a; i32 b`. Ids descend by one in long runs and then jump, which is what a
free list looks like — that reading is **Inferred**.

The trailer varies (1, 319, 801 observed) and its meaning is **Unknown**.

---

### `LS_PLR_` — per-player state; **tail only**

**Observed in a local binary, 2026-09-18.** The section is `{ u32 slot_index; record }*` terminated
by `u32 -1`, then **eight `u32` lord codes** matching `LS_MULT` slots 0..8 in order. The reader
validates `0 <= slot_index < 16`.

The `-1` sentinel sits at exactly `payload_end - 36` in **all twenty files**, which is what makes
the tail parseable from the end regardless of what the records are. The eight lord codes match
`LS_MULT` in all twenty.

**The record size is not established for version 111.** See
[What is not determined](#what-is-not-determined).

---

### `LS_REGN` — region grid and an undecoded tail

**Observed in a local binary, 2026-09-18.**

```text
  u32 width  = 128
  u32 height = 128
  width*height*6 bytes    = 98,304   a fixed 6-byte-per-cell region grid
  <variable tail>
```

The grid is fixed; **all variability lives in the tail.** Observed tail lengths:

| tail | files |
| ---: | --- |
| 8,998 | `combat.sav`, `experience.sav`, `quickstart` |
| 9,389 | `lastsave.lom`, `Merlin I` |
| 9,780 | `magic.sav`, `merc.sav`, `temple.sav` |

The 9,389 value is new: an earlier pass over the six shipped saves alone saw only two values and
recorded "only two values across the corpus". The one genuine mid-game state has a third. The tail's
structure is **Unknown**, and so is the 6-byte cell's field layout — cells are carried as opaque
6-byte arrays.

#### Caveat on the no-encryption claim — this section only

**Observed in a local binary, 2026-09-18, meaning not established.** The writer at `0x004C7390`
brackets this section's I/O with two unresolved imports, `[0x0054D0D8]` and `[0x0054D0DC]`. They are
most likely a lock/unlock pair. **If they turn out to be a transform rather than a lock, the "no
compression, no encryption" finding would need retesting for `LS_REGN` specifically** — not
elsewhere, since the other eight sections are plainly readable in the bytes. This is recorded as an
open caveat, not as a suspicion of a problem.

---

### `LS_ALRM` — pending GameScript callbacks; **header only**

Runs to end of file.

#### Corrected: the turn is at header index 2, not index 1

**Corrected, 2026-09-18.** An earlier reading gave the header as
`[0][turn][15][1][1000000][0]` — turn at index 1, with a constant 1,000,000. Both halves are wrong.
Measured across all twenty files the header is eight words:

| file | header |
| --- | --- |
| `combat.sav` | `0, 1, 69, 15, 1, 999932, 0, 16` |
| `experience.sav` | `0, 1, 91, 15, 1, 999910, 0, 16` |
| `magic.sav` | `0, 1, 3, 15, 1, 999998, 0, 16` |
| `merc.sav` | `0, 1, 3, 15, 1, 999998, 0, 16` |
| `temple.sav` | `0, 7, 6, 15, 1, 999995, 0, 16` |
| `quickstart` | `0, 1, 1, 15, 1, 1000000, 0, 16` |
| `lastsave.lom` / `Merlin I` | `0, 1, 315, 15, 1, 999686, 0, 16` |

**The turn is index 2. Word 5 is exactly `1000001 - turn`** in all twenty. Whether the engine
stores a deadline or a remaining budget is **Unknown**; `COUNTDOWN_BASE` records the arithmetic only.
Index 1 is `1` in seven states and `7` in `temple.sav`, so it is left unnamed.

**Why the wrong reading survived, and it is the same failure as the square-map one.** The file that
had been leaned on is `quickstart`, which is at **turn 1** — and the value `1` appears at **three
separate indexes** of its header (1, 2 and 4). That fixture agrees with several readings at once and
cannot single out the turn field; the wrong one was picked and nothing could contradict it. *A
fixture shaped like the corpus cannot fail on what the corpus hides.* Here the corpus's hidden
property was a turn number numerically equal to its neighbouring constants.

There is a test for exactly this: `a_turn_one_fixture_cannot_locate_the_turn_field` builds the
degenerate case and asserts that three header indexes match the turn, so the *discriminating* test
beside it is demonstrably doing work the degenerate one cannot.

#### The turn reading is Observed, and the correction strengthened it

The turn now appears **three times per file** — `LS_GAME[0]`, `LS_ALRM[2]`, and derived from
`LS_ALRM[5]` — agreeing in all twenty. Three independent fields agreeing across seven distinct
game states is why this is **Observed** and not a guess. The survey asserts all three and prints all
three values.

#### Records

Records follow the header and end in `u32 len` + `len` raw bytes of a GameScript callback name.
Verified names: `monstergenerator` (16), `experience_attack_callback` (26),
`village_security_brain` (22), `engage_special_building_brain` (29), plus
`thief_steal_from_enemy_event`, `dpw_brain`, `explore_brain`, `antispy_brain`.

The count tracks activity: 96 at turn 1, 394 at turn 69. **The record layout is not determined.**

---

## What is not determined

Six gaps, stated as gaps. "Not determined" is the honest answer for each; a confident wrong answer
would cost far more.

### 1. `LS_PLR_`'s record size at format version 111

In `quickstart` (version 108) the section is a clean `9 × 6223 + u32(-1) + 32`, with slot indexes
`0,1,2,3,4,5,6,7,15` — 15 being the neutral/unowned pseudo-player — and a name at `+2984` within the
record. **That layout is Observed for version 108.**

Generalising it to version 111 **failed on four of six files**, and the one apparent fit was
spurious. **No `record_size` field is offered in `PlayerSection`, because offering one would be
claiming it.**

**The reading is also confounded, and this is the part that matters for whoever picks it up.**
`quickstart` is simultaneously the **only version-108 file** and the **only turn-1 file**. Version
and game-age cannot be separated on this corpus. A difference attributed to the version ladder might
equally be a difference between a freshly-started game and a played one. Resolving this needs a new
sample that breaks the pairing: a version-108 file at some turn > 1, or a version-111 file at turn 1.

### 2. `LS_SPR_` record layouts

Ten classes, virtual dispatch, variable length, embedded length-prefixed strings, and no fixed
stride. Decoding this needs the ten class readers reversed from
`0x004F73B8`'s targets, not more corpus arithmetic. The count parses; the records do not.

### 3. `LS_REGN`'s tail

Three observed lengths (8,998 / 9,389 / 9,780) and no known structure. Also unknown: the field
layout of the 6-byte region cell.

### 4. `LS_ALRM`'s record layout

The number of fixed `u32` fields before the trailing length word **varies between records** — 2 in
one case, 10 in another — so alarm records carry variable argument lists. The callback name at the
end of each record is readable; the arguments before it are not.

### 5. The pre-version-99 `LS_MULT` layout is implemented but unexercised

Below version 99 the reader synthesizes the 576-byte slot block from memory instead of reading it,
so a pre-99 `LS_MULT` payload is `4 + declared_setup_len` and stops. The parser implements this and
threads `LS_VER_` into `LS_MULT` to decide, and there are tests for both branches.

**No sample exercises it.** The corpus contains exactly two format versions, 108 and 111, and both
store the block. This path is written **from the disassembly alone** and has never met a real pre-99
file. It is also the one place where a wrong reading would be invisible: a pre-99 save would parse
"successfully" with a plausible-looking setup block and no slots, and nothing here could tell that
from correct behaviour.

### 6. The `[0x0054D0D8]` / `[0x0054D0DC]` imports

Unresolved, and they bracket `LS_REGN`'s I/O at `0x004C7390`. Almost certainly a lock/unlock pair.
Until resolved, the no-encryption claim carries an asterisk **for `LS_REGN` only**. See
[the caveat above](#caveat-on-the-no-encryption-claim--this-section-only).

---

## Corpus limitations

Every claim above is bounded by what these files can show. The limits are sharp and some of them
have already produced wrong readings.

**20 files, but only 7 distinct game states.** The six shipped demo saves are byte-identical across
all three installs, so the Steam, 3.02 and GS5R3 copies are the same six states counted three times.
The survey reports this by grouping on **normalized decoded bytes** — every decoded field, with the
leaked name padding excluded — compared directly rather than hashed, since the group count is a
headline number.

**`lastsave.lom` and `Merlin I` are ONE state, not two.** They have different md5s, which is real,
and eight of their nine sections are byte-identical, which is also real. Their entire difference is
leaked name padding. Counting them as two independent samples would inflate every invariant by one,
and an earlier pass did exactly that — the retraction is recorded here rather than quietly dropped,
because the md5 difference is a genuinely convincing-looking piece of evidence for a false
conclusion.

**Only one of the seven states is a genuine mid-game player save.** The turn-315 state is the single
most informative sample: it is the only one that exercises partial map visibility, a long game
record table, and a `LS_REGN` tail length the demo saves never produce. Where an invariant holds, it
holding there matters more than the other six combined. Where the other six agree with each other
and it does not, suspect a shared generator.

**Six of the seven may share a generator.** They are authored demo scenarios shipped together. Their
agreement on any structural property is weaker evidence than it looks.

**Every save is 128×128.** Both `LS_MAP_` and `LS_REGN`. So the `y*width + x` cell packing is
**inherited from `docs/map-format.md` and is NOT confirmed here** — a square corpus cannot separate
it from `x*height + y`, which is precisely how the X-major reading survived for months in the map
work. The parser's tests use a deliberately non-square **96×64** fixture for both grids, and a
mutation that computes the cell count as `width*width` is caught by them.

**Only one file is version 108, and it is also the only turn-1 file.** Version and game-age are
perfectly confounded. See [gap 1](#1-ls_plr_s-record-size-at-format-version-111).

**No multiplayer save exists anywhere on this machine.** All three installs' `Multisav/` directories
are empty. `LS_MULT` is decoded from single-player saves only, and the multiplayer save path — the
one place its 164-byte setup block and its 16 slots would actually be exercised — is **entirely
unexercised by this corpus.** Slots 8..15 are never populated in anything available here.

**The version ladder is barely sampled.** 40 version constants appear in the binary spanning 50..111,
and the corpus contains exactly two of them.

---

## Testing notes

All fixtures are synthetic and built in code; **no save data is committed**.

Fixtures are deliberately **unlike the corpus** in every way the corpus is uniform: a non-square
96×64 map and region grid, a permuted section order, an empty sprite table, versions far outside the
observed range, and header words chosen so no two coincide. Parametrised tests cover a missing
section, a tag duplicated inside a payload, truncated payloads in four sections, region tails of
five different lengths, and player record areas of four different sizes.

**Assertions are structural, not literals lifted from the corpus.** `N - live_count` is computed and
compared; the stride search is asserted to be the arithmetic it claims rather than a table of past
results; the plane-coverage check is independent of the byte total.

**Every test was mutation-checked, and the first numbers reported were wrong.**

The current figure is **43 mutations, 43 caught, 0 survivors, 0 unapplied.** The previously reported
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
