# cheat-keys-true

The source files under `archives/` are **not committed**: they are derived from the installed game
and this repository stores no game content. `mods/.gitignore` excludes them.

Seed the tree from your own baseline install, then flip the one token:

```sh
scripts/mod-seed.sh mods/cheat-keys-true 'gs.mpq:gs\hotkey.gs'
python3 -c "
path = 'mods/cheat-keys-true/archives/gs.mpq/gs/hotkey.gs'
data = open(path, 'rb').read()
needle = b'/cheat_keys false def'
assert data.count(needle) == 1, 'expected exactly one occurrence'
data = data.replace(needle, b'/cheat_keys true def')
open(path, 'wb').write(data)
"
scripts/mod-validate.sh mods/cheat-keys-true
scripts/mod-build.sh mods/cheat-keys-true --determinism-runs 2
```

`scripts/build-cheat-keys-ladder.sh` does exactly this and additionally asserts that the edit is
the *only* difference from the seeded baseline, at the token level (not a positional byte diff,
which reports every byte after the token as differing merely because `true` is one byte shorter
than `false`).

## Do not edit this tree by hand beyond the one substitution above

`gs\hotkey.gs` in the baseline is **10,792 bytes on a single line with no line terminator at all**
and is pure printable ASCII plus TAB. An editor that adds a trailing newline, reflows it, or saves
it as UTF-8 introduces a second difference this rung is not testing for. Use a byte-level tool
(the Python one-liner above, not a text editor) for the substitution.

## Read `docs/cheat-keys-ladder.md` before installing this build

Flipping this flag unlocks a tier of debug hotkeys, several of which act on the current army or
the current terrain sprite with no confirmation prompt. One of them, `ASCII_VAL"Y"`
(`destroyterrainsprite`), can delete map content permanently. The run sheet enumerates every key
this mod unlocks and marks the destructive ones before any install happens.
