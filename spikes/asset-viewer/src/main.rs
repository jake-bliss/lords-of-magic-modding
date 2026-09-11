use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use lom_asset_viewer::asset::{AssetKind, probe};
use lom_asset_viewer::imp::{ImpHeaderStats, ImpSprite};
use lom_asset_viewer::mpq::{Archive, Entry};
use lom_asset_viewer::pbm::PbmImage;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{Canvas, FRect, ScaleMode};
use sdl3::video::Window;

const WINDOW_WIDTH: u32 = 1100;
const WINDOW_HEIGHT: u32 = 800;

enum Command {
    Catalog(Source),
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
    Scan(Source),
    ValidateImp(Source),
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

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match parse_args()? {
        Command::Catalog(source) => catalog_archive(&source),
        Command::Extract {
            source,
            member,
            output,
        } => extract_member(&source, &member, &output),
        Command::List(source) => list_archive(&source),
        Command::Inspect { source, member } => inspect_archive(&source, member.as_deref()),
        Command::Scan(source) => scan_archive(&source),
        Command::ValidateImp(source) => validate_imp_archive(&source),
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
        "--extract" => {
            require_len(&args, 4)?;
            Ok(Command::Extract {
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
        "--scan" => {
            require_len(&args, 2)?;
            Ok(Command::Scan(source(&args[1], listfile)))
        }
        "--validate-imp" => {
            require_len(&args, 2)?;
            Ok(Command::ValidateImp(source(&args[1], listfile)))
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
    "usage:\n  lom-asset-viewer --list ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --catalog ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --scan ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --validate-imp ARCHIVE.mpq [--listfile FILE]\n  lom-asset-viewer --inspect ARCHIVE.mpq [MEMBER] [--listfile FILE]\n  lom-asset-viewer --extract ARCHIVE.mpq MEMBER OUTPUT [--listfile FILE]\n  lom-asset-viewer ARCHIVE.mpq [MEMBER] [--listfile FILE]".to_owned()
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
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(
            PixelFormat::RGBA32,
            u32::from(image.width),
            u32::from(image.height),
        )
        .map_err(|error| error.to_string())?;
    texture.set_scale_mode(ScaleMode::Nearest);
    texture
        .update(None, &image.rgba, usize::from(image.width) * 4)
        .map_err(|error| error.to_string())?;

    let (output_width, output_height) = canvas.output_size().map_err(|error| error.to_string())?;
    let scale = (output_width as f32 / f32::from(image.width))
        .min(output_height as f32 / f32::from(image.height));
    let destination_width = f32::from(image.width) * scale;
    let destination_height = f32::from(image.height) * scale;
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
