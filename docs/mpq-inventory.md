# MPQ Inventory and Comparison

## Outcome

The baseline, community 3.02, and GS5R3 `gs.mpq` and `pic.mpq` archives have been extracted and compared without modifying any installed game profile.

The most useful conclusions are:

- Community 3.02 is a focused code patch. It adds three `.gs` files and makes token-level changes in thirteen existing scripts. Its `pic.mpq` is byte-for-byte identical to the Steam baseline.
- The 3.02 archive also recovers real names for 363 entries that are anonymous in the baseline listfile. Those are catalog improvements, not gameplay changes. **Confirmed directly 2026-09-18**: all 363 names open in the vanilla archive and return the bytes the block-index manifest recorded ([member names](member-names.md)).
- GS5R3 is a broad fork, not a small balance overlay. Its entry script loads many replacement `*5.gs` subsystems, and PIC5R3 adds or replaces hundreds of visual assets.
- Vanilla and 3.02 can be compared cleanly at script level. GS5R3 requires subsystem-level study because filenames, layout, code, and assets all changed together.

## Reproduce the inventory

Prerequisites:

```sh
brew install stormlib
```

Run from the repository root, choosing a new output directory each time:

```sh
scripts/inventory-installed-profiles.sh \
  ~/Applications \
  artifacts/run-20260911
```

The script builds `lom-mpq`, opens every source archive read-only, writes TSV archive manifests, extracts into the ignored `artifacts/` tree, and generates pairwise SHA-256 comparisons. It refuses to reuse an existing output directory so stale files cannot contaminate a result.

Useful lower-level commands:

```sh
scripts/build-tools.sh
.build/lom-mpq list /path/to/gs.mpq
.build/lom-mpq extract /path/to/gs.mpq /new/output/directory
```

Extracted copyrighted game data is intentionally excluded from Git. Only tools, documentation, and aggregate/generated findings belong in this repository.

## Archive measurements

`Named` includes the internal `(listfile)` where present. `Unknown` denotes StormLib placeholders such as `File00000372.xxx`, not a real file extension. `Unnamed after recovery` is what remains once every available catalogue has been pooled and confirmed against the archive — see [member names](member-names.md).

| Profile/archive | Bytes | SHA-256 | Entries | Named | Unknown | Unnamed after recovery |
|---|---:|---|---:|---:|---:|---:|
| Vanilla `gs.mpq` | 1,795,987 | `6b84ea4c…8196f86a` | 1,688 | 1,316 | 372 | 9 |
| 3.02 `gs.mpq` | 2,089,412 | `0f017a4c…6304437` | 1,691 | 1,682 | 9 | 9 |
| GS5R3 `gs.mpq` | 3,337,814 | `2d394279…0b8d095` | 1,700 | 1,700 | 0 | 0 |
| Vanilla `pic.mpq` | 68,777,986 | `d0df8b92…cc62ca8` | 1,071 | 0 | 1,071 | 1 |
| 3.02 `pic.mpq` | 68,777,986 | `d0df8b92…cc62ca8` | 1,071 | 0 | 1,071 | 1 |
| GS5R3 `pic.mpq` | 73,114,396 | `5c784a6a…078ccf` | 1,406 | 997 | 409 | 1 |

**Corrected 2026-09-18:** `Unknown` was previously counted by matching StormLib's `File%08u.xxx`
placeholder only. That undercounts, because the extension in a placeholder is a **guess from the
member's first bytes** and `.xxx` is only the fallback when StormLib cannot guess. `sndfx.mpq`
(1,880 entries) and `special.mpq` (1,218 entries) list entirely as `File00000000.wav`-style names in
all three profiles and were therefore easy to read as fully catalogued; they carry no internal
`(listfile)` at all and **none of their members had a name**. The same is true of `imp.mpq` (3,600
entries per profile). All three are now fully named by recovery.

Worse than a miscount: such a name is not merely a label StormLib prints, it is one StormLib
**resolves by block position**, ignoring everything after the digits — `File00000022.wavZQNOTREAL`
opens block 22. Any name-recovery or comparison work must exclude that shape, or it will confirm
block numbers as names. Measured, with the exact accepted form, in [member names](member-names.md).

GS5R3 `gs.mpq` identifies 1,696 `.gs` files, two text files, one URL, and its listfile. PIC5R3 identifies 994 LBM images, two BMP images, and its listfile. The remaining 409 picture entries lack names.

PIC5R3 contains two archive entries named `portrait\AIpotM.lbm`. Consequently, extraction produces 1,405 disk files from 1,406 logical entries. The manifest retains both entries; one extracted pathname necessarily overwrites the other.

**Corrected 2026-09-18:** this paragraph previously blamed "the default case-insensitive macOS filesystem". The two entry names are byte-identical, so the loss happens on a case-sensitive filesystem too; case-insensitivity is not the mechanism. The two entries are genuinely distinct members — SHA-256 `9dc00e94…` at block 1108 and `884f4bb9…` at block 1144, with different compressed sizes — measured with `lom-mpq manifest`, which reads members by block index. **Observed** separately: an MPQ cannot hold two members whose names differ only in case at all, because the name hash is case-insensitive; adding `A.txt` then `a.txt` leaves one member under the first name holding the second content.

## Pairwise results

Detailed generated summaries are available for [scripts](../reports/gs/summary.md) and [pictures](../reports/pic/summary.md). The comparer matches paths case-insensitively, hashes raw content, and lexically normalizes `.gs` code to distinguish formatting/comments from semantic token changes.

### Community 3.02 versus vanilla

The script archive comparison found. Individual filename pairings can be ambiguous when multiple entries have identical content, but the aggregate counts are stable:

- 1,311 byte-identical paths
- 363 byte-identical files whose anonymous vanilla slot acquired a real filename
- 3 new dialog scripts
- 13 existing `.gs` scripts with token-level changes
- 1 changed internal listfile
- no removed content after hash-based catalog reconciliation

The three additions are:

- `gs/Dlg/comb_dlg.gs`
- `gs/Dlg/opdlg.gs`
- `gs/Dlg/scroldlg.gs`

The thirteen changed scripts are:

- `START.GS`
- `gs/Dlg/NetGmDlg.gs`
- `gs/Dlg/NetSpDlg.gs`
- `gs/Dlg/infopan.gs`
- `gs/Dlg/newdlg.gs`
- `gs/Dlg/panels.gs`
- `gs/Dlg/sysdlg.gs`
- `gs/buttons.gs`
- `gs/hotkey.gs`
- `gs/makearmy.gs`
- `gs/modeinfo.gs`
- `gs/standard.gs`
- `gs/textdict.gs`

Token-level inspection corroborates the patch notes. Examples include removal of scripted CD checks from network dialogs, new button sizes and options variables, a one-token unit-table correction in `makearmy.gs`, additional hotkey/UI behavior, and new help/version strings. This also shows why line-based diffs are misleading: many vanilla scripts are one minified line, while 3.02 reformats them into readable source.

### GS5R3 versus vanilla and 3.02

GS5R3 shares only 114 byte-identical script paths with either baseline and has hundreds of direct modifications plus more than a thousand added/removed path identities. Some of that count reflects a reorganized source tree, but the entry script confirms genuine architectural replacement:

- script/dictionary limits are increased;
- new runtime options and logging flags are defined;
- numerous original subsystems are supplemented or replaced by `GAMEUTIL5`, `COMBAT5`, `AUTOCALC5`, `DIPLO5`, `DUNGEONS5`, `BRAIN5`, `MAKEARMY5`, and other `*5.gs` modules;
- quest scripts are reorganized into faith-specific `quest/` directories;
- PIC5R3 supplies the matching interface and portrait resources.

This evidence reinforces the profile policy: do not stack 3.02 and GS5R3. Treat each as a separate reference lineage for our future work.

## Tool behavior and limits

- `lom-mpq` uses Homebrew StormLib 9.40 and opens archives with `MPQ_OPEN_READ_ONLY`.
- The tool reads the archive's internal listfile into StormLib before enumeration, recovering names when the archive provides them.
- Extraction rejects absolute paths and `..` components.
- Extraction requires a new or empty destination and warns when an archive contains duplicate case-insensitive paths.
- Uncatalogued entries can be extracted and hashed, but their placeholder names are archive-slot labels only — and the slot moves, both between archives and when an archive is rewritten. `list`, `manifest` and `extract` accept `--listfile NAMES.txt` so recovered names can be supplied to the reader without writing to any archive; `lom-mpq probe-names ARCHIVE NAMES.txt` tests candidate names against an archive directly. See [member names](member-names.md).
- The earlier `.gs` lexical normalizer remains suitable for change triage. The newer bounded lexer tokenizes every named script across all three profiles and inventories definitions, calls, and static loads; it is still not a complete parser or proof of behavioral equivalence. See the [GameScript probe](gamescript-format.md).
- Archive repacking now exists as a verified, non-installing command: `lom-mpq repack` plus the manifest shape check in `tools/mpq_shape.py`, driven by `scripts/repack-archive.sh`. It produces byte-identical output across repeated runs on both GS5R3 archives and refuses any output that lost a member. It does **not** install, and no repacked archive has been put in front of the engine. See [deterministic MPQ repack](repack.md).
- `lom-asset-viewer --set-imp-placement` writes **loose** IMP files (identical length, re-parsed and read back before writing, non-overwriting).
- `lom-mpq manifest` addresses members by block index rather than by name, which is what makes the two same-named PIC5R3 entries visible. `lom-mpq create` exists to build small archives for tests and is not a mod packaging command.

## Next investigation

**Largely done** — the executable's operator dispatch table, its GameScript constant table at `0x00560108`, and individual natives (`map2screen` at `0x0046B0C0`, `getimphotspot` at `0x0049BF90`, `enumimphotspots` at `0x0049C1D0`) have all been read. Track the remainder under [issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5) and see [agent handoff](agent-handoff.md). Expand the new bounded stack/dictionary interpreter only as required for representative utilities, then use the focused 3.02 change set as the first annotated semantic corpus.
