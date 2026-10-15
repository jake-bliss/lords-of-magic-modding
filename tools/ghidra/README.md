# Ghidra import

`ImportKnown.java` is a headless GhidraScript that applies what this repository
already knows about `lomse.exe` to a Ghidra project, so the decompile shows names
instead of `FUN_xxxxxxxx` / `DAT_xxxxxxxx`. Tested on Ghidra 12.1.4.

## Run

The program must already be imported and analysed, and `RecoverFunctions.java`
should have run first (see below); the script only adds names, types and labels.

```bash
export JAVA_HOME=/opt/homebrew/opt/openjdk@21
/opt/homebrew/opt/ghidra/libexec/support/analyzeHeadless <project dir> <project name> \
  -process lomse.exe -noanalysis -scriptPath tools/ghidra \
  -postScript ImportKnown.java "$PWD"
```

The last argument is the repository root. Back up the project first: the script
writes to it. Re-running is safe and changes nothing the second time.

## What it applies

| Input | Result |
| --- | --- |
| `reports/natives/operator-bodies.tsv` | the function at `entry_point` is renamed `op_<name>` (created if Ghidra has none); `?` becomes `_p`, any other non-identifier character `_`. A second operator at the same address becomes a label. Renamed spellings are written to `tools/ghidra/operator-name-map.tsv`. Helper `call_targets` are not touched. |
| `reports/natives/state/structures.tsv`, `structure-fields.tsv` | one struct per base in `/LOM`, named `<Top evidence word>State_<base>` (`Object_` when there is no evidence; `_ind` suffix for `indirect` bases). Fields are `f_<offset>`, integer-typed by the widest observed width; size is `observed_extent`. A struct wholly inside a larger one is embedded in it as `n_<offset>`. Only outermost structs are applied at the base; the global is labelled `g_<name>`, or `gp_<name>` (pointer to the struct) for `indirect` bases. |
| `reports/natives/global-clusters.tsv` | `gcl_<start>` label at each cluster start |
| `reports/natives/direction-table.tsv` | `dir_table_<operand>_<va>` label and an `int[8]` |
| `tools/ghidra/known-symbols.tsv` | `address`, `kind` (`function` or `data`), `name` (`Class::method` becomes a namespace), `source` (`file:line` that ties the name to the address) |

An existing `/LOM` type that differs from the generated one is kept, and that struct and every struct only contained in it are reported and skipped. A struct that would overwrite code, a string, an array, or another `/LOM`
type is reported as `CONFLICT` and skipped; so is a function that already has a
non-default name.

`known-symbols.tsv` only takes a name where a document ties it to an address in
the same sentence or table row. Add a row with its source line; do not add guesses.

## Measure

```bash
/opt/homebrew/opt/ghidra/libexec/support/analyzeHeadless <project dir> <project name> \
  -process lomse.exe -noanalysis -readOnly -scriptPath tools/ghidra \
  -postScript DumpAll.java <out dir>
```

## Rebuild from scratch

Four steps, run from the repository root. `RecoverFunctions.java` must come
before `ImportKnown.java`, because the operator renames need a function at every
entry point. All paths below are placeholders.

```bash
export JAVA_HOME=/opt/homebrew/opt/openjdk@21
A=/opt/homebrew/opt/ghidra/libexec/support/analyzeHeadless
P=<new project dir>
$A $P lomse -import <path to lomse.exe>
$A $P lomse -process lomse.exe -noanalysis -scriptPath tools/ghidra -postScript RecoverFunctions.java "$PWD"
$A $P lomse -process lomse.exe -noanalysis -scriptPath tools/ghidra -postScript ImportKnown.java "$PWD"
$A $P lomse -process lomse.exe -noanalysis -readOnly -scriptPath tools/ghidra -postScript DumpAll.java <out dir>
```

`RecoverFunctions.java` adds the functions that auto-analysis never wraps
because they are reached only through tables or immediates:

| Step | Source |
| --- | --- |
| operator table | `entry_point` in `reports/natives/operator-bodies.tsv`; created without a plausibility check |
| data pointers | `.rdata` / `.data` dwords into `.text` that point at no function, when the byte before is `0x90`, `0xCC` or `0xC3`, or the three bytes before are a `ret imm16`, or the target is 16-aligned; a target that is itself padding is skipped; any other unaligned target is rejected as a constant |
| code operands | immediate or offset operands that point into `.text` outside any function and any defined data; operands of computed jumps and targets that start a run of `.text` pointers (jump tables) are skipped |
| all other candidates | must pass Ghidra's read-only `PseudoDisassembler.isValidSubroutine` before anything is disassembled; a failed creation clears only the addresses that candidate itself disassembled |
| call and tail-jump targets | from instructions inside a function; repeated until nothing new appears |
| aligned after padding | a 16-aligned, undefined, non-padding address after a padding byte that disassembles to at least 16 bytes |
| fragments | undefined non-padding bytes directly after a function's last byte (single `nop` after `ret`, SEH tails), at most 24 bytes, are disassembled and appended when they decode exactly |
| fixups | `tools/ghidra/recovery-fixups.tsv`, one reviewed row per repair (`entry`, `action`, `clear_start`, `clear_end`, `disassemble_at`, `reason`, `evidence`). The one action, `clear_and_refit`, clears unowned instructions in the given range, disassembles at `disassemble_at` and recomputes the function body in place. The only row is `0x544c10` (stray `.data` value `0x544d57`). A row is skipped if the function is missing or the range holds instructions owned by a function |

On `lomse.exe` this gives 6,671 decompiled functions (plus 107 thunks) after the three steps, 95.94% of
`.text` by function bytes.
