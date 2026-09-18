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
   its ids are profile-specific. **See the caveat below: this class is an assumption, not a
   measurement, for two of the three profiles.**
6. `unclassified-residue` — none of the above.

Matching a name against the corpus's definitions is **case-sensitive**, because the VM resolves
names case-sensitively. An earlier version folded case, which made `GOLD` a `script-definition` on
the strength of `gs\barter.gs`'s `/gold` in the same run that reported `GOLD` unresolved in 75
members.

The `broad-candidate` column records whether the scanner's candidate rule admits the name: called,
never defined, and present as an ASCII string in the executable. `likely_engine_names` in
`src/main.rs` — the scanner this column reproduces — carried the same case fold and has been
corrected too, so the tool and this table now agree. The correction **raised the published
candidate totals**, because names like `GOLD` had been suppressed by a lowercase `/gold`:

| | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| broad candidates, case-sensitive | 2,160 | 2,263 | 2,198 |
| under the old case fold | 2,114 | 2,212 | 2,151 |
| recovered by the correction | 46 | 51 | 47 |

Every recovered name is an engine constant — `GOLD` (290 uses), `FOOD`, `CRYSTALS`, `WARRIOR`,
`WIZARD`, `TARGET_ARMY`, `CITY_OWNER` — which is exactly the class the filter was meant to surface.
`--scan-gamescript` reports the same deltas from an independently written code path.

The binary-string half of the rule stays case-folded in both places. It asks whether a name occurs
in the image at all, which case does not bear on; only the "does the corpus define this" half has
to agree with the interpreter.

## Counts per class per profile

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| script-definition | 10,305 | 10,901 | 12,355 |
| language-primitive | 56 | 56 | 55 |
| native-host-call | 1,383 | 1,414 | 1,391 |
| constant-or-data | 693 | 786 | 722 |
| engine-dictionary-key | 112 | 113 | 126 |
| unclassified-residue | 1,531 | 1,904 | 2,206 |
| **distinct executable names** | **14,080** | **15,174** | **16,855** |
| parsed members | 1,315 | 1,681 | 1,696 |
| operator-table entries | 1,906 | 1,906 | 1,906 |

## The same partition, restricted to the broad candidate list

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| language-primitive | 56 | 56 | 55 |
| native-host-call | 1,383 | 1,414 | 1,391 |
| constant-or-data | 689 | 760 | 717 |
| engine-dictionary-key | 0 | 0 | 1 |
| unclassified-residue | 32 | 33 | 34 |
| **broad candidates** | **2,160** | **2,263** | **2,198** |

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

Eleven 3.02 names moved *into* this class when three non-definition shapes were excluded from the
definition scanner — `button2_t`, `crystalsvaluestring`, `up_button_x` and similar, all dictionary
*values* that had been read as keys. One moved out: `exec` had a false definition site and is now
correctly `language-primitive`, which is why that row reads 56 rather than 55. The `script-definition`
total remains an upper bound; on 3.02 its measured contamination is 83 names of 12,979 (0.64%), of
which these fixes removed 12.

## Caveat: `engine-dictionary-key` is GS5R3-derived

`map::TERRAIN_SPRITE_TYPES` was dumped from a **GS5R3** script set by this repository's engine
probe. Applying it to the vanilla and 3.02 columns is therefore an **assumption, not a
measurement**: it assumes the engine's terrain-sprite registry uses the same *names* across the
three profiles. That assumption is weaker than it looks safe — the table's own documentation warns
that a different mod registers different types in a different order, and while only the names are
used here and not the profile-specific ids, nothing in this run establishes that vanilla's registry
holds the same set of names.

Concretely: the 112 vanilla and 113 patch302 rows in this class are *unverified* at the name level,
and the 126 GS5R3 rows are the only ones measured against their own profile. A name wrongly placed
here has moved out of `unclassified-residue` and nowhere else, so the error is confined to those two
classes and does not touch `native-host-call` or the candidate totals. Re-running the sprite-type
probe per profile would settle it.

## Files

- `vocabulary-vanilla.tsv`
- `vocabulary-patch302.tsv`
- `vocabulary-gs5r3.tsv`

Columns: `name`, `uses`, `class`, `broad-candidate`, `operator-entry-point`.
