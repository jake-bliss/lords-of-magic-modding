//! The `.sav` savegame container, and the sections of it that are decodable.
//!
//! # This is a locator, not the engine's reader
//!
//! The engine loads a save with a **tag-dispatch loop** (`0x0048322A`): read an 8-byte tag,
//! `memcmp` its first **7** bytes against a contiguous array of nine `char[8]` at `0x0055B168`,
//! jump through the 9-entry table at `0x00483970`, and let the handler consume exactly as many
//! bytes as its own structure needs. There is **no length word and no count word between a tag and
//! its payload** -- the `u32` that follows a tag is already the first field of that section, and it
//! means something different in each one.
//!
//! That design means a section cannot be skipped without decoding it, and two of the nine cannot be
//! decoded today: [`SpriteSection`] holds polymorphic variable-length records dispatched through a
//! virtual call, and [`PlayerSection`]'s record size is not established for format version 111.
//! So this parser does **not** reimplement the loop. It **scans the whole file for the nine tag
//! byte-strings** and takes each section's extent as running from its payload to the next tag found,
//! or to end of file for the last one.
//!
//! **That shortcut is only honest with a census**, because a tag byte-string could in principle
//! occur inside payload data and nothing in the format forbids it. [`SaveContainer::locate`]
//! therefore requires **exactly nine distinct tags, each appearing exactly once**, and refuses with
//! a [`SaveError::TagCensusFailed`] carrying the full [`TagCensus`] otherwise. A caller that wants
//! to see the counts without a parse can call [`TagCensus::take`] directly.
//!
//! Section **order in the file is irrelevant to the engine** and is irrelevant here. Every
//! inspected file happens to store VER, MULT, MAP, SPR, USER, GAME, PLR, REGN, ALRM in that order;
//! nothing depends on it, and the tests permute it.
//!
//! # What is decoded and what is carried
//!
//! | section | state |
//! | --- | --- |
//! | [`VersionSection`] | fully decoded |
//! | [`MultiplayerSection`] | fully decoded |
//! | [`MapSection`] | fully decoded, through [`crate::map::MapAsset`] |
//! | [`UserSection`] | eight fixed-size records; the known head fields are named, the rest carried |
//! | [`GameSection`] | fully decoded |
//! | [`RegionSection`] | header and grid decoded; **tail carried raw**, structure Unknown |
//! | [`AlarmSection`] | header decoded; **records carried raw**, layout Unknown |
//! | [`PlayerSection`] | tail decoded from the end; **records carried raw**, size Unknown for v111 |
//! | [`SpriteSection`] | **count only**; records carried raw, layout Unknown |
//!
//! Everything in the "carried" column is an explicit `raw` field rather than silence. A parser that
//! drops the bytes it does not understand cannot be grown into a writer.
//!
//! # No compression and no encryption
//!
//! The writer at `0x00482AF0` `fwrite`s struct memory directly. Nothing in the `.sav` path
//! transforms bytes. The one caveat is recorded on [`RegionSection`].

use std::collections::BTreeMap;
use std::fmt;

use crate::map::{MapAsset, MapCell, MapError};

/// The nine section tags, in the order the engine's array at `0x0055B168` stores them.
///
/// **Observed in a local binary, 2026-09-18.** One contiguous array of nine `char[8]`, immediately
/// followed by the dword `111` at `0x0055B1B0`, which serves both as the loop's sentinel and as the
/// build's format-version constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SectionTag {
    Map,
    Sprites,
    User,
    Game,
    Player,
    Version,
    Region,
    Alarm,
    Multiplayer,
}

/// Every tag, in the binary's array order.
pub const SECTION_TAGS: [SectionTag; 9] = [
    SectionTag::Map,
    SectionTag::Sprites,
    SectionTag::User,
    SectionTag::Game,
    SectionTag::Player,
    SectionTag::Version,
    SectionTag::Region,
    SectionTag::Alarm,
    SectionTag::Multiplayer,
];

/// The width of a tag on disk: eight bytes, NUL-padded.
pub const TAG_SIZE: usize = 8;

/// The bytes the engine actually compares: **seven**, not eight.
///
/// **Observed in a local binary, 2026-09-18.** The dispatch loop at `0x0048322A` passes `7` to
/// `memcmp`. The eighth byte of a stored tag is never examined. Every tag in the array is seven
/// ASCII characters and one NUL, so matching seven is matching all of the meaningful ones -- but a
/// writer must not conclude the eighth byte is free, only that this build ignores it.
pub const TAG_COMPARED_BYTES: usize = 7;

impl SectionTag {
    /// The seven bytes the engine compares.
    pub fn compared_bytes(self) -> &'static [u8; TAG_COMPARED_BYTES] {
        match self {
            Self::Map => b"LS_MAP_",
            Self::Sprites => b"LS_SPR_",
            Self::User => b"LS_USER",
            Self::Game => b"LS_GAME",
            Self::Player => b"LS_PLR_",
            Self::Version => b"LS_VER_",
            Self::Region => b"LS_REGN",
            Self::Alarm => b"LS_ALRM",
            Self::Multiplayer => b"LS_MULT",
        }
    }

    /// The eight bytes a writer emits: the compared seven plus the NUL pad.
    pub fn on_disk_bytes(self) -> [u8; TAG_SIZE] {
        let mut bytes = [0_u8; TAG_SIZE];
        bytes[..TAG_COMPARED_BYTES].copy_from_slice(self.compared_bytes());
        bytes
    }

    pub fn name(self) -> &'static str {
        std::str::from_utf8(self.compared_bytes()).expect("tags are ASCII")
    }
}

impl fmt::Display for SectionTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The scan did not find exactly one occurrence of each of the nine tags.
    ///
    /// Carries the whole census rather than the first offending tag, because "which tags are
    /// missing" and "which are duplicated" are both diagnostic and a caller seeing only one of them
    /// will guess at the other.
    TagCensusFailed(TagCensus),
    /// A section's payload does not hold what that section's structure requires.
    Section { tag: SectionTag, message: String },
}

impl SaveError {
    fn section(tag: SectionTag, message: impl Into<String>) -> Self {
        Self::Section {
            tag,
            message: message.into(),
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TagCensusFailed(census) => write!(
                formatter,
                "save tag census failed: {} of 9 tags occur exactly once ({})",
                census.tags_seen_exactly_once(),
                census.describe_anomalies()
            ),
            Self::Section { tag, message } => write!(formatter, "{tag}: {message}"),
        }
    }
}

impl std::error::Error for SaveError {}

// ---------------------------------------------------------------------------
// The census and the container
// ---------------------------------------------------------------------------

/// How many times each of the nine tag byte-strings occurs in a file.
///
/// This is the guard that makes scanning-for-tags an honest substitute for the engine's
/// sequential reader. It is public and separately constructible so that a caller diagnosing a file
/// this parser refuses can see the counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCensus {
    counts: BTreeMap<SectionTag, usize>,
}

impl TagCensus {
    /// Count every occurrence of every tag's seven compared bytes, with overlapping matches
    /// counted separately.
    pub fn take(source: &[u8]) -> Self {
        let mut counts = BTreeMap::new();
        for tag in SECTION_TAGS {
            let needle = tag.compared_bytes();
            let mut occurrences = 0_usize;
            let mut start = 0_usize;
            while start + TAG_COMPARED_BYTES <= source.len() {
                match source[start..]
                    .windows(TAG_COMPARED_BYTES)
                    .position(|window| window == needle)
                {
                    Some(offset) => {
                        occurrences += 1;
                        start += offset + 1;
                    }
                    None => break,
                }
            }
            counts.insert(tag, occurrences);
        }
        Self { counts }
    }

    pub fn count(&self, tag: SectionTag) -> usize {
        self.counts.get(&tag).copied().unwrap_or(0)
    }

    /// How many of the nine tags occur exactly once. Nine is the only acceptable answer.
    pub fn tags_seen_exactly_once(&self) -> usize {
        SECTION_TAGS
            .iter()
            .filter(|tag| self.count(**tag) == 1)
            .count()
    }

    pub fn is_well_formed(&self) -> bool {
        self.tags_seen_exactly_once() == SECTION_TAGS.len()
    }

    /// Every tag whose count is not one, as `LS_XXX=n` pairs.
    pub fn describe_anomalies(&self) -> String {
        let anomalies: Vec<String> = SECTION_TAGS
            .iter()
            .filter(|tag| self.count(**tag) != 1)
            .map(|tag| format!("{tag}={}", self.count(*tag)))
            .collect();
        if anomalies.is_empty() {
            "no anomalies".to_owned()
        } else {
            anomalies.join(", ")
        }
    }
}

/// Where one section lives in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionLocation {
    pub tag: SectionTag,
    /// Offset of the 8-byte tag itself.
    pub tag_offset: usize,
    /// Offset of the first payload byte: `tag_offset + 8`.
    pub payload_offset: usize,
    /// Payload bytes, up to the next tag or to end of file.
    pub payload_len: usize,
}

impl SectionLocation {
    /// The section's total footprint including its tag.
    pub fn section_len(&self) -> usize {
        TAG_SIZE + self.payload_len
    }

    pub fn payload_end(&self) -> usize {
        self.payload_offset + self.payload_len
    }
}

/// The nine located sections of a file, in the order they appear on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveContainer {
    locations: Vec<SectionLocation>,
    census: TagCensus,
}

impl SaveContainer {
    /// Find the nine sections by scanning, refusing anything that is not exactly nine-of-one.
    pub fn locate(source: &[u8]) -> Result<Self, SaveError> {
        let census = TagCensus::take(source);
        if !census.is_well_formed() {
            return Err(SaveError::TagCensusFailed(census));
        }

        let mut starts: Vec<(usize, SectionTag)> = SECTION_TAGS
            .iter()
            .map(|tag| {
                let needle = tag.compared_bytes();
                let offset = source
                    .windows(TAG_COMPARED_BYTES)
                    .position(|window| window == needle)
                    .expect("the census proved this tag occurs exactly once");
                (offset, *tag)
            })
            .collect();
        starts.sort_unstable();

        let mut locations = Vec::with_capacity(starts.len());
        for (index, (tag_offset, tag)) in starts.iter().copied().enumerate() {
            let payload_offset = tag_offset + TAG_SIZE;
            let next = starts
                .get(index + 1)
                .map(|(offset, _)| *offset)
                .unwrap_or(source.len());
            if payload_offset > next {
                return Err(SaveError::section(
                    tag,
                    format!("tag at {tag_offset} overlaps the next tag at {next}"),
                ));
            }
            locations.push(SectionLocation {
                tag,
                tag_offset,
                payload_offset,
                payload_len: next - payload_offset,
            });
        }

        Ok(Self { locations, census })
    }

    /// Every section, in file order.
    pub fn locations(&self) -> &[SectionLocation] {
        &self.locations
    }

    pub fn census(&self) -> &TagCensus {
        &self.census
    }

    pub fn location(&self, tag: SectionTag) -> SectionLocation {
        self.locations
            .iter()
            .copied()
            .find(|location| location.tag == tag)
            .expect("the census proved every tag is present")
    }

    /// The tags in the order this file happens to store them.
    ///
    /// Reported only so a survey can show it. **Nothing may branch on it**: the engine dispatches
    /// on the tag it just read, so order carries no meaning.
    pub fn tag_order(&self) -> Vec<SectionTag> {
        self.locations.iter().map(|location| location.tag).collect()
    }

    fn payload<'a>(&self, source: &'a [u8], tag: SectionTag) -> Result<&'a [u8], SaveError> {
        let location = self.location(tag);
        source
            .get(location.payload_offset..location.payload_end())
            .ok_or_else(|| SaveError::section(tag, "payload runs past end of file"))
    }
}

// ---------------------------------------------------------------------------
// Little-endian readers
// ---------------------------------------------------------------------------

fn read_u32(tag: SectionTag, payload: &[u8], offset: usize) -> Result<u32, SaveError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| SaveError::section(tag, "u32 offset overflow"))?;
    let bytes: [u8; 4] = payload
        .get(offset..end)
        .ok_or_else(|| {
            SaveError::section(tag, format!("u32 at +{offset} runs past the payload end"))
        })?
        .try_into()
        .expect("slice length was checked");
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32(tag: SectionTag, payload: &[u8], offset: usize) -> Result<i32, SaveError> {
    read_u32(tag, payload, offset).map(|value| value as i32)
}

// ---------------------------------------------------------------------------
// LS_VER_
// ---------------------------------------------------------------------------

/// The format version, and nothing else: the payload is exactly four bytes.
///
/// **Observed in a local binary, 2026-09-18.** The engine's version handling is a **monotone
/// feature gate that never rejects**. The handler's only error path is a short read; every
/// subsequent test against `[0x005AA12C]` is `jl` or `jge`, and the binary contains no `jg`, `ja`
/// or `jne` on that field at all. There is no floor and no ceiling, so this parser imposes none
/// either -- a version is data, not a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionSection {
    pub version: u32,
}

/// The version constant this build writes, stored immediately after the tag array at `0x0055B1B0`.
pub const BUILD_FORMAT_VERSION: u32 = 111;

/// The version below which the engine synthesizes [`MultiplayerSection`]'s slot block from memory
/// instead of reading it.
pub const MULTIPLAYER_SLOTS_MIN_VERSION: u32 = 99;

impl VersionSection {
    pub const PAYLOAD_LEN: usize = 4;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Version;
        if payload.len() != Self::PAYLOAD_LEN {
            return Err(SaveError::section(
                tag,
                format!(
                    "payload is {} bytes; the version section is exactly {}",
                    payload.len(),
                    Self::PAYLOAD_LEN
                ),
            ));
        }
        Ok(Self {
            version: read_u32(tag, payload, 0)?,
        })
    }

    /// Whether this version's writer emitted the 16 lord slots, rather than the reader
    /// synthesizing them. See [`MULTIPLAYER_SLOTS_MIN_VERSION`].
    pub fn stores_multiplayer_slots(&self) -> bool {
        self.version >= MULTIPLAYER_SLOTS_MIN_VERSION
    }
}

// ---------------------------------------------------------------------------
// LS_MULT
// ---------------------------------------------------------------------------

/// One of sixteen lord slots: a code and a 32-byte name buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LordSlot {
    /// `0xFFFFFFFF` marks an unused slot. **This code, not the name, is the occupancy marker** --
    /// a populated slot may legitimately carry a zero-length name.
    pub lord_code: u32,
    /// The name field exactly as stored, including the bytes past its terminator.
    pub name_field: [u8; LordSlot::NAME_LEN],
}

impl LordSlot {
    pub const NAME_LEN: usize = 32;
    pub const RECORD_LEN: usize = 4 + Self::NAME_LEN;
    /// The `lord_code` of a slot no player occupies.
    pub const UNUSED_CODE: u32 = u32::MAX;

    /// Whether a player occupies this slot.
    ///
    /// **Observed in a local binary, 2026-09-18.** Keyed on the code and never on the name: in both
    /// turn-315 files, slots 1 and 4 carry valid codes `0x43` and `0x35` with an **empty** name.
    /// Treating an empty name as an absent player would have dropped two live players.
    pub fn is_occupied(&self) -> bool {
        self.lord_code != Self::UNUSED_CODE
    }

    /// The name, up to its NUL terminator. Everything past the terminator is deliberately dropped;
    /// see [`name_padding`](Self::name_padding).
    pub fn name(&self) -> &[u8] {
        let end = self
            .name_field
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(Self::NAME_LEN);
        &self.name_field[..end]
    }

    pub fn name_lossy(&self) -> String {
        String::from_utf8_lossy(self.name()).into_owned()
    }

    /// The bytes after the name's terminator: **uninitialised process memory**, not data.
    ///
    /// **Observed in a local binary, 2026-09-18.** The engine `strcpy`s the name into this 32-byte
    /// field from `[player_i + 0x50AC]` with no preceding `memset`, so whatever the buffer held is
    /// written to disk. Measured directly: `lastsave.lom` and `Merlin I` are the **same saved game
    /// state** -- eight of the nine sections are byte-identical, including all 196,628 bytes of
    /// `LS_MAP_` and all 306,853 of `LS_SPR_` -- and their entire difference is **356 bytes, every
    /// single one of them strictly past a name's NUL**, carrying recognisable Win32 stack and heap
    /// pointers (`0x004d3756`, `0x01bfbc70`, `0x02fc101c`, `0x04537e44`).
    ///
    /// Three consequences, all of which bit the analysis that produced this module:
    ///
    /// 1. **A `.sav` is not a pure function of game state.** Two saves of one state differ. No test
    ///    and no invariant may assume save bytes are reproducible.
    /// 2. **Any save-diffing tool must mask this range**, or it reports two identical states as
    ///    different. Comparing the two files by md5 says they differ; comparing their decoded
    ///    content says they do not, and the content is right.
    /// 3. **Saves carry fragments of process memory.** Benign in this corpus -- pointers, not user
    ///    data -- but worth knowing in a project whose point is people sharing these files.
    pub fn name_padding(&self) -> &[u8] {
        let used = self.name().len();
        if used >= Self::NAME_LEN {
            &[]
        } else {
            &self.name_field[used + 1..]
        }
    }
}

/// The game-setup block and the sixteen lord slots.
///
/// **Observed in a local binary, 2026-09-18.** 744 bytes in all eight inspected files, and the only
/// section whose leading `u32` is a genuine length: the writer stores a hardcoded `sizeof` of 164
/// and the reader *uses it* as the read length for the setup block, which makes this the one
/// forward-compatible section in the format. `4 + 164 + 16 * 36 = 744` exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplayerSection {
    /// The stored `sizeof` of [`setup`](Self::setup). 164 in every inspected file, but read as
    /// data because the engine reads it as data.
    pub declared_setup_len: u32,
    /// The game-setup struct, copied from `[gameobj+0x520]`. Its fields are **Unknown**.
    pub setup: Vec<u8>,
    pub slots: [LordSlot; MultiplayerSection::SLOT_COUNT],
}

impl MultiplayerSection {
    pub const SLOT_COUNT: usize = 16;
    /// How many slots a game can actually seat. Slots 8..16 carry [`LordSlot::UNUSED_CODE`] in
    /// every inspected file.
    pub const SEATED_SLOT_COUNT: usize = 8;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Multiplayer;
        let declared_setup_len = read_u32(tag, payload, 0)?;
        let setup_len = usize::try_from(declared_setup_len).map_err(|_| {
            SaveError::section(tag, "declared setup length exceeds platform limits")
        })?;
        let slots_offset = 4usize
            .checked_add(setup_len)
            .ok_or_else(|| SaveError::section(tag, "setup length overflow"))?;
        let setup = payload
            .get(4..slots_offset)
            .ok_or_else(|| {
                SaveError::section(
                    tag,
                    format!(
                        "declared setup block of {setup_len} bytes does not fit in a {}-byte payload",
                        payload.len()
                    ),
                )
            })?
            .to_vec();

        let mut slots = [LordSlot {
            lord_code: LordSlot::UNUSED_CODE,
            name_field: [0; LordSlot::NAME_LEN],
        }; Self::SLOT_COUNT];
        for (index, slot) in slots.iter_mut().enumerate() {
            let offset = slots_offset + index * LordSlot::RECORD_LEN;
            slot.lord_code = read_u32(tag, payload, offset)?;
            let name_end = offset + LordSlot::RECORD_LEN;
            slot.name_field.copy_from_slice(
                payload.get(offset + 4..name_end).ok_or_else(|| {
                    SaveError::section(tag, format!("slot {index} name is cut off"))
                })?,
            );
        }

        Ok(Self {
            declared_setup_len,
            setup,
            slots,
        })
    }

    /// The bytes this section accounts for: `4 + declared_setup_len + 16 * 36`.
    pub fn accounted_len(&self) -> usize {
        4 + self.setup.len() + Self::SLOT_COUNT * LordSlot::RECORD_LEN
    }

    /// The occupied slots, paired with their index.
    pub fn occupied_slots(&self) -> impl Iterator<Item = (usize, &LordSlot)> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_occupied())
    }

    /// The lord codes of slots `0..8`, in order, for comparison against [`PlayerSection`].
    pub fn seated_lord_codes(&self) -> [u32; Self::SEATED_SLOT_COUNT] {
        let mut codes = [0_u32; Self::SEATED_SLOT_COUNT];
        for (index, code) in codes.iter_mut().enumerate() {
            *code = self.slots[index].lord_code;
        }
        codes
    }
}

// ---------------------------------------------------------------------------
// LS_MAP_
// ---------------------------------------------------------------------------

/// The three values the cell visibility field takes across the whole corpus.
///
/// **Observed in a local binary, 2026-09-18.** Across all 131,072 cells of all eight inspected
/// files the field holds **0, 63 or 128 and nothing else**. What those mean is **Inferred**; see
/// [`MapSection::visibility_histogram`].
pub const OBSERVED_VISIBILITY_LEVELS: [i16; 3] = [0, 63, 128];

/// The embedded world map: the standalone map format **minus its leading `metadata` word**.
///
/// **Observed in a local binary, 2026-09-18.** The payload accounts exactly, with no slack:
///
/// ```text
///   4      width               (128 in every inspected file)
///   4      height              (128)
///   4      bytes_per_cell      (8)
///   w*h*8  cell grid           (131,072)
///   4      count == w*h        (16,384)
///   4*count  a second, parallel per-cell plane   (65,536)
///   4      trailer             (1)
/// ```
///
/// `4+4+4+131072+4+65536+4 = 196,628`, which is the payload length in all eight files.
///
/// Cell decoding is **not duplicated here**. The bytes are handed to [`MapAsset::parse`] with a
/// synthesized zero `metadata` word in front, so the save and the standalone `.scn`/`.smp` path can
/// never drift apart.
#[derive(Debug, Clone, PartialEq)]
pub struct MapSection {
    pub map: MapAsset,
    /// The `count` word preceding the second plane. Equal to `width * height` in every inspected
    /// file, which is why it is checked rather than assumed.
    pub plane_count: u32,
    /// The second per-cell plane, one `u32` per cell. Its meaning is **Unknown**.
    pub plane: Vec<u32>,
    /// The final word. `1` in every inspected file; meaning **Unknown**.
    pub trailer: u32,
}

impl MapSection {
    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Map;
        let width = read_u32(tag, payload, 0)?;
        let height = read_u32(tag, payload, 4)?;
        let bytes_per_cell = read_u32(tag, payload, 8)?;

        let cell_count = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(width, height)| width.checked_mul(height))
            .ok_or_else(|| SaveError::section(tag, "map cell count overflow"))?;
        let grid_bytes = usize::try_from(bytes_per_cell)
            .ok()
            .and_then(|per_cell| cell_count.checked_mul(per_cell))
            .ok_or_else(|| SaveError::section(tag, "map grid byte count overflow"))?;
        let grid_end = 12_usize
            .checked_add(grid_bytes)
            .ok_or_else(|| SaveError::section(tag, "map grid offset overflow"))?;

        // Re-attach the `metadata` word the standalone format has and the save omits, so the one
        // cell decoder in `map.rs` does the work. `0` is not invented data: it is the header field
        // this section does not have, and nothing downstream of here reads it.
        let mut standalone = Vec::with_capacity(4 + grid_end.min(payload.len()));
        standalone.extend_from_slice(&0_u32.to_le_bytes());
        standalone.extend_from_slice(payload.get(..grid_end).ok_or_else(|| {
            SaveError::section(
                tag,
                format!(
                    "cell grid needs {grid_end} bytes but the payload is {}",
                    payload.len()
                ),
            )
        })?);
        let map = MapAsset::parse(&standalone)
            .map_err(|error: MapError| SaveError::section(tag, format!("embedded map: {error}")))?;

        let plane_count = read_u32(tag, payload, grid_end)?;
        let plane_len = usize::try_from(plane_count)
            .map_err(|_| SaveError::section(tag, "plane count exceeds platform limits"))?;
        let mut plane = Vec::with_capacity(plane_len.min(cell_count.max(1)));
        for index in 0..plane_len {
            plane.push(read_u32(tag, payload, grid_end + 4 + index * 4)?);
        }
        let trailer = read_u32(tag, payload, grid_end + 4 + plane_len * 4)?;

        Ok(Self {
            map,
            plane_count,
            plane,
            trailer,
        })
    }

    /// The bytes this section accounts for. Compared against the payload length by the survey; a
    /// total that matches is worth nothing unless the terms match too -- see the Corrected note in
    /// `docs/save-format.md`.
    pub fn accounted_len(&self) -> usize {
        12 + self.map.cells.len() * 8 + 4 + self.plane.len() * 4 + 4
    }

    /// Whether the plane's count word equals the map's cell count.
    pub fn plane_covers_every_cell(&self) -> bool {
        usize::try_from(self.plane_count) == Ok(self.map.cells.len())
            && self.plane.len() == self.map.cells.len()
    }

    /// How many cells hold each visibility level.
    ///
    /// **Inferred, 2026-09-18: this is visibility state -- unexplored, dim, fully visible.** The
    /// evidence is a distribution, not a measurement. The one genuine mid-game state splits
    /// 7,840 / 7,403 / 1,141 across `0 / 63 / 128`, which is what a partly-explored map looks like,
    /// while every authored demo save is almost entirely `128`. It agrees with the dimming
    /// expression `(0x80 - field) * k >> 7` at `0x00519ced`.
    ///
    /// **What is Observed is only that the field takes exactly three values.** The full `0..128`
    /// range and any saturation behaviour are **not** observed and must not be asserted.
    pub fn visibility_histogram(&self) -> BTreeMap<i16, usize> {
        let mut histogram = BTreeMap::new();
        for cell in &self.map.cells {
            *histogram.entry(cell_visibility(cell)).or_insert(0) += 1;
        }
        histogram
    }
}

/// A cell's visibility field: the **signed** upper half of the cell's first word.
///
/// **Observed in a local binary, 2026-09-18, and this supersedes a bit reading.** Cell word 0 is
/// two `u16` fields: `tile_index = tag & 0xffff`, and this one at `+2`. All eleven readers in the
/// binary are `movsx` and none masks, so it is a signed scalar and not a bitfield. The earlier
/// reading of `0x00800000` as a flag was a maxed-out *number*; it is **Refuted**.
///
/// **This deliberately does not go through [`MapCell::tile_index`]**, which masks only
/// `0x00800000`. That masking is correct for the standalone-map corpus it was measured on and is
/// not corrected here, because changing it is outside this module's scope; see the note in
/// `docs/save-format.md`.
pub fn cell_visibility(cell: &MapCell) -> i16 {
    (cell.tag >> 16) as i16
}

/// A cell's tile-atlas slot, as the save's own readers compute it: the low `u16`.
pub fn cell_tile_index(cell: &MapCell) -> u16 {
    (cell.tag & 0xffff) as u16
}

// ---------------------------------------------------------------------------
// LS_SPR_
// ---------------------------------------------------------------------------

/// The unit, army and hero table. **Only the count is decoded.**
///
/// **Observed in a local binary, 2026-09-18.** The records are polymorphic and variable-length:
/// each begins with a `u32 class_id`, and the reader makes a virtual call `call dword [eax+0x20]`
/// dispatched through a 10-entry jump table at `0x004F73B8` bounded by `cmp eax,9 / ja`. Class ids
/// 5 and 6 abort as invalid. The in-memory object sizes -- 0 to 9: 1500, 96, 148, 844, 120,
/// invalid, invalid, a factory at `0x0047CBD0`, 88, 328 -- are sizes in RAM, **not on disk**.
///
/// That no fixed stride exists is established from the file side too, independently of the
/// disassembly: for every candidate header size `0..=1024`, the set of sizes for which
/// `(payload_len - header) % count == 0` has an **empty intersection** across the eight files, and
/// three of them admit no solution at all. The section also contains length-prefixed strings
/// (`u32 len` then `len` raw bytes, no NUL) at irregular offsets, so variable length is directly
/// visible rather than merely inferred.
///
/// So: **parse the count and stop.** [`records_raw`](Self::records_raw) carries the rest verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSection {
    pub record_count: u32,
    /// Every byte after the count word. Layout **Unknown**.
    pub raw: Vec<u8>,
}

impl SpriteSection {
    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Sprites;
        let record_count = read_u32(tag, payload, 0)?;
        Ok(Self {
            record_count,
            raw: payload[4..].to_vec(),
        })
    }

    /// The undecoded record bytes.
    pub fn records_raw(&self) -> &[u8] {
        &self.raw
    }

    /// Every header size in `0..=max_header` for which the remaining bytes divide evenly by the
    /// record count. Used by the survey to re-run the no-stride argument against live data rather
    /// than quoting a past result.
    pub fn candidate_fixed_strides(&self, max_header: usize) -> Vec<usize> {
        let count = match usize::try_from(self.record_count) {
            Ok(count) if count > 0 => count,
            _ => return Vec::new(),
        };
        let total = self.raw.len() + 4;
        (0..=max_header)
            .filter(|header| total > *header && (total - header).is_multiple_of(count))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// LS_USER
// ---------------------------------------------------------------------------

/// One 784-byte per-player record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    pub raw: Vec<u8>,
}

impl UserRecord {
    pub const LEN: usize = 784;

    fn word(&self, offset: usize) -> u32 {
        self.raw
            .get(offset..offset + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
            .unwrap_or(0)
    }

    /// `+0`: the record's own index. Equal to its position in every inspected file.
    pub fn index(&self) -> u32 {
        self.word(0)
    }

    /// `+4`: `0xFFFFFFFF` in every inspected record. Meaning **Unknown**.
    pub fn unknown_4(&self) -> u32 {
        self.word(4)
    }

    /// `+8`: `0` in every inspected record. Meaning **Unknown**.
    pub fn unknown_8(&self) -> u32 {
        self.word(8)
    }

    /// `+12`: the bits `0x3F800000`, which is `1.0f`, in every inspected record.
    pub fn unknown_12_bits(&self) -> u32 {
        self.word(12)
    }

    pub fn unknown_12_as_f32(&self) -> f32 {
        f32::from_bits(self.unknown_12_bits())
    }

    /// `+16`: `0` in every inspected record. Meaning **Unknown**.
    pub fn unknown_16(&self) -> u32 {
        self.word(16)
    }

    /// `+20`: an id. Record 0 carries a small non-`-1` value and records 1..8 carry `0xFFFFFFFF`
    /// in every inspected file. Meaning **Unknown**.
    pub fn unknown_20(&self) -> u32 {
        self.word(20)
    }
}

/// Eight fixed-size per-player records, back to back with no padding.
///
/// **Observed in a local binary, 2026-09-18.** The payload is 6,272 bytes in all eight files, which
/// is `8 * 784` with zero remainder, and block *i*'s first `u32` is exactly *i*. The writer does
/// `push 0x310` (784) and `fwrite` eight times -- note its **in-memory** stride is `0x400`, so the
/// on-disk record is the struct's first 784 bytes and not the whole struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSection {
    pub records: Vec<UserRecord>,
}

impl UserSection {
    pub const RECORD_COUNT: usize = 8;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::User;
        if !payload.len().is_multiple_of(UserRecord::LEN) {
            return Err(SaveError::section(
                tag,
                format!(
                    "payload of {} bytes is not a whole number of {}-byte records",
                    payload.len(),
                    UserRecord::LEN
                ),
            ));
        }
        let records = payload
            .chunks_exact(UserRecord::LEN)
            .map(|chunk| UserRecord {
                raw: chunk.to_vec(),
            })
            .collect();
        Ok(Self { records })
    }

    /// Whether every record's stored index equals its position.
    pub fn indexes_are_positional(&self) -> bool {
        self.records
            .iter()
            .enumerate()
            .all(|(position, record)| u32::try_from(position) == Ok(record.index()))
    }
}

// ---------------------------------------------------------------------------
// LS_GAME
// ---------------------------------------------------------------------------

/// One twelve-byte game record. All three fields' meanings are **Unknown**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameRecord {
    /// Descends by one in long runs and then jumps, which is what a free list looks like.
    /// That reading is **Inferred**.
    pub id: i32,
    pub a: i32,
    pub b: i32,
}

/// The turn counter and a table of twelve-byte records.
///
/// **Observed in a local binary, 2026-09-18.** The shape holds in all eight files with no
/// exception:
///
/// ```text
///   u32 turn
///   u32 unknown_4
///   u32 0
///   u32 live_count
///   u32 12                      -- literally the record size, stored
///   N * 12 bytes                -- N = (payload_len - 24) / 12
///   u32 trailer
/// ```
///
/// `(payload_len - 24) % 12 == 0` in all eight, and **`N - live_count == 71` in all eight**:
/// 1528/1457, 1653/1582, 269/198, 269/198, 424/353, 191/120, 2691/2620, 2691/2620. The constant 71
/// is Observed and **unexplained** -- do not name the fields it relates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSection {
    /// The turn number. See [`SaveFile::turn_agreement`] for why this reading is Observed rather
    /// than guessed.
    pub turn: u32,
    /// Meaning **Unknown**; varies across files with no pattern found.
    pub unknown_4: u32,
    /// `0` in every inspected file.
    pub zero_8: u32,
    /// Meaning **Unknown**, but see the `N - live_count == 71` invariant.
    pub live_count: u32,
    /// The stored record size. Unlike [`MultiplayerSection::declared_setup_len`] there is no
    /// evidence the reader uses this as a length, so it is checked, not trusted.
    pub declared_record_size: u32,
    pub records: Vec<GameRecord>,
    /// The final word. Varies (1, 319, 801 observed); meaning **Unknown**.
    pub trailer: u32,
}

impl GameSection {
    /// The fixed bytes: five leading words and the trailing one.
    pub const FIXED_BYTES: usize = 24;
    pub const RECORD_LEN: usize = 12;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Game;
        if payload.len() < Self::FIXED_BYTES {
            return Err(SaveError::section(
                tag,
                format!(
                    "payload of {} bytes is shorter than the {}-byte fixed part",
                    payload.len(),
                    Self::FIXED_BYTES
                ),
            ));
        }
        let record_bytes = payload.len() - Self::FIXED_BYTES;
        if !record_bytes.is_multiple_of(Self::RECORD_LEN) {
            return Err(SaveError::section(
                tag,
                format!(
                    "{record_bytes} record bytes are not a whole number of {}-byte records",
                    Self::RECORD_LEN
                ),
            ));
        }

        let turn = read_u32(tag, payload, 0)?;
        let unknown_4 = read_u32(tag, payload, 4)?;
        let zero_8 = read_u32(tag, payload, 8)?;
        let live_count = read_u32(tag, payload, 12)?;
        let declared_record_size = read_u32(tag, payload, 16)?;

        let count = record_bytes / Self::RECORD_LEN;
        let mut records = Vec::with_capacity(count);
        for index in 0..count {
            let offset = 20 + index * Self::RECORD_LEN;
            records.push(GameRecord {
                id: read_i32(tag, payload, offset)?,
                a: read_i32(tag, payload, offset + 4)?,
                b: read_i32(tag, payload, offset + 8)?,
            });
        }
        let trailer = read_u32(tag, payload, 20 + count * Self::RECORD_LEN)?;

        Ok(Self {
            turn,
            unknown_4,
            zero_8,
            live_count,
            declared_record_size,
            records,
            trailer,
        })
    }

    /// `records.len() - live_count`, the quantity that is 71 in every inspected file.
    ///
    /// Signed, and computed rather than asserted, because a survey that printed only pass/fail
    /// could not tell "held at 71" from "held at some other constant".
    pub fn record_surplus(&self) -> i64 {
        self.records.len() as i64 - i64::from(self.live_count)
    }

    pub fn accounted_len(&self) -> usize {
        Self::FIXED_BYTES + self.records.len() * Self::RECORD_LEN
    }
}

/// The value [`GameSection::record_surplus`] takes in every inspected file. Unexplained.
pub const OBSERVED_GAME_RECORD_SURPLUS: i64 = 71;

// ---------------------------------------------------------------------------
// LS_PLR_
// ---------------------------------------------------------------------------

/// Per-player state. **The records are carried raw; only the tail is decoded.**
///
/// **Observed in a local binary, 2026-09-18.** The section is `{ u32 slot_index; record }*`
/// terminated by `u32 -1`, followed by **eight `u32` lord codes** that match
/// [`MultiplayerSection`]'s slots 0..8 in order. The reader validates `0 <= slot_index < 16`.
///
/// The `-1` sentinel sits at exactly `payload_end - 36` in all eight files, which is what makes the
/// tail parseable from the end regardless of what the records are.
///
/// **The record size is not established for format version 111.** In the single version-108 file it
/// is a clean `9 * 6223` with slot indexes `0,1,2,3,4,5,6,7,15` -- 15 being the neutral/unowned
/// pseudo-player -- and a name at `+2984` within the record. Generalising that to version 111
/// **failed on four of six files**, and one apparent fit was spurious. The reading is also
/// confounded: the version-108 file is simultaneously the only turn-1 file, so version and
/// game-age cannot be separated. No `record_size` field is offered here, because offering one
/// would be claiming it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSection {
    /// Everything before the sentinel: `{ u32 slot_index; record }*`. Layout **Unknown**.
    pub records_raw: Vec<u8>,
    /// The `0xFFFFFFFF` terminator, carried so a writer reproduces the value rather than minting
    /// one.
    pub sentinel: u32,
    /// Slots 0..8's lord codes, repeated here from [`MultiplayerSection`].
    pub lord_codes: [u32; MultiplayerSection::SEATED_SLOT_COUNT],
}

impl PlayerSection {
    /// The tail's width: the sentinel plus eight lord codes.
    pub const TAIL_LEN: usize = 4 + 4 * MultiplayerSection::SEATED_SLOT_COUNT;
    pub const SENTINEL: u32 = u32::MAX;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Player;
        let sentinel_offset = payload.len().checked_sub(Self::TAIL_LEN).ok_or_else(|| {
            SaveError::section(
                tag,
                format!(
                    "payload of {} bytes is shorter than the {}-byte tail",
                    payload.len(),
                    Self::TAIL_LEN
                ),
            )
        })?;
        let sentinel = read_u32(tag, payload, sentinel_offset)?;
        if sentinel != Self::SENTINEL {
            return Err(SaveError::section(
                tag,
                format!(
                    "expected the {:#x} record terminator at +{sentinel_offset} but found {sentinel:#x}",
                    Self::SENTINEL
                ),
            ));
        }
        let mut lord_codes = [0_u32; MultiplayerSection::SEATED_SLOT_COUNT];
        for (index, code) in lord_codes.iter_mut().enumerate() {
            *code = read_u32(tag, payload, sentinel_offset + 4 + index * 4)?;
        }
        Ok(Self {
            records_raw: payload[..sentinel_offset].to_vec(),
            sentinel,
            lord_codes,
        })
    }

    /// Where the sentinel sat, which is `payload_len - 36`.
    pub fn sentinel_offset(&self) -> usize {
        self.records_raw.len()
    }

    /// The first record's slot index, the only field of a record this parser can read: it precedes
    /// the record body, so its position does not depend on the unknown record size.
    pub fn first_slot_index(&self) -> Option<u32> {
        self.records_raw
            .get(..4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
    }
}

// ---------------------------------------------------------------------------
// LS_REGN
// ---------------------------------------------------------------------------

/// A fixed six-byte-per-cell region grid, plus a tail nobody has decoded.
///
/// **Observed in a local binary, 2026-09-18.** `u32 width`, `u32 height`, then `width * height * 6`
/// bytes, in all eight files with `width == height == 128` and so a 98,304-byte grid. The grid is
/// fixed; every byte of variability lives in the tail.
///
/// Observed tail lengths: **8,998** (combat, experience, quickstart), **9,780** (magic, merc,
/// temple) and **9,389** (both turn-315 files). Its structure is **Unknown**.
///
/// **Caveat on the no-encryption claim, and it applies to this section only.** The writer at
/// `0x004C7390` brackets this section's I/O with two unresolved imports, `[0x0054D0D8]` and
/// `[0x0054D0DC]`. They are most likely a lock/unlock pair. If they turn out to be a transform,
/// the "no compression, no encryption" finding would need retesting **here** -- not elsewhere,
/// since the other eight sections are plainly readable in the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionSection {
    pub width: u32,
    pub height: u32,
    /// One six-byte record per cell. Field layout **Unknown**, so the bytes are carried whole.
    pub cells: Vec<[u8; RegionSection::CELL_LEN]>,
    /// Everything after the grid. Structure **Unknown**.
    pub tail_raw: Vec<u8>,
}

impl RegionSection {
    pub const CELL_LEN: usize = 6;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Region;
        let width = read_u32(tag, payload, 0)?;
        let height = read_u32(tag, payload, 4)?;
        let cell_count = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(width, height)| width.checked_mul(height))
            .ok_or_else(|| SaveError::section(tag, "region cell count overflow"))?;
        let grid_bytes = cell_count
            .checked_mul(Self::CELL_LEN)
            .ok_or_else(|| SaveError::section(tag, "region grid byte count overflow"))?;
        let grid_end = 8_usize
            .checked_add(grid_bytes)
            .ok_or_else(|| SaveError::section(tag, "region grid offset overflow"))?;
        let grid = payload.get(8..grid_end).ok_or_else(|| {
            SaveError::section(
                tag,
                format!(
                    "grid of {cell_count} cells needs {grid_bytes} bytes but only {} remain",
                    payload.len().saturating_sub(8)
                ),
            )
        })?;

        let cells = grid
            .chunks_exact(Self::CELL_LEN)
            .map(|chunk| {
                let mut cell = [0_u8; Self::CELL_LEN];
                cell.copy_from_slice(chunk);
                cell
            })
            .collect();

        Ok(Self {
            width,
            height,
            cells,
            tail_raw: payload[grid_end..].to_vec(),
        })
    }

    pub fn grid_len(&self) -> usize {
        self.cells.len() * Self::CELL_LEN
    }

    pub fn tail_len(&self) -> usize {
        self.tail_raw.len()
    }
}

// ---------------------------------------------------------------------------
// LS_ALRM
// ---------------------------------------------------------------------------

/// Pending GameScript callbacks. **Header decoded, records carried raw.**
///
/// **Observed in a local binary, 2026-09-18.** The eight-word header, measured across all eight
/// files:
///
/// | file | header |
/// | --- | --- |
/// | combat | `0, 1, 69, 15, 1, 999932, 0, 16` |
/// | experience | `0, 1, 91, 15, 1, 999910, 0, 16` |
/// | magic | `0, 1, 3, 15, 1, 999998, 0, 16` |
/// | merc | `0, 1, 3, 15, 1, 999998, 0, 16` |
/// | temple | `0, 7, 6, 15, 1, 999995, 0, 16` |
/// | quickstart | `0, 1, 1, 15, 1, 1000000, 0, 16` |
/// | lastsave / Merlin I | `0, 1, 315, 15, 1, 999686, 0, 16` |
///
/// **The turn sits at index 2, and word 5 is exactly `1000001 - turn`** in all eight. Index 1 is 1
/// in seven files and 7 in `temple.sav`, so it is left unnamed.
///
/// **Corrected 2026-09-18.** An earlier reading put the turn at index 1 and called word 5 a
/// constant 1,000,000. Both are wrong, and the way they went wrong is the lesson: the file that had
/// been leaned on is `quickstart`, which is at **turn 1**, and the value 1 appears at three separate
/// indexes in its header. That fixture could not have located the turn field; it agreed with
/// several readings at once and the wrong one was picked. *A fixture shaped like the corpus cannot
/// fail on what the corpus hides* -- here, a one-file corpus whose turn number was the same as its
/// neighbouring constants.
///
/// The correction makes the turn reading **stronger**: it now appears three times per file --
/// `LS_GAME[0]`, `LS_ALRM[2]`, and derived from `LS_ALRM[5]` -- agreeing across all eight.
///
/// Records follow the header and end in `u32 len` + `len` raw bytes of a GameScript callback name:
/// `monstergenerator` (16), `experience_attack_callback` (26), `village_security_brain` (22),
/// `engage_special_building_brain` (29), `thief_steal_from_enemy_event`, `dpw_brain`,
/// `explore_brain`, `antispy_brain`. **The record layout is not determined**: the number of fixed
/// `u32` fields before the length word varies between records -- 2 in one case, 10 in another -- so
/// alarm records carry variable argument lists. The count tracks activity: 96 at turn 1, 394 at
/// turn 69.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmSection {
    pub header: [u32; AlarmSection::HEADER_WORDS],
    /// Everything after the header. Layout **Unknown**.
    pub records_raw: Vec<u8>,
}

impl AlarmSection {
    pub const HEADER_WORDS: usize = 8;
    pub const HEADER_LEN: usize = 4 * Self::HEADER_WORDS;
    /// The index of the turn word within the header.
    pub const TURN_INDEX: usize = 2;
    /// The index of the word that equals [`COUNTDOWN_BASE`] minus the turn.
    pub const COUNTDOWN_INDEX: usize = 5;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Alarm;
        let mut header = [0_u32; Self::HEADER_WORDS];
        for (index, word) in header.iter_mut().enumerate() {
            *word = read_u32(tag, payload, index * 4)?;
        }
        Ok(Self {
            header,
            records_raw: payload[Self::HEADER_LEN..].to_vec(),
        })
    }

    pub fn turn(&self) -> u32 {
        self.header[Self::TURN_INDEX]
    }

    pub fn countdown(&self) -> u32 {
        self.header[Self::COUNTDOWN_INDEX]
    }

    /// The turn implied by the countdown word, or `None` if the word is out of range.
    pub fn turn_from_countdown(&self) -> Option<u32> {
        COUNTDOWN_BASE.checked_sub(self.countdown())
    }
}

/// The constant the alarm countdown word is measured down from: `countdown == 1000001 - turn`.
///
/// **Observed in a local binary, 2026-09-18**, exact in all eight files. Whether the engine stores
/// a deadline or a remaining budget is **Unknown**; the name records the arithmetic only.
pub const COUNTDOWN_BASE: u32 = 1_000_001;

// ---------------------------------------------------------------------------
// The whole file
// ---------------------------------------------------------------------------

/// A parsed `.sav`.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveFile {
    pub container: SaveContainer,
    pub version: VersionSection,
    pub multiplayer: MultiplayerSection,
    pub map: MapSection,
    pub sprites: SpriteSection,
    pub users: UserSection,
    pub game: GameSection,
    pub players: PlayerSection,
    pub regions: RegionSection,
    pub alarms: AlarmSection,
}

impl SaveFile {
    pub fn parse(source: &[u8]) -> Result<Self, SaveError> {
        let container = SaveContainer::locate(source)?;
        Ok(Self {
            version: VersionSection::parse(container.payload(source, SectionTag::Version)?)?,
            multiplayer: MultiplayerSection::parse(
                container.payload(source, SectionTag::Multiplayer)?,
            )?,
            map: MapSection::parse(container.payload(source, SectionTag::Map)?)?,
            sprites: SpriteSection::parse(container.payload(source, SectionTag::Sprites)?)?,
            users: UserSection::parse(container.payload(source, SectionTag::User)?)?,
            game: GameSection::parse(container.payload(source, SectionTag::Game)?)?,
            players: PlayerSection::parse(container.payload(source, SectionTag::Player)?)?,
            regions: RegionSection::parse(container.payload(source, SectionTag::Region)?)?,
            alarms: AlarmSection::parse(container.payload(source, SectionTag::Alarm)?)?,
            container,
        })
    }

    /// The three independent turn readings, for a caller that wants to show them rather than a
    /// boolean.
    ///
    /// `(LS_GAME[0], LS_ALRM[2], 1000001 - LS_ALRM[5])`. All three agree in all eight inspected
    /// files, which is what makes the turn reading **Observed** and not a guess.
    pub fn turn_readings(&self) -> (u32, u32, Option<u32>) {
        (
            self.game.turn,
            self.alarms.turn(),
            self.alarms.turn_from_countdown(),
        )
    }

    pub fn turn_agreement(&self) -> bool {
        let (game, alarm, countdown) = self.turn_readings();
        game == alarm && countdown == Some(game)
    }

    /// Every invariant this module knows how to check, each carrying its **measured value**.
    ///
    /// Measured values are not optional decoration. The `LS_ALRM` off-by-one that this module
    /// corrects passed a pass/fail check for months -- *some* field equalled the turn, so the
    /// invariant "held"; it was the wrong field. Printing the number is what catches that.
    pub fn invariants(&self) -> Vec<Invariant> {
        let mut checks = Vec::new();

        checks.push(Invariant::new(
            "container: nine tags, each exactly once",
            format!("{}/9", self.container.census().tags_seen_exactly_once()),
            self.container.census().is_well_formed(),
        ));

        let mult = self.container.location(SectionTag::Multiplayer);
        checks.push(Invariant::new(
            "LS_MULT: 4 + declared_setup + 16*36 == payload",
            format!(
                "4+{}+576={} vs {}",
                self.multiplayer.setup.len(),
                self.multiplayer.accounted_len(),
                mult.payload_len
            ),
            self.multiplayer.accounted_len() == mult.payload_len,
        ));

        let map = self.container.location(SectionTag::Map);
        checks.push(Invariant::new(
            "LS_MAP_: 12 + w*h*8 + 4 + 4*count + 4 == payload",
            format!(
                "12+{}+4+{}+4={} vs {}",
                self.map.map.cells.len() * 8,
                self.map.plane.len() * 4,
                self.map.accounted_len(),
                map.payload_len
            ),
            self.map.accounted_len() == map.payload_len,
        ));
        checks.push(Invariant::new(
            "LS_MAP_: plane count == cell count",
            format!(
                "count={} cells={}",
                self.map.plane_count,
                self.map.map.cells.len()
            ),
            self.map.plane_covers_every_cell(),
        ));

        let user = self.container.location(SectionTag::User);
        checks.push(Invariant::new(
            "LS_USER: payload == 8 * 784",
            format!(
                "{} = {} x {}",
                user.payload_len,
                self.users.records.len(),
                UserRecord::LEN
            ),
            user.payload_len == UserSection::RECORD_COUNT * UserRecord::LEN,
        ));
        checks.push(Invariant::new(
            "LS_USER: record[i].index == i",
            format!(
                "{:?}",
                self.users
                    .records
                    .iter()
                    .map(UserRecord::index)
                    .collect::<Vec<_>>()
            ),
            self.users.indexes_are_positional(),
        ));

        let game = self.container.location(SectionTag::Game);
        checks.push(Invariant::new(
            "LS_GAME: (payload - 24) % 12 == 0",
            format!(
                "({} - 24) % 12 = {}",
                game.payload_len,
                game.payload_len.saturating_sub(GameSection::FIXED_BYTES) % GameSection::RECORD_LEN
            ),
            self.game.accounted_len() == game.payload_len,
        ));
        checks.push(Invariant::new(
            "LS_GAME: N - live_count == 71",
            format!(
                "{} - {} = {}",
                self.game.records.len(),
                self.game.live_count,
                self.game.record_surplus()
            ),
            self.game.record_surplus() == OBSERVED_GAME_RECORD_SURPLUS,
        ));
        checks.push(Invariant::new(
            "LS_GAME: stored record size == 12",
            format!("{}", self.game.declared_record_size),
            usize::try_from(self.game.declared_record_size) == Ok(GameSection::RECORD_LEN),
        ));

        let player = self.container.location(SectionTag::Player);
        checks.push(Invariant::new(
            "LS_PLR_: -1 terminator at payload_end - 36",
            format!(
                "+{} of {} holds {:#x}",
                self.players.sentinel_offset(),
                player.payload_len,
                self.players.sentinel
            ),
            self.players.sentinel_offset() + PlayerSection::TAIL_LEN == player.payload_len
                && self.players.sentinel == PlayerSection::SENTINEL,
        ));
        checks.push(Invariant::new(
            "LS_PLR_: 8 lord codes == LS_MULT slots 0..8",
            format!("{:?}", self.players.lord_codes),
            self.players.lord_codes == self.multiplayer.seated_lord_codes(),
        ));

        let region = self.container.location(SectionTag::Region);
        checks.push(Invariant::new(
            "LS_REGN: 8 + w*h*6 + tail == payload",
            format!(
                "8+{}+{}={} vs {}",
                self.regions.grid_len(),
                self.regions.tail_len(),
                8 + self.regions.grid_len() + self.regions.tail_len(),
                region.payload_len
            ),
            8 + self.regions.grid_len() + self.regions.tail_len() == region.payload_len,
        ));

        let (game_turn, alarm_turn, countdown_turn) = self.turn_readings();
        checks.push(Invariant::new(
            "turn: LS_GAME[0] == LS_ALRM[2] == 1000001 - LS_ALRM[5]",
            format!(
                "{game_turn} / {alarm_turn} / {}",
                countdown_turn
                    .map(|turn| turn.to_string())
                    .unwrap_or_else(|| "out-of-range".to_owned())
            ),
            self.turn_agreement(),
        ));
        checks.push(Invariant::new(
            "LS_ALRM: header[3,4,6,7] == 15, 1, 0, 16",
            format!("{:?}", self.alarms.header),
            self.alarms.header[3] == 15
                && self.alarms.header[4] == 1
                && self.alarms.header[6] == 0
                && self.alarms.header[7] == 16,
        ));

        checks.push(Invariant::new(
            "LS_MAP_: visibility field takes only 0, 63, 128",
            format!("{:?}", self.map.visibility_histogram()),
            self.map
                .visibility_histogram()
                .keys()
                .all(|level| OBSERVED_VISIBILITY_LEVELS.contains(level)),
        ));

        checks
    }
}

/// One invariant check, carrying **what was measured** as well as whether it held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invariant {
    pub name: &'static str,
    /// The measured value, formatted. Always populated, including on a pass.
    pub measured: String,
    pub passed: bool,
}

impl Invariant {
    fn new(name: &'static str, measured: String, passed: bool) -> Self {
        Self {
            name,
            measured,
            passed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic save built to be **unlike the corpus in every way the corpus is uniform**.
    ///
    /// Every real save is 128x128, stores its nine sections in one fixed order, and has a
    /// non-trivial sprite table. A fixture that copied those properties could not fail on any of
    /// them, which is exactly how the `LS_ALRM` turn index and the `x * height + y` cell packing
    /// both survived. So the default here is a **non-square 96x64 map**, a **permuted section
    /// order**, and turn values chosen so that no two header words coincide.
    struct Fixture {
        version: u32,
        map_width: u32,
        map_height: u32,
        region_width: u32,
        region_height: u32,
        region_tail: usize,
        turn: u32,
        /// `LS_ALRM` header words 0, 1, 3, 4, 6, 7. Word 2 is the turn and word 5 is derived.
        alarm_filler: [u32; 6],
        game_live_count: u32,
        game_records: u32,
        sprite_count: u32,
        sprite_body: usize,
        declared_setup_len: u32,
        lord_codes: [u32; 16],
        lord_names: [&'static str; 16],
        /// Bytes written into each name field *after* the terminator, standing in for the
        /// uninitialised process memory the engine leaks there.
        name_padding_fill: u8,
        player_records: usize,
        order: Vec<SectionTag>,
    }

    impl Default for Fixture {
        fn default() -> Self {
            Self {
                version: 111,
                map_width: 96,
                map_height: 64,
                region_width: 96,
                region_height: 64,
                region_tail: 37,
                turn: 42,
                alarm_filler: [0, 7, 15, 1, 0, 16],
                game_live_count: 9,
                game_records: 80,
                sprite_count: 5,
                sprite_body: 123,
                declared_setup_len: 164,
                lord_codes: [
                    1,
                    2,
                    3,
                    4,
                    5,
                    6,
                    7,
                    8,
                    0xffff_ffff,
                    0xffff_ffff,
                    0xffff_ffff,
                    0xffff_ffff,
                    0xffff_ffff,
                    0xffff_ffff,
                    0xffff_ffff,
                    0xffff_ffff,
                ],
                lord_names: [
                    "a", "bb", "ccc", "d", "e", "f", "g", "h", "", "", "", "", "", "", "", "",
                ],
                name_padding_fill: 0xcd,
                player_records: 40,
                // Deliberately not the corpus order, and deliberately not the array order either.
                order: vec![
                    SectionTag::Alarm,
                    SectionTag::Region,
                    SectionTag::Player,
                    SectionTag::Game,
                    SectionTag::User,
                    SectionTag::Sprites,
                    SectionTag::Map,
                    SectionTag::Multiplayer,
                    SectionTag::Version,
                ],
            }
        }
    }

    fn push_u32(buffer: &mut Vec<u8>, value: u32) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }

    impl Fixture {
        fn payload(&self, tag: SectionTag) -> Vec<u8> {
            let mut out = Vec::new();
            match tag {
                SectionTag::Version => push_u32(&mut out, self.version),
                SectionTag::Multiplayer => {
                    push_u32(&mut out, self.declared_setup_len);
                    out.extend(
                        (0..self.declared_setup_len).map(|index| (index % 251) as u8 | 0x80),
                    );
                    for slot in 0..MultiplayerSection::SLOT_COUNT {
                        push_u32(&mut out, self.lord_codes[slot]);
                        let name = self.lord_names[slot].as_bytes();
                        let mut field = [self.name_padding_fill; LordSlot::NAME_LEN];
                        field[..name.len()].copy_from_slice(name);
                        field[name.len()] = 0;
                        out.extend_from_slice(&field);
                    }
                }
                SectionTag::Map => {
                    let cells = self.map_width * self.map_height;
                    push_u32(&mut out, self.map_width);
                    push_u32(&mut out, self.map_height);
                    push_u32(&mut out, 8);
                    for index in 0..cells {
                        // Visibility in the high half, tile slot in the low half, and the two are
                        // deliberately different numbers so a reader that confused them fails.
                        let visibility = OBSERVED_VISIBILITY_LEVELS[(index % 3) as usize] as u16;
                        push_u32(&mut out, (u32::from(visibility) << 16) | (index % 600));
                        push_u32(&mut out, 1.5_f32.to_bits());
                    }
                    push_u32(&mut out, cells);
                    for index in 0..cells {
                        push_u32(&mut out, index * 3);
                    }
                    push_u32(&mut out, 1);
                }
                SectionTag::Sprites => {
                    push_u32(&mut out, self.sprite_count);
                    out.extend((0..self.sprite_body).map(|index| (index % 97) as u8 | 0x80));
                }
                SectionTag::User => {
                    for index in 0..UserSection::RECORD_COUNT {
                        let mut record = vec![0_u8; UserRecord::LEN];
                        record[0..4].copy_from_slice(&(index as u32).to_le_bytes());
                        record[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
                        record[12..16].copy_from_slice(&1.0_f32.to_bits().to_le_bytes());
                        out.extend_from_slice(&record);
                    }
                }
                SectionTag::Game => {
                    push_u32(&mut out, self.turn);
                    push_u32(&mut out, 1234);
                    push_u32(&mut out, 0);
                    push_u32(&mut out, self.game_live_count);
                    push_u32(&mut out, 12);
                    for index in 0..self.game_records {
                        push_u32(&mut out, 5000 - index);
                        push_u32(&mut out, index);
                        push_u32(&mut out, index * 2);
                    }
                    push_u32(&mut out, 77);
                }
                SectionTag::Player => {
                    out.extend((0..self.player_records).map(|index| (index % 89) as u8 | 0x80));
                    push_u32(&mut out, PlayerSection::SENTINEL);
                    for code in self.lord_codes.iter().take(8) {
                        push_u32(&mut out, *code);
                    }
                }
                SectionTag::Region => {
                    push_u32(&mut out, self.region_width);
                    push_u32(&mut out, self.region_height);
                    let cells = self.region_width as usize * self.region_height as usize;
                    out.extend(
                        (0..cells * RegionSection::CELL_LEN).map(|i| (i % 211) as u8 | 0x80),
                    );
                    out.extend((0..self.region_tail).map(|i| (i % 83) as u8 | 0x80));
                }
                SectionTag::Alarm => {
                    let [w0, w1, w3, w4, w6, w7] = self.alarm_filler;
                    for word in [
                        w0,
                        w1,
                        self.turn,
                        w3,
                        w4,
                        // Deliberately the literal and not `COUNTDOWN_BASE`. Building the fixture
                        // from the constant under test makes the two move together, and a mutation
                        // of the constant then survives -- which is exactly what happened.
                        1_000_001 - self.turn,
                        w6,
                        w7,
                    ] {
                        push_u32(&mut out, word);
                    }
                    out.extend([0x81, 0x82, 0x83, 0x84]);
                }
            }
            out
        }

        fn build(&self) -> Vec<u8> {
            let mut out = Vec::new();
            for tag in &self.order {
                out.extend_from_slice(&tag.on_disk_bytes());
                out.extend_from_slice(&self.payload(*tag));
            }
            out
        }
    }

    // -- the container ------------------------------------------------------

    #[test]
    fn parses_a_non_square_map_with_the_sections_in_an_arbitrary_order() {
        let fixture = Fixture::default();
        let save = SaveFile::parse(&fixture.build()).unwrap();

        assert_eq!(save.map.map.width, 96);
        assert_eq!(save.map.map.height, 64);
        assert_eq!(save.map.map.cells.len(), 96 * 64);
        assert_eq!(save.container.tag_order(), fixture.order);
        for invariant in save.invariants() {
            assert!(
                invariant.passed,
                "{} failed: {}",
                invariant.name, invariant.measured
            );
        }
    }

    /// Order is irrelevant to the engine, so it must be irrelevant here: the same nine payloads in
    /// two different orders must decode to the same content.
    #[test]
    fn section_order_does_not_change_what_is_decoded() {
        let mut shuffled = Fixture::default();
        shuffled.order.reverse();

        let first = SaveFile::parse(&Fixture::default().build()).unwrap();
        let second = SaveFile::parse(&shuffled.build()).unwrap();

        assert_ne!(first.container.tag_order(), second.container.tag_order());
        assert_eq!(first.game, second.game);
        assert_eq!(first.map, second.map);
        assert_eq!(first.multiplayer, second.multiplayer);
        assert_eq!(first.regions, second.regions);
    }

    /// A section's extent runs to the **next tag**, with no length word anywhere: growing one
    /// payload must change that section's length and nothing else's.
    #[test]
    fn a_section_extends_to_the_next_tag_and_not_to_a_stored_length() {
        let mut grown = Fixture::default();
        grown.sprite_body += 64;

        let before = SaveFile::parse(&Fixture::default().build()).unwrap();
        let after = SaveFile::parse(&grown.build()).unwrap();

        let sprites = |save: &SaveFile| save.container.location(SectionTag::Sprites).payload_len;
        let regions = |save: &SaveFile| save.container.location(SectionTag::Region).payload_len;
        assert_eq!(sprites(&after), sprites(&before) + 64);
        assert_eq!(regions(&after), regions(&before));
        assert_eq!(after.sprites.raw.len(), before.sprites.raw.len() + 64);
        assert_eq!(after.sprites.record_count, before.sprites.record_count);
    }

    #[test]
    fn refuses_a_file_that_is_missing_a_section() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let location = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::User);
        bytes[location.tag_offset] = b'X';

        let error = SaveFile::parse(&bytes).unwrap_err();
        let SaveError::TagCensusFailed(census) = &error else {
            panic!("expected a census failure, got {error}");
        };
        assert_eq!(census.count(SectionTag::User), 0);
        assert_eq!(census.tags_seen_exactly_once(), 8);
        assert!(error.to_string().contains("LS_USER=0"), "{error}");
    }

    /// The census exists precisely because a tag byte-string could occur inside payload data. This
    /// plants one there and checks the parser refuses instead of splitting the section.
    #[test]
    fn refuses_a_tag_that_also_occurs_inside_a_payload() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let region = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Region);
        let planted = region.payload_offset + 512;
        bytes[planted..planted + TAG_SIZE].copy_from_slice(&SectionTag::Game.on_disk_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        let SaveError::TagCensusFailed(census) = &error else {
            panic!("expected a census failure, got {error}");
        };
        assert_eq!(census.count(SectionTag::Game), 2);
        assert!(error.to_string().contains("LS_GAME=2"), "{error}");
    }

    #[test]
    fn the_census_is_available_without_a_successful_parse() {
        let census = TagCensus::take(b"nothing here at all");
        assert!(!census.is_well_formed());
        assert_eq!(census.tags_seen_exactly_once(), 0);
        for tag in SECTION_TAGS {
            assert_eq!(census.count(tag), 0, "{tag}");
        }
    }

    // -- LS_VER_ ------------------------------------------------------------

    /// The engine's version handling has no floor and no ceiling, so neither does this parser.
    #[test]
    fn accepts_any_version_including_far_below_and_far_above_the_corpus() {
        for version in [0, 50, 108, 111, 9999, u32::MAX] {
            let fixture = Fixture {
                version,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            assert_eq!(save.version.version, version);
        }
    }

    /// The version at which the writer starts storing the lord slots, asserted against literal
    /// versions either side of it rather than against the constant -- an assertion phrased in terms
    /// of the constant it is testing cannot fail when the constant moves.
    #[test]
    fn the_multiplayer_slot_block_appears_at_version_99() {
        let stores = |version: u32| {
            let fixture = Fixture {
                version,
                ..Fixture::default()
            };
            SaveFile::parse(&fixture.build())
                .unwrap()
                .version
                .stores_multiplayer_slots()
        };
        assert!(!stores(50));
        assert!(!stores(98));
        assert!(stores(99));
        assert!(stores(108));
        assert!(stores(111));
    }

    #[test]
    fn refuses_a_version_payload_that_is_not_exactly_four_bytes() {
        let mut bytes = Fixture::default().build();
        // LS_VER_ is last in the default order, so appending lengthens exactly it.
        bytes.push(0);
        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_VER_:"), "{error}");
        assert!(error.to_string().contains("exactly 4"), "{error}");
    }

    // -- LS_MULT ------------------------------------------------------------

    /// The stored 164 is a real length the engine reads with, so a shorter or longer one must be
    /// honoured rather than replaced by the constant.
    #[test]
    fn the_multiplayer_setup_block_is_read_at_its_declared_length() {
        for declared in [0, 100, 164, 300] {
            let fixture = Fixture {
                declared_setup_len: declared,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            assert_eq!(save.multiplayer.declared_setup_len, declared);
            assert_eq!(save.multiplayer.setup.len() as u32, declared);
            assert_eq!(
                save.multiplayer.accounted_len(),
                save.container.location(SectionTag::Multiplayer).payload_len
            );
            assert_eq!(save.multiplayer.slots[2].name_lossy(), "ccc");
        }
    }

    /// Reading the declared length as data means a corrupt one must be refused, not trusted into
    /// an out-of-bounds read.
    #[test]
    fn refuses_a_declared_setup_length_the_payload_cannot_hold() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let mult = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Multiplayer);
        bytes[mult.payload_offset..mult.payload_offset + 4]
            .copy_from_slice(&1_000_000_u32.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_MULT:"), "{error}");
        assert!(error.to_string().contains("1000000"), "{error}");
    }

    /// Two saves of one state differ only in the bytes past each name's terminator. A parser that
    /// exposed the raw 32 bytes as the name would call those two files different.
    #[test]
    fn a_name_stops_at_its_terminator_and_the_leaked_padding_is_kept_apart() {
        let first = Fixture::default();
        let second = Fixture {
            name_padding_fill: 0x5a,
            ..Fixture::default()
        };
        let first_bytes = first.build();
        let second_bytes = second.build();
        assert_ne!(
            first_bytes, second_bytes,
            "the fixtures must differ on disk"
        );

        let first = SaveFile::parse(&first_bytes).unwrap();
        let second = SaveFile::parse(&second_bytes).unwrap();

        for slot in 0..MultiplayerSection::SLOT_COUNT {
            let left = &first.multiplayer.slots[slot];
            let right = &second.multiplayer.slots[slot];
            assert_eq!(left.name(), right.name(), "slot {slot} name");
            assert_ne!(
                left.name_padding(),
                right.name_padding(),
                "slot {slot} padding"
            );
            assert!(
                left.name_padding().iter().all(|byte| *byte == 0xcd),
                "slot {slot} padding must be exactly the bytes past the terminator"
            );
            assert_eq!(
                left.name().len() + 1 + left.name_padding().len(),
                LordSlot::NAME_LEN,
                "slot {slot} must account for the whole field"
            );
        }
    }

    /// Occupancy is the code, never the name: two corpus slots carry a live code and no name.
    #[test]
    fn an_occupied_slot_may_carry_an_empty_name() {
        let mut fixture = Fixture::default();
        fixture.lord_names[3] = "";
        fixture.lord_codes[5] = LordSlot::UNUSED_CODE;
        fixture.lord_names[5] = "ghost";

        let save = SaveFile::parse(&fixture.build()).unwrap();

        assert!(save.multiplayer.slots[3].is_occupied());
        assert!(save.multiplayer.slots[3].name().is_empty());
        assert!(!save.multiplayer.slots[5].is_occupied());
        assert_eq!(save.multiplayer.slots[5].name_lossy(), "ghost");
        let occupied: Vec<usize> = save
            .multiplayer
            .occupied_slots()
            .map(|(index, _)| index)
            .collect();
        assert_eq!(occupied, vec![0, 1, 2, 3, 4, 6, 7]);
    }

    // -- LS_MAP_ ------------------------------------------------------------

    /// The whole point of the Corrected note in `docs/save-format.md`: a matching **total** proves
    /// nothing about the **terms**. Here the byte total still accounts exactly, and the structure
    /// is nonetheless wrong, because the plane does not cover the cells.
    #[test]
    fn a_matching_byte_total_does_not_make_the_map_accounting_right() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let map = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Map);
        let cells = (fixture.map_width * fixture.map_height) as usize;
        let count_offset = map.payload_offset + 12 + cells * 8;

        // Halve the plane count and hand the freed words to a longer trailing run, so the payload
        // length is untouched.
        let halved = (cells / 2) as u32;
        bytes[count_offset..count_offset + 4].copy_from_slice(&halved.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        let accounting = |name: &str| {
            save.invariants()
                .into_iter()
                .find(|check| check.name.contains(name))
                .expect("invariant is present")
        };

        // The total no longer accounts, *and* the structural check is independently false. The
        // second is the one that would still fire if a future layout made the totals coincide.
        assert!(!save.map.plane_covers_every_cell());
        assert!(!accounting("plane count == cell count").passed);
        assert!(
            accounting("plane count == cell count")
                .measured
                .contains(&halved.to_string())
        );
    }

    #[test]
    fn refuses_a_map_payload_whose_cell_grid_is_cut_short() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let map = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Map);
        let taller = (fixture.map_height * 4).to_le_bytes();
        bytes[map.payload_offset + 4..map.payload_offset + 8].copy_from_slice(&taller);

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_MAP_:"), "{error}");
        assert!(error.to_string().contains("cell grid"), "{error}");
    }

    /// The visibility field is a **signed** scalar in the upper half of the cell word, and the tile
    /// slot is the lower half. A fixture where the two halves hold the same number could not tell a
    /// reader that confused them from one that did not, so these are deliberately different.
    #[test]
    fn the_visibility_field_is_the_signed_upper_half_and_the_tile_slot_the_lower() {
        let cell = MapCell {
            tag: 0xffff_01a4,
            value_bits: 0,
            value: 0.0,
        };
        assert_eq!(cell_visibility(&cell), -1);
        assert_eq!(cell_tile_index(&cell), 0x01a4);

        let cell = MapCell {
            tag: 0x0080_0003,
            value_bits: 0,
            value: 0.0,
        };
        assert_eq!(cell_visibility(&cell), 128);
        assert_eq!(cell_tile_index(&cell), 3);

        // A slot with bit 15 set. No corpus tile index is this large -- the atlas stops at 623 --
        // but the field is the whole low `u16`, and a fixture that only used small slots could not
        // fail on a mask that dropped the top bit.
        let cell = MapCell {
            tag: 0x003f_8001,
            value_bits: 0,
            value: 0.0,
        };
        assert_eq!(cell_visibility(&cell), 63);
        assert_eq!(cell_tile_index(&cell), 0x8001);
    }

    #[test]
    fn the_visibility_invariant_fails_on_a_level_the_corpus_never_shows() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let map = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Map);
        let cell_tag = map.payload_offset + 12 + 7 * 8;
        bytes[cell_tag + 2..cell_tag + 4].copy_from_slice(&64_u16.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        let histogram = save.map.visibility_histogram();
        assert_eq!(histogram.get(&64), Some(&1));
        let check = save
            .invariants()
            .into_iter()
            .find(|check| check.name.contains("visibility"))
            .expect("invariant is present");
        assert!(!check.passed);
        assert!(check.measured.contains("64"), "{}", check.measured);
    }

    // -- LS_SPR_ ------------------------------------------------------------

    #[test]
    fn accepts_a_sprite_section_holding_no_records() {
        let fixture = Fixture {
            sprite_count: 0,
            sprite_body: 0,
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        assert_eq!(save.sprites.record_count, 0);
        assert!(save.sprites.records_raw().is_empty());
        assert!(save.sprites.candidate_fixed_strides(1024).is_empty());
    }

    /// The stride search must be the arithmetic it claims to be, not a table of corpus results.
    #[test]
    fn the_stride_search_returns_exactly_the_headers_that_divide_evenly() {
        let fixture = Fixture {
            sprite_count: 7,
            sprite_body: 100,
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        let total = save.sprites.raw.len() + 4;
        // Search past the section length, so the bound that excludes a header consuming the whole
        // section is actually exercised. Stopping short of `total` leaves it untested.
        let limit = total + 8;
        let found = save.sprites.candidate_fixed_strides(limit);
        assert!(
            !found.contains(&total),
            "a header filling the whole section leaves no records and is not a stride"
        );

        for header in 0..=limit {
            let divides = total > header && (total - header).is_multiple_of(7);
            assert_eq!(
                found.contains(&header),
                divides,
                "header {header} of {total} bytes over 7 records"
            );
        }
        assert!(!found.is_empty(), "this fixture does admit strides");
    }

    // -- LS_USER ------------------------------------------------------------

    #[test]
    fn refuses_a_user_payload_that_is_not_a_whole_number_of_records() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let user = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::User);
        bytes.insert(user.payload_end(), 0);

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_USER:"), "{error}");
        assert!(error.to_string().contains("784"), "{error}");
    }

    #[test]
    fn the_positional_index_check_fails_when_a_record_is_misnumbered() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let user = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::User);
        let third = user.payload_offset + 3 * UserRecord::LEN;
        bytes[third..third + 4].copy_from_slice(&99_u32.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        assert!(!save.users.indexes_are_positional());
        let check = save
            .invariants()
            .into_iter()
            .find(|check| check.name.contains("record[i].index"))
            .expect("invariant is present");
        assert!(!check.passed);
        assert!(check.measured.contains("99"), "{}", check.measured);
    }

    // -- LS_GAME ------------------------------------------------------------

    /// The surplus must be **computed from the parsed data**, so that a file whose surplus is not
    /// 71 reports the number it actually has.
    #[test]
    fn the_game_record_surplus_is_measured_rather_than_assumed() {
        let fixture = Fixture {
            game_records: 80,
            game_live_count: 9,
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        assert_eq!(save.game.records.len(), 80);
        assert_eq!(save.game.record_surplus(), 71);

        let broken = Fixture {
            game_records: 80,
            game_live_count: 40,
            ..Fixture::default()
        };
        let save = SaveFile::parse(&broken.build()).unwrap();
        assert_eq!(save.game.record_surplus(), 40);
        let check = save
            .invariants()
            .into_iter()
            .find(|check| check.name.contains("N - live_count"))
            .expect("invariant is present");
        assert!(!check.passed);
        assert!(
            check.measured.contains("80 - 40 = 40"),
            "{}",
            check.measured
        );
    }

    /// A live count larger than the record count must produce a negative surplus, not an underflow
    /// and not a saturated zero.
    #[test]
    fn a_live_count_above_the_record_count_gives_a_negative_surplus() {
        let fixture = Fixture {
            game_records: 3,
            game_live_count: 900,
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        assert_eq!(save.game.record_surplus(), -897);
    }

    #[test]
    fn refuses_a_game_payload_that_is_not_a_whole_number_of_records() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let game = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Game);
        bytes.insert(game.payload_end(), 0);

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_GAME:"), "{error}");
        assert!(error.to_string().contains("12-byte records"), "{error}");
    }

    // -- LS_PLR_ ------------------------------------------------------------

    #[test]
    fn the_player_tail_is_read_from_the_end_regardless_of_the_record_size() {
        for records in [0_usize, 1, 40, 4000] {
            let fixture = Fixture {
                player_records: records,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            assert_eq!(save.players.sentinel_offset(), records);
            assert_eq!(save.players.records_raw.len(), records);
            assert_eq!(save.players.lord_codes, [1, 2, 3, 4, 5, 6, 7, 8]);
            assert_eq!(
                save.players.lord_codes,
                save.multiplayer.seated_lord_codes()
            );
        }
    }

    #[test]
    fn refuses_a_player_section_whose_terminator_is_not_at_the_end_minus_36() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let player = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Player);
        let sentinel = player.payload_end() - PlayerSection::TAIL_LEN;
        bytes[sentinel..sentinel + 4].copy_from_slice(&0_u32.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_PLR_:"), "{error}");
        assert!(error.to_string().contains("terminator"), "{error}");
    }

    #[test]
    fn the_lord_code_cross_check_fails_when_the_two_sections_disagree() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let player = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Player);
        let codes = player.payload_end() - PlayerSection::TAIL_LEN + 4;
        bytes[codes..codes + 4].copy_from_slice(&0xabcd_u32.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        let check = save
            .invariants()
            .into_iter()
            .find(|check| check.name.contains("lord codes"))
            .expect("invariant is present");
        assert!(!check.passed);
        assert!(check.measured.contains("43981"), "{}", check.measured);
    }

    // -- LS_REGN ------------------------------------------------------------

    #[test]
    fn the_region_tail_is_carried_whole_at_whatever_length_it_has() {
        for tail in [0_usize, 37, 8998, 9389, 9780] {
            let fixture = Fixture {
                region_tail: tail,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            assert_eq!(save.regions.tail_len(), tail);
            assert_eq!(save.regions.cells.len(), 96 * 64);
            assert_eq!(save.regions.grid_len(), 96 * 64 * 6);
        }
    }

    #[test]
    fn refuses_a_region_grid_the_payload_cannot_hold() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let region = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Region);
        bytes[region.payload_offset..region.payload_offset + 4]
            .copy_from_slice(&4096_u32.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_REGN:"), "{error}");
    }

    // -- LS_ALRM and the turn -----------------------------------------------

    /// The regression test for the off-by-one. The turn is at header index **2**; index 1 holds a
    /// different value here, so a reader that took index 1 -- as an earlier pass did -- fails.
    #[test]
    fn the_turn_is_read_from_alarm_header_index_two_and_not_index_one() {
        let fixture = Fixture {
            turn: 42,
            alarm_filler: [0, 7, 15, 1, 0, 16],
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();

        assert_eq!(save.alarms.header[1], 7);
        assert_eq!(save.alarms.header[AlarmSection::TURN_INDEX], 42);
        assert_eq!(save.alarms.turn(), 42);
        assert_eq!(save.turn_readings(), (42, 42, Some(42)));
        assert!(save.turn_agreement());
    }

    /// `quickstart` is turn 1 and the value 1 appears at three indexes of its header, so it agrees
    /// with several readings at once. This is that shape, and it must be the fixture that *cannot*
    /// distinguish -- proving the discriminating fixture above is doing real work.
    #[test]
    fn a_turn_one_fixture_cannot_locate_the_turn_field() {
        let fixture = Fixture {
            turn: 1,
            alarm_filler: [0, 1, 15, 1, 0, 16],
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();

        let matching: Vec<usize> = (0..AlarmSection::HEADER_WORDS)
            .filter(|index| save.alarms.header[*index] == save.game.turn)
            .collect();
        assert_eq!(
            matching,
            vec![1, 2, 4],
            "a turn-1 save cannot single out the turn word"
        );
    }

    #[test]
    fn the_turn_cross_check_fails_when_the_alarm_turn_disagrees() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let alarm = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Alarm);
        let turn_word = alarm.payload_offset + 4 * AlarmSection::TURN_INDEX;
        bytes[turn_word..turn_word + 4].copy_from_slice(&43_u32.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        assert!(!save.turn_agreement());
        let check = save
            .invariants()
            .into_iter()
            .find(|check| check.name.starts_with("turn:"))
            .expect("invariant is present");
        assert!(!check.passed);
        assert_eq!(check.measured, "42 / 43 / 42");
    }

    #[test]
    fn the_turn_cross_check_fails_when_the_countdown_disagrees() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let alarm = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Alarm);
        let countdown = alarm.payload_offset + 4 * AlarmSection::COUNTDOWN_INDEX;
        bytes[countdown..countdown + 4].copy_from_slice(&(COUNTDOWN_BASE - 9).to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        assert_eq!(save.turn_readings(), (42, 42, Some(9)));
        assert!(!save.turn_agreement());
    }

    /// A countdown word above the base would underflow a naive subtraction.
    #[test]
    fn an_out_of_range_countdown_word_yields_no_turn_rather_than_wrapping() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let alarm = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Alarm);
        let countdown = alarm.payload_offset + 4 * AlarmSection::COUNTDOWN_INDEX;
        bytes[countdown..countdown + 4].copy_from_slice(&u32::MAX.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        assert_eq!(save.alarms.turn_from_countdown(), None);
        assert!(!save.turn_agreement());
        let check = save
            .invariants()
            .into_iter()
            .find(|check| check.name.starts_with("turn:"))
            .expect("invariant is present");
        assert!(
            check.measured.contains("out-of-range"),
            "{}",
            check.measured
        );
    }

    #[test]
    fn alarm_records_are_carried_after_the_eight_word_header() {
        let save = SaveFile::parse(&Fixture::default().build()).unwrap();
        assert_eq!(save.alarms.records_raw, vec![0x81, 0x82, 0x83, 0x84]);
        assert_eq!(AlarmSection::HEADER_LEN, 32);
    }

    // -- tags ---------------------------------------------------------------

    #[test]
    fn a_tag_is_seven_compared_bytes_padded_to_eight_with_a_nul() {
        for tag in SECTION_TAGS {
            let on_disk = tag.on_disk_bytes();
            assert_eq!(&on_disk[..TAG_COMPARED_BYTES], tag.compared_bytes());
            assert_eq!(on_disk[7], 0, "{tag} must be NUL-padded");
            assert_eq!(tag.name().len(), TAG_COMPARED_BYTES);
        }
        let distinct: std::collections::BTreeSet<_> = SECTION_TAGS
            .iter()
            .map(|tag| tag.compared_bytes())
            .collect();
        assert_eq!(distinct.len(), SECTION_TAGS.len());
    }
}
