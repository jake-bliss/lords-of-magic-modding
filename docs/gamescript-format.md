# GameScript Language and Runtime Probe

## Status

**Lexical and archive-wide vocabulary milestone complete; experimental VM core started.** The native Rust tool tokenizes every named `.gs` member in the preserved baseline, 3.02, and GS5R3 archives, inventories names and static `run` references, and correlates executable tokens with strings embedded in each profile's `lomse.exe`. A deliberately small interpreter now supports enough stack, collection, dictionary, definition, procedure, conditional, and numeric behavior to load the 3.02 `gs\standard.gs` utility module and execute two of its procedures.

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
