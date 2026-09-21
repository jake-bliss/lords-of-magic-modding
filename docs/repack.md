# Deterministic MPQ repack

## What this is

One command that takes a source archive and a set of replacement members, writes a new archive, and
**refuses to hand it on unless the output still has the source archive's shape**. Nothing here
installs anything. Installation into a development profile is Phase 3 and is deliberately not
implemented; `--install` exists only so that asking for it produces a refusal instead of a surprise.

This replaces hand-running `spikes/asset-viewer/examples/mpq_replace.rs`, which modifies an installed
archive **in place** and has no verification step at all. That probe is still wired into
`scripts/install-engine-probe.sh` — the attended engine probe deliberately edits an installed archive
under the backup/restore discipline — so it has not been deleted, but nothing new should use it.

## Where the command lives and why

The repack verb went into `tools/lom_mpq.cpp`, the existing StormLib tool, rather than into the Rust
crate or a new script. That tool already opens archives, loads the internal listfile, and enumerates
members; it was the only component that needed a write path added. Putting the writer anywhere else
would have made a *fourth* way to touch an archive (`lom-mpq`, `lom-asset-viewer`, `mpq_replace`,
and a new one) with a fourth set of behaviours around listfiles and flags.

The shape check went into `tools/mpq_shape.py`, separate from the writer, because it is pure logic
over two manifests: it can be unit-tested against archives that do not exist, mutation-tested, and
run without StormLib. It never opens an archive.

`scripts/repack-archive.sh` is the driver that puts them in the only safe order.

| Piece | Job |
|---|---|
| `lom-mpq manifest ARCHIVE` | every member's block index, hash index, size, compressed size, flags, locale and SHA-256 |
| `lom-mpq repack SRC OUT --replace 'NAME=FILE'` | write a new archive; never touches the source |
| `lom-mpq create OUT [--add 'NAME=FILE']` | build small archives for tests; **not** a mod packaging command |
| `tools/mpq_shape.py` | compare two manifests, exit nonzero if the output lost shape |
| `scripts/repack-archive.sh` | manifest → repack → manifest → check → refuse or report |

## How to run it

```sh
brew install stormlib          # once
scripts/repack-archive.sh \
  /path/to/source/gs.mpq \
  artifacts/repack/gs.mpq \
  'gs\hotkey.gs=artifacts/edited/hotkey.gs'
```

Add `--determinism-runs 5` to repack five more times into a throwaway directory and report whether
every run produced the same bytes. A mismatch makes the whole command exit nonzero.

Inputs are named explicitly. The command discovers nothing: it will not look inside `~/Applications`
for a source, and it refuses outright to write anywhere under `~/Applications`.

The lower-level commands are usable on their own:

```sh
.build/lom-mpq manifest /path/to/gs.mpq > source.tsv
.build/lom-mpq repack /path/to/gs.mpq out.mpq --replace 'START.GS=edited/START.GS'
.build/lom-mpq manifest out.mpq > out.tsv
python3 tools/mpq_shape.py --source source.tsv --output out.tsv --expect-changed 'START.GS'
```

## What the shape check proves

The check compares the **source archive** with the **output archive**, member by member, before
anything is installed. It reports:

- **Member count.** 1,700 in, 1,700 out.
- **Every name, with multiplicity.** A dropped member, an added member, or a member whose name came
  back in different case is a failure. The case-folded pair is reported as one named finding rather
  than an unexplained missing/added pair.
- **Every unchanged member, proven unchanged.** Same SHA-256, same size, same flags, same locale.
  Compressed size may move; it is a property of the packer, not of the content.
- **Every changed member, reported with old and new size and digest** — and only the members the
  repack *declared* it would change are allowed to differ. An undeclared difference is a failure.
- **A declared replacement that changed nothing is a failure**, unless the repack declared the
  opposite. A repack that silently did nothing must not pass as a successful repack, and that rule
  needs an escape hatch for the one case where nothing changing is the point: a codec proving it
  can reproduce a shipped member byte for byte (`mods/imp-cursor-noop`, `mods/audio-welcome-noop`).
  `--expect-unchanged NAME` (`scripts/repack-archive.sh`) inverts the expectation for exactly that
  member — the failure moves to the content changing, not to it staying the same — and it is a
  DECLARATION, not an inference: at the mod-tree level (`scripts/mod-build.sh`), naming a member in
  `mod.toml`'s `expect_unchanged` list is the only way `tools/mod_build.py`'s `entry_kind`
  classifies it `unchanged` rather than `replace`. A member that is byte-identical to its base but
  not named there is still classified `replace`, so this rule still fires for it — see
  `tools/mod_build.py`'s own comment on why that was, briefly, not true.
- **A declared addition may not already exist in the source, and must appear in the output.**
  `--expect-added NAME` (`scripts/repack-archive.sh`'s `--add`) declares a member the repack is
  ADDING rather than replacing; every other member absent from the source and present in the
  output is a failure, and a name on this list that did not appear in the output is a failure too.
- **A declared replacement may not change how a member is stored.** Content may change; flags and
  locale may not. The same rule applies to a declared no-op: it may come back byte-identical, but
  not under a different storage class.

**Observed 2026-09-18:** the manifest addresses members by **block index**, not by name, using
StormLib's `File%08u.xxx` pseudo-name. This is what lets the check see the PIC5R3 loss class. PIC5R3
holds two entries under the single name `portrait\AIpotM.lbm`, with **different content**
(`9dc00e94…` at block 1108, `884f4bb9…` at block 1144) and different compressed sizes. A check built
from extracted files, or from names alone, sees one member where the archive holds two. Block-index
addressing was validated against name-based extraction on all 1,700 members of GS5R3 `gs.mpq`: 1,700
matched, 0 mismatched.

**Corrected:** `docs/mpq-inventory.md` attributed the 1,406-entries-to-1,405-files extraction gap to
"the default case-insensitive macOS filesystem". The two entry names are byte-identical, so the loss
happens on any filesystem; case-insensitivity is not the mechanism. Separately, **Observed**: an MPQ
*cannot* hold two members whose names differ only in case — the name hash is case-insensitive.
Adding `A.txt` then `a.txt` yields one member, keeping the first name and the second content.

### The one exemption

`(listfile)` is exempt from the content check, and nothing else is. StormLib regenerates it on every
write (48,469 bytes → 48,457 on GS5R3 `gs.mpq`), so its bytes cannot survive a repack. That is safe
to waive only because the listfile's job — naming members — is checked directly: a name it lost would
turn that member into a `File%08u.xxx` slot in the output manifest and surface as a missing member
plus an added one. Its flags and locale are still checked. An `(attributes)` member, which carries
timestamps, is **not** exempt; if one appears, that is reported as an added member and refused.

The exemption only ever applies to an archive that *has* a `(listfile)`. **Observed 2026-09-18**:
repacking `pic.mpq`, which has none, does **not** cause StormLib to create one. The entry count
holds at 1,071 before and after, and no `(listfile)` appears in the output. So an archive that names
none of its own members stays that way through a repack, and its shape check has nothing to waive.

### Naming members of an archive that names none

`pic.mpq`, `imp.mpq`, `sndfx.mpq` and `special.mpq` carry no `(listfile)` and **no self-named member
at all**, so every member lists under the `File%08u.xxx` pseudo-name synthesised from its block
index. `repack` therefore takes `--listfile NAMES.txt`, and without it those four archives cannot be
repacked at all. Both ways round fail, **Observed 2026-09-18**:

- naming a real member is refused, because the storage-flags map built from the source archive has
  no such key;
- naming the pseudo-name reaches `SFileAddFileEx`, which rejects it with **StormLib error 22**. The
  pseudo-name is a read-side convenience that resolves by *block position*; nothing hashes it into
  the hash table, so there is nothing for a write to replace.

> ### ⚠️ Both of those refusals are about REPLACING, not ADDING
>
> **Observed in a local binary, 2026-09-21.** Adding a name the archive has **never held** works,
> and was measured directly against the pristine `pic.mpq`:
>
> ```
> lom-mpq repack pic.mpq OUT.mpq --listfile NAMES.txt --add 'portrait\LIWMTP00.LBM=donor.lbm'
> ```
>
> - the add succeeded, inheriting storage flags `0x00010100` from every member of the source;
> - `probe-names` resolves the new name **through the archive's own hash table** — the lookup Storm
>   performs — at block index 1071, hash index 653;
> - the member reads back **byte-identical** and inspects as a valid `70x67` IFF-PBM;
> - a full manifest comparison shows **all 1,071 originals unchanged** — block index, hash index,
>   size, compressed size, flags, locale *and* sha256 — with exactly one member added.
>
> So "an archive that names none of its own members" constrains *replacement*, which needs a
> storage-flags key for an existing name, and constrains the `File%08u.xxx` pseudo-name, which
> nothing hashes. Neither constrains a genuinely new name. `imp.mpq` — the same class of archive —
> had already shown this at the **engine** layer: an added member was tolerated (rung 5,
> 2026-09-20) and read by name by a script (2026-09-17). No `pic.mpq` with an added member has yet
> been put in front of the engine.

A recovered name does hash to the member's existing hash-table entry, which is what makes the
replacement a replacement rather than an addition. `scripts/repack-archive.sh` passes the same list
to the repack **and to both manifests** — naming one side and not the other would compare two
different addressings of one archive.

## The determinism actually measured

**Observed 2026-09-18**, StormLib 9.40 via Homebrew, macOS 26.5.2, M4 Max:

| Input | Runs | Result |
|---|---|---|
| GS5R3 `gs.mpq` (3,337,814 B, 1,700 members), replacing `gs\hotkey.gs` | 6 | all `4acb46fe9411187f32601b09e47b8bcc73d8e7d41f667339b71eb1ea3cba585f` |
| GS5R3 `gs.mpq`, replacing `START.GS` | 6 | all `5973b3d6348fe86ae6e93e5e07252d17f68b22e3ec043d0bde3ab3d36a82ee5f` |
| GS5R3 `pic.mpq` (73,114,396 B, 1,406 members), replacing `LBM\ACTIONS5R3A.lbm` | 5 | all `c10b485049bb5ee42eaddb504c78a6a6b9a58949f42735a050548267c2bdfb87` |

Byte-identical output also survived, on GS5R3 `gs.mpq`:

- a different modification time on the replacement file (`199701010000` vs today);
- the replacement file living at a different path;
- a different `umask` (077) and a different `TZ` (`Asia/Tokyo`) and `LC_ALL`;
- the two `--replace` arguments given in the opposite order — the tool sorts them before applying.

So the claim is **byte-level determinism, measured** on both real archives and on fixtures, not
merely "the same members came back". A fixture archive's bytes are pinned as literals in
`tests/test_repack_archive.py`, so a StormLib upgrade that changes the output will fail a test rather
than pass silently.

Two things make that possible and should be understood as conditions, not guarantees:

- The archive is produced by **copying the source file and replacing members in it**, not by building
  a new archive from an extracted tree.

  This used to be justified by "409 PIC5R3 members have no name, and a member's name is part of its
  encryption key". **That premise is stale as of PR #66** and is corrected here rather than quietly
  dropped: name recovery leaves 21 unnamed members in the whole corpus, and PIC5R3's own remainder
  is 1, not 409 ([member names](member-names.md)). The conclusion survives for reasons recovery does
  not touch -- 21 members still have no name at all, `name -> block` is not injective (PIC5R3 holds
  two distinct members under one byte-identical name), and a rebuild must reproduce each member's
  flags, locale and storage decisions, which nothing here addresses.
- **Compaction is off by default.** The replaced member's old data stays in the file as dead space.
  `--compact` is available and also deterministic (three runs identical on `gs.mpq`), but
  `SFileCompactArchive` **fails with `ERROR_UNKNOWN_FILE_NAMES` (10007) on any archive containing
  unnamed members** — measured on PIC5R3 `pic.mpq`, and it applies equally to vanilla `gs.mpq` with
  its 372 unnamed entries. Compaction is not a step this pipeline can rely on.

## What this does not guarantee

- **It does not prove the game runs the output** in general, though the evidence is now broad.
  Four attended sittings have put archives this writer produced in front of `lomse.exe`, covering
  **all five archives** the pipeline targets: `gs.mpq` (2026-09-16, 2026-09-18, and a size-changing
  edit 2026-09-19), `pic.mpq` (2026-09-18, and the full ByteRun1 encoder at a length that shrank
  2026-09-20), `sndfx.mpq` and `special.mpq` (2026-09-19 — the **STORED** `0x80010000` class, two
  members each in one build), and `imp.mpq` (2026-09-20, including a member the archive never had).
  Both storage classes are covered. Shape preserved ≠ playable, and the per-archive limits that
  remain are rendered in [the roadmap](roadmap.md) from `tools/engine_acceptance.py`.
- **It does not validate content.** A syntactically broken `.gs` file passes the shape check
  perfectly. Content validation is Phase 3.
- **A replaced member is rewritten by StormLib, not reproduced.** Its compressed bytes are whatever
  StormLib produces for the declared flags. For `MPQ_FILE_COMPRESS` members the method used is PKWARE
  DCL — an **inference** about what a 1997 engine reads, not a measurement. Engine-verified
  replacements now cover `MPQ_FILE_IMPLODE` members of `gs.mpq`, `pic.mpq` and `imp.mpq`, and
  **STORED** members of `sndfx.mpq` and `special.mpq`.
- **Determinism is measured, not proven.** It holds for this StormLib build on this machine for these
  inputs. Nothing in StormLib's contract promises it. The pinned-hash test is the tripwire.
- **It says nothing about archives it has not seen.** Every number above is from GS5R3. Vanilla and
  3.02 archives have not been repacked.
- **Ordering of same-named members is untested.** Two members sharing a name (the PIC5R3 case) are
  sorted by block index so the manifest is stable, but no fixture can reproduce that case — StormLib
  will not create it — so that tie-break is exercised only by the real `pic.mpq`.
- **The `--install` path does not exist.** It refuses. There is no development profile, no rollback,
  and no install verification yet.
- **An archive member whose name contains `=` cannot be addressed.** `NAME=FILE` splits at the first
  `=`. No member name in the 3,106 GS5R3 `gs`/`pic` entries contains one.

## Tests

```sh
python3 -m unittest discover -s tests
```

`tests/test_mpq_shape.py` covers the comparison logic against archives that do not exist: a dropped
member, a case-folded name in both directions, two members sharing one name, a zero-byte member, an
archive with no members at all, metadata-only drift, and a rewritten listfile. `tests/test_repack_archive.py`
runs the real tool end to end on archives it builds itself, including the refusal paths and the
`~/Applications` guard. It skips if StormLib is unavailable.
