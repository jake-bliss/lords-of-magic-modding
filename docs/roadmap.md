# Roadmap

## Goal

Create a reproducible, source-controlled modding toolkit that lets us inspect, change, validate, package, and test Lords of Magic data without editing a playable installation by hand.

## Where we actually stand

Last reconciled against the tree at `1d1e406` (PR #60). Before that, this file had not been updated
since PR #51 and did not describe nine merged pull requests.

| Phase | State |
|---|---|
| 1 — Archive inventory | **Complete.** The repack command landed with a block-index shape check. |
| 2 — Script documentation | 2 of 3 boxes. The semantic symbol database and its searchable reference are open. |
| **3 — Build and validation pipeline** | **Not started.** |
| **4 — First vertical slice** | **Not started.** |
| 5 — Asset pipeline | Substantially done; see the boxes below for what remains. |
| 6 — Mod direction | Correctly gated on 3 and 4. |
| Format and engine reverse-engineering | Far ahead of what this document ever planned. See the section below. |

**Phases 3 and 4 are the critical path and nothing else is.** Phase 4 exists to force the complete
workflow into being — build reproducibly, validate, install to a development profile, observe the
change in game, roll back. Everything in Phases 1 and 5 is input to it.

The honest summary is that the project has enormous read capability and no delivery pipeline. Maps
can be decoded and rewritten byte-exactly across all 365 installed files, archive write-back is
proven against the running engine, and there is still no repeatable way to get a change into a
profile that is not a live install.

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

**Phase 1 is complete.** Two measured limits are recorded rather than papered over: StormLib
regenerates `(listfile)` on every write so its bytes cannot be preserved — the single exemption,
safe only because every member name is verified individually — and compaction is off by default
because `SFileCompactArchive` fails with `ERROR_UNKNOWN_FILE_NAMES` on any archive holding unnamed
members. Archives are copied-then-patched rather than rebuilt from an extracted tree, because 409
PIC5R3 members have no name and **the name is part of the encryption key**.

Shape preserved is not the same as playable. The only engine evidence remains the attended
2026-09-16 round trip, and that was an `MPQ_FILE_IMPLODE` member of `gs.mpq`; the compression choice
for a `pic.mpq` replacement is **Inferred** and has never faced the engine.

Delivered: [MPQ inventory](mpq-inventory.md), tracked comparison summaries, and reproducible extraction commands.

## Phase 2 — Script documentation

- [x] Infer and implement the first corpus-wide lexical model.
- [x] Inventory executable names, literal definitions, and static `run` references across all three profiles.
- [ ] Build a semantic symbol/index database for units, spells, artifacts, buildings, factions, and encounters.
- [ ] Compare vanilla behavior with 3.02 fixes and GS5R3 changes.
- [ ] Document identifiers, references, ranges, defaults, and likely hard limits.
- [ ] Mark uncertain interpretations and attach evidence examples.

Deliverable: a searchable gameplay-data reference with annotated examples.

Work since this phase was written went deeper than it in a different direction — the engine's
1,906-operator dispatch table, recovered operator arity, the operator bodies, and a GameScript VM
slice. That is language and runtime understanding. The phase asks for something else: a
**gameplay-data** index, so that a person can ask what a unit costs, where a spell is defined, and
what 3.02 changed about it.

One fact makes this phase unusually tractable: the three installs differ **only** in `gs.mpq`
(`lomse.exe` and `imp.mpq` are byte-identical across all three), so script content is the entire
compatibility story between profiles.

## Phase 3 — Build and validation pipeline

**Not started.** No `build`, `validate`, `install-dev` or `restore-dev` exists, and no
`Lords of Magic Development.app` profile has been created.

- [ ] Create a clean source tree for our mod.
- [ ] Add deterministic MPQ creation or patching.
- [ ] Validate duplicate IDs, missing references, invalid paths, encoding, and case mismatches.
- [ ] Produce a change report for every build.
- [ ] Install builds only into a third `Lords of Magic Development.app` profile.
- [ ] Add one-command rollback to the last known-good development build.

Deliverable: `build`, `validate`, `install-dev`, and `restore-dev` commands.

Prior art to build on rather than duplicate: `scripts/restore-game-archives.sh` already encodes the
repo's safety discipline — it refuses to run while `lomse.exe` is alive and always prints the hashes
it produced. `scripts/inventory-installed-profiles.sh` already produces the manifests a change
report would compare against. The validation bullet has a known hard case: PIC5R3 holds **two
distinct members under one byte-identical name**, `portrait\AIpotM.lbm`, so extraction yields 1,405
files from 1,406 entries on *any* filesystem. An earlier reading of this as a case-insensitivity
problem is **Refuted** — the two names are the same bytes, and an MPQ cannot hold two names
differing only in case at all. Any shape check built from extracted files, or from names alone,
therefore cannot tell 1,406 members from 1,405; it has to address members by block index
([MPQ inventory](mpq-inventory.md)).

## Phase 4 — First vertical slice

**Not started, and it is the project's real proof point.**

Choose one deliberately small change that touches the complete workflow. Good candidates:

1. Rebalance one clearly underpowered unit and update every displayed value.
2. Correct an artifact behavior plus its tooltip.
3. Add one new dungeon encounter using existing creatures and rewards.
4. Add a small custom map with a scripted scenario hook.

Candidate 4 is the one the toolchain is most over-equipped for: the map writer re-encodes all 365
installed maps byte-identically, object editing works on all 365, and the engine has been confirmed
to accept maps we write ([PR #47](https://github.com/jake-bliss/lords-of-magic-modding/pull/47)).
The missing pieces are all Phase 3.

Success criteria:

- The archive builds reproducibly.
- Static validation passes.
- The development profile launches.
- The changed behavior is visible in game.
- Existing baseline and mod profiles remain unchanged.
- Rollback succeeds.

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
- [x] Decode IMP animation records ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2) partially — the timing premise it was filed on is **Refuted**; see the issue for the three-way split it became).
- [ ] Prove candidate placed-sprite field behavior, and find out what the six layouts' constant tail words mean — which the corpus cannot answer, since each is constant within its layout ([issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4)). A record minted into any layout but the 49-byte one is Inferred and has never been handed to the engine.
- [ ] Resolve uncommon IMP metadata variants and verify chroma keys and the shadow blend against the game ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2)).
- [ ] Build batch conversion, IMP **pixel** reimport, and lossless repack tests.
- [ ] Evaluate AI upscaling on portraits and interface art.
- [ ] Compare Lanczos runtime scaling against remastered source assets.
- [ ] Add automated dimension and palette validation.

Note that painting has never been verified by a probe loading a painted map, and the core tile
family, road in either role, and painting across an existing boundary are refused rather than
approximated.

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

**Every file format outside the executable is now decoded.** `.smp`, `.scn`, `.lgd` and `.map` are
one format; `.til` tilesets, `.lbm` images, `.imp` sprites, `.gs` scripts and the MPQ container all
have native readers; the audio archives are plain WAVE and `.smk` is Smacker. The last gap, the
savegame container, closed in PR #60 ([savegame format](save-format.md)).

- [x] Recover the engine's operator dispatch table (1,906 operators) and their arity.
- [x] Read the operator bodies ([native operator bodies](native-operator-bodies.md)).
- [x] A bounded GameScript VM slice, with a standard-library corpus. The *next* slice is still open as [issue #5](https://github.com/jake-bliss/lords-of-magic-modding/issues/5).
- [x] Decode the savegame container ([savegame format](save-format.md)). Five of nine sections decode completely; four carry their undecoded bytes verbatim rather than guessing.
- [x] Establish how multiplayer works and why it desyncs ([multiplayer](multiplayer.md)). It is lockstep-deterministic, the shipped desync post-mortem is **gated off at load**, and the asterisk in the game list already means the host's build differs from yours. A home server cannot fix desync, because the problem is determinism rather than the network.
- [ ] Measure difficulty-dependent computer AI behavior in controlled games ([issue #6](https://github.com/jake-bliss/lords-of-magic-modding/issues/6)).

Two known latent defects are recorded rather than fixed, because each needs its own corpus
re-verification: `MapCell::tile_index()` in `src/map.rs` still masks `tag & !0x00800000` rather than
`tag & 0xffff`, which agrees on every shipped map only because the high bits there take just two
values; and the map editor's `set_tile` preserves a bit it should not, which is byte-correct on
every shipped map and wrong in general.

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
