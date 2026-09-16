# GameScript Language and Runtime Probe

## Status

**Lexical and vocabulary milestones complete; the engine's operator tables and their arity are recovered from the binary; the experimental VM core executes shipped utility code with native stubs.** What remains is loading a second engine-light module end to end ([issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5)). The native Rust tool tokenizes every named `.gs` member in the preserved baseline, 3.02, and GS5R3 archives, inventories names and static `run` references, and correlates executable tokens with strings embedded in each profile's `lomse.exe`. A deliberately small interpreter now supports enough stack, collection, dictionary, definition, procedure, conditional, and numeric behavior to load the 3.02 `gs\standard.gs` utility module and execute two of its procedures.

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
| Distinct executable names | 14,080 | 15,174 | 16,855 |
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

- `/name <value-or-procedure> ... def` within three tokens at the same nesting depth, covering
  `/NAME{...}def`, `/INSANE_LEVEL 3 def`, and `/a exch def`;
- `/name <value>` directly inside a `<< >>` dictionary literal, which is how scenario tables such as
  `gs\scenario\default.gs` declare entries.

Measured on GS5R3: 17,641 distinct literal names but only **13,609 definitions**, so 4,032 literals
were never definitions. The native-candidate count rises from 2,091 to **2,151**. The scan reports
`distinct-definition-names` alongside the literal count, and `LOM_CANDIDATE_LIMIT` raises the
50-line display cap for cataloguing the full vocabulary.

Recommended first VM stubs, all pure reads of game state with small return types:
`getdifficultylevel`, `getmultiplayerflag`, `getplayeraistatus`, `getarmydata`, `getunitdata`,
`getuniteffectivedata`. Stubbing those six lets the `gs\LEVLMODS5.gs` AI-bonus block execute
deterministically against a synthetic state.

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

Reconciling the 2,151-name candidate vocabulary against the table:

| Class | Count | Reading |
| --- | ---: | --- |
| Confirmed operators | 1,445 | present in the table with an entry point |
| SCREAMING_CASE | 671 | engine **constants** pushed by name, not operators |
| Remainder | 35 | see below |
| **Candidates** | **2,151** | |

The heuristic's stated weakness — *"can contain false positives from unrelated binary strings"* — is
now measured rather than assumed. Two thirds of the vocabulary are confirmed procedures, and almost
all of the rest are a category the heuristic could not distinguish: **constants are not operators**,
so they are absent from the table by construction rather than by error.

The 35-name remainder is the heuristic's actual error bar. Nineteen are `Type_*` engine type tags
(`Type_Imp`, `Type_Font`, `Type_EditBox`). The other sixteen are short, low-use names (`e1`, `uf`,
`hh`, `jx`, `rx`, `xp`, `ice`, `log`, `no`) that look like dictionary keys our definition-shape
classifier does not recognise as definitions. That is a **precision limit of the classifier**, not
evidence of engine surface, and it is the tightest bound we have on it: roughly 0.7% of candidates.

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
| Mean run length of dominant caller directory | 1.45 | 1.10 | 1.33x |

The first three say the ordering is real and strong. **The fourth says the obvious labelling axis is
the wrong one**, and that is worth as much as the positive results.

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
# extract the .gs members of gs.mpq into a directory first
python3 -m tools.operator_groups scan.txt /path/to/extracted/scripts
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

## Experimental VM checkpoint

The VM currently implements:

- numbers, booleans, strings, literal/executable names, procedures, arrays, and dictionaries;
- an operand stack, dictionary stack, name lookup, definitions, and procedure calls;
- bounded execution with a one-million-step default ceiling;
- array/dictionary construction plus `get`, `put`, and the observed procedure-metadata `replace` pattern;
- a small operator subset covering stack manipulation, numeric arithmetic/comparison, `if`, and `ifelse`;
- structured errors containing the unknown executable name, VM step, and procedure call stack.

The 3.02 `gs\standard.gs` member loads in 339 VM steps, leaves the operand stack empty, and defines 36 names. Evaluating `3 5 min 3 5 max` afterward executes the real script-defined `min` and `max` procedures and leaves numeric values `3` and `5` on the stack.

This is intentionally not a general GameScript VM yet. Collection mutability and procedure metadata use a minimum model sufficient for the observed utility-module load, and many operators, object types, file/module behavior, and native host calls remain absent.

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

## Stop/go assessment

**Go for the next bounded VM experiment.** The first interpreter checkpoint also passes. Reasons:

- every known script can be tokenized with a small, bounds-checked implementation;
- module loads and definition/call vocabularies are statically discoverable;
- the three preserved lineages can be measured with one tool;
- malformed source fragments can be isolated without weakening fatal byte/string checks;
- an actual shipped utility module loads without stack residue, and two script-defined comparison procedures return the expected values.

The principal risk remains the native host surface. The current executable-name heuristic is too broad to support a full-engine estimate or a Stage 2 completion claim.

## Next slice and gate

The continuing checkpoint is tracked in [GitHub issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5). Expand the interpreter only as required to classify and run additional engine-light utilities. Unknown executable names stop with a structured trace rather than being guessed. Use those failures plus static call sites to split the candidate vocabulary into:

- core language operators;
- script-defined procedures;
- native engine/UI/simulation procedures;
- constants or data names;
- heuristic false positives.

Continue toward read-only module loading now that deterministic synthetic fixtures and representative utility procedures pass. Park a complete VM if the next representative subsystem immediately requires a large, inseparable native game state.
