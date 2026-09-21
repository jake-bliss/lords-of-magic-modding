# The `unitcap` run sheet — two declared caps, at their boundaries

**Status: ran 2026-09-21, passed on the third attempt.** Written 2026-09-21.

> ## ✅ What it settled
>
> | question | reading | expected |
> | --- | --- | --- |
> | An aura type past the shipped 70 | `zaura71 70`, `zaura72 71` | 70, 71 |
> | Does `gs\aura.gs` run at boot? | `zauracap 100` | 100 |
> | A unit type at index **199** | `subject army 1106 at 16311` | cell 16311 |
> | One definition **past** capacity | `numunittypes 200; index -1`, no crash | 200, -1 |
>
> **Observed in gameplay, 2026-09-21.** Every prediction made that morning from the disassembly
> held, including the failure prediction.

## Why this run existed

The 2026-09-21 disassembly ([new units](new-units.md#raising-a-full-cap-nothing-in-the-engine-stops-you))
found that every `max*` declaration is a **script literal**, and that no allocator bounds the count
beyond `count >= 1`. It also predicted the failure mode: the append helper returns -1 when
`used >= capacity`, and the operator prints `"<name> failed"`.

All of that was disassembly alone. Nothing had been put in front of the engine.

## The ladder

| rung | what | why it is there |
| ---: | --- | --- |
| 0 | log `numunittypes` against its expected value | a baseline that has moved shows up as a log line, not as a wrong conclusion |
| 1 | read `zauracap`, `zaura71`, `zaura72` out of the patched `aura.gs` | taken FIRST: they need no army and no map, so a session that dies later still returns them |
| 2 | define one type, place it, **read it back** | the control -- proven on 2026-09-21, so it says the mechanism works this session |
| 3 | fill to exactly `200 maxunittypes` | count computed from the LIVE count, never a literal |
| 4 | place the type at index 199, **read it back** | the subject |
| 5 | one definition past capacity | **expected to fail**; the question is whether it fails cleanly |

### The aura registrations are appended to `aura.gs`, not issued from `hotkey.gs`

The two extra `addauratype` calls are written into the patched `gs\aura.gs` itself, copying that
file's own last line verbatim. Every helper in it (`SPELL_ORIGIN2_HOTSPOT`,
`modifier_aura_imp_filename`) resolves in that file's scope, which a call made later from
`hotkey.gs` could not rely on. That is what keeps a failed registration attributable to the **cap**
rather than to a call form this project invented.

`zauracap` is a plain `def` on the same patched file. It says **"this file executed"**, not "the
allocation succeeded" -- deliberately two different readings. If `zauracap` had come back `100`
while `zaura71` came back `-1`, the allocation itself would have failed, which is a real result
rather than an ambiguity.

## 🔴 It took three attempts, and both failures were mine

Recorded in full because the pattern matters more than the result.

**Attempt 1 — the probe never fired.** No `zprobe.log` at all. The body ended `}bindhotkey`. **No
such operator exists**; the working probes end `}addhotkey`. `hotkey.gs` therefore failed to LOAD,
the `z` binding was never registered, and the keypress had nothing to hit. Nothing caught it: the
braces balanced, the tokenizer was happy, and the install verified the member read back
byte-identical. None of those check that a name is a name the engine has.

**Attempt 2 — a full log, every rung green, and an empty map.** `zowner` was passed to
`add_unit_to_location` twice and **never defined**. An edit meant for this body had landed on the
first matching line in the file, which belonged to a different probe -- `str.replace(a, b, 1)`
takes the first match in the file, not the first match in the function. The placement could not
work. Rung 4 nonetheless logged `"placed index 199 at cell 183"`, because it reported the **call**
rather than the **outcome**. A human had to walk the map and say "nothing there".

**Three guards now exist, each mutation-verified to fail when its bug is reintroduced:**

- `ProbeVocabularyTest` — every bare name in every probe body must appear in the corpus vocabulary
  **union** the natives recovered from `lomse.exe`. Neither report alone is the right instrument:
  the vocabulary report misses a tool-facing native no script calls, and `map2screen` is exactly
  that.
- `ProbeNameDefinitionTest.test_no_probe_reads_a_name_it_never_defines` — tokenised, not grepped,
  so `"zprobe.log"` and `ASCII_VAL"z"` are strings rather than reads.
- `ProbeNameDefinitionTest.test_every_placement_is_read_back` — a probe that calls
  `add_unit_to_location` must ask `armyat` whether the unit is there. **`add_unit_to_location`
  leaves nothing on the stack and reports nothing**, so logging "placed" after calling it records
  an intention, not an outcome.

## A by-product: a unit has at least three art channels

**Observed in gameplay, 2026-09-21.** The placed unit drew as an **Elephant on the overworld** --
correct, from its `/impfile_proc{"pyele"unittype_imp_filename}` -- while its **detail-panel
portrait** and its **roster figure** were neither an elephant nor each other. The portrait was an
elf face; the observer reported the roster figure as dwarf infantry.

**Observed in the corpus**, and this is the part I got wrong first: the portrait name comes from
**`(faith, code)`**, not from the unit's symbol. `gs\Dlg\newbuild.gs`'s `get_unit_portrait_name`
builds `portrait/<FF><CODE>P00.LBM`, with codes CHI/GOA/COW/ELE taking the literal prefix `Py`
instead -- all of which [new units](new-units.md#4-portrait--one-member) already recorded. The
recovered names bear it out: the prefixes group by faith (`ch` 22, `or` 20, `wa` 18, `de` 18,
`ai` 18, `fi` 15, `ea` 14, `li` 12) with exactly **4** `py` members, matching the four special
codes.

This probe's unit is `/faith EARTH` + `/code WMT`, so its portrait would be
**`portrait\EAWMTP00.LBM`**. Probed **by name hash against GS5R3's own `pic.mpq`** -- the archive
this run actually used -- on 2026-09-21:

| name | status | block | size |
| --- | --- | ---: | ---: |
| `portrait\EAWMTP00.LBM` | **absent** | — | — |
| `portrait\PyELEP00.LBM` | present | 663 | 10,610 |
| `portrait\ORINFP00.LBM` | present | 612 | 10,594 |
| `portrait\zutestp00.lbm` | **absent** | — | — |

No portrait exists for this unit, so the engine drew a fallback -- which is what was on screen. The
two present rows are the `(faith, code)` rule's own predictions (`OR`+`INF`, and `ELE` taking the
`Py` prefix), so the same probe that confirms the absence confirms the naming.

⚠️ **Why the hash probe and not the recovered listing.** `reports/member-names/` records names this
project *recovered*; GS5R3's `pic.mpq` has no listfile, and its recovered listing contains **no**
`p00` members at all. Concluding "absent" from that file would have been a negative drawn with an
instrument that cannot see the archive the run used. A direct hash lookup can, and does.

🔴 **I first published this as "portraits are named after the unit's symbol."** `portrait\pyelep00.lbm`
looks symbol-shaped only because `ELE` is one of the four codes that take the `Py` prefix; the
coincidence is total. The correct rule was **already in the document I was editing, ninety lines
above my edit, marked Observed in the corpus**. Two reviewers caught it. See
[[feedback-check-the-code-before-publishing-a-measurement]] -- third instance.

⭐ **What that changes about the next run.** The name is confirmed absent from the live archive, so
the test is to add **`portrait\EAWMTP00.LBM`** to
`pic.mpq` -- not `portrait\zutestp00.lbm`, which the engine would never look for. If the panel
face changes, that settles the `(faith, code)` lookup **in the engine** rather than in the script
that builds the name, **and** closes open question 2 -- whether the engine loads a `pic.mpq` with
an added member, currently proven only at the repack layer.

**What this by-product does not establish.** Nothing about the **roster figure**, whose source was
not traced at all; it is a third channel, named here only because it was visibly different from
the other two.

## Running it again

```
LOM_PROBE=unitcap scripts/install-engine-probe.sh
```

World map, army centred with room either side, tap `z` once. Two units appear and are **not**
cleaned up -- they exist until the game exits. Do not save: none of these types exists in any
archive. Afterwards, `scripts/restore-game-archives.sh`.
