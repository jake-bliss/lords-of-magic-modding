# Lords of Magic Modding Lab

This repository is the durable record and future workspace for modding **Lords of Magic: Special Edition** on Apple Silicon macOS. It documents what has been verified locally, separates observations from hypotheses, and provides a safe path toward an AI-assisted modding toolchain.

AI is being used as a development assistant for research, scripting, asset work, validation, and testing. There is currently no plan to add generative AI to the game itself.

## Current state

Game runtime last verified: **2026-09-16** (controlled measurements in the running engine) on an **M4 Max Mac running macOS 26.5.2**. Native asset corpus last rescanned: **2026-09-12**.

| Profile | Purpose | Status |
| --- | --- | --- |
| `Steambuild 32 64bit DXVK.app` | Working baseline and recovery source | Working; preserve unchanged |
| `Lords of Magic 3.02.app` | Near-vanilla community bug-fix build | Installed and launch-tested |
| `Lords of Magic GS5R3.app` | ManTerA balance overhaul | Installed and launch-tested |

All three are stored in `~/Applications/`. They are locally built Wineskin wrappers around a user-owned Steam install; the repository contains no game data or app bundles, and none of it is redistributable. The [macOS runbook](docs/macos-runbook.md) records how they were configured. The modded apps are APFS copy-on-write clones with independent Wine prefixes, game files, registry state, and save directories.

### Preservation-engine track

| Stage | Status | Evidence |
| --- | --- | --- |
| 0. Evidence baseline | Initial pass complete | Reproducible archive/profile inventories and patch/mod comparisons |
| 1. Native asset layer | In progress; image, sprite-export, terrain, and dominant map-record milestones complete | 9,804 members classified, 1,800 IMPs decoded, all 365 maps bounded, original terrain rendered, and 21,117 placed-object records decoded structurally in 365 of 365 maps |
| 2. GameScript VM probe | Started; lexing, vocabulary, host-API recovery, and first interpreter checkpoints complete | All 4,692 `.gs` members tokenize; the engine's 1,906 named operators are recovered from `lomse.exe` with entry points and disassembled arity; 3.02 `standard.gs` loads in the experimental VM and its `min`/`max` utilities execute correctly |
| 3–5. Native game/runtime | Not started | Contingent on the GameScript VM stop/go result |

The current deliverable is a useful native asset and reverse-engineering tool, not yet a native replacement game. See the [candidate plan](docs/native-engine-plan.md) for scope and estimates.

Concretely, today you can **inspect, decode, render, export, and measure**, and you can write a single sprite placement back into an IMP or replace an archive member by hand. There is no mod build pipeline, no validation command, no packaging, and no native game — Phases 3 and 4 of the [roadmap](docs/roadmap.md) have not started.

## Start here

- [macOS runbook](docs/macos-runbook.md) — launch, configuration, recovery, and troubleshooting
- [Native Rust asset tool](spikes/asset-viewer/README.md) — build, commands, what it proves and what it does not
- [Sprite placement and hotspots](docs/hotspots.md) — the solved placement rule and how to write it back
- [Game and data architecture](docs/game-architecture.md) — known modding surfaces and hard engine boundaries
- [Difficulty and computer-player AI](docs/difficulty-ai.md) — verified script gates, mod-specific changes, and remaining gameplay test
- [Agent handoff](docs/agent-handoff.md) — current state, local setup, reproducibility, and next bounded work
- [Native preservation engine plan](docs/native-engine-plan.md) — staged Rust/SDL3 candidate with explicit stop/go gates
- [Native asset layer](docs/native-asset-stage.md) — live Stage 1 evidence, validation gates, and remaining work
- [Map and scenario format](docs/map-format.md) — cell/terrain lookup, placed-sprite records, corpus measurements, and open variants
- [GameScript language and runtime probe](docs/gamescript-format.md) — corpus-derived lexical model, vocabulary measurements, and the next VM gate
- [MPQ inventory](docs/mpq-inventory.md) — reproducible extraction and baseline/3.02/GS5R3 findings
- [Mod ecosystem](docs/mod-ecosystem.md) — 3.02, GS5R3, optional packages, and compatibility
- [Research log](docs/research-log.md) — evidence and conclusions from the working installation
- [Roadmap](docs/roadmap.md) — proposed mod SDK and first experiments
- [Community research](docs/community-research.md) — surviving modding community, prior art, and the verdict on each claim
- [Sources](docs/sources.md) — primary and community references

## Working principles

1. Never experiment directly on the working baseline.
2. Keep each incompatible gameplay profile in a separate app and Wine prefix.
3. Back up `gs.mpq`, `pic.mpq`, configuration files, and saves before replacement.
4. Treat observed behavior, community claims, and our inferences as different evidence classes.
5. Build repeatable extraction, validation, packing, and smoke-test commands before attempting a large mod.
6. If distributing a mod, distribute original work or a patch—not Sierra/Rebellion's complete copyrighted assets.

## Immediate opportunity

The first archive comparison is complete. It isolates the focused 3.02 changes, confirms that GS5R3 is a broad script-and-art fork, and recovers hundreds of filenames that were anonymous in the original archives. The [native Rust asset tool](spikes/asset-viewer/README.md) now classifies all 9,804 core-archive members, decodes every detected PBM image, expands every observed IMP sprite without decoder errors, and parses all 365 installed map/scenario/component files. Its native viewers display masks, navigable action/facing animations, elevation relief, and original-art terrain; the CLI exports indexed sprite frames and terrain previews. The map parser also decodes the trailing object section of every installed map — 21,117 records across six record layouts — so object editing works on all 365. See the [Stage 1 record](docs/native-asset-stage.md) for measured coverage, provisional semantics, and known exceptions.
