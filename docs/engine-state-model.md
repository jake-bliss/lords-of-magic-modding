# The engine's state model

`docs/native-operator-bodies.md` closed with a list of what it could not establish, and the first
item was the one that matters most for a reimplementation:

> The `0x005ae958` / `0x005aa12c` readings are inferred from operator names. **Nothing here
> identifies a field inside either object.**

This is the pass that identifies fields. It recovers **1,198 distinct field offsets across 100
candidate structures**, with the access width and the direction of every access, and it cross-checks
the result against `docs/save-format.md` — which decodes the *serialized* form of the same state
from a different instrument entirely.

It also fails to recover a unit record, an army record, a city record or a player record, and the
reason is structural rather than incidental. That is [reported below](#the-central-negative) at
least as loudly as the successes, because it is the finding that tells the next person where to dig.

- Analyser: `spikes/asset-viewer/src/engine_state.rs`, on the walk in
  `spikes/asset-viewer/src/operator_bodies.rs`
- Driver: `spikes/asset-viewer/examples/operator_state.rs`
- Artifacts: [`reports/natives/state/`](../reports/natives/state/)
- Regression anchors: `spikes/asset-viewer/tests/engine_state.rs`

```
cargo run --release --example operator_state -- <lomse.exe> --out reports/natives/state
cargo run --release --example operator_state -- <lomse.exe> --base 0x005aa12c   # one structure
cargo test --release                                     # offline anchors
LOM_EXE=<lomse.exe> cargo test --release -- --ignored      # checks against the binary
```

Binary: `lomse.exe`, SHA-256 `a505f399d5be73fe0a2215633f663717f28daeb3075bbcc05b47d40653669052`,
1,535,488 bytes. Addresses, offsets, widths and counts are our own analysis; no bytes and no strings
of the binary are committed.

## How a field is recovered at all

The previous pass stated the obstacle precisely: the engine is C++ with singletons in `.data`, an
operator's body is `mov ecx,<object>` followed by a call, and the store happens one frame down
through `this` where no absolute address appears.

The way through is that the two halves are both visible, just not in the same function:

* **the caller names the object** — `mov ecx,0x005aa12c` or `mov ecx,[0x005ae958]`, recorded at
  every direct call site as `this_call_bases`;
* **the callee names the offsets** — every `[this+n]` in its body, recorded with the decoder's
  displacement and operand width as `field_accesses`.

Joining them is attribution, not inference about behaviour, and its single assumption is stated
here: **that `ecx` at a direct call site is the callee's `this`**. That is the `thiscall` convention
this compiler emits throughout, and the previous pass's recovered vtable slots on `0x005d1e84`
corroborate it from a different direction.

### Three pointer forms, kept apart, because they are not the same claim

| form | instruction | what it means | class |
| --- | --- | --- | --- |
| `static` | `mov ecx,0x005aa12c`, `lea eax,[0x005876d0]` | the object **is** at this address; field `+n` also has the absolute address `base+n` | observed |
| `indirect` | `mov ecx,[0x005d1e84]` | the address **holds a pointer** to the object; field `+n` has no absolute address at all | observed |
| `this` | the caller's `ecx` | means nothing until a caller says which object it passed | — |

Six addresses are reached **both** ways — `0x005869f4`, `0x005876d0`, `0x005aad68`, `0x005aee6c`,
`0x005aef04`, `0x005d3830` — and are carried as two separate candidate structures each. For those
six the analysis cannot say whether the object sits at the address or behind it, and it does not
choose. (Observed in a local binary; `reports/natives/state/structures.tsv`.)

### What `lea` cost, and how the savegame documentation caught it

The first version of this analysis recorded a field only when the instruction *dereferenced* the
pointer. `docs/save-format.md` says the save writer copies a 164-byte block from `[gameobj+0x520]`,
so `+0x520` was a falsifiable prediction about the result — and it was **absent**.

The instruction is `lea eax,[ebp+520h]` at **`0x00482c4e`** (observed in a local binary). `lea`
computes an address without reading memory, so the decoder reports no memory access and the whole
class of fields a serialiser touches wholesale — block copies, embedded arrays, sub-objects —
was invisible. Those fields are now recorded with **width 0**, meaning *the offset is observed and
the width is not*: **189 of 1,198 fields** are in that class.

The same disassembly exposed a second gap. `lea edi,[ecx+50ACh]` at **`0x00482c01`** forms an
**interior pointer**, and without carrying the bias every store through `edi` was reported at the
parent object's offset 0. Interior-pointer bias is now carried, which is why `+0x50ac` — the player
name `docs/save-format.md` records — appears at all.

Both fixes are pinned by tests that recompute from the binary, and both are killed by mutation (see
[the sweep](#mutation-sweep)).

## Coverage, and the bound on every negative

| | |
| --- | ---: |
| operators | 1,906 |
| join depth published | 2 |
| operators with at least one recovered field | **748** |
| operators with none | **1,158** |
| direct call sites whose object was named | **1,468** |
| direct call sites whose object was **not** named | **8,749** |
| indirect calls, destination unknown | 130 |
| virtual-dispatch edges named but never followed | 25 |
| bodies the walk cannot finish | 1 (`netlockgame`) |

**The instrument names the object at 14% of the call sites it sees.** Every negative in this
document is bounded by that number and by nothing else. "No operator writes field X" means "none of
the 1,468 attributable call sites, plus 748 operator bodies, reaches a write to X" — it does not
mean the engine never writes it.

The one cross-check that does not share this mechanism is the absolute-address instrument, which
needs no call site at all; where the two are compared, the comparison is
[in its own section](#the-two-instruments-on-static-objects).

### The saturation curve

The depth is chosen from the curve, not from the answer. This is the same argument the previous
pass chooses its import depth by: a join that reaches every object from every operator describes the
call graph, not the operator.

| depth | operators with fields | distinct bases | distinct fields | accesses |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 130 | 55 | 188 | 362 |
| 1 | 632 | 98 | 891 | 3,163 |
| **2** | **748** | **100** | **1,198** | **5,371** |
| 3 | 750 | 100 | 1,291 | 6,669 |
| 4 | 751 | 100 | 1,338 | 7,332 |
| 5 | 751 | 100 | 1,349 | 7,930 |

The set of objects saturates at depth 2 and never grows again; the set of operators is within 0.4%
of its final value there. Depth 2 is published. Past it the table buys a slow trickle of extra
offsets at the cost of attributing them through longer chains, and the `depth` column is kept on
every row so a reader can take depth 0 or 1 instead.

Depth 0 — 130 operators, 7% — is the measure of how little is visible without the join, and it is
the number that says why this work was needed.

## The candidate structures

**Observed**: these operators reach these offsets through this base. **Inferred**: any reading of
what the structure is. The `name evidence` column is the raw word frequencies of the operator names
that converge on the base, given so the naming stays the reader's.

| base | kind | operators | fields | written | extent ≥ | name evidence (inferred) |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| `0x005ae958` | static | 124 | 70 | 46 | `0x2e8` | player 15, army 12, city 12, map 9 |
| `0x005a7b50` | static | 108 | 2 | 2 | `0x8` | imp 19, player 8, map 3, unit 3 |
| `0x005aa12c` | static | 73 | 288 | 214 | `0x6ccd` | combat 9, map 7, unit 6, army 4 |
| `0x005ae974` | static | 66 | 7 | 1 | `0x28` | combat 11, sprite 10, player 9, unit 6 |
| `0x00584ae8` | static | 55 | 57 | 13 | `0x2e00` | army 5, map 3, city 2, imp 2 |
| `0x00584cd4` | static | 38 | 15 | 12 | `0x57` | player 10, army 1, artifact 1 |
| `0x00585408` | static | 28 | 38 | 24 | `0x13b8` | sound 12, unit 2 |
| `0x005876d0` | static | 28 | 87 | 68 | `0x2a5` | map 7, faith 3, combat 1 |
| `0x005aef04` | static | 28 | 16 | 8 | `0x240` | region 16, city 2, faith 2 |
| `0x005d1e84` | indirect | 26 | 10 | 1 | `0x394` | net 7, faith 2 |
| `0x005a7b78` | indirect | 20 | 37 | 17 | `0xaa0` | unit 8, player 2 |
| `0x005cd2f4` | indirect | 20 | 4 | 0 | `0x14c` | unit 15, terrain 4 |
| `0x005a9e04` | static | 17 | 82 | 39 | `0x1d4` | imp 1, map 1 |

The full list is `reports/natives/state/structures.tsv` (100 rows); the per-field detail with
readers and writers is `structure-fields.tsv` (1,198 rows); the per-operator access map asked for by
this work is `operator-field-access.tsv` (5,371 rows, one per operator and field).

**`extent ≥` is a lower bound and never a size.** It is the highest offset seen plus its width. The
object at `0x005aa12c` reaches `+0x6ccd` here and the disassembly of its own save writer touches
`[ebp+23194h]` (`0x00482dd0`, observed in a local binary), so its real size is at least four times
the recovered extent.

### Shape of the recovered fields

Observed in the corpus of 1,198 fields:

| | |
| --- | ---: |
| 4 bytes only | 937 (78%) |
| address-taken, width unobserved | 189 (16%) |
| 1 byte | 49 |
| 2 bytes | 4 |
| reached through an index register — offset is within element **zero**, stride not recovered | 65 |
| read only | 434 |
| written by at least one operator | 764 |

A 32-bit engine of dword fields, which is what the save format also shows.

## Cross-checks against the savegame

`docs/save-format.md` decodes the serialized form of this same state, from the save *file* and from
the three functions that own the format. It is an independent instrument: it reads bytes on disk and
hand-read disassembly of the writer, not a call-site join.

### Agreement: the map's width and height, three ways

**Observed in a local binary.** This is the strongest single result here.

| instrument | says |
| --- | --- |
| absolute addresses, from the previous pass's committed table | `mapw` names `0x005ae9b4` and **no other** engine address; `maph` names `0x005ae9b8` and no other |
| this join | the object at `0x005ae958` has dword fields at `+0x5c` and `+0x60`, reached by `getelevation` and `setelevation` |
| arithmetic | `0x005ae958 + 0x5c = 0x005ae9b4`; `+0x60 = 0x005ae9b8` |
| `docs/save-format.md` | `LS_MAP_` begins `u32 width; u32 height` — two adjacent dwords, in that order |

Three mechanisms that share nothing land on the same two fields, in the same order, at the same
width. Pinned as `the_map_width_and_height_agree_across_three_instruments`.

### Agreement: the save writer's two documented offsets

**Observed in a local binary.** Both were recovered by this analysis independently of the document
that predicted them, and both were **initially absent** — which is what drove the `lea` and
interior-pointer fixes above.

| `docs/save-format.md` says | recovered here |
| --- | --- |
| the `LS_MULT` setup block is copied from `[gameobj+0x520]`, 164 bytes | a width-0 (address-taken) field at `0x005aa12c+0x520`, reached by 4 operators |
| a player name is `strcpy`d from `[player_i + 0x50ac]` | a width-0 field at `+0x50ac`, marked **indexed**, reached by exactly one operator: **`savegame`** |

The second agrees on the offset and **disagrees on the subject**, which is worth stating plainly.
The save documentation calls the base a *player* record. This analysis reaches `+0x50ac` as an
offset of the object at `0x005aa12c`, because the engine forms the pointer as
`lea ecx,[ebp+eax*4]` (`0x00482bef`) and then `lea edi,[ecx+50ACh]` (`0x00482c01`). Both readings
are consistent with **player records being an array embedded in, or indexed off, that object** — but
this instrument does not recover the stride, so it cannot confirm that and does not claim it. The
field is published as indexed precisely so it is not read as a scalar at `+0x50ac`.

### The cross-check that could not be made

`LS_MAP_` continues `u32 bytes_per_cell = 8`, and the natural prediction is a third adjacent dword
at `0x005ae958+0x64`. The join does find a read-write dword there with 58 readers — but **nothing
identifies it as the cell size**, and no operator name settles it. Recorded as **not established**;
it would take reading the `LS_MAP_` writer at `0x004C7390`'s neighbourhood by hand.

Likewise `LS_USER`'s eight 784-byte records with an in-memory stride of `0x400`, `LS_GAME`'s 12-byte
records and `LS_PLR_`'s per-player records have **no counterpart here at all**, for the reason in the
next section.

## The central negative

**No unit record, army record, city record or player record is recovered, and the operator names do
not converge on one.**

The naming instrument is the share of operators carrying a subject word that reach one base.
Measured (`reports/natives/state/subject-convergence.tsv`):

| subject | operators with the word | best base | share |
| --- | ---: | --- | ---: |
| `imp` | 24 | `0x005a7b50` | **79%** |
| `region` | 27 | `0x005aef04` | **59%** |
| `sound` | 21 | `0x00585408` | **57%** |
| `map` | 35 | `0x005ae958` | 26% |
| `net` | 29 | `0x005d1e84` | 24% |
| `city` | 63 | `0x005ae958` | 19% |
| `spell` | 58 | `0x005aee6c` | 16% |
| `player` | 100 | `0x005ae958` | 15% |
| `unit` | 122 | `0x005cd2f4` | 12% |
| `army` | 132 | `0x005ae958` | 9% |
| `building` | 64 | `0x005aa12c` | 6% |
| `artifact` | 62 | `0x005aa12c` | 2% |

The instrument is not broken: `net` picks out `0x005d1e84`, which the previous pass had already
established as the DirectPlay session object by an unrelated mechanism — virtual-dispatch taint.
That is the control, and it passes. `imp`, `region` and `sound` converge the way a subsystem
singleton should.

**The entity subjects do not, and the reason is mechanical.** A unit, an army or a city is not a
singleton in `.data`; it is a heap object reached as `mov eax,[container+n]` and then dispatched
through a vtable. This analysis deliberately **closes** the taint chain at that load — a pointer read
*out of* an object is a different object, and attributing its offsets to the container would merge
two structures into one. So the entity records are exactly the thing the instrument is built not to
guess at.

`docs/save-format.md` independently reports the same shape from the other side: `LS_SPR_` — units,
armies and heroes — is **polymorphic, variable-length, dispatched through the 10-entry table at
`0x004F73B8`**. Two instruments agreeing that the entity records are behind virtual dispatch was
not a recovery, but it did say where the next pass had to go — **reverse the ten class readers at
`0x004F73B8`'s targets** — and later on 2026-09-18 that is what the save work did.

**Updated, 2026-09-18.** The ten readers are walked and `LS_SPR_` now decodes. That gives the
entity records their **extents and their class partition** — eight classes, their allocation sizes,
their constructors, their vtables, and every field boundary inside each record — but it does **not**
give the instrument what it closes the taint chain to avoid: the readers name offsets into an
object, not meanings, and the binary has no RTTI, so not one of the eight classes can be named. The
save work bounds the entity structures from outside; this instrument still declines to guess at
what is inside them, and the 25% ceiling below is unchanged by it.

`no_entity_subject_converges_the_way_the_network_one_does` pins the ceiling at 25%, so a future
change that *does* recover an entity record fails the test. It is written to be falsified.

## The two instruments on static objects

A field of an object in `.data` can be reached two ways that share no mechanism: through the join,
and as a plain absolute address in some other body. The comparison searches
`[base, base + observed_extent)`.

| base | range end | interior bases | both | join only | absolute only |
| --- | --- | ---: | ---: | ---: | ---: |
| `0x005aa12c` | `0x005b0df9` | 27 | **76** | 212 | 97 |
| `0x00584ae8` | `0x005878e8` | 22 | 29 | 28 | 74 |
| `0x005ae958` | `0x005aec40` | 3 | 17 | 53 | 7 |
| `0x00585408` | `0x005867c0` | 8 | 15 | 23 | 6 |
| `0x005876d0` | `0x00587975` | 0 | **13** | 74 | **3** |
| `0x005869f4` | `0x00586ced` | 1 | 10 | 0 | 1 |
| `0x005aef04` | `0x005af144` | 1 | 8 | 8 | 8 |

Read this carefully, because the column that looks best is the weakest.

* **`both` is the real evidence.** 76 offsets of the object at `0x005aa12c` are seen by both
  instruments.
* **`join only` is not a disagreement.** The absolute instrument is blind to any field only ever
  touched inside a method, which is most of them.
* **`absolute only` is where the *range* is doing the work, not the object.** `interior_bases`
  counts how many other materialised bases the range contains: the range for `0x005aa12c` swallows
  27 of them, so an unknown share of its 97 absolute-only addresses belong to neighbours rather than
  to this object. `0x005876d0`, with **zero** interior bases and only 3 absolute-only addresses, is
  the clean row and the one to trust.

**Corrected during this work.** The first version bounded each object at *the next materialised base
above it*, which is wrong because C++ code takes the address of members: `0x005aa1dc` is
materialised as a base while sitting `0xb0` bytes inside the object at `0x005aa12c`. That rule
truncated a structure with fields out to `+0x6ccd` at `+0xb0` and threw away most of the comparison
— it reported 7 agreeing offsets where the corrected rule reports 76. A test asserts the property
the old rule violated: no offset the join found may lie outside the range the comparison searches.

## What this does not establish

* **No structure is named.** Every reading in the `name evidence` column is inferred from operator
  names and none of it is asserted as fact. `struct_0x005ae958` with 70 offsets and their widths is
  the honest description, and it is directly usable by a reimplementation as it stands.
* **No field is named.** Width and direction are observed; meaning is not. The map's width and
  height are the exception, and only because three instruments agree.
* **Sizes are lower bounds.** `observed_extent` is the highest offset reached, never the object's
  size.
* **Strides are not recovered.** 65 fields are reached through an index register; their offset is
  within element zero and the element size is unknown.
* **Nothing follows a virtual call.** 25 dispatch edges are named by object and slot; none is
  entered. This is where the entity records live.
* **Nothing follows a pointer loaded out of an object.** By design: it would merge structures.
* **Behaviour is not recovered.** This says which bytes an operator touches, not what it means to
  touch them.

## Mutation sweep

Thirteen mutants, applied to the decision logic in both directions, with the sources verified
byte-identical against a SHA-256 after every restore. Two controls bracket the sweep: a positive
control that should die, and a **null mutant** — a comment — that should survive. It does, so the
harness is not reporting everything as killed.

The kills are split, because **the staleness test kills any mutant at all** and is therefore not
evidence that the *decision logic* is checked. Only tests that recompute the model from the binary
count as a semantic kill.

| mutant | verdict |
| --- | --- |
| CONTROL: join never leaves the operator body | **killed semantically** |
| CONTROL (null): a comment | **survived**, as required |
| stop recording `lea` (address-taken) fields | **killed semantically** |
| forget that an interior pointer was indexed | **killed semantically** |
| widen the field-offset bound to everything | **killed semantically** |
| agreement range reverts to the next static base | **killed semantically** |
| drop the `dereferenced` guard in `field_offset` | killed only by the artifact comparison |
| attribute a callee's other objects to the caller's base | killed only by the artifact comparison |
| let a statically based store count as `writes_through_pointer` | killed only by the artifact comparison |
| drop the interior-pointer bias | killed only by the artifact comparison |
| let a dereferenced base seed the join | killed only by the artifact comparison |
| off-by-one on the depth cap | killed only by the artifact comparison |
| `element` flag never reaches the published `indexed` column | killed only by the artifact comparison |

**Seven of thirteen are caught only by comparing against a file this repository generated.** That is
a weaker kill than it looks — it detects that the output moved, not that the output became wrong —
and it is stated rather than folded into a "13/13 killed" line. Strengthening those seven means
writing more assertions that recompute, of the kind the map-dimension and save-offset checks already
are.

The harness is `mutate.py`-shaped and lives outside the repository; the mutants are listed above in
full so the sweep can be reconstructed.

## The control that says this changed nothing it should not have

The walk in `operator_bodies.rs` was extended, not replaced. The check is that
`reports/natives/operator-bodies.tsv`, `global-clusters.tsv` and `summary.md` — every number
`docs/native-operator-bodies.md` quotes — regenerate **byte for byte** from the extended analyser.
They do, and `the_previous_operator_table_regenerates_byte_for_byte` fails if that stops being true.

Getting there required two deliberate exclusions, both recorded in the code:

* a store through a **statically** materialised pointer is not folded into `writes_through_pointer`,
  because that column's published membership was counted before static bases were tracked at all;
* a pointer whose bias left the believed range keeps its place in that chain but contributes no
  field, so a guessed offset cannot enter the table.
