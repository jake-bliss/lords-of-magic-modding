# Marauder probe run sheet

**Built 2026-09-25, not yet run.** Everything below was written before any install, and every
prediction is stated ahead of the observation, in the style of
[`docs/cheat-keys-ladder.md`](cheat-keys-ladder.md).

The question: can a human play Lords of Magic: Special Edition as the Marauders, the
wandering-monster player, with no city, faith or capital? This is a feasibility probe, not the mod.
**One attended sitting covers two rungs:**

- **Rung 1** is a build (`mods/marauder-probe`, vanilla base): cheat-keys-true plus four probe
  hotkeys in `gs\hotkey.gs`. It asks whether the scripts alone can bind the human to slot 15.
- **Rung 2** is a save edit (`tools/marauder_save_bind.py`) made between two launches: user record
  **1** of a save from rung 1 is re-bound to player 15. It asks whether `setuserforplayer 15` can
  then switch the human to the marauders, and what happens after that.

| | Question |
| --- | --- |
| **Q1** | What slot is `WANDERING_MONSTER_PLAYER`, and what do `getplayerfaith` / `getplayeraistatus` return for it? |
| **Q2** | Can the human be bound to that slot, and does the human then get a turn and move a Marauder army? |
| **Q3** | Does tactical combat let the human control that army? (Secondary.) |
| **Q4** | Does a save and reload keep the human bound to the Marauder slot? |
| **Q5** | What happens with a current user who has no lord (leader location -1)? |

## What the engine says before the run

Evidence class for this section: **read in a local binary (static)**, not observed in gameplay.
The run exists to confirm or refute it.

- **`WANDERING_MONSTER_PLAYER` is 15.** The engine registers its GameScript constants as a table of
  (name pointer, u32 value) pairs; the entry at `0x0055ED58` is `WANDERING_MONSTER_PLAYER = 15`,
  followed by `MAX_PLAYERS = 16`. The scripts agree: the monster generator in `gs\placedng.gs`
  counts marauder armies as `0 15{pop 1 add}enumplayerarmies`, the one place the corpus spells the
  slot as a number. `scripts/build-marauder-probe.sh` checks both on every build. Faiths from the
  same table: `LIFE 0, DEATH 1, ORDER 2, CHAOS 3, FIRE 4, WATER 5, EARTH 6, AIR 7`.
- **Players and users are different tables.** 16 player records (stride `0x1D84` at `0x005AF194`)
  and **8 user records** (stride `0x400` at `0x005A7D90`; the current user *index* is at
  `0x005A7D8C`). `currentuser` (`0x004E3DF0`) returns **the player field (`+0`) of the current user
  record**. It does not consult the player table at all.
- **The AI flag** is bit `0x2` of the player record's `+0x15D8` word (`setplayeraistatus`,
  `0x004BBFD0`). The **controlling computer** is `+0x15DC` (`getcontrollingcomputer`, `0x004B9270`).
- **`setuserforplayer P`** (`0x004E4A10` -> `0x0052CEF0`) searches the eight user records for one
  whose player field is P. **If there is none it returns at once and changes nothing.** If there is
  one, and P is on this computer, and P's AI bit is clear, it switches the current user index to
  that record (and runs the user-switch routine `0x0052CDB0`, even when the index does not change).
  Only when a record exists but P is AI or on another computer does it fall back to the first
  player on this computer with a clear AI bit that also has a user record.
- **No user record is ever bound to player 15 by the game.** Every new game runs `8 newgame`, and
  the new-game routine (`0x004818AB`) binds user *i* to player *i* for *i* < 8. `initusers`
  (`0x004DEEB0` -> `0x0052B600`) resets the rest of each record but not `+0`. No GameScript operator
  writes it. **So in rung 1, `15 setuserforplayer` is predicted to do nothing, and `currentuser` to
  stay 0.**
- **The binding lives in the save, and a load restores it verbatim.** `LS_USER` is the first 784
  bytes of each user record; its `+0` word is the bound player.
  [`save-format.md`](save-format.md#ls_user--eight-per-player-records) records it as "the record's
  own index" -- in every shipped save the two coincide, because user *i* is player *i* (the rung-2
  tests assert this over the installed saves). The load routine (`0x0052D090`) `fread`s all 784
  bytes of each record into the user table. The writer (`0x0052D040`) `fwrite`s them back out.
- **Every load runs `final_setup`** (all five `loadgame{...}` sites in the corpus call it). It runs
  `setup_player_control` (player 0 human, 1..7 AI), `setup_ai`
  (`WANDERING_MONSTER_PLAYER 1 setplayeraistatus`), `initusers` and `0 setuserforplayer`.
  Anything the probe flips in game is re-set on load. The user binding is not.
- **A load from the main menu starts at user index 0.** The menu runs `8 newgame` (`startmenudialog`)
  before its load dialog, and `newgame` (`0x004DEEC0` -> `0x00481820`) zeroes the current user
  index at `0x004818F4`. The load path writes the user records but never the index: the only other
  writers of `0x005A7D8C` are the start-up initialiser and the user switch itself. An in-game
  Ctrl+L keeps whatever index is current.
- **Why rung 2 re-binds record 1, not record 0.** Because the menu load starts at index 0,
  re-binding record 0 would make `currentuser` read 15 straight after the load whether or not
  `setuserforplayer 15` ever succeeded. With record 1 re-bound, the load leaves `currentuser` at 0,
  a successful `setuserforplayer 15` moves the index to 1 (`currentuser` 15), and a failed one
  leaves it at 0. **Precondition, checked against the corpus:** the user switch
  (`0x0052CDB0`, and `0x0048A400` after it) reads the new record's mode word at `+0x2E8`. It is `2`
  in records 0 and 1 of all 6 vanilla saves and all 11 Development saves (3.02's `Merlin I` and its
  `lastsave.lom` have `5` in record 0, so the mode is not a constant, and `bind` refuses when the two
  differ). Everything else that differs between records 0 and 1 in those saves lies in
  `+0x14`..`+0x2E7`, which `initusers` resets on every load, or is `+0x2F0`, the per-user
  `setcenteronmovement` preference (the load copies the global into record 0 only). One untested
  consequence: player 1, an AI faction, no longer has a user record.
- The same new-game routine marks player 15 always active (`cmp edi, 0xf`), which is why the
  marauders exist in every game.
- The HD overlay 0.5.0 patch to the Development profile's `lomse.exe` differs from vanilla in 85
  bytes, all in pages `0x46A000`, `0x475000` and `0x504000`-`0x51A000`. **None of the routines
  above is in those pages.**

## Other things the scripts say, which the predictions use

- **Marauder armies on turn 1.** The monster generator only spawns when `currentturn 5 gt`, so the
  free-roaming marauder armies do not start until turn 6. Slot 15 still owns armies from turn 1:
  village security forces (`make_village_security_forces`, `gs\brain.gs`) and encounter armies
  placed in dungeons (`createarmyinsprite`, `gs\placedng.gs`). The readout's count includes those.
  The **N** key creates a fresh one anyway.
- **Brains.** Generator armies get `/brain` on every unit; security forces get
  `village_security_brain`. Those alarms keep issuing `longmoveattack`/`longmovetosprite` orders
  whoever owns the slot. The army **N** creates is made by `addunit` and gets no brain, so it is the
  one to test movement with. `setup_ai` also attaches the turn AI to slot 15; that is expected to
  stop acting once slot 15's AI bit is clear.
- **`brain` targets only non-AI players 0..7.** `getclosestbuilding` keeps a building only if its
  owner is `0 7 between` and `getplayeraistatus not`. With player 0 flipped to AI, the marauder
  brains stop targeting its buildings. A human at slot 15 is outside that range.
- **Faith of slot 15 is probably outside 0..7.** `setupplayergraphics` (`gs\player.gs`) guards every
  player's faith with `f 0 7 between not{/f DEATH def}`, and loops 0..15. Many panels index 8-entry
  faith arrays by `currentuser getplayerfaith` with no guard, so a UI bound to slot 15 could raise a
  range error the first time such a panel opens. `final_setup`'s own script path does not: its
  loops are over players 0..7 and none of the scenarios' `existing_game_setup` reads `currentuser`.
- **Combat autocalc.** `multiplayer_startscenario` (called by `final_setup`) may install an
  autocalc rule that auto-resolves combat when both owners are AI. With slot 15's AI bit clear the
  human side of a marauder fight should get tactical combat, if that rule is the one in force.
- **Leader death with a leaderless current user** (`gs\gameover.gs`, `PROC_LEADER_DEATH`). The
  procedure fires when a leader dies. When the dead player has no heir and
  `currentuser getleaderlocation pop -1 eq` is true -- always, for slot 15 -- it only acts if
  `who_died currentuser eq`. So **when any other faction's leader dies, nothing happens**: that
  faction's armies, buildings and cities are *not* handed to the marauders and it is not
  deactivated. No script path ends the game merely because the current user has no leader; a
  native check might, which is what Q5 observes.

## Keys

The build unlocks the whole `cheat_keys` tier. Its table, with the destructive keys marked, is in
[`docs/cheat-keys-ladder.md`](cheat-keys-ladder.md#the-cheat_keys-hotkey-tier-key-by-key).

| Key | What it does | Where |
| --- | --- | --- |
| **Shift+J** | **Readout.** A message box: `t=` turn, `cu=` current user, `cp=` current player (whose turn), `wmp=` the slot constant, `tc=` this computer; then for player 0 (`P0`) and the marauder slot (`MAR`): `f=` faith, `ai=` AI status, `cc=` controlling computer; `nMAR=` armies the marauder slot owns; `lord=` the current user's leader army (-1 = none). In combat it is a balloon instead. | anywhere, single player |
| **Shift+N** | **Spawn.** Creates a one-unit `decr1` marauder army (no brain) on the nearest empty land to the centre of the map view, and centres the camera on it. | map view, not zoomed out to the world, not in combat |
| **Shift+U** | **Takeover.** Marauder slot to human (`ai` 0), player 0 to AI (`ai` 1), marauder slot's controlling computer to this computer, `processgamemessages`, then `WANDERING_MONSTER_PLAYER setuserforplayer`. | not in combat |
| **Shift+H** | **Hand back.** Player 0 to human, marauder slot to AI, `0 setuserforplayer`. Works in both rungs: user record 0 stays bound to player 0. | not in combat |
| `*` | Cheat tier: every unit in the selected army gets 10,000 experience and **1,000 move points**. | single player |
| `E` | End turn (shipped). | not in combat |
| Ctrl+S / Ctrl+L | Save / load (shipped). Ctrl+S also rewrites `lastsave.lom`. | not in combat |

**Never press `Y`** (`destroyterrainsprite`: permanently deletes whatever terrain sprite is
tracked, no confirmation -- this project has lost a village to it) **or `S`** (calls `superduper`,
which is defined nowhere; untested). Also avoid `M` (halts the army's move order), `K`/`k`, `F7`
and `F12` unless the ladder table says you want them. None of J, N, U, H is bound in the shipped
member; the build refuses to produce an archive if that ever stops being true.

## Before the sitting

Paths below are quoted; the game path has spaces. Run everything from the worktree root,
`/Users/jakebliss/personal-projects/lords-of-magic-modding.claude-marauder-probe`.

```sh
DEV="$HOME/Applications/Lords of Magic Development.app/Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/steamapps/common/Lords of Magic Special Edition/English"
KEEP=/Users/jakebliss/personal-projects/lom-artifacts-keep
```

**The Development profile is not currently vanilla.** Measured 2026-09-25 (read-only hashes):

| File | State |
| --- | --- |
| `gs.mpq`, `pic.mpq` | the **`gs5r3-base`** build `ad9fece3123a` (GS5R3 archives) |
| `imp.mpq`, `sndfx.mpq`, `special.mpq` | pristine |
| `lomse.exe` | HD overlay 0.5.0 terrain patch (`ddb43883...`), backup `lomse.exe.lomhd-backup` |
| `ddraw.dll`, `ddraw.ini`, `lomhd_*` | HD overlay 0.5.0 |

The probe is built on the **vanilla** `gs.mpq`. Installing it alone would leave vanilla scripts next
to GS5R3's `pic.mpq`, a combination nothing has tested, and a GS5R3 save loaded under vanilla
scripts would be a second unknown. So restore the archives first:

```sh
# 1. Build and check. Installs nothing. The build id is in the report.
scripts/build-marauder-probe.sh
cat artifacts/marauder-probe/offline-checks.txt

# 2. A byte copy of the profile's savegame folder. The game rewrites lastsave.lom and autosaves
#    during the run; this is the way back for them. (A copy, not a hash.) The script refuses while
#    the game runs, writes a NEW timestamped directory (refusing one that exists), verifies it with
#    diff -r, and prints its path. Keep that path: the restore needs it.
scripts/marauder-probe-savegames.sh backup
SAVEBK="PASTE THE PRINTED PATH HERE"     # e.g. "$KEEP/save-backups/dev-savegame-20260926T010000Z"

# 3. Archives back to pristine vanilla (all five). Refuses while the game is running.
scripts/restore-dev.sh

# 4. Install the probe. Replaces gs.mpq and nothing else.
scripts/install-dev.sh marauder-probe BUILD_ID      # b88ebb19aedc when this sheet was written
```

**What these commands touch, stated plainly.** `install-dev.sh` writes only the archives named in
the build's `build.json`, and this build names **only `gs.mpq`** (checked by the build script).
`restore-dev.sh` writes only the five archives in `PIPELINE_ARCHIVES` (`gs.mpq pic.mpq imp.mpq
sndfx.mpq special.mpq`). **Neither touches `lomse.exe`, `ddraw.dll`, `ddraw.ini`, `lomhd_*` or the
portrait pack.** The HD overlay itself never writes an `.mpq` (its setup only reads `pic.mpq` and
`imp.mpq`). The one interaction: after step 3 the overlay runs against vanilla `pic.mpq` instead of
GS5R3's. Portraits pair by content, so the few that differ between the two installs will simply
not be upscaled; whether the HD terrain art still lines up with vanilla tilesets was not checked.
None of the probe's readings is a picture, so neither affects the result.

For reference, `build id b88ebb19aedc`, `gs.mpq sha256
1b95a7bd82dd17bda9819600aad2b180d73bb94cc480f473a5f57369d11adfbb`. A build id is a digest of the
tree, the base archives and the tool binaries, so recompiling the tools changes it; trust the report.

## Rung 1: can the scripts bind the human? (first launch)

Open **`Lords of Magic Development.app`** from `~/Applications/`, nothing else. Start a **new
single-player game, any faith** (the campaign's New Game). Note the faith you picked. Reach the
world map with the **map view**, not the zoomed-out world view. Write down every number the readout
shows at every step; the numbers are the result.

### 1.1 -- the control: Shift+J before anything else

This proves the readout is connected before it is asked anything.

| Field | Predicted | If not |
| --- | --- | --- |
| `t` | `1` | note it |
| `cu` | `0` | the human is not player 0; U, H and the save edit all assume 0. Stop and report |
| `cp` | `0` (your turn) | note it |
| `wmp` | `15` | **Q1 answered differently from the static reading**; report the number |
| `tc` | a small number, probably `1` | note it |
| `P0 f` | your faith's number (LIFE 0 ... AIR 7) | the readout is not reading what it claims |
| `P0 ai` | `0` | as above |
| `MAR ai` | `1` (set by `setup_ai`) | **Q1**: the marauders are not flagged AI |
| `MAR f` | **unknown**; predicted outside 0..7 | this is the Q1 reading |
| `P0 cc`, `MAR cc` | `P0 cc` equal to `tc`; `MAR cc` unknown | note both: U sets `MAR cc` to `tc`, and this is the only record of what it was |
| `nMAR` | 0 or more (security forces, dungeon garrisons) | note it |
| `lord` | 0 or more (your lord's army) | note it |

If **no box appears at all**: first **close every open message box and dialog, then press J
again** -- the message box shows nothing while another story dialog is open (the game's own
turn-start messages count). If there is still nothing, the key is not reaching the script (a native
binding, or the build is not installed). Press `*` with your lord's army selected: if the lord goes
to level 9 the build is installed and the fault is the J binding itself.

### 1.2 -- Shift+N, then Shift+J

| Observed | Meaning |
| --- | --- |
| Camera jumps to a new one-unit army, and `nMAR` is exactly one more than in 1.1 | spawn works; this army goes into the save and is the one to move in rung 2 |
| `nMAR` unchanged, camera did not move | `getlocationatscreenxy` or `findemptylocation` returned -1 (centre of view off the map?). Scroll to open land and retry |
| An error dialog | report its text; the spawn body is wrong |

### 1.3 -- Shift+U, then Shift+J (Q2, script-only)

The AI fields prove the key ran, whatever `cu` does: predicted `P0 ai=1`, `MAR ai=0`, `MAR cc`
equal to `tc`.

| `cu` after U | Meaning |
| --- | --- |
| **`0` (unchanged)** | **Predicted.** No user record is bound to 15, so `setuserforplayer` returned without doing anything. **The scripts cannot bind the human to slot 15.** Rung 2 is the next step. |
| **`15`** | Not predicted. The binding worked by script; the static reading of `0x0052CEF0` is wrong. Rung 2 is still worth running, but go to 2.3 first in *this* session (move, end turn, Q5) |
| **any other number** | Not predicted either: the fallback path ran, which the static reading says needs a user record bound to 15. Report the number, press Shift+H |

**Watch the map after pressing U.** Marking player 0 as AI in the middle of its own turn might
start the AI playing that turn at once. Note whether anything of yours moves, recruits or builds on
its own between U and H, and roughly what. If it does, the `mprobe` save made below reflects those
AI actions; that is fine for rung 2, but say so in the notes.

### 1.4 -- hand back, save, quit

Press **Shift+H**, then **Shift+J**. Predicted: `cu=0`, `P0 ai=0`, `MAR ai=1`. **Do not end the
turn between U and H**: player 0 is flagged AI in between.

Then **Ctrl+S** and save as **`mprobe`** (type exactly that). Quit the game.

Shipped player-named saves have no extension (`Merlin I`, `Arthur`), so the file is expected at
`"$DEV/savegame/mprobe"`. Check before going on:

```sh
ls -la "$DEV/savegame/"
```

## Between the launches: the save edit (offline)

```sh
# Read the eight bindings. Predicted: 0 1 2 3 4 5 6 7.
python3 tools/marauder_save_bind.py check "$DEV/savegame/mprobe"

# Write a COPY with user record 1 bound to player 15 (--record 1 is the default; stated anyway).
# Never edits mprobe itself.
python3 tools/marauder_save_bind.py bind "$DEV/savegame/mprobe" "$DEV/savegame/mprobe15" \
  --record 1 --backup-dir "$KEEP/save-backups"
```

`bind` refuses, and writes nothing, if: the game is running; `mprobe15` already exists; the save
does not parse, re-encode byte-identically, or have exactly one `LS_USER` of eight 784-byte
records; record 1 is not bound to 1; anything is already bound to 15; record 1's mode word
(`+0x2E8`) differs from record 0's; the output or the backup directory is inside `~/Applications`
but outside the Development profile; or the output differs from `mprobe` anywhere but the four
bytes of that word. It keeps `"$KEEP/save-backups/mprobe.orig"`, a byte copy compared against the
source. Predicted output: `before [0, 1, 2, 3, 4, 5, 6, 7]`, `after [0, 15, 2, 3, 4, 5, 6, 7]`,
one byte changed, `mode` all `2`. The format has no checksum and no compression, and the section
is fixed width, so nothing else in the file moves.

**If `bind` refuses on the mode word**, the discriminating design is not available for this save.
Fall back to `--record 0` (and a new output name). Then `cu` reads 15 straight after the load
whatever happens later, so **`cu` does not discriminate** in 2.1 or 2.2; read the AI fields and
whether the army can be moved instead, and say in the notes that the fallback was used.

## Rung 2: what happens once the binding exists (second launch)

Relaunch the Development profile. From the **main menu**, load **`mprobe15`**. (Loading from the
main menu matters: the menu runs `8 newgame` first, which sets the current user index to 0. Record
0 is still bound to player 0; the edit re-bound record 1.)

### 2.1 -- Shift+J right after the load: the control for rung 2

What happens on load, step by step: the menu has set the current user index to 0; the load routine
reads the eight user records, record 1 now bound to player 15. `final_setup` sets player 0 human and
1..7 AI (`setup_player_control`), sets slot 15 AI (`setup_ai`), leaves the bindings alone
(`initusers`), and calls `0 setuserforplayer` -- which finds record 0 bound to player 0, human and
on this computer, and switches to it (the index is already 0). The scripts in that path do not
index anything by `currentuser`. The natives it calls (`gamemode`, `set_turn_button_image`, the
visibility code) were not read; player 1 having no user record is the untested difference.

| Field | Predicted | If not |
| --- | --- | --- |
| **`cu`** | **`0`** -- index 0, record 0, player 0 | `15`: the index was not 0 after the menu load, so the discrimination below is lost; note it and carry on |
| `cp` | `0` (the save was made on your turn) | note it |
| `t` | the turn you saved on | note it |
| `P0 ai` / `MAR ai` | `0` / `1` (`final_setup` re-set them) | if `MAR ai=0`, `setup_ai` did not run: note it |
| `MAR cc` | `tc` if the controlling computer is saved (U set it in rung 1), otherwise the value from 1.1 | note it |
| `nMAR` | one more than 1.1 (the N army is in the save) | note it |
| `lord` | your lord's army, as in 1.1 | note it |

**An error dialog or a crash on load** is itself a result: the engine cannot hold a user record
bound to slot 15. Note the text and stop the rung.

### 2.2 -- Shift+U, then Shift+J (Q2 with the binding): the discriminating step

A user record bound to 15 now exists. U makes slot 15 non-AI and puts it on this computer, so
`setuserforplayer 15` is predicted to **succeed**: it finds record 1 and moves the current index to
1.

| `cu` after U | Meaning |
| --- | --- |
| **`15`** | **Predicted.** The switch worked: the human is now the Marauders. `lord` should read `-1` (slot 15 has no leader): **the Q5 baseline**. Go on to 2.3 |
| **`0`** | The switch failed even with a bound record: one of the switch's conditions (on this computer, AI bit clear) is not met as the readout claims, or the static reading is wrong. Note `MAR ai` and `MAR cc`, press Shift+H, and stop the rung |

Either way, predicted `P0 ai=1`, `MAR ai=0`, `MAR cc` equal to `tc`. As in rung 1, watch whether
player 0 (now AI) starts moving on its own. Also note, as text, anything about the screen after the
switch: whose flag or colour the interface shows, whether the fog of war changed, any error. Those
are context, not readings.

**Shift+H works here too**: it flips the AI flags back and `0 setuserforplayer` switches to record
0, so `cu` returns to 0.

### 2.3 -- move, then end the turn (Q2, Q5)

1. **Select and move.** Press Shift+N to spawn a fresh army at the centre of the view (or scroll to
   the one from 1.2). Click it. Read its move points on the army panel (a number). Order a short
   move. If it will not move, press `*` with it selected (1,000 move points) and try again. Note
   whether you could select it at all, and the move points before and after.
2. **End the turn.** Press `E`. After control comes back to you (or after a reasonable wait), press
   Shift+J. Repeat for a few turns, noting `t` and `cp` each time.
   - `cp` reads `15` while the game waits for you, and `t` advances: **the human gets a turn as the
     Marauders.** Q2 yes.
   - `cp` never reads 15 and the game waits with `cp=0`: the engine is waiting on player 0, which
     is now AI but was the "human" slot in its turn order, or is skipping slot 15. Q2 blocked in
     the turn order: exe territory.
   - The game appears to hang after `E`: note how long you waited and what was on screen. The
     engine waiting for input from a slot it does not normally hand to a human is the likely cause.
3. **Q5.** Over those turns, note any defeat, "game over" or "restart" dialog and the turn it
   appears on. Predicted from the scripts: none. `lord` stays `-1`.
4. **Q3 (optional).** Attack something with the army. In combat, Shift+J shows a balloon. Can you
   select and order your units, or does the fight resolve itself?

### 2.4 -- save and reload (Q4)

With the human bound to 15 (index 1), **Ctrl+S** as **`mprobe2`**, then **Ctrl+L** and load
`mprobe2`, then Shift+J. An in-game load keeps the current index (1), so for a moment after the
read `currentuser` is 15; then `final_setup` runs `0 setuserforplayer`, finds record 0 bound to
player 0 and switches back to it.

| Observed after the load | Meaning |
| --- | --- |
| `cu=0`, `P0 ai=0`, `MAR ai=1`; then Shift+U gives `cu=15` again | **Predicted.** The binding (record 1 -> 15) survived the save and the load; the AI flags and the current index did not (`final_setup`). A real mod needs the bound record once, and its takeover redone in `final_setup` on every load |
| `cu=0` and Shift+U leaves it at 0 | the save did not keep record 1 bound to 15 (check the file, below). Exe territory |
| `cu=15` right after the load | `final_setup`'s `0 setuserforplayer` did not switch; note it |

Press Shift+H, then quit.

After the sitting, confirm Q4 from the file itself, offline:

```sh
python3 tools/marauder_save_bind.py check "$DEV/savegame/mprobe2"   # predicted: record 1 -> player 15
```

## Predictions and what each outcome means for the plan

| Q | Outcome | Plan |
| --- | --- | --- |
| Q1 | `wmp=15`, `MAR ai=1`, `MAR f` outside 0..7 | **predicted**. A faith outside 0..7 means every unguarded faith-array lookup in the UI is a crash risk for a human at 15: script work, not an exe patch |
| Q2, rung 1 | `cu` stays 0 | **predicted**. Not possible by script alone |
| Q2, rung 1 | `cu=15` | script-only binding is viable; the static reading is wrong and must be corrected |
| Q2, rung 2 | `cu=0` on load, `cu=15` after U, the army moves, `cp` reaches 15 | **the save-edit route works.** A real mod = a save-edited start (or a one-word exe patch to bind a user record) + script for everything else |
| Q2, rung 2 | `cu=15` after U, the army moves, but `cp` never reaches 15 | bound but no turn: slot 15 is not in the human turn order. Exe patch |
| Q2, rung 2 | `cu=0` after U | a bound record is not enough: the switch refuses slot 15 for a reason the readout does not show. Exe work |
| Q2, rung 2 | error or crash on load or on U | the engine cannot hold a user bound to 15 as is. Exe patch or dead end |
| Q3 | units controllable | script-only for combat |
| Q3 | combat plays itself | combat control keys off something other than `currentuser` or the AI bit; exe work |
| Q4 | binding survives (`check` on `mprobe2`: record 1 -> 15; U works again), AI flags and index reset | **predicted**. Redo the takeover in `final_setup` |
| Q4 | binding lost | exe work |
| Q5 | no dialog over several turns | **predicted from the scripts**. Note the side effect: other factions' deaths stop handing their property to the marauders |
| Q5 | a defeat or game-over screen | a native elimination check exists; find it before anything else. Exe patch or dead end |

## Design constraints for a real Marauder mod (GS5R3 FAQ claims)

These come from the GS5R3 FAQ Jake pasted. **They describe GS5R3, not the vanilla base this probe
uses, and they are FAQ claims, not verified against the scripts** except where marked.

1. **Unit cap.** 99 x capitals owned for humans; with no capital, 18. Per player and type: 9
   champions, 9 CR3, 18 military, 18 lesser creature sets, 18 scouts. *Not verified.*
2. **Followers** arrive every 7th turn, from fame, capitals with Great Temples and capital level.
   With no capital a Marauder player gets about none, so its economy must come from elsewhere
   (plunder, combat). **Confirmed in GS5R3 `gs\initiate.gs`** (the FAQ says `initiates.gs`):
   `/turns_between_arrival 7 def`, and `initiate_arrival_equation` only runs its fame and
   stronghold terms under `total_cities 0 gt`, where `total_cities` counts the current player's
   cities with a `KEEP` above level 0. No city, no followers from this path at all.
3. **Mercenaries** from village buildings need a controlled capital with a Stronghold; their cost
   depends on the primary capital's level. *Not verified.*
4. **Level 6+ marauders regain all HP and mana after combat** (GS5R3 anti-cheese); a human Marauder
   would inherit it. *Not verified.* GS5R3's `gs\COMB_RES5.gs` does special-case
   `getcombatloser WANDERING_MONSTER_PLAYER eq` for the loser's resources; the heal rule was not found
   in the time spent.
5. **Leader slain = faith defeated; an heir takes over.** **Matches vanilla `gs\gameover.gs`**:
   `enumplayerheirs` then `setheirasleader`. For a leaderless Marauder player see Q5 above.
6. **Combat XP** = defeated army's total barter value x a difficulty modifier (1.35 unmodded).
   *Not verified.*

A note on names: the earlier research that seeded this probe cited `SCENARIO5.gs`, `PLACEDNG5.gs`,
`BRAIN5.gs` and `PLAYER5.gs`. Those are **GS5R3** members. Vanilla has `gs\scenario.gs`,
`gs\placedng.gs`, `gs\brain.gs` and `gs\player.gs`, and this sheet cites those.


## After the sitting

```sh
# 1. The savegame folder back to its pre-sitting state. Refuses while the game runs. First copies
#    mprobe, mprobe15 and mprobe2 (evidence) to a new timestamped directory under
#    "$KEEP/save-backups", verified with cmp; then moves the live folder aside (kept, not deleted);
#    then copies the backup back and verifies it with diff -r.
scripts/marauder-probe-savegames.sh restore "$SAVEBK"

# 2. Archives back to pristine vanilla. The HD overlay files are untouched.
scripts/restore-dev.sh

# 3. Back to exactly the pre-probe state: the GS5R3 base archives under the HD overlay. The durable
#    copy of that build is outside every worktree; restore-dev.sh resolves build/<mod>/<id> under
#    LOM_ARTIFACTS_DIR, and verifies both archives against the build's build.json before writing.
LOM_ARTIFACTS_DIR=/Users/jakebliss/personal-projects/lom-artifacts-keep \
  scripts/restore-dev.sh --to gs5r3-base ad9fece3123a
```

Step 3 writes `gs.mpq` and `pic.mpq` only. Run the offline Q4 check on `mprobe2` **before** step 1,
or on the evidence copy after it.
