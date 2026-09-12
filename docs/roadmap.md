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
- [ ] Validate a deterministic repacking implementation before any development-profile install.

Delivered: [MPQ inventory](mpq-inventory.md), tracked comparison summaries, and reproducible extraction commands.

## Phase 2 — Script documentation

- Infer grammar from repeated constructs.
- Build a symbol/index database for units, spells, artifacts, buildings, factions, and encounters.
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
- [x] Recover action/cycle ranges and generated action names, navigate them, and export indexed PNG frames losslessly.
- [x] Parse the common header and cell grid of all installed `.scn`, `.smp`, and `.lgd` files and visualize candidate elevation.
- Resolve uncommon IMP metadata variants and verify chroma keys, origins, hotspots, and timing against the game ([issues #1–#3](https://github.com/jake-bliss/lords-of-magic-modding/issues)).
- Decode map terrain tags and trailing object-record variants ([issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4)).
- Build batch conversion, IMP reimport, and lossless repack tests.
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
