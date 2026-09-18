# Gameplay Data Reference

## Read this first: do not publish a "maximum resistance is 100" table

In vanilla and 3.02, all eight unit magic resistances stop at **exactly 100**, across 88 units.
That is precisely what a hard engine cap looks like, and it is the single easiest wrong claim to
take out of this database.

**GS5R3 reaches 125 on air, life and water, and 150 on earth** — running on a `lomse.exe` that is
**byte-identical** to the other two profiles. The same engine accepts 150. So 100 is a design
convention of the vanilla data, not a bound the engine enforces.

Everything in `field-ranges.tsv` is a **corpus-observed maximum**. Not one number in it is an
engine-enforced bound, because nothing in this work asked the engine anything. The distinction is
the whole of [that section](#corpus-observed-maximum-versus-engine-enforced-bound), and this is the
case that proves it matters.

## Status

**A semantic symbol/index database over the GameScript corpus of all three profiles, with a query
path and a generated per-symbol reference.** This closes Phase 2's remaining item. Four of the six
kinds the roadmap named are well served; two are thin, for a reason that is itself a finding, and
[what this does not cover](#what-this-does-not-cover) says which is which before you rely on any of
it.

The database is generated, reproducible and committed:

| File | Rows | What it holds |
| --- | ---: | --- |
| [`reports/gameplay/symbols.tsv`](../reports/gameplay/symbols.tsv) | 1,535 | one row per symbol: kind, evidence class, profiles, defining member, line, byte offset, display name and its source, reference count, anchor |
| [`reports/gameplay/fields.tsv`](../reports/gameplay/fields.tsv) | 62,212 | one row per symbol per field per profile |
| [`reports/gameplay/references.tsv`](../reports/gameplay/references.tsv) | 83,259 | every static reference, with the member and line it occurs in |
| [`reports/gameplay/field-ranges.tsv`](../reports/gameplay/field-ranges.tsv) | 1,348 | per kind and field: presence, observed range, modal value, shape mix |
| [`reports/gameplay/profile-diff.tsv`](../reports/gameplay/profile-diff.tsv) | 1,535 | presence per profile and the difference class for each pair |
| [`reports/gameplay/renames.tsv`](../reports/gameplay/renames.tsv) | 294 | symbols renamed between profiles, matched by identical token fingerprint |
| [`reports/gameplay/reference.md`](../reports/gameplay/reference.md) | 1,535 headings | the generated per-symbol reference, one stable anchor each |

**Why TSV.** `reports/gs/` and `reports/natives/` already work this way; the files stay diffable in
Git so a regeneration is a reviewable change; and `.gitignore` already excludes `reports/**/*.csv`
while tracking `.tsv`, so the repository had already chosen aggregate TSV as the committed form.

No script text is in any of it. A field whose value is a procedure, dictionary or array is recorded
as its shape and token count, never its body.

## Reproducing it

```sh
cd spikes/asset-viewer
cargo run --release --example gameplay_symbols -- \
  --profile vanilla  --gs '/path/vanilla/English/gs.mpq' \
  --profile patch302 --gs '/path/302/English/gs.mpq' \
  --profile gs5r3    --gs '/path/gs5r3/English/gs.mpq' \
  --listfile ../../artifacts/reference-listfiles/lords-of-magic.txt \
  --out ../../reports/gameplay
```

## Querying it

The query path reads the committed TSV and opens no archive, so it answers on a checkout with no
game installed.

```sh
lom-asset-viewer --gameplay-symbol aicav            # by internal code
lom-asset-viewer --gameplay-symbol Windriders       # by display name -- same symbol
lom-asset-viewer --gameplay-symbols-like 'Windrider*'
lom-asset-viewer --gameplay-symbols-like 'summon_creature_*'
lom-asset-viewer --gameplay-symbol potion_health --reports path/to/reports/gameplay
```

### Both names are searched, and the result says which one matched

A visitor knows "Windriders"; the corpus knows `aicav`. Both are searched, case-insensitively, and
every hit carries a `matched` column of `code`, `display-name` or `code+display-name` — a result
nobody can explain is a result nobody can trust.

```text
name    display-name  matched       kind  evidence         profiles                  member
aicav   Windriders    display-name  unit  delimited-block  vanilla,patch302,gs5r3    units\aicav.gs
```

**Multiplicity means two different things, and the two are not treated alike.**

- Several symbols sharing a **code** are all the answer. `potion_health` really is registered as
  both an artifact and a spell, so both records print in full.
- Several symbols sharing a **display name** are a question, not an answer. Sixteen spells are
  labelled "Dispel Magic". The query lists the candidate codes and prints no record, because
  printing sixteen would bury the fact that the query did not identify one.

An **exact code match always wins outright**, so `aicav` can never be made ambiguous by some other
symbol merely being *called* "aicav".

A miss states how many symbols were searched and offers names containing the text, so "no such
symbol" is a statement about a known index rather than an unbounded claim about the game.

### Display-name coverage is 1,439 of 1,535 — not all of them

Search the display name alone and you miss 96 symbols. Every listing therefore prints
`rows-with-no-display-name-to-match`, so a thin result is explainable rather than mysterious.

| Where the display name comes from | Symbols |
| --- | ---: |
| `declared-name` — a text `/name` in the record | 933 |
| `encounter-key` — an encounter's `/key "Air Cave"` | 450 |
| `text-table` — `/name textdict /T_… get` resolved against the corpus's text tables | 56 |
| none | 96 |

The `display-source` column carries this per row. The 96 without are 74 encounters whose `/key` is
computed rather than written, 12 buildings, 8 factions and 2 artifacts — all kinds that have no
human-facing label in GameScript at all.

`encounter-key` and `text-table` were both **added after a review found the search unusable** —
before that, only the 933 `declared-name` symbols were findable by the name a person would type.
`text-table` exists only because a field reader bug was fixed on the way; see
[the field reader defect](#a-field-reader-defect-of-my-own-fixed).

## How a symbol is classified

Not by directory. Three of the kinds are registered by a **named engine operator whose own name
states the kind**, which is the strongest evidence the corpus offers short of running the engine:

```text
/bolt_fire "gs/spells/FIRE/bolt_fire.gs" define_spell def
```

One token triple carries the symbol name, the kind, and the defining member. A directory rule would
agree with this nearly everywhere and be unfalsifiable where it did not; the registrar rule can be
wrong out loud, because a member nothing registers produces no symbol. The integration test
`the_registrar_rule_is_not_the_directory_rule_wearing_a_disguise` fails if the two rules ever stop
disagreeing, which is what keeps the choice honest rather than decorative.

Every record carries an **evidence class**, in the record itself and not only in this prose, because
the six kinds do not share an evidence level and someone querying one symbol must not have to
remember which:

| Evidence class | Meaning | Kinds | Count (union) |
| --- | --- | --- | ---: |
| `registered-by-operator` | `/name "member" define_spell def`. The operator names the kind. **Observed.** | spell, artifact | 769 |
| `delimited-block` | `begin_unit_definition` … `end_unit_definition`, bound by the following `/name exch def`. **Observed.** | unit | 168 |
| `catalog-entry` | reached by `"member" run` from a dungeon catalog; membership is Observed, the *kind* is **Inferred** from the catalog's identity | encounter | 578 |
| `engine-constant` | never script-defined, used as a selector; the definition lives outside GameScript. **Inferred.** | faction | 8 |
| `call-site-tuple` | positional arguments at a `define_building_levels` call site. **Inferred**, and see the caveat below. | building | 12 |

## Per-profile totals

| Kind | vanilla | 3.02 | GS5R3 |
| --- | ---: | ---: | ---: |
| unit | 157 | 157 | 166 |
| spell | 192 | 192 | 262 |
| artifact | 179 | 179 | 250 |
| encounter | 306 | 306 | 314 |
| faction | 8 | 8 | 8 |
| building | 12 | 12 | 12 |
| **total** | **854** | **854** | **1,012** |

### Accuracy and completeness, measured separately

A second extractor was written in Python, over the raw extracted bytes, using regular expressions
and its own comment stripper — no shared code with the Rust tokenizer. Comparing the two, per kind
per profile, over all four well-served kinds:

| | Value |
| --- | ---: |
| symbol identities both instruments agree on | **2,660** |
| only the database has | **0** |
| only the independent extractor has | **3** |

So **accuracy is 100.00%** (nothing in the database is absent from an independently derived set) and
**completeness is 100.00%** for spells, artifacts and encounters in all three profiles, and
99.4% for units. The three residual names are the *same* name in each profile, `lastunittype`, and
it is a **false positive of the validator**: `units\easyunit.gs` writes `/lastunittype exch def`
inside a procedure body, as a local capturing the last defined unit type. The database is right to
exclude it. These are reported as two numbers rather than one because an easy validation set can
hide a systematic failure, and this one did not — but only because the disagreements were each run
down by hand rather than summarised.

### What the instrument could not reach

Stated because "we found none" is only as strong as the search. Each count below comes from a **raw
byte scan** that shares no mechanism with the tokenizer.

- **Vanilla excludes 373 archive entries by extension**, because its listfile cannot name them. A
  raw byte search finds `begin_unit_definition` in **12** of them and no spell or artifact marker in
  any. Those 12 are now admitted on the strength of the byte marker, which recovered the unit
  `boat` — defined nowhere else in the archive. 3.02 and GS5R3 have 10 and 4 excluded entries, with
  **zero** markers between them.
- **GS5R3 ships a stale catalog.** `gs\dungeons.gs` lists 312 encounter paths, of which **266 no
  longer exist in its own archive**: GS5R3 reorganised the tree into per-faith subdirectories and
  catalogs them from `gs\DUNGEONS5.gs` instead. Reading one hardcoded catalog found 46 of 314
  encounters and reported the rest as missing dependencies. Every member is now scanned for `run`
  edges and the union taken. The 266 dead edges are reported, not hidden.
- **Symbol name collisions are reported, not resolved.** 18 in vanilla and 3.02, 1 in GS5R3. They
  are real: `units\gate.gs` binds three units in one member; `units\licr3old.gs` and
  `units\holdchwmi.gs` are superseded copies binding live names; GS5R3's `units\test.gs` binds
  `deldr`. The database keeps one row per (kind, name) and names every loser on the generator's
  stdout. **Eight different members bind `gate` in vanilla**, and the database shows one of them.

## Annotated examples

### unit — `aicav`, "Windriders"

`units\aicav.gs`, line 1, 40 fields, 27 static references.

```text
/aicavm 0{"licav"mount_imp_filename}0 addmounttype def begin_unit_definition
/name"Windriders"def
/code CAV def
/flags UNITTYPELAND CAN_ATTACK or CAN_DEFEND or CAN_BERSERK or def
/race ELF def
/faith AIR def
...
end_unit_definition
/aicav exch def
aicav MOVE acmov setunittypesound
```

How to read it. The block delimiters are engine operators; everything between them is the record.
`/flags ... def` is **one** field whose value is a five-token expression, not four fields — a reader
taking the first token as the value would report `or` and `CAN_DEFEND` as fields of their own. The
block leaves a value on the stack and `/aicav exch def` binds it; **that binding, not the filename,
is the symbol's name**, and it is what the 27 references (`aicav MOVE acmov setunittypesound`, and
`gs\unittype.gs`'s catalog) use. The definitions *after* the binding are not part of the unit.

### spell — `bolt_fire`, "Flame Dart"

Registered by `gs\spells.gs` as `/bolt_fire "gs/spells/FIRE/bolt_fire.gs" define_spell def`;
defined in `gs\spells\FIRE\bolt_fire.gs`; 31 fields; 40 static references.

```text
/name"Flame Dart"def /faith FIRE def /image 1 def /mode COMBAT_SPELL def
/target TARGET_ARMY def /mana 2 def /level 1 def /research_cost 8 def
/maintenance 3 def /range -1 def /barter_value_proc{80}def /book SPELL_CATEGORY_ATTACK def
```

How to read it. `/mana 2 def` is a scalar and carries a range; so do `level`, `research_cost`,
`maintenance` and `range`. `/barter_value_proc{80}def` does **not** — its value is a procedure, and
although this one happens to push a constant, the neighbouring `gs\spells\FIRE\_immolation.gs`
writes

```text
/duration{ismyside?{600}{600 vulnerability_factor mul}ifelse}def
/temporary_modifications << /fire_resistance{...} /water_resistance{...} >> def
```

where the duration is a different number depending on whose side the target is on. The database
records both as `procedure` and `dictionary` with a token count and contributes them to *no* range;
reporting a duration for that spell would be inventing one. `/temporary_modifications` is one field,
and its inner keys (`/fire_resistance`, `/water_resistance`) are **not** fields of the spell.

### artifact — `amulet_demon`, "Demon's Torch"

Registered by `gs\artifact.gs`; defined in `gs\artifact\FIRE\amulet_demon.gs`; 34 fields; 6 static
references.

```text
/image 2 def /faith FIRE def
/name"Demon's Torch"def /portrait_code"fiamul"def
/description_table << /articon_luck"E" /articon_magic_resistance"+25" ... >> def
/val_A{0 artifactarmy artifactunit israngedunit?{5 ...
```

How to read it. The `/val_A`…`/val_E` procedures supply the numbers the `description_table`'s letter
slots interpolate, so the displayed "+4 Movement" is computed at runtime and is not a field value.
The database records `description_table` as one dictionary-shaped field; the artifact's real numeric
surface is in those procedures and is **not** extracted.

### encounter — `air/aicave`

Reached by `"gs/dungeons/air/aicave.gs" run` from `gs\DUNGEONS5.gs`; 37 fields; 0 static references.

```text
40 dict begin
/key"Fire Cave"def
/name{ /dummy begin T_Cave currentterrainsprite -1 gt {...} end }/dummy currentdict replace
    /names[T_Cave T_Cave T_gen_cavern ...]replace bind def
/faith FIRE def
```

How to read it. An encounter has **no script-level bound name** the way a unit does, so its path
below `gs/dungeons/` is its identity — `air/aicave`, not `aicave`. That is not cosmetic:
`earth/encounter11` and `hidden/encounter11` are different encounters. Vanilla's 306 encounters have
only **247 distinct basenames**, so a basename rule silently merges **59** of them — which is what
happened, until the collision report made it visible. Note also that `/name` here
is a **procedure**, not text: the encounter computes its display name from the terrain sprite it
sits under, so `display-name` is correctly empty rather than showing a procedure's token count.

**Zero static references is expected for every encounter**, and is the reason all 577 unreferenced
symbols in the union index are encounters and every non-encounter symbol has at least one. An
encounter is reached by *path*, through `run`, never by name. Its incoming edge is recorded in the
`registered-in` column instead.

### faction — `FIRE`

No defining member. 372 static references in vanilla.

The eight faiths are recovered from the corpus's own `/faith` field values across units, spells and
artifacts — a measured set, not a list written into the tool — and they are exactly
`AIR CHAOS DEATH EARTH FIRE LIFE ORDER WATER`. Nothing in any profile defines them, so they are
engine constants: the name is real and everything else about a faith lives in `lomse.exe`. The only
field the database carries is how many symbols declare it.

### building — `BARRACKS`

`gs\building.gs`, **byte offset 10,795**, 3 fields, 52 static references.

```text
KEEP 0 3{keep_name}buildingdict /strongholdbeginturn get{pop -1} "KEEP"define_building_levels
WIZARDS_TOWER 0 3{pop"Mages's Tower"}buildingdict /wizardstowerbeginturn get{pop -1} "WIZT"define_building_levels
```

How to read it, and why this is the weakest record in the database. There is no record file and no
registrar. The callee's own prologue names its parameters — `/code /dd_func /turn_func /name_func
/last_lvl /first_lvl /bt`, popped in reverse — but **nothing here verifies the caller pushes them in
that order**, and one of the three "procedure" operands is not braced at all (`buildingdict
/strongholdbeginturn get` is three loose tokens). A strict positional walk would misalign. Only the
parts that survive are taken: the type name, the two level bounds, and the four-character code
immediately before the operator. The three procedures are deliberately **not** recorded rather than
recorded wrongly.

Note the byte offset. Vanilla's and 3.02's `gs\building.gs` is a single 14,336-byte line with **no
line terminator at all**, so every building in those profiles is honestly at line 1 and the line
number locates nothing. 501 GS5R3 members have the same shape. The offset is the column to use.

## The three-profile comparison

`profile-diff.tsv` carries presence per profile and, for each pair, a difference class. Formatting is
separated from content the same way [`tools/compare_trees.py`](../tools/compare_trees.py) separates
them for trees, and for the same reason Phase 1 required it — but computed from this crate's lexer
rather than that tool's tokenizer, which matters (see [below](#a-defect-in-toolsgs_syntaxpy)).

### 3.02 changes no gameplay record at all

| | patch302 vs vanilla |
| --- | ---: |
| identical | **1,535** |
| formatting-only | 0 |
| token-level | 0 |
| present in one only | 0 |

Every one of the 1,535 symbols has a byte-identical defining member in vanilla and 3.02. That is not
a null result from a broken comparison: 3.02 does modify 14 members, and they are
`START.GS`, `gs\standard.gs`, `gs\textdict.gs`, `gs\buttons.gs`, `gs\hotkey.gs`, `gs\makearmy.gs`,
`gs\modeinfo.gs` and six `gs\Dlg\*` members. **None of them defines a unit, spell, artifact or
encounter.** Evidence class: Observed.

So the answer to the roadmap's "compare vanilla behavior with 3.02 fixes", at the level of gameplay
data, is that **3.02 is an interface, hotkey, text and standard-library patch and leaves the
gameplay tables untouched**. Anything 3.02 changed about how the game plays is in engine-facing
script, not in the records catalogued here.

### GS5R3 rewrites them extensively

| | gs5r3 vs vanilla |
| --- | ---: |
| identical | 26 |
| formatting-only | 2 |
| token-level | 303 |
| only in vanilla/3.02 | 523 |
| only in GS5R3 | 681 |

**Read the last two rows with care.** They are not 1,204 additions and removals. GS5R3 renamed and
reorganised its spell and artifact sets wholesale — vanilla's `lightning_spll` in
`gs/spells/lightning.gs` is GS5R3's `bolt_air` in `gs/spells/AIR/bolt_air.gs` — so a diff keyed on
name alone reports one rename as one removal plus one addition.

`renames.tsv` recovers the pairs a name comparison cannot, by matching the **token fingerprint of the
defining member**: **294 pairs** whose files are token-for-token identical under two different names
or paths. Most are encounter moves (`death/altosac` → `death/quest/altosac`). This is evidence of a
rename, not proof of one — two encounters could always have been written identically.

It is also a **lower bound, and the largest known gap in this comparison**: a symbol that was both
renamed *and* edited has no identical fingerprint and is invisible to it, which is exactly the case
for the renamed spells. How many of the 523/681 are edited renames is **not established here**.

## Identifiers, references, ranges, defaults and limits

`field-ranges.tsv` reports, per kind and field: how many symbols write it, how many do not, how many
wrote a plain number, the observed minimum and maximum, the modal numeric value and its count, the
distinct-value count, and the mix of value shapes.

Two things it deliberately does not do. A field whose value is a procedure or a multi-token
expression has **no single value**, and contributes to the shape mix but to no range. And a
non-finite number is never summarised — see [the `INF` defect](#a-defect-in-the-shared-lexer).

### The modal value is not a default

The mode is the most common value written, which is a *default* only if the engine also treats it as
one. Nothing here establishes that. `field-ranges.tsv` names the column `modal-value` rather than
`default` for that reason. A field's true default is whatever the engine uses when the record omits
it, and that is not observable from scripts.

### Corpus-observed maximum versus engine-enforced bound

**These are different claims and conflating them is the main way this deliverable could go wrong.**
Everything in `field-ranges.tsv` is the former. Not one number in it is an engine-enforced bound,
because nothing in this work asked the engine anything.

The corpus itself proves the distinction matters. Unit magic resistances, `min`/`max` over each
profile:

| Field | vanilla | 3.02 | GS5R3 |
| --- | --- | --- | --- |
| `air_resistance` | −50 … **100** | −50 … **100** | −100 … **125** |
| `earth_resistance` | −50 … **100** | −50 … **100** | −100 … **150** |
| `life_resistance` | −50 … **100** | −50 … **100** | −100 … **125** |
| `water_resistance` | −200 … **100** | −200 … **100** | −100 … **125** |
| `fire_resistance`, `chaos_`, `death_`, `order_` | −50 … **100** | −50 … **100** | −100 … **100** |

Read vanilla alone and all eight resistances stop at exactly 100, across 88 units — which looks
exactly like a hard cap, and a reader would have written it down as one. **GS5R3 reaches 125 and
150.** And the three installs differ **only** in `gs.mpq`; `lomse.exe` is byte-identical across all
three. So the same engine accepts 150, and **100 is a design convention of the vanilla data, not an
engine-enforced bound**. Evidence class: Observed — this one is settled, and settled in the
direction that refutes the tempting inference.

Applying the same care to the other candidate limits:

| Candidate | Observed | Status |
| --- | --- | --- |
| resistances capped at ±100 | vanilla 100 max; GS5R3 150 | **Refuted** as an engine bound, by a profile sharing the same executable |
| `armor` ≤ 99 | max 99 across 166 GS5R3 units, 25 distinct values | **Inferred, weak.** 99 is suggestive of a two-digit display field, but 99 is also simply the largest value anyone wrote |
| spell `research_cost` ≤ 256, `range` ≤ 256 | max exactly 256 in GS5R3 | **Unknown.** 256 is suspicious, but the corpus cannot distinguish a bound from a round number an author liked |
| spell `level` ∈ 0…9 | 4 distinct values, 228 of 262 spells at level 1 | **Unknown.** The narrow distribution is about spell design, not about what the engine accepts |
| unit `military_units` ∈ 1…3 | 3 distinct values over 59 units | **Unknown** |

**An engine-enforced bound would have to be shown in the operator bodies.** `operator_bodies.rs` and
the recovered arity tables in [gamescript-format.md](gamescript-format.md#operator-arity-recovered-from-the-code)
are where such a check would live — a comparison and a clamp inside `setunittypedata` or its
neighbours. No such search was run here, so **no limit in this document is claimed as
engine-enforced**, and the one limit that looked most like a cap turned out not to be one.

## What this does not cover

### Well served

**units, spells, artifacts, encounters** — 1,515 of the 1,535 indexed symbols. Each has a defining
member, a field table, a static reference list, and a per-profile diff. Accuracy and completeness
are measured against an independent extractor above.

### Thin, and why

**factions (8 symbols, 1 field each).** A faith is an engine constant. Nothing in any profile defines
`FIRE`; scripts only *select* on it. The eight names are recovered from the corpus and are certainly
the complete set, but everything a faith *is* — its starting resources, its unit roster, its AI
temperament — lives in `lomse.exe` or is distributed across the unit and spell records that name it.
There is no faction record to extract because **GameScript does not have one**. That is a finding,
not a gap in the extractor.

**buildings (12 symbols, 3 fields each).** Likewise: there is no building record file and no
registrar. Building *types* are engine constants and their per-level data reaches the engine as
positional arguments to `define_building_levels` and `setbuildingrequirements`. The database recovers
the type name, the level bounds and the four-character code, and deliberately refuses the three
procedure operands because they cannot be aligned reliably (see [the worked
example](#building--barracks)). Upgrade costs, tile setups and requirements are **not** extracted.
Recovering them properly means reading `setbuildingrequirements`' 37 call sites with a stack model,
which is VM work rather than scanner work.

### Other boundaries

- **The database holds one row per (kind, name).** Where a name is bound by several members —
  `gate`, eight times in vanilla — only one is shown, and the rest are named on the generator's
  stdout rather than in the TSV.
- **Text is not joined up.** `gs\text\unit\*.gs` and `gs\text\building\*.gs` hold the displayed blurbs
  and are not linked to their symbols.
- **Nothing here was executed.** Every field value is a static reading of a token span. A record
  whose value is computed at load time is recorded as a procedure and its value is unknown.
- **Artifact and spell numeric effects live in procedures** (`/val_A`, `/temporary_modifications`,
  `/duration`) and are outside the extracted surface. The database says a spell has a duration; it
  does not say what it is.
- **The rename analysis only finds unedited renames** (294). Renamed-and-edited symbols are
  uncounted.
- **`engine-constant` and `call-site-tuple` are Inferred**, and every row says so.

## A field reader defect of my own, fixed

Found while closing the search gap, and worth recording because the symptom was invisible: the
records looked complete, they were just missing the one field a human reads.

The corpus's text-table idiom is

```text
/name textdict /T_artifact_name_adventsword get def
```

The field scanner stops at a literal name appearing before the terminating `def`, because
`/invoke_spell cvx` defers a *native call* and a scanner that ran past it would swallow the rest of
the member into one value. That guard is right, but it also fired on the inner `/T_artifact_…`,
which is not a deferred call — it is a dictionary key about to be read. Two things went wrong at
once: the record **lost its `name` and `description`**, and the *key* was filed as a field of its
own whose value was `get`.

The idiom is not rare — **246 definitions in vanilla and 3.02, 416 in GS5R3**, including 56 `name`
and 48 `description` in vanilla alone.

The scanner now passes a literal name when it is immediately followed by a key-consuming operator.
That list is `["get"]` and nothing else, because every one of those 246 and 416 definitions uses
`get`; adding `known`, `load` or `undef` on the grounds of plausibility would admit shapes the
corpus does not contain, and every name admitted there is a name the deferred-call guard stops
protecting. Two corpus-gated tests hold both halves: no looked-up key may become a field, and
`invoke_spell` may never become one either. Evidence class: Corrected.

Resolving those recovered `/name` values against the corpus's text tables is what supplies the 56
`text-table` display names. Keys that two members define **differently** are dropped rather than
resolved to whichever was read last — 230 to 243 per profile — so no symbol gets a name on the
strength of archive order.

## Two pre-existing defects found on the way

Both are outside this work's scope and are **not fixed here**; both affect other published figures.

### A defect in the shared lexer

`gamescript.rs` classifies a token as a number when `name.parse::<f64>()` succeeds. Rust's
`f64::from_str` accepts `inf`, `infinity` and `nan`, case-insensitively. The shipped infantry unit
code is the literal `INF`:

```text
/code INF def
```

So eight units per profile have `code` lexed as **floating-point infinity** rather than as an
executable name, and the naive range over that field reads `inf … inf`. This module guards against
it locally — a non-finite number keeps its text and its shape but is never summarised, and
`a_non_finite_number_token_is_never_summarised_as_a_value` fails if the guard is removed. The lexer
itself is untouched, because its token counts are load-bearing for the vocabulary work. Anything
else in the repository reading `TokenKind::Number` has the same exposure. Evidence class: Observed.

### A defect in `tools/gs_syntax.py`

`tokens()` terminates a `;` comment at the next `\n`:

```python
if character == ";":
    flush()
    newline = source.find("\n", index)
    index = len(source) if newline == -1 else newline + 1
```

But [bare CR is a line ending in this corpus](gamescript-format.md#line-endings-bare-cr-is-a-line-ending-here),
and 25 GS5R3 members contain a `;` comment and **no LF at all**. In those members the first comment
swallows the rest of the file. `gs\dungeons\water\wacave.gs` is 5,347 bytes and normalises to **six
tokens**. Measured 2026-09-18.

This is the exact failure `gamescript-format.md` already warns about — "a reader that splits on LF
sees a single comment swallow dozens of live statements" — surviving in a tool the warning did not
reach. It feeds `tools/compare_trees.py`'s token hash, so the **"Layout/comments only" column** in
[`reports/gs/summary.md`](../reports/gs/summary.md) is unreliable for those members: two files
differing only inside a swallowed region hash equal and are called formatting-only. It is why the
profile diff here is computed from the Rust lexer instead. Evidence class: Observed.

## Tests

- `src/gameplay_symbols.rs` — 39 unit tests. Fixtures are written to probe shapes the corpus does
  **not** contain as well as ones it does: a repeated top-level key, a value of two numbers, an
  unclosed procedure, a decoy `/decoy bind def` before the real unit binding, a literal followed by
  `put def` rather than `get def`. A suite built only from corpus-shaped input cannot fail on what
  the corpus happens never to do.
- `tests/gameplay_symbols.rs` — 7 corpus-gated tests (`LOM_GS_MPQ`, `#[ignore]`d), passing on all
  three profiles. None compares a count to a constant; each states a property the corpus must have
  if the rule is right. One fails if the registrar rule and the directory rule ever stop disagreeing;
  another fails if any looked-up dictionary key becomes a field name.
- **Mutation run: 49 mutations, 49 caught, 0 survivors**, and every mutation compiled and ran —
  a mutation that fails to build is not a caught mutation. Constants mutated in both directions
  (minimum→maximum and maximum→minimum; `== 1`→`>= 1` and `== 1`→`== 2`; marker search forced to
  both `true` and `false`; code-only and display-name-only search; each `MatchedField` swapped for
  each other). The script is in the session scratchpad and is not committed.
