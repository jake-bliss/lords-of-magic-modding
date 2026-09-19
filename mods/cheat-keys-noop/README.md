# cheat-keys-noop

The source files under `archives/` are **not committed**: they are derived from the installed game
and this repository stores no game content. `mods/.gitignore` excludes them.

Seed the tree from your own baseline install, then build without editing anything:

```sh
scripts/mod-seed.sh mods/cheat-keys-noop 'gs.mpq:gs\hotkey.gs'
scripts/mod-validate.sh mods/cheat-keys-noop
scripts/mod-build.sh mods/cheat-keys-noop --determinism-runs 2
```

`mod-seed.sh` writes the member's bytes exactly as the archive holds them, so `mod-validate.sh`
after seeding, and after building with no edit, reports an unchanged member and no findings. That
is the whole point of this mod: it is rung A of `docs/cheat-keys-ladder.md`, the control that rung
B is read against.

## Do not edit this tree

`gs\hotkey.gs` in the baseline is **10,792 bytes on a single line with no line terminator at all**
and is pure printable ASCII plus TAB. An editor that adds a trailing newline, reflows it, or saves
it as UTF-8 produces a different artifact from the one this rung exists to rule out. Use a byte
copy, not a text editor, if you ever touch this tree by hand -- and for this mod specifically, the
tree should never be touched by hand at all; `mod-seed.sh` is the only writer it should ever see.
