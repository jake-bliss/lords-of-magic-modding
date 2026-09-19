# Inside the native operator bodies

Until now the engine's host API was known from the outside: 1,906 names, 1,906 entry points, and a
static count of operand-stack traffic (PRs #13/#15). Nobody had read the function bodies. This is
the first pass through them.

- Analyser: `spikes/asset-viewer/src/operator_bodies.rs`
- Driver: `spikes/asset-viewer/examples/operator_bodies.rs`
- Artifacts: [`reports/natives/operator-bodies.tsv`](../reports/natives/operator-bodies.tsv),
  [`reports/natives/global-clusters.tsv`](../reports/natives/global-clusters.tsv),
  [`reports/natives/summary.md`](../reports/natives/summary.md)
- Regression anchors: `spikes/asset-viewer/tests/operator_bodies.rs`

```
cargo run --release --example operator_bodies -- <lomse.exe> --out reports/natives
cargo run --release --example operator_bodies -- <lomse.exe> --function 0x004b6020   # one body
cargo test --release                                     # offline anchors
LOM_EXE=<lomse.exe> cargo test --release -- --ignored     # checks against the binary
```

Everything that is an address, a count, a call edge or an import is **observed in a local binary**.
Every reading of *what a cluster is* is **inferred**, and is marked. Five operand counts are
**observed in the shipped script corpus** and are the only independent authority here.

## Checked against the engine's own callers

The `.gs` corpus is the engine's own caller, so a recovered operand count is a falsifiable
prediction about it. Read with `tools/gs_callsites.py`, which resolves every name in the operand
window against the corpus's own definitions:

| Operator | Call site | Operands there | This analysis | Previously recorded |
| --- | --- | ---: | ---: | ---: |
| `xywh` | `PANELS5.gs:51:497` | 4 | **4** ✓ | 4 ✓ |
| `addcitymod` | `gs/spells/fireworks.gs:1:1129` | 9 | **9** ✓ | 2 ✗ |
| `bargraph` | `selarmy2.gs:228:79` | 8 | **8** ✓ | 2 ✗ |
| `setbuildingrequirements` | `building.gs:549:31` | 8 | **8** ✓ | 1 ✗ |
| `getplayergroupintoformation` | `getinfrm.gs:1:1079` | 8 | **8** ✓ | 1 ✗ |

**Re-run 2026-09-18, after five tokenizer fixes, and unchanged.** This table is read through
`tools/gs_callsites.py`, which tokenizes with `tools/gs_syntax.py`, and that tokenizer's `;`
comment, string-escape, `/`-separator, whitespace and parenthesis rules all changed that day. Every
operator cited here — plus `launchmissile` and `relative2actual` below — was re-run with the
tokenizer immediately before those fixes and immediately after, over the same GS5R3 extraction: the
reports are **byte-identical**, call-site counts and operand windows alike. That is a measurement,
not an inference from which members changed. The five members the string fix moved are
`Dlg\lib_dlg.gs` (vanilla and 3.02), `standard.gs` (3.02), and `dlg\LIB_DLG5.gs` and
`dlg\lib_dlg.gs` (GS5R3); the `/` fix moved those four and `spells\AIR\chain_lightning2.gs`. None
holds a call site cited here, which is why nothing moved — but the re-run is what establishes that,
and naming the members is what lets a reader check it.

The re-run did surface one number below that **could not be reproduced**, and it is not the
tokenizer's doing: `launchmissile` reports 4 call sites in vanilla, 7 in 3.02 and 5 in GS5R3 —
16 in total, or fewer if a pooled dump loses same-named members — against the "twelve" recorded
further down. The counts are identical before and after the fixes, so the discrepancy predates
them and is about which corpus that paragraph was measured over. Recorded here rather than quietly
rewritten, because the 21-operand conclusion it supports does not depend on the count and the
count cannot be re-derived without knowing the dump.

The fifth needs one substitution to read: `myowner 1 50 15 relative2actual 50 50 relative2actual
3 0`, and `relative2actual` is itself an operator taking two and returning two, which this table
also says — so the site passes 1+1+2+2+1+1 = 8.

**`launchmissile` is corroborated but not settled.** The prediction is 21. Three of twelve call
sites pass exactly 21 tokens (`SPELTOOL50.gs:2060:74`, `testspel.gs:46:351`,
`dragon_breath_old.gs:68:3`) and every token in them resolves to one value. The other nine sit
inside procedures the engine invokes with operands already on the stack — `gs/fireball.gs` shows
ten visible tokens — so they neither confirm nor refute. What all twelve do settle is that **no call
site passes 2**, which is the number previously recorded.

These five rows are pinned in the test file with the member and offset, so a change to the analyser
that breaks agreement with the corpus fails rather than being noticed later.

## What the controls said

Run against operators whose behaviour is known from outside the binary — from writing map files and
watching the engine load them.

| Control | Known from | Result |
| --- | --- | --- |
| `savescenariomap` and `savespecialmap` write byte-identical files | writing both and diffing | **Reproduced.** Both name object `0x005aa12c` and both call `0x00485550`. They cannot each have their own serialiser, and they do not. |
| The map operators act on one thing | map editing | **Reproduced.** `anythingat`, `armyat`, `buildingat`, `cityat`, `resetvisibility`, `setterrain` and `terrainspriteat` name exactly one address in common: `0x005ae958`. 139 operators name it. |
| `resetvisibility` is nullary | the `terrainrings` probe | **Reproduced.** |
| `drawimpframe` takes 6 and `getimphotspot` 5 | read by hand during #15 | **Reproduced** by a different method. |
| `resetvisibility` clears cell bit `0x00800000` | attended engine run | **Not reproduced.** |
| `setterrain` mutates the map | map editing | **Not reproduced as a mutation.** `setterrain` is `reads-state`: it names the map object, calls a method on it, and stores nothing itself. |

## The limitation: the engine mutates through C++ methods

The engine is C++ with singletons in `.data`. An operator's body is: fetch operands, `mov
ecx,<singleton>`, call a method. The store happens frames down, through `this`, and never touches an
absolute address on the writing side. Both failed controls are that one limitation.

Two tempting substitutes were measured and rejected.

**Import evidence stops being evidence after two calls.** The reach curve is printed in the summary.
`window-input`, `time` and the unclassified `other` bucket all reach **93% at depth 3**; `file-io`
reaches **90% at depth 5** and Storm **94%**; the allocator reaches **94% at depth 6**. Those columns
describe how densely the call graph is connected and say nothing about any operator. At depth 1 the
same rows are 1%, 3% and 2%, and that is the column the classifier reads. The cost is explicit:
`savescenariomap` genuinely writes a file and is **not** labelled `file-or-resource-io`, because it
first reaches a file import at depth 5 — where `dup` reaches one too.

**Attributing a callee's store-through-`this` to the caller's singleton is true of 57% of
operators**, because the engine's getters cache into their own object. Folding it in would classify
**1,336 of 1,906 (70%)** as mutators. It is carried as its own column, `calls_mutating_method`, and
never promoted.

So in the table, **`mutates-state` means this body stores to engine state itself** — an absolute
store, or a store through a pointer it read out of a global, which is how the engine's heap arrays
are reached. **`reads-state` means this body performs no store of its own. It is not a claim that
the operator is free of side effects**; `setterrain` is the standing counter-example and is in the
table as `reads-state`. `state_write_depth` and `calls_mutating_method` are there to be filtered on
when the weaker evidence is what you want.

This was wrong in the first version of this work in the *other* direction too:
`MutatesEngineState` was granted on a direct callee's store, which made the class false for 154 of
its rows — `armycanmove?`, `armystrength`, `ambientlight` and other predicates. Somebody filtering
for the state-editing API got predicates and still did not get `setterrain`.

## What can be named even when it cannot be followed

A virtual call cannot be followed. It can be *identified*: the analyser keeps the taint chain from
`mov ecx,[global]` through `mov eax,[ecx]` to `jmp [eax+0x58]`, so both the object and the vtable
byte offset come out. Twenty operators carry at least one, and the network family resolves into a
partial vtable map of the session object at `0x005d1e84`:

| Slot | Operator |
| --- | --- |
| `+0x08` | `createnetworkgame` |
| `+0x0c` | `joinnetworkgame` |
| `+0x10` | `modemcreate` |
| `+0x14` | `modemdial` |
| `+0x18` | `enumnetworkcomputers` |
| `+0x1c` | `enumnetworkgames` |
| `+0x20` | `selectprovider` |
| `+0x48` | `enumproviders` |
| **`+0x58`** | **`netlockgame`** |
| `+0x64` / `+0x68` | `getproviderbackground` / `setproviderbackground` |
| `+0x6c` / `+0x70` | `getproviderbuttontexture` / `setproviderbuttontexture` |

`netlockgame` is also the **one** body in 1,906 the walk cannot finish, and its six instructions are
why:

```
mov ecx,[5D1E84h]      ; the session object, or null
test ecx,ecx
je  short <ret>        ; no session: do nothing at all
mov eax,[ecx]          ; vtable
jmp dword [eax+58h]    ; nullary tail call into slot 22
```

So it is nullary, it is a pure forward, and it is a no-op when there is no session. `+0x24`
through `+0x54` are slots no operator reaches directly — which is where a turn-synchronisation
method with no script-visible name would sit.

## Function boundaries

Recursive descent from the entry point. A run ends at `ret`, at `int3` padding, at an unresolvable
indirect jump, or at a `jmp` to an address the program calls from elsewhere — a tail call. Jump
tables of the form `jmp [index*4 + table]` are followed.

| | |
| --- | ---: |
| walked to completion | 1,905 of 1,906 (**99.9%**) |
| ended at an unresolvable indirect jump | 1 (`netlockgame`) |
| decoded past the next operator entry point | 56 (**2.9%**) |

The 2.9% is an upper bound on boundary failure, not a count of failures: all 56 are thunk-style
operators whose next table entry is sixteen bytes away and whose implementation is tens of kilobytes
off, reached by an internal `jmp`. For the same reason the table's extent column is named
`decoded_extent_bytes` and not a body size — `abs` decodes 77 instructions across 15,207 bytes. The
instruction count is the size measure.

## Operand counts, and 73 disagreements with the recorded arity

`operator_arity` counts *sites* that commit a pop. This analysis labels every instruction with the
number of operands consumed before it and reads the answer off the `ret`s. The difference matters
because the engine's operand fetch is fetch-or-fail: every fetch checks for underflow and the
failure path consumes nothing, so a body with four fetches has five distinct paths and a site count
describes none of them.

| | |
| --- | ---: |
| one count on every returning path | 544 |
| paths disagree; the nominal count is the successful path | 1,362 |
| **nominal count higher than the recorded site count** | **70** |
| **nominal count lower** | **3** |

The direction is the expected one: `operator_arity`'s own module documents that it undercounts
operators that fetch through a helper. Largest gaps: `launchmissile` 21 against 2 (18 distinct fetch
sites chained down one path), `lightning` 16 against 2, `addbuildinginfo` 15 against 6, `toptriangle`
11 against 3. `getimphotspot` comes out at 5 and `drawimpframe` at 6, the values read by hand in #15.

The three in the other direction — `button`, `setunitdata`, `nsetunitdata` — pop different numbers
on different branches, where a site count is the larger by construction. They are named in the test
rather than excused.

### Variadic, and what is not variadic

Twenty-nine operators consume operands **inside a loop**, so no finite count describes them:
`astore`, `container`, `setformation`, `setregionfaiths`, `setregionraces`, `slider`, the alarm
family. The evidence is not the analogy with PostScript's `astore` — though that fits. It is that
the candidate counts form an **arithmetic progression**: `armyexpense` and `repoman` show
1, 6, 11, 16 … 76, a step of five, and `combat_controltarget` steps by two. A set of converging
early-exit paths cannot produce that; only a loop popping k operands per iteration can.

Loops are found by strongly-connected components over the walked instructions, not by "the
successor sits at a lower address". That proxy looked like a back edge and was not one — the
compiler places the shared error epilogue below the code that jumps to it — and it declared 26
operators variadic that fetch through a per-subsystem wrapper, which then contributed nothing to
their callers and brought them back as nullary.

A separate flag, `operand_state_cap_hit`, records 31 operators where many counts converge on one
instruction without any loop. `launchmissile` is one and keeps its count of 21; `slider` is a loop
and has none. The two conditions were reported under one flag and are now separate columns.

### What the helper search is and is not load-bearing for

The engine has **five** operand-fetch helpers, not the one `main.rs` warns about: a `thiscall` fetch
returning the raw `(tag, value)` pair and four `cdecl` fetches that coerce on the way out. There is
one result-push helper. All six are found *by shape* — a small function many operators call whose own
body pops exactly once and pushes nothing — and not by address.

What that buys was measured rather than asserted. Collapsing the search entirely changes:

| column | rows changed |
| --- | ---: |
| `helper_pops` | 318 |
| `behaviour` | 44 |
| `nominal_arity` | **3** |

So the helper *set* is what makes `helper_pops` and the operand-only/floating-point classes correct,
and the **arity result rests on the generic callee-pop folding**, which credits a call with the
operands the callee's own body consumes whether or not that callee was recognised as a helper. An
earlier iteration of this work, before that folding existed, reported 144 operators as nullary that
are not; that number is a fact about that iteration and not about the shipped analyser. A sixth
helper of identical shape at `0x0043da90` — called by six operators, below the sixteen-caller
threshold — is counted through the generic path, which is why `launchmissile`'s three calls to it
still land in the total.

## The clusters: what the engine's `.data` is made of

Every absolute data address a body touches is recorded, including a displacement behind a register
(an indexed table read) and an address materialised as an immediate (`mov ecx,<singleton>` — how the
engine reaches its objects, with no memory operand at all). Addresses within `0x100` of each other
are one cluster, and a read-only run is never merged with a writable one. The gap is not fitted: the
summary prints the sweep — 284 clusters at `0x20`, 152 at `0x100`, 85 at a page. (The read-only
split raised every count: constants no longer merge into the writable runs beside them.)

The readings in the right-hand column are **inferred** from the names of the operators in each
cluster. The ranges and counts are observed.

| Range | Operators | Inferred subject |
| --- | ---: | --- |
| `0x005aa08c`–`0x005aa264` | 350 | The scenario/game object and its neighbours. `0x005aa12c` alone is named by **299** operators — `addbuilding`, `addcapitol`, `buybuilding`, both map writers. |
| `0x005ae880`–`0x005aea70` | 276 | The world/map object. `0x005ae958` alone is named by **139** — `anythingat`, `armyat`, `buildingat`, `cityat`, `cantmovehere`, `setterrain`, `terrainspriteat`, `resetvisibility`. |
| `0x005a7b50`–`0x005a7da8` | 257 | Armies, animation and the view. Includes `0x005a7d8c`/`0x005a7d90`, the map-instance index and a 1,024-byte-stride table that `resetvisibility` reads. |
| `0x00584ae0`–`0x00584af0` | 128 | Four addresses, 16 bytes: the render targets. `blackbackbuffer`, `blackrenderbuffer`, `ambientlight`. |
| `0x005cd18c`–`0x005cd358` | 88 | Type registries — `addauratype`, `addmissiletype`, `addmounttype`, `addbuildinginfo`. |
| `0x005876c4`–`0x00587720` | 38 | Camera and lighting — `cameraposition`, `cameraorientation`, `calclighttables`. |
| `0x005853ec`–`0x00585588`, `0x0058676c`–`0x005867c0` | 30, 31 | Audio — `fadeoutmusic`, `getmusicvolume`, `loadstaticsound`, `interruptsound`. |
| `0x0054dbc0`–`0x0054dd38` | 28 | **Read-only.** Not state at all: the float pool `abs`, `add`, `atan`, `cos`, `cvf`, `cvr` share. |
| `0x005d1e84` | 27 | The DirectPlay session object — and now its vtable, above. |

Clusters marked read-only in the artifact are constants. Some of them sit in `.data` rather than
`.rdata`, because this linker put string literals there; a NUL-terminated printable literal is
treated as a constant wherever it was placed.

### The bug that hid the float pool

`is_data_address` originally tested only "not executable". The engine's `.rdata` is `0x40000040` and
its `.data` is `0xc0000040`, so the float pool counted as engine state, `abs` and eighteen others
were published as `reads-state`, and the class that describes them had **zero** members in a
1,906-row table — which should have been the tell. The check is now
`is_writable_data_address`, gated on `IMAGE_SCN_MEM_WRITE`, and the synthetic test fixture now has a
real read-only section so the mistake cannot come back unnoticed.

## Behaviour classes

| class | operators | share | meaning |
| --- | ---: | ---: | --- |
| `reads-state` | 1,094 | 57.4% | touches engine data, stores none of it itself |
| `mutates-state` | 334 | 17.5% | stores to a global, or through a pointer read out of one, **in this body** |
| `unknown` | 270 | 14.2% | 1 unfinished walk; 269 bodies that reference no data of their own and only forward |
| `file-or-resource-io` | 86 | 4.5% | reaches a file or Storm import within one call |
| `operand-only` | 81 | 4.2% | acts on its operands and the operand stack; no engine state, and no call to anything that touches it |
| `floating-point` | 26 | 1.4% | `operand-only` and using the x87 unit |
| `stub` | 13 | 0.7% | the body is a single `ret` |
| `rendering` | 2 | 0.1% | reaches GDI or DirectDraw within one call |

Three of these were wrong before and are worth reading as corrections.

**`operand-only` is honestly named; `stack` was not.** The class was called `stack` and contained no
stack primitives: twelve of its members shared one entry point that is a single `ret`. Meanwhile
`dup`, `exch`, `pop` and `roll` sat in `unknown`, because each calls the script error raiser at
`0x004d4550` and that counted as "calls something". Callees are now discounted when they cannot
distinguish one operator from another — the shared operand helpers, any callee that more than half
the table calls, and any leaf that references no data and reaches no import. That is the same
saturation argument the import depth is chosen by, applied to callees. The first attempt at this
rule was written from a guess that the error raiser touches no globals; measuring it with
`--function 0x4d4550` showed it references the engine's error-message objects, so the guess never
held.

**`floating-point` is named for the evidence, not the intent.** It is `operand-only` plus at least
one x87 instruction. That is usually a numeric operator — `abs`, `atan`, `sqrt`, `add` — but `sleep`
is in the class because it coerces a float delay with `fld`. Calling the class `arithmetic` claimed
more than was observed.

**Thirteen operators are `stub`: a registered name with a single `ret` behind it.** Twelve share one
entry point — `savegridflags`, `sunlight`, `makelighttables`, `savecitycombatregions`, `setplane`,
`sprite`, `test`, `inittest`, `pointslope`, `getfinescrolltime`, `bugkeyenable`,
`setstreambuffersize` — and `dumpqueue` has its own. Scripts can call them and nothing happens.

## What this does not establish

* No claim is made about what any called C++ method does. The call graph is direct calls only.
  Virtual dispatch is counted, and named by object and slot where the taint chain survives, but
  never followed.
* `reads-state` does not mean side-effect free.
* The `0x005ae958` / `0x005aa12c` readings are inferred from operator names. Nothing here identifies
  a field inside either object.
* Strings referenced by each body are counted and their addresses recorded; the text is deliberately
  not committed.
