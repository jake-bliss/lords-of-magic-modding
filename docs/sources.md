# Sources

## Game and distribution

- [Steam store page](https://store.steampowered.com/app/404040/Lords_of_Magic_Special_Edition/)
- [SteamDB launch configuration and install registry](https://steamdb.info/app/404040/config/)
- [SteamDB depot manifest](https://steamdb.info/depot/404041/)
- [Lords of Magic: Special Edition manual](https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/404040/manuals/Lomse_Manual.pdf)

## Compatibility

- [Porting Kit](https://www.portingkit.com/app/download.html)
- [cnc-ddraw](https://github.com/FunkyFr3sh/cnc-ddraw)
- [CodeWeavers legacy performance settings](https://www.codeweavers.com/compatibility/crossover/tips/lords-of-magic-special-edition/fix-for-sluggish-performance)

## Community 3.02

- [Steam community 3.02 discussion](https://steamcommunity.com/app/404040/discussions/0/458606248628754057/)
- [The Patches Scrolls entry](https://www.patches-scrolls.com/lords_of_magic.php)
- [Archived Sierra Help 3.02 package](https://web.archive.org/web/20200827112625/https://sierrahelp.com/Files/Patches/LordsOfTheRealm/LordsOfMagicSpecialEditionUnofficial%28LOMSE302_00143%29.zip)

## GS5R3 and related mods

- [Community guide to 3.02, GS5R3, maps, portraits, and music](https://steamcommunity.com/sharedfiles/filedetails/?id=3453918606)
- [GS5R3 community review](https://steamcommunity.com/app/404040/discussions/0/215439774872159175/)
- [ManTerA GS5R3 archive](http://mantera.xorgate.com/mods/GS5/mpq/GS5R3.rar)
- [ManTerA PIC5R3 archive](http://mantera.xorgate.com/downloads/PIC5R3.rar)
- [GSZero+](https://www.moddb.com/mods/gszero)
- [LoMSE Update Mod](https://www.moddb.com/mods/lords-of-magic-special-edition-update-mod)
- [High-quality music fix](https://www.moddb.com/games/lords-of-magic-special-edition/addons/lords-of-magic-high-quality-music-fix)

## Binary analysis

- `spikes/asset-viewer/examples/disasm.rs` disassembles a native entry point with the `iced-x86`
  crate; the operator tables, arity walk, GameScript constant table at `0x00560108`, and the
  `map2screen`/`getimphotspot`/`enumimphotspots` reads all come from it.
- PE section mapping is implemented in `lom_asset_viewer::native_table::PeImage`.

## Community modding archive

- [Mantera's LOMSE site](http://mantera.xorgate.com/website.html) (HTTP only; no TLS listener)
- [GS5 per-revision changelog](http://mantera.xorgate.com/mods/GS5/history.html)
- [LOMSE Modding board](https://impz.proboards.com/board/18)
- [snv's IMP sprite format and RLE algorithm](https://impz.proboards.com/thread/2012/imp-sprites-rle-algorithm)
- [Auto-calc combat resolution explained](https://impz.proboards.com/thread/2243/auto-calc-explained-numbers)
- [GS5R3 updates and reports](https://impz.proboards.com/thread/1682/gs5r3-updates-reports)
- [GSZ updates and reports](https://impz.proboards.com/thread/1948/gsz-updates-reports)
- [Sprite hotspot and mirroring mechanism (thread 2176)](https://impz.proboards.com/thread/2176/great-masters-necropian-abyss-summon)
- [MPQ repack ruleset for `.gs` members](https://impz.proboards.com/thread/2102/error-when-mpq-edditing) — **its ruleset turned out not to be required**; see [mod ecosystem](mod-ecosystem.md#writing-to-archives)
- [2026 Lords of Magic SE Utility Suite](https://impz.proboards.com/thread/2590/lords-magic-utility-suite-open)
- [Generating Maps](https://impz.proboards.com/thread/2206/generating-maps) — named `gs\rmg.gs`, which unblocked issue #22
- [GSZ will support All MAP Sizes](https://impz.proboards.com/thread/2222/gsz-support-all-map-sizes)
- [Map Editor Interface adjustments/updates](https://impz.proboards.com/thread/2334/map-editor-interface-adjustments-updates) — the elevation UI change and the `maxgraphics` note
- [Units/Sprites/.IMP dumping ground](https://impz.proboards.com/thread/2437/new-units-sprites-imp-dumping) — the palette-compositing workaround our index finding makes unnecessary
- [Editing small_doodads](https://impz.proboards.com/thread/2247/editing-small-doodads) — `small_doodad` is a roster-sprite override, not a terrain doodad
- [Game Script Manual](https://impz.proboards.com/thread/2033/game-script-manual-available) — paid PDF; contents are unit/artifact/spell editing only

See [community research](community-research.md) for the verdict on each claim. Two of the
highest-profile claims, both by the mod's own author about his own code, are refuted by our corpus.

## Modding tools and guides

- [Steam Guide to Simple Modding](https://steamcommunity.com/sharedfiles/filedetails/?id=2839540731)
- [StormLib official repository](https://github.com/ladislav-zezula/StormLib)
- [Frost cross-platform MPQ editor](https://github.com/zach-cloud/Frost)
- [Ladislav Zezula's MPQ name-breaking/listfile index](http://www.zezula.net/en/mpq/namebreak.html)
- [Public game listfile bundle](http://www.zezula.net/download/listfiles.zip)

## Native preservation research

- [SierraVault Lords of Magic technical history](https://sierravault.net/games/lords-of-magic/1998-lords-of-magic-special-edition)
- [SDL3 documentation](https://wiki.libsdl.org/SDL3/FrontPage)
- [Rust SDL3 bindings](https://docs.rs/sdl3/latest/sdl3/)
- [Rust `png` crate documentation](https://docs.rs/png/latest/png/)
- [EA IFF 85 specification](https://1fish2.github.io/IFF/IFF%20docs%20with%20Commodore%20revisions/EA%20IFF%2085.pdf)
- [ILBM/PBM specification](https://1fish2.github.io/IFF/IFF%20docs%20with%20Commodore%20revisions/ILBM.pdf)
- [FFmpeg IFF decoder reference](https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/iff.c)

## Source quality notes

- SteamDB and the original manual are used for distribution metadata and documented game behavior.
- The `cnc-ddraw` repository is the authoritative source for its configuration and capabilities.
- Patch/mod behavior is primarily documented by the authors' included readmes and surviving community posts.
- The public Lords of Magic listfile is used only to resolve archive member names; the reproducibility script pins both its bundle and extracted-file SHA-256 hashes.
- Proprietary IMP format findings come from bounded comparison of user-owned binaries with their generated C headers and visible output. A community IMP specification was located on 2026-09-16 and agrees with our header offsets and RLE algorithm exactly; where it guesses, our bytes override it. See [community research](community-research.md).
- Community claims should be verified against extracted data or controlled in-game tests before becoming implementation assumptions.
- SierraVault's GameScript history is secondary context, not a language specification or a substitute for local corpus and runtime evidence.
