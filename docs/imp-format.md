# IMP animation control: cycle mode, direction mirroring, and timing

**What this file covers:** the animation-control fields of an `.imp` — which frame plays next, which
stored facing a direction resolves to, and where the playback cadence comes from. Sprite *placement*
is a separate solved problem and lives in [hotspots.md](hotspots.md); this file does not restate it.

Everything here was read out of the engine's own decoder rather than observed in gameplay. The
binary is `lomse.exe` 3.02,
SHA-256 `a505f399d5be73fe0a2215633f663717f28daeb3075bbcc05b47d40653669052`; the corpus is
`imp.mpq`, SHA-256 `cb5c1068ea21936e5c301ff54d3c498c9c1a8cc19007b7d2cc8ac724abc6e2b2`, 1,800 `.imp`
members, 4,667 sequence records, 14,921 facing records.

Reproduce all of it with:

```
cargo run --release --example imp_anim_survey -- lomse.exe imp.mpq LISTFILE
```

## A note on the three vocabularies

They do not line up, and the confusion is load-bearing, so it is worth stating once:

| The authoring tool's `.h` | This repository's decoder | The engine |
| --- | --- | --- |
| "sequence" = the whole file (`Anm_willowa`) | `ImpSprite` | the `Imp` object |
| **"cycle"** = a named action (`WILLOWA_MOVE`) | `ImpSequence`, 16-byte record | a sequence, indexed by *action* |
| — | `ImpFacing`, 8-byte record | one entry in a list indexed by *direction* |

[Issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2) uses "cycle" for the
8-byte record, i.e. the third row. Below, the decoder's names are used: **sequence** for the 16-byte
record and **facing** for the 8-byte one. A sequence's facing count and its *direction* count are
different numbers, and that difference is the whole of the direction answer.

## The 16-byte sequence record

| Bytes | Meaning | Evidence class | Proof |
| --- | --- | --- | --- |
| `0`, bits 0–2 | **cycle mode** | Observed in a local binary | `mov cl,[edi]` / `and ecx,7` / `jmp dword [ecx*4+49DA68h]` at `0x0049D9E6`–`0x0049D9F0` |
| `0`, bits 3–7 | never read | Observed in a local binary | the only three reads of byte 0 all mask with 7: `0x0049D9E8`, `0x0049AC8A`, `0x0049D900` |
| `1`, bit 7 | **mirror facings to fill directions** | Observed in a local binary | `test byte [edx+1],80h` at `0x0049AC4E`, `0x0049AD50`, `0x0049D95F` |
| `1`, bits 0–6 | never read | Observed in a local binary | those three tests are the only reads of byte 1 on a sequence record |
| `2` | not read by the engine | Observed in a local binary | see [byte 2](#byte-2-the-field-that-is-not-settled) |
| `3` | not read by the engine; **not** garbage | Observed in a local binary + Observed in the corpus | no read exists (below); holds `0x01` in 4,661 of 4,667 records and `0x04` in the other 6 |
| `4` | not read by the engine; **not** garbage | Observed in a local binary + Observed in the corpus | no read exists (below); holds `0xFF` in **all** 4,667 records |
| `5`–`10` | not read by the engine; uninitialised | Observed in a local binary + Observed in the corpus | no read exists (below); contain leftover text such as `frames` and `\imps\` |
| `11` | facing count | Observed in a local binary | `mov bl,[edx+0Bh]` at `0x0049AC5A`, `0x0049AC7E`, `0x0049AD5C`, `0x0049AD7F`, `0x0049C2D6`, `0x0049D967`, `0x0049D973` |
| `12`–`15` | pointer to the facing table | Observed in a local binary | `mov ebx,[edx+0Ch]` at `0x0049AC85`; indexed `[ebx+esi*8]` at `0x0049AC8F` |

Bytes 11–15 were already decoded; they are listed because they are the **control**. The engine reads
the sequence count from loaded-header offset `0x1A` and the table pointer from `0x1C`
(`mov cx,[esi+1Ah]` / `mov ecx,[esi+1Ch]` at `0x0049ADDB`, `0x0049ADE3`), which are exactly the
file offsets 26 and 28 `ImpSprite::parse` already reads. A method that reproduces those two, plus
the 16-byte stride and the 8-byte facing stride, is a method that has found the right structure
before it is asked anything new.

## The 8-byte facing record

| Bytes | Meaning | Evidence class | Proof |
| --- | --- | --- | --- |
| `0`–`1` | **unused** | Observed in the corpus | all 14,921 facing records in `imp.mpq` hold `0x0000` |
| `2`–`3` | frame count | Observed in a local binary | `mov di,[ebx+esi*8+2]` at `0x0049AC8F`; `mov cx,[eax+2]` at `0x0049D915` |
| `4`–`7` | pointer to the frame table | Observed in a local binary | `mov eax,[eax+ecx+4]` at `0x0049C345` |

This is the issue's "raw 16-bit field". **It is zero in every record in the shipped archive**, so
there is nothing in the corpus for it to mean. A corpus test asserts this, so the day a build or a
mod puts something there, the suite says so rather than the reader having to remember.

## Cycle mode: what the engine does at the end of a cycle

`Imp::Advance` (`0x0049D9A0`) increments the frame index at `0x0049D9E0`, then dispatches on the
mode through a five-entry table at `0x0049DA68`. The mode count is not an assumption — it is the
`cmp ecx,4` / `ja` bound at `0x0049D9EB` that the processor itself enforces.

Every mode compares the new index against `Imp::CycleLength` (`0x0049D8F0`), **not** against the
stored frame count. The two are the same number for every mode except the ping-pong, and that
distinction is exactly what makes the ping-pong work: mode 4 wraps at `2N − 1` rather than at `N`.

| Mode | Target | Behaviour | Evidence class |
| ---: | --- | --- | --- |
| 0 | `0x0049DA01` | index reaches the cycle length → **reset to 0**; loop | Observed in a local binary |
| 1 | `0x0049D9F7` | index reaches the cycle length → **held at length − 1**; one-shot | Observed in a local binary |
| 2 | `0x0049D9F7` | same target as mode 1 | Observed in a local binary |
| 3 | `0x0049DA01` | same target as mode 0 | Observed in a local binary |
| 4 | `0x0049DA01` | wraps like mode 0, but the cycle length is doubled: ping-pong | Observed in a local binary |
| 5–7 | none | past the `ja` bound; the index is never clamped and the frame lookup's own range check (`0x0049ACAB`) then fails | Observed in a local binary |

Modes 0 and 4 share a jump-table slot. What makes 4 different is that **two other sites special-case
exactly 4**:

- `Imp::CycleLength` (`0x0049D8F0`): `cmp cl,4` at `0x0049D903`, then `lea eax,[edx+edx-1]` at
  `0x0049D90E` — the cycle is `2 * frames - 1` steps long instead of `frames`.
- `Imp::GetFrame` (`0x0049ABE0`): `cmp dl,4` at `0x0049AC94`, then for a position at or past the
  frame count, `lea ebx,[edi+edi]` / `sub ebx,edx` / `sub ebx,2` at `0x0049ACA1`–`0x0049ACA6` —
  position `i` shows frame `2 * frames - i - 2`.

**This is established, not merely consistent.** The two sites are not independent guesses that
happen to agree: they run on the same sequence record in the same call, `Advance` obtains the length
from the first and hands the index to the second, and composing them is arithmetic rather than
inference. Length `2N − 1` with positions `0..N−1` taken as themselves and `N..2N−2` folded to
`2N − i − 2` enumerates `0,1,…,N−1,N−2,…,0` — one traversal out and back, each endpoint visited
once. There is no reading of those two instructions under which mode 4 is anything else.

Together those are a ping-pong: `0,1,…,N−1,N−2,…,0`, which is `2N−1` steps and visits the two
endpoints once each. The unit test `ping_pong_length_and_reflection_describe_one_traversal` walks
the length rule through the fold rule for every frame count and asserts the traversal, so a wrong
constant in either site fails rather than merely looking plausible.

**Both endings also report completion.** `0x0049DA0C` stores 1 into the local that `Advance` returns
at `0x0049DA4E`. So a looping `DIE` is not a bug: the caller is told the cycle ended and switches the
action itself. That is why only 5 of 4,667 sequences bother with mode 1.

### What the corpus uses

| Mode | Sequences |
| ---: | ---: |
| 0 (loop) | 3,707 |
| 1 (one-shot) | 5 |
| 4 (ping-pong) | 955 |

Cross-referenced against the action names in the generated `.h` files, the split is exactly what the
reading predicts — **Observed in the corpus**:

| Action | mode 0 | mode 4 |
| --- | ---: | ---: |
| `STAND` | 224 | 0 |
| `MOVE` | 250 | 14 |
| `DIE` | 219 | 10 |
| `CORPSE` | 219 | 12 |
| `MELEE_ATTACK` | 44 | **166** |
| `GET_HIT` | 30 | **170** |
| `DEFEND` | 23 | **171** |
| `MINOR_SPELL` | 17 | **65** |

Standing and walking loop; a sword swings out and comes back. The disassembly and the corpus agree
without either having been fitted to the other.

## Direction: five facings, eight directions

`Imp::DirectionCount` (`0x0049D920`) is the rule:

```asm
0049d95f  test byte [eax+1],80h      ; the mirror bit
0049d963  je   0049D971h
0049d967  mov  cl,[eax+0Bh]          ; facing count
0049d96a  lea  eax,[ecx+ecx-2]       ; mirrored: 2N - 2 directions
0049d96e  ret  4
0049d971  ...
0049d973  mov  dl,[eax+0Bh]          ; not mirrored: N directions
```

The fold that picks the facing lives in `Imp::GetFacing` (`0x0049AD5A`–`0x0049AD71`) and in the
identical block inside `Imp::GetFrame` (`0x0049AC54`–`0x0049AC70`): a direction at or past the facing
count becomes `2N − direction − 2`, and the routine raises a flag for its caller.

That flag is a **horizontal flip**. One observation carries the claim, with one corroboration:

- **The observation.** When the flag is set, `ImpPlayer::GetPlacement` (`0x0049CC80`) negates the
  anchor-relative x. `test byte [ecx+28h],1` at `0x0049CCA0` selects the flipped branch, which runs
  `neg ecx` at `0x0049CCCC`. Compare the unflipped branch at `0x0049CCFC`–`0x0049CD01`, which
  computes `placement_x - (width >> 1)` — the x half of the rule in [hotspots.md](hotspots.md),
  established months earlier by a different method. A sign flip of that value is a horizontal
  mirror; nothing else it could be.
- **The corroboration, and only that.** The flag is also pushed as an argument to the blitter:
  `mov ecx,[esi+28h]` / `and ecx,1` / `push ecx` at `0x0049D40C`–`0x0049D419`, into
  `call 004F46F0h`. Nothing inside `0x004F46F0` has been read, so this shows the flag reaches the
  drawing code — not what the drawing code does with it. It is consistent with a flip and would be
  consistent with several other things.

### The flipped x, with the parity the right way round

```asm
0049ccc8  shr  ecx,1                 ; ecx = width >> 1
0049ccca  add  ecx,esi               ; + placement_x
0049cccc  neg  ecx                   ; = -((width >> 1) + placement_x)
0049ccce  test dl,1                  ; dl = low byte of the width
0049ccd1  jne  short 0049CCD4h       ; width ODD -> jump, skipping the dec
0049ccd3  dec  ecx                   ; runs when the width is EVEN
```

```
flipped_x = -((width >> 1) + placement_x) - (width even ? 1 : 0)
```

**Corrected, 2026-09-17.** An earlier revision of this page called `0x0049CCD3` an "odd-width
correction". It is the opposite: `jne` is taken when the tested bit is **set**, so the `dec` it
jumps over runs when the bit is **clear**. Implementing the earlier wording puts every even-width
sprite one pixel off — and even widths are the majority. `recover_mirror_parity` now reads the
branch polarity out of the binary and two unit tests pin both polarities, so the sentence cannot
drift again.

The extra pixel is **Observed, not derived.** Reflecting the unflipped span about the anchor column
gives `-(left + width - 1)`, which equals the engine's value *exactly* on odd widths and is two
pixels away on even ones. So the `dec` is the engine's own convention rather than a consequence of
mirroring, and `the_flipped_placement_reflects_the_established_rule_exactly_on_odd_widths` asserts
both halves of that — including the even-width discrepancy, so nobody "fixes" it to match the
algebra.

So the issue's "five cycles that appear directional" are **five stored facings covering eight
directions**: facings 0–4 as stored, directions 5, 6 and 7 drawn as facings 3, 2 and 1 flipped. The
first and last facings are the two that are never mirrored, because they face straight along the
axis and there is nothing to flip them into.

Corpus, **Observed in the corpus**. 3,379 of 4,667 sequences have the mirror bit set — **not** the
2,931 that the byte-1 histogram shows for the value `0x80` exactly. The other 448 hold `0x81`,
`0xCC` or `0xFF`: the bit is set with other bits alongside it, and since the engine tests the bit and
never the byte, all 448 mirror.

| mirror bit | facings | directions | sequences |
| --- | ---: | ---: | ---: |
| set | 5 | **8** | 2,234 |
| clear | 1 | 1 | 1,232 |
| set | 1 | 0 | 991 |
| set | 2 | 2 | 84 |
| clear | 2 | 2 | 56 |
| set | 33 | 64 | 28 |
| set | 7 | 12 | 15 |
| set | 9 | 16 | 13 |
| set | 3 | 4 | 8 |
| set | 13 | 24 | 6 |

The 991 single-facing sequences with the mirror bit set advertise **zero** directions, which is
`2 × 1 − 2`. It is inert rather than broken: the only direction ever asked for is 0, and 0 is below
the facing count, so `GetFacing` returns facing 0 unmirrored before the fold is reached. Treat the
mirror bit as meaningless when the facing count is 1.

### What is *not* established: which compass bearing is direction 0

`Imp::SetFacing` (`0x0049DCC0`) adds a global at `0x005AEC3C` to the requested value, wraps it into
0–7, and then calls the direction setter with `(facing + 1) & 7` out of 8. So there are two rotations
between a GameScript-level facing and a stored facing index, and neither's zero point is anchored to
a compass bearing by anything read so far. **Inferred**, not observed: direction indices increase in
one consistent rotational order. Naming them N/NE/E/… requires either a gameplay observation or a
GameScript call site whose bearing is known independently.

## Timing: there is none in the file

This is the part of issue #2 that has no answer of the shape the issue expects, and saying so
plainly is the honest result.

**Claim, Observed in a local binary: no field of an `.imp` is read by `lomse.exe` as a duration,
delay, frame rate or tick count.**

The argument is a bounded negative, so the bound is the whole of it. It rests on two mechanical
scans and one structural observation.

### Refuted, 2026-09-17: the first version of this argument

An earlier revision argued that "a sequence-record address can only be formed by scaling an index by
16 and adding the header pointer at `0x1C`". **That is false, and it is refuted by code this page
already cites.** `Imp::SetAction` caches the record pointer into the player object —
`0x0049DAA2 mov [esi+24h],eax` — and `Imp::CycleLength` reads it straight back at `0x0049D8F7`
with no scaling anywhere. A pointer that is stored and reloaded is reachable from every caller of
the function that reloads it, and `CycleLength` has 12 out-of-module callers. Scanning for the
arithmetic was looking in the wrong place.

### Part 1 — who can obtain a sequence-record pointer

`Imp::GetSequence` (`0x0049ADB0`) is the only function that returns one. Enumerating its direct
call sites over the whole `.text` is exhaustive, and there are **five**: `0x0049D89F`,
`0x0049D9C4`, `0x0049DA97`, `0x0049DB31`, `0x0049DBF7`. All five are inside the IMP module. The four
inline formation sites are in-module too. So no code outside the module ever holds a sequence-record
pointer, and bounding the field scan to the module is a closure rather than a guess.

Out-of-module callers of `CycleLength` and `Advance` *call* those functions; they never receive the
pointer.

### Part 2 — what the module reads through such a pointer

The survey follows every sequence-record pointer from both places one comes into existence — the
header's table at `0x1C` (with an index added, since a table base is not a record) and the player's
cached record at `0x24` — and reports every field read through it. Inside the module the answer is:

| Displacement | In-module reads | What they are |
| ---: | ---: | --- |
| 0 | 1 | the control byte (`0x0049AC88`) |
| 1 | 2 | the mirror bit (`0x0049AC4E`, `0x0049AD50`) |
| **2–10** | **0** | — |
| 11 | 5 | the facing count |
| 12 | 6 | the facing-table pointer |

Two limits, since the negative rests on them. The walk is linear and stops at the first `call`, so
reads reached only through a call return are invisible to it — the control-byte reads at
`0x0049D9E6` and `0x0049D900` are two such, and both are at displacement 0, already covered.
And a displacement cannot type a struct: offset `0x1C` is the sequence table on the IMP header but
the *current frame record* on the player object (`0x0049CC80`), and `0x24` is used by many unrelated
objects. That is why the table above is filtered to the module, and why Part 1 has to exist.

### The reporting scan, stated as measured

The survey also lists every base-relative read at displacements 0–15 anywhere in the module,
regardless of which struct it touches. Read that output as measured, not as the negative:

- **Byte-sized reads at displacement 2: exactly one in the whole module** — `0x0049B25D`, inside the
  1,024-byte palette copy loop at `0x0049B220` (which reaches the palette through header offset 8 at
  `0x0049B247`), not on any record. That loop is also the subject of a recorded channel-order
  contradiction — see the research log; `imp.rs` is deliberately unchanged.
- **Byte-sized reads at displacements 3–10: none.**
- **Wider reads at those displacements: 124 of them** — 20 word and 43 dword reads at displacement
  4, 2 and 56 at displacement 8, 2 at displacement 6, 1 at displacement 10. They are not zero, and
  the earlier revision of this page said they were; only the byte-sized rows are empty. They are
  reads of other structures — frame heights at frame `+4`, frame-table pointers at facing `+4`,
  pixel pointers at frame `+0xC`, and fields of unrelated objects. Part 2 is what excludes them from
  being sequence-record reads; this list on its own does not.

The scan excludes absolute and `esp`-based operands, stores, and `lea` — the last because it
computes an address and touches no memory, and counting it inflated these totals by six. **Indexed
operands and `ebp` bases are included**, which matters: `0x0049AC8F mov di,[ebx+esi*8+2]` is a real record read at
displacement 2, and `ebp` is an object pointer in this module rather than a frame pointer
(`0x0049ABFC`, `0x0049AC0F`, `0x0049AC43`). An earlier revision dropped both, which would have let a
timing field read as `mov cx,[edi+ebx*16+2]` produce no hits at all.

### Part 3 — the playback object has no timer

Its constructor (`0x0049C830`) initialises fields `0x04`–`0x3C` and the only numeric default is
`mov dword [edx+14h],8` — the direction denominator, not a period. `Advance` takes no time argument
and advances exactly one step per call.

Cadence is therefore entirely the caller's. Two callers show the two shapes it takes:

- **Event-driven.** Of the 31 direct callers of `Advance`, most are game-logic sites that step the
  animation when something happens — e.g. `0x004AD7D3`, reached after an action change at
  `0x004AD7C4`.
- **A shared global counter.** The terrain-sprite driver at `0x0050C31F`–`0x0050C351` computes
  `(per-object phase + [0x005AF134]) mod CycleLength(player)` and drives the index from that. One
  global counter, one modulus per object: every terrain sprite in a scene animates off the same
  clock at the same rate, differing only in phase.

So the viewer's fixed interval is **not** a guess that better metadata would replace — the metadata
does not exist. The right fix is a single global interval, which is a property of the engine's tick
loop and not of the IMP container. What that interval is in milliseconds has **not** been
established here; see [what is still open](#what-is-still-open).

### Byte 2: the field that is not settled

Byte 2 of the sequence record has a non-garbage distribution. It is **not** the only such byte —
bytes 3 and 4 are patterned too, and so are the high bits of byte 0; those are listed under
[what is still open](#what-is-still-open). Byte 2 is the one with enough spread to be worth a
hypothesis:

| Value | 1 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 15 | 16 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Sequences | 27 | 8 | 55 | 456 | 30 | 239 | 122 | 463 | 114 | 10 | 3,139 | 4 |

- **Observed in a local binary:** the engine never reads it (§ above).
- **Observed in the corpus:** it is not the sequence's frame count (97 of 4,667 match). It is a
  property of the *file*, not of the action: 1,624 of 1,800 members give every sequence the same
  byte 2, and the per-action breakdown has no structure — `MOVE` is most often 10 but also 15, 6, 11
  and 8.
- **Inferred, weakly:** a small integer in 1–16, defaulting to 15, constant per exported file, is
  shaped like a frames-per-second the authoring tool recorded. Nothing rules out a version number, a
  compression setting or a source-tool field.

Using byte 2 as a frame rate in the viewer would be a guess dressed as metadata. It is not what the
engine does.

## What is still open

1. **The engine's animation tick period in milliseconds.** The counter is `0x005AF134`. Establishing
   the period means finding what increments it and at what rate; that was not chased here. A
   literal-dword search over `.text` finds only readers, so the writer is reached through a base
   pointer — probably `0x005AF130 + 4` — and needs a data-xref pass rather than a byte search.
2. **The compass bearing of direction 0.** Two rotations sit between a script-level facing and a
   stored index: a global bias at `0x005AEC3C` and a `+1` at `0x0049DCDD`. Neither zero point is
   anchored to a bearing by anything read here.
3. **Facing-record bytes 0–1.** Zero in all 14,921 records in this archive. Whether they mean
   anything in another build is unanswerable from this corpus.
4. **Sequence byte 2.** Plausibly an export frame rate, unread by the engine, unconfirmed.
5. **Sequence bytes 3 and 4.** Byte 3 is `0x01` in 4,661 records and `0x04` in 6; byte 4 is `0xFF`
   in all 4,667. Unread by the engine and too constant to guess at, but constant is not garbage and
   they should not be filed with bytes 5–10.
6. **Byte 0 bits 3–7.** Masked away by every reader, but **501 of 4,667 records carry a patterned
   value there** — `0xF8` in 265, `0xC0` in 182, `0x78` in 52, `0xE0` in 2 — and those patterns
   correlate with byte 1 taking `0x81`, `0xCC` and `0xFF`. Something wrote them deliberately.
7. **Byte 1 bits 0–6.** Same story: set in 448 records, never tested.

## Test coverage

`src/imp_anim.rs`. The default `cargo test` run covers the recovery machinery against synthetic PEs;
the two tests that need the installed game are `#[ignore]`d, so the summary reports them as
`ignored` rather than as passes.

Default:

- `recovers_the_mode_count_from_the_range_check_rather_than_assuming_it`,
  `refuses_a_switch_whose_mask_is_not_the_three_low_bits` and
  `ignores_a_mask_applied_to_a_register_the_jump_does_not_index_with` — the dispatch recovery must
  read the mode count and mask out of the code, refuse a binary that partitions the byte
  differently, and refuse a mask applied to a register the jump does not index with.
- `recovers_the_ping_pong_mode_from_the_doubled_cycle_length` and
  `a_comparison_without_a_doubling_is_not_taken_for_the_ping_pong_mode` — the ping-pong mode must
  come from the `cmp` that guards the `2N−1`, and a bare comparison must not be mistaken for it.
- `recovers_the_mirror_bit_from_the_doubled_direction_count` — likewise for the mirror bit.
- `a_jne_over_the_decrement_means_the_decrement_runs_on_even_widths` and
  `a_je_over_the_decrement_means_the_opposite_parity` — both branch polarities, which is the
  specific error this page carried.
- `ping_pong_length_and_reflection_describe_one_traversal` — the length and fold rules must compose
  into one out-and-back traversal for every frame count.
- `a_mirrored_sequence_covers_twice_its_facings_less_the_two_shared_ends` — the direction fold's
  fixed points.
- `the_flipped_placement_reflects_the_established_rule_exactly_on_odd_widths` — the flipped x against
  `docs/hotspots.md`'s rule, asserting the even-width discrepancy as well as the odd-width identity.

Opt-in, with `LOM_GAME_DIR` set to the `English` directory and `LOM_LISTFILE` to a listfile, run via
`cargo test --release -- --ignored`:

- `the_recovered_rules_match_the_installed_executable` — `recover` refuses on any disagreement with
  this module, so reaching the end is the check. It also asserts Part 1 of the negative (every
  `GetSequence` call site is in-module) and Part 2 (no in-module read at displacements 2–10 through
  a record pointer).
- `the_corpus_matches_the_recovered_rules` — every mode the archive uses has a dispatch slot, every
  advertised direction resolves, every facing metadata word is zero, and the measured counts hold:
  4,667 sequences, 955 ping-pong, 3,379 with the mirror bit set, 2,931 whose byte 1 is exactly the
  bit. Those counts are what make `PING_PONG_MODE` falsifiable against the shipped archive — set it
  to 3 and the ping-pong count goes to zero.

### Why the constants are parameters

`PING_PONG_MODE` and `SEQUENCE_MIRROR_BIT` are no longer read by the rules that use them. Both are
passed in from what `recover` read out of the binary, and the constants exist only as the value
`recover` refuses to disagree with. The reason is a review finding worth recording: with
`PING_PONG_MODE` mutated from 4 to 3, an earlier version of this suite stayed **green** — every
ping-pong sequence in the archive would have truncated to forward-only and the survey would have
labelled a mode with no users as the ping-pong. The traversal test compared `cycle_length` against
`frame_for_cycle_index`, and both read the same constant, so it was self-consistent for any value.
That is this repository's own recorded lesson: agreement is not confirmation.
