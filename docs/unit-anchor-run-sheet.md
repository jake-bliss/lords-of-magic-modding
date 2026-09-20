# `unitanchor` run sheet

> **RUN 2026-09-19 on GS5R3 (ManTerA GS5R Enhanced 3.02.3243). The probe worked; the question is
> still open, and the reason is a design flaw in this sheet rather than a bug in the probe.**
>
> All four rungs fired, both rung-2 cleanups ran, nothing was left on the map, and
> `scripts/restore-game-archives.sh` verified both archives back to their recorded originals
> (`gs.mpq 2d394279…`, `imp.mpq cb5c1068…`).
>
> | rung | measured union bbox | expected |
> |---|---|---|
> | 0 orchard | 60x70 at (358,152) | — |
> | **1 licr2a via terrain path** | **30x122 at (374,136)** | **30x122** ✅ |
> | 2a unit path | 47x72 at (365,165) | one of the six candidates |
> | 2b unit path | 47x72 at (365,165) | — |
>
> **The control passed exactly** — frame 0, record 0 `(1,-25)`, reproducing the 2026-09-16 result on
> a new cell in a new session. **Rungs 2a and 2b are pixel-identical**, so the unit path is drawing
> reproducibly.
>
> **But 47x72 is not any frame of `licr2a.imp`.** Re-derived with `--describe-imp` over the whole
> file: the nearest sizes are 53x70, 54x73, 41x83 and 45x97, and **no frame measures 47x72**. This is
> stronger than the "matches none of the six candidates" case this sheet anticipated — a different
> *sprite* drew, not a different frame. **Observed in gameplay, 2026-09-19.**
>
> **The probable cause, Inferred and not yet confirmed:** on the world map a unit added to a location
> is drawn as an **army**, and `licr2a.imp` is the unit's combat sprite. That is the same ambiguity
> that sank the first hotspot attempt -- an army banner "whose sequence, facing and cycle position
> were all unknown". Rung 1 works precisely because `addterrainspritetype` bypasses that and draws
> the IMP directly.
>
> **Two unexplained observations, recorded rather than interpreted:**
> - `zprobe.log` reports `facing 4` for BOTH rung 2a and 2b, so this run carries one facing sampled
>   twice, not the two independent facings the design asks for.
> - A 10x26 component at (358,259) appears from `zu4` onward and persists through both cleanups. It
>   is not part of either subject and its source is unknown.
>
> **An instrument defect found on the way:** `tools/probe_captures.py` reports *connected
> components*, and `licr2a`'s frame 0 is split by a transparent gap into 30x111 plus 8x9. Read
> component-wise, a control that passed exactly looks like it failed by 11 pixels. The first reading
> of this run made that error. The tool should report the union alongside the components.
>
> **What would answer the original question:** a design that draws the unit through a path whose art
> is known, or one that identifies whatever the army sprite actually is before assuming it. Until
> then the caveat in [`hotspots.md`](hotspots.md#still-open) stands unnarrowed.


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
| **0** | `terrainsprites /orchard get` — the ladder run's own shipped sanity control | terrain sprite | If this rung's silhouette does not land where the published rule predicts, the capture or the cell logic is wrong here, and rungs 1–2 are not evidence of anything. Skipped, with a log line, if an orchard is already found standing on the target cell — see [What this probe writes](#what-this-probe-writes-and-what-removes-it). |
| **1** | `units/imp/licr2a.imp` registered by filename, exactly the way `aicr2a.imp` was on 2026-09-16 | terrain sprite | **The same art the unit rung uses**, through the already-proven path. This is what the record-0 confirmation itself did; reproducing it here, on the identical cell, is what makes rung 2 comparable rather than merely similar. |
| **2a, 2b** | a real Unicorn (`/licr2`), recruited with `add_unit_to_location`, placed and removed **twice** | **unit** | **The question — and its reproducibility.** Same art (`licr2a.imp` is exactly the file this unit type's `impfile_proc` resolves to), same cell, different draw path. Facing is never forced (see [Why a Unicorn](#why-a-unicorn)), so one placement samples exactly one facing and cannot tell a residual that is *constant* from one that merely happened to be non-zero once. Rung 2 runs twice so the run carries two independent facing draws to compare, not one. |

Every rung is captured against `zu0.bmp`, the one plate taken before anything is placed.

The seed cell for `findemptylocation` is an *offset* from the army's own location (`zax0+2, zay0`),
never the army's own occupied cell — matching the other engine probes in this project. If
`findemptylocation` can return its own seed when it judges that cell acceptable (unverified without
the engine; see the note under [What this probe writes](#what-this-probe-writes-and-what-removes-it)),
seeding from the occupied cell would risk handing rung 2 the player's own starting army instead of
the one it placed.

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
location (`gsandenc.gs`'s `x y armyat`):

```
zcx zcy armyat /zarmy exch def
zarmy ARMY_LOCATION getarmydata /zaloc2 exch def
```

`findemptylocation` guaranteed the cell was empty of units *when it ran*, not immediately before
this line — two terrain-sprite rungs, each with its own `rendermap refreshdirty` and
`screencapture`, execute in between. That the cell is still empty here is therefore an inference,
not an observation, and the probe checks it rather than assuming it: cleanup only runs when
**both** `zarmy` is a valid id and `zaloc2` matches `zcell`.

```
zarmy -1 ne zaloc2 zcell eq and
	{zarmy deletearmynow}
	{log: "cleanup REFUSED"}ifelse
```

Cleanup itself is `zarmy deletearmynow` — the shipped one-operand cleanup every spell-summon in the
corpus uses (`gs\GAMEUTIL5.gs`'s `summon_cleanup`, `gs\spells\AIR_raise_frozen_shade.gs`) — an exact
match on the id `armyat` handed back, never an unconditional call. An army has no sprite type, so
the terrain-sprite type-and-location sweep does not apply to it and is not used here.

Whether `armyat` can ever hand back an army other than the one this rung just placed, and what
`deletearmynow` does when given an id that fails the gate, are GameScript semantics this project has
no way to check without the engine. The gate is correct regardless of the answer to either question
— it is what turns "the cell was empty a moment ago" from an assumption into something the probe
itself verifies before acting on it. This whole sequence — place, find, gate, delete — runs **twice**
in one keypress (rung 2a with `zarmy`/`zaloc2`, rung 2b with `zarmy2`/`zaloc3`), both on the same
cell, so the run carries two independent facing draws rather than generalizing from one.

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
5. Watch for a Unicorn to flash into existence next to your army and vanish, **twice in a row** —
   that is rung 2a's placement and cleanup, immediately followed by rung 2b's, not a bug or a
   double keypress. Expect one redraw, seven captures, and the probe's own log lines in
   `zprobe.log`. If you also see rung 0's placement (a shipped orchard) flash and vanish, that is
   normal too; if the log instead says `rung0 SKIPPED`, an orchard was already standing on the
   target cell and rung 0 did not run — reposition and re-run if you want that control.
6. Quit the game. **Do not save.**

The probe guards itself with `zdone`, so a second `z` does nothing.

## After the run

```sh
scripts/restore-game-archives.sh
```

Then, from the collected run directory:

```sh
python3 tools/probe_captures.py zu0.bmp zu1.bmp zu2.bmp zu3.bmp zu4.bmp zu5.bmp zu6.bmp
```

If rung 0 was skipped (`rung0 SKIPPED` in the log), `zu1.bmp` will not exist — pass the captures
that are actually present. This reports the changed-pixel bounding box for each capture against the
plate. Read `zprobe.log` alongside it for the cell, the two type ids, both army ids, and — for each
of rung 2a and 2b — the reported facing and whether cleanup ran or was refused.

### Solving it by hand

1. **Rung 1** (`zu2.bmp` against the plate): measure its bounding box. It should be `30×122` at
   some `(left, top)` — frame 0's size, since a terrain sprite never animates. Solve
   `anchor = (left, top) - (1, -25) + (15, 61)` (half of `30×122`, floored).
2. **Rung 2a** (`zu3.bmp` against the plate): measure its bounding box. Match its size against the
   six-row table above to identify which frame drew. Using **that** frame's own record-0 placement
   and the **same** `anchor` from step 1, compute the predicted top-left with the published rule
   and compare it to what was actually measured. Call the difference between predicted and measured
   `residual_a` (zero if they match).
3. **Rung 2b** (`zu5.bmp` against the plate): the same procedure, independently — its own frame
   identification, its own predicted top-left from the **same** `anchor`, its own residual
   (`residual_b`). Do not assume it matches rung 2a's frame; the whole point of running it is that
   nothing forces it to.

## What each outcome would mean, stated before the run

This is genuinely two questions, and one capture of one facing can only ever answer the first:

- **(a) Is the residual zero or non-zero?** — answerable from rung 2a alone.
- **(b) If non-zero, is it the *same* non-zero value regardless of facing (i.e. a constant unit
  anchor offset), or does it vary?** — not answerable from one observation. A single placement
  samples exactly one of the five STAND facings (or the MOVE frame); if the true residual varies by
  facing, one capture reading `(4, -2)` looks exactly like a constant `(4, -2)` until a second,
  independent facing is measured and either agrees or does not. That is what rung 2b is for.

- **Rung 0 does not land where the published rule and this session's own recovered anchor predict**
  → the capture or the cell logic is wrong in this run. Nothing else here is evidence, and rung 2
  should not be read as a verdict on the unit path either way. (If rung 0 was skipped because an
  orchard already stood on `zcell`, this check cannot run at all this session; re-run on a cell
  without one if the control matters to you.)
- **Rung 0 passes, rung 1's measured top-left does not match the rule using its own record-0
  placement** → the terrain-sprite path itself has regressed or this cell/session differs from
  2026-09-16 in some way not yet understood. Stop here; rung 2 is uninterpretable without rung 1
  holding.
- **Rungs 0–1 pass, both rung 2a and rung 2b's measured top-lefts match the same anchor** (within a
  pixel — the published rule floors, not rounds) **using each one's own identified frame's record-0
  placement** → residual is zero on both observations. The unit draw path computes its anchor **the
  same way** the terrain-sprite path does. The published rule needs no unit-specific correction, and
  the caveat in `hotspots.md` can be closed as answered rather than merely narrowed.
- **Rungs 0–1 pass, rung 2a and rung 2b are both off by the SAME constant `(dx, dy)`** → the unit
  draw path adds a unit-specific offset to the anchor, and the two facings agreeing supports it
  being constant rather than incidental. That constant is the fix custom-unit modders need, and it
  should still be re-measured on a second unit *type* before being published as general — two
  facings of one unit is not two units, the same way the original hotspot sign needed more than one
  sample.
- **Rungs 0–1 pass, rung 2a and rung 2b disagree** (zero vs non-zero, or two different non-zero
  values) → **this refutes "constant."** The residual is facing-dependent (or otherwise not a
  simple additive offset), and no single number can be published from this run. Do not average the
  two or report either alone; report both residuals and the facing each came from, and treat the
  unit-anchor question as still open pending a design that can hold facing fixed or sample more of
  it.
- **Rung 2a's or rung 2b's silhouette size matches none of the six candidate frames** → the
  identification method itself is broken for that observation (a different frame drew than any
  anticipated, or the capture missed something), not a result about the anchor. Read `zprobe.log`'s
  facing line for that rung and re-derive the frame table from `--describe-imp` before drawing any
  conclusion. The other observation, if it identified cleanly, still stands on its own.
- **`zprobe.log` says `cleanup REFUSED` for rung 2a or 2b** → the army id or its reported location
  did not match what the probe expected, so cleanup deliberately did not run rather than deleting
  something it could not confirm was its own. The corresponding army may still be on the map;
  check in-game and remove it by hand if so before quitting.
- **No army is found at start** (`"no army found; nothing placed"` in the log) → the probe never
  ran its body past the anchor lookup. Not a result; reposition and re-run.

## What this probe writes, and what removes it

- **Writes, if every guard passes:** nothing persistent. `zt0`/`zt1` are terrain sprite type ids
  registered for the duration of the keypress; `zt1`'s sprite is destroyed by a type-only sweep,
  safe because the id was minted this keypress and nothing else on the map can carry it. `zt0` is
  the *shipped* orchard type shared by every orchard the map generator placed, so before rung 0
  places anything it first checks whether an orchard is **already** standing on `zcell` and, if so,
  skips the rung entirely (logging `rung0 SKIPPED`) rather than placing on top of it and later
  sweeping by type-and-cell — the same class of loss as this repository's recorded
  village-deletion incident. When rung 0 does run, its own placement is removed the same way, by
  type **and** the shared cell. The Unicorn army from each of rung 2a and 2b is deleted by its exact
  id (`zarmy`/`zarmy2 deletearmynow`), gated on that id being valid **and** its reported location
  matching `zcell` — never unconditionally, and never by location or type sweep. `gs.mpq` and
  `START.GS` are modified and rolled back by `scripts/restore-game-archives.sh`, verified against
  `MANIFEST.sha256`. No file under the game's loose `map/` directory is touched.
- **What is not independently verifiable here:** whether `findemptylocation` can hand back the
  cell it was seeded from, whether `armyat` on a cell that is no longer empty can return an army
  other than the one this rung just placed, and what `deletearmynow` does with an id that fails the
  gate are all GameScript engine semantics with no offline way to check them (this project has no
  interpreter for the language, only a reader for the compiled form). The guards above are correct
  regardless of the answer to any of those questions — that is why they check rather than assume —
  but a reviewer cannot confirm the underlying engine behavior itself without running the probe.
  Rung 0's orchard check has the matching limit: it proves the generator emits a presence check
  before placing, not that a real orchard was there to detect, since the test fixture is generated
  script text, not a map.
- **If a guard refuses:** the log says so instead of silently proceeding either way. `rung0
  SKIPPED` means the probe never placed its own orchard at all, so there is nothing of this
  probe's to leave behind for that rung. `cleanup REFUSED` for rung 2a or 2b is the case that
  DOES leave something behind: the Unicorn that rung placed is still on the map when you quit —
  check `zprobe.log` before quitting and remove it by hand if so.
- **Removes it:** `scripts/restore-game-archives.sh` restores both archives and collects the
  `zu*.bmp` captures actually present (`capture_names_for("unitanchor")` in `tools/engine_probe.py`
  is the exact list; a skipped rung 0 means `zu1.bmp` was never written and there is nothing to
  collect for it) and `zprobe.log`, into a fresh, per-run directory. `zu6.bmp`, taken after rung 2b's
  Unicorn is deleted, should differ from the plate by nothing but ordinary map animation — if it
  does not, cleanup left something behind, and that is itself worth reporting.

## Cost and risk

One keypress. No archive member is added or replaced — `gs\hotkey.gs` and `START.GS` are patched in
place, both restored and verified byte-identical afterward, and `licr2a.imp` is only ever read. Each
of the two Unicorns exists only for the duration of one capture before its own gated cleanup runs;
nothing is saved.
