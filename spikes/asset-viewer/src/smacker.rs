//! Smacker (`.smk`) container structure.
//!
//! Scope, stated up front because the boundary matters: this module parses the **container** --
//! header, size tables, per-frame flags, tree-section extents, and the chunk layout inside each
//! frame. It does **not** decode video. No Huffman tree is built, no pixel is produced, and
//! nothing here can tell you what a frame looks like.
//!
//! What that still buys is real. It establishes, for every shipped `.smk`, how many frames there
//! are, how long they run, which tracks carry audio and in what format, which frames change the
//! palette, and -- the load-bearing part -- that the declared sizes in the header account for
//! every byte of the file. A container whose arithmetic closes is a container you can cut, splice
//! or re-time; a container you have only sniffed the magic bytes of is not.
//!
//! Field semantics are **Documented** (the Smacker layout is described by the multimedia-format
//! community and implemented in ScummVM and libav) and then **Observed in the corpus** where the
//! installed files agree. Anything the corpus does not settle is carried verbatim in
//! [`SmackerFile::unknown_header_word`] and the per-frame `unknown` flag rather than being given a
//! name this repository has not earned.

use std::fmt;

/// `SMK2` and `SMK4`, the two signatures the format ever had. **Documented.**
pub const SIGNATURES: [&[u8; 4]; 2] = [b"SMK2", b"SMK4"];

/// Number of audio tracks a Smacker header describes. **Documented.**
pub const AUDIO_TRACKS: usize = 7;

/// Bytes of fixed header before the frame-size table. **Documented.**
pub const HEADER_BYTES: usize = 104;

/// `flags` bit 0: the file carries one extra frame beyond `frame_count`, used for seamless
/// looping. Its presence lengthens both size tables by one entry. **Documented.**
pub const FLAG_RING_FRAME: u32 = 0x01;
/// `flags` bit 1: the picture is stored at half height, every other line. **Documented.**
pub const FLAG_Y_INTERLACED: u32 = 0x02;
/// `flags` bit 2: the picture is stored at half height, each line doubled. **Documented.**
pub const FLAG_Y_DOUBLED: u32 = 0x04;

/// `audio_rate` bit 31: the track is present. **Documented.**
pub const AUDIO_PRESENT: u32 = 0x8000_0000;
/// `audio_rate` bit 30: the track uses Smacker's own Huffman audio compression. **Documented.**
pub const AUDIO_COMPRESSED: u32 = 0x4000_0000;
/// `audio_rate` bit 29: 16-bit samples rather than 8-bit. **Documented.**
pub const AUDIO_16_BIT: u32 = 0x2000_0000;
/// `audio_rate` bit 28: stereo rather than mono. **Documented.**
pub const AUDIO_STEREO: u32 = 0x1000_0000;
/// `audio_rate` bit 27: Bink DCT audio rather than Smacker Huffman. **Documented.**
pub const AUDIO_BINK: u32 = 0x0800_0000;
/// `audio_rate` bits 0..=23: the sample rate in Hz. **Documented.**
pub const AUDIO_RATE_MASK: u32 = 0x00ff_ffff;

/// Frame-size table bit 0: the frame is a keyframe. **Documented.**
pub const FRAME_SIZE_KEYFRAME: u32 = 0x01;
/// Frame-size table bit 1: unnamed. Carried, never interpreted. **Documented as unknown.**
pub const FRAME_SIZE_UNKNOWN: u32 = 0x02;

/// Frame-type byte bit 0: the frame begins with a palette chunk. **Documented.**
pub const FRAME_TYPE_PALETTE: u8 = 0x01;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmackerError(String);

impl SmackerError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SmackerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SmackerError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioTrack {
    /// The raw `audio_rate` word, carried so no bit is lost to this module's reading of it.
    pub raw_rate: u32,
    /// The declared size of one track's unpacked audio buffer, from `audio_size[]`.
    pub unpacked_size: u32,
}

impl AudioTrack {
    pub fn present(&self) -> bool {
        self.raw_rate & AUDIO_PRESENT != 0
    }
    pub fn compressed(&self) -> bool {
        self.raw_rate & AUDIO_COMPRESSED != 0
    }
    pub fn bits_per_sample(&self) -> u16 {
        if self.raw_rate & AUDIO_16_BIT != 0 { 16 } else { 8 }
    }
    pub fn channels(&self) -> u16 {
        if self.raw_rate & AUDIO_STEREO != 0 { 2 } else { 1 }
    }
    pub fn bink_audio(&self) -> bool {
        self.raw_rate & AUDIO_BINK != 0
    }
    pub fn sample_rate(&self) -> u32 {
        self.raw_rate & AUDIO_RATE_MASK
    }
    /// Bits of the `audio_rate` word this module does not account for.
    ///
    /// Reported rather than masked away: a nonzero value here is a field nobody has named, and
    /// naming it would require evidence this repository does not have.
    pub fn unaccounted_bits(&self) -> u32 {
        self.raw_rate
            & !(AUDIO_PRESENT
                | AUDIO_COMPRESSED
                | AUDIO_16_BIT
                | AUDIO_STEREO
                | AUDIO_BINK
                | AUDIO_RATE_MASK)
    }
}

/// One chunk inside a frame's payload, in the order the decoder must read them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameChunk {
    /// A palette update. `bytes` includes the leading length byte.
    Palette { offset: usize, bytes: usize },
    /// One audio track's data for this frame. `bytes` includes the leading 4-byte length.
    ///
    /// `unpacked_bytes` is present only for a compressed track, where the payload itself begins
    /// with the decompressed length.
    Audio {
        track: usize,
        offset: usize,
        bytes: usize,
        unpacked_bytes: Option<u32>,
    },
    /// Everything left in the frame after the palette and audio chunks.
    ///
    /// This is the part no decoder here reads. Its extent is established; its content is not.
    Video { offset: usize, bytes: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub index: usize,
    /// Byte offset of the frame payload within the file.
    pub offset: usize,
    /// Payload length, the frame-size table word with its two flag bits cleared.
    pub size: usize,
    pub keyframe: bool,
    /// Frame-size bit 1, whose meaning is not established.
    pub unknown_size_flag: bool,
    /// The raw frame-type byte.
    pub raw_type: u8,
    pub has_palette: bool,
    /// Which of the seven audio tracks carry data in this frame.
    pub audio_tracks: [bool; AUDIO_TRACKS],
    pub chunks: Vec<FrameChunk>,
    /// True when this is the extra looping frame appended because of [`FLAG_RING_FRAME`].
    pub ring_frame: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmackerFile {
    pub signature: [u8; 4],
    pub width: u32,
    pub height: u32,
    /// `frame_count` as declared. The table lengths are one longer when a ring frame is present.
    pub frame_count: u32,
    /// The raw frame-rate word. See [`Self::frame_interval_us`] for the three cases it encodes.
    pub raw_frame_rate: i32,
    pub flags: u32,
    pub audio: [AudioTrack; AUDIO_TRACKS],
    /// Declared byte size of the Huffman tree section that follows the two frame tables.
    pub trees_size: u32,
    /// Sizes of the four decoded tree tables: the MMAP, MCLR, FULL and TYPE trees.
    ///
    /// **Documented** as the sizes of the *decoded* tables a decoder allocates, not as byte
    /// extents inside `trees_size`. They are therefore carried and reported but never used to cut
    /// the tree section up -- this module does not build the trees, and a split it cannot verify
    /// is a split it must not invent.
    pub mmap_size: u32,
    pub mclr_size: u32,
    pub full_size: u32,
    pub type_size: u32,
    /// Header word at offset 100, whose purpose no source consulted here establishes.
    ///
    /// **Carried verbatim, labelled unknown.** Every installed file has the same value, which is
    /// reported by the sweep; one value across 23 files is not evidence of meaning.
    pub unknown_header_word: u32,
    /// Byte offset of the tree section.
    pub trees_offset: usize,
    /// Byte offset of the first frame payload.
    pub first_frame_offset: usize,
    pub frames: Vec<Frame>,
    /// Bytes after the last frame that no declared size accounts for.
    ///
    /// The structural claim this module makes is that this is empty for every shipped file. It is
    /// a field rather than an assertion so that a file where it is *not* empty is reported instead
    /// of failing to parse.
    pub unaccounted_tail: usize,
}

impl SmackerFile {
    pub fn parse(source: &[u8]) -> Result<Self, SmackerError> {
        if source.len() < HEADER_BYTES {
            return Err(SmackerError::new(format!(
                "file is {} bytes, a Smacker header needs {HEADER_BYTES}",
                source.len()
            )));
        }
        let mut signature = [0_u8; 4];
        signature.copy_from_slice(&source[0..4]);
        if !SIGNATURES.iter().any(|known| **known == signature) {
            return Err(SmackerError::new("not a Smacker file"));
        }

        let width = read_u32(source, 4)?;
        let height = read_u32(source, 8)?;
        let frame_count = read_u32(source, 12)?;
        let raw_frame_rate = read_u32(source, 16)? as i32;
        let flags = read_u32(source, 20)?;

        let mut audio = [AudioTrack {
            raw_rate: 0,
            unpacked_size: 0,
        }; AUDIO_TRACKS];
        for (track, slot) in audio.iter_mut().enumerate() {
            slot.unpacked_size = read_u32(source, 24 + track * 4)?;
            slot.raw_rate = read_u32(source, 72 + track * 4)?;
        }

        let trees_size = read_u32(source, 52)?;
        let mmap_size = read_u32(source, 56)?;
        let mclr_size = read_u32(source, 60)?;
        let full_size = read_u32(source, 64)?;
        let type_size = read_u32(source, 68)?;
        let unknown_header_word = read_u32(source, 100)?;

        let ring = flags & FLAG_RING_FRAME != 0;
        // The table length is the one number in the header that sizes an allocation, so it is
        // validated against the bytes actually present before anything is reserved. A file that
        // declares four billion frames must be rejected here, not felt in the allocator.
        let table_entries = (frame_count as usize)
            .checked_add(usize::from(ring))
            .ok_or_else(|| SmackerError::new("frame count overflow"))?;
        let tables_bytes = table_entries
            .checked_mul(5)
            .ok_or_else(|| SmackerError::new("frame table size overflow"))?;
        let trees_offset = HEADER_BYTES
            .checked_add(tables_bytes)
            .ok_or_else(|| SmackerError::new("frame table extends past the address space"))?;
        if trees_offset > source.len() {
            return Err(SmackerError::new(format!(
                "declared {frame_count} frames need {tables_bytes} table bytes, only {} remain",
                source.len() - HEADER_BYTES
            )));
        }
        let first_frame_offset = trees_offset
            .checked_add(trees_size as usize)
            .ok_or_else(|| SmackerError::new("tree section size overflow"))?;
        if first_frame_offset > source.len() {
            return Err(SmackerError::new(format!(
                "tree section declares {trees_size} bytes, only {} remain",
                source.len() - trees_offset
            )));
        }

        let sizes_at = HEADER_BYTES;
        let types_at = HEADER_BYTES + table_entries * 4;
        let mut frames = Vec::with_capacity(table_entries);
        let mut cursor = first_frame_offset;
        for index in 0..table_entries {
            let word = read_u32(source, sizes_at + index * 4)?;
            let size = (word & !(FRAME_SIZE_KEYFRAME | FRAME_SIZE_UNKNOWN)) as usize;
            let raw_type = source[types_at + index];
            let end = cursor
                .checked_add(size)
                .ok_or_else(|| SmackerError::new("frame extends past the address space"))?;
            if end > source.len() {
                return Err(SmackerError::new(format!(
                    "frame {index} declares {size} bytes at offset {cursor}, only {} remain",
                    source.len() - cursor
                )));
            }
            let mut audio_tracks = [false; AUDIO_TRACKS];
            for (track, flag) in audio_tracks.iter_mut().enumerate() {
                *flag = raw_type & (1 << (track + 1)) != 0;
            }
            let chunks = split_frame(
                source,
                cursor,
                size,
                raw_type & FRAME_TYPE_PALETTE != 0,
                &audio_tracks,
                &audio,
            )?;
            frames.push(Frame {
                index,
                offset: cursor,
                size,
                keyframe: word & FRAME_SIZE_KEYFRAME != 0,
                unknown_size_flag: word & FRAME_SIZE_UNKNOWN != 0,
                raw_type,
                has_palette: raw_type & FRAME_TYPE_PALETTE != 0,
                audio_tracks,
                chunks,
                ring_frame: ring && index + 1 == table_entries,
            });
            cursor = end;
        }

        Ok(Self {
            signature,
            width,
            height,
            frame_count,
            raw_frame_rate,
            flags,
            audio,
            trees_size,
            mmap_size,
            mclr_size,
            full_size,
            type_size,
            unknown_header_word,
            trees_offset,
            first_frame_offset,
            frames,
            unaccounted_tail: source.len() - cursor,
        })
    }

    /// Microseconds per frame, from the three cases the rate word encodes. **Documented.**
    ///
    /// `> 0` is milliseconds, `< 0` is negated hundredths of a millisecond, and `0` means the
    /// default ten frames a second.
    pub fn frame_interval_us(&self) -> i64 {
        match self.raw_frame_rate {
            rate if rate > 0 => i64::from(rate) * 1000,
            0 => 100_000,
            rate => -i64::from(rate) * 10,
        }
    }

    pub fn duration_ms(&self) -> i64 {
        (self.frame_interval_us() * i64::from(self.frame_count)) / 1000
    }

    pub fn has_ring_frame(&self) -> bool {
        self.flags & FLAG_RING_FRAME != 0
    }

    /// Header flag bits this module does not account for. Reported, never masked.
    pub fn unaccounted_flag_bits(&self) -> u32 {
        self.flags & !(FLAG_RING_FRAME | FLAG_Y_INTERLACED | FLAG_Y_DOUBLED)
    }

    /// Bytes of each frame that are neither palette nor audio.
    pub fn video_bytes(&self) -> u64 {
        self.frames
            .iter()
            .flat_map(|frame| frame.chunks.iter())
            .map(|chunk| match chunk {
                FrameChunk::Video { bytes, .. } => *bytes as u64,
                _ => 0,
            })
            .sum()
    }

    pub fn audio_bytes(&self) -> u64 {
        self.frames
            .iter()
            .flat_map(|frame| frame.chunks.iter())
            .map(|chunk| match chunk {
                FrameChunk::Audio { bytes, .. } => *bytes as u64,
                _ => 0,
            })
            .sum()
    }

    /// Total decompressed audio a track declares across every frame.
    ///
    /// This is the **negative control on the frame split**. Nothing in the frame walk checks that
    /// an audio chunk was located correctly -- a mis-placed read that happens to fit would pass
    /// silently. But the decompressed length is stored inside each chunk, and the sum of those
    /// lengths has to come out at the track's byte rate times the running time, a number derived
    /// from the header and never from the chunk positions. If the split were wrong, the two would
    /// not agree.
    pub fn audio_unpacked_bytes(&self, track: usize) -> u64 {
        self.frames
            .iter()
            .flat_map(|frame| frame.chunks.iter())
            .map(|chunk| match chunk {
                FrameChunk::Audio {
                    track: index,
                    unpacked_bytes: Some(bytes),
                    ..
                } if *index == track => u64::from(*bytes),
                _ => 0,
            })
            .sum()
    }

    /// Decompressed audio bytes the header's rate and running time predict for a track.
    pub fn audio_expected_bytes(&self, track: usize) -> u64 {
        let Some(descriptor) = self.audio.get(track) else {
            return 0;
        };
        if !descriptor.present() {
            return 0;
        }
        let bytes_per_second = u64::from(descriptor.sample_rate())
            * u64::from(descriptor.channels())
            * u64::from(descriptor.bits_per_sample() / 8);
        let frames = i64::from(self.frame_count).max(0) as u64;
        (bytes_per_second * frames * self.frame_interval_us().max(0) as u64) / 1_000_000
    }

    pub fn palette_frames(&self) -> usize {
        self.frames.iter().filter(|frame| frame.has_palette).count()
    }

    pub fn keyframes(&self) -> usize {
        self.frames.iter().filter(|frame| frame.keyframe).count()
    }
}

/// Walk one frame payload into its palette, audio and video extents.
///
/// Every step is bounded by the frame's own declared size, which was already bounded by the file
/// length before this is called. A chunk that would run past the frame is an error naming the
/// frame, not a truncated read.
fn split_frame(
    source: &[u8],
    offset: usize,
    size: usize,
    has_palette: bool,
    audio_tracks: &[bool; AUDIO_TRACKS],
    audio: &[AudioTrack; AUDIO_TRACKS],
) -> Result<Vec<FrameChunk>, SmackerError> {
    let end = offset + size;
    let mut chunks = Vec::new();
    let mut cursor = offset;

    if has_palette {
        if cursor >= end {
            return Err(SmackerError::new(
                "frame claims a palette chunk but is empty",
            ));
        }
        // The stored byte is the chunk length in units of four bytes, itself included.
        let bytes = usize::from(source[cursor]) * 4;
        if bytes == 0 || cursor + bytes > end {
            return Err(SmackerError::new(format!(
                "palette chunk of {bytes} bytes does not fit the {size}-byte frame"
            )));
        }
        chunks.push(FrameChunk::Palette {
            offset: cursor,
            bytes,
        });
        cursor += bytes;
    }

    for (track, present) in audio_tracks.iter().enumerate() {
        if !present {
            continue;
        }
        if cursor + 4 > end {
            return Err(SmackerError::new(format!(
                "audio track {track} header does not fit the frame"
            )));
        }
        // The stored length counts itself, so it can never be less than its own four bytes.
        let bytes = read_u32(source, cursor)? as usize;
        if bytes < 4 || cursor + bytes > end {
            return Err(SmackerError::new(format!(
                "audio track {track} declares {bytes} bytes, which does not fit the frame"
            )));
        }
        let unpacked_bytes = if audio[track].compressed() && bytes >= 8 {
            Some(read_u32(source, cursor + 4)?)
        } else {
            None
        };
        chunks.push(FrameChunk::Audio {
            track,
            offset: cursor,
            bytes,
            unpacked_bytes,
        });
        cursor += bytes;
    }

    chunks.push(FrameChunk::Video {
        offset: cursor,
        bytes: end - cursor,
    });
    Ok(chunks)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, SmackerError> {
    bytes
        .get(offset..offset + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
        .ok_or_else(|| SmackerError::new(format!("Smacker read past end at offset {offset}")))
}

/// A one-line summary per file, in the tab-separated shape the other sweeps print.
pub fn describe(name: &str, file: &SmackerFile) -> String {
    let tracks: Vec<String> = file
        .audio
        .iter()
        .enumerate()
        .filter(|(_, track)| track.present())
        .map(|(index, track)| {
            format!(
                "track{index}={}Hz/{}bit/{}ch/{}",
                track.sample_rate(),
                track.bits_per_sample(),
                track.channels(),
                if track.bink_audio() {
                    "bink"
                } else if track.compressed() {
                    "smk-huffman"
                } else {
                    "raw"
                }
            )
        })
        .collect();
    format!(
        "{name}\t{}\t{}x{}\tframes={}\tring={}\tinterval-us={}\tduration-ms={}\tkeyframes={}\t\
         palette-frames={}\ttrees={}\tvideo-bytes={}\taudio-bytes={}\taudio-unpacked={}\t\
         audio-expected={}\ttail={}\t{}",
        String::from_utf8_lossy(&file.signature),
        file.width,
        file.height,
        file.frame_count,
        file.has_ring_frame(),
        file.frame_interval_us(),
        file.duration_ms(),
        file.keyframes(),
        file.palette_frames(),
        file.trees_size,
        file.video_bytes(),
        file.audio_bytes(),
        (0..AUDIO_TRACKS)
            .map(|track| file.audio_unpacked_bytes(track))
            .sum::<u64>(),
        (0..AUDIO_TRACKS)
            .map(|track| file.audio_expected_bytes(track))
            .sum::<u64>(),
        file.unaccounted_tail,
        if tracks.is_empty() {
            "no-audio".to_owned()
        } else {
            tracks.join(";")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a Smacker file by hand so the parser is tested against bytes laid out from the
    /// documented field order, not from this module's own reader.
    struct Builder {
        frames: Vec<(Vec<u8>, bool, u8)>,
        flags: u32,
        trees: Vec<u8>,
        audio_rates: [u32; AUDIO_TRACKS],
        frame_rate: i32,
    }

    impl Builder {
        fn new() -> Self {
            Self {
                frames: Vec::new(),
                flags: 0,
                trees: vec![0xaa; 6],
                audio_rates: [0; AUDIO_TRACKS],
                frame_rate: 100,
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(b"SMK2");
            out.extend_from_slice(&320_u32.to_le_bytes());
            out.extend_from_slice(&200_u32.to_le_bytes());
            let declared = self.frames.len() as u32 - u32::from(self.flags & FLAG_RING_FRAME != 0);
            out.extend_from_slice(&declared.to_le_bytes());
            out.extend_from_slice(&self.frame_rate.to_le_bytes());
            out.extend_from_slice(&self.flags.to_le_bytes());
            for _ in 0..AUDIO_TRACKS {
                out.extend_from_slice(&0_u32.to_le_bytes());
            }
            out.extend_from_slice(&(self.trees.len() as u32).to_le_bytes());
            for _ in 0..4 {
                out.extend_from_slice(&1_u32.to_le_bytes());
            }
            for rate in self.audio_rates {
                out.extend_from_slice(&rate.to_le_bytes());
            }
            out.extend_from_slice(&0_u32.to_le_bytes());
            assert_eq!(out.len(), HEADER_BYTES);
            for (payload, keyframe, _) in &self.frames {
                // The low two bits of a frame-size word are flags, so a frame payload can only
                // ever be a multiple of four bytes. A fixture that ignored that would encode its
                // length into the flag bits and then "prove" the parser wrong.
                assert!(
                    payload.len().is_multiple_of(4),
                    "a frame payload must be 4-byte aligned, got {}",
                    payload.len()
                );
                let word = payload.len() as u32 | u32::from(*keyframe);
                out.extend_from_slice(&word.to_le_bytes());
            }
            for (_, _, kind) in &self.frames {
                out.push(*kind);
            }
            out.extend_from_slice(&self.trees);
            for (payload, _, _) in &self.frames {
                out.extend_from_slice(payload);
            }
            out
        }
    }

    #[test]
    fn a_plain_file_accounts_for_every_byte() {
        let mut builder = Builder::new();
        builder.frames.push((vec![1, 2, 3, 4], true, 0));
        builder.frames.push((vec![5, 6, 7, 8], false, 0));
        let bytes = builder.build();
        let file = SmackerFile::parse(&bytes).expect("parse");
        assert_eq!(file.frame_count, 2);
        assert_eq!(file.unaccounted_tail, 0);
        assert_eq!(file.keyframes(), 1);
        assert_eq!(file.video_bytes(), 8);
        assert_eq!(file.frames[0].offset, HEADER_BYTES + 2 * 4 + 2 + 6);
    }

    #[test]
    fn the_ring_frame_flag_lengthens_both_tables() {
        let mut builder = Builder::new();
        builder.flags = FLAG_RING_FRAME;
        builder.frames.push((vec![1, 2, 3, 4], true, 0));
        builder.frames.push((vec![5, 6, 7, 8], false, 0));
        builder.frames.push((vec![9, 10, 11, 12], false, 0));
        let bytes = builder.build();
        let file = SmackerFile::parse(&bytes).expect("parse");
        assert_eq!(file.frame_count, 2, "the header declares two");
        assert_eq!(file.frames.len(), 3, "the tables hold three");
        assert!(file.frames[2].ring_frame);
        assert!(!file.frames[1].ring_frame);
        assert_eq!(file.unaccounted_tail, 0);
    }

    #[test]
    fn a_palette_chunk_is_measured_from_its_leading_length_byte() {
        let mut builder = Builder::new();
        // Length byte 2 means eight bytes including itself, then four bytes of video.
        builder
            .frames
            .push((vec![2, 0, 0, 0, 0, 0, 0, 0, 9, 9, 9, 9], true, FRAME_TYPE_PALETTE));
        let file = SmackerFile::parse(&builder.build()).expect("parse");
        let frame = &file.frames[0];
        assert!(frame.has_palette);
        assert_eq!(
            frame.chunks[0],
            FrameChunk::Palette {
                offset: frame.offset,
                bytes: 8
            }
        );
        assert_eq!(
            frame.chunks[1],
            FrameChunk::Video {
                offset: frame.offset + 8,
                bytes: 4
            }
        );
    }

    #[test]
    fn an_audio_chunk_length_includes_its_own_header() {
        let mut builder = Builder::new();
        builder.audio_rates[0] = AUDIO_PRESENT | AUDIO_COMPRESSED | 22050;
        // 12-byte audio chunk: 4 length + 4 unpacked size + 4 payload, then 2 bytes of video.
        let mut payload = 12_u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&64_u32.to_le_bytes());
        payload.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        payload.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        builder.frames.push((payload, true, 0b10));
        let file = SmackerFile::parse(&builder.build()).expect("parse");
        let frame = &file.frames[0];
        assert!(frame.audio_tracks[0]);
        assert_eq!(
            frame.chunks[0],
            FrameChunk::Audio {
                track: 0,
                offset: frame.offset,
                bytes: 12,
                unpacked_bytes: Some(64)
            }
        );
        assert_eq!(
            frame.chunks[1],
            FrameChunk::Video {
                offset: frame.offset + 12,
                bytes: 4
            }
        );
        assert_eq!(file.audio_bytes(), 12);
        assert_eq!(file.video_bytes(), 4);
    }

    #[test]
    fn an_uncompressed_track_has_no_unpacked_length() {
        let mut builder = Builder::new();
        builder.audio_rates[0] = AUDIO_PRESENT | 11025;
        let mut payload = 8_u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&[1, 2, 3, 4]);
        builder.frames.push((payload, true, 0b10));
        let file = SmackerFile::parse(&builder.build()).expect("parse");
        assert_eq!(
            file.frames[0].chunks[0],
            FrameChunk::Audio {
                track: 0,
                offset: file.frames[0].offset,
                bytes: 8,
                unpacked_bytes: None
            }
        );
    }

    #[test]
    fn a_frame_table_larger_than_the_file_is_refused_before_anything_is_reserved() {
        let mut bytes = Builder::new().build();
        bytes[12..16].copy_from_slice(&0xffff_fff0_u32.to_le_bytes());
        let error = SmackerFile::parse(&bytes).expect_err("a four-billion-frame table must fail");
        assert!(error.to_string().contains("table bytes"), "{error}");
    }

    /// Frame payload sizes are 4-byte aligned because bits 0 and 1 of the size word are flags.
    ///
    /// **Observed in the corpus:** all 23 installed files close exactly on their own length under
    /// this reading, which would not happen if a payload length could carry a value in those bits.
    #[test]
    fn the_flag_bits_are_masked_out_of_the_frame_size() {
        let mut builder = Builder::new();
        builder.frames.push((vec![1, 2, 3, 4], true, 0));
        let bytes = builder.build();
        let word = u32::from_le_bytes([
            bytes[HEADER_BYTES],
            bytes[HEADER_BYTES + 1],
            bytes[HEADER_BYTES + 2],
            bytes[HEADER_BYTES + 3],
        ]);
        assert_eq!(word, 5, "four bytes of payload plus the keyframe bit");
        let file = SmackerFile::parse(&bytes).expect("parse");
        assert_eq!(file.frames[0].size, 4);
        assert!(file.frames[0].keyframe);
        assert_eq!(file.unaccounted_tail, 0);
    }

    #[test]
    fn a_tree_section_larger_than_the_file_is_refused() {
        let mut builder = Builder::new();
        builder.frames.push((vec![1, 2, 3, 4], true, 0));
        let mut bytes = builder.build();
        bytes[52..56].copy_from_slice(&0x0010_0000_u32.to_le_bytes());
        let error = SmackerFile::parse(&bytes).expect_err("an oversized tree section must fail");
        assert!(error.to_string().contains("tree section"), "{error}");
    }

    #[test]
    fn a_frame_running_past_the_file_is_refused() {
        let mut builder = Builder::new();
        builder.frames.push((vec![1, 2, 3, 4], true, 0));
        let mut bytes = builder.build();
        let table_at = HEADER_BYTES;
        bytes[table_at..table_at + 4].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
        let error = SmackerFile::parse(&bytes).expect_err("an oversized frame must fail");
        assert!(error.to_string().contains("frame 0 declares"), "{error}");
    }

    #[test]
    fn the_three_frame_rate_cases_are_distinguished() {
        let mut builder = Builder::new();
        builder.frames.push((vec![1, 2, 3, 4], true, 0));

        builder.frame_rate = 100;
        assert_eq!(
            SmackerFile::parse(&builder.build())
                .expect("parse")
                .frame_interval_us(),
            100_000
        );
        builder.frame_rate = 0;
        assert_eq!(
            SmackerFile::parse(&builder.build())
                .expect("parse")
                .frame_interval_us(),
            100_000
        );
        builder.frame_rate = -6997;
        assert_eq!(
            SmackerFile::parse(&builder.build())
                .expect("parse")
                .frame_interval_us(),
            69_970
        );
    }

    #[test]
    fn unaccounted_bits_are_reported_not_swallowed() {
        let mut builder = Builder::new();
        builder.flags = FLAG_Y_INTERLACED | 0x8000_0000;
        builder.audio_rates[0] = AUDIO_PRESENT | 0x0400_0000 | 22050;
        builder.frames.push((vec![1, 2, 3, 4], true, 0));
        let file = SmackerFile::parse(&builder.build()).expect("parse");
        assert_eq!(file.unaccounted_flag_bits(), 0x8000_0000);
        assert_eq!(file.audio[0].unaccounted_bits(), 0x0400_0000);
    }

    #[test]
    fn the_unpacked_audio_total_is_summed_from_inside_the_chunks() {
        let mut builder = Builder::new();
        builder.audio_rates[0] = AUDIO_PRESENT | AUDIO_COMPRESSED | AUDIO_STEREO | 22050;
        builder.frame_rate = 100; // 100 ms a frame
        for _ in 0..10 {
            let mut payload = 12_u32.to_le_bytes().to_vec();
            payload.extend_from_slice(&4410_u32.to_le_bytes());
            payload.extend_from_slice(&[0, 0, 0, 0]);
            builder.frames.push((payload, false, 0b10));
        }
        let file = SmackerFile::parse(&builder.build()).expect("parse");
        assert_eq!(file.audio_unpacked_bytes(0), 44100);
        // 22050 Hz x 2 channels x 1 byte x 1.0 s of running time.
        assert_eq!(file.audio_expected_bytes(0), 44100);
    }

    #[test]
    fn a_trailing_byte_no_size_accounts_for_is_reported() {
        let mut builder = Builder::new();
        builder.frames.push((vec![1, 2, 3, 4], true, 0));
        let mut bytes = builder.build();
        bytes.push(0x5a);
        let file = SmackerFile::parse(&bytes).expect("parse");
        assert_eq!(file.unaccounted_tail, 1);
    }
    // --- The installed corpus ---------------------------------------------------------------
    //
    // Run with:
    //   LOM_GAME_DIR=.../English cargo test --release -- --ignored
    fn smk_directory() -> std::path::PathBuf {
        let directory = std::env::var_os("LOM_GAME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set LOM_GAME_DIR to the installed English directory");
        assert!(
            directory.join("lomse.exe").is_file(),
            "no lomse.exe under {}",
            directory.display()
        );
        directory.join("smk")
    }

    fn installed_files() -> Vec<(String, Vec<u8>)> {
        let root = smk_directory();
        let mut out = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).expect("read the smk directory") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("smk"))
                {
                    let bytes = std::fs::read(&path).expect("read a smk file");
                    out.push((
                        path.strip_prefix(&root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .into_owned(),
                        bytes,
                    ));
                }
            }
        }
        out.sort_by(|left, right| left.0.cmp(&right.0));
        out
    }

    /// Every shipped `.smk` parses and its declared sizes account for every byte.
    ///
    /// The tail is the whole claim. A header whose frame table, tree section and frame sizes sum
    /// to exactly the file length is a header that has been read correctly; one that leaves bytes
    /// over has not, whatever else parsed.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn every_installed_smacker_accounts_for_every_byte() {
        let files = installed_files();
        assert!(!files.is_empty(), "no .smk files under English/smk");
        for (name, bytes) in &files {
            let file = SmackerFile::parse(bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(file.unaccounted_tail, 0, "{name} left bytes unaccounted for");
            assert_eq!(
                file.first_frame_offset + file.frames.iter().map(|frame| frame.size).sum::<usize>(),
                bytes.len(),
                "{name}: the frame sizes do not close on the file length"
            );
            assert_eq!(file.unaccounted_flag_bits(), 0, "{name} sets an unnamed flag");
        }
    }

    /// The control on the frame split: audio summed out of the chunks matches the header's rate.
    ///
    /// The two numbers share no input. One comes from four-byte lengths found by walking each
    /// frame's palette and audio chunks; the other from `frame_count`, the rate word, and the
    /// track descriptor. A mis-located audio chunk would read a length out of video data, and the
    /// totals would not land within a frame of each other.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn the_audio_chunk_walk_agrees_with_the_header_rate() {
        for (name, bytes) in &installed_files() {
            let file = SmackerFile::parse(bytes).expect("parse");
            for track in 0..AUDIO_TRACKS {
                if !file.audio[track].present() {
                    assert_eq!(file.audio_unpacked_bytes(track), 0, "{name} track {track}");
                    continue;
                }
                let found = file.audio_unpacked_bytes(track) as i64;
                let expected = file.audio_expected_bytes(track) as i64;
                let descriptor = &file.audio[track];
                let bytes_per_frame = (i64::from(descriptor.sample_rate())
                    * i64::from(descriptor.channels())
                    * i64::from(descriptor.bits_per_sample() / 8)
                    * file.frame_interval_us())
                    / 1_000_000;
                assert!(
                    (found - expected).abs() < bytes_per_frame,
                    "{name} track {track}: {found} bytes of audio in the chunks, {expected} \
                     predicted by the header, more than one {bytes_per_frame}-byte frame apart"
                );
            }
        }
    }

    /// Nothing in the corpus exercises the keyframe bit or the unnamed frame-size bit.
    ///
    /// Recorded as a test so the claim in `docs/audio-format.md` cannot quietly go stale: if a
    /// future install does set either bit, this fails and the documentation gets corrected rather
    /// than continuing to say the corpus never sets them.
    #[test]
    #[ignore = "needs LOM_GAME_DIR"]
    fn the_corpus_sets_neither_frame_size_flag() {
        for (name, bytes) in &installed_files() {
            let file = SmackerFile::parse(bytes).expect("parse");
            assert_eq!(file.keyframes(), 0, "{name} sets the keyframe bit");
            assert!(
                file.frames.iter().all(|frame| !frame.unknown_size_flag),
                "{name} sets the unnamed frame-size bit"
            );
            assert!(!file.has_ring_frame(), "{name} carries a ring frame");
        }
    }
}
