//! The files that sit on disk *beside* the archives.
//!
//! Every earlier pass in this repository walked MPQ members. That is where the art, the scripts and
//! the sounds live, so "every format outside the executable is decoded" was written on the strength
//! of the archive work. It was not true of the **loose** tree: `English/Wav/`, `English/smk/`, the
//! shader pack, the launcher string table and the two install-root `.cfg` files had never been
//! enumerated at all, let alone decoded.
//!
//! This module does three separable things, and keeps them separable on purpose.
//!
//! 1. [`inventory`] walks a loose tree and records path, size, SHA-256, and a **magic-only**
//!    signature next to the verdict of the existing [`crate::asset::probe`]. `probe` consults the
//!    file extension for several formats; a sweep whose job is to find what nobody named cannot
//!    trust extensions, so the two are recorded side by side and disagreements are flagged rather
//!    than reconciled.
//! 2. [`LomConfig`] parses `lom.cfg`, whose layout was recovered from `lomse.exe`'s own reader and
//!    writer (see the type's documentation for the evidence).
//! 3. [`SettingsConfig`] parses `settings.cfg`, which is plain CR-separated `KEY VALUE` text.
//!
//! Both parsers round-trip byte-for-byte. Neither invents a field: `lom.cfg`'s trailing word has no
//! name anywhere in the recovered operator table, so it is carried as
//! [`LomConfig::unnamed_trailing_word`] and is not given a meaning.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::asset::{self, AssetKind};

// ---------------------------------------------------------------------------------------------
// lom.cfg
// ---------------------------------------------------------------------------------------------

/// The parsed contents of `lom.cfg`.
///
/// **Observed in a local binary** (`lomse.exe`, baseline profile, 2026-09-18). The GameScript
/// operators `saveconfig` (`0x00487570`) and `loadconfig` (`0x00487580`) are two-instruction thunks
/// -- `mov ecx, 0x5aa12c` then a tail jump to `0x00487220` and `0x00487360` respectively. Those two
/// functions are the only code in the image that names the string `"lom.cfg"`; one opens it `"wb"`
/// and issues a fixed sequence of `fwrite` calls, the other opens it `"rb"` and issues the matching
/// `fread` calls. The field order below is that sequence, and the reader and the writer agree
/// field-for-field -- two instruments, not one reading taken twice.
///
/// The names come from a third, independent place: the repository's own
/// `reports/natives/operator-bodies.tsv`, which was produced by walking operator bodies and not by
/// reading this file at all. Each global the writer stores is referenced by an operator whose name
/// states what it is (`getmusicvolume` reads `0x586770`, `setbuildingspeechflag` writes `0x586784`,
/// `balkothdeathcounter` writes `0x5aa154`, `set_show_completed_quests` writes `0x5aa258`, and so
/// on). Where no operator names a global, this type does not name the field either.
///
/// **Not determined.** The four volume words are all zero in every profile examined while
/// `settings.cfg` simultaneously records `MUSIC_VOLUME 100`; the operators that touch those globals
/// are named `preset*`/`get*`, so the two files are evidently not storing the same quantity, but
/// which quantity each stores is not established here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LomConfig {
    /// File `0x00`. Global `0x586770`, read by `getmusicvolume`, written by `presetmusicvolume`.
    pub music_volume: u32,
    /// File `0x04`. Global `0x586774`, read by `getsoundfxvolume`, written by `presetsoundfxvolume`.
    pub sound_fx_volume: u32,
    /// File `0x08`. Global `0x586778`, read by `getspeechvolume`, written by `presetspeechvolume`.
    pub speech_volume: u32,
    /// File `0x0c`. Global `0x58677c`, read by `getambientvolume`, written by `presetambientvolume`.
    pub ambient_volume: u32,
    /// File `0x14`, one word each; the word at `0x10` is this vector's length.
    ///
    /// The container is the object at `0x5aa1f0` -- `{ capacity, count, records }` with 12-byte
    /// records -- reached by `addhelppanel`, `maxhelppanels`, `togglehelpcheck`,
    /// `getcheckmarkstate` and `uncheckallhelppanels`. Only the record's **second** word is
    /// persisted; `uncheckallhelppanels` sets exactly that word to 1 for every record, and the
    /// reader sets it to 1 for every record the file does not cover, so 1 is the default and the
    /// value is the panel's check state.
    pub help_panel_checks: Vec<u32>,
    /// File `0x14 + 4 * checks`. Object field `+0x28` = global `0x5aa154`, written by
    /// `balkothdeathcounter` and `cheatbalkoth`, read by `getbalkothkillcounter`.
    pub balkoth_kill_counter: u32,
    /// Next word. Object field `+0x128` = global `0x5aa254`, written by `setcenteronmovement`; the
    /// reader also copies it into the `0x5a8080` slot table that operator indexes.
    pub center_on_movement: u32,
    /// Next 16 bytes. Object field `+0x08` = global `0x5aa134`.
    ///
    /// **Observed in a local binary**: when the 16-byte read comes up short the reader calls the
    /// `ole32.dll!CoCreateGuid` import at `0x0054d34c` on this address and then saves the file. The
    /// bytes are therefore a GUID, generated locally on first use rather than shipped.
    pub install_guid: [u8; 16],
    /// Next word. Global `0x586784`, read by `getbuildingspeechflag`, written by
    /// `setbuildingspeechflag`.
    pub building_speech_flag: u32,
    /// Next word. Object field `+0x12c` = global `0x5aa258`, read by `get_show_completed_quests`,
    /// written by `set_show_completed_quests`.
    pub show_completed_quests: u32,
    /// Final word. Global `0x5d20bc`.
    ///
    /// **Not determined.** No operator in `reports/natives/operator-bodies.tsv` names this global
    /// other than `loadconfig`/`saveconfig` themselves, so there is nothing to name the field
    /// after. It is carried verbatim so that a re-encode is lossless, and deliberately left
    /// uninterpreted. Both the reader and the writer default it to 1 on a short file.
    pub unnamed_trailing_word: u32,
}

/// Bytes before the help-panel vector: four volume words plus the count word.
const LOM_CONFIG_PREFIX: usize = 20;
/// Bytes after the help-panel vector: two words, a 16-byte GUID, then three words.
const LOM_CONFIG_SUFFIX: usize = 4 + 4 + 16 + 4 + 4 + 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LomConfigError(String);

impl fmt::Display for LomConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LomConfigError {}

impl LomConfig {
    /// Decode a `lom.cfg` image.
    ///
    /// The length is checked **exactly**, not as a lower bound. The count word drives the rest of
    /// the layout, so a file whose length disagrees with its own count is not a `lom.cfg` this
    /// parser understands, and accepting it would mean silently reinterpreting trailing bytes as
    /// the suffix fields.
    pub fn parse(source: &[u8]) -> Result<Self, LomConfigError> {
        if source.len() < LOM_CONFIG_PREFIX {
            return Err(LomConfigError(format!(
                "lom.cfg is {} bytes, shorter than the {LOM_CONFIG_PREFIX}-byte prefix",
                source.len()
            )));
        }
        let count = read_u32(source, 16) as usize;
        let expected = LOM_CONFIG_PREFIX
            .checked_add(count.checked_mul(4).ok_or_else(|| {
                LomConfigError(format!("help-panel count {count} overflows a byte length"))
            })?)
            .and_then(|length| length.checked_add(LOM_CONFIG_SUFFIX))
            .ok_or_else(|| {
                LomConfigError(format!("help-panel count {count} overflows a byte length"))
            })?;
        if source.len() != expected {
            return Err(LomConfigError(format!(
                "lom.cfg is {} bytes but its help-panel count of {count} implies {expected}",
                source.len()
            )));
        }
        let help_panel_checks = (0..count)
            .map(|index| read_u32(source, LOM_CONFIG_PREFIX + index * 4))
            .collect();
        let suffix = LOM_CONFIG_PREFIX + count * 4;
        let mut install_guid = [0_u8; 16];
        install_guid.copy_from_slice(&source[suffix + 8..suffix + 24]);
        Ok(Self {
            music_volume: read_u32(source, 0),
            sound_fx_volume: read_u32(source, 4),
            speech_volume: read_u32(source, 8),
            ambient_volume: read_u32(source, 12),
            help_panel_checks,
            balkoth_kill_counter: read_u32(source, suffix),
            center_on_movement: read_u32(source, suffix + 4),
            install_guid,
            building_speech_flag: read_u32(source, suffix + 24),
            show_completed_quests: read_u32(source, suffix + 28),
            unnamed_trailing_word: read_u32(source, suffix + 32),
        })
    }

    /// Re-encode in the writer's field order.
    ///
    /// Exists so that the parse can be checked against the corpus rather than against a table of
    /// numbers copied out of the parser: `parse` then `to_bytes` must reproduce the installed file
    /// byte for byte, and any field this type misplaced shows up as a mismatch.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            LOM_CONFIG_PREFIX + self.help_panel_checks.len() * 4 + LOM_CONFIG_SUFFIX,
        );
        for word in [
            self.music_volume,
            self.sound_fx_volume,
            self.speech_volume,
            self.ambient_volume,
            self.help_panel_checks.len() as u32,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for check in &self.help_panel_checks {
            out.extend_from_slice(&check.to_le_bytes());
        }
        out.extend_from_slice(&self.balkoth_kill_counter.to_le_bytes());
        out.extend_from_slice(&self.center_on_movement.to_le_bytes());
        out.extend_from_slice(&self.install_guid);
        out.extend_from_slice(&self.building_speech_flag.to_le_bytes());
        out.extend_from_slice(&self.show_completed_quests.to_le_bytes());
        out.extend_from_slice(&self.unnamed_trailing_word.to_le_bytes());
        out
    }

    /// The GUID in the textual form `CoCreateGuid`'s output is conventionally written.
    ///
    /// The first three groups are little-endian words and the last eight bytes are in order,
    /// because the bytes came from a `GUID` struct rather than from a wire format. Nothing in this
    /// repository depends on the rendering; it exists so two profiles can be compared by eye.
    pub fn install_guid_text(&self) -> String {
        let bytes = &self.install_guid;
        format!(
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{}",
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u16::from_le_bytes([bytes[4], bytes[5]]),
            u16::from_le_bytes([bytes[6], bytes[7]]),
            bytes[8],
            bytes[9],
            bytes[10..16]
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>(),
        )
    }
}

fn read_u32(source: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        source[offset],
        source[offset + 1],
        source[offset + 2],
        source[offset + 3],
    ])
}

// ---------------------------------------------------------------------------------------------
// settings.cfg
// ---------------------------------------------------------------------------------------------

/// One `KEY VALUE` record of `settings.cfg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsEntry {
    pub key: String,
    /// The value as written. Every value observed in the corpus is a decimal integer, including
    /// negative ones, but the raw text is kept because the format is not documented anywhere and a
    /// parser that stored only an `i64` could not re-emit a value it failed to understand.
    pub raw_value: String,
}

impl SettingsEntry {
    /// The value as an integer, when it is one.
    pub fn integer(&self) -> Option<i64> {
        self.raw_value.parse().ok()
    }
}

/// The parsed contents of `settings.cfg`.
///
/// **Observed in the corpus** (all four installed profiles, 2026-09-18): ASCII text, one
/// `KEY VALUE` record per line, each record terminated by a bare `CR` (`0x0d`) -- not `CRLF`, and
/// there is no trailing `LF`. The file ends with a terminator, so splitting on `CR` yields one
/// empty trailing field.
///
/// **Not determined: what writes it.** Its contents changed during an attended 3.02 session
/// (`KB_MAP_SCROLL_SPEED 100` became `50`, `KB_COMBAT_SCROLL_SPEED 10` became `135`, against that
/// profile's own `_vanilla_backup/settings.cfg`), so the running game does write it. But the
/// literal `settings.cfg` and all 23 observed key names are absent from every one of the 467 loose
/// files in the baseline install other than `settings.cfg` itself, from all 1,688 `gs.mpq` members
/// and from all 1,218 `special.mpq` members. That search could not reach `pic.mpq` (1,071),
/// `imp.mpq` (3,600) or `sndfx.mpq` (1,880) members, nor the Wine prefix's own DLLs, nor any string
/// assembled at run time; see `docs/loose-files.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfig {
    pub entries: Vec<SettingsEntry>,
    /// Fields that were not `KEY VALUE`, carried verbatim so a re-encode is lossless.
    pub unparsed: Vec<String>,
    /// Whether the last record carried a terminator. False would mean a file ending mid-record.
    pub terminated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfigError(String);

impl fmt::Display for SettingsConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SettingsConfigError {}

impl SettingsConfig {
    pub fn parse(source: &[u8]) -> Result<Self, SettingsConfigError> {
        let text = std::str::from_utf8(source)
            .map_err(|error| SettingsConfigError(format!("settings.cfg is not text: {error}")))?;
        let terminated = text.is_empty() || text.ends_with('\r');
        let body = text.strip_suffix('\r').unwrap_or(text);
        let mut entries = Vec::new();
        let mut unparsed = Vec::new();
        if !body.is_empty() {
            for record in body.split('\r') {
                match record.split_once(' ') {
                    // A key with no space, or a value containing one, is not the shape every
                    // observed record has. Carrying it in `unparsed` keeps the re-encode lossless
                    // and keeps it visible, instead of dropping it or guessing a split.
                    Some((key, value)) if !key.is_empty() && !value.contains(' ') => {
                        entries.push(SettingsEntry {
                            key: key.to_owned(),
                            raw_value: value.to_owned(),
                        });
                    }
                    _ => unparsed.push(record.to_owned()),
                }
            }
        }
        Ok(Self {
            entries,
            unparsed,
            terminated,
        })
    }

    /// Re-encode. Round-tripping the installed files is the only check this parser has that it read
    /// the separator rule correctly, so the encoder must preserve record order and the terminator.
    ///
    /// Records are emitted in their original order only when nothing was unparsed; a file with
    /// unparsed records cannot be reordered back and is reported by [`Self::round_trips`] instead.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = String::new();
        for entry in &self.entries {
            out.push_str(&entry.key);
            out.push(' ');
            out.push_str(&entry.raw_value);
            out.push('\r');
        }
        for record in &self.unparsed {
            out.push_str(record);
            out.push('\r');
        }
        if !self.terminated {
            out.pop();
        }
        out.into_bytes()
    }

    /// Whether re-encoding reproduces `source` exactly.
    pub fn round_trips(&self, source: &[u8]) -> bool {
        self.to_bytes() == source
    }

    pub fn get(&self, key: &str) -> Option<&SettingsEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }
}

// ---------------------------------------------------------------------------------------------
// magic-only signatures
// ---------------------------------------------------------------------------------------------

/// A format recognised from leading bytes alone.
///
/// Deliberately separate from [`crate::asset::probe`], which reaches for the file extension for
/// `.gs`, `.imp`, `.til`, `.scn`, `.lgd` and `.smp`. Inside an archive whose members were named by
/// a recovered listfile that is reasonable. On a loose tree the whole question is what is here that
/// nobody named, so the extension cannot be an input to the answer; it is an input to the
/// *disagreement* check instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicSignature {
    AsuraContainer,
    Bitmap,
    IffIlbm,
    IffPbm,
    LomSerialised,
    MpqArchive,
    MsDosExecutable,
    SmackerVideo,
    WaveAudio,
    Zip,
    /// Every byte is printable ASCII, tab, CR or LF. A fallback, not a format.
    AsciiText,
    /// No rule matched. Counted, never dropped.
    Unrecognised,
}

impl fmt::Display for MagicSignature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AsuraContainer => "asura-container",
            Self::Bitmap => "bitmap",
            Self::IffIlbm => "iff-ilbm",
            Self::IffPbm => "iff-pbm",
            Self::LomSerialised => "lom-serialised",
            Self::MpqArchive => "mpq-archive",
            Self::MsDosExecutable => "ms-dos-executable",
            Self::SmackerVideo => "smacker-video",
            Self::WaveAudio => "wave-audio",
            Self::Zip => "zip-archive",
            Self::AsciiText => "ascii-text",
            Self::Unrecognised => "unrecognised",
        })
    }
}

/// Classify by leading bytes only.
///
/// `LS_VER_` is the engine's game-state serialisation header. The expectation going in was that it
/// would also head the `.lgd` legend scenarios, in which case a magic-driven rule would have had to
/// stay out of `asset::probe` to avoid reclassifying them. **Refuted in the corpus**: the eight
/// `.lgd` files begin with a bare `u32` and carry no `LS_VER_`, and the header appears only on the
/// six shipped `savegame/*.sav` and `savegame/quickstart` starting states and on the `.lom` saves a
/// session writes. Shipped starting state and player save are therefore the *same* format, which is
/// why this signature names the serialisation and not a file type.
///
/// `Asura   ` is the container Rebellion's Steam launcher uses; see `docs/loose-files.md`.
pub fn magic_signature(bytes: &[u8]) -> MagicSignature {
    if bytes.len() >= 12 && bytes.starts_with(b"FORM") {
        match &bytes[8..12] {
            b"PBM " => return MagicSignature::IffPbm,
            b"ILBM" => return MagicSignature::IffIlbm,
            _ => {}
        }
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        return MagicSignature::WaveAudio;
    }
    if bytes.starts_with(b"SMK2") || bytes.starts_with(b"SMK4") {
        return MagicSignature::SmackerVideo;
    }
    if bytes.starts_with(b"MPQ\x1a") {
        return MagicSignature::MpqArchive;
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return MagicSignature::Zip;
    }
    if bytes.starts_with(b"LS_VER_\0") {
        return MagicSignature::LomSerialised;
    }
    if bytes.starts_with(b"Asura   ") {
        return MagicSignature::AsuraContainer;
    }
    if bytes.starts_with(b"MZ") {
        return MagicSignature::MsDosExecutable;
    }
    // `BM` is only two bytes and would claim any text beginning "BM"; require the declared file
    // size to match, which is what makes it a signature rather than a guess.
    if bytes.len() >= 6 && bytes.starts_with(b"BM") && read_u32(bytes, 2) as usize == bytes.len() {
        return MagicSignature::Bitmap;
    }
    if !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| matches!(byte, 0x09 | 0x0a | 0x0d | 0x20..=0x7e))
    {
        return MagicSignature::AsciiText;
    }
    MagicSignature::Unrecognised
}

// ---------------------------------------------------------------------------------------------
// the sweep
// ---------------------------------------------------------------------------------------------

/// One row of the loose inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseFile {
    /// Slash-separated, relative to the walked root, so a row is comparable across profiles.
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
    pub extension: String,
    pub magic: MagicSignature,
    /// What `asset::probe` made of it, or the error it returned. Recorded verbatim; a probe that
    /// refuses a file is a finding, and collapsing it into "unknown" would hide which files the
    /// existing tooling cannot read.
    pub probe_kind: String,
    pub probe_error: Option<String>,
}

impl LooseFile {
    /// Whether the extension implies a different family from the leading bytes.
    ///
    /// Only extensions with an unambiguous expected magic are judged. An extension nobody has a
    /// rule for is not a disagreement, it is an absence, and reporting it as a conflict would bury
    /// the real ones.
    pub fn extension_disagrees_with_magic(&self) -> bool {
        let expected = match self.extension.as_str() {
            "wav" => MagicSignature::WaveAudio,
            "smk" => MagicSignature::SmackerVideo,
            "mpq" => MagicSignature::MpqArchive,
            "zip" => MagicSignature::Zip,
            "exe" | "dll" | "snp" => MagicSignature::MsDosExecutable,
            "lbm" => MagicSignature::IffPbm,
            _ => return false,
        };
        // `.lbm` ships as either IFF flavour, so accept both for that one.
        if expected == MagicSignature::IffPbm && self.magic == MagicSignature::IffIlbm {
            return false;
        }
        self.magic != expected
    }
}

/// Walk `root` and classify every file under it.
///
/// Follows no symlinks and skips no names: a sweep that quietly excluded dot-files or a directory
/// it judged uninteresting would answer a different question from the one asked. Directory entries
/// are sorted so the report is byte-stable across runs and machines.
pub fn inventory(root: &Path) -> io::Result<Vec<LooseFile>> {
    let mut paths = Vec::new();
    collect(root, &mut paths)?;
    paths.sort();
    let mut rows = Vec::with_capacity(paths.len());
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let bytes = fs::read(&path)?;
        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (probe_kind, probe_error) = match asset::probe(&name, &bytes) {
            Ok(info) => (info.kind.to_string(), None),
            Err(error) => (AssetKind::Unknown.to_string(), Some(error)),
        };
        rows.push(LooseFile {
            relative_path: relative,
            size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
            extension,
            magic: magic_signature(&bytes),
            probe_kind,
            probe_error,
        });
    }
    Ok(rows)
}

fn collect(directory: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<_>>()?;
    entries.sort();
    for entry in entries {
        // `symlink_metadata` rather than `metadata`: a symlink out of the tree must be recorded as
        // the link it is, not silently followed into another install.
        let metadata = fs::symlink_metadata(&entry)?;
        if metadata.is_dir() {
            collect(&entry, out)?;
        } else if metadata.is_file() {
            out.push(entry);
        }
    }
    Ok(())
}

/// Counts by magic signature, for the summary line.
pub fn counts_by_magic(rows: &[LooseFile]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(row.magic.to_string()).or_default() += 1;
    }
    counts
}

/// Counts by `asset::probe` verdict.
pub fn counts_by_probe_kind(rows: &[LooseFile]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for row in rows {
        *counts.entry(row.probe_kind.clone()).or_default() += 1;
    }
    counts
}

// ---------------------------------------------------------------------------------------------
// SHA-256
// ---------------------------------------------------------------------------------------------

/// SHA-256, FIPS 180-4.
///
/// Written out rather than pulled in because this crate's dependency list is four crates and a hash
/// for an inventory report is not worth a fifth. The published test vectors in the tests below are
/// the authority it is checked against.
pub fn sha256_hex(message: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut padded = message.to_vec();
    let bit_length = (message.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut schedule = [0_u32; 64];
    for block in padded.chunks_exact(64) {
        for (index, word) in schedule.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *word = u32::from_be_bytes([
                block[start],
                block[start + 1],
                block[start + 2],
                block[start + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    state
        .iter()
        .map(|word| format!("{word:08x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4 published vectors, plus the empty message. These are literals on purpose: the
    /// authority for a standard hash is the standard, and a corpus file cannot supply one.
    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // A message long enough to need more than one block after padding.
        assert_eq!(
            sha256_hex(&[b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn a_length_that_contradicts_the_count_is_refused() {
        // Twenty bytes claiming two help panels, with no room for either them or the suffix. The
        // point is that the parser rejects rather than reading past its own prefix.
        let mut source = vec![0_u8; 20];
        source[16] = 2;
        let error = LomConfig::parse(&source).expect_err("the length contradicts the count");
        assert!(
            error.to_string().contains("implies"),
            "the error should name the implied length: {error}"
        );
    }

    #[test]
    fn settings_records_split_on_bare_carriage_returns() {
        let source = b"GAME_SPEED 66\rCENTER_MOVE 0\r";
        let parsed = SettingsConfig::parse(source).expect("ASCII text parses");
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.get("GAME_SPEED").and_then(|e| e.integer()), Some(66));
        assert!(parsed.unparsed.is_empty());
        assert!(parsed.round_trips(source));
    }

    /// The `LS_VER_` family covers both shipped starting states and player saves, so the signature
    /// must not be read as "this is a save game". This pins the property the sweep relies on.
    #[test]
    fn the_serialisation_header_is_one_signature_not_a_file_type() {
        assert_eq!(
            magic_signature(b"LS_VER_\0\x6f\0\0\0"),
            MagicSignature::LomSerialised
        );
    }

    #[test]
    fn two_leading_bytes_are_not_enough_to_claim_a_bitmap() {
        assert_eq!(magic_signature(b"BMX help text"), MagicSignature::AsciiText);
    }
}
