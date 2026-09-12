# Map and Scenario Format — Initial Native Probe

## Status

**Header, cell-grid, and diagnostic height rendering milestone complete.** The native Rust parser bounds-checks every installed `.scn`, `.smp`, and `.lgd` file without modifying it. The trailing object-like sections and terrain-art lookup remain under investigation.

The corpus is the working GS5R3 profile, including its supplied custom maps. No original map data or rendered captures are stored in Git.

## Observed prefix

Every one of the 365 inspected files begins with the same little-endian prefix:

| Offset | Size | Current name | Evidence |
| ---: | ---: | --- | --- |
| `0x00` | 4 | `metadata` | Varies by file; meaning unknown |
| `0x04` | 4 | `width` | Matches 32, 48, 64, 128, or 256 cell dimensions |
| `0x08` | 4 | `height` | Matches the same bounded cell dimensions |
| `0x0c` | 4 | `bits_per_pixel` | Always 8 in the observed corpus |
| `0x10` | `width × height × 8` | Cell grid | Exactly one eight-byte record per cell |

The cell record currently preserves both words without assigning engine behavior:

| Cell offset | Size | Current name | Evidence |
| ---: | ---: | --- | --- |
| `+0` | 4 | `tag` | 672 distinct values across the corpus; likely contains terrain/tile identity and possibly flags |
| `+4` | 4 | `value_bits` / `value` | Every value is a finite little-endian `f32` in `0..20`; grayscale rendering produces coherent world relief |

The second word is therefore strongly inferred to be elevation or height. It remains named `value` in the decoder until original-editor or runtime comparison establishes exact units and behavior.

## Corpus result

Verified on 2026-09-12:

| Kind | Files | Dimensions |
| --- | ---: | --- |
| `.lgd` legend scenario | 8 | 128×128 |
| `.scn` map scenario | 20 | 32×32 (1), 64×64 (1), 128×128 (17), 256×256 (1) |
| `.smp` map component | 337 | 48×48 (336), 64×64 (1) |
| **Total** | **365** | **0 parse failures** |

The trailing bytes begin immediately after the cell grid. The first trailing word is retained as `trailing_head_u32`; in many files it behaves like a record count. Exact-length comparisons identify these candidate families:

| Kind | 49-byte records + 8 fixed bytes | 52-byte records + 4 fixed bytes | 53-byte records + 4 fixed bytes | Unknown/ambiguous |
| --- | ---: | ---: | ---: | ---: |
| `.lgd` | 5 | 0 | 1 | 2 |
| `.scn` | 15 | 0 | 5 | 0 |
| `.smp` | 176 | 144 | 0 | 17 |

These are size equations, not yet decoded record structures. Work on cell tags and trailing variants is tracked in [GitHub issue #4](https://github.com/jake-bliss/lords-of-magic-modding/issues/4).

## Commands

```sh
cd spikes/asset-viewer
cargo build --release

target/release/lom-asset-viewer --scan-map-dir '/path/to/Lords of Magic Special Edition/English/map'
target/release/lom-asset-viewer --inspect-file '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
target/release/lom-asset-viewer --view-map '/path/to/Lords of Magic Special Edition/English/map/URAK.scn'
```

The diagnostic viewer starts in candidate-elevation mode. Press `C` to switch to stable colors derived from the opaque cell tags. Neither mode claims original terrain rendering.

## Confidence

- **Observed:** all 365 files have a 16-byte prefix, declared 8-bit depth, a complete `width × height × 8` cell grid, and a bounded trailing section.
- **Observed:** interpreting the second cell word as little-endian `f32` yields no non-finite values and produces a coherent grayscale relief map.
- **Inferred:** the second word is terrain elevation; the first identifies a terrain tile or tile-plus-flags.
- **Unknown:** the first header word, tag bit layout, elevation units, trailing object records, and editor/runtime coordinate conventions.
