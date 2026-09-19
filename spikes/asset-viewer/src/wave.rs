//! RIFF WAVE container parse, PCM sample decode, and a writer for putting edited audio back.
//!
//! The probe in [`crate::asset`] used to read the `fmt ` chunk and stop. That is enough to
//! classify a member and nothing else: it never touched a sample, so it could not tell a file it
//! understood from a file it merely recognised, and it gave a modder no way back in.
//!
//! # What the round-trip numbers do and do not mean
//!
//! Read this before quoting any figure out of [`WaveSweep`]. The byte-identical result is **split**
//! between parts that are genuinely reconstructed and parts that are replayed, and it is only
//! evidence about the first kind.
//!
//! **Reconstructed, and therefore load-bearing.** The `fmt ` chunk is re-serialised field by field
//! from six typed fields, and the `data` chunk is regenerated from decoded, sign-centred samples.
//! Break the field offsets in [`WaveFormat::parse`], or change the 8-bit conversion from `b - 128`
//! to `b - 127`, and the sweep fails. That part of `reserialised_identical` is a real regression
//! guard over the whole corpus.
//!
//! **Carried verbatim, and therefore proving nothing.** `declared_riff_size`, every chunk id, every
//! declared size, every ancillary chunk body, every pad byte and the trailing bytes are copied out
//! of the parse and copied back in. [`WaveFile::encode`] cannot disagree with the input about any
//! of them, so the result says nothing about whether the ancillary chunks, sizes or pads were
//! understood.
//!
//! **`parsed` is still the load-bearing number for the walk itself.** A mis-walked chunk boundary
//! does surface, but as a parse error: the walk is bounded by the file length at every step and a
//! wrong boundary lands on a chunk that does not fit.
//!
//! The claim that covers the *replayed* half is `import_verified`: every member is put back through
//! [`import_samples`] with its own audio, which goes through [`WaveFile::rebuild`] -- a different
//! serialiser that recomputes every size -- and the result is re-parsed and checked against the
//! template chunk by chunk, pad byte by pad byte. That check can fail and has: `rebuild` wrote a
//! zero pad where the template carried `0x20`, and nothing in the `encode` path could have noticed.
//!
//! **byte-identical** remains a fidelity observation about the *original* authoring tool rather
//! than a correctness claim.

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

/// Fixed part of a `smpl` chunk before the loop array. **Documented.**
const SMPL_HEADER_BYTES: usize = 36;
/// One `smpl` loop record: id, type, start, end, fraction, play count. **Documented.**
const SMPL_LOOP_BYTES: usize = 24;
/// One `cue ` point record. Its last field is the sample-frame offset. **Documented.**
const CUE_POINT_BYTES: usize = 24;

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
    /// Deliberately *not* `block_align`. `block_align` is a declared field; it is checked against
    /// this value by [`Self::derived_field_disagreement`] rather than trusted as it.
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

    /// The `block_align` and `byte_rate` a PCM `fmt ` chunk must declare, given the other fields.
    ///
    /// Both are redundant: for PCM, `block_align = channels * bits/8` and
    /// `byte_rate = sample_rate * block_align`. **Documented.** Redundant fields are exactly the
    /// ones an editor gets wrong, and an audio editor that writes `block_align = 0` would sail
    /// through a decoder that computes frame size for itself -- which this one does.
    pub fn derived_fields(&self) -> Result<(u16, u32), WaveError> {
        let frame_bytes = self.frame_bytes()?;
        let block_align = u16::try_from(frame_bytes)
            .map_err(|_| WaveError::new("WAVE frame size does not fit block_align"))?;
        let byte_rate = u64::from(self.sample_rate) * u64::from(block_align);
        let byte_rate = u32::try_from(byte_rate)
            .map_err(|_| WaveError::new("WAVE byte rate does not fit a 32-bit field"))?;
        Ok((block_align, byte_rate))
    }

    /// Which derived field, if any, the file declares inconsistently with the rest of its `fmt `.
    pub fn derived_field_disagreement(&self) -> Result<Option<WaveRefusal>, WaveError> {
        let (block_align, byte_rate) = self.derived_fields()?;
        if self.block_align != block_align {
            return Ok(Some(WaveRefusal::BlockAlignDisagrees {
                declared: self.block_align,
                computed: block_align,
            }));
        }
        if self.byte_rate != byte_rate {
            return Ok(Some(WaveRefusal::ByteRateDisagrees {
                declared: self.byte_rate,
                computed: byte_rate,
            }));
        }
        Ok(None)
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
/// so a caller never has to know which it is holding. The conversion is exact in both directions.
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
        let mut out = Vec::with_capacity(
            self.interleaved.len() * sample_bytes + self.trailing_partial_frame.len(),
        );
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
                        WaveError::new(format!("16-bit sample {sample} is outside -32768..=32767"))
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
/// [`WaveFile::samples`] on encode.
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
    /// no pad, and inventing one would make the re-encode a byte longer than the original. The
    /// *value* matters too -- it is usually zero but nothing requires it to be, and a writer that
    /// assumes zero changes a byte the modder did not ask to change.
    pub pad: Option<u8>,
}

/// The container and `fmt ` chunk, without decoding any samples.
///
/// Exists so a caller can classify a WAVE whose *encoding* this module has no decoder for.
/// Classification and decodability are different claims; collapsing them turns a legal file into a
/// probe failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveHeader {
    pub declared_riff_size: u32,
    pub format: WaveFormat,
    pub data_bytes: usize,
    pub layout: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveFile {
    /// The size field of the `RIFF` header, carried verbatim.
    ///
    /// Never used to bound a read. It is reproduced by [`Self::encode`], recomputed by
    /// [`Self::rebuild`], and *reported* when it disagrees with the file length.
    pub declared_riff_size: u32,
    pub chunks: Vec<WaveChunk>,
    pub format: WaveFormat,
    pub samples: PcmSamples,
    /// Bytes after the final complete chunk that are too short to be a chunk header.
    pub trailing: Vec<u8>,
}

/// Why a file must not be rewritten, or must not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaveRefusal {
    /// A format tag, channel count, rate or depth no shipped file is evidence for.
    UnattestedFormat(String),
    /// The `data` chunk does not hold a whole number of frames.
    PartialFrame { remainder: usize },
    /// `block_align` disagrees with the channel count and bit depth in the same chunk.
    BlockAlignDisagrees { declared: u16, computed: u16 },
    /// `byte_rate` disagrees with the sample rate and block alignment in the same chunk.
    ByteRateDisagrees { declared: u32, computed: u32 },
    /// A `smpl` loop or `cue ` point would point past the end of the audio being written.
    DanglingLoopMetadata {
        chunk: String,
        last_referenced_frame: u64,
        frames: u64,
    },
    /// The writer's own output does not carry the container it was told to carry.
    ///
    /// Reached by re-parsing what [`WaveFile::rebuild`] produced and comparing it with the
    /// template, chunk by chunk. This is the one refusal that can catch a *writer* bug rather than
    /// an input problem, and it has caught one: `rebuild` used to emit a zero pad byte where the
    /// template carried a different value.
    ContainerChanged { detail: String },
}

impl fmt::Display for WaveRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnattestedFormat(reason) => write!(formatter, "unattested-format;{reason}"),
            Self::PartialFrame { remainder } => {
                write!(formatter, "partial-frame;remainder-bytes={remainder}")
            }
            Self::BlockAlignDisagrees { declared, computed } => write!(
                formatter,
                "block-align-disagrees;declared={declared};computed={computed}"
            ),
            Self::ByteRateDisagrees { declared, computed } => write!(
                formatter,
                "byte-rate-disagrees;declared={declared};computed={computed}"
            ),
            Self::DanglingLoopMetadata {
                chunk,
                last_referenced_frame,
                frames,
            } => write!(
                formatter,
                "dangling-loop-metadata;chunk={chunk};last-referenced-frame={last_referenced_frame};frames={frames}"
            ),
            Self::ContainerChanged { detail } => {
                write!(formatter, "container-changed;{detail}")
            }
        }
    }
}

/// One chunk as it sits in the source bytes, before anything is copied out of it.
struct RawChunk<'a> {
    id: [u8; 4],
    declared_size: u32,
    body: &'a [u8],
    pad: Option<u8>,
}

/// Walk the chunk list. Every step is bounded by the slice in hand, never by a declared size.
fn walk_chunks(source: &[u8]) -> Result<(u32, Vec<RawChunk<'_>>, &[u8]), WaveError> {
    if source.len() < 12 || &source[0..4] != b"RIFF" || &source[8..12] != b"WAVE" {
        return Err(WaveError::new("not a RIFF WAVE file"));
    }
    let declared_riff_size = read_u32(source, 4)?;
    let mut chunks = Vec::new();
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
        let pad = if declared_size % 2 == 1 {
            source.get(body_end).copied()
        } else {
            None
        };
        chunks.push(RawChunk {
            id,
            declared_size,
            body: &source[body_start..body_end],
            pad,
        });
        cursor = body_end + usize::from(pad.is_some());
    }
    Ok((declared_riff_size, chunks, &source[cursor..]))
}

fn layout_of(chunks: &[RawChunk<'_>]) -> String {
    chunks
        .iter()
        .map(|chunk| printable_tag(&chunk.id))
        .collect::<Vec<_>>()
        .join("|")
}

impl WaveFile {
    /// Parse the container and the `fmt ` chunk, without decoding samples.
    pub fn parse_header(source: &[u8]) -> Result<WaveHeader, WaveError> {
        let (declared_riff_size, chunks, _) = walk_chunks(source)?;
        let mut format = None;
        let mut data_bytes = None;
        for chunk in &chunks {
            match &chunk.id {
                b"fmt " if format.is_none() => format = Some(WaveFormat::parse(chunk.body)?),
                b"fmt " => return Err(WaveError::new("WAVE has more than one fmt chunk")),
                b"data" if data_bytes.is_none() => data_bytes = Some(chunk.body.len()),
                b"data" => return Err(WaveError::new("WAVE has more than one data chunk")),
                _ => {}
            }
        }
        Ok(WaveHeader {
            declared_riff_size,
            format: format.ok_or_else(|| WaveError::new("WAVE has no fmt chunk"))?,
            data_bytes: data_bytes.ok_or_else(|| WaveError::new("WAVE has no data chunk"))?,
            layout: layout_of(&chunks),
        })
    }

    pub fn parse(source: &[u8]) -> Result<Self, WaveError> {
        let (declared_riff_size, raw, trailing) = walk_chunks(source)?;
        let mut format = None;
        let mut data = None;
        let mut chunks = Vec::with_capacity(raw.len());
        for chunk in &raw {
            let body = match &chunk.id {
                b"fmt " => {
                    if format.is_some() {
                        return Err(WaveError::new("WAVE has more than one fmt chunk"));
                    }
                    format = Some(WaveFormat::parse(chunk.body)?);
                    WaveChunkBody::Format
                }
                b"data" => {
                    if data.is_some() {
                        return Err(WaveError::new("WAVE has more than one data chunk"));
                    }
                    data = Some(chunk.body.to_vec());
                    WaveChunkBody::Data
                }
                _ => WaveChunkBody::Other(chunk.body.to_vec()),
            };
            chunks.push(WaveChunk {
                id: chunk.id,
                declared_size: chunk.declared_size,
                body,
                pad: chunk.pad,
            });
        }
        let format = format.ok_or_else(|| WaveError::new("WAVE has no fmt chunk"))?;
        let data = data.ok_or_else(|| WaveError::new("WAVE has no data chunk"))?;
        let samples = PcmSamples::decode(&format, &data)?;
        Ok(Self {
            declared_riff_size,
            chunks,
            format,
            samples,
            trailing: trailing.to_vec(),
        })
    }

    fn chunk_bodies(&self) -> Result<Vec<Vec<u8>>, WaveError> {
        self.chunks
            .iter()
            .map(|chunk| match &chunk.body {
                WaveChunkBody::Format => Ok(self.format.encode()),
                WaveChunkBody::Data => self.samples.encode(),
                WaveChunkBody::Other(bytes) => Ok(bytes.clone()),
            })
            .collect()
    }

    /// Re-serialise the container exactly as parsed, rebuilding `fmt ` and `data`.
    ///
    /// Sizes, order, pad bytes and the `RIFF` size field are *carried*, not recomputed, so a file
    /// whose header lies re-encodes to the same lie rather than being quietly corrected.
    ///
    /// Given a successful parse this is an exact inverse and therefore always reproduces the
    /// input; see the module header before quoting that as a result. [`Self::rebuild`] is the
    /// serialiser an edit goes through, and it is the one that can be wrong.
    pub fn encode(&self) -> Result<Vec<u8>, WaveError> {
        let bodies = self.chunk_bodies()?;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&self.declared_riff_size.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        for (chunk, body) in self.chunks.iter().zip(&bodies) {
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
            out.extend_from_slice(body);
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
    /// changes the `RIFF` size and can add or remove the pad byte.
    ///
    /// The pad byte is taken from the chunk it belongs to rather than assumed to be zero. It was
    /// assumed to be zero, and importing a file's own unmodified audio through a template with a
    /// `0x20` pad came back differing from the template at that byte -- with a zero exit status
    /// and no warning. The post-condition in [`import_samples`] now catches that class directly.
    pub fn rebuild(&self) -> Result<Vec<u8>, WaveError> {
        let bodies = self.chunk_bodies()?;
        let mut payload = 4_u64; // the "WAVE" form type
        for body in &bodies {
            payload += 8 + body.len() as u64 + (body.len() as u64 % 2);
        }
        payload += self.trailing.len() as u64;
        let declared = u32::try_from(payload)
            .map_err(|_| WaveError::new("rebuilt WAVE exceeds the 4 GiB RIFF size field"))?;

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&declared.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        for (chunk, body) in self.chunks.iter().zip(&bodies) {
            let size = u32::try_from(body.len())
                .map_err(|_| WaveError::new("a rebuilt WAVE chunk exceeds 4 GiB"))?;
            out.extend_from_slice(&chunk.id);
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(body);
            if size % 2 == 1 {
                out.push(chunk.pad.unwrap_or(0));
            }
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }

    /// Why this file must not be used as a rewrite template, or `None` if it may be.
    ///
    /// Note what is *not* here: a check that the file re-encodes to its own bytes. That check was
    /// here, it could not fail, and a guard that cannot fail is worse than none because it reads
    /// like protection. The equivalent claim is now made where it can be false -- against the
    /// writer's own output, in [`import_samples`].
    pub fn rewrite_refusal(&self) -> Result<Option<WaveRefusal>, WaveError> {
        if let Err(reason) = self.format.attested() {
            return Ok(Some(WaveRefusal::UnattestedFormat(reason)));
        }
        if let Some(refusal) = self.format.derived_field_disagreement()? {
            return Ok(Some(refusal));
        }
        if !self.samples.trailing_partial_frame.is_empty() {
            return Ok(Some(WaveRefusal::PartialFrame {
                remainder: self.samples.trailing_partial_frame.len(),
            }));
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

    /// The highest sample frame any `smpl` loop or `cue ` point in this container refers to.
    ///
    /// Returns the chunk that names it alongside the frame. `smpl` loop records and `cue ` points
    /// both address **sample frames**, so shortening the audio under them leaves a loop or marker
    /// pointing past the end. **Documented** layouts; the fields are read, not the semantics.
    pub fn last_referenced_frame(&self) -> Option<(String, u64)> {
        let mut worst: Option<(String, u64)> = None;
        for chunk in &self.chunks {
            let WaveChunkBody::Other(body) = &chunk.body else {
                continue;
            };
            let frame = match &chunk.id {
                b"smpl" => smpl_last_frame(body),
                b"cue " => cue_last_frame(body),
                _ => None,
            };
            if let Some(frame) = frame
                && worst.as_ref().is_none_or(|(_, seen)| frame > *seen)
            {
                worst = Some((printable_tag(&chunk.id), frame));
            }
        }
        worst
    }
}

/// Highest loop end in a `smpl` chunk. **Documented:** 36-byte header, then 24-byte loop records
/// whose third and fourth words are the start and end sample frames.
fn smpl_last_frame(body: &[u8]) -> Option<u64> {
    if body.len() < SMPL_HEADER_BYTES {
        return None;
    }
    let declared = read_u32(body, 28).ok()? as usize;
    // The loop count is a declared value, so the array is bounded by the bytes actually present
    // rather than by the count. A file claiming four billion loops reads the ones it has.
    let available = (body.len() - SMPL_HEADER_BYTES) / SMPL_LOOP_BYTES;
    let mut worst = None;
    for index in 0..declared.min(available) {
        let at = SMPL_HEADER_BYTES + index * SMPL_LOOP_BYTES;
        let start = u64::from(read_u32(body, at + 8).ok()?);
        let end = u64::from(read_u32(body, at + 12).ok()?);
        let highest = start.max(end);
        if worst.is_none_or(|seen| highest > seen) {
            worst = Some(highest);
        }
    }
    worst
}

/// Highest sample offset in a `cue ` chunk. **Documented:** a 4-byte count, then 24-byte points
/// whose last word is the sample-frame offset.
fn cue_last_frame(body: &[u8]) -> Option<u64> {
    if body.len() < 4 {
        return None;
    }
    let declared = read_u32(body, 0).ok()? as usize;
    let available = (body.len() - 4) / CUE_POINT_BYTES;
    let mut worst = None;
    for index in 0..declared.min(available) {
        let at = 4 + index * CUE_POINT_BYTES + 20;
        let offset = u64::from(read_u32(body, at).ok()?);
        if worst.is_none_or(|seen| offset > seen) {
            worst = Some(offset);
        }
    }
    worst
}

/// The two deliberate relaxations `--import-wave` offers, each of which must be asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportOptions {
    /// Accept an edit whose format differs from the template's. Still gated on the attested set.
    pub allow_format_change: bool,
    /// Accept a result whose `smpl` loops or `cue ` points fall past the end of the new audio.
    pub allow_dangling_loops: bool,
}

/// Replace the samples of `template` with those of `edited`, keeping the template's container.
///
/// The template is the shipped member. Its ancillary chunks and their order are what the game was
/// built with, so they are carried; only the audio is taken from the file the modder edited.
///
/// Every refusal is named to the caller. The last one is the important one: after writing, the
/// result is **re-parsed and compared with the template chunk by chunk**. That is the only check
/// in this module that can catch the writer rather than the input, and it exists because the
/// writer was caught -- `rebuild` dropped a non-zero pad byte and nothing noticed.
pub fn import_samples(
    edited: &[u8],
    template: &[u8],
    options: ImportOptions,
) -> Result<Vec<u8>, WaveError> {
    let mut result =
        WaveFile::parse(template).map_err(|error| WaveError::new(format!("template: {error}")))?;
    if let Some(refusal) = result.rewrite_refusal()? {
        return Err(WaveError::new(format!(
            "refusing to rewrite this template: {refusal}"
        )));
    }
    let template_file = result.clone();

    let edited_file =
        WaveFile::parse(edited).map_err(|error| WaveError::new(format!("edited file: {error}")))?;
    if let Some(refusal) = edited_file.rewrite_refusal()? {
        return Err(WaveError::new(format!("edited file: {refusal}")));
    }

    let target_format = resolve_target_format(&template_file.format, &edited_file.format, options)?;
    result.format = target_format.clone();
    result.samples = edited_file.samples.clone();

    // The loop-metadata gate. Corpus members carry `smpl` loops and `cue ` points, all of them
    // addressing sample frames in the audio being replaced. Shortening the audio under them is
    // silent data corruption that plays as a hang or a click, so it is refused by name rather than
    // left in a document the modder never reads.
    if !options.allow_dangling_loops
        && let Some((chunk, last)) = result.last_referenced_frame()
    {
        let frames = result.samples.frames() as u64;
        if last > frames {
            return Err(WaveError::new(format!(
                "refusing to write it: {}; pass --allow-dangling-loops to write it anyway",
                WaveRefusal::DanglingLoopMetadata {
                    chunk,
                    last_referenced_frame: last,
                    frames,
                }
            )));
        }
    }

    let written = result.rebuild()?;
    verify_import(&written, &template_file, &target_format, &edited_file.samples).map_err(
        |refusal| {
            WaveError::new(format!(
                "this tool's own output is wrong and it will not be written: {refusal}"
            ))
        },
    )?;
    Ok(written)
}

/// Decide which `fmt ` chunk the output carries.
///
/// The import replaces the template's **whole** `fmt ` structure, not just the four nominal
/// fields, so the gate has to cover the whole structure. It did not: an edit declaring
/// PCM/mono/22050/8-bit with `block_align = 0` matched on the nominal fields, needed no
/// `--allow-format-change`, and put those zeroes in the output.
///
/// The policy, stated rather than implied:
///
/// * **Nominal format unchanged** -- keep the *template's* `fmt ` chunk entire, including its
///   `byte_rate`, `block_align` and any extension bytes. The shipped file is the authority on
///   fields the modder did not set out to change, and an editor's idea of them is not.
/// * **Nominal format changed** (`--allow-format-change`) -- take the four nominal fields from the
///   edit and **derive** `block_align` and `byte_rate` from them rather than copying the edit's.
/// * **Extension bytes** -- no member of the corpus has a `fmt ` chunk longer than 16 bytes, so
///   there is no evidence for what the engine would do with one. An edit that carries extension
///   bytes differing from the template's is refused by name. On a format change the template's
///   extension bytes are dropped, because they describe a format that is no longer being written.
fn resolve_target_format(
    template: &WaveFormat,
    edited: &WaveFormat,
    options: ImportOptions,
) -> Result<WaveFormat, WaveError> {
    let same_nominal_format = edited.encoding == template.encoding
        && edited.channels == template.channels
        && edited.sample_rate == template.sample_rate
        && edited.bits_per_sample == template.bits_per_sample;
    if !same_nominal_format && !options.allow_format_change {
        return Err(WaveError::new(format!(
            "edited file is {}, the template is {}; pass --allow-format-change to write it anyway",
            describe_format(edited),
            describe_format(template)
        )));
    }
    if !edited.extra.is_empty() && edited.extra != template.extra {
        return Err(WaveError::new(format!(
            "edited file has {} fmt extension byte(s) the template does not; no member of the \
             corpus has a fmt chunk longer than 16 bytes, so there is no evidence for writing one",
            edited.extra.len()
        )));
    }
    if same_nominal_format {
        return Ok(template.clone());
    }
    let mut target = WaveFormat {
        extra: Vec::new(),
        ..edited.clone()
    };
    let (block_align, byte_rate) = target.derived_fields()?;
    target.block_align = block_align;
    target.byte_rate = byte_rate;
    if let Err(reason) = target.attested() {
        return Err(WaveError::new(format!("edited file: {reason}")));
    }
    Ok(target)
}

/// Re-parse what the writer produced and check it against what it was told to write.
///
/// The ancillary chunks, their order, their bodies and their pad bytes must match the template;
/// the format and samples must match the edit. Sizes are expected to differ -- that is the point
/// of `rebuild` -- so they are not compared, which is why the pad byte has to be.
fn verify_import(
    written: &[u8],
    template: &WaveFile,
    target_format: &WaveFormat,
    target_samples: &PcmSamples,
) -> Result<(), WaveRefusal> {
    let parsed = WaveFile::parse(written).map_err(|error| WaveRefusal::ContainerChanged {
        detail: format!("the output does not parse back: {error}"),
    })?;
    if parsed.chunks.len() != template.chunks.len() {
        return Err(WaveRefusal::ContainerChanged {
            detail: format!(
                "chunk count {} does not match the template's {}",
                parsed.chunks.len(),
                template.chunks.len()
            ),
        });
    }
    for (index, (out, source)) in parsed.chunks.iter().zip(&template.chunks).enumerate() {
        if out.id != source.id {
            return Err(WaveRefusal::ContainerChanged {
                detail: format!(
                    "chunk {index} is {} but the template has {}",
                    printable_tag(&out.id),
                    printable_tag(&source.id)
                ),
            });
        }
        if let (WaveChunkBody::Other(written_body), WaveChunkBody::Other(source_body)) =
            (&out.body, &source.body)
            && written_body != source_body
        {
            return Err(WaveRefusal::ContainerChanged {
                detail: format!("the {} chunk body changed", printable_tag(&out.id)),
            });
        }
        // A pad byte is written only when the body is odd, so compare it only when both have one.
        if let (Some(written_pad), Some(source_pad)) = (out.pad, source.pad)
            && written_pad != source_pad
        {
            return Err(WaveRefusal::ContainerChanged {
                detail: format!(
                    "the pad byte after {} became 0x{written_pad:02x}, the template has 0x{source_pad:02x}",
                    printable_tag(&out.id)
                ),
            });
        }
    }
    if parsed.trailing != template.trailing {
        return Err(WaveRefusal::ContainerChanged {
            detail: "the trailing bytes changed".to_owned(),
        });
    }
    if parsed.format != *target_format {
        return Err(WaveRefusal::ContainerChanged {
            detail: format!(
                "the written format is {}, the import resolved {}",
                describe_format(&parsed.format),
                describe_format(target_format)
            ),
        });
    }
    if parsed.samples != *target_samples {
        return Err(WaveRefusal::ContainerChanged {
            detail: "the written samples are not the edited samples".to_owned(),
        });
    }
    if !parsed.riff_size_matches(written.len()) {
        return Err(WaveRefusal::ContainerChanged {
            detail: format!(
                "the written RIFF size field is {} for a {}-byte file",
                parsed.declared_riff_size,
                written.len()
            ),
        });
    }
    Ok(())
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
///
/// See the module header for which of these counters can move and which cannot.
#[derive(Debug, Default)]
pub struct WaveSweep {
    /// Files offered to the sweep, including ones it declined to treat as WAVE.
    pub checked: usize,
    /// Files skipped because they are not RIFF WAVE at all.
    ///
    /// Reported rather than silently dropped: without it the denominator is chosen by the same
    /// magic-byte test being measured, and the archive and directory sweeps would quietly report
    /// over different populations.
    pub skipped: usize,
    /// **The load-bearing number.** A mis-walked chunk boundary surfaces here, as a parse failure.
    pub parsed: usize,
    /// Files whose parsed container re-serialises to their own bytes.
    ///
    /// A real guard over the `fmt ` field offsets and the PCM sample conversion, which are rebuilt
    /// from typed values. **Not** a guard over the ancillary chunks, declared sizes, pad bytes or
    /// trailing bytes, which [`WaveFile::encode`] replays verbatim. `import_verified` is the
    /// counter that covers those.
    pub reserialised_identical: usize,
    /// Files put back through the writer with their own audio and verified against themselves.
    ///
    /// This one can fall below `parsed` and has.
    pub import_verified: usize,
    /// Of those, how many came back byte-identical to the original file.
    pub import_identical: usize,
    pub riff_size_mismatch: usize,
    pub formats: BTreeMap<FormatKey, usize>,
    pub layouts: BTreeMap<String, usize>,
    pub loop_metadata_files: usize,
    pub total_frames: u64,
    pub failures: Vec<(String, String)>,
    pub import_differences: Vec<(String, usize)>,
    pub refusals: Vec<(String, WaveRefusal)>,
}

impl WaveSweep {
    pub fn observe(&mut self, name: &str, bytes: &[u8]) {
        self.checked += 1;
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            self.skipped += 1;
            return;
        }
        let file = match WaveFile::parse(bytes) {
            Ok(file) => file,
            Err(error) => {
                self.failures.push((name.to_owned(), error.to_string()));
                return;
            }
        };
        self.parsed += 1;
        *self
            .formats
            .entry(FormatKey::from(&file.format))
            .or_default() += 1;
        *self.layouts.entry(file.layout()).or_default() += 1;
        self.total_frames += file.samples.frames() as u64;
        if file.last_referenced_frame().is_some() {
            self.loop_metadata_files += 1;
        }
        if !file.riff_size_matches(bytes.len()) {
            self.riff_size_mismatch += 1;
        }
        match file.encode() {
            Ok(encoded) if encoded == bytes => self.reserialised_identical += 1,
            Ok(encoded) => self.failures.push((
                name.to_owned(),
                match first_difference(&encoded, bytes) {
                    Some(offset) => format!(
                        "re-serialising the parsed container changed byte {offset}: the fmt \
                         fields or the PCM conversion are wrong"
                    ),
                    None => "re-serialised length differs".to_owned(),
                },
            )),
            Err(error) => self.failures.push((name.to_owned(), error.to_string())),
        }
        match file.rewrite_refusal() {
            Ok(Some(refusal)) => {
                self.refusals.push((name.to_owned(), refusal));
                return;
            }
            Ok(None) => {}
            Err(error) => {
                self.failures.push((name.to_owned(), error.to_string()));
                return;
            }
        }
        // The falsifiable claim: put the file back through the *writer* with its own audio.
        match import_samples(bytes, bytes, ImportOptions::default()) {
            Ok(written) => {
                self.import_verified += 1;
                match first_difference(&written, bytes) {
                    None => self.import_identical += 1,
                    Some(offset) => self.import_differences.push((name.to_owned(), offset)),
                }
            }
            Err(error) => self.failures.push((name.to_owned(), error.to_string())),
        }
    }

    /// Tab-separated, in the shape the other sweeps in this tool print.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("checked\t{}\n", self.checked));
        out.push_str(&format!("skipped_not_wave\t{}\n", self.skipped));
        out.push_str(&format!("parsed\t{}\n", self.parsed));
        out.push_str(&format!(
            "reserialised_identical\t{}\n",
            self.reserialised_identical
        ));
        out.push_str(&format!("import_verified\t{}\n", self.import_verified));
        out.push_str(&format!("import_identical\t{}\n", self.import_identical));
        out.push_str(&format!("riff_size_mismatch\t{}\n", self.riff_size_mismatch));
        out.push_str(&format!(
            "files_with_loop_metadata\t{}\n",
            self.loop_metadata_files
        ));
        out.push_str(&format!("frames\t{}\n", self.total_frames));
        out.push_str(&format!("refused\t{}\n", self.refusals.len()));
        out.push_str(&format!("failures\t{}\n", self.failures.len()));
        for (format, count) in &self.formats {
            out.push_str(&format!("format\t{format}\t{count}\n"));
        }
        for (layout, count) in &self.layouts {
            out.push_str(&format!("layout\t{layout}\t{count}\n"));
        }
        for (name, offset) in &self.import_differences {
            out.push_str(&format!(
                "import_difference\t{name}\tfirst-byte={offset}\n"
            ));
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

    /// Append a chunk and fix the RIFF size, so a fixture can carry ancillary data.
    fn with_chunk(mut file: Vec<u8>, id: &[u8; 4], body: &[u8], pad: u8) -> Vec<u8> {
        let riff = u32::from_le_bytes([file[4], file[5], file[6], file[7]]);
        let added = 8 + body.len() + body.len() % 2;
        file[4..8].copy_from_slice(&(riff + added as u32).to_le_bytes());
        file.extend_from_slice(id);
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(body);
        if body.len() % 2 == 1 {
            file.push(pad);
        }
        file
    }

    /// A `smpl` chunk with one loop over `start..=end` sample frames.
    fn smpl(start: u32, end: u32) -> Vec<u8> {
        let mut body = vec![0_u8; SMPL_HEADER_BYTES];
        body[28..32].copy_from_slice(&1_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&start.to_le_bytes());
        body.extend_from_slice(&end.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body
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

    /// The bug the review found, pinned: `rebuild` used to write a zero pad byte over the one the
    /// template carried, so importing a file's OWN audio came back different with a zero exit.
    #[test]
    fn rebuild_keeps_a_non_zero_pad_byte() {
        let template = with_chunk(canonical(1, 22050, 8, &[1, 2, 3, 4]), b"LIST", b"INFOx", 0x20);
        let file = WaveFile::parse(&template).expect("parse");
        let list = file
            .chunks
            .iter()
            .find(|chunk| chunk.id == *b"LIST")
            .expect("a LIST chunk");
        assert_eq!(list.pad, Some(0x20), "the fixture must carry a 0x20 pad");
        assert_eq!(
            file.rebuild().expect("rebuild"),
            template,
            "rebuild must not zero a pad byte the template carried"
        );
        assert_eq!(
            import_samples(&template, &template, ImportOptions::default()).expect("import"),
            template
        );
    }

    /// And the post-condition that catches it even if `rebuild` regresses again.
    #[test]
    fn the_import_post_condition_catches_a_changed_pad_byte() {
        let template = with_chunk(canonical(1, 22050, 8, &[1, 2, 3, 4]), b"LIST", b"INFOx", 0x20);
        let good = WaveFile::parse(&template).expect("parse");
        let mut damaged = good.clone();
        for chunk in &mut damaged.chunks {
            if chunk.id == *b"LIST" {
                chunk.pad = Some(0);
            }
        }
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("the pad changed");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("pad byte")),
            "{refusal}"
        );
    }

    #[test]
    fn the_import_post_condition_catches_a_dropped_ancillary_chunk() {
        let template = with_chunk(canonical(1, 22050, 8, &[1, 2, 3, 4]), b"LIST", b"INFO", 0);
        let good = WaveFile::parse(&template).expect("parse");
        let mut damaged = good.clone();
        damaged.chunks.retain(|chunk| chunk.id != *b"LIST");
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("a chunk vanished");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("chunk count")),
            "{refusal}"
        );
    }

    #[test]
    fn a_trailing_list_chunk_survives_the_round_trip() {
        let bytes = with_chunk(
            canonical(1, 22050, 8, &[1, 2, 3]),
            b"LIST",
            b"INFOISFT\x00\x00\x00\x00",
            0,
        );
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
    fn a_non_pcm_format_tag_has_no_decoder_but_still_has_a_header() {
        let mut bytes = canonical(1, 22050, 8, &[1, 2]);
        bytes[20] = 0x11; // WAVE_FORMAT_IMA_ADPCM
        let error = WaveFile::parse(&bytes).expect_err("ADPCM must not decode as PCM");
        assert!(error.to_string().contains("not PCM"), "{error}");
        // Classification is a different claim from decodability, and the container is still legal.
        let header = WaveFile::parse_header(&bytes).expect("the container still parses");
        assert_eq!(header.format.encoding, 0x11);
        assert_eq!(header.data_bytes, 2);
        assert_eq!(header.layout, "fmt |data");
    }

    #[test]
    fn a_depth_with_no_decoder_still_has_a_header() {
        let mut bytes = canonical(1, 22050, 8, &[1, 2, 3]);
        bytes[34] = 24; // bits per sample
        assert!(WaveFile::parse(&bytes).is_err());
        let header = WaveFile::parse_header(&bytes).expect("the container still parses");
        assert_eq!(header.format.bits_per_sample, 24);
    }

    #[test]
    fn a_container_error_is_an_error_for_the_header_parse_too() {
        let error = WaveFile::parse_header(b"RIFF\0\0\0\0WAVE").expect_err("no fmt chunk");
        assert_eq!(error.to_string(), "WAVE has no fmt chunk");
    }

    #[test]
    fn a_partial_final_frame_blocks_a_rewrite() {
        let bytes = canonical(2, 22050, 16, &[1, 0, 2, 0, 3, 0]);
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(
            file.rewrite_refusal().expect("refusal check"),
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
        assert_eq!(file.rewrite_refusal().expect("refusal check"), None);
    }

    #[test]
    fn an_unattested_sample_rate_is_refused_by_name() {
        let bytes = canonical(1, 8000, 8, &[1, 2]);
        let file = WaveFile::parse(&bytes).expect("parse");
        let refusal = file
            .rewrite_refusal()
            .expect("refusal check")
            .expect("8 kHz is not attested");
        assert!(
            matches!(&refusal, WaveRefusal::UnattestedFormat(reason) if reason.contains("8000")),
            "{refusal}"
        );
    }

    /// The guard the old doc comment claimed and nobody had written.
    #[test]
    fn a_block_align_that_contradicts_the_rest_of_the_fmt_chunk_is_refused() {
        let mut bytes = canonical(2, 22050, 8, &[1, 2, 3, 4]);
        bytes[32..34].copy_from_slice(&0_u16.to_le_bytes()); // block_align
        let file = WaveFile::parse(&bytes).expect("the decoder computes frame size for itself");
        assert_eq!(
            file.rewrite_refusal().expect("refusal check"),
            Some(WaveRefusal::BlockAlignDisagrees {
                declared: 0,
                computed: 2
            })
        );
        let error = import_samples(&bytes, &bytes, ImportOptions::default())
            .expect_err("an inconsistent fmt must not be written");
        assert!(error.to_string().contains("block-align-disagrees"), "{error}");
    }

    #[test]
    fn a_byte_rate_that_contradicts_the_rest_of_the_fmt_chunk_is_refused() {
        let mut bytes = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        bytes[28..32].copy_from_slice(&1234_u32.to_le_bytes()); // byte_rate
        let file = WaveFile::parse(&bytes).expect("parse");
        assert_eq!(
            file.rewrite_refusal().expect("refusal check"),
            Some(WaveRefusal::ByteRateDisagrees {
                declared: 1234,
                computed: 22050
            })
        );
    }

    #[test]
    fn import_replaces_the_audio_and_keeps_the_template_container() {
        let template = with_chunk(
            canonical(1, 22050, 8, &[0x80, 0x80, 0x80, 0x80]),
            b"LIST",
            b"INFOISFT\x00\x00\x00\x00",
            0,
        );
        let edited = canonical(1, 22050, 8, &[0x00, 0xff]);
        let result =
            import_samples(&edited, &template, ImportOptions::default()).expect("import");
        let parsed = WaveFile::parse(&result).expect("parse the import");
        assert_eq!(parsed.samples.interleaved, vec![-128, 127]);
        assert_eq!(parsed.layout(), "fmt |data|LIST");
        assert!(parsed.riff_size_matches(result.len()));
    }

    #[test]
    fn importing_a_files_own_audio_reproduces_it() {
        let original = canonical(2, 22050, 8, &[1, 2, 3, 4, 5, 6]);
        let result =
            import_samples(&original, &original, ImportOptions::default()).expect("import");
        assert_eq!(result, original);
    }

    #[test]
    fn a_format_change_is_refused_unless_it_is_asked_for() {
        let template = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let edited = canonical(2, 22050, 8, &[1, 2, 3, 4]);
        let error = import_samples(&edited, &template, ImportOptions::default())
            .expect_err("channels changed");
        assert!(
            error.to_string().contains("--allow-format-change"),
            "{error}"
        );
        let allowed = import_samples(
            &edited,
            &template,
            ImportOptions {
                allow_format_change: true,
                ..ImportOptions::default()
            },
        )
        .expect("explicitly allowed");
        assert_eq!(WaveFile::parse(&allowed).expect("parse").format.channels, 2);
    }

    #[test]
    fn a_format_change_outside_the_attested_set_is_refused_even_when_allowed() {
        let template = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let edited = canonical(1, 48000, 8, &[1, 2, 3, 4]);
        let error = import_samples(
            &edited,
            &template,
            ImportOptions {
                allow_format_change: true,
                allow_dangling_loops: true,
            },
        )
        .expect_err("48 kHz is not attested");
        assert!(error.to_string().contains("48000"), "{error}");
    }

    #[test]
    fn rebuild_recomputes_the_sizes_an_edit_changes() {
        let template = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let edited = canonical(1, 22050, 8, &[1, 2, 3]);
        let result =
            import_samples(&edited, &template, ImportOptions::default()).expect("import");
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

    // --- loop metadata ------------------------------------------------------------------------

    #[test]
    fn a_smpl_loop_past_the_end_of_the_new_audio_is_refused_by_name() {
        let template = with_chunk(
            canonical(1, 22050, 8, &[0; 1000]),
            b"smpl",
            &smpl(0, 1000),
            0,
        );
        assert_eq!(
            WaveFile::parse(&template)
                .expect("parse")
                .last_referenced_frame(),
            Some(("smpl".to_owned(), 1000))
        );
        // Unedited, the loop still fits.
        import_samples(&template, &template, ImportOptions::default())
            .expect("the template is self-consistent");

        let short = canonical(1, 22050, 8, &[0; 100]);
        let error = import_samples(&short, &template, ImportOptions::default())
            .expect_err("the loop now dangles");
        assert!(
            error.to_string().contains("dangling-loop-metadata"),
            "{error}"
        );
        assert!(error.to_string().contains("--allow-dangling-loops"), "{error}");

        let allowed = import_samples(
            &short,
            &template,
            ImportOptions {
                allow_dangling_loops: true,
                ..ImportOptions::default()
            },
        )
        .expect("explicitly allowed");
        assert_eq!(WaveFile::parse(&allowed).expect("parse").layout(), "fmt |data|smpl");
    }

    #[test]
    fn a_cue_point_past_the_end_of_the_new_audio_is_refused() {
        let mut cue = 1_u32.to_le_bytes().to_vec();
        cue.extend_from_slice(&[0_u8; CUE_POINT_BYTES]);
        cue[4 + 20..4 + 24].copy_from_slice(&900_u32.to_le_bytes());
        let template = with_chunk(canonical(1, 22050, 8, &[0; 1000]), b"cue ", &cue, 0);
        assert_eq!(
            WaveFile::parse(&template)
                .expect("parse")
                .last_referenced_frame(),
            Some(("cue ".to_owned(), 900))
        );
        let short = canonical(1, 22050, 8, &[0; 100]);
        let error = import_samples(&short, &template, ImportOptions::default())
            .expect_err("the cue point now dangles");
        assert!(error.to_string().contains("cue "), "{error}");
    }

    #[test]
    fn a_lying_loop_count_reads_only_the_loops_that_are_there() {
        let mut body = smpl(0, 40);
        body[28..32].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        assert_eq!(smpl_last_frame(&body), Some(40));
        let mut cue = 0xffff_ffff_u32.to_le_bytes().to_vec();
        cue.extend_from_slice(&[0_u8; CUE_POINT_BYTES]);
        cue[4 + 20..4 + 24].copy_from_slice(&7_u32.to_le_bytes());
        assert_eq!(cue_last_frame(&cue), Some(7));
    }

    #[test]
    fn the_sweep_separates_what_can_move_from_what_cannot() {
        let mut sweep = WaveSweep::default();
        sweep.observe("clean.wav", &canonical(1, 22050, 8, &[1, 2, 3, 4]));
        // A file whose RIFF size field disagrees with its length. `encode` carries the lie, so the
        // structural counter still rises; `rebuild` corrects it, so the import is verified but is
        // NOT byte-identical. That difference is the sweep reporting something real.
        let mut lying = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        lying[4] = lying[4].wrapping_add(2);
        sweep.observe("lying.wav", &lying);
        sweep.observe("not-a-wave.bin", b"MZ\x00\x00");
        sweep.observe("bad-align.wav", &{
            let mut bytes = canonical(2, 22050, 8, &[1, 2, 3, 4]);
            bytes[32..34].copy_from_slice(&7_u16.to_le_bytes());
            bytes
        });
        assert_eq!(sweep.checked, 4);
        assert_eq!(sweep.skipped, 1);
        assert_eq!(sweep.parsed, 3);
        assert_eq!(sweep.reserialised_identical, 3);
        assert_eq!(sweep.refusals.len(), 1, "the bad block_align is refused");
        assert_eq!(sweep.import_verified, 2, "the refused file is not imported");
        assert_eq!(sweep.import_identical, 1, "the lying RIFF size gets corrected");
        assert_eq!(sweep.import_differences.len(), 1);
        assert_eq!(sweep.riff_size_mismatch, 1);
        assert_eq!(sweep.failures, Vec::new());
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

    fn loose_wave_files() -> Vec<(String, Vec<u8>)> {
        let root = game_directory().join("Wav");
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).expect("read the Wav directory") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("wav"))
                {
                    let bytes = std::fs::read(&path).expect("read a loose wav");
                    out.push((path.to_string_lossy().into_owned(), bytes));
                }
            }
        }
        out.sort_by(|left, right| left.0.cmp(&right.0));
        out
    }

    /// Every archived member parses, is rewritable, and survives the *writer*.
    ///
    /// `parsed` and `import_verified` are the assertions that can fail; the structural counter is
    /// checked only so that a regression in the guarantee the module header states is visible.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_archived_wave_round_trips() {
        let directory = game_directory();
        let mut total = 0_usize;
        for archive in ["sndfx.mpq", "special.mpq"] {
            let sweep = sweep_archive(&directory.join(archive));
            assert!(sweep.checked > 0, "{archive} held no members");
            assert_eq!(sweep.failures, Vec::new(), "{archive}");
            assert_eq!(sweep.skipped, 0, "{archive}: a member is not a WAVE");
            assert_eq!(
                sweep.parsed, sweep.checked,
                "{archive}: a member did not parse"
            );
            assert_eq!(sweep.refusals, Vec::new(), "{archive}");
            assert_eq!(
                sweep.import_verified, sweep.parsed,
                "{archive}: a member did not survive the writer"
            );
            assert_eq!(
                sweep.import_identical, sweep.parsed,
                "{archive}: the writer changed a member given its own audio"
            );
            assert_eq!(sweep.import_differences, Vec::new(), "{archive}");
            assert_eq!(sweep.riff_size_mismatch, 0, "{archive}");
            assert_eq!(
                sweep.reserialised_identical, sweep.parsed,
                "{archive}: the fmt fields or the PCM conversion do not reproduce"
            );
            total += sweep.checked;
        }
        assert_eq!(total, 3_098, "the archived WAVE corpus changed size");
    }

    /// The loose `Wav/` tree beside the archives -- music and scenario narration.
    ///
    /// Held to exactly the same assertions, and to a size tripwire of its own. The review found
    /// this population covered only by `checked > 0`, which is where a writer bug would hide.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_loose_wave_round_trips() {
        let mut sweep = WaveSweep::default();
        for (name, bytes) in &loose_wave_files() {
            sweep.observe(name, bytes);
        }
        assert_eq!(sweep.checked, 42, "the loose WAVE corpus changed size");
        assert_eq!(sweep.failures, Vec::new());
        assert_eq!(sweep.skipped, 0);
        assert_eq!(sweep.parsed, sweep.checked);
        assert_eq!(sweep.refusals, Vec::new());
        assert_eq!(sweep.import_verified, sweep.parsed);
        assert_eq!(sweep.import_identical, sweep.parsed);
        assert_eq!(sweep.riff_size_mismatch, 0);
        assert_eq!(sweep.reserialised_identical, sweep.parsed);
    }

    /// Every archived member is PCM, and every one of its format fields is in the attested set.
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

    /// The export/import pair over **every** member of both archives and the loose tree.
    ///
    /// The review found this covering the first 200 members of `sndfx.mpq` and nothing else, which
    /// is precisely where the `rebuild` pad-byte bug survived. Both populations now run in full.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_member_survives_export_and_import() {
        let directory = game_directory();
        let mut checked = 0_usize;
        for archive in ["sndfx.mpq", "special.mpq"] {
            let handle =
                crate::mpq::Archive::open(&directory.join(archive)).expect("open the archive");
            for entry in &handle.entries().expect("enumerate the archive") {
                let bytes = handle.read(&entry.name).expect("read the member");
                let file = WaveFile::parse(&bytes).expect("parse the member");
                let exported = file.encode().expect("export");
                let reimported = import_samples(&exported, &bytes, ImportOptions::default())
                    .unwrap_or_else(|error| panic!("{archive}!{}: {error}", entry.name));
                assert_eq!(
                    reimported, bytes,
                    "{archive}!{} did not survive export and import",
                    entry.name
                );
                checked += 1;
            }
        }
        for (name, bytes) in &loose_wave_files() {
            let file = WaveFile::parse(bytes).expect("parse");
            let exported = file.encode().expect("export");
            let reimported = import_samples(&exported, bytes, ImportOptions::default())
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(&reimported, bytes, "{name} did not survive export and import");
            checked += 1;
        }
        assert_eq!(checked, 3_140, "the WAVE corpus changed size");
    }

    /// Editing a member that carries loop metadata down to a shorter length is refused.
    ///
    /// Run against real `smpl` and `cue ` chunks rather than a fixture, because the fixture is
    /// written from the same documented layout the parser reads.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn shortening_a_member_with_loop_metadata_is_refused() {
        let directory = game_directory();
        let handle =
            crate::mpq::Archive::open(&directory.join("sndfx.mpq")).expect("open sndfx.mpq");
        let mut refused = 0_usize;
        let mut carried = 0_usize;
        for entry in &handle.entries().expect("enumerate sndfx.mpq") {
            let bytes = handle.read(&entry.name).expect("read the member");
            let file = WaveFile::parse(&bytes).expect("parse");
            let Some((_, last)) = file.last_referenced_frame() else {
                continue;
            };
            carried += 1;
            if last == 0 {
                continue;
            }
            // One frame of audio: anything with a nonzero loop or cue point must now dangle.
            let frame_bytes = file.format.frame_bytes().expect("frame size");
            let short = {
                let mut shortened = file.clone();
                shortened.samples.interleaved.truncate(frame_bytes);
                shortened.rebuild().expect("rebuild a short edit")
            };
            let error = import_samples(&short, &bytes, ImportOptions::default())
                .expect_err("the metadata must dangle");
            assert!(
                error.to_string().contains("dangling-loop-metadata"),
                "{}: {error}",
                entry.name
            );
            import_samples(
                &short,
                &bytes,
                ImportOptions {
                    allow_dangling_loops: true,
                    ..ImportOptions::default()
                },
            )
            .expect("explicitly allowed");
            refused += 1;
        }
        assert!(carried > 0, "no sndfx member carries loop metadata");
        assert!(refused > 0, "no sndfx member has a nonzero loop or cue point");
    }
}
