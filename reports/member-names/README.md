# Recovered member names

Every name in these files was **confirmed by the archive it names**: the archive
opened the name, StormLib reported the block and hash-table slot, and the bytes
read by name digested to the same SHA-256 as the bytes read by block index. A
content match or a catalogue entry on its own is a proposal and is not here.

See [docs/member-names.md](../../docs/member-names.md) for the method, the
negative control, and what this does not guarantee.

## Files

Only **recovered** names are listed. A member the archive already names needs no
help and is not repeated here, so the line count of a file is the number of
members it rescues, not the size of the archive.

| File | Applies to | Names |
|---|---|---:|
| `vanilla-gs-recovered.txt` | vanilla `gs.mpq` | 363 |
| `vanilla-and-302-pic-recovered.txt` | vanilla and 3.02 `pic.mpq` (byte-identical archives) | 1,070 |
| `gs5r3-pic-recovered.txt` | GS5R3 `pic.mpq` | 408 |
| `all-profiles-imp-recovered.txt` | `imp.mpq`, all three profiles | 3,600 |
| `all-profiles-sndfx-recovered.txt` | `sndfx.mpq`, all three profiles | 1,880 |
| `all-profiles-special-recovered.txt` | `special.mpq`, all three profiles | 1,218 |

3.02 `gs.mpq` and GS5R3 `gs.mpq` have no file because nothing was recovered for
them: GS5R3's archive already names all 1,700 members, and 3.02's own listfile
already names all but the nine that no catalogue anywhere supplies.

A file shared between profiles is shared because the recovered set came out
identical, verified by SHA-256 of the sorted lists, not assumed from the
archives being similar.

## Use

```sh
.build/lom-mpq manifest /path/to/pic.mpq \
  --listfile reports/member-names/vanilla-and-302-pic-recovered.txt
.build/lom-mpq extract /path/to/pic.mpq /new/output/dir \
  --listfile reports/member-names/vanilla-and-302-pic-recovered.txt
```

The names are supplied to the reader; no archive is written. Each name is
re-resolved against the archive in front of it on every run, so a list applied
to the wrong archive loses names rather than mislabelling members.

## Regenerate

```sh
scripts/recover-member-names.sh ~/Applications artifacts/names-YYYYMMDD
```

That run also writes per-block **resolution** tables. Those are deliberately not
committed: they are keyed by block index, and a block index is a position that
moves when an archive is rewritten. Only the name lists are durable.
