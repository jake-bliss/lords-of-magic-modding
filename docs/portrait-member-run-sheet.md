# The portrait ladder — what `pic.mpq` art a unit actually uses

**Status: ran three times, 2026-09-21; re-read offline the same day.** Two results, three
retractions — one of them a retraction *of* a retraction — and one question reopened.

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

**"A unit's portrait is keyed on the unit's symbol."** Refuted by the corpus before it ever reached
a run: `gs\Dlg\newbuild.gs`'s `get_unit_portrait_name` derives the name from `(faith, code)`, with
codes CHI/GOA/COW/ELE taking a literal `Py` prefix. `portrait\pyelep00.lbm` only looks
symbol-shaped because `ELE` is one of those four.

**"The engine does not read an added `pic.mpq` member."** Unsupported. It rested entirely on the
panel portrait not changing, and — see below — the panel was never in a state that would have asked
for that name.

## 🔴 And one retraction that was itself wrong

**Withdrawn 2026-09-21, same day.** This run sheet originally recorded a third retraction: *"the
unit-info panel's portrait does not come from `get_unit_portrait_name`."* **That is refuted.** It
does. The panel simply reads a different *unit* than the tester was selecting.

**Observed in the corpus.** `gs\Dlg\lescsys.gs`'s `/show_portrait` takes an **army id**, and every
*unit type* lookup in it is hard-coded to **unit index 0** of that army:

```
/show_portrait{/army_id exch def army_id -1 ne
 {/unit_type army_id 0 UNIT_CHAMPION_TYPE getunitdata def
  unit_type -1 ne
   {/unit_type army_id 0 UNIT_TYPE getunitdata def
    unitinfo_dict begin army_id 0 3 load_portrait ... end}
   {currentarmy ARMY_NUM_UNITS getarmydata 1 gt
     {... player_faith unitinfo_dict begin get_faith_dd end ...}
     {unitinfo_dict begin army_id 0 3 load_military_unit_portrait ... end}ifelse}ifelse}
 {... get_faith_dd ...}ifelse}
```

It is called as `getcurrdisplayingarmy show_portrait` from `gs\sel_army.gs`. **Three branches,
tested on unit 0:**

| state of unit 0 | what is drawn |
| --- | --- |
| it is a champion (`UNIT_CHAMPION_TYPE != -1`) — lord, wizard, warrior, thief, heir | `load_portrait` -> `champion_portrait_filename` |
| not a champion, and **`currentarmy`**'s `ARMY_NUM_UNITS > 1` | **no LBM at all** — `get_faith_dd`, a doodad cut from the `intspr1_page` sprite sheet, keyed on the **player's** faith |
| not a champion, army of exactly 1 | `load_military_unit_portrait` -> **`get_unit_portrait_name`** |

**That accounts for every observation in the run, with no second naming scheme:**

- The lord's face for most selections — the lord is unit 0 of his army.
- A lone Rider isolated into its own army showed the Rider — army of 1, non-champion, third branch.
- The probe unit's portrait never changed — it was never in an army of one without a champion, so
  the branch that composes `LIINFP00.LBM` never ran.

🔴 **One subject is not established: the size test uses a different global.** The type lookups read
`army_id`, which the call site supplies as `getcurrdisplayingarmy`; the size test reads
**`currentarmy ARMY_NUM_UNITS getarmydata`**. Nothing here shows those are the same army, and
`gs\sel_army.gs` uses both within a few lines of one another. Whether `currentarmy` is always
`getcurrdisplayingarmy` at this call is **Unknown** — it needs the two natives traced in the binary.
It matters for the attended test below, which is why the test now carries a third control.

**Observed in the corpus.** `gs\Dlg\INFOPAN5.gs`'s `/load_military_unit_portrait` ends the trail:

```
lbm_name unit_type f building_dict begin get_unit_portrait_name end strcpy
```

The line this run sheet originally cited as evidence *against* `get_unit_portrait_name` —
`/f unit_type UNITTYPE_FAITH getunittypedata def` — is the line immediately **above** that call,
inside the same procedure. The read stopped one line short.

⚠️ **The pop-out panel branches on the same test but on the selected index**, not on 0:
`gs\Dlg\INFOPAN5.gs`'s `/setupunitpanel` uses
`getcurrdisplayingarmy unit_index get_real_unit UNIT_CHAMPION_TYPE getunitdata`, then the same
champion / military pair. `get_real_unit` is the identity outside combat. So the pop-out has **no**
faith-badge branch — it is champion-or-military only. *Observed in the corpus.*

### The champion namer is the one thing that differs by profile

🔴 **Name the profile.** *Observed in the corpus, all three installed profiles.*

**Vanilla and 3.02 compose it**, exactly like the military path plus a variant number:

```
/champion_portrait_filename{... /buf"portrait/"strcpy /buf f faith_code strcat
 /buf unit_code_strings ut UNITTYPE_CODE getunittypedata get strcat /buf"P"strcat
 a u UNIT_CHAMPION_PORTRAIT getunitdata dup 0 99 between not{pop 0}if
 dup 10 lt{/buf"0"strcat}if /buf2 cvs /buf exch strcat /buf".LBM"strcat ...}
```

**GS5R3 replaced that with a lookup table.** `gs\PORTRAITS5.gs` defines `/portrait_file_names`, a
dict keyed by unit type holding an array of names per type — `dewiz` has nine
(`"dewizp00.lbm"`..`"dewizp08.lbm"`), `liinf` has one (`"liinfp00.lbm"`). `champion_portrait_filename`
then reads:

```
portrait_file_names ut known
  {dd -1 gt{portrait_file_names ut get dd portrait_file_names ut get length 1 sub max get}
   {... a u UNIT_CHAMPION_PORTRAIT getunitdata dup 0 <len-1> between not{pop 0}if get}ifelse}
  {["LIFE.lbm" "DEATH.lbm" "ORDER.lbm" "CHAOS.lbm" "FIRE.lbm" "WATER.lbm" "EARTH.lbm" "AIR.lbm" "MARAUDER.lbm"]f get}ifelse
```

**So the name space never changed** — GS5R3's table entries are the same `<ff><code>p<NN>.lbm`
strings the vanilla code composes, under the same `portrait/` prefix. What changed is that GS5R3
can now name a portrait that does *not* follow the pattern, and that an **unknown unit type falls
back to the faith banner** `portrait/<FAITH>.lbm` rather than to a composed name.

🔴 **A "latent defect" I reported here on 2026-09-21 is REFUTED — same day, by this repository's
own prior work.** The index expression is `dd portrait_file_names ut get length 1 sub max get`, and
I read `max` as a true maximum, which would make it always yield `len-1`. That is wrong.

**Observed in the corpus.** `max` and `min` mean **opposite things in GS5R3 and in vanilla**:

| profile | `/min` | `/max` | returns |
| --- | --- | --- | --- |
| vanilla | `{2 copy gt{exch pop}{pop}ifelse}` | `{2 copy lt{exch pop}{pop}ifelse}` | correctly named |
| GS5R3 | `{2 copy lt{exch pop}{pop}ifelse}` | `{2 copy gt{exch pop}{pop}ifelse}` | **swapped** — `max` returns the *smaller* |

`{2 copy gt{exch pop}{pop}ifelse}` on `a b` leaves `b` when `a > b` and `a` otherwise — the smaller
operand. GS5R3 binds that to the name `max`, which its own `gs\standard.gs` comment calls a
"MAXIMUM LIMITOR": a clamp *to* a maximum, not a maximum function. So `dd (len-1) max` is
`min(dd, len-1)` — **a correct clamp**, and the sibling branch's `between not{pop 0}if` is the same
intent written the other way.

⚠️ **This was already established here and I contradicted it by re-deriving from scratch.**
[GameScript format](gamescript-format.md) records the commented-out/redefined pair in GS5R3's
`standard.gs`, the acceptance battery **asserts** GS5R3's `min`/`max` are reversed, and the research
log has the swap measured against raw bytes. Reading the existing documentation would have cost less
than the derivation did. See [check the code first](#instrument-notes-worth-keeping).

**The reusable fact, which is worth more than the non-defect:** every `min`/`max` reading anywhere in
this corpus is **profile-scoped**. A GS5R3 expression and a vanilla expression that look identical
compute opposite things.

## The three channels, as they stand

| channel | keyed on | evidence class |
| --- | --- | --- |
| Overworld sprite | `/impfile_proc` -> `imp.mpq` | Observed in gameplay |
| Army roster figure | `(faith, code)` | Observed in gameplay, 2026-09-21 |
| Recruit-dialog portrait | `(faith, code)` -> `pic.mpq` | Observed in gameplay, 2026-09-21 |
| Unit-panel portrait, unit 0 non-champion, army of 1 | `(faith, code)` -> `get_unit_portrait_name` | Observed in the corpus |
| Unit-panel portrait, unit 0 is a champion | `champion_portrait_filename` — composed `(faith, code, variant)` in vanilla/3.02, table lookup in GS5R3 | Observed in the corpus |
| Unit-panel portrait, non-champion in an army of >1 | **no `pic.mpq` member** — a faith badge cut from `intspr1_page` | Observed in the corpus |

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

## The cheapest test of the panel, now that the branch is known

**One in-game action, no archive change.** The replaced `PORTRAIT\LIINFP00.LBM` is already proven
to reach the engine through the barracks. Split a single Life Infantry unit into an army of its own
with no champion in it and select that army: `show_portrait` then takes the
`ARMY_NUM_UNITS == 1`, non-champion branch and **must** draw the replacement bottom-left.

**State the expected reading first.** If it draws the elephant, the trail above is confirmed
end-to-end in the engine. If it draws the stock Staffmen portrait, the corpus trail is right about
the *name* and wrong about *which member the engine loads*, which would be a new and separate
finding. If it draws a **faith badge**, the size test did not see an army of 1 — and that has **two**
causes, not one: either the split did not take, or `currentarmy` is not the army being displayed.
Confirm the split army is the selected army before reading the slot, or the run cannot tell those
apart.

As a control in the same sitting, select an army whose unit 0 **is** the lord: that must take the
champion branch and be unaffected by the `LIINFP00` replacement. A control that also changed would
mean the branch test is not what decides the draw.

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
