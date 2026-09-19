# Cheat-keys ladder run sheet

**Run attended 2026-09-19. Both rungs passed, and `cheat_keys` is LIVE.** Everything below the
outcome was written before either build was installed; both predictions are recorded ahead of the
observation, in the style of [`docs/engine-acceptance-ladder.md`](engine-acceptance-ladder.md).

## Outcome, 2026-09-19

**Evidence class: observed in gameplay.**

| Rung | `gs.mpq` | Observed |
| --- | --- | --- |
| A, `cheat_keys false`, member byte-identical | `cea24b878bb41b40` | Main menu, music and sound normal. **Delete on the world map: nothing moved, no balloon.** |
| B, `cheat_keys true`, one token | `94ba04db6b5c6bc1` | **`*` on a selected army: the Paladin Lord went from level 1 to LVL 9**, attack 24, armour 15, mana 10/20. Repeated `Delete` visibly rotated the map. |

**The debug tier is reachable by flipping one GameScript token in `gs.mpq`.** That is the cheapest
attended-test harness this project has: `*` sets every unit in the current army to 10,000 experience,
runs `set_level_modifications` and sets 1,000 move points; `K` grants all 160 spells; `k` fills wizard
mana. A future run that needs a levelled army in a particular state can press one key instead of
playing to it.

### `Delete` was the wrong instrument, and nearly cost the whole result

The first reading of rung B was *"a bunch of sprites moved"* after one press of `Delete`. That was
recorded as a confirmation. It should not have been:

- **A single `rotatex` step is below the threshold of visibility.** One press moves the camera by
  one increment and the screen looks the same. Pressing it repeatedly does move the terrain
  unmistakably — tower, huts and coastline all shift — but the run sheet asked for one press.
- **The world map animates on its own.** Creatures wander and idle frames cycle, so "sprites moved"
  is what an *inert* key looks like too. Two screenshots a few seconds apart differ either way.

The reading was withdrawn on that basis and then re-established by a different instrument entirely:
`*`, whose effect is a **number on a panel** rather than a change in a picture. Level 1 to LVL 9 is
not a judgement call.

This is [[feedback-ask-only-what-the-sensor-can-report]] for the third time in one evening. The
lesson that generalises: when an observation can be made either as a *quantity* or as an *image*,
take the quantity. A number needs no repetition, no threshold, and no comparison against a
background that is moving by itself.

### `set_level_modifications` raises MAX hit points and not current ones

**Observed in gameplay 2026-09-19.** After `*`, the Paladin Lord read **26/44** hit points. The
hotkey body does exactly three things — `UNIT_EXPERIENCE_POINTS 10000 setunitdata`,
`set_level_modifications`, `UNIT_MPS 1000 setunitdata` — and **none of them writes current hit
points**. So the maximum moves with the new level and the current value stays where it was.

Stated here because a levelled army *looks wounded*, and the next person to use this harness will
otherwise read it as damage caused by the modified archive.

### The control was added mid-run, and it was necessary

Rung B was read first and its result was **ambiguous as recorded**. `hotkey.gs` binds Delete as
`VK_VAL 46` inside the `cheat_keys` gate, but later — and *outside* the gate — it binds the period
key as `ASCII_VAL 46`, to game speed. The file opens with `256 setmaxhotkeys`. If `addhotkey` keyed a
single table by raw code, the later binding would win and Delete would already do something with the
flag `false`, which would make rung B's sprite movement say nothing about `cheat_keys` at all.

Rung A was therefore re-installed and **the same key pressed on the same screen**. Nothing moved and
no `"Game Speed:"` balloon appeared. Two things follow:

1. `cheat_keys` gates the binding, and the flag is what changed the behaviour.
2. **`VK_VAL` and `ASCII_VAL` are separate namespaces.** `VK_VAL 46` (Delete) and `ASCII_VAL 46`
   (`.`) are distinct hotkey slots, so the collision does not exist. Several other pairs in this file
   would otherwise silently overwrite one another — `ASCII_VAL 42` (`bugkeyenable`, ungated) beside
   the gated `ASCII_VAL "*"`, for one.

The omission is worth naming: the first pass installed only the changed rung and read its effect.
A no-op control existed for the *archive* (rung A's menu) but not for the *key*. Calibrating the
do-nothing case means the specific observation, not merely the build —
[[feedback-calibrate-the-no-op-before-the-change]].

## Why this exists

`gs\hotkey.gs` gates a tier of debug hotkeys behind one GameScript flag, `/cheat_keys`, set to
`false` in every shipped profile. If that tier turns out to work, it is a far better
attended-test harness than anything this project has: instant unit level, experience and move
points would let a future run reach a test state in seconds instead of by playing to it. This
document is the two-rung ladder that finds out, plus the full enumeration of what the tier
contains — several of its keys act on the current army or the current terrain sprite with no
confirmation prompt, and this repository already has a recorded incident of exactly that kind of
key destroying a village (`docs/agent-handoff.md`, `docs/research-log.md`). The table below exists
so nobody presses a key without knowing what it does first.

## The two rungs

| Rung | Mod | Archive | New variable |
| ---: | --- | --- | --- |
| **A** | `cheat-keys-noop` | `gs.mpq` | Control. `gs\hotkey.gs` re-emitted byte-identically. |
| **B** | `cheat-keys-true` | `gs.mpq` | The single token `/cheat_keys false def` -> `/cheat_keys true def`. Nothing else in the member differs. |

Rung A exists because rung B's success has two very different explanations if there is no
control: "the flag did nothing" and "we broke `gs.mpq`" produce the same null result on screen.
`gs\hotkey.gs` has never itself been through this pipeline before — `units\orinf.gs` proved this
storage class (`0x80010100`, EXISTS \| ENCRYPTED \| IMPLODE) survives a repack, but that is a
different member, and a no-op control is cheaper than assuming one repack's acceptance
generalizes to every member without saying so.

**Rung A is the cheaper observation and goes first.** It costs reaching the main menu and nothing
more. Rung B costs starting or loading a game and pressing one additional, deliberately harmless,
key.

## Before the run

```sh
# 1. Build both rungs and run every offline check. Installs nothing, launches nothing.
scripts/build-cheat-keys-ladder.sh

# 2. Record a pristine copy of gs.mpq and pic.mpq for the development profile, if not already done.
scripts/install-dev.sh --create-profile
```

Step 1 writes `artifacts/cheat-keys-ladder/offline-checks.txt`. **Read the build ids out of that
file**, not out of this one: a build id is a digest of the mod tree, the base archives and the
tool binaries, so recompiling the tools changes it. The ids below are what one particular build
produced and are here to be compared against, not copied blindly.

| Rung | `scripts/install-dev.sh MOD_ID BUILD_ID` | `gs.mpq` digest |
| ---: | --- | --- |
| A | `cheat-keys-noop 6dfd73067e89` | `cea24b878bb41b402266a9818726ce0b981fe0b83615a9fbde644ddc2d814055` |
| B | `cheat-keys-true 46d48c8333d2` | `94ba04db6b5c6bc1b1421ee751b104eedebc00ad406a82a2ff5736ce5d9c6e11` |

## The loop, once per rung

```sh
scripts/install-dev.sh MOD_ID BUILD_ID     # refuses while lomse.exe is running
```

1. Open **`Lords of Magic Development.app`** from `~/Applications/`. Nothing else: the three
   other profiles are the recovery baseline and `tools/install_guard.py` will not let this
   pipeline write to them.
2. Make the observation for the rung, below.
3. Quit the game.

```sh
scripts/restore-dev.sh                     # back to pristine, verified against MANIFEST.sha256
```

---

## Rung A — the repack control

**Install** `cheat-keys-noop`. **Where to look:** the main menu, the moment it appears. No
navigation, no clicks needed for the check itself.

**Why there.** `START.GS` runs `"gs/hotkey.gs" run` before the main menu opens, unconditionally,
in every session — this is not a debug-only load path. `gs\hotkey.gs` defines every ordinary
hotkey the game has, not only the cheat tier: arrow-key map panning, Space to pause combat, Escape
to open the in-game menu, F1 for help. If the repack corrupted this member's GameScript in any
way that the lexer or interpreter chokes on, the failure would not be confined to a debug key —
`run` failing partway through `START.GS` is a plausible way for the main menu never to appear at
all, or to appear with ordinary hotkeys silently unbound.

**Expected value:** the main menu appears exactly as it always does, with no error dialog. Not one
byte of the member differs from the shipped archive, so nothing about its appearance should
differ either.

| | |
| --- | --- |
| **If it works** | The main menu appears normally. Later, in an actual game, arrow keys still pan the map and Space still pauses combat — an optional deeper check, not required to read this rung. |
| **If it fails** | No main menu, an error dialog, or a crash on startup. Not one member's bytes differ from the shipped archive, so that would be the repack itself breaking `gs.mpq` — the exact possibility this rung exists to rule out before rung B is read. |

**What it proves.** That StormLib rewriting `gs.mpq` under its own `(listfile)` — with
`gs\hotkey.gs` declared as a repack target but producing identical bytes — still leaves an archive
the engine starts from. The build asserts, offline, that the packed member really is
byte-identical to the shipped one (`artifacts/cheat-keys-ladder/offline-checks.txt`); this rung is
what confirms the *engine* agrees, not just the tool.

**What it does not prove.** That the engine reads `gs\hotkey.gs` from *this* archive rather than
somewhere else — it cannot, because the expected screen is the shipped screen. Nothing in this
ladder needs that distinction: `gs.mpq` has no `sndfx.mpq`/`special.mpq`-style duplicate-elsewhere
question, and Phase 4 already established gs.mpq is the archive read for GameScript.

---

## Rung B — the one-token flip

**Install** `cheat-keys-true`. Start a new game or load a save, reach the world map (not combat),
and press **Delete** once.

**Why Delete, and not one of the tier's other keys.** Every other key in the tier either changes
army or map state (see the table below), requires being in combat, or calls a name this repack
cannot verify is safe (`S`). Delete calls `getxrot 1 add rotatex rendermap` — it reads the current
camera X-rotation, adds one, and re-renders. Nothing about it touches saved game state, and it is
reversible in kind by **Insert**, which does the same subtraction. **Do not press `Y`, `M`, `K`,
`k`, `S`, `F7` or `F12` during this check** — see the table for why.

**Expected value:** either the world-map view visibly rotates or tilts by one step, or nothing
happens. There is no rendered reference image for this one, because the effect (if any) is a
rotation of a live 3D view, not a fixed image `pbm.rs`/`imp.rs` can export ahead of time.

| Observed | What it means |
| --- | --- |
| **The view rotates or tilts** | Unambiguous: `cheat_keys` is `true` and this key is now bound to something it does nothing at all when `cheat_keys` is `false`. Rung A having passed rules out "the archive is broken", so this is the flag taking effect. |
| **Nothing visibly changes** | Ambiguous, the same way a corruption-control rung is ambiguous elsewhere in this project: it could mean the flag did not take effect, or it could mean `rotatex` has no visible effect in this build (the isometric camera may not expose a rotation Wine renders as a visible change). Rung A having passed, it does **not** mean the archive broke. If this happens, the next rung to build is one of the table's few keys with a state check a person can read without risk — e.g. pressing `*` on an army with units below max level and confirming their stats afterward — rather than assuming the null result answers the question. |
| **An error dialog, or a crash** | The interpreter rejected the edit. Rung A having passed with byte-identical content, this would point at the single-token edit itself — check the token boundary (`/cheat_keys true def` must parse exactly like `/cheat_keys false def`, which it should: `true` and `false` are both the same class of token). |

**What this proves and does not.** A clean rotation is strong evidence the tier is live. It does
not, on its own, establish that every other key in the tier behaves as read below — GameScript
lets a key be *bound* to a procedure that itself errors when it runs (see `S`, below), and this
rung tests only that binding exists and that one procedure in it runs cleanly.

---

## The cheat_keys hotkey tier, key by key

Everything here is gated by `cheat_keys` being `true` — none of it exists at all when the flag is
`false`. Most entries carry a **second**, independent runtime gate; the table's "also gated by"
column is that gate, not the flag.

Codes are Windows virtual-key codes for the non-printable keys (`VK_VAL`) and ASCII codes for the
letter/symbol keys (`ASCII_VAL`), exactly as `gs\hotkey.gs` spells them. The VK mapping below is
cross-checked against the file's own non-cheat arrow-key bindings (`VK_VAL 37/38/39/40` = Left/Up
/Right/Down, used for ordinary map panning a few lines earlier in the same member) rather than
assumed from a table alone.

| Key | Code | Also gated by | What it does | Destructive? |
| --- | --- | --- | --- | :---: |
| Delete | `VK_VAL 46` | none | `getxrot 1 add rotatex rendermap` — rotate the camera one step around X | No |
| Page Down | `VK_VAL 34` | none | `getxrot 1 sub rotatex rendermap` — the reverse of Delete | No |
| Page Up | `VK_VAL 33` | none | `getzrot 1 add rotatez rendermap` — rotate the camera one step around Z | No |
| Insert | `VK_VAL 45` | none | `getzrot 1 sub rotatez rendermap` — the reverse of Page Up | No |
| F12 | `VK_VAL 123` | single-player only (`getmultiplayerflag not`) | `toggle_gman_hotkey`, a native call (`0x00478300`). Effect undocumented anywhere else in this corpus — the name is the only evidence of what it does. | Unknown — not a data-deleting call by name, but unverified |
| F5 | `VK_VAL 116` | single-player only | Sets `setlineofsite false` on all four screen types, then either `setuserforplayer` (in combat) or `gamemode` (out of combat) | No — visibility/UI state |
| F4 | `VK_VAL 115` | single-player only | Same as F5 but `setlineofsite true` | No |
| F6 | `VK_VAL 117` | none (its own guard is a literal `true`, i.e. unconditional) | Reads `getnetworkstatusdisplay`, flips it, sets it via `networkstatusdisplay`, and conditionally re-renders (`rendermap`) depending on the value read — the exact stack order of the toggle was not traced past the operator names | No |
| F7 | `VK_VAL 118` | **multiplayer only** (`getmultiplayerflag`, not negated — the opposite of every other entry here) | Dumps every player's gold, food, crystals and army locations to `NetState.txt` via `savescriptwindow`, then **minimizes the game window** | No map/army damage, but writes a file and minimizes the window without asking |
| **Y** | `ASCII_VAL "Y"` (89) | single-player only | If `currentterrainsprite > -1`: **`destroyterrainsprite`**, the same native (`0x0050ddd0`) this repository's own recorded incident used to delete a village that `anythingat?` could not even see (`docs/agent-handoff.md`, `docs/research-log.md`). `currentterrainsprite` is set elsewhere (`gs\tree.gs` is the only other member that touches it — a mouse-hover/target tracker, not a coordinate lookup, and not traced further here) | **YES — permanent deletion of whatever terrain sprite is currently tracked, with no confirmation** |
| `*` | `ASCII_VAL "*"` (42) | single-player only | For every unit in `currentarmy`: sets `UNIT_EXPERIENCE_POINTS` to 10000, applies `set_level_modifications` (the same level-up routine used 279 times elsewhere in the corpus), and sets `UNIT_MPS` (move points) to 1000 | No — pure buff, this is the "instant max experience/level/move points" the brief asks about |
| M | `ASCII_VAL "M"` (77) | single-player only, and only if `currentarmy > -1` | Sets every unit's `UNIT_MPS` to 1000, then `stoparmy` — **halts the army's current move order** | Not map-destructive, but interrupts an in-progress movement with no undo |
| K | `ASCII_VAL "K"` (75) | single-player only, and only if `currentarmy > -1` | Loops spell ids 0–159 and calls `grantspellknowledge` for `currentarmy`'s owner — grants every spell in the game | No |
| k | `ASCII_VAL "k"` (107) | single-player only, and only if `currentarmy > -1` | For units of type LDW/WIZ/WZ2 in `currentarmy`: sets `UNIT_WIZARD_MANA` to 1000 | No |
| Q | `ASCII_VAL "Q"` (81) | none (unconditional) | `"break" erroroutput` — writes the literal string `break` through the error-output call | No visible effect expected; a debug marker, not a gameplay action |
| S | `ASCII_VAL "S"` (83) | single-player only | Calls `superduper`. **This name is defined nowhere in the corpus and is not a recognized native** — `reports/gs/vocabulary-vanilla.tsv` lists it as `unclassified-residue`, count 1, with no defining member anywhere in `gs.mpq`. Pressing this key most likely produces a GameScript "undefined name" error at runtime; it is not verified safe | Unknown — **do not press this key** until its failure mode is checked in isolation |
| L / l | `ASCII_VAL "L"` (76) / `ASCII_VAL "l"` (108), same handler | **combat only** (`incombat`) | `90 rotatemap` — rotates the combat-view camera 90 degrees. Does nothing outside combat | No |
| P / p | `ASCII_VAL "P"` (80) / `ASCII_VAL "p"` (112), same handler | single-player only | Adds a modifier tagged `shieldowind_aura` to `currentarmy` unit 0's `missile_range` stat via `MODIFIER_FLAG_FROM_SPELL` | No — a buff of undocumented magnitude |

**Bold destructive entries: `Y` only.** Nothing else in this tier deletes map or save content by
name. `M` and `F7` are called out separately because they change state a person did not ask to
change (an in-progress move order; a minimized window and a written file) without being
destructive in the "gone forever" sense `Y` is.

## The other cheat mechanism, and why it is not this one

`ASCII_VAL 3` (Ctrl+C) opens `cheat_dlg`, whose `verify_cheat` (`gs\dlg\panels.gs`) is a `strcmp`
chain over four words — `zilla` (spawns a unit of type `ficr3` at the current army's location and
adds it to the army's owner via `naddunit`; `ficr3` is a `unittypedict` entry not otherwise
identified in this pass — `reports/gs/vocabulary-vanilla.tsv` lists it as a plain
`script-definition` with no further gloss), `bingo` (+200 gold/food/crystals), `go far` (refill
move points and halt the army — the same `stoparmy` pattern as `M` above), `all spells` (grant
every spell plus refill wizard mana). A second variant at
`lbm\panels\panels.gs` uses `puff`, `jackpot`, `marathon`, `hocuspocus` for the same four effects
(not independently re-verified here; only the vanilla `gs\dlg\panels.gs` copy was read).

**This is a separate mechanism from `cheat_keys` and the two do not interact.** Verified in the
corpus:

- `verify_cheat` and `cheat_dlg` never reference `cheat_keys`, and `gs\hotkey.gs`'s `cheat_keys`
  tier never opens `cheat_dlg` or touches `cheat_buffer`.
- The Ctrl+C binding itself — `ASCII_VAL 3{getmultiplayerflag 0 eq getcheatmode 1 eq or{incombat
  not{panel_dict begin cheat_dlg opendialog end}if}if}addhotkey` — lives in `gs\hotkey.gs`
  **outside** the `cheat_keys{...}if` block entirely, immediately after it closes. It is gated by
  `(single-player) OR (getcheatmode == 1)`, and separately by `incombat not` (never during
  combat).
- `getcheatmode`/`setcheatmode` (native, `0x00487d00`/`0x00487c30`, as the brief states) are used
  **exactly twice** in the whole vanilla corpus: `gs\hotkey.gs` reads `getcheatmode` at that one
  Ctrl+C gate, and `gs\dlg\newdlg.gs` is the only member that ever calls `setcheatmode`. That call
  is `1 setcheatmode`, bound to `cheatspot` — a **5x5-pixel invisible hotspot at (0,0)** on the
  main-menu screen (`newdlg.gs`'s `allocate_objects`: `/cheatspot 0 0 5 5 xywh def`, added as an
  item with `{}{1 setcheatmode}{}additem`). Clicking that corner of the main menu is what turns
  `getcheatmode` on for the session; nothing else in the corpus calls `setcheatmode` at all, so
  there is no other way to reach `getcheatmode == 1` and therefore no other way to open `cheat_dlg`
  in multiplayer.

So: `cheat_keys` is pure load-time GameScript data, read once from a literal in `hotkey.gs` and
never touched by `getcheatmode`/`setcheatmode`. `cheat_dlg` is a live, native-backed runtime flag,
unlocked by a hidden click, that gates one dialog and nothing this ladder builds. **The brief's
framing lists both together; the corpus does not connect them**, and this is the one place this
document's reading adds something the brief did not already state rather than only confirming it.

## Runtime gates, summarized

| Mechanism | Gated by |
| --- | --- |
| `cheat_keys` tier (this ladder) | The `/cheat_keys` boolean alone, set once at `START.GS` boot from a literal in `gs\hotkey.gs`. **Not** derived from `getcheatmode`/`setcheatmode`. |
| Individual `cheat_keys` entries | Mostly `getmultiplayerflag not` (single-player only); `F7` is the one exception, requiring multiplayer; `L`/`l` requires `incombat`; camera rotation (`Delete`/`Page Up`/`Page Down`/`Insert`), `F6` and `Q` carry no second gate at all. |
| `cheat_dlg` (Ctrl+C word cheats) | `(single-player) OR (getcheatmode == 1)`, and separately `incombat not`. `getcheatmode` starts at 0 every session and is set to 1 only by clicking the hidden `cheatspot` on the main menu. |

## What this run sheet could not check offline

- Whether `toggle_gman_hotkey`, `F6`'s network-status toggle, or the `P`/`p` missile-range
  modifier have any *visible* effect — their natives and script wiring are confirmed to exist and
  to be reachable, not what they render as.
- Whether `S` (`superduper`) errors cleanly, hangs, or crashes — deliberately not tested by this
  ladder's own rung B, and not recommended as a first probe of the tier.
- The `lbm\panels\panels.gs` `puff`/`jackpot`/`marathon`/`hocuspocus` variant of `verify_cheat` was
  read but not independently re-verified against a second profile in this pass.
