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
LOM_EXE=<lomse.exe> cargo test --release          # also checks the artifact is current
```

Everything below that is an address, a count, a call edge or an import is **observed in a local
binary**. Every reading of *what a cluster is* is **inferred**, and is marked.

## What the controls said

The analyser was run first on operators whose behaviour is already known from outside the binary —
from writing map files and watching the engine load them. Four of those checks it reproduces and two
it fails, and the failures are the more useful half.

| Control | Known from | Result |
| --- | --- | --- |
| `savescenariomap` and `savespecialmap` write byte-identical files | writing both and diffing | **Reproduced.** Both name the same object (`0x005aa12c`) and both call the same function (`0x00485550`). They cannot each have their own serialiser, and they do not. |
| The map operators act on one thing | map editing | **Reproduced.** `anythingat`, `armyat`, `buildingat`, `cityat`, `resetvisibility`, `setterrain` and `terrainspriteat` name exactly one address in common: `0x005ae958`. 139 operators name it. |
| `resetvisibility` is nullary | the `terrainrings` probe | **Reproduced.** Zero operands on every returning path. |
| `drawimpframe` takes 6 operands and `getimphotspot` 5 | read by hand during #15 | **Reproduced**, and by a different method: the path-sensitive count says 6 and 5 where the site count in `operator_arity` says 5 and 1. |
| `resetvisibility` clears cell bit `0x00800000` | attended engine run | **Not reproduced.** The body is seven instructions: read a map index, index a 1,024-byte-stride table at `0x005a7d90`, call one method on `0x005ae958`. The bit is cleared somewhere below that, and the static call graph does not get there. |
| `setterrain` mutates the map | map editing | **Not reproduced as a mutation.** `setterrain` is classified `reads-state`: it names the map object and calls a method on it, and performs no store of its own. See the limitation below. |

The last two are the same limitation seen twice, and it is the main thing to know before using the
table.

## The limitation: the engine mutates through C++ methods, so most mutation is invisible here

The engine is C++ with singleton objects in `.data`. An operator's typical body is: fetch operands,
`mov ecx,<singleton>`, call a method. The store happens one or more frames down, through `this`,
and never touches an absolute address on the writing side.

Two consequences, both measured rather than assumed:

* **Import evidence stops being a signal after about three calls.** The summary prints the reach
  curve. At depth 3, 93% of operators reach `user32`, 93% reach a timer and 93% reach the allocator;
  at depth 5, 90% reach `CreateFileA`. A classifier reading those columns would be describing the
  call graph and not the operator. The classifier therefore reads depth 1, where the shares are 1%,
  3% and 0%. That is why `savescenariomap` is **not** labelled `file-or-resource-io`: the archive
  and file imports only become reachable from it at depth 5, where they are reachable from `dup`
  too.
* **"Stores through `this`" is true of 55% of operators**, because the engine's getters cache into
  their own object. Folding it into "mutates engine state" relabelled two thirds of the API on
  evidence that does not distinguish anything, so it is carried as its own column
  (`calls_mutating_method`) and never promoted.

So `mutates-state` in the table means *this body stores to engine state itself*, and `reads-state`
means *this body performs no store of its own*. `reads-state` is **not** a claim that the operator
is free of side effects. `setterrain` is the standing counter-example and it is in the table.

What the analyser does catch is the store through a pointer loaded out of a global — `mov
eax,[global]` then `mov [eax+n],value` — which is how the engine reaches its heap-allocated arrays.
Without that, the map-editing half of the API classified as read-only.

## Function boundaries

Recursive descent from the entry point. A run ends at `ret`, at `int3` padding, at an unresolvable
indirect jump, or at a `jmp` to an address the program calls from somewhere else — a tail call.
Jump tables of the form `jmp [index*4 + table]` are followed.

| | |
| --- | ---: |
| walked to completion | 1,905 of 1,906 (**99.9%**) |
| ended at an unresolvable indirect jump | 1 |
| ran past the next operator entry point | 56 (**2.9%**) |

The 2.9% is the honest upper bound on boundary failure, not a count of failures: the operators are
not laid out contiguously and some genuinely tail call into code far away. The one incomplete body
is reported as incomplete rather than as a small one.

## Operand counts, and 71 disagreements with the recorded arity

`operator_arity` counts *sites* that commit a pop. This analysis labels every instruction with the
number of operands consumed before it and reads the answer off the `ret`s. That difference matters
because the engine's operand fetch is "fetch or fail": every fetch checks for underflow and the
failure path consumes nothing, so a body with four fetches has five distinct paths and a site count
describes none of them.

Two further things had to be found before the counts were usable, and both are in the module because
both were wrong first:

* **The engine has five operand-fetch helpers, not one.** A `thiscall` fetch returning the raw
  `(tag, value)` pair and four `cdecl` fetches that coerce on the way out. Counting only the first
  reported a third of the table as nullary. The helpers are found by shape — a small function many
  operators call whose own body pops exactly once and pushes nothing — not by address.
* **The dataflow has to propagate every count that can reach an instruction, not the first one.**
  Keeping one label per address made the answer depend on queue order: the underflow path of a
  four-operand operator reaches the `ret` first, and the operator came out nullary.

Results:

| | |
| --- | ---: |
| one count on every returning path | 544 |
| paths disagree; the nominal count is the successful path | 1,362 |
| operand count grows around a loop — genuinely variadic | 31 |
| **nominal count higher than the recorded site count** | **68** |
| **nominal count lower** | **3** |

The direction is the expected one: `operator_arity`'s own module documents that it undercounts
operators that fetch through a helper. The largest gaps are `launchmissile` (21 against 2, with 18
distinct fetch sites chained down one path), `addbuildinginfo` (15 against 6) and `toptriangle` (11
against 3). `getimphotspot` comes out at 5 and `drawimpframe` at 6, which are the two values read by
hand in #15 — the disagreement with the table was already known and is now measured.

The three in the other direction — `button`, `setunitdata`, `nsetunitdata` — pop different numbers
on different branches, where a site count is the larger number by construction. They are named in
the test rather than excused.

The 31 variadic operators include `astore`, `container`, `setformation`, `setregionfaiths` and
`setregionraces` — which is the answer PostScript's `astore` should give, and is the strongest
independent check on the loop detection available without the binary's source.

## The clusters: what the engine's `.data` is made of

Every absolute data address a body touches is recorded, including a displacement behind a register
(an indexed table read) and an address materialised as an immediate (`mov ecx,<singleton>` — how the
engine reaches its objects, with no memory operand at all). Addresses within `0x100` of each other
are one cluster. The gap is not fitted: the summary prints the sweep — 247 clusters at `0x20`, 86 at
`0x100`, 12 at a page — and `0x100` is where `.data` still resolves into distinguishable subjects.

The readings in the right-hand column are **inferred** from the names of the operators in each
cluster. The ranges and counts are observed.

| Range | Operators | Inferred subject |
| --- | ---: | --- |
| `0x005aa08c`–`0x005aa264` | 350 | The scenario/game object and its neighbours. `0x005aa12c` alone is named by 299 operators — `addbuilding`, `addcapitol`, `buybuilding`, both map writers. |
| `0x005ae880`–`0x005aea70` | 276 | The world/map object. `0x005ae958` alone is named by 139 — `anythingat`, `armyat`, `buildingat`, `cityat`, `cantmovehere`, `changecombatmap`, `setterrain`, `terrainspriteat`, `resetvisibility`. |
| `0x005a7b50`–`0x005a7da8` | 257 | Armies, animation and the view. Includes `0x005a7d8c`/`0x005a7d90`, the map-instance index and a table with a 1,024-byte stride that `resetvisibility` reads. |
| `0x00584ae0`–`0x00584af0` | 128 | Four addresses, 16 bytes: the render targets. `blackbackbuffer`, `blackrenderbuffer`, `ambientlight`. |
| `0x005cd18c`–`0x005cd358` | 88 | Type registries — `addauratype`, `addmissiletype`, `addmounttype`, `addbuildinginfo`. |
| `0x00572584`–`0x00572af0` | 72 | Map/location queries — `anythinglocation`, `anythingowner`, `army_en_route?`. |
| `0x0054dbc0`–`0x0054dd38` | 28 | `.rdata` constants, not state: `abs`, `add`, `atan`, `cos`, `cvf`, `div`, `eq`, `ge`. The arithmetic operators' float pool. |
| `0x005d1e84`–`0x005d1e88` | 27 | Two addresses reached only by `createnetworkgame`, `enumnetworkgames`, `enumproviders` — the DirectPlay session. |
| `0x005853ec`–`0x00585588`, `0x0058676c`–`0x005867c0` | 30, 31 | Audio: `fadeoutmusic`, `getmusicvolume`, `loadstaticsound`, `interruptsound`. |
| `0x005d2ca0`–`0x005d2d5c` | 21 | Script-callback slots: `getterrainsprite*proc`, `getdungeonarmyinstantiateproc`. |

The two largest clusters being the scenario object and the world object, in that order, is the
shape one would expect of this engine and is not an achievement on its own. What is new is that the
membership is now enumerable: the 139 operators on `0x005ae958` are the map API, recovered without
reading a single name.

## Behaviour classes

| class | operators | share | meaning |
| --- | ---: | ---: | --- |
| `reads-state` | 1,058 | 55.5% | touches engine data, stores none of it itself |
| `mutates-state` | 487 | 25.6% | stores to a global, or through a pointer read out of one |
| `unknown` | 256 | 13.4% | the body references no data of its own and only calls something |
| `file-or-resource-io` | 86 | 4.5% | reaches a file or Storm/MPQ import within one call |
| `stack` | 17 | 0.9% | no engine data and no call but the operand helpers |
| `rendering` | 2 | 0.1% | reaches GDI or DirectDraw within one call |

1,650 of 1,906 carry a class; 256 are `unknown`, and `unknown` here is a real answer — a body that
fetches operands and forwards them to one engine function tells you where to look next and nothing
about what happens there.

## What this does not establish

* No claim is made about what any called C++ method does. The call graph is direct calls only;
  virtual dispatch (`call [eax+n]`) is counted and not followed.
* `reads-state` does not mean side-effect free.
* The `0x005ae958`/`0x005aa12c` readings are inferred from operator names. Nothing here identifies a
  field inside either object.
* Strings referenced by each body are counted but their text is deliberately not committed; only the
  addresses and the count are in the artifact.
