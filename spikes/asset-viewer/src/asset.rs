use std::fmt;

use crate::imp::{ImpHeaderStats, ImpSprite};
use crate::pbm::PbmImage;

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
        return Ok(AssetInfo::new(
            AssetKind::SmackerVideo,
            format!("version={}", printable_tag(&bytes[0..4])),
        ));
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
    let mut kind = match extension {
        "gs" => AssetKind::GameScript,
        "lgd" => AssetKind::LegendScenario,
        "scn" => AssetKind::MapScenario,
        "smp" => AssetKind::MapComponent,
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

fn probe_imp_sprite(bytes: &[u8]) -> Result<AssetInfo, String> {
    let sprite = ImpSprite::parse(bytes).map_err(|error| error.to_string())?;
    let green_key = sprite
        .palette
        .iter()
        .position(|color| color[0..3] == [0, 255, 0])
        .map_or_else(|| "none".to_owned(), |index| index.to_string());
    let stored_pixel_bytes = sprite
        .stored_pixel_bytes
        .map_or_else(|| "unknown".to_owned(), |bytes| bytes.to_string());
    Ok(AssetInfo::new(
        AssetKind::ImpSprite,
        format!(
            "max-width={};max-height={};file-flags=0x{:02x};record-variant={};sequences={};cycles={};frames={};duplicate-frames={};hotspots={};hotspot-bytes={};raw-bytes={};stored-pixel-bytes={stored_pixel_bytes};green-key-index={green_key}",
            sprite.maximum_width,
            sprite.maximum_height,
            sprite.file_flags,
            sprite.record_variant,
            sprite.sequence_count,
            sprite.cycle_count,
            sprite.frame_count,
            sprite.duplicate_frame_count,
            sprite.hotspot_count,
            sprite.hotspot_bytes,
            sprite.raw_pixel_bytes,
        ),
    ))
}

fn probe_imp_header(bytes: &[u8]) -> Result<AssetInfo, String> {
    let stats = ImpHeaderStats::parse(bytes).map_err(|error| error.to_string())?;
    let compression = stats
        .compressed_pixel_bytes
        .map_or_else(|| "none".to_owned(), |bytes| format!("rle:{bytes}"));
    Ok(AssetInfo::new(
        AssetKind::ImpHeader,
        format!(
            "sequence={};sequences={};frames={};duplicate-frames={};raw-bytes={};compression={compression};hotspot-bytes={}",
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

fn probe_wave(bytes: &[u8]) -> Result<AssetInfo, String> {
    let mut cursor = 12_usize;
    let mut format = None;
    let mut data_size = None;
    while cursor
        .checked_add(8)
        .is_some_and(|chunk_header_end| chunk_header_end <= bytes.len())
    {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_size = read_u32_le(bytes, cursor + 4)? as usize;
        let chunk_start = cursor + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_size)
            .ok_or_else(|| "WAVE chunk size overflow".to_owned())?;
        if chunk_end > bytes.len() {
            return Err(format!("truncated WAVE {} chunk", printable_tag(chunk_id)));
        }
        if chunk_id == b"fmt " {
            if chunk_size < 16 {
                return Err("WAVE fmt chunk is too short".to_owned());
            }
            format = Some(WaveFormat {
                encoding: read_u16_le(bytes, chunk_start)?,
                channels: read_u16_le(bytes, chunk_start + 2)?,
                sample_rate: read_u32_le(bytes, chunk_start + 4)?,
                byte_rate: read_u32_le(bytes, chunk_start + 8)?,
                bits_per_sample: read_u16_le(bytes, chunk_start + 14)?,
            });
        } else if chunk_id == b"data" {
            data_size = Some(chunk_size);
        }
        cursor = chunk_end
            .checked_add(chunk_size & 1)
            .ok_or_else(|| "WAVE chunk padding overflow".to_owned())?;
    }

    let format = format.ok_or_else(|| "WAVE has no fmt chunk".to_owned())?;
    let data_size = data_size.ok_or_else(|| "WAVE has no data chunk".to_owned())?;
    if format.channels == 0 || format.sample_rate == 0 || format.byte_rate == 0 {
        return Err("WAVE format contains a zero channel/rate field".to_owned());
    }
    let duration_ms = (data_size as u64 * 1000) / u64::from(format.byte_rate);
    Ok(AssetInfo::new(
        AssetKind::WaveAudio,
        format!(
            "encoding={};channels={};sample-rate={};bits-per-sample={};data-bytes={data_size};duration-ms={duration_ms}",
            format.encoding, format.channels, format.sample_rate, format.bits_per_sample
        ),
    ))
}

struct WaveFormat {
    encoding: u16,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    bits_per_sample: u16,
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
        assert_eq!(
            probe("map\\urak.scn", &[1]).unwrap().kind,
            AssetKind::MapScenario
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
        assert!(info.details.contains("sequences=2;frames=45"));
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
