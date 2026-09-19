use std::collections::BTreeMap;
use std::fmt;

const FILE_HEADER_SIZE: usize = 32;
const SEQUENCE_RECORD_SIZE: usize = 16;
const FACING_RECORD_SIZE: usize = 8;
const FRAME_RECORD_SIZE: usize = 16;
const HOTSPOT_RECORD_SIZE: usize = 6;
const HOTSPOT_ALIGNMENT: usize = 8;
const PALETTE_COLORS: usize = 256;
const PALETTE_BYTES: usize = PALETTE_COLORS * 4;
const FRAME_FLAG_SHARED_PIXELS: u8 = 0x04;
const FRAME_FLAG_DUPLICATE: u8 = 0x08;
const FILE_FLAG_RLE: u8 = 0x01;
const FILE_FLAG_DEPTH: u8 = 0x30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpFrame {
    pub flags: u8,
    pub width: u16,
    pub height: u16,
    pub origin_x: Option<i16>,
    pub origin_y: Option<i16>,
    pub hotspots: Vec<ImpHotspot>,
    pub palette_indices: Vec<u8>,
    pub rgba: Vec<u8>,
    pub source_frame: Option<usize>,
    /// Byte offset of the 16-byte frame record that backs this frame.
    ///
    /// Distinct frames normally have distinct records: a facing's frame table is an ordinary
    /// array of 16-byte records. Records may nevertheless *alias*, because a `0x04` shared-pixel
    /// record can point at another record's pixel payload and two records can hold the same
    /// offsets. Where they do, editing a record edits every frame that shares it — see
    /// [`ImpSprite::frames_sharing_record`].
    pub record_offset: usize,
    /// Byte offset of this frame's hotspot array, when it has one.
    ///
    /// `None` means the frame carries an origin pair in the same dword instead; the two are
    /// mutually exclusive, because they occupy the same four bytes at record offset 8.
    pub hotspot_offset: Option<usize>,
    /// Byte length of this frame's *bit-packed* pixels, before any RLE.
    ///
    /// **Not derivable from the dimensions.** [`packed_sizes`] returns several sizes a frame is
    /// allowed to use — a tight bitstream and a row-padded one, which differ whenever a row's bits
    /// do not fill a whole number of bytes — and the parser accepts any of them. Which one a given
    /// frame actually used is therefore an observation, and a re-encoder that picks for itself
    /// changes the frame's size. Recorded here so an encoder can reproduce it.
    ///
    /// `None` for duplicate and shared-pixel frames, which carry no pixels of their own.
    pub packed_size: Option<usize>,
    /// Absolute file offset of this frame's stored pixel payload.
    ///
    /// `None` for duplicate and shared-pixel frames: their record's pixel dword is either a frame
    /// index or a pointer into *another* frame's payload, so it is not this frame's own.
    pub pixels_offset: Option<usize>,
    /// Byte length of the stored payload at [`ImpFrame::pixels_offset`] — the RLE stream when the
    /// file is compressed, and the packed pixels themselves when it is not.
    pub stored_size: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpHotspot {
    pub id: u16,
    pub x: i16,
    pub y: i16,
    pub raw: [u8; HOTSPOT_RECORD_SIZE],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpFacing {
    pub metadata: u16,
    pub first_frame: usize,
    pub frame_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpSequence {
    pub metadata: [u8; 11],
    pub first_facing: usize,
    pub facing_count: usize,
    pub first_frame: usize,
    pub frame_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpSprite {
    pub file_flags: u8,
    pub record_variant: u8,
    pub compressed: bool,
    pub bits_per_pixel: u8,
    pub maximum_width: u16,
    pub maximum_height: u16,
    /// Palette index treated as transparent, read from header byte 3.
    pub color_key: u8,
    pub sequence_count: usize,
    pub facing_count: usize,
    pub frame_count: usize,
    pub duplicate_frame_count: usize,
    /// Frames carrying only `FRAME_FLAG_DUPLICATE` (0x08), i.e. true back-references
    /// to an earlier frame index. Excludes `FRAME_FLAG_SHARED_PIXELS` (0x04) frames,
    /// which `duplicate_frame_count` also counts.
    ///
    /// Kept separate for analysis only. Validating this against the generated header's
    /// "Duplicate bitmaps found" statistic instead of `duplicate_frame_count` raises corpus
    /// failures from 0 to 107 — measured in a local binary on 2026-09-17 *after* the frame-table
    /// fix; the same substitution before the fix read 10 to 112. So that statistic counts both
    /// flags, on either decoder.
    pub back_reference_frame_count: usize,
    pub hotspot_count: usize,
    pub hotspot_bytes: u64,
    pub raw_pixel_bytes: u64,
    pub stored_pixel_bytes: u64,
    pub palette: Vec<[u8; 4]>,
    pub sequences: Vec<ImpSequence>,
    pub facings: Vec<ImpFacing>,
    pub frames: Vec<ImpFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpHeaderStats {
    pub sequence_name: String,
    pub sequence_labels: Vec<Vec<String>>,
    pub sequence_count: usize,
    pub frame_count: usize,
    pub duplicate_frame_count: usize,
    pub raw_pixel_bytes: u64,
    pub hotspot_bytes: u64,
    pub compressed_pixel_bytes: Option<u64>,
}

/// A statistic that a generated `.h` reports and that the decoder can measure independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImpStatistic {
    SequenceCount,
    FrameCount,
    DuplicateFrameCount,
    RawPixelBytes,
    HotspotBytes,
    StoredPixelBytes,
}

impl ImpStatistic {
    pub fn label(self) -> &'static str {
        match self {
            Self::SequenceCount => "sequence count",
            Self::FrameCount => "frame count",
            Self::DuplicateFrameCount => "duplicate frame count",
            Self::RawPixelBytes => "raw pixel bytes",
            Self::HotspotBytes => "hotspot bytes",
            Self::StoredPixelBytes => "stored pixel bytes",
        }
    }
}

impl fmt::Display for ImpStatistic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// One measured statistic that the binary and its generated header report differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpDisagreement {
    pub statistic: ImpStatistic,
    pub binary: u64,
    pub header: u64,
}

/// Why a member cannot validate exactly, as established by the 2026-09-17 corpus survey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpExceptionClass {
    /// The header describes the same structure but different pixels.
    ///
    /// Sequence count, frame count, duplicate count and hotspot bytes all agree; only the pixel
    /// byte totals differ, so the art was revised without regenerating the `.h`.
    HeaderPredatesArtRevision,
    /// The header describes a structure that is not this file's at all.
    ///
    /// Frame count itself disagrees, so no reading of the statistics can reconcile them.
    HeaderDescribesAnotherBuild,
}

/// A member that is known not to validate exactly, with the reason and the exact numbers.
///
/// The waived list is **value-pinned**: [`ImpValidationException::covers`] only accepts the
/// recorded disagreements, in order, with the recorded values. A decoder regression that changes
/// any of these numbers, or that breaks a statistic the exception does not mention, fails the
/// validator again. Nothing here loosens a bounds check — every member is still fully parsed.
#[derive(Debug, Clone, Copy)]
pub struct ImpValidationException {
    /// Normalized member stem, e.g. `units/imp/orcr4b`. See [`normalize_imp_member`].
    pub member: &'static str,
    pub class: ImpExceptionClass,
    pub reason: &'static str,
    pub waived: &'static [ImpDisagreement],
}

impl ImpValidationException {
    /// True only when `observed` is exactly the recorded disagreement list.
    pub fn covers(&self, observed: &[ImpDisagreement]) -> bool {
        observed == self.waived
    }
}

const fn waive(statistic: ImpStatistic, binary: u64, header: u64) -> ImpDisagreement {
    ImpDisagreement {
        statistic,
        binary,
        header,
    }
}

/// The five archive members that cannot validate exactly, each with its measured numbers.
///
/// Observed in a local binary on 2026-09-17 against the shipped `imp.mpq` of Lords of Magic
/// Special Edition (GS5R3), and re-measured after the frame-table fix of the same day: 1,795 of
/// the 1,800 paired members validate on every statistic and these five do not.
///
/// This list held ten members until the frame-table fix. The other five — `units/imp/aicr3b`,
/// `units/imp/chcr3b`, `units/imp/chwmmb`, `units/imp/ficr3b` and `units/imp/ficr5b` — were not
/// archive defects at all: the decoder keyed a whole facing off its first frame record's `0x04`
/// flag and pointed every frame of that facing at that one record, swallowing real records. They
/// validate exactly now. See `docs/research-log.md` for the correction.
pub const IMP_VALIDATION_EXCEPTIONS: &[ImpValidationException] = &[
    ImpValidationException {
        member: "missile/lsp01ap",
        class: ImpExceptionClass::HeaderPredatesArtRevision,
        reason: "header's 540672 raw bytes is exactly 33 * 128 * 128, a uniform uncropped canvas, \
                 but the file's maximum frame size is 60x83 and its 33 frames are cropped; \
                 missile/lsp01apa declares the same sequence and validates at 31804",
        waived: &[
            waive(ImpStatistic::RawPixelBytes, 47_115, 540_672),
            waive(ImpStatistic::StoredPixelBytes, 22_616, 533_161),
        ],
    },
    ImpValidationException {
        member: "units/imp/lifitam",
        class: ImpExceptionClass::HeaderPredatesArtRevision,
        reason: "structure agrees exactly (2 sequences, 35 frames, 0 duplicates, 0 hotspot bytes) \
                 and only the pixel totals differ; no member in the archive measures 170700",
        waived: &[
            waive(ImpStatistic::RawPixelBytes, 194_985, 170_700),
            waive(ImpStatistic::StoredPixelBytes, 81_157, 76_301),
        ],
    },
    ImpValidationException {
        member: "units/imp/lifitbm",
        class: ImpExceptionClass::HeaderPredatesArtRevision,
        reason: "structure agrees exactly and only the pixel totals differ; no member in the \
                 archive measures 49014",
        waived: &[
            waive(ImpStatistic::RawPixelBytes, 47_524, 49_014),
            waive(ImpStatistic::StoredPixelBytes, 23_104, 25_521),
        ],
    },
    ImpValidationException {
        member: "units/imp/lifitfm",
        class: ImpExceptionClass::HeaderPredatesArtRevision,
        reason: "its .h is byte-identical to units/imp/lifitam.h and declares sequence LIFITAM; \
                 the two .imp members also measure identically, so this fails exactly as \
                 units/imp/lifitam does and for the same stale-header reason",
        waived: &[
            waive(ImpStatistic::RawPixelBytes, 194_985, 170_700),
            waive(ImpStatistic::StoredPixelBytes, 81_157, 76_301),
        ],
    },
    ImpValidationException {
        member: "units/imp/orcr4b",
        class: ImpExceptionClass::HeaderDescribesAnotherBuild,
        reason: "the only pair in the archive where the file holds FEWER duplicates than the \
                 header claims (0 against 50) and the frame count itself disagrees; the file's \
                 structure matches its sibling units/imp/orcr4a exactly (7 sequences, 92 frames, \
                 0 duplicates, 1472 hotspot bytes) while the header's 86/50/576 matches no \
                 member, so the art was rebuilt and the .h was never regenerated",
        waived: &[
            waive(ImpStatistic::FrameCount, 92, 86),
            waive(ImpStatistic::DuplicateFrameCount, 0, 50),
            waive(ImpStatistic::RawPixelBytes, 58_176, 22_957),
            waive(ImpStatistic::HotspotBytes, 1_472, 576),
            waive(ImpStatistic::StoredPixelBytes, 38_761, 15_303),
        ],
    },
];

/// A member with no counterpart, and the catalog reason it has none.
///
/// The note is **value-pinned** the same way a validation exception is: `facts` records what the
/// member measures, and [`ImpOrphanNote::verify`] re-measures it. Accepting an orphan on its name
/// alone would let a truncated or substituted member pass the corpus run silently, which is what
/// the validator did before 2026-09-17.
#[derive(Debug, Clone, Copy)]
pub struct ImpOrphanNote {
    /// Normalized member name including extension, e.g. `imp/fleemarka.imp`.
    pub member: &'static str,
    pub reason: &'static str,
    pub facts: ImpOrphanFacts,
}

/// The measurable properties a catalog note asserts about the member it excuses.
#[derive(Debug, Clone, Copy)]
pub enum ImpOrphanFacts {
    /// A `.h` with no `.imp`: the sequence name and statistics its own text declares.
    Header {
        sequence_name: &'static str,
        sequence_count: usize,
        frame_count: usize,
        duplicate_frame_count: usize,
        raw_pixel_bytes: u64,
        hotspot_bytes: u64,
        compressed_pixel_bytes: Option<u64>,
    },
    /// An `.imp` with no `.h`: the statistics the decoder measures from its bytes.
    Sprite {
        sequence_count: usize,
        frame_count: usize,
        duplicate_frame_count: usize,
        raw_pixel_bytes: u64,
        hotspot_bytes: u64,
        stored_pixel_bytes: u64,
    },
}

impl ImpOrphanNote {
    /// Parse `bytes` as the kind this note claims and check every pinned value.
    ///
    /// A parse error, or any differing statistic, comes back as an error naming the field. A
    /// member that merely shares the catalogued *name* therefore no longer passes.
    pub fn verify(&self, bytes: &[u8]) -> Result<(), ImpError> {
        let mut mismatches = Vec::new();
        let mut check = |field: &str, measured: String, pinned: String| {
            if measured != pinned {
                mismatches.push(format!("{field} measured {measured}, note pins {pinned}"));
            }
        };
        match self.facts {
            ImpOrphanFacts::Header {
                sequence_name,
                sequence_count,
                frame_count,
                duplicate_frame_count,
                raw_pixel_bytes,
                hotspot_bytes,
                compressed_pixel_bytes,
            } => {
                let stats = ImpHeaderStats::parse(bytes)?;
                check(
                    "declared sequence name",
                    stats.sequence_name.to_ascii_lowercase(),
                    sequence_name.to_ascii_lowercase(),
                );
                check(
                    "sequence count",
                    stats.sequence_count.to_string(),
                    sequence_count.to_string(),
                );
                check(
                    "frame count",
                    stats.frame_count.to_string(),
                    frame_count.to_string(),
                );
                check(
                    "duplicate frame count",
                    stats.duplicate_frame_count.to_string(),
                    duplicate_frame_count.to_string(),
                );
                check(
                    "raw pixel bytes",
                    stats.raw_pixel_bytes.to_string(),
                    raw_pixel_bytes.to_string(),
                );
                check(
                    "hotspot bytes",
                    stats.hotspot_bytes.to_string(),
                    hotspot_bytes.to_string(),
                );
                check(
                    "compressed pixel bytes",
                    format!("{:?}", stats.compressed_pixel_bytes),
                    format!("{compressed_pixel_bytes:?}"),
                );
            }
            ImpOrphanFacts::Sprite {
                sequence_count,
                frame_count,
                duplicate_frame_count,
                raw_pixel_bytes,
                hotspot_bytes,
                stored_pixel_bytes,
            } => {
                let sprite = ImpSprite::parse(bytes)?;
                check(
                    "sequence count",
                    sprite.sequence_count.to_string(),
                    sequence_count.to_string(),
                );
                check(
                    "frame count",
                    sprite.frame_count.to_string(),
                    frame_count.to_string(),
                );
                check(
                    "duplicate frame count",
                    sprite.duplicate_frame_count.to_string(),
                    duplicate_frame_count.to_string(),
                );
                check(
                    "raw pixel bytes",
                    sprite.raw_pixel_bytes.to_string(),
                    raw_pixel_bytes.to_string(),
                );
                check(
                    "hotspot bytes",
                    sprite.hotspot_bytes.to_string(),
                    hotspot_bytes.to_string(),
                );
                check(
                    "stored pixel bytes",
                    sprite.stored_pixel_bytes.to_string(),
                    stored_pixel_bytes.to_string(),
                );
            }
        }
        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(ImpError::new(format!(
                "IMP orphan {} does not match its catalog note: {}",
                self.member,
                mismatches.join("; ")
            )))
        }
    }
}

/// Members that remain unpaired after the declared-sequence-name fallback.
///
/// The archive holds 3,600 `.imp`/`.h` members and `1798 * 2 + 4 == 3600`, so nothing is missing;
/// all four originally-orphaned members are naming artifacts. Two of the four pair up once the
/// header's declared sequence name is consulted (`imp/fleemark.h` declares UNMRKA and matches
/// `imp/unmrka.imp`; `units/imp/chwmcbm.h` declares DEWMHB and matches `units/imp/dewmhb.imp`).
/// The remaining two are catalogued here, with the values each one measures.
pub const IMP_ORPHAN_NOTES: &[ImpOrphanNote] = &[
    ImpOrphanNote {
        member: "aura/lsp01ea.h",
        reason: "stray header copy: declares sequence SPL01EA, which has no .imp in the archive, \
                 and is byte-identical to aura/fsp03aa.h, whose .imp matches its statistics \
                 exactly (1 sequence, 9 frames, 0 duplicates, 5597 raw, 2065 stored)",
        facts: ImpOrphanFacts::Header {
            sequence_name: "spl01ea",
            sequence_count: 1,
            frame_count: 9,
            duplicate_frame_count: 0,
            raw_pixel_bytes: 5_597,
            hotspot_bytes: 0,
            compressed_pixel_bytes: Some(2_065),
        },
    },
    ImpOrphanNote {
        member: "imp/fleemarka.imp",
        reason: "unreferenced art copy: measures identically to imp/unmrka.imp (1 sequence, \
                 13 frames, 0 duplicates, 27054 raw, uncompressed) and no header in the archive \
                 declares sequence FLEEMARKA, so it shipped without a header of its own",
        facts: ImpOrphanFacts::Sprite {
            sequence_count: 1,
            frame_count: 13,
            duplicate_frame_count: 0,
            raw_pixel_bytes: 27_054,
            hotspot_bytes: 0,
            stored_pixel_bytes: 27_054,
        },
    },
];

/// Look up a validation exception by normalized member stem.
pub fn imp_validation_exception(member: &str) -> Option<&'static ImpValidationException> {
    IMP_VALIDATION_EXCEPTIONS
        .iter()
        .find(|exception| exception.member == member)
}

/// Look up an orphan catalog note by normalized member name, extension included.
pub fn imp_orphan_note(member: &str) -> Option<&'static ImpOrphanNote> {
    IMP_ORPHAN_NOTES.iter().find(|note| note.member == member)
}

/// Archive member names use backslashes and mixed case; exception keys use neither.
pub fn normalize_imp_member(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

/// The last path component of an archive member name, for either separator.
///
/// Member names in this archive use `\`, but a caller that has already normalized a name holds
/// `/`. Splitting on one separator only silently yields the whole path for the other, so both
/// basename lookups go through here.
pub fn imp_member_basename(name: &str) -> &str {
    name.rsplit(['\\', '/']).next().unwrap_or(name)
}

fn describe_disagreements(found: &[ImpDisagreement]) -> String {
    let parts: Vec<String> = found
        .iter()
        .map(|item| {
            format!(
                "IMP {} mismatch: binary={}, header={}",
                item.statistic, item.binary, item.header
            )
        })
        .collect();
    parts.join("; ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpError(String);

impl ImpError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ImpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ImpError {}

impl ImpSprite {
    pub fn parse(source: &[u8]) -> Result<Self, ImpError> {
        if source.len() < FILE_HEADER_SIZE {
            return Err(ImpError::new("IMP file header is truncated"));
        }

        let file_flags = source[0];
        let record_variant = source[2];
        // Header byte 3 is the transparency index. It is 0 for most unit art but is
        // frequently nonzero for aura and effect sprites, where palette slot 0 is not
        // used by the pixel data at all. Keying transparency on a hardcoded 0 renders
        // those sprites with an opaque background.
        let color_key = source[3];
        let compressed = file_flags & FILE_FLAG_RLE != 0;
        let bits_per_pixel = match file_flags & FILE_FLAG_DEPTH {
            0x00 => 8,
            0x10 => 1,
            0x20 => 2,
            0x30 => 4,
            _ => unreachable!("depth mask covers every possible value"),
        };
        let maximum_width = read_u16(source, 4)?;
        let maximum_height = read_u16(source, 6)?;
        let palette_offset = read_u32(source, 8)? as usize;
        let sequence_count = usize::from(read_u16(source, 26)?);
        let sequence_table_offset = read_u32(source, 28)? as usize;
        if maximum_width == 0 || maximum_height == 0 {
            return Err(ImpError::new("IMP maximum dimensions must be nonzero"));
        }
        if sequence_count == 0 {
            return Err(ImpError::new("IMP has no animation sequences"));
        }
        require_range(
            source,
            sequence_table_offset,
            sequence_count,
            SEQUENCE_RECORD_SIZE,
            "sequence table",
        )?;
        require_range(source, palette_offset, 1, PALETTE_BYTES, "palette")?;

        // Palette entries are stored **blue, red, green, pad** -- not BGRA, and not RGBA.
        // Measured in the running engine on 2026-09-17: a frame was placed twice, once with five
        // entries rewritten as raw bytes, and the rendered pixels were paired with the file bytes
        // for every index in the frame. `(p1, p2, p0)` fits 14 of 14 sampled indices; the next
        // best permutation fits 4. Writing raw `ff 00 00` renders blue, `00 ff 00` renders red and
        // `00 00 ff` renders green, which confirms it independently.
        //
        // The previous reversal swapped red and green, which is why decoded entries and rendered
        // pixels agreed wherever red equalled green and disagreed where they differed -- a symptom
        // this repository recorded for weeks without the cause. It also refutes the community
        // specification's "stored BGRA, swapped to RGB" claim.
        let palette: Vec<[u8; 4]> = source[palette_offset..palette_offset + PALETTE_BYTES]
            .chunks_exact(4)
            .map(|brg| [brg[1], brg[2], brg[0], 255])
            .collect();
        let mut facing_count = 0_usize;
        let mut frame_count = 0_usize;
        let mut hotspot_count = 0_usize;
        let mut hotspot_bytes = 0_u64;
        let mut duplicate_frame_count = 0_usize;
        let mut back_reference_frame_count = 0_usize;
        let mut raw_pixel_bytes = 0_u64;
        let mut stored_pixel_bytes = 0_u64;
        let mut sequences = Vec::with_capacity(sequence_count);
        let mut facings = Vec::new();
        let mut frames = Vec::new();
        let mut pixel_sources = BTreeMap::<usize, usize>::new();

        for sequence_index in 0..sequence_count {
            let sequence_offset = sequence_table_offset + sequence_index * SEQUENCE_RECORD_SIZE;
            let sequence_metadata = source[sequence_offset..sequence_offset + 11]
                .try_into()
                .expect("sequence metadata range was checked");
            let sequence_facings = usize::from(source[sequence_offset + 11]);
            let facing_table_offset = read_u32(source, sequence_offset + 12)? as usize;
            if sequence_facings == 0 {
                return Err(ImpError::new(format!(
                    "IMP sequence {sequence_index} has no facings"
                )));
            }
            require_range(
                source,
                facing_table_offset,
                sequence_facings,
                FACING_RECORD_SIZE,
                "facing table",
            )?;
            facing_count = facing_count
                .checked_add(sequence_facings)
                .ok_or_else(|| ImpError::new("IMP facing count overflow"))?;
            let sequence_first_facing = facings.len();
            let sequence_first_frame = frames.len();

            for facing_index in 0..sequence_facings {
                let facing_offset = facing_table_offset + facing_index * FACING_RECORD_SIZE;
                let facing_metadata = read_u16(source, facing_offset)?;
                let facing_frames = usize::from(read_u16(source, facing_offset + 2)?);
                let frame_table_offset = read_u32(source, facing_offset + 4)? as usize;
                require_range(
                    source,
                    frame_table_offset,
                    1,
                    FRAME_RECORD_SIZE,
                    "frame table",
                )?;
                require_range(
                    source,
                    frame_table_offset,
                    facing_frames,
                    FRAME_RECORD_SIZE,
                    "frame table",
                )?;
                let facing_first_frame = frames.len();
                for frame_index in 0..facing_frames {
                    let frame_offset = frame_table_offset + frame_index * FRAME_RECORD_SIZE;
                    let frame_hotspots = usize::from(source[frame_offset + 1]);
                    let frame_flags = source[frame_offset];
                    let width = read_u16(source, frame_offset + 2)?;
                    let height = read_u16(source, frame_offset + 4)?;
                    let encoded_size = usize::from(read_u16(source, frame_offset + 6)?);
                    let auxiliary = read_u32(source, frame_offset + 8)? as usize;
                    let pixels_offset = read_u32(source, frame_offset + 12)? as usize;
                    let empty_frame = width == 0 && height == 0;
                    if !empty_frame && (width == 0 || height == 0) {
                        return Err(ImpError::new(format!(
                            "IMP frame {frame_index} in facing {facing_index} has partial zero dimensions"
                        )));
                    }
                    if width > maximum_width || height > maximum_height {
                        return Err(ImpError::new(format!(
                            "IMP frame {frame_index} in facing {facing_index} exceeds maximum dimensions"
                        )));
                    }
                    let hotspots = if frame_hotspots > 0 {
                        let frame_hotspot_bytes = hotspot_bytes_for(frame_hotspots)?;
                        require_range(source, auxiliary, 1, frame_hotspot_bytes, "frame hotspots")?;
                        hotspot_bytes = hotspot_bytes
                            .checked_add(frame_hotspot_bytes as u64)
                            .ok_or_else(|| ImpError::new("IMP hotspot byte count overflow"))?;
                        parse_hotspots(source, auxiliary, frame_hotspots)?
                    } else {
                        Vec::new()
                    };
                    hotspot_count = hotspot_count
                        .checked_add(frame_hotspots)
                        .ok_or_else(|| ImpError::new("IMP hotspot count overflow"))?;
                    let shared_pixels = frame_flags & FRAME_FLAG_SHARED_PIXELS != 0;
                    let duplicate_frame = shared_pixels || frame_flags & FRAME_FLAG_DUPLICATE != 0;
                    if duplicate_frame {
                        let source_frame = if shared_pixels {
                            pixel_sources.get(&pixels_offset).copied().ok_or_else(|| {
                                ImpError::new(format!(
                                    "IMP shared-pixel frame references unknown pixel offset {pixels_offset}"
                                ))
                            })?
                        } else {
                            if pixels_offset >= frames.len() {
                                return Err(ImpError::new(format!(
                                    "IMP duplicate frame reference {pixels_offset} is out of range"
                                )));
                            }
                            pixels_offset
                        };
                        duplicate_frame_count = duplicate_frame_count
                            .checked_add(1)
                            .ok_or_else(|| ImpError::new("IMP duplicate frame count overflow"))?;
                        if !shared_pixels {
                            back_reference_frame_count = back_reference_frame_count
                                .checked_add(1)
                                .ok_or_else(|| {
                                    ImpError::new("IMP back reference frame count overflow")
                                })?;
                        }
                        frames.push(ImpFrame {
                            flags: frame_flags,
                            width: 0,
                            height: 0,
                            origin_x: None,
                            origin_y: None,
                            hotspots,
                            palette_indices: Vec::new(),
                            rgba: Vec::new(),
                            source_frame: Some(source_frame),
                            record_offset: frame_offset,
                            hotspot_offset: (frame_hotspots > 0).then_some(auxiliary),
                            packed_size: None,
                            pixels_offset: None,
                            stored_size: None,
                        });
                        continue;
                    }
                    let pixel_count = usize::from(width)
                        .checked_mul(usize::from(height))
                        .ok_or_else(|| ImpError::new("IMP frame pixel count overflow"))?;
                    let (packed_pixels, consumed) = if empty_frame {
                        (Vec::new(), 0)
                    } else {
                        read_frame_pixels(
                            source,
                            pixels_offset,
                            width,
                            height,
                            bits_per_pixel,
                            compressed,
                            record_variant,
                            encoded_size,
                        )?
                    };
                    let palette_indices =
                        unpack_pixels(&packed_pixels, width, height, bits_per_pixel)?;
                    let rgba = palette_indices
                        .iter()
                        .flat_map(|index| palette[usize::from(*index)])
                        .collect();
                    let logical_index = frames.len();
                    if !empty_frame {
                        pixel_sources.entry(pixels_offset).or_insert(logical_index);
                    }
                    raw_pixel_bytes = raw_pixel_bytes
                        .checked_add(pixel_count as u64)
                        .ok_or_else(|| ImpError::new("IMP raw pixel size overflow"))?;
                    stored_pixel_bytes = stored_pixel_bytes
                        .checked_add(consumed as u64)
                        .ok_or_else(|| ImpError::new("IMP stored pixel size overflow"))?;
                    frames.push(ImpFrame {
                        flags: frame_flags,
                        width,
                        height,
                        origin_x: (frame_hotspots == 0)
                            .then(|| read_i16(source, frame_offset + 8))
                            .transpose()?,
                        origin_y: (frame_hotspots == 0)
                            .then(|| read_i16(source, frame_offset + 10))
                            .transpose()?,
                        hotspots,
                        palette_indices,
                        rgba,
                        source_frame: None,
                        record_offset: frame_offset,
                        hotspot_offset: (frame_hotspots > 0).then_some(auxiliary),
                        packed_size: Some(packed_pixels.len()),
                        pixels_offset: Some(pixels_offset),
                        stored_size: Some(consumed),
                    });
                }
                frame_count = frame_count
                    .checked_add(facing_frames)
                    .ok_or_else(|| ImpError::new("IMP frame count overflow"))?;
                facings.push(ImpFacing {
                    metadata: facing_metadata,
                    first_frame: facing_first_frame,
                    frame_count: facing_frames,
                });
            }
            sequences.push(ImpSequence {
                metadata: sequence_metadata,
                first_facing: sequence_first_facing,
                facing_count: sequence_facings,
                first_frame: sequence_first_frame,
                frame_count: frames.len() - sequence_first_frame,
            });
        }

        Ok(Self {
            file_flags,
            record_variant,
            compressed,
            bits_per_pixel,
            maximum_width,
            maximum_height,
            color_key,
            sequence_count,
            facing_count,
            frame_count,
            duplicate_frame_count,
            back_reference_frame_count,
            hotspot_count,
            hotspot_bytes,
            raw_pixel_bytes,
            stored_pixel_bytes,
            palette,
            sequences,
            facings,
            frames,
        })
    }

    /// Every statistic on which this file and its generated header disagree.
    ///
    /// Reports all of them. An earlier version short-circuited on the first `check_equal`, so a
    /// file that disagreed on four statistics reported one, and the missing three were exactly
    /// the evidence needed to tell a stale header from a decoder bug.
    pub fn disagreements(&self, stats: &ImpHeaderStats) -> Vec<ImpDisagreement> {
        let mut found = Vec::new();
        let mut compare = |statistic, binary: u64, header: u64| {
            if binary != header {
                found.push(ImpDisagreement {
                    statistic,
                    binary,
                    header,
                });
            }
        };
        compare(
            ImpStatistic::SequenceCount,
            self.sequence_count as u64,
            stats.sequence_count as u64,
        );
        compare(
            ImpStatistic::FrameCount,
            self.frame_count as u64,
            stats.frame_count as u64,
        );
        compare(
            ImpStatistic::DuplicateFrameCount,
            self.duplicate_frame_count as u64,
            stats.duplicate_frame_count as u64,
        );
        compare(
            ImpStatistic::RawPixelBytes,
            self.raw_pixel_bytes,
            stats.raw_pixel_bytes,
        );
        compare(
            ImpStatistic::HotspotBytes,
            self.hotspot_bytes,
            stats.hotspot_bytes,
        );
        if let Some(header) = stats.compressed_pixel_bytes {
            compare(
                ImpStatistic::StoredPixelBytes,
                self.stored_pixel_bytes,
                header,
            );
        }
        found
    }

    pub fn validate_against(&self, stats: &ImpHeaderStats) -> Result<(), ImpError> {
        let found = self.disagreements(stats);
        if found.is_empty() {
            return Ok(());
        }
        Err(ImpError::new(describe_disagreements(&found)))
    }

    pub fn resolved_frame(&self, index: usize) -> Result<&ImpFrame, ImpError> {
        let mut current = index;
        for _ in 0..=self.frames.len() {
            let frame = self.frames.get(current).ok_or_else(|| {
                ImpError::new(format!("IMP frame index {current} is out of range"))
            })?;
            match frame.source_frame {
                Some(source) => current = source,
                None => return Ok(frame),
            }
        }
        Err(ImpError::new(
            "IMP duplicate-frame references contain a facing",
        ))
    }

    /// Every frame index whose hotspot records live in the same array as `frame_index`.
    ///
    /// Distinct frame records may store the same array pointer, in which case editing a hotspot
    /// through one frame edits the others too — a different sharing relation from
    /// [`ImpSprite::frames_sharing_record`], and the one that matters for hotspot writes.
    ///
    /// Returns an empty vector when the frame carries an origin pair instead.
    pub fn frames_sharing_hotspots(&self, frame_index: usize) -> Result<Vec<usize>, ImpError> {
        let frame = self.frames.get(frame_index).ok_or_else(|| {
            ImpError::new(format!("IMP frame index {frame_index} is out of range"))
        })?;
        let Some(offset) = frame.hotspot_offset else {
            return Ok(Vec::new());
        };
        Ok(self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, other)| other.hotspot_offset == Some(offset))
            .map(|(index, _)| index)
            .collect())
    }

    /// Every frame index backed by the same 16-byte record as `frame_index`, itself included.
    ///
    /// Records may alias, so a write through any of these indices is a write through all of them.
    /// Callers that edit placement should report this rather than surprise the user.
    pub fn frames_sharing_record(&self, frame_index: usize) -> Result<Vec<usize>, ImpError> {
        let frame = self.frames.get(frame_index).ok_or_else(|| {
            ImpError::new(format!("IMP frame index {frame_index} is out of range"))
        })?;
        let offset = frame.record_offset;
        Ok(self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, other)| other.record_offset == offset)
            .map(|(index, _)| index)
            .collect())
    }

    /// Every *other* frame index whose pixels come out of `frame_index`'s stored payload.
    ///
    /// Three distinct ways a frame can be reached through another one's bytes, and a pixel
    /// replacement changes all three whether or not the caller meant to:
    ///
    /// - a `0x04` shared-pixel record whose pixel pointer is this frame's payload offset;
    /// - a `0x08` duplicate record whose chain of frame indices resolves here;
    /// - a second frame slot backed by the very same 16-byte record — see
    ///   [`ImpSprite::frames_sharing_record`].
    ///
    /// Returns them sorted, with `frame_index` itself excluded. 10,293 of the 51,666 frame records
    /// in `imp.mpq` are one of the first two classes, so this is the common case, not the exotic
    /// one. [`write_frame_pixels`] refuses rather than editing a picture the caller did not name.
    pub fn frames_sharing_pixels(&self, frame_index: usize) -> Result<Vec<usize>, ImpError> {
        let frame = self.frames.get(frame_index).ok_or_else(|| {
            ImpError::new(format!("IMP frame index {frame_index} is out of range"))
        })?;
        // Compare *resolved* payload offsets rather than chasing the duplicate chain to this index.
        // Two ordinary records may both point at one payload, in which case a `0x04` record
        // resolves to whichever of them the parser saw first — not necessarily the one the caller
        // named — and a chain search keyed on `frame_index` would miss it.
        let target_payload = frame.pixels_offset;
        let mut sharing = Vec::new();
        for (index, other) in self.frames.iter().enumerate() {
            if index == frame_index {
                continue;
            }
            let effective = self.resolved_frame(index)?.pixels_offset;
            if (target_payload.is_some() && effective == target_payload)
                || other.record_offset == frame.record_offset
            {
                sharing.push(index);
            }
        }
        Ok(sharing)
    }

    pub fn frame_location(&self, frame_index: usize) -> Result<(usize, usize, usize), ImpError> {
        if frame_index >= self.frames.len() {
            return Err(ImpError::new(format!(
                "IMP frame index {frame_index} is out of range"
            )));
        }
        for (sequence_index, sequence) in self.sequences.iter().enumerate() {
            if !(sequence.first_frame..sequence.first_frame + sequence.frame_count)
                .contains(&frame_index)
            {
                continue;
            }
            for facing_index in sequence.first_facing..sequence.first_facing + sequence.facing_count {
                let facing = &self.facings[facing_index];
                if (facing.first_frame..facing.first_frame + facing.frame_count).contains(&frame_index)
                {
                    return Ok((sequence_index, facing_index, frame_index - facing.first_frame));
                }
            }
        }
        Err(ImpError::new(format!(
            "IMP frame index {frame_index} is not owned by a facing"
        )))
    }
}

/// Screen-space top-left at which the engine draws a frame placed at `anchor`.
///
/// Measured in the running engine on 2026-09-16 against four terrain sprites with known,
/// differing placement values, sharing one map cell so the anchor cancels:
///
/// ```text
/// top_left = anchor + placement - (width >> 1, height >> 1)
/// ```
///
/// So the stored pair is the vector from the anchor to the **centre** of the frame, in screen
/// pixels with `+y` downward, and it is **added**. The convention is centre-relative, which is
/// why re-cropping art shifts the value it needs by half the crop.
///
/// **Scope.** This is established for frames carrying the *origin* pair. Frames carrying hotspot
/// records instead have no origin field, and which hotspot type the engine treats as the draw
/// anchor is not yet established — see the research log.
pub fn frame_top_left(
    anchor: (i32, i32),
    placement: (i16, i16),
    width: u16,
    height: u16,
) -> Result<(i32, i32), ImpError> {
    Ok((
        screen_axis(anchor.0, i32::from(placement.0), i32::from(width >> 1), "x")?,
        screen_axis(anchor.1, i32::from(placement.1), i32::from(height >> 1), "y")?,
    ))
}

/// `anchor + placement - half`, refusing to wrap.
///
/// Anchors reach this from the command line unvalidated, so an unchecked `i32` add here panics in a
/// debug build and silently wraps in the release build the README tells users to make.
fn screen_axis(anchor: i32, placement: i32, half: i32, axis: &str) -> Result<i32, ImpError> {
    anchor
        .checked_add(placement)
        .and_then(|sum| sum.checked_sub(half))
        .ok_or_else(|| ImpError::new(format!("IMP screen {axis} overflows a 32-bit integer")))
}

/// The placement pair that makes a frame of this size land at `top_left` when drawn at `anchor`.
///
/// Exact inverse of [`frame_top_left`]. This is the value a re-cropping tool needs to write back:
/// keep the art's existing screen position, solve for the pair.
pub fn placement_for_top_left(
    anchor: (i32, i32),
    top_left: (i32, i32),
    width: u16,
    height: u16,
) -> Result<(i16, i16), ImpError> {
    let axis = |top_left: i32, anchor: i32, half: i32, name: &str| {
        top_left
            .checked_sub(anchor)
            .and_then(|delta| delta.checked_add(half))
            .ok_or_else(|| ImpError::new(format!("IMP placement {name} overflows a 32-bit integer")))
    };
    let x = axis(top_left.0, anchor.0, i32::from(width >> 1), "x")?;
    let y = axis(top_left.1, anchor.1, i32::from(height >> 1), "y")?;
    let narrow = |value: i32, axis: &str| {
        i16::try_from(value)
            .map_err(|_| ImpError::new(format!("IMP placement {axis} {value} does not fit in i16")))
    };
    Ok((narrow(x, "x")?, narrow(y, "y")?))
}

/// Rewrite one frame's origin pair, returning new file bytes of identical length.
///
/// Only the four bytes of that frame record's origin dword change, so every offset stored
/// elsewhere in the file stays valid. Refuses a frame that carries hotspot records, because the
/// origin pair and the hotspot array pointer are the same four bytes.
pub fn write_frame_origin(
    source: &[u8],
    frame_index: usize,
    x: i16,
    y: i16,
) -> Result<Vec<u8>, ImpError> {
    let sprite = ImpSprite::parse(source)?;
    let frame = sprite.frames.get(frame_index).ok_or_else(|| {
        ImpError::new(format!("IMP frame index {frame_index} is out of range"))
    })?;
    if frame.hotspot_offset.is_some() {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} carries {} hotspot records, so its record has no origin pair; edit a hotspot record instead",
            frame.hotspots.len()
        )));
    }
    // A duplicate or shared-pixel frame has no origin of its own: the parser reports `None` for it,
    // and what the dword means for that record class is not established. Refuse by name rather than
    // patching four bytes and letting the caller puzzle over reading back `None`.
    if let Some(source_frame) = frame.source_frame {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} is a duplicate of frame {source_frame} and carries no origin pair of its own; edit frame {source_frame}"
        )));
    }
    let offset = frame.record_offset + 8;
    let mut output = source.to_vec();
    require_range(&output, offset, 1, 4, "frame origin")?;
    output[offset..offset + 2].copy_from_slice(&x.to_le_bytes());
    output[offset + 2..offset + 4].copy_from_slice(&y.to_le_bytes());
    Ok(output)
}

/// Rewrite the offsets of one hotspot record, selected by its type id.
///
/// Returns new file bytes of identical length; only that record's four offset bytes change. The
/// type id itself is left alone. Refuses when the frame has no record of that type, and when more
/// than one record shares it, rather than guessing which was meant.
pub fn write_frame_hotspot(
    source: &[u8],
    frame_index: usize,
    id: u16,
    x: i16,
    y: i16,
) -> Result<Vec<u8>, ImpError> {
    let sprite = ImpSprite::parse(source)?;
    let frame = sprite.frames.get(frame_index).ok_or_else(|| {
        ImpError::new(format!("IMP frame index {frame_index} is out of range"))
    })?;
    let Some(base) = frame.hotspot_offset else {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} carries an origin pair rather than hotspot records"
        )));
    };
    let matches: Vec<usize> = frame
        .hotspots
        .iter()
        .enumerate()
        .filter(|(_, spot)| spot.id == id)
        .map(|(index, _)| index)
        .collect();
    let slot = match matches.as_slice() {
        [only] => *only,
        [] => {
            let present: Vec<u16> = frame.hotspots.iter().map(|spot| spot.id).collect();
            return Err(ImpError::new(format!(
                "IMP frame {frame_index} has no hotspot of type {id}; it carries {present:?}"
            )));
        }
        many => {
            return Err(ImpError::new(format!(
                "IMP frame {frame_index} carries {} hotspots of type {id}; refusing to guess",
                many.len()
            )))
        }
    };
    let offset = base + slot * HOTSPOT_RECORD_SIZE + 2;
    let mut output = source.to_vec();
    require_range(&output, offset, 1, 4, "hotspot offsets")?;
    output[offset..offset + 2].copy_from_slice(&x.to_le_bytes());
    output[offset + 2..offset + 4].copy_from_slice(&y.to_le_bytes());
    Ok(output)
}

/// The archive's payload-start alignment, preserved across a length-changing rewrite.
///
/// Every stored pixel payload in `imp.mpq` begins on an 8-byte boundary — measured by the
/// `--imp-roundtrip` sweep, which counts payload starts and reports the aligned share. It shows up
/// in the gap histogram too: the gaps between consecutive payloads are 0 through 7 and roughly
/// uniform, which is the signature of rounding each start up to a multiple of eight.
///
/// **Nothing has shown that the engine requires it.** The pointers are absolute and nothing in the
/// decoder reads a payload as a machine word. It is preserved anyway because an invariant the whole
/// shipped corpus holds is the wrong thing for a modding tool to be the first to break, and the
/// price is at most seven zero bytes per edit. See [`write_frame_pixels`].
const PAYLOAD_ALIGNMENT: usize = 8;

/// What a pixel replacement did, beyond the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpPixelWrite {
    pub bytes: Vec<u8>,
    /// Stored payload length before and after, excluding the alignment padding.
    pub stored_size: (usize, usize),
    /// Zero bytes appended after the new payload to keep every later payload on its 8-byte start.
    pub alignment_padding: usize,
    /// How far every structure after the replaced payload moved. Always a multiple of
    /// [`PAYLOAD_ALIGNMENT`], and zero when the new payload is the same length as the old.
    pub shift: isize,
    /// How many absolute file offsets stored elsewhere in the file had to be rewritten.
    pub rewritten_offsets: usize,
    /// The layout the replaced frame's pixels were packed in, which is the frame's own observed
    /// layout rather than a choice this encoder made.
    pub layout: PixelLayout,
    /// True when the frame's shape is one where a stored size names no layout at all, so the
    /// pixels handed in may already have been decoded under the wrong one. See
    /// [`layout_is_ambiguous`].
    pub layout_ambiguous: bool,
    /// True when the frame's stored size *does* name a layout but this decoder's reader stops
    /// before it could have seen the row-padded one, so `layout` is what was read rather than what
    /// was necessarily stored. See [`layout_is_unobservable`].
    ///
    /// Reported beside `layout_ambiguous` and never folded into it. The consequence for a caller is
    /// the same -- the exported pixels may have been decoded under the wrong layout, and splicing a
    /// tight payload back over a row-padded one strands the original tail after the alignment
    /// padding -- but this class is sixteen times larger, and a warning that names only the smaller
    /// one would leave the common case silent while reporting `layout: Tight` as fact.
    pub layout_unobservable: bool,
}

/// The span a pixel replacement would overwrite, or the refusal that stops it before anything else
/// is read.
///
/// Split out of [`write_frame_pixels`] so a caller holding a parsed sprite can ask **whether this
/// frame is writable at all** before it commits to work the refusal would throw away. The import
/// path needs exactly that: it used to take the frame's dimensions straight from the record and
/// hand them to the PNG reader, and for the 10,293 records that carry no payload of their own those
/// dimensions are `0x0`, so the user met "PNG is 24x1 but the template is 0x0" instead of being told
/// which frame actually stores the pixels. The refusal text lives here, once, rather than being
/// paraphrased at each caller.
///
/// The refusals themselves, and why each exists, are documented on [`write_frame_pixels`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePixelTarget {
    pub packed_size: usize,
    pub pixels_offset: usize,
    pub stored_size: usize,
}

pub fn frame_pixel_target(
    sprite: &ImpSprite,
    frame_index: usize,
) -> Result<FramePixelTarget, ImpError> {
    let frame = sprite
        .frames
        .get(frame_index)
        .ok_or_else(|| ImpError::new(format!("IMP frame index {frame_index} is out of range")))?;
    if let Some(source_frame) = frame.source_frame {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} is a duplicate of frame {source_frame} and stores no pixels of its own; replace frame {source_frame}"
        )));
    }
    let (Some(packed_size), Some(pixels_offset), Some(stored_size)) =
        (frame.packed_size, frame.pixels_offset, frame.stored_size)
    else {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} carries no pixel payload of its own"
        )));
    };
    if frame.width == 0 || frame.height == 0 {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} is empty; there are no pixels to replace"
        )));
    }
    if stored_size == 0 {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} stores a zero-length payload, so its pixel pointer addresses no span to replace"
        )));
    }
    let sharing = sprite.frames_sharing_pixels(frame_index)?;
    if !sharing.is_empty() {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index} shares its pixels with frame(s) {sharing:?}; replacing it would repaint them too, so this refuses rather than changing art that was not named"
        )));
    }
    Ok(FramePixelTarget {
        packed_size,
        pixels_offset,
        stored_size,
    })
}

/// Replace one frame's pixels, rewriting every file offset the new payload's length disturbs.
///
/// Everything that is not the replaced payload, the offsets that point past it, and the one
/// `encoded_size` field that declares its length is copied **byte for byte**. That is deliberate
/// beyond tidiness: bytes 5–10 of a sequence record hold uninitialised leftover text from the
/// authoring tool (`frames`, `\imps\`), byte 2 has a distribution nobody has explained, and bytes 3
/// and 4 are constant but unread. Re-emitting a "clean" record would destroy evidence and could
/// break a reader nobody has audited. Nothing here re-synthesises a record.
///
/// # What is rewritten
///
/// The payload at the frame's pixel pointer is replaced. If the new payload is a different length,
/// everything after it moves, so every absolute file offset **this repo has identified** is
/// adjusted -- the set [`absolute_offset_fields`] enumerates: the palette pointer at file offset 8,
/// the sequence-table pointer at 28, each sequence's facing-table pointer, each facing's
/// frame-table pointer, and each frame record's hotspot-array and pixel pointers. A `0x08`
/// duplicate record's pixel dword is a frame *index*, not an offset, and is left alone.
///
/// That the enumeration is **complete** is not measured. Header bytes 12-25 are read by nothing in
/// this repo and `docs/imp-format.md` documents the records field by field but never the 32-byte
/// file header, so a pointer hiding there would be adjusted by nobody. A header of that size
/// usually holds counts and maxima rather than pointers, and the `--imp-roundtrip --rewrite` sweep
/// re-parses every written file, which would catch a stale pointer that any reader in this repo
/// follows -- but not one only the engine follows.
///
/// Two things would settle it, neither run here: `imp_anim::field_reads` recovers every
/// displacement the engine reads off a struct pointer, which would say outright whether 12-25 are
/// read at all; and a corpus pass could report whether those bytes ever hold a value that lands
/// inside the file, which is what a pointer would have to do.
///
/// The move is rounded up to [`PAYLOAD_ALIGNMENT`] so later payloads keep the 8-byte starts the
/// whole shipped archive gives them.
///
/// # What it refuses
///
/// - a frame with no payload of its own — a duplicate or shared-pixel record, or an empty frame;
/// - a frame whose payload some **other** frame also reads, by any of the three routes
///   [`ImpSprite::frames_sharing_pixels`] knows. A replacement there would silently repaint a
///   picture the caller never named, and there are 10,293 such records in `imp.mpq`;
/// - a frame whose stored payload is zero bytes long, where the pixel pointer addresses nothing and
///   there is no span to replace;
/// - a payload that will not read back through [`read_frame_pixels`] as exactly what was written,
///   which is what catches a `record_variant == 0` stream that halts early;
/// - a new payload too long for the `encoded_size` field, on the variants that have one.
pub fn write_frame_pixels(
    source: &[u8],
    frame_index: usize,
    indices: &[u8],
) -> Result<ImpPixelWrite, ImpError> {
    let sprite = ImpSprite::parse(source)?;
    let FramePixelTarget {
        packed_size,
        pixels_offset,
        stored_size,
    } = frame_pixel_target(&sprite, frame_index)?;
    let frame = &sprite.frames[frame_index];

    let packed = pack_pixels(
        indices,
        frame.width,
        frame.height,
        sprite.bits_per_pixel,
        packed_size,
    )?;
    let payload = if sprite.compressed {
        encode_rle(&packed)
    } else {
        packed.clone()
    };

    // Read our own payload back through the parser's own pixel path, standing where the file's
    // pixel pointer will stand. A second decoder here could share this encoder's mistakes.
    let (reread, consumed) = read_frame_pixels(
        &payload,
        0,
        frame.width,
        frame.height,
        sprite.bits_per_pixel,
        sprite.compressed,
        sprite.record_variant,
        payload.len(),
    )?;
    // **These two clauses are equivalent on every reachable input, and both are kept anyway.**
    // Worth stating, because a mutation sweep deletes either one and stays green, and an
    // unexplained surviving mutant is indistinguishable from an untested line.
    //
    // The argument. `reread` always has one of the frame's `packed_sizes` as its length. A
    // `record_variant != 0` record reads exactly `payload.len()` bytes by construction, so the
    // first clause is dead there. A `record_variant == 0` record stops at the first acceptable
    // length the stream reaches; the payload was built to encode exactly `packed`, so it reaches
    // `packed.len()` precisely at its last packet. Stopping earlier therefore means stopping at a
    // *different* length, which the second clause already sees.
    //
    // They are kept apart because they fail for different reasons -- "you did not use all my
    // bytes" and "you did not give back my pixels" -- and the equivalence rests on facts about
    // `packed_sizes` and the RLE reader that a future change could break. The redundancy is the
    // cheap half of a writer that must not put a wrong frame in a file.
    if consumed != payload.len() || reread != packed {
        return Err(ImpError::new(format!(
            "IMP frame {frame_index}: the re-encoded payload is {} bytes but reads back as {} bytes of packed pixels after {consumed}; refusing to write it",
            payload.len(),
            reread.len(),
        )));
    }

    // Keep every later payload on the 8-byte start the shipped archive gives it.
    let padding = ((stored_size % PAYLOAD_ALIGNMENT) + PAYLOAD_ALIGNMENT
        - (payload.len() % PAYLOAD_ALIGNMENT))
        % PAYLOAD_ALIGNMENT;
    let new_span = payload
        .len()
        .checked_add(padding)
        .ok_or_else(|| ImpError::new("IMP replacement payload length overflow"))?;
    let shift = isize::try_from(new_span)
        .and_then(|new| isize::try_from(stored_size).map(|old| new - old))
        .map_err(|_| ImpError::new("IMP replacement payload length overflow"))?;

    let splice_end = pixels_offset.checked_add(stored_size).ok_or_else(|| {
        ImpError::new("IMP frame payload extends past the end of the address space")
    })?;
    require_range(source, pixels_offset, 1, stored_size, "frame pixels")?;

    let mut output = source.to_vec();
    let own_pointer = frame.record_offset + 12;
    if sprite.record_variant != 0 {
        let declared = u16::try_from(payload.len()).map_err(|_| {
            ImpError::new(format!(
                "IMP frame {frame_index}: the re-encoded payload is {} bytes, which does not fit the record's 16-bit encoded size",
                payload.len()
            ))
        })?;
        output[frame.record_offset + 6..frame.record_offset + 8]
            .copy_from_slice(&declared.to_le_bytes());
    }
    let rewritten_offsets =
        shift_absolute_offsets(&mut output, own_pointer, pixels_offset, splice_end, shift)?;
    let mut replacement = payload.clone();
    replacement.resize(new_span, 0);
    output.splice(pixels_offset..splice_end, replacement);

    let rewritten = ImpSprite::parse(&output)?;
    check_only_the_named_frame_changed(&sprite, &rewritten, frame_index, indices)?;

    Ok(ImpPixelWrite {
        bytes: output,
        stored_size: (stored_size, payload.len()),
        alignment_padding: padding,
        shift,
        rewritten_offsets,
        layout: pixel_layout_for(
            packed_size,
            frame.width,
            frame.height,
            sprite.bits_per_pixel,
        )?,
        layout_ambiguous: layout_is_ambiguous(frame.width, frame.height, sprite.bits_per_pixel)?,
        layout_unobservable: layout_is_unobservable(
            packed_size,
            PackedSizes::for_frame(frame.width, frame.height, sprite.bits_per_pixel)?,
            sprite.record_variant,
            sprite.compressed,
        ),
    })
}

/// Add `shift` to every absolute file offset the format stores that points past `splice_end`.
///
/// Returns how many were changed. `own_pointer` is the replaced frame's own pixel pointer, which
/// keeps its value because the payload stays where it starts. Every other offset landing inside the
/// replaced span is an error, not something to adjust: nothing else is supposed to address the
/// inside of a frame's pixels, and guessing which end it belonged to would be a fabrication.
///
/// Called **before** the splice, so the field positions are the source file's own.
fn shift_absolute_offsets(
    buffer: &mut [u8],
    own_pointer: usize,
    splice_start: usize,
    splice_end: usize,
    shift: isize,
) -> Result<usize, ImpError> {
    let fields = absolute_offset_fields(buffer)?;
    if !fields.contains(&own_pointer) {
        return Err(ImpError::new(
            "IMP frame's own pixel pointer is not among the file's absolute offsets",
        ));
    }
    let mut rewritten = 0;
    for field in fields {
        let value = read_u32(buffer, field)? as usize;
        if field != own_pointer && (splice_start..splice_end).contains(&value) {
            return Err(ImpError::new(format!(
                "IMP offset at {field} addresses {value}, inside the payload being replaced ({splice_start}..{splice_end})"
            )));
        }
        if value < splice_end {
            continue;
        }
        let moved = isize::try_from(value)
            .ok()
            .and_then(|value| value.checked_add(shift))
            .and_then(|moved| u32::try_from(moved).ok())
            .ok_or_else(|| {
                ImpError::new(format!(
                    "IMP offset at {field} does not fit after the rewrite"
                ))
            })?;
        buffer[field..field + 4].copy_from_slice(&moved.to_le_bytes());
        rewritten += 1;
    }
    Ok(rewritten)
}

/// Every position in the file holding a u32 that is an **absolute file offset**.
///
/// Deduplicated, because two facings may share one frame table and two records may share one
/// hotspot array; adding a shift twice to one field would move it into nothing.
///
/// A `0x08` duplicate record's pixel dword is a frame index rather than an offset and is excluded.
/// A `0x04` shared-pixel record's is a genuine pointer into another frame's payload and is not.
fn absolute_offset_fields(source: &[u8]) -> Result<Vec<usize>, ImpError> {
    // The palette pointer and the sequence-table pointer, at file offsets 8 and 28.
    let mut fields = vec![8_usize, 28];
    let sequence_count = usize::from(read_u16(source, 26)?);
    let sequence_table = read_u32(source, 28)? as usize;
    for sequence_index in 0..sequence_count {
        let sequence = sequence_table + sequence_index * SEQUENCE_RECORD_SIZE;
        require_range(source, sequence, 1, SEQUENCE_RECORD_SIZE, "sequence table")?;
        fields.push(sequence + 12);
        let facing_count = usize::from(source[sequence + 11]);
        let facing_table = read_u32(source, sequence + 12)? as usize;
        for facing_index in 0..facing_count {
            let facing = facing_table + facing_index * FACING_RECORD_SIZE;
            require_range(source, facing, 1, FACING_RECORD_SIZE, "facing table")?;
            fields.push(facing + 4);
            let frame_count = usize::from(read_u16(source, facing + 2)?);
            let frame_table = read_u32(source, facing + 4)? as usize;
            for frame_index in 0..frame_count {
                let record = frame_table + frame_index * FRAME_RECORD_SIZE;
                require_range(source, record, 1, FRAME_RECORD_SIZE, "frame table")?;
                if source[record + 1] > 0 {
                    fields.push(record + 8);
                }
                let flags = source[record];
                let back_reference =
                    flags & FRAME_FLAG_DUPLICATE != 0 && flags & FRAME_FLAG_SHARED_PIXELS == 0;
                if !back_reference {
                    fields.push(record + 12);
                }
            }
        }
    }
    fields.sort_unstable();
    fields.dedup();
    Ok(fields)
}

/// Re-read the written file and refuse it unless it decodes to the original sprite with exactly one
/// frame's pixels replaced.
///
/// The offsets, record positions and stored sizes legitimately move, so those are not compared;
/// everything a reader of this sprite can *see* is. An encoder bug must not reach a file, which is
/// the same rule `--set-imp-placement` and the map editors follow.
fn check_only_the_named_frame_changed(
    original: &ImpSprite,
    rewritten: &ImpSprite,
    frame_index: usize,
    indices: &[u8],
) -> Result<(), ImpError> {
    let complain = |what: &str| Err(ImpError::new(format!("IMP rewrite changed {what}")));
    if (
        original.file_flags,
        original.record_variant,
        original.bits_per_pixel,
        original.maximum_width,
        original.maximum_height,
        original.color_key,
    ) != (
        rewritten.file_flags,
        rewritten.record_variant,
        rewritten.bits_per_pixel,
        rewritten.maximum_width,
        rewritten.maximum_height,
        rewritten.color_key,
    ) {
        return complain("the file header");
    }
    if original.palette != rewritten.palette {
        return complain("the palette");
    }
    if original.sequences != rewritten.sequences {
        return complain("the sequence table");
    }
    if original.facings != rewritten.facings {
        return complain("the facing table");
    }
    if original.frames.len() != rewritten.frames.len() {
        return complain("the frame count");
    }
    for (index, (before, after)) in original.frames.iter().zip(&rewritten.frames).enumerate() {
        if (
            before.flags,
            before.width,
            before.height,
            before.origin_x,
            before.origin_y,
            &before.hotspots,
            before.source_frame,
        ) != (
            after.flags,
            after.width,
            after.height,
            after.origin_x,
            after.origin_y,
            &after.hotspots,
            after.source_frame,
        ) {
            return complain(&format!("frame {index}'s record"));
        }
        let expected = if index == frame_index {
            indices
        } else {
            before.palette_indices.as_slice()
        };
        if after.palette_indices != expected {
            return complain(&format!("frame {index}'s pixels"));
        }
    }
    Ok(())
}

fn parse_hotspots(source: &[u8], offset: usize, count: usize) -> Result<Vec<ImpHotspot>, ImpError> {
    let byte_count = count
        .checked_mul(HOTSPOT_RECORD_SIZE)
        .ok_or_else(|| ImpError::new("IMP hotspot byte count overflow"))?;
    require_range(source, offset, 1, byte_count, "frame hotspots")?;
    Ok(source[offset..offset + byte_count]
        .chunks_exact(HOTSPOT_RECORD_SIZE)
        .map(|bytes| {
            let raw: [u8; HOTSPOT_RECORD_SIZE] =
                bytes.try_into().expect("hotspot chunk size was checked");
            ImpHotspot {
                id: u16::from_le_bytes([raw[0], raw[1]]),
                x: i16::from_le_bytes([raw[2], raw[3]]),
                y: i16::from_le_bytes([raw[4], raw[5]]),
                raw,
            }
        })
        .collect())
}

impl ImpHeaderStats {
    pub fn parse(source: &[u8]) -> Result<Self, ImpError> {
        let text = std::str::from_utf8(source)
            .map_err(|_| ImpError::new("IMP generated header is not UTF-8/ASCII"))?;
        let sequence_name = text
            .lines()
            .find_map(|line| line.strip_prefix("// Sprite headers for sequence "))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ImpError::new("IMP generated header has no sequence name"))?
            .to_owned();
        let compressed_pixel_bytes = stat_value(text, "Bitmap RLE memory usage")?;
        let no_compression = text
            .lines()
            .any(|line| line.trim() == "// No compression scheme used");
        if compressed_pixel_bytes.is_none() && !no_compression {
            return Err(ImpError::new(
                "IMP generated header has no compression statistic",
            ));
        }

        let sequence_count = required_stat(text, "Total number of 'Sequences'")? as usize;
        Ok(Self {
            sequence_labels: parse_sequence_labels(text, &sequence_name, sequence_count),
            sequence_name,
            sequence_count,
            frame_count: required_stat(text, "Total number of 'Frames'")? as usize,
            duplicate_frame_count: required_stat(text, "Duplicate bitmaps found")? as usize,
            raw_pixel_bytes: required_stat(text, "Bitmap raw memory usage")?,
            hotspot_bytes: required_stat(text, "Hotspot raw memory usage")?,
            compressed_pixel_bytes,
        })
    }
}

fn parse_sequence_labels(text: &str, root_name: &str, count: usize) -> Vec<Vec<String>> {
    let prefix = format!("{}_", root_name.to_ascii_uppercase());
    let mut labels = vec![Vec::new(); count];
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("#define") {
            continue;
        }
        let Some(symbol) = fields.next() else {
            continue;
        };
        let Some(value) = fields.next().and_then(|value| value.parse::<usize>().ok()) else {
            continue;
        };
        let Some(label) = symbol.strip_prefix(&prefix) else {
            continue;
        };
        if value < labels.len() && !label.is_empty() {
            labels[value].push(label.to_owned());
        }
    }
    labels
}

fn required_stat(text: &str, label: &str) -> Result<u64, ImpError> {
    stat_value(text, label)?
        .ok_or_else(|| ImpError::new(format!("IMP generated header has no {label} statistic")))
}

fn stat_value(text: &str, label: &str) -> Result<Option<u64>, ImpError> {
    let Some(value) = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("//")?
            .trim()
            .strip_prefix(label)?
            .trim()
            .strip_prefix(':')
            .map(str::trim)
    }) else {
        return Ok(None);
    };
    value
        .parse()
        .map(Some)
        .map_err(|_| ImpError::new(format!("IMP {label} statistic is not an integer")))
}

fn hotspot_bytes_for(count: usize) -> Result<usize, ImpError> {
    count
        .checked_mul(HOTSPOT_RECORD_SIZE)
        .and_then(|size| size.checked_add(HOTSPOT_ALIGNMENT - 1))
        .map(|size| size & !(HOTSPOT_ALIGNMENT - 1))
        .ok_or_else(|| ImpError::new("IMP hotspot size overflow"))
}

/// Reads one frame's stored pixel payload and returns the bit-packed pixels plus the number of
/// bytes the payload occupied.
///
/// This is the parser's own pixel path, lifted out so a re-encoder can be checked against the
/// exact code that will read its output rather than against a second implementation that could
/// drift from it. `source` is the whole file during a parse; a caller checking a freshly encoded
/// payload passes that payload with `pixels_offset` 0 and `encoded_size` equal to its length.
///
/// The four arms are not interchangeable. A compressed `record_variant == 0` record does not
/// declare its payload length at all, so the stream is decoded until it reaches *any* size
/// [`packed_sizes`] accepts; every other variant declares one.
///
/// Empty frames (`width == height == 0`) are the caller's business: this refuses them rather than
/// returning an empty payload, because a zero-size read is not a read.
#[allow(clippy::too_many_arguments)]
pub fn read_frame_pixels(
    source: &[u8],
    pixels_offset: usize,
    width: u16,
    height: u16,
    bits_per_pixel: u8,
    compressed: bool,
    record_variant: u8,
    encoded_size: usize,
) -> Result<(Vec<u8>, usize), ImpError> {
    if width == 0 || height == 0 {
        return Err(ImpError::new(
            "IMP frame has no pixels to read; empty frames carry no payload",
        ));
    }
    let pixel_count = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| ImpError::new("IMP frame pixel count overflow"))?;
    let packed_sizes = packed_sizes(width, height, bits_per_pixel)?;
    if compressed {
        if record_variant == 0 {
            let available = source
                .get(pixels_offset..)
                .ok_or_else(|| ImpError::new("IMP frame pixel offset is invalid"))?;
            decode_rle_until_size(available, &packed_sizes)
        } else {
            require_range(source, pixels_offset, 1, encoded_size, "frame pixels")?;
            let available = &source[pixels_offset..pixels_offset + encoded_size];
            Ok((decode_rle_exact(available, &packed_sizes)?, encoded_size))
        }
    } else {
        let packed_size = if record_variant != 0 {
            if !packed_sizes.contains(&encoded_size) {
                return Err(ImpError::new(format!(
                    "IMP raw frame declares unsupported packed size {encoded_size}"
                )));
            }
            encoded_size
        } else {
            *packed_sizes
                .iter()
                .find(|size| **size * 8 >= pixel_count * usize::from(bits_per_pixel))
                .ok_or_else(|| ImpError::new("IMP raw frame has no packed size"))?
        };
        require_range(source, pixels_offset, 1, packed_size, "frame pixels")?;
        Ok((
            source[pixels_offset..pixels_offset + packed_size].to_vec(),
            packed_size,
        ))
    }
}

/// Every byte length a frame of these dimensions is allowed to store its packed pixels in.
///
/// There is more than one because the format does not settle the question. `row_padded` restarts
/// each scanline on a byte boundary; `tight_ceil` runs the bitstream straight through and pads only
/// the very end. The two coincide whenever a row's bits fill whole bytes — always at 8bpp, and at
/// lower depths only for the right widths — and diverge otherwise, which is why an encoder must be
/// told which one the frame it is replacing used rather than choosing. `tight_floor` is admitted at
/// 1bpp only: a frame whose bits do not fill the last byte may simply omit it there, and the parser
/// zero-fills the missing pixels.
pub fn packed_sizes(width: u16, height: u16, bits_per_pixel: u8) -> Result<Vec<usize>, ImpError> {
    Ok(PackedSizes::for_frame(width, height, bits_per_pixel)?.acceptable(bits_per_pixel))
}

/// The candidate packed sizes for one frame, kept apart by name.
///
/// [`packed_sizes`] collapses these into a sorted, deduplicated list, which is what the parser
/// needs but destroys the one thing a *re-encoder* cares about: which layout a given length means.
/// Whenever `tight_ceil == row_padded` — always at 8bpp, and at lower depths whenever a row's bits
/// fill whole bytes — the original frame's choice left no trace at all, and code that has to report
/// the distribution must say so rather than pick a winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedSizes {
    /// One continuous bitstream with its final partial byte dropped. Legal at 1bpp only; the parser
    /// zero-fills the pixels the missing byte would have held.
    pub tight_floor: usize,
    /// One continuous bitstream, its final partial byte padded out.
    pub tight_ceil: usize,
    /// Each scanline restarted on a byte boundary.
    pub row_padded: usize,
}

impl PackedSizes {
    pub fn for_frame(width: u16, height: u16, bits_per_pixel: u8) -> Result<Self, ImpError> {
        let pixel_count = usize::from(width)
            .checked_mul(usize::from(height))
            .ok_or_else(|| ImpError::new("IMP frame pixel count overflow"))?;
        let bits = pixel_count
            .checked_mul(usize::from(bits_per_pixel))
            .ok_or_else(|| ImpError::new("IMP packed pixel size overflow"))?;
        let tight_floor = bits / 8;
        let tight_ceil = bits
            .checked_add(7)
            .map(|padded| padded / 8)
            .ok_or_else(|| ImpError::new("IMP packed pixel size overflow"))?;
        let row_bits = usize::from(width)
            .checked_mul(usize::from(bits_per_pixel))
            .ok_or_else(|| ImpError::new("IMP packed row size overflow"))?;
        let row_bytes = row_bits
            .checked_add(7)
            .map(|padded| padded / 8)
            .ok_or_else(|| ImpError::new("IMP packed row size overflow"))?;
        let row_padded = row_bytes
            .checked_mul(usize::from(height))
            .ok_or_else(|| ImpError::new("IMP packed row storage overflow"))?;
        Ok(Self {
            tight_floor,
            tight_ceil,
            row_padded,
        })
    }

    /// Every length the parser will accept, sorted and deduplicated.
    pub fn acceptable(&self, bits_per_pixel: u8) -> Vec<usize> {
        let mut sizes = if bits_per_pixel == 1 {
            vec![self.tight_floor, self.tight_ceil, self.row_padded]
        } else {
            vec![self.tight_ceil, self.row_padded]
        };
        sizes.sort_unstable();
        sizes.dedup();
        sizes
    }
}

/// Which of the two bit layouts a frame's packed pixels are laid out in.
///
/// The format does not record this. [`pixel_layout_for`] states the rule that decides it, and both
/// [`unpack_pixels`] and [`pack_pixels`] route through that one function so they cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelLayout {
    /// One continuous bitstream; only the very end is padded.
    Tight,
    /// Every scanline restarts on a byte boundary.
    RowPadded,
}

/// Whether a frame's two packed layouts produce the same *bytes*, so the frame records no choice.
///
/// Two independent ways to be identical, and dropping either one misreports the corpus:
///
/// - **A row's bits fill whole bytes.** Then the row boundaries are byte boundaries and restarting
///   each row on one is a no-op. Always true at 8bpp, which is most of the archive.
/// - **The frame has at most one row.** There is no second row for the row padding to displace,
///   whatever the width. Dropping this invents ambiguity for every 1x1 and 2x1 frame — 43 of them
///   in `imp.mpq`, whose apparent disagreement with the original bytes was padding bits and nothing
///   to do with layout.
///
/// `height == 0` is included in that second clause rather than left to fall through. A frame with
/// no rows has no layout to disagree about, and this is a public predicate that callers may ask
/// about any shape; leaving the boundary to `<`-versus-`<=` luck would have
/// [`layout_is_ambiguous`] report a frame with no pixels as ambiguous.
///
/// Equal *lengths* are not enough and are deliberately not consulted: 7x5 at 1bpp is five bytes
/// under both layouts and five different bytes. That case is [`layout_is_ambiguous`].
pub fn layouts_are_identical(width: u16, height: u16, bits_per_pixel: u8) -> bool {
    usize::from(width) * usize::from(bits_per_pixel) % 8 == 0 || height <= 1
}

/// Whether the two layouts are the same length and **different bytes**, so no stored size can name
/// which one the frame used.
///
/// This is the one shape where the decoder is guessing. [`pixel_layout_for`] resolves it as
/// [`PixelLayout::Tight`] — deliberately, and see that function for why — so a frame the original
/// tool wrote row-padded at one of these shapes is decoded wrong, with nothing in the file to say
/// so. 50 of the 41,373 payload-carrying frames in `imp.mpq` have such a shape -- the sweep's
/// `layout-ambiguous-shape`. The 45 in the `ambiguous-same-length` row of `docs/imp-format.md` is a
/// different question: that row also requires the stored size to BE the common length, and 5 of the
/// 50 are 1bpp frames that dropped their final partial byte.
///
/// Closing it needs information from outside the file: the engine's own reader, or a frame whose
/// pixels are known independently. Nothing in the bytes can do it.
pub fn layout_is_ambiguous(width: u16, height: u16, bits_per_pixel: u8) -> Result<bool, ImpError> {
    let sizes = PackedSizes::for_frame(width, height, bits_per_pixel)?;
    Ok(!layouts_are_identical(width, height, bits_per_pixel)
        && sizes.tight_ceil == sizes.row_padded)
}

/// Whether this frame's layout is one **this decoder could not have observed**, even though the
/// file does record it.
///
/// Distinct from [`layout_is_ambiguous`], and the two must be added rather than conflated. There
/// the file settles nothing; here the file settles it and the *reader* stops too early to see.
///
/// A compressed `record_variant == 0` payload declares no length, so `decode_rle_until_size`
/// consumes packets and tests the output length after each **complete** packet, stopping at the
/// first acceptable length that falls on a packet boundary. That is not the same as "the smallest
/// acceptable length": a 17x4 frame at 1bpp accepts `[8, 9, 12]`, and a single 12-byte literal
/// packet lands on 12 -- the row-padded size -- without ever passing through 8 or 9. Such a frame
/// **is** observed as row-padded, which is why `packed_size != sizes.row_padded` is part of the
/// test rather than "variant 0 and compressed" alone.
///
/// 752 of the 41,373 payload-carrying frames in `imp.mpq` are in this class -- fifteen times the 50
/// [`layout_is_ambiguous`] finds, and with the same consequence for an editor: if such a frame was
/// written row-padded, the pixels this decoder handed out were already wrong.
///
/// Takes the stored `packed_size` rather than a classified layout name because the two are not
/// interchangeable. A 7x1 frame at 1bpp stores `tight_floor` 0 while `tight_ceil == row_padded == 1`,
/// so it classifies as "tight-floor" -- not row-padded -- while this predicate is correctly false.
pub fn layout_is_unobservable(
    packed_size: usize,
    sizes: PackedSizes,
    record_variant: u8,
    compressed: bool,
) -> bool {
    sizes.tight_ceil != sizes.row_padded
        && record_variant == 0
        && compressed
        && packed_size != sizes.row_padded
}

/// The layout a stored payload of `packed_size` bytes is read with, stated as a rule about the
/// frame's **shape** rather than as a comparison of lengths.
///
/// Three cases, in this order, and the order is the whole content:
///
/// 1. [`layouts_are_identical`] — the two layouts emit the same bytes, so there is nothing to
///    decide. Reported as `Tight` because that is the cheaper path, not because the frame is
///    evidence of tightness.
/// 2. The layouts differ in length and `packed_size` is the row-padded one. Only here does a stored
///    size *name* a layout, and only here is `RowPadded` returned.
/// 3. Everything else, which includes [`layout_is_ambiguous`]: `Tight`.
///
/// **Case 3 is a choice, not a deduction.** Where the two layouts have equal length and different
/// meaning, the file settles nothing and some answer must still be given; picking tight here is
/// arbitrary and is written down as such so it can be argued with, rather than falling out of a
/// length comparison where nobody would find it.
///
/// The predecessor of this function was the inline test `packed.len() == row_padded && row_padded >
/// tight`, duplicated in the packer and the unpacker. It agrees with this rule on every input —
/// `layouts_are_identical` implies `row_padded == tight_ceil`, so the length test already covered
/// cases 1 and 2 — but it stated the rule in terms of a *length*, so the ambiguous case was
/// resolved silently and the duplication meant the packer and the unpacker could disagree.
pub fn pixel_layout_for(
    packed_size: usize,
    width: u16,
    height: u16,
    bits_per_pixel: u8,
) -> Result<PixelLayout, ImpError> {
    let sizes = PackedSizes::for_frame(width, height, bits_per_pixel)?;
    // Deleting this arm is an **equivalent mutation**, and the sweep duly reports it as surviving.
    // Identical layouts imply `row_padded == tight_ceil`, so the test below would fall through to
    // `Tight` on its own. The arm is kept because it is the rule's first case: without it the
    // reason an 8bpp frame reads tight is an arithmetic coincidence two functions away, and
    // `layouts_are_identical` would have no caller here at all.
    if layouts_are_identical(width, height, bits_per_pixel) {
        return Ok(PixelLayout::Tight);
    }
    if packed_size == sizes.row_padded && sizes.row_padded != sizes.tight_ceil {
        return Ok(PixelLayout::RowPadded);
    }
    Ok(PixelLayout::Tight)
}

fn decode_rle_exact(source: &[u8], acceptable_sizes: &[usize]) -> Result<Vec<u8>, ImpError> {
    let mut input = 0_usize;
    let mut output = Vec::new();
    while input < source.len() {
        decode_rle_packet(source, &mut input, &mut output)?;
    }
    if acceptable_sizes.contains(&output.len()) {
        Ok(output)
    } else {
        Err(ImpError::new(format!(
            "IMP RLE expands to unsupported packed size {}",
            output.len()
        )))
    }
}

fn decode_rle_until_size(
    source: &[u8],
    acceptable_sizes: &[usize],
) -> Result<(Vec<u8>, usize), ImpError> {
    let maximum_size = acceptable_sizes.iter().copied().max().unwrap_or(0);
    let mut input = 0_usize;
    let mut output = Vec::with_capacity(maximum_size);
    if acceptable_sizes.contains(&0) {
        return Ok((output, input));
    }
    loop {
        decode_rle_packet(source, &mut input, &mut output)?;
        if acceptable_sizes.contains(&output.len()) {
            return Ok((output, input));
        }
        if output.len() > maximum_size {
            return Err(ImpError::new(format!(
                "IMP RLE expands beyond the largest supported packed size {maximum_size}"
            )));
        }
    }
}

fn decode_rle_packet(
    source: &[u8],
    input: &mut usize,
    output: &mut Vec<u8>,
) -> Result<(), ImpError> {
    let control = *source
        .get(*input)
        .ok_or_else(|| ImpError::new("IMP RLE control byte is truncated"))?;
    *input += 1;
    if control < 0x80 {
        let count = usize::from(control) + 3;
        let value = *source
            .get(*input)
            .ok_or_else(|| ImpError::new("IMP RLE repeated value is truncated"))?;
        *input += 1;
        output.extend(std::iter::repeat_n(value, count));
    } else {
        let count = 0x100_usize - usize::from(control);
        let end = input
            .checked_add(count)
            .ok_or_else(|| ImpError::new("IMP RLE literal offset overflow"))?;
        let literal = source
            .get(*input..end)
            .ok_or_else(|| ImpError::new("IMP RLE literal is truncated"))?;
        output.extend_from_slice(literal);
        *input = end;
    }
    Ok(())
}

/// Expands bit-packed pixels into one palette index per pixel.
///
/// Which layout the bytes are read in is [`pixel_layout_for`]'s decision, not a length comparison
/// made here — see that function for the rule and for the one shape where it is a choice rather
/// than a deduction. [`pack_pixels`] asks the same function and must be given the same length.
pub fn unpack_pixels(
    packed: &[u8],
    width: u16,
    height: u16,
    bits_per_pixel: u8,
) -> Result<Vec<u8>, ImpError> {
    let acceptable_sizes = packed_sizes(width, height, bits_per_pixel)?;
    if !acceptable_sizes.contains(&packed.len()) {
        return Err(ImpError::new(format!(
            "IMP packed pixels have unsupported size {}",
            packed.len()
        )));
    }
    let pixel_count = usize::from(width) * usize::from(height);
    if bits_per_pixel == 8 {
        return Ok(packed.to_vec());
    }

    let pixels_per_byte = 8 / usize::from(bits_per_pixel);
    let mask = (1_u8 << bits_per_pixel) - 1;
    let mut pixels = Vec::with_capacity(pixel_count);
    let row_bytes = (usize::from(width) * usize::from(bits_per_pixel)).div_ceil(8);
    if pixel_layout_for(packed.len(), width, height, bits_per_pixel)? == PixelLayout::RowPadded {
        for row in packed.chunks_exact(row_bytes) {
            let row_start = pixels.len();
            for byte in row {
                unpack_byte(*byte, bits_per_pixel, mask, pixels_per_byte, &mut pixels);
            }
            pixels.truncate(row_start + usize::from(width));
        }
    } else {
        for byte in packed {
            unpack_byte(*byte, bits_per_pixel, mask, pixels_per_byte, &mut pixels);
            if pixels.len() >= pixel_count {
                break;
            }
        }
    }
    pixels.truncate(pixel_count);
    pixels.resize(pixel_count, 0);
    Ok(pixels)
}

fn unpack_byte(
    byte: u8,
    bits_per_pixel: u8,
    mask: u8,
    pixels_per_byte: usize,
    pixels: &mut Vec<u8>,
) {
    for subpixel in 0..pixels_per_byte {
        let shift = 8 - usize::from(bits_per_pixel) * (subpixel + 1);
        pixels.push((byte >> shift) & mask);
    }
}

/// The longest run one IMP repeat packet can express.
///
/// Control bytes `0x00`–`0x7F` mean `control + 3` copies, so `0x7F` is 130. **This is not
/// ByteRun1**, which the sibling `pbm` module implements: there the repeat count is `257 - control`
/// and the packet classes are split by sign. Reusing that encoder here produces garbage, and the
/// `+ 3` bias is the reason a two-byte run has no repeat form at all.
///
/// Public because it is not only an encoder detail: `--imp-roundtrip`'s bound on how many stored
/// bytes an unread row-padded continuation would have cost is `2 * ceil(delta / MAX_RLE_REPEAT)`,
/// and the published 752-frame figure moves if this number does.
pub const MAX_RLE_REPEAT: usize = 130;

/// The shortest run an IMP repeat packet can express: control `0x00` means three copies.
///
/// It is also the shortest run *worth* a repeat, so the same constant serves both roles. A repeat
/// costs two bytes; two identical bytes appended to an open literal also cost two. Breaking a
/// literal for a run of two would add a fresh literal header and lose a byte.
const MIN_RLE_REPEAT: usize = 3;

/// The longest literal one IMP packet can express: control `0x80` means the next 128 bytes are
/// literal, and `0xFF` means the next one is.
const MAX_RLE_LITERAL: usize = 128;

/// Compresses bit-packed pixels into the RLE form a file with `FILE_FLAG_RLE` stores.
///
/// Greedy and exact: `decode_rle_exact` on the result returns `packed` unchanged for any input.
/// What it does *not* claim is that the original tool made the same packet boundaries — that is a
/// separate, measured question, and `--imp-roundtrip` reports it as its own number.
///
/// One hazard this cannot rule out on its own. A `record_variant == 0` record stores no payload
/// length, so the reader stops at the first size [`packed_sizes`] accepts. If a packet happens to
/// land on the tight size while the frame is row-padded, the reader stops early and reads the rows
/// wrong. Nothing in the packet choice can prevent that; the caller must decode its own output
/// back through [`read_frame_pixels`] and check the length it got.
pub fn encode_rle(packed: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut cursor = 0;
    while cursor < packed.len() {
        let run = rle_run_length(packed, cursor);
        if run >= MIN_RLE_REPEAT {
            output.push((run - MIN_RLE_REPEAT) as u8);
            output.push(packed[cursor]);
            cursor += run;
            continue;
        }
        // Literal: keep taking bytes until a run worth its own packet starts, or the packet fills.
        let start = cursor;
        while cursor < packed.len()
            && cursor - start < MAX_RLE_LITERAL
            && rle_run_length(packed, cursor) < MIN_RLE_REPEAT
        {
            cursor += 1;
        }
        // `0x100 - count` copies for counts 1..=128, which is exactly `0xFF`..=`0x80`.
        output.push((0x100 - (cursor - start)) as u8);
        output.extend_from_slice(&packed[start..cursor]);
    }
    output
}

/// How many identical bytes start at `offset`, capped at one repeat packet.
fn rle_run_length(packed: &[u8], offset: usize) -> usize {
    let value = packed[offset];
    let mut length = 1;
    while length < MAX_RLE_REPEAT
        && offset + length < packed.len()
        && packed[offset + length] == value
    {
        length += 1;
    }
    length
}

/// Bit-packs palette indices into exactly `packed_size` bytes, inverting [`unpack_pixels`].
///
/// `packed_size` must be one of the frame's [`packed_sizes`] and is the caller's to supply, not the
/// encoder's to choose: the tight and row-padded layouts are both legal and a frame's stored size
/// is an observation about the original file. Passing the wrong one produces a valid frame of the
/// wrong length, which in a real file moves every later frame's pixels.
///
/// Padding — the spare bits at the end of a row or of the buffer — is written as zero. The original
/// files are not required to agree, and where they do not the pixels still survive, because no
/// reader ever looks at those bits.
pub fn pack_pixels(
    indices: &[u8],
    width: u16,
    height: u16,
    bits_per_pixel: u8,
    packed_size: usize,
) -> Result<Vec<u8>, ImpError> {
    let acceptable_sizes = packed_sizes(width, height, bits_per_pixel)?;
    if !acceptable_sizes.contains(&packed_size) {
        return Err(ImpError::new(format!(
            "IMP packed size {packed_size} is not one of {acceptable_sizes:?} for {width}x{height} at {bits_per_pixel}bpp"
        )));
    }
    let pixel_count = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| ImpError::new("IMP frame pixel count overflow"))?;
    if indices.len() != pixel_count {
        return Err(ImpError::new(format!(
            "expected {pixel_count} palette indices for {width}x{height}; got {}",
            indices.len()
        )));
    }
    if bits_per_pixel < 8 {
        let mask = (1_u8 << bits_per_pixel) - 1;
        if let Some(index) = indices.iter().find(|index| **index > mask) {
            return Err(ImpError::new(format!(
                "IMP palette index {index} does not fit in {bits_per_pixel} bits"
            )));
        }
    }
    if pixel_count == 0 {
        return Ok(Vec::new());
    }

    let mut packed = Vec::with_capacity(packed_size);
    // The same rule `unpack_pixels` asks, through the same function: whichever branch it takes on a
    // buffer of this length is the branch that has to have produced it.
    if pixel_layout_for(packed_size, width, height, bits_per_pixel)? == PixelLayout::RowPadded {
        for row in indices.chunks_exact(usize::from(width)) {
            pack_bits(row, bits_per_pixel, &mut packed);
        }
    } else {
        pack_bits(indices, bits_per_pixel, &mut packed);
    }
    // Only ever shortens, and only at 1bpp, where `tight_floor` drops a final partial byte the
    // parser zero-fills back. It cannot lengthen: the row-padded branch produces its own length
    // exactly, and the tight branch produces `tight_ceil`, the largest size the other branch admits.
    packed.resize(packed_size, 0);
    Ok(packed)
}

/// Packs one contiguous stretch of indices, most significant bits first, flushing a partial final
/// byte with zeros.
fn pack_bits(indices: &[u8], bits_per_pixel: u8, output: &mut Vec<u8>) {
    let pixels_per_byte = 8 / usize::from(bits_per_pixel);
    for chunk in indices.chunks(pixels_per_byte) {
        let mut byte = 0_u8;
        for (subpixel, index) in chunk.iter().enumerate() {
            let shift = 8 - usize::from(bits_per_pixel) * (subpixel + 1);
            byte |= index << shift;
        }
        output.push(byte);
    }
}

fn require_range(
    source: &[u8],
    offset: usize,
    count: usize,
    item_size: usize,
    label: &str,
) -> Result<(), ImpError> {
    let size = count
        .checked_mul(item_size)
        .ok_or_else(|| ImpError::new(format!("IMP {label} size overflow")))?;
    let end = offset
        .checked_add(size)
        .ok_or_else(|| ImpError::new(format!("IMP {label} offset overflow")))?;
    if end > source.len() {
        return Err(ImpError::new(format!("IMP {label} is truncated")));
    }
    Ok(())
}

fn read_u16(source: &[u8], offset: usize) -> Result<u16, ImpError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| ImpError::new("IMP u16 offset overflow"))?;
    let bytes: [u8; 2] = source
        .get(offset..end)
        .ok_or_else(|| ImpError::new("IMP u16 is truncated"))?
        .try_into()
        .expect("slice length was checked");
    Ok(u16::from_le_bytes(bytes))
}

fn read_i16(source: &[u8], offset: usize) -> Result<i16, ImpError> {
    read_u16(source, offset).map(|value| i16::from_le_bytes(value.to_le_bytes()))
}

fn read_u32(source: &[u8], offset: usize) -> Result<u32, ImpError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| ImpError::new("IMP u32 offset overflow"))?;
    let bytes: [u8; 4] = source
        .get(offset..end)
        .ok_or_else(|| ImpError::new("IMP u32 is truncated"))?
        .try_into()
        .expect("slice length was checked");
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    /// A variant-0 compressed payload does NOT necessarily resolve to the smallest acceptable size.
    ///
    /// `decode_rle_until_size` tests the output length after each COMPLETE packet, so it stops at
    /// the first acceptable length that falls on a packet boundary. A 17x4 frame at 1bpp accepts
    /// [8, 9, 12]; one 12-byte literal packet lands on 12 -- the row-padded size -- without the
    /// reader ever seeing 8 or 9. The sweep's "unobservable" counter assumed the smallest was
    /// always taken, which let it report `row-padded` and `unobservable` for one frame at once.
    #[test]
    fn a_single_packet_can_land_on_the_row_padded_size_skipping_the_tight_ones() {
        let sizes = packed_sizes(17, 4, 1).expect("sizes");
        assert_eq!(sizes, vec![8, 9, 12], "the three acceptable sizes for 17x4 at 1bpp");

        // One literal packet of 12 bytes: control 0x100 - 12 = 0xf4.
        let mut payload = vec![0xf4_u8];
        payload.extend(std::iter::repeat_n(0xab_u8, 12));

        let (decoded, consumed) = decode_rle_until_size(&payload, &sizes).expect("decode");
        assert_eq!(decoded.len(), 12, "landed on the row-padded size, not on 8 or 9");
        assert_eq!(consumed, payload.len(), "the whole packet was consumed");
    }

    /// The counter-case, so the test above cannot pass by the reader simply never stopping early.
    #[test]
    fn a_packet_boundary_at_the_smallest_acceptable_size_does_stop_there() {
        let sizes = packed_sizes(17, 4, 1).expect("sizes");
        // One literal packet of exactly 8 bytes: control 0x100 - 8 = 0xf8.
        let mut payload = vec![0xf8_u8];
        payload.extend(std::iter::repeat_n(0xab_u8, 8));
        payload.extend([0xf4_u8, 0x00]); // trailing bytes that must never be consumed

        let (decoded, consumed) = decode_rle_until_size(&payload, &sizes).expect("decode");
        assert_eq!(decoded.len(), 8, "stopped at tight_floor, the first boundary that qualifies");
        assert_eq!(consumed, 9, "the trailing packet is left unread");
    }

    use super::*;

    fn synthetic_imp() -> Vec<u8> {
        let mut source = vec![0_u8; 32 + 16 + 8 + 16];
        source[2] = 1;
        source[4..6].copy_from_slice(&2_u16.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        let palette_offset = source.len();
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32..43].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48..50].copy_from_slice(&7_u16.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&1_u16.to_le_bytes());
        source[56 + 6..56 + 8].copy_from_slice(&2_u16.to_le_bytes());
        let pixel_offset = palette_offset + PALETTE_BYTES;
        source[56 + 12..56 + 16].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
        source.resize(pixel_offset, 0);
        source[palette_offset..palette_offset + 4].copy_from_slice(&[3, 2, 1, 0]);
        source.extend_from_slice(&[0xaa, 0xbb]);
        source
    }

    #[test]
    fn parses_tables_palette_and_frame_totals() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        assert_eq!((sprite.maximum_width, sprite.maximum_height), (2, 1));
        assert_eq!(sprite.sequence_count, 1);
        assert_eq!(sprite.facing_count, 1);
        assert_eq!(sprite.frame_count, 1);
        assert_eq!(
            sprite.sequences[0].metadata,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        );
        assert_eq!(sprite.sequences[0].first_facing, 0);
        assert_eq!(sprite.sequences[0].frame_count, 1);
        assert_eq!(sprite.facings[0].metadata, 7);
        assert_eq!(sprite.facings[0].first_frame, 0);
        assert_eq!(sprite.facings[0].frame_count, 1);
        assert_eq!(sprite.frame_location(0).unwrap(), (0, 0, 0));
        assert_eq!(sprite.raw_pixel_bytes, 2);
        assert_eq!(sprite.stored_pixel_bytes, 2);
        // Stored blue, red, green, pad -- so file bytes [3, 2, 1] render as red 2, green 1,
        // blue 3. Measured in the engine, see the research log for 2026-09-17.
        assert_eq!(sprite.palette[0], [2, 1, 3, 255]);
    }

    #[test]
    fn rejects_out_of_bounds_frame_data() {
        let mut source = synthetic_imp();
        source[56 + 12..56 + 16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            ImpSprite::parse(&source).unwrap_err().to_string(),
            "IMP frame pixels is truncated"
        );
    }

    #[test]
    fn duplicate_frames_reference_existing_frame_indices_without_pixel_payloads() {
        let mut source = synthetic_imp();
        source.splice(72..72, [0_u8; FRAME_RECORD_SIZE]);
        source[8..12].copy_from_slice(&88_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&1112_u32.to_le_bytes());
        source[72] = FRAME_FLAG_DUPLICATE;

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frame_count, 2);
        assert_eq!(sprite.duplicate_frame_count, 1);
        assert_eq!(sprite.raw_pixel_bytes, 2);
        assert_eq!(sprite.stored_pixel_bytes, 2);
        assert_eq!(
            sprite.resolved_frame(1).unwrap().palette_indices,
            [0xaa, 0xbb]
        );
    }

    #[test]
    fn shared_pixel_records_resolve_by_payload_offset_inside_a_facing() {
        let mut source = synthetic_imp();
        source.splice(72..72, [0_u8; FRAME_RECORD_SIZE]);
        source[8..12].copy_from_slice(&88_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&1112_u32.to_le_bytes());
        source[72] = FRAME_FLAG_SHARED_PIXELS;
        source[72 + 12..72 + 16].copy_from_slice(&1112_u32.to_le_bytes());

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frame_count, 2);
        assert_eq!(sprite.duplicate_frame_count, 1);
        assert_eq!(sprite.frames[1].source_frame, Some(0));
        assert_eq!(
            sprite.resolved_frame(1).unwrap().palette_indices,
            [0xaa, 0xbb]
        );
    }

    /// A sprite whose second facing holds a *two-record* frame table: record 0 carries `0x04`
    /// and record 1 is a genuinely distinct frame with its own pixels.
    ///
    /// This is the shape the archive actually stores — measured on 2026-09-17, `aicr3b` sequence 2
    /// facing 0 has records `[04 00]` and `chwmmb` sequence 5 facings 0-4 each have
    /// `[04 00 00 00 00 00]`. The decoder used to key the whole facing off record 0's flag and
    /// point every frame slot at that one record, so records 1.. were never read.
    ///
    /// Layout: facing 0 holds the one real frame at record 64; facing 1 holds records 80 (shared)
    /// and 96 (real, its own payload).
    fn synthetic_imp_with_record_array() -> Vec<u8> {
        let palette_offset = 112_usize;
        let first_pixels = palette_offset + PALETTE_BYTES;
        let second_pixels = first_pixels + 2;
        let mut source = vec![0_u8; palette_offset];
        source[2] = 1;
        source[4..6].copy_from_slice(&2_u16.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        // One sequence with two facings.
        source[32 + 11] = 2;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        // Facing 0: one frame, table at 64.
        source[48..50].copy_from_slice(&7_u16.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&64_u32.to_le_bytes());
        // Facing 1: two frames, table at 80.
        source[56..58].copy_from_slice(&8_u16.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 4..56 + 8].copy_from_slice(&80_u32.to_le_bytes());
        // Record at 64: the real frame facing 0 draws.
        source[64 + 2..64 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[64 + 4..64 + 6].copy_from_slice(&1_u16.to_le_bytes());
        source[64 + 6..64 + 8].copy_from_slice(&2_u16.to_le_bytes());
        source[64 + 12..64 + 16].copy_from_slice(&(first_pixels as u32).to_le_bytes());
        // Record at 80: shares record 64's payload.
        source[80] = FRAME_FLAG_SHARED_PIXELS;
        source[80 + 12..80 + 16].copy_from_slice(&(first_pixels as u32).to_le_bytes());
        // Record at 96: a distinct frame with a payload of its own.
        source[96 + 2..96 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[96 + 4..96 + 6].copy_from_slice(&1_u16.to_le_bytes());
        source[96 + 6..96 + 8].copy_from_slice(&2_u16.to_le_bytes());
        source[96 + 12..96 + 16].copy_from_slice(&(second_pixels as u32).to_le_bytes());
        source.resize(first_pixels, 0);
        source[palette_offset..palette_offset + 4].copy_from_slice(&[3, 2, 1, 0]);
        source.extend_from_slice(&[0xaa, 0xbb]);
        source.extend_from_slice(&[0xcc, 0xdd]);
        source
    }

    /// Regression for the decoder bug that produced five of the ten recorded corpus exceptions.
    ///
    /// A facing's frame table is an ordinary array: only the first record may carry `0x04`, and
    /// the records after it are real frames. The old decoder read record 0's flag, aliased every
    /// frame slot to it, and skipped the full-table bounds check, so it reported two duplicates
    /// here and never read the second record's 0xcc 0xdd payload.
    #[test]
    fn a_facing_frame_table_is_an_array_of_records() {
        let sprite = ImpSprite::parse(&synthetic_imp_with_record_array()).unwrap();

        assert_eq!(sprite.frame_count, 3);
        assert_eq!(sprite.duplicate_frame_count, 1);
        assert_eq!(sprite.sequences[0].facing_count, 2);
        assert_eq!(sprite.sequences[0].frame_count, 3);
        assert_eq!(sprite.facings[1].first_frame, 1);
        assert_eq!(sprite.facings[1].frame_count, 2);
        assert_eq!(sprite.frame_location(2).unwrap(), (0, 1, 1));

        // Frame 1 is the `0x04` record; frame 2 is the SECOND record of the same facing, and it
        // is read, not aliased to the first.
        assert_eq!(sprite.frames[1].record_offset, 80);
        assert_eq!(sprite.frames[1].source_frame, Some(0));
        assert_eq!(sprite.frames[2].record_offset, 96);
        assert_eq!(sprite.frames[2].source_frame, None);
        assert_eq!(sprite.frames[2].palette_indices, [0xcc, 0xdd]);
        assert_eq!(
            sprite.resolved_frame(1).unwrap().palette_indices,
            [0xaa, 0xbb]
        );
        // Both payloads are counted: the swallowed record is what made five corpus files report
        // fewer pixels than their headers.
        assert_eq!(sprite.raw_pixel_bytes, 4);
        assert_eq!(sprite.stored_pixel_bytes, 4);
        assert_eq!(sprite.frames_sharing_record(1).unwrap(), [1]);
        assert_eq!(sprite.frames_sharing_record(2).unwrap(), [2]);
    }

    /// The full-table bounds check used to be skipped whenever record 0 carried `0x04`, so a
    /// facing could claim any number of records and the decoder would read none of them.
    #[test]
    fn a_truncated_frame_table_is_rejected_even_when_the_first_record_is_shared() {
        let mut source = synthetic_imp_with_record_array();
        // Facing 1 claims 200 records from offset 80; the file is far shorter.
        source[56 + 2..56 + 4].copy_from_slice(&200_u16.to_le_bytes());
        assert_eq!(
            ImpSprite::parse(&source).unwrap_err().to_string(),
            "IMP frame table is truncated"
        );
    }

    /// A three-frame sprite whose payloads, hotspot array and palette all sit *after* the tables,
    /// so a length-changing rewrite has to move every one of them and every pointer to them.
    ///
    /// Frames 0 and 1 carry origin pairs and frame 2 carries a hotspot record, so one fixture
    /// covers both forms of the record's shared dword. The sequence metadata is deliberately not
    /// zeroed: bytes 5-10 hold the authoring tool's uninitialised leftover text, which a rewrite
    /// must copy verbatim rather than tidy away.
    ///
    /// Frames are 16 pixels wide so a payload can change length by more than the 8-byte payload
    /// grid. At four pixels every edit fits inside one grid cell and nothing ever moves, which
    /// would make the pointer-rewriting tests vacuous.
    const FIXTURE_WIDTH: u16 = 16;
    const FIXTURE_PAYLOADS: [usize; 3] = [104, 128, 152];
    const FIXTURE_HOTSPOTS: usize = 176;
    const FIXTURE_PALETTE: usize = 184;

    fn writable_imp(compressed: bool, pixels: [&[u8]; 3]) -> Vec<u8> {
        let mut source = vec![0_u8; FIXTURE_PALETTE + PALETTE_BYTES];
        source[0] = if compressed { FILE_FLAG_RLE } else { 0 };
        source[2] = 1;
        source[4..6].copy_from_slice(&FIXTURE_WIDTH.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        source[8..12].copy_from_slice(&(FIXTURE_PALETTE as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32..43].copy_from_slice(b"\x06\x80\x0f\x01\xffframes");
        source[43] = 1;
        source[44..48].copy_from_slice(&48_u32.to_le_bytes());
        source[50..52].copy_from_slice(&3_u16.to_le_bytes());
        source[52..56].copy_from_slice(&56_u32.to_le_bytes());
        for (index, frame_pixels) in pixels.iter().enumerate() {
            let record = 56 + index * FRAME_RECORD_SIZE;
            source[record + 2..record + 4].copy_from_slice(&FIXTURE_WIDTH.to_le_bytes());
            source[record + 4..record + 6].copy_from_slice(&1_u16.to_le_bytes());
            let payload = if compressed {
                encode_rle(frame_pixels)
            } else {
                frame_pixels.to_vec()
            };
            source[record + 6..record + 8].copy_from_slice(&(payload.len() as u16).to_le_bytes());
            if index == 2 {
                source[record + 1] = 1;
                source[record + 8..record + 12]
                    .copy_from_slice(&(FIXTURE_HOTSPOTS as u32).to_le_bytes());
            } else {
                source[record + 8..record + 10]
                    .copy_from_slice(&(-3_i16 - index as i16).to_le_bytes());
                source[record + 10..record + 12].copy_from_slice(&7_i16.to_le_bytes());
            }
            source[record + 12..record + 16]
                .copy_from_slice(&(FIXTURE_PAYLOADS[index] as u32).to_le_bytes());
            source[FIXTURE_PAYLOADS[index]..FIXTURE_PAYLOADS[index] + payload.len()]
                .copy_from_slice(&payload);
        }
        source[FIXTURE_HOTSPOTS..FIXTURE_HOTSPOTS + 6]
            .copy_from_slice(&[0x07, 0x00, 0x03, 0x00, 0xfc, 0xff]);
        source[FIXTURE_PALETTE..FIXTURE_PALETTE + 4].copy_from_slice(&[3, 2, 1, 0]);
        source
    }

    /// Sixteen identical bytes: one repeat packet, two bytes stored.
    const RUN: &[u8] = &[9; 16];
    /// Sixteen identical bytes of a different value: also two bytes, so a swap changes nothing but
    /// the pixels.
    const OTHER_RUN: &[u8] = &[5; 16];
    /// Sixteen distinct bytes: one literal packet, seventeen bytes stored. Fifteen more than a run,
    /// which is more than the 8-byte payload grid and so actually moves the file.
    const LITERAL: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

    /// The whole-file acceptance property in miniature: replacing a frame with the pixels just read
    /// out of it must reproduce the file byte for byte. Measured on the real archive by
    /// `--imp-roundtrip --rewrite`, which puts 35,840 frames through this and gets 35,840 back.
    #[test]
    fn rewriting_a_frame_with_its_own_pixels_reproduces_the_file() {
        for compressed in [false, true] {
            let source = writable_imp(compressed, [RUN, LITERAL, OTHER_RUN]);
            let sprite = ImpSprite::parse(&source).unwrap();
            for index in 0..3 {
                let write =
                    write_frame_pixels(&source, index, &sprite.frames[index].palette_indices)
                        .unwrap();
                assert_eq!(write.bytes, source, "compressed={compressed} frame {index}");
                assert_eq!(write.shift, 0);
                assert_eq!(write.alignment_padding, 0);
            }
        }
        // A file with a single frame, whose payload is the last thing in it, so nothing follows to
        // be moved and the pointer walk has exactly one pixel pointer to leave alone.
        let source = synthetic_imp();
        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames.len(), 1);
        let write = write_frame_pixels(&source, 0, &sprite.frames[0].palette_indices).unwrap();
        assert_eq!(write.bytes, source);
    }

    /// Reads back the pointers a rewrite has to maintain, so a test can assert on them by name
    /// rather than on raw offsets.
    fn pointers(source: &[u8]) -> (usize, usize, [usize; 3], usize) {
        let palette = read_u32(source, 8).unwrap() as usize;
        let sequence_table = read_u32(source, 28).unwrap() as usize;
        let payloads = [
            read_u32(source, 56 + 12).unwrap() as usize,
            read_u32(source, 72 + 12).unwrap() as usize,
            read_u32(source, 88 + 12).unwrap() as usize,
        ];
        let hotspots = read_u32(source, 88 + 8).unwrap() as usize;
        (palette, sequence_table, payloads, hotspots)
    }

    /// A payload that grows moves everything after it, and every absolute offset that addressed
    /// something after it has to follow. The tables are *before* the pixels here, so the sequence
    /// table pointer must stay put while the palette and the later payloads move -- an unconditional
    /// shift would break the first and a shift that only knew about payloads would break the rest.
    #[test]
    fn a_longer_payload_moves_every_offset_that_pointed_past_it() {
        let source = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
        let before = pointers(&source);

        // Frame 0's payload is a 2-byte run; sixteen distinct values cost 17.
        let write = write_frame_pixels(&source, 0, LITERAL).unwrap();
        assert_eq!(write.stored_size, (2, 17));
        // 17 bytes rounded back up to the 8-byte payload grid: one byte of padding, and the file
        // grows by two whole grid cells rather than by fifteen bytes.
        assert_eq!(write.alignment_padding, 1);
        assert_eq!(write.shift, 16);
        assert_eq!(write.bytes.len(), source.len() + 16);

        let after = pointers(&write.bytes);
        assert_eq!(after.1, before.1, "the sequence table is before the pixels");
        assert_eq!(
            after.2[0], before.2[0],
            "the rewritten frame keeps its start"
        );
        assert_eq!(after.2[1], before.2[1] + 16);
        assert_eq!(after.2[2], before.2[2] + 16);
        assert_eq!(after.3, before.3 + 16, "the hotspot array moved");
        assert_eq!(after.0, before.0 + 16, "the palette moved");
        // The palette, the hotspot array and the two later payloads: four, and the frame's own
        // pointer is not one of them.
        assert_eq!(write.rewritten_offsets, 4);

        // Everything the sprite means is unchanged apart from the one frame's pixels.
        let rewritten = ImpSprite::parse(&write.bytes).unwrap();
        let original = ImpSprite::parse(&source).unwrap();
        assert_eq!(rewritten.frames[0].palette_indices, LITERAL);
        assert_eq!(
            rewritten.frames[1].palette_indices,
            original.frames[1].palette_indices
        );
        assert_eq!(
            rewritten.frames[2].palette_indices,
            original.frames[2].palette_indices
        );
        assert_eq!(rewritten.frames[2].hotspots, original.frames[2].hotspots);
        assert_eq!(rewritten.frames[0].origin_x, original.frames[0].origin_x);
        assert_eq!(rewritten.palette, original.palette);
        assert_eq!(rewritten.sequences, original.sequences);
    }

    /// The mirror image: a payload that shrinks past a grid cell pulls everything after it back and
    /// the file gets shorter. A writer that only ever appended would pass the growth test and lose
    /// here.
    #[test]
    fn a_shorter_payload_pulls_every_later_offset_back() {
        let source = writable_imp(true, [LITERAL, RUN, OTHER_RUN]);
        let before = pointers(&source);

        let write = write_frame_pixels(&source, 0, RUN).unwrap();
        assert_eq!(write.stored_size, (17, 2));
        assert_eq!(write.alignment_padding, 7);
        assert_eq!(write.shift, -8);
        assert_eq!(write.bytes.len(), source.len() - 8);

        let after = pointers(&write.bytes);
        assert_eq!(after.2[0], before.2[0]);
        assert_eq!(after.2[1], before.2[1] - 8);
        assert_eq!(after.2[2], before.2[2] - 8);
        assert_eq!(after.3, before.3 - 8);
        assert_eq!(after.0, before.0 - 8);
        assert_eq!(
            ImpSprite::parse(&write.bytes).unwrap().frames[0].palette_indices,
            RUN
        );
    }

    /// Equal length is the case a shift-by-delta writer gets right by accident and a splice writer
    /// has to get right on purpose: nothing may move, and every byte outside the payload must be
    /// the source's own.
    #[test]
    fn an_equal_length_payload_moves_nothing() {
        let source = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
        let write = write_frame_pixels(&source, 0, OTHER_RUN).unwrap();

        assert_eq!(write.stored_size, (2, 2));
        assert_eq!(write.shift, 0);
        assert_eq!(write.alignment_padding, 0);
        assert_eq!(write.bytes.len(), source.len());
        assert_eq!(pointers(&write.bytes), pointers(&source));
        let differing: Vec<usize> = (0..source.len())
            .filter(|index| source[*index] != write.bytes[*index])
            .collect();
        assert!(!differing.is_empty(), "the pixels did change");
        assert!(
            differing.iter().all(|index| (104..106).contains(index)),
            "bytes outside the payload changed: {differing:?}"
        );
    }

    /// The last frame has nothing after it but the hotspot array and the palette, and the first has
    /// everything after it. Both ends are exercised because an off-by-one in the pointer walk shows
    /// up at exactly one of them.
    #[test]
    fn the_first_and_the_last_frame_are_both_writable() {
        let source = writable_imp(true, [RUN, LITERAL, RUN]);
        for index in [0_usize, 2] {
            let write = write_frame_pixels(&source, index, LITERAL).unwrap();
            assert_eq!(write.shift, 16, "frame {index}");
            let rewritten = ImpSprite::parse(&write.bytes).unwrap();
            assert_eq!(rewritten.frames[index].palette_indices, LITERAL);
            // The hotspot array still reads, which is the pointer most easily left behind when the
            // *last* payload grows -- it sits after every payload.
            assert_eq!(rewritten.frames[2].hotspots.len(), 1);
            assert_eq!(rewritten.frames[2].hotspots[0].id, 7);
            assert_eq!(rewritten.palette[0], [2, 1, 3, 255]);
        }
    }

    /// Bytes 5-10 of a sequence record hold uninitialised leftover text the engine never reads.
    /// Re-synthesising a "clean" record would destroy evidence; the writer copies, so it survives a
    /// rewrite that moved the whole tail of the file.
    #[test]
    fn a_rewrite_copies_the_sequence_records_uninitialised_bytes_verbatim() {
        let source = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
        let write = write_frame_pixels(&source, 0, LITERAL).unwrap();

        assert_eq!(&source[32..43], b"\x06\x80\x0f\x01\xffframes");
        // The whole sequence record, and the facing record after it: neither holds an offset past
        // the replaced payload, so neither may change by a single byte.
        assert_eq!(&write.bytes[32..56], &source[32..56]);
    }

    /// Every payload in `imp.mpq` starts on an 8-byte boundary -- 41,373 of 41,373, measured by
    /// `--imp-roundtrip`. A rewrite keeps that, so the shift is always a multiple of eight whatever
    /// the payload's own length.
    #[test]
    fn a_rewrite_keeps_every_later_payload_on_the_eight_byte_grid() {
        let mut ragged = LITERAL.to_vec();
        ragged[4] = ragged[3];
        for replacement in [RUN, LITERAL, OTHER_RUN, ragged.as_slice()] {
            let source = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
            let write = write_frame_pixels(&source, 0, replacement).unwrap();
            assert_eq!(
                write.shift % PAYLOAD_ALIGNMENT as isize,
                0,
                "shift {} for {replacement:?}",
                write.shift
            );
            let rewritten = ImpSprite::parse(&write.bytes).unwrap();
            for frame in &rewritten.frames {
                let offset = frame
                    .pixels_offset
                    .expect("every frame carries its own pixels");
                assert_eq!(offset % PAYLOAD_ALIGNMENT, 0, "payload at {offset}");
            }
        }
    }

    /// 3,439 of the 41,373 payload-carrying frames in `imp.mpq` have their pixels read by another
    /// record. Replacing one would repaint a picture the caller never named, so the writer refuses
    /// and says which frames it would have hit.
    #[test]
    fn writing_pixels_refuses_a_frame_whose_payload_another_record_reads() {
        let source = synthetic_imp_with_record_array();
        let sprite = ImpSprite::parse(&source).unwrap();
        // Frame 1 is a `0x04` record pointing at frame 0's payload.
        assert_eq!(sprite.frames_sharing_pixels(0).unwrap(), [1]);
        let message = write_frame_pixels(&source, 0, &[1, 2])
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("shares its pixels with frame(s) [1]"),
            "{message}"
        );

        // The shared record itself is refused for a different reason, and names where to go.
        let message = write_frame_pixels(&source, 1, &[1, 2])
            .unwrap_err()
            .to_string();
        assert!(message.contains("is a duplicate of frame 0"), "{message}");

        // Frame 2 is nobody's source, so it is writable -- the refusal is about sharing, not about
        // being in a file that happens to contain sharing.
        assert!(sprite.frames_sharing_pixels(2).unwrap().is_empty());
        assert!(write_frame_pixels(&source, 2, &[1, 2]).is_ok());
    }

    /// A `0x08` back-reference resolves by frame *index* rather than by payload offset, so it is a
    /// second, independent way for a replacement to reach art it was not given.
    #[test]
    fn writing_pixels_refuses_a_frame_a_back_reference_resolves_to() {
        let mut source = synthetic_imp();
        source.splice(72..72, [0_u8; FRAME_RECORD_SIZE]);
        source[8..12].copy_from_slice(&88_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&1112_u32.to_le_bytes());
        source[72] = FRAME_FLAG_DUPLICATE;

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames[1].source_frame, Some(0));
        assert_eq!(sprite.frames_sharing_pixels(0).unwrap(), [1]);
        let message = write_frame_pixels(&source, 0, &[1, 2])
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("shares its pixels with frame(s) [1]"),
            "{message}"
        );
    }

    /// Two facings may point at one frame table, so one record backs two logical frames and a write
    /// through either index is a write through both.
    #[test]
    fn writing_pixels_refuses_a_record_that_backs_more_than_one_frame() {
        let mut source = synthetic_imp_with_record_array();
        source[56 + 2..56 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[56 + 4..56 + 8].copy_from_slice(&64_u32.to_le_bytes());

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames_sharing_record(0).unwrap(), [0, 1]);
        let message = write_frame_pixels(&source, 0, &[1, 2])
            .unwrap_err()
            .to_string();
        assert!(message.contains("shares its pixels with"), "{message}");
    }

    #[test]
    fn writing_pixels_refuses_the_wrong_number_of_pixels_and_an_unknown_frame() {
        let source = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
        let message = write_frame_pixels(&source, 0, &[1, 2, 3])
            .unwrap_err()
            .to_string();
        assert!(message.contains("expected 16 palette indices"), "{message}");
        let message = write_frame_pixels(&source, 9, RUN).unwrap_err().to_string();
        assert!(
            message.contains("frame index 9 is out of range"),
            "{message}"
        );
    }

    /// A sprite laid out the other way round: **pixels first, every table last**, two payloads that
    /// abut with no gap, one frame table shared by two facings, and a back-reference record.
    ///
    /// [`writable_imp`] puts its tables before the pixels, so a rewrite there never has to move a
    /// sequence, facing or frame table at all, and no pointer ever sits exactly on the end of the
    /// replaced payload. Both of those are ordinary in the shipped archive -- 5,084 of its 39,573
    /// adjacent payload pairs abut exactly -- so a writer tested only against that layout has been
    /// tested against half the problem.
    ///
    /// - frame 0: record `R0` under facing 0, its own payload at 32. This is the one written.
    /// - frame 1: record `R1` under facing 1, payload at 57, **immediately after frame 0's**, and
    ///   carrying the hotspot array.
    /// - frame 2: facing 2 points at `R1`'s table as well, so this frame shares the record and the
    ///   pointer walk reaches `R1`'s fields twice.
    /// - frame 3: a `0x08` record whose pixel dword is the frame *index* 1, not an offset.
    /// - frame 4: a `0x04` record whose pixel dword **is** an offset -- frame 1's payload -- and so
    ///   has to move with it, unlike frame 3's.
    fn tables_after_pixels_imp() -> Vec<u8> {
        const PAYLOADS: [usize; 2] = [32, 57];
        const HOTSPOTS: usize = 82;
        const PALETTE: usize = 96;
        const SEQUENCE_TABLE: usize = 1120;
        const FACING_TABLE: usize = 1136;
        // The facing table now holds five 8-byte records, so the frame tables start after 1176.
        const FRAME_TABLES: [usize; 4] = [1176, 1192, 1208, 1224];
        let width = 24_u16;
        let mut source = vec![0_u8; FRAME_TABLES[3] + FRAME_RECORD_SIZE];
        source[0] = FILE_FLAG_RLE;
        source[2] = 1;
        source[4..6].copy_from_slice(&width.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        source[8..12].copy_from_slice(&(PALETTE as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&(SEQUENCE_TABLE as u32).to_le_bytes());
        source[SEQUENCE_TABLE..SEQUENCE_TABLE + 11]
            .copy_from_slice(b"\x06\x80\x0f\x01\xff\\imps\\");
        source[SEQUENCE_TABLE + 11] = 5;
        source[SEQUENCE_TABLE + 12..SEQUENCE_TABLE + 16]
            .copy_from_slice(&(FACING_TABLE as u32).to_le_bytes());
        // Facings 1 and 2 share one frame table, so the pointer walk meets its record twice.
        for (index, table) in [
            FRAME_TABLES[0],
            FRAME_TABLES[1],
            FRAME_TABLES[1],
            FRAME_TABLES[2],
            FRAME_TABLES[3],
        ]
        .into_iter()
        .enumerate()
        {
            let facing = FACING_TABLE + index * FACING_RECORD_SIZE;
            source[facing + 2..facing + 4].copy_from_slice(&1_u16.to_le_bytes());
            source[facing + 4..facing + 8].copy_from_slice(&(table as u32).to_le_bytes());
        }
        for (index, payload_offset) in PAYLOADS.into_iter().enumerate() {
            let record = FRAME_TABLES[index];
            let pixels: Vec<u8> = (0..width)
                .map(|pixel| (pixel as u8) + 100 * index as u8)
                .collect();
            let payload = encode_rle(&pixels);
            assert_eq!(payload.len(), 25, "one literal packet of 24 distinct bytes");
            source[record + 2..record + 4].copy_from_slice(&width.to_le_bytes());
            source[record + 4..record + 6].copy_from_slice(&1_u16.to_le_bytes());
            source[record + 6..record + 8].copy_from_slice(&(payload.len() as u16).to_le_bytes());
            if index == 1 {
                source[record + 1] = 1;
                source[record + 8..record + 12].copy_from_slice(&(HOTSPOTS as u32).to_le_bytes());
            } else {
                source[record + 8..record + 10].copy_from_slice(&(-3_i16).to_le_bytes());
                source[record + 10..record + 12].copy_from_slice(&7_i16.to_le_bytes());
            }
            source[record + 12..record + 16]
                .copy_from_slice(&(payload_offset as u32).to_le_bytes());
            source[payload_offset..payload_offset + payload.len()].copy_from_slice(&payload);
        }
        // The back-reference: its pixel dword is the frame index 1, and shifting it as if it were a
        // file offset would point it past the end of the frame list.
        source[FRAME_TABLES[2]] = FRAME_FLAG_DUPLICATE;
        source[FRAME_TABLES[2] + 12..FRAME_TABLES[2] + 16].copy_from_slice(&1_u32.to_le_bytes());
        // The shared-pixel record: its dword is frame 1's payload address and must move with it.
        source[FRAME_TABLES[3]] = FRAME_FLAG_SHARED_PIXELS;
        source[FRAME_TABLES[3] + 12..FRAME_TABLES[3] + 16]
            .copy_from_slice(&(PAYLOADS[1] as u32).to_le_bytes());
        source[HOTSPOTS..HOTSPOTS + 6].copy_from_slice(&[0x07, 0x00, 0x03, 0x00, 0xfc, 0xff]);
        source[PALETTE..PALETTE + 4].copy_from_slice(&[3, 2, 1, 0]);
        source
    }

    /// Every absolute offset in the file, read back by name.
    fn late_table_pointers(source: &[u8]) -> Vec<(&'static str, usize)> {
        let sequence_table = read_u32(source, 28).unwrap() as usize;
        let facing_table = read_u32(source, sequence_table + 12).unwrap() as usize;
        vec![
            ("palette", read_u32(source, 8).unwrap() as usize),
            ("sequence-table", sequence_table),
            ("facing-table", facing_table),
            (
                "frame-table-0",
                read_u32(source, facing_table + 4).unwrap() as usize,
            ),
            (
                "frame-table-1",
                read_u32(source, facing_table + 8 + 4).unwrap() as usize,
            ),
            (
                "frame-table-2",
                read_u32(source, facing_table + 24 + 4).unwrap() as usize,
            ),
            (
                "frame-table-3",
                read_u32(source, facing_table + 32 + 4).unwrap() as usize,
            ),
        ]
    }

    /// The sequence, facing and frame tables all sit after the pixels here, so all three pointers
    /// have to move; frame 1's payload begins exactly where frame 0's ended, so a pointer sitting
    /// on the boundary has to move too; and one record is reached twice through two facings, so it
    /// must be moved once rather than twice.
    #[test]
    fn a_rewrite_moves_the_tables_when_they_follow_the_pixels() {
        let source = tables_after_pixels_imp();
        let original = ImpSprite::parse(&source).unwrap();
        assert_eq!(original.frames.len(), 5);
        assert_eq!(original.frames[4].source_frame, Some(1));
        assert_eq!(
            original.frames[1].record_offset,
            original.frames[2].record_offset
        );
        assert_eq!(original.frames[3].source_frame, Some(1));
        // Frame 0's payload ends exactly where frame 1's begins.
        assert_eq!(
            original.frames[0].pixels_offset.unwrap() + original.frames[0].stored_size.unwrap(),
            original.frames[1].pixels_offset.unwrap()
        );
        // And frame 0 is nobody's source, so it is writable.
        assert!(original.frames_sharing_pixels(0).unwrap().is_empty());

        let before = late_table_pointers(&source);
        let write = write_frame_pixels(&source, 0, &[7; 24]).unwrap();
        assert_eq!(write.stored_size, (25, 2));
        assert_eq!(write.alignment_padding, 7);
        assert_eq!(write.shift, -16);

        for ((name, old), (_, new)) in before.iter().zip(late_table_pointers(&write.bytes)) {
            assert_eq!(new, old - 16, "{name} did not move with the pixels");
        }

        let rewritten = ImpSprite::parse(&write.bytes).unwrap();
        assert_eq!(rewritten.frames[0].palette_indices, [7; 24]);
        assert_eq!(
            rewritten.frames[1].palette_indices, original.frames[1].palette_indices,
            "the abutting frame's pixels survived"
        );
        assert_eq!(rewritten.frames[1].hotspots, original.frames[1].hotspots);
        assert_eq!(
            rewritten.frames[2].record_offset,
            rewritten.frames[1].record_offset
        );
        assert_eq!(rewritten.frames[3].source_frame, Some(1));
        assert_eq!(
            rewritten.frames[4].source_frame,
            Some(1),
            "the 0x04 record's payload pointer moved with the payload it names"
        );
        assert_eq!(
            rewritten.frames[1].pixels_offset.unwrap(),
            original.frames[1].pixels_offset.unwrap() - 16,
            "the record reached through two facings moved once, not twice"
        );
    }

    /// The writer re-parses what it is about to write and refuses it unless exactly one frame's
    /// pixels changed. That check cannot be reached through `write_frame_pixels` while the writer
    /// is correct, so every one of its comparisons is exercised here directly -- a comparison that
    /// is never wrong is indistinguishable from a comparison that was deleted.
    ///
    /// Each perturbation is applied **on its own**. Perturbing several at once cannot catch a
    /// comparison that was removed outright, because the assertion still sees an error.
    #[test]
    fn the_rewrite_verifier_compares_every_visible_property_on_its_own() {
        let source = tables_after_pixels_imp();
        let original = ImpSprite::parse(&source).unwrap();
        let pixels = original.frames[0].palette_indices.clone();
        assert!(
            check_only_the_named_frame_changed(&original, &original, 0, &pixels).is_ok(),
            "an unchanged sprite must pass"
        );

        type Perturbation = (&'static str, fn(&mut ImpSprite));
        let perturbations: [Perturbation; 13] = [
            ("the file header", |sprite| sprite.color_key ^= 1),
            ("the file header", |sprite| sprite.maximum_width += 1),
            ("the palette", |sprite| sprite.palette[5][0] ^= 0xff),
            ("the sequence table", |sprite| {
                sprite.sequences[0].metadata[6] ^= 0xff
            }),
            ("the facing table", |sprite| sprite.facings[0].metadata ^= 1),
            ("the frame count", |sprite| {
                let last = sprite.frames[0].clone();
                sprite.frames.push(last)
            }),
            ("frame 1's record", |sprite| {
                sprite.frames[1].origin_x = Some(-77)
            }),
            ("frame 1's pixels", |sprite| {
                sprite.frames[1].palette_indices[0] ^= 0xff
            }),
            ("frame 1's record", |sprite| sprite.frames[1].flags ^= 0x20),
            ("frame 1's record", |sprite| sprite.frames[1].width += 1),
            ("frame 1's record", |sprite| sprite.frames[1].height += 1),
            ("frame 1's record", |sprite| {
                sprite.frames[1].hotspots[0].x ^= 0x55
            }),
            ("frame 3's record", |sprite| {
                sprite.frames[3].source_frame = Some(0)
            }),
        ];
        for (what, perturb) in perturbations {
            let mut rewritten = original.clone();
            perturb(&mut rewritten);
            let message = check_only_the_named_frame_changed(&original, &rewritten, 0, &pixels)
                .expect_err(what)
                .to_string();
            assert!(message.contains(what), "expected {what}, got {message}");
        }

        // And the named frame is the one frame allowed to differ -- but only into the pixels it was
        // given, not into anything else.
        let mut rewritten = original.clone();
        rewritten.frames[0].palette_indices[0] ^= 0xff;
        assert!(check_only_the_named_frame_changed(&original, &rewritten, 0, &pixels).is_err());
        let changed = rewritten.frames[0].palette_indices.clone();
        assert!(check_only_the_named_frame_changed(&original, &rewritten, 0, &changed).is_ok());
    }

    /// The set of file positions the rewrite treats as absolute offsets, asserted directly.
    ///
    /// A whole-file test cannot reach the `0x08` exclusion: a back-reference's dword is a frame
    /// *index*, so it is a small number and sits below any payload offset, and the "is it past the
    /// replaced span" test skips it for the wrong reason. Shifting it would only corrupt a file
    /// whose frame count exceeds one of its own payload offsets. So the rule is asserted where it
    /// lives instead of inferred from an outcome that does not depend on it.
    #[test]
    fn the_offset_walk_lists_every_pointer_and_no_frame_index() {
        let source = tables_after_pixels_imp();
        let fields = absolute_offset_fields(&source).unwrap();
        let sequence_table = 1120_usize;
        let facing_table = 1136_usize;
        let frame_tables = [1176_usize, 1192, 1208, 1224];

        let mut expected = vec![
            8,                   // the palette pointer
            28,                  // the sequence-table pointer
            sequence_table + 12, // the sequence's facing-table pointer
        ];
        // One frame-table pointer per facing, including the two that name the same table.
        expected.extend((0..5).map(|facing| facing_table + facing * FACING_RECORD_SIZE + 4));
        expected.push(frame_tables[0] + 12); // frame 0's pixel pointer
        expected.push(frame_tables[1] + 8); // frame 1's hotspot-array pointer
        expected.push(frame_tables[1] + 12); // frame 1's pixel pointer
        expected.push(frame_tables[3] + 12); // the 0x04 record's pointer into frame 1's payload
        expected.sort_unstable();

        assert_eq!(fields, expected);
        // The one omission that matters, named rather than implied by the list above.
        assert!(
            !fields.contains(&(frame_tables[2] + 12)),
            "a 0x08 record's dword is a frame index, not a file offset"
        );
        assert!(
            fields.contains(&(frame_tables[3] + 12)),
            "a 0x04 record's dword is a real pointer and must move"
        );

        // A record carrying **both** flags is the case that tells the two apart. `ImpSprite::parse`
        // resolves it by payload offset -- `shared_pixels` is tested first and wins -- so its dword
        // is a pointer, and classifying it as a back reference because `0x08` is set would leave it
        // behind when the payload moves. 0x04 alone and 0x08 alone cannot distinguish the two
        // readings, so without this line the rule is only half asserted.
        let mut both_flags = source.clone();
        both_flags[frame_tables[3]] = FRAME_FLAG_SHARED_PIXELS | FRAME_FLAG_DUPLICATE;
        assert_eq!(
            ImpSprite::parse(&both_flags).unwrap().frames[4].source_frame,
            Some(1),
            "the parser reads both-flags as shared pixels, resolved by offset"
        );
        assert!(
            absolute_offset_fields(&both_flags)
                .unwrap()
                .contains(&(frame_tables[3] + 12)),
            "a 0x04|0x08 record's dword is still a pointer"
        );
        // Frame 0 carries an origin pair rather than a hotspot array, so its record has no pointer
        // in that dword either.
        assert!(!fields.contains(&(frame_tables[0] + 8)));
    }

    /// Nothing but the frame's own record is supposed to address the inside of its pixels. If
    /// something does, neither end of the replaced span is the right place to move it to, so the
    /// writer says so rather than picking one.
    #[test]
    fn a_rewrite_refuses_an_offset_pointing_inside_the_replaced_payload() {
        let mut source = tables_after_pixels_imp();
        // Point frame 1's hotspot array into the middle of frame 0's payload.
        source[1192 + 8..1192 + 12].copy_from_slice(&40_u32.to_le_bytes());

        let message = write_frame_pixels(&source, 0, &[7; 24])
            .unwrap_err()
            .to_string();
        assert!(message.contains("addresses 40"), "{message}");
        assert!(
            message.contains("inside the payload being replaced"),
            "{message}"
        );
    }

    /// A frame big enough that detailed art will not fit its record's 16-bit `encoded_size`.
    ///
    /// 300x220 at 8bpp is 66,000 pixels. Stored blank it costs 1,016 RLE bytes; stored as pixels
    /// with no runs in them it costs 66,516, which does not fit the field. This is not a contrived
    /// limit -- it is the first thing a modder hits importing photographic detail into a large
    /// frame, and truncating the declaration would write a file the engine reads as a shorter,
    /// wrong frame.
    fn big_frame_imp() -> (Vec<u8>, Vec<u8>) {
        let (width, height) = (300_u16, 220_u16);
        let pixel_count = usize::from(width) * usize::from(height);
        let palette_offset = 72_usize;
        let pixels_offset = palette_offset + PALETTE_BYTES;
        let mut source = vec![0_u8; pixels_offset];
        source[0] = FILE_FLAG_RLE;
        source[2] = 1;
        source[4..6].copy_from_slice(&width.to_le_bytes());
        source[6..8].copy_from_slice(&height.to_le_bytes());
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&width.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&height.to_le_bytes());
        let blank = encode_rle(&vec![0_u8; pixel_count]);
        assert!(blank.len() < 65_536, "the blank frame must fit the field");
        source[56 + 6..56 + 8].copy_from_slice(&(blank.len() as u16).to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&(pixels_offset as u32).to_le_bytes());
        source.extend_from_slice(&blank);

        // Pixels with no run of three anywhere, so every packet is a literal.
        let detailed: Vec<u8> = (0..pixel_count).map(|index| (index % 251) as u8).collect();
        assert!(
            encode_rle(&detailed).len() > 65_535,
            "the replacement must overflow the field"
        );
        (source, detailed)
    }

    #[test]
    fn writing_pixels_refuses_a_payload_too_long_for_the_records_declared_size() {
        let (source, detailed) = big_frame_imp();
        let message = write_frame_pixels(&source, 0, &detailed)
            .unwrap_err()
            .to_string();
        assert!(message.contains("16-bit encoded size"), "{message}");

        // The same frame accepts anything that does fit, so the refusal is about the length rather
        // than about the frame being large.
        let mut modest = vec![0_u8; detailed.len()];
        modest[0] = 9;
        assert!(write_frame_pixels(&source, 0, &modest).is_ok());
    }

    /// A record may declare 0x0. The corpus has none — `--imp-roundtrip` reports `empty-frames 0` —
    /// but the parser accepts them, so the writer has to say something specific rather than fall
    /// through to the zero-length-payload message, which would send the caller looking at the
    /// payload when the problem is the record.
    #[test]
    fn writing_pixels_refuses_an_empty_frame_by_name() {
        let mut source = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
        source[56 + 2..56 + 4].copy_from_slice(&0_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&0_u16.to_le_bytes());

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!((sprite.frames[0].width, sprite.frames[0].height), (0, 0));
        assert!(sprite.frames[0].palette_indices.is_empty());

        let message = write_frame_pixels(&source, 0, &[]).unwrap_err().to_string();
        assert!(message.contains("is empty"), "{message}");
        assert!(
            !message.contains("zero-length payload"),
            "the empty record must be named before its empty payload: {message}"
        );
    }

    /// A 1bpp frame may store *no* bytes at all: at 1x1 the tight-floor size is zero, and a
    /// `record_variant != 0` record is free to declare it. Four such frames are in `imp.mpq`.
    /// Their pixel pointer addresses a zero-length span, so there is nothing to splice and no way
    /// to tell where a replacement would go; the writer refuses rather than inventing a location.
    #[test]
    fn writing_pixels_refuses_a_frame_that_stores_no_bytes() {
        let palette_offset = 72_usize;
        let mut source = vec![0_u8; palette_offset + PALETTE_BYTES];
        source[0] = 0x10; // uncompressed, 1bpp
        source[2] = 1;
        source[4..6].copy_from_slice(&1_u16.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&1_u16.to_le_bytes());
        // The declared size is zero, which `packed_sizes` accepts at 1bpp as `tight_floor`.
        source[56 + 6..56 + 8].copy_from_slice(&0_u16.to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&(palette_offset as u32).to_le_bytes());

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames[0].packed_size, Some(0));
        assert_eq!(sprite.frames[0].stored_size, Some(0));
        let message = write_frame_pixels(&source, 0, &[1])
            .unwrap_err()
            .to_string();
        assert!(message.contains("zero-length payload"), "{message}");
    }

    /// A `record_variant == 0` compressed frame whose replacement payload the parser stops reading
    /// early. The writer decodes its own output through [`read_frame_pixels`] and refuses; without
    /// that check the file would be written with a payload the engine reads as a different, shorter
    /// frame -- the exact silent-wrong `a_variant_zero_record_can_stop_before_its_payload_ends`
    /// establishes is possible.
    ///
    /// The frame is 17x4 at 1bpp, which accepts 8, 9 and 12 packed bytes. Its stored payload is one
    /// 12-byte literal packet, so the parser reaches 12 in a single step and reads it as
    /// row-padded. A replacement whose first packet is a run of nine zeros lands on 9 instead, and
    /// the parser stops with three packed bytes and four stored bytes still to come.
    #[test]
    fn writing_pixels_refuses_a_payload_the_parser_would_stop_reading_early() {
        let palette_offset = 72_usize;
        let pixels_offset = palette_offset + PALETTE_BYTES;
        let mut source = vec![0_u8; pixels_offset];
        source[0] = FILE_FLAG_RLE | 0x10; // compressed, 1bpp
        source[2] = 0; // variant 0: the record declares no payload length
        source[4..6].copy_from_slice(&17_u16.to_le_bytes());
        source[6..8].copy_from_slice(&4_u16.to_le_bytes());
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&17_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&4_u16.to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&(pixels_offset as u32).to_le_bytes());
        // One literal packet of twelve bytes: control 0x100 - 12 = 0xF4.
        source.push(0xf4);
        source.extend_from_slice(&[
            0x5a, 0xa5, 0x3c, 0xc3, 0x0f, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
        ]);

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(packed_sizes(17, 4, 1).unwrap(), [8, 9, 12]);
        assert_eq!(sprite.frames[0].packed_size, Some(12), "read as row-padded");
        assert_eq!(sprite.frames[0].stored_size, Some(13));

        // Three blank rows then a full one: nine zero bytes, and nine is an acceptable size.
        let replacement: Vec<u8> = std::iter::repeat_n(0_u8, 51)
            .chain(std::iter::repeat_n(1_u8, 17))
            .collect();
        let message = write_frame_pixels(&source, 0, &replacement)
            .unwrap_err()
            .to_string();
        assert!(message.contains("reads back as"), "{message}");
        assert!(message.contains("9 bytes of packed pixels"), "{message}");
    }

    /// Two facings may point at one frame table whose record is a `0x08` back-reference. Both
    /// frames then have no payload of their own, so the payload comparison cannot see the sharing
    /// and only the record comparison can. This is the arm of `frames_sharing_pixels` that the
    /// payload arm does not subsume.
    #[test]
    fn frames_sharing_pixels_sees_two_facings_sharing_one_duplicate_record() {
        let palette_offset = 104_usize;
        let pixels_offset = palette_offset + PALETTE_BYTES;
        let mut source = vec![0_u8; palette_offset];
        source[2] = 1;
        source[4..6].copy_from_slice(&2_u16.to_le_bytes());
        source[6..8].copy_from_slice(&1_u16.to_le_bytes());
        source[8..12].copy_from_slice(&(palette_offset as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        // One sequence, three facings, at 48, 56 and 64.
        source[32 + 11] = 3;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        for (facing, table) in [(48_usize, 72_u32), (56, 88), (64, 88)] {
            source[facing + 2..facing + 4].copy_from_slice(&1_u16.to_le_bytes());
            source[facing + 4..facing + 8].copy_from_slice(&table.to_le_bytes());
        }
        // Record 72 is the one real frame; record 88 is a back-reference to frame 0, and facings 1
        // and 2 both point their tables at it.
        source[72 + 2..72 + 4].copy_from_slice(&2_u16.to_le_bytes());
        source[72 + 4..72 + 6].copy_from_slice(&1_u16.to_le_bytes());
        source[72 + 6..72 + 8].copy_from_slice(&2_u16.to_le_bytes());
        source[72 + 12..72 + 16].copy_from_slice(&(pixels_offset as u32).to_le_bytes());
        source[88] = FRAME_FLAG_DUPLICATE;
        source.resize(pixels_offset, 0);
        source.extend_from_slice(&[0xaa, 0xbb]);

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames.len(), 3);
        // Neither duplicate has a payload of its own, so the payload comparison is blind here.
        assert_eq!(sprite.frames[1].pixels_offset, None);
        assert_eq!(sprite.frames[2].pixels_offset, None);
        assert_eq!(sprite.frames_sharing_pixels(1).unwrap(), [2]);
        assert_eq!(sprite.frames_sharing_pixels(2).unwrap(), [1]);
        // And the real frame still sees both of them, through the duplicate chain.
        assert_eq!(sprite.frames_sharing_pixels(0).unwrap(), [1, 2]);
    }

    /// One 17x4 frame at 1bpp, compressed, with a payload the caller chooses.
    ///
    /// The one shape in this file where a stored size actually names a layout: acceptable sizes are
    /// `[8, 9, 12]`, so 9 is tight and 12 is row-padded. The 8bpp `writable_imp` fixture cannot test
    /// any of this -- at 8bpp the two layouts are the same bytes and every frame is `Identical`.
    fn ragged_1bpp_imp(record_variant: u8, packed: &[u8]) -> Vec<u8> {
        const PAYLOAD: usize = 72;
        const PALETTE: usize = 104;
        let payload = encode_rle(packed);
        assert!(payload.len() <= PALETTE - PAYLOAD, "payload must fit");
        let mut source = vec![0_u8; PALETTE + PALETTE_BYTES];
        source[0] = FILE_FLAG_RLE | 0x10;
        source[2] = record_variant;
        source[4..6].copy_from_slice(&17_u16.to_le_bytes());
        source[6..8].copy_from_slice(&4_u16.to_le_bytes());
        source[8..12].copy_from_slice(&(PALETTE as u32).to_le_bytes());
        source[26..28].copy_from_slice(&1_u16.to_le_bytes());
        source[28..32].copy_from_slice(&32_u32.to_le_bytes());
        source[32 + 11] = 1;
        source[32 + 12..32 + 16].copy_from_slice(&48_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[48 + 4..48 + 8].copy_from_slice(&56_u32.to_le_bytes());
        source[56 + 2..56 + 4].copy_from_slice(&17_u16.to_le_bytes());
        source[56 + 4..56 + 6].copy_from_slice(&4_u16.to_le_bytes());
        source[56 + 6..56 + 8].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        source[56 + 12..56 + 16].copy_from_slice(&(PAYLOAD as u32).to_le_bytes());
        source[PAYLOAD..PAYLOAD + payload.len()].copy_from_slice(&payload);
        source[PALETTE..PALETTE + 4].copy_from_slice(&[3, 2, 1, 0]);
        source
    }

    /// The writer must report the 752-frame class, not only the 50-frame ambiguous one.
    ///
    /// Both have the same consequence for an editor -- the pixels it exported may have been decoded
    /// under the wrong layout, and splicing a tight payload back over a row-padded one strands the
    /// original tail after the alignment padding -- and this one is sixteen times larger. Before
    /// this field the writer reported `layout: Tight` for all 752 as fact, with nothing beside it.
    #[test]
    fn the_writer_reports_a_layout_its_own_reader_could_not_have_observed() {
        // Nine distinct bytes: one literal packet, so the reader's only length test happens at 9.
        // It never passes through 8, and it can never reach the row-padded 12.
        let tight = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x80];
        let source = ragged_1bpp_imp(0, &tight);
        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames[0].packed_size, Some(9));

        let write =
            write_frame_pixels(&source, 0, &sprite.frames[0].palette_indices.clone()).unwrap();
        assert_eq!(write.bytes, source, "an identity rewrite must not move a byte");
        assert_eq!(write.layout, PixelLayout::Tight);
        assert!(
            !write.layout_ambiguous,
            "9 and 12 are different lengths, so the size does name a layout"
        );
        assert!(
            write.layout_unobservable,
            "a compressed variant-0 frame stopping at 9 could not have seen the row-padded 12"
        );

        // The same shape, the same reader, twelve bytes: a single 12-byte literal packet lands on
        // the row-padded size without ever passing through 8 or 9, so the layout WAS observed.
        // Each row's third byte carries one meaningful bit, which is why they are 0x80 and 0x00.
        let padded = [
            0x12, 0x34, 0x80, 0x56, 0x78, 0x00, 0x9a, 0xbc, 0x80, 0xde, 0xf0, 0x00,
        ];
        let source = ragged_1bpp_imp(0, &padded);
        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames[0].packed_size, Some(12));
        let write =
            write_frame_pixels(&source, 0, &sprite.frames[0].palette_indices.clone()).unwrap();
        assert_eq!(write.layout, PixelLayout::RowPadded);
        assert!(
            !write.layout_unobservable,
            "the decoder landed on the row-padded size, so nothing was hidden from it"
        );

        // A variant-1 record declares its length, so the reader never guesses where to stop.
        let declared = ragged_1bpp_imp(1, &tight);
        let write = write_frame_pixels(
            &declared,
            0,
            &ImpSprite::parse(&declared).unwrap().frames[0]
                .palette_indices
                .clone(),
        )
        .unwrap();
        assert!(!write.layout_unobservable);

        // And the 8bpp fixture, where the two layouts are the same bytes: neither flag fires.
        let flat = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
        let write = write_frame_pixels(&flat, 0, RUN).unwrap();
        assert!(!write.layout_ambiguous);
        assert!(!write.layout_unobservable);
    }

    /// A `record_variant != 0` record declares its payload length in 16 bits, so the declaration
    /// has to be rewritten with the payload.
    #[test]
    fn a_declared_payload_length_is_rewritten_with_the_payload() {
        let source = writable_imp(true, [RUN, LITERAL, OTHER_RUN]);
        assert_eq!(read_u16(&source, 56 + 6).unwrap(), 2);
        let write = write_frame_pixels(&source, 0, LITERAL).unwrap();
        assert_eq!(read_u16(&write.bytes, 56 + 6).unwrap(), 17);

        // A `record_variant == 0` record declares nothing, so the field must be left exactly as it
        // was found -- it is not this writer's to invent a meaning for.
        let mut variant_zero = writable_imp(false, [RUN, LITERAL, OTHER_RUN]);
        variant_zero[2] = 0;
        variant_zero[56 + 6..56 + 8].copy_from_slice(&0xbeef_u16.to_le_bytes());
        let write = write_frame_pixels(&variant_zero, 0, LITERAL).unwrap();
        assert_eq!(read_u16(&write.bytes, 56 + 6).unwrap(), 0xbeef);
        assert_eq!(write.shift, 0, "16 raw 8bpp pixels are always 16 bytes");
    }

    #[test]
    fn decodes_literal_and_repeated_rle_packets() {
        let source = [0x00, 0xaa, 0xfe, 0xbb, 0xcc];
        let decoded = decode_rle_exact(&source, &[5]).unwrap();
        assert_eq!(decoded, [0xaa, 0xaa, 0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn expands_packed_pixels_most_significant_bits_first() {
        assert_eq!(
            unpack_pixels(&[0b0001_1011], 4, 1, 2).unwrap(),
            [0, 1, 2, 3]
        );
        assert_eq!(unpack_pixels(&[0b1010_0000], 3, 1, 1).unwrap(), [1, 0, 1]);
        assert_eq!(unpack_pixels(&[0xab], 2, 1, 4).unwrap(), [0x0a, 0x0b]);
        assert_eq!(
            unpack_pixels(&[0b1010_1010], 11, 1, 1).unwrap(),
            [1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0]
        );
    }

    /// A synthetic sprite whose single frame carries two hotspot records instead of an origin.
    fn synthetic_imp_with_hotspots() -> Vec<u8> {
        let mut source = synthetic_imp();
        let hotspot_offset = source.len();
        source[56 + 1] = 2;
        source[56 + 8..56 + 12].copy_from_slice(&(hotspot_offset as u32).to_le_bytes());
        // type 0 at (1, -2), then type 7 at (3, -4)
        source.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0xfe, 0xff]);
        source.extend_from_slice(&[0x07, 0x00, 0x03, 0x00, 0xfc, 0xff]);
        source.resize(hotspot_offset + hotspot_bytes_for(2).unwrap(), 0);
        source
    }

    /// The rule measured in the running engine on 2026-09-16, pinned against the four terrain
    /// sprites it was fitted to. Anchor (320, 180) was recovered from the same run.
    ///
    /// `teeth` is listed at its predicted top-left rather than the measured one: its row 155
    /// contained no changed pixels, so the capture bounded the frame one row low.
    #[test]
    fn frame_top_left_reproduces_the_measured_terrain_sprites() {
        let anchor = (320, 180);
        let cases = [
            ("orchard", (0_i16, -35_i16), 60_u16, 70_u16, (290, 110)),
            ("teeth", (0, -1), 52, 48, (294, 155)),
            ("palm1", (9, -20), 53, 53, (303, 134)),
            ("dtree", (-6, -12), 21, 34, (304, 151)),
        ];
        for (name, placement, width, height, expected) in cases {
            assert_eq!(
                frame_top_left(anchor, placement, width, height).unwrap(),
                expected,
                "{name} top-left"
            );
            assert_eq!(
                placement_for_top_left(anchor, expected, width, height).unwrap(),
                placement,
                "{name} inverse"
            );
        }
    }

    /// Odd sizes settle floor against ceil: palm1 is 53 wide and its silhouette measured exactly
    /// 53 columns from x=303, which ceil would have placed at 302.
    #[test]
    fn frame_top_left_halves_toward_zero_for_odd_sizes() {
        assert_eq!(
            frame_top_left((320, 180), (9, -20), 53, 53).unwrap(),
            (303, 134)
        );
        assert_ne!(
            frame_top_left((320, 180), (9, -20), 53, 53).unwrap(),
            (302, 133)
        );
    }

    #[test]
    fn writing_the_same_origin_back_reproduces_the_input_byte_for_byte() {
        let source = synthetic_imp();
        let original = ImpSprite::parse(&source).unwrap();
        let (x, y) = (
            original.frames[0].origin_x.unwrap(),
            original.frames[0].origin_y.unwrap(),
        );
        assert_eq!(write_frame_origin(&source, 0, x, y).unwrap(), source);
    }

    #[test]
    fn writing_an_origin_changes_only_that_records_four_bytes() {
        let source = synthetic_imp();
        let patched = write_frame_origin(&source, 0, -1234, 567).unwrap();

        assert_eq!(patched.len(), source.len());
        let differing: Vec<usize> = (0..source.len())
            .filter(|index| source[*index] != patched[*index])
            .collect();
        let record = ImpSprite::parse(&source).unwrap().frames[0].record_offset;
        assert!(
            differing.iter().all(|index| (record + 8..record + 12).contains(index)),
            "unexpected bytes changed: {differing:?}"
        );

        let reparsed = ImpSprite::parse(&patched).unwrap();
        assert_eq!(reparsed.frames[0].origin_x, Some(-1234));
        assert_eq!(reparsed.frames[0].origin_y, Some(567));
    }

    /// Regression for a review finding: a duplicate frame has no origin of its own, but the only
    /// guard used to be "does it have hotspots", so the write silently patched four bytes and the
    /// caller read back `None`.
    #[test]
    fn writing_an_origin_refuses_a_duplicate_frame() {
        let source = synthetic_imp_with_record_array();

        let message = write_frame_origin(&source, 1, -1234, 567)
            .unwrap_err()
            .to_string();
        assert!(message.contains("duplicate of frame 0"), "{message}");
        // Both real frames still accept a write, including the one that follows the `0x04`
        // record inside the same facing.
        assert!(write_frame_origin(&source, 0, -1234, 567).is_ok());
        assert!(write_frame_origin(&source, 2, -1234, 567).is_ok());
    }

    /// Regression for a review finding: these overflowed silently in a release build, printing a
    /// wrapped value that looked like a real answer. That is why the release-only probe missed it.
    #[test]
    fn placement_arithmetic_refuses_to_wrap() {
        assert!(placement_for_top_left((i32::MIN, 0), (i32::MAX, 0), 61, 61).is_err());
        assert!(frame_top_left((i32::MAX, 0), (32767, 0), 0, 0).is_err());
        assert!(frame_top_left((i32::MIN, 0), (-32768, 0), 0, 0).is_err());
        // The ordinary range is unaffected.
        assert_eq!(frame_top_left((320, 180), (0, -35), 60, 70).unwrap(), (290, 110));
    }

    /// Two distinct frame records may store the same hotspot array pointer, which
    /// `frames_sharing_record` cannot see. Not present in shipped GS5R3 art, but the writer
    /// documents an aliasing guarantee, so it has to hold for files we did not author.
    #[test]
    fn frames_sharing_hotspots_sees_arrays_shared_across_distinct_records() {
        let mut source = synthetic_imp();
        source.splice(56..56, [0_u8; FRAME_RECORD_SIZE]);
        source[8..12].copy_from_slice(&80_u32.to_le_bytes());
        source[48 + 2..48 + 4].copy_from_slice(&2_u16.to_le_bytes());
        // Two independent frame records, same pixels, same hotspot array.
        for record in [56_usize, 72] {
            source[record + 1] = 2;
            source[record + 2..record + 4].copy_from_slice(&2_u16.to_le_bytes());
            source[record + 4..record + 6].copy_from_slice(&1_u16.to_le_bytes());
            source[record + 6..record + 8].copy_from_slice(&2_u16.to_le_bytes());
        }
        let hotspot_offset = source.len() + PALETTE_BYTES + 2;
        for record in [56_usize, 72] {
            source[record + 8..record + 12]
                .copy_from_slice(&(hotspot_offset as u32).to_le_bytes());
        }
        let palette_offset = source.len();
        source[56 + 12..56 + 16].copy_from_slice(&((palette_offset + PALETTE_BYTES) as u32).to_le_bytes());
        source[72 + 12..72 + 16].copy_from_slice(&((palette_offset + PALETTE_BYTES) as u32).to_le_bytes());
        source.resize(palette_offset + PALETTE_BYTES, 0);
        source.extend_from_slice(&[0xaa, 0xbb]);
        source.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0xfe, 0xff]);
        source.extend_from_slice(&[0x07, 0x00, 0x03, 0x00, 0xfc, 0xff]);
        source.resize(hotspot_offset + hotspot_bytes_for(2).unwrap(), 0);

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames.len(), 2);
        assert_eq!(sprite.frames_sharing_record(0).unwrap(), [0]);
        assert_eq!(sprite.frames_sharing_hotspots(0).unwrap(), [0, 1]);
    }

    #[test]
    fn frames_sharing_hotspots_is_empty_for_an_origin_frame() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        assert!(sprite.frames_sharing_hotspots(0).unwrap().is_empty());
    }

    #[test]
    fn writing_an_origin_refuses_a_frame_that_carries_hotspot_records() {
        let source = synthetic_imp_with_hotspots();
        let message = write_frame_origin(&source, 0, 1, 2).unwrap_err().to_string();
        assert!(message.contains("hotspot records"), "{message}");
    }

    #[test]
    fn writing_a_hotspot_edits_the_selected_type_and_leaves_the_others_alone() {
        let source = synthetic_imp_with_hotspots();
        let patched = write_frame_hotspot(&source, 0, 7, -9, 11).unwrap();

        assert_eq!(patched.len(), source.len());
        let reparsed = ImpSprite::parse(&patched).unwrap();
        let spots = &reparsed.frames[0].hotspots;
        assert_eq!((spots[0].id, spots[0].x, spots[0].y), (0, 1, -2));
        assert_eq!((spots[1].id, spots[1].x, spots[1].y), (7, -9, 11));
    }

    #[test]
    fn writing_a_hotspot_reports_the_types_present_when_the_type_is_absent() {
        let source = synthetic_imp_with_hotspots();
        let message = write_frame_hotspot(&source, 0, 42, 0, 0)
            .unwrap_err()
            .to_string();
        assert!(message.contains("[0, 7]"), "{message}");
    }

    #[test]
    fn writing_a_hotspot_refuses_a_frame_that_carries_an_origin_pair() {
        let source = synthetic_imp();
        let message = write_frame_hotspot(&source, 0, 0, 1, 2)
            .unwrap_err()
            .to_string();
        assert!(message.contains("origin pair"), "{message}");
    }

    /// Two facings may point their frame tables at the *same* offset, so one 16-byte record backs
    /// more than one logical frame and an edit through either index is an edit through both.
    /// Callers have to be told. (Records within a single facing do not alias: that table is an
    /// array — see `a_facing_frame_table_is_an_array_of_records`.)
    #[test]
    fn frames_sharing_record_reports_records_backing_more_than_one_frame() {
        let mut source = synthetic_imp_with_record_array();
        // Point facing 1 at facing 0's one-record table instead of its own.
        source[56 + 2..56 + 4].copy_from_slice(&1_u16.to_le_bytes());
        source[56 + 4..56 + 8].copy_from_slice(&64_u32.to_le_bytes());

        let sprite = ImpSprite::parse(&source).unwrap();
        assert_eq!(sprite.frames.len(), 2);
        assert_eq!(sprite.frames[0].record_offset, 64);
        assert_eq!(sprite.frames[1].record_offset, 64);
        assert_eq!(sprite.frames_sharing_record(0).unwrap(), [0, 1]);
        assert_eq!(sprite.frames_sharing_record(1).unwrap(), [0, 1]);
    }

    #[test]
    fn decodes_hotspot_id_and_signed_offsets_without_discarding_raw_bytes() {
        let source = [0x07, 0x00, 0xfc, 0xff, 0xe0, 0xff];
        let hotspots = parse_hotspots(&source, 0, 1).unwrap();

        assert_eq!(hotspots[0].id, 7);
        assert_eq!(hotspots[0].x, -4);
        assert_eq!(hotspots[0].y, -32);
        assert_eq!(hotspots[0].raw, source);
    }

    /// Statistics that agree with [`synthetic_imp`] on every field, so a test can perturb one.
    fn synthetic_stats() -> ImpHeaderStats {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        ImpHeaderStats {
            sequence_name: "synthetic".to_owned(),
            sequence_labels: Vec::new(),
            sequence_count: sprite.sequence_count,
            frame_count: sprite.frame_count,
            duplicate_frame_count: sprite.duplicate_frame_count,
            raw_pixel_bytes: sprite.raw_pixel_bytes,
            hotspot_bytes: sprite.hotspot_bytes,
            compressed_pixel_bytes: Some(sprite.stored_pixel_bytes),
        }
    }

    #[test]
    fn matching_statistics_produce_no_disagreements() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        assert_eq!(sprite.disagreements(&synthetic_stats()), []);
        assert!(sprite.validate_against(&synthetic_stats()).is_ok());
    }

    /// The old implementation used `?` on each comparison in a fixed order, so a file that
    /// disagreed on four statistics reported one. That hid the evidence that separates a stale
    /// header from a decoder bug, so every disagreement has to come back — and every statistic
    /// has to be compared at all. Perturbing only some of the six let a deleted comparison block
    /// survive the suite.
    #[test]
    fn disagreements_reports_every_statistic_not_only_the_first() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        let mut stats = synthetic_stats();
        stats.sequence_count += 1;
        stats.frame_count += 5;
        stats.duplicate_frame_count += 2;
        stats.raw_pixel_bytes += 7;
        stats.hotspot_bytes += 9;
        stats.compressed_pixel_bytes = Some(sprite.stored_pixel_bytes + 11);

        let found = sprite.disagreements(&stats);
        assert_eq!(
            found
                .iter()
                .map(|item| item.statistic)
                .collect::<Vec<ImpStatistic>>(),
            [
                ImpStatistic::SequenceCount,
                ImpStatistic::FrameCount,
                ImpStatistic::DuplicateFrameCount,
                ImpStatistic::RawPixelBytes,
                ImpStatistic::HotspotBytes,
                ImpStatistic::StoredPixelBytes,
            ]
        );
        let expected = [
            (sprite.sequence_count as u64, sprite.sequence_count as u64 + 1),
            (sprite.frame_count as u64, sprite.frame_count as u64 + 5),
            (
                sprite.duplicate_frame_count as u64,
                sprite.duplicate_frame_count as u64 + 2,
            ),
            (sprite.raw_pixel_bytes, sprite.raw_pixel_bytes + 7),
            (sprite.hotspot_bytes, sprite.hotspot_bytes + 9),
            (sprite.stored_pixel_bytes, sprite.stored_pixel_bytes + 11),
        ];
        for (item, (binary, header)) in found.iter().zip(expected) {
            assert_eq!((item.binary, item.header), (binary, header), "{}", item.statistic);
        }
    }

    /// Each statistic must be compared *independently*: perturbing one alone has to surface it.
    /// Perturbing several at once cannot catch a comparison that was deleted outright, because
    /// the assertion still sees a nonempty list.
    #[test]
    fn every_statistic_is_compared_on_its_own() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        type Perturbation = (ImpStatistic, fn(&mut ImpHeaderStats));
        let perturbations: [Perturbation; 6] = [
            (ImpStatistic::SequenceCount, |stats| stats.sequence_count += 1),
            (ImpStatistic::FrameCount, |stats| stats.frame_count += 1),
            (ImpStatistic::DuplicateFrameCount, |stats| {
                stats.duplicate_frame_count += 1
            }),
            (ImpStatistic::RawPixelBytes, |stats| stats.raw_pixel_bytes += 1),
            (ImpStatistic::HotspotBytes, |stats| stats.hotspot_bytes += 1),
            (ImpStatistic::StoredPixelBytes, |stats| {
                stats.compressed_pixel_bytes = stats.compressed_pixel_bytes.map(|bytes| bytes + 1)
            }),
        ];
        for (statistic, perturb) in perturbations {
            let mut stats = synthetic_stats();
            perturb(&mut stats);
            let found = sprite.disagreements(&stats);
            assert_eq!(
                found.iter().map(|item| item.statistic).collect::<Vec<_>>(),
                [statistic],
                "{statistic} is not compared on its own"
            );
        }
    }

    #[test]
    fn a_header_without_a_compression_statistic_does_not_compare_stored_bytes() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        let mut stats = synthetic_stats();
        stats.compressed_pixel_bytes = None;
        assert_eq!(sprite.disagreements(&stats), []);
    }

    #[test]
    fn the_error_message_names_every_disagreement() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        let mut stats = synthetic_stats();
        stats.sequence_count += 1;
        stats.hotspot_bytes += 16;

        let message = sprite.validate_against(&stats).unwrap_err().to_string();
        assert!(message.contains("sequence count mismatch"), "{message}");
        assert!(message.contains("hotspot bytes mismatch"), "{message}");
    }

    #[test]
    fn normalize_imp_member_lowercases_and_forward_slashes() {
        assert_eq!(
            normalize_imp_member("Units\\IMP\\OrCr4b"),
            "units/imp/orcr4b"
        );
    }

    /// The exception table is a waiver of *specific measured numbers*, never of a check. If the
    /// decoder starts reporting something else, the waiver has to stop applying.
    #[test]
    fn an_exception_covers_only_the_exact_recorded_disagreements() {
        let exception = imp_validation_exception("units/imp/orcr4b").expect("orcr4b is excepted");
        assert!(exception.covers(exception.waived));

        let mut altered = exception.waived.to_vec();
        altered[0].binary += 1;
        assert!(
            !exception.covers(&altered),
            "a changed measurement must re-fail"
        );

        let truncated = &exception.waived[..exception.waived.len() - 1];
        assert!(
            !exception.covers(truncated),
            "a disappearing disagreement must re-fail"
        );

        let mut extra = exception.waived.to_vec();
        extra.push(ImpDisagreement {
            statistic: ImpStatistic::SequenceCount,
            binary: 7,
            header: 6,
        });
        assert!(
            !exception.covers(&extra),
            "a new disagreement must re-fail"
        );

        assert!(!exception.covers(&[]), "an exact pair must not be excepted");
    }

    #[test]
    fn an_unlisted_member_has_no_exception() {
        assert!(imp_validation_exception("units/imp/orcr4a").is_none());
        assert!(imp_validation_exception("UNITS/IMP/ORCR4B").is_none());
    }

    /// Each class makes a falsifiable claim about the shape of the disagreement. Keeping the
    /// table honest to those claims is what stops it becoming a list of shrugs.
    #[test]
    fn every_exception_matches_the_shape_its_class_claims() {
        for exception in IMP_VALIDATION_EXCEPTIONS {
            let statistics: Vec<ImpStatistic> =
                exception.waived.iter().map(|item| item.statistic).collect();
            assert!(
                !exception.waived.is_empty(),
                "{} waives nothing",
                exception.member
            );
            assert!(
                !statistics.contains(&ImpStatistic::SequenceCount),
                "{} waives the sequence count; no member in the archive does",
                exception.member
            );
            // `covers` compares the waived slice element by element, so a correctly measured
            // member whose disagreements were listed out of order would silently re-fail. The
            // decoder emits them in `ImpStatistic` order, so the table must be written that way.
            let mut sorted = statistics.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                statistics, sorted,
                "{} lists its waived statistics out of ImpStatistic order, which `covers` cannot match",
                exception.member
            );
            match exception.class {
                ImpExceptionClass::HeaderPredatesArtRevision => {
                    assert_eq!(
                        statistics,
                        [ImpStatistic::RawPixelBytes, ImpStatistic::StoredPixelBytes],
                        "{} claims the structure agrees, so only pixel totals may differ",
                        exception.member
                    );
                }
                ImpExceptionClass::HeaderDescribesAnotherBuild => {
                    assert!(
                        statistics.contains(&ImpStatistic::FrameCount),
                        "{} claims another build, which has to show in the frame count",
                        exception.member
                    );
                }
            }
        }
    }

    #[test]
    fn the_exception_and_orphan_tables_are_normalized_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for exception in IMP_VALIDATION_EXCEPTIONS {
            assert_eq!(
                exception.member,
                normalize_imp_member(exception.member),
                "{} is not a normalized member stem",
                exception.member
            );
            assert!(!exception.member.ends_with(".imp"));
            assert!(
                seen.insert(exception.member),
                "{} is listed twice",
                exception.member
            );
            assert!(exception.reason.len() > 40, "{} has a stub reason", exception.member);
        }
        let mut orphans = std::collections::BTreeSet::new();
        for note in IMP_ORPHAN_NOTES {
            assert_eq!(note.member, normalize_imp_member(note.member));
            assert!(
                note.member.ends_with(".imp") || note.member.ends_with(".h"),
                "{} should name the member including its extension",
                note.member
            );
            assert!(orphans.insert(note.member), "{} is listed twice", note.member);
            assert!(note.reason.len() > 40, "{} has a stub reason", note.member);
            assert!(
                match note.facts {
                    ImpOrphanFacts::Header { .. } => note.member.ends_with(".h"),
                    ImpOrphanFacts::Sprite { .. } => note.member.ends_with(".imp"),
                },
                "{} pins facts of the wrong member kind",
                note.member
            );
        }
        assert!(imp_orphan_note("imp/fleemarka.imp").is_some());
        assert!(imp_orphan_note("imp/fleemark.h").is_none());
    }

    fn orphan_header_text(frames: usize) -> Vec<u8> {
        format!(
            "// Sprite headers for sequence spl01ea\r\n\
             // Total number of 'Sequences': 1\r\n\
             // Total number of 'Frames': {frames}\r\n\
             // Duplicate bitmaps found : 0\r\n\
             // Bitmap raw memory usage : 5597\r\n\
             // Hotspot raw memory usage : 0\r\n\
             // Bitmap RLE memory usage : 2065\r\n"
        )
        .into_bytes()
    }

    fn orphan_header_note() -> ImpOrphanNote {
        ImpOrphanNote {
            member: "aura/lsp01ea.h",
            reason: "fixture copy of the catalogued stray header, with the same pinned values",
            facts: ImpOrphanFacts::Header {
                sequence_name: "spl01ea",
                sequence_count: 1,
                frame_count: 9,
                duplicate_frame_count: 0,
                raw_pixel_bytes: 5_597,
                hotspot_bytes: 0,
                compressed_pixel_bytes: Some(2_065),
            },
        }
    }

    /// The validator used to accept an orphan on its *name* alone, never reading the member. A
    /// truncated or substituted file at the catalogued name passed the whole corpus run.
    #[test]
    fn an_orphan_note_accepts_only_a_member_that_measures_what_it_pins() {
        let note = orphan_header_note();
        assert!(note.verify(&orphan_header_text(9)).is_ok());

        let message = note.verify(&orphan_header_text(10)).unwrap_err().to_string();
        assert!(message.contains("frame count measured 10"), "{message}");
        assert!(message.contains("aura/lsp01ea.h"), "{message}");
    }

    #[test]
    fn an_orphan_note_rejects_a_member_it_cannot_parse() {
        let note = orphan_header_note();
        let message = note.verify(b"not a generated header").unwrap_err().to_string();
        assert!(message.contains("no sequence name"), "{message}");
    }

    #[test]
    fn an_orphan_sprite_note_measures_the_decoded_file() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        let note = ImpOrphanNote {
            member: "imp/synthetic.imp",
            reason: "fixture note pinning the synthetic sprite's decoded statistics exactly",
            facts: ImpOrphanFacts::Sprite {
                sequence_count: sprite.sequence_count,
                frame_count: sprite.frame_count,
                duplicate_frame_count: sprite.duplicate_frame_count,
                raw_pixel_bytes: sprite.raw_pixel_bytes,
                hotspot_bytes: sprite.hotspot_bytes,
                stored_pixel_bytes: sprite.stored_pixel_bytes,
            },
        };
        assert!(note.verify(&synthetic_imp()).is_ok());

        let mut truncated = synthetic_imp();
        truncated.truncate(truncated.len() - 1);
        assert!(note.verify(&truncated).is_err());
    }

    #[test]
    fn imp_member_basename_splits_on_either_separator() {
        assert_eq!(imp_member_basename("units\\imp\\orcr4b"), "orcr4b");
        assert_eq!(imp_member_basename("units/imp/orcr4b"), "orcr4b");
        assert_eq!(imp_member_basename("orcr4b"), "orcr4b");
    }

    #[test]
    fn parses_generated_header_statistics() {
        let header = b"// Sprite headers for sequence dragon\r\n\
//Facing-name defines\r\n\
#define DRAGON_MOVE 0\r\n\
#define DRAGON_STAND 1\r\n\
#define DRAGON_IDLE 1\r\n\
// Total number of 'Sequences': 2\r\n\
// Total number of 'Frames': 45\r\n\
// Duplicate bitmaps found : 3\r\n\
// Bitmap raw memory usage : 144878\r\n\
// Hotspot raw memory usage : 16\r\n\
// Bitmap RLE memory usage : 60875\r\n";
        let stats = ImpHeaderStats::parse(header).unwrap();
        assert_eq!(stats.sequence_name, "dragon");
        assert_eq!(
            stats.sequence_labels,
            [
                vec!["MOVE".to_owned()],
                vec!["STAND".to_owned(), "IDLE".to_owned()]
            ]
        );
        assert_eq!(stats.sequence_count, 2);
        assert_eq!(stats.frame_count, 45);
        assert_eq!(stats.duplicate_frame_count, 3);
        assert_eq!(stats.raw_pixel_bytes, 144_878);
        assert_eq!(stats.hotspot_bytes, 16);
        assert_eq!(stats.compressed_pixel_bytes, Some(60_875));
    }

    /// Encoder tests hold the *decoder* fixed and ask whether the encoder's output survives it.
    /// Asserting against a literal byte string the encoder itself produced would assert nothing
    /// about the format, only that the encoder is deterministic.
    fn rle_survives(packed: &[u8]) {
        let encoded = encode_rle(packed);
        let decoded = decode_rle_exact(&encoded, &[packed.len()]).unwrap();
        assert_eq!(decoded, packed, "encoded as {encoded:x?}");
    }

    #[test]
    fn rle_repeats_span_the_whole_expressible_range_and_split_past_it() {
        // Control 0x00 is the shortest repeat and 0x7F the longest, so 3 and 130 are the ends of
        // the range and 2 and 131 are the first values outside it on either side.
        for run in [1, 2, MIN_RLE_REPEAT - 1, MIN_RLE_REPEAT, MAX_RLE_REPEAT] {
            rle_survives(&vec![0x5a_u8; run]);
        }
        assert_eq!(encode_rle(&[0x5a; MAX_RLE_REPEAT]), [0x7f, 0x5a]);
        // One past the maximum cannot be one packet, and the remainder is two bytes -- too short
        // for a repeat of its own -- so it must come back as a repeat followed by a literal.
        let over = encode_rle(&[0x5a; MAX_RLE_REPEAT + 1]);
        assert_eq!(over, [0x7f, 0x5a, 0xff, 0x5a]);
        rle_survives(&[0x5a_u8; MAX_RLE_REPEAT + 1]);
        rle_survives(&[0x5a_u8; MAX_RLE_REPEAT * 2]);
        // A run of exactly two never earns a repeat packet: it costs the same inside a literal.
        assert_eq!(encode_rle(&[1, 2, 2, 3]), [0xfc, 1, 2, 2, 3]);
    }

    #[test]
    fn rle_literals_span_the_whole_expressible_range_and_split_past_it() {
        // Control 0xFF is a literal of one and 0x80 a literal of 128, so 128 is the longest one
        // packet can carry and 129 is the first length that must split.
        let ramp: Vec<u8> = (0..MAX_RLE_LITERAL).map(|index| index as u8).collect();
        assert_eq!(encode_rle(&ramp)[0], 0x80);
        assert_eq!(encode_rle(&ramp).len(), 1 + MAX_RLE_LITERAL);
        rle_survives(&ramp);

        // 129 distinct bytes: a full literal plus a literal of one. The ramp above wraps at 256,
        // so it is built to stay distinct rather than reused.
        let long: Vec<u8> = (0..=MAX_RLE_LITERAL).map(|index| index as u8).collect();
        let encoded = encode_rle(&long);
        assert_eq!(encoded[0], 0x80);
        assert_eq!(encoded[1 + MAX_RLE_LITERAL], 0xff);
        assert_eq!(encoded.len(), 1 + MAX_RLE_LITERAL + 2);
        rle_survives(&long);
    }

    #[test]
    fn rle_survives_every_shape_a_frame_can_take() {
        rle_survives(&[]);
        rle_survives(&[0]);
        rle_survives(&[0xff]);
        // A run that ends the buffer, a run that starts it, and alternating bytes that can never
        // form a run at all.
        rle_survives(&[1, 2, 3, 4, 4, 4, 4]);
        rle_survives(&[4, 4, 4, 4, 1, 2, 3]);
        let alternating: Vec<u8> = (0..1000).map(|index| (index % 2) as u8).collect();
        rle_survives(&alternating);
        // Deterministic pseudo-random bytes: runs and literals interleave at every length.
        let mut state = 0x1234_5678_u32;
        let noisy: Vec<u8> = (0..5000)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                ((state >> 16) % 4) as u8
            })
            .collect();
        rle_survives(&noisy);
    }

    #[test]
    fn empty_rle_encodes_to_nothing() {
        assert!(encode_rle(&[]).is_empty());
    }

    /// Packs and unpacks at every depth, asserting the pixels survive and the length is the one
    /// asked for.
    fn packing_survives(indices: &[u8], width: u16, height: u16, bits_per_pixel: u8) {
        for packed_size in packed_sizes(width, height, bits_per_pixel).unwrap() {
            let packed = pack_pixels(indices, width, height, bits_per_pixel, packed_size).unwrap();
            assert_eq!(
                packed.len(),
                packed_size,
                "{width}x{height} at {bits_per_pixel}bpp"
            );
            let unpacked = unpack_pixels(&packed, width, height, bits_per_pixel).unwrap();
            let sizes = PackedSizes::for_frame(width, height, bits_per_pixel).unwrap();
            if packed_size == sizes.tight_floor && sizes.tight_floor < sizes.tight_ceil {
                // The dropped final byte is not recoverable, and the parser zero-fills it; only
                // the pixels the stored bytes actually cover can be claimed.
                let kept = packed_size * 8 / usize::from(bits_per_pixel);
                assert_eq!(unpacked[..kept], indices[..kept]);
                assert!(unpacked[kept..].iter().all(|index| *index == 0));
            } else {
                assert_eq!(unpacked, indices, "{width}x{height} at {bits_per_pixel}bpp");
            }
        }
    }

    #[test]
    fn packing_round_trips_at_every_depth() {
        for bits_per_pixel in [1, 2, 4, 8] {
            let mask = if bits_per_pixel == 8 {
                0xff
            } else {
                (1_u8 << bits_per_pixel) - 1
            };
            for (width, height) in [(1, 1), (1, 7), (7, 1), (3, 5), (8, 8), (17, 13), (64, 3)] {
                let indices: Vec<u8> = (0..usize::from(width) * usize::from(height))
                    .map(|index| (index as u8) & mask)
                    .collect();
                packing_survives(&indices, width, height, bits_per_pixel);
            }
        }
    }

    #[test]
    fn a_one_pixel_frame_packs_at_every_depth() {
        for bits_per_pixel in [1, 2, 4, 8] {
            let sizes = PackedSizes::for_frame(1, 1, bits_per_pixel).unwrap();
            assert_eq!(sizes.tight_ceil, 1);
            assert_eq!(sizes.row_padded, 1);
            let packed = pack_pixels(&[1], 1, 1, bits_per_pixel, 1).unwrap();
            // The pixel occupies the high bits and the rest of the byte is padding written as
            // zero, so the stored byte is the index shifted up rather than the index itself.
            assert_eq!(packed, [1_u8 << (8 - bits_per_pixel)]);
            assert_eq!(unpack_pixels(&packed, 1, 1, bits_per_pixel).unwrap(), [1]);
        }
    }

    #[test]
    fn an_empty_frame_packs_to_nothing_at_every_depth() {
        for bits_per_pixel in [1, 2, 4, 8] {
            assert_eq!(packed_sizes(0, 0, bits_per_pixel).unwrap(), [0]);
            let packed = pack_pixels(&[], 0, 0, bits_per_pixel, 0).unwrap();
            assert!(packed.is_empty());
        }
    }

    #[test]
    fn the_two_layouts_are_told_apart_only_when_their_lengths_differ() {
        // 8 pixels per byte at 1bpp: a width of 16 fills whole bytes, so the tight and row-padded
        // sizes coincide and the frame cannot record which it used. A width of 17 does not.
        let equal = PackedSizes::for_frame(16, 4, 1).unwrap();
        assert_eq!(equal.tight_ceil, equal.row_padded);
        assert_eq!(packed_sizes(16, 4, 1).unwrap(), [equal.tight_ceil]);

        let different = PackedSizes::for_frame(17, 4, 1).unwrap();
        assert_eq!(different.tight_ceil, 9);
        assert_eq!(different.row_padded, 12);
        assert_eq!(different.tight_floor, 8);
        assert_eq!(packed_sizes(17, 4, 1).unwrap(), [8, 9, 12]);

        // Same pixels, three lengths, and each one still reads back as the same picture. This is
        // the property a re-encoder depends on -- and the reason it must be *told* the length,
        // since all three are equally valid and only one keeps the frame its original size.
        let indices: Vec<u8> = (0..17 * 4).map(|index| (index % 2) as u8).collect();
        packing_survives(&indices, 17, 4, 1);
        let tight = pack_pixels(&indices, 17, 4, 1, 9).unwrap();
        let padded = pack_pixels(&indices, 17, 4, 1, 12).unwrap();
        assert_ne!(tight.len(), padded.len());
        assert_eq!(unpack_pixels(&tight, 17, 4, 1).unwrap(), indices);
        assert_eq!(unpack_pixels(&padded, 17, 4, 1).unwrap(), indices);
    }

    #[test]
    fn the_two_layouts_emit_the_same_bytes_when_they_are_the_same_length() {
        // Why three mutants of `pack_pixels`'s layout discriminator survived the sweep: they are
        // equivalent, not missed. The condition that makes them so is **a row's bits filling whole
        // bytes**, not the two sizes being equal -- 7x5 at 1bpp is five bytes under either layout
        // and they are still different bytes. Where the rows do align, the row boundaries are byte
        // boundaries and packing row by row is the same operation as packing straight through.
        // Asserted rather than argued, so the day `pack_bits` stops respecting row boundaries the
        // equivalence claim fails instead of quietly rotting.
        for bits_per_pixel in [1, 2, 4, 8] {
            let pixels_per_byte = 8 / usize::from(bits_per_pixel);
            let mask = if bits_per_pixel == 8 {
                0xff
            } else {
                (1_u8 << bits_per_pixel) - 1
            };
            let mut coinciding = 0;
            for width in 1..=64_u16 {
                if usize::from(width) % pixels_per_byte != 0 {
                    continue;
                }
                let sizes = PackedSizes::for_frame(width, 5, bits_per_pixel).unwrap();
                assert_eq!(sizes.tight_ceil, sizes.row_padded);
                coinciding += 1;
                let indices: Vec<u8> = (0..usize::from(width) * 5)
                    .map(|index| (index as u8).wrapping_mul(7) & mask)
                    .collect();
                let mut by_row = Vec::new();
                for row in indices.chunks_exact(usize::from(width)) {
                    pack_bits(row, bits_per_pixel, &mut by_row);
                }
                let mut straight = Vec::new();
                pack_bits(&indices, bits_per_pixel, &mut straight);
                assert_eq!(by_row, straight, "{width} wide at {bits_per_pixel}bpp");
            }
            // A zero here would make the loop above vacuous and the claim unearned.
            assert!(coinciding > 0, "no widths coincide at {bits_per_pixel}bpp");
        }
    }

    #[test]
    fn equal_packed_lengths_do_not_mean_equal_layouts() {
        // 7 pixels a row at 1bpp: one byte a row padded, and 35 bits -- five bytes -- run straight
        // through. The two layouts are the same *length* and different *bytes*, so a stored size of
        // five names neither, and `unpack_pixels`'s discriminator (`row_padded > tight`) is false
        // here and reads it as tight unconditionally.
        let sizes = PackedSizes::for_frame(7, 5, 1).unwrap();
        assert_eq!(sizes.tight_ceil, sizes.row_padded);
        assert_eq!(packed_sizes(7, 5, 1).unwrap(), [4, 5]);

        let indices: Vec<u8> = (0..35).map(|index| (index % 2) as u8).collect();
        let mut by_row = Vec::new();
        for row in indices.chunks_exact(7) {
            pack_bits(row, 1, &mut by_row);
        }
        let mut straight = Vec::new();
        pack_bits(&indices, 1, &mut straight);
        assert_eq!(by_row.len(), straight.len());
        assert_ne!(by_row, straight);

        // `pack_pixels` follows the decoder rather than the other layout, which is the only choice
        // that round-trips; a frame the original tool wrote row-padded at this shape is already
        // being decoded wrong, and that is a decoder question, not an encoder one.
        assert_eq!(pack_pixels(&indices, 7, 5, 1, 5).unwrap(), straight);
        assert_eq!(unpack_pixels(&straight, 7, 5, 1).unwrap(), indices);
    }

    #[test]
    fn at_8bpp_the_layouts_always_coincide() {
        // Every width, so this is a statement about the depth rather than about the sample.
        for width in 1..=u16::from(u8::MAX) {
            let sizes = PackedSizes::for_frame(width, 3, 8).unwrap();
            assert_eq!(sizes.tight_ceil, sizes.row_padded);
            assert_eq!(sizes.tight_ceil, sizes.tight_floor);
        }
    }

    #[test]
    fn the_layouts_coincide_on_whole_byte_rows_and_on_single_rows() {
        // Whole-byte rows: every width at 8bpp, and the multiples of 8 at 1bpp.
        for width in 1..=64_u16 {
            assert!(layouts_are_identical(width, 5, 8));
            assert_eq!(layouts_are_identical(width, 5, 1), width % 8 == 0);
            assert_eq!(layouts_are_identical(width, 5, 4), width % 2 == 0);
        }
        // A single row coincides whatever the width, and that is a separate reason -- 7 at 1bpp
        // fails the alignment test and still has nothing to displace.
        assert!(!layouts_are_identical(7, 5, 1));
        assert!(layouts_are_identical(7, 1, 1));
        assert!(layouts_are_identical(1, 1, 4));
        // Two rows is the first height where the padding has somewhere to go.
        assert!(!layouts_are_identical(7, 2, 1));
        // And no rows at all is not a disagreement either, so a frame with no pixels is never
        // reported ambiguous. The boundary is pinned rather than left to the comparison operator.
        assert!(layouts_are_identical(7, 0, 1));
        assert!(!layout_is_ambiguous(7, 0, 1).unwrap());
        assert!(layout_is_ambiguous(7, 2, 1).unwrap());
    }

    /// The rule, stated as a table rather than argued.
    #[test]
    fn the_layout_rule_names_row_padded_only_when_a_length_can_name_it() {
        // Case 1: the two layouts are the same bytes. Reported tight, and the reason is the shape,
        // not the length.
        assert_eq!(
            pixel_layout_for(200, 20, 10, 8).unwrap(),
            PixelLayout::Tight
        );
        assert_eq!(pixel_layout_for(1, 7, 1, 1).unwrap(), PixelLayout::Tight);

        // Case 2: the lengths differ, so the stored size names a layout. 17 pixels at 1bpp need 17
        // bits, so a row costs 3 bytes padded and the whole frame costs 9 tight.
        assert_eq!(
            pixel_layout_for(12, 17, 4, 1).unwrap(),
            PixelLayout::RowPadded
        );
        assert_eq!(pixel_layout_for(9, 17, 4, 1).unwrap(), PixelLayout::Tight);
        assert_eq!(pixel_layout_for(8, 17, 4, 1).unwrap(), PixelLayout::Tight);

        // Case 3: the ambiguous shape. 7 pixels a row at 1bpp over five rows costs five bytes
        // either way and the five bytes differ; tight is a choice, made here and nowhere else.
        assert!(layout_is_ambiguous(7, 5, 1).unwrap());
        assert_eq!(pixel_layout_for(5, 7, 5, 1).unwrap(), PixelLayout::Tight);
    }

    /// The ambiguity is a property of the *shape* and nothing else, so the predicate has to agree
    /// with the definition on a whole grid rather than on the handful of shapes a test picked.
    #[test]
    fn a_shape_is_ambiguous_exactly_when_the_layouts_differ_at_equal_length() {
        let mut ambiguous = 0;
        for bits_per_pixel in [1, 2, 4, 8] {
            for width in 1..=40_u16 {
                for height in 1..=8_u16 {
                    let sizes = PackedSizes::for_frame(width, height, bits_per_pixel).unwrap();
                    let expected = !layouts_are_identical(width, height, bits_per_pixel)
                        && sizes.tight_ceil == sizes.row_padded;
                    assert_eq!(
                        layout_is_ambiguous(width, height, bits_per_pixel).unwrap(),
                        expected,
                        "{width}x{height} at {bits_per_pixel}bpp"
                    );
                    ambiguous += usize::from(expected);
                }
            }
        }
        // A zero here would make every assertion above vacuous on the interesting side.
        assert!(ambiguous > 0, "no shape in the grid is ambiguous");
    }

    /// **What the rule does to a row-padded frame of an ambiguous shape: it reads it as tight, and
    /// the pixels come out scrambled.** That is not a defect to be fixed in the decoder -- the two
    /// layouts are the same length and the file records nothing that tells them apart, so no rule
    /// over the bytes can do better. It is written down as a test so the behaviour is a decision
    /// with a name rather than a surprise, and so the day external evidence settles the question
    /// this test is what has to change.
    #[test]
    fn a_row_padded_frame_of_an_ambiguous_shape_is_read_as_tight() {
        // 7x5 at 1bpp: 35 bits tight, five bytes; one byte a row padded, also five.
        let indices: Vec<u8> = (0..35).map(|index| (index % 2) as u8).collect();
        assert!(layout_is_ambiguous(7, 5, 1).unwrap());

        let mut row_padded = Vec::new();
        for row in indices.chunks_exact(7) {
            pack_bits(row, 1, &mut row_padded);
        }
        assert_eq!(row_padded.len(), 5);

        let read_back = unpack_pixels(&row_padded, 7, 5, 1).unwrap();
        assert_ne!(
            read_back, indices,
            "an ambiguous-shape frame written row-padded does not survive; that is the open question"
        );
        // And it is read as tight *specifically*: the stored bytes expanded straight through,
        // most significant bit first, with no row boundary anywhere. Asserted against the bits
        // rather than against another call to the decoder, which would only say it agrees with
        // itself.
        let straight_through: Vec<u8> = row_padded
            .iter()
            .flat_map(|byte| (0..8).map(move |bit| (byte >> (7 - bit)) & 1))
            .take(35)
            .collect();
        assert_eq!(read_back, straight_through);

        // The same pixels written tight do survive, so the loss is the layout and not the pixels.
        let mut tight = Vec::new();
        pack_bits(&indices, 1, &mut tight);
        assert_eq!(tight.len(), row_padded.len());
        assert_ne!(tight, row_padded);
        assert_eq!(unpack_pixels(&tight, 7, 5, 1).unwrap(), indices);
    }

    /// The rule replaced an inline length comparison duplicated in the packer and the unpacker. It
    /// has to agree with that comparison on every input, because the corpus was measured under the
    /// old one and 0 of 41,373 frames read row-padded; a rule that reclassified even one frame
    /// would invalidate that measurement rather than improve it.
    ///
    /// The equivalence holds because `layouts_are_identical` implies `row_padded == tight_ceil`, so
    /// the extra clause is already covered by the length test. Asserted rather than argued.
    #[test]
    fn the_layout_rule_agrees_with_the_length_comparison_it_replaced() {
        let mut row_padded_cases = 0;
        for bits_per_pixel in [1, 2, 4, 8] {
            for width in 1..=40_u16 {
                for height in 1..=8_u16 {
                    let sizes = PackedSizes::for_frame(width, height, bits_per_pixel).unwrap();
                    for packed_size in sizes.acceptable(bits_per_pixel) {
                        let old =
                            packed_size == sizes.row_padded && sizes.row_padded > sizes.tight_ceil;
                        let new = pixel_layout_for(packed_size, width, height, bits_per_pixel)
                            .unwrap()
                            == PixelLayout::RowPadded;
                        assert_eq!(
                            old, new,
                            "{width}x{height} at {bits_per_pixel}bpp, {packed_size} bytes"
                        );
                        row_padded_cases += usize::from(new);
                    }
                }
            }
        }
        assert!(row_padded_cases > 0, "no case in the grid reads row-padded");
    }

    #[test]
    fn packing_refuses_a_length_the_parser_would_not_accept() {
        // One byte short of the tight size at 8bpp, where nothing else is legal.
        assert!(pack_pixels(&[0; 12], 4, 3, 8, 11).is_err());
        assert!(pack_pixels(&[0; 12], 4, 3, 8, 13).is_err());
        // Right length, wrong number of pixels.
        assert!(pack_pixels(&[0; 11], 4, 3, 8, 12).is_err());
        // An index that does not fit the depth would silently corrupt its neighbours.
        assert_eq!(
            pack_pixels(&[0, 4, 0, 0], 2, 2, 2, 1)
                .unwrap_err()
                .to_string(),
            "IMP palette index 4 does not fit in 2 bits"
        );
        assert!(pack_pixels(&[0, 3, 0, 0], 2, 2, 2, 1).is_ok());
    }

    #[test]
    fn a_re_encoded_frame_reads_back_through_the_parsers_own_pixel_path() {
        let indices: Vec<u8> = (0..17 * 4).map(|index| (index % 3) as u8).collect();
        for compressed in [false, true] {
            for packed_size in [17, 20] {
                let packed = pack_pixels(&indices, 17, 4, 2, packed_size).unwrap();
                let payload = if compressed {
                    encode_rle(&packed)
                } else {
                    packed.clone()
                };
                // A declared-length record reads exactly what was written, at either layout.
                let (read, consumed) =
                    read_frame_pixels(&payload, 0, 17, 4, 2, compressed, 1, payload.len()).unwrap();
                assert_eq!(read, packed);
                assert_eq!(consumed, payload.len());
                assert_eq!(unpack_pixels(&read, 17, 4, 2).unwrap(), indices);
            }
        }
    }

    #[test]
    fn a_variant_zero_record_can_stop_before_its_payload_ends() {
        // This is not a hypothetical. A `record_variant == 0` record stores no payload length, so
        // its reader halts at the first size `packed_sizes` accepts, checked between packets. A
        // 17x4 1bpp frame accepts 8, 9 and 12, and a stream whose first packet lands on 9 is read
        // as a nine-byte tight frame no matter that twelve were written.
        let packed: Vec<u8> = std::iter::repeat_n(0_u8, 9)
            .chain(std::iter::repeat_n(1_u8, 3))
            .collect();
        assert_eq!(packed_sizes(17, 4, 1).unwrap(), [8, 9, 12]);
        let payload = encode_rle(&packed);
        let (read, consumed) =
            read_frame_pixels(&payload, 0, 17, 4, 1, true, 0, payload.len()).unwrap();
        assert_eq!(read.len(), 9);
        assert!(consumed < payload.len());
        assert_ne!(
            unpack_pixels(&read, 17, 4, 1).unwrap(),
            unpack_pixels(&packed, 17, 4, 1).unwrap()
        );
        // The same bytes under a declared length come back whole, which is what makes the halt a
        // property of the variant rather than of the encoder.
        let (whole, whole_consumed) =
            read_frame_pixels(&payload, 0, 17, 4, 1, true, 1, payload.len()).unwrap();
        assert_eq!(whole, packed);
        assert_eq!(whole_consumed, payload.len());
    }

    #[test]
    fn reading_pixels_refuses_an_empty_frame_rather_than_returning_nothing() {
        assert_eq!(
            read_frame_pixels(&[], 0, 0, 0, 8, false, 1, 0)
                .unwrap_err()
                .to_string(),
            "IMP frame has no pixels to read; empty frames carry no payload"
        );
    }

    #[test]
    fn parsed_frames_record_where_their_pixels_are_and_how_long_they_are() {
        let sprite = ImpSprite::parse(&synthetic_imp()).unwrap();
        let frame = &sprite.frames[0];
        // The 2x1 8bpp frame in the fixture stores two raw bytes right after the palette.
        assert_eq!(frame.packed_size, Some(2));
        assert_eq!(frame.stored_size, Some(2));
        let offset = frame.pixels_offset.unwrap();
        assert_eq!(&synthetic_imp()[offset..offset + 2], &[0xaa, 0xbb]);
        // Re-encoding that frame reproduces those bytes exactly.
        let packed = pack_pixels(&frame.palette_indices, 2, 1, 8, 2).unwrap();
        assert_eq!(packed, [0xaa, 0xbb]);
    }
    // -----------------------------------------------------------------------
    // The installed corpus
    // -----------------------------------------------------------------------
    //
    // Run with:
    //   LOM_GAME_DIR=.../English LOM_LISTFILE=.../lords-of-magic.txt \
    //     cargo test --release -- --ignored
    //
    // `LOM_LISTFILE` is not optional here, unlike the PBM sweep. `imp.mpq` carries no listfile and
    // an IMP is identified by its member name, so with StormLib's synthesised `File%08u.xxx` names
    // the probe recognises nothing and the sweep sees **zero** members. That is precisely the
    // silent-empty-run failure the tripwire below exists to catch, and it is reachable by simply
    // forgetting an environment variable.

    /// **Observed in the corpus, 2026-09-19.** IMP members in `imp.mpq`, identical in the stock
    /// Steam archive and in GS5R3's.
    const ARCHIVED_IMP_MEMBERS: usize = 1_800;

    /// **Observed in the corpus, 2026-09-19.** Payload-carrying frames across those members --
    /// frames that own their pixels, so neither duplicate records nor shared-pixel aliases.
    const IMP_PAYLOAD_FRAMES: usize = 41_373;

    /// **Observed in the corpus, 2026-09-19.** Records that alias another frame's pixels rather
    /// than owning any, counted separately because the whole-file rewrite refuses them by name.
    const IMP_DUPLICATE_FRAMES: usize = 10_293;

    /// **Observed in the corpus, 2026-09-19.** Frames whose own payload re-encodes to the exact
    /// bytes the archive stores. The remaining `41,373 - 39,108 = 2,265` differ only in this
    /// encoder's RLE packet boundaries; their pixels are unchanged, which is what
    /// `pixel_lossless == checked` asserts.
    const IMP_BYTE_IDENTICAL_PAYLOADS: usize = 39_108;

    /// **Observed in the corpus, 2026-09-19.** Frames the whole-file writer accepts. The
    /// difference from [`IMP_PAYLOAD_FRAMES`] is 3,443 refusals: 3,439 shared payloads and 4
    /// zero-length ones, refused and named rather than silently overwritten.
    const IMP_REWRITE_ATTEMPTED: usize = 37_930;

    /// **Observed in the corpus, 2026-09-19.** Whole-file rewrites that came back byte-identical.
    ///
    /// The headline `35,840 of 35,840` means this: of the attempted rewrites, every one whose
    /// payload re-encoded byte-identically produced a byte-identical *file*. The other
    /// `37,930 - 35,840 = 2,090` differ only because their payload differs, which the sweep
    /// separates out -- a file that changed while its payload did not is a writer defect and is a
    /// hard failure.
    const IMP_REWRITE_BYTE_IDENTICAL: usize = 35_840;

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

    fn open_imp_archive() -> crate::mpq::Archive {
        let archive =
            crate::mpq::Archive::open(&game_directory().join("imp.mpq")).expect("open imp.mpq");
        let listfile =
            std::env::var("LOM_LISTFILE").expect("set LOM_LISTFILE alongside LOM_GAME_DIR");
        let contents = std::fs::read(&listfile).expect("read the listfile");
        archive
            .add_listfile_contents(&contents)
            .expect("apply the listfile");
        archive
    }

    /// Every payload-carrying frame in `imp.mpq` re-encodes losslessly, and every frame the
    /// whole-file writer accepts rewrites to a byte-identical file unless its payload itself
    /// differs.
    ///
    /// This is the acceptance property `docs/imp-format.md` and the roadmap quote. Nothing
    /// synthetic reaches this bar: it exercises every record, every absolute pointer and every
    /// uninitialised leftover byte in 1,800 real files at once.
    #[test]
    #[ignore = "needs LOM_GAME_DIR and LOM_LISTFILE"]
    fn every_archived_imp_frame_round_trips() {
        let archive = open_imp_archive();
        let entries = archive.entries().expect("enumerate imp.mpq");

        let mut members = 0_usize;
        let mut frames = 0_usize;
        let mut duplicate_frames = 0_usize;
        let mut empty_frames = 0_usize;
        let mut pixel_lossless = 0_usize;
        let mut byte_identical_payloads = 0_usize;
        let mut rewrite_attempted = 0_usize;
        let mut rewrite_identical = 0_usize;
        let mut rewrite_differs_by_payload = 0_usize;
        let mut rewrite_refused = 0_usize;
        let mut failures = Vec::new();

        for entry in &entries {
            let Ok(bytes) = archive.read(&entry.name) else {
                failures.push(format!("{}: could not read", entry.name));
                continue;
            };
            if !matches!(
                crate::asset::probe(&entry.name, &bytes).map(|info| info.kind),
                Ok(crate::asset::AssetKind::ImpSprite)
            ) {
                continue;
            }
            let sprite = match ImpSprite::parse(&bytes) {
                Ok(sprite) => sprite,
                Err(error) => {
                    failures.push(format!("{}: {error}", entry.name));
                    continue;
                }
            };
            members += 1;

            for (index, frame) in sprite.frames.iter().enumerate() {
                let (Some(packed_size), Some(pixels_offset), Some(stored_size)) =
                    (frame.packed_size, frame.pixels_offset, frame.stored_size)
                else {
                    duplicate_frames += 1;
                    continue;
                };
                if frame.width == 0 || frame.height == 0 {
                    empty_frames += 1;
                    continue;
                }

                let packed = match pack_pixels(
                    &frame.palette_indices,
                    frame.width,
                    frame.height,
                    sprite.bits_per_pixel,
                    packed_size,
                ) {
                    Ok(packed) => packed,
                    Err(error) => {
                        failures.push(format!(
                            "{} frame {index}: could not pack: {error}",
                            entry.name
                        ));
                        continue;
                    }
                };
                let payload = if sprite.compressed {
                    encode_rle(&packed)
                } else {
                    packed
                };

                // Read our own payload back with the parser's code, standing where the file's
                // pixel pointer stands. This is what catches a variant-0 stream that halts on the
                // tight size when the frame is row-padded: the pixels come back rearranged rather
                // than merely packed differently.
                let (repacked, consumed) = match read_frame_pixels(
                    &payload,
                    0,
                    frame.width,
                    frame.height,
                    sprite.bits_per_pixel,
                    sprite.compressed,
                    sprite.record_variant,
                    payload.len(),
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        failures.push(format!(
                            "{} frame {index}: re-encoded payload does not decode: {error}",
                            entry.name
                        ));
                        continue;
                    }
                };
                if consumed != payload.len() || repacked.len() != packed_size {
                    failures.push(format!(
                        "{} frame {index}: re-encoded payload is {} bytes decoding to {} packed \
                         bytes; the parser stops after {consumed} and {packed_size} was stored",
                        entry.name,
                        payload.len(),
                        repacked.len(),
                    ));
                    continue;
                }
                let indices = match unpack_pixels(
                    &repacked,
                    frame.width,
                    frame.height,
                    sprite.bits_per_pixel,
                ) {
                    Ok(indices) => indices,
                    Err(error) => {
                        failures.push(format!(
                            "{} frame {index}: could not unpack: {error}",
                            entry.name
                        ));
                        continue;
                    }
                };

                frames += 1;
                let frame_pixel_lossless = indices == frame.palette_indices;
                if frame_pixel_lossless {
                    pixel_lossless += 1;
                } else {
                    let at = indices
                        .iter()
                        .zip(&frame.palette_indices)
                        .position(|(wrote, read)| wrote != read);
                    failures.push(format!(
                        "{} frame {index}: pixels changed (first differing pixel {at:?})",
                        entry.name
                    ));
                }
                let theirs = bytes
                    .get(pixels_offset..pixels_offset + stored_size)
                    .unwrap_or_default();
                let payload_identical = theirs == payload.as_slice();
                if payload_identical {
                    byte_identical_payloads += 1;
                }

                match write_frame_pixels(&bytes, index, &frame.palette_indices) {
                    Ok(write) => {
                        rewrite_attempted += 1;
                        if write.bytes == bytes {
                            rewrite_identical += 1;
                        } else if !payload_identical {
                            rewrite_differs_by_payload += 1;
                        } else {
                            // A frame whose payload re-encodes to the original bytes and whose
                            // file still changed is a defect in the *writer*: the pointer
                            // rewriting, the alignment padding, or the byte-for-byte preservation.
                            // Nothing else is left to blame, which is what makes this sharp.
                            failures.push(format!(
                                "{} frame {index}: identity rewrite changed the file although its \
                                 payload is byte-identical (shift={}, stored {}->{}, padding={})",
                                entry.name,
                                write.shift,
                                write.stored_size.0,
                                write.stored_size.1,
                                write.alignment_padding,
                            ));
                        }
                    }
                    Err(_) => rewrite_refused += 1,
                }
            }
        }

        // Report the first few failures rather than only their count: a bare number would make a
        // regression here as opaque as the prose this test replaced.
        assert!(
            failures.is_empty(),
            "{} frames did not round-trip; first: {:#?}",
            failures.len(),
            &failures[..failures.len().min(5)]
        );

        // The tripwires. Without them every assertion above holds vacuously over zero members,
        // which is exactly what happens when `LOM_LISTFILE` is unset.
        assert_eq!(members, ARCHIVED_IMP_MEMBERS, "the IMP corpus changed size");
        assert_eq!(frames, IMP_PAYLOAD_FRAMES, "the frame population changed");
        assert_eq!(duplicate_frames, IMP_DUPLICATE_FRAMES);
        assert_eq!(empty_frames, 0, "a frame with a zero dimension appeared");

        assert_eq!(
            pixel_lossless, frames,
            "a frame did not survive pixel-lossless"
        );
        assert_eq!(byte_identical_payloads, IMP_BYTE_IDENTICAL_PAYLOADS);
        assert_eq!(rewrite_attempted, IMP_REWRITE_ATTEMPTED);
        assert_eq!(rewrite_identical, IMP_REWRITE_BYTE_IDENTICAL);
        assert_eq!(
            rewrite_refused,
            frames - rewrite_attempted,
            "the refusal population is not the complement of the attempted one"
        );
        // The accounting closes: every attempted rewrite is either byte-identical or differs only
        // because its payload does. A rewrite that escaped both arms would already have been a
        // failure above; asserting the sum keeps the two constants from drifting apart silently.
        assert_eq!(
            rewrite_identical + rewrite_differs_by_payload,
            rewrite_attempted
        );
    }
}
