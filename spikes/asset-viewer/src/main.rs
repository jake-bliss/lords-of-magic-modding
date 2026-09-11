use std::env;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

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
    List {
        archive: PathBuf,
    },
    Inspect {
        archive: PathBuf,
        member: Option<String>,
    },
    Scan {
        archive: PathBuf,
    },
    View {
        archive: PathBuf,
        member: Option<String>,
    },
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
        Command::List { archive } => list_archive(&archive),
        Command::Inspect { archive, member } => inspect_archive(&archive, member.as_deref()),
        Command::Scan { archive } => scan_archive(&archive),
        Command::View { archive, member } => view_archive(&archive, member.as_deref()),
    }
}

fn parse_args() -> Result<Command, String> {
    let mut args = env::args().skip(1);
    let first = args.next().ok_or_else(usage)?;
    match first.as_str() {
        "--list" => {
            let archive = args.next().ok_or_else(usage)?;
            reject_extra_args(args)?;
            Ok(Command::List {
                archive: archive.into(),
            })
        }
        "--inspect" => {
            let archive = args.next().ok_or_else(usage)?;
            let member = args.next();
            reject_extra_args(args)?;
            Ok(Command::Inspect {
                archive: archive.into(),
                member,
            })
        }
        "--scan" => {
            let archive = args.next().ok_or_else(usage)?;
            reject_extra_args(args)?;
            Ok(Command::Scan {
                archive: archive.into(),
            })
        }
        "--help" | "-h" => Err(usage()),
        _ => {
            let member = args.next();
            reject_extra_args(args)?;
            Ok(Command::View {
                archive: first.into(),
                member,
            })
        }
    }
}

fn reject_extra_args(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    if args.next().is_some() {
        Err(usage())
    } else {
        Ok(())
    }
}

fn usage() -> String {
    "usage:\n  lom-asset-viewer --list ARCHIVE.mpq\n  lom-asset-viewer --scan ARCHIVE.mpq\n  lom-asset-viewer --inspect ARCHIVE.mpq [MEMBER]\n  lom-asset-viewer ARCHIVE.mpq [MEMBER]".to_owned()
}

fn open_archive(path: &Path) -> Result<(Archive, Vec<Entry>), String> {
    let archive = Archive::open(path).map_err(|error| error.to_string())?;
    let entries = archive.entries().map_err(|error| error.to_string())?;
    Ok((archive, entries))
}

fn list_archive(path: &Path) -> Result<(), String> {
    let (_archive, entries) = open_archive(path)?;
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

fn inspect_archive(path: &Path, requested: Option<&str>) -> Result<(), String> {
    let (archive, entries) = open_archive(path)?;
    let selected = select_image(&archive, &entries, requested)?;
    println!("name\t{}", selected.name);
    println!(
        "dimensions\t{}x{}",
        selected.image.width, selected.image.height
    );
    println!("palette_entries\t{}", selected.image.palette_entries);
    println!("compression\t{}", selected.image.compression);
    println!("masking\t{}", selected.image.masking);
    println!("rgba_bytes\t{}", selected.image.rgba.len());
    Ok(())
}

fn scan_archive(path: &Path) -> Result<(), String> {
    let (archive, entries) = open_archive(path)?;
    let mut readable = 0_usize;
    let mut pbm_files = 0_usize;
    let mut decoded = 0_usize;
    let mut decode_failures = Vec::new();

    for entry in &entries {
        let Ok(bytes) = archive.read(&entry.name) else {
            continue;
        };
        readable += 1;
        if bytes.len() < 12 || &bytes[0..4] != b"FORM" || &bytes[8..12] != b"PBM " {
            continue;
        }
        pbm_files += 1;
        match PbmImage::decode(&bytes) {
            Ok(_) => decoded += 1,
            Err(error) => decode_failures.push((entry.name.clone(), error.to_string())),
        }
    }

    println!("archive_entries\t{}", entries.len());
    println!("readable_entries\t{readable}");
    println!("pbm_files\t{pbm_files}");
    println!("decoded_pbm_files\t{decoded}");
    println!("decode_failures\t{}", decode_failures.len());
    for (name, error) in decode_failures {
        println!("failure\t{name}\t{error}");
    }
    Ok(())
}

fn view_archive(path: &Path, requested: Option<&str>) -> Result<(), String> {
    let (archive, entries) = open_archive(path)?;
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
