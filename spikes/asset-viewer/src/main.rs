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
use lom_asset_viewer::gamescript_vm::GameScriptVm;
use lom_asset_viewer::imp::{ImpHeaderStats, ImpSprite};
use lom_asset_viewer::map::MapAsset;
use lom_asset_viewer::mpq::{Archive, Entry};
use lom_asset_viewer::pbm::PbmImage;
use lom_asset_viewer::png_export::{write_imp_frame_png, write_rgba_png};
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
    ExportMapPreview {
        map: PathBuf,
        tile_set: PathBuf,
        atlas: PathBuf,
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
    ProbeGameScript {
        source: Source,
        member: String,
        expression: Option<String>,
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
        Command::ExportMapPreview {
            map,
            tile_set,
            atlas,
            output,
        } => export_map_preview(&map, &tile_set, &atlas, &output),
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
        Command::ProbeGameScript {
            source,
            member,
            expression,
        } => probe_gamescript_member(&source, &member, expression.as_deref()),
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
        "--export-map-preview" => {
            require_len(&args, 5)?;
            Ok(Command::ExportMapPreview {
                map: args[1].clone().into(),
                tile_set: args[2].clone().into(),
                atlas: args[3].clone().into(),
                output: args[4].clone().into(),
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
        "--probe-gamescript" => {
            require_len(&args, 3)?;
            Ok(Command::ProbeGameScript {
                source: source(&args[1], listfile),
                member: args[2].clone(),
                expression,
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
        return Err(format!("{option} requires a path"));
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
    "usage:\n  lom-asset-viewer --list ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --catalog ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan-gamescript ARCHIVE.mpq [--listfile FILE] [--exe lomse.exe]\n  lom-asset-viewer --probe-gamescript ARCHIVE.mpq MEMBER [--listfile FILE] [--eval SOURCE]\n  lom-asset-viewer --scan-map-dir DIRECTORY\n  lom-asset-viewer --describe-map FILE\n  lom-asset-viewer --validate-imp ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --describe-imp ARCHIVE.mpq MEMBER [--listfile FILE]\n  lom-asset-viewer --view-imp ARCHIVE.mpq MEMBER [FRAME] [--listfile FILE]\n  lom-asset-viewer --view-map FILE [TILESET.til TILE_ATLAS.lbm]\n  lom-asset-viewer --export-map-preview FILE TILESET.til TILE_ATLAS.lbm OUTPUT.png\n  lom-asset-viewer --export-imp-frame ARCHIVE.mpq MEMBER FRAME OUTPUT.png [--listfile FILE]\n  lom-asset-viewer --inspect ARCHIVE.mpq [MEMBER] [--listfile FILE]\n  lom-asset-viewer --inspect-file FILE\n  lom-asset-viewer --extract ARCHIVE.mpq MEMBER OUTPUT [--listfile FILE]\n  lom-asset-viewer ARCHIVE.mpq [MEMBER] [--listfile FILE]".to_owned()
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
            "sequence\t{sequence_index}\t-\t{}\t{}\tcycle:{};frame:{}\tcycle:{};frame:{}\t-",
            clean_field(&labels),
            hex_bytes(&sequence.metadata),
            sequence.first_cycle,
            sequence.first_frame,
            sequence.cycle_count,
            sequence.frame_count,
        );
        for cycle_index in sequence.first_cycle..sequence.first_cycle + sequence.cycle_count {
            let cycle = &sprite.cycles[cycle_index];
            println!(
                "cycle\t{cycle_index}\tsequence:{sequence_index}\t-\t0x{:04x}\tframe:{}\tframe:{}\t-",
                cycle.metadata, cycle.first_frame, cycle.frame_count,
            );
        }
    }
    for (frame_index, frame) in sprite.frames.iter().enumerate() {
        let (sequence_index, cycle_index, frame_in_cycle) = sprite
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
            "frame\t{frame_index}\tsequence:{sequence_index};cycle:{cycle_index};offset:{frame_in_cycle}\t0x{:02x};{source_frame}\t{}x{}\t-\t{}\t{}",
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

fn probe_gamescript_member(
    source: &Source,
    member: &str,
    expression: Option<&str>,
) -> Result<(), String> {
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
    vm.execute_document(&document)
        .map_err(|error| error.to_string())?;
    let expression_tokens = expression
        .map(|source| {
            let expression =
                GameScriptDocument::parse(source.as_bytes()).map_err(|error| error.to_string())?;
            let tokens = expression.tokens.len();
            vm.execute_document(&expression)
                .map_err(|error| error.to_string())?;
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
        .map(|path| likely_engine_names(path, &executable_names, &literal_names))
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
    println!("static-run-reference-edges\t{}", dependency_edges.len());
    println!("resolved-static-run-references\t{resolved_dependencies}");
    println!(
        "unresolved-static-run-references\t{}",
        missing_dependencies.len()
    );
    if let Some(names) = &likely_engine_names {
        println!("likely-hardcoded-engine-names\t{}", names.len());
        for (name, count) in names.iter().take(50) {
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
    literal_names: &BTreeMap<String, usize>,
) -> Result<Vec<(String, usize)>, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read executable {}: {error}", path.display()))?;
    let binary_strings = ascii_strings(&bytes);
    let literal_names: BTreeSet<String> = literal_names
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    let mut candidates: Vec<_> = executable_names
        .iter()
        .filter(|(name, _)| {
            let lower = name.to_ascii_lowercase();
            !literal_names.contains(&lower) && binary_strings.contains(&lower)
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
                    frame_index = step_imp_cycle(&sprite, frame_index, 1)?;
                    last_advance = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Up),
                    repeat: false,
                    ..
                } => {
                    frame_index = step_imp_cycle(&sprite, frame_index, -1)?;
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
        let (sequence_index, cycle_index, frame_in_cycle) = sprite
            .frame_location(frame_index)
            .map_err(|error| error.to_string())?;
        let sequence = &sprite.sequences[sequence_index];
        let cycle = &sprite.cycles[cycle_index];
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
            "Lords of Magic IMP viewer — {} — {} {}/{} — cycle {}/{} — frame {}/{} (global {}/{}, {}×{}, {} bpp, {}, {}, seq={}, cycle=0x{:04x}{})",
            entry.name,
            sequence_label,
            sequence_index + 1,
            sprite.sequences.len(),
            cycle_index - sequence.first_cycle + 1,
            sequence.cycle_count,
            frame_in_cycle + 1,
            cycle.frame_count,
            frame_index + 1,
            sprite.frames.len(),
            frame.width,
            frame.height,
            sprite.bits_per_pixel,
            display_mode.label(),
            placement,
            hex_bytes(&sequence.metadata),
            cycle.metadata,
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
    let (_, cycle_index, frame_in_cycle) = sprite
        .frame_location(current)
        .map_err(|error| error.to_string())?;
    find_visible_in_cycle(sprite, cycle_index, frame_in_cycle, direction, false)
}

fn step_imp_cycle(sprite: &ImpSprite, current: usize, direction: isize) -> Result<usize, String> {
    let (sequence_index, cycle_index, _) = sprite
        .frame_location(current)
        .map_err(|error| error.to_string())?;
    let sequence = &sprite.sequences[sequence_index];
    let relative_cycle = cycle_index - sequence.first_cycle;
    for distance in 1..=sequence.cycle_count {
        let relative = (relative_cycle as isize + direction * distance as isize)
            .rem_euclid(sequence.cycle_count as isize) as usize;
        let candidate = sequence.first_cycle + relative;
        if let Ok(frame) = find_visible_in_cycle(sprite, candidate, 0, 1, true) {
            return Ok(frame);
        }
    }
    Err("IMP sequence contains no visible cycles".to_owned())
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
        for relative_cycle in 0..sequence.cycle_count {
            if let Ok(frame) =
                find_visible_in_cycle(sprite, sequence.first_cycle + relative_cycle, 0, 1, true)
            {
                return Ok(frame);
            }
        }
    }
    Err("IMP sprite contains no visible sequences".to_owned())
}

fn find_visible_in_cycle(
    sprite: &ImpSprite,
    cycle_index: usize,
    current_offset: usize,
    direction: isize,
    include_current: bool,
) -> Result<usize, String> {
    let cycle = sprite
        .cycles
        .get(cycle_index)
        .ok_or_else(|| format!("IMP cycle index {cycle_index} is out of range"))?;
    if cycle.frame_count == 0 {
        return Err("IMP cycle contains no frames".to_owned());
    }
    let first_distance = usize::from(!include_current);
    for distance in first_distance..first_distance + cycle.frame_count {
        let offset = (current_offset as isize + direction * distance as isize)
            .rem_euclid(cycle.frame_count as isize) as usize;
        let index = cycle.first_frame + offset;
        let frame = sprite
            .resolved_frame(index)
            .map_err(|error| error.to_string())?;
        if frame.width > 0 && frame.height > 0 && !frame.rgba.is_empty() {
            return Ok(index);
        }
    }
    Err("IMP cycle contains no visible frames".to_owned())
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use lom_asset_viewer::imp::{ImpCycle, ImpFrame, ImpSequence, ImpSprite};
    use lom_asset_viewer::map::{MapAsset, MapCell};
    use lom_asset_viewer::pbm::PbmImage;
    use lom_asset_viewer::tile::{TileDefinition, TileSetDefinition};

    use super::{
        ImpDisplayMode, MapDisplayMode, imp_display_rgba, map_display_rgba, step_imp_cycle,
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
    fn imp_navigation_respects_cycle_and_sequence_boundaries() {
        let sprite = navigation_sprite();

        assert_eq!(step_imp_frame(&sprite, 1, 1).unwrap(), 0);
        assert_eq!(step_imp_frame(&sprite, 0, -1).unwrap(), 1);
        assert_eq!(step_imp_cycle(&sprite, 0, 1).unwrap(), 2);
        assert_eq!(step_imp_cycle(&sprite, 2, -1).unwrap(), 0);
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

    fn navigation_sprite() -> ImpSprite {
        let frames = (0..6)
            .map(|_| ImpFrame {
                flags: 0,
                width: 1,
                height: 1,
                origin_x: None,
                origin_y: None,
                hotspots: Vec::new(),
                palette_indices: vec![2],
                rgba: vec![1, 2, 3, 255],
                source_frame: None,
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
            cycle_count: 3,
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
                    first_cycle: 0,
                    cycle_count: 2,
                    first_frame: 0,
                    frame_count: 4,
                },
                ImpSequence {
                    metadata: [0; 11],
                    first_cycle: 2,
                    cycle_count: 1,
                    first_frame: 4,
                    frame_count: 2,
                },
            ],
            cycles: vec![
                ImpCycle {
                    metadata: 0,
                    first_frame: 0,
                    frame_count: 2,
                },
                ImpCycle {
                    metadata: 0,
                    first_frame: 2,
                    frame_count: 2,
                },
                ImpCycle {
                    metadata: 0,
                    first_frame: 4,
                    frame_count: 2,
                },
            ],
            frames,
        }
    }
}
