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
5. `heuristic-false-positive` — none of the above.

The `broad-candidate` column records whether the older heuristic (called but never defined, and
present as an ASCII string in the executable) admitted the name, so the two can be compared row by
row.

## Counts per class per profile

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| script-definition | 10,365 | 10,967 | 12,417 |
| language-primitive | 55 | 55 | 54 |
| native-host-call | 1,383 | 1,414 | 1,390 |
| constant-or-data | 647 | 734 | 675 |
| heuristic-false-positive | 1,630 | 2,004 | 2,319 |
| **distinct executable names** | **14,080** | **15,174** | **16,855** |
| parsed members | 1,315 | 1,681 | 1,696 |
| operator-table entries | 1,906 | 1,906 | 1,906 |

## The same partition, restricted to the broad candidate list

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| language-primitive | 55 | 55 | 54 |
| native-host-call | 1,383 | 1,414 | 1,390 |
| constant-or-data | 643 | 709 | 670 |
| heuristic-false-positive | 32 | 33 | 34 |
| **broad candidates** | **2,113** | **2,211** | **2,148** |

`script-definition` is zero in this table by construction: the heuristic already excludes every
name the corpus defines. About 98.5% of the candidate list is real; roughly 33 names per profile
are coincidences of the string filter.

## Files

- `vocabulary-vanilla.tsv`
- `vocabulary-patch302.tsv`
- `vocabulary-gs5r3.tsv`

Columns: `name`, `uses`, `class`, `broad-candidate`, `operator-entry-point`.
