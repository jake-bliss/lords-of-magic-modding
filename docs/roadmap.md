# Roadmap

## Goal

Create a reproducible, source-controlled modding toolkit that lets us inspect, change, validate, package, and test Lords of Magic data without editing a playable installation by hand.

## Phase 1 — Archive inventory (initial pass complete)

- [x] Select and validate StormLib for read-only cross-platform MPQ extraction.
- [x] Extract `gs.mpq` and `pic.mpq` from baseline, 3.02, and GS5R3.
- [x] Preserve archive paths and listfile names where available.
- [x] Produce machine-readable manifests containing path, size, compressed size, flags, locale, and content hashes.
- [x] Diff all three extracted trees and distinguish formatting/comment-only changes from token-level script changes.
- [x] Recover the public Lords of Magic filename catalog and apply it without modifying archives.
- [x] Classify every member in the five core GS5R3 archives by detected format.
- [x] Prove archive write-back: `SFileAddFileEx` round-trips a `gs.mpq` member and the game executes the rewritten archive (2026-09-16).
- [ ] Wrap that in a deterministic, reproducible repack command with a byte-shape check before any development-profile install.

Delivered: [MPQ inventory](mpq-inventory.md), tracked comparison summaries, and reproducible extraction commands.

## Phase 2 — Script documentation

- [x] Infer and implement the first corpus-wide lexical model.
- [x] Inventory executable names, literal definitions, and static `run` references across all three profiles.
- [ ] Build a semantic symbol/index database for units, spells, artifacts, buildings, factions, and encounters.
- Compare vanilla behavior with 3.02 fixes and GS5R3 changes.
- Document identifiers, references, ranges, defaults, and likely hard limits.
- Mark uncertain interpretations and attach evidence examples.

Deliverable: a searchable gameplay-data reference with annotated examples.

## Phase 3 — Build and validation pipeline

- Create a clean source tree for our mod.
- Add deterministic MPQ creation or patching.
- Validate duplicate IDs, missing references, invalid paths, encoding, and case mismatches.
- Produce a change report for every build.
- Install builds only into a third `Lords of Magic Development.app` profile.
- Add one-command rollback to the last known-good development build.

Deliverable: `build`, `validate`, `install-dev`, and `restore-dev` commands.

## Phase 4 — First vertical slice

Choose one deliberately small change that touches the complete workflow. Good candidates:

1. Rebalance one clearly underpowered unit and update every displayed value.
2. Correct an artifact behavior plus its tooltip.
3. Add one new dungeon encounter using existing creatures and rewards.
4. Add a small custom map with a scripted scenario hook.

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
- [x] Resolve standard map tags through `.til` definitions and original terrain atlases; export coherent map previews. (Orientation is **not** established — see [map format](map-format.md). The previews transpose as of 2026-09-17.)
- [x] Structurally decode and corpus-validate the dominant 49-byte placed-sprite record family.
- [x] Verify sprite origins and hotspot placement against the running engine ([issue #1](https://github.com/jake-bliss/lords-of-magic-modding/issues/1), closed 2026-09-16 — see [hotspots](hotspots.md)).
- Resolve uncommon IMP metadata variants and verify chroma keys, the shadow blend, and timing against the game ([issue #2](https://github.com/jake-bliss/lords-of-magic-modding/issues/2), [issue #3](https://github.com/jake-bliss/lords-of-magic-modding/issues/3)).
- Decode the remaining map-tail variants and prove candidate placed-sprite field behavior ([issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4)).
- Measure which transition tiles `setterrain` blends into the 8-neighbourhood, so the writer can offer terrain painting rather than single-cell forcing ([map format](map-format.md#writing-maps)).
- [x] IMP placement write-back (`--set-imp-placement`, `--imp-placement-for`).
- Build batch conversion, IMP **pixel** reimport, and lossless repack tests.
- Evaluate AI upscaling on portraits and interface art.
- Compare Lanczos runtime scaling against remastered source assets.
- Add automated dimension and palette validation.

## Phase 6 — Larger mod direction

After the toolchain and one vertical slice are proven, select a product direction:

- Near-vanilla restoration and bug-fix continuation.
- A new balance model derived from GS5R3.
- New Legends of Urak campaigns.
- A visual/audio remaster compatible with multiple gameplay profiles.
- A curated expansion combining new maps, quests, artifacts, and encounters.

## AI-assisted workflow

AI can accelerate schema discovery, documentation, transformation scripts, consistency checks, dialogue drafts, asset processing, and test design. All generated changes should remain reviewable as diffs and should be validated in game. Balance decisions require human playtesting and should not be accepted solely from model output.

## Parallel preservation-engine investigation

A native 64-bit reimplementation is a separate, gated track rather than a prerequisite for modding. The [candidate plan](native-engine-plan.md) begins with reusable archive/asset tools, then uses a GameScript VM spike to decide whether a faithful engine is economically reasonable. The successful [asset-viewer spike](../spikes/asset-viewer/README.md) is evidence for the first stage only.
