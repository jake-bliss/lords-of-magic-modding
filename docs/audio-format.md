# Audio and video formats

Sound effects, speech and music are **RIFF WAVE**. Cutscenes are **Smacker**. This page records
what has been measured about both, what the tool can now read and write, and — in its own section
at the end — what is still not determined.

Evidence classes are used throughout, as everywhere in this repository: **Observed in the corpus**,
**Observed in a local binary**, **Documented**, **Inferred**, **Refuted**. An unlabelled assertion
here is a defect.

**No second party has verified any corpus number on this page.** Two independent reviews were run
over the branch; neither reviewer had an installed game, so every count below rests on one machine's
measurement. They are reproducible with the commands given and should be re-run rather than
inherited.

Corpus, last measured **2026-09-18** on the installed profiles:

| Source | Files | Note |
| --- | --- | --- |
| `English/sndfx.mpq` | 1,880 WAVE members | every member is a WAVE; nothing else is in the archive |
| `English/special.mpq` | 1,218 WAVE members | every member is a WAVE |
| `English/Wav/` (loose) | 42 `.wav` files | music, plus the `lou_scenarios` narration |
| `English/smk/` (loose) | 23 `.smk` files | all cutscenes; no `.smk` appears inside any archive |

**Observed in the corpus:** `sndfx.mpq` and `special.mpq` are byte-for-byte identical across all
four installed profiles (baseline, 3.02, GS5R3, Development), as are the `smk/` and `Wav/` trees.
Neither community mod touches audio or video. Measurements below therefore describe one corpus, not
four.

## WAVE

### Encoding histogram

Measured with `--wave-roundtrip`. **Observed in the corpus:**

| Format tag | Channels | Sample rate | Bits | `sndfx.mpq` | `special.mpq` | loose `Wav/` |
| --- | --- | --- | --- | --- | --- | --- |
| 1 (PCM) | 1 | 11025 | 8 | 53 | 53 | 0 |
| 1 (PCM) | 1 | 22050 | 8 | 1,790 | 1,124 | 0 |
| 1 (PCM) | 2 | 22050 | 8 | 36 | 40 | 42 |
| 1 (PCM) | 1 | 44100 | 16 | 1 | 1 | 0 |
| | | | **total** | **1,880** | **1,218** | **42** |

There is **no ADPCM, no µ-law, no compressed WAVE of any kind** in this game. Every one of the 3,140
files is linear PCM. That was measured, not assumed: the decoder refuses any format tag other than
1, and no member reached that refusal.

The single 44.1 kHz 16-bit member is the same audio in both archives
(`sndfx.mpq!File00000099.wav` = `special.mpq!File00001105.wav`, 124,420 bytes). It is the only file
in the game at either 44.1 kHz or 16-bit, so the decoder's 16-bit path rests on exactly one
witness — see *what is not determined*.

### The two archives overlap

**Observed in the corpus:** the 3,098 archived members contain only **1,802 distinct files** by
SHA-256, and **1,142** of those appear in both `sndfx.mpq` and `special.mpq`. The archives are not
disjoint sets of sounds; `special.mpq` re-ships a large part of `sndfx.mpq`. Which of the two the
engine reads a given sound from has **not** been established — but it matters for modding, because
editing one archive may leave the other's copy in place.

### Container layout

Every member is a standard RIFF: `RIFF`/`WAVE`, a 16-byte PCM `fmt ` chunk, a `data` chunk, and in
most files one or more trailing metadata chunks. **Observed in the corpus**, chunk sequences and
their counts:

| Layout | `sndfx.mpq` | `special.mpq` | loose `Wav/` |
| --- | --- | --- | --- |
| `fmt \|data` | 1,003 | 407 | 18 |
| `fmt \|data\|LIST` | 781 | 715 | 1 |
| `fmt \|data\|cue \|LIST` | 3 | 5 | 22 |
| `fmt \|data\|LIST\|cue \|LIST` | 35 | 33 | 0 |
| `fmt \|data\|smpl` | 1 | 1 | 0 |
| `fmt \|data\|smpl\|LIST` | 51 | 51 | 1 |
| `fmt \|data\|smpl\|LIST\|cue \|LIST` | 6 | 6 | 0 |

No member has a `fmt ` chunk longer than 16 bytes, and no member's `RIFF` size field disagrees with
its file length (`riff_size_mismatch 0` on all three sweeps).

**Observed in the corpus, and the two numbers are different questions.** **215** files (96 + 96 +
23) *carry* a `smpl` or `cue ` chunk — that is the sum of the `smpl`/`cue `-bearing rows above.
Only **147** (63 + 62 + 22) declare at least one **record** inside it: 116 `smpl` chunks between the
two archives declare just 41 loops in total, so most of them are empty. The length-change gate
below acts on records, so 147 is its population. Both counts are pinned by a corpus test.

The `LIST INFO` blocks name the authoring tool. **Observed in the corpus:** every one of the 1,678
`ISFT` fields across both archives reads `Sound Forge 4.0`, and the `ICRD` dates run 1996 (16),
1997 (1,232) and 1998 (430). One tool cut the entire corpus.

### What the `smpl` and `cue ` positions mean

**Documented, and the only claim in this branch grounded in an external authority.** The Microsoft
RIFF 1994 specification says of a `smpl` loop's `dwEnd` that "this sample will also be played" — the
endpoint is **inclusive**, so a loop ending at `dwEnd` needs `dwEnd + 1` frames of audio. A `cue `
point's `dwSampleOffset` is a position, so an offset equal to the frame count is already past the
end; there is no ambiguity of the same kind there.

Writers are known to disagree about `dwEnd`, so the corpus was asked as well, and it
**discriminates between the two readings**. Under the exclusive reading a loop running to the end
of a file is written `dwEnd == frames`; under the inclusive reading it is written
`dwEnd == frames - 1`.

**Observed in the corpus:** of the 41 `smpl` loop records in the two archives, **34 end at exactly
`frames - 1` and not one at `frames`**; of the 116 `cue ` points, 26 sit at `frames - 1` and none at
`frames`. The remaining seven loops are interior — 238, 5,584, 42,415 and 76,437 frames short of the
end — so being nowhere near the boundary they settle nothing either way, and are named here rather
than left as an unexplained remainder. The external authority and the shipped files agree, so the
gate rejects `end >= frames`. **This distribution is asserted by a corpus test** — 41 records, 34 terminal, 116 cue points, 26
terminal, and zero references at or past the end — because it is the evidence the decision rests on
and an unpinned measurement becomes folklore. The sweep also carries
`files_with_references_at_or_past_end`, which is **0** across all three populations, so the
falsifier covers the loose tree too and not only the two archives.

One limit of the corpus worth recording here, because it decides where a test has to come from:
**every `smpl` chunk in the game declares at most one loop** — 41 declare one, 75 declare none. The
24-byte loop stride is therefore never multiplied by anything the corpus can check, and a wrong
stride survives every corpus assertion. It is pinned by a three-loop fixture instead. `cue ` chunks
do carry 2, 3 and 4 points, so their stride is corroborated by the shipped files.

This matters because the gate first shipped with `end > frames`, which admits a loop whose endpoint
is one past the last frame — the precise failure it exists to prevent. The fixture encoded that
error as correct without stating that a choice was being made.

### Read the round-trip numbers for what they are

`--wave-roundtrip` and `--wave-roundtrip-dir` report several counters, and they are **not** equally
strong. This section says which is which, because the first version of this page quoted a
3,140/3,140 result without saying what part of the file it covered.

`reserialised_identical` compares the original bytes with a re-serialisation of the parsed
structure. That serialisation is **split**:

* **Reconstructed, and therefore load-bearing.** The `fmt ` chunk is rebuilt field by field from six
  typed values, and the `data` chunk is regenerated from decoded, sign-centred samples. Break the
  `fmt ` field offsets, or change the 8-bit conversion from `b - 128` to `b - 127`, and the sweep
  fails across the whole corpus. This is a real regression guard, and it was mutation-tested in both
  directions.
* **Carried verbatim, and therefore proving nothing.** The `RIFF` size field, every chunk id, every
  declared size, every ancillary chunk body, every pad byte and the trailing bytes are copied out of
  the parse and copied back in. The comparison cannot disagree about any of them.

`parsed` is the load-bearing number for the container walk itself: the walk is bounded by the file
length at every step, so a mis-walked chunk boundary lands on a chunk that does not fit and surfaces
as a parse failure rather than as a wrong-but-quiet result.

`import_verified` is the counter that covers the *carried* half. Every member is put back through
`--import-wave` with its own audio, which goes through a **different** serialiser — one that
recomputes every size — and the result is then re-parsed and checked against the template chunk by
chunk, body by body, pad byte by pad byte. **That check can fail, and it caught a real bug**: the
rebuilding serialiser wrote a zero pad byte where a template carried `0x20`, so importing a member's
own unmodified audio came back different with a zero exit status. Nothing in the re-serialisation
path could have noticed, because that path replays the pad.

**Observed in the corpus:**

| Sweep | Checked | Parsed | Reserialised identical | Import verified | Import identical | Refused | Failures |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `sndfx.mpq` | 1,880 | 1,880 | 1,880 | 1,880 | 1,880 | 0 | 0 |
| `special.mpq` | 1,218 | 1,218 | 1,218 | 1,218 | 1,218 | 0 | 0 |
| loose `Wav/` | 42 | 42 | 42 | 42 | 42 | 0 | 0 |
| **total** | **3,140** | **3,140** | **3,140** | **3,140** | **3,140** | **0** | **0** |

`skipped_not_wave` is 0 in all three. It is reported because without it the denominator would be
chosen by the same magic-byte test being measured, and the archive and directory sweeps would
quietly report over different populations.

#### The pad byte, and why the guard does not call the writer

One byte in the RIFF container has been wrong three times, in three different ways, and the
sequence is worth recording because the third failure was caused by the fix for the second.

1. The rewriting serialiser emitted `0x00` unconditionally, discarding a template's own pad value.
2. Fixed — but the check on the writer compared pads only when *both* sides had one, so a template
   whose **final** odd-sized chunk simply ends without a pad (a legal shape the parser records as
   `pad: None`) gained a `0x00`. A 47-byte member came back 48 bytes, with the invented byte folded
   into the recomputed `RIFF` size so every other check agreed.
3. Fixed by giving the writer and the check **one shared helper** — and the helper was wrong. It
   turned on the body length being *unchanged* rather than on its *parity*, so editing a 3-byte
   chunk to 5 bytes rewrote a `0x20` pad to `0x00`, and the check approved it by calling the same
   function. Unifying the two had removed the disagreement and the independence together.

The rule now turns on parity: an even body takes no pad; an odd body whose template body was also
odd takes the template's pad, including `None`; an odd body whose template body was even takes
`Some(0)`. And the check derives its expectation **from the template's own bytes rather than by
calling the writer's helper**. That duplication is deliberate. This repository's own recorded
lesson is that two implementations agreeing is not confirmation when they share an assumption, and
a guard that calls the code it guards cannot catch that code being wrong. A test mutates the
writer's rule back to the broken one and requires the check to reject the result, and mutating the
writer and the guard *separately* both fail the suite.

**One branch of that rule still has no oracle, and should be read as such.** For the first two
cases, byte-identity against the template is an independent check: the 3,140-member sweep compares
the writer's output with the original bytes, so the post-condition is not the only thing watching.
The third — an edit that turns an **even** final chunk **odd**, where the pad is invented as
`Some(0)` — has no such witness. The writer invents it, the check expects it by the same reasoning,
the recomputed `RIFF` size agrees because it uses the same rule, and **no corpus member has that
shape**. The choice follows RIFF and is documented; it is not corroborated.

**Why byte-identity is total, and why that is not a boast.** WAVE `data` is not compressed: there is
exactly one byte sequence that encodes a given set of PCM samples. The only freedom a WAVE writer
has is in the container, and this corpus was cut by a single tool, so there is one set of choices to
reproduce. Contrast the PBM corpus, which reached 8 of 1,045 byte-identical because IFF `BODY` data
*is* compressed and at least two packers had made different, equally valid choices. A 100% result
here says the container walk is exact; it does not say this tool would reproduce a WAVE some other
program wrote.

### Export and import

```sh
# Decode a member and write it out as a standard .wav any audio editor opens.
lom-asset-viewer --export-wave English/sndfx.mpq File00000001.wav /tmp/sound.wav

# The shipped bytes, to use as the import template.
lom-asset-viewer --extract English/sndfx.mpq File00000001.wav /tmp/template.wav

# Put the edited audio back into the member's container.
lom-asset-viewer --import-wave /tmp/edited.wav /tmp/template.wav /tmp/new-member.wav
```

`--export-wave` writes **this tool's re-encode**, not a copy of the member. That is deliberate: a
copy proves nothing about the decoder, and the modder needs the file they edit to be the file the
importer will compare against.

`--import-wave` takes the audio from the edited file and **everything else from the template**. An
editor that drops the Sound Forge `LIST INFO` block on save does not cost you the block — verified
end to end: a `fmt |data|LIST` member exported, stripped back to a bare 44-byte `fmt |data` file,
and re-imported reproduces the original member byte for byte.

### Which `fmt ` chunk the output carries

The import replaces the template's whole `fmt ` structure, so the gate has to cover the whole
structure — not just the four nominal fields. It did not, and an edit declaring PCM/mono/22050/8-bit
with `block_align = 0` put that zero in the output without needing any flag. The policy now, stated
rather than implied:

* **Nominal format unchanged** — the *template's* `fmt ` chunk is kept entire, including its
  `byte_rate`, `block_align` and any extension bytes. The shipped file is the authority on fields
  the modder did not set out to change; an editor's idea of them is not.
* **Nominal format changed** (`--allow-format-change`) — the four nominal fields come from the edit
  and `block_align` and `byte_rate` are **derived** from them rather than copied.
* **Extension bytes** — no corpus member has a `fmt ` chunk longer than 16 bytes, so there is no
  evidence for what the engine does with one. An edit carrying extension bytes the template does not
  have is refused by name; on a format change the template's are dropped, because they describe a
  format no longer being written.

### What the tool refuses, and why

The precedent is the IMP writer, which refuses 3,439 frames whose pixels are shared.

| Refusal | Condition | Count in the corpus |
| --- | --- | --- |
| `unattested-format` | a format tag, channel count, rate or depth that no shipped file is evidence for | **0** |
| `partial-frame` | the `data` chunk does not hold a whole number of interleaved frames | **0** |
| `block-align-disagrees` | `block_align` contradicts the channel count and bit depth in the same `fmt ` chunk | **0** |
| `byte-rate-disagrees` | `byte_rate` contradicts the sample rate and block alignment | **0** |
| `dangling-loop-metadata` | a `smpl` loop or `cue ` point would land at or past the end of the audio being written | **0** on unedited members |
| `container-changed` | the writer's own output does not carry the container it was told to carry | **0**, and this one has fired on a real bug |

All 3,140 corpus files are rewritable; nothing that ships is refused. Four of these six are gates on
what comes *in* rather than on what ships, and the last is a gate on the tool itself.

`block-align-disagrees` and `byte-rate-disagrees` exist because both fields are redundant — for PCM,
`block_align = channels × bits/8` and `byte_rate = sample_rate × block_align`, **Documented** — and
redundant fields are the ones an editor gets wrong. The decoder computes frame size for itself and
would otherwise never notice.

`dangling-loop-metadata` is the gate on the hazard this page previously only described. `smpl` loop
records and `cue ` points address **sample frames**, inclusively; shortening the audio under them
leaves a loop or marker at or past the end, which plays as a hang or a click. 147 corpus files
declare such a record. Refused by name; `--allow-dangling-loops` writes it anyway.

The attested sets are **Observed in the corpus**, not **Documented**: format tag PCM (1) only;
channels 1 or 2; rate 11025, 22050 or 44100; depth 8 or 16. Nothing in `lomse.exe` has been read to
establish what the engine actually accepts. An import at 48 kHz is refused by name even with
`--allow-format-change`.

### Classification is not decodability

`asset::probe` now walks the WAVE container rather than reading the `fmt ` header and stopping, and
decodes the samples when there is a decoder. The two claims are kept apart:

* a **container** error, or **malformed supported PCM**, means the member is not classified — the
  probe returns an error and `--scan` counts a failure. A PCM member declaring zero channels or a
  zero bit depth belongs here. Malformedness is decided **before** any bit depth is dispatched;
  deciding it afterwards let a zero depth fall to the unsupported catch-all and be classified;
* a legal WAVE in a format with **no decoder here** is classified, with `undecoded` set. Reporting
  it as a probe failure would be the tool mistaking its own reach for the file being broken.

Only the second case is downgraded, and the distinction is typed rather than a substring of the
details string. Downgrading *every* post-header error made the probe-failure count insensitive:
supported PCM could stop decoding across an entire archive and the scan would still report zero
failures. `--scan` also now prints `decoded_entries` and `undecoded_entries` unconditionally,
because it records only the asset kind and throws the details away — without those counters a sweep
of 3,098 undecoded WAVEs reports exactly what a sweep of 3,098 decoded ones reports.

**The repository-wide "9,804 members, 0 probe failures" figure was measured against the old probe.**
It has been **re-measured** against the current one on the GS5R3 profile, and is now reported with
the counter that can falsify it: `pic.mpq` 1,406, `special.mpq` 1,218, `gs.mpq` 1,700, `imp.mpq`
3,600, `sndfx.mpq` 1,880 — **9,804 entries, 9,804 classified, 9,804 decoded, 0 undecoded, 0 probe
failures.** That figure's sensitivity has now been wrong twice and been re-measured four times; it
is quoted with the counter that can falsify it precisely because the bare "0 failures" form could
not. The counter is named `classified_entries` rather than `decoded_entries` because it counts
members of every kind — PBM, IMP, maps — and only the audio ones can be `undecoded`.

## Smacker

### Scope

**The video codec is not implemented.** No Huffman tree is built and no frame is decoded to pixels.
What is established is the **container**: the header, both frame tables, the tree-section extent,
and the palette/audio/video split inside every frame.

### Header

104 bytes, little-endian. **Documented** field layout (the format is described by the
multimedia-format community and implemented in ScummVM and libav), **Observed in the corpus** where
the installed files bear it out. **No one has diffed this layout against ScummVM or libav source**
— a wrong offset shared by the parser and this table would be invisible to the test suite, which is
why the arithmetic cross-checks below matter more than the table does.

| Offset | Size | Field | Corpus |
| --- | --- | --- | --- |
| 0 | 4 | signature `SMK2` or `SMK4` | `SMK2` in all 23 |
| 4 | 4 | width | 500 in 22, 640 in `Credits.smk` |
| 8 | 4 | height | 292 in 22, 480 in `Credits.smk` |
| 12 | 4 | frame count | 144–1,098 |
| 16 | 4 | frame rate, signed | `-8333` in all 23 → 83,330 µs ≈ 12.0 fps |
| 20 | 4 | flags | `0` in all 23 |
| 24 | 28 | `audio_size[7]`, unpacked buffer per track | track 0 nonzero, tracks 1–6 zero in all 23 |
| 52 | 4 | tree-section size in bytes | 54,264–302,630 |
| 56 | 4 | `mmap_size` | decoded-table size, not a byte extent |
| 60 | 4 | `mclr_size` | as above |
| 64 | 4 | `full_size` | as above |
| 68 | 4 | `type_size` | as above |
| 72 | 28 | `audio_rate[7]`, packed descriptor per track | see below |
| 100 | 4 | **unknown** | `0x00000000` in all 23 |

The frame-rate word has three cases, **Documented**: positive is milliseconds per frame, negative is
negated hundredths of a millisecond, zero means ten frames a second. Every installed file uses the
negative case.

`flags` bit 0 is the ring frame (an extra looping frame beyond `frame_count`, lengthening both
tables by one entry), bit 1 is Y-interlaced and bit 2 is Y-doubled. **Observed in the corpus:** all
three are clear in all 23 files, and no bit outside those three is ever set.

`audio_rate[track]` packs the descriptor into one word: bit 31 present, bit 30 Smacker Huffman
compression, bit 29 16-bit, bit 28 stereo, bit 27 Bink DCT audio, bits 0–23 the sample rate.
**Observed in the corpus:** every one of the 23 files has exactly one track, track 0, at **22050 Hz,
8-bit, stereo, Smacker-Huffman compressed**. No bit outside the six named above is ever set.

### Frame tables and frame layout

Immediately after the header come `frame_count` (+1 with a ring frame) 4-byte size words, then the
same number of 1-byte type flags, then `trees_size` bytes of tree data, then the frame payloads back
to back.

A size word's low two bits are flags — bit 0 keyframe, bit 1 unnamed — so **a frame payload size is
always a multiple of four**. **Observed in the corpus:** reading it that way makes the header, the
tables, the tree section and all frame sizes sum to *exactly* the file length for all 23 files
(`sizes_account_for_every_byte 23`). That closure is the structural claim, and because it *is* the
claim, a `.smk` whose sizes do not close is **not classified** as Smacker video and makes
`--scan-smk-dir` exit non-zero. (The asymmetry with the WAVE probe above is deliberate: there, the
undecodable case is a limit of this tool; here, a file whose sizes do not close is one this module
has not understood.)

A type byte's bit 0 says the frame opens with a palette chunk; bits 1–7 say which of the seven audio
tracks carry data. Inside a frame, in order: the palette chunk if present (a leading byte holding
the chunk length in units of four, itself included), then one chunk per flagged audio track (a
leading 4-byte length including itself, and for a compressed track the decompressed length in the
next four bytes), then the remainder, which is the video data this tool does not read. A chunk on a
compressed track that declares fewer than 8 bytes cannot hold its own decompressed length and is
rejected rather than read as uncompressed.

**The control on that walk.** Nothing in the walk itself checks that an audio chunk was found in the
right place — a mis-placed read that happened to fit would pass silently. But each compressed chunk
carries its own decompressed length, and the sum of those lengths must come out at the track's byte
rate times the running time, a number derived from `frame_count` and the rate word and never from a
chunk position. **Observed in the corpus:** the two totals agree for all 23 files, the largest
disagreement being **36 bytes** against per-file totals of 0.5–4.0 MB — well under one frame's 3,675
bytes of audio, and explained by the 83,330 µs frame interval not dividing evenly into milliseconds.

Every factor in that control is a **file-declared** value, so all of it is computed in `u128` and
only the quotient has to fit `u64`. A crafted file can name a rate, channel count, frame count and
interval whose *quotient* leaves `u64`; the control then reports `unrepresentable` and
`--scan-smk-dir` exits non-zero, rather than presenting a wrapped number as a measurement. The same
rule this module already applied to allocation now applies to arithmetic — in both directions: the
first version checked each multiplication instead of the result and so refused to compute a value
of 18,734,973,333,169, which fits `u64` with room to spare. An over-rejecting guard is a wrong
answer too.

Two measurements are reported, deliberately separated. `unpacked-from-chunks` totals **every**
frame; `unpacked-in-timed-frames` excludes a ring frame and is what the control compares against.
Excluding the ring frame is right for a comparison with the running time and wrong for a question
about what the file contains, and letting the control's need silently redefine a reporting number
is how a figure comes to mean something other than its name. Neither total is produced for a
**Bink DCT** track: the payload's decoded length is not stated anywhere this module reads, and
calling its compressed byte count "unpacked" would be false, so it is a named skip.

### Measured corpus

`--scan-smk-dir 'English/smk'` — all 23 files, 0 failures, `worst_audio_total_drift_bytes 36`,
`unrepresentable_audio_controls 0`. Sample of the result:

| File | Size | Frames | Duration | Palette frames | Tree bytes | Video bytes | Audio bytes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `Intro.smk` | 500×292 | 1,098 | 91.5 s | 22 | 302,630 | 21,963,980 | 1,860,616 |
| `Credits.smk` | 640×480 | 800 | 66.7 s | 1 | 54,264 | 4,315,120 | 1,489,432 |
| `legend/legends.smk` | 500×292 | 493 | 41.1 s | 50 | 293,367 | 15,277,908 | 987,440 |
| `PlayerLose/AirDie.smk` | 500×292 | 260 | 21.7 s | 6 | 99,460 | 3,612,336 | 426,804 |

**Observed in the corpus:** the keyframe bit is set on **zero** frames across all 23 files, and the
unnamed bit 1 is set on zero frames. Both flags are parsed, and this corpus exercises neither in the
positive direction.

## Commands

```sh
lom-asset-viewer --wave-roundtrip ARCHIVE.mpq [--listfile FILE]
lom-asset-viewer --wave-roundtrip-dir DIRECTORY
lom-asset-viewer --export-wave ARCHIVE.mpq MEMBER OUTPUT.wav [--listfile FILE]
lom-asset-viewer --import-wave EDITED.wav TEMPLATE.wav OUTPUT.wav \
    [--allow-format-change] [--allow-dangling-loops]
lom-asset-viewer --describe-smk FILE.smk
lom-asset-viewer --scan-smk-dir DIRECTORY
```

Corpus-gated tests run with `LOM_GAME_DIR='.../English' cargo test --release -- --ignored`. They
assert the documented sizes (3,098 archived members, 42 loose, 23 `.smk`) so a corpus that silently
shrinks is caught, and they assert the round-trip and closure *rules* rather than a table of
expected per-member results.

## What is not determined

**WAVE**

1. **Which archive the engine reads a sound from.** `sndfx.mpq` and `special.mpq` share 1,142 files
   by content. Nothing here establishes the lookup order, so a mod that edits one may be overridden
   by the other's copy. Not measured.
2. **What the engine actually accepts.** The attested format sets come from what the game ships, not
   from reading `lomse.exe` or from an engine probe. A rate outside them may well work; a rate
   inside them may fail in a context no shipped file covers. **Inferred**, and gated conservatively.
3. **The 16-bit path rests on one file.** Exactly one member in the whole game is 16-bit. Its decode
   and re-encode are byte-exact, but "16-bit PCM round-trips" is a claim with a single witness.
4. **No file was put in front of the engine.** Nothing written by `--import-wave` has been installed
   and played. The claim is that the bytes are a correct WAVE with the template's container, not
   that the game loads it. That is the same gap the map writer had before the `mapload` probe.
5. **`cue ` and `smpl` contents are read for extent only.** The highest sample frame each refers to
   is extracted and gated on; nothing here interprets a loop type, a play count, or a cue's
   `chunkStart`/`blockStart`. A loop whose *start* still fits but whose semantics the edit breaks is
   not caught.
6. **`--allow-dangling-loops` leaves the metadata dangling.** It does not fix, retarget or strip the
   chunk; it only stops refusing. There is no tool here that rescales loop points to a new length.
7. **The `fmt ` extension policy is a decision, not a measurement.** No corpus member has extension
   bytes, so refusing an edit that introduces them is the conservative reading of no evidence —
   not a finding about the engine.

**Smacker**

8. **The video codec.** Explicitly out of scope. Frame extents are known; frame *contents* are not
   decoded, and nothing here can render, re-encode or re-time a frame's pixels.
9. **The tree section is an opaque extent.** `trees_size` bytes are located and their length is
   confirmed by the closure, but the four trees inside are not split apart. `mmap_size`, `mclr_size`,
   `full_size` and `type_size` are **Documented** as decoded-table sizes rather than byte extents
   and are reported without being used.
10. **Header word at offset 100.** `0x00000000` in all 23 files. One value across 23 files is not
    evidence of meaning. Carried verbatim, labelled unknown.
11. **Frame-size bit 1.** Never set in this corpus. Parsed, named `unknown`, never interpreted.
12. **The keyframe bit is never set here.** The parse of bit 0 is exercised only in the negative
    direction by this corpus.
13. **`SMK4`, ring frames, Y-interlaced and Y-doubled, Bink audio, 16-bit and multi-track audio are
    all unexercised by the corpus.** The parser handles them from the documented layout, and the
    uncompressed-track, ring-frame and Bink paths have unit tests, but no installed file tests any
    of them. Bink is a **named skip** rather than a handled case: the decoded length of a Bink DCT
    payload is not determined here, so no unpacked total is produced for such a track.
14. **The header layout has not been checked against an external implementation.** It is
    **Documented** from community descriptions; ScummVM and libav source were not diffed against it.
    The size closure and the audio-rate control are what stand behind it.
15. **There is no Smacker writer.** The container is read-only.
