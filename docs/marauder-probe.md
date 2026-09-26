# Marauder probe run sheet

**Built 2026-09-25, not yet run.** Everything below was written before the build was installed, and
the predictions are stated ahead of the observation, in the style of
[`docs/cheat-keys-ladder.md`](cheat-keys-ladder.md).

The question: can a human play Lords of Magic: Special Edition as the Marauders, the
wandering-monster player, with no city, faith or capital? This is a feasibility probe, not the
mod. It is one build (`mods/marauder-probe`) on the vanilla base: cheat-keys-true plus four probe
hotkeys in `gs\hotkey.gs`.

| | Question |
| --- | --- |
| **Q1** | What slot is `WANDERING_MONSTER_PLAYER`, and what do `getplayerfaith` / `getplayeraistatus` return for it? |
| **Q2** | Does `setuserforplayer` accept that slot, and does the human then get a turn and move a Marauder army? |
| **Q3** | Does tactical combat let the human control that army? (Secondary.) |
| **Q4** | Does a save and reload keep the human bound to the Marauder slot? |
| **Q5** | What happens with a current user who has no lord (leader location -1)? |

## Read this first: the static reading predicts the takeover will not bind

The probe was designed from the scripts, and then the engine was read. **Static analysis of the
vanilla `lomse.exe` predicts that `setuserforplayer` cannot bind the human to slot 15**, whatever
the scripts do. Evidence class for everything in this section: **read in a local binary (static)**,
not observed in gameplay. The run exists to confirm or refute it.

- **`WANDERING_MONSTER_PLAYER` is 15.** The engine registers its GameScript constants as a table of
  (name pointer, u32 value) pairs; the entry at `0x0055ED58` is `WANDERING_MONSTER_PLAYER = 15`,
  followed by `MAX_PLAYERS = 16`. The scripts agree: the monster generator in `gs\placedng.gs`
  counts marauder armies as `0 15{pop 1 add}enumplayerarmies`, the one place the corpus spells the
  slot as a number. `scripts/build-marauder-probe.sh` checks both on every build. Faiths from the
  same table: `LIFE 0, DEATH 1, ORDER 2, CHAOS 3, FIRE 4, WATER 5, EARTH 6, AIR 7`.
- **Players and users are different tables.** There are 16 player records (stride `0x1D84` at
  `0x005AF194`) and **8 user records** (stride `0x400` at `0x005A7D90`; the current user index is
  `0x005A7D8C`). `currentuser` (`0x004E3DF0`) returns *the player field of the current user record*.
- **The AI flag** is bit `0x2` of the player record's `+0x15D8` word (`setplayeraistatus`,
  `0x004BBFD0`). The **controlling computer** is `+0x15DC` (`getcontrollingcomputer`, `0x004B9270`).
- **`setuserforplayer P`** (`0x004E4A10` -> `0x0052CEF0`) looks for a *user record whose player
  field is P*. If there is one, and P's controlling computer is this computer, and P's AI bit is
  clear, it switches to that user. Otherwise it falls back to the first player (0..15) that is on
  this computer, has its AI bit clear **and has a user record**, and switches to that one. If none
  qualifies it does nothing.
- **No user record is ever bound to player 15.** Every new game runs `8 newgame`, and the new-game
  routine (`0x004818AB`) sets user *i* to player *i* for *i* < 8. `initusers` (`0x004DEEB0`) resets
  the rest of each user record but does not touch the player field. No GameScript operator writes
  it. **So `15 setuserforplayer` has no user to switch to, and the prediction is that
  `currentuser` does not become 15.**
- The same routine marks player 15 always active (`cmp edi, 0xf` in the activation loop), which is
  why the marauders exist in every game.
- **The binding lives in the save.** A save's `LS_USER` section is the first 784 bytes of each of
  the 8 user records, so its `+0` word is the user's bound player.
  [`save-format.md`](save-format.md#ls_user--eight-per-player-records) records it as "the record's
  own index"; in every save inspected the two coincide, because user *i* is player *i*.
- **Every load runs `final_setup`**, and `final_setup` runs `setup_player_control` (player 0 back
  to human), `setup_ai` (`WANDERING_MONSTER_PLAYER 1 setplayeraistatus`), `initusers` and
  `0 setuserforplayer`. Anything the probe flips in-game is re-set on load.
- The HD overlay 0.5.0 patch to the Development profile's `lomse.exe` differs from vanilla in 85
  bytes, all in pages `0x46A000`, `0x475000` and `0x504000`-`0x51A000`. **None of the operators
  above is in those pages**, so the overlay's exe cannot explain a probe result.

If the run confirms this, the script-only answer to Q2 is **no**, and the cheapest next step is not
an exe patch but a save edit: see [Rung 2](#rung-2-not-built-bind-a-user-record-to-slot-15-in-a-save).

## Other things the scripts say, which the predictions use

- **Marauder armies on turn 1.** The monster generator only spawns when `currentturn 5 gt`, so the
  free-roaming marauder armies do not start until turn 6. Slot 15 still owns armies from turn 1:
  village security forces (`make_village_security_forces`, `gs\brain.gs`) and encounter armies
  placed in dungeons (`createarmyinsprite`, `gs\placedng.gs`). The readout's count includes those.
  The **N** key creates a fresh one anyway.
- **Brains.** Generator armies get `/brain` on every unit; security forces get
  `village_security_brain`. Those alarms keep issuing `longmoveattack`/`longmovetosprite` orders
  whoever owns the slot. The army **N** creates is made by `addunit` and gets no brain, so it is the
  one to test movement with. Separately, `setup_ai` attaches the turn AI to slot 15, and that is
  expected to stop acting once slot 15's AI bit is clear.
- **`brain` targets only non-AI players 0..7.** `getclosestbuilding` keeps a building only if its
  owner is `0 7 between` and `getplayeraistatus not`. With player 0 flipped to AI, the marauder
  brains stop targeting its buildings. A human at slot 15 is outside that range; a real mod must
  decide what the brains should hunt.
- **Faith of slot 15 is probably outside 0..7.** `setupplayergraphics` (`gs\player.gs`) guards every
  player's faith with `f 0 7 between not{/f DEATH def}`, and loops 0..15. Many panels index 8-entry
  faith arrays by `currentuser getplayerfaith` with no guard, so a UI bound to slot 15 could raise a
  range error the first time such a panel opens.
- **Leader death with a leaderless current user** (`gs\gameover.gs`, `PROC_LEADER_DEATH`). The
  procedure fires when a leader dies. When the dead player has no heir and
  `currentuser getleaderlocation pop -1 eq` is true -- which is always the case for slot 15 -- it
  only acts if `who_died currentuser eq`. So **when any other faction's leader dies, nothing
  happens**: that faction's armies, buildings and cities are *not* handed to the marauders and it is
  not deactivated. No script path ends the game merely because the current user has no leader; a
  native check might, which is what Q5 observes.

## Keys

The build unlocks the whole `cheat_keys` tier. Its table, with the destructive keys marked, is in
[`docs/cheat-keys-ladder.md`](cheat-keys-ladder.md#the-cheat_keys-hotkey-tier-key-by-key).

| Key | What it does | Where |
| --- | --- | --- |
| **Shift+J** | **Readout.** A message box: `cu=` current user, `cp=` current player (whose turn), `wmp=` the slot constant, `tc=` this computer, then for player 0 and the marauder slot (`MAR`): `f=` faith, `ai=` AI status, `cc=` controlling computer; `nMAR=` armies the marauder slot owns; `lord=` the current user's leader army (-1 = none). In combat it is a balloon instead. | anywhere, single player |
| **Shift+N** | **Spawn.** Creates a one-unit `decr1` marauder army (no brain) on the nearest empty land to the centre of the map view, and centres the camera on it. | map view, not zoomed out to the world, not in combat |
| **Shift+U** | **Takeover.** Marauder slot to human (`ai` 0), player 0 to AI (`ai` 1), then `WANDERING_MONSTER_PLAYER setuserforplayer`. | not in combat |
| **Shift+H** | **Hand back.** Player 0 to human, marauder slot to AI, `0 setuserforplayer`. | not in combat |
| `*` | Cheat tier: every unit in the selected army gets 10,000 experience and **1,000 move points**. | single player |
| `E` | End turn (shipped). | not in combat |
| Ctrl+S / Ctrl+L | Save / load (shipped). | not in combat |

**Never press `Y`** (`destroyterrainsprite`: permanently deletes whatever terrain sprite is
tracked, no confirmation -- this project has lost a village to it) **or `S`** (calls `superduper`,
which is defined nowhere; untested). Also avoid `M` (halts the army's move order), `K`/`k`, `F7`
and `F12` unless the ladder table says you want them. None of J, N, U, H is bound in the shipped
member; the build refuses to produce an archive if that ever stops being true.

## Before the run

**The Development profile is not currently vanilla.** Measured 2026-09-25 (read-only hashes):

| File | State |
| --- | --- |
| `gs.mpq`, `pic.mpq` | the **`gs5r3-base`** build `ad9fece3123a` (GS5R3 archives) |
| `imp.mpq`, `sndfx.mpq`, `special.mpq` | pristine |
| `lomse.exe` | HD overlay 0.5.0 terrain patch (`ddb43883...`), backup `lomse.exe.lomhd-backup` |
| `ddraw.dll`, `ddraw.ini`, `lomhd_*` | HD overlay 0.5.0 |

The probe is built on the **vanilla** `gs.mpq`. Installing it alone would leave vanilla scripts next
to GS5R3's `pic.mpq`, a combination nothing has tested. So restore the archives first:

```sh
# 1. Build and check. Installs nothing. Read the build id out of the report.
scripts/build-marauder-probe.sh
cat artifacts/marauder-probe/offline-checks.txt

# 2. Archives back to pristine vanilla (all five). Refuses while lomse.exe is running.
scripts/restore-dev.sh

# 3. Install the probe. Replaces gs.mpq and nothing else.
scripts/install-dev.sh marauder-probe BUILD_ID      # 366133c32874 when this sheet was written
```

**What these commands touch, stated plainly.** `install-dev.sh` writes only the archives named in
the build's `build.json`, and this build names **only `gs.mpq`** (checked by the build script).
`restore-dev.sh` writes only the five archives in `PIPELINE_ARCHIVES` (`gs.mpq pic.mpq imp.mpq
sndfx.mpq special.mpq`). **Neither touches `lomse.exe`, `ddraw.dll`, `ddraw.ini`, `lomhd_*` or the
portrait pack.** The HD overlay itself never writes an `.mpq` (its setup only reads `pic.mpq` and
`imp.mpq`). The one interaction: after step 2 the overlay runs against vanilla `pic.mpq` instead of
GS5R3's. Portraits pair by content, so the few that differ between the two installs will simply not
be upscaled; whether the HD terrain art still lines up with vanilla tilesets was not checked. None
of the probe's readings is a picture, so neither affects the result.

For reference, `build id 366133c32874`, `gs.mpq sha256
b6db884c024800d874d9af36f7c8289f1b27d20158d502a7a18ce18bf5e6a9d8`. A build id is a digest of the
tree, the base archives and the tool binaries, so recompiling the tools changes it; trust the report.

## The run

Open **`Lords of Magic Development.app`** from `~/Applications/`, nothing else. Start a **new
single-player game, any faith** (the campaign's New Game). Note the faith you picked. Reach the
world map with the **map view**, not the zoomed-out world view. Write down every number the readout
shows at every step; the numbers are the result.

**How many factions does the game have?** The default campaign has all eight. If you pick a game
with fewer, the prediction for step 3 changes (see outcome B), which is fine but must be noted.

### Step 1 -- the control: Shift+J before anything else

This proves the readout is connected before it is asked anything.

| Field | Predicted | If not |
| --- | --- | --- |
| `cu` | `0` | the human is not player 0; the U and H keys hard-code 0 for the original human, so stop and report |
| `cp` | `0` (your turn) | note it |
| `wmp` | `15` | **Q1 answered differently from the static reading**; report the number |
| `tc` | a small number, probably `1` | note it |
| `P0 f` | your faith's number (LIFE 0 ... AIR 7) | the readout is not reading what it claims |
| `P0 ai` | `0` | as above |
| `MAR ai` | `1` (set by `setup_ai`) | **Q1**: the marauders are not flagged AI |
| `MAR f` | **unknown**; predicted outside 0..7 | this is the Q1 reading |
| `P0 cc`, `MAR cc` | probably equal to `tc` | if `MAR cc` differs from `tc`, that alone blocks `setuserforplayer` (it requires the player to be on this computer), and it names the blocker |
| `nMAR` | 0 or more (security forces, dungeon garrisons) | note it |
| `lord` | 0 or more (your lord's army) | note it |

If **no box appears at all**: the key is not reaching the script (a native binding, or the build is
not installed). Press `*` with your lord's army selected: if the lord goes to level 9 the build is
installed and the fault is the J binding itself.

### Step 2 -- Shift+N, then Shift+J

| Observed | Meaning |
| --- | --- |
| Camera jumps to a new one-unit army, and `nMAR` is exactly one more than in step 1 | spawn works; this is the army to use from now on |
| `nMAR` unchanged, camera did not move | `findemptylocation` or `getlocationatscreenxy` returned -1 (centre of view off the map?) -- scroll to open land and retry |
| An error dialog | report its text; the spawn body is wrong |

### Step 3 -- Shift+U, then Shift+J (Q2)

The AI fields prove the key ran, whatever `cu` does: predicted `P0 ai=1` and `MAR ai=0`.

| `cu` after U | Meaning | Prediction |
| --- | --- | --- |
| **A. `0` (unchanged)** | `setuserforplayer 15` found no user bound to 15 and nothing to fall back to. **The static reading is confirmed: the scripts cannot bind the human to slot 15.** | **predicted, most likely** |
| **B. some `k` in 1..7** | the fallback ran: the first non-AI player on this computer that has a user record was an unused faction slot `k`. Also confirms the static reading. | possible in games with fewer than 8 factions |
| **C. `15`** | the binding worked; the static reading is wrong somewhere | not predicted |

**A or B:** press **Shift+H**, then **Shift+J**: predicted `cu=0`, `P0 ai=0`, `MAR ai=1`. Do **not**
end the turn between U and H: player 0 is flagged AI in between. Then go to step 4.

**C:** go to step 5.

### Step 4 -- the save round trip (Q4), after outcome A or B

Shift+U, then Ctrl+S and save as `mprobe`. Ctrl+L and load `mprobe`. Shift+J.

| Observed after the load | Meaning |
| --- | --- |
| `P0 ai=0`, `MAR ai=1`, `cu=0` | **predicted**: `final_setup` re-ran `setup_player_control`, `setup_ai` and `0 setuserforplayer` on load. The AI flags do not survive a load; a real mod must redo its setup in `final_setup` |
| `P0 ai=1` or `MAR ai=0` | the flags survived the load; `final_setup` did not run, or ran something else |

Keep `mprobe` (from `savegame/` in the Development profile): it is the input for Rung 2.
Press Shift+H before quitting.

### Step 5 -- only after outcome C

1. **Move.** Click the army from step 2 and read its move points on the army panel (a number). Order
   a short move. If it will not move, press `*` with it selected (1,000 move points) and try again.
   Shift+J: `lord` should read `-1`.
2. **Turn.** Press `E`. After each AI turn, Shift+J. **Q2 turn**: does `cp` ever read `15` while
   the game waits for you? Or does play come back with `cp=0` (player 0, now AI) and skip you?
3. **Q5.** Keep ending turns for a few rounds. Note any dialog, especially a defeat or "game over"
   screen, and the turn it appears on. Predicted from the scripts: none, because nothing in script
   ends the game for a leaderless user; a native check is the open question.
4. **Q3 (optional).** Attack something with the army. In combat, Shift+J shows a balloon. Can you
   select and order your units?
5. **Q4.** Ctrl+S as `mprobe-c`, Ctrl+L it, Shift+J. Predicted: `MAR ai=1` again (`setup_ai`), and
   `cu` stays `15` only if the binding lives in the user record, which the static reading says it
   cannot.

Press Shift+H, then quit.

## Predictions and what each outcome means for the plan

| Q | Outcome | Plan |
| --- | --- | --- |
| Q1 | `wmp=15`, `MAR ai=1`, `MAR f` outside 0..7 | **predicted**. Script-only is fine for reading the slot. A faith outside 0..7 means every unguarded faith-array lookup in the UI is a crash risk for a human at 15: script work, not an exe patch. |
| Q1 | `MAR cc` differs from `tc` | one named blocker, script-fixable (`WANDERING_MONSTER_PLAYER thiscomputer setcontrollingcomputer`, as `gs\scenario\network.gs` already does for the host) |
| Q2 | A or B | **predicted**. **Not possible by script alone.** Next: Rung 2 (save edit). If that works, a real mod needs either a save-edited start or a small exe patch that lets `setuserforplayer` re-point the current user record. |
| Q2 | C, and the army moves and `cp` reaches 15 | script-only is viable; the static reading of `0x0052CEF0` is wrong and must be corrected |
| Q2 | C, but `cp` never reaches 15 | bound but no turn: the engine skips slot 15 in its turn order. Exe patch territory |
| Q3 | units controllable | script-only for combat |
| Q3 | combat plays itself | combat control keys off something other than `currentuser`; exe work |
| Q4 | flags reset on load | **predicted**. A real mod must redo the takeover in `final_setup`. The user binding itself survives only as the save's `LS_USER +0` word |
| Q5 | no dialog over several turns | **predicted from the scripts** -- a leaderless user is not killed by any script path. Note the side effect: other factions' deaths stop handing their property to the marauders |
| Q5 | a defeat or game-over screen | a native elimination check exists; find it before anything else. Exe patch or dead end |

## Rung 2 (not built): bind a user record to slot 15 in a save

If step 3 gives A or B, the static reading says the binding can only come from the user record.
The save carries it: `LS_USER` record 0's first word is user 0's bound player. The next rung:

1. From step 4's `mprobe`, write a copy whose `LS_USER` record 0 `+0` word is `15` instead of `0`.
   Nothing does this yet; the save module in `spikes/asset-viewer` parses and re-encodes `LS_USER`
   (eight 784-byte records) and is the place to add it.
2. Load it with this same build. `final_setup` will set slot 15 back to AI and call
   `0 setuserforplayer`, which finds no user for player 0 and -- in an eight-faction game, where
   players 1..7 are all AI -- no non-AI fallback with a user, so it changes nothing.
   **Predicted: `cu=15` immediately after the load**, `MAR ai=1`, `P0 ai=0`. (With fewer
   factions the fallback may land on an unused slot, as in outcome B.)
3. Press Shift+U. Now slot 15 is non-AI and has a user, so `setuserforplayer 15` succeeds and the
   AI flags are right. Then run step 5.

That tests the whole question without an exe patch, and tells the plan whether the binding is the
only obstacle.

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

## After the run

```sh
scripts/restore-dev.sh                   # archives back to pristine vanilla; HD overlay files untouched
```

To put the Development profile back exactly as it was before the probe (GS5R3 archives under the HD
overlay), restore the `gs5r3-base` build. It lives only in two other worktrees' `artifacts/`, which
are removed when those branches merge, so point at one explicitly:

```sh
LOM_ARTIFACTS_DIR=/Users/jakebliss/personal-projects/lords-of-magic-modding.claude-hd-terrain/artifacts \
  scripts/restore-dev.sh --to gs5r3-base ad9fece3123a
```

That verifies both archives against the build's `build.json` before writing, and writes `gs.mpq`
and `pic.mpq` only.
