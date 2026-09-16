use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use lom_asset_viewer::asset::{AssetKind, probe};
use lom_asset_viewer::gamescript::GameScriptDocument;
use lom_asset_viewer::gamescript_vm::{
    GameScriptVm, GameScriptVmError, Value as GameScriptValue,
};
use lom_asset_viewer::imp;
use lom_asset_viewer::imp::{ImpHeaderStats, ImpSprite};
use lom_asset_viewer::map::MapAsset;
use lom_asset_viewer::mpq::{Archive, Entry};
use lom_asset_viewer::native_table;
use lom_asset_viewer::operator_arity;
use lom_asset_viewer::pbm::PbmImage;
use lom_asset_viewer::png_export::{write_imp_frame_png, write_pbm_png, write_rgba_png};
use lom_asset_viewer::tile::TileSetDefinition;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode};
use sdl3::video::Window;

const WINDOW_WIDTH: u32 = 1100;
const WINDOW_HEIGHT: u32 = 800;
const TERRAIN_PREVIEW_TILE_SIZE: u32 = 8;

enum Command {
    Catalog(Source),
    DescribeMap(PathBuf),
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
    ScanMapDirectory(PathBuf),
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
        Command::ScanNatives { executable, source } => {
            scan_native_table(&executable, source.as_ref())
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
    let executable = take_option(&mut args, "--exe")?.map(PathBuf::from);
    let expression = take_option(&mut args, "--eval")?;
    let hotspot = take_option(&mut args, "--hotspot")?
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|_| format!("hotspot type must be a nonnegative integer: {value}"))
        })
        .transpose()?;
    let stubs = take_repeated_option(&mut args, "--stub")
        .iter()
        .map(|specification| parse_native_stub(specification))
        .collect::<Result<Vec<_>, String>>()?;
    let first = args.first().ok_or_else(usage)?.as_str();
    match first {
        "--catalog" => {
            require_len(&args, 2)?;
            Ok(Command::Catalog(source(&args[1], listfile)))
        }
        "--describe-map" => {
            require_len(&args, 2)?;
            Ok(Command::DescribeMap(args[1].clone().into()))
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
        other => GameScriptValue::Number(other.parse::<f64>().map_err(|_| {
            format!("--stub value must be a number, true, or false, got {other}")
        })?),
    };
    Ok((name.to_owned(), value))
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
    "usage:\n  lom-asset-viewer --list ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --catalog ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan-gamescript ARCHIVE.mpq [--listfile FILE] [--exe lomse.exe]\n  lom-asset-viewer --scan-natives lomse.exe [GS.MPQ] [--listfile FILE]\n  lom-asset-viewer --probe-gamescript ARCHIVE.mpq MEMBER [--listfile FILE] [--eval SOURCE] [--stub NAME=VALUE]...\n  lom-asset-viewer --scan-map-dir DIRECTORY\n  lom-asset-viewer --describe-map FILE\n  lom-asset-viewer --validate-imp ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --describe-imp ARCHIVE.mpq MEMBER [--listfile FILE]\n  lom-asset-viewer --view-imp ARCHIVE.mpq MEMBER [FRAME] [--listfile FILE]\n  lom-asset-viewer --view-map FILE [TILESET.til TILE_ATLAS.lbm]\n  lom-asset-viewer --set-imp-placement IN.imp FRAME X Y OUT.imp [--hotspot TYPE]\n  lom-asset-viewer --imp-placement-for WIDTH HEIGHT ANCHOR_X ANCHOR_Y TOP_LEFT_X TOP_LEFT_Y\n  lom-asset-viewer --export-map-preview FILE TILESET.til TILE_ATLAS.lbm OUTPUT.png\n  lom-asset-viewer --export-imp-frame ARCHIVE.mpq MEMBER FRAME OUTPUT.png [--listfile FILE]\n  lom-asset-viewer --export-pbm ARCHIVE.mpq MEMBER OUTPUT.png [--listfile FILE]\n  lom-asset-viewer --inspect ARCHIVE.mpq [MEMBER] [--listfile FILE]\n  lom-asset-viewer --inspect-file FILE\n  lom-asset-viewer --extract ARCHIVE.mpq MEMBER OUTPUT [--listfile FILE]\n  lom-asset-viewer ARCHIVE.mpq [MEMBER] [--listfile FILE]".to_owned()
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

fn describe_map(path: &Path) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
    let section = map.placed_sprites_49.as_ref().ok_or_else(|| {
        format!(
            "{} does not use the decoded 49-byte placed-sprite record family",
            path.display()
        )
    })?;

    println!(
        "map\t{}\t{}x{}\trecords:{}\tfooter:{}",
        clean_field(&path.display().to_string()),
        map.width,
        map.height,
        section.records.len(),
        section.footer,
    );
    println!(
        "record\tcell-index\tx\ty\tinstance-id\tattribute-bits\tattribute-code\tsprite-type-candidate\tprocedure-id-candidate\traw"
    );
    for (index, record) in section.records.iter().enumerate() {
        let (x, y) = record.coordinates(map.height);
        println!(
            "{index}\t{}\t{x}\t{y}\t{}\t0x{:08x}\t{}\t{}\t{}\t{}",
            record.cell_index,
            record.instance_id,
            record.attribute_bits,
            record.attribute_code_candidate(),
            record.sprite_type_candidate,
            record.procedure_id_candidate,
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
    let mut forced_texture_cells = 0_usize;
    let mut placed_sprite_49_files = 0_usize;
    let mut placed_sprite_49_records = 0_usize;
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
            .or_insert((map.trailing_bytes, map.trailing_bytes));
        range.0 = range.0.min(map.trailing_bytes);
        range.1 = range.1.max(map.trailing_bytes);
        let layouts = map.candidate_tail_layouts();
        let layout = match layouts.as_slice() {
            [] => "unknown".to_owned(),
            [layout] => layout.to_string(),
            layouts => format!(
                "ambiguous:{}",
                layouts
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("|")
            ),
        };
        *tail_layout_counts.entry((kind, layout)).or_default() += 1;
        if let Some(section) = &map.placed_sprites_49 {
            placed_sprite_49_files += 1;
            placed_sprite_49_records += section.records.len();
            for record in &section.records {
                placed_sprite_types.insert(record.sprite_type_candidate);
                placed_sprite_attribute_codes.insert(record.attribute_code_candidate());
            }
        }
        for cell in &map.cells {
            cell_tags.insert(cell.tag);
            tile_indexes.insert(cell.tile_index_candidate());
            forced_texture_cells += usize::from(cell.forced_texture_candidate());
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
        println!("tail-layout-candidate\t{kind}\t{layout}\t{count}");
    }
    println!("distinct-cell-tags\t{}", cell_tags.len());
    println!("distinct-tile-indexes\t{}", tile_indexes.len());
    if let (Some(minimum), Some(maximum)) = (tile_indexes.first(), tile_indexes.last()) {
        println!("tile-index-range\t{minimum}..{maximum}");
    }
    println!("forced-texture-cells\t{forced_texture_cells}");
    println!("placed-sprite-49-files\t{placed_sprite_49_files}");
    println!("placed-sprite-49-records\t{placed_sprite_49_records}");
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
            Ok(info) => *kinds.entry(info.kind).or_default() += 1,
            Err(error) => failures.push((entry.name.clone(), error)),
        }
    }

    println!("archive_entries\t{}", entries.len());
    println!("readable_entries\t{readable}");
    for (kind, count) in kinds {
        println!("kind\t{kind}\t{count}");
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
                    "unknown-name-remedy\tthe engine implements this; a static site count ({confidence}) says it takes {} operand{} and returns {} result{} — a sound upper bound, not proven arity (see mul); supply it with --stub {name}=VALUE",
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
        println!("likely-hardcoded-engine-names\t{}", names.len());
        for (name, count) in names.iter().take(candidate_display_limit) {
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

fn likely_engine_names(
    path: &Path,
    executable_names: &BTreeMap<String, usize>,
    definition_names: &BTreeMap<String, usize>,
) -> Result<Vec<(String, usize)>, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read executable {}: {error}", path.display()))?;
    let binary_strings = ascii_strings(&bytes);
    // Exclude names the corpus actually DEFINES, not every name that appears as a literal.
    // The scripts push a native name and convert it to defer the call (`/invoke_spell cvx`),
    // so excluding on literal presence hid genuine host calls.
    let definition_names: BTreeSet<String> = definition_names
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    let mut candidates: Vec<_> = executable_names
        .iter()
        .filter(|(name, _)| {
            let lower = name.to_ascii_lowercase();
            !definition_names.contains(&lower) && binary_strings.contains(&lower)
        })
        .map(|(name, count)| (name.clone(), *count))
        .collect();
    candidates.sort_by(|(left_name, left_count), (right_name, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_name.cmp(right_name))
    });
    Ok(candidates)
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

fn validate_imp_archive(source: &Source) -> Result<(), String> {
    let (archive, entries) = open_archive(source)?;
    let mut pairs = BTreeMap::<String, ImpPair>::new();

    for entry in &entries {
        let lower_name = entry.name.to_ascii_lowercase();
        if let Some(stem) = lower_name.strip_suffix(".h") {
            pairs.entry(stem.to_owned()).or_default().header = Some(entry.name.clone());
        } else if let Some(stem) = lower_name.strip_suffix(".imp") {
            pairs.entry(stem.to_owned()).or_default().sprite = Some(entry.name.clone());
        }
    }

    let mut validated = 0_usize;
    let mut matched_pairs = 0_usize;
    let mut orphan_entries = 0_usize;
    let mut validation_failures = 0_usize;
    let mut failures = Vec::new();
    for (stem, pair) in &pairs {
        let (Some(header_name), Some(sprite_name)) = (&pair.header, &pair.sprite) else {
            let missing = if pair.header.is_none() { ".h" } else { ".imp" };
            failures.push(format!("{stem}: missing {missing} counterpart"));
            orphan_entries += 1;
            continue;
        };
        matched_pairs += 1;
        let result = (|| {
            let header_bytes = archive
                .read(header_name)
                .map_err(|error| error.to_string())?;
            let sprite_bytes = archive
                .read(sprite_name)
                .map_err(|error| error.to_string())?;
            let stats = ImpHeaderStats::parse(&header_bytes).map_err(|error| error.to_string())?;
            let sprite = ImpSprite::parse(&sprite_bytes).map_err(|error| error.to_string())?;
            sprite
                .validate_against(&stats)
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(()) => validated += 1,
            Err(error) => {
                validation_failures += 1;
                failures.push(format!("{stem}: {error}"));
            }
        }
    }

    println!("candidate_stems\t{}", pairs.len());
    println!("matched_pairs\t{matched_pairs}");
    println!("validated\t{validated}");
    println!("validation_failures\t{validation_failures}");
    println!("orphan_entries\t{orphan_entries}");
    println!("failures\t{}", failures.len());
    for failure in &failures {
        println!("failure\t{}", clean_field(failure));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} IMP validations or catalog pairings failed",
            failures.len()
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
            let tile_index = cell.tile_index_candidate();
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
            // those slots hold (often green and red) are incidental art-tool choices.
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
    let candidates = likely_engine_names(executable, &executable_names, &definition_names)?;

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

#[cfg(test)]
mod tests {
    use super::{parse_coordinate, parse_dimension, parse_offset, set_imp_placement};
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    use lom_asset_viewer::imp::{ImpFacing, ImpFrame, ImpSequence, ImpSprite};
    use lom_asset_viewer::map::{MapAsset, MapCell};
    use lom_asset_viewer::pbm::PbmImage;
    use lom_asset_viewer::tile::{TileDefinition, TileSetDefinition};

    use super::{
        ImpDisplayMode, MapDisplayMode, imp_display_rgba, map_display_rgba, step_imp_facing,
        step_imp_frame, step_imp_sequence, terrain_preview_rgba,
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

    #[test]
    fn map_display_modes_convert_x_major_cells_to_display_rows() {
        let map = MapAsset {
            metadata: 1,
            width: 2,
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
            ],
            trailing_offset: 48,
            trailing_bytes: 0,
            trailing_head_u32: None,
            placed_sprites_49: None,
        };

        let tags = map_display_rgba(&map, MapDisplayMode::CellTags);
        let elevation = map_display_rgba(&map, MapDisplayMode::CandidateElevation);

        assert_eq!(tags.len(), 16);
        assert_ne!(&tags[0..3], &tags[4..7]);
        assert_eq!(&elevation[0..4], &[0, 0, 0, 255]);
        assert_eq!(&elevation[4..8], &[170, 170, 170, 255]);
        assert_eq!(&elevation[8..12], &[85, 85, 85, 255]);
        assert_eq!(&elevation[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn terrain_preview_resolves_map_tile_indexes_through_the_atlas() {
        let map = MapAsset {
            metadata: 0,
            width: 2,
            height: 1,
            bits_per_pixel: 8,
            cells: vec![
                MapCell {
                    tag: 1,
                    value_bits: 0,
                    value: 0.0,
                },
                MapCell {
                    tag: 0,
                    value_bits: 0,
                    value: 0.0,
                },
            ],
            trailing_offset: 32,
            trailing_bytes: 0,
            trailing_head_u32: None,
            placed_sprites_49: None,
        };
        let tile_set = TileSetDefinition {
            atlas_member: "test.lbm".to_owned(),
            columns: 2,
            rows: 1,
            tile_width: 1,
            tile_height: 1,
            terrain_types: BTreeMap::new(),
            tiles: BTreeMap::from([
                (
                    0,
                    TileDefinition {
                        index: 0,
                        terrain_type: 0,
                    },
                ),
                (
                    1,
                    TileDefinition {
                        index: 1,
                        terrain_type: 0,
                    },
                ),
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

        assert_eq!(&preview[0..4], &[200, 210, 220, 255]);
        assert_eq!(&preview[7 * 4..8 * 4], &[200, 210, 220, 255]);
        assert_eq!(&preview[8 * 4..9 * 4], &[10, 20, 30, 255]);
    }

    /// A minimal single-frame IMP with an origin pair, built here because the library's own
    /// fixture is `#[cfg(test)]` inside the library crate and so is not visible to this binary.
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
}
