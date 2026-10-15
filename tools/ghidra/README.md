# Ghidra import

`ImportKnown.java` is a headless GhidraScript that applies what this repository
already knows about `lomse.exe` to a Ghidra project, so the decompile shows names
instead of `FUN_xxxxxxxx` / `DAT_xxxxxxxx`. Tested on Ghidra 12.1.4.

## Run

The program must already be imported and analysed; the script only adds names,
types and labels.

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

A struct that would overwrite code, a string, an array, or another `/LOM`
type is reported as `CONFLICT` and skipped; so is a function that already has a
non-default name.

`known-symbols.tsv` only takes a name where a document ties it to an address in
the same sentence or table row. Add a row with its source line; do not add guesses.

## Measure

```bash
/opt/homebrew/opt/ghidra/libexec/support/analyzeHeadless <project dir> <project name> \
  -process lomse.exe -noanalysis -readOnly -scriptPath <dir with DumpAll.java> \
  -postScript DumpAll.java <out dir>
```
