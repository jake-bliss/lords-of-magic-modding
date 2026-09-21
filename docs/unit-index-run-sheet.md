# `unitindex` run sheet — does a unit type above 154 register and draw?

> # ✅ RAN AND PASSED 2026-09-21
>
> **A unit type above index 154 registers and draws.** The probe defined one at runtime at index
> **160**, placed it, and it drew a sprite indistinguishable from the shipped control's. Both
> cleanups gated and done, zero `REFUSED`, all five captures written.
>
> ```
> rung0 numunittypes  160  expected  155          <-- the baseline was wrong; see below
> control army 1104 at 58   expected 58    control cleanup done
> rung2 defined; numunittypes 160 -> 161  index 160
> rung2 lastunittype 160  expected 160            <-- exact
> subject army 1104 at 183  expected 183   subject cleanup done
> ```
>
> | capture | result |
> | --- | --- |
> | `zi1` control, cell 0 | 65x78 at (342,165) |
> | `zi2` after control cleanup | **0 components** |
> | `zi3` subject, cell 1 | **65x78 at (206,104)** -- same size as the control |
> | `zi4` after subject cleanup | one 10x27 map animation, nothing of ours |
>
> ## 🔴 The baseline was wrong, and that is the bigger finding
>
> `numunittypes` reported **160**, not the 155 this sheet predicted -- and the prediction was
> measured from the wrong archive. `gs\unittype.gs` was read out of the **Development profile's
> pristine `gs.mpq`, which is vanilla**; the probe ran on **GS5R3**. **Observed in the corpus,
> 2026-09-21:**
>
> | profile | `units/*.gs` runs | unique |
> | --- | ---: | ---: |
> | **GS5R3** | **159** | **159** |
> | 3.02 | 153 | 152 |
> | vanilla | 153 | 152 |
>
> **So GS5R3 already ships unit types at indices 155-159, and this sheet's own premise --
> "nothing above 154 has ever existed at runtime" -- was false when it was written.** The shipped
> game had already answered the question; nobody had looked, because the count was measured on one
> profile and the engine run happened on another.
>
> That does not weaken the result. The probe still put a type at index **160**, above anything any
> profile ships, and it drew. But the honest statement of what was learned is *"we confirmed
> directly what GS5R3 was already demonstrating"*, not *"we did something the game has never done"*.
>
> ## A by-product, filed where it belongs
>
> The control landed at **(342,165)** and the subject at **(206,104)** -- *exactly* the cell-0 and
> cell-1 positions the [2026-09-20 `unitanchor` run](unit-anchor-run-sheet.md#the-2026-09-20-run)
> measured, on the same map with the army in the same place. Same 65x78 silhouette, which is
> `pyeleb.imp` frame 33 mirrored. So this run **independently reproduces the mirror-sign result**
> through a different probe built for a different question.
>
> ## What it did NOT establish
>
> Unchanged from the list below: nothing about passing **199**, nothing about durability across
> save/load (the type exists in no archive), nothing about new art, and nothing about the three
> adjacent caps -- of which `maxauratypes` is 70 of 70 with **zero headroom** and will bite a real
> new unit long before the unit-type count does.

**Status: ran 2026-09-21, passed.** Written 2026-09-21.

This closes open question 1 of [new units](new-units.md#open-questions). Everything else in that
document is settled offline: the engine has **no** unit-type cap (the table is heap-allocated at a
size the script picks), nothing downstream narrows the index, and `pic.mpq` accepts a new member
name. What has never happened is a unit type existing at runtime **above index 154**.

## The cheap design, and why this run is smaller than it looks

**The obvious design is wrong and expensive.** "Add a 156th unit" sounds like it needs a new `.gs`
member, two new `.imp` members, their two `.H` companions, a portrait in `pic.mpq`, and an edit to
`gs\unittype.gs`. That is [the full new-unit build](new-units.md#the-build-concretely--one-new-unit-life--wmt),
it touches three archives, and it confounds at least five independent questions into one run.

**None of that is needed to answer this one.** **Observed in the corpus:** `gs\unittype.gs`
declares `200 maxunittypes` and uses **155** slots. So slots **155–199 are already free at
runtime**, in the shipped game, with no archive surgery at all. `maxunittypes` does not need
raising to reach index 155 — it needs raising only to pass 199, which is a **different and later
question**.

**And a unit type does not need its own art.** `impfile_proc` names an art file; nothing requires
that file to be new. A unit declared with `/impfile_proc{"pyele"unittype_imp_filename}` draws the
Elephant. So the subject can reuse shipped art entirely.

**Therefore this probe touches `gs\hotkey.gs` only** — the same one-member patch every other probe
in this repository uses — and needs no `imp.mpq`, no `pic.mpq`, and no `gs\unittype.gs` edit.

⚠️ **The one premise this rests on.** `begin_unit_definition` and `end_unit_definition` are
**script definitions** (`reports/gs/vocabulary-vanilla.tsv`: `script-definition`, 163 uses),
declared in `gs.mpq` member `units\easyunit.gs`, which boot runs. They should therefore still be in
scope at hotkey time. **That they work after boot is Inferred, not observed** — it is exactly what
rung 2 tests, and rung 2 failing means *that*, not that index 155 is bad. The ladder is built so the
two cannot be confused.

## The ladder

Every rung states its expected value **before** the run, because a prediction written afterwards is
not a prediction.

| Rung | What it does | Expected result |
| ---: | --- | --- |
| **0** | Capture the plate. Log `numunittypes`. | **155.** If it is not, every later number in this sheet is measured against the wrong baseline — stop and re-derive. |
| **1** | **Control.** `add_unit_to_location` a shipped `/pyele` at cell A, capture, gated cleanup. | An Elephant appears and vanishes. This is the 2026-09-20 `unitanchor` result reproduced, and it proves the placement mechanism works *this session, on this map*. If it fails, nothing below is evidence. |
| **2** | Define a new unit type in the probe body: `begin_unit_definition … end_unit_definition`, with `/impfile_proc{"pyele"unittype_imp_filename}` so it reuses shipped art. Log the handle and `numunittypes`. | Handle resolves; `numunittypes` becomes **156**. The new type occupies **index 155**. |
| **3** | **Subject.** `add_unit_to_location` the *new* type at cell B, capture, gated cleanup. | An Elephant appears at cell B, indistinguishable from rung 1's. **That is the answer: index 155 registers and draws.** |

Cells A and B come from `findemptylocation` seeded at offsets from the player's army, exactly as
`unitanchor` does, and each cleanup is gated on **both** a valid army id **and** its reported
location matching that cell — never an unconditional delete, never a sweep.

## What each outcome means, stated before the run

- **Rung 0 logs something other than 155** → the baseline is wrong, not the engine. Possibly a
  different boot script is live (see the [boot-path caveat](new-units.md#nothing-downstream-caps-the-unit-type-count)).
  Re-derive before running anything else.
- **Rung 1 fails** → the placement mechanism or the map position is wrong in this session. **Not a
  result about unit indices.** Reposition and re-run.
- **Rung 2 fails to define** → `begin_unit_definition` does not work after boot. That is a finding
  about *when unit types can be declared*, and it is worth recording, but it says **nothing** about
  index 155. The next design would move the declaration into a `gs.mpq` member and pay the larger
  build.
- **Rung 2 defines but `numunittypes` stays 155** → the definition silently failed to append. Read
  the log for the handle; `unittype` returns `-1` from the append helper when `used == capacity`,
  and capacity should be 200 here, so this outcome would itself be surprising and worth chasing.
- **Rung 3 draws** → ✅ **the question is answered.** A unit type above 154 registers and draws.
  Combined with the offline work, the only remaining unknown about the count is what happens past
  **199**.
- **Rung 3 places but draws nothing** → the type registered and the army exists (the log will say
  so) but the art did not resolve. Suspect `impfile_proc` or the `.H` remap
  ([imp format](imp-format.md#-the-remap-is-parsed-from-the-h-companion-member-at-load-time)),
  **not** the index.
- **`cleanup REFUSED`** → that unit is still on the map. Remove it by hand before quitting.

## What this run does NOT establish

Stated here because a clean pass is exactly when a limit gets rounded away:

- **Nothing about passing 199.** That needs `maxunittypes` raised *and* `/unittypedict 200 dict`
  raised with it, which is a size-changing `gs.mpq` edit — a class the engine has accepted
  (2026-09-19) but not in this combination.
- **Nothing about a unit type surviving a save/load.** The type is declared at hotkey time and does
  not exist in any archive, so it would not exist on reload. Testing durability needs the real
  `gs.mpq` build.
- **Nothing about new art.** The subject deliberately reuses `pyele`'s. Adding art is a separate
  question with its own separate unknowns (`imp.mpq` added members are proven; `pic.mpq` added
  members are proven at the repack layer but **not** in front of the engine).
- **Nothing about recruitment, AI production, or combat.** Placement only.
- **Nothing about the three adjacent caps** — `maxauratypes` is **70 of 70 with zero headroom**,
  which will bite a real new unit long before the unit-type count does.

## What it writes, and what removes it

**Writes, if every guard passes:** nothing persistent on disk. `gs\hotkey.gs` and `START.GS` are
patched in place and restored byte-identically afterwards. No archive gains a member. Each placed
army exists for one capture before its own gated cleanup.

🔴 **One thing cannot be cleaned up: the unit TYPE.** There is no `deleteunittype`, and the append
helper only ever increments `used`. The type persists for the rest of the session and vanishes when
the game exits. **This is why the sheet says DO NOT SAVE** — the type exists in no archive, so a
save referencing it would carry an index nothing on disk defines.

**Removes it:** `scripts/restore-game-archives.sh`, which also collects `zi0`–`zi4.bmp` and
`zprobe.log` into a fresh per-run directory. `capture_names_for("unitindex")` is the exact list, so
nothing is collected or cleared by glob.

## How the dictionary scoping works, because it is what would silently go wrong

**Observed in the corpus.** `gs\unittype.gs` runs every `units/*.gs` inside
`soundfxdict begin unittypedict begin ... end end`. So a unit's `/pyele exch def` lands in
**unittypedict** — and so does all of `units\easyunit.gs`'s machinery. `begin_unit_definition`,
`end_unit_definition`, `unitdict` and `unitdictxref` are reachable **only** from inside
`unittypedict begin ... end`, and `add_unit_to_location`'s body does `unittypedict this_type get`,
so a symbol defined anywhere else is invisible to it. The probe wraps its definition accordingly,
and a test asserts it.

The dict stack balances, and that was checked rather than assumed:

```
/begin_unit_definition{missiledict begin unitdict begin default_unit}
/end_unit_definition{... end end pop dup userdict begin /lastunittype exch def end}
```

`begin` opens two, `end end` closes two, and the tail leaves exactly **one** handle on the stack —
which is why every shipped unit reads `end_unit_definition /<name> exch def`. It also parks that
handle in `userdict /lastunittype`, which the probe logs: the handle **is** the table index
(`end_unit_definition` feeds it straight to `setunittypedata`, whose guard at `0x00524959` bounds it
against the live `used` count), so that line should read back exactly the pre-definition count. It
is the sharpest single line in the log.

## Verification before the run

```sh
python3 -m unittest tests.test_engine_probe
```

Eight `UnitIndexProbeTest` cases, plus the shared safety suite that runs against every probe. All
eight were mutation-verified: removing the count gate, pointing the control at the subject's own
symbol, and moving the definition outside `unittypedict` each make the suite fail.

## Before, during, after

Identical to every other probe on the [attended run index](attended-run-index.md):

```sh
cd spikes/asset-viewer && cargo build --release && cd ../..
scripts/restore-dev.sh                       # confirm PRISTINE on all five
LOM_PROBE=unitindex scripts/install-engine-probe.sh
```

Launch **Lords of Magic GS5R3**, start a single-player game, reach the world map with the starting
army **centred** and room around it, tap **`z`** once, then read `zprobe.log` **before quitting**.
Do not save.

```sh
scripts/restore-game-archives.sh             # restores and collects the captures
```

🔴 In any profile with `cheat_keys` enabled, **never press `Y`** (`destroyterrainsprite`, no prompt
— it deleted a village on 2026-09-16) and **never press `S`** (`superduper`, defined nowhere).
