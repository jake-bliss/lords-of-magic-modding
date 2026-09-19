# Audio and video formats

Sound effects, speech and music are **RIFF WAVE**. Cutscenes are **Smacker**. This page records
what has been measured about both, what the tool can now read and write, and — in its own section
at the end — what is still not determined.

Evidence classes are used throughout, as everywhere in this repository: **Observed in the corpus**,
**Observed in a local binary**, **Documented**, **Inferred**, **Refuted**. An unlabelled assertion
here is a defect.

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

Measured with `--wave-roundtrip`, which classifies by **decoding** each member, not by reading its
header. **Observed in the corpus:**

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
engine reads a given sound from has **not** been established and is not needed for any measurement
here — but it matters for modding, because editing one archive may leave the other's copy in place.

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

The `LIST INFO` blocks name the authoring tool. **Observed in the corpus:** every one of the 1,678
`ISFT` fields across both archives reads `Sound Forge 4.0`, and the `ICRD` dates run 1996 (16),
1997 (1,232) and 1998 (430). One tool cut the entire corpus.

### Round-trip: 3,140 of 3,140 byte-identical

`--wave-roundtrip` and `--wave-roundtrip-dir` parse each file, rebuild the `fmt ` chunk from the
decoded format fields and the `data` chunk from the decoded samples, re-serialise the container, and
compare with the original. Two counts are reported and must not be conflated — the same distinction
[the PBM and IMP sweeps](native-asset-stage.md) make:

* **sample-lossless** — decode, re-encode, decode again gives the same samples. This is the
  correctness claim; anything short of every file is a bug.
* **byte-identical** — the re-serialised file equals the original byte for byte. This is a
  *fidelity observation about the original authoring tool*, not a correctness claim.

**Observed in the corpus:**

| Sweep | Checked | Parsed | Sample-lossless | Byte-identical | Refused | Failures |
| --- | --- | --- | --- | --- | --- | --- |
| `sndfx.mpq` | 1,880 | 1,880 | 1,880 | 1,880 | 0 | 0 |
| `special.mpq` | 1,218 | 1,218 | 1,218 | 1,218 | 0 | 0 |
| loose `Wav/` | 42 | 42 | 42 | 42 | 0 | 0 |
| **total** | **3,140** | **3,140** | **3,140** | **3,140** | **0** | **0** |

**There is no gap between the two counts, and the reason is worth stating rather than celebrating.**
The PBM corpus reached only 8 of 1,045 byte-identical because IFF `BODY` data is *compressed*, and
at least two different packers had made different, equally valid choices about run boundaries. WAVE
`data` is not compressed: there is exactly one byte sequence that encodes a given set of PCM
samples. The only freedom a WAVE writer has is in the container — chunk order, padding, which
metadata chunks to emit — and this corpus was cut by a single tool, so there is only one set of
choices to reproduce. A 100% result here is therefore a much weaker statement about the encoder than
8/1,045 was about the IMP one. It says the container walk is exact; it does not say this tool would
reproduce a WAVE some other program wrote.

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
importer will compare against. It refuses any member that does not re-encode to its own bytes.

`--import-wave` takes the audio from the edited file and **everything else from the template**. An
editor that drops the Sound Forge `LIST INFO` block on save does not cost you the block — verified
end to end: a `fmt |data|LIST` member exported, stripped back to a bare 44-byte `fmt |data` file,
and re-imported reproduces the original member byte for byte. All `data`, `RIFF` and pad sizes are
recomputed from what is actually written, so an edit that changes the length is written correctly.

### What the tool refuses to rewrite, and why

The precedent is the IMP writer, which refuses 3,439 frames whose pixels are shared. The WAVE
importer has three refusals, each named to the caller rather than silently applied:

| Refusal | Condition | Count in the corpus |
| --- | --- | --- |
| `container-not-reproducible` | the template does not re-encode to its own bytes, so a rewrite would change bytes the modder never asked to change | **0** |
| `partial-frame` | the `data` chunk does not hold a whole number of interleaved frames | **0** |
| `unattested-format` | a format tag, channel count, rate or depth that no shipped file is evidence for | **0** |

**All 3,140 files are rewritable.** Nothing in this corpus is refused. That is an honest result and
not a claim that the refusals are decorative — the first two are reachable by any file that is not
from this corpus, and `unattested-format` is reached by the *incoming edit* rather than by the
template. The attested sets are:

* format tag: PCM (1) only;
* channels: 1 or 2;
* sample rate: 11025, 22050 or 44100;
* bit depth: 8 or 16.

An import at, say, 48 kHz is refused by name even with `--allow-format-change`. These sets are
**Observed in the corpus**, not **Documented**: nothing in `lomse.exe` has been read to establish
what the engine actually accepts. They are the set for which a shipped file is evidence that the
game plays it, which is the strongest ground available without an engine probe. Changing the format
at all — even within the set — requires `--allow-format-change`, because a member's format matching
its neighbours' is itself evidence and departing from it is a decision, not a default.

## Smacker

### Scope

**The video codec is not implemented.** No Huffman tree is built and no frame is decoded to pixels.
What is established is the **container**: the header, both frame tables, the tree-section extent,
and the palette/audio/video split inside every frame. `docs/` records this as a container result
and nothing more.

### Header

104 bytes, little-endian. **Documented** field layout (the format is described by the
multimedia-format community and implemented in ScummVM and libav), **Observed in the corpus** where
the installed files bear it out.

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
three are clear in all 23 files, and no bit outside those three is ever set. The ring-frame path is
therefore implemented and **unexercised by this corpus**.

`audio_rate[track]` packs the descriptor into one word: bit 31 present, bit 30 Smacker Huffman
compression, bit 29 16-bit, bit 28 stereo, bit 27 Bink DCT audio, bits 0–23 the sample rate.
**Observed in the corpus:** every one of the 23 files has exactly one track, track 0, at **22050 Hz,
8-bit, stereo, Smacker-Huffman compressed**. No file uses Bink audio, 16-bit audio, or a second
track. No bit outside the six named above is ever set.

### Frame tables and frame layout

Immediately after the header come `frame_count` (+1 with a ring frame) 4-byte size words, then the
same number of 1-byte type flags, then `trees_size` bytes of tree data, then the frame payloads back
to back.

A size word's low two bits are flags — bit 0 keyframe, bit 1 unnamed — so **a frame payload size is
always a multiple of four**. **Observed in the corpus:** reading it that way makes the header, the
tables, the tree section and all frame sizes sum to *exactly* the file length for all 23 files
(`sizes_account_for_every_byte 23`, `tail 0` on every file). That closure is the structural claim.

A type byte's bit 0 says the frame opens with a palette chunk; bits 1–7 say which of the seven audio
tracks carry data. Inside a frame, in order: the palette chunk if present (a leading byte holding
the chunk length in units of four, itself included), then one chunk per flagged audio track (a
leading 4-byte length including itself, and for a compressed track the decompressed length in the
next four bytes), then the remainder, which is the video data this tool does not read.

**The control on that walk.** Nothing in the walk itself checks that an audio chunk was found in the
right place — a mis-placed read that happened to fit would pass silently. But each compressed chunk
carries its own decompressed length, and the sum of those lengths must come out at the track's byte
rate times the running time, a number derived from `frame_count` and the rate word and never from a
chunk position. **Observed in the corpus:** the two totals agree for all 23 files, the largest
disagreement being **36 bytes** against per-file totals of 0.5–4.0 MB — well under one frame's 3,675
bytes of audio, and explained by the 83,330 µs frame interval not dividing evenly into milliseconds.
If the split were wrong, lengths would be read out of video data and the totals would not land
close.

### Measured corpus

`--scan-smk-dir 'English/smk'` — all 23 files, 0 failures. Sample of the result:

| File | Size | Frames | Duration | Palette frames | Tree bytes | Video bytes | Audio bytes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `Intro.smk` | 500×292 | 1,098 | 91.5 s | 22 | 302,630 | 21,963,980 | 1,860,616 |
| `Credits.smk` | 640×480 | 800 | 66.7 s | 1 | 54,264 | 4,315,120 | 1,489,432 |
| `legend/legends.smk` | 500×292 | 493 | 41.1 s | 50 | 293,367 | 15,277,908 | 987,440 |
| `PlayerLose/AirDie.smk` | 500×292 | 260 | 21.7 s | 6 | 99,460 | 3,612,336 | 426,804 |

**Observed in the corpus:** the keyframe bit is set on **zero** frames across all 23 files, and the
unnamed bit 1 is set on zero frames. Both flags are parsed, and this corpus exercises neither in the
positive direction.

`--inspect-file` and `--scan` now classify a `.smk` by parsing the container rather than by matching
four magic bytes, so a file reported as `smacker-video` is one whose tables were walked and whose
sizes closed.

## Commands

```sh
lom-asset-viewer --wave-roundtrip ARCHIVE.mpq [--listfile FILE]
lom-asset-viewer --wave-roundtrip-dir DIRECTORY
lom-asset-viewer --export-wave ARCHIVE.mpq MEMBER OUTPUT.wav [--listfile FILE]
lom-asset-viewer --import-wave EDITED.wav TEMPLATE.wav OUTPUT.wav [--allow-format-change]
lom-asset-viewer --describe-smk FILE.smk
lom-asset-viewer --scan-smk-dir DIRECTORY
```

Corpus-gated tests run with
`LOM_GAME_DIR='.../English' cargo test --release -- --ignored`; they assert the round-trip and
closure *rules* over whatever the archives hold rather than against a table of expected per-member
results.

## What is not determined

**WAVE**

1. **Which archive the engine reads a sound from.** `sndfx.mpq` and `special.mpq` share 1,142 files
   by content. Nothing here establishes the lookup order, so a mod that edits one may be overridden
   by the other's copy. Not measured.
2. **What the engine actually accepts.** The attested format sets come from what the game ships, not
   from reading `lomse.exe` or from an engine probe. A rate outside them may well work; a rate
   inside them may fail in a context no shipped file covers. **Inferred**, and deliberately gated
   conservatively.
3. **The 16-bit path rests on one file.** Exactly one member in the whole game is 16-bit. Its decode
   and re-encode are byte-exact, but "16-bit PCM round-trips" is a claim with a single witness.
4. **No file was put in front of the engine.** Nothing written by `--import-wave` has been installed
   and played. The claim is that the bytes are a correct WAVE with the template's container, not
   that the game loads it. That is the same gap the map writer had before the `mapload` probe.
5. **`cue ` and `smpl` chunk contents are not interpreted.** They are carried verbatim through every
   round trip and reproduce byte-for-byte, but nothing here reads a loop point or a cue position. A
   modder who shortens a looping sound will preserve a `smpl` chunk that now points past the end.
   **Not determined, and a real hazard.**
6. **`byte_rate` and `block_align` are carried, not validated.** They are reproduced exactly and are
   self-consistent in every shipped file, but the decoder computes frame size from channels and bit
   depth and never trusts `block_align`. A file where the two disagree would round-trip without
   comment.

**Smacker**

7. **The video codec.** Explicitly out of scope. Frame extents are known; frame *contents* are not
   decoded, and nothing here can render, re-encode or re-time a frame's pixels.
8. **The tree section is an opaque extent.** `trees_size` bytes are located and their length is
   confirmed by the closure, but the four trees inside are not split apart. `mmap_size`, `mclr_size`,
   `full_size` and `type_size` are **Documented** as decoded-table sizes rather than byte extents
   and are reported without being used.
9. **Header word at offset 100.** `0x00000000` in all 23 files. One value across 23 files is not
   evidence of meaning. Carried verbatim, labelled unknown.
10. **Frame-size bit 1.** Never set in this corpus. Parsed, named `unknown`, never interpreted.
11. **The keyframe bit is never set here.** The parse of bit 0 is exercised only in the negative
    direction by this corpus.
12. **`SMK4`, ring frames, Y-interlaced and Y-doubled, Bink audio, 16-bit and multi-track audio are
    all unexercised.** The parser handles them from the documented layout; no installed file tests
    any of them.
13. **There is no Smacker writer.** The container is read-only. Nothing in this repository can
    produce or modify a `.smk`.
