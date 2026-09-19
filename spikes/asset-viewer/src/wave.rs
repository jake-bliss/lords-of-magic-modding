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
///
/// `dwEnd` is **inclusive** -- the Microsoft RIFF 1994 specification says of it that "this sample
/// will also be played", so a loop ending at `dwEnd` needs `dwEnd + 1` frames of audio. Writers are
/// known to disagree about this, so the corpus was asked as well: of the 41 loop records in
/// `sndfx.mpq` and `special.mpq`, **34 end at exactly `frames - 1` and none at `frames`**
/// (**Observed in the corpus**). Both authorities agree, which is why the gate rejects
/// `end >= frames` rather than `end > frames`.
///
/// This is the first claim in this module grounded in an **external authority** rather than in the
/// shipped files. It is stronger evidence than the corpus can give on its own -- and it is exactly
/// the instrument the Smacker header layout still lacks.
const SMPL_LOOP_BYTES: usize = 24;
/// One `cue ` point record. Its last field is the sample-frame offset. **Documented.**
///
/// `dwSampleOffset` is a **position**, so an offset equal to the frame count is already past the
/// end; there is no ambiguity here of the kind `dwEnd` has. **Observed in the corpus:** 26 of the
/// 116 cue points sit at `frames - 1` and none at `frames`.
const CUE_POINT_BYTES: usize = 24;

/// Why a WAVE could not be turned into samples.
///
/// The distinction is not cosmetic. **`Unsupported` is a limit of this tool** -- a legal file in a
/// format with no decoder here -- and a classifier may report it and carry on. **`Malformed` is a
/// property of the file**, and a classifier that downgrades it has made its own failure count
/// invisible: a PCM member declaring zero channels would be reported as "a WAVE this tool merely
/// cannot decode" and the archive would still scan with zero failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveErrorKind {
    /// The container or the `fmt ` fields are wrong, or supported PCM will not decode.
    Malformed,
    /// A format tag or bit depth this module does not implement. The file may be perfectly valid.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveError {
    message: String,
    kind: WaveErrorKind,
}

impl WaveError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: WaveErrorKind::Malformed,
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: WaveErrorKind::Unsupported,
        }
    }

    pub fn kind(&self) -> WaveErrorKind {
        self.kind
    }
}

impl fmt::Display for WaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
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
            return Err(WaveError::unsupported(format!(
                "WAVE format tag {} is not PCM and has no decoder here",
                format.encoding
            )));
        }
        // `frame_bytes` before the depth dispatch, not after. The other order let a **malformed**
        // file be reported as merely unsupported: `bits_per_sample = 0` fell to the catch-all arm
        // and came back `Unsupported`, so the probe classified it and the archive still scanned
        // with zero failures. Zero channels paired with an unsupported depth hid the same way.
        // A valid 24-bit file passes this line and is still `Unsupported` at the next one.
        let frame_bytes = format.frame_bytes()?;
        let sample_bytes = match format.bits_per_sample {
            8 => 1_usize,
            16 => 2_usize,
            other => {
                return Err(WaveError::unsupported(format!(
                    "PCM bit depth {other} has no decoder here"
                )));
            }
        };
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
        /// `smpl` or `cue `, kept so the refusal names which kind of metadata dangles.
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

/// The pad byte a rewritten chunk must carry, given what the template carried.
///
/// RIFF pads an odd-sized chunk to an even boundary, but the **final** chunk in a file may simply
/// end: the parser records `pad: None` for it, because inventing one would make the output a byte
/// longer than the input.
///
/// The rule turns on **parity**, not on the lengths being equal:
///
/// * an even-length body takes no pad;
/// * an odd-length body whose template body was **also odd** takes exactly what the template had,
///   including `None`. The length may have changed; the pad is still the template's, because the
///   template had one for this parity;
/// * an odd-length body whose template body was **even** takes `Some(0)`, because the template
///   carried no pad for a parity it did not have.
///
/// The middle case is the one this got wrong twice. It first read "unchanged **length**" instead
/// of "unchanged **parity**", so editing a 3-byte chunk to 5 bytes rewrote a `0x20` pad to `0x00`
/// and made an unpadded final chunk gain a byte -- silently, because the check on the writer was
/// calling this same function and agreeing with it. See [`verify_import`] for why it no longer
/// does.
fn expected_pad(template_pad: Option<u8>, template_len: u32, new_len: u32) -> Option<u8> {
    if new_len.is_multiple_of(2) {
        None
    } else if !template_len.is_multiple_of(2) {
        template_pad
    } else {
        Some(0)
    }
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
        for (chunk, body) in self.chunks.iter().zip(&bodies) {
            let size = u32::try_from(body.len())
                .map_err(|_| WaveError::new("a rebuilt WAVE chunk exceeds 4 GiB"))?;
            let pad = expected_pad(chunk.pad, chunk.declared_size, size);
            payload += 8 + body.len() as u64 + u64::from(pad.is_some());
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
            if let Some(pad) = expected_pad(chunk.pad, chunk.declared_size, size) {
                out.push(pad);
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

    /// Every sample frame this container's `smpl` loops and `cue ` points refer to.
    ///
    /// Exposed rather than folded straight into a maximum so the *distribution* can be asserted
    /// against the corpus. That distribution is the evidence the inclusive reading rests on, and a
    /// decision whose evidence no test pins is a decision that quietly becomes folklore.
    pub fn loop_and_cue_references(&self) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        for chunk in &self.chunks {
            let WaveChunkBody::Other(body) = &chunk.body else {
                continue;
            };
            let frames = match &chunk.id {
                b"smpl" => smpl_frames(body),
                b"cue " => cue_frames(body),
                _ => Vec::new(),
            };
            for frame in frames {
                out.push((printable_tag(&chunk.id), frame));
            }
        }
        out
    }

    /// The highest sample frame any `smpl` loop or `cue ` point in this container refers to.
    ///
    /// Returns the chunk that names it alongside the frame. Both address **sample frames**, and
    /// both are inclusive positions, so the audio must hold `frame + 1` frames for the reference to
    /// land inside it. See [`SMPL_LOOP_BYTES`] for the evidence behind that reading.
    pub fn last_referenced_frame(&self) -> Option<(String, u64)> {
        self.loop_and_cue_references()
            .into_iter()
            .max_by_key(|(_, frame)| *frame)
    }
}

/// Every loop endpoint in a `smpl` chunk. **Documented:** 36-byte header, then 24-byte loop
/// records whose third and fourth words are the start and end sample frames.
///
/// One entry per loop, carrying `max(start, end)`: both are positions in the audio, and a start
/// past the end of a shortened file dangles exactly as an end does.
fn smpl_frames(body: &[u8]) -> Vec<u64> {
    if body.len() < SMPL_HEADER_BYTES {
        return Vec::new();
    }
    let Ok(declared) = read_u32(body, 28) else {
        return Vec::new();
    };
    // The loop count is a declared value, so the array is bounded by the bytes actually present
    // rather than by the count. A file claiming four billion loops reads the ones it has.
    let available = (body.len() - SMPL_HEADER_BYTES) / SMPL_LOOP_BYTES;
    let mut out = Vec::new();
    for index in 0..(declared as usize).min(available) {
        let at = SMPL_HEADER_BYTES + index * SMPL_LOOP_BYTES;
        let (Ok(start), Ok(end)) = (read_u32(body, at + 8), read_u32(body, at + 12)) else {
            continue;
        };
        out.push(u64::from(start).max(u64::from(end)));
    }
    out
}

/// Every sample offset in a `cue ` chunk. **Documented:** a 4-byte count, then 24-byte points
/// whose last word is the sample-frame offset.
fn cue_frames(body: &[u8]) -> Vec<u64> {
    if body.len() < 4 {
        return Vec::new();
    }
    let Ok(declared) = read_u32(body, 0) else {
        return Vec::new();
    };
    let available = (body.len() - 4) / CUE_POINT_BYTES;
    let mut out = Vec::new();
    for index in 0..(declared as usize).min(available) {
        let at = 4 + index * CUE_POINT_BYTES + 20;
        if let Ok(offset) = read_u32(body, at) {
            out.push(u64::from(offset));
        }
    }
    out
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
        // `>=`, not `>`: both kinds of reference are inclusive positions, so a reference to frame
        // `frames` names a frame one past the last one that exists.
        if last >= frames {
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
    // No `attested()` call here. There was one, and it could not fire: `import_samples` runs
    // `rewrite_refusal` over the whole edited file first, which checks the same four fields and
    // gives a better message. A check that cannot fail reads like protection and is not, which is
    // the mistake this module has already had to undo once. The invariant is asserted instead, so
    // a future caller that skips the whole-file check trips it in a debug build rather than
    // inheriting a silent gap.
    debug_assert!(
        target.attested().is_ok(),
        "the caller must reject an unattested edit before resolving a target format"
    );
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
        // Derived here rather than by calling `expected_pad`, and the duplication is the point.
        //
        // This repository has a recorded lesson that two implementations agreeing is not
        // confirmation when they share an assumption, and the pad byte has now demonstrated it
        // three times: the writer zeroed a pad, then the check could not see a pad being added,
        // then -- once the two were unified on one helper -- both agreed on the wrong answer for
        // an odd-to-odd length change and nothing could notice. A guard that calls the code it
        // guards cannot catch that code being wrong.
        //
        // The expectation is read off the TEMPLATE's own bytes: a pad exists iff the written body
        // is odd and the template carried one for an odd body, and its value is the template's.
        let wanted = if out.declared_size.is_multiple_of(2) {
            None
        } else if source.declared_size.is_multiple_of(2) {
            Some(0)
        } else {
            source.pad
        };
        if out.pad != wanted {
            return Err(WaveRefusal::ContainerChanged {
                detail: match (out.pad, wanted) {
                    (Some(written), None) => format!(
                        "a pad byte 0x{written:02x} was invented after {}, which the template does \
                         not carry",
                        printable_tag(&out.id)
                    ),
                    (None, Some(_)) => format!(
                        "the pad byte after {} was dropped",
                        printable_tag(&out.id)
                    ),
                    (Some(written), Some(source_pad)) => format!(
                        "the pad byte after {} became 0x{written:02x}, the template has \
                         0x{source_pad:02x}",
                        printable_tag(&out.id)
                    ),
                    (None, None) => unreachable!("equal options do not reach this arm"),
                },
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

    /// The pad case the guard could not see: a template whose final odd chunk carries no pad.
    ///
    /// `walk_chunks` records `pad: None` only at end of file, so this is a real 47-byte member
    /// shape. The writer used to invent a `0x00`, fold it into the recomputed `RIFF` size so that
    /// every other check agreed, and return a 48-byte "identical" import.
    #[test]
    fn a_final_odd_chunk_with_no_pad_does_not_gain_one() {
        let template = canonical(1, 22050, 8, &[1, 2, 3]);
        // `canonical` pads, so strip the pad back off to build the unpadded shape.
        let mut template = template;
        assert_eq!(template.pop(), Some(0));
        let payload = template.len() as u32 - 8;
        template[4..8].copy_from_slice(&payload.to_le_bytes());
        assert_eq!(template.len(), 47);

        let file = WaveFile::parse(&template).expect("parse");
        let data = file
            .chunks
            .iter()
            .find(|chunk| chunk.id == *b"data")
            .expect("a data chunk");
        assert_eq!(data.pad, None, "the fixture must carry no final pad");
        assert_eq!(file.encode().expect("encode"), template);
        assert_eq!(file.rebuild().expect("rebuild"), template, "rebuild invented a pad");

        let imported =
            import_samples(&template, &template, ImportOptions::default()).expect("import");
        assert_eq!(imported.len(), 47, "the import grew by an invented pad byte");
        assert_eq!(imported, template);
    }

    #[test]
    fn the_import_post_condition_catches_an_invented_pad_byte() {
        let mut template = canonical(1, 22050, 8, &[1, 2, 3]);
        assert_eq!(template.pop(), Some(0));
        let payload = template.len() as u32 - 8;
        template[4..8].copy_from_slice(&payload.to_le_bytes());
        let good = WaveFile::parse(&template).expect("parse");
        let mut damaged = good.clone();
        for chunk in &mut damaged.chunks {
            if chunk.id == *b"data" {
                chunk.pad = Some(0);
            }
        }
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("a pad was invented");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("invented")),
            "{refusal}"
        );
    }

    /// `expected_pad` is the rule the writer and the check on the writer share.
    #[test]
    fn the_pad_rule_turns_on_parity_not_on_the_length_being_unchanged() {
        // Even body: never a pad, whatever the template had.
        assert_eq!(expected_pad(Some(0x20), 3, 4), None);
        assert_eq!(expected_pad(None, 3, 4), None);
        // Odd body, unchanged length: exactly what the template had, including nothing.
        assert_eq!(expected_pad(Some(0x20), 3, 3), Some(0x20));
        assert_eq!(expected_pad(None, 3, 3), None);
        // Odd to odd with a DIFFERENT length: still the template's pad. The template had one for
        // this parity, so there is nothing to invent and nothing to drop.
        assert_eq!(expected_pad(Some(0x20), 3, 5), Some(0x20));
        assert_eq!(expected_pad(None, 3, 5), None);
        assert_eq!(expected_pad(Some(0x20), 5, 3), Some(0x20));
        // Even to odd: the template carried no pad for this parity, so zero is the only choice.
        assert_eq!(expected_pad(None, 4, 5), Some(0));
        assert_eq!(expected_pad(Some(0x20), 4, 5), Some(0));
    }

    /// The odd-to-odd case end to end, for both a padded and an unpadded template.
    #[test]
    fn an_odd_to_odd_edit_keeps_the_templates_pad_byte() {
        // Padded: a 3-byte data chunk with a 0x20 pad, edited to 5 bytes.
        let mut template = canonical(1, 22050, 8, &[1, 2, 3]);
        let last = template.len() - 1;
        template[last] = 0x20;
        template = with_chunk(template, b"LIST", b"INFO", 0);
        let edited = canonical(1, 22050, 8, &[1, 2, 3, 4, 5]);
        let written =
            import_samples(&edited, &template, ImportOptions::default()).expect("import");
        let parsed = WaveFile::parse(&written).expect("parse");
        let data = parsed
            .chunks
            .iter()
            .find(|chunk| chunk.id == *b"data")
            .expect("a data chunk");
        assert_eq!(data.declared_size, 5);
        assert_eq!(
            data.pad,
            Some(0x20),
            "an odd-to-odd edit must keep the template's own pad byte"
        );

        // Unpadded: a final 3-byte data chunk that simply ends, edited to 5 bytes.
        let mut bare = canonical(1, 22050, 8, &[1, 2, 3]);
        assert_eq!(bare.pop(), Some(0));
        let payload = bare.len() as u32 - 8;
        bare[4..8].copy_from_slice(&payload.to_le_bytes());
        let written = import_samples(&edited, &bare, ImportOptions::default()).expect("import");
        assert_eq!(
            written.len(),
            49,
            "an unpadded final chunk must not gain a pad when it stays odd"
        );
        let parsed = WaveFile::parse(&written).expect("parse");
        assert_eq!(parsed.chunks.last().expect("a chunk").pad, None);
    }

    /// The guard must catch the writer's pad rule being wrong, not agree with it.
    ///
    /// `verify_import` derives its expectation from the template's bytes rather than calling
    /// `expected_pad`. This test proves the two are genuinely independent: it feeds the check
    /// output built with the *broken* rule -- "unchanged length" instead of "unchanged parity" --
    /// and the check has to reject it. If the guard ever calls `expected_pad` again, this fails.
    #[test]
    fn the_post_condition_rejects_the_writers_pad_rule_being_wrong() {
        let mut template = canonical(1, 22050, 8, &[1, 2, 3]);
        let last = template.len() - 1;
        template[last] = 0x20;
        let template = with_chunk(template, b"LIST", b"INFO", 0);
        let good = WaveFile::parse(&template).expect("parse");

        let mut edited = good.clone();
        edited.samples.interleaved = vec![1, 2, 3, 4, 5];

        // Serialise the way the broken rule would have: odd-to-odd resets the pad to zero.
        let mut written = edited.rebuild().expect("rebuild");
        let data_pad = written
            .windows(4)
            .position(|window| window == b"data")
            .expect("a data chunk")
            + 8
            + 5;
        assert_eq!(written[data_pad], 0x20, "the correct writer keeps 0x20");
        written[data_pad] = 0x00;

        let refusal = verify_import(&written, &good, &edited.format, &edited.samples)
            .expect_err("the pad was reset by the broken rule");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("pad byte")),
            "{refusal}"
        );
    }

    // --- one damaged-container case per `verify_import` branch -----------------------------
    //
    // A mutation sweep found four of this guard's six comparisons could be deleted with the suite
    // staying green. A guard nobody mutated reports safety it has not earned, which is the same
    // failure the guard replaced. Each case below fails when its branch is removed.

    fn post_condition_fixture() -> (Vec<u8>, WaveFile) {
        let template = with_chunk(
            canonical(1, 22050, 8, &[1, 2, 3, 4]),
            b"LIST",
            b"INFOISFT",
            0,
        );
        let file = WaveFile::parse(&template).expect("parse");
        (template, file)
    }

    #[test]
    fn the_import_post_condition_catches_a_renamed_chunk() {
        let (_, good) = post_condition_fixture();
        let mut damaged = good.clone();
        damaged.chunks[2].id = *b"junk";
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("a chunk id changed");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("but the template has")),
            "{refusal}"
        );
    }

    #[test]
    fn the_import_post_condition_catches_a_changed_ancillary_body() {
        let (_, good) = post_condition_fixture();
        let mut damaged = good.clone();
        damaged.chunks[2].body = WaveChunkBody::Other(b"INFOXXXX".to_vec());
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("an ancillary body changed");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("chunk body changed")),
            "{refusal}"
        );
    }

    #[test]
    fn the_import_post_condition_catches_changed_trailing_bytes() {
        let (_, good) = post_condition_fixture();
        let mut damaged = good.clone();
        damaged.trailing = vec![0xab];
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("trailing bytes appeared");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("trailing")),
            "{refusal}"
        );
    }

    #[test]
    fn the_import_post_condition_catches_a_format_that_is_not_the_resolved_one() {
        let (_, good) = post_condition_fixture();
        let mut damaged = good.clone();
        damaged.format.sample_rate = 11025;
        let (block_align, byte_rate) = damaged.format.derived_fields().expect("derive");
        damaged.format.block_align = block_align;
        damaged.format.byte_rate = byte_rate;
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("the written format is not the resolved one");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("the import resolved")),
            "{refusal}"
        );
    }

    #[test]
    fn the_import_post_condition_catches_samples_that_are_not_the_edit() {
        let (_, good) = post_condition_fixture();
        let mut damaged = good.clone();
        damaged.samples.interleaved[0] = damaged.samples.interleaved[0].wrapping_add(1);
        let written = damaged.rebuild().expect("rebuild");
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("the samples are not the edit's");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("written samples")),
            "{refusal}"
        );
    }

    /// The `RIFF` size branch, reached by handing `verify_import` bytes whose header is wrong.
    ///
    /// `rebuild` computes that field, so the only way to exercise the check is to corrupt its
    /// output -- which is exactly what the check is for: it is the last line between a writer bug
    /// and a file on disk.
    #[test]
    fn the_import_post_condition_catches_a_wrong_riff_size() {
        let (_, good) = post_condition_fixture();
        let mut written = good.rebuild().expect("rebuild");
        let riff = u32::from_le_bytes([written[4], written[5], written[6], written[7]]);
        written[4..8].copy_from_slice(&(riff + 2).to_le_bytes());
        let refusal = verify_import(&written, &good, &good.format, &good.samples)
            .expect_err("the RIFF size field is wrong");
        assert!(
            matches!(&refusal, WaveRefusal::ContainerChanged { detail } if detail.contains("RIFF size field")),
            "{refusal}"
        );
    }

    // --- one case per `resolve_target_format` branch ---------------------------------------

    /// Unchanged nominal format: the **template's** fmt survives, not the edit's.
    ///
    /// Fails if the retention branch is reverted to returning the edit's whole `fmt `, which was a
    /// surviving mutant. The edit here carries a `byte_rate` the template does not.
    #[test]
    fn an_unchanged_format_keeps_the_templates_own_fmt_chunk() {
        let template = canonical(1, 22050, 8, &[1, 2, 3, 4]);
        let mut edited = canonical(1, 22050, 8, &[9, 9, 9, 9]);
        // A legal-but-different fmt extension the template does not have would be refused, so use
        // a distinguishable `extra`-free difference: the template is authoritative on everything.
        edited[16..20].copy_from_slice(&16_u32.to_le_bytes());
        let resolved = resolve_target_format(
            &WaveFile::parse(&template).expect("parse").format,
            &WaveFile::parse(&edited).expect("parse").format,
            ImportOptions::default(),
        )
        .expect("same nominal format");
        assert_eq!(
            resolved,
            WaveFile::parse(&template).expect("parse").format,
            "the template's fmt chunk is the one that is kept"
        );
    }

    /// Changed format: `block_align` and `byte_rate` are **derived**, not copied from the edit.
    ///
    /// Fails if the two derivation lines are deleted, both of which were surviving mutants.
    #[test]
    fn a_changed_format_derives_the_redundant_fields_rather_than_copying_them() {
        let template = WaveFile::parse(&canonical(1, 22050, 8, &[1, 2, 3, 4]))
            .expect("parse")
            .format;
        let mut edited = WaveFile::parse(&canonical(2, 22050, 8, &[1, 2, 3, 4]))
            .expect("parse")
            .format;
        // An edit that declares nonsense in the redundant fields. `rewrite_refusal` would catch
        // this on a whole file; `resolve_target_format` must not propagate it either way.
        edited.block_align = 99;
        edited.byte_rate = 7;
        let resolved = resolve_target_format(
            &template,
            &edited,
            ImportOptions {
                allow_format_change: true,
                ..ImportOptions::default()
            },
        )
        .expect("an allowed format change");
        assert_eq!(resolved.channels, 2);
        assert_eq!(resolved.block_align, 2, "derived, not the edit's 99");
        assert_eq!(resolved.byte_rate, 44100, "derived, not the edit's 7");
    }

    /// A depth-only change: same channels, same rate, 8-bit to 16-bit.
    ///
    /// The channel and rate arms of `same_nominal_format` each had a case; the depth arm did not,
    /// so removing it left the suite green. Without it an 8-bit template would be retained over
    /// 16-bit sample bytes and the post-condition would reject an import that should succeed.
    #[test]
    fn a_depth_only_format_change_is_gated_and_then_derived() {
        let template = canonical(1, 22050, 8, &[0x80, 0x80, 0x80, 0x80]);
        let edited = canonical(1, 22050, 16, &[0, 0, 0, 0, 0, 0, 0, 0]);
        let error = import_samples(&edited, &template, ImportOptions::default())
            .expect_err("a depth change still has to be asked for");
        assert!(
            error.to_string().contains("--allow-format-change"),
            "{error}"
        );

        let written = import_samples(
            &edited,
            &template,
            ImportOptions {
                allow_format_change: true,
                ..ImportOptions::default()
            },
        )
        .expect("explicitly allowed");
        let parsed = WaveFile::parse(&written).expect("parse");
        assert_eq!(parsed.format.bits_per_sample, 16);
        assert_eq!(parsed.format.channels, 1);
        assert_eq!(parsed.format.block_align, 2, "derived from the new depth");
        assert_eq!(parsed.format.byte_rate, 44100, "derived from the new depth");
        assert_eq!(parsed.samples.frames(), 4);
    }

    /// The extension-byte refusal, which survived an `if false` mutation.
    #[test]
    fn an_edit_that_introduces_fmt_extension_bytes_is_refused() {
        let template = WaveFile::parse(&canonical(1, 22050, 8, &[1, 2, 3, 4]))
            .expect("parse")
            .format;
        let mut edited = template.clone();
        edited.extra = vec![0x00, 0x00];
        let error = resolve_target_format(&template, &edited, ImportOptions::default())
            .expect_err("extension bytes have no evidence behind them");
        assert!(error.to_string().contains("extension byte"), "{error}");
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

    /// A format change to something unattested is refused even when changes are allowed.
    ///
    /// This used to pass for a reason other than the one it names: the whole-file
    /// `rewrite_refusal` on the edit caught 48 kHz before `resolve_target_format` was reached, so
    /// removing `sample_rate` from the nominal-format comparison left it green. The `fmt `-level
    /// case below reaches the gate this test is about.
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

        // Same depth and channel count, different rate: the case that isolates the rate field in
        // `same_nominal_format` rather than being caught by an earlier whole-file check.
        let attested_template = WaveFile::parse(&canonical(1, 22050, 8, &[1, 2, 3, 4]))
            .expect("parse")
            .format;
        let attested_edit = WaveFile::parse(&canonical(1, 11025, 8, &[1, 2, 3, 4]))
            .expect("parse")
            .format;
        let error = resolve_target_format(
            &attested_template,
            &attested_edit,
            ImportOptions::default(),
        )
        .expect_err("a rate change still needs to be asked for");
        assert!(
            error.to_string().contains("--allow-format-change"),
            "{error}"
        );
        let resolved = resolve_target_format(
            &attested_template,
            &attested_edit,
            ImportOptions {
                allow_format_change: true,
                ..ImportOptions::default()
            },
        )
        .expect("11025 is attested");
        assert_eq!(resolved.sample_rate, 11025);
        assert_eq!(resolved.byte_rate, 11025, "derived from the new rate");
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

    /// `dwEnd` is inclusive, so a loop over a 1,000-frame file ends at 999.
    ///
    /// The fixture used to say 1,000 and pass, which pinned the exclusive reading in as correct
    /// without ever stating that a choice was being made.
    #[test]
    fn a_smpl_loop_past_the_end_of_the_new_audio_is_refused_by_name() {
        let template = with_chunk(
            canonical(1, 22050, 8, &[0; 1000]),
            b"smpl",
            &smpl(0, 999),
            0,
        );
        assert_eq!(
            WaveFile::parse(&template)
                .expect("parse")
                .last_referenced_frame(),
            Some(("smpl".to_owned(), 999))
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
        assert!(error.to_string().contains("chunk=smpl"), "{error}");
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

    /// The three boundaries the inclusive reading turns on.
    #[test]
    fn the_loop_gate_is_inclusive_at_every_boundary() {
        let audio = |frames: usize| canonical(1, 22050, 8, &vec![0_u8; frames]);
        let template = |end: u32| with_chunk(audio(1000), b"smpl", &smpl(0, end), 0);

        // end == frames - 1: the last frame that exists. Allowed.
        import_samples(&audio(100), &template(99), ImportOptions::default())
            .expect("a loop ending on the last frame fits");
        // end == frames: one past the end. Refused -- this is the case `>` used to admit.
        let error = import_samples(&audio(100), &template(100), ImportOptions::default())
            .expect_err("a loop ending at the frame count is past the end");
        assert!(error.to_string().contains("last-referenced-frame=100"), "{error}");
        assert!(error.to_string().contains("frames=100"), "{error}");
        // Empty audio holds no frame at all, so even a reference to frame 0 dangles.
        let error = import_samples(&audio(0), &template(0), ImportOptions::default())
            .expect_err("empty audio cannot hold a loop point");
        assert!(error.to_string().contains("frames=0"), "{error}");
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
        assert_eq!(smpl_frames(&body), vec![40]);
        let mut cue = 0xffff_ffff_u32.to_le_bytes().to_vec();
        cue.extend_from_slice(&[0_u8; CUE_POINT_BYTES]);
        cue[4 + 20..4 + 24].copy_from_slice(&7_u32.to_le_bytes());
        assert_eq!(cue_frames(&cue), vec![7]);
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
    ///
    /// The edit is built by truncating the **interleaved sample vector**, and the truncation is
    /// expressed in samples. An earlier version passed `frame_bytes` -- a byte count -- to
    /// `truncate`, which for stereo 16-bit is four samples, not the "one frame" its comment
    /// claimed. Here the edit is a single frame by construction, so any reference to a frame other
    /// than 0 must dangle.
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
            let channels = usize::from(file.format.channels);
            if file.samples.frames() < 2 {
                continue;
            }
            let short = {
                let mut shortened = file.clone();
                // Exactly one frame: `channels` samples.
                shortened.samples.interleaved.truncate(channels);
                assert_eq!(shortened.samples.frames(), 1, "{}", entry.name);
                shortened.rebuild().expect("rebuild a short edit")
            };
            // With one frame, frame 0 is the only valid position, so any reference at or above 1
            // dangles -- and `last >= 1` for every member that reaches here, because a member
            // whose only reference is frame 0 would need `last == 0`.
            if last == 0 {
                import_samples(&short, &bytes, ImportOptions::default())
                    .expect("a reference to frame 0 still fits in a one-frame edit");
                continue;
            }
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
        assert_eq!(carried, 63, "the sndfx loop-metadata population changed");
        assert!(refused > 0, "no sndfx member has a nonzero loop or cue point");
    }

    /// The distribution the inclusive reading of `dwEnd` rests on.
    ///
    /// The Microsoft RIFF 1994 spec says `dwEnd` is inclusive, but writers disagree, so the gate's
    /// `>=` was decided on what the authoring tool actually emitted. Until now nothing pinned that
    /// measurement -- the sweep would reject any record at or past the end, but the *shape* of the
    /// data underneath the decision was uncorroborated, which is how a measurement becomes folklore.
    ///
    /// Under the **exclusive** reading a loop running to the end of a file is written
    /// `dwEnd == frames`. Not one record in the corpus does that. Under the **inclusive** reading
    /// it is written `dwEnd == frames - 1`, and 34 of the 41 records do exactly that. The corpus
    /// discriminates, and it agrees with the spec.
    ///
    /// The other seven are interior loops, 238 to 76,437 frames short of the end. They are named
    /// here rather than left implicit: being nowhere near the boundary, they carry no information
    /// about the convention either way.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn the_corpus_shows_smpl_end_is_an_inclusive_position() {
        let directory = game_directory();
        let mut smpl_records = 0_usize;
        let mut smpl_on_last_frame = 0_usize;
        let mut cue_points = 0_usize;
        let mut cue_on_last_frame = 0_usize;
        let mut at_or_past_end = 0_usize;
        let mut interior_gaps = std::collections::BTreeSet::new();
        for archive in ["sndfx.mpq", "special.mpq"] {
            let handle =
                crate::mpq::Archive::open(&directory.join(archive)).expect("open the archive");
            for entry in &handle.entries().expect("enumerate the archive") {
                let bytes = handle.read(&entry.name).expect("read the member");
                let file = WaveFile::parse(&bytes).expect("parse");
                let frames = file.samples.frames() as u64;
                for (kind, frame) in file.loop_and_cue_references() {
                    if frame >= frames {
                        at_or_past_end += 1;
                    }
                    let on_last = frames > 0 && frame == frames - 1;
                    if kind == "smpl" {
                        smpl_records += 1;
                        if on_last {
                            smpl_on_last_frame += 1;
                        } else {
                            interior_gaps.insert(frames - 1 - frame);
                        }
                    } else {
                        cue_points += 1;
                        if on_last {
                            cue_on_last_frame += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(smpl_records, 41, "the smpl loop population changed");
        assert_eq!(cue_points, 116, "the cue point population changed");
        assert_eq!(
            smpl_on_last_frame, 34,
            "loops ending on the last frame -- the inclusive signature"
        );
        assert_eq!(cue_on_last_frame, 26);
        assert_eq!(
            at_or_past_end, 0,
            "a shipped record at or past the end would refute the inclusive reading"
        );
        assert_eq!(
            interior_gaps,
            [238_u64, 5_584, 42_415, 76_437].into_iter().collect(),
            "the seven non-terminal loops sit far from the boundary and settle nothing"
        );
    }

    /// `files_with_loop_metadata` counts chunks that declare at least one **record**.
    ///
    /// Pinned because the number it replaced was wrong, and because it does not match the
    /// `smpl`/`cue `-bearing rows of the layout table: 96 + 96 + 23 files **carry a chunk**, while
    /// 63 + 62 + 22 declare a record inside it. Most `smpl` chunks in this corpus declare zero
    /// loops. See `docs/audio-format.md`.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn the_loop_metadata_population_is_what_the_documentation_says() {
        let directory = game_directory();
        let sndfx = sweep_archive(&directory.join("sndfx.mpq"));
        let special = sweep_archive(&directory.join("special.mpq"));
        let mut loose = WaveSweep::default();
        for (name, bytes) in &loose_wave_files() {
            loose.observe(name, bytes);
        }
        assert_eq!(sndfx.loop_metadata_files, 63);
        assert_eq!(special.loop_metadata_files, 62);
        assert_eq!(loose.loop_metadata_files, 22);
        assert_eq!(
            sndfx.loop_metadata_files + special.loop_metadata_files + loose.loop_metadata_files,
            147
        );

        // The other number, reached by a different path: chunk PRESENCE, counted off the layout
        // strings rather than by parsing any record. 215 files carry a `smpl` or `cue ` chunk and
        // only 147 put a record in one, which is the discrepancy a reader checking the layout
        // table's arithmetic would otherwise conclude was an error.
        let carrying = |sweep: &WaveSweep| -> usize {
            sweep
                .layouts
                .iter()
                .filter(|(layout, _)| layout.contains("smpl") || layout.contains("cue "))
                .map(|(_, count)| *count)
                .sum()
        };
        assert_eq!(carrying(&sndfx), 96);
        assert_eq!(carrying(&special), 96);
        assert_eq!(carrying(&loose), 23);
        assert_eq!(
            carrying(&sndfx) + carrying(&special) + carrying(&loose),
            215,
            "files carrying a chunk, as opposed to the 147 declaring a record in one"
        );
    }
}
