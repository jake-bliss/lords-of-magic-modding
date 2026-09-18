# GameScript vocabulary classification

Derived from the user's local installations by
`spikes/asset-viewer/examples/gamescript_vocabulary.rs`. Only names, use counts and classes are
recorded here; no `.gs` contents are stored in Git.

Each distinct executable name in a profile's `gs.mpq` is placed in exactly one class, in the order
the interpreter resolves:

1. `script-definition` — some member of the corpus defines it, so a dictionary lookup finds it first.
2. `language-primitive` — `gamescript_vm` implements it as language rather than game state.
3. `native-host-call` — that profile's `lomse.exe` registers it in the operator table.
4. `constant-or-data` — SCREAMING_CASE and absent from the operator table.
5. `engine-dictionary-key` — a key in a dictionary the engine owns, recognised against this crate's
   recovered terrain-sprite registry (`map::TERRAIN_SPRITE_TYPES`). Only the table's names are used;
   its ids are profile-specific.
6. `unclassified-residue` — none of the above.

Matching a name against the corpus's definitions is **case-sensitive**, because the VM resolves
names case-sensitively. An earlier version folded case, which made `GOLD` a `script-definition` on
the strength of `gs\barter.gs`'s `/gold` in the same run that reported `GOLD` unresolved in 75
members.

The `broad-candidate` column records whether the older heuristic (called but never defined, and
present as an ASCII string in the executable) admitted the name, so the two can be compared row by
row. It reproduces that heuristic **including its case fold**, deliberately: a repaired classifier
is only worth comparing against a faithful baseline. That is why `GOLD` is not a candidate.

## Counts per class per profile

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| script-definition | 10,317 | 10,913 | 12,368 |
| language-primitive | 55 | 55 | 54 |
| native-host-call | 1,383 | 1,414 | 1,390 |
| constant-or-data | 693 | 786 | 722 |
| engine-dictionary-key | 112 | 113 | 126 |
| unclassified-residue | 1,520 | 1,893 | 2,195 |
| **distinct executable names** | **14,080** | **15,174** | **16,855** |
| parsed members | 1,315 | 1,681 | 1,696 |
| operator-table entries | 1,906 | 1,906 | 1,906 |

## The same partition, restricted to the broad candidate list

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| language-primitive | 55 | 55 | 54 |
| native-host-call | 1,383 | 1,414 | 1,390 |
| constant-or-data | 643 | 709 | 670 |
| engine-dictionary-key | 0 | 0 | 1 |
| unclassified-residue | 32 | 33 | 33 |
| **broad candidates** | **2,113** | **2,211** | **2,148** |

`script-definition` is zero in this table by construction: the heuristic already excludes every
name the corpus defines. About 98.5% of the candidate list is real; roughly 33 names per profile
are coincidences of the string filter.

## What `unclassified-residue` is not

It is **not** a false-positive list, and an earlier version of this file said it was. Its
highest-use rows are genuine script definitions whose definition sites the scanner still cannot
see:

| Name | Uses | Defined in |
| --- | ---: | --- |
| `build_statement` | 340 | `gs\text.gs` |
| `set_level_modifications` | 279 | `gs\levlmods.gs` |
| `getdungeonstrength` | 204 | `gs\placedng.gs` |

A further 113 rows in 3.02 are engine terrain-sprite dictionary keys, now split out into
`engine-dictionary-key`. Shrinking this class is work for the definition scanner, not the operator
table. Every row discussed here is `broad-candidate = no`, so none of it bears on the 98.5% figure
above.

## Files

- `vocabulary-vanilla.tsv`
- `vocabulary-patch302.tsv`
- `vocabulary-gs5r3.tsv`

Columns: `name`, `uses`, `class`, `broad-candidate`, `operator-entry-point`.
