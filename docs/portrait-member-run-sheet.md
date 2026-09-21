# The portrait ladder — what `pic.mpq` art a unit actually uses

**Status: ran three times, 2026-09-21.** Two results, one retraction, one question reopened.

## ⭐ What it settled

**Observed in gameplay, 2026-09-21.** `PORTRAIT\LIINFP00.LBM` was **replaced** with the Elephant's
portrait, and the **barracks recruit dialog for Elven Staffmen drew the elephant**.

- **The engine reads a modified `pic.mpq`.** That archive had never been put in front of the engine
  in any form -- `gs.mpq` and `imp.mpq` had, `pic.mpq` had not.
- **The `(faith, code)` portrait rule is confirmed in the ENGINE**, not merely in the script that
  builds the name. Elven Staffmen are LIFE + INF, and the member named
  `portrait/` + `faith_code(LIFE)` + `INF` + `P00.LBM` is what changed on screen.

**Observed in gameplay.** The **army roster figure** is keyed on `(faith, code)` too: the probe unit
showed a dwarf-infantry figure as `EARTH`+`WMT` and an **Elven Staffmen** figure as `LIFE`+`INF`.
That was the third art channel, previously untraced.

## 🔴 What it retracted

**The unit-info panel's portrait does not come from `get_unit_portrait_name`.** A probe unit defined
as `LIFE`+`INF` -- whose portrait member is the one this build replaced, and which provably draws
the replacement in the barracks -- showed an **unchanged** portrait in both the pop-out panel and
the bottom-left slot.

That invalidates two conclusions reached earlier the same evening:

1. **"A unit's portrait is keyed on the unit's symbol."** Refuted by the corpus before it ever
   reached a run: `gs\Dlg\newbuild.gs`'s `get_unit_portrait_name` derives the name from
   `(faith, code)`, with codes CHI/GOA/COW/ELE taking a literal `Py` prefix. `portrait\pyelep00.lbm`
   only looks symbol-shaped because `ELE` is one of those four.
2. **"The engine does not read an added `pic.mpq` member."** Unsupported. It rested entirely on the
   panel portrait not changing, and the panel does not ask for that name.

## The three channels, as they stand

| channel | keyed on | evidence class |
| --- | --- | --- |
| Overworld sprite | `/impfile_proc` -> `imp.mpq` | Observed in gameplay |
| Army roster figure | `(faith, code)` | Observed in gameplay, 2026-09-21 |
| Recruit-dialog portrait | `(faith, code)` -> `pic.mpq` | Observed in gameplay, 2026-09-21 |
| Unit-panel portrait | **unknown** | it is NOT `get_unit_portrait_name`; nothing further established |

## The name rule, read out of the corpus

```
unit_portrait_name = "portrait/"
  if code > SHP and code <= ELE : + "Py"
  else                          : + faith_code(f)      ; LI DE OR CH FI WA EA AI
  + unit_code_strings[code]                            ; "INF" "MIS" ... "WMT"
  + "P00.LBM"
```

`f` is the unit's **own** faith at the info-panel call site (`gs\dlg\INFOPAN5.gs:3222`,
`/f unit_type UNITTYPE_FAITH getunittypedata def`) and the **building's** faith in the build dialog.
`unit_code_strings` has 37 entries; `SHP` is 15, `ELE` is 19, `WMT` is 34.

## Still open: does the engine read an ADDED `pic.mpq` member?

`portrait\EAWMTP00.LBM` was added and never appeared -- but no UI was ever shown to request it, so
this run says nothing either way. Open question 2 stands.

**A design that would settle it.** The barracks dialog provably requests by `(faith, code)` and
provably reads `pic.mpq`. Six `(faith, code)` pairs used by shipped unit definitions have **no**
portrait member in GS5R3's `pic.mpq` -- `CHLD2`, `DELD1`, `DEWM2`, `EALD1`, `WALD1`, `WALD2`.
Adding one of those and reaching a dialog that displays that unit is the clean test. All six are
non-LIFE, so it needs a game of the matching faith.

## Instrument notes worth keeping

- 🔴 **`pic.mpq` had no backup** before this work. The manifest covered `gs.mpq` and `imp.mpq` only.
  It is now backed up, hash-verified and in `ARCHIVE_NAMES`, so verify and restore both cover it.
- 🔴 **GS5R3's `pic.mpq` has mixed storage classes** -- 1,405 members, 995 at `0x200` and 410 at
  `0x10100`, and the named `portrait\` members are themselves split 662/88. `lom_mpq`'s own comment
  claimed all `pic.mpq` members share one class; that was measured on a different profile. Adding a
  member therefore needs `--add-storage-of NAME`, which copies an existing member's class -- a fact
  about the mod rather than a modal guess.
- ⚠️ **A repack rewrites `(listfile)` and loses names.** GS5R3's `pic.mpq` went from 1,081 names to
  996 plus the added one, while **every member's bytes stayed identical**. Members resolve by hash,
  so no run here was affected, but any archive this pipeline ships carries the loss. Not measured
  for `gs.mpq` or `imp.mpq`.
- **Name lookups are case-insensitive in MPQ but were not in this tool.** `--replace` and
  `--add-storage-of` now normalise case and `/` vs `\`.
- **`probe-names` is the right instrument for a negative** about an archive with no listfile.
  `reports/member-names/` holds only names this project *recovered*, and GS5R3's `pic.mpq` listing
  contains no `p00` members at all.
