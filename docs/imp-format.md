# IMP animation control: cycle mode, direction mirroring, and timing

**What this file covers:** the animation-control fields of an `.imp` — which frame plays next, which
stored facing a direction resolves to, where the playback cadence comes from, and how the viewer
consumes all of that. Sprite *placement*
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

## Timing: there is none in the *file*

This is the part of issue #2 that has no answer of the shape the issue expects, and saying so
plainly is the honest result. It is only half the answer, though: the interval exists, it is simply
not in the asset. See [timing: not in the asset, but in the
engine](#timing-not-in-the-asset-but-in-the-engine) for where it is. This section establishes the
negative that sends the question there.

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

`Imp::GetSequence` (`0x0049ADB0`) is the only function that returns one. Enumerating its direct call
sites over the whole `.text` gives **five**: `0x0049D89F`, `0x0049D9C4`, `0x0049DA97`, `0x0049DB31`,
`0x0049DBF7`. All five are inside the IMP module.

That enumeration only sees `NearBranch32` operands, so on its own it leaves the indirect route open
— a vtable slot, a dispatch table, a `mov reg,imm32` and an indirect call. **The address
`0x0049ADB0` appears nowhere in the file as a literal dword**, which is what actually closes it: an
address that is never taken cannot be called indirectly. The survey prints that count, and the
opt-in test asserts it is zero.

Out-of-module callers of `CycleLength` and `Advance` *call* those functions; they never receive the
pointer.

**What Part 1 does not cover.** A sequence record can also be formed inline, without calling
`GetSequence`, by anything holding the IMP header. The stride heuristic cannot bound that:
`sequence_record_sites` over the whole `.text` finds 226 `shl reg,4` sites, 22 of them near a dword
load at displacement `0x1C`, and **13 of those 22 are outside the module**. Offset `0x1C` is used by
plenty of structs, so that scan is a device for narrowing a set to read by hand, not evidence.
Parts 2 and 2b are what cover the inline route. An earlier revision of this page ran that scan over
the module only and concluded the sites were all in-module, which was true by construction and
therefore worth nothing.

### Part 2 — what the module reads through such a pointer

The survey follows every sequence-record pointer from both places one comes into existence — the
header's table at `0x1C` (with an index added, since a table base is not a record) and the player's
cached record at `0x24` — and reports every field read through it. Inside the module:

| Displacement | In-module reads | What they are |
| ---: | ---: | --- |
| 0 | 2 | the control byte (`0x0049AC88`, `0x0049D8FE`) |
| 1 | 3 | the mirror bit (`0x0049AC4E`, `0x0049AD50`, `0x0049D95F`) |
| **2–10** | **0** | — |
| 11 | 6 | the facing count |
| 12 | 6 | the facing-table pointer |

**Corrected, 2026-09-17 (second round).** The first version of this table read 1 / 2 / 5 / 5,
because the walk had two blind spots and both were in the code that produced the table:

- It cleared taint from any register that appeared as a first operand, whether or not the
  instruction wrote it. `test`, `cmp` and `push` all have a register first operand and write
  nothing. **Every cached-pointer reload in this engine is immediately null-checked** —
  `0x0049D8F7 mov ecx,[ecx+24h]` / `0x0049D8FA test ecx,ecx` / `0x0049D8FE mov cl,[ecx]` — so the
  `test` erased a pointer it never touched and the `0x24` source contributed **zero** reads. Part 2
  was a walk of the `0x1C` chains only. Now decided by `instr_info` operand access, not by a
  mnemonic list.
- Its `add` rule only carried the pointer when the *destination* was already tainted.
  `Imp::DirectionCount` forms its record the other way round — `0x0049D95D add eax,edx`, table in
  `edx` — so `0x0049D95F` and `0x0049D967` were invisible.

**The 2–10 conclusion is unchanged by both fixes**, which is the only reason the result stands; it
was checked, not assumed. Three regression tests pin the shapes, and the opt-in test pins all four
row counts, including that the `0x24` source contributes at least one in-module read — because "zero
reads from that source" is the signature of the bug, not a fact about the engine.

Two limits, and they are real limits. The walk is linear and stops at the first `call`, so a read
reached only through a call return is invisible. And a displacement cannot type a struct: offset
`0x1C` is the sequence table on the IMP header but the **current frame record** on the player object
(`0x0049CC80`), and `0x24` is used by many unrelated objects. That second limit is why Part 2b
exists.

**Withdrawn, 2026-09-17 (second round).** An earlier revision of this page attributed the missing
rows to the `call` boundary and cited `0x0049D9E6` and `0x0049D900`. That was wrong twice —
`0x0049D900` is `and cl,7`, not a read at all, and `0x0049D8FE` is reachable in a straight line from
`0x0049D8F7` with no `call` in between — and it was worse than merely wrong, because a stated
limitation that plausibly explains a defect stops anyone looking for the defect.

### Part 2b — typing the out-of-module hits

Following the pointer over all of `.text` finds hits at displacements 2, 4 and 8 outside the module.
Setting them aside as "offset collisions" is an assertion; this checks it. A `[x+0x1C]` load is only
an IMP header load if `x` is an IMP file image, and a file image is only ever obtained from an `Imp`
object's field 8 (`0x0049ADB7 mov esi,[eax+8]`). Following that field finds **25** distinct
`[x+0x1C]` loads, of which the 10 in-module ones are exactly those Part 2 already reports.

**Reads at displacements 2–10 whose source is one of those 25: zero** — in-module or not. That is a
stronger statement than the module filter alone, and it is the one the negative rests on.

This leg is a heuristic too: offset 8 is as common as `0x1C`, so the 15 out-of-module members of
that set are probably not IMP headers either. It does not need to be tight in that direction. It
needs to be *inclusive*, and being inclusive is what makes the zero mean something.

### The reporting scan, stated as measured

The survey also lists every base-relative read at displacements 0–15 anywhere in the module,
regardless of which struct it touches. This is reporting, not the negative — read it as measured:

- **Displacement 2: 1 byte-sized read and 27 wider ones.** The byte-sized one is `0x0049B25D`,
  inside the 1,024-byte palette copy loop at `0x0049B220` (which reaches the palette through header
  offset 8 at `0x0049B247`), not on any record. That loop was also the subject of a recorded
  channel-order contradiction — **resolved 2026-09-23 in favour of this disassembly**, see the
  [research
  log](research-log.md#2026-09-23--imp-palettes-are-bgr-after-all-the-capture-reader-swapped-red-and-green)
  — and `imp.rs` now matches it. The 27
  wider ones are 24 word reads with a plain base, 2 word reads with a base and index, and 1 dword
  read with a base and index — frame widths at frame `+2` and facing frame counts at facing `+2`,
  mostly.
- **Displacements 3–10: 0 byte-sized reads and 124 wider ones** — 20 word and 43 dword at
  displacement 4, 2 word and 56 dword at 8, 2 at 6, 1 at 10. Only the byte-sized rows are empty.
  They are reads of other structures: frame heights at frame `+4`, frame-table pointers at facing
  `+4`, pixel pointers at frame `+0xC`, and fields of unrelated objects.

**Corrected, 2026-09-17 (second round).** The displacement-2 bullet previously said "exactly one
read in the whole module", counting only the byte-sized row — the identical asymmetry corrected at
displacements 3–10 in the round before, repeated on the headline byte. Neither bullet excludes the
wider reads from being sequence-record reads; Parts 2 and 2b do that.

The scan excludes absolute and `esp`-based operands, and anything that does not actually read its
memory operand. That last test is asked of `instr_info`'s operand access rather than of a mnemonic
list, which is where two rounds of bugs came from: one predicate covers the store
(`mov [mem],reg` — memory access `Write`), the address computation (`lea` — `NoMemAccess`) and the
read-modify-write (`add [mem],reg` — `ReadWrite`, and it does read). **Indexed operands and `ebp`
bases are included**, which matters: `0x0049AC8F mov di,[ebx+esi*8+2]` is a real record read at
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

So the interval is **not** a guess that better sprite metadata would replace — that metadata does
not exist. It is a single global interval, a property of the engine's tick loop rather than of the
IMP container. **Answered 2026-09-19**: that loop's period is the field at object `+0x228AC`, and
`gs/modeinfo.gs` sets it per screen mode. See [timing: not in the asset, but in the
engine](#timing-not-in-the-asset-but-in-the-engine). Note that the `[0x005AF134]` in the
terrain-sprite driver above is the *counter* this scheme advances, not the period — an earlier
revision of this page confused the two.

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

## Pixel layout: what "no frame is row-padded" rests on, and where it stops

A frame's pixels are a bitstream, and at 1, 2 and 4 bits per pixel there are two legal ways to lay
one out. **Tight** runs the bits straight through and pads only the very end. **Row-padded**
restarts each scanline on a byte boundary. Nothing in the file says which a frame used. The decoder
in `src/imp.rs` picks one — `pixel_layout_for` states the rule — and a re-encoder that picked
differently would change a frame's stored length, which in a real file moves every payload after it.

Measured over the shipped `imp.mpq` by `--imp-roundtrip`, 1,800 members and 41,373 payload-carrying
frames:

| | frames |
| --- | ---: |
| the two layouts emit the same bytes, so the frame records no choice | 38,831 |
| the two layouts differ and the stored size names **tight** | 2,395 |
| 1bpp, final partial byte omitted (`tight_floor`) | 102 |
| the two layouts are the same **length** and different **bytes** | 45 |
| the two layouts differ and the stored size names **row-padded** | **0** |

Reproduce with:

```
cargo run --release --bin lom-asset-viewer -- --imp-roundtrip imp.mpq --listfile LISTFILE
```

That zero is what makes a single-frame replacement possible at all: a frame's stored size is
reproducible from its own pixels, so a replacement does not have to guess a layout. It is a
**bounded** negative, and the bound is worth stating precisely, because this repository's standing
lesson is that a negative is only as strong as the instrument that looked for it.

### Why 752 frames cannot be reached by this instrument

The instrument is the decoder, and the decoder cannot see a row-padded layout on one class of frame.

A payload's length is declared in the frame record's `encoded_size` field on `record_variant != 0`
records. **A `record_variant == 0` record declares nothing.** Its reader — `decode_rle_until_size` —
consumes RLE packets and tests the output length after each **complete packet**, stopping at the
first acceptable length that falls on a packet boundary.

That is *not* the same as "stops at the smallest acceptable length", and an earlier version of this
section said it was. The difference is reachable, not theoretical: a 17×4 frame at 1bpp has
acceptable sizes `[8, 9, 12]`, and a single 12-byte literal packet — control `0xf4` — lands on 12,
the row-padded size, without the reader ever seeing 8 or 9. Such a frame **is** observed as
row-padded. Counting it as unobservable let the sweep report `row-padded` and `unobservable` for one
frame at the same time, which cannot both be true.

So the unobservable class is narrower than "every compressed variant-0 frame": it is those where the
decoder actually stopped **short**, at a tight or tight-floor boundary, leaving a continuation it
could not have seen. The sweep reports that population directly:

```
layout-unobservable-by-this-decoder            752
layout-unobservable-and-not-cleared-by-gaps    752
```

752 of 41,373 is **1.8%**. Rounding it to "no frame is row-padded, full stop" would be claiming the
instrument reached where it does not.

**These are not the only frames this instrument cannot resolve, and the two populations must be
added rather than conflated.** A further **50** frames have two layouts of the same *length* and
different *bytes* — 7×5 at 1bpp is five bytes either way — and no rule over the file's bytes can
separate them; `pixel_layout_for` resolves them as tight by an arbitrary documented choice. The
sweep prints that as `layout-ambiguous-shape`. The table above says 45 for `ambiguous-same-length`
and the two differ for a reason worth keeping straight: ambiguity is a property of the shape, while
the table's row also requires the stored size to *be* that common length, so 5 frames of an
ambiguous shape that dropped a final partial byte are filed under `tight-floor` while remaining just
as undecidable. The undecidable population is the 50.

The two classes are disjoint by construction — unobservability needs `tight_ceil != row_padded` and
ambiguity needs them equal — so the frames where this instrument could have seen row padding and did
not is at most **41,373 − 752 − 50 = 40,571**, not 40,621. The difference is small and the
distinction is not: 752 frames are merely unseen, and the 50 are undecidable.

### Why the payload alignment cannot clear the last 752

There is a second, independent line of evidence, and it very nearly works. Every payload in the
archive starts on an 8-byte boundary — 41,373 of 41,373, counted directly as
`payload-starts-8-byte-aligned`, and visible again in the gaps between consecutive payloads, which
are 0 through 7 in roughly equal numbers. So if a variant-0 stream really stopped early, the bytes
it failed to consume would have to show up as an oversized gap before the next payload.

They would — but only if the shortfall were big enough to escape the alignment. A row-padded frame
read as tight leaves `row_padded - tight_ceil` packed bytes unread, which costs at least two stored
bytes per RLE repeat packet, so at least `2 * ceil(delta / 130)` stored bytes (130 is the longest
run one IMP repeat packet expresses). Where that minimum is **8 or more**, a gap of 8 or more would
have to exist somewhere, and the measured gap histogram tops out at 7. Where it is **under 8**, the
shortfall fits inside the padding the archive already leaves and the histogram says nothing at all.

For all 752 frames the minimum is under 8. The gap evidence clears none of them, which is why the
sweep prints the two counters and why they are equal.

That 41,373 is a count of **payload starts**, and the 41,373 above it is a count of **frames**. They
are not the same population by definition and the sweep no longer leaves the reader to assume they
coincide: two ordinary frame records may point at one payload, which would cost a start without
costing a frame. So both are printed, and the difference with them:

```
payload-offset-records          41373
payload-starts                  41373
payload-shared-by-two-records       0
```

No payload in `imp.mpq` is addressed by two ordinary records, so the two populations do coincide
here — measured, not assumed. (Duplicate and shared-pixel records cannot enter either count: the
parser leaves their `pixels_offset` unset, because a `0x08` record's pixel dword is a frame index
and a `0x04` record's payload belongs to the frame it aliases.)

Nothing in `imp.mpq` comes near the 391-byte shortfall that would escape the alignment, so the
corpus can only ever confirm one side of that boundary. The arithmetic — the gating condition, the
`2 * ceil(delta / 130)` cost, and the `< 8` comparison — is therefore unit-tested against synthetic
`PackedSizes` in `main.rs`, driven past 390 and 391 from both sides, with the `130` and the `8`
mutated in both directions. A fixture shaped like the corpus could not fail on what the corpus
hides.

### What the editor is told

`--import-png-imp` prints `layout=...` for the frame it wrote, and that line is a statement about
what this decoder **read**, not necessarily about what the file stores. Both unobservable classes
get a warning beside it, in the same shape and with no refusal: the 50 whose stored size names no
layout, and the 752 whose stored size names one this reader stopped too early to see. Warning about
only the smaller class would leave the common case printing `layout: Tight` as fact — and if such a
frame really is stored row-padded, the exported pixels were already scrambled and the import splices
a tight payload over only the prefix the reader consumed, stranding the original tail after the
padding. The sweep counts both over the archive as `rewrite-ambiguous-layout` (46) and
`rewrite-unobservable-layout` (746), over the 37,930 frames the writer accepts rather than all
41,373 — it refuses 3,439 shared payloads and 4 zero-length ones before it classifies anything.

`rewrite-ambiguous-layout` being 46 where the table above says 45 is not a discrepancy, and the
sweep now prints the number that explains it: `layout-ambiguous-shape` is **50**. Ambiguity is a
property of the *shape* — width, height, depth — while the `ambiguous-same-length` row of the table
also requires the stored size to be that common length. The other 5 are 1bpp frames of an ambiguous
shape that dropped their final partial byte, so they classify as `tight-floor` while remaining
exactly as undecidable. The 45 and the 50 are different questions, and both are printed.

### What probe would close it

Not built, and deliberately so — it needs the engine, not more reading of the archive.

Pick one of the 752 frames at a shape where the two layouts differ, replace its palette with
entries that make every index visually distinct, run the game to a screen that draws that frame, and
capture it. A tight read and a row-padded read of the same bytes produce **different pictures**, not
different byte counts, so the capture decides it outright: one of the two predicted images matches
and the other does not. The same run answers the 50 equal-length ambiguous frames, which no amount
of arithmetic over the file can touch, because there the two layouts are indistinguishable in every
measurable property except the pixels they draw.

Until then: 0 of 41,373 frames are observed row-padded, 40,571 of them by an instrument that could
have seen it, 752 by one that could not, and 50 by one where the file settles nothing.

## Issue #2's acceptance criterion, restated

The issue asks the viewer to derive playback timing from verified metadata, and that conflates two
different questions. Separating them is the answer:

**The asset says which frame comes next. The engine's screen mode says how long to wait.**

- **The `.imp` carries no timing at all.** No field of one is read by `lomse.exe` as a duration,
  delay, rate or tick count — the bounded negative above, **Observed in a local binary**. So no
  amount of further work on the asset can produce an interval, and the half of the criterion that
  expected one from the file is unsatisfiable as written.
- **`AnimRules` determines frame order, end-of-cycle behaviour, reflection, and which stored facing
  each direction draws** — and only those.
- **The interval is nevertheless sourceable**, from the engine and from `gs.mpq`, not from the
  sprite. See the next section.

**Restated:** the viewer plays an `.imp` using the animation rules recovered from `lomse.exe` for
frame order, reflection and mirrored facings, and takes its frame interval from the engine's
per-screen-mode tick time rather than from the asset or from a constant of its own.

## Timing: not in the asset, but in the engine

**Corrected and superseded, 2026-09-19.** An earlier revision of this page said the interval was
"none in the file" and stopped there, and the viewer shipped a 100 ms constant labelled unsourced.
The first clause is still true and still matters. The stopping was premature: the period is in the
engine, and it is a number.

### The period field

| Claim | Evidence class | Proof |
| --- | --- | --- |
| The tick method at `0x00482230` reads the wall clock from `KERNEL32!GetTickCount` | Observed in a local binary | it loads `[0x0054D0E8]` into `ebx` and `call ebx`; parsing the import directory resolves that IAT slot to `KERNEL32.dll!GetTickCount` |
| It divides the elapsed time by a field at object `+0x228AC` to get how many ticks to catch up | Observed in a local binary | `cdq` / `idiv dword [esi+0x228AC]` at `0x004822FF` |
| That field's constructor default is **100** | Observed in a local binary | `mov dword [ebp+0x228AC],0x64` — bytes `c7 85 ac 28 02 00 64 00 00 00` — at `0x0047F7EA`, the only occurrence of that encoding in the image |
| The object base is `0x5AA12C`, so the field is `0x005CC9D8` | Observed in a local binary | from the parallel binary-analysis pass; `0x5AA12C + 0x228AC = 0x5CC9D8` is arithmetic, the base is theirs and was not re-derived here |

So the viewer's old "arbitrary" 100 ms was in fact the engine's **pre-script default** — which is a
coincidence worth naming rather than a vindication, because that default is overwritten before any
gameplay happens.

### Refuted, 2026-09-19: `0x005AF134` is not the period

This page previously filed the tick period as unknown and pointed at `0x005AF134`, guessing its
writer was reached through `0x005AF130 + 4`. Both halves are wrong.

- `0x005AF134` is a **tick counter**, not a period. The GameScript operator `combattime`
  (`0x00464F60`) is `mov ecx,[0x005AF134]` — **Observed in a local binary**, the bytes at that
  address are `8b 0d 34 f1 5a 00`. It increments by one per tick at `0x00482796` and is reset at
  level and combat start (`0x0045B5FA`, `0x0048191D`). The five animation sites use it as
  `(phase[obj+0x28] + [0x5AF134]) mod CycleLength`: it is the modulo *dividend*, an advancing
  counter.
- The base is `0x5AA12C + 0x5008`, not `0x5AF130 + 4`. `0x5AF130` is independently known as
  `currentturn`, which cross-checks that object layout.

### What the screen modes set it to, Observed in the corpus

`gs/modeinfo.gs` in `gs.mpq` assigns the tick time per screen mode at load. Read out of the shipped
archive on 2026-09-19, and **identical in all three distinct `gs.mpq` files on this machine** —
stock (Steam build and `Lords of Magic Development`, byte-identical), 3.02, and GS5R3:

```
SCROLLINGMAP_SCREEN 66 setmodeticktime
COMBAT_SCREEN 121 setmodeticktime
LOCATION_SCREEN 121 setmodeticktime
REGION_SCREEN 66 setmodeticktime
WORLD_SCREEN 66 setmodeticktime
INTRO_SCREEN 66 setmodeticktime
SETUP_SCREEN 66 setmodeticktime
```

It is also **user-adjustable**, and the range is the game's own, not a viewer invention:
`gs/hotkey.gs` steps the map and combat tick times by 11 ms and clamps them with
`11 sub 11 max` / `11 add 330 min`; `gs/hotkey.gs` also restores 66 and 121 outright;
`gs/Dlg/opdlg.gs:547,579` drives the same field from the options dialog. All **Observed in the
corpus**.

**The clamp is spelled differently in GS5R3, and the spelling is not the semantics.** Measured
2026-09-19 across all three distinct archives, four occurrences of each form:

| profile | `gs.mpq` members | faster key | slower key | `gs/standard.gs` defines |
| --- | --- | --- | --- | --- |
| stock (Steam build, `Lords of Magic Development`) | 1,688 | `11 sub 11 max` | `11 add 330 min` | `/min` with `gt`, `/max` with `lt` |
| 3.02 | 1,691 | `11 sub 11 max` | `11 add 330 min` | `/min` with `gt`, `/max` with `lt` |
| GS5R3 | 1,700 | `11 sub 11 min` | `11 add 330 max` | `/min` with `lt`, `/max` with `gt` |

GS5R3 swaps the two call sites **and** reverses the two definitions, under ManTerA's own comment
`WILL WORK TO REVERSE THE TWO ABOVE BY USING THE TWO BELOW`. The two changes cancel: in GS5R3 the
token `min` computes a maximum, so `11 sub 11 min` is the same floor of 11 that `11 sub 11 max` is
elsewhere, and `11 add 330 max` is the same ceiling of 330.

**The effective clamp is therefore identical in all four installs: step 11, floor 11, ceiling 330**,
which is what the viewer implements. An earlier revision of this section read the swapped tokens at
face value and claimed GS5R3's speed keys "move to an extreme instead of stepping". That was an
inference from the spelling presented as an observation, and it is **Refuted** — by
`gs/standard.gs`, which defines what the spelling means.

In GameScript both are `/NAME{2 copy CMP{exch pop}{pop}ifelse}bind def`. With `a b` on the stack,
`2 copy` gives `a b a b`, `CMP` pops two and leaves a boolean, the true branch `{exch pop}` leaves
`b` and the false branch `{pop}` leaves `a`. So the body returns `b` when the comparison holds:
`gt` yields the smaller operand and `lt` the larger one. Read the comparison, never the name.

### The honest limit

**Inferred, not observed:** which of the five animation sites belongs to which screen mode was not
traced. The viewer starts sprite playback at **66 ms** because the terrain-sprite driver at
`0x50C334` is a map-screen site — that is reasoning from where the driver sits, not a measurement
of which period it runs under. 121 ms is one keypress away in the viewer for exactly that reason,
and this should not be promoted to an observation without tracing the sites.

## The viewer plays by these rules

`src/imp_playback.rs` joins the decoded sprite to the recovered rules, and `--view-imp` consumes
it. The playhead is the engine's own state — an action, a direction, and a position in the *cycle*
— rather than the global frame index the viewer used to walk.

Because the rules come from the binary, `--view-imp` now needs one. It takes `--exe PATH`, and
falls back to a `lomse.exe` sitting beside the archive, which is where an installed archive is.
With neither it **refuses to open** rather than inventing an order. Whatever it is given goes
through `recover`, which refuses a binary that does not hold the decoder this page read.

Keys: arrows scrub the cycle (left/right) and turn through the directions (up/down), page up/down
change the action, space plays, `C` cycles the display mode, `T` switches between the two screen
modes' tick times, and `-` / `=` step the interval by 11 ms. The title bar names the cycle mode,
the direction and whether it is mirrored, the position in the cycle, and the interval with where
that interval came from.

### What the fold changed, Observed in the corpus

Measured 2026-09-19 against `imp.mpq` (1,800 `.imp` members, 4,667 sequence records) with the rules
recovered from `lomse.exe` 3.02. Each row is the previous walk — "advance one index inside the
current facing, wrap at its end" — compared against the fold.

| | Sequences | What was wrong before |
| --- | ---: | --- |
| Ping-pong, mode 4 | **955** | played `0,1,…,N−1` and jumped back to 0 instead of reflecting |
| One-shot, mode 1 | **5** | looped instead of holding the last frame |
| Five facings, eight directions | **2,234** | directions 5, 6 and 7 were unreachable |
| Any synthesised direction | **2,304** | as above, at facing counts 3, 5, 7, 9, 13 and 33 |
| Synthesised directions, total | **7,810** | drawn by mirroring an earlier facing |

Two of those numbers need their arithmetic stated, because both look wrong at a glance:

- **2,304, not 2,388.** 2,388 sequences have the mirror bit set *and* two or more facings. The 84
  with exactly two advertise `2 × 2 − 2 = 2` directions, and both are below the facing count, so
  the fold is never reached and nothing is synthesised. Mirroring is set on them and inert.
- **7,810** is `2,234 × 3 + 28 × 31 + 15 × 5 + 13 × 7 + 8 × 1 + 6 × 11`, i.e. the per-facing-count
  rows of the direction table above. The corpus total and the table agree exactly; neither was
  fitted to the other.

**941 sequences, not 960, actually change what the viewer shows.** 955 ping-pong plus 5 one-shot is
960, and 19 of those have a one-frame facing: a one-frame cycle reflects onto itself and holds the
frame it was already showing, so the rule applies and changes nothing visible. The corpus test
asserts 941 and 19 separately and asserts that they sum to 955 + 5, which is what would catch a
sequence outside those two modes starting to differ.

### What is deliberately the viewer's own behaviour, not the engine's

Three things, each of which would be a lie if filed as engine behaviour:

- **Starting at the map screens' 66 ms.** The number itself is the engine's
  (`gs/modeinfo.gs`, **Observed in the corpus**); pairing *sprite playback* with that screen mode
  rather than the combat one is **Inferred**, as above. `T` switches to 121 ms, and `-` / `=` step
  by 11 ms within the game's own 11..=330 clamp.
- **Stopping on a one-shot.** The engine reports completion and its *caller* switches action. The
  viewer has no caller to switch to, so it stops playback on the held frame rather than pretending
  to be a game loop.
- **Skipping frames that decode to nothing.** The corpus has facing records whose frames carry no
  pixels. The viewer steps past them; it cannot change which frames the engine would show, only
  how long the viewer dwells on nothing.

### The remaining piece of the roadmap's box

Wiring the fold and restating the criterion are done. **Anchoring direction 0 to a compass bearing
is the only part left**, and it is item 2 of [what is still open](#what-is-still-open). It cannot
be done from the file or the binary: two rotations sit between a GameScript-level facing and a
stored index, and closing it needs a gameplay observation or a call site whose bearing is known
independently. Nothing here should be read as having narrowed it.

Two pointers for whoever picks it up, from a parallel pass and **not** verified here:
`locationindirection` is **Refuted** as the anchor; the better target is whatever builds the
runtime neighbour table behind the pointer at `0x5AE970`.

## The animation ACTION enum, and the remap between action and sequence slot

**Observed in a local binary, 2026-09-21.** The engine registers its script-visible constants as
`{char* name; int32 value}` 8-byte records. One contiguous run gives the animation actions:

| value | name | value | name | value | name |
| ---: | --- | ---: | --- | ---: | --- |
| 0 | `MOVE` | 5 | `SUBDUE_ATTACK` | 10 | `MAJOR_SPELL` |
| 1 | `RIDE` | 6 | `DEFEND` | 11 | `MINOR_SPELL` |
| 2 | `STAND` | 7 | `GET_HIT` | 12 | `INVISIBLE` |
| 3 | `MELEE_ATTACK` | 8 | `DIE` | 13 | `RALLY` |
| 4 | `RANGED_ATTACK` | 9 | `CORPSE` | 14 | `BERSERK` |

The run begins at VA **`0x0055f150`** and continues past the animation actions into the sound
actions — `SELECT` 23, `MOVE_ACKNOWLEDGE` 24, `ATTACK_ACKNOWLEDGE` 25 — which is corroborated
independently of the sprite subsystem by `setunittypesound` (`0x00525430`), which uses the raw
action integer as a direct index and bound-checks it with `cmp eax,0x1A` (26).

⚠️ An earlier write-up of this finding placed the run at `0x0055d350`. That address holds debug
strings; it was a transcription error and is recorded here so the wrong address does not get
re-derived.

### The lookup

`Imp::GetSequence` (`0x0049ADB0`), which resolves an action to a sequence:

```
0049adbe  mov edx,[ecx+4]      ; optional int32[] REMAP table
0049adc7  je  0049ADD5h        ; remap == NULL -> use the action as the index
0049adcd  cmp eax,[ecx+8]      ; else bound against remap_count
0049add2  mov eax,[edx+eax*4]  ; index = remap[action]
0049addb  mov cx,[esi+1Ah]     ; sequence count, from the .imp header at +0x1A
0049ade1  jge FAIL             ; index >= count -> return NULL
0049adeb  shl eax,4 ; add eax,ecx
```

**Out of range returns NULL.** It does not clamp to slot 0 and does not read out of bounds;
`Imp::SetAction` (`0x0049DA84`) caches the NULL and every consumer null-checks it. So a missing
action draws **nothing**, never the wrong sprite.

### The remap must be populated for shipped units

**Inferred, but forced by arithmetic.** `units\imp\orinfa.imp` has **7** sequences, labelled
MOVE..CORPSE at slots 0-6 by its generated `.h`. Under direct indexing `DIE` = 8 and `CORPSE` = 9
both exceed 7 and would return NULL, so Footmen would have no death animation — and they visibly
die. An independent second check: `BERSERK` = 14 would need 15 sequences, the corpus maximum is 11,
and 46 units carry `CAN_BERSERK`.

So the remap array **is** populated for shipped units, the `.h` slot order is the physical order,
and the `.h` names are the labels the remap resolves to. That also explains the 51 distinct slot
orderings measured across the corpus, and why a unit's A and B files agree on order in 128 of 139
cases.

### 🔴 The remap is parsed from the `.H` companion member at load time

**Answered 2026-09-21. The `.H` files in `imp.mpq` are a LIVE ENGINE INPUT, not build residue.**

This document, [game architecture](game-architecture.md), [the native asset stage](native-asset-stage.md)
and [the engine plan](native-engine-plan.md) all describe them as "generated C headers" and use them
only to cross-check frame statistics. **The engine reads them itself, every time it loads a sprite.**

**Observed in a local binary.** `Imp::BuildActionRemap` (`0x0049AE00`), called from `Imp::Load`
(`0x0049B450`) at `0x0049B5B2`:

1. Allocates `enum->count * 4` bytes -> wrapper `+4`, count -> `+8` (`0x0049AE68`-`0x0049AE79`).
2. Fills every slot with **0 if `enum->flags & 1`, else -1** (`0x0049AED4`-`0x0049AEE2`).
3. Copies the imp name, truncates at the last `'.'` (`strrchr`, `0x0049AF25`), appends the literal
   at `0x0055C728` = **`".H"`**, and opens that member through the same archive-aware stream opener
   the `.imp` itself used (`0x004FE720`).
4. Reads it line by line with a reader accepting a bare `\r` **or** a bare `\n`
   (`0x0049AFF9`-`0x0049B036`) -- the [bare-CR convention](gamescript-format.md) again.
5. Per line: `strncmp(line, "#define", 7)` (literal `0x0055C730`); `strchr(line, '_')`;
   `sscanf(p+1, "%s %d", name, &val)` (literal `0x0055C73C`), requiring **exactly 2** conversions.
6. `idx = EnumDesc::FindIndex(name)` (`0x0049DD80`), a linear `strcmp` over a `char**`.
7. `remap[idx] = val` (`0x0049B0CF`).

Its error strings name the mechanism outright: `0x0055C744` `"%s: failed to load header %s\r\n"`,
`0x0055C764` `"%s: sequence_table_size<1\r\n"`.

**The action-name array is at `0x00574008`** -- a NULL-terminated `char**`, 22 entries, index ==
ACTION value. It extends past the 15 animation actions into the gate states:

```
15 CLOSED_100  16 CLOSED_75  17 CLOSED_50  18 CLOSED_25  19 DESTROYED  20 OPENING  21 OPEN
```

⚠️ This is **not** the `0x0055F150` `{name, value}` table above -- that is the *GameScript* enum, a
different consumer. Note slot 14 is spelled **`BERSERK_ATTACK`** here, and **appears zero times in
the entire `.h` corpus**.

### Worked example, verified end to end

**Observed in the corpus.** `imp.mpq` member `units\imp\orinfa.h` is real, bare-CR, and holds:

```
#define	ORINFA_MOVE	0          #define	ORINFA_GET_HIT	4
#define	ORINFA_STAND	1          #define	ORINFA_DIE	5
#define	ORINFA_MELEE_ATTACK	2      #define	ORINFA_CORPSE	6
#define	ORINFA_DEFEND	3          // Total number of 'Sequences':	7
```

The `#define __ORINFA_H__` include guard also passes the `"#define"` test, but `sscanf` returns 1
rather than 2 and it is discarded (`0x0049B0A6 cmp eax,2`).

So **`remap[8] = 5` (DIE) and `remap[9] = 6` (CORPSE)** -- which is exactly the arithmetic the
forced-remap argument above required, now read out of the shipped data rather than inferred.

### The fallback table, `0x00521CD0`

The `.H` alone does not explain `BERSERK`. This does. Two accessors sit beside `GetSequence`:
`Imp::HasAction` (`0x0049B110`) and `Imp::AliasAction` (`0x0049B150`, `remap[dst] = remap[src]`).
`0x00521CD0` applies, in order:

```
!Has(MAJOR_SPELL)   -> MAJOR_SPELL   = MINOR_SPELL      !Has(DIE)           -> DIE    = DESTROYED
!Has(MINOR_SPELL)   -> MINOR_SPELL   = MAJOR_SPELL      !Has(CORPSE)        -> CORPSE = DESTROYED
!Has(MOVE)          -> MOVE          = RIDE             !Has(BERSERK)       -> BERSERK       = MELEE_ATTACK
!Has(RIDE)          -> RIDE          = MOVE             !Has(RANGED_ATTACK) -> RANGED_ATTACK = MELEE_ATTACK
!Has(STAND)         -> STAND         = MOVE             !Has(SUBDUE_ATTACK) -> SUBDUE_ATTACK = MELEE_ATTACK
!Has(STAND)         -> STAND         = CLOSED_100       for i in 0..21: !Has(i) -> i = STAND
```

So an orc footman's `BERSERK` plays the melee sequence, and everything else unnamed plays `STAND`.
That is how 46 units carry `CAN_BERSERK` with no BERSERK cycle.

⚠️ **A real limit, stated rather than papered over.** `0x00521CD0` runs only when the table was
filled with `-1`, i.e. `flags = 0`. Ten call sites in `0x005229xx`-`0x0052313x` use the 2-arg
`EnumDesc` ctor with explicit `flags = 0` and *do* call it. A **second** family (`0x00421A45`,
`0x0049B807`, `0x004B0CA1`, ...) uses the 1-arg ctor `0x0049DD20`, which sets `flags = 1` -> fill
**0** -> `HasAction` is vacuously true, the fallbacks are a no-op, and every unnamed action resolves
to sequence 0. **Which family loads `units\imp\orinfa.imp` in a live game is Inferred, not
observed.** The `flags=0` family is the strong candidate for combat units.

### What this means for modding

🔴 **Adding a sequence to an `.imp` is inert unless the matching
`#define <PREFIX>_<ACTION> <n>` is added to the `.H` and the `.H` is repacked into `imp.mpq`.**
A mod pipeline that ships the binary and drops the header silently loses every animation.

- The recognised token set is exactly `0x00574008`. The berserk spelling is `BERSERK_ATTACK`.
- `strchr(line, '_')` takes the **first** underscore, so an imp basename containing `_` would
  mis-parse every line. No shipped file does this.
- Sequence indices are bounds-checked against header `+0x1A`, so an out-of-range `#define` yields a
  NULL sequence rather than a crash.
- Cloning a donor `.imp` **and its `.H` together** inherits a working remap, so the
  [new-unit path](new-units.md) is unaffected.

**Refuted: IMP header bytes 12-25 are not the remap.** Nothing in the IMP module reads them, and
they could not hold a 22-entry `int32` table in 14 bytes. **Observed in the corpus:** across all
1,800 members, bytes `0x10`-`0x19` are zero and `0x0C`-`0x0F` hold uninitialised leftovers --
`'Buil'` in 100 files, `'Read'` in 25, stack addresses like `0x0012f6cc` in 34. `orinfa.imp` holds
the ASCII `'Buil'`. Same class of scratch already documented for sequence bytes 5-10.

## What is still open

1. **Which screen mode each of the five animation sites runs under.** The period itself is
   answered — it is the field at object `+0x228AC`, defaulting to 100 and set to 66 or 121 by
   `gs/modeinfo.gs` (see [timing](#timing-not-in-the-asset-but-in-the-engine)). What is *not*
   traced is which site takes which mode's value, so "sprite playback runs at 66 ms" is
   **Inferred** from the terrain-sprite driver being a map-screen site. The earlier version of
   this item, which called the period unknown and pointed at `0x005AF134`, is **Refuted** and
   recorded as such rather than deleted.
2. **The compass bearing of direction 0.** Two rotations sit between a script-level facing and a
   stored index: a global bias at `0x005AEC3C` and a `+1` at `0x0049DCDD`. Neither zero point is
   anchored to a bearing by anything read here. This is the **only** remaining part of the
   roadmap's issue-#2 box; the other two, wiring the fold into the viewer and restating the
   acceptance criterion, are done.
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
8. **Whether any frame is stored row-padded.** 0 of 41,373 are observed to be, but 752 of them are
   `record_variant == 0` frames whose reader could not have seen it, and 50 more have a shape where
   the two layouts are indistinguishable in the file. See
   [pixel layout](#pixel-layout-what-no-frame-is-row-padded-rests-on-and-where-it-stops) for the
   probe that would close it.

## Test coverage

`src/imp_anim.rs` and `src/imp_playback.rs`. The default `cargo test` run covers the recovery
machinery against synthetic PEs and the fold against skeleton sprites; the tests that need the
installed game are `#[ignore]`d, so the summary reports them as `ignored` rather than as passes.

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
- `a_null_check_between_a_load_and_a_dereference_does_not_erase_the_pointer`,
  `an_index_added_to_a_table_carries_the_pointer_either_way_round`,
  `stores_and_address_computations_are_not_reported_as_reads` and
  `a_table_base_read_before_any_index_is_not_reported` — the four shapes the pointer walk has to
  get right. The first two are regressions: each one silently emptied part of the timing negative,
  and reintroducing either now fails here as well as in the opt-in test.
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
`cargo test --release -- --ignored`. The IMP sweeps below were measured on the **GS5R3** profile and
their pinned counts are that archive's; `the_viewer_ticks_at_the_rates_the_shipped_scripts_set` is
separate, takes `LOM_GS_MPQ` rather than `LOM_GAME_DIR`, and is **profile-independent — it passes on
all four installs**, keying the part that genuinely differs between them rather than pinning one:

- `the_recovered_rules_match_the_installed_executable` — `recover` refuses on any disagreement with
  this module, so reaching the end is the check. It also asserts Part 1 (every `GetSequence` call
  site is in-module **and** its address appears nowhere as a literal dword), Part 2 (no in-module
  read at displacements 2–10 through a record pointer, plus all four measured row counts and the
  fact that the cached-pointer source contributes at least one read) and Part 2b (no read at an
  unexplained displacement is reached from a confirmed IMP header load).
- `the_corpus_matches_the_recovered_rules` — every mode the archive uses has a dispatch slot, every
  advertised direction resolves, every facing metadata word is zero, and the measured counts hold:
  4,667 sequences, **14,921 facing records**, 955 ping-pong, 3,379 with the mirror bit set, 2,931
  whose byte 1 is exactly the bit. Those counts are what make `PING_PONG_MODE` falsifiable against
  the shipped archive — set it to 3 and the ping-pong count goes to zero. The facing-record total is
  pinned because the "all 14,921 are zero" claim is otherwise satisfied vacuously by a corpus that
  has shrunk.

### The fold, in `src/imp_playback.rs`

Default, on skeleton sprites whose only job is to let a test hand the fold a value the binary does
not hold. They are **not** corpus stand-ins and nothing about the archive is asserted against them:

- `the_reflection_follows_the_mode_the_rules_name` and
  `a_held_ending_stops_where_a_wrapping_one_restarts` — point the rules at a different ping-pong
  mode, or a different `CycleEnd`, and the played order must change accordingly. A fold reading
  `imp_anim::PING_PONG_MODE` passes half of the first and fails the other half, which is the
  point.
- `the_mirror_fold_follows_the_bit_the_rules_name` — likewise for the mirror bit: a bit the record
  does not set must take the direction count back to the stored facing count.
- `a_mode_outside_the_dispatch_is_refused` — a mode past the dispatch bound is refused by name,
  not given an invented ending.
- `playback_skips_blank_frames_and_keeps_the_engine_order`,
  `a_held_cycle_reports_completion_and_stays_put` and `a_blank_direction_is_stepped_over` — the
  viewer-side skipping must not flatten the reflection or spin on a held cycle.

Opt-in, against the shipped archive:

- `every_shipped_sequence_resolves_at_every_direction_and_position` — 4,667 sequences and 97,232
  frame resolutions, every one inside the decoded frame table, and **zero refusals**. The test used
  to allow a refusal wherever the sequence had a facing with no frames; a byte-level walk of all
  1,800 members that does not share this decoder (`tools/imp_structure_scan.py`) **refutes** the
  premise — **observed in the corpus**
  2026-09-19, none of the 14,921 facing records has `frame_count == 0`. (The same walk counts 6,552
  zero-*dimension* frames across 107 files, which is the separate fact behind the viewer's
  blank-skipping and is not the same thing.) The escape hatch was therefore dead code that also
  weakened the test, since a refusal from any other cause would have been diverted into an assertion
  about facings. `resolve` is now asserted to refuse nothing at all.
- `the_fold_changes_these_many_shipped_sequences` — the counts in the table above, plus the 941/19
  split and the structural assertion that they sum to 955 + 5.
- `single_facing_mirrored_sequences_advertise_nothing_and_still_play` — the 991 inert records.

### Why the constants are parameters

`PING_PONG_MODE` and `SEQUENCE_MIRROR_BIT` are not read by the rules that use them. `cycle_length`,
`frame_for_cycle_index`, `mirrors_facings`, `direction_count` and `facing_for_direction` all take
the value `recover` read out of the binary; the constants exist only as the value `recover` refuses
to disagree with.

The reason is a review finding worth recording. With `PING_PONG_MODE` mutated from 4 to 3, an
earlier version of this suite stayed **green** — every ping-pong sequence in the archive would have
truncated to forward-only and the survey would have labelled a mode with no users as the ping-pong.
The traversal test compared `cycle_length` against `frame_for_cycle_index`, and both read the same
constant, so it was self-consistent for any value. That is this repository's own recorded lesson:
agreement is not confirmation.

**Corrected, 2026-09-17 (second round).** The previous revision of this paragraph claimed both
constants had been parameterised. Only `PING_PONG_MODE` had. `mirrors_facings` still read
`SEQUENCE_MIRROR_BIT` directly, so `direction_count` and `facing_for_direction` did too, and
`AnimRules::mirror_bit` was consumed by nothing — it was printed and asserted and never used. The
hole `ping_pong_mode` was threaded through `AnimRules` to close was still open on the mirror axis,
and open exactly for the caller who wires this into `src/main.rs` without calling `recover` first,
which is the next step on issue #2. Both are parameters now.
