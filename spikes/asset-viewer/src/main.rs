use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use lom_asset_viewer::asset::{AssetKind, probe};
use lom_asset_viewer::imp::{ImpHeaderStats, ImpSprite};
use lom_asset_viewer::map::MapAsset;
use lom_asset_viewer::mpq::{Archive, Entry};
use lom_asset_viewer::pbm::PbmImage;
use lom_asset_viewer::png_export::write_imp_frame_png;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode};
use sdl3::video::Window;

const WINDOW_WIDTH: u32 = 1100;
const WINDOW_HEIGHT: u32 = 800;

enum Command {
    Catalog(Source),
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
    ScanMapDirectory(PathBuf),
    ValidateImp(Source),
    ViewImp {
        source: Source,
        member: String,
        frame: usize,
    },
    ViewMap(PathBuf),
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
}

impl MapDisplayMode {
    fn next(self) -> Self {
        match self {
            Self::CellTags => Self::CandidateElevation,
            Self::CandidateElevation => Self::CellTags,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CellTags => "diagnostic cell tags",
            Self::CandidateElevation => "candidate elevation",
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
        Command::DescribeImp { source, member } => describe_imp(&source, &member),
        Command::ExportImpFrame {
            source,
            member,
            frame,
            output,
        } => export_imp_frame(&source, &member, frame, &output),
        Command::Extract {
            source,
            member,
            output,
        } => extract_member(&source, &member, &output),
        Command::List(source) => list_archive(&source),
        Command::Inspect { source, member } => inspect_archive(&source, member.as_deref()),
        Command::InspectFile(path) => inspect_file(&path),
        Command::Scan(source) => scan_archive(&source),
        Command::ScanMapDirectory(path) => scan_map_directory(&path),
        Command::ValidateImp(source) => validate_imp_archive(&source),
        Command::ViewImp {
            source,
            member,
            frame,
        } => view_imp_archive(&source, &member, frame),
        Command::ViewMap(path) => view_map_file(&path),
        Command::View { source, member } => view_archive(&source, member.as_deref()),
    }
}

fn parse_args() -> Result<Command, String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let listfile = take_option(&mut args, "--listfile")?.map(PathBuf::from);
    let first = args.first().ok_or_else(usage)?.as_str();
    match first {
        "--catalog" => {
            require_len(&args, 2)?;
            Ok(Command::Catalog(source(&args[1], listfile)))
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
            require_len(&args, 2)?;
            Ok(Command::ViewMap(args[1].clone().into()))
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
    "usage:\n  lom-asset-viewer --list ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --catalog ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan-map-dir DIRECTORY\n  lom-asset-viewer --validate-imp ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --describe-imp ARCHIVE.mpq MEMBER [--listfile FILE]\n  lom-asset-viewer --view-imp ARCHIVE.mpq MEMBER [FRAME] [--listfile FILE]\n  lom-asset-viewer --view-map FILE\n  lom-asset-viewer --export-imp-frame ARCHIVE.mpq MEMBER FRAME OUTPUT.png [--listfile FILE]\n  lom-asset-viewer --inspect ARCHIVE.mpq [MEMBER] [--listfile FILE]\n  lom-asset-viewer --inspect-file FILE\n  lom-asset-viewer --extract ARCHIVE.mpq MEMBER OUTPUT [--listfile FILE]\n  lom-asset-viewer ARCHIVE.mpq [MEMBER] [--listfile FILE]".to_owned()
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
        for cell in &map.cells {
            cell_tags.insert(cell.tag);
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
        let display_rgba = imp_display_rgba(&frame.palette_indices, &frame.rgba, display_mode);
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

fn view_map_file(path: &Path) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let map = MapAsset::parse(&bytes).map_err(|error| error.to_string())?;
    let width = u16::try_from(map.width)
        .map_err(|_| format!("map width {} exceeds viewer limits", map.width))?;
    let height = u16::try_from(map.height)
        .map_err(|_| format!("map height {} exceeds viewer limits", map.height))?;
    let mut display_mode = MapDisplayMode::CandidateElevation;

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
                } => display_mode = display_mode.next(),
                _ => {}
            }
        }
        let title = format!(
            "Lords of Magic diagnostic map viewer — {} — {}×{} — {} — C changes mode",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unnamed map"),
            map.width,
            map.height,
            display_mode.label(),
        );
        canvas
            .window_mut()
            .set_title(&title)
            .map_err(|error| error.to_string())?;
        let rgba = map_display_rgba(&map, display_mode);
        draw_rgba_in_bounds(&mut canvas, width, height, width, height, &rgba)?;
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
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
    map.cells
        .iter()
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

fn imp_display_rgba(palette_indices: &[u8], source: &[u8], mode: ImpDisplayMode) -> Vec<u8> {
    debug_assert_eq!(palette_indices.len() * 4, source.len());
    if matches!(mode, ImpDisplayMode::Raw) {
        return source.to_vec();
    }
    source
        .chunks_exact(4)
        .zip(palette_indices)
        .flat_map(|(rgba, palette_index)| {
            let mut pixel: [u8; 4] = rgba.try_into().expect("RGBA chunks have four bytes");
            let background = *palette_index == 0;
            let secondary_mask = *palette_index == 1;
            if background || matches!(mode, ImpDisplayMode::Preview) && secondary_mask {
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
    use lom_asset_viewer::imp::{ImpCycle, ImpFrame, ImpSequence, ImpSprite};
    use lom_asset_viewer::map::{MapAsset, MapCell};

    use super::{
        ImpDisplayMode, MapDisplayMode, imp_display_rgba, map_display_rgba, step_imp_cycle,
        step_imp_frame, step_imp_sequence,
    };

    #[test]
    fn imp_display_modes_preserve_decoder_pixels() {
        let indices = [0, 1, 42, 0];
        let source = [0, 255, 0, 255, 255, 0, 0, 255, 1, 2, 3, 255, 0, 255, 0, 128];

        assert_eq!(
            imp_display_rgba(&indices, &source, ImpDisplayMode::Preview),
            [0, 255, 0, 0, 255, 0, 0, 0, 1, 2, 3, 255, 0, 255, 0, 0,]
        );
        assert_eq!(
            imp_display_rgba(&indices, &source, ImpDisplayMode::Mask),
            [0, 255, 0, 0, 255, 0, 0, 255, 1, 2, 3, 255, 0, 255, 0, 0,]
        );
        assert_eq!(
            imp_display_rgba(&indices, &source, ImpDisplayMode::Raw),
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
    fn map_display_modes_preserve_cell_count_and_order_elevation() {
        let map = MapAsset {
            metadata: 1,
            width: 2,
            height: 1,
            bits_per_pixel: 8,
            cells: vec![
                MapCell {
                    tag: 4,
                    value_bits: 0.0_f32.to_bits(),
                    value: 0.0,
                },
                MapCell {
                    tag: 5,
                    value_bits: 20.0_f32.to_bits(),
                    value: 20.0,
                },
            ],
            trailing_offset: 32,
            trailing_bytes: 0,
            trailing_head_u32: None,
        };

        let tags = map_display_rgba(&map, MapDisplayMode::CellTags);
        let elevation = map_display_rgba(&map, MapDisplayMode::CandidateElevation);

        assert_eq!(tags.len(), 8);
        assert_ne!(&tags[0..3], &tags[4..7]);
        assert_eq!(&elevation[0..4], &[0, 0, 0, 255]);
        assert_eq!(&elevation[4..8], &[255, 255, 255, 255]);
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
            duplicate_frame_count: 0,
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
