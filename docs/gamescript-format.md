# GameScript Language and Runtime Probe

## Status

**Lexical, vocabulary and language milestones complete; the engine's operator tables and their arity are recovered from the binary; the VM executes shipped utility code and stops, traceably, at the engine boundary.** What remains is module loading ([issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5)). The native Rust tool tokenizes every named `.gs` member in the preserved baseline, 3.02, and GS5R3 archives, inventories names and static `run` references, and correlates executable tokens with strings embedded in each profile's `lomse.exe`. The interpreter implements 68 language primitives plus GameScript's two non-PostScript features — procedure locals and typed dictionary keys — which is enough to load 3.02's `gs\standard.gs` and run its whole utility surface, and enough for 406 of that profile's 1,681 members to execute with no engine support at all. The candidate vocabulary is now partitioned against the operator table rather than guessed at.

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
and not as ordinary names. Both of ours already do — checked, not assumed: `tools/gs_syntax.py`
tokenises them separately, and `gamescript_vm.rs` opens and closes a `CollectionKind::Dictionary` on
them.

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

## Line endings: bare CR is a line ending here

`gs.mpq` mixes all three conventions. In a local GS5R3 install: **1,123 members CRLF, 242 bare CR, 63 bare LF**, and 501 with no line terminator at all (the `fonts\*.gs` members are single-line). Evidence class: Observed in a local binary.

This is not a cosmetic detail. A `;` comment runs to the end of its line, so a reader that splits on LF sees a single comment swallow dozens of live statements — which is how this project once harvested commented-out code as if it were bindings. The lexer terminates comments at `\r` or `\n`, counts a bare CR as a line ending, and counts a CRLF pair once; `a_comment_ends_at_a_bare_carriage_return` and `line_numbers_count_bare_carriage_returns_and_pair_crlf` fail if either rule is removed.

The corpus relies on it. GS5R3's `gs\standard.gs` comments out its inherited `min`/`max` at lines 68 and 70 and redefines them at 73 and 74; reading the commented pair as live would give the wrong bodies.

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

The interpreter implements 68 language primitives, listed in `gamescript_vm::PRIMITIVE_NAMES`:

- **stack** — `dup pop exch copy index roll count clear`
- **arithmetic** — `add sub mul div idiv mod neg abs sqrt sin cos round truncate floor ceiling bitshift`
- **comparison and logic** — `eq ne gt lt ge le and or xor not`
- **control flow** — `if ifelse repeat for loop exit forall exec`
- **aggregates** — `array dict string [ ] << >> get put length known undef load currentdict begin end def replace`
- **conversion** — `cvx cvlit cvi cvr cvs type bind`

`sin` and `cos` take **radians**, unlike PostScript's degree-taking pair: `standard.gs` defines `/radians {180 div 3.141596 mul}` and applies it before every call. Evidence class: Inferred.

`PRIMITIVE_NAMES` is checked against the dispatch itself by a test that reads the match arms back out of the source, because the vocabulary classification below calls a name a native host call precisely when the engine lists it and this VM does not implement it.

### The `standard.gs` battery

`cargo run --example gamescript_standard` loads 3.02's `gs\standard.gs` (339 VM steps, 37 names, empty operand stack) and then runs 31 exercises whose expected stacks were worked out from the shipped bodies rather than recorded from output. **All 31 behave as expected**: 22 produce a stated stack, and 9 stop on a named engine call. Six were wrong on the first run and the run said so, which is the point of stating the expectation first.

Three findings fell out of executing rather than reading:

1. **`get_if_known`'s header comment states its operands backwards.** The comment says `;val dict key get_if_known`; the code's `3 1 roll` requires `dictionary key default`. Evidence class: Corrected.
2. **`dump_flags` does nothing to its operand.** After the first iteration its `2 copy` reads the loop counter rather than the flags, and the trailing `pop` discards the one index it kept. `5 dump_flags` leaves `5`. Evidence class: Observed in a local binary, by execution.
3. **GS5R3 ships `min` and `max` swapped.** Vanilla and 3.02 both define `/min{2 copy gt{exch pop}{pop}ifelse}`. GS5R3 comments that line out and redefines the gt-body as `max` and the lt-body as `min`, so under GS5R3 `3 7 min` is `7` and `3 7 max` is `3`. The same battery run against GS5R3 flags exactly those two exercises. Evidence class: Observed in a local binary. (GS5R3 also has no `string_cvi` or `char_cvs`; those are 3.02 additions.)

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

The three unresolved ones are **not** engine calls: `build_statement` is defined in `gs\text.gs`, `sysdlg` in `gs\dlg\sysdlg.gs`, `unitdictxref` in `units\easyunit.gs`. They are the module-loading gap, not the host-API gap.

`standard.gs`'s whole static debt to the engine is 21 names — 17 in the operator table, 4 unresolved.

## Vocabulary classification

`cargo run --example gamescript_vocabulary` partitions every distinct executable name in a profile, in the order the interpreter resolves: script definition, then language primitive, then native host call, then constant, then nothing. It writes `reports/gs/vocabulary-<profile>.tsv` (name, uses, class, whether the old broad heuristic admitted it, operator entry point).

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| script-definition | 10,365 | 10,967 | 12,417 |
| language-primitive | 55 | 55 | 54 |
| native-host-call | 1,383 | 1,414 | 1,390 |
| constant-or-data | 647 | 734 | 675 |
| heuristic-false-positive | 1,630 | 2,004 | 2,319 |
| **distinct executable names** | **14,080** | **15,174** | **16,855** |

Restricted to the broad "likely hardcoded engine name" candidate list, which is what issue #5 asked to partition:

| Class | vanilla | patch302 | gs5r3 |
| --- | ---: | ---: | ---: |
| language-primitive | 55 | 55 | 54 |
| native-host-call | 1,383 | 1,414 | 1,390 |
| constant-or-data | 643 | 709 | 670 |
| heuristic-false-positive | 32 | 33 | 34 |
| **broad candidates** | **2,113** | **2,211** | **2,148** |

So **roughly 98.5% of the broad candidate list is real** — a primitive, an operator the engine registers, or a constant — and about 33 names per profile are coincidences. `script-definition` is zero there by construction: the heuristic already excludes anything the corpus defines.

The full-vocabulary `heuristic-false-positive` class is the interesting residue: names the corpus calls, nothing defines, the engine does not register, and which are not constant-shaped. It is dominated by cross-member references the definition scanner still cannot see (`build_statement`, 340 uses, is defined in `gs\text.gs` with its value spanning more than the attachment window) and by mixed-case engine type names such as `Type_GraphicPage` that `is_screaming_case` does not match.

Names the corpus defines that the engine *also* registers are reported separately — 15 in 3.02, including `exec`, `ne`, `type`, `run` and `sleep`. The dictionary wins at run time, so these are script overrides of engine behaviour.

## How much of the corpus is pure language

`--survey` loads every `.gs` member on its own machine and records what stopped it. On 3.02's 1,681 members:

| Outcome | Members |
| --- | ---: |
| loaded to completion with no engine support at all | 406 |
| stopped on a name | 1,267 |
| failed some other way | 8 |

Of the 1,267 stops, classified by what the **first** blocking name is:

| First blocker | Members |
| --- | ---: |
| defined in another member | 640 |
| engine constant | 480 |
| engine operator | 124 |
| unresolved | 23 |

Read that as a lower bound on what loading would unblock, not a projection: resolving a member's first blocker only reveals its second.

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
cargo run --release --example gamescript_standard -- \
  --gs '/path/to/English/gs.mpq' --exe '/path/to/English/lomse.exe' --survey

# Classify a profile's whole executable vocabulary and write the derived table.
cargo run --example gamescript_vocabulary -- \
  --profile patch302 --gs '/path/to/English/gs.mpq' --exe '/path/to/English/lomse.exe' \
  --out ../../reports/gs
```

## Stop/go assessment

**Go, and the scope is now measured rather than estimated.**

- every known script tokenizes with a small, bounds-checked implementation, across all three line-ending conventions;
- 406 of 3.02's 1,681 `.gs` members execute to completion with **no** engine support at all;
- `gs\standard.gs` loads and 31 stated expectations over its utility procedures all hold, including three that contradicted a reading of the source;
- the broad engine-name heuristic is now resolved: ~98.5% of it is real, ~33 names per profile are coincidence;
- unknown engine names stop with a trace naming the operator's entry point, and no host call has been guessed.

The residual risk is unchanged in kind and smaller in size: the native host surface is 1,414 operators in 3.02, and nothing here executes any of them.

## Next slice and gate: continue for module loading

**Continue.** The measurement that decides it: of the 1,267 members that stop on a name, the single largest group — **640** — stops on a name **another member of the same archive defines**. Those are resolved by loading, not by implementing engine behaviour. The three "unresolved" names the `standard.gs` battery hit are all of this kind (`build_statement` in `gs\text.gs`, `sysdlg` in `gs\dlg\sysdlg.gs`, `unitdictxref` in `units\easyunit.gs`), and the static `run` reference graph already resolves against the archive.

The next slice is therefore:

1. `run` backed by the archive, with a load set and cycle detection, so `"gs/standard.gs" run` resolves inside the VM;
2. a declared engine-constant table — the second-largest blocking group, 480 members, is faith and resource names such as `GOLD`, `ORDER`, `AIR`;
3. a per-module load report, **not** a boot. `START.GS` reaches `protectdictstack` and `dialog` almost immediately, so attempting to load the tree as the engine does will stop early and tell us little.

Constants must be *declared*, with their values stated as inputs, exactly as `--stub` already works. An engine constant whose value is invented is the same failure mode as an invented host call, one step further from being noticed.

**Park if** the next subsystem's first blocker stops being a script definition. The moment the dominant blocker class flips from `defined-in-another-member` to `engine-operator`, further VM work is buying simulation, not loading, and the honest move is to stop.
