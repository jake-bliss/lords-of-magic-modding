# Lords of Magic Modding Lab

This repository is the durable record and future workspace for modding **Lords of Magic: Special Edition** on Apple Silicon macOS. It documents what has been verified locally, separates observations from hypotheses, and provides a safe path toward an AI-assisted modding toolchain.

AI is being used as a development assistant for research, scripting, asset work, validation, and testing. There is currently no plan to add generative AI to the game itself.

## Current state

Last verified: **2026-09-11** on an **M4 Max Mac running macOS 26.5.2**.

| Profile | Purpose | Status |
| --- | --- | --- |
| `Steambuild 32 64bit DXVK.app` | Working baseline and recovery source | Working; preserve unchanged |
| `Lords of Magic 3.02.app` | Near-vanilla community bug-fix build | Installed and launch-tested |
| `Lords of Magic GS5R3.app` | ManTerA balance overhaul | Installed and launch-tested |

All three are stored in `/Users/jakebliss/Applications/`. The modded apps are APFS copy-on-write clones with independent Wine prefixes, game files, registry state, and save directories.

## Start here

- [macOS runbook](docs/macos-runbook.md) — launch, configuration, recovery, and troubleshooting
- [Game and data architecture](docs/game-architecture.md) — known modding surfaces and hard engine boundaries
- [Native preservation engine plan](docs/native-engine-plan.md) — staged Rust/SDL3 candidate with explicit stop/go gates
- [MPQ inventory](docs/mpq-inventory.md) — reproducible extraction and baseline/3.02/GS5R3 findings
- [Mod ecosystem](docs/mod-ecosystem.md) — 3.02, GS5R3, optional packages, and compatibility
- [Research log](docs/research-log.md) — evidence and conclusions from the working installation
- [Roadmap](docs/roadmap.md) — proposed mod SDK and first experiments
- [Sources](docs/sources.md) — primary and community references

## Working principles

1. Never experiment directly on the working baseline.
2. Keep each incompatible gameplay profile in a separate app and Wine prefix.
3. Back up `gs.mpq`, `pic.mpq`, configuration files, and saves before replacement.
4. Treat observed behavior, community claims, and our inferences as different evidence classes.
5. Build repeatable extraction, validation, packing, and smoke-test commands before attempting a large mod.
6. If distributing a mod, distribute original work or a patch—not Sierra/Rebellion's complete copyrighted assets.

## Immediate opportunity

The first archive comparison is complete. It isolates the focused 3.02 changes, confirms that GS5R3 is a broad script-and-art fork, and recovers hundreds of filenames that were anonymous in the original archives. A [native Rust asset-viewer spike](spikes/asset-viewer/README.md) now also decodes every detected PBM image in the GS5R3 picture archive. The next modding step is a searchable `.gs` symbol index; the next native-engine decision point is a narrowly scoped GameScript VM spike.
