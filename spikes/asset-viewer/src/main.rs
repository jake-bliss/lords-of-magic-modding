use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use lom_asset_viewer::asset::{AssetKind, probe};
use lom_asset_viewer::gameplay_symbols;
use lom_asset_viewer::gamescript::{GameScriptDocument, is_number_token};
use lom_asset_viewer::gs_facts::GsFacts;
use lom_asset_viewer::gamescript_vm::{
    GameScriptVm, GameScriptVmError, ModuleSource, Value as GameScriptValue, normalize_module_path,
};
use lom_asset_viewer::imp;
use lom_asset_viewer::imp::{
    IMP_ORPHAN_NOTES, IMP_VALIDATION_EXCEPTIONS, ImpHeaderStats, ImpOrphanNote, ImpSprite,
    ImpValidationException, MAX_RLE_REPEAT, PackedSizes, encode_rle, frame_pixel_target,
    imp_member_basename, layout_is_unobservable, layouts_are_identical, normalize_imp_member,
    pack_pixels, read_frame_pixels, unpack_pixels, write_frame_pixels,
};
use lom_asset_viewer::map::{
    GENERATED_HEADER_WORD, MapAsset, MintProvenance, PaintRefusal, ROAD_TERRAIN,
    TERRAIN_SPRITE_ARRAYS,
    TERRAIN_SPRITE_NAME_ONLY, TERRAIN_SPRITE_TYPES, TERRAIN_TYPES, TRANSITION_RING_OFFSETS,
    TerrainPaintPlan, interior_tile_family, road_background_ring, terrain_sprite_name,
    terrain_sprite_type, terrain_type_base_tile, transition_anchor, transition_ring,
};
use lom_asset_viewer::mpq::{Archive, Entry};
use lom_asset_viewer::native_table;
use lom_asset_viewer::paths::paths_are_same_file;
use lom_asset_viewer::server::{TileSetSource, serve};
use lom_asset_viewer::smacker::{self, SmackerFile};
use lom_asset_viewer::wave::{ImportOptions, WaveFile, WaveSweep, import_samples};
use lom_asset_viewer::operator_arity;
use lom_asset_viewer::pbm::{PbmChunk, PbmFile, PbmImage};
use lom_asset_viewer::png_export::{
    read_indexed_png, write_imp_frame_png, write_pbm_png, write_rgba_png,
};
use lom_asset_viewer::tile::{
    Direction, MapClass, TileChoice, TileSelector, TileSetDefinition, TileSetResolution,
    combat_tileset_array_candidates, resolve_tileset, tileset_mismatch,
};
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode};
use sdl3::video::Window;

const WINDOW_WIDTH: u32 = 1100;
const WINDOW_HEIGHT: u32 = 800;
const TERRAIN_PREVIEW_TILE_SIZE: u32 = 8;

/// One edit applied to a map between reading it and writing a new file.
///
/// Every variant is expressed in the engine's own terms -- a tile-atlas slot, one of the eleven
/// terrain types, a placed terrain sprite -- rather than in raw offsets, because the offsets are
/// the part that was wrong twice.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MapEdit {
    SetTile { x: u32, y: u32, tile_index: u32 },
    SetTerrain { x: u32, y: u32, terrain_type: u32 },
    SetElevation { x: u32, y: u32, value: f32 },
    FillTerrain { terrain_type: u32 },
    /// Paint a rectangular region and blend the measured transition ring around it.
    ///
    /// This is `setterrain` driven by the tileset the map was authored against. `SetTerrain` stays
    /// exactly what it was -- `forcetexture` on one cell -- because forcing a slot is a different
    /// operation, not a worse one, and it is the only one that needs no tileset at all.
    PaintTerrain {
        rect: (u32, u32, u32, u32),
        terrain_type: u32,
        /// How to break a tie when several tiles match a cell equally.
        ///
        /// The engine draws at random and that draw cannot be reproduced, so the tool is
        /// deterministic by default and seedable on request. See [`TileSelector`].
        selector: TileSelector,
    },
    PlaceSprite { x: u32, y: u32, sprite_type: u32 },
    RemoveSprite { instance_id: u32 },
    /// Parse and re-encode, changing nothing.
    ///
    /// Not a no-op: the output is bytes *this writer produced*, which is a different claim from
    /// the bytes on disk even when the two are equal. The `mapload` probe needs exactly that
    /// distinction -- its control rung asks whether the engine accepts a file we wrote, and a `cp`
    /// would test the filesystem instead.
    Rewrite,
    SetHighFlag { x: u32, y: u32, set: bool },
    /// Set tag bit `0x00800000` on the perimeter, or on an interior rectangle.
    ///
    /// Interior is the interesting one: the corpus only ever flags the border ring, so an interior
    /// flag is a shape the engine has never been given.
    FlagRegion { border: bool, rect: Option<(u32, u32, u32, u32)> },
}

enum Command {
    Catalog(Source),
    DescribeMap(PathBuf),
    /// Report which shipped `.til` the engine reads this map through.
    TileSetForMap(PathBuf),
    DumpMapCells {
        path: PathBuf,
        rect: Option<(u32, u32, u32, u32)>,
    },
    DiffMaps {
        left: PathBuf,
        right: PathBuf,
    },
    RoundtripMaps(PathBuf),
    SpriteTypes,
    TransitionRings,
    CreateMap {
        width: u32,
        height: u32,
        terrain_type: u32,
        output: PathBuf,
    },
    EditMap {
        input: PathBuf,
        edit: MapEdit,
        output: PathBuf,
        /// The `.til` the map was authored against, for edits that re-select tiles.
        ///
        /// Positional and optional, following `--view-map`'s shape. Only `--map-paint-terrain`
        /// reads it, and without it that verb refuses rather than guessing a tileset.
        tile_set: Option<PathBuf>,
    },
    /// Run the local map-editor web UI.
    Serve {
        tile_set: TileSetSource,
        port: u16,
    },
    DescribeImp {
        source: Source,
        member: String,
    },
    ExportImpFrame {
        source: Source,
        member: String,
        frame: usize,
        output: PathBuf,
    },
    ImpPlacementFor {
        width: u16,
        height: u16,
        anchor: (i32, i32),
        top_left: (i32, i32),
    },
    SetImpPlacement {
        input: PathBuf,
        frame: usize,
        x: i16,
        y: i16,
        hotspot: Option<u16>,
        output: PathBuf,
    },
    ExportMapPreview {
        map: PathBuf,
        tile_set: PathBuf,
        atlas: PathBuf,
        output: PathBuf,
    },
    ExportPbm {
        source: Source,
        member: String,
        output: PathBuf,
    },
    Extract {
        source: Source,
        member: String,
        output: PathBuf,
    },
    List(Source),
    Inspect {
        source: Source,
        member: Option<String>,
    },
    InspectFile(PathBuf),
    Scan(Source),
    ScanGameScript {
        source: Source,
        executable: Option<PathBuf>,
    },
    /// Emit one JSON object per `.gs` member, for the build pipeline's validator.
    GsFacts {
        target: GsFactsTarget,
    },
    ScanNatives {
        executable: PathBuf,
        source: Option<Source>,
    },
    ProbeGameScript {
        source: Source,
        member: String,
        expression: Option<String>,
        stubs: Vec<(String, GameScriptValue)>,
        executable: Option<PathBuf>,
    },
    /// Look one gameplay symbol up in the committed index.
    GameplaySymbol {
        name: String,
        reports: PathBuf,
    },
    /// List the gameplay symbols whose name matches a glob.
    GameplaySymbolsLike {
        pattern: String,
        reports: PathBuf,
    },
    ScanMapDirectory(PathBuf),
    /// Re-encode every PBM in an archive and compare the result with the original.
    RoundtripPbm(Source),
    /// Re-encode every IMP frame in an archive and compare the result with the original.
    ///
    /// `rewrite` escalates the check from "the payload re-encodes" to "the whole *file* rewrites":
    /// each frame is replaced with its own unmodified pixels through `write_frame_pixels`, which
    /// exercises the pointer rewriting and the byte-for-byte preservation of everything else.
    RoundtripImp { source: Source, rewrite: bool },
    /// Write an indexed PNG back into a PBM, inheriting palette and chunks from a source file.
    ImportPngPbm {
        png: PathBuf,
        template: PathBuf,
        output: PathBuf,
    },
    /// Write an indexed PNG back over one frame of an IMP, inheriting everything else.
    ImportPngImp {
        png: PathBuf,
        template: PathBuf,
        frame: usize,
        output: PathBuf,
    },
    /// Decode and re-encode every WAVE member of an archive, reporting both claims separately.
    RoundtripWave(Source),
    /// The same sweep over a directory of loose `.wav` files, for the shipped `Wav/` tree.
    RoundtripWaveDirectory(PathBuf),
    /// Write one WAVE member out as a standard `.wav` any audio editor opens.
    ExportWave {
        source: Source,
        member: String,
        output: PathBuf,
    },
    /// Write an edited `.wav` back into a member's container.
    ImportWave {
        edited: PathBuf,
        template: PathBuf,
        output: PathBuf,
        options: ImportOptions,
    },
    /// Report the container structure of one Smacker file.
    DescribeSmacker(PathBuf),
    /// Parse every `.smk` under a directory and report the structure of each.
    ScanSmackerDirectory(PathBuf),
    ValidateImp(Source),
    ViewImp {
        source: Source,
        member: String,
        frame: usize,
    },
    ViewMap {
        path: PathBuf,
        tile_set: Option<(PathBuf, PathBuf)>,
    },
    View {
        source: Source,
        member: Option<String>,
    },
}

struct Source {
    archive: PathBuf,
    listfile: Option<PathBuf>,
}

struct SelectedImage {
    index: usize,
    name: String,
    image: PbmImage,
}

struct TerrainPreview {
    width: u16,
    height: u16,
    rgba: Vec<u8>,
    atlas_name: String,
}

#[derive(Clone, Copy)]
enum ImpDisplayMode {
    Preview,
    Mask,
    Raw,
}

impl ImpDisplayMode {
    fn next(self) -> Self {
        match self {
            Self::Preview => Self::Mask,
            Self::Mask => Self::Raw,
            Self::Raw => Self::Preview,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Mask => "mask",
            Self::Raw => "raw",
        }
    }
}

#[derive(Clone, Copy)]
enum MapDisplayMode {
    CellTags,
    CandidateElevation,
    TerrainArtwork,
}

impl MapDisplayMode {
    fn next(self, has_terrain_artwork: bool) -> Self {
        match (self, has_terrain_artwork) {
            (Self::CandidateElevation, true) => Self::TerrainArtwork,
            (Self::TerrainArtwork, _) => Self::CellTags,
            (Self::CellTags, _) => Self::CandidateElevation,
            (Self::CandidateElevation, false) => Self::CellTags,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CellTags => "diagnostic cell tags",
            Self::CandidateElevation => "candidate elevation",
            Self::TerrainArtwork => "terrain artwork",
        }
    }
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match parse_args()? {
        Command::Catalog(source) => catalog_archive(&source),
        Command::DescribeMap(path) => describe_map(&path),
        Command::TileSetForMap(path) => tile_set_for_map(&path),
        Command::DumpMapCells { path, rect } => dump_map_cells(&path, rect),
        Command::DiffMaps { left, right } => diff_maps(&left, &right),
        Command::RoundtripMaps(path) => roundtrip_maps(&path),
        Command::SpriteTypes => sprite_types(),
        Command::TransitionRings => transition_rings(),
        Command::CreateMap {
            width,
            height,
            terrain_type,
            output,
        } => create_map(width, height, terrain_type, &output),
        Command::EditMap {
            input,
            edit,
            output,
            tile_set,
        } => edit_map(&input, edit, &output, tile_set.as_deref()),
        Command::Serve { tile_set, port } => serve(tile_set, port),
        Command::DescribeImp { source, member } => describe_imp(&source, &member),
        Command::ExportImpFrame {
            source,
            member,
            frame,
            output,
        } => export_imp_frame(&source, &member, frame, &output),
        Command::ImpPlacementFor {
            width,
            height,
            anchor,
            top_left,
        } => imp_placement_for(width, height, anchor, top_left),
        Command::SetImpPlacement {
            input,
            frame,
            x,
            y,
            hotspot,
            output,
        } => set_imp_placement(&input, frame, x, y, hotspot, &output),
        Command::ExportMapPreview {
            map,
            tile_set,
            atlas,
            output,
        } => export_map_preview(&map, &tile_set, &atlas, &output),
        Command::ExportPbm {
            source,
            member,
            output,
        } => export_pbm(&source, &member, &output),
        Command::Extract {
            source,
            member,
            output,
        } => extract_member(&source, &member, &output),
        Command::List(source) => list_archive(&source),
        Command::Inspect { source, member } => inspect_archive(&source, member.as_deref()),
        Command::InspectFile(path) => inspect_file(&path),
        Command::Scan(source) => scan_archive(&source),
        Command::ScanGameScript { source, executable } => {
            scan_gamescript_archive(&source, executable.as_deref())
        }
        Command::GsFacts { target } => emit_gs_facts(&target),
        Command::ScanNatives { executable, source } => {
            scan_native_table(&executable, source.as_ref())
        }
        Command::GameplaySymbol { name, reports } => gameplay_symbol(&name, &reports),
        Command::GameplaySymbolsLike { pattern, reports } => {
            gameplay_symbols_like(&pattern, &reports)
        }
        Command::ProbeGameScript {
            source,
            member,
            expression,
            stubs,
            executable,
        } => probe_gamescript_member(
            &source,
            &member,
            expression.as_deref(),
            &stubs,
            executable.as_deref(),
        ),
        Command::ScanMapDirectory(path) => scan_map_directory(&path),
        Command::RoundtripPbm(source) => roundtrip_pbm(&source),
        Command::RoundtripImp { source, rewrite } => roundtrip_imp(&source, rewrite),
        Command::ImportPngPbm {
            png,
            template,
            output,
        } => import_png_pbm(&png, &template, &output),
        Command::ImportPngImp {
            png,
            template,
            frame,
            output,
        } => import_png_imp(&png, &template, frame, &output),
        Command::RoundtripWave(source) => roundtrip_wave(&source),
        Command::RoundtripWaveDirectory(path) => roundtrip_wave_directory(&path),
        Command::ExportWave {
            source,
            member,
            output,
        } => export_wave(&source, &member, &output),
        Command::ImportWave {
            edited,
            template,
            output,
            options,
        } => import_wave(&edited, &template, &output, options),
        Command::DescribeSmacker(path) => describe_smacker(&path),
        Command::ScanSmackerDirectory(path) => scan_smacker_directory(&path),
        Command::ValidateImp(source) => validate_imp_archive(&source),
        Command::ViewImp {
            source,
            member,
            frame,
        } => view_imp_archive(&source, &member, frame),
        Command::ViewMap { path, tile_set } => view_map_file(&path, tile_set.as_ref()),
        Command::View { source, member } => view_archive(&source, member.as_deref()),
    }
}

fn parse_args() -> Result<Command, String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let listfile = take_option(&mut args, "--listfile")?.map(PathBuf::from);
    // Taken here with the other global options, but only ONE command honours it, so the command
    // arm below must reject it everywhere else. A flag that is silently swallowed and discarded
    // hands back a successful exit for a mode that never ran -- `--pbm-roundtrip X --rewrite` used
    // to print a clean sweep and rewrite nothing.
    let rewrite = take_flag(&mut args, "--rewrite")?;
    // Same treatment as `--rewrite`: taken globally, honoured by exactly one command, and refused
    // everywhere else rather than silently discarded.
    let import_options = ImportOptions {
        allow_format_change: take_flag(&mut args, "--allow-format-change")?,
        allow_dangling_loops: take_flag(&mut args, "--allow-dangling-loops")?,
    };
    let executable = take_option(&mut args, "--exe")?.map(PathBuf::from);
    let expression = take_option(&mut args, "--eval")?;
    let reports = take_option(&mut args, "--reports")?.map(PathBuf::from);
    let hotspot = take_option(&mut args, "--hotspot")?
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|_| format!("hotspot type must be a nonnegative integer: {value}"))
        })
        .transpose()?;
    let seed = take_option(&mut args, "--seed")?
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| format!("seed must be a nonnegative integer: {value}"))
        })
        .transpose()?;
    let port = take_option(&mut args, "--port")?
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|_| format!("port must be a number from 0 to 65535: {value}"))
        })
        .transpose()?;
    let pic = take_option(&mut args, "--pic")?.map(PathBuf::from);
    let stubs = take_repeated_option(&mut args, "--stub")
        .iter()
        .map(|specification| parse_native_stub(specification))
        .collect::<Result<Vec<_>, String>>()?;
    let first = args.first().ok_or_else(usage)?.as_str();
    // `--rewrite` is taken with the global options above but honoured by exactly one command, so
    // every other command must refuse it rather than swallow it. Silently discarding it returns a
    // successful exit for a mode that never ran: `--pbm-roundtrip X --rewrite` printed a clean
    // sweep and rewrote nothing.
    if rewrite && first != "--imp-roundtrip" {
        return Err(format!(
            "--rewrite is only meaningful with --imp-roundtrip, not {first}"
        ));
    }
    if import_options != ImportOptions::default() && first != "--import-wave" {
        return Err(format!(
            "--allow-format-change and --allow-dangling-loops are only meaningful with \
             --import-wave, not {first}"
        ));
    }
    match first {
        "--catalog" => {
            require_len(&args, 2)?;
            Ok(Command::Catalog(source(&args[1], listfile)))
        }
        "--describe-map" => {
            require_len(&args, 2)?;
            Ok(Command::DescribeMap(args[1].clone().into()))
        }
        "--map-tileset-for" => {
            require_len(&args, 2)?;
            Ok(Command::TileSetForMap(args[1].clone().into()))
        }
        "--dump-map-cells" => {
            let rect = match args.len() {
                2 => None,
                6 => Some((
                    parse_u32(&args[2])?,
                    parse_u32(&args[3])?,
                    parse_u32(&args[4])?,
                    parse_u32(&args[5])?,
                )),
                _ => return Err(usage()),
            };
            Ok(Command::DumpMapCells {
                path: args[1].clone().into(),
                rect,
            })
        }
        "--diff-maps" => {
            require_len(&args, 3)?;
            Ok(Command::DiffMaps {
                left: args[1].clone().into(),
                right: args[2].clone().into(),
            })
        }
        "--map-roundtrip" => {
            require_len(&args, 2)?;
            Ok(Command::RoundtripMaps(args[1].clone().into()))
        }
        "--map-sprite-types" => {
            require_len(&args, 1)?;
            Ok(Command::SpriteTypes)
        }
        "--map-transition-rings" => {
            require_len(&args, 1)?;
            Ok(Command::TransitionRings)
        }
        "--map-create" => {
            require_len(&args, 5)?;
            Ok(Command::CreateMap {
                width: parse_u32(&args[1])?,
                height: parse_u32(&args[2])?,
                terrain_type: parse_terrain_type(&args[3])?,
                output: args[4].clone().into(),
            })
        }
        "--map-rewrite" => {
            require_len(&args, 3)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::Rewrite,
                output: args[2].clone().into(),
                tile_set: None,
            })
        }
        "--map-set-high-flag" => {
            require_len(&args, 6)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::SetHighFlag {
                    x: parse_u32(&args[2])?,
                    y: parse_u32(&args[3])?,
                    set: parse_flag(&args[4])?,
                },
                output: args[5].clone().into(),
                tile_set: None,
            })
        }
        "--map-flag-border" => {
            require_len(&args, 3)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::FlagRegion { border: true, rect: None },
                output: args[2].clone().into(),
                tile_set: None,
            })
        }
        "--map-flag-rect" => {
            require_len(&args, 7)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::FlagRegion {
                    border: false,
                    rect: Some((
                        parse_u32(&args[2])?,
                        parse_u32(&args[3])?,
                        parse_u32(&args[4])?,
                        parse_u32(&args[5])?,
                    )),
                },
                output: args[6].clone().into(),
                tile_set: None,
            })
        }
        "--map-set-tile" => {
            require_len(&args, 6)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::SetTile {
                    x: parse_u32(&args[2])?,
                    y: parse_u32(&args[3])?,
                    tile_index: parse_u32(&args[4])?,
                },
                output: args[5].clone().into(),
                tile_set: None,
            })
        }
        "--map-set-terrain" => {
            require_len(&args, 6)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::SetTerrain {
                    x: parse_u32(&args[2])?,
                    y: parse_u32(&args[3])?,
                    terrain_type: parse_terrain_type(&args[4])?,
                },
                output: args[5].clone().into(),
                tile_set: None,
            })
        }
        "--map-set-elevation" => {
            require_len(&args, 6)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::SetElevation {
                    x: parse_u32(&args[2])?,
                    y: parse_u32(&args[3])?,
                    value: parse_elevation(&args[4])?,
                },
                output: args[5].clone().into(),
                tile_set: None,
            })
        }
        "--map-fill-terrain" => {
            require_len(&args, 4)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::FillTerrain {
                    terrain_type: parse_terrain_type(&args[2])?,
                },
                output: args[3].clone().into(),
                tile_set: None,
            })
        }
        "--map-paint-terrain" => {
            // The tileset is a trailing positional, the shape `--view-map` already uses. Omitting
            // it is accepted by the parser and refused by the paint, so the refusal names the
            // missing tileset instead of the argument count.
            if args.len() != 8 && args.len() != 9 {
                return Err(usage());
            }
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::PaintTerrain {
                    rect: (
                        parse_u32(&args[2])?,
                        parse_u32(&args[3])?,
                        parse_u32(&args[4])?,
                        parse_u32(&args[5])?,
                    ),
                    terrain_type: parse_paint_terrain_type(&args[6])?,
                    selector: seed.map_or(TileSelector::LowestSlot, TileSelector::Seeded),
                },
                output: args[7].clone().into(),
                tile_set: args.get(8).map(|value| value.clone().into()),
            })
        }
        "--map-place-sprite" => {
            require_len(&args, 6)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::PlaceSprite {
                    x: parse_u32(&args[2])?,
                    y: parse_u32(&args[3])?,
                    sprite_type: parse_sprite_type(&args[4])?,
                },
                output: args[5].clone().into(),
                tile_set: None,
            })
        }
        "--map-remove-sprite" => {
            require_len(&args, 4)?;
            Ok(Command::EditMap {
                input: args[1].clone().into(),
                edit: MapEdit::RemoveSprite {
                    instance_id: parse_u32(&args[2])?,
                },
                output: args[3].clone().into(),
                tile_set: None,
            })
        }
        "--describe-imp" => {
            require_len(&args, 3)?;
            Ok(Command::DescribeImp {
                source: source(&args[1], listfile),
                member: args[2].clone(),
            })
        }
        "--extract" => {
            require_len(&args, 4)?;
            Ok(Command::Extract {
                source: source(&args[1], listfile),
                member: args[2].clone(),
                output: args[3].clone().into(),
            })
        }
        "--export-imp-frame" => {
            require_len(&args, 5)?;
            Ok(Command::ExportImpFrame {
                source: source(&args[1], listfile),
                member: args[2].clone(),
                frame: parse_frame_index(&args[3])?,
                output: args[4].clone().into(),
            })
        }
        "--imp-placement-for" => {
            require_len(&args, 7)?;
            Ok(Command::ImpPlacementFor {
                width: parse_dimension(&args[1])?,
                height: parse_dimension(&args[2])?,
                anchor: (parse_coordinate(&args[3])?, parse_coordinate(&args[4])?),
                top_left: (parse_coordinate(&args[5])?, parse_coordinate(&args[6])?),
            })
        }
        "--set-imp-placement" => {
            require_len(&args, 6)?;
            Ok(Command::SetImpPlacement {
                input: args[1].clone().into(),
                frame: parse_frame_index(&args[2])?,
                x: parse_offset(&args[3])?,
                y: parse_offset(&args[4])?,
                hotspot,
                output: args[5].clone().into(),
            })
        }
        "--export-map-preview" => {
            require_len(&args, 5)?;
            Ok(Command::ExportMapPreview {
                map: args[1].clone().into(),
                tile_set: args[2].clone().into(),
                atlas: args[3].clone().into(),
                output: args[4].clone().into(),
            })
        }
        "--wave-roundtrip" => {
            require_len(&args, 2)?;
            Ok(Command::RoundtripWave(source(&args[1], listfile)))
        }
        "--wave-roundtrip-dir" => {
            require_len(&args, 2)?;
            Ok(Command::RoundtripWaveDirectory(args[1].clone().into()))
        }
        "--export-wave" => {
            require_len(&args, 4)?;
            Ok(Command::ExportWave {
                source: source(&args[1], listfile),
                member: args[2].clone(),
                output: args[3].clone().into(),
            })
        }
        "--import-wave" => {
            require_len(&args, 4)?;
            Ok(Command::ImportWave {
                edited: args[1].clone().into(),
                template: args[2].clone().into(),
                output: args[3].clone().into(),
                options: import_options,
            })
        }
        "--describe-smk" => {
            require_len(&args, 2)?;
            Ok(Command::DescribeSmacker(args[1].clone().into()))
        }
        "--scan-smk-dir" => {
            require_len(&args, 2)?;
            Ok(Command::ScanSmackerDirectory(args[1].clone().into()))
        }
        "--pbm-roundtrip" => {
            require_len(&args, 2)?;
            Ok(Command::RoundtripPbm(source(&args[1], listfile)))
        }
        "--imp-roundtrip" => {
            require_len(&args, 2)?;
            Ok(Command::RoundtripImp {
                source: source(&args[1], listfile),
                rewrite,
            })
        }
        "--import-png-pbm" => {
            require_len(&args, 4)?;
            Ok(Command::ImportPngPbm {
                png: args[1].clone().into(),
                template: args[2].clone().into(),
                output: args[3].clone().into(),
            })
        }
        "--import-png-imp" => {
            require_len(&args, 5)?;
            Ok(Command::ImportPngImp {
                png: args[1].clone().into(),
                template: args[2].clone().into(),
                frame: parse_frame_index(&args[3])?,
                output: args[4].clone().into(),
            })
        }
        "--export-pbm" => {
            require_len(&args, 4)?;
            Ok(Command::ExportPbm {
                source: source(&args[1], listfile),
                member: args[2].clone(),
                output: args[3].clone().into(),
            })
        }
        "--list" => {
            require_len(&args, 2)?;
            Ok(Command::List(source(&args[1], listfile)))
        }
        "--inspect" => {
            if !(2..=3).contains(&args.len()) {
                return Err(usage());
            }
            Ok(Command::Inspect {
                source: source(&args[1], listfile),
                member: args.get(2).cloned(),
            })
        }
        "--inspect-file" => {
            require_len(&args, 2)?;
            Ok(Command::InspectFile(args[1].clone().into()))
        }
        "--scan" => {
            require_len(&args, 2)?;
            Ok(Command::Scan(source(&args[1], listfile)))
        }
        "--scan-gamescript" => {
            require_len(&args, 2)?;
            Ok(Command::ScanGameScript {
                source: source(&args[1], listfile),
                executable,
            })
        }
        "--gameplay-symbol" => {
            require_len(&args, 2)?;
            Ok(Command::GameplaySymbol {
                name: args[1].clone(),
                reports: reports.clone().unwrap_or_else(default_gameplay_reports),
            })
        }
        "--gameplay-symbols-like" => {
            require_len(&args, 2)?;
            Ok(Command::GameplaySymbolsLike {
                pattern: args[1].clone(),
                reports: reports.clone().unwrap_or_else(default_gameplay_reports),
            })
        }
        "--gs-facts" => {
            if !(2..=3).contains(&args.len()) {
                return Err(usage());
            }
            let path = PathBuf::from(&args[1]);
            // An archive is addressed as ARCHIVE plus an optional member; anything else is a
            // local file or directory.
            //
            // The directory test comes FIRST and deliberately outranks the `.mpq` suffix: a mod
            // tree's source directory for `gs.mpq` is itself named `archives/gs.mpq`, so deciding
            // on the suffix alone would try to open a directory as an archive.
            let target = if args.len() == 3 {
                GsFactsTarget::ArchiveMember {
                    source: source(&args[1], listfile),
                    member: args[2].clone(),
                }
            } else if !path.is_dir()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("mpq"))
            {
                GsFactsTarget::Archive(source(&args[1], listfile))
            } else {
                GsFactsTarget::Path(path)
            };
            Ok(Command::GsFacts { target })
        }
        "--scan-natives" => {
            if args.len() != 2 && args.len() != 3 {
                return Err(usage());
            }
            Ok(Command::ScanNatives {
                executable: PathBuf::from(&args[1]),
                source: args.get(2).map(|archive| source(archive, listfile)),
            })
        }
        "--probe-gamescript" => {
            require_len(&args, 3)?;
            Ok(Command::ProbeGameScript {
                source: source(&args[1], listfile),
                member: args[2].clone(),
                expression,
                stubs,
                executable,
            })
        }
        "--scan-map-dir" => {
            require_len(&args, 2)?;
            Ok(Command::ScanMapDirectory(args[1].clone().into()))
        }
        "--validate-imp" => {
            require_len(&args, 2)?;
            Ok(Command::ValidateImp(source(&args[1], listfile)))
        }
        "--view-imp" => {
            if !(3..=4).contains(&args.len()) {
                return Err(usage());
            }
            let frame = args
                .get(3)
                .map(|value| parse_frame_index(value))
                .transpose()?
                .unwrap_or(0);
            Ok(Command::ViewImp {
                source: source(&args[1], listfile),
                member: args[2].clone(),
                frame,
            })
        }
        "--serve" => {
            // The tileset is named two ways and never defaulted: `--pic` lets the resolved member
            // be read straight out of the archive, and the positional pair is `--view-map`'s own
            // shape for loose files. Requiring exactly one keeps a run from silently preferring one
            // over the other.
            let tile_set = match (pic, args.len()) {
                (Some(archive), 1) => TileSetSource::Archive(archive),
                (None, 3) => TileSetSource::Loose {
                    definition: args[1].clone().into(),
                    atlas: args[2].clone().into(),
                },
                (Some(_), _) | (None, _) => {
                    return Err(
                        "--serve needs either --pic PIC.MPQ, and it will read the tileset the \
                         gamescript binds each map to, or a loose TILESET.til TILE_ATLAS.lbm pair \
                         -- not both and not neither"
                            .to_owned(),
                    );
                }
            };
            Ok(Command::Serve {
                tile_set,
                port: port.unwrap_or(8731),
            })
        }
        "--view-map" => {
            if args.len() != 2 && args.len() != 4 {
                return Err(usage());
            }
            Ok(Command::ViewMap {
                path: args[1].clone().into(),
                tile_set: (args.len() == 4)
                    .then(|| (args[2].clone().into(), args[3].clone().into())),
            })
        }
        "--help" | "-h" => Err(usage()),
        _ => {
            if !(1..=2).contains(&args.len()) {
                return Err(usage());
            }
            Ok(Command::View {
                source: source(&args[0], listfile),
                member: args.get(1).cloned(),
            })
        }
    }
}

fn parse_offset(value: &str) -> Result<i16, String> {
    value
        .parse()
        .map_err(|_| format!("placement offset must fit in a signed 16-bit integer: {value}"))
}

fn parse_dimension(value: &str) -> Result<u16, String> {
    value
        .parse()
        .map_err(|_| format!("frame dimension must be a nonnegative 16-bit integer: {value}"))
}

fn parse_coordinate(value: &str) -> Result<i32, String> {
    value
        .parse()
        .map_err(|_| format!("screen coordinate must be an integer: {value}"))
}

fn parse_u32(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{value} is not a map coordinate"))
}

fn parse_frame_index(value: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("IMP frame index must be a nonnegative integer: {value}"))
}

fn source(archive: &str, listfile: Option<PathBuf>) -> Source {
    Source {
        archive: archive.into(),
        listfile,
    }
}

/// Collect every occurrence of a repeatable option, in command-line order.
fn take_repeated_option(args: &mut Vec<String>, option: &str) -> Vec<String> {
    let mut values = Vec::new();
    while let Some(position) = args.iter().position(|argument| argument == option) {
        if position + 1 >= args.len() {
            args.remove(position);
            break;
        }
        values.push(args.remove(position + 1));
        args.remove(position);
    }
    values
}

/// Parse a `NAME=VALUE` native stub. Values are integers, `true`, or `false` — the return
/// shapes of the pure state reads worth stubbing. Anything richer needs real host modelling.
fn parse_native_stub(specification: &str) -> Result<(String, GameScriptValue), String> {
    let (name, value) = specification
        .split_once('=')
        .ok_or_else(|| format!("--stub expects NAME=VALUE, got {specification}"))?;
    if name.is_empty() {
        return Err("--stub requires a name before =".to_owned());
    }
    let value = match value {
        "true" => GameScriptValue::Boolean(true),
        "false" => GameScriptValue::Boolean(false),
        // Classified with the lexer's own predicate, not `f64::from_str`, so `--stub x=inf` is
        // the error it reads as rather than a silent infinity on the stub's stack. Evidence
        // class: Corrected.
        other if is_number_token(other) => {
            GameScriptValue::Number(other.parse::<f64>().map_err(|_| {
                format!("--stub value must be a number, true, or false, got {other}")
            })?)
        }
        other => {
            return Err(format!(
                "--stub value must be a number, true, or false, got {other}"
            ));
        }
    };
    Ok((name.to_owned(), value))
}

/// Remove a valueless flag, refusing a repeat the way [`take_option`] refuses a repeated option.
///
/// A flag silently accepted twice is a flag whose second spelling was a typo nobody was told about.
fn take_flag(args: &mut Vec<String>, flag: &str) -> Result<bool, String> {
    let Some(position) = args.iter().position(|argument| argument == flag) else {
        return Ok(false);
    };
    if args
        .iter()
        .skip(position + 1)
        .any(|argument| argument == flag)
    {
        return Err(format!("{flag} may only be supplied once"));
    }
    args.remove(position);
    Ok(true)
}

fn take_option(args: &mut Vec<String>, option: &str) -> Result<Option<String>, String> {
    let Some(position) = args.iter().position(|argument| argument == option) else {
        return Ok(None);
    };
    if args
        .iter()
        .skip(position + 1)
        .any(|argument| argument == option)
    {
        return Err(format!("{option} may only be supplied once"));
    }
    if position + 1 >= args.len() {
        return Err(format!("{option} requires a value"));
    }
    let value = args.remove(position + 1);
    args.remove(position);
    Ok(Some(value))
}

fn require_len(args: &[String], expected: usize) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(usage())
    }
}

fn usage() -> String {
    "usage:\n  lom-asset-viewer --list ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --catalog ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan-gamescript ARCHIVE.mpq [--listfile FILE] [--exe lomse.exe]\n  lom-asset-viewer --scan-natives lomse.exe [GS.MPQ] [--listfile FILE]\n  lom-asset-viewer --gs-facts ARCHIVE.mpq [MEMBER] [--listfile FILE]\n  lom-asset-viewer --gs-facts FILE-OR-DIRECTORY\n  lom-asset-viewer --gameplay-symbol NAME [--reports DIR]\n  lom-asset-viewer --gameplay-symbols-like PATTERN [--reports DIR]\n  lom-asset-viewer --probe-gamescript ARCHIVE.mpq MEMBER [--listfile FILE] [--eval SOURCE] [--stub NAME=VALUE]...\n  lom-asset-viewer --scan-map-dir DIRECTORY\n  lom-asset-viewer --describe-map FILE\n  lom-asset-viewer --map-tileset-for FILE\n  lom-asset-viewer --dump-map-cells FILE [X0 Y0 X1 Y1]\n  lom-asset-viewer --diff-maps LEFT RIGHT\n  lom-asset-viewer --map-roundtrip FILE-OR-DIRECTORY\n  lom-asset-viewer --map-create WIDTH HEIGHT TERRAIN OUT\n  lom-asset-viewer --map-sprite-types\n  lom-asset-viewer --map-transition-rings\n  lom-asset-viewer --map-rewrite IN OUT\n  lom-asset-viewer --map-set-high-flag IN X Y 0|1 OUT\n  lom-asset-viewer --map-flag-border IN OUT\n  lom-asset-viewer --map-flag-rect IN X0 Y0 X1 Y1 OUT\n  lom-asset-viewer --map-set-tile IN X Y TILE_SLOT OUT\n  lom-asset-viewer --map-set-terrain IN X Y TERRAIN OUT\n  lom-asset-viewer --map-set-elevation IN X Y VALUE OUT\n  lom-asset-viewer --map-fill-terrain IN TERRAIN OUT\n  lom-asset-viewer --map-paint-terrain IN X0 Y0 X1 Y1 TERRAIN OUT TILESET.til [--seed N]\n  lom-asset-viewer --map-place-sprite IN X Y SPRITE_TYPE OUT\n  lom-asset-viewer --map-remove-sprite IN INSTANCE_ID OUT\n  lom-asset-viewer --validate-imp ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --describe-imp ARCHIVE.mpq MEMBER [--listfile FILE]\n  lom-asset-viewer --view-imp ARCHIVE.mpq MEMBER [FRAME] [--listfile FILE]\n  lom-asset-viewer --view-map FILE [TILESET.til TILE_ATLAS.lbm]\n  lom-asset-viewer --serve --pic PIC.MPQ [--port N]\n  lom-asset-viewer --serve TILESET.til TILE_ATLAS.lbm [--port N]\n  lom-asset-viewer --set-imp-placement IN.imp FRAME X Y OUT.imp [--hotspot TYPE]\n  lom-asset-viewer --imp-placement-for WIDTH HEIGHT ANCHOR_X ANCHOR_Y TOP_LEFT_X TOP_LEFT_Y\n  lom-asset-viewer --export-map-preview FILE TILESET.til TILE_ATLAS.lbm OUTPUT.png\n  lom-asset-viewer --export-imp-frame ARCHIVE.mpq MEMBER FRAME OUTPUT.png [--listfile FILE]\n  lom-asset-viewer --export-pbm ARCHIVE.mpq MEMBER OUTPUT.png [--listfile FILE]\n  lom-asset-viewer --import-png-pbm INPUT.png SOURCE.lbm OUTPUT.lbm\n  lom-asset-viewer --import-png-imp INPUT.png SOURCE.imp FRAME OUTPUT.imp\n  lom-asset-viewer --wave-roundtrip ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --wave-roundtrip-dir DIRECTORY\n  lom-asset-viewer --export-wave ARCHIVE.mpq MEMBER OUTPUT.wav [--listfile FILE]\n  lom-asset-viewer --import-wave EDITED.wav TEMPLATE.wav OUTPUT.wav [--allow-format-change] [--allow-dangling-loops]\n  lom-asset-viewer --describe-smk FILE.smk\n  lom-asset-viewer --scan-smk-dir DIRECTORY\n  lom-asset-viewer --pbm-roundtrip ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --imp-roundtrip ARCHIVE.mpq [--listfile FILE] [--rewrite]\n  lom-asset-viewer --inspect ARCHIVE.mpq [MEMBER] [--listfile FILE]\n  lom-asset-viewer --inspect-file FILE\n  lom-asset-viewer --extract ARCHIVE.mpq MEMBER OUTPUT [--listfile FILE]\n  lom-asset-viewer ARCHIVE.mpq [MEMBER] [--listfile FILE]".to_owned()
}

fn open_archive(source: &Source) -> Result<(Archive, Vec<Entry>), String> {
    let archive = Archive::open(&source.archive).map_err(|error| error.to_string())?;
    if let Some(path) = &source.listfile {
        let contents = fs::read(path)
            .map_err(|error| format!("could not read listfile {}: {error}", path.display()))?;
        archive
            .add_listfile_contents(&contents)
            .map_err(|error| error.to_string())?;
    }
    let entries = archive.entries().map_err(|error| error.to_string())?;
    Ok((archive, entries))
}

fn list_archive(source: &Source) -> Result<(), String> {
    let (_archive, entries) = open_archive(source)?;
    println!("size\tcompressed\tlocale\tflags\tname");
    for entry in &entries {
        println!(
            "{}\t{}\t{}\t0x{:08x}\t{}",
            entry.size, entry.compressed_size, entry.locale, entry.flags, entry.name
        );
    }
    eprintln!("{} entries", entries.len());
    Ok(())
}

fn extract_member(source: &Source, member: &str, output: &PathBuf) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| format!("archive has no member named {member}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    file.write_all(&bytes)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!("wrote\t{}\t{}", output.display(), bytes.len());
    Ok(())
}

fn export_imp_frame(
    source: &Source,
    member: &str,
    frame_index: usize,
    output: &PathBuf,
) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| format!("archive has no member named {member}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let sprite = ImpSprite::parse(&bytes).map_err(|error| error.to_string())?;
    let frame = sprite
        .resolved_frame(frame_index)
        .map_err(|error| error.to_string())?;
    let mut encoded = Vec::new();
    write_imp_frame_png(&mut encoded, &sprite, frame_index)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    file.write_all(&encoded)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "wrote\t{}\t{}\t{}x{}\tframe={}",
        output.display(),
        encoded.len(),
        frame.width,
        frame.height,
        frame_index
    );
    Ok(())
}

fn export_pbm(source: &Source, member: &str, output: &PathBuf) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| format!("archive has no member named {member}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let image = PbmImage::decode(&bytes).map_err(|error| error.to_string())?;
    let mut encoded = Vec::new();
    write_pbm_png(&mut encoded, &image)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    file.write_all(&encoded)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "wrote\t{}\t{}\t{}x{}\tpalette={}",
        output.display(),
        encoded.len(),
        image.width,
        image.height,
        image.palette_entries,
    );
    Ok(())
}

/// Decode and re-encode every WAVE member of an archive.
///
/// Prints the corpus histogram alongside the two round-trip counts, because the histogram is the
/// measurement the round-trip numbers only mean something against: "every member re-encodes" is a
/// much weaker statement when every member turns out to be the same shape.
fn roundtrip_wave(source: &Source) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let mut sweep = WaveSweep::default();
    for entry in &entries {
        let bytes = match archive.read(&entry.name) {
            Ok(bytes) => bytes,
            Err(error) => {
                sweep.failures.push((entry.name.clone(), error.to_string()));
                sweep.checked += 1;
                continue;
            }
        };
        sweep.observe(&entry.name, &bytes);
    }
    print!("{}", sweep.report());
    if sweep.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} member(s) failed", sweep.failures.len()))
    }
}

/// The same sweep over the loose `Wav/` tree the game ships beside its archives.
fn roundtrip_wave_directory(directory: &Path) -> Result<(), String> {
    if !directory.is_dir() {
        return Err(format!("not a directory: {}", directory.display()));
    }
    let mut paths = Vec::new();
    collect_paths_with_extension(directory, "wav", &mut paths)?;
    paths.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    let mut sweep = WaveSweep::default();
    for path in &paths {
        let name = path
            .strip_prefix(directory)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        match fs::read(path) {
            Ok(bytes) => sweep.observe(&name, &bytes),
            Err(error) => {
                sweep.checked += 1;
                sweep.failures.push((name, error.to_string()));
            }
        }
    }
    print!("{}", sweep.report());
    if sweep.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} file(s) failed", sweep.failures.len()))
    }
}

/// Write one WAVE member out as a `.wav` an audio editor opens.
///
/// The bytes written are this tool's re-encode, not a copy of the member: the `fmt ` chunk and the
/// sample data are rebuilt from the decoded structure. That is deliberate. An extraction proves
/// nothing about the decoder, and a modder who edits the exported file needs the file they edit to
/// be the file `--import-wave` will compare against.
fn export_wave(source: &Source, member: &str, output: &PathBuf) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| format!("archive has no member named {member}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let file = WaveFile::parse(&bytes).map_err(|error| error.to_string())?;
    let encoded = file.encode().map_err(|error| error.to_string())?;
    if encoded != bytes {
        return Err(format!(
            "{member} does not re-encode to its own bytes, so this tool will not claim to export it"
        ));
    }
    let mut handle = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    handle
        .write_all(&encoded)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "wrote\t{}\t{}\t{}\tframes={}\tduration-ms={}",
        output.display(),
        encoded.len(),
        lom_asset_viewer::wave::describe_format(&file.format),
        file.samples.frames(),
        file.samples.duration_ms()
    );
    Ok(())
}

/// Put an edited `.wav` back into a member's container.
///
/// The template is the shipped member -- extract it with `--extract`, or use the file
/// `--export-wave` produced. Its ancillary chunks survive; only the audio comes from the edit.
fn import_wave(
    edited: &Path,
    template: &Path,
    output: &PathBuf,
    options: ImportOptions,
) -> Result<(), String> {
    for input in [edited, template] {
        if paths_are_same_file(input, output) {
            return Err(format!(
                "refusing to write {} over its own input",
                output.display()
            ));
        }
    }
    let edited_bytes =
        fs::read(edited).map_err(|error| format!("could not read {}: {error}", edited.display()))?;
    let template_bytes = fs::read(template)
        .map_err(|error| format!("could not read {}: {error}", template.display()))?;
    let result = import_samples(&edited_bytes, &template_bytes, options)
        .map_err(|error| error.to_string())?;
    let parsed = WaveFile::parse(&result).map_err(|error| {
        format!("the file this tool just wrote does not parse back: {error}")
    })?;
    let mut handle = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    handle
        .write_all(&result)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "wrote\t{}\t{}\t{}\tframes={}\tlayout={}\tidentical-to-template={}",
        output.display(),
        result.len(),
        lom_asset_viewer::wave::describe_format(&parsed.format),
        parsed.samples.frames(),
        parsed.layout(),
        result == template_bytes
    );
    Ok(())
}

fn describe_smacker(path: &Path) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let file = SmackerFile::parse(&bytes).map_err(|error| error.to_string())?;
    println!("file\t{}", path.display());
    println!("bytes\t{}", bytes.len());
    println!("signature\t{}", String::from_utf8_lossy(&file.signature));
    println!("dimensions\t{}x{}", file.width, file.height);
    println!("frame_count\t{}", file.frame_count);
    println!("table_entries\t{}", file.frames.len());
    println!("ring_frame\t{}", file.has_ring_frame());
    println!("flags\t0x{:08x}", file.flags);
    println!(
        "unaccounted_flag_bits\t0x{:08x}",
        file.unaccounted_flag_bits()
    );
    println!("raw_frame_rate\t{}", file.raw_frame_rate);
    println!("frame_interval_us\t{}", file.frame_interval_us());
    println!(
        "duration_ms\t{}",
        file.duration_ms()
            .map_or_else(|| "unrepresentable".to_owned(), |value| value.to_string())
    );
    println!("trees_offset\t{}", file.trees_offset);
    println!("trees_size\t{}", file.trees_size);
    println!(
        "tree_table_sizes\tmmap={};mclr={};full={};type={}",
        file.mmap_size, file.mclr_size, file.full_size, file.type_size
    );
    println!("first_frame_offset\t{}", file.first_frame_offset);
    println!("keyframes\t{}", file.keyframes());
    println!("palette_frames\t{}", file.palette_frames());
    println!("video_bytes\t{}", file.video_bytes());
    println!("audio_bytes\t{}", file.audio_bytes());
    println!("unaccounted_tail\t{}", file.unaccounted_tail);
    println!("unknown_header_word\t0x{:08x}", file.unknown_header_word);
    for (index, track) in file.audio.iter().enumerate() {
        if !track.present() {
            continue;
        }
        println!(
            "audio_track\t{index}\trate={}\tbits={}\tchannels={}\tcompressed={}\tbink={}\t\
             unpacked-buffer={}\tunaccounted-bits=0x{:08x}",
            track.sample_rate(),
            track.bits_per_sample(),
            track.channels(),
            track.compressed(),
            track.bink_audio(),
            track.unpacked_size,
            track.unaccounted_bits()
        );
        println!(
            "audio_track_totals\t{index}\tunpacked-from-chunks={}\tunpacked-in-timed-frames={}\t\
             expected-from-header={}",
            describe_optional(file.audio_unpacked_bytes(index)),
            describe_optional(file.audio_unpacked_bytes_in_timed_frames(index)),
            describe_optional(file.audio_expected_bytes(index))
        );
    }
    Ok(())
}

fn scan_smacker_directory(directory: &Path) -> Result<(), String> {
    if !directory.is_dir() {
        return Err(format!("not a directory: {}", directory.display()));
    }
    let mut paths = Vec::new();
    collect_paths_with_extension(directory, "smk", &mut paths)?;
    paths.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());

    let mut parsed = 0_usize;
    let mut closed = 0_usize;
    let mut signatures = BTreeMap::<String, usize>::new();
    let mut unknown_header_words = BTreeSet::<u32>::new();
    let mut unaccounted_flags = BTreeSet::<u32>::new();
    let mut unknown_size_flags = 0_usize;
    // The frame-split control: how far the audio summed out of the chunks lands from the total the
    // header's rate and running time predict. A wrong split would not land close.
    let mut worst_audio_drift = 0_u64;
    let mut unrepresentable_controls = 0_usize;
    let mut failures = Vec::new();
    for path in &paths {
        let name = path
            .strip_prefix(directory)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        match SmackerFile::parse(&bytes) {
            Ok(file) => {
                parsed += 1;
                if file.unaccounted_tail == 0 {
                    closed += 1;
                }
                *signatures
                    .entry(String::from_utf8_lossy(&file.signature).into_owned())
                    .or_default() += 1;
                unknown_header_words.insert(file.unknown_header_word);
                unaccounted_flags.insert(file.unaccounted_flag_bits());
                unknown_size_flags += file
                    .frames
                    .iter()
                    .filter(|frame| frame.unknown_size_flag)
                    .count();
                for track in 0..smacker::AUDIO_TRACKS {
                    match (
                        file.audio_unpacked_bytes_in_timed_frames(track),
                        file.audio_expected_bytes(track),
                    ) {
                        // `abs_diff`, not a signed subtraction of two `u64`s: the difference is
                        // what is wanted, and casting both sides to `i64` to get it is another
                        // declared-value overflow waiting behind a plausible-looking number.
                        (Some(found), Some(expected)) => {
                            worst_audio_drift = worst_audio_drift.max(found.abs_diff(expected));
                        }
                        // Not folded into the maximum as a zero: a control that cannot be computed
                        // is not a control that came out well.
                        _ => unrepresentable_controls += 1,
                    }
                }
                println!("{}", smacker::describe(&name, &file));
            }
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    println!("files\t{}", paths.len());
    println!("parsed\t{parsed}");
    println!("sizes_account_for_every_byte\t{closed}");
    println!("frames_with_unknown_size_flag\t{unknown_size_flags}");
    println!("worst_audio_total_drift_bytes\t{worst_audio_drift}");
    println!("unrepresentable_audio_controls\t{unrepresentable_controls}");
    for (signature, count) in &signatures {
        println!("signature\t{signature}\t{count}");
    }
    println!(
        "unknown_header_word_values\t{}",
        unknown_header_words
            .iter()
            .map(|value| format!("0x{value:08x}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    println!(
        "unaccounted_flag_bit_values\t{}",
        unaccounted_flags
            .iter()
            .map(|value| format!("0x{value:08x}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    println!("failures\t{}", failures.len());
    for failure in &failures {
        println!("failure\t{failure}");
    }
    // The result this command reports IS the size closure, so a file that parses but leaves bytes
    // over is a failure of the claim even though it is not a parse failure. Exiting zero on it
    // would let `sizes_account_for_every_byte < parsed` pass unnoticed.
    if !failures.is_empty() {
        return Err(format!("{} file(s) failed", failures.len()));
    }
    if closed != parsed {
        return Err(format!(
            "{} of {parsed} file(s) leave bytes unaccounted for",
            parsed - closed
        ));
    }
    if unrepresentable_controls != 0 {
        return Err(format!(
            "{unrepresentable_controls} audio control(s) could not be computed"
        ));
    }
    Ok(())
}

/// Render an optional measurement, naming the absent case rather than printing a plausible zero.
fn describe_optional(value: Option<u64>) -> String {
    value.map_or_else(|| "unrepresentable".to_owned(), |value| value.to_string())
}

/// Collect every file under `directory` whose extension matches, case-insensitively.
///
/// Separate from `collect_map_paths` rather than a generalisation of it: that one filters on
/// `map_kind`, which is a classification and not an extension, and folding the two would make the
/// map walk depend on a string it does not use.
fn collect_paths_with_extension(
    directory: &Path,
    extension: &str,
    paths: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not read directory {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "could not read directory entry in {}: {error}",
                directory.display()
            )
        })?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_paths_with_extension(&path, extension, paths)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        {
            paths.push(path);
        }
    }
    Ok(())
}

/// Re-encode every PBM in an archive and report two numbers that must not be
/// conflated.
///
/// **Pixel-lossless** is the correctness claim: the pixels survive a decode,
/// encode and decode. Anything short of every image is a bug here.
/// **Byte-identical** is a fidelity observation about the *original* packer's
/// choices, which nothing in this project has reverse-engineered. A miss there
/// means our packet boundaries differ from theirs, not that pixels were lost, so
/// it is reported rather than treated as a failure.
fn roundtrip_pbm(source: &Source) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let mut checked = 0_usize;
    let mut pixel_lossless = 0_usize;
    let mut body_identical = 0_usize;
    let mut file_identical = 0_usize;
    let mut odd_width = 0_usize;
    let mut body_bytes_original = 0_usize;
    let mut body_bytes_written = 0_usize;
    let mut failures = Vec::new();
    let mut differences = Vec::new();

    for entry in &entries {
        let bytes = match archive.read(&entry.name) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{}: could not read: {error}", entry.name));
                continue;
            }
        };
        // Only PBM members are in scope. ILBM is planar and this encoder does
        // not write it, so skipping is honest rather than a silent pass.
        if !matches!(
            probe(&entry.name, &bytes).map(|info| info.kind),
            Ok(AssetKind::IffPbm)
        ) {
            continue;
        }
        let file = match PbmFile::parse(&bytes) {
            Ok(file) => file,
            Err(error) => {
                failures.push(format!("{}: {error}", entry.name));
                continue;
            }
        };
        checked += 1;
        if file.image.width % 2 == 1 {
            odd_width += 1;
        }

        let encoded = match file.encode() {
            Ok(encoded) => encoded,
            Err(error) => {
                failures.push(format!("{}: could not re-encode: {error}", entry.name));
                continue;
            }
        };
        let rewritten = match PbmFile::parse(&encoded) {
            Ok(rewritten) => rewritten,
            Err(error) => {
                failures.push(format!(
                    "{}: re-encoded file does not parse: {error}",
                    entry.name
                ));
                continue;
            }
        };
        if rewritten.image.indices == file.image.indices {
            pixel_lossless += 1;
        } else {
            let at = rewritten
                .image
                .indices
                .iter()
                .zip(&file.image.indices)
                .position(|(wrote, read)| wrote != read);
            failures.push(format!(
                "{}: pixels changed (first differing pixel {at:?})",
                entry.name
            ));
        }

        // Pixels surviving is not the whole claim. `BODY` is the only chunk a
        // re-encode is allowed to rewrite; CRNG colour cycling, DPPS and the
        // TINY thumbnail must come back byte for byte and in order. Without
        // this, deleting the chunk-preserving arm of the encoder outright still
        // printed `failures 0` here while 917 files silently lost them --
        // `byte-identical-file` is only a handful of members and cannot serve
        // as the gate. This path re-encodes the *same* pixels, so TINY is
        // preserved rather than dropped and the assertion covers it too.
        let original_chunks = chunks_other_than_body(&file);
        let written_chunks = chunks_other_than_body(&rewritten);
        if original_chunks != written_chunks {
            failures.push(format!(
                "{}: non-BODY chunks changed: theirs={} ours={}",
                entry.name,
                describe_chunks(&original_chunks),
                describe_chunks(&written_chunks),
            ));
        }

        let original_body = body_chunk(&file).unwrap_or_default();
        let written_body = body_chunk(&rewritten).unwrap_or_default();
        body_bytes_original += original_body.len();
        body_bytes_written += written_body.len();
        if original_body == written_body {
            body_identical += 1;
        } else {
            let at = original_body
                .iter()
                .zip(written_body)
                .position(|(read, wrote)| read != wrote)
                .unwrap_or_else(|| original_body.len().min(written_body.len()));
            differences.push(format!(
                "{}\t{}x{}\tfirst-differing-body-byte={at}\ttheirs={}\tours={}\ttheir-body={}\tour-body={}",
                entry.name,
                file.image.width,
                file.image.height,
                describe_byte(original_body, at),
                describe_byte(written_body, at),
                original_body.len(),
                written_body.len(),
            ));
        }
        if encoded == bytes {
            file_identical += 1;
        }
    }

    println!("checked\t{checked}");
    println!("pixel-lossless\t{pixel_lossless}");
    println!("byte-identical-body\t{body_identical}");
    println!("byte-identical-file\t{file_identical}");
    println!("odd-width\t{odd_width}");
    println!("their-body-bytes\t{body_bytes_original}");
    println!("our-body-bytes\t{body_bytes_written}");
    println!("body-differences\t{}", differences.len());
    for difference in differences.iter().take(10) {
        println!("difference\t{}", clean_field(difference));
    }
    println!("failures\t{}", failures.len());
    for failure in &failures {
        println!("failure\t{}", clean_field(failure));
    }

    if !failures.is_empty() {
        return Err(format!("{} PBM images did not round-trip", failures.len()));
    }
    // Zero images checked is not a pass, for the same reason `--map-roundtrip`
    // refuses an empty sweep: a mistyped archive would otherwise print a green
    // report about nothing.
    if checked == 0 {
        return Err(format!(
            "no PBM images were checked in {}",
            source.archive.display()
        ));
    }
    Ok(())
}

/// Which of the several legal packed sizes a frame stored its pixels in.
///
/// This is the whole reason IMP re-encoding is not a matter of picking a layout. The variants are
/// counted separately because [`Unobservable`](PackedSizeChoice::Unobservable) proves nothing: at
/// 8bpp, and at any depth whose rows fill whole bytes, both layouts are the same length and the
/// original's choice left no trace. Only the frames where the sizes differ are evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PackedSizeChoice {
    /// The two layouts are the same *bytes*, not merely the same length, so there was never a
    /// choice to record. Two independent ways to land here: a row's bits fill whole bytes, which is
    /// always so at 8bpp; or the frame is a single row, where "restart each row on a byte boundary"
    /// has nothing to restart.
    Identical,
    /// The two layouts are the same length and **different bytes**. The frame's stored size names
    /// neither, and `unpack_pixels` resolves it as tight unconditionally -- so a frame the original
    /// tool wrote row-padded at one of these shapes is being decoded wrong today, and no
    /// round-trip can notice, because the re-encode reproduces whatever the decode produced.
    AmbiguousLength,
    /// One continuous bitstream, shorter than the row-padded form.
    Tight,
    /// Every scanline restarts on a byte boundary.
    RowPadded,
    /// The tight form with its final partial byte omitted; 1bpp only.
    TightFloor,
}

impl PackedSizeChoice {
    /// Names the layout a stored length implies, or `None` when it implies none.
    ///
    /// Order matters and is not arbitrary. `tight_ceil == row_padded` is the common case and is
    /// reported *first*, because calling such a frame "tight" would claim evidence the frame does
    /// not carry. That case then splits on whether the layouts are also the same bytes, which the
    /// caller decides and passes in. Equal lengths alone do not settle it -- 7x5 at 1bpp is five
    /// bytes under both layouts and five *different* bytes.
    fn classify(packed_size: usize, sizes: PackedSizes, layouts_identical: bool) -> Option<Self> {
        if packed_size == sizes.tight_ceil && packed_size == sizes.row_padded {
            if layouts_identical {
                Some(Self::Identical)
            } else {
                Some(Self::AmbiguousLength)
            }
        } else if packed_size == sizes.tight_ceil {
            Some(Self::Tight)
        } else if packed_size == sizes.row_padded {
            Some(Self::RowPadded)
        } else if packed_size == sizes.tight_floor {
            Some(Self::TightFloor)
        } else {
            None
        }
    }

    /// Every variant, so a report prints a zero rather than omitting the line. A missing row reads
    /// as "not measured"; `frames=0` reads as "measured, none".
    const ALL: [Self; 5] = [
        Self::Identical,
        Self::AmbiguousLength,
        Self::Tight,
        Self::RowPadded,
        Self::TightFloor,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Identical => "identical-layouts",
            Self::AmbiguousLength => "ambiguous-same-length",
            Self::Tight => "tight",
            Self::RowPadded => "row-padded",
            Self::TightFloor => "tight-floor",
        }
    }
}

/// The two "this instrument could not have seen it" verdicts for one frame, as arithmetic with no
/// archive in it.
///
/// Extracted from the sweep because the published bound -- 752 frames unobservable, and the gap
/// evidence clearing **none** of them -- rests entirely on three numbers that a corpus run can only
/// confirm in aggregate: the gating condition, the `2 * ceil(delta / MAX_RLE_REPEAT)` cost of an
/// unread continuation, and the `< 8` comparison against the archive's payload alignment. A sweep
/// over 41,373 real frames prints one total; it cannot say the `130` was right, because every frame
/// in the corpus happens to land on the same side of that boundary. These are unit-testable only
/// once they are a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LayoutObservability {
    /// The decoder stopped short of a row-padded layout it could not have seen. See
    /// [`imp::layout_is_unobservable`], which the writer reports on the same rule.
    unobservable: bool,
    /// Of those, the ones the payload-gap evidence cannot clear either.
    ///
    /// A row-padded frame read as tight leaves `row_padded - tight_ceil` packed bytes unconsumed.
    /// Those bytes cost at least two stored bytes per RLE repeat packet, so at least
    /// `2 * ceil(delta / MAX_RLE_REPEAT)` stored bytes. Where that minimum is 8 or more, a gap of 8
    /// or more would have to exist between two payloads and none does -- the histogram tops out at
    /// 7. Where it is under 8, the shortfall hides inside the archive's 8-byte payload alignment and
    /// the gaps say nothing.
    ///
    /// **Implies `unobservable`.** A frame the decoder *could* have seen row-padded needs no
    /// clearing, so this is a subset counter and never a second population; the sweep prints both
    /// and they are equal over `imp.mpq`.
    not_cleared_by_gaps: bool,
}

impl LayoutObservability {
    /// The archive's payload alignment, and so the size of shortfall a gap could not distinguish
    /// from ordinary padding. Named rather than inlined because it is the same 8 as
    /// `imp::PAYLOAD_ALIGNMENT` and for the same reason, not a coincidence.
    const GAP_RESOLUTION: usize = 8;

    fn for_frame(
        packed_size: usize,
        sizes: PackedSizes,
        record_variant: u8,
        compressed: bool,
    ) -> Self {
        if !layout_is_unobservable(packed_size, sizes, record_variant, compressed) {
            return Self::default();
        }
        let delta = sizes.row_padded - sizes.tight_ceil;
        let minimum_extra = 2 * delta.div_ceil(MAX_RLE_REPEAT);
        Self {
            unobservable: true,
            not_cleared_by_gaps: minimum_extra < Self::GAP_RESOLUTION,
        }
    }
}

/// Bucket a writer refusal by its cause, so the sweep reports a distribution rather than 40,000
/// distinct strings.
///
/// Matching on message text is fragile, so an unrecognised message is reported as `other` rather
/// than folded into a neighbour; a nonzero `other` is a signal that this list has fallen behind
/// `write_frame_pixels`, not a rounding error.
fn refusal_class(message: &str) -> &'static str {
    if message.contains("is a duplicate of frame") {
        "duplicate-record"
    } else if message.contains("shares its pixels with") {
        "shared-pixels"
    } else if message.contains("zero-length payload") {
        "zero-length-payload"
    } else if message.contains("carries no pixel payload") {
        "no-payload"
    } else if message.contains("is empty") {
        "empty-frame"
    } else if message.contains("16-bit encoded size") {
        "encoded-size-overflow"
    } else if message.contains("reads back as") {
        "does-not-read-back"
    } else {
        "other"
    }
}

/// Frames checked, frames whose pixels survived, and frames whose bytes came back identical.
#[derive(Debug, Clone, Copy, Default)]
struct ImpRoundtripTally {
    checked: usize,
    pixel_lossless: usize,
    byte_identical: usize,
}

impl ImpRoundtripTally {
    fn record(&mut self, pixel_lossless: bool, byte_identical: bool) {
        self.checked += 1;
        self.pixel_lossless += usize::from(pixel_lossless);
        self.byte_identical += usize::from(byte_identical);
    }
}

/// Re-encode every frame of every IMP in an archive and report three numbers that must not be
/// conflated.
///
/// **Pixel-lossless** is the correctness claim: `unpack(repack(unpack(x))) == unpack(x)`, checked
/// by pushing the re-encoded payload back through [`read_frame_pixels`] — the parser's own code —
/// rather than through a second decoder that could share this encoder's mistakes. Anything short of
/// every frame is a bug here.
///
/// **Byte-identical** is a fidelity observation about the *original* packer, which nothing in this
/// project has reverse-engineered. A miss means our packet boundaries or our padding bits differ
/// from theirs, not that pixels were lost, so it is reported rather than failed.
///
/// **The packed-size distribution** is neither. It is the measurement that decides whether a frame
/// replacement can preserve a frame's length at all: a frame stores its pixels tight or row-padded,
/// the parser accepts both, and an encoder that chooses for itself silently resizes the frame.
fn roundtrip_imp(source: &Source, rewrite: bool) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let mut members = 0_usize;
    let mut duplicate_frames = 0_usize;
    let mut empty_frames = 0_usize;
    let mut their_pixel_bytes = 0_usize;
    let mut our_pixel_bytes = 0_usize;
    let mut total = ImpRoundtripTally::default();
    let mut by_variant = BTreeMap::<u8, ImpRoundtripTally>::new();
    let mut by_depth = BTreeMap::<u8, ImpRoundtripTally>::new();
    let mut by_choice = BTreeMap::<PackedSizeChoice, ImpRoundtripTally>::new();
    // How many frames the sweep could not have seen a row-padded layout on even if they had one,
    // and how many of those the payload-gap evidence cannot clear either. The rule for both, and
    // the reason neither covers the equal-length ambiguous frames, is on `LayoutObservability`.
    let mut layout_unreachable = 0_usize;
    let mut layout_unreachable_and_unclearable = 0_usize;
    // Frames whose *shape* is ambiguous, which is not the same population as the
    // `ambiguous-same-length` choice above and is printed so the difference can be read rather than
    // guessed at. `layout_is_ambiguous` asks only about width, height and depth; the choice also
    // requires the stored size to BE that common length, so an ambiguous-shaped 1bpp frame that
    // dropped its final partial byte classifies as `tight-floor` while still being undecidable.
    let mut layout_ambiguous_shape = 0_usize;
    // The cross-check for that counter, and it deliberately does not share its mechanism.
    //
    // Payload spans are taken from the *file's own* pixel pointers, not from the decoder's stop
    // rule. If a compressed variant-0 stream really stopped early -- read as tight when the frame
    // was row-padded -- the span this sweep measured would fall short of where the next payload
    // begins, and a gap would appear here. Payloads that abut exactly are evidence the decoder
    // consumed the whole stream, arrived at independently.
    let mut payload_pairs = 0_usize;
    let mut payload_abutting = 0_usize;
    let mut payload_gaps = 0_usize;
    let mut payload_overlaps = 0_usize;
    let mut payload_gap_sizes = BTreeMap::<usize, usize>::new();
    // Every distinct payload start, and how many of them sit on an 8-byte boundary. This is the
    // measurement `imp::PAYLOAD_ALIGNMENT` rests on; the gap histogram is consistent with it but
    // does not state it, so it is counted directly rather than inferred from the gaps.
    let mut payload_starts = 0_usize;
    let mut payload_starts_aligned = 0_usize;
    // Records carrying their own pixel pointer, counted BEFORE the distinct-offset dedup.
    //
    // `payload_starts` is a count of offsets and `frames` is a count of frames, and a report that
    // pairs them is asserting they coincide. They need not: two ordinary records may point at one
    // payload -- `ImpSprite::frames_sharing_pixels` exists precisely because they can -- and each
    // such pair costs one start without costing a frame. So the coincidence is measured here rather
    // than assumed, and the difference from `payload_starts` is the number of shared payloads.
    // (Duplicate and shared-pixel records cannot contribute: the parser leaves their
    // `pixels_offset` as `None`, so they are absent from both populations. A zero-by-zero frame
    // does carry a pointer and so is counted here while `frames` skips it, which is why
    // `empty-frames` is printed too; it is 0 in this archive.)
    let mut payload_offset_records = 0_usize;
    // The `--rewrite` tally: frames put back through the whole-file writer with their own
    // unmodified pixels, and how many produced a byte-identical file.
    let mut rewrite_attempted = 0_usize;
    let mut rewrite_identical = 0_usize;
    let mut rewrite_differs_by_payload = 0_usize;
    let mut rewrite_refused = BTreeMap::<&'static str, usize>::new();
    let mut rewrite_ambiguous = 0_usize;
    // The writer's own report of the 752-frame class, counted here so the two flags it now returns
    // are both measured over the corpus rather than only the smaller one.
    let mut rewrite_unobservable = 0_usize;
    let mut failures = Vec::new();
    let mut differences = Vec::new();

    for entry in &entries {
        let bytes = match archive.read(&entry.name) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{}: could not read: {error}", entry.name));
                continue;
            }
        };
        if !matches!(
            probe(&entry.name, &bytes).map(|info| info.kind),
            Ok(AssetKind::ImpSprite)
        ) {
            continue;
        }
        let sprite = match ImpSprite::parse(&bytes) {
            Ok(sprite) => sprite,
            Err(error) => {
                failures.push(format!("{}: {error}", entry.name));
                continue;
            }
        };
        members += 1;

        // Distinct payload starts only: shared-pixel records alias an earlier frame's bytes, and
        // counting an alias as a second payload would manufacture an overlap.
        let mut spans = BTreeMap::<usize, usize>::new();
        for frame in &sprite.frames {
            if let (Some(offset), Some(size)) = (frame.pixels_offset, frame.stored_size) {
                let span = spans.entry(offset).or_insert(size);
                *span = (*span).max(size);
            }
        }
        let starts: Vec<(usize, usize)> = spans.into_iter().collect();
        payload_starts += starts.len();
        payload_offset_records += sprite
            .frames
            .iter()
            .filter(|frame| frame.pixels_offset.is_some())
            .count();
        payload_starts_aligned += starts.iter().filter(|(offset, _)| offset % 8 == 0).count();
        for pair in starts.windows(2) {
            let [(offset, size), (next_offset, _)] = pair else {
                continue;
            };
            payload_pairs += 1;
            match (offset + size).cmp(next_offset) {
                std::cmp::Ordering::Equal => payload_abutting += 1,
                std::cmp::Ordering::Less => {
                    payload_gaps += 1;
                    *payload_gap_sizes
                        .entry(next_offset - (offset + size))
                        .or_default() += 1;
                }
                std::cmp::Ordering::Greater => payload_overlaps += 1,
            }
        }

        for (index, frame) in sprite.frames.iter().enumerate() {
            // Duplicate and shared-pixel records carry no payload of their own; re-encoding one
            // would be re-encoding whichever frame it points at, counted twice.
            let (Some(packed_size), Some(pixels_offset), Some(stored_size)) =
                (frame.packed_size, frame.pixels_offset, frame.stored_size)
            else {
                duplicate_frames += 1;
                continue;
            };
            if frame.width == 0 || frame.height == 0 {
                empty_frames += 1;
                continue;
            }

            let sizes =
                match PackedSizes::for_frame(frame.width, frame.height, sprite.bits_per_pixel) {
                    Ok(sizes) => sizes,
                    Err(error) => {
                        failures.push(format!("{} frame {index}: {error}", entry.name));
                        continue;
                    }
                };
            let layouts_identical =
                layouts_are_identical(frame.width, frame.height, sprite.bits_per_pixel);
            let observability = LayoutObservability::for_frame(
                packed_size,
                sizes,
                sprite.record_variant,
                sprite.compressed,
            );
            if observability.unobservable {
                layout_unreachable += 1;
            }
            if observability.not_cleared_by_gaps {
                layout_unreachable_and_unclearable += 1;
            }
            match imp::layout_is_ambiguous(frame.width, frame.height, sprite.bits_per_pixel) {
                Ok(true) => layout_ambiguous_shape += 1,
                Ok(false) => {}
                Err(error) => failures.push(format!("{} frame {index}: {error}", entry.name)),
            }
            let Some(choice) = PackedSizeChoice::classify(packed_size, sizes, layouts_identical)
            else {
                failures.push(format!(
                    "{} frame {index}: stored packed size {packed_size} matches none of {sizes:?}",
                    entry.name
                ));
                continue;
            };

            let packed = match pack_pixels(
                &frame.palette_indices,
                frame.width,
                frame.height,
                sprite.bits_per_pixel,
                packed_size,
            ) {
                Ok(packed) => packed,
                Err(error) => {
                    failures.push(format!(
                        "{} frame {index}: could not pack: {error}",
                        entry.name
                    ));
                    continue;
                }
            };
            let payload = if sprite.compressed {
                encode_rle(&packed)
            } else {
                packed
            };

            // Read our own payload back with the parser's code, standing where the file's pixel
            // pointer would stand. This is what catches a `record_variant == 0` stream that halts
            // on the tight size when the frame is row-padded: the pixels come back rearranged
            // rather than merely differently packed.
            let reread = read_frame_pixels(
                &payload,
                0,
                frame.width,
                frame.height,
                sprite.bits_per_pixel,
                sprite.compressed,
                sprite.record_variant,
                payload.len(),
            );
            let (repacked, consumed) = match reread {
                Ok(result) => result,
                Err(error) => {
                    failures.push(format!(
                        "{} frame {index}: re-encoded payload does not decode: {error}",
                        entry.name
                    ));
                    continue;
                }
            };
            if consumed != payload.len() {
                failures.push(format!(
                    "{} frame {index}: re-encoded payload is {} bytes but the parser stops after {consumed}",
                    entry.name,
                    payload.len()
                ));
                continue;
            }
            if repacked.len() != packed_size {
                failures.push(format!(
                    "{} frame {index}: re-encoded payload decodes to {} packed bytes, not {packed_size}",
                    entry.name,
                    repacked.len()
                ));
                continue;
            }
            let indices =
                match unpack_pixels(&repacked, frame.width, frame.height, sprite.bits_per_pixel) {
                    Ok(indices) => indices,
                    Err(error) => {
                        failures.push(format!(
                            "{} frame {index}: could not unpack: {error}",
                            entry.name
                        ));
                        continue;
                    }
                };

            let pixel_lossless = indices == frame.palette_indices;
            let theirs = bytes
                .get(pixels_offset..pixels_offset + stored_size)
                .unwrap_or_default();
            let byte_identical = theirs == payload.as_slice();
            their_pixel_bytes += stored_size;
            our_pixel_bytes += payload.len();
            total.record(pixel_lossless, byte_identical);
            by_variant
                .entry(sprite.record_variant)
                .or_default()
                .record(pixel_lossless, byte_identical);
            by_depth
                .entry(sprite.bits_per_pixel)
                .or_default()
                .record(pixel_lossless, byte_identical);
            by_choice
                .entry(choice)
                .or_default()
                .record(pixel_lossless, byte_identical);

            if !pixel_lossless {
                let at = indices
                    .iter()
                    .zip(&frame.palette_indices)
                    .position(|(wrote, read)| wrote != read);
                failures.push(format!(
                    "{} frame {index}: pixels changed (first differing pixel {at:?})",
                    entry.name
                ));
            } else if !byte_identical && differences.len() < 10 {
                let at = theirs
                    .iter()
                    .zip(&payload)
                    .position(|(read, wrote)| read != wrote)
                    .unwrap_or_else(|| theirs.len().min(payload.len()));
                differences.push(format!(
                    "{} frame {index}\t{}x{}\t{}bpp\tvariant={}\t{}\tfirst-differing-byte={at}\ttheirs={}\tours={}\ttheir-bytes={stored_size}\tour-bytes={}",
                    entry.name,
                    frame.width,
                    frame.height,
                    sprite.bits_per_pixel,
                    sprite.record_variant,
                    choice.label(),
                    describe_byte(theirs, at),
                    describe_byte(&payload, at),
                    payload.len(),
                ));
            }

            if !rewrite {
                continue;
            }
            // The acceptance test that matters: put the frame back with its OWN pixels through the
            // whole-file writer and require the file to come back unchanged. Nothing synthetic
            // reaches this bar -- it exercises every record, every pointer and every uninitialised
            // leftover byte in 1,800 real files at once.
            match write_frame_pixels(&bytes, index, &frame.palette_indices) {
                Ok(write) => {
                    rewrite_attempted += 1;
                    if write.layout_ambiguous {
                        rewrite_ambiguous += 1;
                    }
                    if write.layout_unobservable {
                        rewrite_unobservable += 1;
                    }
                    if write.bytes == bytes {
                        rewrite_identical += 1;
                    } else if !byte_identical {
                        // Expected, and it is the *payload* that differs, not the rewrite: this
                        // frame is already one of the `byte-differences` above, where our RLE packet
                        // boundaries differ from the original packer's. A whole-file rewrite cannot
                        // be more faithful than the payload it splices in.
                        rewrite_differs_by_payload += 1;
                    } else {
                        // A frame whose payload re-encodes to the original bytes and whose file
                        // still changed is a defect in the *writer*: the pointer rewriting, the
                        // alignment padding, or the byte-for-byte preservation. Nothing else is
                        // left to blame, which is what makes this a sharp gate.
                        failures.push(format!(
                            "{} frame {index}: identity rewrite changed the file although its payload is byte-identical (shift={}, stored {}->{}, padding={})",
                            entry.name,
                            write.shift,
                            write.stored_size.0,
                            write.stored_size.1,
                            write.alignment_padding,
                        ));
                    }
                }
                Err(error) => {
                    *rewrite_refused
                        .entry(refusal_class(&error.to_string()))
                        .or_default() += 1;
                }
            }
        }
    }

    println!("members\t{members}");
    println!("frames\t{}", total.checked);
    println!("duplicate-frames\t{duplicate_frames}");
    println!("empty-frames\t{empty_frames}");
    println!("pixel-lossless\t{}", total.pixel_lossless);
    println!("byte-identical\t{}", total.byte_identical);
    println!("their-pixel-bytes\t{their_pixel_bytes}");
    println!("our-pixel-bytes\t{our_pixel_bytes}");
    for choice in PackedSizeChoice::ALL {
        let tally = by_choice.get(&choice).copied().unwrap_or_default();
        println!(
            "packed-size\t{}\tframes={}\tpixel-lossless={}\tbyte-identical={}",
            choice.label(),
            tally.checked,
            tally.pixel_lossless,
            tally.byte_identical,
        );
    }
    println!("layout-ambiguous-shape\t{layout_ambiguous_shape}");
    println!("layout-unobservable-by-this-decoder\t{layout_unreachable}");
    println!("layout-unobservable-and-not-cleared-by-gaps\t{layout_unreachable_and_unclearable}");
    println!("payload-offset-records\t{payload_offset_records}");
    println!("payload-starts\t{payload_starts}");
    println!(
        "payload-shared-by-two-records\t{}",
        payload_offset_records - payload_starts
    );
    println!("payload-starts-8-byte-aligned\t{payload_starts_aligned}");
    println!("payload-adjacent-pairs\t{payload_pairs}");
    println!("payload-abutting\t{payload_abutting}");
    println!("payload-gaps\t{payload_gaps}");
    println!("payload-overlaps\t{payload_overlaps}");
    let mut ranked: Vec<(usize, usize)> = payload_gap_sizes
        .iter()
        .map(|(size, count)| (*count, *size))
        .collect();
    ranked.sort_unstable_by(|left, right| right.cmp(left));
    println!("payload-gap-distinct-sizes\t{}", ranked.len());
    for (count, size) in ranked.iter().take(8) {
        println!("payload-gap\t{size}\tpairs={count}");
    }
    for (variant, tally) in &by_variant {
        println!(
            "variant\t{variant}\tframes={}\tpixel-lossless={}\tbyte-identical={}",
            tally.checked, tally.pixel_lossless, tally.byte_identical,
        );
    }
    for (depth, tally) in &by_depth {
        println!(
            "bpp\t{depth}\tframes={}\tpixel-lossless={}\tbyte-identical={}",
            tally.checked, tally.pixel_lossless, tally.byte_identical,
        );
    }
    if rewrite {
        println!("rewrite-attempted\t{rewrite_attempted}");
        println!("rewrite-byte-identical\t{rewrite_identical}");
        println!("rewrite-differs-only-by-payload\t{rewrite_differs_by_payload}");
        println!("rewrite-ambiguous-layout\t{rewrite_ambiguous}");
        println!("rewrite-unobservable-layout\t{rewrite_unobservable}");
        println!(
            "rewrite-refused\t{}",
            rewrite_refused.values().sum::<usize>()
        );
        for (reason, count) in &rewrite_refused {
            println!("rewrite-refusal\t{reason}\tframes={count}");
        }
    }
    println!("byte-differences\t{}", total.checked - total.byte_identical);
    for difference in &differences {
        println!("difference\t{}", clean_field(difference));
    }
    println!("failures\t{}", failures.len());
    for failure in failures.iter().take(50) {
        println!("failure\t{}", clean_field(failure));
    }

    if !failures.is_empty() {
        return Err(format!("{} IMP frames did not round-trip", failures.len()));
    }
    // Zero frames checked is not a pass: a mistyped archive would otherwise print a green report
    // about nothing.
    if total.checked == 0 {
        return Err(format!(
            "no IMP frames were checked in {}",
            source.archive.display()
        ));
    }
    Ok(())
}

/// Every chunk a re-encode must hand back untouched: `BODY` is the only one it
/// is allowed to rewrite.
fn chunks_other_than_body(file: &PbmFile) -> Vec<&PbmChunk> {
    file.chunks
        .iter()
        .filter(|chunk| &chunk.id != b"BODY")
        .collect()
}

/// Names a chunk list as `ID:len` pairs, so a mismatch report says which chunk
/// went missing or changed size rather than dumping its bytes.
fn describe_chunks(chunks: &[&PbmChunk]) -> String {
    chunks
        .iter()
        .map(|chunk| {
            format!(
                "{}:{}",
                String::from_utf8_lossy(&chunk.id),
                chunk.data.len()
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn body_chunk(file: &PbmFile) -> Option<&[u8]> {
    file.chunks
        .iter()
        .find(|chunk| &chunk.id == b"BODY")
        .map(|chunk| chunk.data.as_slice())
}

fn describe_byte(source: &[u8], at: usize) -> String {
    source
        .get(at)
        .map_or_else(|| "end-of-body".to_owned(), |byte| format!("0x{byte:02x}"))
}

/// Write an edited indexed PNG back into a PBM.
///
/// The palette, masking, and every chunk the decoder does not model -- `CRNG`,
/// `DPPS`, and the `TINY` thumbnail -- come from `template`, which is normally
/// the file the PNG was exported from. Only the pixels come from the PNG, and
/// the PNG's own palette is checked against the template's rather than adopted,
/// because the indices are meaningless under a different palette.
fn import_png_pbm(png_path: &Path, template_path: &Path, output: &Path) -> Result<(), String> {
    for existing in [png_path, template_path] {
        if paths_are_same_file(existing, output) {
            return Err(format!(
                "refusing to write to the input file {}; pass a different output path",
                existing.display()
            ));
        }
    }

    let template_bytes = fs::read(template_path)
        .map_err(|error| format!("could not read {}: {error}", template_path.display()))?;
    let template = PbmFile::parse(&template_bytes).map_err(|error| error.to_string())?;

    let png_file = fs::File::open(png_path)
        .map_err(|error| format!("could not read {}: {error}", png_path.display()))?;
    // The template's size is handed to the reader rather than checked after it,
    // so a mismatched header is refused before it can size a decode buffer.
    let png = read_indexed_png(
        BufReader::new(png_file),
        (template.image.width, template.image.height),
        "the PBM header",
    )
    .map_err(|error| {
        format!(
            "{}: {error} (template {})",
            png_path.display(),
            template_path.display()
        )
    })?;

    if png.palette.len() > template.image.palette.len() {
        return Err(format!(
            "{} has {} palette entries but {} has {}",
            png_path.display(),
            png.palette.len(),
            template_path.display(),
            template.image.palette.len(),
        ));
    }
    // A PNG whose palette was merely trimmed to the entries it uses is fine; a
    // PNG whose colours were *remapped* is not, because the written file keeps
    // the template's CMAP and every index would then mean a different colour.
    if let Some((index, _)) = png
        .palette
        .iter()
        .zip(&template.image.palette)
        .enumerate()
        .find(|(_, (edited, original))| edited != original)
    {
        return Err(format!(
            "{} changed palette entry {index}; the PBM keeps {}'s CMAP, so re-export and edit \
             only the pixels",
            png_path.display(),
            template_path.display(),
        ));
    }

    let encoded = template
        .encode_with_indices(&png.indices)
        .map_err(|error| error.to_string())?;
    // Read the bytes that are about to be written, the way --set-imp-placement
    // and the map editors do: an encoder bug must not reach a file.
    let written = PbmFile::parse(&encoded).map_err(|error| error.to_string())?;
    if written.image.indices != png.indices {
        return Err("the re-encoded PBM does not read back the pixels it was given".to_owned());
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    file.write_all(&encoded)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "wrote\t{}\t{}\t{}x{}\tpalette={}\tchunks={}",
        output.display(),
        encoded.len(),
        written.image.width,
        written.image.height,
        written.image.palette_entries,
        written.chunks.len(),
    );
    Ok(())
}

/// Write an edited indexed PNG back over one frame of an `.imp`.
///
/// The palette, every table, every record and every byte the decoder does not model come from
/// `template`, which is normally the file the PNG was exported from with `--export-imp-frame`. Only
/// one frame's pixels come from the PNG, and the PNG's own palette is checked against the
/// template's rather than adopted, because the indices are meaningless under a different palette.
///
/// The refusals -- duplicate records, shared payloads, and anything that will not read back -- are
/// [`write_frame_pixels`]'s, and the reason each one exists is documented there.
fn import_png_imp(
    png_path: &Path,
    template_path: &Path,
    frame_index: usize,
    output: &Path,
) -> Result<(), String> {
    for existing in [png_path, template_path] {
        if paths_are_same_file(existing, output) {
            return Err(format!(
                "refusing to write to the input file {}; pass a different output path",
                existing.display()
            ));
        }
    }

    let template_bytes = fs::read(template_path)
        .map_err(|error| format!("could not read {}: {error}", template_path.display()))?;
    let sprite = ImpSprite::parse(&template_bytes).map_err(|error| error.to_string())?;
    // Ask the writer whether this frame is writable **before** the PNG is opened. The frame's
    // dimensions are taken from the record, and a `0x08` duplicate or `0x04` shared-pixel record
    // carries `0x0` there -- 10,293 of them in `imp.mpq`, and `--export-imp-frame` exports them
    // happily by resolving to the frame that owns the payload. Sizing the PNG check off the
    // unresolved record made the re-import of an exported frame fail with "PNG is 24x1 but the
    // template is 0x0", which names the wrong dimensions and says nothing about which frame to edit
    // instead. `frame_pixel_target` returns the refusal that does.
    frame_pixel_target(&sprite, frame_index).map_err(|error| error.to_string())?;
    let frame = sprite
        .frames
        .get(frame_index)
        .ok_or_else(|| format!("IMP frame index {frame_index} is out of range"))?;

    let png_file = fs::File::open(png_path)
        .map_err(|error| format!("could not read {}: {error}", png_path.display()))?;
    // The frame's size is handed to the reader rather than checked after it, so a mismatched header
    // is refused before it can size a decode buffer.
    let png = read_indexed_png(
        BufReader::new(png_file),
        (frame.width, frame.height),
        "the IMP frame record",
    )
    .map_err(
        |error| {
            format!(
                "{}: {error} (template {} frame {frame_index})",
                png_path.display(),
                template_path.display()
            )
        },
    )?;

    // No "too many palette entries" check, unlike the PBM import: an IMP's palette is always
    // exactly 256 entries -- `ImpSprite::parse` reads a fixed 1,024-byte block -- and 256 is also
    // the most a PNG PLTE can hold, so the comparison could never fail. A guard that cannot fire is
    // a guard the next reader will trust for something it does not do.
    //
    // A PNG whose palette was merely trimmed to the entries it uses is fine; a PNG whose colours
    // were *remapped* is not, because the written file keeps the template's palette and every index
    // would then mean a different colour.
    if let Some((index, _)) = png
        .palette
        .iter()
        .zip(&sprite.palette)
        .enumerate()
        .find(|(_, (edited, original))| *edited != &[original[0], original[1], original[2]])
    {
        return Err(format!(
            "{} changed palette entry {index}; the IMP keeps {}'s palette, so re-export and edit \
             only the pixels",
            png_path.display(),
            template_path.display(),
        ));
    }

    let write = write_frame_pixels(&template_bytes, frame_index, &png.indices)
        .map_err(|error| error.to_string())?;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    file.write_all(&write.bytes)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "wrote\t{}\t{}\tframe={frame_index}\t{}x{}\t{}bpp\tlayout={:?}\tstored={}->{}\tpadding={}\tshift={}\toffsets-rewritten={}",
        output.display(),
        write.bytes.len(),
        frame.width,
        frame.height,
        sprite.bits_per_pixel,
        write.layout,
        write.stored_size.0,
        write.stored_size.1,
        write.alignment_padding,
        write.shift,
        write.rewritten_offsets,
    );
    if write.layout_ambiguous {
        // Not a refusal: the file round-trips through this decoder either way. It is a warning
        // because the *engine* may read these bytes row-padded, in which case the pixels exported
        // to the PNG were already scrambled and the edit was made on scrambled art.
        println!(
            "warning\tframe {frame_index} is {}x{} at {}bpp, a shape where the tight and row-padded layouts are the same length and different bytes; the stored size names neither and this decoder reads it as tight",
            frame.width, frame.height, sprite.bits_per_pixel,
        );
    }
    if write.layout_unobservable {
        // The larger sibling of the warning above, and printed in the same shape rather than folded
        // into it. Here the file *does* record the layout; this decoder's variant-0 reader stops at
        // the first acceptable length on a packet boundary and so could not have seen a row-padded
        // one. 752 frames in `imp.mpq` are in this class against 45 in the ambiguous one, and
        // `layout=Tight` above is printed as fact for every one of them.
        //
        // Not a refusal, for the same reason: the file round-trips through this decoder either way,
        // and refusing would block the 752 from being edited at all on a suspicion no measurement
        // here can settle. What it must not do is stay silent -- if the frame was stored
        // row-padded, the exported pixels were already scrambled, and this import splices a tight
        // payload over only the prefix the reader consumed, stranding the original tail.
        println!(
            "warning\tframe {frame_index} is a compressed record-variant-0 frame whose stored size is not the row-padded one, so this decoder's reader stopped before it could observe row padding; layout={:?} is what was read, not what the file necessarily stores",
            write.layout,
        );
    }
    Ok(())
}

fn export_map_preview(
    map_path: &Path,
    definition_path: &Path,
    atlas_path: &Path,
    output: &Path,
) -> Result<(), String> {
    let bytes = fs::read(map_path)
        .map_err(|error| format!("could not read {}: {error}", map_path.display()))?;
    let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
    let preview = load_terrain_preview(&map, definition_path, atlas_path)?;
    let mut encoded = Vec::new();
    write_rgba_png(&mut encoded, preview.width, preview.height, &preview.rgba)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    file.write_all(&encoded)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    println!(
        "wrote\t{}\t{}\t{}x{}\tatlas={}",
        output.display(),
        encoded.len(),
        preview.width,
        preview.height,
        preview.atlas_name,
    );
    Ok(())
}

/// Solve for the placement pair that keeps a frame where it is after its size changed.
///
/// This is the calculation a re-cropping tool needs. It is pure arithmetic over the rule in
/// [`lom_asset_viewer::imp::frame_top_left`], so it takes no file.
fn imp_placement_for(
    width: u16,
    height: u16,
    anchor: (i32, i32),
    top_left: (i32, i32),
) -> Result<(), String> {
    let (x, y) = imp::placement_for_top_left(anchor, top_left, width, height)
        .map_err(|error| error.to_string())?;
    println!("placement\t{x}\t{y}");
    let check = imp::frame_top_left(anchor, (x, y), width, height)
        .map_err(|error| error.to_string())?;
    println!("check\ttop-left {check:?} for a {width}x{height} frame drawn at {anchor:?}");
    Ok(())
}

/// Rewrite one frame's placement in a loose IMP file.
///
/// Works on a file rather than an archive member because the workflow it serves is repairing a
/// sprite a third-party tool emitted, before it is packed back into an MPQ.
fn set_imp_placement(
    input: &Path,
    frame: usize,
    x: i16,
    y: i16,
    hotspot: Option<u16>,
    output: &Path,
) -> Result<(), String> {
    let source = fs::read(input)
        .map_err(|error| format!("could not read {}: {error}", input.display()))?;
    let sprite = ImpSprite::parse(&source).map_err(|error| error.to_string())?;

    // The unit of sharing differs by path: an origin lives in the frame record, a hotspot lives in
    // the array the record points at, and two distinct records can point at one array.
    let shared = match hotspot {
        Some(_) => sprite.frames_sharing_hotspots(frame),
        None => sprite.frames_sharing_record(frame),
    }
    .map_err(|error| error.to_string())?;
    if shared.len() > 1 {
        let unit = if hotspot.is_some() { "hotspot array" } else { "record" };
        eprintln!("note: frames {shared:?} share one {unit}, so this writes all of them");
    }

    let patched = match hotspot {
        Some(id) => imp::write_frame_hotspot(&source, frame, id, x, y),
        None => imp::write_frame_origin(&source, frame, x, y),
    }
    .map_err(|error| error.to_string())?;

    // Re-parse before writing: a file we cannot read back is a file we must not emit.
    let reparsed = ImpSprite::parse(&patched).map_err(|error| {
        format!("refusing to write: the patched sprite no longer parses: {error}")
    })?;
    let written = &reparsed.frames[frame];
    let observed = match hotspot {
        Some(id) => written
            .hotspots
            .iter()
            .find(|spot| spot.id == id)
            .map(|spot| (spot.x, spot.y)),
        None => written.origin_x.zip(written.origin_y),
    };
    if observed != Some((x, y)) {
        return Err(format!(
            "refusing to write: expected placement ({x}, {y}) but read back {observed:?}"
        ));
    }

    // Create-new, like every other output path in this tool. This is the one command that mutates
    // game art, so silently truncating an existing file is the worst place to allow it.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    file.write_all(&patched)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    let changed = source
        .iter()
        .zip(&patched)
        .filter(|(before, after)| before != after)
        .count();
    println!("wrote\t{}\t{changed} bytes changed", output.display());
    Ok(())
}

fn describe_imp(source: &Source, member: &str) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| format!("archive has no member named {member}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let sprite = ImpSprite::parse(&bytes).map_err(|error| error.to_string())?;
    let sequence_labels = load_imp_sequence_labels(&archive, &entries, &entry.name);

    println!("record\tindex\towner\tlabels-or-flags\tmetadata-or-size\tfirst\tcount\tplacement");
    for (sequence_index, sequence) in sprite.sequences.iter().enumerate() {
        let labels = sequence_labels
            .get(sequence_index)
            .filter(|labels| !labels.is_empty())
            .map(|labels| labels.join("|"))
            .unwrap_or_else(|| "unnamed".to_owned());
        println!(
            "sequence\t{sequence_index}\t-\t{}\t{}\tfacing:{};frame:{}\tfacing:{};frame:{}\t-",
            clean_field(&labels),
            hex_bytes(&sequence.metadata),
            sequence.first_facing,
            sequence.first_frame,
            sequence.facing_count,
            sequence.frame_count,
        );
        for facing_index in sequence.first_facing..sequence.first_facing + sequence.facing_count {
            let facing = &sprite.facings[facing_index];
            println!(
                "facing\t{facing_index}\tsequence:{sequence_index}\t-\t0x{:04x}\tframe:{}\tframe:{}\t-",
                facing.metadata, facing.first_frame, facing.frame_count,
            );
        }
    }
    for (frame_index, frame) in sprite.frames.iter().enumerate() {
        let (sequence_index, facing_index, frame_in_facing) = sprite
            .frame_location(frame_index)
            .map_err(|error| error.to_string())?;
        let resolved = sprite
            .resolved_frame(frame_index)
            .map_err(|error| error.to_string())?;
        let placement = if !frame.hotspots.is_empty() {
            frame
                .hotspots
                .iter()
                .map(|hotspot| format!("{}:{}:{}", hotspot.id, hotspot.x, hotspot.y))
                .collect::<Vec<_>>()
                .join("|")
        } else if let (Some(x), Some(y)) = (frame.origin_x, frame.origin_y) {
            format!("origin:{x}:{y}")
        } else {
            "inherited-or-empty".to_owned()
        };
        let source_frame = frame
            .source_frame
            .map_or_else(|| "direct".to_owned(), |index| format!("source:{index}"));
        println!(
            "frame\t{frame_index}\tsequence:{sequence_index};facing:{facing_index};offset:{frame_in_facing}\t0x{:02x};{source_frame}\t{}x{}\t-\t{}\t{}",
            frame.flags,
            resolved.width,
            resolved.height,
            frame.hotspots.len(),
            clean_field(&placement),
        );
    }
    Ok(())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn catalog_archive(source: &Source) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    println!("name\tsize\tkind\tdetails");
    for entry in &entries {
        match archive.read(&entry.name) {
            Ok(bytes) => match probe(&entry.name, &bytes) {
                Ok(info) => println!(
                    "{}\t{}\t{}\t{}",
                    clean_field(&entry.name),
                    entry.size,
                    info.kind,
                    clean_field(&info.details)
                ),
                Err(error) => println!(
                    "{}\t{}\tinvalid\t{}",
                    clean_field(&entry.name),
                    entry.size,
                    clean_field(&error)
                ),
            },
            Err(error) => println!(
                "{}\t{}\tunreadable\t{}",
                clean_field(&entry.name),
                entry.size,
                clean_field(&error.to_string())
            ),
        }
    }
    Ok(())
}

fn inspect_archive(source: &Source, requested: Option<&str>) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let entry = match requested {
        Some(name) => entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("archive has no member named {name}"))?,
        None => entries
            .first()
            .ok_or_else(|| "archive is empty".to_owned())?,
    };
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let info = probe(&entry.name, &bytes)?;
    println!("name\t{}", entry.name);
    println!("size\t{}", entry.size);
    println!("kind\t{}", info.kind);
    println!("details\t{}", info.details);
    Ok(())
}

fn inspect_file(path: &Path) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("file name is not valid UTF-8: {}", path.display()))?;
    let info = probe(name, &bytes)?;
    println!("path\tsize\tkind\tdetails");
    println!(
        "{}\t{}\t{}\t{}",
        clean_field(&path.display().to_string()),
        bytes.len(),
        info.kind,
        clean_field(&info.details)
    );
    Ok(())
}

/// Print the header and every requested cell of a map, one line per cell.
///
/// Prints `x`, `y`, the packed cell index, the raw tag, the masked tile index, whether the
/// unexplained `0x00800000` flag is set, and the second word as a float. With no rect it dumps the
/// whole grid; `X0 Y0 X1 Y1` bounds it inclusively, which is what makes a 64x64 probe map readable.
///
/// `--describe-map` only prints the trailing sprite records, and refuses a file whose tail is not
/// the 49-byte family. The cells themselves -- the half of the format issue #4 is actually about
/// -- had no way out of the parser at all. Writing a map from the engine with values chosen by
/// the probe is only useful if those values can be read back, so this is the readback.
fn dump_map_cells(path: &Path, rect: Option<(u32, u32, u32, u32)>) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
    let (x0, y0, x1, y1) = rect.unwrap_or((0, 0, map.width - 1, map.height - 1));
    if x1 >= map.width || y1 >= map.height || x0 > x1 || y0 > y1 {
        return Err(format!(
            "rect {x0},{y0}..{x1},{y1} is not inside a {}x{} map",
            map.width, map.height
        ));
    }
    println!(
        "map\t{}\t{}x{}\tmetadata:0x{:08x}\tbpp:{}\ttrailing-offset:{}\ttrailing-bytes:{}\ttrailing-head:{}",
        clean_field(&path.display().to_string()),
        map.width,
        map.height,
        map.metadata,
        map.bits_per_pixel,
        map.trailing_offset(),
        map.trailing_bytes(),
        map.trailing_head_u32()
            .map_or_else(|| "-".to_owned(), |head| head.to_string()),
    );
    println!("cell\tx\ty\tindex\ttag\ttile-index\thigh-flag\televation");
    for y in y0..=y1 {
        for x in x0..=x1 {
            let index = map
                .cell_index(x, y)
                .ok_or_else(|| format!("map has no cell at ({x}, {y})"))?;
            let cell = map.cells[index];
            println!(
                "cell\t{x}\t{y}\t{index}\t0x{:08x}\t{}\t{}\t{}",
                cell.tag,
                cell.tile_index(),
                cell.high_flag_set(),
                cell.value,
            );
        }
    }
    Ok(())
}

/// Report every difference between two maps: headers, differing cells, then the trailing section.
///
/// Prints one line per differing cell (`x`, `y`, index, both tags, both elevations), a count, and
/// then the byte offset of the first difference in the trailing section with a hex window either
/// side. The trailing sections are compared as raw bytes from their own starts, deliberately: the
/// question being asked is what they contain, so assuming a record size would beg it.
///
/// The probe saves the same state four ways on purpose -- as a `.scn` and a `.smp`, then with
/// three sprites added and removed again -- so that what a format choice costs and what a placed
/// sprite costs can be read off a diff instead of pattern-matched out of shipped files. Doing that
/// by eye over a 16 KiB trailing section is how a wrong record size gets believed.
fn diff_maps(left: &Path, right: &Path) -> Result<(), String> {
    let read = |path: &Path| -> Result<(Vec<u8>, MapAsset), String> {
        let bytes = fs::read(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
        Ok((bytes, map))
    };
    let (left_bytes, left_map) = read(left)?;
    let (right_bytes, right_map) = read(right)?;

    println!(
        "left\t{}\t{} bytes\t{}x{}\tmetadata:0x{:08x}\ttrailing-bytes:{}",
        clean_field(&left.display().to_string()),
        left_bytes.len(),
        left_map.width,
        left_map.height,
        left_map.metadata,
        left_map.trailing_bytes(),
    );
    println!(
        "right\t{}\t{} bytes\t{}x{}\tmetadata:0x{:08x}\ttrailing-bytes:{}",
        clean_field(&right.display().to_string()),
        right_bytes.len(),
        right_map.width,
        right_map.height,
        right_map.metadata,
        right_map.trailing_bytes(),
    );

    if left_map.width != right_map.width || left_map.height != right_map.height {
        println!("cells\tnot comparable\tdifferent dimensions");
    } else {
        let mut differing = 0usize;
        for (index, (a, b)) in left_map.cells.iter().zip(&right_map.cells).enumerate() {
            // Compare the bytes as stored, not the decoded float. `MapCell` derives `PartialEq`
            // over an `f32`, and `NaN != NaN`, so two byte-identical cells holding a non-finite
            // elevation would report as differing -- in a tool whose whole job is to say which
            // bytes a save changed. The corpus is all finite today; a generated map need not be.
            if a.tag == b.tag && a.value_bits == b.value_bits {
                continue;
            }
            differing += 1;
            // Packed, y-major: cells are `y * width + x`. Observed in gameplay, 2026-09-17.
            let (x, y) = (index as u32 % left_map.width, index as u32 / left_map.width);
            println!(
                "cell\t{x}\t{y}\t{index}\t0x{:08x}\t0x{:08x}\t{}\t{}",
                a.tag, b.tag, a.value, b.value
            );
        }
        println!("cells\tdiffering:{differing}\tof:{}", left_map.cells.len());
    }

    let left_tail = &left_bytes[left_map.trailing_offset()..];
    let right_tail = &right_bytes[right_map.trailing_offset()..];
    let first_difference = first_tail_difference(left_tail, right_tail);
    println!(
        "tail\tleft:{}\tright:{}\tfirst-difference:{}",
        left_tail.len(),
        right_tail.len(),
        first_difference.map_or_else(|| "none".to_owned(), |at| at.to_string()),
    );
    if let Some(at) = first_difference {
        let window = 64;
        let from = at.saturating_sub(16);
        println!(
            "tail-left\t{from}\t{}",
            hex_bytes(&left_tail[from..(from + window).min(left_tail.len())])
        );
        println!(
            "tail-right\t{from}\t{}",
            hex_bytes(&right_tail[from..(from + window).min(right_tail.len())])
        );
    }
    Ok(())
}

/// The first offset at which two trailing sections diverge, or `None` when they are identical.
///
/// A shared prefix with different lengths still diverges: one side ends where the other continues,
/// so the divergence is at the end of the shorter. Scanning only the common range and reporting
/// nothing would read as "the tails agree" -- which is exactly how a record appended at the very
/// end would be missed, and appending a record is the main thing these diffs are used to watch.
fn first_tail_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    let common = left.len().min(right.len());
    (0..common)
        .find(|at| left[*at] != right[*at])
        .or((left.len() != right.len()).then_some(common))
}

/// Report the `.til` the gamescript reads this map through, or say plainly that it does not say.
///
/// A world map has one answer, `tilesb01.til`, from `maptileset`. A combat map's answer belongs to
/// the **encounter** that loads it, so it comes from the per-encounter `mapfile`/`tileset` pairs in
/// `gs.mpq` -- see [`lom_asset_viewer::tile::COMBAT_TILESET_BINDINGS`]. That means three possible
/// outcomes for a `.smp`: one tileset, several (different encounters, different art), or none
/// recorded. **The none case prints `unresolved` and does not fall back to a class default.** The
/// map is parsed before answering, so this cannot report a tileset for something that is not a map.
fn tile_set_for_map(path: &Path) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
    let class = MapClass::from_path(path).ok_or_else(|| {
        format!(
            "{} parses as a map but its extension is not one the corpus classifies, so which \
             tileset the engine would read it through is unknown; the known classes are .smp \
             (combat) and .scn/.lgd/.map (world)",
            path.display()
        )
    })?;
    let resolution = resolve_tileset(path).ok_or_else(|| {
        format!("could not resolve a tileset for {}", path.display())
    })?;
    println!(
        "tileset-for\t{}\t{}x{}\t{}\t{}",
        path.display(),
        map.width,
        map.height,
        class.description(),
        resolution.describe(),
    );
    // The plural selector form: tilesets one encounter may reach at runtime beyond the declared
    // one. Reported on its own line and explicitly as coarse, rather than folded into the answer
    // above, because it resolves no additional map and widens most of the ones it touches across
    // more than one tileset rule class.
    let reachable = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(combat_tileset_array_candidates)
        .unwrap_or(&[]);
    if !reachable.is_empty() {
        println!(
            "also-reachable\t{}\tcoarse\t{}",
            path.display(),
            reachable.join(" ")
        );
        eprintln!(
            "note: a `/tilesets` procedure in the gamescript may select any of those at runtime \
             from a sprite's map location. That reading is coarse -- every tileset in the \
             member's array is listed for every map in it, because the two arrays cannot be \
             zipped positionally -- so treat them as reachable, not as this map's tileset."
        );
    }
    if matches!(resolution, TileSetResolution::CombatUnresolved) {
        eprintln!(
            "note: no gamescript encounter binds this combat map to a tileset, so this project \
             does not know which one it uses and will not guess. 168 of the 337 installed .smp \
             files are in this position. Scoring cannot settle it either: the 26 shipped tilesets \
             collapse to 16 distinct rule sets, and most unbound maps have sixteen of them tied."
        );
    }
    Ok(())
}

fn describe_map(path: &Path) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
    let section = map.placed_sprites.as_ref().ok_or_else(|| {
        format!(
            "{}'s trailing section is not one of the six decoded placed-sprite layouts",
            path.display()
        )
    })?;

    println!(
        "map\t{}\t{}x{}\tlayout:{}\trecords:{}\tfooter:{}",
        clean_field(&path.display().to_string()),
        map.width,
        map.height,
        section.layout,
        section.records.len(),
        section
            .footer
            .map_or_else(|| "none".to_owned(), |footer| footer.to_string()),
    );
    println!(
        "record\tcell-index\tx\ty\tinstance-id\tattribute-bits\tattribute-code\tsprite-type\tprocedure-id-candidate\traw"
    );
    for (index, record) in section.records.iter().enumerate() {
        let (x, y) = map.record_coordinates(record);
        println!(
            "{index}\t{}\t{x}\t{y}\t{}\t0x{:08x}\t{}\t{}\t{}\t{}",
            record.cell_index,
            record.instance_id,
            record.attribute_bits,
            record.attribute_code_candidate(),
            record.sprite_type,
            record
                .procedure_id_candidate()
                .map_or_else(|| "none".to_owned(), |id| id.to_string()),
            hex_bytes(&record.raw),
        );
    }
    Ok(())
}

fn scan_map_directory(directory: &Path) -> Result<(), String> {
    if !directory.is_dir() {
        return Err(format!(
            "map directory does not exist: {}",
            directory.display()
        ));
    }
    let mut paths = Vec::new();
    collect_map_paths(directory, &mut paths)?;
    paths.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());

    let mut kind_counts = BTreeMap::<AssetKind, usize>::new();
    let mut dimension_counts = BTreeMap::<(AssetKind, u32, u32), usize>::new();
    let mut metadata_values = BTreeMap::<AssetKind, BTreeSet<u32>>::new();
    let mut cell_tags = BTreeSet::<u32>::new();
    let mut tile_indexes = BTreeSet::<u32>::new();
    let mut high_flag_cells = 0_usize;
    let mut placed_sprite_files = 0_usize;
    let mut placed_sprite_records = 0_usize;
    let mut placed_sprite_types = BTreeSet::<u32>::new();
    let mut placed_sprite_attribute_codes = BTreeSet::<u8>::new();
    let mut finite_min = f32::INFINITY;
    let mut finite_max = f32::NEG_INFINITY;
    let mut nonfinite_values = 0_usize;
    let mut trailing_ranges = BTreeMap::<AssetKind, (usize, usize)>::new();
    let mut tail_layout_counts = BTreeMap::<(AssetKind, String), usize>::new();
    let mut parsed = 0_usize;
    let mut failures = Vec::new();

    for path in &paths {
        let Some(kind) = map_kind(path) else {
            continue;
        };
        let result = fs::read(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))
            .and_then(|bytes| MapAsset::parse(&bytes).map_err(|error| error.to_string()));
        let map = match result {
            Ok(map) => map,
            Err(error) => {
                failures.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        parsed += 1;
        *kind_counts.entry(kind).or_default() += 1;
        *dimension_counts
            .entry((kind, map.width, map.height))
            .or_default() += 1;
        metadata_values
            .entry(kind)
            .or_default()
            .insert(map.metadata);
        let range = trailing_ranges
            .entry(kind)
            .or_insert((map.trailing_bytes(), map.trailing_bytes()));
        range.0 = range.0.min(map.trailing_bytes());
        range.1 = range.1.max(map.trailing_bytes());
        // The layout the parser resolved, not the arithmetic candidates: a section whose length
        // fits two layouts is common (four 48-byte records and four 47-byte records plus a footer
        // are both 196 bytes) and reporting that as "ambiguous" hid which one was decoded.
        let layout = match map.resolved_tail_layout() {
            Some(layout) => layout.to_string(),
            None => format!(
                "undecoded:{}",
                match map.candidate_tail_layouts().as_slice() {
                    [] => "no-layout-fits".to_owned(),
                    layouts => layouts
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("|"),
                }
            ),
        };
        *tail_layout_counts.entry((kind, layout)).or_default() += 1;
        if let Some(section) = &map.placed_sprites {
            placed_sprite_files += 1;
            placed_sprite_records += section.records.len();
            for record in &section.records {
                placed_sprite_types.insert(record.sprite_type);
                placed_sprite_attribute_codes.insert(record.attribute_code_candidate());
            }
        }
        for cell in &map.cells {
            cell_tags.insert(cell.tag);
            tile_indexes.insert(cell.tile_index());
            high_flag_cells += usize::from(cell.high_flag_set());
            if cell.value.is_finite() {
                finite_min = finite_min.min(cell.value);
                finite_max = finite_max.max(cell.value);
            } else {
                nonfinite_values += 1;
            }
        }
    }

    println!("map_files\t{}", paths.len());
    println!("parsed\t{parsed}");
    for (kind, count) in &kind_counts {
        println!("kind\t{kind}\t{count}");
    }
    for ((kind, width, height), count) in &dimension_counts {
        println!("dimensions\t{kind}\t{width}x{height}\t{count}");
    }
    for (kind, values) in &metadata_values {
        println!("distinct-header-metadata\t{kind}\t{}", values.len());
    }
    for (kind, (minimum, maximum)) in &trailing_ranges {
        println!("trailing-bytes\t{kind}\t{minimum}..{maximum}");
    }
    for ((kind, layout), count) in &tail_layout_counts {
        println!("tail-layout\t{kind}\t{layout}\t{count}");
    }
    println!("distinct-cell-tags\t{}", cell_tags.len());
    println!("distinct-tile-indexes\t{}", tile_indexes.len());
    if let (Some(minimum), Some(maximum)) = (tile_indexes.first(), tile_indexes.last()) {
        println!("tile-index-range\t{minimum}..{maximum}");
    }
    println!("high-flag-cells\t{high_flag_cells}");
    println!("placed-sprite-files\t{placed_sprite_files}");
    println!("placed-sprite-records\t{placed_sprite_records}");
    println!("placed-sprite-types\t{}", placed_sprite_types.len());
    println!(
        "placed-sprite-attribute-codes\t{}",
        placed_sprite_attribute_codes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    if finite_min.is_finite() {
        println!("candidate-value-range\t{finite_min}..{finite_max}");
    } else {
        println!("candidate-value-range\tnone");
    }
    println!("nonfinite-values\t{nonfinite_values}");
    println!("failures\t{}", failures.len());
    for failure in &failures {
        println!("failure\t{}", clean_field(failure));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} map files failed to parse", failures.len()))
    }
}

fn collect_map_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not read directory {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "could not read directory entry in {}: {error}",
                directory.display()
            )
        })?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            collect_map_paths(&entry.path(), paths)?;
        } else if file_type.is_file() && map_kind(&entry.path()).is_some() {
            paths.push(entry.path());
        }
    }
    Ok(())
}

fn map_kind(path: &Path) -> Option<AssetKind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "lgd" => Some(AssetKind::LegendScenario),
        "scn" => Some(AssetKind::MapScenario),
        "smp" => Some(AssetKind::MapComponent),
        _ => None,
    }
}

fn scan_archive(source: &Source) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let mut readable = 0_usize;
    let mut kinds = BTreeMap::<AssetKind, usize>::new();
    // Classified, but with contents this tool has no decoder for. Counted separately and printed
    // unconditionally so the "0 probe failures" figure carries its own falsifier: without it, an
    // archive whose members had all stopped decoding reports exactly what a clean one reports.
    let mut undecoded = BTreeMap::<AssetKind, usize>::new();
    let mut undecoded_reasons = Vec::new();
    let mut failures = Vec::new();

    for entry in &entries {
        let bytes = match archive.read(&entry.name) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push((entry.name.clone(), error.to_string()));
                continue;
            }
        };
        readable += 1;
        match probe(&entry.name, &bytes) {
            Ok(info) => {
                *kinds.entry(info.kind).or_default() += 1;
                if let Some(reason) = info.undecoded {
                    *undecoded.entry(info.kind).or_default() += 1;
                    undecoded_reasons.push((entry.name.clone(), reason));
                }
            }
            Err(error) => failures.push((entry.name.clone(), error)),
        }
    }

    println!("archive_entries\t{}", entries.len());
    println!("readable_entries\t{readable}");
    let undecoded_total: usize = undecoded.values().sum();
    // Counted from the classifications that actually happened, not by subtracting failures from
    // `readable`. `readable` already excludes members whose read failed, so subtracting them again
    // undercounted -- and with only an unreadable member it underflowed, which is a debug panic
    // and a wrapped count in release.
    // `classified_entries`, not `decoded_entries`: this counts every member the probe recognised,
    // of any kind, and most of them are not audio at all. The old name sat directly above an
    // audio-specific `undecoded_entries` and invited the two to be read as a pair.
    let classified: usize = kinds.values().sum();
    println!("classified_entries\t{classified}");
    println!(
        "classified_and_decoded\t{}",
        classified - undecoded_total
    );
    println!("undecoded_entries\t{undecoded_total}");
    for (kind, count) in &kinds {
        println!("kind\t{kind}\t{count}");
    }
    for (kind, count) in &undecoded {
        println!("undecoded\t{kind}\t{count}");
    }
    // Capped: the whole point of this counter is the case where thousands of members stop
    // decoding, and printing one line each would bury the totals it sits under.
    const UNDECODED_SAMPLE_LIMIT: usize = 20;
    for (name, reason) in undecoded_reasons.iter().take(UNDECODED_SAMPLE_LIMIT) {
        println!("undecoded_member\t{name}\t{reason}");
    }
    if undecoded_reasons.len() > UNDECODED_SAMPLE_LIMIT {
        println!(
            "undecoded_member_elided\t{}",
            undecoded_reasons.len() - UNDECODED_SAMPLE_LIMIT
        );
    }
    let failure_count = failures.len();
    println!("failures\t{failure_count}");
    for (name, error) in failures {
        println!("failure\t{name}\t{error}");
    }
    if failure_count == 0 {
        Ok(())
    } else {
        Err(format!("{failure_count} archive members failed probing"))
    }
}

/// Print the structured trace for a VM failure before surfacing it.
///
/// An unresolved name is the interesting outcome, not merely a failure: it names a host
/// call and shows how far the script got before it needed one. The VM stops rather than
/// guessing, so this trace is the classification evidence.
fn report_gamescript_failure(
    error: GameScriptVmError,
    operators: Option<(&native_table::OperatorIndex, &native_table::PeImage<'_>)>,
) -> String {
    for line in gamescript_failure_lines(&error, operators) {
        eprintln!("{line}");
    }
    error.to_string()
}

/// Build the structured trace lines for a VM failure. Split out from
/// `report_gamescript_failure` so the wording is unit-testable without capturing stderr.
fn gamescript_failure_lines(
    error: &GameScriptVmError,
    operators: Option<(&native_table::OperatorIndex, &native_table::PeImage<'_>)>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(trace) = error.unknown_name() else {
        return lines;
    };
    lines.push(format!("unknown-native-name\t{}", trace.name));
    lines.push(format!("unknown-at-step\t{}", trace.steps));
    for (depth, frame) in trace.call_stack.iter().enumerate() {
        lines.push(format!("unknown-call-stack\t{depth}\t{frame}"));
    }
    if let Some((operators, pe_image)) = operators {
        // With the engine's operator tables loaded, a stop is a classification rather than a
        // research question: the name is either something the engine implements, a constant it
        // exposes, or neither.
        match operators.classify(&trace.name) {
            native_table::NameClass::Operator { entry_point } => {
                lines.push("unknown-name-class\toperator".to_owned());
                lines.push(format!("unknown-name-entry-point\t{entry_point:#010x}"));
                lines.extend(operator_signature_lines(&trace.name, entry_point, pe_image));
            }
            native_table::NameClass::EngineConstant => {
                lines.push("unknown-name-class\tengine-constant".to_owned());
                lines.push(format!(
                    "unknown-name-remedy\tSCREAMING_CASE and absent from the operator tables, so this is a constant; supply its value with --stub {}=VALUE",
                    trace.name
                ));
            }
            native_table::NameClass::Unresolved => {
                lines.push("unknown-name-class\tunresolved".to_owned());
                lines.push(
                    "unknown-name-remedy\tneither an operator nor constant-shaped; most likely defined in a module this run has not loaded"
                        .to_owned(),
                );
            }
        }
    }
    lines
}

/// Report the recovered stack effect for an unresolved operator name, computed lazily for just
/// that one entry point so a probe that stops early never pays for the other ~1,900 operators.
///
/// The counts are static site counts, not proven arity — they equal the operator's true arity
/// only when every stack commit in its body lies on a single execution path. `mul` is the
/// documented counterexample: it reports two pushes because it commits on two mutually exclusive
/// type paths, one per operand type. The remedy line states that caveat instead of presenting the
/// count as fact; the confidence marker matches the one `--scan-natives` reports, so a "well
/// formed" walk still means only that the walk completed cleanly, not that the arity is proven.
fn operator_signature_lines(
    name: &str,
    entry_point: u32,
    pe_image: &native_table::PeImage<'_>,
) -> Vec<String> {
    match operator_arity::stack_effect(pe_image, entry_point) {
        Ok(effect) => {
            let confidence = effect.confidence();
            vec![
                format!("unknown-name-pops\t{}", effect.pops),
                format!("unknown-name-pushes\t{}", effect.pushes),
                format!("unknown-name-confidence\t{confidence}"),
                format!(
                    "unknown-name-remedy\tthe engine implements this; a static site count ({confidence}) says it takes {} operand{} and returns {} result{} — not proven arity. Pushes are a sound upper bound, but pops UNDERCOUNT operators that pop through the shared helper at 0x0040ADB0 (drawimpframe really takes 6, getimphotspot 5) \u{2014} read the entry point before relying on it. Supply a value with --stub {name}=VALUE",
                    effect.pops,
                    if effect.pops == 1 { "" } else { "s" },
                    effect.pushes,
                    if effect.pushes == 1 { "" } else { "s" },
                ),
            ]
        }
        Err(error) => vec![format!(
            "unknown-name-remedy\tthe engine implements this, but its stack effect could not be recovered ({error}); supply it with --stub {name}=VALUE"
        )],
    }
}

/// A read-only [`ModuleSource`] over one archive, for `--probe-gamescript`.
///
/// Members are named with `\` and `run` targets are written with `/`, and the archive resolves
/// case-insensitively, so the index is keyed on the folded form and read under the archive's own
/// spelling.
struct ProbeModules {
    archive: Rc<Archive>,
    members: BTreeMap<String, String>,
}

impl ModuleSource for ProbeModules {
    fn load(&self, path: &str) -> Result<Vec<u8>, String> {
        let name = self
            .members
            .get(&normalize_module_path(path))
            .ok_or_else(|| format!("archive has no member named {path}"))?;
        self.archive.read(name).map_err(|error| error.to_string())
    }
}

fn probe_gamescript_member(
    source: &Source,
    member: &str,
    expression: Option<&str>,
    stubs: &[(String, GameScriptValue)],
    executable: Option<&Path>,
) -> Result<(), String> {
    let image_bytes = executable
        .map(|path| {
            fs::read(path)
                .map_err(|error| format!("could not read executable {}: {error}", path.display()))
        })
        .transpose()?;
    let operators = image_bytes
        .as_deref()
        .map(|image| {
            native_table::OperatorIndex::from_image(image)
                .map_err(|error| format!("could not read the operator table: {error}"))
        })
        .transpose()?;
    let pe_image = image_bytes
        .as_deref()
        .map(|image| {
            native_table::PeImage::parse(image)
                .map_err(|error| format!("could not read the executable: {error}"))
        })
        .transpose()?;
    // Both come from the same successfully-loaded image, so either both are present or neither is.
    let operator_context = operators.as_ref().zip(pe_image.as_ref());
    let (archive, entries) = open_archive(source)?;
    let archive = Rc::new(archive);
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| format!("archive has no member named {member}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let document = GameScriptDocument::parse(&bytes).map_err(|error| error.to_string())?;
    let token_count = document.tokens.len();
    let anomaly_count = document.procedure_anomalies.len();
    let mut vm = GameScriptVm::new(1_000_000);
    // `run` reads from the same archive the member came from, read-only. Without this a probed
    // member that loads a module stops on the load itself, which is a worse trace than the one it
    // would have produced inside the module.
    vm.set_module_source(Rc::new(ProbeModules {
        members: entries
            .iter()
            .map(|entry| (normalize_module_path(&entry.name), entry.name.clone()))
            .collect(),
        archive: Rc::clone(&archive),
    }));
    for (name, value) in stubs {
        vm.define_native_stub(name.clone(), value.clone());
    }
    vm.execute_document(&document)
        .map_err(|error| report_gamescript_failure(error, operator_context))?;
    let expression_tokens = expression
        .map(|source| {
            let expression =
                GameScriptDocument::parse(source.as_bytes()).map_err(|error| error.to_string())?;
            let tokens = expression.tokens.len();
            vm.execute_document(&expression)
                .map_err(|error| report_gamescript_failure(error, operator_context))?;
            Ok::<usize, String>(tokens)
        })
        .transpose()?;
    let defined_names = vm.defined_names();
    let mut stack_kinds = BTreeMap::<&str, usize>::new();
    for value in vm.operand_stack() {
        *stack_kinds.entry(value.kind()).or_default() += 1;
    }

    println!("member\t{}", clean_field(&entry.name));
    println!("source-bytes\t{}", bytes.len());
    println!("tokens\t{token_count}");
    println!("procedure-anomalies\t{anomaly_count}");
    if let Some(tokens) = expression_tokens {
        println!("eval-tokens\t{tokens}");
    }
    println!("vm-steps\t{}", vm.steps());
    println!("operand-stack-depth\t{}", vm.operand_stack().len());
    for (kind, count) in stack_kinds {
        println!("operand-stack-kind\t{kind}\t{count}");
    }
    for (index, value) in vm.operand_stack().iter().enumerate() {
        if let Some(summary) = value.scalar_summary() {
            println!("operand-stack-scalar\t{index}\t{summary}");
        }
    }
    for (name, count) in vm.native_calls() {
        println!("native-stub-call\t{}\t{count}", clean_field(name));
    }
    println!("defined-names\t{}", defined_names.len());
    for name in defined_names.iter().take(50) {
        println!("defined-name\t{}", clean_field(name));
    }
    Ok(())
}

/// What `--gs-facts` was pointed at.
///
/// Three forms and no discovery: an archive, one member of an archive, or a local file or
/// directory. Nothing here searches `~/Applications` for anything, matching the rule
/// `scripts/repack-archive.sh` already sets.
enum GsFactsTarget {
    Archive(Source),
    ArchiveMember { source: Source, member: String },
    Path(PathBuf),
}

/// Emit one JSON object per `.gs` file, on its own line.
///
/// JSON Lines rather than one document, so a caller can stream a 1,700-member archive and so a
/// failure part-way through leaves the lines already written readable. The bytes are read and
/// measured; they are never rewritten. A member that fails to lex still gets a record -- with its
/// `parse_error` populated -- because "this file does not lex" is the single most useful thing the
/// validator can report, and dropping the record would turn an error into a silence.
fn emit_gs_facts(target: &GsFactsTarget) -> Result<(), String> {
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut emitted = 0_usize;
    let mut unreadable = Vec::new();

    match target {
        GsFactsTarget::Path(path) => {
            let mut files = Vec::new();
            collect_gamescript_files(path, &mut files)?;
            files.sort();
            for file in files {
                let name = file
                    .strip_prefix(path)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .into_owned();
                match fs::read(&file) {
                    Ok(bytes) => {
                        writeln!(out, "{}", GsFacts::measure(&name, &bytes).to_json())
                            .map_err(|error| error.to_string())?;
                        emitted += 1;
                    }
                    Err(error) => unreadable.push(format!("{}: {error}", file.display())),
                }
            }
        }
        GsFactsTarget::Archive(source) => {
            let (archive, entries) = open_archive(source)?;
            let mut names: Vec<String> = entries
                .iter()
                .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
                .map(|entry| entry.name.clone())
                .collect();
            names.sort();
            for name in names {
                match archive.read(&name) {
                    Ok(bytes) => {
                        writeln!(out, "{}", GsFacts::measure(&name, &bytes).to_json())
                            .map_err(|error| error.to_string())?;
                        emitted += 1;
                    }
                    Err(error) => unreadable.push(format!("{name}: {error}")),
                }
            }
        }
        GsFactsTarget::ArchiveMember { source, member } => {
            let (archive, entries) = open_archive(source)?;
            // Resolved case-insensitively to find the member, but reported under the name the
            // archive spells, so a caller can see a case difference rather than inherit the one
            // it asked with.
            let entry = entries
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(member))
                .ok_or_else(|| format!("archive has no member named {member}"))?;
            let bytes = archive.read(&entry.name).map_err(|error| error.to_string())?;
            writeln!(out, "{}", GsFacts::measure(&entry.name, &bytes).to_json())
                .map_err(|error| error.to_string())?;
            emitted += 1;
        }
    }

    out.flush().map_err(|error| error.to_string())?;
    // The count of what could not be read goes to stderr with the count of what could, so a
    // bounded negative downstream can say how large its blind spot was.
    eprintln!("{emitted} member(s) measured, {} unreadable", unreadable.len());
    for failure in &unreadable {
        eprintln!("  unreadable {failure}");
    }
    if unreadable.is_empty() {
        Ok(())
    } else {
        Err(format!("{} member(s) could not be read", unreadable.len()))
    }
}

fn collect_gamescript_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    let entries = fs::read_dir(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let child = entry.path();
        if child.is_dir() {
            collect_gamescript_files(&child, files)?;
        } else if child
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gs"))
        {
            files.push(child);
        }
    }
    Ok(())
}

fn scan_gamescript_archive(source: &Source, executable: Option<&Path>) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let archive_names: BTreeSet<String> = entries
        .iter()
        .map(|entry| normalize_member_name(&entry.name))
        .collect();
    let script_names: BTreeSet<String> = entries
        .iter()
        .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
        .map(|entry| normalize_member_name(&entry.name))
        .collect();
    let mut script_files = 0_usize;
    let mut source_bytes = 0_usize;
    let mut tokens = 0_usize;
    let mut comments = 0_usize;
    let mut strings = 0_usize;
    let mut numbers = 0_usize;
    let mut maximum_procedure_depth = 0_usize;
    let mut empty_files = 0_usize;
    let mut procedure_anomalies = Vec::<(String, String, usize, usize, usize)>::new();
    let mut executable_names = BTreeMap::<String, usize>::new();
    let mut literal_names = BTreeMap::<String, usize>::new();
    let mut definition_names = BTreeMap::<String, usize>::new();
    let mut dependency_edges = BTreeSet::<(String, String)>::new();
    let mut failures = Vec::new();

    for entry in entries
        .iter()
        .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
    {
        let bytes = match archive.read(&entry.name) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{}: {error}", entry.name));
                continue;
            }
        };
        let document = match GameScriptDocument::parse(&bytes) {
            Ok(document) => document,
            Err(error) => {
                failures.push(format!("{}: {error}", entry.name));
                continue;
            }
        };
        let analysis = document.analyze();
        script_files += 1;
        source_bytes += bytes.len();
        if bytes.is_empty() {
            empty_files += 1;
        }
        tokens += analysis.token_count;
        comments += analysis.comment_count;
        strings += analysis.string_count;
        numbers += analysis.number_count;
        maximum_procedure_depth = maximum_procedure_depth.max(analysis.maximum_procedure_depth);
        for anomaly in document.procedure_anomalies {
            procedure_anomalies.push((
                entry.name.clone(),
                anomaly.message,
                anomaly.offset,
                anomaly.line,
                anomaly.column,
            ));
        }
        merge_name_counts(&mut executable_names, &analysis.executable_names);
        merge_name_counts(&mut literal_names, &analysis.literal_names);
        merge_name_counts(&mut definition_names, &analysis.definition_names);
        for dependency in analysis.static_run_dependencies {
            dependency_edges.insert((entry.name.clone(), dependency));
        }
    }

    let resolved_dependencies = dependency_edges
        .iter()
        .filter(|(_, dependency)| archive_names.contains(&normalize_member_name(dependency)))
        .count();
    let missing_dependencies: Vec<_> = dependency_edges
        .iter()
        .filter(|(_, dependency)| !archive_names.contains(&normalize_member_name(dependency)))
        .collect();
    let likely_engine_names = executable
        .map(|path| likely_engine_names(path, &executable_names, &definition_names))
        .transpose()?;

    println!("archive-entries\t{}", entries.len());
    println!("gamescript-files\t{}", script_names.len());
    println!("parsed-files\t{script_files}");
    println!("empty-files\t{empty_files}");
    println!("source-bytes\t{source_bytes}");
    println!("tokens\t{tokens}");
    println!("comments\t{comments}");
    println!("strings\t{strings}");
    println!("numbers\t{numbers}");
    println!("maximum-procedure-depth\t{maximum_procedure_depth}");
    println!("procedure-anomalies\t{}", procedure_anomalies.len());
    println!("distinct-executable-names\t{}", executable_names.len());
    println!("distinct-literal-names\t{}", literal_names.len());
    println!("distinct-definition-names\t{}", definition_names.len());
    println!("static-run-reference-edges\t{}", dependency_edges.len());
    println!("resolved-static-run-references\t{resolved_dependencies}");
    println!(
        "unresolved-static-run-references\t{}",
        missing_dependencies.len()
    );
    if let Some(names) = &likely_engine_names {
        // The full candidate vocabulary is the useful artefact, but printing ~2,000 lines by
        // default buries the summary. `LOM_CANDIDATE_LIMIT` raises the cap for cataloguing.
        let candidate_display_limit = std::env::var("LOM_CANDIDATE_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50);
        println!("likely-hardcoded-engine-names\t{}", names.candidates.len());
        // The published total moved when the definition check stopped folding case. Say by how
        // much, and name the entries, rather than leaving a reader to wonder why it changed.
        println!(
            "engine-names-recovered-from-the-old-case-fold\t{}",
            names.hidden_by_the_old_case_fold.len()
        );
        for (name, count) in names
            .hidden_by_the_old_case_fold
            .iter()
            .take(candidate_display_limit)
        {
            println!("engine-name-recovered-from-case-fold\t{name}\t{count}");
        }
        for (name, count) in names.candidates.iter().take(candidate_display_limit) {
            println!("engine-name-candidate\t{name}\t{count}");
        }
    }
    for (owner, message, offset, line, column) in procedure_anomalies {
        println!(
            "procedure-anomaly\t{}\t{} at byte {}, line {}, column {}",
            clean_field(&owner),
            clean_field(&message),
            offset,
            line,
            column
        );
    }
    for (owner, dependency) in missing_dependencies {
        println!(
            "unresolved-run-reference\t{}\t{}",
            clean_field(owner),
            clean_field(dependency)
        );
    }
    println!("failures\t{}", failures.len());
    for failure in &failures {
        println!("failure\t{}", clean_field(failure));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} GameScript files failed to parse",
            failures.len()
        ))
    }
}

fn merge_name_counts(target: &mut BTreeMap<String, usize>, source: &BTreeMap<String, usize>) {
    for (name, count) in source {
        *target.entry(name.clone()).or_default() += count;
    }
}

fn normalize_member_name(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

/// The scanner's engine-name candidate list, with the size of the defect it used to carry.
struct EngineNameCandidates {
    /// Names the corpus calls, never defines, and which also appear verbatim in the executable,
    /// ordered by use count.
    candidates: Vec<(String, usize)>,
    /// Those a case-folded definition check used to suppress. Reported rather than quietly
    /// absorbed, because this list is published and its total moved when the fold was fixed.
    hidden_by_the_old_case_fold: Vec<(String, usize)>,
}

fn likely_engine_names(
    path: &Path,
    executable_names: &BTreeMap<String, usize>,
    definition_names: &BTreeMap<String, usize>,
) -> Result<EngineNameCandidates, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read executable {}: {error}", path.display()))?;
    let binary_strings = ascii_strings(&bytes);
    // Exclude names the corpus actually DEFINES, not every name that appears as a literal.
    // The scripts push a native name and convert it to defer the call (`/invoke_spell cvx`),
    // so excluding on literal presence hid genuine host calls.
    //
    // **Case-sensitively.** `GameScriptVm::lookup` builds a `DictKey::Name` from the name exactly
    // as written, so `GOLD` and `gold` are two different names to the interpreter. This check used
    // to fold case, which let `gs\barter.gs`'s `/gold` suppress `GOLD` -- an engine constant with
    // 290 uses in 3.02 that the corpus never defines -- from the list this tool prints. Folding
    // here contradicted the VM's own resolution and hid real engine names, which is the exact
    // failure this filter exists to prevent.
    let defined: BTreeSet<&String> = definition_names.keys().collect();
    let defined_folded: BTreeSet<String> = definition_names
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    let mut candidates = Vec::new();
    let mut hidden_by_the_old_case_fold = Vec::new();
    for (name, count) in executable_names {
        if defined.contains(name) {
            continue;
        }
        // The *binary-string* comparison stays folded. That is a coincidence filter asking whether
        // the name occurs in the image at all, where a difference of case carries no meaning. It
        // is a different question from "does the corpus define this name", and only the second one
        // has to agree with the interpreter.
        let lower = name.to_ascii_lowercase();
        if !binary_strings.contains(&lower) {
            continue;
        }
        if defined_folded.contains(&lower) {
            hidden_by_the_old_case_fold.push((name.clone(), *count));
        }
        candidates.push((name.clone(), *count));
    }
    let by_use_then_name =
        |(left_name, left_count): &(String, usize), (right_name, right_count): &(String, usize)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_name.cmp(right_name))
        };
    candidates.sort_by(by_use_then_name);
    hidden_by_the_old_case_fold.sort_by(by_use_then_name);
    Ok(EngineNameCandidates {
        candidates,
        hidden_by_the_old_case_fold,
    })
}

fn ascii_strings(source: &[u8]) -> BTreeSet<String> {
    source
        .split(|byte| !(0x20..=0x7e).contains(byte))
        .filter(|bytes| bytes.len() >= 2)
        .map(|bytes| String::from_utf8_lossy(bytes).to_ascii_lowercase())
        .collect()
}

#[derive(Default)]
struct ImpPair {
    header: Option<String>,
    sprite: Option<String>,
}

/// How a `.imp` member found the `.h` it was validated against.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImpPairing {
    /// The two members share a stem, e.g. `units\imp\aicr3b.imp` and `units\imp\aicr3b.h`.
    Stem,
    /// The stem had no counterpart, so the header's declared sequence name was used instead.
    ///
    /// Headers in this archive are routinely copied between members: 602 of the 1,800 declare a
    /// sequence name other than their own stem, and 388 of them fall into 115 groups of
    /// byte-identical files. Worse, 1,800 headers declare only 1,370 distinct sequence names:
    /// 155 names are declared by more than one header, covering 585 headers, and `deaura` alone
    /// is declared by 32. A declared name is therefore not a key. This fallback is consulted only
    /// for a member that stem pairing left unmatched, it demands a *uniquely* supported match,
    /// and the counterpart it finds may already be paired with somebody else.
    DeclaredSequenceName,
}

/// The waiver tables a validation run is judged against.
///
/// Injected rather than read from the `imp` module's constants so the verdict logic can be tested
/// against synthetic members: an exception is value-pinned to numbers only the real archive
/// produces, so a test needs a table pinned to its own fixture instead.
#[derive(Clone, Copy)]
struct ImpCatalog<'a> {
    exceptions: &'a [ImpValidationException],
    orphans: &'a [ImpOrphanNote],
}

impl<'a> ImpCatalog<'a> {
    fn exception(&self, member: &str) -> Option<&'a ImpValidationException> {
        self.exceptions
            .iter()
            .find(|exception| exception.member == member)
    }

    fn orphan(&self, member: &str) -> Option<&'a ImpOrphanNote> {
        self.orphans.iter().find(|note| note.member == member)
    }
}

const ARCHIVE_CATALOG: ImpCatalog<'static> = ImpCatalog {
    exceptions: IMP_VALIDATION_EXCEPTIONS,
    orphans: IMP_ORPHAN_NOTES,
};

/// Everything a corpus validation run measures, with the archive I/O held at arm's length.
#[derive(Default)]
struct ImpValidationReport {
    candidate_stems: usize,
    matched_pairs: usize,
    paired_by_stem: usize,
    paired_by_declared_name: usize,
    ambiguous_pairings: usize,
    validated: usize,
    excepted: usize,
    documented_orphans: usize,
    orphan_entries: usize,
    validation_failures: usize,
    /// Stem-paired members whose `.imp` and `.h` both parsed, i.e. the denominator of the two
    /// counters below. Pairs that fail to parse are excluded and counted as failures instead;
    /// they used to vanish from the comparison with nothing to show for it.
    dedup_compared_stem_pairs: usize,
    dedup_at_least_header: usize,
    dedup_below_header: usize,
    /// Pairs made by the declared-sequence-name fallback, excluded from the two counters above.
    ///
    /// Those pairs compare a sprite against a *foreign* header, so agreement between them says
    /// nothing about whether a build tool's own header can undercount duplicates. They used to be
    /// counted as confirmations.
    dedup_foreign_header_pairs: usize,
    notes: Vec<String>,
    failures: Vec<String>,
}

/// The single candidate a pairing may use, or the reason there is not one.
enum ImpCandidate<'a> {
    One(&'a str),
    None,
    Ambiguous(Vec<&'a str>),
}

/// Pick the one candidate that a declared-sequence-name pairing may use.
///
/// A declared name is not unique in this archive, so "take the first" is a guess dressed as a
/// result. Where several members answer to the name, one in `directory` wins — a header sitting
/// beside its art is the only tie-break the archive's layout supports — and anything still
/// ambiguous is reported rather than resolved.
fn unique_candidate<'a>(candidates: &'a [String], directory: &str) -> ImpCandidate<'a> {
    match candidates {
        [] => ImpCandidate::None,
        [only] => ImpCandidate::One(only),
        many => {
            let local: Vec<&str> = many
                .iter()
                .filter(|name| imp_member_directory(name) == directory)
                .map(String::as_str)
                .collect();
            match local.as_slice() {
                [only] => ImpCandidate::One(only),
                _ => ImpCandidate::Ambiguous(many.iter().map(String::as_str).collect()),
            }
        }
    }
}

/// The normalized directory part of a member name, `""` for a member at the archive root.
fn imp_member_directory(name: &str) -> String {
    let normalized = normalize_imp_member(name);
    match normalized.rfind('/') {
        Some(index) => normalized[..index].to_owned(),
        None => String::new(),
    }
}

/// Validate every `.imp`/`.h` pair among `members`, reading bytes through `read`.
///
/// Separated from the archive so the pairing and verdict rules are testable: every branch here
/// used to be reachable only by running the shipped 3,600-member archive.
fn validate_imp_members(
    members: &[String],
    read: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    catalog: ImpCatalog<'_>,
) -> ImpValidationReport {
    let mut report = ImpValidationReport::default();
    let mut pairs = BTreeMap::<String, ImpPair>::new();

    for name in members {
        let lower_name = name.to_ascii_lowercase();
        if let Some(stem) = lower_name.strip_suffix(".h") {
            pairs.entry(stem.to_owned()).or_default().header = Some(name.clone());
        } else if let Some(stem) = lower_name.strip_suffix(".imp") {
            pairs.entry(stem.to_owned()).or_default().sprite = Some(name.clone());
        }
    }

    // Sequence name a header declares -> every member name declaring it, and basename -> every
    // sprite with that basename. Both are one-to-many: collapsing either to one entry is what
    // made the fallback arbitrary.
    //
    // A header that cannot be read or parsed is left out of the index rather than reported here:
    // every header is visited again below, either as half of a stem pair or on the orphan path,
    // and that is where its error is collected. Reporting it twice would inflate the failure
    // count.
    let mut declared_by_name = BTreeMap::<String, Vec<String>>::new();
    for pair in pairs.values() {
        let Some(header_name) = &pair.header else {
            continue;
        };
        let Ok(bytes) = read(header_name) else {
            continue;
        };
        let Ok(stats) = ImpHeaderStats::parse(&bytes) else {
            continue;
        };
        declared_by_name
            .entry(stats.sequence_name.to_ascii_lowercase())
            .or_default()
            .push(header_name.clone());
    }
    let mut sprite_by_basename = BTreeMap::<String, Vec<String>>::new();
    for (stem, pair) in &pairs {
        if let Some(sprite) = &pair.sprite {
            sprite_by_basename
                .entry(imp_member_basename(stem).to_owned())
                .or_default()
                .push(sprite.clone());
        }
    }
    let stem_paired: BTreeSet<&String> = pairs
        .values()
        .filter(|pair| pair.header.is_some() && pair.sprite.is_some())
        .flat_map(|pair| [pair.header.as_ref(), pair.sprite.as_ref()])
        .flatten()
        .collect();

    report.candidate_stems = pairs.len();

    for (stem, pair) in &pairs {
        let resolved = match (&pair.header, &pair.sprite) {
            (Some(header), Some(sprite)) => Some((header.clone(), sprite.clone(), ImpPairing::Stem)),
            (Some(header), None) => {
                // A header with no `.imp` of its own may still describe a sequence that ships.
                // A read or parse error here used to abort the whole run, or be discarded.
                let bytes = match read(header) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        report.validation_failures += 1;
                        report.failures.push(format!("{stem}: {error}"));
                        continue;
                    }
                };
                let stats = match ImpHeaderStats::parse(&bytes) {
                    Ok(stats) => stats,
                    Err(error) => {
                        report.validation_failures += 1;
                        report.failures.push(format!("{stem}: {error}"));
                        continue;
                    }
                };
                let declared = stats.sequence_name.to_ascii_lowercase();
                let candidates = sprite_by_basename.get(&declared);
                match unique_candidate(
                    candidates.map_or(&[][..], Vec::as_slice),
                    &imp_member_directory(header),
                ) {
                    ImpCandidate::One(sprite) => Some((
                        header.clone(),
                        sprite.to_owned(),
                        ImpPairing::DeclaredSequenceName,
                    )),
                    ImpCandidate::None => None,
                    ImpCandidate::Ambiguous(names) => {
                        report.ambiguous_pairings += 1;
                        report.failures.push(format!(
                            "{stem}: declares sequence {declared}, which {} members answer to ({}); refusing to guess",
                            names.len(),
                            names.join(", ")
                        ));
                        continue;
                    }
                }
            }
            (None, Some(sprite)) => {
                // A sprite with no `.h` of its own may be named by somebody else's header.
                let basename = imp_member_basename(stem);
                let candidates = declared_by_name.get(basename);
                match unique_candidate(
                    candidates.map_or(&[][..], Vec::as_slice),
                    &imp_member_directory(sprite),
                ) {
                    ImpCandidate::One(header) => Some((
                        header.to_owned(),
                        sprite.clone(),
                        ImpPairing::DeclaredSequenceName,
                    )),
                    ImpCandidate::None => None,
                    ImpCandidate::Ambiguous(names) => {
                        report.ambiguous_pairings += 1;
                        report.failures.push(format!(
                            "{stem}: sequence {basename} is declared by {} headers ({}); refusing to guess",
                            names.len(),
                            names.join(", ")
                        ));
                        continue;
                    }
                }
            }
            (None, None) => None,
        };

        let Some((header_name, sprite_name, pairing)) = resolved else {
            // The member that exists, not the one that is missing: the catalog note names it.
            let present = match (&pair.header, &pair.sprite) {
                (Some(header), None) => header.clone(),
                (None, Some(sprite)) => sprite.clone(),
                _ => unreachable!("a stem with both halves always resolves"),
            };
            let member = normalize_imp_member(&present);
            match catalog.orphan(&member) {
                Some(note) => {
                    // Read and re-measure it. Accepting the note on the member's *name* let a
                    // truncated or substituted file pass the corpus run without being parsed.
                    let verified = read(&present)
                        .and_then(|bytes| note.verify(&bytes).map_err(|error| error.to_string()));
                    match verified {
                        Ok(()) => {
                            report.documented_orphans += 1;
                            report
                                .notes
                                .push(format!("orphan\t{member}\t{}", note.reason));
                        }
                        Err(error) => {
                            report.validation_failures += 1;
                            report.failures.push(format!("{stem}: {error}"));
                        }
                    }
                }
                None => {
                    report.orphan_entries += 1;
                    let missing = if pair.header.is_none() { ".h" } else { ".imp" };
                    report
                        .failures
                        .push(format!("{stem}: no {missing} counterpart and no catalog note"));
                }
            }
            continue;
        };

        report.matched_pairs += 1;
        match pairing {
            ImpPairing::Stem => report.paired_by_stem += 1,
            ImpPairing::DeclaredSequenceName => {
                report.paired_by_declared_name += 1;
                let reuse = if stem_paired.contains(&header_name) || stem_paired.contains(&sprite_name)
                {
                    "reuses_a_stem_paired_member"
                } else {
                    "partner_is_otherwise_unpaired"
                };
                report.notes.push(format!(
                    "paired_by_declared_sequence\t{stem}\t{}\t{}\t{reuse}",
                    clean_field(&header_name),
                    clean_field(&sprite_name)
                ));
            }
        }

        let measured = (|| {
            let header_bytes = read(&header_name)?;
            let sprite_bytes = read(&sprite_name)?;
            let stats = ImpHeaderStats::parse(&header_bytes).map_err(|error| error.to_string())?;
            let sprite = ImpSprite::parse(&sprite_bytes).map_err(|error| error.to_string())?;
            Ok::<_, String>((sprite, stats))
        })();
        let (sprite, stats) = match measured {
            Ok(measured) => measured,
            Err(error) => {
                report.validation_failures += 1;
                report.failures.push(format!("{stem}: {error}"));
                continue;
            }
        };

        // With the frame-table fix in place this no longer tests a hypothesis: binary and header
        // duplicate tallies now agree on every pair but the five catalogued ones. It is kept as
        // the instrument behind one recorded claim — that `units/imp/orcr4b` is the archive's
        // only pair whose file holds fewer duplicates than its header claims — and it is only
        // meaningful over stem pairs, where the header is the file's own.
        match pairing {
            ImpPairing::DeclaredSequenceName => report.dedup_foreign_header_pairs += 1,
            ImpPairing::Stem => {
                report.dedup_compared_stem_pairs += 1;
                if sprite.duplicate_frame_count >= stats.duplicate_frame_count {
                    report.dedup_at_least_header += 1;
                } else {
                    report.dedup_below_header += 1;
                    report.notes.push(format!(
                        "dedup_below_header\t{stem}\tbinary={}\theader={}",
                        sprite.duplicate_frame_count, stats.duplicate_frame_count
                    ));
                }
            }
        }

        let found = sprite.disagreements(&stats);
        if found.is_empty() {
            report.validated += 1;
            continue;
        }
        let member = normalize_imp_member(stem);
        match catalog
            .exception(&member)
            .filter(|exception| exception.covers(&found))
        {
            Some(exception) => {
                report.excepted += 1;
                report.notes.push(format!(
                    "exception\t{member}\t{:?}\t{}",
                    exception.class,
                    clean_field(exception.reason)
                ));
            }
            None => {
                report.validation_failures += 1;
                report.failures.push(format!(
                    "{stem}: {}",
                    sprite.validate_against(&stats).unwrap_err()
                ));
            }
        }
    }

    report
}

fn validate_imp_archive(source: &Source) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let members: Vec<String> = entries.iter().map(|entry| entry.name.clone()).collect();
    let report = validate_imp_members(
        &members,
        &|name| archive.read(name).map_err(|error| error.to_string()),
        ARCHIVE_CATALOG,
    );

    println!("candidate_stems\t{}", report.candidate_stems);
    println!("matched_pairs\t{}", report.matched_pairs);
    println!("paired_by_stem\t{}", report.paired_by_stem);
    println!("paired_by_declared_sequence\t{}", report.paired_by_declared_name);
    println!("ambiguous_pairings\t{}", report.ambiguous_pairings);
    println!("validated\t{}", report.validated);
    println!("validated_with_exception\t{}", report.excepted);
    println!("validation_failures\t{}", report.validation_failures);
    println!("documented_orphans\t{}", report.documented_orphans);
    println!("orphan_entries\t{}", report.orphan_entries);
    println!(
        "dedup_compared_stem_pairs\t{}",
        report.dedup_compared_stem_pairs
    );
    println!("dedup_at_least_header\t{}", report.dedup_at_least_header);
    println!("dedup_below_header\t{}", report.dedup_below_header);
    println!(
        "dedup_foreign_header_pairs\t{}",
        report.dedup_foreign_header_pairs
    );
    println!("failures\t{}", report.failures.len());
    // Notes are already tab-delimited; only their trailing field can need scrubbing, and each
    // one is scrubbed where it is built.
    for note in &report.notes {
        println!("{note}");
    }
    for failure in &report.failures {
        println!("failure\t{}", clean_field(failure));
    }
    if report.failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} IMP validations or catalog pairings failed",
            report.failures.len()
        ))
    }
}

fn clean_field(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn view_imp_archive(source: &Source, member: &str, requested_frame: usize) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| format!("archive has no member named {member}"))?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let sprite = ImpSprite::parse(&bytes).map_err(|error| error.to_string())?;
    let sequence_labels = load_imp_sequence_labels(&archive, &entries, &entry.name);
    if requested_frame >= sprite.frames.len() {
        return Err(format!(
            "IMP frame {requested_frame} is out of range; {} frames are available",
            sprite.frames.len()
        ));
    }
    let mut frame_index = find_imp_frame(&sprite, requested_frame, 1, true)?;
    let mut playing = false;
    let mut display_mode = ImpDisplayMode::Preview;
    let mut last_advance = Instant::now();

    let sdl = sdl3::init().map_err(|error| error.to_string())?;
    let video = sdl.video().map_err(|error| error.to_string())?;
    let window = video
        .window("Lords of Magic IMP viewer", WINDOW_WIDTH, WINDOW_HEIGHT)
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| error.to_string())?;
    let mut canvas = window.into_canvas();
    let mut event_pump = sdl.event_pump().map_err(|error| error.to_string())?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    repeat: false,
                    ..
                } => {
                    frame_index = step_imp_frame(&sprite, frame_index, 1)?;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    repeat: false,
                    ..
                } => {
                    frame_index = step_imp_frame(&sprite, frame_index, -1)?;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Down),
                    repeat: false,
                    ..
                } => {
                    frame_index = step_imp_facing(&sprite, frame_index, 1)?;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Up),
                    repeat: false,
                    ..
                } => {
                    frame_index = step_imp_facing(&sprite, frame_index, -1)?;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::PageDown),
                    repeat: false,
                    ..
                } => {
                    frame_index = step_imp_sequence(&sprite, frame_index, 1)?;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::PageUp),
                    repeat: false,
                    ..
                } => {
                    frame_index = step_imp_sequence(&sprite, frame_index, -1)?;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Space),
                    repeat: false,
                    ..
                } => {
                    playing = !playing;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    repeat: false,
                    ..
                } => display_mode = display_mode.next(),
                _ => {}
            }
        }
        if playing && last_advance.elapsed() >= Duration::from_millis(100) {
            frame_index = step_imp_frame(&sprite, frame_index, 1)?;
            last_advance = Instant::now();
        }

        let frame = sprite
            .resolved_frame(frame_index)
            .map_err(|error| error.to_string())?;
        let logical_frame = &sprite.frames[frame_index];
        let (sequence_index, facing_index, frame_in_facing) = sprite
            .frame_location(frame_index)
            .map_err(|error| error.to_string())?;
        let sequence = &sprite.sequences[sequence_index];
        let facing = &sprite.facings[facing_index];
        let sequence_label = sequence_labels
            .get(sequence_index)
            .filter(|labels| !labels.is_empty())
            .map(|labels| labels.join("/"))
            .unwrap_or_else(|| "unnamed".to_owned());
        let placement = if !logical_frame.hotspots.is_empty() {
            logical_frame
                .hotspots
                .iter()
                .map(|hotspot| format!("{}:({},{})", hotspot.id, hotspot.x, hotspot.y))
                .collect::<Vec<_>>()
                .join("|")
        } else if let (Some(x), Some(y)) = (logical_frame.origin_x, logical_frame.origin_y) {
            format!("origin=({x},{y})")
        } else {
            "placement=inherited".to_owned()
        };
        let title = format!(
            "Lords of Magic IMP viewer — {} — {} {}/{} — facing {}/{} — frame {}/{} (global {}/{}, {}×{}, {} bpp, {}, {}, seq={}, facing=0x{:04x}{})",
            entry.name,
            sequence_label,
            sequence_index + 1,
            sprite.sequences.len(),
            facing_index - sequence.first_facing + 1,
            sequence.facing_count,
            frame_in_facing + 1,
            facing.frame_count,
            frame_index + 1,
            sprite.frames.len(),
            frame.width,
            frame.height,
            sprite.bits_per_pixel,
            display_mode.label(),
            placement,
            hex_bytes(&sequence.metadata),
            facing.metadata,
            if playing { ", playing" } else { "" }
        );
        canvas
            .window_mut()
            .set_title(&title)
            .map_err(|error| error.to_string())?;
        let display_rgba = imp_display_rgba(
            &frame.palette_indices,
            &frame.rgba,
            display_mode,
            sprite.color_key,
        );
        draw_rgba_in_bounds(
            &mut canvas,
            frame.width,
            frame.height,
            sprite.maximum_width,
            sprite.maximum_height,
            &display_rgba,
        )?;
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

fn load_imp_sequence_labels(
    archive: &Archive,
    entries: &[Entry],
    sprite_name: &str,
) -> Vec<Vec<String>> {
    let Some(stem) = sprite_name.strip_suffix(".imp").or_else(|| {
        sprite_name
            .to_ascii_lowercase()
            .strip_suffix(".imp")
            .map(|_| &sprite_name[..sprite_name.len() - 4])
    }) else {
        return Vec::new();
    };
    let header_name = format!("{stem}.h");
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(&header_name))
    else {
        return Vec::new();
    };
    archive
        .read(&entry.name)
        .ok()
        .and_then(|bytes| ImpHeaderStats::parse(&bytes).ok())
        .map(|stats| stats.sequence_labels)
        .unwrap_or_default()
}

fn find_imp_frame(
    sprite: &ImpSprite,
    current: usize,
    direction: isize,
    include_current: bool,
) -> Result<usize, String> {
    if sprite.frames.is_empty() {
        return Err("IMP sprite contains no frames".to_owned());
    }
    let first_distance = usize::from(!include_current);
    for distance in first_distance..first_distance + sprite.frames.len() {
        let index = (current as isize + direction * distance as isize)
            .rem_euclid(sprite.frames.len() as isize) as usize;
        let frame = sprite
            .resolved_frame(index)
            .map_err(|error| error.to_string())?;
        if frame.width > 0 && frame.height > 0 && !frame.rgba.is_empty() {
            return Ok(index);
        }
    }
    Err("IMP sprite contains no visible frames".to_owned())
}

fn step_imp_frame(sprite: &ImpSprite, current: usize, direction: isize) -> Result<usize, String> {
    let (_, facing_index, frame_in_facing) = sprite
        .frame_location(current)
        .map_err(|error| error.to_string())?;
    find_visible_in_facing(sprite, facing_index, frame_in_facing, direction, false)
}

fn step_imp_facing(sprite: &ImpSprite, current: usize, direction: isize) -> Result<usize, String> {
    let (sequence_index, facing_index, _) = sprite
        .frame_location(current)
        .map_err(|error| error.to_string())?;
    let sequence = &sprite.sequences[sequence_index];
    let relative_facing = facing_index - sequence.first_facing;
    for distance in 1..=sequence.facing_count {
        let relative = (relative_facing as isize + direction * distance as isize)
            .rem_euclid(sequence.facing_count as isize) as usize;
        let candidate = sequence.first_facing + relative;
        if let Ok(frame) = find_visible_in_facing(sprite, candidate, 0, 1, true) {
            return Ok(frame);
        }
    }
    Err("IMP sequence contains no visible facings".to_owned())
}

fn step_imp_sequence(
    sprite: &ImpSprite,
    current: usize,
    direction: isize,
) -> Result<usize, String> {
    let (sequence_index, _, _) = sprite
        .frame_location(current)
        .map_err(|error| error.to_string())?;
    for distance in 1..=sprite.sequences.len() {
        let candidate = (sequence_index as isize + direction * distance as isize)
            .rem_euclid(sprite.sequences.len() as isize) as usize;
        let sequence = &sprite.sequences[candidate];
        for relative_facing in 0..sequence.facing_count {
            if let Ok(frame) =
                find_visible_in_facing(sprite, sequence.first_facing + relative_facing, 0, 1, true)
            {
                return Ok(frame);
            }
        }
    }
    Err("IMP sprite contains no visible sequences".to_owned())
}

fn find_visible_in_facing(
    sprite: &ImpSprite,
    facing_index: usize,
    current_offset: usize,
    direction: isize,
    include_current: bool,
) -> Result<usize, String> {
    let facing = sprite
        .facings
        .get(facing_index)
        .ok_or_else(|| format!("IMP facing index {facing_index} is out of range"))?;
    if facing.frame_count == 0 {
        return Err("IMP facing contains no frames".to_owned());
    }
    let first_distance = usize::from(!include_current);
    for distance in first_distance..first_distance + facing.frame_count {
        let offset = (current_offset as isize + direction * distance as isize)
            .rem_euclid(facing.frame_count as isize) as usize;
        let index = facing.first_frame + offset;
        let frame = sprite
            .resolved_frame(index)
            .map_err(|error| error.to_string())?;
        if frame.width > 0 && frame.height > 0 && !frame.rgba.is_empty() {
            return Ok(index);
        }
    }
    Err("IMP facing contains no visible frames".to_owned())
}

fn view_map_file(path: &Path, tile_set_paths: Option<&(PathBuf, PathBuf)>) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
    let map_width = u16::try_from(map.width)
        .map_err(|_| format!("map width {} exceeds viewer limits", map.width))?;
    let map_height = u16::try_from(map.height)
        .map_err(|_| format!("map height {} exceeds viewer limits", map.height))?;
    let terrain_preview = tile_set_paths
        .map(|(definition, atlas)| load_terrain_preview(&map, definition, atlas))
        .transpose()?;
    let mut display_mode = if terrain_preview.is_some() {
        MapDisplayMode::TerrainArtwork
    } else {
        MapDisplayMode::CandidateElevation
    };

    let sdl = sdl3::init().map_err(|error| error.to_string())?;
    let video = sdl.video().map_err(|error| error.to_string())?;
    let window = video
        .window(
            "Lords of Magic diagnostic map viewer",
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        )
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| error.to_string())?;
    let mut canvas = window.into_canvas();
    let mut event_pump = sdl.event_pump().map_err(|error| error.to_string())?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    repeat: false,
                    ..
                } => display_mode = display_mode.next(terrain_preview.is_some()),
                _ => {}
            }
        }
        let atlas_suffix = match (display_mode, terrain_preview.as_ref()) {
            (MapDisplayMode::TerrainArtwork, Some(preview)) => {
                format!(" — atlas {}", preview.atlas_name)
            }
            _ => String::new(),
        };
        let title = format!(
            "Lords of Magic map viewer — {} — {}×{} — {}{} — C changes mode",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unnamed map"),
            map.width,
            map.height,
            display_mode.label(),
            atlas_suffix,
        );
        canvas
            .window_mut()
            .set_title(&title)
            .map_err(|error| error.to_string())?;
        match (display_mode, terrain_preview.as_ref()) {
            (MapDisplayMode::TerrainArtwork, Some(preview)) => draw_rgba_in_bounds(
                &mut canvas,
                preview.width,
                preview.height,
                preview.width,
                preview.height,
                &preview.rgba,
            )?,
            _ => {
                let rgba = map_display_rgba(&map, display_mode);
                draw_rgba_in_bounds(
                    &mut canvas,
                    map_width,
                    map_height,
                    map_width,
                    map_height,
                    &rgba,
                )?;
            }
        }
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

fn load_terrain_preview(
    map: &MapAsset,
    definition_path: &Path,
    atlas_path: &Path,
) -> Result<TerrainPreview, String> {
    let definition_bytes = fs::read(definition_path).map_err(|error| {
        format!(
            "could not read tile definition {}: {error}",
            definition_path.display()
        )
    })?;
    let tile_set = TileSetDefinition::parse(&definition_bytes).map_err(|error| {
        format!(
            "could not parse tile definition {}: {error}",
            definition_path.display()
        )
    })?;
    let atlas_bytes = fs::read(atlas_path).map_err(|error| {
        format!(
            "could not read tile atlas {}: {error}",
            atlas_path.display()
        )
    })?;
    let atlas = PbmImage::decode(&atlas_bytes).map_err(|error| {
        format!(
            "could not decode tile atlas {}: {error}",
            atlas_path.display()
        )
    })?;

    let expected_width = tile_set
        .columns
        .checked_mul(tile_set.tile_width)
        .ok_or_else(|| "tile atlas width overflow".to_owned())?;
    let expected_height = tile_set
        .rows
        .checked_mul(tile_set.tile_height)
        .ok_or_else(|| "tile atlas height overflow".to_owned())?;
    if u32::from(atlas.width) != expected_width || u32::from(atlas.height) != expected_height {
        return Err(format!(
            "tile atlas {} is {}x{}, but {} declares {}x{}",
            atlas_path.display(),
            atlas.width,
            atlas.height,
            definition_path.display(),
            expected_width,
            expected_height,
        ));
    }

    let width = map
        .width
        .checked_mul(TERRAIN_PREVIEW_TILE_SIZE)
        .and_then(|width| u16::try_from(width).ok())
        .ok_or_else(|| "terrain preview width exceeds viewer limits".to_owned())?;
    let height = map
        .height
        .checked_mul(TERRAIN_PREVIEW_TILE_SIZE)
        .and_then(|height| u16::try_from(height).ok())
        .ok_or_else(|| "terrain preview height exceeds viewer limits".to_owned())?;
    let rgba = terrain_preview_rgba(map, &tile_set, &atlas)?;
    Ok(TerrainPreview {
        width,
        height,
        rgba,
        atlas_name: tile_set.atlas_member,
    })
}

fn terrain_preview_rgba(
    map: &MapAsset,
    tile_set: &TileSetDefinition,
    atlas: &PbmImage,
) -> Result<Vec<u8>, String> {
    let preview_width = usize::try_from(map.width)
        .ok()
        .and_then(|width| width.checked_mul(TERRAIN_PREVIEW_TILE_SIZE as usize))
        .ok_or_else(|| "terrain preview width overflow".to_owned())?;
    let preview_height = usize::try_from(map.height)
        .ok()
        .and_then(|height| height.checked_mul(TERRAIN_PREVIEW_TILE_SIZE as usize))
        .ok_or_else(|| "terrain preview height overflow".to_owned())?;
    let output_bytes = preview_width
        .checked_mul(preview_height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "terrain preview byte count overflow".to_owned())?;
    let atlas_width = usize::from(atlas.width);
    let sample_dimension = TERRAIN_PREVIEW_TILE_SIZE as usize;
    let mut rgba = vec![0; output_bytes];

    for y in 0..map.height {
        for x in 0..map.width {
            let cell = map
                .cell(x, y)
                .ok_or_else(|| format!("map has no cell at ({x}, {y})"))?;
            let tile_index = cell.tile_index();
            if tile_index >= tile_set.atlas_capacity() {
                return Err(format!(
                    "map cell ({x}, {y}) references tile {tile_index}, outside atlas capacity {}",
                    tile_set.atlas_capacity()
                ));
            }
            if !tile_set.tiles.contains_key(&tile_index) {
                return Err(format!(
                    "map cell ({x}, {y}) references undefined tile {tile_index}"
                ));
            }
            let tile_x = tile_index % tile_set.columns;
            let tile_y = tile_index / tile_set.columns;
            for sample_y in 0..sample_dimension {
                let atlas_y = usize::try_from(tile_y * tile_set.tile_height).unwrap()
                    + ((sample_y * 2 + 1) * tile_set.tile_height as usize) / (sample_dimension * 2);
                let output_y = y as usize * sample_dimension + sample_y;
                for sample_x in 0..sample_dimension {
                    let atlas_x = usize::try_from(tile_x * tile_set.tile_width).unwrap()
                        + ((sample_x * 2 + 1) * tile_set.tile_width as usize)
                            / (sample_dimension * 2);
                    let output_x = x as usize * sample_dimension + sample_x;
                    let source_offset = (atlas_y * atlas_width + atlas_x) * 4;
                    let output_offset = (output_y * preview_width + output_x) * 4;
                    rgba[output_offset..output_offset + 4]
                        .copy_from_slice(&atlas.rgba[source_offset..source_offset + 4]);
                }
            }
        }
    }
    Ok(rgba)
}

fn map_display_rgba(map: &MapAsset, mode: MapDisplayMode) -> Vec<u8> {
    let (minimum, maximum) = map
        .cells
        .iter()
        .filter_map(|cell| cell.value.is_finite().then_some(cell.value))
        .fold(
            (f32::INFINITY, f32::NEG_INFINITY),
            |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
        );
    (0..map.height)
        .flat_map(|y| (0..map.width).map(move |x| map.cell(x, y).expect("bounded map cell")))
        .flat_map(|cell| {
            let rgb = match mode {
                MapDisplayMode::CellTags => diagnostic_tag_color(cell.tag),
                MapDisplayMode::CandidateElevation => {
                    let normalized = if maximum > minimum && cell.value.is_finite() {
                        (cell.value - minimum) / (maximum - minimum)
                    } else {
                        0.0
                    };
                    let intensity = (normalized.clamp(0.0, 1.0) * 255.0).round() as u8;
                    [intensity, intensity, intensity]
                }
                MapDisplayMode::TerrainArtwork => unreachable!("terrain artwork is pre-rendered"),
            };
            [rgb[0], rgb[1], rgb[2], 255]
        })
        .collect()
}

fn diagnostic_tag_color(tag: u32) -> [u8; 3] {
    let mut mixed = tag.wrapping_mul(0x9e37_79b1);
    mixed ^= mixed >> 16;
    [
        48 + ((mixed >> 16) as u8 % 192),
        48 + ((mixed >> 8) as u8 % 192),
        48 + (mixed as u8 % 192),
    ]
}

fn view_archive(source: &Source, requested: Option<&str>) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let mut selected = select_image(&archive, &entries, requested)?;

    let sdl = sdl3::init().map_err(|error| error.to_string())?;
    let video = sdl.video().map_err(|error| error.to_string())?;
    let window = video
        .window("Lords of Magic asset viewer", WINDOW_WIDTH, WINDOW_HEIGHT)
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| error.to_string())?;
    let mut canvas = window.into_canvas();
    let mut event_pump = sdl.event_pump().map_err(|error| error.to_string())?;
    set_title(&mut canvas, &selected)?;

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Right | Keycode::Down | Keycode::Space),
                    repeat: false,
                    ..
                } => {
                    selected = scan_images(&archive, &entries, selected.index, 1)?;
                    set_title(&mut canvas, &selected)?;
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Left | Keycode::Up),
                    repeat: false,
                    ..
                } => {
                    selected = scan_images(&archive, &entries, selected.index, -1)?;
                    set_title(&mut canvas, &selected)?;
                }
                _ => {}
            }
        }

        draw(&mut canvas, &selected.image)?;
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

fn select_image(
    archive: &Archive,
    entries: &[Entry],
    requested: Option<&str>,
) -> Result<SelectedImage, String> {
    if entries.is_empty() {
        return Err("archive is empty".to_owned());
    }

    if let Some(name) = requested {
        let index = entries
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("archive has no member named {name}"))?;
        return decode_entry(archive, entries, index)
            .map_err(|error| format!("{name} is not a supported PBM image: {error}"));
    }

    for index in 0..entries.len() {
        if let Ok(image) = decode_entry(archive, entries, index) {
            return Ok(image);
        }
    }
    Err("archive contains no decodable IFF PBM images".to_owned())
}

fn scan_images(
    archive: &Archive,
    entries: &[Entry],
    current: usize,
    direction: isize,
) -> Result<SelectedImage, String> {
    for distance in 1..=entries.len() {
        let index = (current as isize + direction * distance as isize)
            .rem_euclid(entries.len() as isize) as usize;
        if let Ok(image) = decode_entry(archive, entries, index) {
            return Ok(image);
        }
    }
    Err("archive contains no decodable IFF PBM images".to_owned())
}

fn decode_entry(
    archive: &Archive,
    entries: &[Entry],
    index: usize,
) -> Result<SelectedImage, String> {
    let entry = entries
        .get(index)
        .ok_or_else(|| "archive entry index is out of bounds".to_owned())?;
    let bytes = archive
        .read(&entry.name)
        .map_err(|error| error.to_string())?;
    let image = PbmImage::decode(&bytes).map_err(|error| error.to_string())?;
    Ok(SelectedImage {
        index,
        name: entry.name.clone(),
        image,
    })
}

fn set_title(canvas: &mut Canvas<Window>, selected: &SelectedImage) -> Result<(), String> {
    let title = format!(
        "Lords of Magic asset viewer — {} ({}×{})",
        selected.name, selected.image.width, selected.image.height
    );
    canvas
        .window_mut()
        .set_title(&title)
        .map_err(|error| error.to_string())
}

fn draw(canvas: &mut Canvas<Window>, image: &PbmImage) -> Result<(), String> {
    draw_rgba(canvas, image.width, image.height, &image.rgba)
}

fn draw_rgba(
    canvas: &mut Canvas<Window>,
    width: u16,
    height: u16,
    rgba: &[u8],
) -> Result<(), String> {
    draw_rgba_in_bounds(canvas, width, height, width, height, rgba)
}

fn imp_display_rgba(
    palette_indices: &[u8],
    source: &[u8],
    mode: ImpDisplayMode,
    color_key: u8,
) -> Vec<u8> {
    debug_assert_eq!(palette_indices.len() * 4, source.len());
    if matches!(mode, ImpDisplayMode::Raw) {
        return source.to_vec();
    }
    source
        .chunks_exact(4)
        .zip(palette_indices)
        .flat_map(|(rgba, palette_index)| {
            let mut pixel: [u8; 4] = rgba.try_into().expect("RGBA chunks have four bytes");
            // Compositing is keyed by palette INDEX, not by colour: the header's colour
            // key marks transparency and slot 1 is the shadow silhouette. The RGB values
            // those slots hold are incidental art-tool choices, confirmed by experiment:
            // rewriting slot 1 to magenta rendered identically to the untouched copy.
            let background = *palette_index == color_key;
            let shadow = *palette_index == 1;
            if background || matches!(mode, ImpDisplayMode::Preview) && shadow {
                pixel[3] = 0;
            }
            pixel
        })
        .collect()
}

fn draw_rgba_in_bounds(
    canvas: &mut Canvas<Window>,
    width: u16,
    height: u16,
    bounds_width: u16,
    bounds_height: u16,
    rgba: &[u8],
) -> Result<(), String> {
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(PixelFormat::RGBA32, u32::from(width), u32::from(height))
        .map_err(|error| error.to_string())?;
    texture.set_scale_mode(ScaleMode::Nearest);
    texture.set_blend_mode(BlendMode::Blend);
    texture
        .update(None, rgba, usize::from(width) * 4)
        .map_err(|error| error.to_string())?;

    let (output_width, output_height) = canvas.output_size().map_err(|error| error.to_string())?;
    let scale = (output_width as f32 / f32::from(bounds_width))
        .min(output_height as f32 / f32::from(bounds_height));
    let destination_width = f32::from(width) * scale;
    let destination_height = f32::from(height) * scale;
    let destination = FRect::new(
        (output_width as f32 - destination_width) / 2.0,
        (output_height as f32 - destination_height) / 2.0,
        destination_width,
        destination_height,
    );

    canvas.set_draw_color(Color::RGB(18, 18, 22));
    canvas.clear();
    canvas
        .copy(&texture, None, destination)
        .map_err(|error| error.to_string())?;
    canvas.present();
    Ok(())
}

/// Executable-name counts paired with definition-name counts for a whole archive.
type GameScriptNameCounts = (BTreeMap<String, usize>, BTreeMap<String, usize>);

/// Collect executable-name and definition-name counts across every `.gs` member of an archive.
fn collect_gamescript_names(source: &Source) -> Result<GameScriptNameCounts, String> {
    let (archive, entries) = open_archive(source)?;
    let mut executable_names = BTreeMap::<String, usize>::new();
    let mut definition_names = BTreeMap::<String, usize>::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.name.to_ascii_lowercase().ends_with(".gs"))
    {
        let Ok(bytes) = archive.read(&entry.name) else {
            continue;
        };
        let Ok(document) = GameScriptDocument::parse(&bytes) else {
            continue;
        };
        let analysis = document.analyze();
        merge_name_counts(&mut executable_names, &analysis.executable_names);
        merge_name_counts(&mut definition_names, &analysis.definition_names);
    }
    Ok((executable_names, definition_names))
}

/// Report the engine's operator tables, and optionally reconcile them against a script corpus.
fn scan_native_table(executable: &Path, source: Option<&Source>) -> Result<(), String> {
    let image = fs::read(executable)
        .map_err(|error| format!("could not read executable {}: {error}", executable.display()))?;
    let runs = native_table::extract(&image)
        .map_err(|error| format!("could not read the operator table: {error}"))?;

    println!("operator-table-runs\t{}", runs.len());
    for run in &runs {
        println!(
            "operator-table-run\t{:#x}\t{}\t{}",
            run.file_offset,
            run.entries.len(),
            run.entries
                .iter()
                .take(4)
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    let natives = native_table::operator_names(&runs);
    println!("distinct-operators\t{}", natives.len());

    let Some(source) = source else {
        let pe = native_table::PeImage::parse(&image)
            .map_err(|error| format!("could not read the executable: {error}"))?;
        let mut exact = 0_usize;
        let mut walked = 0_usize;
        let mut rows = Vec::new();
        for run in &runs {
            for entry in &run.entries {
                match operator_arity::stack_effect(&pe, entry.entry_point) {
                    Ok(effect) => {
                        walked += 1;
                        if effect.is_well_formed() {
                            exact += 1;
                        }
                        rows.push(format!(
                            "operator\t{}\t{:#010x}\t{}\t{}\t{}",
                            entry.name,
                            entry.entry_point,
                            effect.pops,
                            effect.pushes,
                            effect.confidence()
                        ));
                    }
                    Err(error) => {
                        rows.push(format!(
                            "operator\t{}\t{:#010x}\t-\t-\t{error}",
                            entry.name, entry.entry_point
                        ));
                    }
                }
            }
        }
        println!("operators-walked\t{walked}");
        println!("operators-with-well-formed-walk\t{exact}");
        println!("operator-columns\tname\tentry-point\tpops\tpushes\tconfidence");
        for row in rows {
            println!("{row}");
        }
        return Ok(());
    };

    let (executable_names, definition_names) = collect_gamescript_names(source)?;

    // Reuse the established candidate rule: a name the corpus calls, never defines, and which
    // appears verbatim in the executable. Comparing the operator table against the raw
    // called-but-not-defined set instead is misleading, because that set is dominated by names
    // whose definition site our definition-shape classifier does not recognise.
    let candidates =
        likely_engine_names(executable, &executable_names, &definition_names)?.candidates;

    let mut confirmed = 0_usize;
    let mut unconfirmed = Vec::new();
    for (name, count) in &candidates {
        if natives.contains(&name.to_ascii_lowercase()) {
            confirmed += 1;
        } else {
            unconfirmed.push((name.clone(), *count));
        }
    }

    let called: BTreeSet<String> = candidates
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    let never_called: Vec<&String> = natives.iter().filter(|name| !called.contains(*name)).collect();

    // Unconfirmed names divide sharply by shape: SCREAMING_CASE names are engine constants pushed
    // by name rather than operators, so they are absent from the operator table by construction.
    let constant_like = unconfirmed
        .iter()
        .filter(|(name, _)| is_screaming_case(name))
        .count();

    println!("engine-name-candidates\t{}", candidates.len());
    println!("candidates-confirmed-as-operators\t{confirmed}");
    println!("candidates-unconfirmed\t{}", unconfirmed.len());
    println!("unconfirmed-screaming-case\t{constant_like}");
    println!("unconfirmed-other\t{}", unconfirmed.len() - constant_like);
    println!("operators-never-called\t{}", never_called.len());

    for (name, count) in unconfirmed
        .iter()
        .filter(|(name, _)| !is_screaming_case(name))
    {
        println!("unconfirmed-name\t{name}\t{count}");
    }
    for name in never_called {
        println!("operator-never-called\t{name}");
    }
    Ok(())
}

/// Whether a name is written in the SCREAMING_CASE the corpus uses for engine constants.
fn is_screaming_case(name: &str) -> bool {
    name.chars().any(|character| character.is_ascii_uppercase())
        && !name.chars().any(|character| character.is_ascii_lowercase())
}

/// Print the engine's terrain-sprite-type table.
///
/// Profile-specific: these ids come from the working GS5R3 script set, assigned in script execution
/// order, so a different mod shifts every one of them. That warning is printed with the table
/// rather than buried in a document, because the table is most useful to somebody about to write an
/// id into a file.
fn sprite_types() -> Result<(), String> {
    println!("sprite-types\t{}", TERRAIN_SPRITE_TYPES.len());
    for (name, id) in TERRAIN_SPRITE_TYPES {
        println!("sprite\t{id}\t{name}");
    }
    for name in TERRAIN_SPRITE_ARRAYS {
        println!("array\t{name}\tper-faith table, not yet enumerated");
    }
    for name in TERRAIN_SPRITE_NAME_ONLY {
        println!("name-only\t{name}\tlogged a name, no usable value");
    }
    println!("dict-entries-counted-by-the-probe\t196");
    println!("dict-entries-unresolved\t1\tcounted but never logged; unidentified");
    eprintln!(
        "note: these ids are assigned in script execution order and are specific to the profile \
         they were dumped from. Re-run the terrainrings probe against any profile whose maps you \
         intend to edit."
    );
    Ok(())
}

/// Print the measured `setterrain` transition behaviour for every background terrain.
fn transition_rings() -> Result<(), String> {
    // Derived from the constant, not written out. A hand-typed header would silently disagree with
    // the table the moment the table was regenerated, and this command exists to be trusted.
    let header = TRANSITION_RING_OFFSETS
        .iter()
        .map(|entry| {
            let name = match entry.direction {
                (0, -1) => "N",
                (0, 1) => "S",
                (-1, 0) => "W",
                (1, 0) => "E",
                (-1, -1) => "NW",
                (1, -1) => "NE",
                (-1, 1) => "SW",
                _ => "SE",
            };
            format!("{name}:{}", entry.offset)
        })
        .collect::<Vec<_>>()
        .join("\t");
    println!("direction-offsets\t{header}");
    for entry in TERRAIN_TYPES {
        let background = entry.terrain_type;
        let name = entry.script_names[0];
        match transition_anchor(background) {
            Some(anchor) => {
                // Any painted terrain other than this one or road gives the same ring, which is
                // the finding; pick the first such terrain rather than hard-coding one.
                let painted = TERRAIN_TYPES
                    .iter()
                    .map(|other| other.terrain_type)
                    .find(|painted| *painted != background && *painted != ROAD_TERRAIN)
                    .unwrap_or(0);
                let ring = transition_ring(background, painted)
                    .ok_or("a blending background must yield a ring")?;
                println!(
                    "ring\t{background}\t{name}\tanchor:{anchor}\t{}",
                    ring.iter().map(u32::to_string).collect::<Vec<_>>().join("\t")
                );
            }
            None if background == ROAD_TERRAIN => {
                println!("ring\t{background}\t{name}\tper-painted-terrain; corners keep the background");
                for painted in TERRAIN_TYPES.iter().map(|other| other.terrain_type) {
                    match road_background_ring(painted) {
                        Some(ring) => println!(
                            "road-ring\tpainted:{painted}\t{}",
                            ring.iter().map(u32::to_string).collect::<Vec<_>>().join("\t")
                        ),
                        None => println!("road-ring\tpainted:{painted}\tno ring"),
                    }
                }
            }
            None => println!("ring\t{background}\t{name}\tblends nothing: the ring keeps the background tile"),
        }
    }
    for entry in TERRAIN_TYPES {
        if let Some((low, high)) = interior_tile_family(entry.terrain_type) {
            println!(
                "interior\t{}\t{}\t{low}..{high}\trandomised per paint",
                entry.terrain_type, entry.script_names[0]
            );
        }
    }
    eprintln!(
        "note: rings measured for 3x3 regions on a forced uniform background. tt_road is ragged \
         along every edge as a PAINTED terrain, so it has no per-direction ring. A region's \
         interior is picked at random from its terrain's family and cannot be reproduced."
    );
    Ok(())
}

/// A terrain type, given either as its number `0..=10` or as one of its `gs\maplib.gs` names.
///
/// Names are accepted with or without the `tt_` prefix, because a modder reading `maplib.gs` sees
/// `tt_water` and a modder reading a map dump sees `water`.
/// A terrain type for `--map-paint-terrain`, whose ceiling is the **supplied tileset** and not the
/// eleven-name world table.
///
/// **Found by measurement, 2026-09-17.** `parse_terrain_type` rejects any numeric id above 10
/// because `terrain_type_base_tile` only knows `tilesb01.til`'s eleven types. That is right for
/// `--map-set-terrain` and `--map-fill-terrain`, which have no tileset and really do depend on that
/// table. It is wrong for a paint, which is handed a `.til` and asks it for candidates: terrain ids
/// are **tileset-local** and the combat tilesets reach 42, so the eleven-type ceiling refused every
/// terrain the shipped battle maps are actually made of. Painting a sampled grid over the 169 bound
/// combat maps succeeded at 7.5% of sites before this and fails almost entirely on
/// `21 is not one of the 11 terrain types`.
///
/// Nothing is loosened downstream: [`PaintRefusal::TerrainTypeNotInTileSet`] already refuses a
/// terrain the supplied tileset declares no tiles for, which is the check that should have been
/// deciding this all along. Names still resolve through the world table, because a name is only
/// meaningful in a vocabulary and that is the only vocabulary this project has one for.
fn parse_paint_terrain_type(value: &str) -> Result<u32, String> {
    if let Ok(number) = value.parse::<u32>() {
        return Ok(number);
    }
    parse_terrain_type(value)
}

fn parse_terrain_type(value: &str) -> Result<u32, String> {
    if let Ok(number) = value.parse::<u32>() {
        return terrain_type_base_tile(number)
            .map(|_| number)
            .ok_or_else(|| format!("{number} is not one of the 11 terrain types"));
    }
    let wanted = value.to_ascii_lowercase();
    TERRAIN_TYPES
        .iter()
        .find(|entry| {
            entry.script_names.iter().any(|name| {
                *name == wanted || name.strip_prefix("tt_") == Some(wanted.as_str())
            })
        })
        .map(|entry| entry.terrain_type)
        .ok_or_else(|| {
            let names = TERRAIN_TYPES
                .iter()
                .map(|entry| entry.script_names[0])
                .collect::<Vec<_>>()
                .join(", ");
            format!("{value} is not a terrain type; expected 0..10 or one of: {names}")
        })
}

/// A terrain sprite type, given either as its name or as a raw id.
///
/// Names come from the engine's own `terrainsprites` dict, dumped by the 2026-09-17 probe. This is
/// the thing that makes object placement usable: `sprite_type` is assigned in script execution
/// order, so a raw id says nothing about what it is, and until the table existed a caller had no
/// way to ask for a keep rather than a number.
///
/// A raw id is still accepted, and deliberately not range-checked against the table: ids above it
/// are runtime registrations by `addterrainspritetype`, which is how the probe's own type 470 came
/// to exist, and refusing those would refuse a legitimate record shape.
fn parse_sprite_type(value: &str) -> Result<u32, String> {
    if let Ok(id) = value.parse::<u32>() {
        return Ok(id);
    }
    if let Some(id) = terrain_sprite_type(value) {
        // The dangerous path, and it used to be the silent one. `--map-sprite-types` warned that
        // the table is profile-specific; resolving a name for an actual write did not, so on a
        // modded profile this would quietly put GS5R3's id into a file where it means something
        // else. The warning belongs where the id gets written, not only where it gets listed.
        eprintln!(
            "note: {value} is type {id} in the GS5R3 script set this table was dumped from. Ids \
             are assigned in script execution order, so a profile with different scripts assigns \
             them differently -- re-run the terrainrings probe against the profile you are editing."
        );
        return Ok(id);
    }
    let mut close: Vec<&str> = TERRAIN_SPRITE_TYPES
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| {
                let needle = value.to_ascii_lowercase();
                name.contains(&needle) || needle.contains(*name)
            })
            .collect();
        close.sort_unstable();
        close.truncate(8);
    let hint = if close.is_empty() {
        "run --map-sprite-types for the full list".to_owned()
    } else {
        format!("did you mean: {}", close.join(", "))
    };
    Err(format!("{value} is not a known terrain sprite type; {hint}"))
}

fn parse_flag(value: &str) -> Result<bool, String> {
    match value {
        "0" | "false" | "clear" => Ok(false),
        "1" | "true" | "set" => Ok(true),
        other => Err(format!("{other} is not 0 or 1")),
    }
}

/// Create a map from nothing, composing only byte patterns the engine was observed writing.
///
/// The engine has never been asked to load a map this project created, and no map anywhere has
/// ever been non-square, so both are warned about at the point of use rather than only in a
/// document nobody reads at the terminal.
fn create_map(width: u32, height: u32, terrain_type: u32, output: &Path) -> Result<(), String> {
    let map = MapAsset::create(width, height, terrain_type).map_err(|error| error.to_string())?;
    let encoded = map.to_bytes().map_err(|error| error.to_string())?;
    let reparsed = MapAsset::parse(&encoded)
        .map_err(|error| format!("refusing to write: the new map does not parse: {error}"))?;
    if (reparsed.width, reparsed.height) != (width, height) {
        return Err("refusing to write: the new map read back with different dimensions".to_owned());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    if let Err(error) = file.write_all(&encoded) {
        drop(file);
        let _ = fs::remove_file(output);
        return Err(format!("could not write {}: {error}", output.display()));
    }

    eprintln!(
        "note: the engine loaded a map created this way on 2026-09-17 and re-saved it \
         byte-identically, at 64x64 and at 96x64. Header word \
         0x{GENERATED_HEADER_WORD:02x} and the empty trailing section are values the engine itself \
         wrote; the engine rewrites the header word on save regardless."
    );
    if width != height {
        eprintln!(
            "note: {width}x{height} is non-square. No shipped or engine-generated map is; the \
             engine loaded a 96x64 created this way on 2026-09-17 and reported its size back \
             correctly. Other non-square shapes are still untested."
        );
    }
    println!("wrote\t{}\t{} bytes", output.display(), encoded.len());
    println!(
        "created\t{width}x{height}\tterrain:{terrain_type}\ttile:{}\theader:0x{GENERATED_HEADER_WORD:02x}",
        terrain_type_base_tile(terrain_type).unwrap_or_default()
    );
    Ok(())
}

fn parse_elevation(value: &str) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| format!("{value} is not an elevation"))?;
    if !parsed.is_finite() {
        return Err(format!("elevation {value} is not finite"));
    }
    Ok(parsed)
}

/// Re-encode every map under `path` and compare the result to the bytes on disk.
///
/// This is the load-bearing test for everything else in this file. Editing a map is only safe if
/// *not* editing it is a no-op at the byte level, across the families this project has decoded and
/// the ones it has not. Run it against the installed `map/` directory before trusting an edit.
fn roundtrip_maps(path: &Path) -> Result<(), String> {
    let mut paths = Vec::new();
    if path.is_dir() {
        collect_map_paths(path, &mut paths)?;
        paths.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    } else {
        paths.push(path.to_path_buf());
    }

    let mut checked = 0_usize;
    let mut identical = 0_usize;
    let mut record_roundtrips = 0_usize;
    let mut failures = Vec::new();

    // `collect_map_paths` already filtered a directory walk down to map extensions. A path given
    // explicitly is tried whatever it is called, so a file outside the naming convention -- a
    // community map, a probe output -- can still be checked.
    for path in &paths {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{}: could not read: {error}", path.display()));
                continue;
            }
        };
        let map = match MapAsset::parse(&bytes) {
            Ok(map) => map,
            Err(error) => {
                failures.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        checked += 1;

        // Each decoded record must rebuild its own bytes from its typed fields alone. This is
        // stricter than the file comparison and it is what makes an *edited* record trustworthy:
        // the file can round-trip through `trailing_raw` while a field is being written back wrong.
        if let Some(section) = &map.placed_sprites {
            for (index, record) in section.records.iter().enumerate() {
                if record.to_bytes().as_deref() != Ok(record.raw.as_slice()) {
                    failures.push(format!(
                        "{}: record {index} does not rebuild from its fields",
                        path.display()
                    ));
                } else {
                    record_roundtrips += 1;
                }
            }
        }

        let encoded = map.to_bytes().map_err(|error| error.to_string())?;
        match first_tail_difference(&bytes, &encoded) {
            None => identical += 1,
            Some(at) => {
                // Distinguish "these bytes differ" from "one side ended here": at a length
                // mismatch `first_tail_difference` returns the common length, and reporting a
                // byte that does not exist as a value would be a wrong failure message on the one
                // command whose whole job is to be believed.
                let describe = |source: &[u8]| {
                    source
                        .get(at)
                        .map_or_else(|| "end-of-file".to_owned(), |byte| format!("0x{byte:02x}"))
                };
                failures.push(format!(
                    "{}: byte {at} differs (read {}, wrote {}; {} bytes in, {} bytes out)",
                    path.display(),
                    describe(&bytes),
                    describe(&encoded),
                    bytes.len(),
                    encoded.len(),
                ))
            }
        }
    }

    println!("checked\t{checked}");
    println!("byte-identical\t{identical}");
    println!("records-rebuilt-from-fields\t{record_roundtrips}");
    println!("failures\t{}", failures.len());
    for failure in &failures {
        println!("failure\t{}", clean_field(failure));
    }
    if !failures.is_empty() {
        return Err(format!("{} maps did not round-trip", failures.len()));
    }
    // Zero files checked is not a pass. This command is the load-bearing evidence for everything
    // else here, and a mistyped-but-existing directory would otherwise print `failures 0` and exit
    // 0 -- a green corpus validation that validated nothing.
    if checked == 0 {
        return Err(format!("no map files were checked under {}", path.display()));
    }
    Ok(())
}

/// Apply one edit and write a **new** file.
///
/// The shape of this deliberately matches `--set-imp-placement`: apply, re-parse the bytes that
/// are about to be written, confirm the edit reads back, then `create_new`. The loose `map/`
/// directory has no backup, so there is no in-place mode and no overwrite of an existing output.
fn edit_map(
    input: &Path,
    edit: MapEdit,
    output: &Path,
    tile_set_path: Option<&Path>,
) -> Result<(), String> {
    if paths_are_same_file(input, output) {
        return Err(format!(
            "refusing to write to the input file {}; pass a different output path",
            input.display()
        ));
    }
    // **The output path decides what the edited map will be loaded as, so it has to agree with the
    // input.** A reviewer found `--map-paint-terrain realm.scn ... battle.smp tilesb01.til`
    // accepted: a world map painted through the world tileset and written under a name the tool
    // itself then reports as a combat map read through something else. Only the input path was
    // ever classified. Both are now, and a paint across the classes is refused.
    if matches!(edit, MapEdit::PaintTerrain { .. }) {
        let input_class = MapClass::from_path(input);
        let output_class = MapClass::from_path(output);
        if input_class != output_class {
            return Err(format!(
                "refusing to paint {} into {}: {} and {} are read through different tilesets, so \
                 writing one under the other's extension produces a map the game will draw with \
                 the wrong art. Use a matching extension",
                input.display(),
                output.display(),
                input_class.map_or_else(
                    || "an unclassified extension".to_owned(),
                    |class| format!("a {}", class.description())
                ),
                output_class.map_or_else(
                    || "an unclassified extension".to_owned(),
                    |class| format!("a {}", class.description())
                ),
            ));
        }
    }

    // A shipped tileset the gamescript does not bind this map to is refused before anything is
    // read. The binding is per encounter, so this consults the extracted `mapfile`/`tileset`
    // table and not a rule about map class. Two cases deliberately do **not** refuse: a tileset
    // name that is not one of the 26 shipped members is presumed modded, and a map with no
    // recorded binding has nothing to be measured against. An earlier version refused on class
    // alone and so rejected `aicave.smp` + `aibldg01.til` -- the gamescript's own pairing --
    // while accepting `tilesa01.til`, which writes slot 392 into a map whose atlas has 64 slots.
    if let Some(path) = tile_set_path
        && let Some(mismatch) = tileset_mismatch(input, path)
    {
        return Err(format!(
            "refusing to edit {}: {mismatch}",
            input.display()
        ));
    }

    // The tileset stays a required argument in effect -- nothing is defaulted and no file is
    // guessed at -- but the refusal now *names* the one the engine uses, which is the difference
    // between "supply a tileset" and "supply this tileset". The `.til` lives inside `pic.mpq`, so
    // only the member name can be resolved here; the caller still has to extract it and say where
    // it is.
    if tile_set_path.is_none()
        && matches!(edit, MapEdit::PaintTerrain { .. })
        && let Some(resolution) = resolve_tileset(input)
    {
        let advice = match &resolution {
            TileSetResolution::World(members) | TileSetResolution::Combat(members) => format!(
                "the gamescript reads it through {}. Extract that member from pic.mpq and pass \
                 its path as the trailing argument",
                members[0]
            ),
            TileSetResolution::CombatAmbiguous(members) => format!(
                "different encounters read it through {}, so there is no single answer -- pick \
                 the one matching the encounter you are editing and pass its path",
                members.join(" or ")
            ),
            TileSetResolution::CombatUnresolved => "no gamescript encounter binds this combat map \
                 to a tileset, so this project cannot name one and will not guess"
                .to_owned(),
        };
        return Err(format!(
            "no tileset was supplied. {} is a {}, and the map file does not record its tileset: \
             {advice}",
            input.display(),
            MapClass::from_path(input)
                .map_or("map", MapClass::description),
        ));
    }

    let source =
        fs::read(input).map_err(|error| format!("could not read {}: {error}", input.display()))?;
    let mut map = MapAsset::parse(&source).map_err(|error| error.to_string())?;
    let map_before = MapAsset::parse(&source).map_err(|error| error.to_string())?;

    // Loaded once and handed to both the edit and its verification. Two loads could disagree if
    // the file changed underneath, and then the check would be confirming a different decision.
    let tile_set = tile_set_path.map(load_tile_set).transpose()?;

    let note = apply_map_edit(&mut map, edit, tile_set.as_ref())?;
    let encoded = map.to_bytes().map_err(|error| error.to_string())?;

    // Re-parse before writing: a map we cannot read back is a map we must not emit.
    let reparsed = MapAsset::parse(&encoded).map_err(|error| {
        format!("refusing to write: the edited map no longer parses: {error}")
    })?;
    verify_map_edit(&reparsed, &map_before, edit, tile_set.as_ref())?;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    if let Err(error) = file.write_all(&encoded) {
        // A short write leaves a truncated map behind, which the documented "a refused edit leaves
        // no partial file" property does not allow. Drop the handle first so the removal is not
        // racing an open descriptor.
        drop(file);
        let _ = fs::remove_file(output);
        return Err(format!("could not write {}: {error}", output.display()));
    }

    let changed_cells = changed_cell_indexes(&map_before, &map).len();
    println!("wrote\t{}\t{} bytes", output.display(), encoded.len());
    println!("cells-changed\t{changed_cells}");
    println!("{note}");
    Ok(())
}

/// Read and parse a `.til`.
fn load_tile_set(path: &Path) -> Result<TileSetDefinition, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read tile definition {}: {error}", path.display()))?;
    TileSetDefinition::parse(&bytes)
        .map_err(|error| format!("could not parse tile definition {}: {error}", path.display()))
}

fn apply_map_edit(
    map: &mut MapAsset,
    edit: MapEdit,
    tile_set: Option<&TileSetDefinition>,
) -> Result<String, String> {
    match edit {
        MapEdit::SetTile { x, y, tile_index } => {
            map.set_tile(x, y, tile_index)
                .map_err(|error| error.to_string())?;
            Ok(format!("set-tile\t({x}, {y})\ttile:{tile_index}"))
        }
        MapEdit::SetTerrain {
            x,
            y,
            terrain_type,
        } => {
            map.set_terrain(x, y, terrain_type)
                .map_err(|error| error.to_string())?;
            let tile = terrain_type_base_tile(terrain_type).unwrap_or_default();
            eprintln!(
                "note: this writes one cell, like the editor's forcetexture. The engine's \
                 setterrain also blends transition tiles into the 8-neighbourhood, and which \
                 tiles it blends is unmeasured, so that is not reproduced here."
            );
            Ok(format!(
                "set-terrain\t({x}, {y})\tterrain:{terrain_type}\ttile:{tile}"
            ))
        }
        MapEdit::SetElevation { x, y, value } => {
            map.set_elevation(x, y, value)
                .map_err(|error| error.to_string())?;
            Ok(format!("set-elevation\t({x}, {y})\tvalue:{value}"))
        }
        MapEdit::FillTerrain { terrain_type } => {
            map.fill_terrain(terrain_type)
                .map_err(|error| error.to_string())?;
            let tile = terrain_type_base_tile(terrain_type).unwrap_or_default();
            Ok(format!(
                "fill-terrain\tterrain:{terrain_type}\ttile:{tile}\tcells:{}",
                map.cells.len()
            ))
        }
        MapEdit::PaintTerrain {
            rect,
            terrain_type,
            selector,
        } => {
            let tile_set = tile_set.ok_or_else(|| PaintRefusal::TileSetUnknown.to_string())?;
            let paint = map
                .paint_terrain(rect, terrain_type, tile_set, selector)
                .map_err(|error| error.to_string())?;
            let (x0, y0, x1, y1) = rect;
            // Which cells the engine would have drawn differently is the one thing the person
            // about to write this into a game directory cannot see in the output bytes, so say it
            // here rather than only in a document.
            let drawn = paint.plan.drawn_cells();
            if drawn > 0 {
                eprintln!(
                    "note: {drawn} of the {} written cells were newly painted with several equally \
                     valid tiles and none already in place. The engine draws among those at random \
                     -- the same paint run twice gave centre tiles 385 and 390 -- so they are a \
                     legal choice, not the engine's. Pass --seed N for a different legal draw.",
                    paint.plan.region.len() + paint.plan.ring.len()
                );
            }
            let edge = paint.plan.cells_touching_a_map_edge();
            if edge > 0 {
                eprintln!(
                    "note: {edge} written cells have a neighbour off the map. This writer treats \
                     an off-map neighbour as satisfying any constraint, which no saved artifact \
                     tests."
                );
            }
            Ok(format!(
                "paint-terrain\t({x0}, {y0})..({x1}, {y1})\tterrain:{terrain_type}\t{}\tcells-changed:{}",
                paint_summary(&paint.plan),
                paint.cells_changed,
            ))
        }
        MapEdit::PlaceSprite {
            x,
            y,
            sprite_type,
        } => {
            let instance_id = map
                .place_sprite(x, y, sprite_type)
                .map_err(|error| error.to_string())?;
            // A record minted into one of the four layouts the engine has never been watched
            // writing is a different kind of output from one minted into the 49-byte layout, and it
            // used to print identically. `--map-paint-terrain`, a few screens up, already emits a
            // note for exactly this class of uncertainty; this is the same class and a stronger
            // case, because a bad record here is a record the engine may reject outright.
            // A record minted into a layout no engine run has been watched writing is a different
            // kind of output from one minted into the 49-byte layout, and it used to print
            // identically. Five of the six layouts are in that position, including the 47-byte one
            // -- sharing the measured layout's tail shape is not the same as being it.
            if let Some((layout, provenance)) = map
                .resolved_tail_layout()
                .map(|layout| (layout, layout.mint_provenance()))
                .filter(|(_, provenance)| *provenance != MintProvenance::EngineObserved)
            {
                let basis = match provenance {
                    MintProvenance::InferredProcedureTail =>
                        "its attribute field at +24 is the 0x00000001 the engine was watched \
                         writing into a 49-byte record -- a measured value carried across layouts",
                    MintProvenance::InferredPlainTail | MintProvenance::EngineObserved =>
                        "every value including the attribute field at +24 is this layout's own \
                         corpus constant, and +24 is 0 because that is the only value all 4,003 \
                         records of the three plain-tail layouts hold",
                };
                eprintln!(
                    "note: this map uses the {layout} layout. The record just minted is Inferred: \
                     {basis}. No engine run has been observed writing, or accepting, a record in \
                     this layout; the one attended run wrote the 49-byte layout. Verify in the \
                     game before shipping a map edited this way."
                );
            }
            // Name the type in the output. A bare id is what made these records unreadable in
            // the first place, and a caller who passed an id deserves to see what it resolved to.
            // A name, or an honest account of why there isn't one. Saying "unregistered" for an
            // id inside the table's gaps told the user the opposite of the truth: those gaps are
            // the per-faith types the arrays hold, and they are the commonest objects on a real
            // map -- 31 distinct gap ids appear across the installed corpus.
            let named = terrain_sprite_name(sprite_type).map_or_else(
                || {
                    let highest = TERRAIN_SPRITE_TYPES
                        .iter()
                        .map(|(_, id)| *id)
                        .max()
                        .unwrap_or(0);
                    if sprite_type > highest {
                        "above the dumped table: a runtime registration".to_owned()
                    } else {
                        "inside a gap in the dumped table: probably a per-faith type held by one \
                         of the arrays"
                            .to_owned()
                    }
                },
                str::to_owned,
            );
            Ok(format!(
                "place-sprite\t({x}, {y})\ttype:{sprite_type}\tname:{named}\tinstance:{instance_id}"
            ))
        }
        MapEdit::RemoveSprite { instance_id } => {
            map.remove_sprite(instance_id)
                .map_err(|error| error.to_string())?;
            Ok(format!("remove-sprite\tinstance:{instance_id}"))
        }
        MapEdit::Rewrite => Ok("rewrite\tno edit applied".to_owned()),
        MapEdit::SetHighFlag { x, y, set } => {
            map.set_high_flag(x, y, set)
                .map_err(|error| error.to_string())?;
            eprintln!(
                "note: bit 0x00800000's meaning is Unknown. Across the 146 corpus files that carry \
                 it, the flagged cells are exactly the perimeter ring in 146 of 146 -- so an \
                 interior flag is a shape the engine has never been given."
            );
            Ok(format!("set-high-flag\t({x}, {y})\tset:{set}"))
        }
        MapEdit::FlagRegion { border, rect } => {
            let cells = flag_region_cells(map, border, rect)?;
            for (x, y) in &cells {
                map.set_high_flag(*x, *y, true)
                    .map_err(|error| error.to_string())?;
            }
            let what = if border { "border" } else { "interior" };
            Ok(format!("flag-region\t{what}\tcells:{}", cells.len()))
        }
    }
}

/// Read the edit back out of the bytes that are about to be written.
///
/// Applying an edit to an in-memory struct proves nothing about what lands on disk; only re-parsing
/// the encoded bytes does. This is the same guard `--set-imp-placement` uses.
/// The packed indexes whose eight bytes differ between two maps of the same shape.
fn changed_cell_indexes(before: &MapAsset, after: &MapAsset) -> Vec<usize> {
    before
        .cells
        .iter()
        .zip(&after.cells)
        .enumerate()
        .filter(|(_, (before, after))| !after.has_same_bytes(before))
        .map(|(index, _)| index)
        .collect()
}

/// Check that a single-cell edit touched **exactly one** cell, and the one that was asked for.
///
/// This is the answer to a real weakness the review found: reading the edit back through
/// `map.cell(x, y)` asks the same `cell_index` the setter used, so on the coordinate axis it is a
/// tautology -- flip the packing in both and the check still passes. The cell *diff* is a second
/// witness. It cannot make the packing formula independent of itself, and it is not claimed to:
/// what it adds is that an edit which strayed to another cell, or to several, is caught, and that
/// the byte that moved is the byte that was meant to. The packing itself is held by the byte-offset
/// test in `map.rs`, which computes the offset without going through `cell_index` at all.
fn verify_single_cell_edit(
    before: &MapAsset,
    after: &MapAsset,
    x: u32,
    y: u32,
) -> Result<(), String> {
    let expected = after
        .cell_index(x, y)
        .ok_or_else(|| format!("refusing to write: ({x}, {y}) is outside the map"))?;
    match changed_cell_indexes(before, after).as_slice() {
        [] => Ok(()),
        [only] if *only == expected => Ok(()),
        [only] => Err(format!(
            "refusing to write: the edit landed on cell {only}, not the cell {expected} at ({x}, {y})"
        )),
        several => Err(format!(
            "refusing to write: a single-cell edit changed {} cells: {:?}",
            several.len(),
            &several[..several.len().min(8)]
        )),
    }
}

/// The cells a `FlagRegion` edit targets.
///
/// Shared by the edit and its verification on purpose: two copies of this could disagree about
/// which cells were meant, and then the check would be confirming the wrong thing.
fn flag_region_cells(
    map: &MapAsset,
    border: bool,
    rect: Option<(u32, u32, u32, u32)>,
) -> Result<Vec<(u32, u32)>, String> {
    if border {
        return Ok((0..map.height)
            .flat_map(|y| (0..map.width).map(move |x| (x, y)))
            .filter(|(x, y)| *x == 0 || *y == 0 || *x == map.width - 1 || *y == map.height - 1)
            .collect());
    }
    let (x0, y0, x1, y1) = rect.ok_or("a rectangle is required")?;
    if x0 > x1 || y0 > y1 {
        return Err(format!("({x0}, {y0})..({x1}, {y1}) is not a rectangle"));
    }
    if x1 >= map.width || y1 >= map.height {
        return Err(format!(
            "({x1}, {y1}) is outside this {}x{} map",
            map.width, map.height
        ));
    }
    Ok((y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .collect())
}

/// How a paint's ring turned out, for the one line the CLI prints.
///
/// The three no-ring outcomes are named separately rather than reported as "0 cells", because
/// "the engine blends nothing onto dirt" and "your rectangle was already that terrain" are
/// different facts and a caller cannot tell them apart from a count.
/// The part of a paint's result line that describes what the tileset decided.
fn paint_summary(plan: &TerrainPaintPlan) -> String {
    // `reproducible` is Unique plus Kept. Reporting only Unique as "determined" understated the
    // tool badly: a whole-map paint of one terrain is every cell Kept, which is exactly what the
    // engine does, and it used to print `determined:0`.
    let unique = plan
        .cells()
        .filter(|cell| matches!(cell.choice, TileChoice::Unique(_)))
        .count();
    let kept = plan
        .cells()
        .filter(|cell| matches!(cell.choice, TileChoice::Kept { .. }))
        .count();
    let tiles: BTreeSet<u32> = plan.region.iter().map(|cell| cell.tile_index).collect();
    format!(
        "region-cells:{}\tregion-tiles:{}\tring-cells:{}\tdetermined:{unique}\tkept:{kept}\tdrawn:{}",
        plan.region.len(),
        tiles.iter().map(u32::to_string).collect::<Vec<_>>().join(","),
        plan.ring.len(),
        plan.drawn_cells(),
    )
}

fn verify_map_edit(
    map: &MapAsset,
    before: &MapAsset,
    edit: MapEdit,
    tile_set: Option<&TileSetDefinition>,
) -> Result<(), String> {
    let cell_tile = |x: u32, y: u32| -> Result<u32, String> {
        map.cell(x, y)
            .map(MapCellTile::tile)
            .ok_or_else(|| format!("refusing to write: ({x}, {y}) is missing after the edit"))
    };
    match edit {
        MapEdit::SetTile { x, y, tile_index } => {
            verify_single_cell_edit(before, map, x, y)?;
            let observed = cell_tile(x, y)?;
            if observed != tile_index {
                return Err(format!(
                    "refusing to write: expected tile {tile_index} at ({x}, {y}) but read {observed}"
                ));
            }
        }
        MapEdit::SetTerrain {
            x,
            y,
            terrain_type,
        } => {
            let expected = terrain_type_base_tile(terrain_type)
                .ok_or_else(|| format!("{terrain_type} is not a terrain type"))?;
            verify_single_cell_edit(before, map, x, y)?;
            let observed = cell_tile(x, y)?;
            if observed != expected {
                return Err(format!(
                    "refusing to write: expected tile {expected} at ({x}, {y}) but read {observed}"
                ));
            }
        }
        MapEdit::SetElevation { x, y, value } => {
            verify_single_cell_edit(before, map, x, y)?;
            let observed = map
                .cell(x, y)
                .ok_or_else(|| format!("refusing to write: ({x}, {y}) is missing after the edit"))?
                .value_bits;
            if observed != value.to_bits() {
                return Err(format!(
                    "refusing to write: elevation at ({x}, {y}) read back as {}",
                    f32::from_bits(observed)
                ));
            }
        }
        MapEdit::FillTerrain { terrain_type } => {
            let expected = terrain_type_base_tile(terrain_type)
                .ok_or_else(|| format!("{terrain_type} is not a terrain type"))?;
            if let Some(index) = map
                .cells
                .iter()
                .position(|cell| cell.tile_index() != expected)
            {
                return Err(format!(
                    "refusing to write: cell {index} did not take the fill tile {expected}"
                ));
            }
        }
        MapEdit::PaintTerrain {
            rect,
            terrain_type,
            ..
        } => {
            let tile_set = tile_set.ok_or_else(|| {
                format!("refusing to write: {}", PaintRefusal::TileSetUnknown)
            })?;
            // **Nothing here calls the planner.** An earlier version of this arm re-ran
            // `plan_terrain_paint` and compared the written map to its output, which is the same
            // decision procedure the applier used: no planner defect could ever show up, because a
            // wrong plan was compared against itself. The two loops that followed were worse than
            // idle -- `tiles.get(&tile_index)` and `definition.terrain_type != cell.terrain_type`
            // are both *guaranteed* by `candidates()`, which filters on `terrain_type` and yields
            // keys of `tiles`, so they evaluated no neighbour constraint at all while the comment
            // claimed they proved the plan legal by the tileset's own rules.
            //
            // This is the third verifier in this repository that could not fail. What follows is
            // derived from the before/after maps and the tileset independently, and each check has
            // a named input that breaks it:
            //
            // - the affected set: a paint that touches a cell it had no business touching;
            // - the region's terrain: a tile of the wrong terrain written inside the rectangle;
            // - **constraint satisfaction**: a tile of the *right* terrain whose own declared
            //   constraints are violated by the neighbours actually written -- for example tile 15,
            //   the plains shore tile, dropped into the middle of a plains field. Every earlier
            //   version of this arm accepted that.
            let (x0, y0, x1, y1) = rect;
            if x0 > x1 || y0 > y1 || x1 >= before.width || y1 >= before.height {
                return Err(format!(
                    "refusing to write: ({x0}, {y0})..({x1}, {y1}) is not a rectangle inside this \
                     {}x{} map",
                    before.width, before.height
                ));
            }
            let terrain_at = |source: &MapAsset, x: u32, y: u32| -> Result<u32, String> {
                let tile = source
                    .cell(x, y)
                    .map(|cell| cell.tile_index())
                    .ok_or_else(|| format!("refusing to write: ({x}, {y}) is outside the map"))?;
                tile_set.terrain_type_of_tile(tile).ok_or_else(|| {
                    format!(
                        "refusing to write: tile {tile} at ({x}, {y}) is not declared by the \
                         supplied tileset"
                    )
                })
            };

            // Which region cells changed terrain, read from `before` alone.
            let mut moved: BTreeSet<(u32, u32)> = BTreeSet::new();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if terrain_at(before, x, y)? != terrain_type {
                        moved.insert((x, y));
                    }
                }
            }
            // The cells this paint is allowed to have written: the rectangle, plus any cell outside
            // it with a neighbour whose terrain moved.
            let mut allowed: BTreeSet<(u32, u32)> = BTreeSet::new();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    allowed.insert((x, y));
                }
            }
            for &(cx, cy) in &moved {
                for dy in -1_i64..=1 {
                    for dx in -1_i64..=1 {
                        let (Ok(x), Ok(y)) = (
                            u32::try_from(i64::from(cx) + dx),
                            u32::try_from(i64::from(cy) + dy),
                        ) else {
                            continue;
                        };
                        if x < map.width && y < map.height {
                            allowed.insert((x, y));
                        }
                    }
                }
            }
            let allowed_indexes: BTreeSet<usize> = allowed
                .iter()
                .filter_map(|(x, y)| map.cell_index(*x, *y))
                .collect();
            let changed: BTreeSet<usize> =
                changed_cell_indexes(before, map).into_iter().collect();
            if let Some(stray) = changed.difference(&allowed_indexes).next() {
                return Err(format!(
                    "refusing to write: cell {stray} changed but is neither in the painted \
                     rectangle nor beside a cell whose terrain moved"
                ));
            }

            // Every cell inside the rectangle now holds a tile of the painted terrain.
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let observed = terrain_at(map, x, y)?;
                    if observed != terrain_type {
                        return Err(format!(
                            "refusing to write: ({x}, {y}) is inside the painted rectangle but \
                             holds terrain {observed}, not {terrain_type}"
                        ));
                    }
                }
            }

            // And every written tile satisfies its own declared constraints against the
            // neighbourhood *as actually encoded*. Each of the eight columns is evaluated by name
            // through `Direction::offset`, not through `Neighbourhood`/`TileDefinition::accepts`,
            // so this is a second reading of the tileset rather than a second call to the first.
            for &(x, y) in &allowed {
                let Some(index) = map.cell_index(x, y) else {
                    continue;
                };
                if !changed.contains(&index) {
                    continue;
                }
                let tile = map
                    .cell(x, y)
                    .map(|cell| cell.tile_index())
                    .ok_or_else(|| format!("refusing to write: ({x}, {y}) is outside the map"))?;
                let Some(definition) = tile_set.tiles.get(&tile) else {
                    return Err(format!(
                        "refusing to write: tile {tile} at ({x}, {y}) is not declared by the \
                         supplied tileset"
                    ));
                };
                if !definition.constraints_are_complete() {
                    return Err(format!(
                        "refusing to write: tile {tile} at ({x}, {y}) does not declare all eight \
                         neighbour constraints, so it cannot be painted"
                    ));
                }
                let own = definition.terrain_type;
                for direction in Direction::ALL {
                    let (dx, dy) = direction.offset();
                    let (Ok(nx), Ok(ny)) = (
                        u32::try_from(i64::from(x) + i64::from(dx)),
                        u32::try_from(i64::from(y) + i64::from(dy)),
                    ) else {
                        continue;
                    };
                    // Off the map on the high side too, and an off-map neighbour is read as this
                    // cell's own terrain -- the closed reading the selector chose from. Verifying
                    // against the open reading would accept the phantom coastline that reading
                    // produces.
                    let neighbour = if nx >= map.width || ny >= map.height {
                        own
                    } else {
                        terrain_at(map, nx, ny)?
                    };
                    if !definition.neighbour(direction).accepts(Some(neighbour)) {
                        return Err(format!(
                            "refusing to write: tile {tile} at ({x}, {y}) requires {} {:?} of \
                             itself, but the written map has terrain {neighbour} there",
                            direction.column_name(),
                            definition.neighbour(direction)
                        ));
                    }
                }
            }
        }
        MapEdit::PlaceSprite { x, y, .. } => {
            let cell_index = map
                .cell_index(x, y)
                .ok_or_else(|| format!("refusing to write: ({x}, {y}) is outside the map"))?;
            let present = map.placed_sprites.as_ref().is_some_and(|section| {
                section
                    .records
                    .iter()
                    .any(|record| usize::try_from(record.cell_index) == Ok(cell_index))
            });
            if !present {
                return Err(format!(
                    "refusing to write: no placed sprite at ({x}, {y}) after the edit"
                ));
            }
        }
        MapEdit::RemoveSprite { instance_id } => {
            let present = map.placed_sprites.as_ref().is_some_and(|section| {
                section
                    .records
                    .iter()
                    .any(|record| record.instance_id == instance_id)
            });
            if present {
                return Err(format!(
                    "refusing to write: instance {instance_id} is still present after removal"
                ));
            }
        }
        MapEdit::Rewrite => {
            if !changed_cell_indexes(before, map).is_empty() {
                return Err("refusing to write: a rewrite changed cells".to_owned());
            }
        }
        MapEdit::SetHighFlag { x, y, set } => {
            verify_single_cell_edit(before, map, x, y)?;
            let observed = map
                .cell(x, y)
                .ok_or_else(|| format!("refusing to write: ({x}, {y}) is missing after the edit"))?
                .high_flag_set();
            if observed != set {
                return Err(format!(
                    "refusing to write: the flag at ({x}, {y}) read back as {observed}"
                ));
            }
        }
        MapEdit::FlagRegion { border, rect } => {
            // This arm used to be empty, with a comment claiming the edit verified itself. It did
            // not: `apply_map_edit` only propagated out-of-range errors, so a mask bug in
            // `set_high_flag` that clobbered the tile field would have been encoded, reparsed and
            // written into a directory with no backup, while every other verb refused. It is the
            // only verb that writes many cells at once, which makes it the worst one to leave
            // unchecked.
            let region = flag_region_cells(map, border, rect)?;
            let wanted: std::collections::BTreeSet<usize> = region
                .iter()
                .filter_map(|(x, y)| map.cell_index(*x, *y))
                .collect();
            for (x, y) in &region {
                let cell = map.cell(*x, *y).ok_or_else(|| {
                    format!("refusing to write: ({x}, {y}) is missing after the edit")
                })?;
                if !cell.high_flag_set() {
                    return Err(format!(
                        "refusing to write: the flag at ({x}, {y}) did not take"
                    ));
                }
            }
            // And nothing outside the region moved. The tile field lives in the same word as the
            // flag, so a bad mask shows up here as a changed cell that was never targeted.
            let changed: std::collections::BTreeSet<usize> =
                changed_cell_indexes(before, map).into_iter().collect();
            if let Some(stray) = changed.difference(&wanted).next() {
                return Err(format!(
                    "refusing to write: cell {stray} changed but is outside the flagged region"
                ));
            }
        }
    }
    Ok(())
}

/// A tiny shim so `verify_map_edit` can read a cell's tile through `Option::map`.
trait MapCellTile {
    fn tile(&self) -> u32;
}

impl MapCellTile for lom_asset_viewer::map::MapCell {
    fn tile(&self) -> u32 {
        self.tile_index()
    }
}


/// Where the committed gameplay index lives when `--reports` is not given.
///
/// The binary is normally run from `spikes/asset-viewer`, so the default is the repository's
/// `reports/gameplay`. A checkout with no game installed can still answer every query from it,
/// which is the point of making the index the query path's only input.
fn default_gameplay_reports() -> PathBuf {
    PathBuf::from("../../reports/gameplay")
}

fn read_gameplay_index(reports: &Path) -> Result<Vec<gameplay_symbols::IndexRow>, String> {
    let path = reports.join("symbols.tsv");
    let text = fs::read_to_string(&path).map_err(|error| {
        format!(
            "could not read {}: {error}. Pass --reports DIR, or regenerate with \
             `cargo run --release --example gameplay_symbols`.",
            path.display()
        )
    })?;
    gameplay_symbols::parse_index(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Print everything the database records about one symbol.
///
/// The query is matched against the symbol's code first and, only if nothing matches there, against
/// its display name -- so `aicav` and `Windriders` both reach the Windriders unit. A name may
/// legitimately answer more than once (`potion_health` is registered as both an artifact and a
/// spell), and a display name may be shared outright, so an ambiguous query lists its candidates
/// rather than picking one.
fn gameplay_symbol(name: &str, reports: &Path) -> Result<(), String> {
    let rows = read_gameplay_index(reports)?;
    let (matches, matched_on) = gameplay_symbols::lookup_rows(name, &rows);
    if matches.is_empty() {
        // A miss states the size of what was searched, so "no such symbol" is a statement about a
        // known index rather than an unbounded claim about the game.
        println!(
            "no gameplay symbol whose code or display name is {name}, in {} indexed symbols",
            rows.len()
        );
        let near: Vec<String> = rows
            .iter()
            .filter(|row| {
                let needle = name.to_ascii_lowercase();
                row.name.to_ascii_lowercase().contains(&needle)
                    || gameplay_symbols::row_display_name(row)
                        .is_some_and(|display| display.to_ascii_lowercase().contains(&needle))
            })
            .map(|row| match gameplay_symbols::row_display_name(row) {
                Some(display) => format!("{} ({display})", row.name),
                None => row.name.clone(),
            })
            .take(10)
            .collect();
        if !near.is_empty() {
            println!("containing that text: {}", near.join(", "));
        }
        return Ok(());
    }
    // Several rows sharing a *code* are all the answer -- `potion_health` really is both an
    // artifact and a spell -- so those are printed in full. Several rows sharing a *display name*
    // are a question, not an answer: sixteen spells are labelled "Dispel Magic" and only one is
    // meant. Those are listed as candidates and nothing is printed in full, because printing
    // sixteen records would bury the fact that the query did not identify one.
    if matches.len() > 1 && matched_on == gameplay_symbols::MatchedField::DisplayName {
        println!(
            "{name} is a display name shared by {} symbols. Re-run with one of these codes:",
            matches.len()
        );
        for row in &matches {
            println!("candidate\t{}\t{}\t{}", row.name, row.kind, row.member);
        }
        return Ok(());
    }
    if matches.len() > 1 {
        println!(
            "{name} names {} symbols, of different kinds. All are shown.\n",
            matches.len()
        );
    }
    for row in matches {
        println!("name\t{}", row.name);
        println!("kind\t{}", row.kind);
        println!("evidence\t{}", row.evidence);
        println!("profiles\t{}", row.profiles.join(","));
        println!("display-name\t{}", row.display_name);
        println!("display-source\t{}", row.display_source);
        println!("defined-in\t{}", row.member);
        println!("line\t{}", row.line);
        println!("byte-offset\t{}", row.byte_offset);
        println!("registered-in\t{}", row.registered_in);
        println!("fields\t{}", row.fields);
        println!("static-references\t{}", row.references);
        println!(
            "reference-anchor\treports/gameplay/reference.md#{}",
            row.anchor
        );
        println!();
    }
    Ok(())
}

/// List the symbols whose code **or display name** matches a glob.
///
/// Searching the code alone made the reference unusable by anyone who did not already know the
/// code: `Windrider*` returned nothing while `aicav` returned the unit called "Windriders". The
/// `matched` column says which field produced each hit, because a result nobody can explain is a
/// result nobody can trust.
fn gameplay_symbols_like(pattern: &str, reports: &Path) -> Result<(), String> {
    let rows = read_gameplay_index(reports)?;
    let mut shown = 0_usize;
    println!("name\tdisplay-name\tmatched\tkind\tevidence\tprofiles\tmember");
    for row in &rows {
        let Some(matched) = gameplay_symbols::match_row(pattern, row) else {
            continue;
        };
        shown += 1;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.name,
            gameplay_symbols::row_display_name(row).unwrap_or("-"),
            matched.label(),
            row.kind,
            row.evidence,
            row.profiles.join(","),
            row.member
        );
    }
    // How many rows could not have matched by display name at all, so a thin result is explainable
    // rather than mysterious.
    let without = rows
        .iter()
        .filter(|row| gameplay_symbols::row_display_name(row).is_none())
        .count();
    println!("\nmatched\t{shown}\tof\t{}", rows.len());
    println!("rows-with-no-display-name-to-match\t{without}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        GENERATED_HEADER_WORD, GameScriptValue, LayoutObservability, MapEdit, PackedSizeChoice,
        TRANSITION_RING_OFFSETS,
        create_map, edit_map, import_png_imp, import_png_pbm, parse_coordinate, parse_dimension,
        parse_elevation, parse_native_stub, parse_offset, parse_sprite_type, parse_terrain_type,
        refusal_class, roundtrip_maps, set_imp_placement, sprite_types, terrain_sprite_name,
        transition_rings,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    use lom_asset_viewer::imp::{
        ImpDisagreement, ImpExceptionClass, ImpFacing, ImpFrame, ImpOrphanFacts, ImpOrphanNote,
        ImpSequence, ImpSprite, ImpStatistic, ImpValidationException, PackedSizes,
        write_frame_pixels,
    };
    use lom_asset_viewer::map::{MapAsset, MapCell};
    use lom_asset_viewer::pbm::PbmFile;
    use lom_asset_viewer::pbm::PbmImage;
    use lom_asset_viewer::png_export::{write_imp_frame_png, write_pbm_png};
    use lom_asset_viewer::tile::{TileDefinition, TileSelector, TileSetDefinition};

    use super::{
        ImpCatalog, ImpDisplayMode, ImpValidationReport, MapDisplayMode,
        TERRAIN_PREVIEW_TILE_SIZE, imp_display_rgba, map_display_rgba, step_imp_facing,
        step_imp_frame, step_imp_sequence, terrain_preview_rgba, validate_imp_members,
    };

    #[test]
    fn imp_display_modes_preserve_decoder_pixels() {
        let indices = [0, 1, 42, 0];
        let source = [0, 255, 0, 255, 255, 0, 0, 255, 1, 2, 3, 255, 0, 255, 0, 128];

        assert_eq!(
            imp_display_rgba(&indices, &source, ImpDisplayMode::Preview, 0),
            [0, 255, 0, 0, 255, 0, 0, 0, 1, 2, 3, 255, 0, 255, 0, 0,]
        );
        assert_eq!(
            imp_display_rgba(&indices, &source, ImpDisplayMode::Mask, 0),
            [0, 255, 0, 0, 255, 0, 0, 255, 1, 2, 3, 255, 0, 255, 0, 0,]
        );
        assert_eq!(
            imp_display_rgba(&indices, &source, ImpDisplayMode::Raw, 0),
            source
        );
    }

    #[test]
    fn imp_navigation_respects_facing_and_sequence_boundaries() {
        let sprite = navigation_sprite();

        assert_eq!(step_imp_frame(&sprite, 1, 1).unwrap(), 0);
        assert_eq!(step_imp_frame(&sprite, 0, -1).unwrap(), 1);
        assert_eq!(step_imp_facing(&sprite, 0, 1).unwrap(), 2);
        assert_eq!(step_imp_facing(&sprite, 2, -1).unwrap(), 0);
        assert_eq!(step_imp_sequence(&sprite, 2, 1).unwrap(), 4);
        assert_eq!(step_imp_sequence(&sprite, 4, -1).unwrap(), 0);
    }

    /// Corrected 2026-09-17: cells are packed `y * width + x`, so consecutive cells are one
    /// display *row*, not one column. The fixture is 3x2 because the old 2x2 one agreed with both
    /// encodings -- it was fitted to square data and could not fail.
    #[test]
    fn map_display_modes_lay_packed_cells_out_in_rows() {
        let map = MapAsset {
            metadata: 1,
            width: 3,
            height: 2,
            bits_per_pixel: 8,
            cells: vec![
                MapCell {
                    tag: 4,
                    value_bits: 0.0_f32.to_bits(),
                    value: 0.0,
                },
                MapCell {
                    tag: 5,
                    value_bits: 10.0_f32.to_bits(),
                    value: 10.0,
                },
                MapCell {
                    tag: 6,
                    value_bits: 20.0_f32.to_bits(),
                    value: 20.0,
                },
                MapCell {
                    tag: 7,
                    value_bits: 30.0_f32.to_bits(),
                    value: 30.0,
                },
                MapCell {
                    tag: 8,
                    value_bits: 40.0_f32.to_bits(),
                    value: 40.0,
                },
                MapCell {
                    tag: 9,
                    value_bits: 50.0_f32.to_bits(),
                    value: 50.0,
                },
            ],
            placed_sprites: None,
            trailing_raw: Vec::new(),
        };

        let tags = map_display_rgba(&map, MapDisplayMode::CellTags);
        let elevation = map_display_rgba(&map, MapDisplayMode::CandidateElevation);

        assert_eq!(tags.len(), 24);
        assert_ne!(&tags[0..3], &tags[4..7]);
        // Row y=0 is cells 0..3 and row y=1 is cells 3..6; x-major would interleave them.
        assert_eq!(&elevation[0..4], &[0, 0, 0, 255]);
        assert_eq!(&elevation[4..8], &[51, 51, 51, 255]);
        assert_eq!(&elevation[8..12], &[102, 102, 102, 255]);
        assert_eq!(&elevation[12..16], &[153, 153, 153, 255]);
        assert_eq!(&elevation[16..20], &[204, 204, 204, 255]);
        assert_eq!(&elevation[20..24], &[255, 255, 255, 255]);
    }

    #[test]
    fn terrain_preview_resolves_map_tile_indexes_through_the_atlas() {
        let map = MapAsset {
            metadata: 0,
            width: 2,
            height: 3,
            bits_per_pixel: 8,
            // Packed `y * width + x`, and non-square with alternating tiles so that a row read as
            // a column resolves different atlas pixels. A 2x1 fixture cannot tell them apart.
            cells: (0..6)
                .map(|index| MapCell {
                    tag: index % 2,
                    value_bits: 0,
                    value: 0.0,
                })
                .collect(),
            placed_sprites: None,
            trailing_raw: Vec::new(),
        };
        let tile_set = TileSetDefinition {
            atlas_member: "test.lbm".to_owned(),
            columns: 2,
            rows: 1,
            tile_width: 1,
            tile_height: 1,
            terrain_types: BTreeMap::new(),
            tiles: BTreeMap::from([
                (0, TileDefinition::unconstrained(0, 0)),
                (1, TileDefinition::unconstrained(1, 0)),
            ]),
        };
        let atlas = PbmImage {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 255, 200, 210, 220, 255],
            indices: vec![0, 1],
            palette: Vec::new(),
            palette_entries: 0,
            compression: 0,
            masking: 0,
            transparent_color: 0,
        };

        let preview = terrain_preview_rgba(&map, &tile_set, &atlas).unwrap();
        let preview_row = 2 * TERRAIN_PREVIEW_TILE_SIZE as usize;
        let at = |x: usize, y: usize| {
            let pixel = (y * TERRAIN_PREVIEW_TILE_SIZE as usize) * preview_row
                + x * TERRAIN_PREVIEW_TILE_SIZE as usize;
            &preview[pixel * 4..pixel * 4 + 4]
        };

        assert_eq!(
            preview.len(),
            preview_row * 3 * TERRAIN_PREVIEW_TILE_SIZE as usize * 4
        );
        assert_eq!(at(0, 0), &[10, 20, 30, 255]);
        assert_eq!(at(1, 0), &[200, 210, 220, 255]);
        // Cells 2 and 3 are the second row. Read X-major they would be (0,1) and (1,1) swapped.
        assert_eq!(at(0, 1), &[10, 20, 30, 255]);
        assert_eq!(at(1, 1), &[200, 210, 220, 255]);
        assert_eq!(at(0, 2), &[10, 20, 30, 255]);
        assert_eq!(at(1, 2), &[200, 210, 220, 255]);
    }

    // --- corpus validation: pairing and verdicts, with no archive -------------------------

    /// A generated `.h` whose statistics match [`minimal_imp`] unless a caller perturbs them.
    fn generated_header(sequence: &str, frames: usize, raw: u64, stored: u64) -> Vec<u8> {
        format!(
            "// Sprite headers for sequence {sequence}\r\n\
             // Total number of 'Sequences': 1\r\n\
             // Total number of 'Frames': {frames}\r\n\
             // Duplicate bitmaps found : 0\r\n\
             // Bitmap raw memory usage : {raw}\r\n\
             // Hotspot raw memory usage : 0\r\n\
             // Bitmap RLE memory usage : {stored}\r\n"
        )
        .into_bytes()
    }

    fn matching_header(sequence: &str) -> Vec<u8> {
        generated_header(sequence, 1, 2, 2)
    }

    /// A stand-in archive: named members, plus names whose read fails.
    struct FakeArchive {
        members: BTreeMap<String, Vec<u8>>,
        unreadable: BTreeSet<String>,
    }

    impl FakeArchive {
        fn new(members: &[(&str, Vec<u8>)]) -> Self {
            Self {
                members: members
                    .iter()
                    .map(|(name, bytes)| ((*name).to_owned(), bytes.clone()))
                    .collect(),
                unreadable: BTreeSet::new(),
            }
        }

        fn unreadable(mut self, name: &str) -> Self {
            self.members.entry(name.to_owned()).or_default();
            self.unreadable.insert(name.to_owned());
            self
        }

        fn names(&self) -> Vec<String> {
            self.members.keys().cloned().collect()
        }

        fn run(&self, catalog: ImpCatalog<'_>) -> ImpValidationReport {
            validate_imp_members(
                &self.names(),
                &|name| {
                    if self.unreadable.contains(name) {
                        return Err(format!("could not read {name}"));
                    }
                    self.members
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("archive has no member named {name}"))
                },
                catalog,
            )
        }
    }

    const EMPTY_CATALOG: ImpCatalog<'static> = ImpCatalog {
        exceptions: &[],
        orphans: &[],
    };

    /// A waiver pinned to the fixture's numbers. The shipped table is pinned to values only the
    /// real archive produces, so the verdict logic can only be tested against a table of its own.
    const FIXTURE_EXCEPTIONS: &[ImpValidationException] = &[ImpValidationException {
        member: "units/imp/stale",
        class: ImpExceptionClass::HeaderPredatesArtRevision,
        reason: "fixture waiver: the header declares 99 pixel bytes where the file stores 2",
        waived: &[
            ImpDisagreement {
                statistic: ImpStatistic::RawPixelBytes,
                binary: 2,
                header: 99,
            },
            ImpDisagreement {
                statistic: ImpStatistic::StoredPixelBytes,
                binary: 2,
                header: 99,
            },
        ],
    }];

    const FIXTURE_ORPHANS: &[ImpOrphanNote] = &[ImpOrphanNote {
        member: "imp/lonely.imp",
        reason: "fixture note: art with no header of its own, pinned to what it measures",
        facts: ImpOrphanFacts::Sprite {
            sequence_count: 1,
            frame_count: 1,
            duplicate_frame_count: 0,
            raw_pixel_bytes: 2,
            hotspot_bytes: 0,
            stored_pixel_bytes: 2,
        },
    }];

    #[test]
    fn a_stem_pair_that_agrees_validates() {
        let archive = FakeArchive::new(&[
            ("units\\imp\\a.imp", minimal_imp()),
            ("units\\imp\\a.h", matching_header("a")),
        ]);
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.failures, Vec::<String>::new());
        assert_eq!(report.matched_pairs, 1);
        assert_eq!(report.paired_by_stem, 1);
        assert_eq!(report.validated, 1);
        assert_eq!(report.dedup_compared_stem_pairs, 1);
        assert_eq!(report.dedup_at_least_header, 1);
    }

    #[test]
    fn a_uniquely_declared_sequence_name_pairs_a_member_with_no_stem_counterpart() {
        let archive = FakeArchive::new(&[
            ("units\\imp\\b.imp", minimal_imp()),
            ("units\\imp\\other.h", matching_header("b")),
        ]);
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.failures, Vec::<String>::new());
        assert_eq!(report.paired_by_declared_name, 2);
        assert_eq!(report.validated, 2);
        assert_eq!(report.ambiguous_pairings, 0);
        // A foreign header is not evidence about a build tool's own header, so it stays out of
        // the duplicate-tally counters entirely.
        assert_eq!(report.dedup_compared_stem_pairs, 0);
        assert_eq!(report.dedup_foreign_header_pairs, 2);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("partner_is_otherwise_unpaired")),
            "{:?}",
            report.notes
        );
    }

    /// 155 sequence names in the shipped archive are declared by more than one header, `deaura`
    /// by 32. Taking the first was arbitrary wherever it mattered.
    #[test]
    fn a_sequence_name_several_headers_declare_is_reported_not_guessed() {
        let archive = FakeArchive::new(&[
            ("far\\b.imp", minimal_imp()),
            ("one\\p.h", matching_header("b")),
            ("two\\q.h", matching_header("b")),
        ]);
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.ambiguous_pairings, 1);
        // Each header still finds the one sprite answering to `b`; it is the sprite that cannot
        // say which of the two headers describes it.
        assert_eq!(report.paired_by_declared_name, 2);
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("declared by 2 headers")
                    && failure.contains("refusing to guess")),
            "{:?}",
            report.failures
        );
    }

    #[test]
    fn a_candidate_in_the_members_own_directory_settles_an_otherwise_ambiguous_name() {
        let archive = FakeArchive::new(&[
            ("near\\b.imp", minimal_imp()),
            ("near\\p.h", matching_header("b")),
            ("far\\q.h", matching_header("b")),
        ]);
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.ambiguous_pairings, 0);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("near\\p.h") && note.contains("near\\b.imp")),
            "{:?}",
            report.notes
        );
    }

    /// Both fallback pairs in the shipped archive take a partner that is already stem-paired, so
    /// the pairing is a second opinion about that sprite rather than a new pair. Say so.
    #[test]
    fn a_fallback_pairing_says_when_its_partner_is_already_stem_paired() {
        let archive = FakeArchive::new(&[
            ("units\\imp\\c.imp", minimal_imp()),
            ("units\\imp\\c.h", matching_header("c")),
            ("units\\imp\\alias.h", matching_header("c")),
        ]);
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.paired_by_stem, 1);
        assert_eq!(report.paired_by_declared_name, 1);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("reuses_a_stem_paired_member")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn a_catalogued_orphan_is_read_and_re_measured() {
        let archive = FakeArchive::new(&[("imp\\lonely.imp", minimal_imp())]);
        let catalog = ImpCatalog {
            exceptions: &[],
            orphans: FIXTURE_ORPHANS,
        };
        let report = archive.run(catalog);

        assert_eq!(report.failures, Vec::<String>::new());
        assert_eq!(report.documented_orphans, 1);
    }

    /// The validator used to accept an orphan by name without ever reading it, so a truncated
    /// replacement at the catalogued name exited 0.
    #[test]
    fn a_malformed_member_at_a_catalogued_orphan_name_still_fails() {
        let mut truncated = minimal_imp();
        truncated.truncate(16);
        let archive = FakeArchive::new(&[("imp\\lonely.imp", truncated)]);
        let catalog = ImpCatalog {
            exceptions: &[],
            orphans: FIXTURE_ORPHANS,
        };
        let report = archive.run(catalog);

        assert_eq!(report.documented_orphans, 0);
        assert_eq!(report.validation_failures, 1);
        assert!(
            report.failures[0].contains("truncated") || report.failures[0].contains("catalog note"),
            "{:?}",
            report.failures
        );
    }

    /// A member that parses but measures something else must re-fail: the note asserts values,
    /// not just a name.
    #[test]
    fn a_different_but_parseable_member_at_an_orphan_name_re_fails() {
        let mut other = minimal_imp();
        // Widen the frame from 2x1 to 1x1 so it measures one raw byte instead of two.
        other[56 + 2..56 + 4].copy_from_slice(&1_u16.to_le_bytes());
        other[56 + 6..56 + 8].copy_from_slice(&1_u16.to_le_bytes());
        let archive = FakeArchive::new(&[("imp\\lonely.imp", other)]);
        let catalog = ImpCatalog {
            exceptions: &[],
            orphans: FIXTURE_ORPHANS,
        };
        let report = archive.run(catalog);

        assert_eq!(report.documented_orphans, 0);
        assert_eq!(report.validation_failures, 1);
        assert!(
            report.failures[0].contains("does not match its catalog note"),
            "{:?}",
            report.failures
        );
    }

    #[test]
    fn an_uncatalogued_orphan_is_a_failure() {
        let archive = FakeArchive::new(&[("imp\\nobody.imp", minimal_imp())]);
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.orphan_entries, 1);
        assert!(
            report.failures[0].contains("no .h counterpart and no catalog note"),
            "{:?}",
            report.failures
        );
    }

    #[test]
    fn an_exception_that_matches_exactly_is_waived_and_reported() {
        let archive = FakeArchive::new(&[
            ("units\\imp\\stale.imp", minimal_imp()),
            ("units\\imp\\stale.h", generated_header("stale", 1, 99, 99)),
        ]);
        let catalog = ImpCatalog {
            exceptions: FIXTURE_EXCEPTIONS,
            orphans: &[],
        };
        let report = archive.run(catalog);

        assert_eq!(report.failures, Vec::<String>::new());
        assert_eq!(report.excepted, 1);
        assert_eq!(report.validated, 0);
        assert!(
            report.notes.iter().any(|note| note
                .starts_with("exception\tunits/imp/stale\tHeaderPredatesArtRevision")),
            "{:?}",
            report.notes
        );
    }

    /// The waiver is of specific numbers. A file that disagrees by a different amount is a new
    /// finding, not a known one.
    #[test]
    fn an_exception_does_not_cover_a_different_measurement() {
        let archive = FakeArchive::new(&[
            ("units\\imp\\stale.imp", minimal_imp()),
            ("units\\imp\\stale.h", generated_header("stale", 1, 98, 99)),
        ]);
        let catalog = ImpCatalog {
            exceptions: FIXTURE_EXCEPTIONS,
            orphans: &[],
        };
        let report = archive.run(catalog);

        assert_eq!(report.excepted, 0);
        assert_eq!(report.validation_failures, 1);
        assert!(
            report.failures[0].contains("raw pixel bytes mismatch"),
            "{:?}",
            report.failures
        );
    }

    #[test]
    fn an_unexplained_disagreement_is_a_failure_that_names_every_statistic() {
        let archive = FakeArchive::new(&[
            ("units\\imp\\d.imp", minimal_imp()),
            ("units\\imp\\d.h", generated_header("d", 4, 99, 99)),
        ]);
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.validation_failures, 1);
        let failure = &report.failures[0];
        assert!(failure.contains("frame count mismatch"), "{failure}");
        assert!(failure.contains("raw pixel bytes mismatch"), "{failure}");
        assert!(failure.contains("stored pixel bytes mismatch"), "{failure}");
    }

    /// One unreadable member used to abort the run with `?`, printing nothing at all about the
    /// other 3,599.
    #[test]
    fn an_unreadable_member_is_a_failure_line_and_the_run_continues() {
        let archive = FakeArchive::new(&[
            ("units\\imp\\a.imp", minimal_imp()),
            ("units\\imp\\a.h", matching_header("a")),
        ])
        .unreadable("imp\\broken.h");
        let report = archive.run(EMPTY_CATALOG);

        assert_eq!(report.validated, 1);
        assert_eq!(report.validation_failures, 1);
        assert!(
            report.failures[0].contains("could not read"),
            "{:?}",
            report.failures
        );
    }

    /// A four-by-two `FORM PBM ` carrying a `TINY` thumbnail, built here for the
    /// same reason as `minimal_imp`: the library's fixtures are not visible to
    /// this binary.
    fn minimal_pbm() -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&4_u16.to_be_bytes());
        header.extend_from_slice(&2_u16.to_be_bytes());
        header.extend_from_slice(&[0, 0, 0, 0, 8, 0, 1, 0]);
        header.extend_from_slice(&0_u16.to_be_bytes());
        header.extend_from_slice(&[1, 1]);
        header.extend_from_slice(&4_u16.to_be_bytes());
        header.extend_from_slice(&2_u16.to_be_bytes());
        let palette: Vec<u8> = (0..256_u16)
            .flat_map(|index| [index as u8, (index as u8) ^ 0x5a, (index as u8) ^ 0xa5])
            .collect();
        // Rows of 4: literal 0,1,2,3 then literal 3,2,1,0.
        let body = vec![3, 0, 1, 2, 3, 3, 3, 2, 1, 0];

        let mut chunks = Vec::new();
        for (id, data) in [
            (b"BMHD".as_slice(), header),
            (b"CMAP", palette),
            (b"TINY", vec![7; 5]),
            (b"BODY", body),
        ] {
            chunks.extend_from_slice(id);
            chunks.extend_from_slice(&(data.len() as u32).to_be_bytes());
            chunks.extend_from_slice(&data);
            if data.len() % 2 == 1 {
                chunks.push(0);
            }
        }
        let mut source = Vec::new();
        source.extend_from_slice(b"FORM");
        source.extend_from_slice(&((chunks.len() + 4) as u32).to_be_bytes());
        source.extend_from_slice(b"PBM ");
        source.extend_from_slice(&chunks);
        source
    }

    /// Writes `image` out as the indexed PNG the import path expects.
    fn png_bytes(image: &PbmImage) -> Vec<u8> {
        let mut encoded = Vec::new();
        write_pbm_png(&mut encoded, image).unwrap();
        encoded
    }

    /// A genuinely edited image is the case that matters: `TINY` is a thumbnail
    /// of `BODY`, so keeping it here would ship a file whose own preview is the
    /// artwork the editor just replaced. Dropping it is format-valid -- 128 of
    /// the 1,045 shipped PBMs carry no TINY at all.
    ///
    /// The round-trip test below cannot catch this: it imports an *unmodified*
    /// export, so TINY survives there either way.
    #[test]
    fn import_png_pbm_drops_the_thumbnail_when_the_pixels_change() {
        let dir = scratch_dir("pbm-import-tiny");
        let template = dir.join("source.lbm");
        let png = dir.join("edited.png");
        let output = dir.join("out.lbm");
        fs::write(&template, minimal_pbm()).unwrap();
        let mut image = PbmImage::decode(&minimal_pbm()).unwrap();
        let original = image.indices.clone();
        image.indices = vec![9, 9, 9, 9, 200, 201, 202, 203];
        assert_ne!(image.indices, original, "the fixture must really be edited");
        fs::write(&png, png_bytes(&image)).unwrap();

        import_png_pbm(&png, &template, &output).unwrap();

        let written = PbmFile::parse(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(written.image.indices, image.indices);
        let ids: Vec<&[u8]> = written
            .chunks
            .iter()
            .map(|chunk| chunk.id.as_slice())
            .collect();
        assert_eq!(
            ids,
            [b"BMHD".as_slice(), b"CMAP", b"BODY"],
            "the stale TINY must be gone, and no other chunk with it"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The whole point of the import path: pixels edited in a PNG come back as a
    /// PBM the decoder reads identically, with the chunks it never models intact.
    ///
    /// This one imports an *unmodified* export, which is exactly why it can
    /// still assert TINY survives: the thumbnail still describes the pixels.
    /// `import_png_pbm_drops_the_thumbnail_when_the_pixels_change` covers the
    /// case where it does not.
    #[test]
    fn import_png_pbm_round_trips_an_exported_image() {
        let dir = scratch_dir("pbm-import");
        let template = dir.join("source.lbm");
        let png = dir.join("edited.png");
        let output = dir.join("out.lbm");
        fs::write(&template, minimal_pbm()).unwrap();
        let image = PbmImage::decode(&minimal_pbm()).unwrap();
        let mut encoded = Vec::new();
        write_pbm_png(&mut encoded, &image).unwrap();
        fs::write(&png, &encoded).unwrap();

        import_png_pbm(&png, &template, &output).unwrap();

        let written = PbmFile::parse(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(written.image.indices, image.indices);
        let tiny = written
            .chunks
            .iter()
            .find(|chunk| &chunk.id == b"TINY")
            .expect("the TINY thumbnail must survive the import");
        assert_eq!(tiny.data, vec![7; 5]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_png_pbm_refuses_to_overwrite_an_existing_output() {
        let dir = scratch_dir("pbm-import-overwrite");
        let template = dir.join("source.lbm");
        let png = dir.join("edited.png");
        let output = dir.join("out.lbm");
        fs::write(&template, minimal_pbm()).unwrap();
        let image = PbmImage::decode(&minimal_pbm()).unwrap();
        let mut encoded = Vec::new();
        write_pbm_png(&mut encoded, &image).unwrap();
        fs::write(&png, &encoded).unwrap();
        fs::write(&output, b"precious").unwrap();

        let error = import_png_pbm(&png, &template, &output).unwrap_err();

        assert!(error.contains("could not create"), "{error}");
        assert_eq!(fs::read(&output).unwrap(), b"precious");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The written file keeps the template's CMAP, so a PNG whose palette was
    /// remapped would silently recolour every pixel. It is refused instead.
    #[test]
    fn import_png_pbm_refuses_a_remapped_palette() {
        let dir = scratch_dir("pbm-import-palette");
        let template = dir.join("source.lbm");
        let png = dir.join("edited.png");
        let output = dir.join("out.lbm");
        fs::write(&template, minimal_pbm()).unwrap();
        let mut image = PbmImage::decode(&minimal_pbm()).unwrap();
        image.palette[2] = [9, 9, 9];
        let mut encoded = Vec::new();
        write_pbm_png(&mut encoded, &image).unwrap();
        fs::write(&png, &encoded).unwrap();

        let error = import_png_pbm(&png, &template, &output).unwrap_err();

        assert!(error.contains("changed palette entry 2"), "{error}");
        assert!(!output.exists(), "a refused import must write nothing");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_png_pbm_refuses_a_png_of_the_wrong_size() {
        let dir = scratch_dir("pbm-import-size");
        let template = dir.join("source.lbm");
        let png = dir.join("edited.png");
        let output = dir.join("out.lbm");
        fs::write(&template, minimal_pbm()).unwrap();
        let mut image = PbmImage::decode(&minimal_pbm()).unwrap();
        image.width = 2;
        image.height = 4;
        let mut encoded = Vec::new();
        write_pbm_png(&mut encoded, &image).unwrap();
        fs::write(&png, &encoded).unwrap();

        let error = import_png_pbm(&png, &template, &output).unwrap_err();

        assert!(error.contains("2x4"), "{error}");
        assert!(!output.exists(), "a refused import must write nothing");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A minimal single-frame IMP with an origin pair, built here because the library's own
    /// fixture is `#[cfg(test)]` inside the library crate and so is not visible to this binary.
    /// `refusal_class` buckets a writer refusal by matching its message text, which is exactly the
    /// kind of coupling that rots silently: the writer rewords an error, the sweep quietly files it
    /// under `other`, and a report that used to say "3,439 frames share their pixels" starts saying
    /// nothing in particular.
    ///
    /// So the cases are driven by **errors `write_frame_pixels` actually produced**, not by literal
    /// strings copied out of it. A reworded message fails here rather than degrading a report.
    #[test]
    fn every_writer_refusal_lands_in_a_named_bucket() {
        // A two-frame file whose second record is a `0x08` back-reference to the first.
        let mut duplicated = minimal_imp();
        duplicated.splice(72..72, [0_u8; 16]);
        duplicated[8..12].copy_from_slice(&88_u32.to_le_bytes());
        duplicated[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        duplicated[56 + 12..56 + 16].copy_from_slice(&1112_u32.to_le_bytes());
        duplicated[72] = 0x08;

        // A 1x1 frame at 1bpp declaring a zero-byte payload, which is legal.
        let mut zero_length = minimal_imp();
        zero_length[0] = 0x10;
        zero_length[4..6].copy_from_slice(&1_u16.to_le_bytes());
        zero_length[56 + 2..56 + 4].copy_from_slice(&1_u16.to_le_bytes());
        zero_length[56 + 6..56 + 8].copy_from_slice(&0_u16.to_le_bytes());

        // A record declaring 0x0.
        let mut empty = minimal_imp();
        empty[56 + 2..56 + 4].copy_from_slice(&0_u16.to_le_bytes());
        empty[56 + 4..56 + 6].copy_from_slice(&0_u16.to_le_bytes());
        empty[56 + 6..56 + 8].copy_from_slice(&0_u16.to_le_bytes());

        // A 17x4 1bpp `record_variant == 0` frame stored as one 12-byte literal packet, so the
        // parser reads it row-padded. A replacement whose first packet lands on nine packed bytes
        // stops the parser early, and the writer refuses it.
        let mut stops_early = vec![0_u8; 72 + 1024];
        stops_early[0] = 0x01 | 0x10; // compressed, 1bpp
        stops_early[4..6].copy_from_slice(&17_u16.to_le_bytes());
        stops_early[6..8].copy_from_slice(&4_u16.to_le_bytes());
        stops_early[8..12].copy_from_slice(&72_u32.to_le_bytes());
        stops_early[26..28].copy_from_slice(&1_u16.to_le_bytes());
        stops_early[28..32].copy_from_slice(&32_u32.to_le_bytes());
        stops_early[32 + 11] = 1;
        stops_early[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        stops_early[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        stops_early[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        stops_early[56 + 2..56 + 4].copy_from_slice(&17_u16.to_le_bytes());
        stops_early[56 + 4..56 + 6].copy_from_slice(&4_u16.to_le_bytes());
        stops_early[56 + 12..56 + 16].copy_from_slice(&(72_u32 + 1024).to_le_bytes());
        stops_early.push(0xf4);
        stops_early.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let blank_then_full: Vec<u8> = std::iter::repeat_n(0_u8, 51)
            .chain(std::iter::repeat_n(1_u8, 17))
            .collect();

        let cases: [(&str, Vec<u8>, usize, Vec<u8>); 6] = [
            ("shared-pixels", duplicated.clone(), 0, vec![1, 2]),
            ("duplicate-record", duplicated, 1, vec![1, 2]),
            ("zero-length-payload", zero_length, 0, vec![1]),
            ("empty-frame", empty, 0, Vec::new()),
            ("does-not-read-back", stops_early, 0, blank_then_full),
            ("other", minimal_imp(), 9, vec![1, 2]),
        ];
        for (bucket, source, frame, pixels) in cases {
            let message = write_frame_pixels(&source, frame, &pixels)
                .expect_err(bucket)
                .to_string();
            assert_eq!(refusal_class(&message), bucket, "{message}");
        }
    }

    /// Export a frame and import it straight back: the file must come back byte for byte, which is
    /// the same acceptance property `--imp-roundtrip --rewrite` measures over the whole archive.
    #[test]
    fn import_png_imp_round_trips_an_exported_frame() {
        let dir = scratch_dir("imp-import");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        let output = dir.join("out.imp");
        fs::write(&template, minimal_imp()).unwrap();
        let sprite = ImpSprite::parse(&minimal_imp()).unwrap();
        let mut encoded = Vec::new();
        write_imp_frame_png(&mut encoded, &sprite, 0).unwrap();
        fs::write(&png, &encoded).unwrap();

        import_png_imp(&png, &template, 0, &output).unwrap();

        assert_eq!(fs::read(&output).unwrap(), minimal_imp());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Only the named frame's pixels come from the PNG. Everything else -- palette, records, the
    /// bytes the decoder does not model -- comes from the template.
    #[test]
    fn import_png_imp_replaces_only_the_named_frames_pixels() {
        let dir = scratch_dir("imp-import-pixels");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        let output = dir.join("out.imp");
        fs::write(&template, minimal_imp()).unwrap();
        let sprite = ImpSprite::parse(&minimal_imp()).unwrap();
        let palette: Vec<[u8; 3]> = sprite
            .palette
            .iter()
            .map(|entry| [entry[0], entry[1], entry[2]])
            .collect();
        let mut encoded = Vec::new();
        write_indexed_png_for_test(&mut encoded, 2, 1, &palette, &[0x11, 0x22]);
        fs::write(&png, &encoded).unwrap();

        import_png_imp(&png, &template, 0, &output).unwrap();

        let written = fs::read(&output).unwrap();
        assert_eq!(written.len(), minimal_imp().len());
        assert_eq!(
            ImpSprite::parse(&written).unwrap().frames[0].palette_indices,
            [0x11, 0x22]
        );
        // Only the two payload bytes at the end of the file changed.
        assert_eq!(
            &written[..written.len() - 2],
            &minimal_imp()[..written.len() - 2]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The PNG's size is checked against the *frame*, not the file, and it is checked from the
    /// header before a decode buffer is sized. A frame-sized mismatch is the common editing
    /// mistake, so the message has to name both sizes.
    #[test]
    fn import_png_imp_refuses_a_png_of_the_wrong_size() {
        let dir = scratch_dir("imp-import-size");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        let output = dir.join("out.imp");
        fs::write(&template, minimal_imp()).unwrap();
        let palette = vec![[0_u8, 0, 0]; 256];
        let mut encoded = Vec::new();
        write_indexed_png_for_test(&mut encoded, 3, 1, &palette, &[0, 1, 2]);
        fs::write(&png, &encoded).unwrap();

        let error = import_png_imp(&png, &template, 0, &output).unwrap_err();

        assert!(error.contains("PNG is 3x1"), "{error}");
        assert!(!output.exists(), "nothing may be written on a refusal");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The written file keeps the template's palette, so a PNG whose colours were remapped would
    /// silently mean different colours under the same indices.
    #[test]
    fn import_png_imp_refuses_a_remapped_palette() {
        let dir = scratch_dir("imp-import-palette");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        let output = dir.join("out.imp");
        fs::write(&template, minimal_imp()).unwrap();
        let mut palette = vec![[0_u8, 0, 0]; 256];
        // The template's entry 0 decodes to [2, 1, 3]; claim something else.
        palette[0] = [9, 9, 9];
        let mut encoded = Vec::new();
        write_indexed_png_for_test(&mut encoded, 2, 1, &palette, &[0, 1]);
        fs::write(&png, &encoded).unwrap();

        let error = import_png_imp(&png, &template, 0, &output).unwrap_err();

        assert!(error.contains("changed palette entry 0"), "{error}");
        assert!(!output.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Writing the edit back over its own template would destroy the only copy of the art. The
    /// `create_new` open would refuse it too, but with "file exists", which reads like a stale
    /// leftover rather than the mistake it is.
    #[test]
    fn import_png_imp_refuses_to_write_over_its_own_input() {
        let dir = scratch_dir("imp-import-selfwrite");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        fs::write(&template, minimal_imp()).unwrap();
        let sprite = ImpSprite::parse(&minimal_imp()).unwrap();
        let mut encoded = Vec::new();
        write_imp_frame_png(&mut encoded, &sprite, 0).unwrap();
        fs::write(&png, &encoded).unwrap();

        for output in [&template, &png] {
            let error = import_png_imp(&png, &template, 0, output).unwrap_err();
            assert!(
                error.contains("refusing to write to the input file"),
                "{error}"
            );
        }
        assert_eq!(fs::read(&template).unwrap(), minimal_imp());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_png_imp_refuses_to_overwrite_an_existing_output() {
        let dir = scratch_dir("imp-import-overwrite");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        let output = dir.join("out.imp");
        fs::write(&template, minimal_imp()).unwrap();
        let sprite = ImpSprite::parse(&minimal_imp()).unwrap();
        let mut encoded = Vec::new();
        write_imp_frame_png(&mut encoded, &sprite, 0).unwrap();
        fs::write(&png, &encoded).unwrap();
        fs::write(&output, b"do not clobber me").unwrap();

        let error = import_png_imp(&png, &template, 0, &output).unwrap_err();

        assert!(error.contains("could not create"), "{error}");
        assert_eq!(fs::read(&output).unwrap(), b"do not clobber me");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Encodes an indexed PNG directly, so a test can build files the exporter would refuse.
    fn write_indexed_png_for_test(
        into: &mut Vec<u8>,
        width: u32,
        height: u32,
        palette: &[[u8; 3]],
        indices: &[u8],
    ) {
        let bytes: Vec<u8> = palette.iter().flatten().copied().collect();
        let mut encoder = png::Encoder::new(into, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(bytes);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(indices).unwrap();
    }

    fn minimal_imp() -> Vec<u8> {
        const PALETTE_BYTES: usize = 256 * 4;
        let mut source = vec![0_u8; 32 + 16 + 8 + 16];
        source[2] = 1;
        source[4..6].copy_from_slice(&2_u16.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        let palette_offset = source.len();
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&1_u16.to_le_bytes());
        source[56 + 6..56 + 8].copy_from_slice(&2_u16.to_le_bytes());
        let pixel_offset = palette_offset + PALETTE_BYTES;
        source[56 + 12..56 + 16].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        source.resize(pixel_offset, 0);
        source[palette_offset..palette_offset + 4].copy_from_slice(&[3, 2, 1, 0]);
        source.extend_from_slice(&[0xaa, 0xbb]);
        source
    }

    /// `minimal_imp` with a second frame that is a `0x08` back-reference to the first.
    ///
    /// The class that matters here: 10,293 of `imp.mpq`'s 51,666 frame records carry no payload of
    /// their own, and the parser leaves their width and height at zero because the record does.
    fn minimal_imp_with_a_duplicate_frame() -> Vec<u8> {
        const PALETTE_BYTES: usize = 256 * 4;
        const FRAME_FLAG_DUPLICATE: u8 = 0x08;
        let mut source = vec![0_u8; 32 + 16 + 8 + 16 * 2];
        source[2] = 1;
        source[4..6].copy_from_slice(&2_u16.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        let palette_offset = source.len();
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&1_u16.to_le_bytes());
        source[56 + 6..56 + 8].copy_from_slice(&2_u16.to_le_bytes());
        let pixel_offset = palette_offset + PALETTE_BYTES;
        source[56 + 12..56 + 16].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        // Frame 1: a back-reference, whose pixel dword is the frame *index* 0, not an offset.
        source[72] = FRAME_FLAG_DUPLICATE;
        source.resize(pixel_offset, 0);
        source[palette_offset..palette_offset + 4].copy_from_slice(&[3, 2, 1, 0]);
        source.extend_from_slice(&[0xaa, 0xbb]);
        source
    }

    /// Export resolves a duplicate record to the frame that owns the pixels; import must refuse it
    /// by name rather than by its unresolved `0x0` dimensions.
    ///
    /// The bug this pins: the import took the PNG's expected size from `sprite.frames[i]` straight,
    /// which is `0x0` for the 10,293 records that store no pixels, so the round trip of an exported
    /// frame died inside the PNG reader with "PNG is 2x1 but the template is 0x0" -- the wrong
    /// dimensions, a message about the PBM header on an IMP path, and no hint that the fix is to
    /// edit frame 0 instead.
    #[test]
    fn import_png_imp_names_the_frame_that_owns_the_pixels_rather_than_its_zero_size() {
        let dir = scratch_dir("imp-import-duplicate");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        let output = dir.join("out.imp");
        let bytes = minimal_imp_with_a_duplicate_frame();
        fs::write(&template, &bytes).unwrap();
        let sprite = ImpSprite::parse(&bytes).unwrap();
        assert_eq!(sprite.frames[1].source_frame, Some(0));
        assert_eq!((sprite.frames[1].width, sprite.frames[1].height), (0, 0));
        // Export succeeds, because it resolves the reference. That asymmetry is the whole problem.
        let mut encoded = Vec::new();
        write_imp_frame_png(&mut encoded, &sprite, 1).unwrap();
        fs::write(&png, &encoded).unwrap();

        let error = import_png_imp(&png, &template, 1, &output).unwrap_err();

        assert!(
            error.contains("is a duplicate of frame 0") && error.contains("replace frame 0"),
            "{error}"
        );
        assert!(
            !error.contains("the template is 0x0"),
            "the unresolved dimensions must never reach the user: {error}"
        );
        assert!(!output.exists(), "nothing may be written on a refusal");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The size-mismatch message is shared with the PBM import, and it used to name the PBM header
    /// unconditionally -- on a path that has no PBM in it.
    #[test]
    fn the_size_mismatch_refusal_names_the_format_it_was_reached_from() {
        let dir = scratch_dir("imp-import-size-wording");
        let template = dir.join("source.imp");
        let png = dir.join("edited.png");
        let output = dir.join("out.imp");
        fs::write(&template, minimal_imp()).unwrap();
        let palette = vec![[0_u8, 0, 0]; 256];
        let mut encoded = Vec::new();
        write_indexed_png_for_test(&mut encoded, 3, 1, &palette, &[0, 1, 2]);
        fs::write(&png, &encoded).unwrap();

        let error = import_png_imp(&png, &template, 0, &output).unwrap_err();

        assert!(error.contains("PNG is 3x1 but the template is 2x1"), "{error}");
        assert!(error.contains("the IMP frame record"), "{error}");
        assert!(!error.contains("PBM"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("lom-placement-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch dir");
        path
    }

    /// Regression for the review's critical finding: this is the only command that mutates game
    /// art, and it used `fs::write`, which truncates. Every other output path here is create-new.
    #[test]
    fn set_imp_placement_refuses_to_overwrite_an_existing_output() {
        let dir = scratch_dir("overwrite");
        let input = dir.join("in.imp");
        let output = dir.join("out.imp");
        fs::write(&input, minimal_imp()).unwrap();
        fs::write(&output, b"precious").unwrap();

        let error = set_imp_placement(&input, 0, 1, 2, None, &output).unwrap_err();
        assert!(error.contains("could not create"), "{error}");
        assert_eq!(fs::read(&output).unwrap(), b"precious");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The published candidate list must not hide an engine name because the corpus happens to
    /// define a differently-cased one.
    ///
    /// `GameScriptVm::lookup` resolves case-sensitively, so `GOLD` and `gold` are two names. This
    /// check used to fold, and `gs\barter.gs`'s `/gold` suppressed `GOLD` -- 290 uses in 3.02 --
    /// from the list this tool prints. Restoring the fold fails both assertions below: `GOLD`
    /// leaves the candidate list and the recovery count drops to zero. Verified by mutation.
    #[test]
    fn the_candidate_list_does_not_let_a_lowercase_definition_hide_an_uppercase_call() {
        let dir = scratch_dir("candidate-case");
        let image = dir.join("fake.exe");
        // The filter only asks whether the name occurs as an ASCII run in the image.
        fs::write(&image, b"\0gold\0getarmydata\0GOLD\0").unwrap();

        let executable_names = BTreeMap::from([
            ("GOLD".to_owned(), 290_usize),
            ("gold".to_owned(), 5),
            ("getarmydata".to_owned(), 3),
        ]);
        let definition_names = BTreeMap::from([("gold".to_owned(), 1_usize)]);

        let found =
            super::likely_engine_names(&image, &executable_names, &definition_names).unwrap();
        let names: Vec<&str> = found
            .candidates
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();

        // `GOLD` is called and never defined, so it belongs in the list; `gold` is defined, so it
        // does not. The two decisions are independent.
        assert_eq!(names, ["GOLD", "getarmydata"]);
        assert_eq!(
            found.hidden_by_the_old_case_fold,
            [("GOLD".to_owned(), 290_usize)]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_imp_placement_writes_a_new_file_and_reads_the_value_back() {
        let dir = scratch_dir("write");
        let input = dir.join("in.imp");
        let output = dir.join("out.imp");
        fs::write(&input, minimal_imp()).unwrap();

        set_imp_placement(&input, 0, -7, 9, None, &output).unwrap();
        let written = ImpSprite::parse(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(written.frames[0].origin_x, Some(-7));
        assert_eq!(written.frames[0].origin_y, Some(9));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn placement_arguments_reject_values_that_do_not_fit() {
        assert!(parse_offset("40000").is_err());
        assert!(parse_offset("-40000").is_err());
        assert_eq!(parse_offset("-35").unwrap(), -35);
        assert!(parse_dimension("-1").is_err());
        assert_eq!(parse_dimension("53").unwrap(), 53);
        assert_eq!(parse_coordinate("-320").unwrap(), -320);
        assert!(parse_coordinate("x").is_err());
    }

    #[test]
    fn a_packed_length_only_names_a_layout_when_the_layouts_differ() {
        // 8bpp: every width makes the two the same length, so no frame at that depth is evidence
        // about which layout its packer chose. Reporting those as "tight" would be a claim the
        // corpus does not support, and the corpus is almost all 8bpp.
        let square = PackedSizes::for_frame(20, 10, 8).unwrap();
        assert_eq!(square.tight_ceil, square.row_padded);
        assert_eq!(
            PackedSizeChoice::classify(200, square, true),
            Some(PackedSizeChoice::Identical)
        );

        // A single row is free of the choice too, whatever its width: there is no second row for
        // the row padding to push along. The corpus's entire ambiguous-looking population turned
        // out to be 1x1 and 2x1 frames, so getting this wrong invented 88 phantom ambiguities.
        let one_row = PackedSizes::for_frame(7, 1, 1).unwrap();
        assert_eq!(one_row.tight_ceil, one_row.row_padded);
        assert_eq!(
            PackedSizeChoice::classify(1, one_row, true),
            Some(PackedSizeChoice::Identical)
        );

        // Equal lengths are not equal layouts. 7 pixels a row at 1bpp over five rows costs five
        // bytes packed either way, and the five bytes differ.
        let ragged_but_equal = PackedSizes::for_frame(7, 5, 1).unwrap();
        assert_eq!(ragged_but_equal.tight_ceil, ragged_but_equal.row_padded);
        assert_eq!(
            PackedSizeChoice::classify(5, ragged_but_equal, false),
            Some(PackedSizeChoice::AmbiguousLength)
        );

        // 17 pixels at 1bpp need 17 bits, so a row costs 3 bytes padded and the whole frame costs
        // 9 tight. Those differ, and only then does a length name a layout.
        let ragged = PackedSizes::for_frame(17, 4, 1).unwrap();
        assert_eq!(
            (ragged.tight_floor, ragged.tight_ceil, ragged.row_padded),
            (8, 9, 12)
        );
        assert_eq!(
            PackedSizeChoice::classify(9, ragged, false),
            Some(PackedSizeChoice::Tight)
        );
        assert_eq!(
            PackedSizeChoice::classify(12, ragged, false),
            Some(PackedSizeChoice::RowPadded)
        );
        assert_eq!(
            PackedSizeChoice::classify(8, ragged, false),
            Some(PackedSizeChoice::TightFloor)
        );
        assert_eq!(PackedSizeChoice::classify(10, ragged, false), None);
        assert_eq!(PackedSizeChoice::classify(0, ragged, false), None);

        // A frame whose tight size already fills its last byte has no floor form to fall back to,
        // so the floor and the ceiling coincide and the length still names the padded layout.
        let flush = PackedSizes::for_frame(4, 4, 1).unwrap();
        assert_eq!(
            (flush.tight_floor, flush.tight_ceil, flush.row_padded),
            (2, 2, 4)
        );
        assert_eq!(
            PackedSizeChoice::classify(2, flush, false),
            Some(PackedSizeChoice::Tight)
        );
        assert_eq!(
            PackedSizeChoice::classify(4, flush, false),
            Some(PackedSizeChoice::RowPadded)
        );
    }

    /// A synthetic frame shape, so the arithmetic can be driven past where the corpus happens to
    /// sit.
    ///
    /// `PackedSizes` is built by hand rather than from real dimensions on purpose. The whole point
    /// of these tests is the `delta = row_padded - tight_ceil` boundary at 390/391, and no frame in
    /// `imp.mpq` is anywhere near it -- a corpus run can only ever report that all 752 landed on
    /// one side. A fixture shaped like the corpus could not fail on what the corpus hides.
    fn sizes_with_delta(tight_ceil: usize, delta: usize) -> PackedSizes {
        PackedSizes {
            tight_floor: tight_ceil.saturating_sub(1),
            tight_ceil,
            row_padded: tight_ceil + delta,
        }
    }

    /// The gate: only a compressed, variant-0 frame whose stored size is *not* the row-padded one
    /// is out of this decoder's reach. Each of the four clauses is tested by removing it alone.
    #[test]
    fn only_a_compressed_variant_0_frame_stopping_short_is_unobservable() {
        let ragged = PackedSizes::for_frame(17, 4, 1).unwrap();
        assert_eq!(
            (ragged.tight_floor, ragged.tight_ceil, ragged.row_padded),
            (8, 9, 12)
        );

        // The tight size on a shape where the layouts differ: the reader stopped at 9 and could
        // never have reached 12. This is the class the 752 are drawn from.
        assert!(LayoutObservability::for_frame(9, ragged, 0, true).unobservable);
        // So is the tight-floor size, for the same reason: 8 is short of 12 too.
        assert!(LayoutObservability::for_frame(8, ragged, 0, true).unobservable);

        // The row-padded size itself. The reader DID land on 12 -- a single 12-byte literal packet
        // reaches it without passing through 8 or 9 -- so the layout was observed, and counting it
        // would let the sweep report `row-padded` and `unobservable` for one frame at once.
        assert!(!LayoutObservability::for_frame(12, ragged, 0, true).unobservable);

        // A variant-1 record declares its length, so the reader consumes exactly that many bytes
        // and never has to guess where to stop.
        assert!(!LayoutObservability::for_frame(9, ragged, 1, true).unobservable);

        // An uncompressed payload is read by length, not by decoding packets until a size matches.
        assert!(!LayoutObservability::for_frame(9, ragged, 0, false).unobservable);

        // A shape whose two layouts are the same length records no choice for the reader to miss.
        // Those frames are the separate `ambiguous-same-length` population and must not be added
        // here as well.
        let equal = PackedSizes::for_frame(7, 5, 1).unwrap();
        assert_eq!(equal.tight_ceil, equal.row_padded);
        assert!(!LayoutObservability::for_frame(5, equal, 0, true).unobservable);

        // The case that makes `tight_ceil != row_padded` load-bearing on its own, and the reason
        // this predicate takes a stored size rather than a classified layout name. A 7x1 frame at
        // 1bpp stores `tight_floor` 0 while `tight_ceil == row_padded == 1`: its stored size is not
        // the row-padded one, it is compressed and variant 0, and it is still not unobservable,
        // because there is no row-padded layout for the reader to have missed. Every other clause
        // here is satisfied, so dropping this one would count it.
        let floored = PackedSizes::for_frame(7, 1, 1).unwrap();
        assert_eq!(
            (floored.tight_floor, floored.tight_ceil, floored.row_padded),
            (0, 1, 1)
        );
        assert!(!LayoutObservability::for_frame(0, floored, 0, true).unobservable);
    }

    /// `not_cleared_by_gaps` is a strict subset of `unobservable`, which is what lets the report
    /// print both counters and call them equal rather than adding them.
    #[test]
    fn a_frame_the_decoder_could_see_is_never_counted_as_uncleared() {
        let ragged = PackedSizes::for_frame(17, 4, 1).unwrap();
        for (packed_size, variant, compressed) in
            [(12, 0, true), (9, 1, true), (9, 0, false), (12, 1, false)]
        {
            let verdict = LayoutObservability::for_frame(packed_size, ragged, variant, compressed);
            assert_eq!(
                verdict,
                LayoutObservability::default(),
                "packed_size={packed_size} variant={variant} compressed={compressed}"
            );
        }
    }

    /// The 390/391 boundary, from both sides, and it is the whole of the "the gaps clear none of
    /// them" claim.
    ///
    /// An unread row-padded continuation of `delta` packed bytes costs at least
    /// `2 * ceil(delta / 130)` stored bytes. That reaches 8 -- the archive's payload alignment, and
    /// so the smallest shortfall a gap could distinguish from ordinary padding -- exactly when
    /// `ceil(delta / 130)` reaches 4, at `delta == 391`.
    ///
    /// Both constants are pinned here in both directions. Raising the threshold to `<= 8` flips
    /// delta 391; lowering it to `< 6` flips delta 390. Widening the repeat length to 131 flips
    /// delta 391 the other way, and narrowing it to 129 flips delta 390. What is NOT killable is
    /// `< 7`: the minimum is `2 * k` and therefore always even, so no input separates it from
    /// `< 8`, and a surviving mutant there is equivalent rather than untested.
    #[test]
    fn the_gap_evidence_clears_a_frame_exactly_when_the_shortfall_reaches_eight_stored_bytes() {
        // delta 390 = 3 full repeat packets, 6 stored bytes, under the 8-byte alignment: it hides,
        // and the gap histogram says nothing.
        let hides = sizes_with_delta(16, 390);
        assert!(LayoutObservability::for_frame(16, hides, 0, true).unobservable);
        assert!(LayoutObservability::for_frame(16, hides, 0, true).not_cleared_by_gaps);

        // delta 391 spills into a fourth packet: 8 stored bytes, which could not hide inside a
        // 7-byte gap. Still unobservable by the decoder; cleared by the alignment evidence.
        let shows = sizes_with_delta(16, 391);
        assert!(LayoutObservability::for_frame(16, shows, 0, true).unobservable);
        assert!(!LayoutObservability::for_frame(16, shows, 0, true).not_cleared_by_gaps);

        // Far past the boundary, where no arithmetic slip could still land under 8.
        let obvious = sizes_with_delta(16, 4000);
        assert!(!LayoutObservability::for_frame(16, obvious, 0, true).not_cleared_by_gaps);

        // And the smallest possible shortfall, one packed byte: two stored bytes, well inside the
        // padding.
        let tiny = sizes_with_delta(16, 1);
        assert!(LayoutObservability::for_frame(16, tiny, 0, true).not_cleared_by_gaps);
    }

    /// The corpus's own 752 are all far below the boundary, which is the fact the doc reports and
    /// the reason the two printed counters are equal. Stated as a test so a change to the threshold
    /// cannot quietly move the published number.
    #[test]
    fn the_real_frame_shapes_in_the_archive_all_hide_inside_the_alignment() {
        // The largest frame the format can hold at 1bpp is bounded by the header's maximum
        // dimensions, but the shapes that actually occur are small: a 17x4 leaves 3 packed bytes.
        let ragged = PackedSizes::for_frame(17, 4, 1).unwrap();
        assert_eq!(ragged.row_padded - ragged.tight_ceil, 3);
        assert!(LayoutObservability::for_frame(9, ragged, 0, true).not_cleared_by_gaps);

        // A real shape CAN escape -- the arithmetic is not vacuously one-sided -- but it takes a
        // 391-byte shortfall, and at 1bpp the shortfall grows by under a byte per row. A 9-pixel
        // row wastes 7 bits, so it takes a frame around 450 rows tall, and `imp.mpq`'s tallest
        // frame is nothing like that. This is the reason the boundary above had to be driven with
        // a synthetic `PackedSizes`: no frame in the corpus sits near it.
        let tall = PackedSizes::for_frame(9, 500, 1).unwrap();
        assert!(
            tall.row_padded - tall.tight_ceil >= 391,
            "{tall:?} should overshoot the boundary"
        );
        assert!(LayoutObservability::for_frame(tall.tight_ceil, tall, 0, true).unobservable);
        assert!(!LayoutObservability::for_frame(tall.tight_ceil, tall, 0, true).not_cleared_by_gaps);
    }

    fn navigation_sprite() -> ImpSprite {
        let frames = (0..6)
            .map(|index| ImpFrame {
                flags: 0,
                width: 1,
                height: 1,
                origin_x: None,
                origin_y: None,
                hotspots: Vec::new(),
                palette_indices: vec![2],
                rgba: vec![1, 2, 3, 255],
                source_frame: None,
                record_offset: index * 16,
                hotspot_offset: None,
                packed_size: Some(1),
                pixels_offset: Some(index * 16),
                stored_size: Some(1),
            })
            .collect();
        ImpSprite {
            file_flags: 0,
            record_variant: 1,
            compressed: false,
            bits_per_pixel: 8,
            maximum_width: 1,
            maximum_height: 1,
            sequence_count: 2,
            facing_count: 3,
            frame_count: 6,
            color_key: 0,
            duplicate_frame_count: 0,
            back_reference_frame_count: 0,
            hotspot_count: 0,
            hotspot_bytes: 0,
            raw_pixel_bytes: 6,
            stored_pixel_bytes: 6,
            palette: vec![[0, 0, 0, 255]; 256],
            sequences: vec![
                ImpSequence {
                    metadata: [0; 11],
                    first_facing: 0,
                    facing_count: 2,
                    first_frame: 0,
                    frame_count: 4,
                },
                ImpSequence {
                    metadata: [0; 11],
                    first_facing: 2,
                    facing_count: 1,
                    first_frame: 4,
                    frame_count: 2,
                },
            ],
            facings: vec![
                ImpFacing {
                    metadata: 0,
                    first_frame: 0,
                    frame_count: 2,
                },
                ImpFacing {
                    metadata: 0,
                    first_frame: 2,
                    frame_count: 2,
                },
                ImpFacing {
                    metadata: 0,
                    first_frame: 4,
                    frame_count: 2,
                },
            ],
            frames,
        }
    }

    use super::{gamescript_failure_lines, operator_signature_lines};
    use lom_asset_viewer::gamescript_vm::GameScriptVmError;
    use lom_asset_viewer::native_table::{OperatorIndex, PeImage};

    /// Matches the private `RECORD_SIZE` in `native_table`: a name pointer and a code pointer.
    const NATIVE_RECORD_SIZE: usize = 8;

    /// Wrap a run of machine code and an operator table in a minimal PE, following the pattern in
    /// `native_table`'s and `operator_arity`'s own test modules, so the reporting path can be
    /// exercised without the proprietary binary.
    ///
    /// `code` is placed at the start of `.text`; `names` become a native-operator table whose
    /// records all point at that same entry point (its arity is what these tests check, not any
    /// distinction between operators).
    fn synthetic_probe_image(code: &[u8], names: &[&str]) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        const IMAGE_BASE: u32 = 0x0040_0000;
        const CODE_VA: u32 = 0x1000;
        const CODE_RAW: u32 = 0x200;
        const CODE_SIZE: u32 = 0x400;
        const DATA_VA: u32 = 0x2000;
        const DATA_RAW: u32 = 0x800;
        const DATA_SIZE: u32 = 0x400;

        let mut image = vec![0_u8; (DATA_RAW + DATA_SIZE) as usize];
        image[0x3c..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        image[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        image[PE_OFFSET + 6..PE_OFFSET + 8].copy_from_slice(&2_u16.to_le_bytes());
        let optional_size: u16 = 0xe0;
        image[PE_OFFSET + 20..PE_OFFSET + 22].copy_from_slice(&optional_size.to_le_bytes());
        image[PE_OFFSET + 24..PE_OFFSET + 26].copy_from_slice(&0x10b_u16.to_le_bytes());
        image[PE_OFFSET + 52..PE_OFFSET + 56].copy_from_slice(&IMAGE_BASE.to_le_bytes());

        let section_table = PE_OFFSET + 24 + usize::from(optional_size);
        let mut write_section =
            |index: usize, name: &[u8], va: u32, raw_size: u32, raw: u32, characteristics: u32| {
                let base = section_table + index * 40;
                image[base..base + name.len()].copy_from_slice(name);
                image[base + 12..base + 16].copy_from_slice(&va.to_le_bytes());
                image[base + 16..base + 20].copy_from_slice(&raw_size.to_le_bytes());
                image[base + 20..base + 24].copy_from_slice(&raw.to_le_bytes());
                image[base + 36..base + 40].copy_from_slice(&characteristics.to_le_bytes());
            };
        write_section(0, b".text", CODE_VA, CODE_SIZE, CODE_RAW, 0x2000_0000);
        write_section(1, b".data", DATA_VA, DATA_SIZE, DATA_RAW, 0x4000_0000);

        image[CODE_RAW as usize..CODE_RAW as usize + code.len()].copy_from_slice(code);
        let entry_point = IMAGE_BASE + CODE_VA;

        let mut name_addresses = Vec::new();
        let mut cursor = (DATA_RAW + DATA_SIZE) as usize - 0x100;
        for name in names {
            let bytes = name.as_bytes();
            image[cursor..cursor + bytes.len()].copy_from_slice(bytes);
            image[cursor + bytes.len()] = 0;
            name_addresses.push(IMAGE_BASE + DATA_VA + (cursor as u32 - DATA_RAW));
            cursor += bytes.len() + 1;
        }
        for (index, address) in name_addresses.iter().enumerate() {
            let record = DATA_RAW as usize + index * NATIVE_RECORD_SIZE;
            image[record..record + 4].copy_from_slice(&address.to_le_bytes());
            image[record + 4..record + 8].copy_from_slice(&entry_point.to_le_bytes());
        }
        image
    }

    /// `mov eax,[esi+0x54]`
    const LOAD_INDEX: [u8; 3] = [0x8b, 0x46, 0x54];
    /// `inc eax`
    const INC_EAX: [u8; 1] = [0x40];
    /// `mov [esi+0x54],eax`
    const STORE_INDEX: [u8; 3] = [0x89, 0x46, 0x54];
    /// `ret`
    const RET: [u8; 1] = [0xc3];

    /// Enough distinct names to clear `MINIMUM_RUN` in `native_table::extract`.
    const NINE_NAMES: [&str; 9] = [
        "add", "sub", "mul", "dup", "exch", "def", "undef", "begin", "end",
    ];

    fn two_pop_one_push_body() -> Vec<u8> {
        [
            LOAD_INDEX.as_slice(),
            &INC_EAX,
            &STORE_INDEX,
            &LOAD_INDEX,
            &INC_EAX,
            &STORE_INDEX,
            &LOAD_INDEX,
            &[0x48], // dec eax
            &STORE_INDEX,
            &RET,
        ]
        .concat()
    }

    fn unknown_name_error(name: &str) -> GameScriptVmError {
        GameScriptVmError {
            message: format!("unknown executable name {name}"),
            step: 7,
            call_stack: vec!["outer".to_owned()],
        }
    }

    #[test]
    fn operator_signature_lines_report_a_well_formed_two_operand_one_result_operator() {
        let bytes = synthetic_probe_image(&two_pop_one_push_body(), &NINE_NAMES);
        let pe_image = PeImage::parse(&bytes).expect("synthetic image parses");
        let lines = operator_signature_lines("add", 0x0040_1000, &pe_image);

        assert!(lines.contains(&"unknown-name-pops\t2".to_owned()));
        assert!(lines.contains(&"unknown-name-pushes\t1".to_owned()));
        assert!(lines.contains(&"unknown-name-confidence\twell-formed".to_owned()));
        let remedy = lines
            .iter()
            .find(|line| line.starts_with("unknown-name-remedy"))
            .expect("a remedy line");
        assert!(remedy.contains("takes 2 operands and returns 1 result"), "{remedy}");
        // The site-count caveat must survive even for a well-formed walk: `mul` is well formed
        // and still overcounts, so "well-formed" must not be sold as proof of arity.
        assert!(
            remedy.contains("not proven arity"),
            "remedy must not present the count as certain: {remedy}"
        );
    }

    #[test]
    fn operator_signature_lines_flag_an_unclassified_store_as_lower_confidence() {
        // A commit with no recognised adjustment: the idiom does not apply here, and the walk
        // must say so rather than reporting a clean pop/push count.
        let code = [LOAD_INDEX.as_slice(), &STORE_INDEX, &RET].concat();
        let bytes = synthetic_probe_image(&code, &NINE_NAMES);
        let pe_image = PeImage::parse(&bytes).expect("synthetic image parses");
        let lines = operator_signature_lines("add", 0x0040_1000, &pe_image);

        assert!(lines.contains(&"unknown-name-confidence\tunclassified-store".to_owned()));
    }

    #[test]
    fn gamescript_failure_lines_include_the_recovered_signature_for_an_operator_name() {
        let bytes = synthetic_probe_image(&two_pop_one_push_body(), &NINE_NAMES);
        let operators = OperatorIndex::from_image(&bytes).expect("synthetic image parses");
        let pe_image = PeImage::parse(&bytes).expect("synthetic image parses");
        let error = unknown_name_error("add");

        let lines = gamescript_failure_lines(&error, Some((&operators, &pe_image)));

        assert!(lines.contains(&"unknown-name-class\toperator".to_owned()));
        assert!(lines.iter().any(|line| line.starts_with("unknown-name-pops")));
        assert!(lines.iter().any(|line| line.starts_with("unknown-name-pushes")));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("unknown-name-confidence"))
        );
    }

    #[test]
    fn gamescript_failure_lines_report_no_signature_for_a_constant() {
        let bytes = synthetic_probe_image(&two_pop_one_push_body(), &NINE_NAMES);
        let operators = OperatorIndex::from_image(&bytes).expect("synthetic image parses");
        let pe_image = PeImage::parse(&bytes).expect("synthetic image parses");
        let error = unknown_name_error("SD_MANA");

        let lines = gamescript_failure_lines(&error, Some((&operators, &pe_image)));

        assert!(lines.contains(&"unknown-name-class\tengine-constant".to_owned()));
        assert!(!lines.iter().any(|line| line.starts_with("unknown-name-pops")));
    }

    #[test]
    fn a_tail_that_is_a_prefix_of_the_other_differs_where_it_ends() {
        assert_eq!(super::first_tail_difference(b"ABCD", b"ABCD"), None);
        assert_eq!(super::first_tail_difference(b"ABCD", b"ABCDEF"), Some(4));
        assert_eq!(super::first_tail_difference(b"ABCDEF", b"ABCD"), Some(4));
        assert_eq!(super::first_tail_difference(b"ABCD", b"ABXD"), Some(2));
        assert_eq!(super::first_tail_difference(b"", b""), None);
        assert_eq!(super::first_tail_difference(b"", b"A"), Some(0));
    }

    // --- map editing ---------------------------------------------------------------------

    /// A deliberately **non-square** map with one placed-sprite record, so a test that gets the
    /// two coordinate axes the wrong way round cannot pass by symmetry.
    fn editable_map(width: u32, height: u32) -> Vec<u8> {
        let mut source = Vec::new();
        source.extend_from_slice(&0x6c_u32.to_le_bytes());
        source.extend_from_slice(&width.to_le_bytes());
        source.extend_from_slice(&height.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        for _ in 0..width * height {
            source.extend_from_slice(&15_u32.to_le_bytes());
            source.extend_from_slice(&1.0_f32.to_bits().to_le_bytes());
        }
        source.extend_from_slice(&0_u32.to_le_bytes());
        source.extend_from_slice(&1_u32.to_le_bytes());
        source
    }

    /// An editable map in the **52-byte** record layout: header word 79, and a trailing section
    /// that is a lone zero count with **no footer**.
    ///
    /// Four of the six layouts have no footer word at all, which `editable_map` -- a 49-byte-layout
    /// fixture -- cannot exercise. Until 2026-09-17 a map shaped like this left its tail raw and
    /// `--map-place-sprite` refused it, which was the state of 169 of the 365 installed maps.
    fn editable_plain_map(width: u32, height: u32) -> Vec<u8> {
        let mut source = Vec::new();
        source.extend_from_slice(&79_u32.to_le_bytes());
        source.extend_from_slice(&width.to_le_bytes());
        source.extend_from_slice(&height.to_le_bytes());
        source.extend_from_slice(&8_u32.to_le_bytes());
        for _ in 0..width * height {
            source.extend_from_slice(&15_u32.to_le_bytes());
            source.extend_from_slice(&1.0_f32.to_bits().to_le_bytes());
        }
        source.extend_from_slice(&0_u32.to_le_bytes());
        source
    }

    #[test]
    fn placing_a_sprite_on_a_footerless_map_writes_that_layouts_record() {
        let dir = scratch_dir("map-plain-place");
        let input = dir.join("in.smp");
        let output = dir.join("out.smp");
        let removed = dir.join("removed.smp");
        let source = editable_plain_map(5, 3);
        fs::write(&input, &source).unwrap();

        edit_map(
            &input,
            MapEdit::PlaceSprite { x: 3, y: 2, sprite_type: 105 },
            &output,
            None,
        )
        .unwrap();
        let written_bytes = fs::read(&output).unwrap();
        assert_eq!(
            written_bytes.len(),
            source.len() + 52,
            "a 52-byte layout must grow by 52 bytes, and no footer may appear"
        );
        let written = MapAsset::parse(&written_bytes).unwrap();
        let section = written.placed_sprites.as_ref().unwrap();
        assert_eq!(section.layout.record_size, 52);
        assert_eq!(section.footer, None);
        let record = &section.records[0];
        assert_eq!(record.size(), 52);
        assert_eq!(record.sprite_type, 105);
        assert_eq!(written.record_coordinates(record), (3, 2));

        // And removing it leaves the file it started from, as the engine's own removal does.
        edit_map(
            &output,
            MapEdit::RemoveSprite { instance_id: record.instance_id },
            &removed,
            None,
        )
        .unwrap();
        assert_eq!(fs::read(&removed).unwrap(), source);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A paint's terrain ceiling is the **tileset**, not the eleven-name world table.
    ///
    /// Terrain ids are tileset-local and the combat tilesets reach 42, so the shared parser's
    /// `0..10` ceiling refused every terrain the shipped battle maps are made of. Id 21 is the
    /// input that makes this fail: it is a real `chbldg01.til` terrain and an error for
    /// `--map-set-terrain`. The other half matters too -- the narrow parser must **keep** its
    /// ceiling, because the verbs using it have no tileset to check against.
    #[test]
    fn a_paint_accepts_a_tileset_local_terrain_id_that_the_world_table_rejects() {
        use super::parse_paint_terrain_type;

        for id in [11_u32, 18, 21, 25, 42] {
            assert_eq!(parse_paint_terrain_type(&id.to_string()).unwrap(), id);
            assert!(
                parse_terrain_type(&id.to_string()).is_err(),
                "{id} must still be refused by the tileset-less parser"
            );
        }
        // Ids the world table does know are unchanged in both.
        for id in [0_u32, 6, 10] {
            assert_eq!(parse_paint_terrain_type(&id.to_string()).unwrap(), id);
            assert_eq!(parse_terrain_type(&id.to_string()).unwrap(), id);
        }
        // Names still go through the world vocabulary, and nonsense is still nonsense.
        assert_eq!(parse_paint_terrain_type("water").unwrap(), 1);
        assert!(parse_paint_terrain_type("tt_nonsense").is_err());
        assert!(parse_paint_terrain_type("-1").is_err());
    }

    /// A paint of a terrain the supplied tileset does not declare is still refused -- the ceiling
    /// moved to the tileset, it did not disappear.
    #[test]
    fn a_paint_of_a_terrain_the_tileset_does_not_declare_is_still_refused() {
        let dir = scratch_dir("map-paint-terrainceiling");
        let input = dir.join("in.scn");
        let tileset = dir.join("fixture.til");
        fs::write(&input, grass_map(11, 5)).unwrap();
        fs::write(&tileset, CLI_FIXTURE_TILESET).unwrap();

        let output = dir.join("out.scn");
        let error = edit_map(
            &input,
            MapEdit::PaintTerrain {
                rect: (5, 2, 5, 2),
                terrain_type: 37,
                selector: TileSelector::LowestSlot,
            },
            &output,
            Some(&tileset),
        )
        .unwrap_err();
        assert!(error.contains("no tiles for terrain type 37"), "{error}");
        assert!(!output.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn terrain_types_are_accepted_by_number_and_by_script_name() {
        assert_eq!(parse_terrain_type("1").unwrap(), 1);
        assert_eq!(parse_terrain_type("tt_water").unwrap(), 1);
        assert_eq!(parse_terrain_type("water").unwrap(), 1);
        assert_eq!(parse_terrain_type("WATER").unwrap(), 1);
        // Both names of a two-named type reach the same type.
        assert_eq!(parse_terrain_type("dirt").unwrap(), 0);
        assert_eq!(parse_terrain_type("rough").unwrap(), 0);
        assert!(parse_terrain_type("11").is_err());
        assert!(parse_terrain_type("tt_nonsense").is_err());
    }

    #[test]
    fn elevation_rejects_the_values_no_corpus_cell_holds() {
        assert_eq!(parse_elevation("2.5").unwrap(), 2.5);
        assert!(parse_elevation("nan").is_err());
        assert!(parse_elevation("inf").is_err());
        assert!(parse_elevation("high").is_err());
    }

    /// The loose `map/` directory has no backup, so writing over the file being read is the one
    /// mistake that cannot be undone.
    #[test]
    fn editing_refuses_to_write_over_its_own_input() {
        let dir = scratch_dir("map-in-place");
        let input = dir.join("m.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();
        let before = fs::read(&input).unwrap();

        let error = edit_map(
            &input,
            MapEdit::SetTile {
                x: 0,
                y: 0,
                tile_index: 392,
            },
            &input,
            None,
        )
        .unwrap_err();
        assert!(error.contains("refusing to write to the input file"), "{error}");
        assert_eq!(fs::read(&input).unwrap(), before);

        // Reached by a different spelling of the same file, too.
        let indirect = dir.join(".").join("m.scn");
        // Assert the *message*, not just is_err: `create_new` also fails here, so a bare is_err
        // would still pass with `paths_are_same_file` deleted outright -- the one test covering the
        // guard's unique contribution could not fail on it.
        let error = edit_map(
            &indirect,
            MapEdit::SetTile {
                x: 0,
                y: 0,
                tile_index: 392,
            },
            &input,
            None,
        )
        .unwrap_err();
        assert!(error.contains("refusing to write to the input file"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Two hardlinks to one inode are the same file, and the old canonical-path comparison said
    /// they were not. No overwrite was reachable -- `create_new` refuses either way -- but the
    /// guard did not do what its name claimed, which is the mismatch class this branch already got
    /// caught on twice.
    #[test]
    fn two_hardlinks_to_one_inode_are_recognised_as_the_same_file() {
        let dir = scratch_dir("map-hardlink");
        let input = dir.join("in.scn");
        let alias = dir.join("alias.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();
        fs::hard_link(&input, &alias).unwrap();

        assert!(super::paths_are_same_file(&input, &alias));
        let error = edit_map(
            &input,
            MapEdit::SetTile {
                x: 0,
                y: 0,
                tile_index: 392,
            },
            &alias,
            None,
        )
        .unwrap_err();
        assert!(error.contains("refusing to write to the input file"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The cell diff is the verifier's second witness. A single-cell edit that strayed must be
    /// refused before anything is written.
    #[test]
    fn a_single_cell_edit_that_touched_more_than_one_cell_is_refused() {
        let source = editable_map(5, 3);
        let before = MapAsset::parse(&source).unwrap();
        let mut after = MapAsset::parse(&source).unwrap();
        after.set_tile(3, 2, 392).unwrap();
        // The edit that was asked for is fine.
        super::verify_single_cell_edit(&before, &after, 3, 2).unwrap();
        // A stray second write is not.
        after.set_tile(0, 0, 392).unwrap();
        let error = super::verify_single_cell_edit(&before, &after, 3, 2).unwrap_err();
        assert!(error.contains("changed 2 cells"), "{error}");
        // Nor is landing on the wrong cell.
        let mut wrong = MapAsset::parse(&source).unwrap();
        wrong.set_tile(0, 0, 392).unwrap();
        let error = super::verify_single_cell_edit(&before, &wrong, 3, 2).unwrap_err();
        assert!(error.contains("not the cell 13"), "{error}");
    }

    #[test]
    fn editing_refuses_to_overwrite_an_existing_output() {
        let dir = scratch_dir("map-overwrite");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();
        fs::write(&output, b"precious").unwrap();

        let error = edit_map(
            &input,
            MapEdit::SetTile {
                x: 0,
                y: 0,
                tile_index: 392,
            },
            &output,
            None,
        )
        .unwrap_err();
        assert!(error.contains("could not create"), "{error}");
        assert_eq!(fs::read(&output).unwrap(), b"precious");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_written_map_reads_the_edit_back_at_the_right_cell() {
        let dir = scratch_dir("map-write");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        edit_map(
            &input,
            MapEdit::SetTerrain {
                x: 3,
                y: 2,
                terrain_type: 1,
            },
            &output,
            None,
        )
        .unwrap();

        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(written.cell(3, 2).unwrap().tile_index(), 392);
        // (2, 3) is off this 5x3 map entirely, which is the point of a non-square fixture.
        assert!(written.cell(2, 3).is_none());
        assert_eq!(
            written.cells.iter().filter(|c| c.tile_index() == 392).count(),
            1
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_edit_leaves_no_output_file_behind() {
        let dir = scratch_dir("map-refused");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        assert!(
            edit_map(
                &input,
                MapEdit::SetTile {
                    x: 2,
                    y: 4,
                    tile_index: 392
                },
                &output,
            None,
        )
            .is_err()
        );
        assert!(
            !output.exists(),
            "an edit that could not be applied must not leave a partial file"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A tileset fixture written to a scratch file, so the CLI's own loading path is exercised.
    ///
    /// **No game asset is committed.** `.til` files are proprietary; this is a synthetic tileset
    /// built to be unlike the shipped one -- a 7x3 atlas, a three-tile grass interior, a one-tile
    /// stone interior, and asymmetric edge constraints -- for the same reason the library fixture
    /// is.
    const CLI_FIXTURE_TILESET: &[u8] = br#"
LBM=fixture.lbm
TILES= 12, 3
TILESIZE= 8, 8
TERRAINTYPE= 1, 40, "grass",  0, 100, 200, 11, 12, 5, 13, 14
TERRAINTYPE= 2, 41, "stone",  2, 300, 400, 21, 22, 9, 23, 24
TERRAINTYPE= 3, 42, "path",   0, 0, 9999, 0, 0, 1, 0, 0
TERRAINTYPE= 4, 43, "dust",   0, 0, 9999, 0, 0, 1, 0, 0
;         self, n,    ne,  e,    se,  s,    sw,  w,    nw,   index
TILE=  0,    1, 1,    1,   1,    1,   1,    1,   1,    1,    99
TILE=  1,    1, 1,    1,   1,    1,   1,    1,   1,    1,    99
TILE=  2,    1, 1,    1,   1,    1,   1,    1,   1,    1,    99
TILE=  3,    1, 2,    *,   1,    *,   1|3,  *,   1,    *,    3
TILE=  4,    1, 1,    *,   1,    *,   2,    *,   1,    *,    4
TILE=  5,    1, 1,    *,   1,    *,   1,    *,   2,    *,    5
TILE=  6,    1, 1,    *,   2,    *,   1,    *,   1,    *,    6
TILE=  7,    1, 1,    1,   1,    1,   1,    1,   1,    2,    7
TILE=  8,    1, 1,    2,   1,    1,   1,    1,   1,    1,    8
TILE=  9,    1, 1,    1,   1,    1,   1,    2,   1,    1,    9
TILE= 10,    1, 1,    1,   1,    2,   1,    1,   1,    1,    10
TILE= 11,    2, ~1,   ~1,  ~1,   ~1,  ~1,   ~1,  ~1,   ~1,   11
TILE= 12,    2, 1,    1,   1,    1,   1,    1,   1,    1,    5
TILE= 13,    2, 1,    1,   2,    1,   1,    1,   1,    1,    13
TILE= 14,    2, 1,    1,   1,    1,   1,    1,   2,    1,    14
TILE= 15,    1, 2,    2,   1,    1,   1,    2,   2,    2,    15
TILE= 16,    1, 2,    2,   2,    2,   1,    1,   1,    2,    16
TILE= 17,    1, 1,    1,   1,    2,   2,    2,   2,    2,    17
TILE= 18,    1, 1,    2,   2,    2,   2,    2,   1,    1,    18
TILE= 19,    2, 1,    *,   2,    *,   2,    *,   2,    *,    19
TILE= 20,    2, 2,    *,   2,    *,   1,    *,   2,    *,    20
TILE= 21,    2, 2,    *,   2,    *,   2,    *,   1,    *,    21
TILE= 22,    2, 2,    *,   1,    *,   2,    *,   2,    *,    22
TILE= 23,    2, 2,    2,   2,    1,   2,    2,   2,    2,    23
TILE= 24,    2, 2,    2,   2,    2,   2,    1,   2,    2,    24
TILE= 25,    2, 2,    1,   2,    2,   2,    2,   2,    2,    25
TILE= 26,    2, 2,    2,   2,    2,   2,    2,   2,    1,    26
TILE= 30,    4, *,    *,   *,    *,   *,    *,   *,    *,    30
TILE= 31,    4, *,    *,   *,    *,   *,    *,   *,    *,    31
TILE= 32,    4, *,    *,   *,    *,   *,    *,   *,    *,    32
"#;

    /// A map of uniform grass, so the CLI paint has a field the fixture tileset can read.
    fn grass_map(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x6f_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        for index in 0..width * height {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&(index as f32).to_le_bytes());
        }
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes
    }

    /// The CLI is the layer that writes into a game directory with no backup, so the paint verb
    /// gets its own end-to-end test and not only a library one.
    ///
    /// Read back at computed byte offsets on a **non-square** 11x3 map. `N` and `S` take different
    /// tiles and so do `W`/`E` and each diagonal pair, so a mirrored direction convention fails
    /// here rather than passing by symmetry.
    #[test]
    fn painting_through_the_cli_re_selects_the_ring_from_the_tileset() {
        let dir = scratch_dir("map-paint");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        let tileset = dir.join("fixture.til");
        fs::write(&input, grass_map(11, 5)).unwrap();
        fs::write(&tileset, CLI_FIXTURE_TILESET).unwrap();

        edit_map(
            &input,
            MapEdit::PaintTerrain {
                rect: (5, 2, 5, 2),
                terrain_type: 2,
                selector: TileSelector::LowestSlot,
            },
            &output,
            Some(&tileset),
        )
        .unwrap();

        let bytes = fs::read(&output).unwrap();
        let tag_at = |x: u32, y: u32| {
            let offset = 16 + ((y * 11 + x) as usize) * 8;
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
        };
        assert_eq!(tag_at(5, 2), 12, "region");
        assert_eq!(tag_at(5, 1), 4, "N");
        assert_eq!(tag_at(5, 3), 3, "S");
        assert_eq!(tag_at(4, 2), 6, "W");
        assert_eq!(tag_at(6, 2), 5, "E");
        assert_eq!(tag_at(4, 1), 10, "NW");
        assert_eq!(tag_at(6, 1), 9, "NE");
        assert_eq!(tag_at(4, 3), 8, "SW");
        assert_eq!(tag_at(6, 3), 7, "SE");
        // Everything two cells out is still background, and nothing was ambiguous: the whole
        // footprint was determined by the tileset alone.
        for y in 0..5 {
            for x in [0, 1, 2, 3, 7, 8, 9, 10] {
                assert_eq!(tag_at(x, y), 0, "({x}, {y})");
            }
        }
        for x in 0..11 {
            assert_eq!(tag_at(x, 0), 0, "row 0 x={x}");
            assert_eq!(tag_at(x, 4), 0, "row 4 x={x}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// A refused paint must leave no file at all, like every other refused edit: the output
    /// directory is the game's own `map/`.
    #[test]
    fn a_refused_paint_writes_no_file() {
        let dir = scratch_dir("map-paint-refused");
        let input = dir.join("in.scn");
        let tileset = dir.join("fixture.til");
        fs::write(&input, grass_map(11, 5)).unwrap();
        fs::write(&tileset, CLI_FIXTURE_TILESET).unwrap();

        // No tileset at all: the map does not record which one it was authored against, so there
        // is nothing to fall back to.
        let output = dir.join("no-tileset.scn");
        let error = edit_map(
            &input,
            MapEdit::PaintTerrain {
                rect: (5, 1, 5, 1),
                terrain_type: 2,
                selector: TileSelector::LowestSlot,
            },
            &output,
            None,
        )
        .unwrap_err();
        assert!(error.contains("no tileset was supplied"), "{error}");
        assert!(!output.exists());

        // A terrain the tileset does not declare.
        let output = dir.join("unknown-terrain.scn");
        let error = edit_map(
            &input,
            MapEdit::PaintTerrain {
                rect: (5, 1, 5, 1),
                terrain_type: 9,
                selector: TileSelector::LowestSlot,
            },
            &output,
            Some(&tileset),
        )
        .unwrap_err();
        assert!(error.contains("no tiles for terrain type 9"), "{error}");
        assert!(!output.exists());

        // A rectangle off the short axis of a non-square map.
        let output = dir.join("outside.scn");
        let error = edit_map(
            &input,
            MapEdit::PaintTerrain {
                rect: (1, 1, 1, 6),
                terrain_type: 2,
                selector: TileSelector::LowestSlot,
            },
            &output,
            Some(&tileset),
        )
        .unwrap_err();
        assert!(error.contains("outside this 11x5 map"), "{error}");
        assert!(!output.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The gamescript pairing is enforced by **map name**, and the same bytes under three tileset
    /// names get three answers.
    ///
    /// The map is called `aicave.smp` because the scripts pair that map with `aibldg01.til`. So
    /// the fixture tileset is written three times -- as `aibldg01.til` (the pairing), as
    /// `tilesa01.til` (what the previous version of this code recommended, and a real mismatch),
    /// and as `mymod.til` (unshipped, presumed deliberate). Only the *name* differs, so nothing
    /// but the pairing table can decide the outcome.
    ///
    /// **The two accepted outputs are asserted byte-identical to each other.** Without that, a
    /// paint that silently did nothing, or did something different, would still pass -- which was
    /// the defect in the version of this test that shipped.
    #[test]
    fn a_combat_map_accepts_its_paired_tileset_refuses_another_shipped_one_and_allows_a_modded_one()
    {
        let dir = scratch_dir("map-paint-pairing");
        let input = dir.join("aicave.smp");
        fs::write(&input, grass_map(11, 5)).unwrap();

        let paint = |tileset: &std::path::Path, output: &std::path::Path| {
            edit_map(
                &input,
                MapEdit::PaintTerrain {
                    rect: (5, 2, 5, 2),
                    terrain_type: 2,
                    selector: TileSelector::LowestSlot,
                },
                output,
                Some(tileset),
            )
        };

        // The gamescript's own pairing: accepted. This is the case the shipped version refused.
        let paired = dir.join("aibldg01.til");
        fs::write(&paired, CLI_FIXTURE_TILESET).unwrap();
        let paired_out = dir.join("paired.smp");
        paint(&paired, &paired_out).unwrap();

        // A different shipped tileset: refused, nothing written.
        let wrong = dir.join("tilesa01.til");
        fs::write(&wrong, CLI_FIXTURE_TILESET).unwrap();
        let wrong_out = dir.join("wrong.smp");
        let error = paint(&wrong, &wrong_out).unwrap_err();
        assert!(error.contains("tilesa01.til is a shipped tileset"), "{error}");
        assert!(error.contains("aibldg01.til"), "{error}");
        assert!(!wrong_out.exists(), "a refused paint must leave no file");

        // An unshipped name: accepted, not second-guessed.
        let modded = dir.join("mymod.til");
        fs::write(&modded, CLI_FIXTURE_TILESET).unwrap();
        let modded_out = dir.join("modded.smp");
        paint(&modded, &modded_out).unwrap();

        // Identical tileset bytes under two accepted names must produce identical maps, and the
        // paint must actually have changed something.
        let paired_bytes = fs::read(&paired_out).unwrap();
        let modded_bytes = fs::read(&modded_out).unwrap();
        assert_eq!(
            paired_bytes, modded_bytes,
            "the same tileset under two accepted names must paint identically"
        );
        assert_ne!(
            paired_bytes,
            grass_map(11, 5),
            "the paint must change the map, or this test cannot fail on the paint being a no-op"
        );
        let tag_at = |bytes: &[u8], x: u32, y: u32| {
            let offset = 16 + ((y * 11 + x) as usize) * 8;
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
        };
        assert_eq!(tag_at(&paired_bytes, 5, 2), 12, "region");
        assert_eq!(tag_at(&paired_bytes, 5, 1), 4, "N");

        // An **unbound** combat map accepts any shipped tileset, because nothing is known about it.
        let unbound = dir.join("aibrks0.smp");
        fs::write(&unbound, grass_map(11, 5)).unwrap();
        let unbound_out = dir.join("unbound.smp");
        edit_map(
            &unbound,
            MapEdit::PaintTerrain {
                rect: (5, 2, 5, 2),
                terrain_type: 2,
                selector: TileSelector::LowestSlot,
            },
            &unbound_out,
            Some(&wrong),
        )
        .unwrap();
        assert_eq!(fs::read(&unbound_out).unwrap(), paired_bytes);

        // A world map still refuses a shipped combat tileset.
        let world = dir.join("world.scn");
        fs::write(&world, grass_map(11, 5)).unwrap();
        let world_out = dir.join("world-out.scn");
        let error = edit_map(
            &world,
            MapEdit::PaintTerrain {
                rect: (5, 2, 5, 2),
                terrain_type: 2,
                selector: TileSelector::LowestSlot,
            },
            &world_out,
            Some(&wrong),
        )
        .unwrap_err();
        assert!(error.contains("world map"), "{error}");
        assert!(error.contains("tilesb01.til"), "{error}");
        assert!(!world_out.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A paint whose **output** path is a different map class than its input is refused.
    ///
    /// Found by review: `--map-paint-terrain realm.scn ... battle.smp tilesb01.til` was accepted,
    /// writing a world map under a name the tool itself then reported as a combat map read through
    /// something else. Only the input was classified. The `.smp`-to-`.scn` direction is asserted
    /// too, so a check that only looked one way fails here.
    #[test]
    fn painting_across_map_classes_is_refused_on_the_output_extension() {
        let dir = scratch_dir("map-paint-crossclass");
        let tileset = dir.join("fixture.til");
        fs::write(&tileset, CLI_FIXTURE_TILESET).unwrap();
        let edit = MapEdit::PaintTerrain {
            rect: (5, 2, 5, 2),
            terrain_type: 2,
            selector: TileSelector::LowestSlot,
        };

        let world = dir.join("realm.scn");
        fs::write(&world, grass_map(11, 5)).unwrap();
        let combat_out = dir.join("battle.smp");
        let error = edit_map(&world, edit, &combat_out, Some(&tileset)).unwrap_err();
        assert!(error.contains("refusing to paint"), "{error}");
        assert!(error.contains("world map"), "{error}");
        assert!(error.contains("combat map"), "{error}");
        assert!(!combat_out.exists());

        // And the other direction.
        let combat = dir.join("battle2.smp");
        fs::write(&combat, grass_map(11, 5)).unwrap();
        let world_out = dir.join("realm2.scn");
        let error = edit_map(&combat, edit, &world_out, Some(&tileset)).unwrap_err();
        assert!(error.contains("refusing to paint"), "{error}");
        assert!(!world_out.exists());

        // An unclassified output extension is refused against a classified input too.
        let junk_out = dir.join("out.dat");
        let error = edit_map(&world, edit, &junk_out, Some(&tileset)).unwrap_err();
        assert!(error.contains("unclassified extension"), "{error}");
        assert!(!junk_out.exists());

        // Matching classes still paint.
        let same_out = dir.join("realm3.scn");
        edit_map(&world, edit, &same_out, Some(&tileset)).unwrap();
        assert!(same_out.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Omitting the tileset refuses, and the refusal says what the gamescript says -- including
    /// when the gamescript says nothing.
    ///
    /// Three different messages for three resolution states, on real table entries. A rule that
    /// resolved every combat map to one tileset could not pass this.
    #[test]
    fn the_missing_tileset_refusal_reports_the_gamescript_binding_or_says_it_is_unresolved() {
        let dir = scratch_dir("map-paint-noset");
        let edit = MapEdit::PaintTerrain {
            rect: (5, 2, 5, 2),
            terrain_type: 2,
            selector: TileSelector::LowestSlot,
        };

        // A single-valued binding is named.
        let bound = dir.join("aicave.smp");
        fs::write(&bound, grass_map(11, 5)).unwrap();
        let output = dir.join("a.smp");
        let error = edit_map(&bound, edit, &output, None).unwrap_err();
        assert!(error.contains("combat map"), "{error}");
        assert!(error.contains("aibldg01.til"), "{error}");
        assert!(!error.contains("tilesa01.til"), "{error}");
        assert!(!output.exists());

        // An ambiguous map says so, and names every candidate.
        let ambiguous = dir.join("licave.smp");
        fs::write(&ambiguous, grass_map(11, 5)).unwrap();
        let output = dir.join("b.smp");
        let error = edit_map(&ambiguous, edit, &output, None).unwrap_err();
        assert!(error.contains("no single answer"), "{error}");
        assert!(error.contains("libldg01.til"), "{error}");
        assert!(error.contains("wabldg01.til"), "{error}");
        assert!(!output.exists());

        // An unbound map refuses without naming any tileset at all.
        let unbound = dir.join("aibrks0.SMP");
        fs::write(&unbound, grass_map(11, 5)).unwrap();
        let output = dir.join("c.smp");
        let error = edit_map(&unbound, edit, &output, None).unwrap_err();
        assert!(error.contains("cannot name one and will not guess"), "{error}");
        assert!(!error.contains(".til"), "no tileset may be suggested: {error}");
        assert!(!output.exists());

        // World maps name theirs.
        let world = dir.join("realm.scn");
        fs::write(&world, grass_map(11, 5)).unwrap();
        let output = dir.join("d.scn");
        let error = edit_map(&world, edit, &output, None).unwrap_err();
        assert!(error.contains("world map"), "{error}");
        assert!(error.contains("tilesb01.til"), "{error}");
        assert!(!output.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// `--map-tileset-for` parses the map before answering, so it cannot name a tileset for a file
    /// that is not a map, and refuses an extension the corpus does not classify.
    #[test]
    fn the_tileset_for_verb_requires_a_parsable_map_and_a_classified_extension() {
        let dir = scratch_dir("map-tileset-for");

        let combat = dir.join("battle.SMP");
        fs::write(&combat, grass_map(11, 5)).unwrap();
        super::tile_set_for_map(&combat).unwrap();

        let world = dir.join("realm.scn");
        fs::write(&world, grass_map(11, 5)).unwrap();
        super::tile_set_for_map(&world).unwrap();

        // A real map under an extension with no recorded class: answered with a refusal, not with
        // a default.
        let unknown = dir.join("realm.dat");
        fs::write(&unknown, grass_map(11, 5)).unwrap();
        let error = super::tile_set_for_map(&unknown).unwrap_err();
        assert!(error.contains("not one the corpus classifies"), "{error}");

        // Not a map at all.
        let junk = dir.join("notes.smp");
        fs::write(&junk, b"not a map").unwrap();
        assert!(super::tile_set_for_map(&junk).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The verification arm has to *verify*, and this is the third time in this repository that it
    /// could not.
    ///
    /// The `FlagRegion` arm once shipped empty with a comment claiming the edit checked itself. The
    /// paint arm then shipped re-running `plan_terrain_paint` and comparing the result to itself,
    /// which no planner defect can fail, followed by two loops asserting things `candidates()`
    /// already guarantees. So each case below names the input that breaks the check it stands for,
    /// and **the last one is the case every earlier version accepted**: a tile of the right terrain,
    /// present in the tileset, whose own declared constraints the written neighbourhood violates.
    #[test]
    fn the_paint_verifier_rejects_a_tile_whose_own_constraints_the_map_violates() {
        let tile_set = TileSetDefinition::parse(CLI_FIXTURE_TILESET).unwrap();
        let source = grass_map(11, 5);
        let before = MapAsset::parse(&source).unwrap();
        let edit = MapEdit::PaintTerrain {
            rect: (5, 2, 5, 2),
            terrain_type: 2,
            selector: TileSelector::LowestSlot,
        };
        let painted = || {
            let mut map = MapAsset::parse(&source).unwrap();
            map.paint_terrain((5, 2, 5, 2), 2, &tile_set, TileSelector::LowestSlot)
                .unwrap();
            map
        };

        super::verify_map_edit(&painted(), &before, edit, Some(&tile_set)).unwrap();

        // **The check that could not fail before.** Tile 3 is grass, is in the tileset, and is what
        // the old two loops tested for -- but it declares terrain 2 to its north, and the written
        // map has grass there. Nothing about the terrain or the atlas is wrong; only the constraint
        // is, which is exactly what a blend writer gets wrong.
        let mut illegal = painted();
        illegal.set_tile(5, 1, 3).unwrap();
        let error =
            super::verify_map_edit(&illegal, &before, edit, Some(&tile_set)).unwrap_err();
        assert!(error.contains("tile 3 at (5, 1) requires n"), "{error}");
        assert!(error.contains("terrain 1 there"), "{error}");

        // A region cell holding a tile of the wrong terrain.
        let mut wrong_terrain = painted();
        wrong_terrain.set_tile(5, 2, 0).unwrap();
        let error =
            super::verify_map_edit(&wrong_terrain, &before, edit, Some(&tile_set)).unwrap_err();
        assert!(
            error.contains("(5, 2) is inside the painted rectangle but holds terrain 1"),
            "{error}"
        );

        // A cell nowhere near anything whose terrain moved.
        let mut stray = painted();
        stray.set_tile(0, 0, 1).unwrap();
        let error = super::verify_map_edit(&stray, &before, edit, Some(&tile_set)).unwrap_err();
        assert!(error.contains("neither in the painted rectangle nor beside"), "{error}");

        // A tile the tileset does not declare at all.
        let mut foreign = painted();
        foreign.set_tile(5, 1, 28).unwrap();
        let error = super::verify_map_edit(&foreign, &before, edit, Some(&tile_set)).unwrap_err();
        assert!(error.contains("tile 28 at (5, 1) is not declared"), "{error}");

        // Verifying without the tileset the edit used refuses rather than passing vacuously.
        let error = super::verify_map_edit(&painted(), &before, edit, None).unwrap_err();
        assert!(error.contains("no tileset was supplied"), "{error}");
    }

    /// The verifier does not consult the planner, so a broken planner cannot verify itself.
    ///
    /// Hand it a map that is *internally* legal by the tileset but is not what a paint of this
    /// rectangle would produce: the rectangle simply was not painted. A verifier that re-plans and
    /// compares to its own plan reports nothing here, because it would find the same plan and the
    /// same map. This one reports that the rectangle does not hold the painted terrain.
    #[test]
    fn the_paint_verifier_does_not_ask_the_planner_what_the_answer_was() {
        let tile_set = TileSetDefinition::parse(CLI_FIXTURE_TILESET).unwrap();
        let source = grass_map(11, 5);
        let before = MapAsset::parse(&source).unwrap();
        let edit = MapEdit::PaintTerrain {
            rect: (5, 2, 5, 2),
            terrain_type: 2,
            selector: TileSelector::LowestSlot,
        };
        // Untouched: every cell is grass interior tile 0, which satisfies its own constraints
        // everywhere, so no constraint check can object. Only "the rectangle is not terrain 2" can.
        let untouched = MapAsset::parse(&source).unwrap();
        let error = super::verify_map_edit(&untouched, &before, edit, Some(&tile_set)).unwrap_err();
        assert!(
            error.contains("(5, 2) is inside the painted rectangle but holds terrain 1, not 2"),
            "{error}"
        );
    }

    /// `--map-set-terrain` is not replaced. Painting is an addition, and the single-cell
    /// `forcetexture` behaviour is the only one that works where the background cannot be read.
    #[test]
    fn setting_one_cell_still_touches_exactly_one_cell() {
        let dir = scratch_dir("map-set-terrain-unchanged");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(11, 3)).unwrap();

        edit_map(
            &input,
            MapEdit::SetTerrain { x: 4, y: 1, terrain_type: 1 },
            &output,
            None,
        )
        .unwrap();
        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(
            written.cells.iter().filter(|cell| cell.tile_index() != 15).count(),
            1,
            "forcetexture semantics: one cell, no ring"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_tripping_a_directory_with_no_maps_is_not_a_pass() {
        let dir = scratch_dir("map-roundtrip-empty");
        let error = roundtrip_maps(&dir).unwrap_err();
        assert!(error.contains("no map files were checked"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_tripping_an_untouched_map_reports_it_identical() {
        let dir = scratch_dir("map-roundtrip");
        let input = dir.join("in.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();
        assert!(roundtrip_maps(&input).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_tripping_reports_a_writer_that_does_not_reproduce_its_input() {
        let dir = scratch_dir("map-roundtrip-bad");
        let input = dir.join("in.scn");
        // A map with a trailing byte the parser keeps but no decoder claims; if `to_bytes` ever
        // stops emitting `trailing_raw`, this is what notices.
        let mut source = editable_map(5, 3);
        source.push(0x7f);
        fs::write(&input, &source).unwrap();
        // It still round-trips -- that is the assertion. The tail is opaque, not dropped.
        assert!(roundtrip_maps(&input).is_ok());
        let map = MapAsset::parse(&source).unwrap();
        assert!(map.placed_sprites.is_none());
        assert_eq!(map.to_bytes().unwrap(), source);
        let _ = fs::remove_dir_all(&dir);
    }

    // --- the verbs added for the mapload probe -------------------------------------------
    //
    // These write into the game's no-backup map/ directory like every other edit verb, and they
    // shipped without CLI tests. A reviewer pointed out that the layer which actually touches the
    // game directory was the untested one, and that this is why the FlagRegion verification gap
    // was invisible.

    /// The two listing verbs had no test at all, which the previous review already flagged as a
    /// pattern on this project. They only print, so the risk is low -- but the header used to be a
    /// hand-written literal that would have silently disagreed with the table.
    #[test]
    fn the_listing_verbs_run_and_derive_their_header_from_the_table() {
        sprite_types().unwrap();
        transition_rings().unwrap();
        // The header is built from TRANSITION_RING_OFFSETS, so it cannot drift from it.
        let offsets: Vec<i32> = TRANSITION_RING_OFFSETS.iter().map(|e| e.offset).collect();
        assert_eq!(offsets, vec![-13, -14, -11, -12, 3, 4, 2, 1]);
    }

    #[test]
    fn an_id_inside_a_table_gap_is_not_called_unregistered() {
        let dir = scratch_dir("map-gapid");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        // 105 sits inside the 95..118 gap, which the shipped corpus uses heavily -- those are the
        // per-faith types the arrays hold, not runtime registrations.
        edit_map(
            &input,
            MapEdit::PlaceSprite { x: 3, y: 2, sprite_type: 105 },
            &output,
            None,
        )
        .unwrap();
        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(written.placed_sprites.as_ref().unwrap().records[0].sprite_type, 105);
        assert_eq!(terrain_sprite_name(105), None, "105 is a gap, not a name");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sprite_type_can_be_named_or_numbered() {
        assert_eq!(parse_sprite_type("castle1").unwrap(), 0);
        assert_eq!(parse_sprite_type("CASTLE1").unwrap(), 0);
        // A raw id is still accepted, and NOT range-checked: ids above the table are runtime
        // registrations by addterrainspritetype, which is how the probe's own 470 exists.
        assert_eq!(parse_sprite_type("470").unwrap(), 470);
        assert_eq!(parse_sprite_type("0").unwrap(), 0);
        // A near miss suggests, a miss points at the listing.
        let error = parse_sprite_type("castl").unwrap_err();
        assert!(error.contains("castle1"), "{error}");
        let error = parse_sprite_type("zzzz").unwrap_err();
        assert!(error.contains("--map-sprite-types"), "{error}");
    }

    #[test]
    fn placing_a_sprite_by_name_writes_the_registered_id() {
        let dir = scratch_dir("map-spritename");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        edit_map(
            &input,
            MapEdit::PlaceSprite {
                x: 3,
                y: 2,
                sprite_type: parse_sprite_type("castle1").unwrap(),
            },
            &output,
            None,
        )
        .unwrap();

        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        let record = &written.placed_sprites.as_ref().unwrap().records[0];
        assert_eq!(record.sprite_type, 0, "castle1 is type 0");
        assert_eq!(written.record_coordinates(record), (3, 2));
        assert_eq!(terrain_sprite_name(record.sprite_type), Some("castle1"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn creating_a_map_writes_a_file_the_parser_accepts() {
        let dir = scratch_dir("map-create");
        let output = dir.join("new.scn");
        create_map(96, 64, 6, &output).unwrap();

        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        assert_eq!((written.width, written.height), (96, 64));
        assert_eq!(written.metadata, GENERATED_HEADER_WORD);
        assert!(written.cells.iter().all(|cell| cell.tile_index() == 15));
        assert!(written.cells.iter().all(|cell| !cell.high_flag_set()));
        assert_eq!(written.placed_sprites.as_ref().unwrap().records.len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn creating_a_map_refuses_bad_input_and_an_existing_output() {
        let dir = scratch_dir("map-create-refuse");
        let output = dir.join("new.scn");
        assert!(create_map(0, 64, 6, &output).is_err());
        assert!(create_map(64, 64, 11, &output).is_err());
        // Bounded rather than allocating: this used to die in the allocator.
        assert!(create_map(100_000, 100_000, 6, &output).is_err());
        assert!(!output.exists(), "a refused create must leave no file");

        fs::write(&output, b"precious").unwrap();
        let error = create_map(64, 64, 6, &output).unwrap_err();
        assert!(error.contains("could not create"), "{error}");
        assert_eq!(fs::read(&output).unwrap(), b"precious");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewriting_reproduces_the_input_bytes_without_changing_a_cell() {
        let dir = scratch_dir("map-rewrite");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        let source = editable_map(5, 3);
        fs::write(&input, &source).unwrap();

        edit_map(&input, MapEdit::Rewrite, &output, None).unwrap();
        assert_eq!(fs::read(&output).unwrap(), source, "a rewrite must be byte-exact");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn setting_the_high_flag_through_the_cli_keeps_the_tile() {
        let dir = scratch_dir("map-highflag");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        edit_map(
            &input,
            MapEdit::SetHighFlag { x: 3, y: 2, set: true },
            &output,
            None,
        )
        .unwrap();
        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        let cell = written.cell(3, 2).unwrap();
        assert!(cell.high_flag_set());
        assert_eq!(cell.tile_index(), 15, "the tile must survive the flag");
        assert_eq!(
            written.cells.iter().filter(|c| c.high_flag_set()).count(),
            1,
            "exactly one cell may change"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The region verbs are the only ones that write many cells at once, and their verification
    /// arm was empty with a comment claiming otherwise. This is the test that arm needed.
    #[test]
    fn flagging_a_rectangle_touches_exactly_that_rectangle() {
        let dir = scratch_dir("map-flagrect");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        edit_map(
            &input,
            MapEdit::FlagRegion {
                border: false,
                rect: Some((1, 1, 2, 1)),
            },
            &output,
            None,
        )
        .unwrap();

        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        for y in 0..3 {
            for x in 0..5 {
                let flagged = written.cell(x, y).unwrap().high_flag_set();
                let inside = y == 1 && (1..=2).contains(&x);
                assert_eq!(flagged, inside, "({x}, {y}) flagged={flagged}");
                assert_eq!(written.cell(x, y).unwrap().tile_index(), 15);
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn flagging_the_border_hits_the_ring_and_nothing_inside_it() {
        let dir = scratch_dir("map-flagborder");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        edit_map(
            &input,
            MapEdit::FlagRegion { border: true, rect: None },
            &output,
            None,
        )
        .unwrap();

        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        // On a 5x3 map only (1,1), (2,1) and (3,1) are interior.
        assert_eq!(written.border_ring().len(), 12);
        for x in 1..=3 {
            assert!(!written.cell(x, 1).unwrap().high_flag_set(), "({x}, 1)");
        }
        assert_eq!(
            written.cells.iter().filter(|c| c.high_flag_set()).count(),
            12
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rectangle_outside_the_map_is_refused_and_writes_nothing() {
        let dir = scratch_dir("map-flagbad");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        // Reversed, and off the map on the axis a square fixture would hide.
        for rect in [(2, 1, 1, 1), (0, 0, 4, 4), (0, 0, 9, 2)] {
            assert!(
                edit_map(
                    &input,
                    MapEdit::FlagRegion { border: false, rect: Some(rect) },
                    &output,
            None,
        )
                .is_err(),
                "{rect:?} should have been refused"
            );
            assert!(!output.exists(), "{rect:?} left a file behind");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn placing_a_sprite_through_the_cli_writes_a_record_at_the_y_major_cell() {
        let dir = scratch_dir("map-sprite");
        let input = dir.join("in.scn");
        let output = dir.join("out.scn");
        fs::write(&input, editable_map(5, 3)).unwrap();

        edit_map(
            &input,
            MapEdit::PlaceSprite {
                x: 3,
                y: 2,
                sprite_type: 470,
            },
            &output,
            None,
        )
        .unwrap();

        let written = MapAsset::parse(&fs::read(&output).unwrap()).unwrap();
        let section = written.placed_sprites.as_ref().unwrap();
        assert_eq!(section.records.len(), 1);
        assert_eq!(section.records[0].cell_index, 2 * 5 + 3);
        assert_eq!(section.records[0].instance_id, 200);
        assert_eq!(section.records[0].sprite_type, 470);
        assert_eq!(written.record_coordinates(&section.records[0]), (3, 2));
        let _ = fs::remove_dir_all(&dir);
    }


    #[test]
    fn a_stub_value_spelled_as_an_infinity_is_refused_rather_than_pushed() {
        // `--stub` takes a value from a person, so the lexer's predicate is what decides whether
        // it is a number; `f64::from_str` would have made every one of these an infinity or a NaN
        // on the stub's stack, silently.
        for spelling in [
            "inf", "INF", "Inf", "infinity", "INFINITY", "nan", "NaN", "NAN", "+inf", "-INF",
        ] {
            assert!(
                spelling.parse::<f64>().is_ok(),
                "{spelling} is meant to be the hole f64::from_str leaves open"
            );
            let error = parse_native_stub(&format!("getgold={spelling}"))
                .expect_err("a spelled-out infinity is not a stub value");
            assert!(
                error.contains(spelling),
                "the error should quote {spelling}"
            );
        }
        let (name, value) = parse_native_stub("getgold=-12.5").expect("a real is a stub value");
        assert_eq!(name, "getgold");
        assert_eq!(value, GameScriptValue::Number(-12.5));
        assert_eq!(
            parse_native_stub("getmultiplayerflag=false")
                .expect("a boolean is a stub value")
                .1,
            GameScriptValue::Boolean(false)
        );
    }
}
