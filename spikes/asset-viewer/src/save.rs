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
//! That design means a section cannot be skipped without decoding it. All nine now decode, but
//! this parser still does **not** reimplement the loop: it **scans the whole file for the nine tag
//! byte-strings** and takes each section's extent as running from its payload to the next tag found,
//! or to end of file for the last one. Scanning is what lets it report on a file one of whose
//! sections it refuses, which sequential decoding cannot do.
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
//! | [`SpriteSection`] | fully decoded, through ten class readers; field *meanings* Unknown |
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

    /// The **structural checks**: the format's own requirements, re-derived from the raw bytes.
    ///
    /// Two deliberate properties.
    ///
    /// **They are computed here, not from the parsed sections.** An invariant read off a struct
    /// that `parse` already validated cannot report a failure -- `parse` rejected the file before
    /// the caller ever saw it. These re-read the words out of `source` and redo the arithmetic, so
    /// they are a second implementation that can genuinely disagree with the first.
    ///
    /// **They hang off the container, not off [`SaveFile`], so they run when parsing fails.** That
    /// is the whole point: on a file this parser refuses, these are what say *which* section is
    /// malformed and by how much. A check reachable only after a successful parse is a check that
    /// never fires on the files that need it.
    pub fn structural_checks(&self, source: &[u8]) -> Vec<Invariant> {
        let word = |offset: usize| -> Option<u32> {
            source
                .get(offset..offset + 4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
        };
        let structural = |name, measured, passed| {
            Invariant::new(name, InvariantKind::Structural, measured, passed)
        };
        let mut checks = Vec::new();

        checks.push(structural(
            "container: nine tags, each exactly once",
            format!(
                "{}/9 ({})",
                self.census.tags_seen_exactly_once(),
                self.census.describe_anomalies()
            ),
            self.census.is_well_formed(),
        ));

        let mult = self.location(SectionTag::Multiplayer);
        let declared = word(mult.payload_offset);
        // The slot block's presence depends on the version, which lives in a different section --
        // so this arithmetic has to reach across to `LS_VER_` exactly as the engine's handlers do
        // through their shared singleton.
        let version = word(self.location(SectionTag::Version).payload_offset);
        let slot_bytes = match version {
            Some(version) if (version as i32) >= (MULTIPLAYER_SLOTS_MIN_VERSION as i32) => {
                MultiplayerSection::SLOT_COUNT * LordSlot::RECORD_LEN
            }
            Some(_) => 0,
            None => 0,
        };
        checks.push(structural(
            "LS_MULT: 4 + declared_setup + slot block == payload",
            match declared {
                Some(declared) => format!(
                    "4+{declared}+{slot_bytes}={} vs {}",
                    4 + declared as usize + slot_bytes,
                    mult.payload_len
                ),
                None => "unreadable".to_owned(),
            },
            declared.is_some_and(|declared| 4 + declared as usize + slot_bytes == mult.payload_len),
        ));

        let map = self.location(SectionTag::Map);
        let map_accounting = (|| {
            let width = word(map.payload_offset)? as usize;
            let height = word(map.payload_offset + 4)? as usize;
            let per_cell = word(map.payload_offset + 8)? as usize;
            let grid = width.checked_mul(height)?.checked_mul(per_cell)?;
            let count = word(map.payload_offset + 12 + grid)? as usize;
            let total = 12 + grid + 4 + count.checked_mul(4)? + 4;
            Some((grid, count, total))
        })();
        checks.push(structural(
            "LS_MAP_: 12 + w*h*bpc + 4 + 4*count + 4 == payload",
            match map_accounting {
                Some((grid, count, total)) => {
                    format!("12+{grid}+4+{}+4={total} vs {}", count * 4, map.payload_len)
                }
                None => "unreadable".to_owned(),
            },
            map_accounting.is_some_and(|(_, _, total)| total == map.payload_len),
        ));

        let user = self.location(SectionTag::User);
        let expected_user = UserSection::RECORD_COUNT * UserRecord::LEN;
        checks.push(structural(
            "LS_USER: payload == 8 records of 784",
            format!("{} vs {expected_user}", user.payload_len),
            user.payload_len == expected_user,
        ));

        let game = self.location(SectionTag::Game);
        let game_records = game.payload_len.checked_sub(GameSection::FIXED_BYTES);
        checks.push(structural(
            "LS_GAME: (payload - 24) % 12 == 0",
            match game_records {
                Some(bytes) => format!(
                    "({} - 24) = {bytes}, % 12 = {}",
                    game.payload_len,
                    bytes % GameSection::RECORD_LEN
                ),
                None => format!("{} is shorter than the 24-byte head", game.payload_len),
            },
            game_records.is_some_and(|bytes| bytes.is_multiple_of(GameSection::RECORD_LEN)),
        ));

        let player = self.location(SectionTag::Player);
        let sentinel = player
            .payload_len
            .checked_sub(PlayerSection::TAIL_LEN)
            .and_then(|offset| word(player.payload_offset + offset));
        checks.push(structural(
            "LS_PLR_: 0xffffffff terminator at payload_end - 36",
            match sentinel {
                Some(value) => format!("{value:#x} at +{}", player.payload_len - 36),
                None => format!("{} is shorter than the 36-byte tail", player.payload_len),
            },
            sentinel == Some(PlayerSection::SENTINEL),
        ));

        let region = self.location(SectionTag::Region);
        // **Not** `8 + grid + tail == payload`. The tail is *defined* as the bytes left over, so
        // that equation is true by construction and can never report anything. What is genuinely
        // checkable is that the declared grid fits at all.
        let region_grid = (|| {
            let width = word(region.payload_offset)? as usize;
            let height = word(region.payload_offset + 4)? as usize;
            width
                .checked_mul(height)?
                .checked_mul(RegionSection::CELL_LEN)
        })();
        checks.push(structural(
            "LS_REGN: 8 + w*h*6 fits inside the payload",
            match region_grid {
                Some(grid) => format!("8+{grid}={} vs {}", 8 + grid, region.payload_len),
                None => "unreadable".to_owned(),
            },
            region_grid.is_some_and(|grid| 8 + grid <= region.payload_len),
        ));

        let alarm = self.location(SectionTag::Alarm);
        let alarm_walk = account_for_alarm_queues(
            source
                .get(alarm.payload_offset..alarm.payload_end())
                .unwrap_or_default(),
        );
        checks.push(structural(
            "LS_ALRM: the six queues account for the payload",
            match alarm_walk {
                Some(consumed) => format!("{consumed} vs {}", alarm.payload_len),
                None => "ran off the end".to_owned(),
            },
            alarm_walk == Some(alarm.payload_len),
        ));

        let region_tail_walk = region_grid.and_then(|grid| {
            source
                .get(region.payload_offset + 8 + grid..region.payload_end())
                .and_then(account_for_region_table)
        });
        checks.push(structural(
            "LS_REGN: the region table accounts for the tail",
            match (region_tail_walk, region_grid) {
                (Some(consumed), Some(grid)) => {
                    format!("{consumed} vs {}", region.payload_len - 8 - grid)
                }
                _ => "ran off the end".to_owned(),
            },
            match (region_tail_walk, region_grid) {
                (Some(consumed), Some(grid)) => consumed == region.payload_len - 8 - grid,
                _ => false,
            },
        ));

        let version_section = self.location(SectionTag::Version);
        checks.push(structural(
            "LS_VER_: payload is exactly 4 bytes",
            format!("{}", version_section.payload_len),
            version_section.payload_len == VersionSection::PAYLOAD_LEN,
        ));

        checks
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
    ///
    /// **Compared as `i32`, deliberately, because the engine's gates are `jl`/`jge` and those are
    /// SIGNED.** It matters at the top of the range: for a stored version of `0xFFFFFFFF` the
    /// engine sees `-1 < 99` and takes the *low* path. An unsigned comparison here would answer
    /// the opposite and disagree with the engine on exactly the inputs a hostile file would use.
    pub fn stores_multiplayer_slots(&self) -> bool {
        (self.version as i32) >= (MULTIPLAYER_SLOTS_MIN_VERSION as i32)
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
    /// `LS_MAP_` and every byte of `LS_SPR_` -- and their entire difference is **356 bytes, every
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
/// **Observed in a local binary, 2026-09-18.** 744 bytes in every corpus file, and the only
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
    /// The sixteen lord slots, or `None` when the format version omits the block.
    ///
    /// **Observed in a local binary, 2026-09-18; the low-version path is UNEXERCISED by any
    /// sample.** Below version 99 the reader synthesizes this block from memory instead of reading
    /// it, so a pre-99 save's `LS_MULT` payload is `4 + declared_setup_len` and stops. This parser
    /// implements that from the disassembly alone -- the corpus contains only versions 108 and 111,
    /// both of which store the block, so **nothing here has ever been checked against a real
    /// pre-99 file.**
    pub slots: Option<[LordSlot; MultiplayerSection::SLOT_COUNT]>,
}

impl MultiplayerSection {
    pub const SLOT_COUNT: usize = 16;
    /// How many slots a game can actually seat. Slots 8..16 carry [`LordSlot::UNUSED_CODE`] in
    /// every inspected file.
    pub const SEATED_SLOT_COUNT: usize = 8;

    /// Parse, using `version` to decide whether the slot block is present at all.
    ///
    /// The version is a parameter rather than something re-read here because `LS_VER_` is a
    /// different section: the engine's handlers share the singleton at `0x005AA12C`, and this is
    /// the one cross-section dependency this parser reproduces.
    pub fn parse(payload: &[u8], version: &VersionSection) -> Result<Self, SaveError> {
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

        if !version.stores_multiplayer_slots() {
            return Ok(Self {
                declared_setup_len,
                setup,
                slots: None,
            });
        }

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
            slots: Some(slots),
        })
    }

    /// The bytes this section accounts for: `4 + declared_setup_len`, plus `16 * 36` when the
    /// version stores the slot block.
    pub fn accounted_len(&self) -> usize {
        4 + self.setup.len()
            + match self.slots {
                Some(_) => Self::SLOT_COUNT * LordSlot::RECORD_LEN,
                None => 0,
            }
    }

    /// The occupied slots, paired with their index. Empty when the version omits the block.
    pub fn occupied_slots(&self) -> impl Iterator<Item = (usize, &LordSlot)> {
        self.slots
            .iter()
            .flatten()
            .enumerate()
            .filter(|(_, slot)| slot.is_occupied())
    }

    /// The lord codes of slots `0..8`, in order, for comparison against [`PlayerSection`], or
    /// `None` when the version omits the block.
    pub fn seated_lord_codes(&self) -> Option<[u32; Self::SEATED_SLOT_COUNT]> {
        let slots = self.slots.as_ref()?;
        let mut codes = [0_u32; Self::SEATED_SLOT_COUNT];
        for (index, code) in codes.iter_mut().enumerate() {
            *code = slots[index].lord_code;
        }
        Some(codes)
    }
}

// ---------------------------------------------------------------------------
// LS_MAP_
// ---------------------------------------------------------------------------

/// The three values the cell visibility field takes across the whole corpus.
///
/// **Observed in a local binary, 2026-09-18.** Across all 131,072 cells of every corpus file
/// the field holds **0, 63 or 128 and nothing else**. What those mean is **Inferred**; see
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
/// `4+4+4+131072+4+65536+4 = 196,628`, which is the payload length in every corpus file.
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

/// One entry of the `LS_SPR_` class-dispatch table at `0x004F73B8`.
///
/// **Observed in a local binary, 2026-09-18.** The section's reader at `0x004F7122` reads a
/// `u32 class_id`, bounds it with `cmp eax,9 / ja`, and jumps through this ten-entry table. Each
/// arm allocates an object of a class-specific size, runs that class's constructor, stores the
/// owning table at `object+0x40`, and calls the object's **reader at `vtable+0x24`**. The writer
/// at `0x004F6BC0` is the mirror image and calls `vtable+0x20`.
///
/// `in_memory_size` is the argument to the allocator. It is a size in RAM and is **not** the
/// on-disk record length -- every record is shorter than its object, and three classes are
/// variable-length on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteClassEntry {
    pub class_id: u32,
    /// The jump-table target for this id.
    pub jump_target: u32,
    /// The allocation size the arm passes to the allocator, or `None` for the two invalid ids.
    pub in_memory_size: Option<u32>,
    /// The constructor the arm calls, or `None` for the two invalid ids.
    pub constructor: Option<u32>,
    /// The class's vtable, or `None` for the two invalid ids.
    pub vtable: Option<u32>,
    /// `vtable+0x20`, the writer the save writer calls.
    pub writer: Option<u32>,
    /// `vtable+0x24`, the reader the save reader calls.
    pub reader: Option<u32>,
    /// The on-disk reader the `vtable+0x24` entry point delegates to, when it is a wrapper that
    /// also does post-load registration. Equal to `reader` when there is no wrapper.
    pub on_disk_reader: Option<u32>,
}

/// The ten entries of `0x004F73B8`, in class-id order.
///
/// **Observed in a local binary, 2026-09-18**, read out of `lomse.exe` under
/// `Lords of Magic Development.app`. Ids **5 and 6 share one target**, `0x004F73A3`, which raises
/// rather than allocating -- they are not classes. Id **7** is the odd one: its arm calls a
/// pooled factory at `0x0047CBD0` instead of allocating, so the size below is the pool's block
/// size taken from `0x0047E9E8`, and its arm skips the exception-state store the other eight do.
pub const SPRITE_CLASS_DISPATCH: [SpriteClassEntry; 10] = [
    SpriteClassEntry {
        class_id: 0,
        jump_target: 0x004F_71B0,
        in_memory_size: Some(0x5DC),
        constructor: Some(0x0041_1610),
        vtable: Some(0x0054_D3A8),
        writer: Some(0x0041_2090),
        reader: Some(0x0041_2590),
        on_disk_reader: Some(0x0041_22A0),
    },
    SpriteClassEntry {
        class_id: 1,
        jump_target: 0x004F_71E1,
        in_memory_size: Some(0x60),
        constructor: Some(0x0050_C0B0),
        vtable: Some(0x0054_E910),
        writer: Some(0x0050_D930),
        reader: Some(0x0050_DD70),
        on_disk_reader: Some(0x0050_DA70),
    },
    SpriteClassEntry {
        class_id: 2,
        jump_target: 0x004F_7213,
        in_memory_size: Some(0x94),
        constructor: Some(0x0043_B930),
        vtable: Some(0x0054_D4D8),
        writer: Some(0x0043_D000),
        reader: Some(0x0043_D1A0),
        on_disk_reader: Some(0x0043_D0A0),
    },
    SpriteClassEntry {
        class_id: 3,
        jump_target: 0x004F_7248,
        in_memory_size: Some(0x34C),
        constructor: Some(0x0044_E850),
        vtable: Some(0x0054_D630),
        writer: Some(0x0045_1610),
        reader: Some(0x0045_17A0),
        on_disk_reader: Some(0x0045_16A0),
    },
    SpriteClassEntry {
        class_id: 4,
        jump_target: 0x004F_727D,
        in_memory_size: Some(0x78),
        constructor: Some(0x004E_F6F0),
        vtable: Some(0x0054_DD88),
        writer: Some(0x004F_0C80),
        reader: Some(0x004F_0D80),
        on_disk_reader: Some(0x004F_0D80),
    },
    SpriteClassEntry {
        class_id: 5,
        jump_target: 0x004F_73A3,
        in_memory_size: None,
        constructor: None,
        vtable: None,
        writer: None,
        reader: None,
        on_disk_reader: None,
    },
    SpriteClassEntry {
        class_id: 6,
        jump_target: 0x004F_73A3,
        in_memory_size: None,
        constructor: None,
        vtable: None,
        writer: None,
        reader: None,
        on_disk_reader: None,
    },
    SpriteClassEntry {
        class_id: 7,
        jump_target: 0x004F_72A8,
        in_memory_size: Some(0xC4),
        constructor: Some(0x0047_CA70),
        vtable: Some(0x0054_D968),
        writer: Some(0x0047_CCD0),
        reader: Some(0x0047_CDA0),
        on_disk_reader: Some(0x0047_CDD0),
    },
    SpriteClassEntry {
        class_id: 8,
        jump_target: 0x004F_72AF,
        in_memory_size: Some(0x58),
        constructor: Some(0x004B_A150),
        vtable: Some(0x0054_DC68),
        writer: Some(0x004F_6A80),
        reader: Some(0x004F_6B00),
        on_disk_reader: Some(0x004F_6B00),
    },
    SpriteClassEntry {
        class_id: 9,
        jump_target: 0x004F_72DA,
        in_memory_size: Some(0x148),
        constructor: Some(0x004A_CF00),
        vtable: Some(0x0054_DAF8),
        writer: Some(0x004A_D910),
        reader: Some(0x004A_DB40),
        on_disk_reader: Some(0x004A_DA10),
    },
];

/// The two class ids the dispatch table routes to the raise at `0x004F73A3`.
pub const SPRITE_INVALID_CLASS_IDS: [u32; 2] = [5, 6];

/// The highest class id the reader's `cmp eax,9 / ja` bound admits.
pub const SPRITE_MAX_CLASS_ID: u32 = 9;

/// The six dwords every record begins with, written by the base writer at `0x004F6A80` and read
/// back by the base reader at `0x004F6B00`.
///
/// **Observed in a local binary, 2026-09-18.** The base reader `fread`s four bytes each into
/// `this+4`, `this+0x1C`, `this+0x20`, `this+0x24`, `this+0x28` and `this+0x30`, in that order and
/// with no version gate anywhere in it. Twenty-four bytes, always.
///
/// **`class_id_echo` is the class id a second time, and that is a deduction rather than a
/// prediction.** The container's loop already consumed a `u32 class_id` to choose the class;
/// `this+4` is where the class id lives in the object, so the base reader reads it again.
///
/// **Corrected, 2026-09-18.** An earlier draft of this comment called it "a prediction of the
/// disassembly" that "could have failed and did not". It could not have. The **writer** puts the
/// same field on disk twice -- the outer writer reads `[object+4]` at `0x004F6C25` and the base
/// writer writes `[this+4]` at `0x004F6A8B` -- so every save this engine produces carries the
/// equality by construction, whatever else is right or wrong about the record. Two writers
/// writing one field always agree, and their agreement says nothing about any boundary
/// downstream.
///
/// What measuring it buys is **one-way**. Disagreement proves something is wrong: corruption, a
/// misaligned parse, or another writer. Agreement proves only that those two dwords match -- flip
/// any byte of a record's class-specific body and the check still reports zero. It is not an
/// integrity check on the record, and it is not evidence about the layouts. See
/// [`SpriteRecord::class_id_echo_agrees`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteBase {
    /// `this+4`: the class id again.
    pub class_id_echo: u32,
    /// `this+0x1C`. Meaning **Unknown**.
    pub unknown_1c: u32,
    /// `this+0x20`. Meaning **Unknown**.
    pub unknown_20: u32,
    /// `this+0x24`. Meaning **Unknown**.
    pub unknown_24: u32,
    /// `this+0x28`. Meaning **Unknown**.
    pub unknown_28: u32,
    /// `this+0x30`. A bitfield -- class 0's reader sets and clears bit 3 of it at `0x0041254B`.
    /// Meaning of the individual bits **Unknown**.
    pub unknown_30: u32,
}

impl SpriteBase {
    /// The bytes this block occupies on disk. A constant in the instruction stream, not a stored
    /// length.
    pub const LEN: usize = 24;

    fn to_bytes(self) -> [u8; Self::LEN] {
        let mut out = [0_u8; Self::LEN];
        for (slot, word) in out.chunks_exact_mut(4).zip([
            self.class_id_echo,
            self.unknown_1c,
            self.unknown_20,
            self.unknown_24,
            self.unknown_28,
            self.unknown_30,
        ]) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

/// One polymorphic `LS_SPR_` record.
///
/// The record's **extent** is decoded -- that is the whole difficulty of this section, and it is
/// what makes the table walkable at all. Its **contents past the base block are carried
/// verbatim** in [`body`](Self::body), because the readers name offsets into an object and not
/// meanings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteRecord {
    /// The dword the container's dispatch loop reads at `0x004F7194`.
    pub class_id: u32,
    pub base: SpriteBase,
    /// Every byte the class's own reader consumed after the base block. Layout established;
    /// field meanings **Unknown**, so the bytes are kept rather than parsed into invented names.
    pub body: Vec<u8>,
}

impl SpriteRecord {
    /// The dispatch entry this record's class id selects.
    pub fn class_entry(&self) -> Option<&'static SpriteClassEntry> {
        SPRITE_CLASS_DISPATCH.get(self.class_id as usize)
    }

    /// Whether the base block's echo of the class id matches the dispatch dword.
    ///
    /// **Forced by the writer**, which emits the same `[object+4]` at `0x004F6C25` and
    /// `0x004F6A8B`, so this is a **one-way** detector.
    ///
    /// `false` means something is wrong: the file is damaged, the parse is misaligned, or another
    /// writer produced it. `true` means **only that these two dwords match** -- every other byte
    /// of the record could be corrupt and this would still be `true`. It does not mean a record
    /// layout is right and it cannot detect a wrong one.
    pub fn class_id_echo_agrees(&self) -> bool {
        self.base.class_id_echo == self.class_id
    }

    /// The bytes this record occupies on disk.
    pub fn encoded_len(&self) -> usize {
        4 + SpriteBase::LEN + self.body.len()
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.class_id.to_le_bytes());
        out.extend_from_slice(&self.base.to_bytes());
        out.extend_from_slice(&self.body);
    }
}

// The version gates the `LS_SPR_` readers test, as they appear in the instruction stream. Named
// rather than inlined because a mutation sweep has to be able to move each one independently, and
// because an unlabelled `0x62` in a reader is indistinguishable from a typo.
//
// Every one of these is a signed `jl` / `jge` against `[0x005AA12C]`, exactly like the rest of the
// format, so the comparison here is on `i32`.
const SPR_CLASS0_SKIP_FOUR_WORDS_BELOW: i32 = 0x3F;
const SPR_CLASS0_TAIL_MIN: i32 = 0x33;
const SPR_CLASS0_TAIL_SECOND_WORD_MIN: i32 = 0x38;
const SPR_CLASS0_TAIL_SIX_WORDS_MIN: i32 = 0x3D;
const SPR_CLASS0_TAIL_PAIR_MIN: i32 = 0x42;
const SPR_CLASS0_TAIL_PAIR_WIDE_MIN: i32 = 0x5D;
const SPR_CLASS0_TAIL_SPARE_WORD_MIN: i32 = 0x59;
const SPR_CLASS0_TAIL_SPARE_WORD_MAX: i32 = 0x5B;
const SPR_CLASS0_TAIL_FINAL_BYTE_MIN: i32 = 0x6A;
const SPR_CLASS1_BYTE_PAIR_MIN: i32 = 0x62;
const SPR_CLASS1_NESTED_MIN: i32 = 0x36;
const SPR_CLASS1_WORD_MIN: i32 = 0x4A;
const SPR_CLASS1_BYTE_MIN: i32 = 0x60;
const SPR_CLASS1_ARRAY_MIN: i32 = 0x66;
const SPR_CLASS2_NESTED_MIN: i32 = 0x3B;
const SPR_CLASS2_TAIL_WORD_MIN: i32 = 0x5A;
const SPR_CLASS3_STORED_LEN_MIN: i32 = 0x34;
const SPR_CLASS3_TAIL_MIN: i32 = 0x53;
const SPR_ITEM_A_PAIR_MIN: i32 = 0x43;
const SPR_ITEM_A_WORD_MIN: i32 = 0x51;
const SPR_ITEM_B_WORD_MIN: i32 = 0x4B;
const SPR_ITEM_B_SECOND_WORD_MIN: i32 = 0x58;
const SPR_NESTED_NAME_BELOW: i32 = 0x65;
const SPR_NESTED_SKIP_EIGHT_BELOW: i32 = 0x3E;
/// `0x0044B6E0`. **Unreachable in this build**, and the only gate in the section that is: the
/// field it guards sits behind a second test against the build constant at `0x0055B1B0`, which is
/// 111 and satisfies `jge 0x48` at compile time. A mutation of this constant is undetectable by
/// any test, which is why it is called out here rather than left looking like the others.
const SPR_NESTED_LAST_WORD_MIN: i32 = 0x41;
const SPR_NESTED0_SKIP_BELOW: i32 = 0x3C;
const SPR_NESTED0_TRIPLE_MIN: i32 = 0x4F;
const SPR_NESTED3_STORED_COUNT_MIN: i32 = 0x55;
const SPR_SLOT_NESTED_MIN: i32 = 0x37;
const SPR_SLOT_TRAILER_MIN: i32 = 0x3E;

/// The class-3 blob length the reader assumes for saves older than
/// `SPR_CLASS3_STORED_LEN_MIN`, `0x004516E3`.
const SPR_CLASS3_LEGACY_BLOB_LEN: usize = 0x2BC;

/// The widths of class 0's per-slot blob, `0x00524E3C`. A five-rung ladder on the save's version.
const SPR_SLOT_BLOB_WIDTHS: [(i32, usize); 5] = [
    (0x48, 0x20),
    (0x49, 0x24),
    (0x65, 0x28),
    (0x67, 0x48),
    (i32::MAX, 0x4C),
];

/// The width of the per-slot blob at `0x00524E82` for a given format version.
pub fn sprite_slot_blob_width(version: u32) -> usize {
    let version = version as i32;
    for (below, width) in SPR_SLOT_BLOB_WIDTHS {
        if version < below {
            return width;
        }
    }
    SPR_SLOT_BLOB_WIDTHS[SPR_SLOT_BLOB_WIDTHS.len() - 1].1
}

/// The nested class family behind the four-entry factory at `0x0044B4F0`, reached from a class-0
/// slot. Ids outside `0..=3` are what the factory's own `cmp ecx,3 / ja` rejects.
pub const SPRITE_NESTED_MAX_TYPE_ID: u32 = 3;

/// A count the engine guards with `test / jle` or `cmp / jle`: it is **signed**, and a
/// non-positive value skips its loop entirely.
///
/// **Observed in a local binary, 2026-09-18.** Six counts in this section are read into a
/// register and then tested with a signed branch before their loop is entered -- the **top-level
/// record count** at `0x004F717F`, class 0's slot count at `0x004122FC`, the counted byte array's
/// length at `0x00427A5E`, class 1's array count at `0x0050DC81`, nested type 3's group count at
/// `0x0044BCC7`, and class 3's tail count at `0x00452EBF`. Treating any of them as unsigned turns a value the engine skips into a read of
/// up to two billion records, which this parser would refuse -- and because `LS_SPR_` can now
/// fail, refusing it fails the **whole file** on a save the game loads.
///
/// Not every count in the section is guarded this way, and the ones that are not are **not**
/// routed through here. The section has **three** shapes in total, and every file-declared number
/// in it goes through exactly one of the three: [`spr_signed_count`] and [`spr_signed_count16`]
/// for the `jle`-guarded counts, [`spr_unguarded_count`] for the `test / je` do-while loops, and
/// [`spr_unguarded_length`] for the two lengths handed straight to `fread`.
fn spr_signed_count(raw: u32) -> u32 {
    (raw as i32).max(0) as u32
}

/// The same rule for class 1's array count, which is a **word**: `0x0050DC81` compares
/// `word [esi+0x4E]` against zero with `jle`, and `0x0050DD35` re-reads it with `movsx` for the
/// continue test.
fn spr_signed_count16(raw: u16) -> u16 {
    (raw as i16).max(0) as u16
}

/// A count the engine guards only with `test / je` and then decrements -- a `do { } while (--n)`
/// loop with **no signed test at all**.
///
/// The three list counts inside a class-0 slot are like this (`0x00524FC6`, `0x0052503B`,
/// `0x0044BD07`) -- each address is the `test` itself, not the load that precedes it. A negative value does not skip: it decrements away from zero and the engine
/// runs away. There is no correct behaviour to mirror, so this parser reads the value unsigned
/// and lets `Cursor::take` refuse — **a refusal where the engine would misbehave**, which is the
/// right direction to differ in but is a difference, and is recorded here rather than hidden
/// behind a cast that looks like the guarded case.
fn spr_unguarded_count(raw: u32) -> u32 {
    raw
}

/// A **length** the engine passes straight to `fread` with no test on it at all.
///
/// **Observed in a local binary, 2026-09-18.** Class 2's blob length reaches the `fread` at
/// `0x0043D0E3` and class 3's the one at `0x004516F7`, in both cases as the `size` argument with
/// no compare, no branch and no clamp anywhere between the read of the word and the call. They
/// are neither of the two count shapes above, and routing them through either would misstate the
/// engine.
///
/// The behaviour here is unchanged -- `Cursor::take` bounds them, as it bounds everything -- so
/// this marker exists purely so the taxonomy is complete where it is introduced. Two call sites
/// silently outside a three-way classification is how a convention rots.
fn spr_unguarded_length(raw: u32) -> usize {
    raw as usize
}

// 0x004F6B00 -- the base reader every class calls first.
fn spr_read_base(cursor: &mut Cursor<'_>) -> Result<SpriteBase, SaveError> {
    Ok(SpriteBase {
        class_id_echo: cursor.u32()?,
        unknown_1c: cursor.u32()?,
        unknown_20: cursor.u32()?,
        unknown_24: cursor.u32()?,
        unknown_28: cursor.u32()?,
        unknown_30: cursor.u32()?,
    })
}

// 0x00427A40 -- `u32 len` then `len` raw bytes, with no terminator. The engine parks the payload
// in a pool and never treats it as text; in the corpus the bytes are small integers, so this is
// deliberately not called a string reader.
fn spr_skip_counted_bytes(cursor: &mut Cursor<'_>) -> Result<(), SaveError> {
    // `0x00427A5E` is `test eax,eax / jle`, and the skip covers the allocation *and* the read.
    let len = spr_signed_count(cursor.u32()?) as usize;
    cursor.take(len)?;
    Ok(())
}

// 0x0044B660 -- the base of the four nested classes reached from a class-0 slot.
fn spr_nested_base(cursor: &mut Cursor<'_>, version: i32) -> Result<(), SaveError> {
    cursor.take(4)?;
    // Two gates in this function test the **build** constant at `0x0055B1B0`, not the save's
    // version, so they are decided at compile time and are dead in this build. Written out rather
    // than dropped, because dropping them would silently hard-code one build's behaviour.
    if (BUILD_FORMAT_VERSION as i32) < 0x47 {
        cursor.take(4)?;
    }
    cursor.take(4)?;
    if version < SPR_NESTED_NAME_BELOW {
        cursor.take(0x1F)?;
    }
    if version < SPR_NESTED_SKIP_EIGHT_BELOW {
        cursor.take(8)?;
    }
    if version >= SPR_NESTED_LAST_WORD_MIN && (BUILD_FORMAT_VERSION as i32) < 0x48 {
        cursor.take(4)?;
    }
    cursor.take(4)?;
    Ok(())
}

// 0x00526C20 -- the list item class 0's slots and nested type 3 both carry.
fn spr_item_a(cursor: &mut Cursor<'_>, version: i32) -> Result<(), SaveError> {
    cursor.take(16)?;
    if version >= SPR_ITEM_A_PAIR_MIN {
        cursor.take(8)?;
    }
    if version >= SPR_ITEM_A_WORD_MIN {
        cursor.take(4)?;
    }
    Ok(())
}

// 0x00427FA0 -- the second list item a class-0 slot carries.
fn spr_item_b(cursor: &mut Cursor<'_>, version: i32) -> Result<(), SaveError> {
    cursor.take(12)?;
    if version >= SPR_ITEM_B_WORD_MIN {
        cursor.take(4)?;
    }
    if version >= SPR_ITEM_B_SECOND_WORD_MIN {
        cursor.take(4)?;
    }
    Ok(())
}

// 0x0044B810 / 0x0044B920 / 0x0044BBC0 -- the readers behind the factory at `0x0044B4F0`.
fn spr_nested(cursor: &mut Cursor<'_>, version: i32, type_id: u32) -> Result<(), SaveError> {
    match type_id {
        // 0x0044B810
        0 => {
            spr_nested_base(cursor, version)?;
            if version < SPR_NESTED0_SKIP_BELOW {
                cursor.take(16)?;
            }
            cursor.take(4)?;
            if version >= SPR_NESTED0_TRIPLE_MIN {
                cursor.take(12)?;
            }
            cursor.take(4)?;
        }
        // 0x0044B920, shared by ids 1 and 2 -- the two classes have distinct vtables at
        // `0x0054D5E0` and `0x0054D5F0` whose reader slots hold the same function.
        1 | 2 => {
            spr_nested_base(cursor, version)?;
            if version < SPR_NESTED0_SKIP_BELOW {
                cursor.take(4)?;
            }
        }
        // 0x0044BBC0
        3 => {
            spr_nested_base(cursor, version)?;
            let groups = if version >= SPR_NESTED3_STORED_COUNT_MIN {
                spr_signed_count(cursor.u32()?)
            } else {
                6
            };
            for _ in 0..groups {
                cursor.take(4)?;
                let items = spr_unguarded_count(cursor.u32()?);
                for _ in 0..items {
                    spr_item_a(cursor, version)?;
                }
            }
        }
        other => {
            return Err(SaveError::section(
                SectionTag::Sprites,
                format!(
                    "nested type id {other} at +{} is outside the factory's 0..={} range",
                    cursor.offset() - 4,
                    SPRITE_NESTED_MAX_TYPE_ID
                ),
            ));
        }
    }
    Ok(())
}

// 0x00524D70 -- one of class 0's `n` slots.
fn spr_slot(cursor: &mut Cursor<'_>, version: i32, blob_width: usize) -> Result<(), SaveError> {
    let blob = cursor.take(blob_width)?;
    // `0x00524EE0` tests a field **inside the blob just read**, at offset 0x14, and a save older
    // than `SPR_SLOT_NESTED_MIN` has its own value forced to zero at `0x00524ECF` regardless.
    let has_nested = version >= SPR_SLOT_NESTED_MIN
        && blob_width >= 0x18
        && u32::from_le_bytes(blob[0x14..0x18].try_into().expect("four bytes")) != 0;
    if has_nested {
        let type_id = cursor.u32()?;
        spr_nested(cursor, version, type_id)?;
    }
    let items_a = spr_unguarded_count(cursor.u32()?);
    for _ in 0..items_a {
        spr_item_a(cursor, version)?;
    }
    if version >= SPR_SLOT_TRAILER_MIN {
        let items_b = spr_unguarded_count(cursor.u32()?);
        for _ in 0..items_b {
            spr_item_b(cursor, version)?;
        }
    }
    Ok(())
}

// 0x004122A0 -- class 0's on-disk reader, and the one three other classes embed.
fn spr_class0_body(cursor: &mut Cursor<'_>, version: i32) -> Result<(), SaveError> {
    cursor.take(8)?;
    // `0x004122FC` is `test eax,eax / jle`: a non-positive slot count skips the loop.
    let slots = spr_signed_count(cursor.u32()?);
    cursor.take(4)?;
    let blob_width = sprite_slot_blob_width(version as u32);
    for _ in 0..slots {
        spr_slot(cursor, version, blob_width)?;
    }
    cursor.take(16)?;
    spr_skip_counted_bytes(cursor)?;
    if version < SPR_CLASS0_SKIP_FOUR_WORDS_BELOW {
        cursor.take(16)?;
    }
    cursor.take(12)?;
    cursor.take(88)?;
    if version >= SPR_CLASS0_TAIL_MIN {
        cursor.take(4)?;
        if version >= SPR_CLASS0_TAIL_SECOND_WORD_MIN {
            cursor.take(4)?;
        }
        if version >= SPR_CLASS0_TAIL_SIX_WORDS_MIN {
            cursor.take(24)?;
        }
        if version >= SPR_CLASS0_TAIL_PAIR_MIN {
            cursor.take(if version >= SPR_CLASS0_TAIL_PAIR_WIDE_MIN {
                4
            } else {
                1
            })?;
            cursor.take(1)?;
        }
        if (SPR_CLASS0_TAIL_SPARE_WORD_MIN..=SPR_CLASS0_TAIL_SPARE_WORD_MAX).contains(&version) {
            cursor.take(4)?;
        }
        if version >= SPR_CLASS0_TAIL_FINAL_BYTE_MIN {
            cursor.take(1)?;
        }
    }
    Ok(())
}

// The full class-0 record, base block included, as three other classes embed it.
fn spr_embedded_class0(cursor: &mut Cursor<'_>, version: i32) -> Result<(), SaveError> {
    spr_read_base(cursor)?;
    spr_class0_body(cursor, version)
}

/// Advance `cursor` over one record's class-specific body, the base block already consumed.
fn spr_class_body(cursor: &mut Cursor<'_>, version: i32, class_id: u32) -> Result<(), SaveError> {
    match class_id {
        // 0x004122A0
        0 => spr_class0_body(cursor, version)?,
        // 0x0050DA70
        1 => {
            cursor.take(4)?;
            if version >= SPR_CLASS1_BYTE_PAIR_MIN {
                cursor.take(2)?;
            } else {
                cursor.take(8)?;
            }
            if version >= SPR_CLASS1_NESTED_MIN {
                cursor.take(4)?;
                if cursor.u32()? != 0 {
                    spr_embedded_class0(cursor, version)?;
                }
            }
            if version >= SPR_CLASS1_WORD_MIN {
                cursor.take(4)?;
            }
            if version >= SPR_CLASS1_BYTE_MIN {
                cursor.take(1)?;
            }
            if version >= SPR_CLASS1_ARRAY_MIN {
                // A `u16`, not a `u32`: `fread(this+0x4E, 2, 1)` at `0x0050DBD1`, and the loop
                // bound is re-read as a signed word at `0x0050DD35`.
                let entries = spr_signed_count16(cursor.u16()?);
                for _ in 0..entries {
                    cursor.take(8)?;
                    if cursor.u32()? != 0 {
                        spr_embedded_class0(cursor, version)?;
                    }
                }
            }
        }
        // 0x0043D0A0
        2 => {
            // Straight into `fread`'s size argument at `0x0043D0E3`, untested.
            let blob = spr_unguarded_length(cursor.u32()?);
            cursor.take(blob)?;
            cursor.take(4)?;
            if version >= SPR_CLASS2_NESTED_MIN && cursor.u32()? != 0 {
                spr_embedded_class0(cursor, version)?;
            }
            if version >= SPR_CLASS2_TAIL_WORD_MIN {
                cursor.take(4)?;
            }
        }
        // 0x004516A0
        3 => {
            // Straight into `fread`'s size argument at `0x004516F7`, untested.
            let blob = if version >= SPR_CLASS3_STORED_LEN_MIN {
                spr_unguarded_length(cursor.u32()?)
            } else {
                SPR_CLASS3_LEGACY_BLOB_LEN
            };
            cursor.take(blob)?;
            if cursor.u32()? != 0 {
                spr_embedded_class0(cursor, version)?;
            }
            if version >= SPR_CLASS3_TAIL_MIN {
                // 0x00452E20, whose per-item reader at 0x00452CE0 is 36 bytes with no gates.
                // `0x00452EBF` is `cmp eax,0 / jle`.
                let items = spr_signed_count(cursor.u32()?);
                for _ in 0..items {
                    cursor.take(36)?;
                }
            }
        }
        // 0x004F0D80 -- eleven dwords, no version gate.
        4 => {
            cursor.take(44)?;
        }
        // Ids 5 and 6 never reach a class body: the dispatch raises first.
        5 | 6 => {
            return Err(SaveError::section(
                SectionTag::Sprites,
                format!(
                    "class id {class_id} at +{} is one of the two ids the dispatch table routes \
                     to the raise at {:#x}",
                    cursor.offset() - 4 - SpriteBase::LEN,
                    0x004F_73A3_u32
                ),
            ));
        }
        // 0x0047CDD0 -- eleven dwords, no version gate.
        7 => {
            cursor.take(44)?;
        }
        // 0x004F6B00 -- class 8 inherits the base reader unchanged and adds nothing.
        8 => {}
        // 0x004ADA10 -- a 92-byte blob and ten dwords, no version gate.
        9 => {
            cursor.take(0x5C)?;
            cursor.take(40)?;
        }
        other => {
            return Err(SaveError::section(
                SectionTag::Sprites,
                format!(
                    "class id {other} at +{} is past the reader's `cmp eax,{}` bound",
                    cursor.offset() - 4 - SpriteBase::LEN,
                    SPRITE_MAX_CLASS_ID
                ),
            ));
        }
    }
    Ok(())
}

/// The unit, army and hero table. **Decoded, 2026-09-18.**
///
/// **Observed in a local binary, 2026-09-18.** `u32 count`, then `count` polymorphic records. Each
/// record opens with a `u32 class_id` that the reader at `0x004F7122` bounds with `cmp eax,9 / ja`
/// and dispatches through the ten-entry table at [`SPRITE_CLASS_DISPATCH`]; the selected class's
/// reader at `vtable+0x24` then consumes exactly what its own structure needs. Three of the eight
/// real classes are variable-length on disk, which is why no fixed stride exists -- see
/// [`candidate_fixed_strides`](Self::candidate_fixed_strides), which still reports the file-side
/// version of that argument.
///
/// Every record begins with the 24-byte [`SpriteBase`] block, and **three of the eight classes
/// embed a whole class-0 record inside themselves**, so the reader is genuinely recursive rather
/// than a table of widths.
///
/// The model is checked against the files the same way the three sections decoded before it were:
/// parse `count` records and assert the cursor lands **exactly** on the section end. Nothing in
/// that check is tunable -- every length here is either a constant in the instruction stream or a
/// count the file itself stores, so a merely plausible model stops short or overruns. It lands
/// exactly, with zero slack, in every corpus file, across both format versions present.
/// `save_survey` prints the file count it measured -- a count typed into a comment goes stale,
/// and the one in this module already did.
///
/// **What is not determined**: the *meaning* of any field past the class id. The readers name
/// offsets into an object, not semantics, so each record's bytes past the base block are carried
/// verbatim in [`SpriteRecord::body`] rather than parsed into invented names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSection {
    /// The count word exactly as stored. **Signed** at the engine's branch, so this is not
    /// necessarily the number of records: see [`live_record_count`](Self::live_record_count).
    pub record_count: u32,
    pub records: Vec<SpriteRecord>,
    /// Every byte after the count word, kept as written. The records above are a view of exactly
    /// these bytes. Keeping both is for a future writer and for diagnosing a refused file -- it
    /// is **not** what makes the round trip meaningful, because `records` is a view of exactly
    /// these bytes and comparing the two is an identity. See [`encode`](Self::encode).
    pub raw: Vec<u8>,
}

impl SpriteSection {
    pub fn parse(payload: &[u8], version: &VersionSection) -> Result<Self, SaveError> {
        let tag = SectionTag::Sprites;
        let record_count = read_u32(tag, payload, 0)?;
        // `0x004F717F` is `cmp eax,0 / jle`, and `0x004F7336` continues with `jl`: the record
        // count is **signed**, and a non-positive one consumes no records at all. A save whose
        // whole `LS_SPR_` payload is `FF FF FF FF` is one the engine loads.
        let live_record_count = spr_signed_count(record_count);
        let version_value = version.version as i32;

        let mut cursor = Cursor::new(tag, payload);
        cursor.u32()?;
        let mut records = Vec::new();
        for index in 0..live_record_count {
            let record_start = cursor.offset();
            let class_id = cursor.u32()?;
            if class_id > SPRITE_MAX_CLASS_ID {
                return Err(SaveError::section(
                    tag,
                    format!(
                        "record {index} at +{record_start} has class id {class_id}, past the \
                         reader's `cmp eax,{SPRITE_MAX_CLASS_ID}` bound"
                    ),
                ));
            }
            let base = spr_read_base(&mut cursor)?;
            let body_start = cursor.offset();
            spr_class_body(&mut cursor, version_value, class_id)?;
            let body = payload
                .get(body_start..cursor.offset())
                .expect("the cursor only advances over bytes it has taken")
                .to_vec();
            records.push(SpriteRecord {
                class_id,
                base,
                body,
            });
        }
        if cursor.remaining() != 0 {
            return Err(SaveError::section(
                tag,
                format!(
                    "{live_record_count} records account for {} of {} payload bytes, leaving {} over",
                    cursor.offset(),
                    payload.len(),
                    cursor.remaining()
                ),
            ));
        }

        Ok(Self {
            record_count,
            records,
            raw: payload[4..].to_vec(),
        })
    }

    /// The number of records the engine's loop actually runs, which is
    /// [`record_count`](Self::record_count) clamped at zero because `0x004F717F` tests it with
    /// `jle`. Equal to `records.len()` after a successful parse.
    pub fn live_record_count(&self) -> u32 {
        spr_signed_count(self.record_count)
    }

    /// The undecoded record bytes, exactly as they appear on disk.
    pub fn records_raw(&self) -> &[u8] {
        &self.raw
    }

    /// Re-emit the section payload from the decoded records.
    ///
    /// **What a matching re-encode proves, and what it does not.** `class_id` and the six base
    /// dwords are read little-endian and written back little-endian, and `body` is an exact
    /// contiguous slice, so once [`parse`](Self::parse) has succeeded this is an **identity**: it
    /// necessarily equals the payload. It proves the records **tile the payload contiguously and
    /// in order, with no gap, no overlap and no reordering** -- lossless preservation, which a
    /// future writer will need -- and that is a restatement of the zero-slack check at record
    /// granularity, **not** independent evidence that any record's internal field boundaries are
    /// right.
    ///
    /// A worked counter-example, because the distinction is easy to lose: change class 0's two
    /// consecutive fixed reads from `12 + 88` to the compensating `16 + 84`. Every boundary
    /// inside that record is then wrong, the aggregate size is unchanged, `parse` still lands
    /// exactly on the section end, `body` captures the same bytes, and this function still
    /// returns the payload byte for byte. A compensating error costs nothing here.
    ///
    /// The evidence for the boundaries is the disassembly, corroborated by the version sweep --
    /// the fixture emitters are an independently written second transcription with literal gate
    /// values, so a boundary the parser and the fixture disagree about shows up as a refusal.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.raw.len());
        out.extend_from_slice(&self.record_count.to_le_bytes());
        for record in &self.records {
            record.encode_into(&mut out);
        }
        out
    }

    /// How many records disagree with their own class-id echo.
    ///
    /// Zero on any file this engine wrote, because [the writer emits the field twice](SpriteBase),
    /// so **zero is the uninformative answer** -- it is the count of records that failed a
    /// one-way detector, not a count of records known to be sound. Non-zero means damage or a
    /// misaligned parse, never a layout error.
    pub fn class_id_echo_disagreements(&self) -> usize {
        self.records
            .iter()
            .filter(|record| !record.class_id_echo_agrees())
            .count()
    }

    /// How many records of each class id the section holds.
    pub fn class_histogram(&self) -> BTreeMap<u32, usize> {
        let mut counts = BTreeMap::new();
        for record in &self.records {
            *counts.entry(record.class_id).or_insert(0) += 1;
        }
        counts
    }

    /// Every header size in `0..=max_header` for which the remaining bytes divide evenly by the
    /// record count.
    ///
    /// Kept after the section was decoded, because the survey still reports the file-side argument
    /// that no common fixed stride exists, and that argument is now **corroborated** by a decode
    /// rather than standing alone.
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
/// **Observed in a local binary, 2026-09-18.** The payload is 6,272 bytes in every corpus file, which
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
        // **Exactly eight, not merely a whole number of records.** The writer `fwrite`s 784 bytes
        // eight times unconditionally, so zero records is not something the engine can produce --
        // and an empty payload is a multiple of 784, so a divisibility test accepts it and hands
        // every caller an empty `records`. That is not hypothetical: it panicked the survey on the
        // first byte-level probe of this parser, at `records[0]`. This repository has already
        // shipped one decode path that killed a process; the guard belongs here, at the parse.
        let expected = UserSection::RECORD_COUNT * UserRecord::LEN;
        if payload.len() != expected {
            return Err(SaveError::section(
                tag,
                format!(
                    "payload is {} bytes; the user section is exactly {} records of {} = {expected}",
                    payload.len(),
                    UserSection::RECORD_COUNT,
                    UserRecord::LEN,
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
/// **Observed in a local binary, 2026-09-18.** The shape holds in every corpus file with no
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
/// `(payload_len - 24) % 12 == 0` in every corpus file, and **`N - live_count == 71` in every**.
/// The constant 71 is Observed and **unexplained** -- do not name the fields it relates.
///
/// The per-file `N`/`live_count` pairs are **not** listed here. They used to be, as eight of them,
/// and the list was left behind when the claim above was re-measured over the whole corpus --
/// a universal quantified over 31 files, evidenced by a list of 8, disagreeing with
/// `docs/save-format.md`'s nine rows. `save_survey` prints the pairs it measured, which is the
/// only version that cannot go stale.
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
// A forward-only payload cursor
// ---------------------------------------------------------------------------

/// A forward-only reader over one section's payload.
///
/// The three sections decoded from the writer are **sequential, not addressed**: a count decides
/// how many records follow and a record's own fields decide its length, so no field in them has a
/// fixed offset. The engine does not use offsets either -- it `fread`s straight down the struct --
/// and a reader written against offsets cannot express what these sections are.
struct Cursor<'a> {
    tag: SectionTag,
    bytes: &'a [u8],
    offset: usize,
}

/// A bitset as the engine writes it: `u32 bit_count`, then `ceil(bit_count / 32)` words.
///
/// **Observed in a local binary, 2026-09-18**, at `0x004BF0A0` (writer) and `0x004BF100` (reader).
/// The stored count is a count of **bits**, and the word count is derived from it -- `add eax,0x1f`
/// / `sar eax,5` at `0x004BF0C7`. Bit meanings are **Unknown**, so the words are carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitset {
    pub bit_count: u32,
    /// `ceil(bit_count / 32) * 4` bytes. Meaning **Unknown**.
    pub words_raw: Vec<u8>,
}

impl Bitset {
    /// The bytes this bitset occupies on disk: the count word plus its words.
    pub fn encoded_len(&self) -> usize {
        4 + self.words_raw.len()
    }
}

impl<'a> Cursor<'a> {
    fn new(tag: SectionTag, bytes: &'a [u8]) -> Self {
        Self {
            tag,
            bytes,
            offset: 0,
        }
    }

    fn offset(&self) -> usize {
        self.offset
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], SaveError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| SaveError::section(self.tag, "read length overflow"))?;
        let slice = self.bytes.get(self.offset..end).ok_or_else(|| {
            SaveError::section(
                self.tag,
                format!(
                    "a {len}-byte field at +{} runs past the {}-byte payload",
                    self.offset,
                    self.bytes.len()
                ),
            )
        })?;
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, SaveError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, SaveError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("two bytes");
        Ok(u16::from_le_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, SaveError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("four bytes");
        Ok(u32::from_le_bytes(bytes))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SaveError> {
        Ok(self.take(N)?.try_into().expect("length checked by take"))
    }

    fn words(&mut self, count: usize) -> Result<Vec<u32>, SaveError> {
        let len = count
            .checked_mul(4)
            .ok_or_else(|| SaveError::section(self.tag, "word count overflow"))?;
        Ok(self
            .take(len)?
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("four bytes")))
            .collect())
    }

    /// A count read as a `usize`, refusing a value this platform cannot address.
    fn count(&mut self) -> Result<usize, SaveError> {
        let raw = self.u32()?;
        usize::try_from(raw)
            .map_err(|_| SaveError::section(self.tag, format!("count {raw} does not fit a usize")))
    }

    /// `u32 bit_count`, then `ceil(bit_count / 32)` words -- the engine's bitset at `0x004BF0A0`.
    fn bitset(&mut self) -> Result<Bitset, SaveError> {
        let bit_count = self.u32()?;
        let words = usize::try_from(bit_count)
            .map_err(|_| SaveError::section(self.tag, "bitset bit count does not fit a usize"))?
            .div_ceil(32);
        let len = words
            .checked_mul(4)
            .ok_or_else(|| SaveError::section(self.tag, "bitset byte count overflow"))?;
        Ok(Bitset {
            bit_count,
            words_raw: self.take(len)?.to_vec(),
        })
    }

    /// `u32 len` then `len` raw bytes with **no terminator** -- the engine's string writer at
    /// `0x004D5F20`, which `strlen`s the name and writes the length without the NUL.
    fn counted_string(&mut self) -> Result<Vec<u8>, SaveError> {
        let len = self.count()?;
        Ok(self.take(len)?.to_vec())
    }

    /// Fail unless the cursor landed exactly on the end of the payload.
    ///
    /// This is the whole proof that a model recovered from the writer is right. A model that is
    /// merely *plausible* stops somewhere inside the payload or runs off the end; only the true
    /// one consumes it to the byte, and there is nothing to tune -- every length in these sections
    /// is either a constant in the instruction stream or a count the file itself stores.
    fn expect_exhausted(&self, what: &str) -> Result<(), SaveError> {
        if self.remaining() != 0 {
            return Err(SaveError::section(
                self.tag,
                format!(
                    "{what} consumed {} of {} bytes, leaving {} unaccounted for",
                    self.offset,
                    self.bytes.len(),
                    self.remaining()
                ),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LS_PLR_
// ---------------------------------------------------------------------------

/// One entry of a player's unit queue. **Observed in a local binary, 2026-09-18**, `0x0050B800`.
///
/// Six `u32` written in the engine's field order `+0, +4, +8, +0x1c, +0xc, +0x10` -- the on-disk
/// order is **not** the struct order, which is why this is six anonymous words and not six named
/// ones. Meanings **Unknown**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerQueueEntry {
    pub words: [u32; 6],
}

/// One unit inside an army. **Observed in a local binary, 2026-09-18**, `0x00509FC0`: five `u32`
/// at `+0, +4, +8, +0xc, +0x10`. Meanings **Unknown**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerUnit {
    pub words: [u32; 5],
}

/// One of a player's sixteen armies. **Observed in a local binary, 2026-09-18**, `0x0050A360`
/// (writer) and `0x0050A230` (reader), called sixteen times at `0x004BCEB3` with a RAM stride of
/// `0x88`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerArmy {
    /// `+0, +4, +8`. Meanings **Unknown**.
    pub leading_words: [u32; 3],
    /// A linked list at `+0x84`, walked through `+0x14`, counted into the file before its members.
    pub units: Vec<PlayerUnit>,
    /// `0x0049F2A0`: `0x28` bytes from `+0`, `0x34` from `+0x28`, then `+0x5c` and `+0x60`.
    /// A hundred bytes of **Unknown** structure, carried verbatim.
    pub stats_raw: [u8; PlayerArmy::STATS_LEN],
    /// `0x004BF0A0` on `+0x7c`.
    pub flags: Bitset,
    /// `+0xc` and `+0x14`. Meanings **Unknown**.
    pub trailing_words: [u32; 2],
}

impl PlayerArmy {
    /// The fixed block `0x0049F2A0` writes: `0x28 + 0x34 + 4 + 4`.
    pub const STATS_LEN: usize = 100;

    /// The bytes this army occupies on disk.
    pub fn encoded_len(&self) -> usize {
        3 * 4 + 4 + self.units.len() * 5 * 4 + Self::STATS_LEN + self.flags.encoded_len() + 2 * 4
    }

    fn parse(cursor: &mut Cursor<'_>) -> Result<Self, SaveError> {
        let leading_words = [cursor.u32()?, cursor.u32()?, cursor.u32()?];
        let unit_count = cursor.count()?;
        let mut units = Vec::with_capacity(unit_count.min(1024));
        for _ in 0..unit_count {
            units.push(PlayerUnit {
                words: [
                    cursor.u32()?,
                    cursor.u32()?,
                    cursor.u32()?,
                    cursor.u32()?,
                    cursor.u32()?,
                ],
            });
        }
        let stats_raw = cursor.array::<{ PlayerArmy::STATS_LEN }>()?;
        let flags = cursor.bitset()?;
        let trailing_words = [cursor.u32()?, cursor.u32()?];
        Ok(Self {
            leading_words,
            units,
            stats_raw,
            flags,
            trailing_words,
        })
    }
}

/// A player's roster block. **Observed in a local binary, 2026-09-18**, `0x0051C6C0` (writer) and
/// `0x0051C790` (reader).
///
/// The `entry_count` is genuinely stored and genuinely iterated, and the per-entry writer at
/// `0x004BD210` is `mov eax,1 / ret 4` -- **it writes nothing**. So the count is on disk and its
/// members are not, which is why this carries a count with no vector beside it. Inventing a
/// `Vec<Entry>` of zero-byte entries here would be minting a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRoster {
    /// `+0x68`, `+0x6c`. Meanings **Unknown**.
    pub leading_words: [u32; 2],
    /// `+0x74`: how many entries the engine iterated. Each entry writes zero bytes.
    pub entry_count: u32,
    /// The writer pushes this as a **literal 22** (`0x0051C735`); the reader takes it from the
    /// file. It is stored, so it is parsed rather than assumed.
    pub slot_count: u32,
    /// `slot_count` words from `+0x10`. Meanings **Unknown**.
    pub slots: Vec<u32>,
    /// 64 bytes from `+0x78`. Structure **Unknown**.
    pub block_raw: [u8; 64],
}

impl PlayerRoster {
    /// The slot count the writer emits as a literal. Parsed from the file, never assumed.
    pub const WRITTEN_SLOT_COUNT: u32 = 22;

    /// The bytes this roster occupies on disk.
    pub fn encoded_len(&self) -> usize {
        4 * 4 + self.slots.len() * 4 + self.block_raw.len()
    }

    fn parse(cursor: &mut Cursor<'_>) -> Result<Self, SaveError> {
        let leading_words = [cursor.u32()?, cursor.u32()?];
        let entry_count = cursor.u32()?;
        let slot_count = cursor.u32()?;
        let slots = cursor.words(usize::try_from(slot_count).map_err(|_| {
            SaveError::section(SectionTag::Player, "roster slot count does not fit a usize")
        })?)?;
        Ok(Self {
            leading_words,
            entry_count,
            slot_count,
            slots,
            block_raw: cursor.array::<64>()?,
        })
    }
}

/// The version ladder inside the `LS_PLR_` record reader.
///
/// **Observed in a local binary, 2026-09-18.** Eight `cmp dword [0x5AA12C], n / jl` gates in
/// `0x004BCBD0`. Everything before the first gate is unconditional. The constants are the
/// engine's, read off the instruction stream:
///
/// | gate VA | minimum version | field |
/// | --- | ---: | --- |
/// | `0x004BCCDE` | 57 | the fifteen interleaved words |
/// | `0x004BCD28` | 68 | `+0x3c` |
/// | -- | always | the `+0x34` bitset |
/// | `0x004BCD4B` | 76 | the 31-byte name |
/// | `0x004BCD65` | 86 | `+0x15d8` |
/// | `0x004BCD82` | 104 | the 3,200-byte block |
/// | `0x004BCD9F` | 110 | `+0x15b4` and `+0x15b8` |
/// | `0x004BCDD0` | 111 | `+0x40` |
///
/// The writer at `0x004BCE20` has **no gates at all** -- it always writes the full record. So a
/// save is readable by the build that wrote it and by every later build, and the ladder exists
/// only to read older files. That asymmetry is why the record must be parsed against `LS_VER_`
/// and not against its own length.
pub mod player_record_versions {
    pub const INTERLEAVED_WORDS: u32 = 57;
    pub const UNKNOWN_3C: u32 = 68;
    pub const NAME: u32 = 76;
    pub const UNKNOWN_15D8: u32 = 86;
    pub const BLOCK_68: u32 = 104;
    pub const UNKNOWN_15B4_15B8: u32 = 110;
    pub const UNKNOWN_40: u32 = 111;
}

/// One player's saved state. **Observed in a local binary, 2026-09-18**, `0x004BCE20` (writer) and
/// `0x004BCBD0` (reader).
///
/// **There is no record size.** The record is a tree of counted lists -- a queue, sixteen armies
/// each with its own unit list and its own bit-counted flag set -- so two players in one file
/// differ in length. Measured across the corpus the same record runs 6,303 to 7,983 bytes.
///
/// Fields whose type is `Option` are the version-gated ones; see [`player_record_versions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRecord {
    /// The `u32` that precedes the record body. The reader validates `0 <= slot_index < 16`;
    /// 15 is the neutral/unowned pseudo-player.
    pub slot_index: u32,
    /// `+0x15a8, +0x15ac, +0x15b0`. Meanings **Unknown**.
    pub leading_words: [u32; 3],
    /// A linked list at `+0xd24`, walked through `+0x20`, counted into the file before its members.
    pub queue: Vec<PlayerQueueEntry>,
    /// Exactly sixteen, always.
    pub armies: Vec<PlayerArmy>,
    /// `0x00462FF0` on `+0x15e0`, which writes the single word at `+0x15e4`. Meaning **Unknown**.
    pub unknown_15e4: u32,
    pub roster: PlayerRoster,
    /// Five triples read `[edi-0x14], [edi], [edi+0x14]` with `edi` starting at `+0xcfc` and
    /// stepping by 4 -- three parallel five-word arrays, interleaved on disk. Meanings **Unknown**.
    pub interleaved_words: Option<[[u32; 3]; 5]>,
    /// `+0x3c`. Meaning **Unknown**.
    pub unknown_3c: Option<u32>,
    /// `0x004BF0A0` on `+0x34`. Ungated: present at every version.
    pub flags: Bitset,
    /// 31 bytes at `+0x44`, NUL-terminated within the field. Use [`PlayerRecord::name`].
    pub name_raw: Option<[u8; PlayerRecord::NAME_LEN]>,
    /// `+0x15d8`. Meaning **Unknown**.
    pub unknown_15d8: Option<u32>,
    /// 3,200 bytes at `+0x68`. Structure **Unknown**, carried verbatim.
    pub block_68_raw: Option<Vec<u8>>,
    /// `+0x15b4`, `+0x15b8`. Meanings **Unknown**.
    pub unknown_15b4_15b8: Option<[u32; 2]>,
    /// `+0x40`. Meaning **Unknown**.
    pub unknown_40: Option<u32>,
}

impl PlayerRecord {
    /// The engine writes `0x1f` bytes, so a name of 31 characters has no terminator on disk.
    pub const NAME_LEN: usize = 31;
    /// Sixteen army sub-objects per player, `0x004BCEB3`.
    pub const ARMY_COUNT: usize = 16;
    /// The fixed block at `+0x68`, `push 0xc80`.
    pub const BLOCK_68_LEN: usize = 3200;

    /// The name up to its first NUL, or `None` when this version does not store one.
    pub fn name(&self) -> Option<&[u8]> {
        self.name_raw.as_ref().map(|field| {
            let end = field
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(field.len());
            &field[..end]
        })
    }

    pub fn name_lossy(&self) -> Option<String> {
        self.name()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    /// The bytes this record's body occupies on disk, excluding its leading slot index.
    pub fn encoded_len(&self) -> usize {
        let mut len = 3 * 4 + 4 + self.queue.len() * 6 * 4;
        len += self
            .armies
            .iter()
            .map(PlayerArmy::encoded_len)
            .sum::<usize>();
        len += 4 + self.roster.encoded_len();
        if self.interleaved_words.is_some() {
            len += 15 * 4;
        }
        if self.unknown_3c.is_some() {
            len += 4;
        }
        len += self.flags.encoded_len();
        if self.name_raw.is_some() {
            len += Self::NAME_LEN;
        }
        if self.unknown_15d8.is_some() {
            len += 4;
        }
        if let Some(block) = &self.block_68_raw {
            len += block.len();
        }
        if self.unknown_15b4_15b8.is_some() {
            len += 8;
        }
        if self.unknown_40.is_some() {
            len += 4;
        }
        len
    }

    fn parse(cursor: &mut Cursor<'_>, slot_index: u32, version: u32) -> Result<Self, SaveError> {
        let leading_words = [cursor.u32()?, cursor.u32()?, cursor.u32()?];
        let queue_len = cursor.count()?;
        let mut queue = Vec::with_capacity(queue_len.min(1024));
        for _ in 0..queue_len {
            queue.push(PlayerQueueEntry {
                words: [
                    cursor.u32()?,
                    cursor.u32()?,
                    cursor.u32()?,
                    cursor.u32()?,
                    cursor.u32()?,
                    cursor.u32()?,
                ],
            });
        }
        let mut armies = Vec::with_capacity(Self::ARMY_COUNT);
        for _ in 0..Self::ARMY_COUNT {
            armies.push(PlayerArmy::parse(cursor)?);
        }
        let unknown_15e4 = cursor.u32()?;
        let roster = PlayerRoster::parse(cursor)?;

        let interleaved_words = if version >= player_record_versions::INTERLEAVED_WORDS {
            let mut triples = [[0_u32; 3]; 5];
            for triple in &mut triples {
                *triple = [cursor.u32()?, cursor.u32()?, cursor.u32()?];
            }
            Some(triples)
        } else {
            None
        };
        let unknown_3c = if version >= player_record_versions::UNKNOWN_3C {
            Some(cursor.u32()?)
        } else {
            None
        };
        let flags = cursor.bitset()?;
        let name_raw = if version >= player_record_versions::NAME {
            Some(cursor.array::<{ PlayerRecord::NAME_LEN }>()?)
        } else {
            None
        };
        let unknown_15d8 = if version >= player_record_versions::UNKNOWN_15D8 {
            Some(cursor.u32()?)
        } else {
            None
        };
        let block_68_raw = if version >= player_record_versions::BLOCK_68 {
            Some(cursor.take(Self::BLOCK_68_LEN)?.to_vec())
        } else {
            None
        };
        let unknown_15b4_15b8 = if version >= player_record_versions::UNKNOWN_15B4_15B8 {
            Some([cursor.u32()?, cursor.u32()?])
        } else {
            None
        };
        let unknown_40 = if version >= player_record_versions::UNKNOWN_40 {
            Some(cursor.u32()?)
        } else {
            None
        };

        Ok(Self {
            slot_index,
            leading_words,
            queue,
            armies,
            unknown_15e4,
            roster,
            interleaved_words,
            unknown_3c,
            flags,
            name_raw,
            unknown_15d8,
            block_68_raw,
            unknown_15b4_15b8,
            unknown_40,
        })
    }
}

/// Per-player state. **Decoded, 2026-09-18.**
///
/// **Observed in a local binary.** The section is `{ u32 slot_index; record }*` terminated by
/// `u32 -1`, followed by **eight `u32` lord codes** that match [`MultiplayerSection`]'s slots
/// 0..8 in order (`0x00482E1D` .. `0x00482F33`). The reader validates `0 <= slot_index < 16`.
///
/// **Observed in the corpus.** Parsing each record with the model recovered from `0x004BCE20`
/// lands exactly on the sentinel in **all ten distinct game states** on this machine, with no
/// slack in any file, and the names it recovers at the version-gated name field are the lords'
/// (`Merlin`, `Balkoth`, `Amazon Princess`, ...).
///
/// **Refuted, 2026-09-18: "the record size is not established for version 111".** The premise was
/// wrong, not just the answer. There is **no record size at any version** -- records are trees of
/// counted lists and vary within a single file (6,303 to 7,983 bytes across the corpus). The
/// version-108 file's clean `9 x 6223` is a coincidence of a scenario in which every player has an
/// empty queue, sixteen empty armies and a same-sized bitset; it is not a stride. Two things were
/// therefore being sought that do not exist, which is why "generalising it to version 111 failed
/// on four of six files".
///
/// **Corrected: version and game-age were not in fact confounded here.** The doc recorded that the
/// only version-108 file is also the only turn-1 file, so the 108/111 difference might have been
/// game age. The reader's gates settle it without a new sample: version 111 stores three words
/// (`+0x15b4`, `+0x15b8`, `+0x40`) that 108 does not, gated at `0x004BCD9F` and `0x004BCDD0`, and
/// 12 bytes per record is exactly the difference the corpus shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSection {
    /// One per seated player, in file order.
    pub records: Vec<PlayerRecord>,
    /// Everything before the sentinel, carried verbatim beside the decode.
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
    /// The reader's bound on a slot index, `0x004BCE4A`.
    pub const MAX_SLOT_INDEX: u32 = 16;

    pub fn parse(payload: &[u8], version: &VersionSection) -> Result<Self, SaveError> {
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

        let body = &payload[..sentinel_offset];
        let mut cursor = Cursor::new(tag, body);
        let mut records = Vec::new();
        while cursor.remaining() != 0 {
            let slot_index = cursor.u32()?;
            if slot_index >= Self::MAX_SLOT_INDEX {
                return Err(SaveError::section(
                    tag,
                    format!(
                        "slot index {slot_index} at +{} is outside the reader's 0..{} range",
                        cursor.offset() - 4,
                        Self::MAX_SLOT_INDEX
                    ),
                ));
            }
            records.push(PlayerRecord::parse(
                &mut cursor,
                slot_index,
                version.version,
            )?);
        }
        cursor.expect_exhausted("the LS_PLR_ records")?;

        Ok(Self {
            records,
            records_raw: body.to_vec(),
            sentinel,
            lord_codes,
        })
    }

    /// Where the sentinel sat, which is `payload_len - 36`.
    pub fn sentinel_offset(&self) -> usize {
        self.records_raw.len()
    }

    /// The first record's slot index.
    pub fn first_slot_index(&self) -> Option<u32> {
        self.records.first().map(|record| record.slot_index)
    }

    /// Every record's on-disk length, recomputed from the decoded content.
    ///
    /// Reported as a list rather than a single number on purpose: the point of this section is
    /// that there is no single number, and an accessor called `record_size` would re-mint the
    /// claim this module just refuted. The arithmetic is a **second implementation** -- the parse
    /// walks forward and this one adds sizes up -- so the two can disagree, which is what makes
    /// the structural check worth running.
    pub fn record_lengths(&self) -> Vec<usize> {
        self.records
            .iter()
            .map(|record| 4 + record.encoded_len())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// LS_REGN
// ---------------------------------------------------------------------------

/// One region. **Observed in a local binary, 2026-09-18**, `0x004C5840` (writer) and `0x004C5950`
/// (reader).
///
/// ```text
///   u8  +0x08
///   u8  +0x09
///   u8  name_len         strlen(name) + 1, or 0 when the engine's pointer is null
///   name_len bytes       the name INCLUDING its NUL
///   u32 +0x10
///   6 x 64 bytes         +0x18, +0x58, +0x98, +0xd8, +0x118, +0x158
/// ```
///
/// So a region is **391 bytes plus its name**, and the name's length byte is a `u8` -- a region
/// name longer than 254 characters cannot be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionRecord {
    /// `+0x08`, `+0x09`. Meanings **Unknown**. Across the corpus these two always hold the same
    /// value as each other, and the values are the eight powers of two 1..128.
    pub bytes_8_9: [u8; 2],
    /// The name exactly as stored, terminator included. Empty when the engine's pointer was null.
    pub name_raw: Vec<u8>,
    /// `+0x10`. Meaning **Unknown**.
    pub unknown_10: u32,
    /// Six 64-byte blocks. Structure **Unknown**, carried verbatim.
    pub blocks_raw: [[u8; RegionRecord::BLOCK_LEN]; RegionRecord::BLOCK_COUNT],
}

impl RegionRecord {
    pub const BLOCK_LEN: usize = 64;
    pub const BLOCK_COUNT: usize = 6;
    /// Everything but the name: `1 + 1 + 1 + 4 + 6 * 64`.
    pub const FIXED_LEN: usize = 3 + 4 + Self::BLOCK_COUNT * Self::BLOCK_LEN;

    /// The name up to its NUL.
    pub fn name(&self) -> &[u8] {
        let end = self
            .name_raw
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(self.name_raw.len());
        &self.name_raw[..end]
    }

    fn parse(cursor: &mut Cursor<'_>) -> Result<Self, SaveError> {
        let bytes_8_9 = [cursor.u8()?, cursor.u8()?];
        let name_len = usize::from(cursor.u8()?);
        let name_raw = cursor.take(name_len)?.to_vec();
        let unknown_10 = cursor.u32()?;
        let mut blocks_raw = [[0_u8; Self::BLOCK_LEN]; Self::BLOCK_COUNT];
        for block in &mut blocks_raw {
            *block = cursor.array::<{ RegionRecord::BLOCK_LEN }>()?;
        }
        Ok(Self {
            bytes_8_9,
            name_raw,
            unknown_10,
            blocks_raw,
        })
    }
}

/// The region grid and the region table. **Decoded, 2026-09-18.**
///
/// **Observed in a local binary**, `0x004C7390` (writer) and `0x004C7450` (reader):
///
/// ```text
///   u32 width
///   u32 height
///   width * height * 6 bytes       the region grid; cell layout Unknown
///   u32 array_count                [this+0x1b0]
///   (array_count + 1) records      0x004C5840 each
/// ```
///
/// The trailing `+1` is real and is not an off-by-one: after the counted array at `[this+0x1ac]`
/// the writer calls the same record writer once more on the object embedded at `[this+0x10]`
/// (`0x004C742E`). Across the corpus that final record is the only one carrying a name, and the
/// name is empty -- a one-byte NUL -- which is exactly what a non-null pointer to an empty string
/// produces.
///
/// **Observed in the corpus.** The tail accounts to the byte in **all ten distinct game states**.
/// The three tail lengths the previous pass could only list -- 8,998 / 9,389 / 9,780 -- are
/// `4 + n * 391 + 1` for n = 23, 24, 25, and the 391-byte gaps between them are one region each.
/// Three lengths differing by exactly the fixed record size is what the earlier reading was
/// looking at without a record size to compare it to.
///
/// **Corrected: the `[0x0054D0D8]` / `[0x0054D0DC]` pair is a lock, not a transform.** They bracket
/// the writer's I/O at `0x004C739D` and `0x004C7438` and take a pointer to `[this+0x1bc]`, an
/// object field, with no other argument and no return use -- the shape of an
/// `EnterCriticalSection` / `LeaveCriticalSection` pair and not of a codec, which would need the
/// buffer and a length. The bytes between them decode without any transform applied, which is the
/// independent confirmation: a cipher that leaves 391-byte records and readable region structure
/// in place is not a cipher. The no-encryption finding no longer carries an asterisk here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionSection {
    pub width: u32,
    pub height: u32,
    /// One six-byte record per cell. Field layout **Unknown**, so the bytes are carried whole.
    pub cells: Vec<[u8; RegionSection::CELL_LEN]>,
    /// The stored count of the region **array**. One more region follows it.
    pub array_count: u32,
    /// `array_count + 1` regions: the array, then the embedded one at `[this+0x10]`.
    pub regions: Vec<RegionRecord>,
    /// Everything after the grid, carried verbatim beside the decode.
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

        let tail = &payload[grid_end..];
        let mut cursor = Cursor::new(tag, tail);
        let array_count = cursor.u32()?;
        let written = usize::try_from(array_count)
            .ok()
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| SaveError::section(tag, "region count overflow"))?;
        let mut regions = Vec::with_capacity(written.min(4096));
        for _ in 0..written {
            regions.push(RegionRecord::parse(&mut cursor)?);
        }
        cursor.expect_exhausted("the LS_REGN region table")?;

        Ok(Self {
            width,
            height,
            cells,
            array_count,
            regions,
            tail_raw: tail.to_vec(),
        })
    }

    pub fn grid_len(&self) -> usize {
        self.cells.len() * Self::CELL_LEN
    }

    pub fn tail_len(&self) -> usize {
        self.tail_raw.len()
    }

    /// The tail length the decoded regions account for, recomputed from the records.
    ///
    /// A second implementation of the arithmetic, so it can disagree with the parse rather than
    /// restate it: the parse walks forward and this one adds sizes up.
    pub fn accounted_tail_len(&self) -> usize {
        4 + self
            .regions
            .iter()
            .map(|region| RegionRecord::FIXED_LEN + region.name_raw.len())
            .sum::<usize>()
    }
}

// ---------------------------------------------------------------------------
// LS_ALRM
// ---------------------------------------------------------------------------

/// One field of an alarm record, in the order the engine writes it.
///
/// **Observed in a local binary, 2026-09-18.** The six queues differ only in this schedule; every
/// one of them then writes `u32 argument_count`, that many words, and one trailing word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmField {
    /// A plain `u32`.
    Word,
    /// `u32 len` then `len` bytes with no terminator -- a GameScript callback name, written by
    /// `0x004D5F20`.
    Name,
}

/// The six alarm queues, in the order the writer emits them.
///
/// **They are unnamed.** Nothing in the binary names them, and this module will not invent names
/// for six queues it can only tell apart by their element writer's address and field schedule.
/// What each queue is *for* is **Unknown**; what goes in it is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmQueue {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
}

impl AlarmQueue {
    /// In writer order: `0x00482F77` .. `0x00482FBA`.
    pub const ALL: [AlarmQueue; 6] = [
        AlarmQueue::Zero,
        AlarmQueue::One,
        AlarmQueue::Two,
        AlarmQueue::Three,
        AlarmQueue::Four,
        AlarmQueue::Five,
    ];

    /// The element writer's virtual address, which is the evidence for this queue's schedule.
    pub fn element_writer(self) -> u32 {
        match self {
            Self::Zero => 0x0040_B180,
            Self::One => 0x0040_BBE0,
            Self::Two => 0x0040_C650,
            Self::Three => 0x0040_D3B0,
            Self::Four => 0x0040_DDA0,
            Self::Five => 0x0040_EC50,
        }
    }

    /// The fields the element writer emits before the argument vector.
    pub fn schedule(self) -> &'static [AlarmField] {
        use AlarmField::{Name, Word};
        match self {
            Self::Zero => &[Word, Word, Word, Word, Name],
            Self::One => &[Word, Word, Word, Word, Word, Name],
            Self::Two => &[Name],
            Self::Three => &[Name, Word, Name],
            Self::Four => &[Word, Word, Word, Name],
            Self::Five => &[Word, Word, Word, Word, Name],
        }
    }
}

/// One pending GameScript callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmRecord {
    /// The queue's fixed words, in schedule order. Meanings **Unknown** except for queue
    /// [`AlarmQueue::One`]'s first two: see [`AlarmSection::turn`].
    pub words: Vec<u32>,
    /// The callback names, in schedule order. Every queue writes one; [`AlarmQueue::Three`] writes
    /// two.
    pub names: Vec<Vec<u8>>,
    /// The callback's arguments: a stored count, then that many words.
    pub arguments: Vec<u32>,
    /// The word after the arguments. Meaning **Unknown**.
    pub trailer: u32,
}

impl AlarmRecord {
    /// The first name, lossily decoded, for display.
    pub fn name_lossy(&self) -> String {
        self.names
            .first()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default()
    }

    fn parse(cursor: &mut Cursor<'_>, queue: AlarmQueue) -> Result<Self, SaveError> {
        let mut words = Vec::new();
        let mut names = Vec::new();
        for field in queue.schedule() {
            match field {
                AlarmField::Word => words.push(cursor.u32()?),
                AlarmField::Name => names.push(cursor.counted_string()?),
            }
        }
        let argument_count = cursor.count()?;
        let arguments = cursor.words(argument_count)?;
        Ok(Self {
            words,
            names,
            arguments,
            trailer: cursor.u32()?,
        })
    }
}

/// One alarm queue as stored: a count, then that many records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmQueueContents {
    pub queue: AlarmQueue,
    pub records: Vec<AlarmRecord>,
}

/// Pending GameScript callbacks. **Decoded, 2026-09-18.**
///
/// **Observed in a local binary.** `0x00482F77` .. `0x00482FBA` writes **six independent linked
/// lists**, each as `u32 count` followed by that many records. There is no header.
///
/// **Refuted, 2026-09-18: the "eight-word header".** The previous reading -- and the `Corrected`
/// note that replaced an earlier one -- described this section as eight header words followed by
/// records. There are no header words. What was being read as a header is
/// `count(queue 0) = 0`, `count(queue 1)`, and then the **first five fields of queue 1's first
/// record**. The turn genuinely sits at payload word 2 in every file inspected, and it sits there
/// *because* queue 0 is empty in every file inspected; a save with one queue-0 alarm moves it.
/// The eighth "header word" -- the constant 16 -- was the length of the string
/// `monstergenerator`.
///
/// This is the same failure mode the previous `Corrected` note diagnosed, one level up: the
/// correction moved the turn from index 1 to index 2 and kept the frame that produced the error.
/// Indexing into a payload is not a structure.
///
/// **Observed in the corpus.** The six queues account for the payload to the byte in **all ten
/// distinct game states**, and every name they recover is a GameScript callback name --
/// `monstergenerator`, `explore_brain`, `experience_attack_callback`, `spy_brain`,
/// `thief_steal_from_enemy_event`, `engage_special_building_brain`, `unmodify_champion`.
///
/// **Refuted: records do not end with their name.** The name is followed by a stored argument
/// count, that many words, and a trailing word. The previous reading's observation that "the
/// number of fixed `u32` fields before the length word varies between records -- 2 in one case, 10
/// in another" was the argument vector of the *preceding* record being counted as the fixed fields
/// of the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmSection {
    pub queues: Vec<AlarmQueueContents>,
    /// The whole payload, carried verbatim beside the decode.
    pub payload_raw: Vec<u8>,
}

impl AlarmSection {
    /// Queue [`AlarmQueue::One`] carries the turn in its first record's first word, and
    /// `COUNTDOWN_BASE - turn` in the fourth.
    pub const TURN_QUEUE: AlarmQueue = AlarmQueue::One;
    pub const TURN_WORD: usize = 0;
    pub const COUNTDOWN_WORD: usize = 3;

    pub fn parse(payload: &[u8]) -> Result<Self, SaveError> {
        let tag = SectionTag::Alarm;
        let mut cursor = Cursor::new(tag, payload);
        let mut queues = Vec::with_capacity(AlarmQueue::ALL.len());
        for queue in AlarmQueue::ALL {
            let count = cursor.count()?;
            let mut records = Vec::with_capacity(count.min(4096));
            for _ in 0..count {
                records.push(AlarmRecord::parse(&mut cursor, queue)?);
            }
            queues.push(AlarmQueueContents { queue, records });
        }
        cursor.expect_exhausted("the LS_ALRM queues")?;
        Ok(Self {
            queues,
            payload_raw: payload.to_vec(),
        })
    }

    /// Queue [`AlarmQueue::One`]'s first record, which is the one that carries the turn.
    pub fn turn_record(&self) -> Option<&AlarmRecord> {
        self.queues
            .iter()
            .find(|contents| contents.queue == Self::TURN_QUEUE)
            .and_then(|contents| contents.records.first())
    }

    /// The turn, or `None` when the queue that carries it is empty.
    ///
    /// `Option` and not a `u32`: nothing in the format requires this queue to be occupied. It is
    /// occupied in every file inspected here, and that is a corpus regularity, not a requirement.
    pub fn turn(&self) -> Option<u32> {
        self.turn_record()
            .and_then(|record| record.words.get(Self::TURN_WORD).copied())
    }

    pub fn countdown(&self) -> Option<u32> {
        self.turn_record()
            .and_then(|record| record.words.get(Self::COUNTDOWN_WORD).copied())
    }

    /// The turn implied by the countdown word.
    pub fn turn_from_countdown(&self) -> Option<u32> {
        COUNTDOWN_BASE.checked_sub(self.countdown()?)
    }

    /// Every record in every queue, in file order.
    pub fn records(&self) -> impl Iterator<Item = (AlarmQueue, &AlarmRecord)> {
        self.queues
            .iter()
            .flat_map(|contents| contents.records.iter().map(move |r| (contents.queue, r)))
    }

    pub fn record_count(&self) -> usize {
        self.queues
            .iter()
            .map(|contents| contents.records.len())
            .sum()
    }
}

/// The constant the alarm countdown word is measured down from: `countdown == 1000001 - turn`.
///
/// **Observed in the corpus, 2026-09-18**, exact in all ten distinct game states. Whether the
/// engine stores a deadline or a remaining budget is **Unknown**; the name records the arithmetic
/// only.
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
        // Explicit ordering, not a struct literal: `LS_MULT`'s shape depends on `LS_VER_`, and
        // relying on struct-literal field-evaluation order to sequence a real data dependency is
        // the kind of thing that survives until someone reorders the fields alphabetically.
        let version = VersionSection::parse(container.payload(source, SectionTag::Version)?)?;
        let multiplayer = MultiplayerSection::parse(
            container.payload(source, SectionTag::Multiplayer)?,
            &version,
        )?;
        Ok(Self {
            version,
            multiplayer,
            map: MapSection::parse(container.payload(source, SectionTag::Map)?)?,
            sprites: SpriteSection::parse(
                container.payload(source, SectionTag::Sprites)?,
                &version,
            )?,
            users: UserSection::parse(container.payload(source, SectionTag::User)?)?,
            game: GameSection::parse(container.payload(source, SectionTag::Game)?)?,
            players: PlayerSection::parse(
                container.payload(source, SectionTag::Player)?,
                &version,
            )?,
            regions: RegionSection::parse(container.payload(source, SectionTag::Region)?)?,
            alarms: AlarmSection::parse(container.payload(source, SectionTag::Alarm)?)?,
            container,
        })
    }

    /// Reassemble the whole file, with `LS_SPR_` regenerated from its decoded records and the
    /// other eight payloads copied from `source`.
    ///
    /// **This is not a savegame writer.** There is no writer in this repository.
    ///
    /// **Corrected, 2026-09-18.** This comment used to claim that "if the record model got any
    /// record's extent wrong, `LS_SPR_`'s re-encoded payload would be a different length and the
    /// file would not match byte for byte." That is false unconditionally -- see
    /// [`SpriteSection::encode`], which this wraps: once `parse` has succeeded the re-encode is
    /// an **identity**, so the payload can never come out a different length. The correction had
    /// been applied to `encode` and to `docs/save-format.md` and **not here**, which is the
    /// sibling an API consumer reads first.
    ///
    /// What a match proves is what [`SpriteSection::encode`] says it proves: lossless
    /// preservation and correct container splicing. It does not prove any record's field
    /// boundaries, and a compensating pair of errors passes it -- `tools/mutate_save_constants.py`
    /// runs one.
    ///
    /// `source` must be the bytes this `SaveFile` was parsed from; passing anything else compares
    /// two unrelated files.
    pub fn reencode_with_sprites(&self, source: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(source.len());
        for location in self.container.locations() {
            out.extend_from_slice(&source[location.tag_offset..location.payload_offset]);
            if location.tag == SectionTag::Sprites {
                out.extend_from_slice(&self.sprites.encode());
            } else {
                out.extend_from_slice(&source[location.payload_offset..location.payload_end()]);
            }
        }
        out
    }

    /// The three independent turn readings, for a caller that wants to show them rather than a
    /// boolean.
    ///
    /// `LS_GAME`'s turn, and the two readings inside `LS_ALRM`'s turn-carrying record.
    ///
    /// The alarm readings are `Option` because the queue that carries them is not required to be
    /// occupied. It is occupied in every file inspected here; that is a corpus regularity and this
    /// signature refuses to promote it to a guarantee.
    pub fn turn_readings(&self) -> (u32, Option<u32>, Option<u32>) {
        (
            self.game.turn,
            self.alarms.turn(),
            self.alarms.turn_from_countdown(),
        )
    }

    pub fn turn_agreement(&self) -> bool {
        let (game, alarm, countdown) = self.turn_readings();
        alarm == Some(game) && countdown == Some(game)
    }

    /// Every invariant this module knows how to check, each carrying its **measured value**.
    ///
    /// Measured values are not optional decoration. The `LS_ALRM` off-by-one that this module
    /// corrects passed a pass/fail check for months -- *some* field equalled the turn, so the
    /// invariant "held"; it was the wrong field. Printing the number is what catches that.
    /// The **corpus regularities**: things every inspected save happens to do, which nothing in
    /// the format requires.
    ///
    /// These are deliberately *not* mixed in with [`SaveContainer::structural_checks`]. A real save
    /// that breaks one of these is a **discovery**, not a malformed file -- `N - live_count` being
    /// 71 is an unexplained constant over every game state in the corpus, and a save with 72
    /// would be the most
    /// interesting file in the corpus. Reporting it as a failure and exiting nonzero would train a
    /// reader to ignore exactly the signal worth acting on.
    pub fn regularities(&self) -> Vec<Invariant> {
        let mut checks = Vec::new();
        let regularity = |name, measured, passed| {
            Invariant::new(name, InvariantKind::Regularity, measured, passed)
        };

        checks.push(regularity(
            "LS_MAP_: plane count == cell count",
            format!(
                "count={} cells={}",
                self.map.plane_count,
                self.map.map.cells.len()
            ),
            self.map.plane_covers_every_cell(),
        ));
        // Forced by the writer, so on a well-formed save this cannot fail -- which is exactly
        // why it belongs among the **regularities** and not the structural checks. A file that
        // breaks it is damaged or came from another writer, and that is a discovery about the
        // file, not a failure of this parser's model.
        checks.push(regularity(
            "LS_SPR_: no record disagrees with its own class-id echo",
            format!(
                "{} of {} records disagree",
                self.sprites.class_id_echo_disagreements(),
                self.sprites.records.len()
            ),
            self.sprites.class_id_echo_disagreements() == 0,
        ));
        checks.push(regularity(
            "LS_MAP_: visibility levels are a subset of {0, 63, 128}",
            format!("{:?}", self.map.visibility_histogram()),
            self.map
                .visibility_histogram()
                .keys()
                .all(|level| OBSERVED_VISIBILITY_LEVELS.contains(level)),
        ));
        checks.push(regularity(
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
        checks.push(regularity(
            "LS_GAME: N - live_count == 71",
            format!(
                "{} - {} = {}",
                self.game.records.len(),
                self.game.live_count,
                self.game.record_surplus()
            ),
            self.game.record_surplus() == OBSERVED_GAME_RECORD_SURPLUS,
        ));
        checks.push(regularity(
            "LS_GAME: stored record size == 12",
            format!("{}", self.game.declared_record_size),
            usize::try_from(self.game.declared_record_size) == Ok(GameSection::RECORD_LEN),
        ));
        checks.push(regularity(
            "LS_PLR_: 8 lord codes == LS_MULT slots 0..8",
            format!(
                "{:?} vs {:?}",
                self.players.lord_codes,
                self.multiplayer.seated_lord_codes()
            ),
            self.multiplayer
                .seated_lord_codes()
                .is_some_and(|codes| codes == self.players.lord_codes),
        ));
        checks.push(regularity(
            "LS_REGN: tail length is one of 8998 / 9389 / 9780",
            format!("{}", self.regions.tail_len()),
            OBSERVED_REGION_TAIL_LENGTHS.contains(&self.regions.tail_len()),
        ));

        let (game_turn, alarm_turn, countdown_turn) = self.turn_readings();
        let describe = |turn: Option<u32>| {
            turn.map(|turn| turn.to_string())
                .unwrap_or_else(|| "absent".to_owned())
        };
        checks.push(regularity(
            "turn: LS_GAME agrees with the LS_ALRM turn record and its countdown",
            format!(
                "{game_turn} / {} / {}",
                describe(alarm_turn),
                describe(countdown_turn)
            ),
            self.turn_agreement(),
        ));
        let turn_record = self.alarms.turn_record();
        checks.push(regularity(
            "LS_ALRM: queue 0 is empty, so the turn lands at payload word 2",
            format!(
                "{}",
                self.alarms
                    .queues
                    .first()
                    .map_or(usize::MAX, |queue| queue.records.len())
            ),
            self.alarms
                .queues
                .first()
                .is_some_and(|queue| queue.records.is_empty()),
        ));
        checks.push(regularity(
            "LS_ALRM: the turn record is monstergenerator with words 15, 1 and 0",
            match turn_record {
                Some(record) => format!("{:?} {:?}", record.name_lossy(), record.words),
                None => "queue 1 is empty".to_owned(),
            },
            turn_record.is_some_and(|record| {
                record.words.get(1) == Some(&15)
                    && record.words.get(2) == Some(&1)
                    && record.words.get(4) == Some(&0)
                    && record.names.first().map(Vec::as_slice)
                        == Some(b"monstergenerator".as_slice())
            }),
        ));
        let named_regions = self
            .regions
            .regions
            .iter()
            .filter(|region| !region.name_raw.is_empty())
            .count();
        checks.push(regularity(
            "LS_REGN: exactly one region stores a name, and it is the empty string",
            format!(
                "{named_regions} named; last name {:?}",
                self.regions
                    .regions
                    .last()
                    .map(|region| String::from_utf8_lossy(region.name()).into_owned())
            ),
            named_regions == 1
                && self
                    .regions
                    .regions
                    .last()
                    .is_some_and(|region| region.name().is_empty() && region.name_raw == [0]),
        ));
        checks.push(regularity(
            "LS_PLR_: the record lengths sum to the body",
            format!(
                "{} vs {}",
                self.players.record_lengths().iter().sum::<usize>(),
                self.players.sentinel_offset()
            ),
            self.players.record_lengths().iter().sum::<usize>() == self.players.sentinel_offset(),
        ));

        checks
    }
}

/// How many bytes `LS_ALRM`'s six queues account for, walked straight over the raw payload.
///
/// A **second implementation** of [`AlarmSection::parse`], deliberately sharing no code with it:
/// this one counts and never builds a record. An invariant read back off the structs `parse`
/// already validated cannot fail, which is the same defect as a test that cannot fail -- and this
/// one runs on files `SaveFile::parse` refuses.
fn account_for_alarm_queues(payload: &[u8]) -> Option<usize> {
    let mut at = 0_usize;
    let word = |at: &mut usize| -> Option<u32> {
        let bytes = payload.get(*at..at.checked_add(4)?)?;
        *at += 4;
        Some(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
    };
    for queue in AlarmQueue::ALL {
        let count = word(&mut at)?;
        for _ in 0..count {
            for field in queue.schedule() {
                match field {
                    AlarmField::Word => {
                        word(&mut at)?;
                    }
                    AlarmField::Name => {
                        let len = usize::try_from(word(&mut at)?).ok()?;
                        at = at.checked_add(len)?;
                        payload.get(..at)?;
                    }
                }
            }
            let arguments = usize::try_from(word(&mut at)?).ok()?;
            at = at.checked_add(arguments.checked_mul(4)?)?;
            payload.get(..at)?;
            word(&mut at)?;
        }
    }
    Some(at)
}

/// How many bytes `LS_REGN`'s region table accounts for, walked straight over the raw tail.
///
/// The counterpart of [`account_for_alarm_queues`], and for the same reason.
fn account_for_region_table(tail: &[u8]) -> Option<usize> {
    let count = u32::from_le_bytes(tail.get(..4)?.try_into().expect("four bytes"));
    let mut at = 4_usize;
    for _ in 0..u64::from(count) + 1 {
        let name_len = usize::from(*tail.get(at.checked_add(2)?)?);
        at = at.checked_add(3)?.checked_add(name_len)?;
        at = at.checked_add(4 + RegionRecord::BLOCK_COUNT * RegionRecord::BLOCK_LEN)?;
        tail.get(..at)?;
    }
    Some(at)
}

/// The `LS_REGN` tail lengths the corpus contains. Unexplained; see `docs/save-format.md`.
pub const OBSERVED_REGION_TAIL_LENGTHS: [usize; 3] = [8998, 9389, 9780];

/// Whether a check is a format requirement or an empirical regularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvariantKind {
    /// A genuine requirement of the format. A violation means the file is malformed, or that this
    /// project's model of the container is wrong. A caller should treat it as an error.
    Structural,
    /// Something every inspected save happens to do, which nothing in the format requires. A
    /// violation is a **discovery** and must not be reported as a malformed file.
    Regularity,
}

/// One invariant check, carrying **what was measured** as well as whether it held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invariant {
    pub name: &'static str,
    pub kind: InvariantKind,
    /// The measured value, formatted. Always populated, including on a pass.
    pub measured: String,
    pub passed: bool,
}

impl Invariant {
    fn new(name: &'static str, kind: InvariantKind, measured: String, passed: bool) -> Self {
        Self {
            name,
            kind,
            measured,
            passed,
        }
    }

    pub fn is_structural(&self) -> bool {
        self.kind == InvariantKind::Structural
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
        /// The `LS_MAP_` plane's stored word count. `None` means `width * height`.
        map_plane_words: Option<u32>,
        region_width: u32,
        region_height: u32,
        /// One entry per region the fixture emits, innermost first; the last is the embedded one.
        /// `None` is a null name pointer, `Some` a stored NUL-terminated name. The stored array
        /// count is `len() - 1`.
        region_names: Vec<Option<&'static str>>,
        /// `Some` forces the slot block on or off; `None` decides from the fixture's own literal
        /// threshold. **Never from `MULTIPLAYER_SLOTS_MIN_VERSION`** -- a fixture generated from
        /// the constant it is testing moves with the constant and cannot fail on it.
        emit_mult_slots: Option<bool>,
        turn: u32,
        /// How many records each of the six alarm queues holds. **Queue 0 is deliberately not
        /// empty**: it is empty in every corpus file, which is the only reason the turn lands at
        /// payload word 2 there, and a fixture that copied that could not fail on a reader which
        /// went back to indexing the payload.
        alarm_queue_records: [usize; 6],
        /// The argument count every synthetic alarm record carries.
        alarm_arguments: usize,
        game_live_count: u32,
        game_records: u32,
        /// One entry per `LS_SPR_` record. The shapes differ on purpose: the records are
        /// polymorphic and three of the eight classes are variable-length, so a fixture whose
        /// records were all one class could not fail on a reader that guessed a stride.
        sprite_records: Vec<SpriteFixture>,
        declared_setup_len: u32,
        lord_codes: [u32; 16],
        lord_names: [&'static str; 16],
        /// Bytes written into each name field *after* the terminator, standing in for the
        /// uninitialised process memory the engine leaks there.
        name_padding_fill: u8,
        /// One entry per `LS_PLR_` record. The shapes differ from each other on purpose: a record
        /// has no size, and a fixture whose records are all the same length cannot fail on a
        /// reader that assumes one.
        player_records: Vec<PlayerFixture>,
        order: Vec<SectionTag>,
    }

    impl Default for Fixture {
        fn default() -> Self {
            Self {
                version: 111,
                map_width: 96,
                map_height: 64,
                map_plane_words: None,
                region_width: 96,
                region_height: 64,
                region_names: vec![None, Some("Ruins of Balkoth"), Some(""), None],
                emit_mult_slots: None,
                turn: 42,
                alarm_queue_records: [1, 2, 3, 1, 2, 4],
                alarm_arguments: 3,
                game_live_count: 9,
                game_records: 80,
                sprite_records: SpriteFixture::default_set(),
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
                player_records: PlayerFixture::default_set(),
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

    /// `u32 len` then the bytes, with no terminator -- the engine's counted string.
    fn push_name(buffer: &mut Vec<u8>, name: &str) {
        push_u32(buffer, name.len() as u32);
        buffer.extend_from_slice(name.as_bytes());
    }

    /// `u32 bit_count` then `ceil(bit_count / 32)` words.
    fn push_bitset(buffer: &mut Vec<u8>, bits: u32, fill: u8) {
        push_u32(buffer, bits);
        // Literal 32, not a constant shared with the parser.
        let words = (bits as usize).div_ceil(32);
        buffer.extend(std::iter::repeat_n(fill, words * 4));
    }

    // -- LS_SPR_ fixture builders -------------------------------------------
    //
    // A deliberately **second implementation** of the record layouts, written from the
    // disassembly with literal widths and literal version gates rather than from the parser's
    // constants. A fixture generated from `SPR_*` would move with any mutation of them and could
    // not fail on one.

    /// One of the four nested classes the factory at `0x0044B4F0` builds, as a fixture.
    #[derive(Debug, Clone, Default)]
    struct NestedFixture {
        type_id: u32,
        /// Nested type 3 only: one entry per group, holding that group's item count.
        groups: Vec<usize>,
        /// Emit this raw dword as the group count and emit no groups. For driving the signed
        /// guard at `0x0044BCC7` with a value `Vec::len()` cannot produce.
        groups_override: Option<u32>,
        /// Emit this raw dword as **each group's** item count and emit no items. For driving the
        /// **unguarded** `test / je` loop at `0x0044BD07`.
        group_items_override: Option<u32>,
    }

    /// One of class 0's per-slot records.
    #[derive(Debug, Clone, Default)]
    struct SlotFixture {
        nested: Option<NestedFixture>,
        items_a: usize,
        items_b: usize,
        /// Emit this raw dword as the `items_a` count and emit no items. For driving the
        /// **unguarded** `test / je` loop at `0x00524FC6` with a value `Vec::len()` cannot make.
        items_a_override: Option<u32>,
        /// The same for `items_b`, whose guard is the `test / je` at `0x0052503B`.
        items_b_override: Option<u32>,
    }

    /// The body of a class-0 record, which three other classes also embed whole.
    #[derive(Debug, Clone, Default)]
    struct Class0Fixture {
        slots: Vec<SlotFixture>,
        /// The counted byte array at `0x00427A40`. Not text: the engine never terminates it and
        /// the corpus holds small integers there.
        counted: Vec<u8>,
        /// Emit this raw dword as the slot count and emit no slots. Drives `0x004122FC`.
        slots_override: Option<u32>,
        /// Emit this raw dword as the counted array's length and emit no bytes. Drives
        /// `0x00427A5E`.
        counted_len_override: Option<u32>,
    }

    #[derive(Debug, Clone)]
    struct SpriteFixture {
        class_id: u32,
        class0: Class0Fixture,
        /// Classes 2 and 3: the counted blob each stores before anything else.
        blob: Vec<u8>,
        /// Classes 1, 2 and 3: whether the optional embedded class-0 record is present.
        embed_class0: bool,
        /// Class 1 only: one entry per array element, holding whether it embeds a class 0.
        class1_entries: Vec<bool>,
        /// Class 3 only: the number of 36-byte tail items.
        class3_tail: usize,
        /// Emit this raw dword as class 3's tail count and emit no items. Drives `0x00452EBF`.
        class3_tail_override: Option<u32>,
        /// Emit this raw word as class 1's array count and emit no entries. Drives `0x0050DC81`.
        class1_entries_override: Option<u16>,
    }

    impl SpriteFixture {
        fn bare(class_id: u32) -> Self {
            Self {
                class_id,
                class0: Class0Fixture::default(),
                blob: Vec::new(),
                embed_class0: false,
                class1_entries: Vec::new(),
                class3_tail: 0,
                class3_tail_override: None,
                class1_entries_override: None,
            }
        }

        /// One record of every class that is not one of the two the dispatch table raises on,
        /// each carrying a different shape.
        fn default_set() -> Vec<Self> {
            let rich = Class0Fixture {
                slots: vec![
                    SlotFixture {
                        nested: None,
                        items_a: 0,
                        items_b: 0,
                        ..Default::default()
                    },
                    SlotFixture {
                        nested: Some(NestedFixture {
                            type_id: 0,
                            groups: Vec::new(),
                            ..Default::default()
                        }),
                        items_a: 2,
                        items_b: 1,
                        ..Default::default()
                    },
                    SlotFixture {
                        nested: Some(NestedFixture {
                            type_id: 3,
                            groups: vec![0, 2, 1, 0, 3, 0],
                            ..Default::default()
                        }),
                        items_a: 1,
                        items_b: 4,
                        ..Default::default()
                    },
                ],
                counted: vec![3, 4, 5, 0, 7, 9],
                ..Default::default()
            };
            let mut class0 = Self::bare(0);
            class0.class0 = rich.clone();

            let mut class1 = Self::bare(1);
            class1.embed_class0 = true;
            class1.class0 = rich.clone();
            class1.class1_entries = vec![false, true, false];

            let mut class2 = Self::bare(2);
            class2.blob = (0..37_u8).collect();
            class2.embed_class0 = true;

            let mut class3 = Self::bare(3);
            class3.blob = vec![0xA5; 700];
            class3.class3_tail = 2;

            let mut class3_bare = Self::bare(3);
            class3_bare.blob = vec![0x5A; 12];

            let mut nested_type_two = Self::bare(0);
            nested_type_two.class0 = Class0Fixture {
                slots: vec![SlotFixture {
                    nested: Some(NestedFixture {
                        type_id: 2,
                        groups: Vec::new(),
                        ..Default::default()
                    }),
                    items_a: 3,
                    items_b: 0,
                    ..Default::default()
                }],
                counted: Vec::new(),
                ..Default::default()
            };

            vec![
                class0,
                Self::bare(8),
                class1,
                class2,
                Self::bare(4),
                class3,
                Self::bare(7),
                Self::bare(9),
                class3_bare,
                nested_type_two,
            ]
        }

        fn push(&self, out: &mut Vec<u8>, version: i32) {
            push_u32(out, self.class_id);
            push_spr_base(out, self.class_id);
            match self.class_id {
                0 => push_spr_class0_body(out, version, &self.class0),
                1 => {
                    push_filler(out, 4);
                    if version >= 0x62 {
                        push_filler(out, 2);
                    } else {
                        push_filler(out, 8);
                    }
                    if version >= 0x36 {
                        push_filler(out, 4);
                        push_u32(out, u32::from(self.embed_class0));
                        if self.embed_class0 {
                            push_spr_embedded_class0(out, version, &self.class0);
                        }
                    }
                    if version >= 0x4A {
                        push_filler(out, 4);
                    }
                    if version >= 0x60 {
                        push_filler(out, 1);
                    }
                    if version >= 0x66 {
                        let stored = self
                            .class1_entries_override
                            .unwrap_or(self.class1_entries.len() as u16);
                        out.extend_from_slice(&stored.to_le_bytes());
                        let entries: &[bool] = if self.class1_entries_override.is_some() {
                            &[]
                        } else {
                            &self.class1_entries
                        };
                        for entry in entries {
                            push_filler(out, 8);
                            push_u32(out, u32::from(*entry));
                            if *entry {
                                push_spr_embedded_class0(out, version, &self.class0);
                            }
                        }
                    }
                }
                2 => {
                    push_u32(out, self.blob.len() as u32);
                    out.extend_from_slice(&self.blob);
                    push_filler(out, 4);
                    if version >= 0x3B {
                        push_u32(out, u32::from(self.embed_class0));
                        if self.embed_class0 {
                            push_spr_embedded_class0(out, version, &self.class0);
                        }
                    }
                    if version >= 0x5A {
                        push_filler(out, 4);
                    }
                }
                3 => {
                    // Below 0x34 the writer stores no length and the reader assumes 700 bytes, so
                    // the fixture emits 700 whatever length it was asked for. Padding here rather
                    // than refusing is what lets the version sweeps drive the same record set
                    // across the whole ladder.
                    if version >= 0x34 {
                        push_u32(out, self.blob.len() as u32);
                        out.extend_from_slice(&self.blob);
                    } else {
                        let mut blob = self.blob.clone();
                        blob.resize(0x2BC, 0x7E);
                        out.extend_from_slice(&blob);
                    }
                    push_u32(out, u32::from(self.embed_class0));
                    if self.embed_class0 {
                        push_spr_embedded_class0(out, version, &self.class0);
                    }
                    if version >= 0x53 {
                        match self.class3_tail_override {
                            Some(raw) => push_u32(out, raw),
                            None => {
                                push_u32(out, self.class3_tail as u32);
                                push_filler(out, 36 * self.class3_tail);
                            }
                        }
                    }
                }
                4 | 7 => push_filler(out, 44),
                8 => {}
                9 => push_filler(out, 92 + 40),
                other => panic!("no fixture for class id {other}"),
            }
        }
    }

    /// Bytes that are recognisably not zero, so a reader that skips a field instead of consuming
    /// it lands on something that stands out in a failure message.
    fn push_filler(out: &mut Vec<u8>, len: usize) {
        out.extend((0..len).map(|index| (index % 251) as u8 | 0x40));
    }

    /// The six dwords of the base block, the first of which is the class id a second time.
    fn push_spr_base(out: &mut Vec<u8>, class_id: u32) {
        push_u32(out, class_id);
        push_u32(out, 0x2246);
        push_u32(out, 0xffff_ffff);
        push_u32(out, 2);
        push_u32(out, 0xc8);
        push_u32(out, 0x2002_1050);
    }

    /// The per-slot blob width ladder at `0x00524E3C`, written out as literals.
    fn spr_fixture_slot_width(version: i32) -> usize {
        if version < 0x48 {
            0x20
        } else if version < 0x49 {
            0x24
        } else if version < 0x65 {
            0x28
        } else if version < 0x67 {
            0x48
        } else {
            0x4C
        }
    }

    fn push_spr_item_a(out: &mut Vec<u8>, version: i32) {
        push_filler(out, 16);
        if version >= 0x43 {
            push_filler(out, 8);
        }
        if version >= 0x51 {
            push_filler(out, 4);
        }
    }

    fn push_spr_item_b(out: &mut Vec<u8>, version: i32) {
        push_filler(out, 12);
        if version >= 0x4B {
            push_filler(out, 4);
        }
        if version >= 0x58 {
            push_filler(out, 4);
        }
    }

    fn push_spr_nested_base(out: &mut Vec<u8>, version: i32) {
        push_filler(out, 4);
        // The two gates on the build constant at `0x0055B1B0` are dead in this build; the fixture
        // says so by not emitting their fields.
        push_filler(out, 4);
        if version < 0x65 {
            push_filler(out, 0x1F);
        }
        if version < 0x3E {
            push_filler(out, 8);
        }
        push_filler(out, 4);
    }

    fn push_spr_nested(out: &mut Vec<u8>, version: i32, nested: &NestedFixture) {
        push_spr_nested_base(out, version);
        match nested.type_id {
            0 => {
                if version < 0x3C {
                    push_filler(out, 16);
                }
                push_filler(out, 4);
                if version >= 0x4F {
                    push_filler(out, 12);
                }
                push_filler(out, 4);
            }
            1 | 2 => {
                if version < 0x3C {
                    push_filler(out, 4);
                }
            }
            3 => {
                // Below 0x55 the group count is not stored and the loop is a fixed six.
                let mut groups = nested.groups.clone();
                if version >= 0x55 {
                    match nested.groups_override {
                        Some(raw) => {
                            push_u32(out, raw);
                            groups.clear();
                        }
                        None => push_u32(out, groups.len() as u32),
                    }
                } else {
                    groups.resize(6, 0);
                    groups.truncate(6);
                }
                for items in &groups {
                    push_filler(out, 4);
                    match nested.group_items_override {
                        Some(raw) => push_u32(out, raw),
                        None => {
                            push_u32(out, *items as u32);
                            for _ in 0..*items {
                                push_spr_item_a(out, version);
                            }
                        }
                    }
                }
            }
            other => panic!("no fixture for nested type id {other}"),
        }
    }

    fn push_spr_slot(out: &mut Vec<u8>, version: i32, slot: &SlotFixture) {
        let width = spr_fixture_slot_width(version);
        let mut blob = vec![0_u8; width];
        for (index, byte) in blob.iter_mut().enumerate() {
            *byte = (index % 241) as u8 | 0x20;
        }
        // The word at blob offset 0x14 is what `0x00524EE0` tests.
        let present = u32::from(slot.nested.is_some());
        blob[0x14..0x18].copy_from_slice(&present.to_le_bytes());
        out.extend_from_slice(&blob);
        // A save older than 0x37 has the flag forced to zero, so the nested object is absent even
        // when the fixture asked for one.
        if version >= 0x37
            && let Some(nested) = &slot.nested
        {
            push_u32(out, nested.type_id);
            push_spr_nested(out, version, nested);
        }
        match slot.items_a_override {
            Some(raw) => push_u32(out, raw),
            None => {
                push_u32(out, slot.items_a as u32);
                for _ in 0..slot.items_a {
                    push_spr_item_a(out, version);
                }
            }
        }
        if version >= 0x3E {
            match slot.items_b_override {
                Some(raw) => push_u32(out, raw),
                None => {
                    push_u32(out, slot.items_b as u32);
                    for _ in 0..slot.items_b {
                        push_spr_item_b(out, version);
                    }
                }
            }
        }
    }

    fn push_spr_class0_body(out: &mut Vec<u8>, version: i32, class0: &Class0Fixture) {
        push_filler(out, 8);
        match class0.slots_override {
            Some(raw) => push_u32(out, raw),
            None => {
                push_u32(out, class0.slots.len() as u32);
            }
        }
        push_filler(out, 4);
        if class0.slots_override.is_none() {
            for slot in &class0.slots {
                push_spr_slot(out, version, slot);
            }
        }
        push_filler(out, 16);
        match class0.counted_len_override {
            Some(raw) => push_u32(out, raw),
            None => {
                push_u32(out, class0.counted.len() as u32);
                out.extend_from_slice(&class0.counted);
            }
        }
        if version < 0x3F {
            push_filler(out, 16);
        }
        push_filler(out, 12);
        push_filler(out, 88);
        if version >= 0x33 {
            push_filler(out, 4);
            if version >= 0x38 {
                push_filler(out, 4);
            }
            if version >= 0x3D {
                push_filler(out, 24);
            }
            if version >= 0x42 {
                push_filler(out, if version >= 0x5D { 4 } else { 1 });
                push_filler(out, 1);
            }
            if (0x59..=0x5B).contains(&version) {
                push_filler(out, 4);
            }
            if version >= 0x6A {
                push_filler(out, 1);
            }
        }
    }

    fn push_spr_embedded_class0(out: &mut Vec<u8>, version: i32, class0: &Class0Fixture) {
        push_spr_base(out, 0);
        push_spr_class0_body(out, version, class0);
    }

    /// One synthetic `LS_PLR_` record. The shapes are deliberately unequal.
    #[derive(Debug, Clone)]
    struct PlayerFixture {
        slot: u32,
        queue_len: usize,
        /// Units in each of the sixteen armies.
        unit_counts: [usize; 16],
        army_bits: u32,
        flag_bits: u32,
        roster_entries: u32,
        roster_slots: u32,
        name: &'static str,
    }

    impl PlayerFixture {
        /// Three records, no two the same length, and slot 15 -- the neutral pseudo-player.
        fn default_set() -> Vec<Self> {
            vec![
                Self {
                    slot: 0,
                    queue_len: 2,
                    unit_counts: [1, 0, 3, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5],
                    army_bits: 64,
                    flag_bits: 300,
                    roster_entries: 4,
                    roster_slots: 22,
                    name: "Merlin",
                },
                Self {
                    slot: 3,
                    queue_len: 0,
                    unit_counts: [0; 16],
                    army_bits: 1,
                    flag_bits: 31,
                    roster_entries: 0,
                    roster_slots: 7,
                    name: "Amazon Princess",
                },
                Self {
                    slot: 15,
                    queue_len: 7,
                    unit_counts: [2; 16],
                    army_bits: 129,
                    flag_bits: 0,
                    roster_entries: 9,
                    roster_slots: 40,
                    name: "",
                },
            ]
        }

        fn write(&self, out: &mut Vec<u8>, version: u32) {
            push_u32(out, self.slot);
            for word in 0..3_u32 {
                push_u32(out, 0x1500 + word);
            }
            push_u32(out, self.queue_len as u32);
            for entry in 0..self.queue_len {
                for word in 0..6_u32 {
                    push_u32(out, 0xb800 + entry as u32 * 16 + word);
                }
            }
            for (army, units) in self.unit_counts.iter().enumerate() {
                for word in 0..3_u32 {
                    push_u32(out, 0xa360 + army as u32 * 16 + word);
                }
                push_u32(out, *units as u32);
                for unit in 0..*units {
                    for word in 0..5_u32 {
                        push_u32(out, 0x9fc0 + unit as u32 * 8 + word);
                    }
                }
                // Literal 100: the block `0x0049F2A0` writes.
                out.extend((0..100_usize).map(|byte| (army + byte) as u8 | 0x80));
                push_bitset(out, self.army_bits, 0xa5);
                push_u32(out, 0xa36c);
                push_u32(out, 0xa374);
            }
            push_u32(out, 0x15e4);
            // The roster.
            push_u32(out, 0x6800);
            push_u32(out, 0x6c00);
            push_u32(out, self.roster_entries);
            push_u32(out, self.roster_slots);
            for slot in 0..self.roster_slots {
                push_u32(out, 0xc600 + slot);
            }
            out.extend((0..64_usize).map(|byte| byte as u8 | 0x80));
            // Version gates, as literals. Never `player_record_versions::*`: a fixture generated
            // from the ladder it is testing moves with the ladder.
            if version >= 57 {
                for word in 0..15_u32 {
                    push_u32(out, 0xcfc0 + word);
                }
            }
            if version >= 68 {
                push_u32(out, 0x003c);
            }
            push_bitset(out, self.flag_bits, 0x5a);
            if version >= 76 {
                // Literal 31.
                let mut field = [0_u8; 31];
                field[..self.name.len()].copy_from_slice(self.name.as_bytes());
                out.extend_from_slice(&field);
            }
            if version >= 86 {
                push_u32(out, 0x15d8);
            }
            if version >= 104 {
                // Literal 3200.
                out.extend((0..3200_usize).map(|byte| (byte % 251) as u8 | 0x80));
            }
            if version >= 110 {
                push_u32(out, 0x15b4);
                push_u32(out, 0x15b8);
            }
            if version >= 111 {
                push_u32(out, 0x0040);
            }
        }
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
                    // The fixture's own threshold, written as a literal.
                    let emit = self.emit_mult_slots.unwrap_or((self.version as i32) >= 99);
                    if !emit {
                        return out;
                    }
                    for slot in 0..16_usize {
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
                        // Literal levels. Indexing `OBSERVED_VISIBILITY_LEVELS` here made a
                        // `63 -> 62` mutation self-consistent and it survived the suite.
                        let visibility = [0_u16, 63, 128][(index % 3) as usize];
                        push_u32(&mut out, (u32::from(visibility) << 16) | (index % 600));
                        push_u32(&mut out, 1.5_f32.to_bits());
                    }
                    let plane_words = self.map_plane_words.unwrap_or(cells);
                    push_u32(&mut out, plane_words);
                    for index in 0..plane_words {
                        push_u32(&mut out, index * 3);
                    }
                    push_u32(&mut out, 1);
                }
                SectionTag::Sprites => {
                    push_u32(&mut out, self.sprite_records.len() as u32);
                    for record in &self.sprite_records {
                        record.push(&mut out, self.version as i32);
                    }
                }
                SectionTag::User => {
                    // Literal 8 and literal 784. Generating this from `UserSection::RECORD_COUNT`
                    // and `UserRecord::LEN` made both the fixture and the assertion move with the
                    // constants, and a `RECORD_COUNT` 8 -> 7 mutation survived the whole suite.
                    for index in 0..8_usize {
                        let mut record = vec![0_u8; 784];
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
                    for record in &self.player_records {
                        record.write(&mut out, self.version);
                    }
                    // Literal, not `PlayerSection::SENTINEL`: writing the constant here let a
                    // `SENTINEL` mutation change both sides together and survive.
                    push_u32(&mut out, 0xffff_ffff);
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
                    // The stored count is of the *array*; one further region follows it, so the
                    // count written here is one less than the number of records emitted. A
                    // fixture that wrote `len()` would agree with an off-by-one reader.
                    push_u32(&mut out, self.region_names.len().saturating_sub(1) as u32);
                    for (index, name) in self.region_names.iter().enumerate() {
                        out.push((index + 1) as u8);
                        out.push((index + 1) as u8);
                        match name {
                            None => out.push(0),
                            Some(name) => {
                                out.push((name.len() + 1) as u8);
                                out.extend_from_slice(name.as_bytes());
                                out.push(0);
                            }
                        }
                        push_u32(&mut out, 0x5000 + index as u32);
                        // Literal 6 x 64, not the constants: a fixture built from the constant it
                        // is testing moves with it and the mutation goes invisible.
                        for block in 0..6_usize {
                            out.extend((0..64).map(|byte| (index * 6 + block + byte) as u8 | 0x80));
                        }
                    }
                }
                SectionTag::Alarm => {
                    // The six schedules as literals, not `AlarmQueue::schedule()`. Generating the
                    // fixture from the table under test makes both sides move together, which is
                    // how three mutations survived an earlier sweep of this module.
                    const WORDS: [usize; 6] = [4, 5, 0, 1, 3, 4];
                    const NAMES: [usize; 6] = [1, 1, 1, 2, 1, 1];
                    for queue in 0..6_usize {
                        push_u32(&mut out, self.alarm_queue_records[queue] as u32);
                        for record in 0..self.alarm_queue_records[queue] {
                            let mut words = Vec::new();
                            for word in 0..WORDS[queue] {
                                words.push((queue * 100 + record * 10 + word) as u32 + 900);
                            }
                            // Queue 1's first record is the turn record.
                            if queue == 1 && record == 0 {
                                words[0] = self.turn;
                                words[1] = 15;
                                words[2] = 1;
                                // Deliberately the literal and not `COUNTDOWN_BASE`. Building the
                                // fixture from the constant under test makes the two move together
                                // and a mutation of the constant survives.
                                words[3] = 1_000_001 - self.turn;
                                words[4] = 0;
                            }
                            // Queue 3 interleaves its word between two names, so emitting the
                            // words first would pass a reader that got the order wrong.
                            let mut word_iter = words.iter();
                            let mut names_left = NAMES[queue];
                            if queue == 3 {
                                push_name(&mut out, "explore_brain");
                                push_u32(&mut out, *word_iter.next().expect("queue 3 has a word"));
                                push_name(&mut out, "dpw_brain");
                                names_left = 0;
                            } else {
                                for word in word_iter.by_ref() {
                                    push_u32(&mut out, *word);
                                }
                            }
                            for _ in 0..names_left {
                                if queue == 1 && record == 0 {
                                    push_name(&mut out, "monstergenerator");
                                } else {
                                    push_name(&mut out, "antispy_brain");
                                }
                            }
                            push_u32(&mut out, self.alarm_arguments as u32);
                            for argument in 0..self.alarm_arguments {
                                push_u32(&mut out, 0x7000 + argument as u32);
                            }
                            push_u32(&mut out, 0x1234 + queue as u32);
                        }
                    }
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

        let bytes = fixture.build();
        let structural = save.container.structural_checks(&bytes);
        assert_eq!(structural.len(), 10);
        for check in &structural {
            assert!(check.is_structural());
            assert!(check.passed, "{} failed: {}", check.name, check.measured);
        }
        let regularities = save.regularities();
        assert_eq!(regularities.len(), 13);
        for check in &regularities {
            assert!(!check.is_structural());
        }
    }

    /// A synthetic save may legitimately break a **corpus regularity** without being malformed.
    /// That is the whole reason the two classes are separate: this fixture's regions, its alarm
    /// queue 0 and its `LS_REGN` tail length are all perfectly well-formed and none of them is
    /// what the seven shipped scenarios happen to contain. Calling that malformed would bury a
    /// real discovery under a parse error.
    #[test]
    fn a_broken_regularity_is_not_a_broken_structure() {
        let bytes = Fixture::default().build();
        let save = SaveFile::parse(&bytes).unwrap();

        for check in save.container.structural_checks(&bytes) {
            assert!(check.passed, "{} failed: {}", check.name, check.measured);
        }
        let broken: Vec<&str> = save
            .regularities()
            .into_iter()
            .filter(|check| !check.passed)
            .map(|check| check.name)
            .collect();
        assert_eq!(
            broken,
            vec![
                "LS_REGN: tail length is one of 8998 / 9389 / 9780",
                "LS_ALRM: queue 0 is empty, so the turn lands at payload word 2",
                "LS_REGN: exactly one region stores a name, and it is the empty string",
            ],
            "the fixture is deliberately unlike the corpus in exactly these three ways"
        );
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
        // Grow one existing record rather than adding one: the record count must stay put while
        // the section's extent moves, which is the whole point of "no length word".
        grown.sprite_records[0]
            .class0
            .counted
            .extend(std::iter::repeat_n(0x11_u8, 64));

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
            let slots = save.multiplayer.slots.expect("v111 stores the slot block");
            assert_eq!(save.multiplayer.declared_setup_len, declared);
            assert_eq!(save.multiplayer.setup.len() as u32, declared);
            assert_eq!(
                save.multiplayer.accounted_len(),
                save.container.location(SectionTag::Multiplayer).payload_len
            );
            assert_eq!(slots[2].name_lossy(), "ccc");
        }
    }

    /// Reading the declared length as data means a corrupt one must be refused, not trusted into
    /// an out-of-bounds read.
    /// Below version 99 the reader synthesizes the slot block instead of reading it, so the
    /// payload stops after the setup struct. **Unexercised by any real file** -- this is
    /// implemented from the disassembly and the corpus has only versions 108 and 111.
    ///
    /// The fixture must actually omit the block. A fixture that emitted it anyway would make this
    /// test pass against a parser that ignored the version entirely, which is what the previous
    /// version of this test did.
    #[test]
    fn a_pre_version_99_multiplayer_section_has_no_slot_block() {
        let fixture = Fixture {
            version: 50,
            emit_mult_slots: Some(false),
            ..Fixture::default()
        };
        let bytes = fixture.build();
        let save = SaveFile::parse(&bytes).unwrap();

        assert!(!save.version.stores_multiplayer_slots());
        assert_eq!(save.multiplayer.slots, None);
        assert_eq!(save.multiplayer.setup.len(), 164);
        assert_eq!(save.multiplayer.accounted_len(), 4 + 164);
        assert_eq!(
            save.container.location(SectionTag::Multiplayer).payload_len,
            168
        );
        assert_eq!(save.multiplayer.occupied_slots().count(), 0);
        assert_eq!(save.multiplayer.seated_lord_codes(), None);

        for check in save.container.structural_checks(&bytes) {
            assert!(check.passed, "{} failed: {}", check.name, check.measured);
        }
        // The lord-code cross-check cannot be made, and must report that rather than pass.
        let check = save
            .regularities()
            .into_iter()
            .find(|check| check.name.contains("lord codes"))
            .expect("check is present");
        assert!(!check.passed);
        assert!(check.measured.contains("None"), "{}", check.measured);
    }

    /// A pre-99 save that carries the block anyway must not silently decode it.
    #[test]
    fn a_pre_version_99_payload_carrying_slots_does_not_account() {
        let fixture = Fixture {
            version: 50,
            emit_mult_slots: Some(true),
            ..Fixture::default()
        };
        let bytes = fixture.build();
        let save = SaveFile::parse(&bytes).unwrap();

        assert_eq!(save.multiplayer.slots, None);
        let check = save
            .container
            .structural_checks(&bytes)
            .into_iter()
            .find(|check| check.name.contains("LS_MULT"))
            .expect("check is present");
        assert!(!check.passed);
        assert_eq!(check.measured, "4+164+0=168 vs 744");
    }

    /// The engine's version gates are `jl`/`jge`, which are **signed**. At `0xFFFFFFFF` the engine
    /// sees `-1` and takes the low path; an unsigned comparison answers the opposite.
    #[test]
    fn the_version_gate_is_signed_like_the_engines_jl_jge() {
        let stores = |version: u32, emit: bool| {
            let fixture = Fixture {
                version,
                emit_mult_slots: Some(emit),
                ..Fixture::default()
            };
            SaveFile::parse(&fixture.build())
                .unwrap()
                .version
                .stores_multiplayer_slots()
        };
        // Literal versions either side of the threshold, not the constant under test.
        assert!(!stores(50, false));
        assert!(!stores(98, false));
        assert!(stores(99, true));
        assert!(stores(111, true));
        // The signed cases. `0x80000000` and `0xffffffff` are negative to the engine.
        assert!(!stores(0x8000_0000, false));
        assert!(!stores(u32::MAX, false));
    }

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
        let first_slots = first.multiplayer.slots.expect("v111 stores the slot block");
        let second_slots = second
            .multiplayer
            .slots
            .expect("v111 stores the slot block");

        for slot in 0..16_usize {
            let left = &first_slots[slot];
            let right = &second_slots[slot];
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
        let slots = save.multiplayer.slots.expect("v111 stores the slot block");

        assert!(slots[3].is_occupied());
        assert!(slots[3].name().is_empty());
        assert!(!slots[5].is_occupied());
        assert_eq!(slots[5].name_lossy(), "ghost");
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
    /// The regrouping the Corrected note in `docs/save-format.md` is about: a map whose byte total
    /// accounts **exactly** and whose structure is nonetheless wrong.
    ///
    /// The previous version of this test only lowered the plane count, which breaks the total as
    /// well -- so a mutant implementing `plane_covers_every_cell` as the total-accounting check
    /// passed it, and the test did not check the thing its name claimed. This constructs the real
    /// case: a **96x32** map with a **12,288**-word plane occupies exactly as many bytes as a
    /// 96x64 map with a 6,144-word plane, because `8*3072 + 4*12288 == 8*6144 + 4*6144`. The total
    /// is identical and the plane covers four times the cells.
    #[test]
    fn a_matching_byte_total_does_not_make_the_map_accounting_right() {
        let regrouped = Fixture {
            map_height: 32,
            map_plane_words: Some(12_288),
            ..Fixture::default()
        };
        let bytes = regrouped.build();
        let baseline = Fixture::default().build();

        let map = |source: &[u8]| {
            SaveContainer::locate(source)
                .unwrap()
                .location(SectionTag::Map)
                .payload_len
        };
        assert_eq!(
            map(&bytes),
            map(&baseline),
            "the two shapes must occupy the same bytes, or this proves nothing"
        );

        let save = SaveFile::parse(&bytes).unwrap();

        // The total accounts exactly, and the structural byte check therefore PASSES.
        assert_eq!(save.map.accounted_len(), map(&bytes));
        let total_check = save
            .container
            .structural_checks(&bytes)
            .into_iter()
            .find(|check| check.name.contains("LS_MAP_"))
            .expect("check is present");
        assert!(
            total_check.passed,
            "the byte total must still account: {}",
            total_check.measured
        );

        // And the structure is still wrong. This is the check a sum cannot make.
        assert_eq!(save.map.map.cells.len(), 3072);
        assert_eq!(save.map.plane.len(), 12_288);
        assert!(!save.map.plane_covers_every_cell());
        let coverage = save
            .regularities()
            .into_iter()
            .find(|check| check.name.contains("plane count == cell count"))
            .expect("check is present");
        assert!(!coverage.passed);
        assert_eq!(coverage.measured, "count=12288 cells=3072");
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
            .regularities()
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
            sprite_records: Vec::new(),
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        assert_eq!(save.sprites.record_count, 0);
        assert!(save.sprites.records.is_empty());
        assert!(save.sprites.records_raw().is_empty());
        assert!(save.sprites.candidate_fixed_strides(1024).is_empty());
    }

    /// The stride search must be the arithmetic it claims to be, not a table of corpus results.
    #[test]
    fn the_stride_search_returns_exactly_the_headers_that_divide_evenly() {
        // Seven records of one class, which is the only way to make the stride search find
        // anything at all -- the real section never does.
        let fixture = Fixture {
            sprite_records: (0..7).map(|_| SpriteFixture::bare(8)).collect(),
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

    /// The dispatch table itself, as data: ten entries, ids 0..=9, and the two that raise.
    #[test]
    fn the_class_dispatch_table_has_ten_entries_and_exactly_two_invalid_ids() {
        assert_eq!(SPRITE_CLASS_DISPATCH.len(), 10);
        for (index, entry) in SPRITE_CLASS_DISPATCH.iter().enumerate() {
            assert_eq!(
                entry.class_id, index as u32,
                "entry {index} is out of order"
            );
        }
        let invalid: Vec<u32> = SPRITE_CLASS_DISPATCH
            .iter()
            .filter(|entry| entry.reader.is_none())
            .map(|entry| entry.class_id)
            .collect();
        assert_eq!(invalid, SPRITE_INVALID_CLASS_IDS.to_vec());
        // Ids 5 and 6 share one target, and it is not any real class's arm.
        assert_eq!(
            SPRITE_CLASS_DISPATCH[5].jump_target,
            SPRITE_CLASS_DISPATCH[6].jump_target
        );
        for entry in SPRITE_CLASS_DISPATCH.iter().filter(|e| e.reader.is_some()) {
            assert_ne!(entry.jump_target, SPRITE_CLASS_DISPATCH[5].jump_target);
        }
        assert_eq!(SPRITE_MAX_CLASS_ID, 9);
        // Class 8 is the one class that inherits the base reader unchanged, which is why its
        // record is the base block and nothing else.
        assert_eq!(SPRITE_CLASS_DISPATCH[8].on_disk_reader, Some(0x004F_6B00));
    }

    /// Every record opens with the class id **twice**: once for the container's dispatch and once
    /// because the base reader puts it back at `this+4`.
    #[test]
    fn every_record_echoes_its_class_id_in_the_base_block() {
        let save = SaveFile::parse(&Fixture::default().build()).unwrap();
        assert_eq!(
            save.sprites.records.len(),
            save.sprites.record_count as usize
        );
        for record in &save.sprites.records {
            assert!(
                record.class_id_echo_agrees(),
                "class {} echoed {}",
                record.class_id,
                record.base.class_id_echo
            );
            assert!(record.class_entry().is_some());
        }
    }

    /// **The class-id echo is a one-way detector, and this is its blind spot.**
    ///
    /// Corrupt any byte of a record's class-specific body, leave the two class-id dwords alone,
    /// and the check still reports zero disagreements and the regularity still passes. Pinned as
    /// a test rather than left as a caveat, because the claim this replaced -- that agreement
    /// shows a file is "uncorrupted" -- reads plausibly right up until someone relies on it.
    #[test]
    fn the_class_id_echo_does_not_notice_a_corrupted_record_body() {
        let mut bytes = Fixture::default().build();
        let clean = SaveFile::parse(&bytes).unwrap();
        assert_eq!(clean.sprites.class_id_echo_disagreements(), 0);

        // Record 0 is class 0; its body starts after the count word, the dispatch word and the
        // base block. Literal 32, so this test does not move with the parser's constants.
        let sprites = clean.container.location(SectionTag::Sprites);
        let victim = sprites.payload_offset + 4 + 32;
        bytes[victim] ^= 0xFF;

        let corrupt = SaveFile::parse(&bytes).unwrap();
        assert_ne!(
            corrupt.sprites.records[0].body, clean.sprites.records[0].body,
            "the fixture must actually have been corrupted for this test to mean anything"
        );
        assert_eq!(
            corrupt.sprites.class_id_echo_disagreements(),
            0,
            "agreement is not an integrity claim about the record"
        );
        let check = corrupt
            .regularities()
            .into_iter()
            .find(|check| check.name.contains("class-id echo"))
            .expect("the regularity is present");
        assert!(
            check.passed,
            "a corrupt body still passes: that is the point"
        );
    }

    /// Class 8 is the base reader and nothing more, so its record is exactly the dispatch dword
    /// plus the 24-byte base block. A reader that added any field of its own would fail this.
    #[test]
    fn a_class_eight_record_is_exactly_the_dispatch_word_and_the_base_block() {
        let fixture = Fixture {
            sprite_records: vec![SpriteFixture::bare(8)],
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        let record = &save.sprites.records[0];
        assert!(record.body.is_empty());
        // Literal 28, not `4 + SpriteBase::LEN`: the point is the number.
        assert_eq!(record.encoded_len(), 28);
        assert_eq!(save.sprites.records_raw().len(), 28);
    }

    /// The whole model, checked the way the corpus checks it: the records must account for the
    /// payload **exactly**, and the section must re-emit the same bytes from the decode.
    #[test]
    fn the_records_account_for_the_payload_exactly_and_re_encode_byte_for_byte() {
        let fixture = Fixture::default();
        let save = SaveFile::parse(&fixture.build()).unwrap();

        let mut original = Vec::new();
        push_u32(&mut original, save.sprites.record_count);
        original.extend_from_slice(save.sprites.records_raw());
        assert_eq!(save.sprites.encode(), original);

        let accounted: usize = save
            .sprites
            .records
            .iter()
            .map(SpriteRecord::encoded_len)
            .sum();
        assert_eq!(accounted, save.sprites.records_raw().len());
    }

    /// The whole file reassembles byte-identically with `LS_SPR_` regenerated from its records.
    #[test]
    fn a_file_reassembles_byte_identically_with_the_sprite_section_regenerated() {
        let bytes = Fixture::default().build();
        let save = SaveFile::parse(&bytes).unwrap();
        assert_eq!(save.reencode_with_sprites(&bytes), bytes);
    }

    /// A record of every class the dispatch table admits, in one section, in an order that is not
    /// the class-id order.
    #[test]
    fn decodes_a_section_holding_one_record_of_every_valid_class() {
        let save = SaveFile::parse(&Fixture::default().build()).unwrap();
        let histogram = save.sprites.class_histogram();
        for class_id in [0_u32, 1, 2, 3, 4, 7, 8, 9] {
            assert!(
                histogram.contains_key(&class_id),
                "class {class_id} missing from {histogram:?}"
            );
        }
        for class_id in SPRITE_INVALID_CLASS_IDS {
            assert!(!histogram.contains_key(&class_id));
        }
    }

    /// The two ids the dispatch table routes to the raise are refused, and so is anything past
    /// the reader's own `cmp eax,9` bound.
    #[test]
    fn refuses_the_two_invalid_class_ids_and_anything_past_the_bound() {
        for class_id in [5_u32, 6, 10, 11, u32::MAX] {
            let mut payload = Vec::new();
            push_u32(&mut payload, 1);
            push_u32(&mut payload, class_id);
            push_spr_base(&mut payload, class_id);
            let version = VersionSection { version: 111 };
            let error = SpriteSection::parse(&payload, &version).unwrap_err();
            let text = error.to_string();
            assert!(text.starts_with("LS_SPR_:"), "{text}");
            assert!(text.contains(&class_id.to_string()), "{text}");
        }
    }

    /// A count that claims more records than the payload holds must be an error, not a panic and
    /// not a silent short read.
    #[test]
    fn refuses_a_record_count_the_payload_cannot_supply() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 4);
        for _ in 0..2 {
            push_u32(&mut payload, 8);
            push_spr_base(&mut payload, 8);
        }
        let error = SpriteSection::parse(&payload, &VersionSection { version: 111 }).unwrap_err();
        assert!(error.to_string().starts_with("LS_SPR_:"), "{error}");
    }

    /// Trailing bytes the record count does not reach are a refusal, not slack.
    ///
    /// This is the strongest of the *corpus-side* checks -- a wrong record length leaves a
    /// remainder -- but it is not what makes the whole model falsifiable, and an earlier version
    /// of this comment said it was. It sees **aggregate** extents only: a compensating pair of
    /// errors inside one record leaves no remainder at all. The field boundaries rest on the
    /// disassembly and on the version sweep against an independently written fixture.
    #[test]
    fn refuses_a_payload_with_bytes_left_over_after_the_last_record() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 1);
        push_u32(&mut payload, 8);
        push_spr_base(&mut payload, 8);
        payload.push(0xEE);
        let error = SpriteSection::parse(&payload, &VersionSection { version: 111 }).unwrap_err();
        assert!(error.to_string().contains("leaving 1 over"), "{error}");
    }

    /// A length a record declares for itself is a file-declared allocation size, and it is
    /// bounded by the payload rather than trusted.
    #[test]
    fn refuses_a_class_two_blob_length_that_runs_past_the_payload() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 1);
        push_u32(&mut payload, 2);
        push_spr_base(&mut payload, 2);
        push_u32(&mut payload, 0xffff_fff0);
        let error = SpriteSection::parse(&payload, &VersionSection { version: 111 }).unwrap_err();
        assert!(error.to_string().contains("runs past"), "{error}");
    }

    /// The nested factory at `0x0044B4F0` bounds its type id with `cmp ecx,3 / ja`, and so does
    /// this reader.
    #[test]
    fn refuses_a_nested_type_id_past_the_factorys_bound() {
        let mut payload = Vec::new();
        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_spr_base(&mut payload, 0);
        push_filler(&mut payload, 8);
        push_u32(&mut payload, 1); // one slot
        push_filler(&mut payload, 4);
        let mut blob = vec![0x33_u8; spr_fixture_slot_width(111)];
        blob[0x14..0x18].copy_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&blob);
        push_u32(&mut payload, SPRITE_NESTED_MAX_TYPE_ID + 1);
        let error = SpriteSection::parse(&payload, &VersionSection { version: 111 }).unwrap_err();
        assert!(error.to_string().contains("nested type id 4"), "{error}");
    }

    /// The per-slot blob ladder, swept at every rung **and the version immediately below it**, so
    /// a mutation in either direction is caught.
    #[test]
    fn the_slot_blob_width_ladder_moves_at_exactly_its_five_rungs() {
        // Literals, not `SPR_SLOT_BLOB_WIDTHS`.
        let expected: [(u32, usize); 10] = [
            (0x00, 0x20),
            (0x47, 0x20),
            (0x48, 0x24),
            (0x49, 0x28),
            (0x64, 0x28),
            (0x65, 0x48),
            (0x66, 0x48),
            (0x67, 0x4C),
            (108, 0x4C),
            (111, 0x4C),
        ];
        for (version, width) in expected {
            assert_eq!(
                sprite_slot_blob_width(version),
                width,
                "version {version:#x}"
            );
        }
    }

    /// The blob width is not merely reported by a helper: it changes what the parser consumes.
    /// One slot, one version step, and the record must grow by exactly the ladder's step.
    #[test]
    fn a_slot_record_grows_by_the_ladder_step_when_the_version_crosses_a_rung() {
        let one_slot = |version: u32| {
            let mut class0 = SpriteFixture::bare(0);
            class0.class0 = Class0Fixture {
                slots: vec![SlotFixture {
                    nested: None,
                    items_a: 0,
                    items_b: 0,
                    ..Default::default()
                }],
                counted: Vec::new(),
                ..Default::default()
            };
            let fixture = Fixture {
                version,
                sprite_records: vec![class0],
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            save.sprites.records[0].encoded_len()
        };
        // 0x66 -> 0x67 is the 0x48 -> 0x4C rung: exactly four bytes.
        assert_eq!(one_slot(0x67), one_slot(0x66) + 4);
        // 0x64 -> 0x65 is 0x28 -> 0x48: exactly 32 bytes.
        assert_eq!(one_slot(0x65), one_slot(0x64) + 0x20);
    }

    /// Every version gate in the section, swept at the gate and one below it. Each pair must
    /// differ by the width of exactly the field that gate controls -- a gate moved by one in
    /// either direction changes which pair disagrees.
    #[test]
    fn each_version_gate_changes_the_record_length_by_its_own_fields_width() {
        let measure = |class: u32, version: u32| -> usize {
            let mut record = SpriteFixture::bare(class);
            record.embed_class0 = false;
            record.blob = match class {
                2 => (0..20_u8).collect(),
                3 => vec![0x5A; 0x2BC],
                _ => Vec::new(),
            };
            let fixture = Fixture {
                version,
                sprite_records: vec![record],
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            save.sprites.records[0].encoded_len()
        };

        // (class, gate, how many bytes appear at the gate). Negative means the field is present
        // *below* the gate and disappears at it.
        let cases: [(u32, u32, i64); 13] = [
            (0, 0x33, 4),
            (0, 0x38, 4),
            (0, 0x3D, 24),
            (0, 0x3F, -16),
            (0, 0x42, 2),
            (0, 0x5D, 3),
            (0, 0x6A, 1),
            (1, 0x36, 8),
            (1, 0x4A, 4),
            (1, 0x60, 1),
            (1, 0x66, 2),
            (1, 0x62, -6),
            (2, 0x5A, 4),
        ];
        for (class, gate, delta) in cases {
            let below = measure(class, gate - 1) as i64;
            let at = measure(class, gate) as i64;
            assert_eq!(
                at - below,
                delta,
                "class {class} across gate {gate:#x}: {below} -> {at}"
            );
        }
    }

    /// **Every version gate at once, in both directions.**
    ///
    /// The fixture emitters are a second implementation of these layouts written from the
    /// disassembly with literal gates. So if the parser's gate and the fixture's gate ever
    /// disagree at any version, the record lengths disagree and the section either overruns or
    /// leaves bytes over -- both of which `parse` refuses. Sweeping the whole range around the
    /// ladder is therefore a mutation test of every gate constant in the section, without any
    /// hand-computed field widths to get wrong.
    ///
    /// The range deliberately runs well past the two versions the corpus contains. None of the
    /// gates below 108 has ever met a real file, and a synthetic fixture cannot confirm that the
    /// engine's own writer produced what this reader expects -- it can only confirm the reader is
    /// self-consistent with the instruction stream it was transcribed from.
    #[test]
    fn the_whole_record_set_decodes_and_re_encodes_at_every_version_across_the_ladder() {
        let mut versions: Vec<u32> = (0x30..=0x80).collect();
        versions.extend([0, 1, 50, 108, 111, 200, 9999, u32::MAX]);
        for version in versions {
            let fixture = Fixture {
                version,
                ..Fixture::default()
            };
            let bytes = fixture.build();
            let save = match SaveFile::parse(&bytes) {
                Ok(save) => save,
                Err(error) => panic!("version {version:#x} ({version}): {error}"),
            };
            assert_eq!(
                save.sprites.records.len(),
                save.sprites.record_count as usize,
                "version {version:#x}"
            );
            assert_eq!(
                save.reencode_with_sprites(&bytes),
                bytes,
                "version {version:#x} did not round-trip"
            );
        }
    }

    /// The one gate in the section that no test can move, and the reason it cannot.
    ///
    /// Recorded as an assertion rather than a comment so that a future build constant which
    /// *does* reach it makes this test fail and forces the gate back into the sweep.
    #[test]
    fn the_nested_last_word_gate_is_dead_because_of_the_build_constant() {
        // Literal 0x48, not `SPR_NESTED_LAST_WORD_MIN`'s neighbour: this is the build-constant
        // test at `0x0044B6E9`, a different comparison against a different value.
        assert!(
            (BUILD_FORMAT_VERSION as i32) >= 0x48,
            "the build constant now reaches the gate at 0x0044B6E0; add it to the gate sweep"
        );
    }

    /// **Counts in this section are signed, and a non-positive one skips its loop.**
    ///
    /// Six sites are guarded with `jle` in the engine. Modelling any of them as unsigned turns a
    /// value the engine *skips* into a read of up to two billion records, which this parser
    /// refuses -- and because `LS_SPR_` can now fail, that refusal fails the **whole file** on a
    /// save the game loads. Each case below is a record the engine reads successfully.
    #[test]
    fn a_non_positive_count_skips_its_loop_rather_than_failing_the_file() {
        // **`0x8000_0001` is the one that pins the WIDTH, and an earlier version of this test
        // did not have it.** That version claimed these values caught "a reader that clamped with
        // `as i16` on a `u32` field, or vice versa". They do not: 0x80000000, 0xFFFFFFFF and
        // 0xFFFFFF9C all truncate to a negative `i16` too, so both widths clamp them to zero and
        // agree. Mutating `spr_signed_count` to `(raw as i16)` left the whole suite green.
        //
        // 0x8000_0001 separates them: as `i32` it is negative and clamps to 0, as `i16` it is
        // **+1**. The realistic failure it stands for is the opposite sign -- a slot count of
        // 0x0001_8000, a large but legitimately positive 98,304 that a 16-bit read turns into 0,
        // stopping the cursor short and refusing a whole file the engine loads.
        let negatives: [u32; 4] = [0x8000_0000, 0xFFFF_FFFF, 0xFFFF_FF9C, 0x8000_0001];

        for raw in negatives {
            // Class 0's slot count, 0x004122FC.
            let mut record = SpriteFixture::bare(0);
            record.class0.slots_override = Some(raw);
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build())
                .unwrap_or_else(|error| panic!("slot count {raw:#x}: {error}"));
            assert_eq!(save.sprites.records.len(), 1);

            // The counted byte array's length, 0x00427A5E. The skip covers the read as well as
            // the allocation, so no bytes follow the length word.
            let mut record = SpriteFixture::bare(0);
            record.class0.counted_len_override = Some(raw);
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            SaveFile::parse(&fixture.build())
                .unwrap_or_else(|error| panic!("counted length {raw:#x}: {error}"));

            // Class 3's tail count, 0x00452EBF.
            let mut record = SpriteFixture::bare(3);
            record.blob = vec![0x11; 8];
            record.class3_tail_override = Some(raw);
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            SaveFile::parse(&fixture.build())
                .unwrap_or_else(|error| panic!("class 3 tail {raw:#x}: {error}"));

            // Nested type 3's group count, 0x0044BCC7.
            let mut record = SpriteFixture::bare(0);
            record.class0.slots = vec![SlotFixture {
                nested: Some(NestedFixture {
                    type_id: 3,
                    groups: Vec::new(),
                    groups_override: Some(raw),
                    ..Default::default()
                }),
                items_a: 0,
                items_b: 0,
                ..Default::default()
            }];
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            SaveFile::parse(&fixture.build())
                .unwrap_or_else(|error| panic!("nested group count {raw:#x}: {error}"));
        }

        // Class 1's array count is a signed **word**, 0x0050DC81 / 0x0050DD35.
        for raw in [0x8000_u16, 0xFFFF, 0xFF9C] {
            let mut record = SpriteFixture::bare(1);
            record.class1_entries_override = Some(raw);
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            SaveFile::parse(&fixture.build())
                .unwrap_or_else(|error| panic!("class 1 array count {raw:#x}: {error}"));
        }
    }

    /// The top-level record count, 0x004F717F. A save whose whole `LS_SPR_` payload is
    /// `FF FF FF FF` is one the engine loads and consumes nothing from.
    ///
    /// `0x8000_0001` is here to pin the clamp's **width** through a real parse rather than only
    /// through the helper: as `i32` it is negative and yields no records, as `i16` it is `+1` and
    /// the parser would go looking for a record in an empty payload and refuse the file.
    #[test]
    fn a_negative_top_level_record_count_yields_no_records_rather_than_a_refusal() {
        for raw in [0x8000_0000_u32, 0xFFFF_FFFF, 0x8000_0001] {
            let payload = raw.to_le_bytes().to_vec();
            let section = SpriteSection::parse(&payload, &VersionSection { version: 111 })
                .unwrap_or_else(|error| panic!("record count {raw:#x}: {error}"));
            assert!(section.records.is_empty());
            assert_eq!(section.live_record_count(), 0);
            // The stored word is kept verbatim: it is what the file says, and clamping it in the
            // field would lose the difference between "no records" and "a negative count".
            assert_eq!(section.record_count, raw);
            assert_eq!(section.encode(), payload);
        }
    }

    /// Clamping must not **saturate**: a large positive count is data, not an overflow.
    ///
    /// `.max(0)` is the whole of the transform, and the two helpers differ only in the width they
    /// reinterpret at. A reader that clamped to a maximum as well as a minimum, or that used the
    /// wrong width, changes exactly these.
    #[test]
    fn a_signed_count_preserves_large_positive_values_at_both_widths() {
        // Expected values are the engine's semantics restated -- reinterpret at the field's own
        // width, then take the non-negative part -- not readings taken from the helpers.
        for (raw, expected) in [
            (0x0000_0000_u32, 0_u32),
            (0x0000_0001, 1),
            (0x0000_8000, 0x8000), // positive as i32; NEGATIVE as i16, so the width shows
            (0x0001_8000, 0x0001_8000), // 98,304 -- a 16-bit read makes this 0
            (0x7FFF_FFFF, 0x7FFF_FFFF), // must not saturate to i16::MAX or anything else
            (0x8000_0000, 0),
            (0x8000_0001, 0), // +1 under a 16-bit read
            (0xFFFF_FFFF, 0),
        ] {
            assert_eq!(
                spr_signed_count(raw),
                expected,
                "spr_signed_count({raw:#010x})"
            );
        }
        for (raw, expected) in [
            (0x0000_u16, 0_u16),
            (0x0001, 1),
            (0x7FFF, 0x7FFF), // i16::MAX, preserved rather than saturated
            (0x8000, 0),
            (0xFF9C, 0),
            (0xFFFF, 0),
        ] {
            assert_eq!(
                spr_signed_count16(raw),
                expected,
                "spr_signed_count16({raw:#06x})"
            );
        }
    }

    /// The three list counts inside a slot are **not** signed-guarded: the engine writes
    /// `test / je` and then decrements, so a negative value does not skip, it runs away. This
    /// parser refuses instead, which is a deliberate divergence.
    ///
    /// **All three call sites, not one.** A review found an earlier version of this test covered
    /// only `items_a`: either of the other two could have been switched to the guarded helper and
    /// a record carrying `0xFFFF_FFFF` there would have been accepted as an empty list, with this
    /// test and every structural mutation still green. `tools/mutate_save_constants.py` now
    /// carries one mutation per call site for the same reason.
    ///
    /// Each case is **discriminating**: the record is otherwise complete and well-formed, and
    /// only the one count word is poisoned. If the parser wrongly clamped that count to zero the
    /// way it clamps the six guarded ones, the rest of the record would read straight through and
    /// the file would parse. An earlier version truncated the payload instead, which failed under
    /// both readings and proved nothing.
    #[test]
    fn every_unguarded_list_count_is_refused_rather_than_silently_clamped() {
        /// A call site, and how to build a class-0 record whose only variable is that site's
        /// raw count word.
        type Site = (&'static str, fn(u32) -> SpriteFixture);

        let sites: [Site; 3] = [
            ("items_a, 0x00524FC6", |raw| {
                let mut record = SpriteFixture::bare(0);
                record.class0.slots = vec![SlotFixture {
                    items_a_override: Some(raw),
                    ..Default::default()
                }];
                record
            }),
            ("items_b, 0x0052503B", |raw| {
                let mut record = SpriteFixture::bare(0);
                record.class0.slots = vec![SlotFixture {
                    items_b_override: Some(raw),
                    ..Default::default()
                }];
                record
            }),
            ("nested type 3 group items, 0x0044BD07", |raw| {
                let mut record = SpriteFixture::bare(0);
                record.class0.slots = vec![SlotFixture {
                    nested: Some(NestedFixture {
                        type_id: 3,
                        groups: vec![0, 0],
                        group_items_override: Some(raw),
                        ..Default::default()
                    }),
                    ..Default::default()
                }];
                record
            }),
        ];

        for (label, build) in sites {
            // The control comes first: the identical record with a count `Vec::len()` could have
            // produced must parse, so the refusal below is about the value and not the shape.
            let sane = Fixture {
                sprite_records: vec![build(0)],
                ..Fixture::default()
            };
            SaveFile::parse(&sane.build()).unwrap_or_else(|error| {
                panic!("{label}: the control record does not parse: {error}")
            });

            let poisoned = Fixture {
                sprite_records: vec![build(0xFFFF_FFFF)],
                ..Fixture::default()
            };
            match SaveFile::parse(&poisoned.build()) {
                Err(error) => assert!(
                    error.to_string().starts_with("LS_SPR_:"),
                    "{label}: {error}"
                ),
                Ok(save) => panic!(
                    "{label}: 0xFFFFFFFF was accepted as an empty list -- this call site is \
                     clamped as though it were signed-guarded. Records: {}",
                    save.sprites.records.len()
                ),
            }
        }
    }

    /// Class 1's array length is a `u16`, not a `u32`. A reader that read four bytes there would
    /// consume two bytes too many and land off the end of the record.
    #[test]
    fn class_ones_array_length_is_a_sixteen_bit_count() {
        let entries = |count: usize| {
            let mut record = SpriteFixture::bare(1);
            record.class1_entries = vec![false; count];
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            save.sprites.records[0].encoded_len()
        };
        // Two bytes for the count, then 12 per entry: eight filler bytes and the flag word.
        assert_eq!(entries(1), entries(0) + 12);
        assert_eq!(entries(5), entries(0) + 60);
    }

    /// Three classes embed a whole class-0 record, base block included. The embedded copy is not
    /// a separate top-level record, so the count must not move.
    #[test]
    fn an_embedded_class_zero_lengthens_its_host_without_adding_a_record() {
        let host = |embed: bool| {
            let mut record = SpriteFixture::bare(2);
            record.blob = (0..9_u8).collect();
            record.embed_class0 = embed;
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            assert_eq!(save.sprites.record_count, 1);
            assert_eq!(save.sprites.records.len(), 1);
            save.sprites.records[0].encoded_len()
        };
        assert!(host(true) > host(false) + 28);
    }

    /// A slot's nested object is gated on a word **inside the blob the slot just read**, so the
    /// same fixture with that word cleared is a shorter record.
    #[test]
    fn a_slots_nested_object_is_gated_on_a_word_inside_its_own_blob() {
        let with = |nested: Option<NestedFixture>| {
            let mut record = SpriteFixture::bare(0);
            record.class0 = Class0Fixture {
                slots: vec![SlotFixture {
                    nested,
                    items_a: 0,
                    items_b: 0,
                    ..Default::default()
                }],
                counted: Vec::new(),
                ..Default::default()
            };
            let fixture = Fixture {
                sprite_records: vec![record],
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            save.sprites.records[0].encoded_len()
        };
        let bare = with(None);
        // Nested type 1 is the smallest: the nested base block and nothing else, plus the type id.
        assert_eq!(
            with(Some(NestedFixture {
                type_id: 1,
                groups: Vec::new(),
                ..Default::default()
            })),
            bare + 4 + 12
        );
        assert!(
            with(Some(NestedFixture {
                type_id: 0,
                groups: Vec::new(),
                ..Default::default()
            })) > bare + 4 + 12
        );
    }

    // -- LS_USER ------------------------------------------------------------

    /// The proof-of-concept that panicked the survey: a **valid nine-section save with an empty
    /// `LS_USER`**. Zero is a multiple of 784, so a divisibility test accepts it, `records` comes
    /// back empty, and the first caller to touch `records[0]` dies. The guard is at the parse.
    #[test]
    fn refuses_an_empty_user_payload_rather_than_yielding_zero_records() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let user = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::User);
        bytes.drain(user.payload_offset..user.payload_end());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_USER:"), "{error}");
        assert!(error.to_string().contains("6272"), "{error}");

        // And the container-level check names it too, without needing a successful parse.
        let container = SaveContainer::locate(&bytes).unwrap();
        let check = container
            .structural_checks(&bytes)
            .into_iter()
            .find(|check| check.name.contains("LS_USER"))
            .expect("check is present");
        assert!(!check.passed);
        assert_eq!(check.measured, "0 vs 6272");
    }

    /// Seven records is also not eight. A divisibility test accepts this too.
    #[test]
    fn refuses_a_user_payload_of_seven_records() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let user = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::User);
        bytes.drain(user.payload_offset..user.payload_offset + 784);

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_USER:"), "{error}");
        assert!(error.to_string().contains("5488"), "{error}");
    }

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
            .regularities()
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
            .regularities()
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
    fn player_records_decode_at_every_shape_and_have_no_common_size() {
        let save = SaveFile::parse(&Fixture::default().build()).unwrap();
        assert_eq!(save.players.records.len(), 3);
        assert_eq!(
            save.players
                .records
                .iter()
                .map(|record| record.slot_index)
                .collect::<Vec<u32>>(),
            vec![0, 3, 15]
        );
        assert_eq!(
            save.players
                .records
                .iter()
                .map(|record| record.name_lossy().unwrap_or_default())
                .collect::<Vec<String>>(),
            vec![
                "Merlin".to_owned(),
                "Amazon Princess".to_owned(),
                String::new()
            ]
        );
        let lengths = save.players.record_lengths();
        assert_eq!(lengths.len(), 3);
        assert_eq!(
            lengths.iter().sum::<usize>(),
            save.players.sentinel_offset()
        );
        assert!(
            lengths[0] != lengths[1] && lengths[1] != lengths[2],
            "three records of one size could not fail on a fixed-stride reader: {lengths:?}"
        );
        assert_eq!(save.players.first_slot_index(), Some(0));
        assert_eq!(save.players.lord_codes, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            Some(save.players.lord_codes),
            save.multiplayer.seated_lord_codes()
        );
    }

    /// A section holding no records at all is well-formed: the sentinel is the first word.
    #[test]
    fn a_player_section_with_no_records_is_accepted() {
        let fixture = Fixture {
            player_records: Vec::new(),
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        assert!(save.players.records.is_empty());
        assert_eq!(save.players.sentinel_offset(), 0);
        assert_eq!(save.players.first_slot_index(), None);
    }

    /// The version ladder at `0x004BCBD0`. Below 111 the record is shorter, and the difference is
    /// the reason the version-108 file's records were 12 bytes smaller than the writer emits.
    #[test]
    fn the_record_shrinks_by_exactly_the_fields_each_version_gate_adds() {
        // Every gate and the version **immediately below** it, as literals -- never from
        // `player_record_versions`. A sweep that only samples the gates catches a gate moved up
        // and misses one moved down: 76 -> 75, 104 -> 103 and 110 -> 109 all survived a sweep
        // built that way, because no fixture stood between the old value and the new one.
        let mut lengths = Vec::new();
        for version in [56_u32, 57, 67, 68, 75, 76, 85, 86, 103, 104, 109, 110, 111] {
            let fixture = Fixture {
                version,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            let record = &save.players.records[0];
            assert_eq!(record.interleaved_words.is_some(), version >= 57);
            assert_eq!(record.unknown_3c.is_some(), version >= 68);
            assert_eq!(record.name_raw.is_some(), version >= 76);
            assert_eq!(record.unknown_15d8.is_some(), version >= 86);
            assert_eq!(record.block_68_raw.is_some(), version >= 104);
            assert_eq!(record.unknown_15b4_15b8.is_some(), version >= 110);
            assert_eq!(record.unknown_40.is_some(), version >= 111);
            lengths.push(record.encoded_len());
        }
        // The gaps between consecutive versions in the sweep. A gate contributes its field's width
        // when it is crossed and zero when it is not, so the zeros are as load-bearing as the
        // widths: they are what fails when a gate moves down onto the version below it.
        let gaps: Vec<usize> = lengths.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert_eq!(gaps, vec![60, 0, 4, 0, 31, 0, 4, 0, 3200, 0, 8, 4]);
    }

    /// The 12 bytes that separate version 108 from version 111, which is what the corpus shows.
    #[test]
    fn version_108_records_are_twelve_bytes_shorter_than_version_111() {
        let at = |version| {
            let fixture = Fixture {
                version,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            save.players.record_lengths()
        };
        let old = at(108);
        let new = at(111);
        assert_eq!(old.len(), new.len());
        for (old, new) in old.iter().zip(new.iter()) {
            assert_eq!(new - old, 12);
        }
    }

    #[test]
    fn refuses_a_player_slot_index_outside_the_readers_range() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let player = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Player);
        bytes[player.payload_offset..player.payload_offset + 4]
            .copy_from_slice(&16_u32.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_PLR_:"), "{error}");
        assert!(error.to_string().contains("slot index 16"), "{error}");
    }

    /// Every count inside a record is a length, so a corrupted one must be refused rather than
    /// walked off the end of the payload.
    #[test]
    fn refuses_a_record_whose_army_unit_count_runs_past_the_payload() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let player = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Player);
        // 4 slot index + 12 leading words + 4 queue count, then the queue, then army 0's three
        // words: the unit count sits right after them.
        let queue_entries = 2_usize;
        let at = player.payload_offset + 4 + 12 + 4 + queue_entries * 24 + 12;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_PLR_:"), "{error}");
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

    /// The structural checks must report on a file `SaveFile::parse` **refuses**. That is the case
    /// they exist for, and it is only reachable through the container, so it needs its own test --
    /// a mutant stubbing this check to `true` survived the whole suite without one.
    #[test]
    fn the_structural_checks_report_on_a_file_that_fails_to_parse() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let player = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Player);
        let sentinel = player.payload_end() - PlayerSection::TAIL_LEN;
        bytes[sentinel..sentinel + 4].copy_from_slice(&0x1234_5678_u32.to_le_bytes());

        // The full parse refuses, so nothing downstream of it can report anything.
        assert!(SaveFile::parse(&bytes).is_err());

        // The container-level check still runs, and names the value it found.
        let container = SaveContainer::locate(&bytes).unwrap();
        let checks = container.structural_checks(&bytes);
        let terminator = checks
            .iter()
            .find(|check| check.name.contains("terminator"))
            .expect("check is present");
        assert!(!terminator.passed);
        assert!(
            terminator.measured.contains("0x12345678"),
            "{}",
            terminator.measured
        );
        // And it is the ONLY structural failure: nothing else about the file changed.
        assert_eq!(checks.iter().filter(|check| !check.passed).count(), 1);
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
            .regularities()
            .into_iter()
            .find(|check| check.name.contains("lord codes"))
            .expect("invariant is present");
        assert!(!check.passed);
        assert!(check.measured.contains("43981"), "{}", check.measured);
    }

    // -- LS_REGN ------------------------------------------------------------

    #[test]
    fn the_region_table_accounts_for_the_tail_at_every_shape_tried() {
        let shapes: Vec<Vec<Option<&'static str>>> = vec![
            vec![None],
            vec![None, None],
            vec![Some(""), None, Some("x")],
            vec![None; 23],
            (0..25).map(|_| Some("Lake of Mist")).collect(),
        ];
        for names in shapes {
            let regions = names.len();
            let named: usize = names.iter().filter_map(|n| n.map(|n| n.len() + 1)).sum();
            let fixture = Fixture {
                region_names: names,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            assert_eq!(save.regions.regions.len(), regions);
            assert_eq!(save.regions.array_count as usize, regions - 1);
            assert_eq!(save.regions.cells.len(), 96 * 64);
            assert_eq!(save.regions.grid_len(), 96 * 64 * 6);
            // The arithmetic written out, not `RegionRecord::FIXED_LEN`: 3 + 4 + 6 * 64.
            assert_eq!(save.regions.tail_len(), 4 + regions * 391 + named);
            assert_eq!(save.regions.accounted_tail_len(), save.regions.tail_len());
        }
    }

    /// The three tail lengths the previous pass could only list are `4 + n * 391 + 1`, and the
    /// 391-byte gaps between them are one region each. Written as the arithmetic, so it fails if
    /// the record's fixed size is wrong rather than agreeing with a remembered table.
    #[test]
    fn the_corpus_region_tail_lengths_are_whole_numbers_of_records() {
        for (regions, tail) in [(23_usize, 8998_usize), (24, 9389), (25, 9780)] {
            let mut names: Vec<Option<&'static str>> = vec![None; regions - 1];
            names.push(Some(""));
            let fixture = Fixture {
                region_names: names,
                ..Fixture::default()
            };
            let save = SaveFile::parse(&fixture.build()).unwrap();
            assert_eq!(save.regions.tail_len(), tail);
        }
    }

    #[test]
    fn refuses_a_region_table_whose_last_record_runs_off_the_end() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let region = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Region);
        let count = region.payload_offset + 8 + 96 * 64 * 6;
        let inflated = u32::from_le_bytes(bytes[count..count + 4].try_into().unwrap()) + 1;
        bytes[count..count + 4].copy_from_slice(&inflated.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_REGN:"), "{error}");

        let container = SaveContainer::locate(&bytes).unwrap();
        let check = container
            .structural_checks(&bytes)
            .into_iter()
            .find(|check| check.name.contains("region table"))
            .expect("the structural check is present");
        assert!(!check.passed, "{}", check.measured);
    }

    #[test]
    fn refuses_a_region_table_that_stops_short_of_the_tail_end() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let region = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Region);
        let count = region.payload_offset + 8 + 96 * 64 * 6;
        let deflated = u32::from_le_bytes(bytes[count..count + 4].try_into().unwrap()) - 1;
        bytes[count..count + 4].copy_from_slice(&deflated.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().contains("unaccounted for"), "{error}");
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

    /// The regression test for two generations of the same mistake. An earlier pass put the turn
    /// at payload word 1; a correction moved it to word 2 and kept the frame. There is no header:
    /// this fixture's queue 0 is **occupied**, so the turn is not at word 2 either, and a reader
    /// that indexes the payload fails here whichever index it picks.
    #[test]
    fn the_turn_comes_from_the_queue_record_and_not_from_a_payload_index() {
        let fixture = Fixture {
            turn: 42,
            alarm_queue_records: [1, 2, 3, 1, 2, 4],
            ..Fixture::default()
        };
        let bytes = fixture.build();
        let save = SaveFile::parse(&bytes).unwrap();

        assert_eq!(save.alarms.turn(), Some(42));
        assert_eq!(save.turn_readings(), (42, Some(42), Some(42)));
        assert!(save.turn_agreement());

        let alarm = save.container.location(SectionTag::Alarm);
        let payload = &bytes[alarm.payload_offset..alarm.payload_end()];
        let indexes: Vec<usize> = (0..8)
            .filter(|index| {
                u32::from_le_bytes(payload[index * 4..index * 4 + 4].try_into().unwrap()) == 42
            })
            .collect();
        assert!(
            !indexes.contains(&2),
            "word 2 must not be the turn in this fixture, or the test proves nothing: {indexes:?}"
        );
    }

    /// The corpus's shape: queue 0 empty and queue 1 occupied, which is the *only* reason the turn
    /// lands at payload word 2 there. Building it deliberately shows that the coincidence is a
    /// property of those files and not of the format.
    #[test]
    fn an_empty_queue_zero_is_what_put_the_turn_at_payload_word_two() {
        let fixture = Fixture {
            turn: 42,
            alarm_queue_records: [0, 1, 0, 0, 0, 0],
            ..Fixture::default()
        };
        let bytes = fixture.build();
        let save = SaveFile::parse(&bytes).unwrap();
        let alarm = save.container.location(SectionTag::Alarm);
        let payload = &bytes[alarm.payload_offset..alarm.payload_end()];
        let word2 = u32::from_le_bytes(payload[8..12].try_into().unwrap());
        assert_eq!(word2, 42);
        assert_eq!(save.alarms.turn(), Some(42));
    }

    /// `quickstart` is turn 1 and the value 1 appears at three payload indexes, so it agrees with
    /// several readings at once. This is that shape, and it must be the fixture that *cannot*
    /// distinguish -- proving the discriminating fixtures above do real work.
    #[test]
    fn a_turn_one_fixture_cannot_locate_the_turn_field() {
        let fixture = Fixture {
            turn: 1,
            alarm_queue_records: [0, 1, 0, 0, 0, 0],
            ..Fixture::default()
        };
        let bytes = fixture.build();
        let save = SaveFile::parse(&bytes).unwrap();
        let alarm = save.container.location(SectionTag::Alarm);
        let payload = &bytes[alarm.payload_offset..alarm.payload_end()];
        let matching: Vec<usize> = (0..8)
            .filter(|index| {
                u32::from_le_bytes(payload[index * 4..index * 4 + 4].try_into().unwrap())
                    == save.game.turn
            })
            .collect();
        assert_eq!(
            matching,
            vec![1, 2, 4],
            "a turn-1 save cannot single out the turn word"
        );
    }

    #[test]
    fn the_turn_cross_check_fails_when_the_alarm_turn_disagrees() {
        let fixture = Fixture {
            alarm_queue_records: [0, 1, 0, 0, 0, 0],
            ..Fixture::default()
        };
        let mut bytes = fixture.build();
        let alarm = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Alarm);
        // Queue 0 is empty here, so the turn record's first word is payload word 2.
        let turn_word = alarm.payload_offset + 8;
        bytes[turn_word..turn_word + 4].copy_from_slice(&43_u32.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        assert!(!save.turn_agreement());
        let check = save
            .regularities()
            .into_iter()
            .find(|check| check.name.starts_with("turn:"))
            .expect("invariant is present");
        assert!(!check.passed);
        assert_eq!(check.measured, "42 / 43 / 42");
    }

    #[test]
    fn the_turn_cross_check_fails_when_the_countdown_disagrees() {
        let fixture = Fixture {
            alarm_queue_records: [0, 1, 0, 0, 0, 0],
            ..Fixture::default()
        };
        let mut bytes = fixture.build();
        let alarm = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Alarm);
        let countdown = alarm.payload_offset + 8 + 4 * 3;
        bytes[countdown..countdown + 4].copy_from_slice(&(COUNTDOWN_BASE - 9).to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        assert_eq!(save.turn_readings(), (42, Some(42), Some(9)));
        assert!(!save.turn_agreement());
    }

    /// A countdown word above the base would underflow a naive subtraction.
    #[test]
    fn an_out_of_range_countdown_word_yields_no_turn_rather_than_wrapping() {
        let fixture = Fixture {
            alarm_queue_records: [0, 1, 0, 0, 0, 0],
            ..Fixture::default()
        };
        let mut bytes = fixture.build();
        let alarm = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Alarm);
        let countdown = alarm.payload_offset + 8 + 4 * 3;
        bytes[countdown..countdown + 4].copy_from_slice(&u32::MAX.to_le_bytes());

        let save = SaveFile::parse(&bytes).unwrap();
        assert_eq!(save.alarms.turn_from_countdown(), None);
        assert!(!save.turn_agreement());
        let check = save
            .regularities()
            .into_iter()
            .find(|check| check.name.starts_with("turn:"))
            .expect("invariant is present");
        assert!(check.measured.contains("absent"), "{}", check.measured);
    }

    /// An empty turn queue is well-formed. The reading must go absent rather than invent a turn.
    #[test]
    fn an_empty_turn_queue_yields_no_turn_rather_than_zero() {
        let fixture = Fixture {
            alarm_queue_records: [2, 0, 0, 0, 0, 1],
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        assert_eq!(save.alarms.turn(), None);
        assert_eq!(save.alarms.countdown(), None);
        assert!(!save.turn_agreement());
    }

    #[test]
    fn every_queue_decodes_its_own_field_schedule() {
        let fixture = Fixture {
            alarm_queue_records: [1, 1, 1, 1, 1, 1],
            alarm_arguments: 2,
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        // The schedules written out, not read back from `AlarmQueue::schedule()`.
        let expected_words = [4_usize, 5, 0, 1, 3, 4];
        let expected_names = [1_usize, 1, 1, 2, 1, 1];
        for (index, queue) in save.alarms.queues.iter().enumerate() {
            let record = &queue.records[0];
            assert_eq!(record.words.len(), expected_words[index], "queue {index}");
            assert_eq!(record.names.len(), expected_names[index], "queue {index}");
            assert_eq!(record.arguments.len(), 2, "queue {index}");
            assert_eq!(record.trailer, 0x1234 + index as u32, "queue {index}");
        }
        assert_eq!(save.alarms.record_count(), 6);
    }

    #[test]
    fn a_queue_three_record_keeps_its_two_names_and_the_word_between_them() {
        let fixture = Fixture {
            alarm_queue_records: [0, 1, 0, 1, 0, 0],
            ..Fixture::default()
        };
        let save = SaveFile::parse(&fixture.build()).unwrap();
        let record = &save.alarms.queues[3].records[0];
        assert_eq!(record.names.len(), 2);
        assert_eq!(record.names[0], b"explore_brain");
        assert_eq!(record.names[1], b"dpw_brain");
        assert_eq!(record.words.len(), 1);
    }

    #[test]
    fn refuses_an_alarm_payload_whose_queues_do_not_reach_the_end() {
        let fixture = Fixture::default();
        let mut bytes = fixture.build();
        let alarm = SaveContainer::locate(&bytes)
            .unwrap()
            .location(SectionTag::Alarm);
        // One fewer record in queue 0 leaves that record's bytes unaccounted for.
        bytes[alarm.payload_offset..alarm.payload_offset + 4].copy_from_slice(&0_u32.to_le_bytes());

        let error = SaveFile::parse(&bytes).unwrap_err();
        assert!(error.to_string().starts_with("LS_ALRM:"), "{error}");

        let container = SaveContainer::locate(&bytes).unwrap();
        let check = container
            .structural_checks(&bytes)
            .into_iter()
            .find(|check| check.name.contains("six queues"))
            .expect("the structural check is present");
        assert!(!check.passed, "{}", check.measured);
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
    // -----------------------------------------------------------------------
    // The installed corpus
    // -----------------------------------------------------------------------
    //
    // Run with:
    //   LOM_GAME_DIR=.../English cargo test --release -- --ignored
    //
    // and optionally widened to every install on the machine:
    //   LOM_SAVE_DIRS='/path/one/savegame:/path/two/savegame' ...
    //
    // **This corpus is a live directory, and the test is written for that.** The `.lom` family
    // under GS5R3 was last written by the game on 2026-09-18, after part of `docs/save-format.md`
    // had been drafted; files appear, disappear and are re-saved between runs. So this asserts a
    // **pinned required set** plus a **floor**, never a total. The prose figure of "31 of 31" is a
    // union across four installs and is not reproducible from one directory by construction --
    // `docs/save-format.md` has already had that headline wrong twice, in opposite directions.

    /// **Observed in the corpus, 2026-09-19.** The saves shipped with the game, present in all four
    /// installs on this machine. Pinned by name: these do not move, and requiring them is what
    /// stops a sweep over an empty or wrong directory from passing.
    const SHIPPED_SAVEGAMES: &[&str] = &[
        "combat.sav",
        "experience.sav",
        "magic.sav",
        "merc.sav",
        "quickstart",
        "temple.sav",
    ];

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

    /// Every file in every savegame directory under survey, whatever it is called.
    ///
    /// Extension is not filtered on: two corpus files (`quickstart`, `Merlin I`) have none, and a
    /// sweep that filtered on `.sav` would skip the only mid-game player states in existence here.
    fn savegame_files() -> Vec<(String, Vec<u8>)> {
        let mut directories = vec![game_directory().join("savegame")];
        if let Some(extra) = std::env::var_os("LOM_SAVE_DIRS") {
            directories.extend(
                extra
                    .to_string_lossy()
                    .split(':')
                    .filter(|value| !value.is_empty())
                    .map(std::path::PathBuf::from),
            );
        }
        let mut out = Vec::new();
        for directory in &directories {
            let entries = std::fs::read_dir(directory)
                .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
            for entry in entries {
                let path = entry.expect("a directory entry").path();
                if !path.is_file() {
                    continue;
                }
                let bytes = std::fs::read(&path).expect("read a savegame");
                out.push((path.to_string_lossy().into_owned(), bytes));
            }
        }
        out.sort_by(|left, right| left.0.cmp(&right.0));
        out
    }

    /// Every installed savegame parses, satisfies every structural invariant, accounts for its
    /// bytes with zero slack, and reassembles byte-identically with `LS_SPR_` regenerated.
    ///
    /// **What this proves and what it does not.** The byte account is the real content: a section
    /// model that is merely plausible stops short of its section's end or runs off it, and every
    /// length in the three writer-derived sections is either a constant in the instruction stream
    /// or a count the file stores, so there is nothing to tune. The **re-encode** is weaker than it
    /// looks -- once `parse` succeeds it is an identity, so it establishes lossless preservation
    /// and correct container splicing and nothing about any record's internal field boundaries. It
    /// is asserted because a splicing regression is otherwise invisible, not because it falsifies
    /// the record model. See `docs/save-format.md`.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_savegame_accounts_for_its_bytes() {
        let files = savegame_files();

        // The tripwire, in two parts. The floor alone would pass on six copies of the wrong file;
        // the pinned set alone would pass on a directory that had lost everything else.
        assert!(
            files.len() >= SHIPPED_SAVEGAMES.len(),
            "found {} savegames, fewer than the {} the game ships",
            files.len(),
            SHIPPED_SAVEGAMES.len()
        );
        for shipped in SHIPPED_SAVEGAMES {
            assert!(
                files.iter().any(|(name, _)| std::path::Path::new(name)
                    .file_name()
                    .is_some_and(|base| base.eq_ignore_ascii_case(std::ffi::OsStr::new(shipped)))),
                "the shipped savegame {shipped} is not in the surveyed directories"
            );
        }

        let mut failures = Vec::new();
        for (name, bytes) in &files {
            let save = match SaveFile::parse(bytes) {
                Ok(save) => save,
                Err(error) => {
                    failures.push(format!("{name}: {error}"));
                    continue;
                }
            };

            // Structural invariants are re-derived from the raw bytes on the container, not read
            // off the parsed structs, so they are a second implementation that can genuinely
            // disagree with the first. Corpus regularities are deliberately NOT asserted: a real
            // save that breaks one is a discovery, and failing on it would train a reader to
            // ignore the signal worth acting on.
            for invariant in save.container.structural_checks(bytes) {
                if !invariant.passed {
                    failures.push(format!(
                        "{name}: structural invariant `{}` failed, measured {}",
                        invariant.name, invariant.measured
                    ));
                }
            }

            // The byte account, section by section, for every section that models its own extent.
            let accounted = [
                (SectionTag::Multiplayer, save.multiplayer.accounted_len()),
                (SectionTag::Map, save.map.accounted_len()),
                (SectionTag::Game, save.game.accounted_len()),
            ];
            for (tag, len) in accounted {
                let payload = save.container.location(tag).payload_len;
                if len != payload {
                    failures.push(format!(
                        "{name}: {} accounts for {len} bytes of a {payload}-byte payload, a slack \
                         of {}",
                        tag.name(),
                        payload as i64 - len as i64,
                    ));
                }
            }

            // `LS_SPR_` has no `accounted_len`; its account is that the decoded records re-emit
            // the payload exactly.
            let sprites_payload = save.container.location(SectionTag::Sprites);
            let reemitted = save.sprites.encode();
            let original = &bytes[sprites_payload.payload_offset..sprites_payload.payload_end()];
            if reemitted != original {
                failures.push(format!(
                    "{name}: LS_SPR_ re-emitted {} bytes for a {}-byte payload",
                    reemitted.len(),
                    original.len()
                ));
            }

            if save.reencode_with_sprites(bytes) != *bytes {
                failures.push(format!(
                    "{name}: the whole file did not reassemble byte-identically"
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "{} savegame(s) did not account for their bytes: {:#?}",
            failures.len(),
            failures
        );
    }
}
