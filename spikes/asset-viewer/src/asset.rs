use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::imp::{ImpHeaderStats, ImpSprite};
use crate::map::MapAsset;
use crate::pbm::PbmImage;
use crate::smacker::SmackerFile;
use crate::tile::TileSetDefinition;
use crate::wave::WaveFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssetKind {
    Bitmap,
    Empty,
    GameScript,
    IffIlbm,
    IffPbm,
    ImpHeader,
    ImpSprite,
    LegendScenario,
    Listfile,
    MapComponent,
    MapScenario,
    MpqArchive,
    PortableExecutable,
    SmackerVideo,
    Text,
    TileSetDefinition,
    Unknown,
    UrlShortcut,
    WaveAudio,
}

impl fmt::Display for AssetKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bitmap => "bitmap",
            Self::Empty => "empty",
            Self::GameScript => "game-script",
            Self::IffIlbm => "iff-ilbm",
            Self::IffPbm => "iff-pbm",
            Self::ImpHeader => "imp-header",
            Self::ImpSprite => "imp-sprite",
            Self::LegendScenario => "legend-scenario",
            Self::Listfile => "mpq-listfile",
            Self::MapComponent => "map-component",
            Self::MapScenario => "map-scenario",
            Self::MpqArchive => "mpq-archive",
            Self::PortableExecutable => "portable-executable",
            Self::SmackerVideo => "smacker-video",
            Self::Text => "text",
            Self::TileSetDefinition => "tile-set-definition",
            Self::Unknown => "unknown",
            Self::UrlShortcut => "url-shortcut",
            Self::WaveAudio => "wave-audio",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    pub kind: AssetKind,
    pub details: String,
}

impl AssetInfo {
    fn new(kind: AssetKind, details: impl Into<String>) -> Self {
        Self {
            kind,
            details: details.into(),
        }
    }
}

pub fn probe(name: &str, bytes: &[u8]) -> Result<AssetInfo, String> {
    if bytes.is_empty() {
        return Ok(AssetInfo::new(AssetKind::Empty, ""));
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"FORM" {
        return match &bytes[8..12] {
            b"PBM " => probe_pbm(bytes),
            b"ILBM" => Ok(AssetInfo::new(AssetKind::IffIlbm, "")),
            _ => Ok(AssetInfo::new(
                AssetKind::Unknown,
                format!("iff-form={}", printable_tag(&bytes[8..12])),
            )),
        };
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return probe_wave(bytes);
    }
    if bytes.starts_with(b"BM") {
        return probe_bitmap(bytes);
    }
    if bytes.starts_with(b"SMK2") || bytes.starts_with(b"SMK4") {
        return probe_smacker(bytes);
    }
    if bytes.starts_with(b"MPQ\x1a") {
        return Ok(AssetInfo::new(AssetKind::MpqArchive, ""));
    }
    if bytes.starts_with(b"MZ") {
        return Ok(AssetInfo::new(AssetKind::PortableExecutable, ""));
    }

    let lower_name = name.to_ascii_lowercase();
    if lower_name == "(listfile)" {
        return Ok(AssetInfo::new(AssetKind::Listfile, text_details(bytes)));
    }
    let extension = lower_name.rsplit('.').next().unwrap_or_default();
    if extension == "h" {
        return probe_imp_header(bytes);
    }
    if extension == "imp" {
        return probe_imp_sprite(bytes);
    }
    if extension == "til" {
        return probe_tile_set(bytes);
    }
    if let Some(kind) = match extension {
        "lgd" => Some(AssetKind::LegendScenario),
        "scn" => Some(AssetKind::MapScenario),
        "smp" => Some(AssetKind::MapComponent),
        _ => None,
    } {
        return probe_map(kind, bytes);
    }
    let mut kind = match extension {
        "gs" => AssetKind::GameScript,
        "txt" => AssetKind::Text,
        "url" => AssetKind::UrlShortcut,
        _ => AssetKind::Unknown,
    };
    if kind == AssetKind::Unknown && looks_like_text(bytes) {
        kind = AssetKind::Text;
    }
    let details = match kind {
        AssetKind::GameScript | AssetKind::Listfile | AssetKind::Text | AssetKind::UrlShortcut => {
            text_details(bytes)
        }
        _ => String::new(),
    };
    Ok(AssetInfo::new(kind, details))
}

fn probe_tile_set(bytes: &[u8]) -> Result<AssetInfo, String> {
    let tile_set = TileSetDefinition::parse(bytes).map_err(|error| error.to_string())?;
    Ok(AssetInfo::new(
        AssetKind::TileSetDefinition,
        format!(
            "atlas={};columns={};rows={};tile-width={};tile-height={};capacity={};defined-tiles={};terrain-types={}",
            tile_set.atlas_member,
            tile_set.columns,
            tile_set.rows,
            tile_set.tile_width,
            tile_set.tile_height,
            tile_set.atlas_capacity(),
            tile_set.tiles.len(),
            tile_set.terrain_types.len(),
        ),
    ))
}

fn probe_map(kind: AssetKind, bytes: &[u8]) -> Result<AssetInfo, String> {
    let map = MapAsset::parse(bytes).map_err(|error| error.to_string())?;
    let distinct_tags: BTreeSet<u32> = map.cells.iter().map(|cell| cell.tag).collect();
    let distinct_tile_indexes: BTreeSet<u32> =
        map.cells.iter().map(|cell| cell.tile_index()).collect();
    let high_flag_cells = map.cells.iter().filter(|cell| cell.high_flag_set()).count();
    let mut finite_min = f32::INFINITY;
    let mut finite_max = f32::NEG_INFINITY;
    let mut nonfinite_values = 0_usize;
    for cell in &map.cells {
        if cell.value.is_finite() {
            finite_min = finite_min.min(cell.value);
            finite_max = finite_max.max(cell.value);
        } else {
            nonfinite_values += 1;
        }
    }
    let value_range = if finite_min.is_infinite() {
        "none".to_owned()
    } else {
        format!("{finite_min}..{finite_max}")
    };
    let trailing_head = map
        .trailing_head_u32()
        .map_or_else(|| "none".to_owned(), |count| count.to_string());
    let tail_layouts = map.resolved_tail_layout().map_or_else(
        || {
            let candidates = map
                .candidate_tail_layouts()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            if candidates.is_empty() {
                "unknown".to_owned()
            } else {
                format!("undecoded:{candidates}")
            }
        },
        |layout| layout.to_string(),
    );
    let (placed_sprite_count, placed_sprite_types, placed_sprite_footer) = map
        .placed_sprites
        .as_ref()
        .map(|section| {
            let types = section
                .records
                .iter()
                .map(|record| record.sprite_type)
                .collect::<BTreeSet<_>>()
                .len();
            (
                section.records.len().to_string(),
                types.to_string(),
                section
                    .footer
                    .map_or_else(|| "none".to_owned(), |footer| footer.to_string()),
            )
        })
        .unwrap_or_else(|| ("none".to_owned(), "none".to_owned(), "none".to_owned()));
    Ok(AssetInfo::new(
        kind,
        format!(
            "metadata={};width={};height={};bits-per-pixel={};cells={};distinct-cell-tags={};distinct-tile-indexes={};high-flag-cells={high_flag_cells};candidate-value-range={value_range};nonfinite-values={nonfinite_values};trailing-bytes={};trailing-head-u32={trailing_head};tail-layout={tail_layouts};placed-sprite-records={placed_sprite_count};placed-sprite-types={placed_sprite_types};placed-sprite-footer={placed_sprite_footer}",
            map.metadata,
            map.width,
            map.height,
            map.bits_per_pixel,
            map.cells.len(),
            distinct_tags.len(),
            distinct_tile_indexes.len(),
            map.trailing_bytes(),
        ),
    ))
}

fn probe_imp_sprite(bytes: &[u8]) -> Result<AssetInfo, String> {
    let sprite = ImpSprite::parse(bytes).map_err(|error| error.to_string())?;
    let origin_frames = sprite
        .frames
        .iter()
        .filter(|frame| frame.origin_x.is_some() && frame.origin_y.is_some())
        .count();
    let hotspot_frames = sprite
        .frames
        .iter()
        .filter(|frame| !frame.hotspots.is_empty())
        .count();
    let mut hotspot_ids = BTreeMap::<u16, usize>::new();
    for hotspot in sprite.frames.iter().flat_map(|frame| frame.hotspots.iter()) {
        *hotspot_ids.entry(hotspot.id).or_default() += 1;
    }
    let hotspot_id_values = hotspot_ids
        .keys()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let hotspot_id_counts = hotspot_ids
        .iter()
        .map(|(id, count)| format!("{id}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    let origin_x_range = i16_range(sprite.frames.iter().filter_map(|frame| frame.origin_x));
    let origin_y_range = i16_range(sprite.frames.iter().filter_map(|frame| frame.origin_y));
    let hotspot_x_range = i16_range(
        sprite
            .frames
            .iter()
            .flat_map(|frame| frame.hotspots.iter().map(|hotspot| hotspot.x)),
    );
    let hotspot_y_range = i16_range(
        sprite
            .frames
            .iter()
            .flat_map(|frame| frame.hotspots.iter().map(|hotspot| hotspot.y)),
    );
    let green_key = sprite
        .palette
        .iter()
        .position(|color| color[0..3] == [0, 255, 0])
        .map_or_else(|| "none".to_owned(), |index| index.to_string());
    Ok(AssetInfo::new(
        AssetKind::ImpSprite,
        format!(
            "max-width={};max-height={};file-flags=0x{:02x};record-variant={};compressed={};bits-per-pixel={};sequences={};facings={};frames={};duplicate-frames={};origin-frames={origin_frames};origin-x-range={origin_x_range};origin-y-range={origin_y_range};hotspot-frames={hotspot_frames};hotspots={};hotspot-ids={};hotspot-id-values={hotspot_id_values};hotspot-id-counts={hotspot_id_counts};hotspot-x-range={hotspot_x_range};hotspot-y-range={hotspot_y_range};hotspot-bytes={};raw-bytes={};stored-pixel-bytes={};green-key-index={green_key}",
            sprite.maximum_width,
            sprite.maximum_height,
            sprite.file_flags,
            sprite.record_variant,
            sprite.compressed,
            sprite.bits_per_pixel,
            sprite.sequence_count,
            sprite.facing_count,
            sprite.frame_count,
            sprite.duplicate_frame_count,
            sprite.hotspot_count,
            hotspot_ids.len(),
            sprite.hotspot_bytes,
            sprite.raw_pixel_bytes,
            sprite.stored_pixel_bytes,
        ),
    ))
}

fn i16_range(values: impl Iterator<Item = i16>) -> String {
    let mut minimum = None::<i16>;
    let mut maximum = None::<i16>;
    for value in values {
        minimum = Some(minimum.map_or(value, |current| current.min(value)));
        maximum = Some(maximum.map_or(value, |current| current.max(value)));
    }
    match (minimum, maximum) {
        (Some(minimum), Some(maximum)) => format!("{minimum}..{maximum}"),
        _ => "none".to_owned(),
    }
}

fn probe_imp_header(bytes: &[u8]) -> Result<AssetInfo, String> {
    let stats = ImpHeaderStats::parse(bytes).map_err(|error| error.to_string())?;
    let named_sequences = stats
        .sequence_labels
        .iter()
        .filter(|labels| !labels.is_empty())
        .count();
    let compression = stats
        .compressed_pixel_bytes
        .map_or_else(|| "none".to_owned(), |bytes| format!("rle:{bytes}"));
    Ok(AssetInfo::new(
        AssetKind::ImpHeader,
        format!(
            "sequence={};sequences={};named-sequences={named_sequences};frames={};duplicate-frames={};raw-bytes={};compression={compression};hotspot-bytes={}",
            stats.sequence_name,
            stats.sequence_count,
            stats.frame_count,
            stats.duplicate_frame_count,
            stats.raw_pixel_bytes,
            stats.hotspot_bytes,
        ),
    ))
}

fn looks_like_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace() || *byte >= 0x80)
}

fn probe_pbm(bytes: &[u8]) -> Result<AssetInfo, String> {
    let image = PbmImage::decode(bytes).map_err(|error| error.to_string())?;
    let green_key = image
        .palette
        .iter()
        .position(|color| *color == [0, 255, 0])
        .map_or_else(|| "none".to_owned(), |index| index.to_string());
    Ok(AssetInfo::new(
        AssetKind::IffPbm,
        format!(
            "width={};height={};palette={};compression={};masking={};transparent-index={};green-key-index={green_key}",
            image.width,
            image.height,
            image.palette_entries,
            image.compression,
            image.masking,
            image.transparent_color,
        ),
    ))
}

fn probe_bitmap(bytes: &[u8]) -> Result<AssetInfo, String> {
    if bytes.len() < 30 {
        return Err("truncated Windows bitmap header".to_owned());
    }
    let dib_size = read_u32_le(bytes, 14)?;
    if dib_size < 40 || bytes.len() < 34 {
        return Err(format!(
            "unsupported Windows bitmap DIB header size {dib_size}"
        ));
    }
    let width = read_i32_le(bytes, 18)?;
    let height = read_i32_le(bytes, 22)?;
    let bits_per_pixel = read_u16_le(bytes, 28)?;
    let compression = read_u32_le(bytes, 30)?;
    Ok(AssetInfo::new(
        AssetKind::Bitmap,
        format!(
            "width={};height={};bits-per-pixel={bits_per_pixel};compression={compression}",
            width.unsigned_abs(),
            height.unsigned_abs()
        ),
    ))
}

/// Classify a WAVE by **decoding it**, not by reading its header.
///
/// This used to stop at the `fmt ` chunk, which meant `--scan` reported a clean sweep over 3,098
/// members it had never read a sample of. Delegating to [`crate::wave`] makes the classification
/// as strong as the decoder: a member that appears here as `wave-audio` is one whose whole
/// container was walked and whose sample data was decoded.
fn probe_wave(bytes: &[u8]) -> Result<AssetInfo, String> {
    let file = WaveFile::parse(bytes).map_err(|error| error.to_string())?;
    let data_bytes = file.samples.interleaved.len()
        * usize::from(file.format.bits_per_sample / 8).max(1)
        + file.samples.trailing_partial_frame.len();
    Ok(AssetInfo::new(
        AssetKind::WaveAudio,
        format!(
            "encoding={};channels={};sample-rate={};bits-per-sample={};data-bytes={data_bytes};\
             frames={};duration-ms={};layout={}",
            file.format.encoding,
            file.format.channels,
            file.format.sample_rate,
            file.format.bits_per_sample,
            file.samples.frames(),
            file.samples.duration_ms(),
            file.layout(),
        ),
    ))
}

/// Classify a Smacker video by **parsing its container**, not by matching four magic bytes.
///
/// The frame tables and per-frame chunk extents are walked; the video codec is not implemented and
/// no frame is decoded to pixels. See [`crate::smacker`] for exactly where that line falls.
fn probe_smacker(bytes: &[u8]) -> Result<AssetInfo, String> {
    let file = SmackerFile::parse(bytes).map_err(|error| error.to_string())?;
    let tracks = file.audio.iter().filter(|track| track.present()).count();
    Ok(AssetInfo::new(
        AssetKind::SmackerVideo,
        format!(
            "version={};width={};height={};frames={};interval-us={};duration-ms={};\
             audio-tracks={tracks};palette-frames={};unaccounted-tail={}",
            printable_tag(&file.signature),
            file.width,
            file.height,
            file.frame_count,
            file.frame_interval_us(),
            file.duration_ms(),
            file.palette_frames(),
            file.unaccounted_tail,
        ),
    ))
}

fn text_details(bytes: &[u8]) -> String {
    let lines = bytes.iter().filter(|byte| **byte == b'\n').count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
    let encoding = if std::str::from_utf8(bytes).is_ok() {
        "utf-8"
    } else {
        "legacy-8-bit"
    };
    format!("encoding={encoding};lines={lines}")
}

fn printable_tag(tag: &[u8]) -> String {
    tag.iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            }
        })
        .collect()
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| "little-endian u16 offset overflow".to_owned())?;
    let value: [u8; 2] = bytes
        .get(offset..end)
        .ok_or_else(|| "truncated little-endian u16".to_owned())?
        .try_into()
        .expect("slice length was checked");
    Ok(u16::from_le_bytes(value))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| "little-endian u32 offset overflow".to_owned())?;
    let value: [u8; 4] = bytes
        .get(offset..end)
        .ok_or_else(|| "truncated little-endian u32".to_owned())?
        .try_into()
        .expect("slice length was checked");
    Ok(u32::from_le_bytes(value))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| "little-endian i32 offset overflow".to_owned())?;
    let value: [u8; 4] = bytes
        .get(offset..end)
        .ok_or_else(|| "truncated little-endian i32".to_owned())?
        .try_into()
        .expect("slice length was checked");
    Ok(i32::from_le_bytes(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_pcm_wave_metadata() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&22_050_u32.to_le_bytes());
        bytes.extend_from_slice(&22_050_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&22_050_u32.to_le_bytes());
        bytes.resize(bytes.len() + 22_050, 128);

        let info = probe("voice.wav", &bytes).unwrap();
        assert_eq!(info.kind, AssetKind::WaveAudio);
        assert!(info.details.contains("sample-rate=22050"));
        assert!(info.details.contains("duration-ms=1000"));
        assert!(info.details.contains("frames=22050"));
        assert!(info.details.contains("layout=fmt |data"));
    }

    #[test]
    fn probes_windows_bitmap_metadata() {
        let mut bytes = vec![0_u8; 54];
        bytes[0..2].copy_from_slice(b"BM");
        bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&400_i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&(-144_i32).to_le_bytes());
        bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());

        let info = probe("background.bmp", &bytes).unwrap();
        assert_eq!(info.kind, AssetKind::Bitmap);
        assert!(info.details.contains("width=400;height=144"));
        assert!(info.details.contains("bits-per-pixel=24"));
    }

    #[test]
    fn classifies_proprietary_formats_by_recovered_name() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&108_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        bytes.extend_from_slice(&413_u32.to_le_bytes());
        bytes.extend_from_slice(&6.0_f32.to_bits().to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());

        let info = probe("map\\urak.scn", &bytes).unwrap();
        assert_eq!(info.kind, AssetKind::MapScenario);
        assert!(info.details.contains("metadata=108;width=1;height=1"));
        assert!(info.details.contains("candidate-value-range=6..6"));
        assert!(info.details.contains("trailing-head-u32=0"));
    }

    #[test]
    fn probes_tile_set_definitions() {
        let bytes = b"LBM=tilesb01.lbm\nTILES=16,39\nTILESIZE=32,32\nTERRAINTYPE=6,125,\"plains\"\nTILE=0,6\n";

        let info = probe("til\\tilesb01.til", bytes).unwrap();

        assert_eq!(info.kind, AssetKind::TileSetDefinition);
        assert_eq!(
            info.details,
            "atlas=tilesb01.lbm;columns=16;rows=39;tile-width=32;tile-height=32;capacity=624;defined-tiles=1;terrain-types=1"
        );
    }

    #[test]
    fn parses_generated_imp_header_statistics() {
        let header = b"// Sprite headers for sequence dragon\r\n\
// Total number of 'Sequences': 2\r\n\
// Total number of 'Frames': 45\r\n\
// Duplicate bitmaps found : 0\r\n\
// Bitmap raw memory usage: 144878\r\n\
// Hotspot raw memory usage: 12\r\n\
// Bitmap RLE memory usage: 60875\r\n";
        let info = probe("units\\dragon.h", header).unwrap();
        assert_eq!(info.kind, AssetKind::ImpHeader);
        assert!(info.details.contains("sequence=dragon"));
        assert!(
            info.details
                .contains("sequences=2;named-sequences=0;frames=45")
        );
        assert!(
            info.details
                .contains("raw-bytes=144878;compression=rle:60875")
        );
        assert!(info.details.contains("hotspot-bytes=12"));
    }

    #[test]
    fn rejects_truncated_recognized_formats() {
        assert_eq!(
            probe("bad.bmp", b"BM").unwrap_err(),
            "truncated Windows bitmap header"
        );
        assert_eq!(
            probe("bad.wav", b"RIFF\0\0\0\0WAVE").unwrap_err(),
            "WAVE has no fmt chunk"
        );
    }

    #[test]
    fn detects_text_when_an_mpq_member_has_no_recovered_name() {
        let info = probe("File00000001.xxx", b"unknown original filename\r\n").unwrap();
        assert_eq!(info.kind, AssetKind::Text);
        assert_eq!(info.details, "encoding=utf-8;lines=1");
    }
}
