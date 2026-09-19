# `unitanchor` run sheet

One attended keypress. It closes the one limit [`hotspots.md`](hotspots.md#still-open) states
about its own central result.

## The question

[`hotspots.md`](hotspots.md) is the project's source of truth for sprite placement:

```
top_left = anchor + placement - (width >> 1, height >> 1)
```

Record 0 of a hotspot-bearing frame was confirmed as the `placement` in that rule on 2026-09-16
([research log](research-log.md#2026-09-16--hotspot-record-0-is-the-draw-placement-and-the-rule-is-the-same-one))
— but by injecting a **unit** IMP (`units\imp\aicr2a.imp`) through the **terrain sprite** draw
path, via `addterrainspritetype`. That run's own write-up says what it does not prove:

> This placed a unit IMP through the **terrain sprite** path. It proves the renderer reads record 0
> from the IMP frame and applies the measured rule; it does not prove the unit draw path computes
> its *anchor* the same way. The sign, the centre-relative form and the choice of record 0 are
> settled; a unit-specific constant offset in the anchor is not ruled out.

If the unit draw path adds a constant to the anchor that the terrain-sprite path does not, every
number `hotspots.md` publishes is consistently wrong for a real recruited/summoned unit — which is
exactly the case a custom-unit modder hits. **This probe places a real unit through the real unit
draw path and asks whether its measured top-left matches the published rule using the *same*
anchor the terrain-sprite path uses, on the *same* cell, in the *same* capture.**

## The method

The one that already worked twice on 2026-09-16 (the hotspot-sign measurement and the record-0
confirmation): **one cell**, subjects placed and removed in turn, each captured against a single
shared plate, so the camera position and cell-to-screen projection — both unknown constants —
cancel between rungs instead of needing to be solved for or assumed equal.

| Rung | What it places | Path | Purpose |
| ---: | --- | --- | --- |
| **0** | `terrainsprites /orchard get` — the ladder run's own shipped sanity control | terrain sprite | If this rung's silhouette does not land where the published rule predicts, the capture or the cell logic is wrong here, and rungs 1–2 are not evidence of anything. |
| **1** | `units/imp/licr2a.imp` registered by filename, exactly the way `aicr2a.imp` was on 2026-09-16 | terrain sprite | **The same art the unit rung uses**, through the already-proven path. This is what the record-0 confirmation itself did; reproducing it here, on the identical cell, is what makes rung 2 comparable rather than merely similar. |
| **2** | a real Unicorn (`/licr2`), recruited with `add_unit_to_location` | **unit** | **The question.** Same art (`licr2a.imp` is exactly the file this unit type's `impfile_proc` resolves to), same cell, different draw path. |

Every rung is captured against `zu0.bmp`, the one plate taken before anything is placed.

### Why a Unicorn

`units/imp/licr2a.imp` has the same shape of file as the `aicr2a`/`aicr2b` pair the record-0
experiment used: two hotspot records per frame (record 0 the draw placement, record 1 type 7).
Its **STAND** sequence is five frames, 30–34, one *static* pose per facing — no animation cycle to
land on mid-tick, which is exactly the ambiguity that sank the very first hotspot attempt (an army
banner "whose sequence, facing and cycle position were all unknown"). Read offline with
`--describe-imp` (no game running), the five STAND sizes plus the MOVE frame the terrain-sprite
path always draws are:

| Frame | Sequence, facing | Size | Record 0 `x,y` |
| ---: | --- | --- | --- |
| 0 | MOVE, facing 0 | 30×122 | `(1, -25)` |
| 30 | STAND, facing 0 | 28×124 | `(-1, -23)` |
| 31 | STAND, facing 1 | 100×105 | `(-2, -25)` |
| 32 | STAND, facing 2 | 136×81 | `(-1, -27)` |
| 33 | STAND, facing 3 | 93×85 | `(0, -15)` |
| 34 | STAND, facing 4 | 27×99 | `(0, -13)` |

**All six sizes are distinct.** That is the whole reason facing does not need to be forced or even
known ahead of time: whichever frame the unit draw path actually shows is identified from its
measured silhouette size alone, against this table, by `tools/probe_captures.py` after the run.
Forcing the facing with `... ARMY_FACING n setarmydata` was considered and rejected — `setarmydata`'s
three-operand shape is shipped (`attacker ARMY_OWNER 0 setarmydata`), but no shipped call site sets
`ARMY_FACING` specifically, and constructing that call would be exactly the kind of
reconstruction-from-resemblance [`hotspots.md`'s "Do not" section](hotspots.md#do-not) warns
against. `getarmydata ARMY_FACING` is still logged, purely as a free cross-check if it turns out to
agree with the size-based identification.

### The unit call, copied not reconstructed

`add_unit_to_location` (`gs\ENC_TOOL5.gs`) pops `/owner /loc /this_name /this_artlist /this_str
/this_type`, so it is called `TYPE STR ARTLIST NAME LOC OWNER add_unit_to_location`. This probe's
call is `gs\PLAYER5.gs:430` **verbatim**, with only the location and owner swapped for its own:

```
shipped:  unittypedict begin /licr2 end 0{}0 start_loc 2 add_unit_to_location
probe:    unittypedict begin /licr2 end 0{}0 zcell zowner add_unit_to_location
```

`add_unit_to_location`'s own body ends on `... ischampion?{...}if`, a boolean branch — it pushes
nothing back. The created army is found afterward the way three shipped call sites do it, by its
location (`gsandenc.gs`'s `x y armyat`), which is exact here because `findemptylocation` guaranteed
the cell was empty of units immediately beforehand:

```
zcx zcy armyat /zarmy exch def
```

Cleanup is `zarmy deletearmynow` — the shipped one-operand cleanup every spell-summon in the corpus
uses (`gs\GAMEUTIL5.gs`'s `summon_cleanup`, `gs\spells\AIR_raise_frozen_shade.gs`) — an exact match
on the id `armyat` handed back. An army has no sprite type, so the terrain-sprite type-and-location
sweep does not apply to it and is not used here.

## Before the run

```sh
cd spikes/asset-viewer && cargo build --release && cd ../..
LOM_PROBE=unitanchor scripts/install-engine-probe.sh
```

No sprites are injected — `licr2a.imp` is shipped art, referenced by its existing path, exactly the
way the `elevation` probe references `imp/tree4e.imp`. Only `gs\hotkey.gs` and `START.GS` are
patched.

## The run

1. Launch **Lords of Magic GS5R3**.
2. Start (or resume) a **single-player game** and reach the **world map**, with your starting army
   visible on screen. Ordinary macOS-blocks-synthetic-input territory — this part has to be a
   person.
3. Prefer a spot with **no hostile stack adjacent** to your army. The probe's Unicorn belongs to
   `currentplayer`, so it should never provoke combat, but there is no reason to test that.
4. Press **`z`**, once.
5. Watch for a Unicorn to flash into existence next to your army and vanish a moment later — that
   is rung 2's placement and its own cleanup, not a bug. Expect one redraw, five captures, and the
   probe's own log lines in `zprobe.log`.
6. Quit the game. **Do not save.**

The probe guards itself with `zdone`, so a second `z` does nothing.

## After the run

```sh
scripts/restore-game-archives.sh
```

Then, from the collected run directory:

```sh
python3 tools/probe_captures.py zu0.bmp zu1.bmp zu2.bmp zu3.bmp zu4.bmp
```

This reports the changed-pixel bounding box for each capture against the plate. Read `zprobe.log`
alongside it for the cell, the two type ids, the army id, and — for rung 2 — the reported facing.

### Solving it by hand

1. **Rung 1** (`zu2.bmp` against the plate): measure its bounding box. It should be `30×122` at
   some `(left, top)` — frame 0's size, since a terrain sprite never animates. Solve
   `anchor = (left, top) - (1, -25) + (15, 61)` (half of `30×122`, floored).
2. **Rung 2** (`zu3.bmp` against the plate): measure its bounding box. Match its size against the
   six-row table above to identify which frame drew. Using **that** frame's own record-0 placement
   and the **same** `anchor` from step 1, compute the predicted top-left with the published rule
   and compare it to what was actually measured.

## What each outcome would mean, stated before the run

- **Rung 0 does not land where the published rule and this session's own recovered anchor predict**
  → the capture or the cell logic is wrong in this run. Nothing else here is evidence, and rung 2
  should not be read as a verdict on the unit path either way.
- **Rung 0 passes, rung 1's measured top-left does not match the rule using its own record-0
  placement** → the terrain-sprite path itself has regressed or this cell/session differs from
  2026-09-16 in some way not yet understood. Stop here; rung 2 is uninterpretable without rung 1
  holding.
- **Rungs 0–1 pass, rung 2's measured top-left matches the same anchor** (within a pixel — the
  published rule floors, not rounds) **using its own identified frame's record-0 placement** → the
  unit draw path computes its anchor **the same way** the terrain-sprite path does. The published
  rule needs no unit-specific correction, and the caveat in `hotspots.md` can be closed as
  answered rather than merely narrowed.
- **Rungs 0–1 pass, rung 2 is off by a constant `(dx, dy)`** → the unit draw path adds a
  unit-specific offset to the anchor. That constant is the fix custom-unit modders need, and it
  should be re-measured on a second unit type before being published as general, the same way the
  original hotspot sign needed more than one sample.
- **Rung 2's silhouette size matches none of the six candidate frames** → the identification method
  itself is broken (a different frame drew than any anticipated, or the capture missed something),
  not a result about the anchor. Read `zprobe.log`'s facing line and re-derive the frame table from
  `--describe-imp` before drawing any conclusion.
- **No army is found at start** (`"no army found; nothing placed"` in the log) → the probe never
  ran its body past the anchor lookup. Not a result; reposition and re-run.

## What this probe writes, and what removes it

- **Writes:** nothing persistent. `zt0`/`zt1` are terrain sprite type ids registered for the
  duration of the keypress; `zt0`'s sprite and `zt1`'s sprite are both destroyed, matched by type
  **and** the shared cell (`zt0` is the *shipped* orchard type, so a type-only sweep on it would
  delete every orchard the map generator placed — the same trap the ladder run's own rung 0
  guards against). `zt1` additionally gets a type-only sweep, safe because it was minted this
  keypress. The Unicorn army is deleted by its exact id (`zarmy deletearmynow`), never by location
  or type. `gs.mpq` and `START.GS` are modified and rolled back by
  `scripts/restore-game-archives.sh`, verified against `MANIFEST.sha256`. No file under the game's
  loose `map/` directory is touched.
- **Removes it:** `scripts/restore-game-archives.sh` restores both archives and collects the five
  `zu*.bmp` captures and `zprobe.log` into a fresh, per-run directory. `zu4.bmp`, taken after the
  Unicorn is deleted, should differ from the plate by nothing but ordinary map animation — if it
  does not, cleanup left something behind, and that is itself worth reporting.

## Cost and risk

One keypress. No archive member is added or replaced — `gs\hotkey.gs` and `START.GS` are patched in
place, both restored and verified byte-identical afterward, and `licr2a.imp` is only ever read. The
new army exists only for the duration of one capture before it deletes itself; nothing is saved.
