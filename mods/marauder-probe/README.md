# marauder-probe

The source files under `archives/` are **not committed**: they are derived from the installed game
and this repository stores no game content. `mods/.gitignore` excludes them.

Build it, with every offline check, from your own vanilla install:

```sh
scripts/build-marauder-probe.sh
```

That seeds `gs\hotkey.gs`, applies the edit with `tools/marauder_probe.py`, verifies that the edit
is the only difference, builds with `scripts/mod-build.sh` (which validates first), reads the member
back out of the packed archive and verifies it again. The run sheet is
[`docs/marauder-probe.md`](../../docs/marauder-probe.md).

## What the edit is

Two changes to `gs\hotkey.gs`, and nothing else:

1. `/cheat_keys false def` becomes `/cheat_keys true def`, as in `mods/cheat-keys-true`. This
   unlocks the whole cheat tier, **including `Y`, which permanently deletes a terrain sprite**. Read
   the key table in `docs/cheat-keys-ladder.md` before installing.
2. Four bindings are inserted as one block immediately before the Ctrl+V binding
   (`ASCII_VAL 22{... "Lords of Magic v3.01 December 3, 1998" ...}addhotkey`), which is the first
   binding after the cheat tier closes and still inside the member's own `20 dict begin ... end`.

The keys are upper case, so each needs Shift. The build refuses if the shipped member binds any of
them in the `ASCII_VAL` namespace, and it refuses to say a key is free unless its parser accounts
for every `addhotkey` in the file.

## Stack effects, operator by operator

Each operator's argument count is the nominal arity in `reports/natives/operator-bodies.tsv`; the
number of results is taken from a shipped call site, cited. No body defines a name: a hotkey runs
under whatever dictionary stack is current when the key is pressed.

**J, readout.** `{ ... }build_statement` collects everything its procedure pushes into one string,
as the `"Game Speed: "` balloon in the same member does. Inside:

| Pushed | Operator | Pops | Pushes | Shipped call site |
| --- | --- | ---: | ---: | --- |
| `t` | `currentturn` | 0 | 1 | `turnstring{currentturn}build_statement` (scenario.gs) |
| `cu` | `currentuser` | 0 | 1 | `currentuser getplayeraistatus` (gameover.gs) |
| `cp` | `currentplayer` | 0 | 1 | `currentplayer set_turn_button_image` (scenario.gs) |
| `wmp` | `WANDERING_MONSTER_PLAYER` | 0 | 1 | constant, 15 in `lomse.exe`'s table |
| `tc` | `thiscomputer` | 0 | 1 | `getcontrollingcomputer thiscomputer eq` (story.gs) |
| faith | `P getplayerfaith` | 1 | 1 | `who_died getplayerfaith` (gameover.gs) |
| ai | `P getplayeraistatus` | 1 | 1 | `currentuser getplayeraistatus 0 eq` (gameover.gs) |
| cc | `P getcontrollingcomputer` | 1 | 1 | `currentplayer getcontrollingcomputer` (story.gs) |
| count | `0 P{pop 1 add}enumplayerarmies` | 2 | 0 (the accumulator is left) | `0 15{pop 1 add}enumplayerarmies` (placedng.gs) |
| lord army | `P getleaderlocation pop` | 1 | 2, one popped | `currentuser getleaderlocation pop -1 eq` (gameover.gs) |

Out of combat the string goes to `storydict begin open_messagebox_dialog end` (the Ctrl+V box, a
modal box that stays until dismissed); in combat to `center_balloonhelp_quick` (the game-speed
balloon), because the message box has no shipped in-combat caller.

**N, spawn.** Single player, not in combat, not zoomed out to the world view (the arrow keys' guard):

```text
320 190 getlocationatscreenxy          -> loc                  (arrow keys: 320 50 getlocationatscreenxy)
dup -1 gt { ... } { pop } ifelse       -> loc
UNITTYPELAND findemptylocation         -> loc'                 (brain.gs make_village_security_forces)
dup -1 gt { ... } { pop } ifelse       -> loc'
dup xy_to_x_y                          -> loc' x y
unittypedict /decr1 get                -> loc' x y type
WANDERING_MONSTER_PLAYER -1 addunit    -> loc'                 (scenario.gs create_mopup_crew: decr1 WANDERING_MONSTER_PLAYER -1 addunit)
processgamemessages xy_to_x_y armyat   -> army                 (same site: processgamemessages dest_loc xy_to_x_y armyat)
dup -1 gt { centeronarmy } { pop } ifelse                      (enc_tool.gs: dup -1 gt{centeronarmy}{pop}ifelse)
rendermap
```

The army is created by `addunit`, so no champion brain is attached to it. The monster generator's
armies (`gs\placedng.gs`) get `/brain` on every unit and the village security forces get
`village_security_brain`; this one gets neither.

**U, takeover.** `WANDERING_MONSTER_PLAYER 0 setplayeraistatus`, `0 1 setplayeraistatus`,
`WANDERING_MONSTER_PLAYER thiscomputer setcontrollingcomputer`,
`WANDERING_MONSTER_PLAYER setuserforplayer`, `rendermap`. `setplayeraistatus` is `player value`
(`i 0 setplayeraistatus` in scenario.gs `setup_player_control`); `setcontrollingcomputer` is
`player computer` (`WANDERING_MONSTER_PLAYER 1 setcontrollingcomputer` in scenario\network.gs,
for the host); `setuserforplayer` pushes nothing. The controlling-computer write is there because
`setuserforplayer` only switches to a player on this computer; in rung 2, where a user record is
bound to slot 15, that is the one remaining condition the scripts can satisfy.

**H, hand back.** The reverse: `0 0 setplayeraistatus`, `WANDERING_MONSTER_PLAYER 1
setplayeraistatus`, `0 setuserforplayer` (exactly what `final_setup` calls), `rendermap`.

## Do not edit this tree by hand

`gs\hotkey.gs` is a single line of printable ASCII with no line terminator. The probe block follows
suit. An editor that adds a newline or re-encodes the file introduces a difference the build's
verification will reject.
