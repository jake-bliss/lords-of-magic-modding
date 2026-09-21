# Attended run index

## Status, 2026-09-20

| Item | State |
|---|---|
| **A1** command line of the unmeasured profiles | ✅ **DONE** 09-19 — GS5R3 runs `d:\lomse.exe /*`; premise holds |
| **A2** guard refuses per profile | ✅ **DONE** 09-19 — refuses exit 1 running, permits exit 0 closed |
| **B1** unit anchor probe | ✅ **DONE 09-20** — three cells, three anchors, residual **zero**; and the mirror sign fell: `placement.x` is negated |
| **D1** a unit type above index 154 | ✅ **DONE 09-21** — defined at runtime at index **160** and drew. Also found GS5R3 already ships 160 types, so 155-159 already existed |
| **D2** the two declared caps | ✅ **DONE 2026-09-21**, with its claims scoped. `maxauratypes` raised 70->100 and two auras registered at **70/71** (*Observed in gameplay*). A unit type at **199** registered and an army was placed at the subject cell, `armyat` confirming the location — that the army *is* the type at 199 is **Derived** from that plus `lastunittype`, not read back. One definition past capacity returned **-1**, left the count at 200 and did not crash the session. ⚠️ `maxunittypes` itself was **never raised**. Took three attempts, both failures mine ([run sheet](unit-cap-run-sheet.md)). 🔴 **Offline follow-up, same day:** the static audit says a raise is safe, needs `/unittypedict N dict` moved in lockstep, and hits a **silent ceiling at 1000** imposed by a fixed 1000-dword stack histogram at `0x0052BE20` — see [the ceiling audit](new-units.md#-the-real-ceiling-is-1000-unit-types--and-it-is-silent) |
| **B2** direction 0 to a bearing | 🟡 **HALF CLOSED — read carefully, there are two direction spaces.** The **stored map direction** (`+0x44`) is fully closed offline: 09-20 gave the ring, 09-21 gave its screen bearing from the 09-17 capture (`s` is down-and-left, `se` straight down — [table](map-format.md#the-screen-bearing-of-each-direction)). The **IMP facing index** is what the sheet below still runs, and it is untouched by any of that |
| **C1** PBM ByteRun1 encoder | ✅ **DONE 09-20** — ladder rungs 3+4; member shrank, `TINY` dropped, stripe rendered clean |
| **C2** a size-changing edit | ✅ **DONE 09-19** — cheat-keys ladder, 21→20 bytes |
| **C3** a member added, not replaced | ✅ **DONE 09-20** — ladder rung 5; `imp.mpq` grew to 3,601 members |
| **C4** two members in one build | ✅ **DONE 09-19** — acceptance rungs 6–9 |
| **C5** first `imp.mpq` run | ✅ **DONE 09-20** — ladder rungs 0+1, 2; repack accepted and our IMP pixels rendered |
| **C6** load a savegame we wrote | ✅ **DONE 09-20** — control loaded and played; an authored `LS_PLR_` name reached the screen; found a lord's name is stored **three times** and two screens read different copies |

**Group C is closed.** Every engine-acceptance gap on this sheet has been run. The ladder that
`docs/engine-acceptance-ladder.md` describes is complete — all ten rungs, across four sittings.

**What that means for modding.** The two things standing between this project and shipping **new
art** were adding a member and touching `imp.mpq` at all. Both fell on 2026-09-20. Sprites, images,
audio, GameScript and savegames all now have a proven path from this pipeline into the running
engine. `docs/roadmap.md`'s per-archive regions carry the exact limits that remain, derived from the
runs rather than typed beside them.

**What is left on this sheet:** B2's **IMP facing** half, and nothing else. Its *stored map
direction* half closed offline on 2026-09-21, from a capture taken on 2026-09-17 for a different
purpose.

🔴 **Do not let the two collapse into one.** They are different direction spaces with a rotation
between them: `stored = (arg + [0x5AEC3C]) mod 8` at `0x0049DCC0`, and `0x5AEC3C`'s runtime value is
unknown offline. Everything 09-20 and 09-21 established is about the **stored map direction**.
Knowing where `s` is drawn says nothing about which IMP frame the engine picks for a facing — the
one data point on that is B1's, below, and it already refutes the obvious reading.

⚠️ **The pattern worth carrying, though, is real.** Both the map-direction bearing and the
`maxauratypes` verdict were reachable from evidence this project already held — the cost was not
instrument time, but that the evidence sat under the heading of the question it was gathered for.

**B1's two surviving limits are not the ones it was built for.** Its residual and its mirror sign
both came back clean. What did *not* close is that only **one unit type** has ever been measured,
and that all four placements across both runs reported the same facing — nothing in this probe can
force a facing, and different cells did not buy different ones.

### Limits that survived the sitting

Stated here because a clean sweep is exactly when a limit gets rounded away:

- The engine **tolerates** an added member. Whether it can **read** one is untested and no on-screen
  observation could settle it — nothing asks for `iface\ladder.imp`.
- `imp.mpq` has never been shown a **size-changing** member edit. All three of its rungs were
  length-preserving by construction.
- A `TINY` thumbnail **regenerated** rather than dropped.


Every measurement that needs a human at the keyboard, ordered so one sitting closes as many as
possible. Each entry states **what it would mean** before the run, because a prediction written
afterwards is not a prediction.

Written 2026-09-19 while the operator was away. Nothing here has been run.

## How to use this

Runs are grouped by what they need. Group A costs about two minutes and needs no build at all —
do it first, because one of its answers gates a safety guard that every other run depends on.
Group B is the existing ladders, which have their own sheets. Group C needs mods built first.

**Before any run:** `scripts/restore-dev.sh`, and confirm it reports PRISTINE for all five
archives. **After any run:** the same, and record the digests.

🔴 In any profile with `cheat_keys` enabled, **never press `Y`** (`destroyterrainsprite`, no
prompt — it deleted a village on 2026-09-16) and **never press `S`** (`superduper`, defined
nowhere in the corpus).

---

## Group A — costs two minutes, needs no build

### A1. Measure the command line of the two unmeasured profiles ✅ RUN 2026-09-19

> **GS5R3 measured and the guard verified in both directions.** The command line is
> **`d:\lomse.exe /*`** (PID 6328) — a bare DOS path, the same form as 3.02. The anchor's premise
> holds on the profile `scripts/restore-game-archives.sh` actually targets. **Observed in gameplay,
> 2026-09-19.**
>
> `wineserver` and `winedevice.exe` run as separate processes; the game itself presents the DOS
> path as argv[0], so the wrapper-prefix class the guard deliberately declined to cover does not
> arise on this profile.
>
> Both arms of the guard were exercised, not just the one that was worrying:
>
> | condition | `refuse_if_game_running` |
> |---|---|
> | game at the main menu | `lomse.exe is running; quit the game first.` — exit 1 |
> | game closed | exit 0 |
>
> **Vanilla remains unmeasured, deliberately.** It is the preserved baseline, the engine writes
> `lom.cfg`/`settings.cfg`, and nothing in the pipeline restores *to* vanilla — it is the source the
> other profiles are cloned from. Three of four profiles are now measured.
>
> **Side measurement, Observed in gameplay 2026-09-19:** a menu-only launch does **not** write
> `lom.cfg`. GS5R3's digest was `c9fb19d9d16d8849d29d5f405009746d` before the launch and identical
> after. The `development` config drift recorded in `docs/loose-files.md` therefore did not come
> from merely starting the game.

### A1 (as written before the run)

**Why this is first.** `refuse_if_game_running` is the guard that refuses to write archives while
the game is live. Its whole premise is that a Wine process presents `argv[0]` as a bare DOS path.
That premise is **Observed in gameplay** for exactly two profiles:

| Profile | Observed command line | Date |
|---|---|---|
| Development | `c:\program files (x86)\steam\steamapps\common\lords of magic special edition\english\lomse.exe /* MVK_CONFIG_FULL_IMAGE_VIEW_SWIZZLE=1` | 2026-09-19, PID 77245 |
| 3.02 | `d:\lomse.exe /*` | 2026-09-19, PID 47723 |

**Vanilla and GS5R3 have never been observed.** `scripts/restore-game-archives.sh` targets
**GS5R3**. This guard has already been wrong four times, and defect #3 was caused by exactly this:
a pattern measured on one real profile and then applied to a profile it had never been measured
against. We are in that position again, on the profile whose restore script now calls the guard.

**Steps.** For each of vanilla (`Steambuild 32 64bit DXVK.app`) and GS5R3:

1. Launch the profile. Get to the main menu. Do not start a game.
2. In a terminal: `ps -Ao pid,command= | grep -i lomse`
3. Record the **full** line verbatim, including the leading executable and every argument.
4. Quit the game.

**What each outcome means.**

| Reading | Meaning |
|---|---|
| Starts with a drive letter, e.g. `c:\...\lomse.exe` or `z:\...\lomse.exe` | The premise holds on all four profiles. The guard's anchor is established rather than assumed, and this finding closes. |
| Starts with a unix path — `/usr/bin/wine64-preloader ...`, `.../wineskinlauncher ...`, `.../wine ...` | **The guard is inert for this profile.** The `^` anchor cannot match. This is a live fail-open on the profile the restore script targets, and it must be fixed before any Group C run. |
| Uses forward slashes, or is quoted (`"c:\...\lomse.exe"`) | The widened pattern handles it, but record the exact form so the fixture is measured rather than constructed. |
| No process listed while the game is visibly running | The instrument is wrong, not the guard. Say so and stop — do not conclude the guard is fine. |

**Cheap and worth doing in the same breath:** with the game running, check the guard actually
fires. `bash -c 'source scripts/lib-mod-pipeline.sh; refuse_if_game_running'` should **refuse**.
If it returns quietly while the game is up, that is the fail-open, observed directly.

### A2. Confirm the guard refuses under each profile ✅ RUN 2026-09-19

Same two minutes, needs A1's answer. With each profile running, run the guard and confirm a loud
refusal. A silent pass is the failure, and it is the one that costs an archive.

---

## Group B — existing ladders, sheets already written

### B1. Unit anchor probe — `docs/unit-anchor-run-sheet.md` ✅ ANSWERED 2026-09-20

> **The rebuilt three-cell probe ran, and both questions it was rebuilt for came back clean.**
> All three cells ran, all three cleanups gated and done, zero `REFUSED`, all 11 captures written.
>
> **The residual is zero in x and y on three independently recovered anchors** — so the unit draw
> path computes its anchor the same way the terrain-sprite path does, and no additive
> unit-specific offset survives three anchors agreeing. The caveat `hotspots.md` states about its
> own central result is closed as answered rather than narrowed.
>
> ⭐ **The mirror sign fell.** The body is `units\imp\pyeleb.imp` frame 33 **mirrored** — the
> unique fit among all 86 frames in both orientations, on each of the three anchors. Its record-0
> `x` is `+14`: as stored the rule predicts a left edge of 370, mirrored it predicts 342, and 342
> was measured. `hotspots.md` previously said mirroring's effect was undetermined and now states
> both orientations.
>
> 🔴 **But the repository already knew the rule, more precisely.** `imp_anim::mirrored_anchor_x`
> has carried it since it was read out of `ImpPlayer::GetPlacement` — **including one more pixel
> subtracted on even widths**, which this run cannot see because its frame is 65 wide. Derived
> knowledge that lives in one module is not published. The even-width `dec` has still never faced
> the engine.
>
> **What did not close:** one unit type, and the facing — all four placements across both runs
> reported `facing 4`. Different *cells* did not buy different facings, which is the one thing this
> design hoped for and could not guarantee.
>
> Captures in `artifacts/engine-probe-captures/run-20260920-210403`.

The brief this box was written against:


**Ran 2026-09-19. The probe worked and half the question is answered; it has been rebuilt for the
other half.** Read offline against the art the engine actually draws, the residual is **zero in x
and y**, for the unit body *and* independently for the player flag, both solved from the anchor the
terrain-sprite control rung recovered on the same cell. *Inferred*, conditional on two frame
identifications.

**Why a second run:** that run carried **one facing sampled twice**, one unit type, and a frame
whose record-0 `x` was `0` — so **mirroring's effect on the placement x sign is undetermined**. The
probe now works three cells, each with its own control rung and therefore its own independently
recovered anchor, and its subject was chosen by sweeping all 141 `units\imp\*b.imp` members for the
two properties that failure showed are needed.

⚠️ **The cost of the first run was a premise, not a bug.** The old sheet named the subject's art
instead of deriving it, and named the wrong file — `unit_zoom_letter` in `gs\imps.gs` sends the
world map to `...b.imp`, not `...a.imp`. Read the sheet's expected values *and* how it derives what
it is measuring.

### B2. Anchor direction 0 to a compass bearing

> ## 🟡 The offline chase paid off, 2026-09-20
>
> **This sheet said "prefer the offline chase first — it is free, and it may make this run
> unnecessary." It did, for most of the question.**
>
> **Direction 0 is the `.til` column `s`**, and the whole ring is
> `0=s, 1=sw, 2=w, 3=nw, 4=n, 5=ne, 6=e, 7=se`. The engine carries a **static** 8-entry direction
> table in `.data` at `0x005557E8`/`0x00555808` (duplicated at `0x555828`/`0x555848`), indexed by
> `direction * 4`, and composing it with this project's own `n = (0,-1)` derivation gives the ring
> directly. Both halves are in the same operand frame, so it does **not** inherit the x/y ambiguity
> `map-format.md` warns about. Full derivation, addresses and limits:
> [map-format.md — the eight-direction table](map-format.md#the-eight-direction-table-and-what-direction-0-means).
>
> ## ✅ The *stored map direction's* screen orientation closed offline, 2026-09-21
>
> 🔴 **This does NOT cancel the run below, which measures the IMP facing index.** What closed — "does the column named `s` point toward
> the bottom of the screen" — was already measured, on 2026-09-17, by the flatground projection
> run. That capture varied the two cell operands **independently** and read the drawn top off the
> screen each time, and both operands move a cell **down** by the same 14.4 pixels. Direction 0 is
> `(0, +1)`, so it is drawn **down and to the left** — 45 degrees off straight down, not straight
> down; `se` is the one drawn straight down the screen, and `nw` straight up. The vertical half is immune to the x/y labelling ambiguity because the projection's
> vertical output is symmetric in the two operands.
>
> Table, strength of each half, and the one link the left/right answer rides on:
> [map-format.md — the screen bearing of each direction](map-format.md#the-screen-bearing-of-each-direction).
> Executable form: `map_projection.direction_screen_step`.
>
> It remains true that `lomse.exe` contains **no compass vocabulary at all**. The bearing comes
> from the projection, not from the binary's words.
>
> ⚠️ **What this does NOT buy the run below.** A first draft of this note claimed the result gives
> the run a readout — compare each facing's screenshot against a predicted screen position. **That
> is wrong, and a reviewer caught it.** `direction_screen_step` returns the pixels a drawn cell
> moves when you step one cell in a direction. A unit that changes *facing* stands on the same
> cell, so all eight facings put it at the identical screen position and the comparison would
> separate nothing. Acting on it would have burned a sitting.
>
> What the result does buy, if the run wants it, is a **movement** readout: step the unit one cell
> in a stored direction and the screen displacement identifies which of the eight the engine took.
> That is a different measurement from the facing question this sheet asks.
>
> **The `0x5AE970` builder is out of reach until the disassembly phase**, and now for a stated
> reason rather than as a guess: the dword occurs 36 times in `.text` and **every one is a read
> encoding**. It is field `+0x18` of a singleton based at `0x5AE958`, written through a register,
> which no byte-level technique can find.
>
> ⚠️ **One trap recorded.** `reports/natives/global-clusters.tsv` marks the cluster `read_only=no`.
> That means "not in a read-only PE section", **not** "something writes it". Reading it as writer
> information would have sent the next chase in the wrong direction.
>
> ### A by-product from B1's run, filed here
>
> B1's run logged `ARMY_FACING 4` for all three placements, and the art the engine drew was
> `pyeleb.imp` frame 33 — STAND, **stored facing index 3**, **mirrored**. So direction 4 does not
> select stored facing 4 unmirrored, which is what the five-stored-facings layout most obviously
> suggests. **Observed in gameplay, 2026-09-20**, conditional on the frame identification in
> [the run sheet](unit-anchor-run-sheet.md#the-2026-09-20-run).
>
> This is the *IMP facing index*, which the static table above does **not** cover — that table is
> the stored map-direction field `+0x44`. The script-to-stored path at `0x0049DCC0` is
> `stored = (arg + [0x5AEC3C]) mod 8`, and `0x5AEC3C` is BSS with seven references, all reads, so
> its runtime value is unknown offline. One data point constrains this mapping; it does not
> determine it.
>
> It is recorded here, and not only in B1's run sheet, because a by-product filed under its own run
> is how this project three times went on publishing a question it had already answered.


The last open piece of the `AnimRules` box (issue #2). The other two jobs close offline.

**The cheap route was checked and refuted.** `setimpplayerfacing` (`0x0049EB60`) and
`setimpplayerdirection` (`0x0049E9A0`) have **zero call sites** across all 1,691 `gs.mpq` members
— confirmed by the tokenizer and by an independent raw byte grep. IMP facing is set in native
C++, not script. `locationindirection` (`0x004AC520`) holds **no static offset table**; it indexes
a runtime, map-dependent table through a pointer at `0x5AE970`.

So this needs either an engine run or an offline chase of that table builder. **Prefer the offline
chase first** — it is free, and it may make this run unnecessary.

If run attended: place a unit at a known map cell, face it at each of directions 0–7, and
screenshot each. Note `map2screen`'s third value is screen x, and `screencapture` writes R,G,B
not BGR.

⚠️ `docs/map-format.md:76-79, 598-601` records that which map operand is x versus y, and how the
isometric axes relate to compass directions, is itself only **Inferred**. A bearing derived
through that mapping inherits the uncertainty. State the raw screen observation separately from
any compass claim.

---

## Group C — needs a mod built first

These were the engine-acceptance gaps. **All of them are now closed** — C2 and C4 on 2026-09-19,
C1, C3 and C5 on 2026-09-20. Both storage classes, size-changing edits, multi-member builds and an
added member have all been shown to the engine. Each box below keeps the brief it was written
against, with its outcome quoted above it, so the prediction can be read against the result.

### C1. The PBM ByteRun1 encoder, in front of the engine ✅ CLOSED 2026-09-20

> **Ran as ladder rungs 3 and 4, and passed.** Rung 3 re-encoded `lbm\newgame.lbm` with the shipped
> pixels: 302,432 -> 302,714 bytes, every ByteRun1 packet repacked, and the menu was reported
> unchanged. Rung 4 filled a rectangle: 302,432 -> **256,996** bytes -- the member *shrank* -- and
> the observer reported a clean straight-edged red stripe with the surrounding art intact and all
> four screen edges untouched. **Observed in gameplay, 2026-09-20.**
>
> The `TINY` drop was the real question and it is answered: rung 4's member carries no thumbnail and
> nothing broke. That was corpus inference (128 of 1,045 shipped images carry no `TINY`) until now.
>
> Straight edges are the part `pbm_patch.py` could not have produced -- it repaints whole runs and
> leaves a ragged edge -- so this is the encoder, not the old mechanism wearing a new name.

The brief this box was written against:

**The single highest-value run available.** `pic.mpq` ran on 2026-09-18 — but with
`tools/pbm_patch.py`, which rewrites only the *data byte* of an existing repeat packet. A two-byte
in-place repaint. It never re-compresses and never changes length, because no encoder existed then.

**The encoder exists now and has never faced the engine.** It changes length, and it **drops the
`TINY` thumbnail** on a pixel change, because regenerating one needs a downscaler we do not have.
That drop is justified from the corpus — 128 of 1,045 shipped images carry no `TINY` — but that
justification is **corpus inference, not engine evidence**. This run is what converts it.

**Expected value, stated in advance:** the image renders correctly and the rest of the screen is
untouched. Build the edit so it carries its own control — change a region with a hard boundary, so
"changed top, correct bottom" is a different observation from "the image is broken".

| Reading | Meaning |
|---|---|
| Edited region correct, rest of screen correct | The encoder is accepted. Closes the image half of Phase 5 for real. |
| Image renders but `TINY`-related breakage appears elsewhere | `TINY` is load-bearing somewhere we have not identified. A finding, not a failure. |
| Archive rejected / game will not start | Either the length change or the encoder. C2 separates them — run it next. |

### C2. A size-changing edit ✅ CLOSED 2026-09-19

> **Closed by the cheat-keys ladder, not by a run designed to test it.** `/cheat_keys false def`
> -> `/cheat_keys true def` in `gs\hotkey.gs` is **21 bytes to 20**. That archive shipped to the
> Development profile, the engine loaded it, and the debug hotkey tier was exercised in gameplay
> (`*` took a level-1 Paladin Lord to LVL 9). **Observed in gameplay, 2026-09-19.**
>
> The ladder was about the flag, so the length change went unremarked for a day while the roadmap
> kept publishing "the engine has not been shown an edit that changes a member's size".

The claim this box was written against:
**No archive, in any format, had ever been accepted with a member whose length changed.** This is
the limit every engine-acceptance marker repeats, and it caps every format delivered via MPQ. If
C1 fails, this isolates whether length or the encoder was responsible — so build both before the
sitting.

### C3. A member added rather than replaced ✅ CLOSED 2026-09-20

> **Ran as ladder rung 5, and passed.** `iface\ladder.imp` -- a member no shipped archive holds --
> was added to `imp.mpq` alongside rung 2's repainted cursor, taking the archive to 3,601 members.
> The repainted pointer still drew, so the hash table, the block table and all 3,600 originals
> survived the archive growing. **Observed in gameplay, 2026-09-20.**
>
> ⚠️ **This establishes tolerance, not readability.** Nothing in the game asks for
> `iface\ladder.imp` and no on-screen observation could settle whether the engine can read an added
> member. What is established offline: `lom-mpq probe-names` resolves the name through the archive's
> own hash table -- the lookup Storm performs -- and the member reads back as the bytes it was added
> from.

The brief this box was written against: never attempted. `allow_new_members` exists and is
deliberately gated behind two separate acts.

### C4. Two members in one build ✅ CLOSED 2026-09-19

> **Closed by acceptance rungs 6-9.** Each rewrote **two** members -- `wav\welcome.wav` and
> `wav\button.wav` -- in **both** `sndfx.mpq` and `special.mpq`, four changed members in one
> build, and the engine played them. **Observed in gameplay, 2026-09-19.**

The claim this box was written against:
Never attempted. Nothing establishes that a *large* mod loads — every acceptance so far is one
member. Note rungs 6–9 did rewrite two members across two archives, so this is partly addressed;
confirm against the ladder outcome before spending a sitting on it.

### C5. First `imp.mpq` run ✅ CLOSED 2026-09-20

> **Ran as ladder rungs 0+1 and 2, and passed.** Rung 0+1 repacked `imp.mpq` under the recovered
> names with the cursor member re-emitted byte-identically by our IMP encoder; the pointer was
> reported unchanged. Rung 2 repainted it white by eight disjoint palette-index swaps at unchanged
> payload length, and the observer reported the change. **Observed in gameplay, 2026-09-20.**
>
> The sprite pipeline is no longer offline-only. Every IMP this project had ever written landed in a
> loose `.imp`, and loose files were measured on 2026-09-16 not to override MPQ members -- so until
> this run, no IMP we wrote had ever been read by the engine.
>
> **The pointer-enumeration worry, addressed rather than dismissed.** The stated risk was that the
> IMP writer rewrites every absolute pointer *the repo has identified* and that the enumeration
> being complete was not measured -- header bytes 12-25 are read by nothing here. Rungs 0+1 and 2
> both passed, which is evidence against a hidden pointer in the path these frames exercise. It is
> not proof of completeness: both rungs were length-preserving by construction, so a pointer that
> only matters when offsets move was never put under load. **A size-changing `imp.mpq` edit remains
> untested and is the honest next rung.**

The brief this box was written against:

**`imp.mpq` has never been repacked or put in front of the engine at all.** It is the largest
wholly-untouched archive, and the entire sprite pipeline is offline-only as a result.

⚠️ Known limit to state plainly: the IMP writer rewrites every absolute pointer *the repo has
identified*, and that the enumeration is **complete is not measured**. Header bytes 12–25 are read
by nothing in this repo. A pointer hiding there would be adjusted by nobody, and the offline
round-trip sweep would not catch it — because it only catches a stale pointer that one of *our*
readers follows, not one only the engine follows. That is precisely what this run tests.

### C6. Load a savegame this project wrote ✅ CLOSED 2026-09-20

> **Three rungs, all passed.** Rung 0 -- a re-encode asserted byte-identical to `quickstart` --
> **loaded and played**; the lord's army moved on the map. Rung 1 rewrote eight `LS_PLR_` names
> through the decoded model (54 bytes, length unmoved) and the **Party Roster** header showed
> `ZEBRA`. **Observed in gameplay, 2026-09-20.**
>
> ⭐ **The unplanned result is the better one.** Rung 1's rename moved the roster header and left
> the **overworld panel still reading `LYLENDNAR`** -- so a lord's name is stored **three times**,
> and two screens read different copies. Rung 2 gave each copy its own value: `LS_PLR_` feeds the
> party roster, `LS_SPR_` feeds the overworld unit panel, and `LS_MULT`'s copy is displayed by
> neither. Those are the first `LS_PLR_`/`LS_SPR_` field meanings established by observation rather
> than by reading the writer.
>
> **Safety:** the subject was copied out of the profile, never edited in place; all three rungs were
> installed under new filenames; the six shipped saves were backed up, `cmp`-verified before and
> after, and the game wrote nothing.
>
> **Still open:** whether `LS_MULT`'s copy is used at all (the multiplayer path was not exercised),
> and a save whose section lengths *move* -- all three rungs were length-preserving.

The brief this box was written against:

**No save we wrote has ever been loaded.** Currently only 1 of 9 sections has an encoder
(`LS_SPR_`); the other eight are copied through. So this tests splicing, not authoring.

⚠️ The savegame corpus is a **live directory** — GS5R3 saves have mtimes inside the measurement
window. The 31/31 byte-account figure will not necessarily reproduce.

---

## Not on this list, deliberately

- **AI upscaling on portraits**, and the restated Lanczos question. Dropped by the operator for
  this pass.
- **Difficulty-dependent AI in controlled games** (issue #6). Dropped for this pass.
- **Anything in `map/`.** That directory has **no backup**.
- **The executable.** It is the next phase, not this one.
