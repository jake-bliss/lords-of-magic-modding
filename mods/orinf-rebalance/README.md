# orinf-rebalance

The source files under `archives/` are **not committed**: they are derived from the installed game
and this repository stores no game content. `mods/.gitignore` excludes them.

Seed the tree from your own baseline install, then edit the file in place:

```sh
scripts/mod-seed.sh mods/orinf-rebalance 'gs.mpq:units\orinf.gs'
scripts/mod-validate.sh mods/orinf-rebalance
scripts/mod-build.sh mods/orinf-rebalance
```

`mod-seed.sh` writes the member's bytes exactly as the archive holds them, so the first
`mod-validate.sh` after seeding reports an unchanged member and no findings. That is the control:
it establishes that the pipeline agrees with the archive *before* anything is edited.

## Editing the file

`units\orinf.gs` in the baseline is **1,798 bytes on a single line with no trailing newline** and
is pure printable ASCII. An editor that adds a final newline, reflows it, or saves it as UTF-8
produces a different artifact from the one you meant to make. Validation reports each of those as a
named finding rather than repairing it -- see "Encoding" in `docs/build-pipeline.md` -- but the
easiest way to avoid them is a byte editor or `python3 -c` rather than a text editor.
