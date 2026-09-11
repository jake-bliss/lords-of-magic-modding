# Roadmap

## Goal

Create a reproducible, source-controlled modding toolkit that lets us inspect, change, validate, package, and test Lords of Magic data without editing a playable installation by hand.

## Phase 1 — Archive inventory (initial pass complete)

- [x] Select and validate StormLib for read-only cross-platform MPQ extraction.
- [x] Extract `gs.mpq` and `pic.mpq` from baseline, 3.02, and GS5R3.
- [x] Preserve archive paths and listfile names where available.
- [x] Produce machine-readable manifests containing path, size, compressed size, flags, locale, and content hashes.
- [x] Diff all three extracted trees and distinguish formatting/comment-only changes from token-level script changes.
- [ ] Identify and name uncatalogued archive slots where practical.
- [ ] Classify every non-script file by detected format and purpose.
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

- Inventory image formats, palettes, dimensions, and transparency.
- Build lossless extract/convert/repack tests.
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
