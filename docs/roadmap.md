# Roadmap

## Goal

Create a reproducible, source-controlled modding toolkit that lets us inspect, change, validate, package, and test Lords of Magic data without editing a playable installation by hand.

## Where we actually stand

Last reconciled against the tree at `1d1e406` (PR #60). Before that, this file had not been updated
since PR #51 and did not describe nine merged pull requests.

| Phase | State |
|---|---|
| 1 — Archive inventory | **Complete.** The repack command landed with a block-index shape check. |
| 2 — Script documentation | **Complete.** 1,535 symbols indexed across three profiles, searchable by code or display name. |
| 3 — Build and validation pipeline | **Complete.** `validate`, `build`, `install-dev` and `restore-dev` exist, are tested, and have been run end to end against the engine. |
| 4 — First vertical slice | **Complete, Observed in gameplay 2026-09-18.** `units\orinf.gs` built, validated, installed and read off the unit panel; rollback verified. |
| 5 — Asset pipeline | **22 of 25 boxes, and the critical path is done.** A rewritten `pic.mpq` runs in the engine, PBM and IMP both have pixel encoders, images are content-validated, and issue #4's placed-sprite fields fell to a controlled save-diff (all 2026-09-18). What remains is not blocking: batch conversion, the IMP `AnimRules` wiring, and AI upscaling. The Lanczos box is recorded as not well-posed rather than ticked. |
| 6 — Mod direction | Gated on 3 and 4, and **that gate is now open**. |
| Format and engine reverse-engineering | Far ahead of what this document ever planned. See the section below. |

**Phases 3 and 4 were the critical path. Both are now closed.** Phase 4 exists to force the complete
workflow into being — build reproducibly, validate, install to a development profile, observe the
change in game, roll back. Everything in Phases 1 and 5 is input to it.

The honest summary used to be that the project had enormous read capability and no delivery
pipeline. **That is no longer true.** The pipeline exists ([build pipeline](build-pipeline.md)),
takes a mod source tree to a verified reproducible archive with a change report, installs it into a
cloned development profile, and on 2026-09-18 the engine loaded one and rendered the change -- in
**two** independent places in the interface. Every link of the chain has now been exercised for
real rather than against fabricated directories.

A second, related caution: a large amount of recent work — multiplayer, the savegame format, the
GameScript VM, the operator bodies — was **not on this roadmap at all**. It is genuine and it is
documented, but it advanced understanding rather than the stated deliverable. It is recorded in its
own section below rather than being retro-fitted into phases it was never part of.

## Phase 1 — Archive inventory (initial pass complete)

- [x] Select and validate StormLib for read-only cross-platform MPQ extraction.
- [x] Extract `gs.mpq` and `pic.mpq` from baseline, 3.02, and GS5R3.
- [x] Preserve archive paths and listfile names where available.
- [x] Produce machine-readable manifests containing path, size, compressed size, flags, locale, and content hashes.
- [x] Diff all three extracted trees and distinguish formatting/comment-only changes from token-level script changes.
- [x] Recover the public Lords of Magic filename catalog and apply it without modifying archives.
- [x] Classify every member in the five core GS5R3 archives by detected format.
- [x] Prove archive write-back: `SFileAddFileEx` round-trips a `gs.mpq` member and the game executes the rewritten archive (2026-09-16).
- [x] Wrap that in a deterministic, reproducible repack command with a byte-shape check before any development-profile install. **`scripts/repack-archive.sh` + `lom-mpq repack` + `tools/mpq_shape.py`** ([repack](repack.md)). Byte-identical output measured across repeated runs, and across changed mtimes, changed source paths, `umask` and `TZ`. Refuses and exits nonzero on a shape mismatch, and `--install` refuses outright, because installing is Phase 3.

- [x] Recover the names of members no archive catalogues, by pooling every catalogue and confirming each candidate against the target archive itself. **23,005 of the 23,026 unnamed members across the three profiles are now named**, leaving 21 ([member names](member-names.md)).

**Phase 1 is complete.** Two measured limits are recorded rather than papered over: StormLib
regenerates `(listfile)` on every write so its bytes cannot be preserved — the single exemption,
safe only because every member name is verified individually — and compaction is off by default
because `SFileCompactArchive` fails with `ERROR_UNKNOWN_FILE_NAMES` on any archive holding unnamed
members.

**Updated 2026-09-18:** the second of those limits used to be stated as permanent, on the grounds
that 409 PIC5R3 members have no name and **the name is part of the encryption key**. The premise has
largely gone: name recovery leaves 21 unnamed members in the whole corpus, not 12,000
([member names](member-names.md)). The conclusion has not changed, and neither has the practice.
Archives are still copied-then-patched rather than rebuilt from an extracted tree, for three
reasons that recovery does not touch:

- 21 members still have no name at all — 9 combat-AI plays in vanilla and 3.02 `gs.mpq`, and one
  image in each `pic.mpq`;
- recovered names are supplied to a *reader*. `SFileCompactArchive` reads names from the archive's
  own `(listfile)`, which `pic.mpq`, `imp.mpq`, `sndfx.mpq` and `special.mpq` do not have at all, so
  compaction still fails them. Writing a recovered listfile into an archive might change that and
  has not been tried; it is a Phase 3 decision;
- a rebuild must reproduce each member's flags, locale and storage decisions, which nothing here
  addresses.

Shape preserved is not the same as playable. **Updated 2026-09-18:** this paragraph used to end
"the compression choice for a `pic.mpq` replacement is **Inferred** and has never faced the engine."
Both halves are now settled, and neither the way it expected. Compression was never the open
question: every one of the 1,071 members of the baseline `pic.mpq` carries flags `0x80010100`
(EXISTS | ENCRYPTED | IMPLODE), the same class as the `gs.mpq` member of the 2026-09-16 round trip.
And a rewritten `pic.mpq` has now faced the engine and been read by it -- see Phase 5 below.

Delivered: [MPQ inventory](mpq-inventory.md), tracked comparison summaries, and reproducible extraction commands.

## Phase 2 — Script documentation

- [x] Infer and implement the first corpus-wide lexical model.
- [x] Inventory executable names, literal definitions, and static `run` references across all three profiles.
- [x] Build a semantic symbol/index database for units, spells, artifacts, buildings, factions, and encounters. **1,535 symbols** in `reports/gameplay/`, generated and committed.
- [x] Compare vanilla behavior with 3.02 fixes and GS5R3 changes.
- [x] Document identifiers, references, ranges, defaults, and likely hard limits.
- [x] Mark uncertain interpretations and attach evidence examples.

Deliverable: a searchable gameplay-data reference with annotated examples — [gameplay reference](gameplay-reference.md), queryable with `--gameplay-symbol` / `--gameplay-symbols-like` by internal code or display name.

**Phase 2 is complete**, with its coverage stated rather than implied. Units, spells, artifacts and
encounters are well served: 1,515 of the 1,535 symbols. **Factions (8) and buildings (12) are thin,
and that is a finding, not a gap** — GameScript carries no faction or building record, so that data
lives in `lomse.exe` or in unaligned call-site tuples. 1,439 symbols carry a human-facing display
name; the 96 that do not are kinds GameScript never labels.

**The result to carry into everything downstream: a corpus-observed maximum is not an engine bound,
and here the corpus refutes the tempting reading itself.** All eight unit magic resistances stop at
exactly 100 across 88 vanilla units, which is precisely what a hard cap looks like. GS5R3 reaches
125 and 150 against a **byte-identical `lomse.exe`**. No limit in the reference is claimed as
engine-enforced, and `docs/gameplay-reference.md` opens with this rather than burying it.

Two further results worth keeping in view: **3.02 changes no gameplay record at all** — all 1,535
symbols are byte-identical to vanilla, and its 14 modified members are dialog, hotkeys, text and
stdlib — and GS5R3's apparent 1,204 add/removes are largely a **wholesale rename** of its spell set,
294 of which were recovered by token fingerprint.

Work since this phase was written went deeper than it in a different direction — the engine's
1,906-operator dispatch table, recovered operator arity, the operator bodies, and a GameScript VM
slice. That is language and runtime understanding. The phase asks for something else: a
**gameplay-data** index, so that a person can ask what a unit costs, where a spell is defined, and
what 3.02 changed about it.

One fact makes this phase unusually tractable: the three installs differ **only** in `gs.mpq`
(`lomse.exe` and `imp.mpq` are byte-identical across all three), so script content is the entire
compatibility story between profiles.

## Phase 3 — Build and validation pipeline

All four commands exist ([build pipeline](build-pipeline.md)). The two install boxes are ticked for
the **act** as of 2026-09-18, not merely for the command: `Lords of Magic Development.app` exists
and the engine has loaded archives this pipeline built. Read the Phase 4 and Phase 5 sections below
for what those runs did and did not establish — each acceptance so far is **one member, replaced
rather than added, length-preserving**, in storage class `0x80010100` (`gs.mpq`, `pic.mpq`), plus
the `0x80010000` STORED class proven on 2026-09-19 (`sndfx.mpq`, `special.mpq`).

*(This paragraph asserted the opposite until 2026-09-19, contradicting both line 17 and the Phase 4
section of this same file. The profile's existence was verified directly before correcting it.)*

- [x] Create a clean source tree for our mod. `mods/<mod-id>/` with `mod.toml` and
      `archives/<archive>/…`; the path below the archive directory is the member name with `/`
      turned into `\`. Source files are gitignored -- they are game content -- and
      `scripts/mod-seed.sh` reconstructs them byte-for-byte from a local install.
- [x] Add deterministic MPQ creation or patching. Reuses `scripts/repack-archive.sh` rather than
      adding a second writer. Three runs of the `units\orinf.gs` build were byte-identical.
- [x] Validate duplicate IDs, missing references, invalid paths, encoding, and case mismatches.
      Thirteen checks, each with a severity and a `file:line:column`, and a published "what this run
      could not check" block with counts.
- [x] Produce a change report for every build. Old and new size and digest per member, and for
      `.gs` a token-, value- and symbol-level summary -- `hit_points: 13 -> 18` rather than "the
      bytes differ".
- [x] Install builds only into a third `Lords of Magic Development.app` profile. Enforced by an
      allowlist of exactly one directory (`tools/install_guard.py`), not by a check that the target
      is not the baseline. **The profile has not been created; the command's refusals are tested
      against fabricated directories.**
- [x] Add one-command rollback to the last known-good development build. `scripts/restore-dev.sh`,
      to pristine or `--to MOD_ID BUILD_ID`, verified against an independent record. **Never run
      against a real profile.**

Deliverable: `build`, `validate`, `install-dev`, and `restore-dev` commands. Delivered as
`scripts/mod-validate.sh`, `scripts/mod-build.sh`, `scripts/install-dev.sh`,
`scripts/restore-dev.sh`.

Two results from building it are worth carrying forward, because both refute something this file
previously implied.

**The shape check could not repack vanilla `gs.mpq` at all.** *Observed 2026-09-18*: StormLib
renumbers unnamed blocks when it rewrites an archive, and an unnamed member's only name is the
`File%08u.xxx` pseudo-name synthesised from its block index. Replacing `units\orinf.gs` produced an
archive with all 1,688 entries, all 372 unnamed slots and all 1,316 named members intact and exactly
one declared change -- and the check refused it as 4 added, 4 missing and 26 undeclared content
changes. `tools/mpq_shape.py` now compares unnamed members as a multiset of content identities and
keeps per-block addressing for named ones, which is what the PIC5R3 case needs. Phase 1's "byte-level
determinism measured" was true and its scope note -- "every number above is from GS5R3" -- was doing
more work than it looked like.

**The summary "GameScript uses bare CR line endings" is Refuted as a general rule -- though this
repository never said it.** *Observed 2026-09-18* across all 4,692 `.gs` members of the three
profiles, as an exclusive partition: **3,050 have no line ending at all**, 1,337 are pure CRLF, 193
mix CRLF with bare CR, 49 are pure bare CR, 40 mix CRLF with bare LF, 23 are pure bare LF. The
Phase 4 target is in the 3,050.

That survey was run to check a claim handed in from outside and instead **confirmed
[gamescript-format.md](gamescript-format.md#line-endings-bare-cr-is-a-line-ending-here) on all six
of its GS5R3 figures** (1,123 / 242 / 63 / 501 / 49 / 193) and its "3.02 has no bare CR at all". That
document had already said the counts overlap and are not a partition. Two independent measurements
agreeing on six counts is confirmation, so nothing there is corrected; what the wider run adds is
scope -- 1,696 members to 4,692 -- and the fact that **bare CR is a GS5R3 phenomenon only**, absent
from vanilla and 3.02 entirely.

That also sized the `gs_syntax.py` defect below, which was **fixed on 2026-09-18**. Measured by
comparing its token count against the same rule with CR treated as a terminator, **34 members lose
tokens, all 34 in GS5R3 and none in vanilla or 3.02** -- the 25 already recorded there are a subset.
The same measurement after the fix reports zero. Counting every member containing a bare CR would
give 242 and overstate the reach sevenfold.

Separately, the only control bytes anywhere in the corpus are TAB, CR and LF, and the 17 members
with a byte above 0x7e are **none of them valid UTF-8** -- so a validator demanding UTF-8 would
reject shipped members.

Prior art to build on rather than duplicate: `scripts/restore-game-archives.sh` already encodes the
repo's safety discipline — it refuses to run while `lomse.exe` is alive and always prints the hashes
it produced. `scripts/inventory-installed-profiles.sh` already produces the manifests a change
report would compare against. The validation bullet has a known hard case: PIC5R3 holds **two
distinct members under one byte-identical name**, `portrait\AIpotM.lbm`, so extraction yields 1,405
files from 1,406 entries on *any* filesystem. An earlier reading of this as a case-insensitivity
problem is **Refuted** — the two names are the same bytes, and an MPQ cannot hold two names
differing only in case at all. Any shape check built from extracted files, or from names alone,
therefore cannot tell 1,406 members from 1,405; it has to address members by block index
([MPQ inventory](mpq-inventory.md)). Name recovery does not dissolve this case and must not be read
as doing so: both members are already named, and **name → block is not injective**. It does reduce
PIC5R3's unnamed remainder from 409 to 1, which matters for the separate reason that block indexes
are unstable across a rewrite ([member names](member-names.md)).

## Phase 4 — First vertical slice

**COMPLETE. Observed in gameplay, 2026-09-18.** The engine loaded an archive this pipeline built,
and the change was read off the unit panel by a human.

The slice was `units\orinf.gs`, Order's Footmen, symbol `orinf`. It was chosen because it is the
one member class with real engine evidence behind it -- flags `0x80010100` (EXISTS | ENCRYPTED |
IMPLODE) in the baseline `gs.mpq`, the same compression class as the attended 2026-09-16 round trip.

| Success criterion | Result |
|---|---|
| The archive builds reproducibly | **Yes.** Four fresh packs, byte-identical. Build id is a digest of the tree, base archives and tools, not a timestamp. |
| Static validation passes | **Yes**, 0 findings -- and the run prints what it could not check. |
| The development profile launches | **Yes.** Cloned from the baseline in 3.8s for 13.9 MiB (APFS `clonefile`). |
| The changed behavior is visible in game | **Yes.** The unit panel reads `IRONGUARD`. |
| Existing baseline and mod profiles remain unchanged | **Yes.** Baseline `gs.mpq` still `6b84ea4c…`, `imp.mpq` `cb5c1068…` across all three, maps 354/354/366. |
| Rollback succeeds | **Yes**, tested before the engine run and verified against the pristine manifest rather than against the file it copied from. |

### The first attempt was inconclusive, and why

The slice originally changed `hit_points` from 13 to 26 only. The panel showed `90/90`, which
confirms nothing: it also showed **attack 10 and armor 6 where `orinf.gs` declares 7 and 5**, and
neither of those was touched. **The unit panel applies modifiers to declared statistics**, so a
displayed number is not a raw read of the record and cannot be predicted in advance. A measurement
whose expected value is unknown is not a measurement.

The change was re-cut against a field the panel prints **verbatim**: the unit's name,
`/name"Footmen"def` -> `/name"Ironguard"def`. There is no formula to invert, so the observation is
unambiguous. The build carried both changes, so the panel's `90/90` is now known to be the
**modified** value and vanilla reads half of it.

The lesson generalises and belongs with the others in this file: **pick an observable whose
expected value you can state before you look.** Choosing `hit_points` meant checking the subject
with an uncalibrated instrument.

### What this does and does not establish

**Established, Observed in gameplay:** the engine accepts an archive produced by
`scripts/mod-build.sh` -> `scripts/repack-archive.sh` for an `MPQ_FILE_IMPLODE` member of `gs.mpq`,
installed into a cloned profile, and applies the changed record. The full loop -- seed, edit,
validate, build, install, observe, roll back -- works end to end.

The rename was observed in **two independent places**: the selected-unit panel on the map, and the
**barracks recruitment panel**. That matters more than one sighting, because it shows the engine is
reading the changed record into the unit *type* rather than patching a single already-instantiated
army. The recruitment panel is also the obvious place to look for **undecorated** declared values,
since it describes a unit that does not exist yet and so has no level, leader or terrain modifier
applied -- which is what the map panel demonstrably does have. A future numeric slice should be read
there, not on the map.

**Not established by Phase 4.** This was one member, one archive, one compression class, and
nothing here says a *large* mod loads -- only that a correct one-member rewrite does. **Two of the
three limits this paragraph recorded have since fallen:** a size-changing edit was accepted on
2026-09-19 (the cheat-keys ladder, 21 bytes to 20) and a member *added* rather than replaced on
2026-09-20 (acceptance rung 5, `imp.mpq` grown to 3,601 members). Both are recorded in the rendered
per-archive regions below, which are the authority; this paragraph states what **Phase 4 alone**
established and is left standing for that reason.

<!-- The two blocks marked `engine-acceptance:<archive>` are RENDERED from
     tools/engine_acceptance.py. Do not edit them by hand: change the facts there and paste
     what `roadmap_paragraph("<archive>")` prints. tests/test_mod_pipeline.py requires each
     marked region to equal the render, for every archive with a run -- which is what keeps a
     claim here and the caveat in every build.json from drifting apart. -->
<!-- engine-acceptance:gs.mpq -->
**Not established.** 1 member of one `gs.mpq`, replaced rather than added, with a length-preserving edit made by the mod pipeline's own writer, 2026-09-16. The attended 2026-09-16 round trip of an MPQ_FILE_IMPLODE member of gs.mpq. 1 member of one `gs.mpq`, replaced rather than added, with a size-changing edit made by the mod pipeline's own writer, 2026-09-19. The cheat-keys ladder flipped a single token in a member of gs.mpq, which made the member shorter, and the debug hotkey tier it gates was then exercised in gameplay. The engine has not been shown a member added to an archive rather than replaced. The engine has not been shown a second member of the same archive in one build. The engine has not been shown any flag combination other than 0x80010100. The member carried flags 0x80010100 (EXISTS | ENCRYPTED | IMPLODE), which is the only storage class any run has covered.
<!-- /engine-acceptance:gs.mpq -->

The `pic.mpq` half of this paragraph was closed on 2026-09-18; see the `pic.mpq` slice under Phase 5.

## Phase 5 — Asset pipeline

- [x] Prove read-only native MPQ access and decode the observed IFF PBM corpus in a Rust/SDL3 viewer spike.
- [x] Inventory core-archive image/audio formats, palettes, dimensions, and candidate transparency metadata.
- [x] Parse all IMP sprite containers structurally and cross-check the common variants against generated headers.
- [x] Decode both observed IMP pixel-storage variants and display individual/animated frames.
- [x] Recover action/facing ranges and generated action names, navigate them, and export indexed PNG frames losslessly.
- [x] Parse the common header and cell grid of all installed `.scn`, `.smp`, and `.lgd` files and visualize candidate elevation.
- [x] Write maps back: an unedited map re-encodes byte-identically across all 365 installed files, with tile, terrain, elevation and placed-sprite editing on top. Unknown fields are copied, never minted. ([map format](map-format.md#writing-maps))
- [x] Resolve standard map tags through `.til` definitions and original terrain atlases; export coherent map previews.
- [x] Establish cell packing: cells are `y × width + x`, **Observed** from a probe on a non-square map, where every shipped map being square had hidden the transpose. The axis *names* remain **Inferred** and nothing downstream depends on them ([map format](map-format.md)).
- [x] Structurally decode and corpus-validate the dominant 49-byte placed-sprite record family.
- [x] Verify sprite origins and hotspot placement against the running engine ([issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1), closed 2026-09-16 — see [hotspots](hotspots.md)).
- [x] Decode the remaining map-tail variants. Six record layouts (47, 48, 49, 52 and 53 bytes, two with a footer); all 365 maps decode and 21,117 records rebuild from typed fields. The "18 unmatched tails" were nine files of each new size, not a separate phenomenon.
- [x] Measure which transition tiles `setterrain` blends into the 8-neighbourhood, and offer terrain painting rather than single-cell forcing (`--map-paint-terrain`). The blend rule turned out to be **shipped as data**: 26 `.til` tilesets declare per-tile 8-neighbour constraints, and all 1,258,496 cells of 365 maps resolve against them.
- [x] IMP placement write-back (`--set-imp-placement`, `--imp-placement-for`).
- [x] Bind `.smp` tilesets per encounter rather than per map class; 169 of 337 resolve and 168 honestly refuse.
- [x] A map editor drivable with a mouse (`--serve`), loopback-only, with terrain paint, undo and Save As.
- [x] Put a rewritten `pic.mpq` in front of the engine. **Observed in gameplay 2026-09-18** — see the section below. Re-run with the **full ByteRun1 encoder** and a member that *shrank* (302,432 → 256,996), **Observed in gameplay 2026-09-20** as acceptance rungs 3 and 4.
- [x] Put a rewritten `imp.mpq` in front of the engine. **Observed in gameplay 2026-09-20** — acceptance rungs 0+1 and 2. The repack is accepted and the engine renders pixels our IMP encoder wrote; see the `imp.mpq` section below. Until this run no IMP this project wrote had ever been read by the engine, because loose `.imp` files were measured on 2026-09-16 not to override MPQ members.
- [x] Add a member an archive never had. **Observed in gameplay 2026-09-20** — acceptance rung 5 took `imp.mpq` to 3,601 members and the engine ran it. Establishes **tolerance**, not readability: nothing in the game asks for the added member.
- [x] Resolve the IMP chroma key and the shadow blend against the game (2026-09-17). Shadow index 1 draws the background at half brightness as a **palette remap, not per-pixel arithmetic**, on 100% of the 903 and 889 pixels measured; palette entries are stored **B, R, G, pad**, not RGB, and the decoder was corrected. (**Corrected 2026-09-23:** they are stored **B, G, R, pad**, plain BGRA, and the 2026-09-17 decoder change was the error — it was fitted through a capture reader that swapped red and green; see the [research log](research-log.md#2026-09-23--imp-palettes-are-bgr-after-all-the-capture-reader-swapped-red-and-green).) Both are in [hotspots](hotspots.md). The `[ ]` box below is what is LEFT of issue #2, not the whole of it.
- [x] Decode IMP animation records ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2) partially — the timing premise it was filed on is **Refuted**; see the issue for the three-way split it became).
- [x] Prove candidate placed-sprite field behavior, and find out what the six layouts' constant tail words mean. **Observed in gameplay 2026-09-18** by a controlled save-diff with a human driving the Map Editor — macOS blocks synthetic input to Wine, but not a person. The `+24` attribute field is the encounter LEVEL in its high nibble; `+34` is an encounter reference with `0xffffffff` for none; the record count is a `u32` at tail offset 0; the footer counts nothing and is written by the editor. The tail-word question is answered from the other side: **a no-op load-and-save rewrites 53-byte records as 49-byte ones**, dropping four bytes each while preserving every modelled field and all 16,384 cells, so those bytes are not information the engine needs — and the caution that a non-49-byte record is Inferred is retired, because the editor normalises *to* 49. Still open: which registry assigns the `+34` id (Life 568, Death 569 — adjacent, so an ordered registry, but **Refuted** as the encounter catalogue). See [map format](map-format.md).
- [ ] Resolve the uncommon IMP metadata variants and anchor direction 0 to a compass bearing ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2)). **Two of this box's three jobs closed 2026-09-19; the bearing is the only part left, and it is the one that needs the running game.**
  - [x] **Wire the recovered `AnimRules` into the viewer.** `src/imp_playback.rs` joins the decoded sprite to the rules and `--view-imp` plays through them, replacing a walk of its own invention. Against the shipped archive, **Observed in the corpus** on 2026-09-19: 955 ping-pong sequences now reflect instead of jumping back, 5 one-shots hold their last frame instead of looping, and 2,234 five-facing sequences expose all eight directions — 7,810 synthesised directions across 2,304 sequences, drawn mirrored. 941 sequences change what the viewer shows; the other 19 of those 960 have a one-frame facing, where the rule applies and changes nothing visible. The viewer now requires the binary (`--exe`, or a `lomse.exe` beside the archive) and refuses to open without it rather than inventing an order.
  - [x] **Restate the acceptance criterion.** As filed it conflates two questions. The asset says which frame comes next; the engine's screen mode says how long to wait. The `.imp` genuinely carries no timing field (**Observed in a local binary**) and `AnimRules` determines order, end-of-cycle behaviour, reflection and mirrored facings only — but the interval *is* sourceable, from the engine rather than the asset. The tick period is the field at object `+0x228AC` (`idiv` at `0x004822FF`, `GetTickCount` via the IAT slot at `0x0054D0E8`), whose constructor default is 100 (`0x0047F7EA`) and which `gs/modeinfo.gs` overwrites per screen mode with **66 ms** for the map screens and **121 ms** for combat and location, user-adjustable in 11 ms steps clamped to 11..=330 (`gs/hotkey.gs`). The viewer takes its interval from there. **Refuted**: the old note that the period was unknown and lived near `0x005AF134` — that address is `combattime`, a counter used as the modulo dividend, not a period. Still **Inferred**, deliberately not promoted: which animation site runs under which screen mode was not traced, so "sprite playback at 66 ms" rests on the terrain-sprite driver at `0x50C334` being a map-screen site. See [imp-format](imp-format.md).
  - [ ] **Anchor direction 0 to a compass bearing.** Needs the running game. Two rotations sit between a GameScript-level facing and a stored index. The **stored** index's zero point is now anchored — direction 0 is the `.til` column `s`, drawn down-and-left ([map-format.md](map-format.md#the-screen-bearing-of-each-direction), 2026-09-21) — but the **GameScript-facing** zero point is not, and `stored = (arg + [0x5AEC3C]) mod 8` keeps them apart. A parallel pass **Refutes** `locationindirection` as the anchor and points instead at whatever builds the runtime neighbour table behind the pointer at `0x5AE970`; that lead is unverified here.
- [x] Build IMP **pixel** reimport and lossless repack tests. **2026-09-18.** PBM gained a ByteRun1 encoder and writer (#70). **Observed in the corpus**, re-measured 2026-09-19 by corpus-gated test rather than by the manual CLI sweep the older figures came from, and the archive is now named because there are two: the **stock** `pic.mpq` (the Steam build and `Lords of Magic Development`) holds 1,045 PBM members, all 1,045 pixel-lossless and 8 byte-identical; **GS5R3** ships a replacement `pic.mpq` holding 1,377, all 1,377 pixel-lossless and *also* exactly 8 byte-identical. Eight of each is low because the corpus has at least two original packers, which is a finding rather than a shortfall. (Both numbers were already in the repo — [native asset stage](native-asset-stage.md) said 1,045 "of vanilla", [agent handoff](agent-handoff.md) said 1,377 — each correct about its own archive, with nothing saying they were different archives.) IMP gained its own encoder and a single-frame writer that rewrites every absolute pointer: 41,373 of 41,373 frames pixel-lossless, and **37,930 whole-file rewrites attempted** — the other 3,443 are **refused and named** rather than silently overwritten, 3,439 whose pixels are shared by another record and 4 of zero length. Of those 37,930, **35,840 are byte-identical and 2,090 differ only because the frame's own payload re-encodes differently**, and those two categories account for all 37,930 with nothing left over — so **zero files changed while their payload did not**, which is the assertion that can actually fail. The old "35,840 of 35,840" is retired because it counted only the frames it claimed and so could not have come out any other way. Both sweeps assert non-BODY chunk preservation, so the corpus is the regression gate. Batch conversion is NOT done and is the remainder of this line.
- [ ] Evaluate AI upscaling on portraits and interface art.
- [ ] ~~Compare Lanczos runtime scaling against remastered source assets.~~ **Restated 2026-09-18: this box is not well-posed and was not run.** It presumes remastered source assets, which is the box above and has never been done, so there is nothing to compare against. More fundamentally the engine renders at a fixed 640x480 and interface art is 8-bit palettised at that size, so "remastered source assets" cannot mean higher-resolution art for an LBM — an asset cannot carry more detail than the render target the shader already receives. The answerable question is narrower and should replace it: **does a same-dimension asset authored for the scaler (cleaner palette mapping, less dither noise) survive Lanczos upscaling visibly better than the shipped one?** That needs one deliberately-authored asset first, which the PBM encoder now makes possible, and an attended side-by-side to judge. Recorded rather than quietly ticked.
- [x] Add automated dimension and palette validation. `tools/asset_validate.py` decodes every `.lbm` member a mod tree replaces and checks the IFF chunk walk, the BMHD's dimensions, planes and compression, the CMAP's size, and **every pixel index against the palette that member carries**; `tools/mod_validate.py` runs it and no longer claims those members are uninspected. Swept across the 3,463 images extracted from the three installed profiles' `pic.mpq`: zero errors.
  (3,467 image entries exist and 3,464 carry a name; extraction yields 3,463 files because GS5R3's
  `pic.mpq` holds `portrait\AIpotM.lbm` twice under one byte-identical name. All three counts are
  correct and `tools/asset_validate.py` states which is which.) Two rules were measured rather than assumed, and both would have rejected shipped content otherwise -- ByteRun1 rows are padded to an even byte count (88 of the stock archive's 1,045 images are odd-width and decode only under that rule — and **exactly 88** of GS5R3's 1,377 are too, so the odd-width population is stable across both archives; **Observed in the corpus**, re-measured 2026-09-19), and a single trailing 0x00 in the BODY is the chunk's even-size pad (65 images have one). The one member that still draws a warning, GS5R3's `PORTRAIT/decr5p00.lbm`, is a genuine defect in shipped art: 30 packets overrun their rows from row 36 on, 1,353 pixels are discarded by the clamp that makes it render, and the last 531 bytes of its BODY are unreachable.

### The `pic.mpq` slice, Observed in gameplay 2026-09-18

The engine read an archive this pipeline built from `pic.mpq`, and a human read the change off the
screen. This was the largest open item the roadmap named, and closing it needed a correction first.

**The stated risk was the wrong one.** The roadmap said the compression choice for a `pic.mpq`
replacement was **Inferred**. It is not, and one histogram settled it: every one of the 1,071 members
of the baseline `pic.mpq` carries flags `0x80010100` (EXISTS | ENCRYPTED | IMPLODE) -- the *same*
storage class as the `gs.mpq` member Phase 4 proved. `imp.mpq` is the same again; `sndfx.mpq` and
`special.mpq` are `0x80010000`, stored rather than imploded.

**The real blocker was addressing, and it was total.** `pic.mpq` has no `(listfile)` and **no
self-named member at all**, so every member lists under a `File%08u.xxx` pseudo-name, and both ways
round it fail. Naming a real member is refused for not being in the archive's own catalogue. Naming
the pseudo-name reaches `SFileAddFileEx`, which rejects it with **StormLib error 22** -- the
pseudo-name resolves by *block position* and nothing ever hashes it into the hash table. Until the
names recovered in PR #66 were threaded through the pipeline, **`pic.mpq` could not be packed at
all**, and no amount of care about compression would have found that.

The fix is `--listfile` on `lom-mpq repack`, threaded by `scripts/repack-archive.sh` through the
repack *and both manifests* -- naming one side and not the other would compare two different
addressings of one archive. `gs.mpq` is deliberately left alone: it carries its own `(listfile)`,
and supplying recovered names for its 372 unnamed entries would re-address them inside the shape
check, changing the exact path Phase 4 proved.

Measured on the baseline: **1 of 1,071 members changed, block index 141 preserved, no `(listfile)`
injected** (the entry count holds at 1,071), and three repacks byte-identical.

| Success criterion | Result |
|---|---|
| The archive builds reproducibly | **Yes.** Three packs, byte-identical. |
| Static validation passes | **Yes**, 0 errors; the one warning is the engine-acceptance notice this run retires. |
| The changed image is visible in game | **Yes.** The top third of the main menu renders spattered red. |
| The change is confined to the declared member | **Yes.** `lbm\start01.lbm`, a different member of the same rewritten archive, renders correctly. |
| Baseline, 3.02 and GS5R3 unchanged | **Yes.** `pic.mpq` `d0df8b92`, `d0df8b92`, `5c784a67`. |
| Rollback succeeds | **Yes**, tested before the engine run, and afterwards verified by extracting the member and comparing bytes to the pristine seed. |

**How the expected value was fixed in advance.** Phase 4's lesson was to pick an observable whose
value can be stated before looking. Here it was not *stated*, it was **rendered**: the build's
`pic.mpq` digest is byte-identical to an archive our own decoder had already exported to PNG, so the
screen was compared against a picture rather than against a description. The edit also carries its
own control -- rows 160 and below are untouched, so "red top, correct bottom" is a different
observation from "the image is broken" and the two cannot be confused.

**The mechanism, and why the edit looks like that.** There is no PBM or LBM *encoder* in this
repository, so an edit that has to re-compress could not be made. `BMHD.compression` is 1, ByteRun1,
which leaves exactly one opening: a *repeat* packet is two bytes standing for up to 128 pixels, so
rewriting the second of them repaints those pixels and the file keeps its byte count.
`tools/pbm_patch.py` does only that, refuses to split a run straddling the edge, and never touches a
literal packet. The repainted region therefore has a ragged, speckled edge. That is the shape of the
mechanism, not damage, and it was visible in the render before it was visible on screen.

**A naming trap worth recording.** `lbm\newgame.lbm` is the **main menu**, not a screen shown during
new-game setup. The name means "the new-game menu". An instruction to the human observer based on
reading the image rather than on knowing where the engine draws it was wrong about where to look;
the observation succeeded anyway because the main menu is the first thing drawn.

<!-- engine-acceptance:pic.mpq -->
**Not established.** 1 member of one `pic.mpq`, replaced rather than added, with a length-preserving edit made by tools/pbm_patch.py, 2026-09-18. The engine read an archive this pipeline built from pic.mpq, and a human read the change off the screen. 1 member of one `pic.mpq`, replaced rather than added, with a size-changing edit made by the ByteRun1 PBM encoder, 2026-09-20. The acceptance ladder re-encoded the main-menu backdrop with the ByteRun1 encoder, once with the shipped pixels and once with a filled rectangle; the member shrank, its TINY thumbnail was dropped, and the observer read the straight-edged rectangle off the screen with the surrounding art intact. The engine has not been shown a member added to an archive rather than replaced. The engine has not been shown a second member of the same archive in one build. The engine has not been shown a TINY thumbnail regenerated rather than dropped, which needs a downscaler this repo does not have. The compression choice is not Inferred: all 1,071 baseline members carry flags 0x80010100, the same storage class the gs.mpq run proved.
<!-- /engine-acceptance:pic.mpq -->

### The audio archives, Observed in gameplay 2026-09-19

The acceptance ladder's rungs 6-9 put `sndfx.mpq` and `special.mpq` in front of the engine for the
first time. These are the **`0x80010000` STORED** storage class -- every member of both archives --
which no earlier run had covered, and each rung rewrote **two** members in **both** archives in one
build. Rung 7 gave each archive a different tone and the listener named the one the engine played;
rung 8 exchanged the two tones and the report flipped, which is what makes it a measurement rather
than a reading. Both members read from `sndfx.mpq`.

<!-- engine-acceptance:sndfx.mpq -->
**Not established.** 2 members of one `sndfx.mpq`, replaced rather than added, with a length-preserving edit made by the WAVE importer, 2026-09-19. The acceptance ladder wrote a tone into both audio archives and the listener named which archive the engine had opened; exchanging the two tones flipped the report. The engine has not been shown an edit that changes a member's size. The engine has not been shown a member added to an archive rather than replaced. The members carried flags 0x80010000 (EXISTS | STORED), a storage class no earlier run had covered.
<!-- /engine-acceptance:sndfx.mpq -->

<!-- engine-acceptance:special.mpq -->
**Not established.** 2 members of one `special.mpq`, replaced rather than added, with a length-preserving edit made by the WAVE importer, 2026-09-19. The acceptance ladder wrote a tone into both audio archives and the listener named which archive the engine had opened; exchanging the two tones flipped the report. The engine has not been shown an edit that changes a member's size. The engine has not been shown a member added to an archive rather than replaced. The members carried flags 0x80010000 (EXISTS | STORED), a storage class no earlier run had covered.
<!-- /engine-acceptance:special.mpq -->

### `imp.mpq`, Observed in gameplay 2026-09-20

The acceptance ladder's rungs 0-5 put `imp.mpq` in front of the engine for the **first time**. It
was the largest wholly-untouched archive, and the whole sprite pipeline was offline-only because of
it. Three things were settled in one sitting, each with its own control.

**Rung 0+1, the repack control.** `iface\cursors.imp` was re-emitted by our IMP pixel encoder,
asserted byte-identical to the shipped member, and the archive repacked under the recovered names.
The observer reported the shipped pointer unchanged -- so StormLib rewriting this archive leaves one
the engine can still read, and the bytes it read came out of `write_frame_pixels`.

**Rung 2, the pixel change.** Eight disjoint palette-index swaps repainted the gauntlet white at
unchanged payload length (`shift=0`, the member still 90,000 bytes), leaving outline and silhouette
alone so that "wrong colour", "wrong shape" and "unchanged" are three different observations. The
observer reported the change. That is the engine reading our IMP pixels.

**Rung 5, a member the archive never had.** `iface\ladder.imp` was added alongside rung 2's
repaint, and the repainted pointer still drew -- so the hash table, the block table and all 3,600
original members survived the archive growing by one.

**The limit rung 5 leaves, stated plainly.** Rung 5 establishes that the engine **tolerates** an
added member, not that it can **read** one: nothing in the game asks for `iface\ladder.imp`, and no
observation on screen could. What is established offline is that `lom-mpq probe-names` resolves that
name through the archive's own hash table -- the same lookup Storm performs -- and that the member
reads back as the bytes it was added from.

⚠️ **Readability was already established by a different run, and this project published the
opposite for three days.** The 2026-09-17 sprite-ladder probe added `imp\zzpal.imp` and
`imp\zzctl.imp` -- names no shipped archive holds, confirmed 2026-09-20 by `read_member` failing on
the pristine archive while the donor reads fine -- and a script asked for one **by name** through
`addterrainspritetype`. The engine registered a terrain sprite type from it and **painted its
pixels**: that is how the stored palette order was measured, because the authored entries
(`ff 00 00` rendering blue) exist in no other member. (**Corrected 2026-09-23:** the order that
measurement gave was wrong — it was read through a capture reader that swapped red and green; see the
[research log](research-log.md#2026-09-23--imp-palettes-are-bgr-after-all-the-capture-reader-swapped-red-and-green). That the engine read the added member stands.) So the engine **can** read an added member.
What rung 5 adds is that one added by the **mod pipeline** is tolerated; the 09-17 member was added
with `mpq_replace`, and no single run has yet done both. This is the third time a limit was
republished after a run had refuted it, which is why the record derives from the runs.

<!-- engine-acceptance:imp.mpq -->
**Not established.** 2 members of one `imp.mpq`, added rather than replaced, with a length-preserving edit made by the IMP pixel encoder, 2026-09-17. The sprite-ladder probe added two members no shipped archive holds and a script asked for one of them BY NAME; the engine registered a terrain sprite type from it and painted its pixels, which is how the stored palette order was measured -- the authored entries exist in no other member. (The order it gave was wrong: the capture reader swapped red and green, as the research log's later correction records.) 1 member of one `imp.mpq`, replaced rather than added, with a length-preserving edit made by the IMP pixel encoder, 2026-09-20. The repack control: the cursor member was re-emitted by the IMP encoder byte for byte, the archive was repacked under the recovered names, and the observer reported the shipped pointer unchanged. 1 member of one `imp.mpq`, replaced rather than added, with a length-preserving edit made by the IMP pixel encoder, 2026-09-20. Disjoint palette-index swaps repainted the cursor frame at unchanged payload length, and the observer reported the pointer had changed colour. 2 members of one `imp.mpq`, added rather than replaced, with a length-preserving edit made by the IMP pixel encoder, 2026-09-20. A member no shipped archive holds was added alongside the repainted cursor; the archive grew, and the observer reported the repainted pointer still drawn, so every original member survived the hash and block tables growing. The engine has not been shown an edit that changes a member's size. The engine has not been shown an added member reaching the engine through the MOD PIPELINE and being read; the 2026-09-17 probe added its member with mpq_replace, and rung 5's pipeline-built member was never asked for by anything in the game. The member carried flags 0x80010100 (EXISTS | ENCRYPTED | IMPLODE), which is the only storage class any run has covered.
<!-- /engine-acceptance:imp.mpq -->


Note that painting has never been verified by a probe loading a painted map, and the core tile
family, road in either role, and painting across an existing boundary are refused rather than
approximated.

## Capability answers, 2026-09-21

Three questions were asked directly and answered against the binary and the corpus rather than by
estimate. Each has its own document; the headlines:

- **[Creating new units](new-units.md) — yes, and the cap everyone assumes does not exist, though
  a different one, at 1000, does.** The unit-type table is heap-allocated at a size the *script*
  picks (`200 maxunittypes` in `gs\unittype.gs`; **155 slots used in vanilla and 3.02, 160 in
  GS5R3** — name the profile); the allocator's only bounds are `count >= 1` and `used < capacity`,
  and a raise needs `/unittypedict N dict` moved in lockstep or the 201st definition raises
  "Dictionary Full". 🔴 **The real ceiling is 1000**, imposed by a fixed 1000-dword stack histogram
  at `0x0052BE20` that indexes on the raw type value with no bounds check, and unlike every other
  limit here it fails **silently**. The real constraint is that this project has an IMP frame **repainter**, not
  an IMP **author**, so a new unit's art must be a repainted clone of a donor. Also established:
  `pic.mpq` **does** accept a brand-new member name at the repack layer, which had been recorded as
  the weakest link in the chain.
- **[Screen resolution and upscaling](resolution-and-upscaling.md) — 2x art will not work, and the
  blocker is absolute.** `SetDisplayMode(640, 480, 16)` is two `push` immediates at `0x00475263`
  with no config key, no registry value and no command-line switch; `push imm32 600` and
  `push imm32 768` occur **zero** times in the image. The formats would mostly take the art; the
  renderer will not use it.
- **[The engine rewrite](native-engine-plan.md#decision-status) — still a candidate, and this
  document's own estimates are measurably wrong.** Stages 0 and 1 closed in 9 days against a
  2-6 week estimate that assumed full-time work. The recommendation is to finish the Stage 2 module
  loader and re-run the execution census first, because the 25x gap between "operators named by a
  script" and "operators execution stands on" is the single input that decides the whole question.

## Phase 6 — Larger mod direction

After the toolchain and one vertical slice are proven, select a product direction:

- Near-vanilla restoration and bug-fix continuation.
- A new balance model derived from GS5R3.
- New Legends of Urak campaigns.
- A visual/audio remaster compatible with multiple gameplay profiles.
- A curated expansion combining new maps, quests, artifacts, and encounters.

## Format and engine reverse-engineering

This track was never in the original plan. It is recorded here because it is a large part of what
the repository now contains, and because pretending it was always a phase would misrepresent how the
project actually went.

**Every *archived* file format is now decoded.** `.smp`, `.scn` and `.lgd` are one format; `.til`
tilesets, `.lbm` images, `.imp` sprites, `.gs` scripts and the MPQ container all have native
readers; the audio archives are plain WAVE and `.smk` is Smacker. The savegame container closed in
PR #60 ([savegame format](save-format.md)).

That claim used to read "every file format outside the executable", and it was written on the
strength of the archive work alone. A [loose-file sweep](loose-files.md) found the **loose** tree
had never been enumerated. Most of what it turned up closed on 2026-09-19:

- **`map/e3map2.map`** — **decoded**. It is the map format *without* its leading version word, and
  the engine proves the relationship rather than a resemblance: `loadscenariomap` reads one `u32`
  and then calls the whole of the grid reader `0x004a52e0`, which `loadmap` calls on its own. It
  parses, re-encodes byte for byte, and `--scan-map-dir` counts it.
- **`English/Text/Menu/Menu_En.asr`** — **decoded, with a parser and an emitter**. This is the Asura
  string table behind the Steam launcher, and it belongs to `LOMLauncher.exe`, which is now
  Observed in that binary rather than inferred from the file. Its hash function is recovered
  (`LOMLauncher.exe` `0x004018b0`), so a new key can be *added*, not only an existing string
  replaced.
- **The trailing word of `lom.cfg`** — **decoded**: the used-DrawBlt flag, global `0x5d20bc`, read
  by `loadconfig`'s last `fread` at `0x004874d3`.
- **What writes `settings.cfg`** — **answered**: `gs\dlg\opdlg.gs` in the **3.02 patch's**
  `gs.mpq`, not the shipped engine. It opens the file `"w"`, so the whole file is rewritten, and
  terminates each record with `carriage_return`. The earlier negative was true but measured on the
  vanilla archive, which is one game short of the question.

One loose format is still open, and it is a different file from the Asura table:
**`English/custldr/0templdr.ldr`** (GS5R3 only), the custom-leader editor's scratch save. What it
is, what writes and reads it, and its outer container are established; the 3,294-byte army object
inside it is not decoded, and with exactly one `.ldr` file across four profiles no layout for it
could be validated against a corpus.

- [x] Recover the engine's operator dispatch table (1,906 operators) and their arity.
- [x] Read the operator bodies ([native operator bodies](native-operator-bodies.md)).
- [x] A bounded GameScript VM slice, with a standard-library corpus. The *next* slice is still open as [issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5).

  **Is the 33.1% execution figure profile-scoped? Checked 2026-09-19 — no, and the question was worth asking.** The `standard.gs` conformance battery beside it *was* effectively 3.02-only: it defaulted to `LOM_GS_PROFILE=patch302` and tolerated four "declared disagreements" on GS5R3 and two on vanilla, so a headline measured only there would have been a real hazard. The **execution census is a different instrument** and was not affected. `--survey` loads every `.gs` member and records what stopped it, independently of the battery, and [the census](gamescript-format.md#the-execution-census) already publishes the per-profile split rather than one number: vanilla 429 of 1,315, patch302 635 of 1,681, gs5r3 487 of 1,696, totalling **1,551 of 4,692 (33.1%)**. Re-running all three on 2026-09-19 reproduced 429 / 635 / 487 exactly, and the committed `reports/gs/standard-run-*.tsv` carry the same figures. So the coverage claim is **already scoped across all three profiles and stays as written** — this is a confirmation, not a correction. The battery itself was fixed separately and is now green on all four installs with no environment variable.
- [x] Decode the savegame container ([savegame format](save-format.md)). Five of nine sections decode completely; four carry their undecoded bytes verbatim rather than guessing.
- [x] Establish how multiplayer works and why it desyncs ([multiplayer](multiplayer.md)). It is lockstep-deterministic, the shipped desync post-mortem is **gated off at load**, and the asterisk in the game list already means the host's build differs from yours. A home server cannot fix desync, because the problem is determinism rather than the network.
- [ ] Measure difficulty-dependent computer AI behavior in controlled games ([issue #6](https://github.com/jake-bliss/lords-of-magic-modding/issues/6)).

This list recorded three known latent defects. **All three are now fixed, and the list was itself
stale when checked on 2026-09-18** — two of the three had been corrected in code on 2026-09-17 and
only this page still said otherwise:

- `MapCell::tile_index()` in `src/map.rs` — **fixed 2026-09-17.** It masks `self.tag & 0xffff`; the
  claim that it still masks `tag & !0x00800000` was stale. The upper half of the word is a separate
  sixteen-bit field, measured from the engine's bounds-checked tileset lookup rather than from the
  corpus, which cannot distinguish the two rules.
- The map editor's `set_tile` — **fixed 2026-09-17.** It writes
  `(cell.tag & CELL_TAG_UPPER_FIELD) | tile_index`, preserving the whole upper field rather than one
  bit of it. Latent either way: across 353 shipped maps and 1,040,384 cells that field takes only
  `0x0000` and `0x0080`.
- **`tools/gs_syntax.py` diverged from the authority in five places** — **all five fixed
  2026-09-18.** Only the first was recorded here; the other four were found by reading the two
  lexers side by side after it, which is the reason this entry is now a list. (1) A `;` comment
  ended at `\n` only, though bare CR is a line ending, so in a member with no LF the first comment
  swallowed the rest of the file: **34 members**, every one in GS5R3, 25 of them with no LF at all,
  and `gs\dungeons\water\wacave.gs` normalised to **six tokens** against its real 712 (it now
  yields 712, the count `--gs-facts` reports). (2) `\` was treated as a string escape, which the
  authority's `read_string` has no rule for, so a string ending in a backslash ate its own closing
  quote: **5 members**, in **all three profiles** — vanilla's `gs\Dlg\lib_dlg.gs` holds the
  punctuation table `"@#${}()[]\"` and lexed to 3,247 tokens against its real 2,324. That one
  refutes the earlier claim on this page that the defect class could not reach a mod built against
  `vanilla`. (3) `/` did not end a name, though `is_separator` lists it: **5 members**. (4)
  `str.isspace()` is wider than `is_ascii_whitespace` — it also accepts ASCII `\x0b` and
  `\x1c`-`\x1f`: **0 members**, and the one member holding such a byte holds it inside a string
  literal, where no layout rule looks. (5) `(` and `)` were delimiters here and are ordinary name
  bytes to the authority, which lists no parenthesis in `is_separator`, so `foo(1)` was four tokens
  here and one there: **0 members** — 530 hold a parenthesis and in every one of them it is inside
  a string or a comment. The last two were fixed despite measuring zero, because a disagreement
  about the grammar is a defect whether or not the shipped corpus exercises it. Measured over all 4,692 `.gs` members of the three profiles
  before and after, with a port of the Rust rules validated against `--gs-facts` on 4,692 of 4,692
  first: **after the five fixes, zero members tokenize differently.** Zero of the 4,692 produced a
  parse error, so none dropped out of the comparison unseen. `reports/gs/summary.md`
  regenerates byte-identical throughout — token hashes moved, **no member changed status**. What is
  left is listed in
  [the lexer parity section](gamescript-format.md#lexer-parity-the-two-tokenizers-and-what-still-separates-them):
  `<` and `>` end a name for the authority and not here, which no corpus member reaches and which
  cannot be closed without an error channel this tokenizer does not have. That one also retired a
  "checked, not assumed" claim elsewhere in that document, which had read the corpus's habit of
  whitespace-separating `<<` as evidence that both lexers treated it as a delimiter.

**Fixed since:** `gamescript.rs` no longer lexes the shipped infantry unit code `INF` as
floating-point infinity. Classification was `name.parse::<f64>().is_ok()`, and Rust accepts `inf`,
`INF`, `nan` and `INFINITY` case-insensitively with an optional sign; it is now
`gamescript::is_number_token`, which models PostScript's integer and real forms instead. `INF` and
`CAV` stand side by side in `/unit_code_strings["INF""MIS""CAV"...` and now report the same use
count in every profile — 32 in vanilla, 35 in 3.02, 38 in GS5R3 — where `reports/gs/vocabulary-vanilla.tsv`
previously listed `CAV` and had no `INF` row at all. The eight infantry units per profile whose
`code` field read as a number now read as a name. Published vocabulary and gameplay counts moved
accordingly and the reports are regenerated. Evidence class: Corrected.

## AI-assisted workflow

AI can accelerate schema discovery, documentation, transformation scripts, consistency checks, dialogue drafts, asset processing, and test design. All generated changes should remain reviewable as diffs and should be validated in game. Balance decisions require human playtesting and should not be accepted solely from model output.

Two practices have earned their place and should be treated as defaults:

- **Cross-model review before anything merges.** Claude and Codex miss different things, and the
  disagreements are adjudicated by checking the disputed line rather than by stapling two lists
  together. This has repeatedly caught defects that a single-model pass declared clean.
- **Mutation testing.** A test that cannot fail is worse than no test, because it reports safety.
  Seven such tests have been removed from this repository. Mutate constants in **both** directions;
  a one-direction sweep reports a clean result it has not earned.

The recurring failure mode worth naming: **a bounded negative is only as strong as the instrument
that searched.** Claims of the form "we searched and found none" have been wrong here repeatedly —
a misaligned decode, a search for the wrong instruction form, a `strings | grep` that missed five
literals a raw byte scan found. The fix is never a better pattern; it is a **counter** for what the
search could not reach, plus one cross-check that does not share the mechanism.

## Parallel preservation-engine investigation

A native 64-bit reimplementation is a separate, gated track rather than a prerequisite for modding.
The [candidate plan](native-engine-plan.md) begins with reusable archive/asset tools, then uses a
GameScript VM spike to decide whether a faithful engine is economically reasonable.

That gate has moved. The first stage is done — archive, image, sprite, tileset, map and savegame
readers all exist natively — and the GameScript VM spike is no longer hypothetical: there is a
running VM slice, the full operator table with arity, and analysis of the operator bodies. The
decision the plan defers to is now closer to answerable than it was, and the honest blocker is no
longer capability but **scope**: the engine's behaviour is in 1,906 operators and the rendering,
combat and AI systems around them, none of which the asset work touches.

This remains gated. It should not be allowed to consume the effort that Phases 3 and 4 need.
