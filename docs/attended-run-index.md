# Attended run index

## Status, 2026-09-19

| Item | State |
|---|---|
| **A1** command line of the unmeasured profiles | ✅ **DONE** — GS5R3 runs `d:\lomse.exe /*`; premise holds |
| **A2** guard refuses per profile | ✅ **DONE** — refuses exit 1 running, permits exit 0 closed |
| **B1** unit anchor probe | ⚠️ **RUN, NOT ANSWERED** — control passed, subject was wrong art. Redesign in progress. |
| **B2** direction 0 to a bearing | ⏸ **OFFLINE FIRST** — script route refuted; chase the table builder behind `0x5AE970` |
| **C1** PBM ByteRun1 encoder | 🔴 **QUEUED, HIGHEST VALUE** — the encoder has still never faced the engine |
| **C2** a size-changing edit | ✅ **ALREADY DONE** — see below; this box was stale |
| **C3** a member added, not replaced | 🔴 **QUEUED** — genuinely never attempted, and it blocks new art |
| **C4** two members in one build | ✅ **ALREADY DONE** — see below; this box was stale |
| **C5** first `imp.mpq` run | 🔴 **QUEUED** — the last wholly-untouched archive |
| **C6** load a savegame we wrote | ⏸ **BLOCKED** — encoders for the other eight sections in progress |

**Two boxes closed without anyone noticing.** C2 and C4 were both satisfied by runs already made,
and the roadmap went on publishing them as limits until 2026-09-19. C2 fell to the cheat-keys
ladder: `/cheat_keys false def` -> `/cheat_keys true def` is 21 bytes to 20, the archive shipped,
and the engine ran it. C4 fell to acceptance rungs 6-9, which rewrote two members in each of two
archives in one build. Neither was the point of its run, which is exactly why neither was noticed.
The limits are now derived from the recorded runs by `tools/engine_acceptance.py` rather than
typed beside them, so this cannot recur silently.

**What that leaves as the real frontier:** adding a member rather than replacing one (C3), and
`imp.mpq` in any form (C5). Those two together are what stand between this project and shipping
**new art**. Everything needed for a gameplay or balance mod is already proven.


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

These are the engine-acceptance gaps. The engine has only ever been shown:
**one member, replaced not added, length-preserving, storage class `0x80010100`** — plus the
`0x80010000` STORED class proven by rungs 6–9 on 2026-09-19.

### C1. The PBM ByteRun1 encoder, in front of the engine 🔴 HIGHEST VALUE

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

### C3. A member added rather than replaced

Never attempted. `allow_new_members` exists and is deliberately gated behind two separate acts.

### C4. Two members in one build ✅ CLOSED 2026-09-19

> **Closed by acceptance rungs 6-9.** Each rewrote **two** members -- `wav\welcome.wav` and
> `wav\button.wav` -- in **both** `sndfx.mpq` and `special.mpq`, four changed members in one
> build, and the engine played them. **Observed in gameplay, 2026-09-19.**

The claim this box was written against:
Never attempted. Nothing establishes that a *large* mod loads — every acceptance so far is one
member. Note rungs 6–9 did rewrite two members across two archives, so this is partly addressed;
confirm against the ladder outcome before spending a sitting on it.

### C5. First `imp.mpq` run

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
