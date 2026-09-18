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
| `2` | not read by the engine | Observed in a local binary | see [byte 2](#byte-2-the-one-field-that-is-not-settled) |
| `3`–`10` | not read by the engine; largely uninitialised | Observed in a local binary + Observed in the corpus | no read exists (below); bytes 5–10 contain leftover text such as `frames` and `\imps\` |
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

| Mode | Target | Behaviour | Evidence class |
| ---: | --- | --- | --- |
| 0 | `0x0049DA01` | index reaches the frame count → **reset to 0**; loop | Observed in a local binary |
| 1 | `0x0049D9F7` | index reaches the frame count → **held at count − 1**; one-shot | Observed in a local binary |
| 2 | `0x0049D9F7` | same target as mode 1 | Observed in a local binary |
| 3 | `0x0049DA01` | same target as mode 0 | Observed in a local binary |
| 4 | `0x0049DA01` | wraps like mode 0, but over a **doubled** cycle: ping-pong | Observed in a local binary |
| 5–7 | none | past the `ja` bound; the index is never clamped and the frame lookup's own range check (`0x0049ACAB`) then fails | Observed in a local binary |

Modes 0 and 4 share a jump-table slot. What makes 4 different is that **two other sites special-case
exactly 4**:

- `Imp::CycleLength` (`0x0049D8F0`): `cmp cl,4` at `0x0049D903`, then `lea eax,[edx+edx-1]` at
  `0x0049D90E` — the cycle is `2 * frames - 1` steps long instead of `frames`.
- `Imp::GetFrame` (`0x0049ABE0`): `cmp dl,4` at `0x0049AC94`, then for a position at or past the
  frame count, `lea ebx,[edi+edi]` / `sub ebx,edx` / `sub ebx,2` at `0x0049ACA1`–`0x0049ACA6` —
  position `i` shows frame `2 * frames - i - 2`.

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

That flag is a **horizontal flip**, on two independent counts:

- It is passed to the blitter as an argument: `mov ecx,[esi+28h]` / `and ecx,1` / `push ecx` at
  `0x0049D40C`–`0x0049D419`, into `call 004F46F0h`.
- When it is set, the sprite's anchor x is negated: `test byte [ecx+28h],1` at `0x0049CCA0`, then
  `neg ecx` at `0x0049CCCC` with an odd-width correction at `0x0049CCD3`. That is the mirror image
  of the placement rule in [hotspots.md](hotspots.md), which is the strongest possible check that
  the flag means what it looks like.

So the issue's "five cycles that appear directional" are **five stored facings covering eight
directions**: facings 0–4 as stored, directions 5, 6 and 7 drawn as facings 3, 2 and 1 flipped. The
first and last facings are the two that are never mirrored, because they face straight along the
axis and there is nothing to flip them into.

Corpus, **Observed in the corpus**:

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

The argument is a bounded negative, so the bound matters:

1. A sequence-record address can only be formed one way — scale an index by 16 and add the header's
   sequence-table pointer from offset `0x1C`. The survey lists every `shl reg,4` in the IMP module
   (`0x00499000`–`0x004A0000`) and flags the nine that sit near such a load. All nine were read by
   hand; four of them index frame records, which share the 16-byte stride.
2. Across the whole module, the survey lists every register-relative memory read at displacement
   0–15. At displacement 2 there is **exactly one byte-sized read in the entire module**,
   `0x0049B25D`, and it belongs to the 1,024-byte palette copy loop at `0x0049B220`, not to any
   record. At displacements 3–10 there are **none at all**.
3. The playback object has no timer. Its constructor (`0x0049C830`) initialises fields `0x04`–`0x3C`
   and the only numeric default is `mov dword [edx+14h],8` — the direction denominator, not a
   period. `Advance` takes no time argument and advances exactly one step per call.

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

### Byte 2: the one field that is not settled

Byte 2 of the sequence record is the only unexplained byte with a non-garbage distribution:

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
   the period means finding what increments it and at what rate; that was not chased here.
2. **The compass bearing of direction 0.** See above.
3. **Facing-record bytes 0–1.** Zero everywhere in this archive. Whether they mean anything in some
   other build is unanswerable from this corpus.
4. **Sequence byte 2.** Plausibly a frame rate, unread by the engine, unconfirmed.

## Test coverage

`src/imp_anim.rs`:

- `recovers_the_mode_count_from_the_range_check_rather_than_assuming_it` and
  `refuses_a_switch_whose_mask_is_not_the_three_low_bits` build a synthetic PE with the engine's
  dispatch shape; the recovery must read the mode count and mask out of the code and refuse a
  binary whose partition differs.
- `ping_pong_length_and_reflection_describe_one_traversal` checks the two ping-pong sites against
  each other rather than against a literal.
- `a_mirrored_sequence_covers_twice_its_facings_less_the_two_shared_ends` checks the direction fold's
  fixed points.
- `every_cycle_mode_the_corpus_uses_has_a_slot_in_the_recovered_dispatch` runs the recovered
  dispatch against all 4,667 sequences and asserts every facing metadata word is zero. It needs the
  installed game: set `LOM_GAME_DIR` to the `English` directory and `LOM_LISTFILE` to a listfile.
  Without them it prints a skip line.
