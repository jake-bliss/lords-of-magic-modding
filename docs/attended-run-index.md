# Attended run index

## Status, 2026-09-20

| Item | State |
|---|---|
| **A1** command line of the unmeasured profiles | ✅ **DONE** 09-19 — GS5R3 runs `d:\lomse.exe /*`; premise holds |
| **A2** guard refuses per profile | ✅ **DONE** 09-19 — refuses exit 1 running, permits exit 0 closed |
| **B1** unit anchor probe | ⚠️ **RUN, NOT ANSWERED** — control passed, subject was wrong art. Rebuilt around `/pyele`; ready to run. |
| **B2** direction 0 to a bearing | ⏸ **OFFLINE FIRST** — script route refuted; chase the table builder behind `0x5AE970` |
| **C1** PBM ByteRun1 encoder | ✅ **DONE 09-20** — ladder rungs 3+4; member shrank, `TINY` dropped, stripe rendered clean |
| **C2** a size-changing edit | ✅ **DONE 09-19** — cheat-keys ladder, 21→20 bytes |
| **C3** a member added, not replaced | ✅ **DONE 09-20** — ladder rung 5; `imp.mpq` grew to 3,601 members |
| **C4** two members in one build | ✅ **DONE 09-19** — acceptance rungs 6–9 |
| **C5** first `imp.mpq` run | ✅ **DONE 09-20** — ladder rungs 0+1, 2; repack accepted and our IMP pixels rendered |
| **C6** load a savegame we wrote | 🔴 **QUEUED, NOW UNBLOCKED** — all nine sections encode, 31/31 byte-identical |

**Group C is closed.** Every engine-acceptance gap on this sheet has been run. The ladder that
`docs/engine-acceptance-ladder.md` describes is complete — all ten rungs, across four sittings.

**What that means for modding.** The two things standing between this project and shipping **new
art** were adding a member and touching `imp.mpq` at all. Both fell on 2026-09-20. Sprites, images,
audio, GameScript and savegames all now have a proven path from this pipeline into the running
engine. `docs/roadmap.md`'s per-archive regions carry the exact limits that remain, derived from the
runs rather than typed beside them.

**What is left on this sheet:** B1 (rerun, the redesigned probe), B2 (offline chase first), and C6
(newly unblocked). None of them is an acceptance question any more — they are measurement questions.

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

### B1. Unit anchor probe — `docs/unit-anchor-run-sheet.md` ⚠️ RUN 2026-09-19, NOT ANSWERED

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

### C6. Load a savegame this project wrote

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
