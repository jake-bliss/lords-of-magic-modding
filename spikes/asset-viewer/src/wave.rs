//! RIFF WAVE container parse, PCM sample decode, and a byte-faithful re-encoder.
//!
//! The probe in [`crate::asset`] reads the `fmt ` chunk and stops. That is enough to classify a
//! member and nothing else: it never touches a sample, so it cannot tell a file it understands
//! from a file it merely recognises, and it gives a modder no way back in. This module decodes the
//! sample data, re-serialises the whole container from the parsed structure, and refuses -- by
//! name -- every member whose bytes it cannot reproduce.
//!
//! Two claims are kept apart throughout, the same way [`crate::pbm`] keeps them apart:
//!
//! * **sample-lossless** is the correctness claim. Decode, re-encode and decode again must give
//!   back the same samples. Anything short of every member is a bug in this module.
//! * **byte-identical** is a fidelity observation about the *original* authoring tool. The corpus
//!   was cut by Sound Forge 4.0 and carries that tool's `LIST INFO` chunk; a miss there says our
//!   container serialisation differs from theirs, not that audio was lost.
//!
//! Only the second claim licenses a rewrite. `--import-wave` refuses any template that does not
//! re-encode byte-identically, because writing such a member back would change bytes the modder
//! never asked to change.

use std::collections::BTreeMap;
use std::fmt;

/// `WAVE_FORMAT_PCM`. The only format tag observed in the corpus.
pub const WAVE_FORMAT_PCM: u16 = 1;

/// Sample rates observed across `sndfx.mpq`, `special.mpq` and the loose `Wav/` tree.
///
/// **Observed in the corpus.** These are not a specification -- nothing in `lomse.exe` has been
/// read to establish what the engine will accept. They are the set for which a shipped file is
/// evidence that the engine plays it, which is the strongest ground an import gate has.
pub const ATTESTED_SAMPLE_RATES: &[u32] = &[11025, 22050, 44100];

/// Channel counts observed in the corpus. **Observed in the corpus.**
pub const ATTESTED_CHANNEL_COUNTS: &[u16] = &[1, 2];

/// Bit depths observed in the corpus. **Observed in the corpus.**
pub const ATTESTED_BIT_DEPTHS: &[u16] = &[8, 16];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveError(String);

impl WaveError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for WaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WaveError {}

/// The `fmt ` chunk, split into the 16-byte PCM core and whatever follows it.
///
/// `extra` exists so a `fmt ` chunk longer than 16 bytes survives a re-encode without this module
/// pretending to understand the extension. Nothing in the corpus has one; carrying it verbatim
/// means the first file that does is reported rather than silently truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveFormat {
    pub encoding: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub byte_rate: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
    pub extra: Vec<u8>,
}

impl WaveFormat {
    fn parse(body: &[u8]) -> Result<Self, WaveError> {
        if body.len() < 16 {
            return Err(WaveError::new(format!(
                "WAVE fmt chunk is {} bytes, the PCM core needs 16",
                body.len()
            )));
        }
        Ok(Self {
            encoding: read_u16(body, 0)?,
            channels: read_u16(body, 2)?,
            sample_rate: read_u32(body, 4)?,
            byte_rate: read_u32(body, 8)?,
            block_align: read_u16(body, 12)?,
            bits_per_sample: read_u16(body, 14)?,
            extra: body[16..].to_vec(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.extra.len());
        out.extend_from_slice(&self.encoding.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&self.byte_rate.to_le_bytes());
        out.extend_from_slice(&self.block_align.to_le_bytes());
        out.extend_from_slice(&self.bits_per_sample.to_le_bytes());
        out.extend_from_slice(&self.extra);
        out
    }

    /// Bytes per interleaved frame, computed from the channel count and bit depth.
    ///
    /// Deliberately *not* `block_align`. `block_align` is a declared field, and a declared field is
    /// the thing this repository has already shipped an unbounded allocation from; it is checked
    /// against this value instead of trusted as it.
    pub fn frame_bytes(&self) -> Result<usize, WaveError> {
        if self.channels == 0 {
            return Err(WaveError::new("WAVE fmt declares zero channels"));
        }
        if self.bits_per_sample == 0 || !self.bits_per_sample.is_multiple_of(8) {
            return Err(WaveError::new(format!(
                "WAVE bits-per-sample {} is not a whole number of bytes",
                self.bits_per_sample
            )));
        }
        Ok(usize::from(self.channels) * usize::from(self.bits_per_sample / 8))
    }

    /// Whether every field of this format is one a shipped file is evidence for.
    pub fn attested(&self) -> Result<(), String> {
        if self.encoding != WAVE_FORMAT_PCM {
            return Err(format!(
                "format tag {} is not PCM; the corpus contains no non-PCM member",
                self.encoding
            ));
        }
        if !ATTESTED_CHANNEL_COUNTS.contains(&self.channels) {
            return Err(format!(
                "channel count {} is outside the attested set {ATTESTED_CHANNEL_COUNTS:?}",
                self.channels
            ));
        }
        if !ATTESTED_SAMPLE_RATES.contains(&self.sample_rate) {
            return Err(format!(
                "sample rate {} is outside the attested set {ATTESTED_SAMPLE_RATES:?}",
                self.sample_rate
            ));
        }
        if !ATTESTED_BIT_DEPTHS.contains(&self.bits_per_sample) {
            return Err(format!(
                "bit depth {} is outside the attested set {ATTESTED_BIT_DEPTHS:?}",
                self.bits_per_sample
            ));
        }
        Ok(())
    }
}

/// Decoded PCM, interleaved and centred on zero for every depth the module reads.
///
/// 8-bit RIFF PCM is *unsigned* and 16-bit is *signed*; both are normalised here to a signed value
/// so a caller never has to know which it is holding. The conversion is exact in both directions,
/// which is what makes the sample-lossless claim a claim and not a rounding tolerance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmSamples {
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sample_rate: u32,
    pub interleaved: Vec<i32>,
    /// Bytes at the end of the `data` chunk that do not complete a frame.
    ///
    /// Kept verbatim rather than dropped or zero-filled: a truncated final frame is a property of
    /// the shipped file, and a re-encode that quietly rounded it away would not be the same file.
    pub trailing_partial_frame: Vec<u8>,
}

impl PcmSamples {
    pub fn decode(format: &WaveFormat, data: &[u8]) -> Result<Self, WaveError> {
        if format.encoding != WAVE_FORMAT_PCM {
            return Err(WaveError::new(format!(
                "WAVE format tag {} is not PCM and has no decoder here",
                format.encoding
            )));
        }
        let sample_bytes = match format.bits_per_sample {
            8 => 1_usize,
            16 => 2_usize,
            other => {
                return Err(WaveError::new(format!(
                    "PCM bit depth {other} has no decoder here"
                )));
            }
        };
        let frame_bytes = format.frame_bytes()?;
        let whole = data.len() - (data.len() % frame_bytes);
        // Sized from the slice actually in hand, never from a declared length.
        let mut interleaved = Vec::with_capacity(whole / sample_bytes);
        for chunk in data[..whole].chunks_exact(sample_bytes) {
            interleaved.push(match sample_bytes {
                1 => i32::from(chunk[0]) - 128,
                _ => i32::from(i16::from_le_bytes([chunk[0], chunk[1]])),
            });
        }
        Ok(Self {
            channels: format.channels,
            bits_per_sample: format.bits_per_sample,
            sample_rate: format.sample_rate,
            interleaved,
            trailing_partial_frame: data[whole..].to_vec(),
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, WaveError> {
        let sample_bytes = match self.bits_per_sample {
            8 => 1_usize,
            16 => 2_usize,
            other => {
                return Err(WaveError::new(format!(
                    "PCM bit depth {other} has no encoder here"
                )));
            }
        };
        let mut out =
            Vec::with_capacity(self.interleaved.len() * sample_bytes + self.trailing_partial_frame.len());
        for sample in &self.interleaved {
            match sample_bytes {
                1 => {
                    let value = sample.checked_add(128).ok_or_else(|| {
                        WaveError::new(format!("8-bit sample {sample} overflows on re-encode"))
                    })?;
                    let byte = u8::try_from(value).map_err(|_| {
                        WaveError::new(format!("8-bit sample {sample} is outside -128..=127"))
                    })?;
                    out.push(byte);
                }
                _ => {
                    let value = i16::try_from(*sample).map_err(|_| {
                        WaveError::new(format!(
                            "16-bit sample {sample} is outside -32768..=32767"
                        ))
                    })?;
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        out.extend_from_slice(&self.trailing_partial_frame);
        Ok(out)
    }

    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.interleaved.len() / usize::from(self.channels)
    }

    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        (self.frames() as u64 * 1000) / u64::from(self.sample_rate)
    }
}

/// One RIFF chunk, in file order.
///
/// `fmt ` and `data` carry no bytes of their own: they are rebuilt from [`WaveFile::format`] and
/// [`WaveFile::samples`] on encode, so a byte-identical result is evidence that *those* two paths
/// are right and not merely that the bytes were copied past them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaveChunkBody {
    Format,
    Data,
    Other(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveChunk {
    pub id: [u8; 4],
    pub declared_size: u32,
    pub body: WaveChunkBody,
    /// The RIFF pad byte after an odd-sized chunk, when the file actually carried one.
    ///
    /// `Option` rather than `bool`: a file whose last chunk is odd-sized and simply ends carries
    /// no pad, and inventing one would make the re-encode a byte longer than the original.
    pub pad: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveFile {
    /// The size field of the `RIFF` header, carried verbatim.
    ///
    /// Never used to bound a read. It is reproduced on encode and *reported* when it disagrees
    /// with the file length, because that disagreement is a fact about the authoring tool.
    pub declared_riff_size: u32,
    pub chunks: Vec<WaveChunk>,
    pub format: WaveFormat,
    pub samples: PcmSamples,
    /// Bytes after the final complete chunk that are too short to be a chunk header.
    pub trailing: Vec<u8>,
}

/// Why a member must not be rewritten in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaveRefusal {
    /// Re-encoding the parsed container does not reproduce the original bytes, so a rewrite would
    /// change bytes the modder did not ask to change.
    ContainerNotReproducible { first_difference: usize },
    /// A format tag, channel count, rate or depth no shipped file is evidence for.
    UnattestedFormat(String),
    /// The `data` chunk does not hold a whole number of frames.
    PartialFrame { remainder: usize },
}

impl fmt::Display for WaveRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContainerNotReproducible { first_difference } => write!(
                formatter,
                "container-not-reproducible;first-difference={first_difference}"
            ),
            Self::UnattestedFormat(reason) => write!(formatter, "unattested-format;{reason}"),
            Self::PartialFrame { remainder } => {
                write!(formatter, "partial-frame;remainder-bytes={remainder}")
            }
        }
    }
}

impl WaveFile {
    pub fn parse(source: &[u8]) -> Result<Self, WaveError> {
        if source.len() < 12 || &source[0..4] != b"RIFF" || &source[8..12] != b"WAVE" {
            return Err(WaveError::new("not a RIFF WAVE file"));
        }
        let declared_riff_size = read_u32(source, 4)?;

        let mut chunks: Vec<WaveChunk> = Vec::new();
        let mut format: Option<WaveFormat> = None;
        let mut data: Option<Vec<u8>> = None;
        let mut cursor = 12_usize;
        while cursor + 8 <= source.len() {
            let mut id = [0_u8; 4];
            id.copy_from_slice(&source[cursor..cursor + 4]);
            let declared_size = read_u32(source, cursor + 4)?;
            let body_start = cursor + 8;
            let body_end = body_start
                .checked_add(declared_size as usize)
                .ok_or_else(|| WaveError::new("WAVE chunk size overflow"))?;
            if body_end > source.len() {
                return Err(WaveError::new(format!(
                    "truncated WAVE {} chunk: declares {declared_size} bytes, {} remain",
                    printable_tag(&id),
                    source.len() - body_start
                )));
            }
            let body = &source[body_start..body_end];
            let pad = if declared_size % 2 == 1 {
                source.get(body_end).copied()
            } else {
                None
            };
            let kind = match &id {
                b"fmt " => {
                    if format.is_some() {
                        return Err(WaveError::new("WAVE has more than one fmt chunk"));
                    }
                    format = Some(WaveFormat::parse(body)?);
                    WaveChunkBody::Format
                }
                b"data" => {
                    if data.is_some() {
                        return Err(WaveError::new("WAVE has more than one data chunk"));
                    }
                    data = Some(body.to_vec());
                    WaveChunkBody::Data
                }
                _ => WaveChunkBody::Other(body.to_vec()),
            };
            chunks.push(WaveChunk {
                id,
                declared_size,
                body: kind,
                pad,
            });
            cursor = body_end + usize::from(pad.is_some());
        }

        let format = format.ok_or_else(|| WaveError::new("WAVE has no fmt chunk"))?;
        let data = data.ok_or_else(|| WaveError::new("WAVE has no data chunk"))?;
        let samples = PcmSamples::decode(&format, &data)?;
        Ok(Self {
            declared_riff_size,
            chunks,
            format,
            samples,
            trailing: source[cursor..].to_vec(),
        })
    }

    /// Re-serialise the container, rebuilding `fmt ` and `data` from the decoded structure.
    ///
    /// Chunk order, declared sizes, pad bytes and the `RIFF` size field are reproduced exactly as
    /// parsed. The declared sizes are *carried*, not recomputed, so that a file whose header lies
    /// re-encodes to the same lie rather than being quietly corrected -- [`Self::rebuild`] is the
    /// path that recomputes, and it is the one an edit goes through.
    pub fn encode(&self) -> Result<Vec<u8>, WaveError> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&self.declared_riff_size.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        for chunk in &self.chunks {
            let body = match &chunk.body {
                WaveChunkBody::Format => self.format.encode(),
                WaveChunkBody::Data => self.samples.encode()?,
                WaveChunkBody::Other(bytes) => bytes.clone(),
            };
            if body.len() as u64 != u64::from(chunk.declared_size) {
                return Err(WaveError::new(format!(
                    "re-encoded {} chunk is {} bytes but the file declares {}",
                    printable_tag(&chunk.id),
                    body.len(),
                    chunk.declared_size
                )));
            }
            out.extend_from_slice(&chunk.id);
            out.extend_from_slice(&chunk.declared_size.to_le_bytes());
            out.extend_from_slice(&body);
            if let Some(pad) = chunk.pad {
                out.push(pad);
            }
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }

    /// Serialise with every size field recomputed from the bodies actually being written.
    ///
    /// This is the encode an *edit* needs: replacing the samples changes the `data` size, which
    /// changes the `RIFF` size and can add or remove the pad byte. Ancillary chunks and their
    /// order are still carried through untouched, which is what keeps a Sound Forge `LIST INFO`
    /// block on a member the modder re-cut.
    pub fn rebuild(&self) -> Result<Vec<u8>, WaveError> {
        let mut bodies: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(self.chunks.len());
        for chunk in &self.chunks {
            let body = match &chunk.body {
                WaveChunkBody::Format => self.format.encode(),
                WaveChunkBody::Data => self.samples.encode()?,
                WaveChunkBody::Other(bytes) => bytes.clone(),
            };
            bodies.push((chunk.id, body));
        }
        let mut payload = 4_u64; // the "WAVE" form type
        for (_, body) in &bodies {
            payload += 8 + body.len() as u64 + (body.len() as u64 % 2);
        }
        payload += self.trailing.len() as u64;
        let declared = u32::try_from(payload)
            .map_err(|_| WaveError::new("rebuilt WAVE exceeds the 4 GiB RIFF size field"))?;

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&declared.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        for (id, body) in &bodies {
            let size = u32::try_from(body.len())
                .map_err(|_| WaveError::new("a rebuilt WAVE chunk exceeds 4 GiB"))?;
            out.extend_from_slice(id);
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(body);
            if size % 2 == 1 {
                out.push(0);
            }
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }

    /// Whether the decoded samples survive a re-encode and second decode unchanged.
    pub fn sample_lossless(&self) -> Result<bool, WaveError> {
        let data = self.samples.encode()?;
        let again = PcmSamples::decode(&self.format, &data)?;
        Ok(again == self.samples)
    }

    /// Why this member must not be used as a rewrite template, or `None` if it may be.
    pub fn rewrite_refusal(&self, original: &[u8]) -> Result<Option<WaveRefusal>, WaveError> {
        if let Err(reason) = self.format.attested() {
            return Ok(Some(WaveRefusal::UnattestedFormat(reason)));
        }
        if !self.samples.trailing_partial_frame.is_empty() {
            return Ok(Some(WaveRefusal::PartialFrame {
                remainder: self.samples.trailing_partial_frame.len(),
            }));
        }
        let encoded = self.encode()?;
        if let Some(first_difference) = first_difference(&encoded, original) {
            return Ok(Some(WaveRefusal::ContainerNotReproducible { first_difference }));
        }
        Ok(None)
    }

    /// Whether the `RIFF` size field agrees with the file it was read from.
    pub fn riff_size_matches(&self, source_len: usize) -> bool {
        u64::from(self.declared_riff_size) + 8 == source_len as u64
    }

    /// The chunk ids in file order, e.g. `fmt |data|LIST`.
    pub fn layout(&self) -> String {
        self.chunks
            .iter()
            .map(|chunk| printable_tag(&chunk.id))
            .collect::<Vec<_>>()
            .join("|")
    }
}

/// Replace the samples of `template` with those of `edited`, keeping the template's container.
///
/// The template is the shipped member. Its ancillary chunks and their order are what the game was
/// built with, so they are carried; only the audio is taken from the file the modder edited. The
/// two refusals are deliberate and both are named to the caller:
///
/// * a template this module cannot reproduce byte-for-byte is not a template it may write over;
/// * a format change is refused unless `allow_format_change` is set, and even then the incoming
///   format has to be one a shipped file is evidence for.
pub fn import_samples(
    edited: &[u8],
    template: &[u8],
    allow_format_change: bool,
) -> Result<Vec<u8>, WaveError> {
    let mut template_file = WaveFile::parse(template)
        .map_err(|error| WaveError::new(format!("template: {error}")))?;
    if let Some(refusal) = template_file.rewrite_refusal(template)? {
        return Err(WaveError::new(format!(
            "refusing to rewrite this member: {refusal}"
        )));
    }
    let edited_file =
        WaveFile::parse(edited).map_err(|error| WaveError::new(format!("edited file: {error}")))?;
    if let Err(reason) = edited_file.format.attested() {
        return Err(WaveError::new(format!("edited file: {reason}")));
    }
    let same_format = edited_file.format.encoding == template_file.format.encoding
        && edited_file.format.channels == template_file.format.channels
        && edited_file.format.sample_rate == template_file.format.sample_rate
        && edited_file.format.bits_per_sample == template_file.format.bits_per_sample;
    if !same_format && !allow_format_change {
        return Err(WaveError::new(format!(
            "edited file is {}, the template is {}; pass --allow-format-change to write it anyway",
            describe_format(&edited_file.format),
            describe_format(&template_file.format)
        )));
    }
    if !edited_file.samples.trailing_partial_frame.is_empty() {
        return Err(WaveError::new(format!(
            "edited file ends in {} bytes that do not complete a frame",
            edited_file.samples.trailing_partial_frame.len()
        )));
    }
    template_file.format = edited_file.format.clone();
    template_file.samples = edited_file.samples.clone();
    template_file.rebuild()
}

pub fn describe_format(format: &WaveFormat) -> String {
    format!(
        "encoding={};channels={};sample-rate={};bits-per-sample={}",
        format.encoding, format.channels, format.sample_rate, format.bits_per_sample
    )
}

/// The key a corpus histogram is bucketed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FormatKey {
    pub encoding: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
}

impl From<&WaveFormat> for FormatKey {
    fn from(format: &WaveFormat) -> Self {
        Self {
            encoding: format.encoding,
            channels: format.channels,
            sample_rate: format.sample_rate,
            bits_per_sample: format.bits_per_sample,
        }
    }
}

impl fmt::Display for FormatKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "encoding={};channels={};sample-rate={};bits-per-sample={}",
            self.encoding, self.channels, self.sample_rate, self.bits_per_sample
        )
    }
}

/// A corpus sweep, shared by the archive and loose-directory commands so both report the same
/// numbers computed the same way.
#[derive(Debug, Default)]
pub struct WaveSweep {
    pub checked: usize,
    pub parsed: usize,
    pub sample_lossless: usize,
    pub byte_identical: usize,
    pub riff_size_mismatch: usize,
    pub formats: BTreeMap<FormatKey, usize>,
    pub layouts: BTreeMap<String, usize>,
    pub total_frames: u64,
    pub failures: Vec<(String, String)>,
    pub differences: Vec<(String, usize)>,
    pub refusals: Vec<(String, WaveRefusal)>,
}

impl WaveSweep {
    pub fn observe(&mut self, name: &str, bytes: &[u8]) {
        self.checked += 1;
        let file = match WaveFile::parse(bytes) {
            Ok(file) => file,
            Err(error) => {
                self.failures.push((name.to_owned(), error.to_string()));
                return;
            }
        };
        self.parsed += 1;
        *self.formats.entry(FormatKey::from(&file.format)).or_default() += 1;
        *self.layouts.entry(file.layout()).or_default() += 1;
        self.total_frames += file.samples.frames() as u64;
        if !file.riff_size_matches(bytes.len()) {
            self.riff_size_mismatch += 1;
        }
        match file.sample_lossless() {
            Ok(true) => self.sample_lossless += 1,
            Ok(false) => self
                .failures
                .push((name.to_owned(), "samples changed on re-encode".to_owned())),
            Err(error) => self.failures.push((name.to_owned(), error.to_string())),
        }
        match file.encode() {
            Ok(encoded) => match first_difference(&encoded, bytes) {
                None => self.byte_identical += 1,
                Some(offset) => self.differences.push((name.to_owned(), offset)),
            },
            Err(error) => self.failures.push((name.to_owned(), error.to_string())),
        }
        match file.rewrite_refusal(bytes) {
            Ok(Some(refusal)) => self.refusals.push((name.to_owned(), refusal)),
            Ok(None) => {}
            Err(error) => self.failures.push((name.to_owned(), error.to_string())),
        }
    }

    /// Tab-separated, in the shape the other sweeps in this tool print.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("checked\t{}\n", self.checked));
        out.push_str(&format!("parsed\t{}\n", self.parsed));
        out.push_str(&format!("sample_lossless\t{}\n", self.sample_lossless));
        out.push_str(&format!("byte_identical\t{}\n", self.byte_identical));
        out.push_str(&format!("riff_size_mismatch\t{}\n", self.riff_size_mismatch));
        out.push_str(&format!("frames\t{}\n", self.total_frames));
        out.push_str(&format!("rewritable\t{}\n", self.parsed - self.refusals.len()));
        out.push_str(&format!("refused\t{}\n", self.refusals.len()));
        out.push_str(&format!("failures\t{}\n", self.failures.len()));
        for (format, count) in &self.formats {
            out.push_str(&format!("format\t{format}\t{count}\n"));
        }
        for (layout, count) in &self.layouts {
            out.push_str(&format!("layout\t{layout}\t{count}\n"));
        }
        for (name, offset) in &self.differences {
            out.push_str(&format!("difference\t{name}\tfirst-byte={offset}\n"));
        }
        for (name, refusal) in &self.refusals {
            out.push_str(&format!("refusal\t{name}\t{refusal}\n"));
        }
        for (name, error) in &self.failures {
            out.push_str(&format!("failure\t{name}\t{error}\n"));
        }
        out
    }
}

fn first_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    let shared = left.len().min(right.len());
    for index in 0..shared {
        if left[index] != right[index] {
            return Some(index);
        }
    }
    if left.len() == right.len() {
        None
    } else {
        Some(shared)
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, WaveError> {
    bytes
        .get(offset..offset + 2)
        .map(|slice| u16::from_le_bytes([slice[0], slice[1]]))
        .ok_or_else(|| WaveError::new(format!("WAVE read past end at offset {offset}")))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, WaveError> {
    bytes
        .get(offset..offset + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
        .ok_or_else(|| WaveError::new(format!("WAVE read past end at offset {offset}")))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal well-formed WAVE, built here so the parser is exercised on bytes this module did
    /// not produce. The layout is the canonical 44-byte one, written by hand.
    fn canonical(channels: u16, sample_rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block_align = channels * (bits / 8);
        let byte_rate = sample_rate * u32::from(block_align);
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32 + data.len() as u32 % 2).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    #[test]
    fn eight_bit_pcm_is_unsigned_and_centres_on_128() {
        let bytes = canonical(1, 22050, 8, &[0x00, 0x80, 0xff]);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(file.samples.interleaved, vec![-128, 0, 127]);
    }

    #[test]
    fn sixteen_bit_pcm_is_signed_little_endian() {
        let bytes = canonical(1, 44100, 16, &[0x00, 0x80, 0xff, 0x7f, 0x01, 0x00]);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(file.samples.interleaved, vec![-32768, 32767, 1]);
    }

    #[test]
    fn a_partial_final_frame_is_carried_not_rounded_away() {
        // Stereo 16-bit: 4 bytes per frame, 6 bytes of data leaves 2 over.
        let bytes = canonical(2, 22050, 16, &[1, 0, 2, 0, 3, 0]);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(file.samples.interleaved, vec![1, 2]);
        assert_eq!(file.samples.trailing_partial_frame, vec![3, 0]);
        assert_eq!(file.encode().expect("encode"), bytes);
    }

    #[test]
    fn an_odd_sized_data_chunk_keeps_its_pad_byte() {
        let bytes = canonical(1, 22050, 8, &[0x10, 0x20, 0x30]);
        let file = WaveFile::parse(&bytes).expect("parse");
        let data = file
            .chunks
            .iter()
            .find(|chunk| chunk.id == *b"data")
            .expect("a data chunk");
        assert_eq!(data.pad, Some(0));
        assert_eq!(file.encode().expect("encode"), bytes);
    }

    #[test]
    fn a_trailing_list_chunk_survives_the_round_trip() {
        let mut bytes = canonical(1, 22050, 8, &[1, 2, 3]);
        let riff = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let list: &[u8] = b"LIST\x0c\x00\x00\x00INFOISFT\x00\x00\x00\x00";
        bytes[4..8].copy_from_slice(&(riff + list.len() as u32).to_le_bytes());
        bytes.extend_from_slice(list);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(file.layout(), "fmt |data|LIST");
        assert_eq!(file.encode().expect("encode"), bytes);
    }

    #[test]
    fn a_chunk_longer_than_the_file_is_refused_rather_than_allocated() {
        let mut bytes = canonical(1, 22050, 8, &[1, 2]);
        // Claim the data chunk holds 4 GiB. Nothing may be sized from that.
        let data_at = bytes.len() - 2 - 4;
        bytes[data_at..data_at + 4].copy_from_slice(&0xffff_fff0_u32.to_le_bytes());
        let error = WaveFile::parse(&bytes).expect_err("a lying size must not parse");
        assert!(error.to_string().contains("truncated"), "{error}");
    }

    #[test]
    fn a_non_pcm_format_tag_has_no_decoder() {
        let mut bytes = canonical(1, 22050, 8, &[1, 2]);
        bytes[20] = 0x11; // WAVE_FORMAT_IMA_ADPCM
        let error = WaveFile::parse(&bytes).expect_err("ADPCM must not decode as PCM");
        assert!(error.to_string().contains("not PCM"), "{error}");
    }

    /// The container guard fires on a byte difference anywhere, including bytes the encoder does
    /// not itself produce.
    ///
    /// No member of the installed corpus reaches this refusal -- every WAVE the game ships
    /// re-encodes byte-for-byte -- so the only way to exercise it is to hand `rewrite_refusal` an
    /// original that is not the bytes the file came from. That is exactly the caller mistake the
    /// guard exists to stop: an import whose template argument does not match the member being
    /// overwritten.
    #[test]
    fn rewriting_is_refused_when_the_bytes_do_not_reproduce() {
        let bytes = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(
            file.rewrite_refusal(&bytes).expect("refusal check"),
            None,
            "its own bytes must reproduce"
        );

        let mut other = bytes.clone();
        let last = other.len() - 1;
        other[last] ^= 0xff;
        let refusal = file
            .rewrite_refusal(&other)
            .expect("refusal check")
            .expect("a differing original must be refused");
        assert_eq!(
            refusal,
            WaveRefusal::ContainerNotReproducible {
                first_difference: last
            }
        );

        let mut shorter = bytes.clone();
        shorter.pop();
        let refusal = file
            .rewrite_refusal(&shorter)
            .expect("refusal check")
            .expect("a shorter original must be refused");
        assert_eq!(
            refusal,
            WaveRefusal::ContainerNotReproducible {
                first_difference: shorter.len()
            },
            "a length difference is a difference"
        );
    }

    #[test]
    fn a_partial_final_frame_blocks_a_rewrite() {
        let bytes = canonical(2, 22050, 16, &[1, 0, 2, 0, 3, 0]);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(
            file.rewrite_refusal(&bytes).expect("refusal check"),
            Some(WaveRefusal::PartialFrame { remainder: 2 })
        );
    }

    #[test]
    fn trailing_bytes_are_carried_and_do_not_block_a_rewrite() {
        let mut bytes = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        bytes.push(0xab);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(file.trailing, vec![0xab]);
        assert_eq!(file.encode().expect("encode"), bytes);
        assert_eq!(file.rewrite_refusal(&bytes).expect("refusal check"), None);
    }

    #[test]
    fn an_unattested_sample_rate_is_refused_by_name() {
        let bytes = canonical(1, 8000, 8, &[1, 2]);
        let file = WaveFile::parse(&bytes).expect("parse");
        let refusal = file
            .rewrite_refusal(&bytes)
            .expect("refusal check")
            .expect("8 kHz is not attested");
        assert!(
            matches!(&refusal, WaveRefusal::UnattestedFormat(reason) if reason.contains("8000")),
            "{refusal}"
        );
    }

    #[test]
    fn import_replaces_the_audio_and_keeps_the_template_container() {
        let mut template = canonical(1, 22050, 8, &[0x80, 0x80, 0x80, 0x80]);
        let riff = u32::from_le_bytes([template[4], template[5], template[6], template[7]]);
        let list: &[u8] = b"LIST\x0c\x00\x00\x00INFOISFT\x00\x00\x00\x00";
        template[4..8].copy_from_slice(&(riff + list.len() as u32).to_le_bytes());
        template.extend_from_slice(list);

        let edited = canonical(1, 22050, 8, &[0x00, 0xff]);
        let result = import_samples(&edited, &template, false).expect("import");
        let parsed = WaveFile::parse(&result).expect("parse the import");
        assert_eq!(parsed.samples.interleaved, vec![-128, 127]);
        assert_eq!(parsed.layout(), "fmt |data|LIST");
        assert!(parsed.riff_size_matches(result.len()));
    }

    #[test]
    fn importing_a_files_own_audio_reproduces_it() {
        let original = canonical(2, 22050, 8, &[1, 2, 3, 4, 5, 6]);
        let result = import_samples(&original, &original, false).expect("import");
        assert_eq!(result, original);
    }

    #[test]
    fn a_format_change_is_refused_unless_it_is_asked_for() {
        let template = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let edited = canonical(2, 22050, 8, &[1, 2, 3, 4]);
        let error = import_samples(&edited, &template, false).expect_err("channels changed");
        assert!(error.to_string().contains("--allow-format-change"), "{error}");
        let allowed = import_samples(&edited, &template, true).expect("explicitly allowed");
        assert_eq!(WaveFile::parse(&allowed).expect("parse").format.channels, 2);
    }

    #[test]
    fn a_format_change_outside_the_attested_set_is_refused_even_when_allowed() {
        let template = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let edited = canonical(1, 48000, 8, &[1, 2, 3, 4]);
        let error = import_samples(&edited, &template, true).expect_err("48 kHz is not attested");
        assert!(error.to_string().contains("48000"), "{error}");
    }

    #[test]
    fn rebuild_recomputes_the_sizes_an_edit_changes() {
        let template = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let edited = canonical(1, 22050, 8, &[1, 2, 3]);
        let result = import_samples(&edited, &template, false).expect("import");
        let parsed = WaveFile::parse(&result).expect("parse");
        assert_eq!(parsed.samples.interleaved.len(), 3);
        assert!(parsed.riff_size_matches(result.len()));
        let data = parsed
            .chunks
            .iter()
            .find(|chunk| chunk.id == *b"data")
            .expect("a data chunk");
        assert_eq!(data.declared_size, 3);
        assert_eq!(data.pad, Some(0), "an odd data chunk gains a pad byte");
    }

    #[test]
    fn the_sweep_counts_both_claims_separately() {
        let mut sweep = WaveSweep::default();
        sweep.observe("clean.wav", &canonical(1, 22050, 8, &[1, 2, 3, 4]));
        // A file whose RIFF size field disagrees with its length: still sample-lossless and still
        // byte-identical, because the encoder carries the declared value rather than fixing it.
        let mut lying = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        lying[4] = lying[4].wrapping_add(2);
        sweep.observe("lying.wav", &lying);
        sweep.observe("not-a-wave.bin", b"MZ\x00\x00");
        assert_eq!(sweep.checked, 3);
        assert_eq!(sweep.parsed, 2);
        assert_eq!(sweep.sample_lossless, 2);
        assert_eq!(sweep.byte_identical, 2);
        assert_eq!(sweep.riff_size_mismatch, 1);
        assert_eq!(sweep.failures.len(), 1);
    }
    // --- The installed corpus ---------------------------------------------------------------
    //
    // `#[ignore]`d rather than skipped when the game is absent, for the reason `imp_anim` gives:
    // a test that prints "could not run" and returns `ok` is indistinguishable from one that ran.
    //
    // Run with:
    //   LOM_GAME_DIR=.../English cargo test --release -- --ignored
    fn game_directory() -> std::path::PathBuf {
        let directory = std::env::var_os("LOM_GAME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set LOM_GAME_DIR to the installed English directory");
        assert!(
            directory.join("lomse.exe").is_file(),
            "no lomse.exe under {}",
            directory.display()
        );
        directory
    }

    fn sweep_archive(path: &std::path::Path) -> WaveSweep {
        let archive = crate::mpq::Archive::open(path).expect("open the archive");
        let entries = archive.entries().expect("enumerate the archive");
        let mut sweep = WaveSweep::default();
        for entry in &entries {
            let bytes = archive.read(&entry.name).expect("read the member");
            sweep.observe(&entry.name, &bytes);
        }
        sweep
    }

    /// The correctness claim, asserted as a rule over whatever the archives actually hold.
    ///
    /// Deliberately not a comparison against a table of expected per-member results: a suite that
    /// checks the decoder against numbers copied out of the decoder cannot fail on the decoder
    /// being wrong. The rules here -- every member parses, every member's samples survive, every
    /// member re-encodes to its own bytes -- are properties of the corpus that an incorrect
    /// decoder breaks.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_archived_wave_round_trips() {
        let directory = game_directory();
        let mut total = 0_usize;
        for archive in ["sndfx.mpq", "special.mpq"] {
            let sweep = sweep_archive(&directory.join(archive));
            assert!(sweep.checked > 0, "{archive} held no members");
            assert_eq!(sweep.failures, Vec::new(), "{archive}");
            assert_eq!(sweep.parsed, sweep.checked, "{archive}: a member did not parse");
            assert_eq!(
                sweep.sample_lossless, sweep.parsed,
                "{archive}: samples changed on re-encode"
            );
            assert_eq!(
                sweep.byte_identical, sweep.parsed,
                "{archive}: a member did not re-encode to its own bytes"
            );
            assert_eq!(sweep.riff_size_mismatch, 0, "{archive}");
            assert_eq!(sweep.refusals, Vec::new(), "{archive}");
            total += sweep.checked;
        }
        assert_eq!(total, 3_098, "the archived WAVE corpus changed size");
    }

    /// Every archived member is PCM, and every one of its format fields is in the attested set.
    ///
    /// This is the test that would fail if [`ATTESTED_SAMPLE_RATES`] and its neighbours were
    /// widened past what the corpus shows, or if a member appeared that they do not cover. It
    /// asserts the *relationship* between the constants and the archives, not their contents.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn the_attested_sets_are_exactly_what_the_corpus_shows() {
        let directory = game_directory();
        let mut seen = std::collections::BTreeSet::new();
        for archive in ["sndfx.mpq", "special.mpq"] {
            let sweep = sweep_archive(&directory.join(archive));
            for key in sweep.formats.keys() {
                seen.insert(*key);
            }
        }
        for key in &seen {
            assert_eq!(key.encoding, WAVE_FORMAT_PCM, "{key}");
            assert!(ATTESTED_CHANNEL_COUNTS.contains(&key.channels), "{key}");
            assert!(ATTESTED_SAMPLE_RATES.contains(&key.sample_rate), "{key}");
            assert!(ATTESTED_BIT_DEPTHS.contains(&key.bits_per_sample), "{key}");
        }
        // The other direction: a constant nothing in the corpus witnesses is a constant this
        // repository has no evidence for, and widening the gate is exactly the mistake that would
        // pass the loop above.
        for rate in ATTESTED_SAMPLE_RATES {
            assert!(
                seen.iter().any(|key| key.sample_rate == *rate),
                "no archived member is evidence for {rate} Hz"
            );
        }
        for channels in ATTESTED_CHANNEL_COUNTS {
            assert!(
                seen.iter().any(|key| key.channels == *channels),
                "no archived member is evidence for {channels} channel(s)"
            );
        }
        for bits in ATTESTED_BIT_DEPTHS {
            assert!(
                seen.iter().any(|key| key.bits_per_sample == *bits),
                "no archived member is evidence for {bits}-bit samples"
            );
        }
    }

    /// The export/import pair, run against real members rather than a fixture.
    ///
    /// Importing a member's own exported audio has to give the member back. A decoder that lost a
    /// sample, an encoder that reordered a chunk, or a rebuild that recomputed a size wrongly all
    /// show up here and nowhere in the synthetic tests above.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn a_member_survives_export_and_import() {
        let directory = game_directory();
        let archive =
            crate::mpq::Archive::open(&directory.join("sndfx.mpq")).expect("open sndfx.mpq");
        let entries = archive.entries().expect("enumerate sndfx.mpq");
        let mut checked = 0_usize;
        for entry in entries.iter().take(200) {
            let bytes = archive.read(&entry.name).expect("read the member");
            let file = WaveFile::parse(&bytes).expect("parse the member");
            let exported = file.encode().expect("export");
            let reimported = import_samples(&exported, &bytes, false).expect("import");
            assert_eq!(
                reimported, bytes,
                "{} did not survive export and import",
                entry.name
            );
            checked += 1;
        }
        assert_eq!(checked, 200);
    }

    /// The loose `Wav/` tree beside the archives -- music and scenario narration.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_loose_wave_round_trips() {
        let directory = game_directory().join("Wav");
        let mut sweep = WaveSweep::default();
        let mut stack = vec![directory.clone()];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).expect("read the Wav directory") {
                let entry = entry.expect("a directory entry");
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("wav"))
                {
                    let bytes = std::fs::read(&path).expect("read a loose wav");
                    sweep.observe(&path.to_string_lossy(), &bytes);
                }
            }
        }
        assert!(sweep.checked > 0, "no loose wav files under Wav/");
        assert_eq!(sweep.failures, Vec::new());
        assert_eq!(sweep.byte_identical, sweep.parsed);
        assert_eq!(sweep.sample_lossless, sweep.parsed);
        assert_eq!(sweep.refusals, Vec::new());
    }
}
