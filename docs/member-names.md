# Recovering MPQ member names

## What this is

An MPQ stores a **hash** of each member's name, never the name. A member whose
name no catalogue supplies can be read and hashed, but it cannot be addressed,
cannot be replaced by name, and cannot be verified by name. This repository
treated that remainder as a hard limit; it is not one.

Names are **archive-independent**. A name learned from one profile's `(listfile)`
can be tested directly against another profile's archive, and a name from a
published catalogue can be tested against all of them. Pooling every catalogue
available and asking each archive, directly, which of those names it will open
reduces the unnamed remainder across the three installed profiles from **23,026
of 28,721 members to 21**.

The distinction the whole exercise rests on:

- a **proposal** is a name that something suggests — a matching content digest,
  an entry in some catalogue, a coinciding hash index;
- a **confirmation** is the target archive opening that name and handing back the
  block, the hash-table slot, and bytes that agree with what the block-index
  manifest already recorded for that block.

Only confirmations are reported as names. **A content match or a hash-index match
is a proposal, never a conclusion.**

## Why it matters beyond addressing

An unnamed member's only handle is StormLib's `File%08u.xxx` pseudo-name, which
is its **block index** — a position, not an identity. Positions move. They differ
between archives, and, as the Phase 3 work measured on 2026-09-18, StormLib
**renumbers unnamed blocks when it rewrites an archive**: a repack of vanilla
`gs.mpq` that replaced one named member produced 1,688 entries in and out with
372 unnamed in and out, and the shape check read the renumbering as 4 added, 4
missing and 26 undeclared content changes. `tools/mpq_shape.py` had to be
weakened to compare unnamed members as a **multiset of content identities**: it
can still see that an unnamed member was lost, added, or altered in content,
size, flags or locale, but it cannot say *which*, and it cannot see two
identical ones swap.

**Every name recovered here moves a member out of that weakened check and back
under the strong per-member one.** What follows is therefore not only about
making assets addressable from a mod tree; it is about how much of the archive
write path can be verified at all.

| Profile / archive | Members | Under the weakened check before | After | Left weak |
|---|---:|---:|---:|---:|
| vanilla `gs.mpq` | 1,688 | 372 (22.0%) | 9 | 0.5% |
| vanilla `pic.mpq` | 1,071 | 1,071 (100%) | 1 | 0.1% |
| vanilla `imp.mpq` | 3,600 | 3,600 (100%) | 0 | 0% |
| vanilla `sndfx.mpq` | 1,880 | 1,880 (100%) | 0 | 0% |
| vanilla `special.mpq` | 1,218 | 1,218 (100%) | 0 | 0% |
| 3.02 `gs.mpq` | 1,691 | 9 (0.5%) | 9 | 0.5% |
| 3.02 `pic.mpq` | 1,071 | 1,071 (100%) | 1 | 0.1% |
| 3.02 `imp.mpq` | 3,600 | 3,600 (100%) | 0 | 0% |
| 3.02 `sndfx.mpq` | 1,880 | 1,880 (100%) | 0 | 0% |
| 3.02 `special.mpq` | 1,218 | 1,218 (100%) | 0 | 0% |
| GS5R3 `gs.mpq` | 1,700 | 0 | 0 | 0% |
| GS5R3 `pic.mpq` | 1,406 | 409 (29.1%) | 1 | 0.1% |
| GS5R3 `imp.mpq` | 3,600 | 3,600 (100%) | 0 | 0% |
| GS5R3 `sndfx.mpq` | 1,880 | 1,880 (100%) | 0 | 0% |
| GS5R3 `special.mpq` | 1,218 | 1,218 (100%) | 0 | 0% |
| **Total** | **28,721** | **23,026 (80.2%)** | **21** | **0.1%** |

This does **not** make the shape check strong again by itself. The weakening in
`tools/mpq_shape.py` is still there and still applies to whatever remains
unnamed; supplying a recovered listfile to the manifest step is what converts a
member. Twenty-one members across three profiles stay under the weakened
comparison permanently unless someone finds their names.

## The pipeline

```sh
scripts/recover-member-names.sh ~/Applications artifacts/names-20260918
```

Every archive is opened read-only and no installed profile is written. The
script refuses an existing output directory and refuses to write anywhere under
`~/Applications`.

| Piece | Job |
|---|---|
| `lom-mpq manifest ARCHIVE` | every member by **block index**, with hash index, size, flags, locale and SHA-256 |
| `lom-mpq probe-names ARCHIVE NAMES` | open each candidate name; report block index, hash index, size and the digest of the bytes read **by name**. `NAMES` may be `-` for stdin |
| `tools/member_names.py` | pool the catalogues, derive the controls, decide which proposals are confirmations, refuse the rest |
| `lom-mpq … --listfile NAMES` | supply recovered names to `list`, `manifest` and `extract` |

`probe-names` deliberately does **not** load the target archive's own
`(listfile)`. A hit is therefore a property of the archive's hash table, reached
by the same mechanism for a name the archive knows and a name it does not.

### What counts as a confirmation

All four, or it is not a name:

1. the archive opened the candidate;
2. the block StormLib resolved it to is the block in question;
3. the hash-table slot StormLib reported equals the slot the manifest recorded
   for that block;
4. the SHA-256 of the bytes read **by name** equals the SHA-256 of the bytes read
   **by block index**.

Term 3 is what settles ambiguity. A content digest can appear under several
names; a hash slot cannot. The slot is a function of the name alone, so at most
one candidate can occupy a given block's slot, and combined with a successful
open that is conclusive.

## Results

**Observed 2026-09-18**, StormLib 9.40 via Homebrew, macOS 26.5.2.

9,775 distinct candidate names were pooled from the catalogues below and probed
against all fifteen archives.

| Profile / archive | Members | Already named | Recovered | Still unnamed |
|---|---:|---:|---:|---:|
| vanilla `gs.mpq` | 1,688 | 1,316 | **363** | 9 |
| vanilla `pic.mpq` | 1,071 | 0 | **1,070** | 1 |
| vanilla `imp.mpq` | 3,600 | 0 | **3,600** | 0 |
| vanilla `sndfx.mpq` | 1,880 | 0 | **1,880** | 0 |
| vanilla `special.mpq` | 1,218 | 0 | **1,218** | 0 |
| 3.02 `gs.mpq` | 1,691 | 1,682 | 0 | 9 |
| 3.02 `pic.mpq` | 1,071 | 0 | **1,070** | 1 |
| 3.02 `imp.mpq` | 3,600 | 0 | **3,600** | 0 |
| 3.02 `sndfx.mpq` | 1,880 | 0 | **1,880** | 0 |
| 3.02 `special.mpq` | 1,218 | 0 | **1,218** | 0 |
| GS5R3 `gs.mpq` | 1,700 | 1,700 | 0 | 0 |
| GS5R3 `pic.mpq` | 1,406 | 997 | **408** | 1 |
| GS5R3 `imp.mpq` | 3,600 | 0 | **3,600** | 0 |
| GS5R3 `sndfx.mpq` | 1,880 | 0 | **1,880** | 0 |
| GS5R3 `special.mpq` | 1,218 | 0 | **1,218** | 0 |
| **Total** | **28,721** | **5,695** | **23,005** | **21** |

Ambiguous confirmations: **zero**. No block in any of the fifteen archives was
confirmed by more than one name.

### Where the names came from

`covered` counts members a source could name; `only this source` counts members
no other source could.

| Source | Covered | Only this source |
|---|---:|---:|
| Published catalogue (`artifacts/reference-listfiles/lords-of-magic.txt`) | 22,642 | 21,318 |
| 3.02 `gs.mpq` internal listfile | 363 | 363 |
| GS5R3 `pic.mpq` internal listfile | 1,324 | 0 |

The published catalogue is the overwhelming contributor and is the only source
for `imp.mpq`, `sndfx.mpq` and `special.mpq`, which no profile's listfile names
at all. Cross-profile donation matters in exactly one place — the 363 vanilla
`gs.mpq` scripts that only 3.02's listfile names — and GS5R3's `pic.mpq`
listfile, while it confirms 1,324 members, is redundant with the catalogue in
every one of them.

The catalogue was already fetched by `scripts/fetch-lom-listfile.sh` and pinned
by SHA-256, and had never been applied to `imp.mpq`, `sndfx.mpq` or
`special.mpq`.

### What is still unnamed, and why

**Nine members of `gs.mpq`, in vanilla and in 3.02.** Blocks 227–235 in both, at
hash slots 1570, 1094, 1188, 1648, 858, 1977, 1842, 641 and 435; byte-identical
between the two profiles; 274 to 4,341 bytes; `MPQ_FILE_IMPLODE`. They are
GameScript combat-AI plays, defining `/assault_play`,
`/attack_with_ranged_play`, `/attack_ranged_enemy_play`, `/default_play`,
`/defense_play`, `/digin_play`, `/flank_attack`, `/flank_attack_play` and
`/retreat_play`. **Inferred**, not observed: they are alternate or superseded
copies of nine plays the listfile already names — `gs\plays\assault.gs`,
`gs\plays\default.gs`, `gs\plays\defense.gs`, `gs\plays\digin.gs`,
`gs\plays\flank.gs`, `gs\plays\flankat.gs`, `gs\plays\retreat.gs`,
`gs\plays\attackr.gs`, `gs\plays\attackre.gs` — because those named members
exist separately and define the same symbols. Nothing establishes what the nine
are called.

**One member of `pic.mpq`, in all three profiles.** 239,126 bytes, SHA-256
`cdc69af6…`, an IFF `FORM`/`PBM ` image, `MPQ_FILE_IMPLODE`. It sits at block
1070 in vanilla and 3.02 and block **745** in GS5R3 — while occupying hash slot
**281** in all three. That is the block-index-versus-hash-index distinction in
one measurement: the position moved, the name-derived slot did not.

## The negative control, and the chance rate

**The instrument's reach is stated before its hits are.**

Negative controls are derived from the candidate pool itself: every
twenty-fifth pooled name with `ZQNOTREAL` appended. Drawing them from the pool
keeps them in the same shape distribution as the real candidates and makes their
number scale with the run. **391 controls** were probed against each of the
fifteen archives — **5,865 control probes, 0 opened**. A single control that
opened fails the whole run and `tools/member_names.py` exits nonzero; this is
not advisory.

**Chance rate, stated before the result.** An MPQ hash-table lookup matches on
two independent 32-bit name hashes, so a name that is not a member has a
2⁻⁶⁴ chance of resolving to any given slot. Across 10,166 probes per archive and
28,721 members in total, the expected number of spurious confirmations for the whole run
is 10,166 × 28,721 / 2⁶⁴ ≈ **1.6 × 10⁻¹¹**. The independence and uniformity of
the two hashes is **Inferred** from the format, not measured here; the 5,865
refusals are **Observed**.

That rate is what makes brute force safe as well as cheap. `probe-names` sustains
about 7.3 million candidates per second against `gs.mpq`.

### The bounded search behind the nine

161,802,554 generated names were probed against vanilla `gs.mpq`:

- `gs\plays\` + 1–5 characters from `[a-z0-9_]` + `.gs` — 71,270,177 names;
- the same 1–4 character set under `` (root), `gs\`, `gs\turnai\`, `gs\ai\`,
  `gs\combat\`, `gs\special\`, `gs\scenario\`, `gs\dungeons\`, `gs\plays2\`,
  `gs\oldplays\`, `gs\plays\old\`, plus `gs\` at 5 characters — 90,532,377 names.

45 opened, and **none landed on blocks 227–235**. Expected spurious hits for a
search that size: 161,802,554 × 1,688 / 2⁶⁴ ≈ 1.5 × 10⁻⁸.

Those 45 are also a **positive control that the catalogues were not needed to get
the right answer**. Every one of them names a member that recovery had already
named — 11 of them members that vanilla's own listfile does *not* name, recovered
from the published catalogue — and all 45 agree with the recovered name
character for character, case-folded. A search that knows nothing about any
catalogue reproduced the catalogue's answers exactly, and still found nothing for
the nine.

### The bounded search behind the one picture

277,375,791 generated names were probed against vanilla `pic.mpq`: 1–5
characters from `[a-z0-9_]` plus `.lbm` under `` (root), `lbm\` and
`portrait\`, and 1–4 characters with `.lbm`, `.bmp` and `.pbm` under those and
`iface\`, `keeps\`, `building\`, `units\`, `palette\`, `library\`,
`missile\` and `aura\`.

19 opened, on 15 distinct members. All 15 agree with the recovered name, and
**none is block 1070**. Expected spurious hits: 277,375,791 × 1,071 / 2⁶⁴ ≈
1.6 × 10⁻⁸.

### What the bound is worth

Both searches bound the negative; neither proves it. Names longer than the
searched lengths, names in directories not tried, names with other extensions,
and names using characters outside `[a-z0-9_]` were all out of reach — and since
most real names in this corpus are six to eight characters, the reach is a small
fraction of the plausible space. What the searches do establish is that the
instrument works on this archive without any catalogue at all: between them they
reproduced 60 names, every one matching what recovery had already concluded, and
found nothing whatever for the 21 members that stay unnamed.

## The trap that nearly produced a false result

**Observed 2026-09-18** on vanilla `sndfx.mpq`: a pseudo-name is not a label
StormLib prints, it is a name StormLib **resolves positionally**, and the
resolution ignores everything after the digits.

| Candidate | Result |
|---|---|
| `File00000022.wav` | opens block 22 |
| `File00000022.xxx` | opens block 22 |
| `file00000022.wav` | opens block 22 |
| `FILE00000022.WAV` | opens block 22 |
| `File00000022.wavZQNOTREAL` | opens block 22 |
| `File00000022` | does not open |
| `File00000022ZQ` | does not open |
| `File22.wav` | does not open |
| `File000000022.wav` | does not open |
| `sub\File00000022.wav` | does not open |

Case-insensitive `File`, exactly eight digits, a dot, then anything, with no
directory component. Two consequences:

1. **A pseudo-name must never enter a candidate pool.** It confirms against its
   own block on every one of the four terms and reports a "recovery" that is the
   block number written back out. This was caught by the negative controls —
   `File00000022.wavZQNOTREAL` was a control, and it opened. Without the
   controls the run would have reported thousands of recoveries that were
   nothing at all.
2. **The extension StormLib prints is a guess from the member's first bytes.**
   `.xxx` is only the fallback when it cannot guess. An archive whose listing
   appears full of `File00000000.wav` entries has no names.

**Corrected:** an earlier count in this work treated only `File%08u.xxx` as a
placeholder and consequently reported vanilla `sndfx.mpq` as 1,880 named and
`special.mpq` as 1,218 named. Both are **zero** named. They carry no internal
`(listfile)` at all; every entry in their listings was a guessed-extension
pseudo-name.

## Using the recovered names

The names are committed, per archive, in [`reports/member-names/`](../reports/member-names/).

```sh
.build/lom-mpq manifest /path/to/pic.mpq \
  --listfile reports/member-names/vanilla-and-302-pic-recovered.txt
.build/lom-mpq extract /path/to/pic.mpq /new/output/dir \
  --listfile reports/member-names/vanilla-and-302-pic-recovered.txt
```

Two deliberate choices about how they are wired in:

- **Nothing is written to any archive.** The names are supplied to the reader.
  Adding them to an archive's `(listfile)` would mean writing every installed
  archive to gain what a text file gains for free.
- **A name is re-resolved against the archive in front of it, every time.**
  Nothing is cached as a block→name table, because a block index moves when an
  archive is rewritten. A list applied to the wrong archive, or to an archive
  that has since been repacked, loses names; it cannot mislabel a member.

Block-index addressing is untouched. `lom-mpq manifest` still reads every member
by block index, which is what lets it see the two distinct PIC5R3 members that
share the name `portrait\AIpotM.lbm`. Supplying a listfile changes the `path`
column and nothing else: verified on vanilla `pic.mpq`, where all 1,071 blocks
kept identical hash index, size, compressed size, flags, locale and SHA-256.

## What this does not guarantee

- **It does not make compaction safe.** `SFileCompactArchive` reads names from
  the archive's own `(listfile)`, not from a text file handed to a reader. The
  three `imp.mpq` archives and the `pic.mpq` archives still have no internal
  listfile at all, so compaction still fails them with
  `ERROR_UNKNOWN_FILE_NAMES`. Whether writing a recovered listfile into an
  archive would make compaction succeed is **untested**, and it is a change to
  an archive's contents, which is a Phase 3 decision, not a Phase 1 one.
- **It does not make rebuilding an archive from a tree possible.** A member's
  name is part of its encryption key, and 21 members across the three profiles
  still have no name. More importantly, a rebuild must reproduce flags, locale
  and storage decisions per member, and nothing here addresses that. The
  copy-then-patch discipline in [deterministic MPQ repack](repack.md) stands
  unchanged.
- **It does not repair the weakened shape check.** `tools/mpq_shape.py` still
  compares unnamed members as a multiset. Recovered names reduce how many
  members fall under that weakening; they do not remove it, and the 21 that
  remain are permanently under it.
- **A recovered name's *case* is not measured.** MPQ name hashing is
  case-insensitive, so `LBM\ACTIONS5R3A.lbm` and `lbm\actions5r3a.lbm` are one
  name to the archive. The case printed is whatever the donor catalogue
  recorded. A successful open establishes the name; it establishes nothing about
  its capitalisation.
- **A confirmation is a 64-bit hash agreement, not a proof of the string.** Two
  different strings with identical `hashA` and `hashB` would be
  indistinguishable to the archive. The expected number of such collisions in
  this corpus is ~10⁻¹¹, which is why this is recorded as Observed rather than
  hedged — but it is a probabilistic argument, not a decoding.
- **The published catalogue is not independent evidence of a spelling.** It is a
  collection of names; where it is right, the archive confirms it. Where a name
  in it were subtly wrong, the archive would simply refuse it, which is the
  failure mode this method has — silence, not a wrong answer.
- **Name → block is not injective.** PIC5R3 holds two distinct members under the
  byte-identical name `portrait\AIpotM.lbm`, at blocks 1108 and 1144 and hash
  slots 628 and 627. Opening that name reaches one of them. Nothing here assumes
  a name identifies exactly one member, and nothing downstream should either.
- **It says nothing about the engine.** Not one recovered name has been put in
  front of `lomse.exe`. Naming a member does not make a replacement of it
  playable.
- **Everything above is one StormLib build on one machine.** StormLib 9.40 via
  Homebrew, macOS 26.5.2. The positional pseudo-name behaviour in particular is
  a property of StormLib, not of the MPQ format, and could change.

## Tests

```sh
python3 -m unittest discover -s tests
```

`tests/test_member_names.py` splits the two halves deliberately. The deciding
logic is tested against manifests and probe results describing archives that do
not exist, so it can pose the cases the corpus does not: a block two names both
claim, a name that reaches the right block through the wrong hash slot, a
pseudo-name that would otherwise confirm itself, a size or digest that
disagrees. The probe and the `--listfile` wiring are tested against real
archives the suite builds with `lom-mpq create`, including `--no-listfile`,
which exists **because** a fixture whose members are all already named would let
the wiring tests pass without the wiring doing anything.

Ten mutations of `tools/member_names.py` and three of `tools/lom_mpq.cpp` were
each applied and each failed the suite, in both directions where a constant has
one: the pseudo-name rule narrowed and widened, the control stride shifted each
way, each of the four confirmation terms removed in turn, pseudo-name hits
accepted, already-named members re-emitted into the listfile, an opened control
no longer failing the run, `--listfile` silently ignored, absent names reported
present, and `--no-listfile` ignored.
