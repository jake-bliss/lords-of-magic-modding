# GameScript Language and Runtime Probe

## Status

**Lexical, vocabulary and language milestones complete; the engine's operator tables and their arity are recovered from the binary; module loading works; the VM executes shipped script and stops, traceably, at the engine boundary.** The native Rust tool tokenizes every named `.gs` member in the preserved baseline, 3.02, and GS5R3 archives, inventories names and static `run` references, and correlates executable tokens with strings embedded in each profile's `lomse.exe`. The interpreter implements 68 language primitives plus GameScript's two non-PostScript features — procedure locals and typed dictionary keys — and **1,551 of the corpus's 4,692 `.gs` members (33.1%) now execute to completion with no engine support at all**, up from 912 (19.4%). The candidate vocabulary is partitioned against the operator table rather than guessed at. See [the execution census](#the-execution-census) for what the remaining two thirds are waiting on; the short answer is that it is **not** the 1,906-operator host API.

This is the first Stage 2 preservation-engine result. It establishes that the source language is tractable enough for a bounded parser and experimental interpreter. It does **not** prove that the native host API, simulation, or complete game can be reproduced economically.

No scripts, executable contents, or other proprietary game data are stored in Git. The figures below are metadata produced from the user's local installations.

## Observed lexical model

The corpus supports this initial token model:

| Form | Current interpretation | Evidence |
| --- | --- | --- |
| `name` | Executable name | Repeated postfix/stack-oriented call sites |
| `/name` | Literal name | Used for definitions and dictionary keys |
| `"text"` | String | Used for UI text, paths, and file names |
| integer/decimal text | Number | Accepted using Rust's `f64` lexical syntax for inventory purposes |
| `{` and `}` | Procedure delimiters | Nested procedure bodies throughout the corpus |
| `[` and `]` | Runtime array/mark operators | Shipped scripts do not always balance them lexically |
| `<<` and `>>` | Runtime dictionary operators | Usually paired, but not safe to validate as source nesting |
| `;` through end of line | Comment | Human-readable 3.02 and GS5R3 sources use this form |

Definitions terminate with `def`, not with `;`. The GS5R3 corpus contains 77,391 bare `def` tokens
and 6,462 `}def` sequences, and zero occurrences of `}` followed by `;`. A community post implying a
`;` terminator is quoting its own trailing comment; see [community research](community-research.md).
Key/value pairs written as `/name { ... }` without `def` are dictionary-literal entries inside
`<< ... >>`, not definitions.

The corpus's dominant closure and static-data idiom is
`X /dummy <dict> replace bind def` — 7,472 uses of `replace` in GS5R3 alone. The VM already
special-cases it; it belongs in the grammar model rather than only in the runtime notes.

Strings do not use C-style backslash escaping. A backslash is preserved as an ordinary byte; this is required to tokenize the shipped `char_array` definition and Windows-style paths correctly.

The lexer records byte offset, line, and column for every token. It rejects an unterminated string or an empty literal name, but it does not reject unmatched procedure braces. A few shipped fragments are incomplete or intentionally loaded as snippets, so brace problems are retained as explicit diagnostics while the rest of the archive remains analyzable.

This is a corpus-derived lexer, not a complete language specification. It does not yet model executable versus literal arrays, object types, name lookup, binding, numeric edge cases, or evaluation semantics.

Surviving community documentation independently describes GameScript as a PostScript-derived, stack-oriented engine language. That history is useful context, but the implementation here relies on locally observed syntax and behavior rather than treating the community description as a formal specification.

## Three-profile corpus result

Measured read-only on 2026-09-12 with the same public listfile used by the asset inventory:

| Metric | Baseline | 3.02 | GS5R3 |
| --- | ---: | ---: | ---: |
| Archive entries | 1,688 | 1,691 | 1,700 |
| `.gs` members | 1,315 | 1,681 | 1,696 |
| Tokenized | 1,315 | 1,681 | 1,696 |
| Empty `.gs` members | 9 | 12 | 7 |
| Source bytes | 4,455,555 | 5,098,841 | 10,058,269 |
| Tokens | 557,649 | 633,525 | 1,149,348 |
| Comments | 1 | 843 | 17,007 |
| Distinct executable names | 14,081 | 15,176 | 16,856 |
| Distinct literal names | 14,160 | 15,162 | 17,641 |
| Maximum observed procedure depth | 19 | 19 | 20 |
| Procedure-brace anomalies | 4 | 6 | 1 |
| Static `run` reference edges | 650 | 789 | 1,081 |
| References resolving inside the same archive | 648 | 788 | 813 |
| Executable/binary name candidates | 2,071 | 2,167 | 2,091 |

The GS5R3 source corpus is approximately twice the baseline by bytes and contains substantially more names and static load references. Its 268 unresolved references are not 268 proven missing runtime dependencies: 266 originate in `gs\dungeons.gs`, which acts as a catalog of optional dungeon script paths. The scanner therefore calls these **unresolved static references**, not errors.

All three archives tokenize with zero fatal failures. The procedure diagnostics are reproducible and concentrated in a few shipped files:

- baseline: three unmatched openings in `gs\dlg\lib_dlg.gs` and one in `gs\dlg\speedop.gs`;
- 3.02: the same four plus one each in root-level `profile.gs` and `timing.gs`;
- GS5R3: one unexpected closing brace in `gs\combat\physical_impairments\crack_armor_defender.gs`.

## Native host API candidate heuristic

For each profile, the scanner:

1. counts executable and literal names across all scripts;
2. extracts lowercase printable ASCII strings of at least two bytes from that profile's `lomse.exe`;
3. retains executable names present in the binary;
4. excludes names also defined literally by scripts.

The result is a useful **candidate vocabulary**, not a built-in count. It includes language primitives such as `if`, `get`, `exch`, and `bind`, and can contain false positives from unrelated binary strings. Conversely, a built-in stored in a non-ASCII table or exposed dynamically would be missed. The roughly 2,100 candidates bound the first host-API cataloging pass but must be classified by call sites and runtime experiments before estimating VM completion.

## Confirmed native host names

Cross-checking never-script-defined executable names against `lomse.exe` strings on 2026-09-16
promotes a first set from candidate to **confirmed native**, with GS5R3 call counts:

`getdifficultylevel`, `setdifficultylevel`, `getmultiplayerflag`, `getplayeraistatus`,
`gamerand` (3,968), `addsoundfx` (3,587), `additem` (2,996), `getspelldefdata` (2,017),
`editbox` (1,924), `strcpy` (1,862), `getarmydata` (1,761), `setunittypesound` (1,692),
`getunitdata` (1,676), `strcat` (1,569), `getuniteffectivedata` (1,284), `doodad` (1,190),
`getspellinfo` (1,070), `button` (918), `getunittypedata` (817), `closedialog` (753),
`getcurrentartifactdata` (749), `currentuser` (689), `getbuildingdata` (645), `currentplayer` (563),
`currentturn` (533), `addterrainspritetype` (532), `getplayerdata` (527), `incombat` (522),
plus `enumunits`, `enumarmies`, `tickalarm`, `invoke_spell`, and `addunitmodifier`.

The native constants `EASY_LEVEL`, `MEDIUM_LEVEL`, and `HARD_LEVEL` are likewise never
script-defined. GS5R3 adds a script-side `/INSANE_LEVEL 3 def`, implying native values 0, 1, 2.

**The classifier now requires a definition shape. Implemented 2026-09-16.** Presence as a `/literal`
never meant a name was script-defined: `gs\artifact\_custom\sword_enlightenment.gs:53` contains
`/invoke_spell cvx`, which defers a *native* call, and `removeunitmodifiers` is the same trap. The
old heuristic excluded any name appearing as a literal anywhere, hiding genuine host calls.

`GameScriptAnalysis::definition_names` counts only literals in a definition position:

- `/name <value-or-procedure> ... def` where the `def` is reached at the same nesting depth, within
  three tokens of anything, or further out across only the operators that *finish* a definition
  (`bind`, `dup`, `put`, `dict`, `array`, `string`, `replace`, `currentdict`, `begin`, `end`).
  That covers `/NAME{...}def`, `/INSANE_LEVEL 3 def`, `/a exch def`, and the two procedure-local
  attachment forms `/NAME{...}dup 0 N dict put bind def` and `/NAME{...}/dummy N dict replace bind
  def`. Any other operator at that depth ends the statement, so `/invoke_spell cvx` is still not a
  definition;
- `/name <value>` directly inside a `<< >>` dictionary literal, which is how scenario tables such as
  `gs\scenario\default.gs` declare entries — where the `<<` must be the **innermost** enclosing
  group, and the name must sit at an **even** offset from it.

Three shapes are explicitly *not* definitions, each confirmed against a local archive before being
excluded. Evidence class: Corrected.

| Shape | Corpus example | Why it is not a definition |
| --- | --- | --- |
| the name is consumed immediately | `fonts\balloon.gs`: `/CopperplateGothicBT-BoldCond pop /gridsize[16 14]def`; `gs\diplo.gs`: `/i undef /majorrace?{4 lt}bind def` | `pop` and `undef` discard the name; the `def` belongs to the next statement |
| a dictionary *value* read as a key | `<< /a /value /b 1 >>` | `/value` is `/a`'s value; key positions are the even offsets |
| a literal nested inside a dictionary | `gs\actvrect.gs`: `/xdict << /left{/x parentrect /x get def} … >>` | the innermost enclosing group is the procedure, not the dictionary |

**The residual, measured rather than asserted.** A fourth shape remains: a literal that is an
operand of a later operator, as the second `/x` in `/x parentrect /x get def`. Separating it from
the genuine leading `/x` needs the operand arity of `parentrect`, a native whose arity this project
does not have, so it is left in and bounded instead. On 3.02: of 12,979 names the pre-fix rule
admitted, **83** (0.64%) had no sound definition site anywhere in the corpus; fixing the three
shapes above reclassified **12** of them, and the rest are this residual. Of the **1,131** names
the definition-window widening newly admitted, **6** were bad-shape-only — so the widening itself
was ~99.5% sound. The 10,902 script-definition figure is still an upper bound; it is an upper bound
whose error is now measured at well under one percent rather than unknown.

Measured on GS5R3: 17,641 distinct literal names and **14,917 definitions**, so 2,724 literals were
never definitions. The native-candidate count is **2,199**. Two corrections moved it from the
13,609 definitions and 2,151 candidates that the flat three-token rule produced: the widened rule
recognised definitions the window had missed, which removes candidates, and the case-fold
correction added back 48 engine constants that a lowercase definition of the same word had been
suppressing. The net is +44. Both are described below and both are labelled Corrected. The scan reports `distinct-definition-names` alongside the
literal count, and `LOM_CANDIDATE_LIMIT` raises the 50-line display cap for cataloguing the full
vocabulary.

Name matching against this set is **case-sensitive** wherever it is compared with names the VM
resolves, because `GameScriptVm` resolves case-sensitively. Folding is a real defect and not a
cosmetic one: nothing in 3.02 defines `GOLD`, `gs\barter.gs` defines `/gold`, and one fold moved 54
engine-constant names into the script-defined class.

Recommended first VM stubs, all pure reads of game state with small return types:
`getdifficultylevel`, `getmultiplayerflag`, `getplayeraistatus`, `getarmydata`, `getunitdata`,
`getuniteffectivedata`. Stubbing those six lets the `gs\LEVLMODS5.gs` AI-bonus block execute
deterministically against a synthetic state.

`bind` is not cosmetic and is not optional. It captures, inside a procedure, every name that
*currently* means a language primitive, so a later `def` of the same name cannot change what the
body does. `START.GS:135` relies on exactly that to wrap module loading in a progress meter:
`/run{ ... run}...bind def`, where the trailing `run` must stay the primitive. Evidence class:
Observed in the corpus.

A shipped member demonstrates that scripts can shadow native names: `START.GS:76` redefines `run`
itself. `gs5_globals.gs` ships a 50-line constant table intended to *"supplement, add or replace EXE
variables"*, but its `run` is **commented out** at `START.GS:34`, so it is not live behavior.

## The engine's operator tables

The candidate heuristic above infers the host API from the script side. The binary states it
directly. `lomse.exe` registers every native operator in a table of eight-byte records, each a
pointer to the operator's NUL-terminated name followed by a pointer to its implementation.
`--scan-natives` recovers it:

| Table | File offset | Entries | First names |
| --- | --- | ---: | --- |
| Interpreter primitives | `0x15bd20` | 104 | `pop`, `def`, `undef`, `begin` |
| Game operators | `0x15f120` | 1,804 | `cameraposition`, `cameraorientation`, `lightorientation`, `ambientlight` |

**1,908 records, 1,906 distinct names, each with an entry-point address.** This is the host API
itself, not a bound on it.

Recovery is structural, not fitted: a record is accepted only when its first dword resolves to a
string inside the image and its second lands in an executable section. Unrelated data satisfies
those constraints by coincidence — the shipped binary has locale tables of exactly that shape — so
names must additionally be lexable as GameScript executable names. That rule comes from our own
grammar, not from tuning against this binary, and it reduces the result to exactly the two tables
above with no length threshold applied.

### What this settles about the candidate vocabulary

Reconciling the GS5R3 candidate vocabulary against the table (re-measured after the definition-shape
widening, the case-fold correction and the three rejected shapes; the figures were
2,151 / 1,445 / 671 / 35 before all three):

| Class | Count | Reading |
| --- | ---: | --- |
| Confirmed operators | 1,446 | present in the table with an entry point |
| SCREAMING_CASE | 718 | engine **constants** pushed by name, not operators |
| Remainder | 35 | see below |
| **Candidates** | **2,199** | |

The heuristic's stated weakness — *"can contain false positives from unrelated binary strings"* — is
now measured rather than assumed. Two thirds of the vocabulary are confirmed procedures, and almost
all of the rest are a category the heuristic could not distinguish: **constants are not operators**,
so they are absent from the table by construction rather than by error. The constant count rose by
47 as the net of both corrections; the case-fold fix alone recovered 48 names for GS5R3 — `GOLD`,
`FOOD`, `CRYSTALS` and the rest — of which 47 are SCREAMING_CASE. Those were always constants and
were always absent from the operator table, but they had been absent from the *candidate list* too,
which is the part that was wrong.

The 35-name remainder is the heuristic's actual error bar. Twenty are `Type_*` engine type tags
(`Type_Imp`, `Type_Font`, `Type_EditBox`) that `is_screaming_case` does not match because they are
mixed case. The other fourteen are short, low-use names (`e1`, `uf`, `hh`, `jx`, `rx`, `xp`, `ice`,
`log`, `no`, `building_type`) that look like dictionary keys our definition-shape classifier does not
recognise as definitions. That is a **precision limit of the classifier**, not evidence of engine
surface, and it is the tightest bound we have on it: roughly 1.6% of candidates.

### Operator arity, recovered from the code

Every operator reaches the operand stack through one inlined idiom. The interpreter context arrives
as the first argument and three of its fields matter:

| Offset | Meaning |
| --- | --- |
| `+0x50` | base of the operand array, entries being an eight-byte `(tag, value)` pair |
| `+0x54` | current index, which counts **down** as values are pushed |
| `+0x58` | the limit index, compared against `+0x54` to detect underflow |

A pop increments the index and stores it back; a push decrements it and stores it back. Counting
those commits from each entry point recovers stack effect, and `--scan-natives` reports it per
operator as `pops`, `pushes` and a confidence column. The `pops` column **undercounts** operators that pop through the shared helper at `0x0040ADB0` — see below.

Three complications had to be handled, each found by a prediction disagreeing with a known answer:

- **The adjustment is not adjacent to the commit.** The shipped `pop` writes an error slot between
  `inc eax` and the store, so pattern-matching on adjacent instructions misses it. The register is
  tracked through the block instead — loaded from the field, adjusted, stored back — and any other
  write to it abandons the tracking.
- **The compiler also spells the adjustment `lea ecx,[eax+1]`.** The comparison operators use that
  form. Recognising only `inc`/`dec` reported `gt`, `ge`, `lt`, `le` and `div` as one-operand.
- **Results are usually pushed by a shared helper**, `0x0041d1d0`, which takes `(tag, value)` and
  pushes once. A body-only walk reported `add` and `sub` as pushing nothing. Calls are followed one
  level, and each distinct callee contributes once however many sites reach it.

#### Measured accuracy, and what the confidence column really means

Checked against 24 operators whose arity follows from PostScript semantics, plus
`getdifficultylevel`, which we had already established independently: **23 of 24 agree.**

The single disagreement is `mul`, reported as pushing twice. It has two push sites on mutually
exclusive type paths — the shared helper at `0x004cae73` for one operand type and an inline commit
at `0x004cae98` for the other. Each path pushes one result; a static count sees both. **So the
numbers are site counts, and they equal arity only when every commit lies on one path.**

**And the count is not a safe upper bound either: it undercounts.** Operators that take their
operands through the shared pop helper at `0x0040ADB0` commit the stack index inside the helper, so a
walk that does not follow it sees no pop at all. Three operators have been caught this way and
corrected against the binary: `drawimpframe` takes **six** operands rather than the five reported,
`map2screen` takes **three**, and `getimphotspot` takes **five** rather than one. Treat a reported
`pops` figure as a lower bound, and read the entry point before designing anything around an
operator whose disassembly reaches `0x0040ADB0`.

Completeness is a separate question from correctness, and the two must not be conflated:

| Confidence | Operators | Meaning |
| --- | ---: | --- |
| `well-formed` | 118 | the walk reached every instruction and explained every store |
| `indirect-branch` | 1,781 | the walk met a computed jump it cannot follow |
| `unclassified-store` | 7 | a store to the index that the idiom does not explain |

Most operators dispatch on operand type through a jump table — `jmp [table + eax*4]` — whose arms are
unreachable to a static walk, so stack traffic behind them is invisible. `getarmydata` is the
concrete case: it pops two operands and pushes its result from inside a helper that dispatches that
way, so the walk reports **0 pushes** for an operator that plainly returns a value.

**All 24 validation operators are flagged `indirect-branch`, and 23 of them are still right.** The
flag is conservative: it marks *possible* incompleteness, not probable error, and should be read as
"do not trust this without checking" rather than "this is wrong".

An earlier version of this analysis reported 1,875 walks as well formed. That was wrong. It treated
an unfollowable computed jump as an ordinary end of block, so walks that had silently given up were
counted as complete, and `getarmydata` was reported `well-formed` with a missing push. Detecting
indirect branches, and inheriting them from followed callees, is what turned that silent wrong
answer into a flagged one.

### A call site that looks like a counter-example has to be read, not counted

The recovered table gives stack effect. It cannot say what the operands *are*, and it cannot resolve a
call site whose operands are themselves computed. `addterrainsprite` is the worked example.

Most shipped sites read as three operands:

```
s_x s_y terrainsprites /tower3 get addterrainsprite
x y esp03 addterrainsprite
xy_to_x_y terrainsprites begin dirtpil end addterrainsprite
```

Several read as four, with a faith in the middle:

```
cx cy f terrainsprites begin keep_ttype end addterrainsprite
currfaithx currfaithy currfaith keep_ttype addterrainsprite
```

They are the same call. `keep_ttype`, `leader_ttype` and `great_temple` are **procedures** in the
`terrainsprites` dict that consume the faith and return one type id — `gs\tree.gs` line 178:

```
/great_temple{/dummy exch get}/dummy great_temple_array replace bind def
```

So `f terrainsprites begin keep_ttype end` is a single value by the time the operator sees it, and
`addterrainsprite` takes **three** operands: `x y ttype`. The engine probe already emitted that form;
this is what turned it from a working guess into a checked fact.

The general rule: when call sites disagree about a shipped operator's operand count, resolve the
names before concluding the operator is variadic. A name looked up through `begin`/`end` may be a
procedure rather than a constant.

`tools/gs_callsites.py` does this scan as a command rather than by hand, and flags the trap
automatically. Point it at an extracted corpus:

```sh
.build/lom-mpq extract "<game>/gs.mpq" /tmp/gsx
PYTHONPATH=tools python3 tools/gs_callsites.py /tmp/gsx addterrainsprite
```

It groups call sites by the token window before the operator and cites each with
`file:line:column` — shipped lines run to thousands of characters, so the column is not optional.

Three things it does that a `grep` cannot, each of which was getting a real answer wrong:

- **A procedure literal is an operand.** `currentplayer{...}enumplayerarmies` takes two operands.
  Stopping the window at the `}` reported a blank one, which reads as "takes none" — for an
  operator two of our probes are built on. The walk matches the brace, prints the procedure as
  `{...}`, and carries on to the operand in front of it.
- **A cut window says so.** A window that hit the token limit rather than a real boundary is
  printed with a leading `...` and a warning. Before that, the `make_custom_random_map` call site
  this repository's operand order rests on printed with `map_width` silently missing.
- **Names are classified against the corpus, in both directions.** Names defined as procedures are
  listed, because a procedure in an operand window means the token count is not the operand count.
  Names defined *nowhere* are listed too, because those are engine operators with stack effects of
  their own: `terrainsprites /tower3 get` is three tokens and **one** operand, and only flagging
  corpus-defined procedures left that case silent.

A `/name` counts as a definition only when a `def` follows within a few tokens. `/sprite_type get`
looks up a dictionary key and a dictionary literal is full of `key{procedure}` pairs; recording
those as definitions makes a name look known and stops it being reported as an unresolved operator.

Run on `addterrainsprite` it flags `keep_ttype`, `leader_ttype`, `great_temple`, `polar` and
`terrainsprites` as procedures, and `get`, `begin`, `add`, `copy` and `xy_to_x_y` as engine
operators, without being told about any of them.

### Other operand orders read from shipped call sites

All **Observed in the archive**, each with its call site:

| Operator | Operands | Call site |
| --- | --- | --- |
| `newmap` | `width height` | `gs\generate.gs` — `64 64 newmap`; `gs\maplib.gs` — `mapw maph newmap` |
| `savescenariomap` | `filename` → result | `gs\hotkey.gs` 562 — writes `.scn`; 6 of 6 sites read `savescenariomap pop`, so it pushes exactly one value. The operand is `mapfilename`, a `100 string` buffer |
| `savespecialmap` | `filename` → result | `gs\hotkey.gs` 562 — writes `.smp`; same shape, also 6 of 6 `pop` |
| `make_custom_random_map` | `width height` | `gs\edit\mapgen.gs` 306 (defined in `gs\rmg.gs`, pops height first) |
| `paintelevation` | `x y terrain elevation` | `gs\rmg.gs` — `tx ty tt e paintelevation` |
| `setelevation` | `x y elevation` | `gs\rmg.gs` — `x dx add y dy add 0 setelevation` |
| `getelevation` | `x y` | `gs\rmg.gs` — `/e x y getelevation def` |
| `setterrain` | `x y terrain` | `gs\rmg.gs` — `tx ty tt setterrain` |
| `getterrain` | `x y` | `gs\rmg.gs` — `x -1 1 singlerand add y getterrain` |
| `setterrainwithradius` | `x y radius terrain` | `gs\rmg.gs` — `... 3 5 singlerand tt setterrainwithradius` |
| `maxslope` | `slope` | `gs\rmg.gs` — `0.7 maxslope` |
| `addcapitol` | `x y faith faith` | `gs\generate.gs` — `16 16 LIFE LIFE addcapitol` |
| `clearmap` | `texture` | `gs\maplib.gs` — re-runs `mapw maph newmap` at the current size |
| `addunit` | `x y unittype player ?` | `gs\generate.gs` — `20 16 liinf 1 -1 addunit`, inside `unittypedict begin ... end` |

`mapw` and `maph` are readable names giving the current map's dimensions, which removes the need to
assume a width when unpacking a location.

Note the coordinate convention split: these terrain and elevation operators take **pairs**, while
`anythinglocation`, `getterrainspritelocation` and `findemptylocation` deal in **packed** locations
(`y * mapw + x`). Mixing the two is the mistake that destroyed a map object on 2026-09-16.

### `<<` and `>>` dictionary literals are in the shipped corpus

`gs\rmg.gs` builds a dispatch table with PostScript dictionary-literal syntax:

```
/dummy2 << -1{neutral_landscape}LIFE{life_landscape}DEATH{death_landscape} ... >> replace bind def
```

Any lexer or VM for this language has to handle `<<` and `>>` as delimiters, not as shift operators
and not as ordinary names. **Corrected 2026-09-18:** this used to say both of ours did, "checked,
not assumed". `gamescript.rs` and `gamescript_vm.rs` do — the lexer emits
`Delimiter::DictionaryOpen`/`Close` and the VM opens and closes a `CollectionKind::Dictionary` on
them. `tools/gs_syntax.py` does **not**: `<` and `>` are ordinary name bytes there, so `x<<y` is
three tokens to the authority and one to it. It only ever looked right because every `<<` and `>>`
in the corpus is whitespace-delimited, which is what the original check actually observed. The
check that was missing is in [lexer parity](#lexer-parity-the-two-tokenizers-and-what-still-separates-them),
where this is the one divergence still open.

### Unused engine surface

**465 operators are never called by any GS5R3 script.** Examples: `addfollower`, `addbuilding`,
`aimedattack`, `animatearmy`, `airstrengthinregion`, `armyspyarmyartifacts`, `addspelleffect`. These
are engine capabilities the shipped mod does not reach, which makes them the most interesting part of
the table for anything that wants to do something the game does not currently do.

### Is the table order meaningful? Measured, not assumed

The operator table's order is fixed at build time, so if neighbouring entries belong to the same
subsystem the table is a free outline of the host API. That is a tempting thing to assert from
eyeballing it — `getcheckmarkstate, togglehelpcheck, addhelppanel, enablemap, disablemap` certainly
*looks* like a UI cluster. `tools/operator_groups.py` tests it instead, comparing the real order
against a shuffled baseline on signals the table itself does not contain.

| Signal | Real order | Shuffled | Ratio |
| --- | ---: | ---: | ---: |
| Adjacent operators' caller-set overlap (mean Jaccard) | 0.3145 | 0.0133 | **23.6x** |
| Adjacent operators sharing at least one caller file | 58.5% | 10.5% | **5.6x** |
| Adjacent operators sharing a name stem | 12.87% | 0.021% | **611x** |
| Mean run length of dominant caller directory | 1.44 | 1.10 | 1.31x |

Unrounded, because two of these cells are close enough that rounding hides what moves:
0.31445655255437444 / 0.01332579503380394 = 23.597582865163627; 0.5846702317290553 / 0.1048 =
5.578914424857398; 0.12867443150305047 / 0.00021075984470327234 = 610.5263157894736;
1.4397446129289704 / 1.0991692916425675 = 1.3098479223136381. The displayed cells are rounded to
two or three places and the ratios to three significant figures; the fourth row's 1.31x is
1.3098479223136381, not a ratio of the two rounded cells above it.

**Re-measured 2026-09-18, and the re-measurement found a defect in the measurement.** This table is
computed through `tools/gs_syntax.py`, five of whose rules changed that day, so it was re-run with
the tokenizer immediately before those fixes and immediately after, over one 1,696-member GS5R3
dump. **All four rows are bit-identical between the two runs** — not approximately, the same
floats. The fixes restore tokens (706 to `wacave.gs` alone) and split `w/name`-shaped tokens, and
those changes fall inside members whose caller sets they do not alter for any operator in the
table.

An earlier version of this note claimed the fourth row moved, 1.45 → 1.44 observed with the ratio
going 1.33x → 1.32x, and credited the tokenizer. **That was wrong, and the way it was wrong is the
useful part.** `caller_group_agreement` chose each operator's dominant directory with
`Counter.most_common`, which breaks a tie by insertion order, and the insertion order came from
iterating a `set` of file names — so it varied with Python's string hash randomisation between
*processes*. Five runs over one corpus with one unchanged tokenizer give observed 1.43, 1.44, 1.44,
1.44, 1.45 and ratio 1.31x-1.32x. The "delta" was noise, and comparing one pre-run against one
post-run could not have told the difference. The tie is now broken by name, `tests/test_operator_groups.py`
runs the measurement under eight hash seeds and requires one answer, and the numbers above are from
the fixed, deterministic version. The published fourth row before all this was 1.45 / 1.10 / 1.33x,
which is inside the noise band it was measured in.

The first three say the ordering is real and strong — though "three rows" overstates how many
independent things they are, and this page used to. Rows 1 and 2 are two projections of a single
`call_site_agreement` computation, mean overlap and share-any, so they cannot disagree with each
other; row 3 comes from `stem_agreement`, whose only argument is the operator sequence — it never
touches the caller index and is mathematically incapable of moving when the tokenizer changes. So
the evidence is one statistic about callers, two ways, plus one constant. **The fourth says the
obvious labelling axis is the wrong one**, and that is worth as much as the positive results.

#### The negative result

Grouping operators by the directory their callers live in barely beats chance. The reason is
structural: GS5R3's script tree is organised for the mod's authors — `gs\dlg`, `gs\dungeons`,
`gs\barters` — and two of those directories dominate the corpus, so "dominant caller directory" is a
coarse, lopsided label that cuts across the engine's internal seams rather than along them. Anyone
inferring subsystems from where the calling scripts live will produce a plausible map with no way to
tell where it is wrong.

#### What does label them

Name morphology, overwhelmingly. The engine names accessors around a shared subject, so stripping
the leading verb recovers it: `getcastdata`, `setcastdata` and `initcastdata` are one subject, and
`isdetectthief?` and `candetectthief?` are another. Adjacent operators share a stem **611 times more
often than chance**, and the runs fall out directly:

```
scriptwindow       updatescriptwindow, openscriptwindow, closescriptwindow,
                   clearscriptwindow, savescriptwindow
multiplayeroption  getmultiplayeroption, setmultiplayeroption,
                   incmultiplayeroption, decmultiplayeroption
lighttables        makelighttables, calclighttables, loadlighttables, savelighttables
movingstealthily   startmovingstealthily, stopmovingstealthily, ismovingstealthily?
detectthief        detectthief, isdetectthief?, candetectthief?
hotkey             addhotkey, removehotkey, gethotkey
```

Eleven runs of three or more reach that bar on exact stem equality alone. That is a floor, not a
ceiling: it counts only strictly consecutive entries whose stems match exactly, so
`enumchildbuildings` beside `enumpossiblebuildings` does not register. A fuzzier stem comparison
would find more, at the cost of a threshold nobody can justify — so the strict count stands as the
conservative measurement.

#### Reproducing it

```sh
target/release/lom-asset-viewer --scan-natives '/path/to/English/lomse.exe' > scan.txt
# Extract gs.mpq first, then FLATTEN its .gs members into one directory, joining path components
# with `__`: `gs/dlg/lib_dlg.gs` becomes `gs__dlg__lib_dlg.gs`. Both halves matter and neither is
# optional. `caller_index` uses `Path.iterdir`, so it does not descend into subdirectories and a
# nested tree yields 30 operators with callers instead of 1,371; and the dominant-caller-directory
# row recovers the directory by splitting the file *name* on `__`, so the separator is the one the
# tool reads. `caller_index` also applies NO suffix filter -- it reads every regular file in the
# directory -- so a stray `scan.txt` or `.DS_Store` becomes a caller and shifts the caller-set
# rows. Keep the dump to `.gs` members only. Link rather than copy; nothing is written to it.
# The measurement is deterministic since 2026-09-18; before that the fourth row varied by hash
# seed.
python3 -m tools.operator_groups scan.txt /path/to/flattened/scripts
```

Tokenising uses the project lexer rather than a regex, so a name appearing only inside a `;` comment
is not counted as a call site.

### Operator diagnostics name their parameters

Separately from the table, 83 diagnostic strings in the binary follow the form `operator - message`,
covering 57 operators and naming their arguments:

```
getarmydata - invalid data_id          setarmydata - invalid value for owner
getarmydata - no army                  setarmydata - invalid value for location
getunitdata - invalid unit_num         setarmydata - invalid value for num_units
getcitydata - invalid player reference setarmydata - invalid value for drawn_unit
getimphotspot - no such hotspot        setarmydata - cannot set army mps directly
```

This gives a partial field vocabulary for the core data accessors — `data_id`, `unit_num`,
`player reference`, `owner`, `location`, `num_units`, `drawn_unit` — without running the game.

## Native host stubs and unknown-name traces

The host API is not implemented and is not guessed at. Two mechanisms added on 2026-09-16 let script
logic that depends on it be executed and observed anyway:

- `GameScriptVm::define_native_stub` supplies a value for a native call, exposed as
  `--stub NAME=VALUE` (integer, `true`, or `false`). A stub stands in for a **pure read of game
  state** under a declared input. Anything with side effects must not be stubbed this way.
  `native_calls()` counts which stubs were reached, which is the evidence that a candidate really is
  a host call rather than a script definition.
- `GameScriptVmError::unknown_name` returns a structured trace — the name, the VM step, and the call
  stack at the point of failure. It is read from the error rather than the VM because the call stack
  unwinds as the failure propagates. The probe prints `unknown-native-name`, `unknown-at-step`, and
  `unknown-call-stack` rows before surfacing the error.

Executing the real GS5R3 difficulty idiom from `gs\MAKEARMY5.gs`, `[25 50 75]getdifficultylevel get`,
returns 25, 50, and 75 for Easy, Medium, and Hard.

**This upgrades the difficulty finding from reading to execution.** The shipped `extra_strong?` body
from `gs\scenario\default.gs`,
`[false false false]getdifficultylevel get getmultiplayerflag{pop false}if`, evaluates to `false` at
every difficulty and in both multiplayer states. The body quoted on the forum,
`true getmultiplayerflag{pop false}if`, evaluates to `true` on Hard in single-player and `false` in
multiplayer — exactly the behaviour its author described. So the description matched real code that
did not ship. See [difficulty and AI](difficulty-ai.md).

### Classifying a stop

`--probe-gamescript` stops on the first name it cannot resolve and emits a structured trace. With
`--exe`, the trace is also classified against the operator tables, which turns a stop from a research
question into a decision:

| `unknown-name-class` | Meaning | What to do |
| --- | --- | --- |
| `operator` | the engine implements it; the entry point is reported | supply `--stub NAME=VALUE` |
| `engine-constant` | SCREAMING_CASE and absent from the tables | supply `--stub NAME=VALUE` |
| `unresolved` | neither | the definition is probably in a module this run has not loaded |

```
unknown-native-name       getdifficultylevel
unknown-at-step           236
unknown-name-class        operator
unknown-name-entry-point  0x00485e90
unknown-name-remedy       the engine implements this; supply it with --stub getdifficultylevel=VALUE
```

The `unresolved` class is the one worth reading carefully. At corpus scale it means the
definition-shape classifier missed a definition, but during a single-module run it far more often
means exactly what it says: the name is defined somewhere that has not been executed yet. The class
does not distinguish those, and should not be read as evidence of engine surface.

## Static module references

The analyzer records a dependency only for the adjacent token pattern:

```text
"path/to/module.gs" run
```

It resolves the normalized, case-insensitive path against every member in the same MPQ. Dynamic path construction, conditional catalogs, aliases, and loose files remain outside this model. The graph is therefore useful for entry-point and subsystem discovery, not proof that an installation is complete.

## Line endings: bare CR is a line ending here

`gs.mpq` mixes all three conventions, and members mix them internally, so these are **overlapping counts and not a partition**. Of GS5R3's 1,696 members: **1,123 contain at least one CRLF, 242 contain at least one bare CR, 63 contain at least one bare LF**, and 501 contain no line terminator at all (the `fonts\*.gs` members are single-line). Only **49** are bare-CR-*only*; the other 193 bare-CR members also contain CRLF. 3.02 has 302 CRLF members and no bare CR at all. `--survey` prints this census (`line-endings-*`). Evidence class: Observed in a local binary.

This is not a cosmetic detail. A `;` comment runs to the end of its line, so a reader that splits on LF sees a single comment swallow dozens of live statements — which is how this project once harvested commented-out code as if it were bindings. The lexer terminates comments at `\r` or `\n`, counts a bare CR as a line ending, and counts a CRLF pair once; `a_comment_ends_at_a_bare_carriage_return` and `line_numbers_count_bare_carriage_returns_and_pair_crlf` fail if either rule is removed.

For the 49 bare-CR-only members, the line counter is the only thing between a reader and a line number of 1 for every position in the file. For the other 193 it is not absent but *understated*: the counter advances on the CRLF pairs and stalls across the bare CRs, so reported lines drift rather than collapse — which is the harder error to notice.

### The same census across all three profiles

The counts above are GS5R3's. Re-measured 2026-09-18 over **all 4,692 `.gs` members of the three
installed profiles' `gs.mpq`** (vanilla 1,315, 3.02 1,681, GS5R3 1,696; 0 unreadable), as an
*exclusive partition* rather than overlapping counts:

| Exclusive style | vanilla | 3.02 | GS5R3 | all |
|---|---:|---:|---:|---:|
| no terminator at all | 1,170 | 1,379 | 501 | **3,050** |
| CRLF only | 145 | 302 | 890 | 1,337 |
| CRLF + bare CR | 0 | 0 | 193 | 193 |
| bare CR only | 0 | 0 | 49 | 49 |
| CRLF + bare LF | 0 | 0 | 40 | 40 |
| bare LF only | 0 | 0 | 23 | 23 |

This was produced independently of the numbers above and agrees with every one of them: ≥1 CRLF
890 + 193 + 40 = 1,123; ≥1 bare CR 193 + 49 = 242; ≥1 bare LF 40 + 23 = 63; no terminator 501;
bare-CR-only 49; and 3.02 with no bare CR at all. Evidence class: Observed in a local binary.

Two things the wider view adds. **Bare CR is a GS5R3 phenomenon**: neither vanilla nor 3.02 contains
a single bare CR, so a tool that mishandles them is correct on two of the three profiles and on the
whole of the vanilla corpus a first mod is most likely to be built against. And **the single
commonest shape in the corpus is a member with no line ending at all** — 65% of it — which is why a
"preserve the line endings" rule is not a useful check on its own. `units\orinf.gs`, the Phase 4
target, is one of those: 1,798 bytes, one line, no trailing newline. The build pipeline therefore
compares a replacement's census against its base member's rather than against any fixed style; see
[the build pipeline](build-pipeline.md).

The damage `tools/gs_syntax.py`'s LF-only comment rule did was narrower than the bare-CR
population suggests, because a bare CR cost it nothing unless a `;` comment was there to run on.
Comparing its token count against the same rule with CR treated as a terminator: **34 members lost
tokens, all 34 in GS5R3, none in vanilla or 3.02.** The rule was **fixed on 2026-09-18** — a comment
now ends at the first of `\r` or `\n` — and the same measurement after the fix reports zero.

The corpus relies on it. GS5R3's `gs\standard.gs` comments out its inherited `min`/`max` at lines 68 and 70 and redefines them at 73 and 74; reading the commented pair as live would give the wrong bodies.

## Lexer parity: the two tokenizers and what still separates them

This repository has two lexers for one grammar. `spikes/asset-viewer/src/gamescript.rs` is the
authority — it is what `--gs-facts` runs and what the build pipeline validates with — and
`tools/gs_syntax.py` is the tokenizer `compare_trees.py` normalises with. Where they disagree, one
of them is wrong about a shipped member, so the change report prints both and says so
([build pipeline](build-pipeline.md#why-the-lexer-is-the-rust-one)). This section is the list that
message points at.

**Closed 2026-09-18** — five divergences, each measured over all 4,692 `.gs` members of the three
installed profiles before and after. Evidence class: Observed; the archives are not in this
repository, so this measurement is the evidence and no reader can re-derive it from the repository
alone. The port it was measured with is a throwaway instrument and is **not committed**: what is
committed instead is `AuthorityParityTest` in `tests/test_gs_syntax.py`, which runs the real
`lom-asset-viewer` against one fixture per divergence.

| Divergence | Members changed | Where |
|---|---:|---|
| `;` comment ended at `\n` only, though bare CR is a line ending | 34 | all GS5R3 (25 with no LF at all) |
| `\` treated as a string escape; the authority has no escape rule | 5 | vanilla 1, 3.02 2, GS5R3 2 |
| `/` did not end a name, though `is_separator` lists it | 5 | vanilla 1, 3.02 1, GS5R3 3 |
| `str.isspace()` wider than `is_ascii_whitespace` (`\x0b`, `\x1c`-`\x1f`, `\x85`, `\xa0`) | 0 | one member holds such a byte, inside a string |
| `(` and `)` were delimiters here and are ordinary name bytes to the authority | 0 | 530 members hold a parenthesis; in every one it is inside a string or a comment |

After them, **0 of 4,692 members tokenize differently**.

**Two of the five had zero corpus reach and were fixed anyway.** A divergence about a shipped
grammar is a defect whether or not the shipped corpus happens to exercise it: `foo(1)` was four
tokens here (`foo`, `(`, `1`, `)`) and one to the engine's lexer, so an edit to `foo (1)` would have read as a real change
to the authority and as layout-only here — in a mod nobody has written yet, which is exactly the
kind of file this pipeline exists to check. Reach decides urgency, not whether something is wrong.

**Still open — one divergence.** `<` and `>` end a name for the authority (`is_separator`) and not
for `gs_syntax.py`, so `x<<y` is three tokens to one and one token to the other. No corpus member
reaches it. Unlike the parentheses, it cannot be closed by editing a set: a single `<` not followed
by `<` is a *parse error* to the authority, and `gs_syntax.py` has no error channel — the same
holds for an unterminated string and an empty literal name after `/`, both of which this tokenizer
keeps as tokens rather than inventing an error. `tests/test_mod_report.py` uses this divergence as
its disagreement fixture, so the change report's check stays exercised, and
`tests/test_gs_syntax.py` pins the two error-shaped behaviours so a later simplification cannot
change them silently.

**Decoding is not a lexing divergence, and is easy to mistake for one.** The Rust lexer decodes
member bytes with `from_utf8_lossy`, so a byte above 0x7e that is not valid UTF-8 becomes U+FFFD in
its token text and in its digest; `gs_syntax.py` decodes latin1 and keeps it. Seventeen corpus
members contain such a byte. Their token *boundaries* agree — only the text of one token differs —
so any comparison of the two tokenizers has to decode both sides the same way or it will report
seventeen false divergences.

## Procedure locals: `replace` and the slot-zero dictionary

GameScript procedures carry private storage, which PostScript has no equivalent of. Two forms appear, used interchangeably for structurally identical procedures:

```text
/NAME {...} /dummy 5 dict replace bind def     ; attach a dictionary under a name
/NAME {...} dup 0 5 dict put bind def          ; attach one in slot zero
```

`PROC /name VALUE replace` leaves `PROC` on the stack and attaches `VALUE` under `name`. While that procedure runs, the name resolves to the attached value, so:

- `/dummy begin ... end` opens the procedure's private dictionary, and `def` inside writes into it;
- `/dummy /low known` and `/high undef` operate on that dictionary;
- `/char_array exch get` in `gs\standard.gs`'s `char_cvs` indexes a private *array*, and `gs\autochat.gs`'s `/dummy length` measures one.

Values other than dictionaries are attached the same way, so the mechanism is "named local", not "local dictionary". Evidence class: Observed in a local binary for the shape (`standard.gs`, `autochat.gs`, `chess.gs`, `citytest.gs`); Inferred for the meaning — it is the reading under which every one of those bodies resolves and produces the expected results.

Slot zero is read as the name `dummy` because every procedure using the `put` form opens with `/dummy begin`, and the `replace` form of the same procedures names it `dummy` explicitly. Evidence class: Inferred.

**This changed the definition scanner.** The old three-token definition window could not see past `dup 0 N dict put bind` or `/dummy N dict replace bind`, so `standard.gs`'s own `writestring`, `pushonstack`, `popoffstack` and `onstack?` were filed as names nothing in the corpus defines. The window now admits a short list of definition-finishing operators (`bind`, `dup`, `put`, `dict`, `array`, `string`, `replace`, `currentdict`, `begin`, `end`) beyond the first three tokens, and stops at any other operator so `/invoke_spell cvx` is still not a definition. Evidence class: Corrected.

## Dictionary keys are typed

`gs\spells\weaken.gs` ships `/level_advantage_table << 3 1.0 0 0.5 -1 0 >> def`, and `standard.gs`'s `interpolate` walks it with `forall` and compares each key numerically. Keys are therefore numbers or names, not strings, and `forall` on a dictionary pushes key then value. Evidence class: Observed in a local binary.

## Conditions may be integers

`standard.gs`'s `getflagvalue` is `1 exch bitshift and {true}{false} ifelse`. `and` on two integers yields an integer, which `ifelse` then consumes. A boolean-only `if`/`ifelse` cannot run the shipped flag helpers at all, so a number is a condition and zero is false. Evidence class: Inferred.

## VM checkpoint: what executes now

The interpreter implements **68** language primitives, listed in `gamescript_vm::PRIMITIVE_NAMES`,
plus the two names the lexer resolves before dispatch ever sees them (`true`, `false`), which
`gamescript_vm::PRIMITIVE_LITERAL_NAMES` holds. This paragraph used to say 68 when the list held
67; the count is now read off the list.

- **stack** — `dup pop exch copy index roll count clear`
- **arithmetic** — `add sub mul div idiv mod neg abs sqrt sin cos round truncate floor ceiling bitshift`
- **comparison and logic** — `eq ne gt lt ge le and or xor not`
- **control flow** — `if ifelse repeat for loop exit forall exec`
- **aggregates** — `array dict string [ ] << >> get put length known undef load currentdict begin end def replace`
- **conversion** — `cvx cvlit cvi cvr cvs type bind`
- **module loading** — `run`

`sin` and `cos` take **radians**, unlike PostScript's degree-taking pair: `standard.gs` defines `/radians {180 div 3.141596 mul}` and applies it before every call. Evidence class: Inferred.

`PRIMITIVE_NAMES` is checked against the dispatch itself by a test that reads the match arms back out of the source, because the vocabulary classification below calls a name a native host call precisely when the engine lists it and this VM does not implement it.

### What the VM refuses rather than answers

Issue #5's rule — never invent a value — binds the language half as much as the host half, and at
first it did not. Four invented results were reachable without touching a native call:

| Expression | Was | Now |
| --- | --- | --- |
| `1 0 div`, `1 0 idiv` | `inf` | stops: "has no defined result" |
| `1 0 mod`, `-4 sqrt` | `NaN` | stops |
| `1e308 1e308 mul` | `inf` | stops |
| `1 32 bitshift` | `1` (shift count masked) | stops: the engine's behaviour past 32 bits is not established |
| `1 -1e300 bitshift` | **panic**, "attempt to negate with overflow" | stops |

The consequences were worse than the values. `1 0 div 1000000 gt` answered `true`, and a released
`NaN` makes every later `gt` **and** `lt` answer `false`, so one undefined result quietly turns
every subsequent comparison into a wrong answer that looks like a real one. `bitshift` mattered
most concretely: `getflagvalue` is `1 exch bitshift and`, so a masked shift count reported a set
flag for a bit index of 32 or more. PostScript raises `undefinedresult` for the arithmetic cases;
for the shift width, a 32-bit x86 `shl` masks the count to five bits and C's `1 << 32` is undefined,
so which one `lomse.exe` does is **refused rather than modelled**. Within `-31..=31` the shift runs
on the 32-bit pattern with vacated bits zero-filled, which is what PostScript documents.

No shipped member reaches any of these — the 406 that load are unchanged by the change — so this is
a claim about the VM, not about the corpus.

Execution is bounded in three dimensions, because a step ceiling bounds only one of them:

- **steps.** `repeat` did not charge them, so `100000000000 {} repeat` never returned; `for` and
  `loop` always did.
- **call depth.** Script recursion consumes the *host* stack, not the step budget: `/f {f} def f`
  aborted the process outright with a Rust stack overflow at roughly twenty thousand frames. That
  mattered most for `--survey`, which runs all 1,681 members in one process, so a single recursive
  member destroyed every other result. Capped at 256 frames, measured against the 2 MB stack
  `cargo test` gives a test thread, and confirmed not to change any survey count.
- **allocation.** `100000000000 array` asks for 1.6 TB before a single step is charged. Refused at
  a million elements — refused, not clamped, because a shorter array than the script asked for
  would silently change what the script computes.

### Nearer bindings shadow primitives, deliberately

`frame_local` is consulted before the dictionary stack, and the dictionary stack before the
primitives. That is the PostScript model: the operators live at the bottom of the dictionary stack,
so any nearer binding wins. 15 of 3.02's script definitions depend on it by overriding a name the
engine also implements, `standard.gs`'s own `/index` among them. A procedure local named after a
primitive would shadow it too; nothing in the corpus does that today.

### Where the battery runs, and what a default `cargo test` covers

The exercise table lives in `src/gamescript_standard.rs` and is executed by
`tests/gamescript_standard.rs`. It used to live inside the example, which meant **it ran only when
a person typed the command**: 32 exercises, zero `#[test]` attributes, and no `tests/` directory in
the crate. Every "225 passed" figure published about this work was a suite that never executed one
exercise, and pointing `sin` at `f64::cos` would have left it green. Evidence class: Corrected.

The battery needs the shipped `gs\standard.gs`, which is not in Git, so its three tests are
`#[ignore]`d — they report as `ignored` rather than not existing:

```sh
LOM_GS_MPQ='/path/to/English/gs.mpq' LOM_GS_PROFILE=patch302 cargo test -- --ignored
```

`LOM_GS_PROFILE` is asserted, not tolerated: `patch302` must disagree on nothing, `vanilla` on
exactly the two 3.02 string helpers it does not ship, and `gs5r3` on those two plus `min` and `max`.
A declared disagreement that stops happening fails the test too, which is what catches an
expectation quietly rotting into agreement.

A *default* `cargo test` cannot touch the corpus, so what covers the primitives underneath is
`gamescript_vm`'s own unit tests — in particular `trigonometry_is_not_self_consistent_under_a_swap`,
which pins `sin` and `cos` by their odd/even identities rather than by tabulated values, and
`arithmetic_and_comparison_primitives_are_wired_to_the_right_operations`, which uses
non-commutativity to catch swapped operands. The `sin`-calls-`cos` mutation now fails both the
default suite and the battery; verified by making it.

### The `standard.gs` battery

`cargo run --example gamescript_standard` loads 3.02's `gs\standard.gs` (339 VM steps, 36 names, empty operand stack) and then runs 31 exercises whose expected stacks were worked out from the shipped bodies rather than recorded from output. **All 31 behave as expected**: 22 produce a stated stack, and 9 stop on a named engine call. Six were wrong on the first run and the run said so, which is the point of stating the expectation first.

Three findings fell out of executing rather than reading:

1. **`get_if_known`'s header comment states its operands backwards.** The comment says `;val dict key get_if_known`; the code's `3 1 roll` requires `dictionary key default`. Evidence class: Corrected.
2. **`dump_flags` does nothing to its operand.** After the first iteration its `2 copy` reads the loop counter rather than the flags, and the trailing `pop` discards the one index it kept. `5 dump_flags` leaves `5`. Evidence class: Observed in a local binary, by execution.
3. **GS5R3 ships `min` and `max` swapped.** Vanilla and 3.02 both define `/min{2 copy gt{exch pop}{pop}ifelse}`. GS5R3 comments that line out at `gs\standard.gs:68`/`:70` and redefines the gt-body as `max` and the lt-body as `min` at `:73`/`:74`, so under GS5R3 `3 7 min` is `7` and `3 7 max` is `3`. Evidence class: Observed in a local binary.

  The GS5R3 run reports **four** failures, not two: `min` and `max` for the swap, plus `string_cvi` and `char_cvs`, which stop on an unknown name because GS5R3 does not ship them — they are 3.02 additions. The vanilla run reports those same two and nothing else. The committed transcripts in `reports/gs/standard-run-*.tsv` are the three runs verbatim.

The nine engine names the battery reached, with their `lomse.exe` entry points where the operator table lists them:

| Name | Class | Entry point | Reached through |
| --- | --- | --- | --- |
| `additem` | operator | `0x00490be0` | `addrect` |
| `dialogisopen?` | operator | `0x00505830` | `closeifopen` |
| `free` | operator | `0x004dc600` | `free_stack_elements` |
| `gettemppath` | operator | `0x005053f0` | `eval` |
| `rand` | operator | `0x004c9f20` | `makeregion` |
| `write` | operator | `0x004cc120` | `writestring` |
| `build_statement` | unresolved | — | `retrievefromstack` |
| `sysdlg` | unresolved | — | `exitapplication` |
| `unitdictxref` | unresolved | — | `geteasyunitdata` |

None of the three unresolved ones is an engine *call*: `build_statement` is defined in `gs\text.gs`, `sysdlg` in `gs\dlg\sysdlg.gs`, `unitdictxref` in `units\easyunit.gs`. For `build_statement` and `unitdictxref` that closes the dependency; for `sysdlg` it only **relocates** it, because its definition is `/sysdlg 50 dialog def` and `dialog` is an operator. Loading resolves the name and then needs the host anyway.

The tool reports `standard.gs`'s static debt as 20 names, down from 21 now that the VM implements
`run`. One of them, `outfilename`, is the module's own local, defined inside `eval` with a value
spanning more than the attachment window, so the real debt is **19** — 16 operator-table entries and
3 unresolved names.

## Vocabulary classification

`cargo run --example gamescript_vocabulary` partitions every distinct executable name in a profile, in the order the interpreter resolves. It writes `reports/gs/vocabulary-<profile>.tsv` (name, uses, class, whether the old broad heuristic admitted it, operator entry point).

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| script-definition | 10,305 | 10,902 | 12,355 |
| language-primitive | 56 | 56 | 55 |
| native-host-call | 1,383 | 1,414 | 1,391 |
| constant-or-data | 694 | 787 | 723 |
| engine-dictionary-key | 112 | 113 | 126 |
| unclassified-residue | 1,531 | 1,904 | 2,206 |
| **distinct executable names** | **14,081** | **15,176** | **16,856** |

Restricted to the broad "likely hardcoded engine name" candidate list, which is what issue #5 asked to partition:

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| language-primitive | 56 | 56 | 55 |
| native-host-call | 1,383 | 1,414 | 1,391 |
| constant-or-data | 690 | 761 | 718 |
| engine-dictionary-key | 0 | 0 | 1 |
| unclassified-residue | 32 | 33 | 34 |
| **broad candidates** | **2,161** | **2,264** | **2,199** |

So **roughly 98.5% of the broad candidate list is real** — a primitive, an operator the engine registers, or a constant — and about 33 names per profile are coincidences of the string filter. `script-definition` is zero there, now for a principled reason rather than by construction: a candidate is by definition a name the corpus does not define, and both sides of that comparison are case-sensitive.

**The published candidate totals moved when the fold was fixed.** `likely_engine_names` in `src/main.rs`, which is what `--scan-gamescript` prints, carried the same case-folded definition check, so a lowercase `/gold` suppressed `GOLD` — 290 uses in 3.02 — from the list a person actually reads:

| | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| candidates, case-sensitive | 2,161 | 2,264 | 2,199 |
| under the old fold | 2,114 | 2,212 | 2,151 |
| recovered | 47 | 52 | 48 |

Every recovered name is an engine constant (`GOLD`, `FOOD`, `CRYSTALS`, `WARRIOR`, `WIZARD`, `TARGET_ARMY`, `CITY_OWNER`), which is the class the filter exists to surface. Both the scanner and the classifier report the same deltas from independently written code, and both print them (`engine-names-recovered-from-the-old-case-fold`, `broad-candidates-recovered-from-the-old-case-fold`) so the movement is stated rather than silent. Evidence class: Corrected.

The *binary-string* half of the rule stays folded in both places: it asks whether a name occurs in the image at all, a question case does not bear on. Only the "does the corpus define this name" half has to agree with the interpreter.

`engine-dictionary-key` recognises keys of a dictionary the engine owns, against this crate's own recovered terrain-sprite registry (`map::TERRAIN_SPRITE_TYPES`). That accounts for 113 of 3.02's residue rows — `cave` at 157 uses, plus `eemush2`, `minec`, `crystb`, `fish`, `brew` and the rest. The table's *ids* are profile-specific and unused here; only its names are.

That table was dumped from a **GS5R3** script set, so applying it to the vanilla and 3.02 columns is an **assumption, not a measurement**: it assumes the registry carries the same names across profiles, which this run does not establish. The 126 GS5R3 rows are measured against their own profile; the 112 vanilla and 113 patch302 rows are not. A name wrongly placed here has moved out of `unclassified-residue` and nowhere else, so the error is confined to those two classes. Re-running the sprite-type probe per profile would settle it.

`unclassified-residue` is named for what is not known about it. It is **not** a false-positive list: its highest-use members are genuine script definitions whose definition sites the scanner still cannot see — `set_level_modifications` (279 uses, `gs\levlmods.gs`), `getdungeonstrength` (204 uses, `gs\placedng.gs`), `build_statement` (340 uses, `gs\text.gs`, its value spanning more than the attachment window). The rest is mixed-case engine type names such as `Type_GraphicPage` that `is_screaming_case` does not match. Shrinking this class is a job for the definition scanner, not for the operator table.

Eleven 3.02 names moved *into* it when the three non-definition shapes were excluded — `button2_t`, `crystalsvaluestring`, `up_button_x` and the like, all dictionary *values* that had been read as keys. One name moved the other way: `exec` had a false definition site and is now correctly a `language-primitive`, which is why that column reads 56 rather than 55.

Names the corpus defines that the engine *also* registers are reported separately — 15 in 3.02, including `exec`, `ne`, `type`, `run` and `sleep`. The dictionary wins at run time, so these are script overrides of engine behaviour.

## How much of the corpus is pure language

**Superseded by [the execution census](#the-execution-census), 2026-09-18.** This section is the
measurement *before* `run`, `bind` and opaque engine constants; it is kept because the prediction
recorded against it was wrong and the reasoning it supported was partly refuted. The current
figure for 3.02 is 635 members, not 406, and `engine-constant` is no longer a first-blocker class
at all because a declared constant no longer stops a member that only stores it.

`--survey` loads every `.gs` member on its own machine and records what stopped it. On 3.02's 1,681 members:

| Outcome | Members |
| --- | ---: |
| loaded to completion with no engine support at all | 406 |
| stopped on a name | 1,267 |
| failed some other way | 8 |

Of the 1,267 stops, classified by what the **first** blocking name is. Matching against the corpus's definitions is case-sensitive here, as the VM's own resolution is:

| First blocker | Members |
| --- | ---: |
| defined in another member | 564 |
| engine constant | 556 |
| engine operator | 124 |
| unresolved | 23 |

Read that as a lower bound on what loading would unblock, not a projection: resolving a member's first blocker only reveals its second.

Three caveats, because the top two are effectively tied and an earlier version of this table was not:

1. **A case fold produced 640/480 and inverted the comparison.** Nothing in 3.02 defines `GOLD`; `gs\barter.gs` defines `/gold`. Folding put `GOLD` and 53 other constant names — 75 members by `GOLD` alone — in the wrong column. **Corrected.**
2. **41 of the 564 are `userdict`.** That is the root dictionary the entry-point module builds for itself (`START.GS`: `/userdict 1000 dict dup begin def`), so on a standalone-member reading it is not another member's definition in any useful sense. Discount it and the two groups cross: 523 against 556.
3. **`engine-constant` is itself a shape heuristic** — SCREAMING_CASE and absent from the operator table. So 523-against-556 pits one heuristic against another. The *direction* is solid; the crossover point is not establishable from this measurement.

The eight "failed some other way" members are two groups, not one. Four are the procedure-as-array modelling gap: `type` reports a procedure as `/arraytype` and `length` accepts one, but `forall` refuses it (2 members) and `PROC 0 get` reads the attachment table rather than the body (`procedure has no metadata slot 0`, 1 member; `put does not support procedure with procedure index`, 1 member). The other four are genuinely unbalanced `{` in the shipped source.

## Command

```sh
cd spikes/asset-viewer
cargo build --release

target/release/lom-asset-viewer \
  --scan-gamescript '/path/to/English/gs.mpq' \
  --listfile '../../artifacts/reference-listfiles/lords-of-magic.txt' \
  --exe '/path/to/English/lomse.exe'

# Recover the engine's operator tables. With an archive, the candidate vocabulary is reconciled
# against them; without one, every operator is listed with its entry point, recovered arity and a
# confidence column.
target/release/lom-asset-viewer --scan-natives '/path/to/English/lomse.exe'

target/release/lom-asset-viewer \
  --scan-natives '/path/to/English/lomse.exe' '/path/to/English/gs.mpq' \
  --listfile '../../artifacts/reference-listfiles/lords-of-magic.txt'

target/release/lom-asset-viewer \
  --probe-gamescript '/path/to/English/gs.mpq' 'gs\standard.gs' \
  --listfile '../../artifacts/reference-listfiles/lords-of-magic.txt' \
  --eval '3 5 min 3 5 max'

# Native state reads may be stubbed so dependent logic can be executed and observed.
target/release/lom-asset-viewer \
  --probe-gamescript '/path/to/English/gs.mpq' 'gs\standard.gs' \
  --listfile '../../artifacts/reference-listfiles/lords-of-magic.txt' \
  --stub getdifficultylevel=2 --stub getmultiplayerflag=false \
  --eval '[25 50 75]getdifficultylevel get'

# Passing --exe classifies any name the VM stops on against the engine's operator tables.
target/release/lom-asset-viewer \
  --probe-gamescript '/path/to/English/gs.mpq' 'gs\standard.gs' \
  --listfile '../../artifacts/reference-listfiles/lords-of-magic.txt' \
  --exe '/path/to/English/lomse.exe' \
  --eval 'getdifficultylevel'
```

Omit `--exe` to skip binary correlation. The command is read-only and emits tab-separated summary and diagnostic lines to standard output.

The two example drivers are read-only as well and print tab-separated lines:

```sh
# Load gs\standard.gs, run the 31-exercise battery, and trace every engine name it reaches.
# Non-zero exit if any exercise disagrees with its stated expectation.
cargo run --example gamescript_standard -- \
  --gs '/path/to/English/gs.mpq' --exe '/path/to/English/lomse.exe'

# Add --survey to load every .gs member on its own machine and tabulate what stopped each one.
# `--exe` is what makes engine constants declarable, so without it the survey runs stricter.
cargo run --release --example gamescript_standard -- \
  --gs '/path/to/English/gs.mpq' --exe '/path/to/English/lomse.exe' --survey

# --preload names modules to execute into each survey machine first. A declared input, like
# --stub: nothing is preloaded unless it is named, and the run reports what it preloaded.
cargo run --release --example gamescript_standard -- \
  --gs '/path/to/English/gs.mpq' --exe '/path/to/English/lomse.exe' --survey \
  --preload 'gs/textdict.gs' --preload 'gs/standard.gs' 

# Classify a profile's whole executable vocabulary and write the derived table.
cargo run --example gamescript_vocabulary -- \
  --profile patch302 --gs '/path/to/English/gs.mpq' --exe '/path/to/English/lomse.exe' \
  --out ../../reports/gs
```

## The execution census

`--survey` runs every `.gs` member on its own fresh machine and records what stopped it. Measured
2026-09-18 against all three installed profiles; reproduced verbatim in the
`survey-*` and `reach-*` lines of `reports/gs/standard-run-*.tsv`. Two runs of the same command are byte-identical.

Predictions were written down before the run. They are kept here because both were wrong in the
same direction, and by a lot:

| | predicted | measured |
| --- | ---: | ---: |
| members executing to completion, before this slice | 1,150 (24.5%) | **912 (19.4%)** |
| members executing to completion, after | 2,350 (50%) | **1,551 (33.1%)** |

### Members executing to completion

| Profile | members | before | after | with the declared preload |
| --- | ---: | ---: | ---: | ---: |
| vanilla | 1,315 | 242 | 429 | 473 |
| patch302 | 1,681 | 406 | 635 | 680 |
| gs5r3 | 1,696 | 264 | 487 | 529 |
| **total** | **4,692** | **912 (19.4%)** | **1,551 (33.1%)** | **1,682 (35.8%)** |

Three things moved the number, and none of them is a host call:

- **`run`, backed by the archive.** Read-only, with cycle detection and a nesting bound. A module
  already on the load stack is refused; a module reached twice on disjoint paths is executed twice,
  which is PostScript's rule.
- **`bind`, implemented.** It was a no-op. `START.GS:135` defines a progress-meter wrapper around
  module loading whose body **ends in `run`** — `bind` runs before `def`, so the wrapper captures
  the primitive; without that the name resolves after the `def` to the wrapper itself and the
  engine's own entry point recurses to the call-depth limit.
- **Opaque engine constants.** `/faith FIRE def` files a value; nothing defines `FIRE`. A constant
  the caller has *declared* (SCREAMING_CASE and absent from `lomse.exe`'s operator tables) now
  pushes its **identity and nothing else**. It can be stored, copied and read back; every operation
  needing its number stops. Two references to `FIRE` compare equal; `FIRE` against `WATER` **stops**,
  because `gs5_globals.gs` ships `/UNDEFINED_ATTACK{0}def` beside `/ATTACK_UNDEFINED{0}def` and two
  constant names are therefore not known to hold two values.

The preload column is a *declared input*, exactly as `--stub` is: `gs/textdict.gs` and
`gs/standard.gs`, executed into each machine before the member under test. Both run to completion
on their own. Preloading `START.GS` itself buys nothing — it stops at **step 2**, on `maxgraphics`.

### How much of the engine's operator surface is actually reached

| | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| operators in the table | 1,906 | 1,906 | 1,906 |
| never named by any script | 461 | 426 | 453 |
| named by some script | 1,445 | 1,480 | 1,453 |
| **reached by execution** | **52** | **63** | **57** |
| of those, implemented here as a primitive | 19 | 20 | 22 |
| of those, satisfied by a declared stub | 0 | 0 | 0 |
| of those, **blocking** | **33** | **43** | **35** |

The gap between "named by some script" and "reached by execution" is the honest limit of this
instrument: a survey that stops each member at its first unresolved name cannot see what lies past
the stop. So 1,445–1,480 is a sound **upper** bound on what the corpus could reach, 52–63 is a
sound **lower** bound on what it does reach, and the truth is between them. The one number that is
not an estimate is the last row: **33–43 distinct engine operators are what execution is actually
standing on today**, out of 1,906.

### The blocking ranking — the backlog, in order

Collated across all three profiles, by members blocked (each member counted at its *first* blocker,
so a name below the top can only be undercounted, never over):

| Name | members blocked | class |
| --- | ---: | --- |
| `textdict` | 576 | defined in a member that runs |
| `begin_unit_definition` | 413 | defined in a member that does **not** run |
| `spelldict` | 357 | defined in a member that does **not** run |
| `userdict` | 193 | defined in a member that does **not** run |
| `dialog` | 135 | engine operator |
| `addmounttype` | 86 | engine operator |
| `stack` | 53 | defined in a member that runs |
| `terrainsprites` | 51 | defined in a member that does **not** run |
| `T_Crystal_Mine` | 30 | defined in a member that runs |
| `panel_dict` | 27 | defined in a member that does **not** run |
| `file` | 26 | engine operator |
| `spell_page` | 15 | unresolved |

### What this refutes

The previous stop/go assessment said the two large blocking classes were **both cheap** — "a `run`
that reads the archive, and a declared constant table — and neither requires simulating anything."
The constant half held. The module-loading half did not, and the survey now splits the class so the
difference cannot be stated wrongly again:

`--survey` reports `defined-in-a-member-that-runs` separately from
`defined-in-a-member-that-does-not-run-either`. Of the 2,758 members still blocked on a name across
the corpus, 1,155 name something a runnable member defines and **1,117 name something whose only
definers stop too** — so barely half of the "cheap" class is actually cheap. Module loading does not
help the other half, because the module that would define the name stops on its own first line:

- `gs\tree.gs` defines `terrainsprites` and stops at its **step 2** on `maxterrainspritetypes`.
- `units\easyunit.gs` defines `begin_unit_definition` and stops at its **step 1** on `userdict`.
- 3.02's own `START.GS` names **30 members its own `gs.mpq` does not contain** (`gs/TEXTDICT5.gs`,
  `gs/GRAPHICS5.gs`, `gs/PLAYER5.gs` and the rest of the `*5.gs` set), so a loader driven by it is
  incomplete against that archive by construction. Evidence class: Observed in a local binary.

So the cheap class is roughly half the size it was described as, and the path forward is not "load
more modules" but "stub the handful of operators that gate the modules everything else depends on".
`maxterrainspritetypes`, `maxunittypes`, `dialog` and `addmounttype` are the whole of it at the top,
along with `userdict`, which is not an operator at all — `gs\editor.gs` defines it.

The **park trigger** the previous assessment set — park when `engine-operator` becomes the largest
first-blocker class — has **not** fired. It is 98/139/157 per profile against 679/735/858 for the
two script-definition classes combined. But the trigger should be read alongside the row above it:
the reason the script-definition class stays large is that its members are themselves waiting on a
few engine operators, so the classes are not as independent as the trigger assumes.

## Stop/go assessment

**Go, and the scope is now measured rather than estimated.**

- every known script tokenizes with a small, bounds-checked implementation, across all three line-ending conventions;
- 1,551 of the corpus's 4,692 `.gs` members — 429/635/487 across the three profiles — execute to
  completion with **no** engine support at all, and 1,682 with a two-module declared preload;
- `gs\standard.gs` loads and 31 stated expectations over its utility procedures all hold, including three that contradicted a reading of the source;
- the broad engine-name heuristic is now resolved: ~98.5% of it is real, ~33 names per profile are coincidence;
- unknown engine names stop with a trace naming the operator's entry point, no host call has been guessed, and no arithmetic result is invented either;
- execution is bounded in steps, call depth and allocation, so one pathological member cannot take down a survey of the whole archive.

The residual risk is unchanged in kind and smaller in size: the native host surface is 1,414 operators in 3.02, and nothing here executes any of them.

## Next slice and gate: continue for module loading

**Continue.** The evidence is *not* "the largest blocking group is a script definition" — that sentence rested on a case fold and does not survive fixing it. Corrected, the two candidate groups are a near tie: **564** members stop first on a name another member defines, **556** on an engine constant, and discounting `userdict` crosses them at 523 against 556.

What actually decides it is the third row, which no correction touched: only **124** of 1,267 members stop first on a real engine operator, and **23** on a name nothing in the corpus or the binary explains. Both of the large groups are cheap — a `run` that reads the archive, and a declared constant table — and neither requires simulating anything. The expensive class is small and stayed small.

The three "unresolved" names the `standard.gs` battery hit are of the cheap kind (`build_statement` in `gs\text.gs`, `sysdlg` in `gs\dlg\sysdlg.gs`, `unitdictxref` in `units\easyunit.gs`), though `sysdlg`'s own definition needs the `dialog` operator, so loading relocates that one rather than removing it. The static `run` reference graph already resolves against the archive.

The next slice is therefore:

1. `run` backed by the archive, with a load set and cycle detection, so `"gs/standard.gs" run` resolves inside the VM;
2. a declared engine-constant table — 556 members stop first on one, and they are faith and resource names such as `GOLD`, `ORDER`, `AIR`, matched **case-sensitively**;
3. a per-module load report, **not** a boot. `START.GS` reaches `protectdictstack` and `dialog` almost immediately, so attempting to load the tree as the engine does will stop early and tell us little.

Constants must be *declared*, with their values stated as inputs, exactly as `--stub` already works. An engine constant whose value is invented is the same failure mode as an invented host call, one step further from being noticed.

**Park if** the cheap classes stop dominating. Concretely: re-run `--survey` after each of the two steps above and park when `engine-operator` becomes the largest first-blocker class. It is 124 of 1,267 today. The trigger is stated against `engine-operator` rather than against which of the two cheap classes is larger, because that comparison is a near tie between two heuristics and it already moved once under correction.

**Update, 2026-09-18.** Both steps are now done and the trigger has not fired — `engine-operator` is
98/139/157 per profile. But the reasoning above needs one correction, recorded in
[What this refutes](#what-this-refutes): about half of the `defined-in-another-member` class is
**not** unblocked by module loading, because the defining module stops on an engine operator of its
own. The classes are coupled, so the next slice is stubbing the few operators that gate the shared
modules — `maxterrainspritetypes`, `maxunittypes`, `dialog`, `addmounttype` — not loading more.
