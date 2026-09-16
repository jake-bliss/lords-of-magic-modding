# Difficulty and computer-player AI

## Short answer

**Yes, difficulty can change computer-player behavior, not just its resources.** In the original Steam scripts, the difficulty setting gates at least two strategic mercenary attack routines and changes which opposing Faiths are initially treated as active rather than neutral. It also changes AI experience and generated army strength. The 3.02 patch preserves the examined AI routines. GS5R3 additionally loads replacement AI scripts with difficulty-dependent target and combat choices.

GS5R3 also applies **difficulty-scaled stat bonuses to computer-controlled units**, in
`gs\LEVLMODS5.gs`, suppressed in multiplayer. That path is gated on an opt-in `insane_mode?` rather
than on the difficulty slider alone, and it is absent from 3.02 and baseline.

This is a **static script finding**, not yet a controlled gameplay measurement. It does not establish that Easy, Medium, and Hard use entirely different AI engines, nor that every computer-player decision is difficulty-dependent.

## Evidence by lineage

| Behavior | Steam baseline | 3.02 | GS5R3 |
| --- | --- | --- | --- |
| AI mercenary attack/invasion gates | Easy requires turn >100 or >150 respectively; Medium/Hard can pass immediately, provided resource and war requirements also pass | Examined `turnai/mercs.gs` is byte-identical to baseline | May retain or alter behavior; requires an explicit call-path audit |
| Neutral versus active opposing Faiths | `scenario.gs` has separate Medium/Hard activation tables | Not audited separately here | Modded scenario logic needs its own audit |
| AI unit experience | `turn_ai.gs` applies larger per-turn experience increments by difficulty; non-neutral computer players receive 10/20/30 on Easy/Medium/Hard in the examined routine | Examined `turn_ai.gs` is byte-identical | Reworked AI progression |
| Marauder target decisions | No difficulty call in examined `brain.gs` | `brain.gs` is byte-identical | Loaded `BRAIN5.gs` uses a difficulty-selected chance of approaching a guarded city when its army is weaker |
| Combat decisions | No difficulty call in examined `combatai.gs` | `combatai.gs` is byte-identical | Loaded `COMBATAI5.gs` uses difficulty-scaled random tests in flee/parry branches |

The [Special Edition manual](https://sierrahelp.com/Documents/Manuals/Lords_of_Magic_-_Special_Edition_-_Manual.pdf) describes different starting resources, dungeon spoils, scrolls, fealty shares, and Hard-mode Balkoth advantages. Those are real difficulty effects but do not, by themselves, prove an AI algorithm change. The shipped scripts provide the additional evidence above.

## How this was checked

On 2026-09-15, the legally installed baseline, 3.02, and GS5R3 `gs.mpq` archives were extracted to a disposable local directory using the repository's read-only MPQ tool. Searches were limited to difficulty references and AI call sites; source files were not added to Git.

- Baseline `gs/turn_ai.gs` loads `gs/turnai/mercs.gs`; its `update_ai_unit_experience` routine dispatches on `getdifficultylevel`.
- Baseline `gs/turnai/mercs.gs` tests difficulty inside `emergency_mercenaries` and `mercenary_invasion`. Easy can reach those routines only after turns 100 and 150, respectively, while Medium/Hard are not delayed by those particular gates. The resource thresholds, enemy-at-war tests, and other selection rules still apply.
- Baseline `gs/scenario.gs` defines different Medium/Hard `extra_active_ai_table` values used while setting neutral AI statuses. This can change which rivals act against the player from the outset; the exact in-game result depends on Faith and player configuration.
- Baseline `gs/makearmy.gs` uses difficulty-specific army-experience tables. `gs/placedng.gs` also changes dungeon/security generation, which can indirectly alter what the computer player faces.
- `cmp` verified baseline and 3.02 `gs/brain.gs`, `gs/combatai.gs`, `gs/turn_ai.gs`, and `gs/turnai/mercs.gs` are byte-identical. `gs/makearmy.gs` differs because 3.02 fixes a unit-table entry; see [MPQ inventory](mpq-inventory.md).
- GS5R3 `START.GS` loads `gs/BRAIN5.gs`; `gs/ai_plays.gs` loads `gs/COMBATAI5.gs`. In the former, the guarded-city target check uses a difficulty-indexed chance table. In the latter, random flee/parry branches use `getdifficultylevel` as a probability factor. A separate commented-out difficulty expression in `BRAIN5.gs` was **not** counted as active behavior.

## GS5R3 AI stat bonuses, and a refuted community claim

On 2026-09-16 the GS5R3 corpus was searched after the mod's author, posting as Boaster in December
2020, stated that *"AI controlled units on Hard mode get extra 'level' stat bonuses"* and named
`extra_strong?` in `gs/LEVLMODS5.gs` as the control.

**The phenomenon is real; the named control word is not.**

- `extra_strong?` is defined once, in `gs\scenario\default.gs` lines 147-152, as
  `[false false false]getdifficultylevel get` followed by `getmultiplayerflag{pop false}if`. That is
  **false on Easy, Medium and Hard**. The forum post quotes `true` in that position, so it describes
  a hand-edited or pre-ship version.
- `extra_strong?` appears nowhere in `gs\LEVLMODS5.gs`. It occurs in exactly two GS5R3 members and in
  neither 3.02 nor baseline. Treat it as vestigial as installed.
- The bonus block in `gs\LEVLMODS5.gs` lines 366-383 is gated on `insane_mode?`, an opt-in mode
  initialised `false` at `START.GS:45` and enabled by a custom token artifact or the pick-faith UI.
  `gs\SCENARIO5.gs:721` couples it to Hard, so it is difficulty-adjacent but not the difficulty
  slider.
- The bonus body itself **is** difficulty-scaled, via `getdifficultylevel add` against a
  `{35 70 140 280}` turn ladder, with further multipliers when an opposing player is human-controlled
  and when a human player has taken Death.
- A second, live difficulty path the forum never mentioned: `gs\artifact\_custom\misc\ai_stat_bonus.gs`
  is an invisible GS5R3 artifact described in its own source as *"designed to improve leader/heir
  stats based on difficulty level"*, with `/wield_mods` entries of the form
  `{0 getdifficultylevel add 3 mul add}`.

Supporting native facts established in the same pass: `getdifficultylevel`, `setdifficultylevel`,
`getmultiplayerflag`, `getplayeraistatus`, and the constants `EASY_LEVEL` / `MEDIUM_LEVEL` /
`HARD_LEVEL` are never defined in any script and all appear as strings in `lomse.exe`, so they are
native. GS5R3 adds a script-side `/INSANE_LEVEL 3 def`, implying the native values are 0, 1, 2.

## What remains unproven

The original executable may have native AI logic that these scripts do not expose. We have not instrumented an identical-seed, identical-Faith game on all three settings, so we cannot yet quantify how often the script gates change observed decisions. GS5R3's replacement scripts are a mod-specific finding and should not be attributed to vanilla or 3.02.

The community's own claims about difficulty are weak evidence and one was wrong about the author's
own code; see [community research](community-research.md). Do not substitute a changelog for the
shipped scripts.

For the next agent, a useful bounded follow-up is [issue #6](https://github.com/jake-bliss/lords-of-magic-modding/issues/6): trace the GS5R3 call paths, record difficulty constants, then run a controlled vanilla/3.02 test around turns 100 and 150 while holding Faith, map, opponent, and resources constant. Keep proprietary scripts and saves out of the repository; commit only analysis and derived test observations.
