# `unitanchor` run sheet

> **Status, 2026-09-19.** The probe ran attended on GS5R3 and worked. Its captures were then
> re-read offline against the art the engine actually draws, and **they answer the first half of
> the question**: on that one cell, with that one facing, the unit draw path's residual against the
> published rule is **zero, in both x and y, for two different sprites of the composite**.
>
> That reading is [recorded below](#what-the-2026-09-19-run-established-read-offline-afterwards).
> It is *Inferred*, not *Observed* — it rests on identifying which frame drew, and that
> identification is strong but not forced. Three things stay open, and this sheet is rebuilt around
> them:
>
> 1. **One facing, sampled twice.** Both placements reported `facing 4` and produced
>    pixel-identical captures. A residual that varies by facing is not ruled out.
> 2. **The mirror sign is undetermined.** The engine mirrors frames, and the frame that drew had
>    record-0 `x = 0`, so `+placement.x` and `-placement.x` predict the identical pixel.
> 3. **One unit type.**
>
> **The design flaw that cost the first reading, stated plainly.** The old sheet assumed the
> world-map unit draws `units\imp\licr2a.imp`. It does not — it draws `licr2b.imp`, and the
> difference is settled in the shipped scripts, not a matter of judgement (see
> [what the world map actually draws](#what-the-world-map-actually-draws)). Measured against the
> wrong file, the capture matched nothing and looked like a failed experiment. **A run sheet that
> assumes which art drew is a run sheet that cannot fail honestly.** This one identifies it.

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
number `hotspots.md` publishes is consistently wrong for a real recruited or summoned unit — which
is exactly the case a custom-unit modder hits.

## What the world map actually draws

**Observed in the corpus.** A unit type declares its art indirectly. `units\licr2.gs`:

```
/impfile_proc{"licr2"unittype_imp_filename}def
```

and `gs\imps.gs` resolves that name *per screen*:

```
/unit_zoom_letter{dup /dummy exch known{/dummy exch get}{pop"B"}ifelse}
	/dummy << COMBAT_SCREEN"A" LOCATION_SCREEN"A"
	          SCROLLINGMAP_SCREEN"B" REGION_SCREEN"B" WORLD_SCREEN"B"
	          FAITHSELECT_SCREEN"A" >> replace bind def

/unittype_imp_filename{imp_filename"units/imp/"strcpy imp_filename exch strcat
                       unit_zoom_letter imp_filename exch strcat
                       imp_filename".imp"strcat imp_filename}bind def
```

`unittype_imp_filename` consumes two operands — the base name `impfile_proc` pushed, and beneath it
a **screen mode the engine pushed**. `gs\imps.gs` documents that operand in the neighbouring
`citygate_imp_filename` comment as `<zoom_mode (from c++)>`. **Observed in a local binary:**
`lomse.exe` carries `getzoommode` (arity 0, reads-state) and `setspritezoommode` (arity 1).

So on the world, region and scrolling-map screens the drawn file is `units\imp\licr2b.imp`; in
combat and at a location it is `licr2a.imp`. Both exist in `imp.mpq`. **The B variant is the
subject of this experiment.**

**A world-map army is a composite, not one sprite.** `gs\PLAYER5.gs`'s `setupplayergraphics`
attaches, per player:

| Attachment | Call | Offset |
| --- | --- | --- |
| faith flag | `"iface/<xx>flagb.imp" 0 -40 setplayerflagimp` | `(0, -40)` |
| health bars | `setplayerbars` (32x5 and 24x4 doodads) | per unit type |
| group number | `setplayergroupnumberobject`, `0 -20 setplayergroupnumberoffset` | `(0, -20)` |
| faith indicator aura and halo colour | `setplayerindicatoraura`, `setplayerindicatorcolor` | — |

`setplayerflagimp`'s shipped call site pushes four operands (player, filename, dx, dy), which
matches its disassembled arity of 4 in `reports/natives/operator-bodies.tsv`. **Observed in the
corpus and in a local binary, agreeing.**

The practical consequence for the analysis is the whole reason the first reading went wrong: the
diff of a placed unit contains **at least two independent sprites**. Their union box is not any
frame of anything, and must never be matched against one.

## The hypothesis this refutes

The 2026-09-19 write-up proposed that a unit added to a location is drawn on the world map **as an
army banner** and that `licr2a.imp` is only its combat sprite. **Half of that is Refuted and half
is Observed.** The body *is* the unit's own art — just the B-zoom variant, not the A — so there is
no unknown "army sprite" behind it. But there *is* an army-level banner drawn with it, the player
flag above, and it is a real second sprite with its own art, its own 8-frame cycle and its own
offset. The ambiguity the hypothesis was reaching for is real; its location was not.

## What the 2026-09-19 run established, read offline afterwards

`zprobe.log`: army loc 185, cell (57,1), owner 0, target cell 58, both placements `facing 4`, both
cleanups done, both archives verified back to their recorded originals.

`tools/probe_captures.py` against the plate, with the union line this change added:

| capture | components | union |
| --- | --- | --- |
| `zu1` rung 0 orchard | 60x70 at (358,152) | 60x70 |
| `zu2` rung 1 `licr2a` via terrain | 30x111 at (374,136) **+** 8x9 at (396,249) | **30x122 at (374,136)** |
| `zu3` rung 2a unit | 47x44 at (365,193) **+** 12x21 at (377,165) **+** 3px at (366,237) | 47x74 |
| `zu5` rung 2b unit | pixel-identical to `zu3`, plus the ambient blob below | — |

**The control passed exactly.** 30x122 is frame 0 of `licr2a.imp`, so

```
anchor = (374,136) - (1,-25) + (30>>1, 122>>1) = (388, 222)
```

**The body.** `units\imp\licr2b.imp` frame 33 — STAND, facing record 8 — is 47x46 with record 0
`(0,-6)`:

```
predicted = (388,222) + (0,-6) - (23,23) = (365,193)      measured = (365,193)
```

Identification, stated as narrowly as it holds. Of all 86 frames of `licr2b.imp`, admitting both
orientations, exactly four predict `(365,193)`: frames 19 (50x47), 20 (45x46) and 23 (49x46)
mirrored, and frame 33 in either orientation. **Only frame 33 is 47 pixels wide, and 47 is the
measured width.** Two further facts point the same way and were not used to pick it:

- the 3-pixel component at `(366,237)` sits at sprite-relative columns 1-3, rows 44-45. Frame 33 is
  opaque there **only when mirrored** — unmirrored those rows are opaque at columns 43-45.
- the capture shows a unicorn facing left; frame 33 as stored faces right.

So the engine drew frame 33 **mirrored**, and the rule predicted its top-left to the pixel.

**The flag, an independent second test of the same anchor.** `iface\liflagb.imp` frame 111 —
UNIT_UNSELECTED, cycle offset 7 — is 12x21 with origin `(-5,-7)`, and the flag hangs off
`anchor + (0,-40)`:

```
predicted = (388,182) + (-5,-7) - (6,10) = (377,165)      measured = (377,165)
```

Frame 111 is the **unique** frame of all 112 in that file predicting `(377,165)`, and its stored
size matches the measured silhouette exactly rather than approximately.

**Two sprites, two files, two sub-paths of the army composite, one anchor recovered from the
terrain-sprite rung, zero residual in x and y for both.** *Inferred* — conditional on those two
frame identifications — but the two are independent of each other, and a coincidence would have to
hit both.

### The persistent 10x26 component at (358,259)

**Observed in gameplay.** It is absent from `zu0`–`zu3`, appears in `zu4`, and is *byte-identical*
across `zu4`, `zu5` and `zu6`. It lies 22 px left of and 66 px below the probe's cell, inside a
green humanoid figure that is already present in the plate; only the figure's lower body changes.

What it is **not**, with reasons rather than assertions:

- **Not anything the probe placed.** It is absent from the two captures in which the probe's own
  sprites were on screen, and it survives both id-exact gated cleanups into `zu6`.
- **Not the army's group-number badge.** That would land at
  `anchor + (0,-20) - (14,14) = (374,188)`, not `(358,259)`.
- **Not a health or morale bar.** Those are 32x5 and 24x4 doodads; a 10-wide, 26-tall shape is
  neither.
- **Not an ordinary animation cycle.** It advanced once and then held across three further
  `rendermap refreshdirty` passes and three more captures.

**Inferred:** a one-step state or animation advance of a pre-existing world-map figure. *Why* it
stepped once between `zu3` and `zu4` and then not again **cannot be established offline** and is
not established here. It is recorded so the next run can recognise it rather than re-derive it.

## The design

The method that has now worked three times: **one cell per observation**, subjects placed and
removed in turn, every capture against a single shared plate, so the camera position and the
cell-to-screen projection — both unknown constants — cancel between rungs instead of needing to be
solved for.

| Rung | What it places | Path | Purpose |
| ---: | --- | --- | --- |
| **0** | `terrainsprites /orchard get`, on cell 0 only | terrain sprite | The ladder run's own shipped sanity control. If this does not land where the published rule predicts, the capture or the cell logic is wrong here and nothing below is evidence. Skipped, with a log line, if an orchard already stands there. |
| **control**, once per cell | `units/imp/licr2a.imp` registered by filename | terrain sprite | **Recovers that cell's anchor.** Its expected result is known to the pixel in advance — 30x122, frame 0, record 0 `(1,-25)` — measured 2026-09-16 and reproduced exactly 2026-09-19. This is the one rung whose job is not to discover anything. |
| **subject**, once per cell | a real Elephant (`/pyele`), recruited with `add_unit_to_location` | **unit** | **The question.** Same cell, same plate, different draw path. Measured against `units\imp\pyeleb.imp` — derived from `unit_zoom_letter`, never registered, only ever read. |

Three cells, three controls, three subjects. **The control art and the subject art are different
files on purpose.** The control's only job is to recover the anchor through a path whose behaviour
is already measured; conflating it with the subject's art is what made the last run unreadable.

### Why three cells and not three placements on one

The 2026-09-19 run placed the unit twice on one cell precisely to get two independent facing draws.
Both reported `facing 4` and the two captures were pixel-identical: one facing sampled twice.

Nothing in this probe can force a facing. `setarmydata`'s three-operand shape is shipped
(`attacker ARMY_OWNER 0 setarmydata`) but no shipped call site sets `ARMY_FACING`, and constructing
that call would be exactly the reconstruction-from-resemblance
[`hotspots.md`'s "Do not" section](hotspots.md#do-not) forbids. Different **cells** are the only
lever this probe actually has, and they buy something a repeat on one cell cannot: **each
observation gets its own control rung and therefore its own independently recovered anchor.** A
residual that is really a property of one cell's projection then cannot masquerade as a property of
the unit draw path.

It still does not *force* the three facings to differ. Nothing offline can. If all three come back
identical, this run has three independent anchors and one facing, and says so.

### Why an Elephant

Chosen by measurement over all 141 `units\imp\*b.imp` members, 103 of which have a five-frame STAND
sequence — not by resemblance. `/pyele` (`units\pyele.gs`: Elephant, race LESSER_STONE_GIANT, faith
EARTH, flags `UNITTYPELAND CAN_ATTACK or`, loaded by `gs\unittype.gs`) is the **only** one that
satisfies both of the properties the last run showed are needed:

| Frame | Sequence, facing | Size | Record 0 |
| ---: | --- | --- | --- |
| 0 | MOVE, facing 0 | 40×74 | `(-1, -26)` |
| 30 | STAND, facing 0 | 40×72 | `(-4, -25)` |
| 31 | STAND, facing 1 | 56×71 | `(11, -26)` |
| 32 | STAND, facing 2 | 77×63 | `(18, -22)` |
| 33 | STAND, facing 3 | 65×61 | `(14, -10)` |
| 34 | STAND, facing 4 | 44×70 | `(4, -6)` |

1. **Every STAND frame is identifiable from the capture in both orientations.** The engine mirrors,
   so the check has to admit both. For `licr2b` frame 33 the predicted top-left was shared with
   three MOVE frames mirrored and only the silhouette *width* separated them. For `pyeleb` every
   STAND frame is separated from every other frame in the file, in both orientations, by position
   or by size. Asserted in `tests/test_engine_probe.py`, not merely claimed here.
2. **Record 0's x is far from zero on every STAND frame** — `-4, 11, 18, 14, 4`. `licr2b` frame
   33's is `0`, which is exactly why the last run could not tell `+placement.x` from
   `-placement.x` under mirroring: the two predict the identical pixel. Here they differ by 8 to 36
   pixels on every facing, so **whichever facing turns up settles the sign.**

Frame 0 is kept in the table as the MOVE fallback in case a freshly placed unit does not draw a
STAND pose. It is deliberately *not* held to property 2 — its record-0 x is `-1` — so an
observation landing on frame 0 is a weaker observation, not a broken one.

### The unit call, copied not reconstructed

`add_unit_to_location` (`gs\ENC_TOOL5.gs`) pops `/owner /loc /this_name /this_artlist /this_str
/this_type`, so it is called `TYPE STR ARTLIST NAME LOC OWNER add_unit_to_location`. This probe's
call is `gs\PLAYER5.gs:430` **verbatim**, with only the unit key, the location and the owner
swapped for its own:

```
shipped:  unittypedict begin /licr2 end 0{}0 start_loc 2 add_unit_to_location
probe:    unittypedict begin /pyele end 0{}0 zcellN zowner add_unit_to_location
```

`add_unit_to_location`'s own body ends on `... ischampion?{...}if`, a boolean branch — it pushes
nothing back. The created army is found afterward the way three shipped call sites do it, by its
location (`gs\andenc.gs`'s `x y armyat`):

```
zcxN zcyN armyat /zarmyN exch def
zarmyN ARMY_LOCATION getarmydata /zalocN exch def
```

`findemptylocation` guaranteed the cell was empty of units *when it ran*, not immediately before
this line — the control rung, with its own `rendermap refreshdirty` and `screencapture`, executes
in between. That the cell is still empty here is therefore an inference, not an observation, and
the probe checks it rather than assuming it: cleanup runs only when **both** the army id is valid
**and** its reported location matches this cell.

```
zarmyN -1 ne zalocN zcellN eq and
	{zarmyN deletearmynow}
	{log: "cellN cleanup REFUSED"}ifelse
```

Cleanup itself is `zarmyN deletearmynow` — the shipped one-operand cleanup every spell-summon in
the corpus uses (`gs\GAMEUTIL5.gs`'s `summon_cleanup`,
`gs\spells\AIR_raise_frozen_shade.gs`) — an exact match on the id `armyat` handed back, never an
unconditional call. An army has no sprite type, so the terrain-sprite sweep does not apply to it
and is not used.

The seed for each `findemptylocation` is an *offset* from the army's own location
(`(+2,0)`, `(-2,0)`, `(0,+2)`), never the army's own occupied cell — matching the other engine
probes. If `findemptylocation` can return its own seed when it judges that cell acceptable
(unverified without the engine), seeding from the occupied cell would risk handing a subject rung
the player's own starting army instead of the one it placed. A cell that comes back `-1` is
refused, with a log line, rather than used.

## Before the run

```sh
cd spikes/asset-viewer && cargo build --release && cd ../..
LOM_PROBE=unitanchor scripts/install-engine-probe.sh
```

No sprites are injected — `licr2a.imp` is shipped art referenced by its existing path, and
`pyeleb.imp` is never referenced by the probe at all, only by the offline analysis afterwards. Only
`gs\hotkey.gs` and `START.GS` are patched.

## The run

1. Launch **Lords of Magic GS5R3**.
2. Start (or resume) a **single-player game** and reach the **world map**, with your starting army
   visible on screen. Ordinary macOS-blocks-synthetic-input territory — this part has to be a
   person.
3. Leave **room around the army in all directions**: the probe works three cells, at `(+2,0)`,
   `(-2,0)` and `(0,+2)` from it. A cell that is off-screen produces a control rung with nothing in
   it, and that observation is void — so centre the army rather than working at the edge of the
   view.
4. Prefer a spot with **no hostile stack adjacent**. The probe's Elephants belong to
   `currentplayer`, so they should never provoke combat, but there is no reason to test that.
5. Press **`z`**, once.
6. Watch for an Elephant to flash into existence and vanish, **three times**. Expect up to eleven
   captures and the probe's own log lines in `zprobe.log`. A shipped orchard flashing on the first
   cell is rung 0 and is normal; `rung0 SKIPPED` in the log means one was already standing there.
7. **Read `zprobe.log` before quitting.** Any `cleanup REFUSED` line means that Elephant is still
   on the map; remove it by hand.
8. Quit the game. **Do not save.**

The probe guards itself with `zdone`, so a second `z` does nothing.

## After the run

```sh
scripts/restore-game-archives.sh
```

Then, from the collected run directory:

```sh
python3 tools/probe_captures.py zu0.bmp zu1.bmp zu2.bmp zu3.bmp zu4.bmp zu5.bmp \
                                zu6.bmp zu7.bmp zu8.bmp zu9.bmp zu10.bmp
```

Pass only the captures that are actually present; a skipped rung 0 or a refused cell means some
were never written. The captures map to the rungs like this:

| capture | rung |
| --- | --- |
| `zu0` | the plate — everything is differenced against this |
| `zu1` | rung 0, the shipped orchard on cell 0 |
| `zu2` / `zu5` / `zu8` | the **control** on cell 0 / 1 / 2 |
| `zu3` / `zu6` / `zu9` | the **subject** on cell 0 / 1 / 2 |
| `zu4` / `zu7` / `zu10` | after that cell's gated cleanup |

Read `zprobe.log` alongside: the army cell, the owner and its faith, the control type id, each
cell, and per cell the army id, its reported location, its facing, and whether cleanup ran.

### Solving it by hand, per cell

1. **The control** (`zu2`/`zu5`/`zu8` against the plate). Take the **union** box the tool now
   reports, not a single component — `licr2a.imp` frame 0 is one sprite that arrives as two
   components, and reading it component-wise is what made a control that passed exactly look like
   it had failed by 11 pixels. It should be `30x122` at some `(left, top)`. Solve
   `anchor = (left, top) - (1, -25) + (15, 61)`. **If it is not 30x122, stop: that cell's
   observation is void, not a result.**
2. **The subject** (`zu3`/`zu6`/`zu9` against the plate). It is a **composite**. Separate the
   components before matching anything:
   - the **body** is the largest component, and the one to match against the `pyeleb` frame table
     above;
   - the **flag** is a detached component exactly **21 pixels tall**, sitting above the body;
   - small components of two or three pixels are usually the body's own detached tail, below the
     reporting threshold. The tool prints the union including them; use it.
3. **Identify the body frame, do not assume it.** For each of the six candidate frames, and for
   each of the two orientations (`placement.x` as stored, and negated), compute
   `predicted = anchor + placement - (w>>1, h>>1)`. Exactly one (frame, orientation) pair should
   both predict the measured top-left *and* have the measured silhouette size. Record which.
   - **The orientation that fits is a result in its own right** — it is the mirror-sign question
     the last run could not answer.
   - If no pair fits, that is **not** a verdict on the anchor. Re-derive the table with
     `--describe-imp` and read the logged facing before concluding anything.
4. **The residual** is `measured - predicted` for that frame, in x and y, zero if they match. The
   published rule floors rather than rounds, so a one-pixel difference is within it.
5. **Cross-check with the flag.** Its anchor is `anchor + (0, -40)`; match its measured top-left and
   12-to-23-wide, 21-tall silhouette against the frames of
   `iface\<faith>flagb.imp`'s UNIT_SELECTED and UNIT_UNSELECTED sequences, using the owner's faith
   from the log. This is a second, independent test of the same anchor through a different
   attachment of the same composite, and it cost nothing to obtain.

## What each outcome would mean, stated before the run

- **Rung 0 does not land where the published rule and this session's own recovered anchor predict**
  → the capture or the cell logic is wrong in this run. Nothing else here is evidence. (If rung 0
  was skipped because an orchard already stood on cell 0, this check cannot run at all this
  session; re-run elsewhere if the control matters to you.)
- **A cell's control rung is not 30x122** → that cell's anchor was not recovered, so that cell's
  subject is uninterpretable. Drop the cell, keep the others; each cell stands alone.
- **All three subjects match their own cell's anchor, using their own identified frame** →
  residual zero on three independently anchored observations. The unit draw path computes its
  anchor **the same way** the terrain-sprite path does, and the caveat in `hotspots.md` can be
  closed as answered rather than narrowed. If the three also landed on more than one facing, the
  facing-dependence question closes with it; if they did not, say so — three anchors and one
  facing is what the run carries.
- **All three are off by the SAME `(dx, dy)`** → the unit draw path adds a unit-specific offset to
  the anchor, and three independent anchors agreeing is real support for it being constant rather
  than a property of one cell. That constant is the fix custom-unit modders need — and it should
  still be re-measured on a second unit *type* before being published as general.
- **The three disagree** → the residual is not a simple additive offset. Do not average them. Report
  all three with the cell, the facing and the identified frame each came from, and treat the
  question as open pending a design that can hold facing fixed.
- **The body's measured top-left fits the MIRRORED orientation** → the rule's `placement.x` is
  negated when the engine mirrors a frame, which `hotspots.md` does not currently state. That is
  publishable on its own, and it is the reason this subject was chosen.
- **No (frame, orientation) pair fits a subject's measurements** → the identification method failed
  for that observation, not the anchor. The other cells, if they identified cleanly, still stand.
- **`cleanup REFUSED` for a cell** → the army id or its reported location did not match, so cleanup
  deliberately did not run rather than deleting something it could not confirm was its own. That
  Elephant may still be on the map; check in-game and remove it by hand before quitting.
- **`cellN SKIPPED -- findemptylocation returned -1`** → no empty cell at that seed. Not a result;
  the other cells are unaffected.
- **No army is found at start** → the probe never ran its body past the anchor lookup. Not a
  result; reposition and re-run.

## What this probe writes, and what removes it

- **Writes, if every guard passes:** nothing persistent. `zt0` is the *shipped* orchard type shared
  by every orchard the map generator placed, so before rung 0 places anything it checks whether an
  orchard is **already** standing on cell 0 and, if so, skips the rung entirely rather than placing
  on top of it and later sweeping by type-and-cell — the same class of loss as this repository's
  recorded village-deletion incident. When rung 0 does run, its placement is removed by type **and**
  the shared cell. `zt1` is minted this keypress, so nothing else on the map can carry it and a
  type-only sweep is exact; it is swept once per cell. Each Elephant is deleted by its exact id,
  gated on that id being valid **and** its reported location matching that cell — never
  unconditionally, never by location or type sweep. `gs.mpq` and `START.GS` are modified and rolled
  back by `scripts/restore-game-archives.sh`, verified against `MANIFEST.sha256`. No file under the
  game's loose `map/` directory is touched.
- **What is not independently verifiable here:** whether `findemptylocation` can hand back the cell
  it was seeded from, whether `armyat` on a cell that is no longer empty can return an army other
  than the one this rung just placed, and what `deletearmynow` does with an id that fails the gate
  are all GameScript engine semantics with no offline way to check them. The guards are correct
  regardless of the answer to any of them — that is why they check rather than assume — but a
  reviewer cannot confirm the underlying engine behaviour without running the probe. Rung 0's
  orchard check has the matching limit: the test fixture is generated script text, not a map, so it
  proves the generator emits a presence check, not that a real orchard was there to detect.
- **If a guard refuses:** the log says so instead of silently proceeding. `rung0 SKIPPED` and
  `cellN SKIPPED` leave nothing behind. `cellN cleanup REFUSED` is the case that DOES: that
  Elephant is still on the map when you quit.
- **Removes it:** `scripts/restore-game-archives.sh` restores both archives and collects the
  `zu*.bmp` captures actually present (`capture_names_for("unitanchor")` in
  `tools/engine_probe.py` is the exact list) and `zprobe.log` into a fresh per-run directory. Each
  cell's post-cleanup capture should differ from the plate by nothing but ordinary map animation —
  if it does not, cleanup left something behind, and that is itself worth reporting.

## Cost and risk

One keypress. No archive member is added or replaced — `gs\hotkey.gs` and `START.GS` are patched in
place, both restored and verified byte-identical afterward, and `licr2a.imp` is only ever read.
Each of the three Elephants exists only for the duration of one capture before its own gated
cleanup runs; nothing is saved.
