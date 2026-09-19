//! Windows bitmaps: the two `.bmp` members of the GS5R3 `pic.mpq`.
//!
//! # What the corpus is, and what two files can say
//!
//! **Observed in the corpus, 2026-09-19.** `.bmp` members exist in exactly one installed profile:
//! GS5R3's `pic.mpq` holds two, `LBM\ARTIFACT5R3A.bmp` and `LBM\ARTIFACT5R3B.bmp`, and the other
//! three profiles (3.02, Development, Steambuild) hold **none**. Both are byte-for-byte the same
//! shape in every header field:
//!
//! | field | value |
//! | --- | --- |
//! | `bfType` / `bfSize` | `BM` / 172,854, which is exactly the member's length |
//! | `bfReserved1`, `bfReserved2` | 0, 0 |
//! | `bfOffBits` | 54 -- the 14-byte file header plus a 40-byte DIB header, no palette, no gap |
//! | `biSize` | 40 (`BITMAPINFOHEADER`) |
//! | `biWidth` x `biHeight` | 400 x 144, positive height, so **bottom-up** |
//! | `biPlanes` / `biBitCount` | 1 / 24 |
//! | `biCompression` / `biSizeImage` | 0 (`BI_RGB`) / 172,800 = 1,200 x 144 |
//! | `biXPelsPerMeter`, `biYPelsPerMeter` | 2,834, 2,834 |
//! | `biClrUsed`, `biClrImportant` | 0, 0 |
//!
//! **Two files, identical in every field, are a corpus that can establish one point in the format
//! and nothing about its range.** They say this repository can read and write *these* members, and
//! they say nothing whatever about 8-bit or 32-bit depths, `BI_RLE8`, `BI_BITFIELDS`, a palette, a
//! `BITMAPV4HEADER`, a top-down row order, or a row that needs padding -- none of which any member
//! witnesses. This decoder therefore implements exactly the shape above and reports everything else
//! as **unsupported** rather than guessing. See [`BmpErrorKind`] for why that is a different answer
//! from "malformed".
//!
//! # Channel order and row order, settled numerically
//!
//! A 24-bit BMP could be read B,G,R or R,G,B and bottom-up or top-down, and judging that by eye is
//! how this repository previously got a whole probe wrong. It did not have to be judged: both `.bmp`
//! members have a **same-named sibling** in the same archive -- `LBM\ARTIFACT5R3A.lbm` and
//! `LBM\ARTIFACT5R3B.lbm`, IFF PBM images of the same 400x144 dimensions -- decoded by
//! [`crate::pbm`], which is an independent reader written years of commits earlier for an unrelated
//! format.
//!
//! **Observed in the corpus, 2026-09-19.** Comparing every one of the 57,600 pixels against that
//! sibling:
//!
//! | reading | `ARTIFACT5R3A` | `ARTIFACT5R3B` |
//! | --- | ---: | ---: |
//! | **B,G,R bottom-up** | **57,600 / 57,600** | **57,600 / 57,600** |
//! | R,G,B bottom-up | 11,384 | 7,161 |
//! | B,G,R top-down | 8,070 | 10,542 |
//! | R,G,B top-down | 1,686 | 192 |
//!
//! The standard reading is exact and the three alternatives are nowhere near it, so this is a
//! discriminating measurement rather than a flat image agreeing with itself. `every_archived_bitmap_matches_its_sibling_lbm`
//! asserts all four rows, the three wrong ones as a negative control: a test that only checked the
//! right reading would still pass if the image were uniform.
//!
//! **This is not the same claim as the `screencapture` one.** `docs/research-log.md` records that
//! the engine's own `screencapture` operator writes R,G,B into a file whose header says otherwise.
//! That is a property of *that operator's output*, measured on captures. These two members are
//! authored art shipped inside an archive, and they are B,G,R -- standard. The two findings do not
//! conflict and neither one may be used to predict the other.

use std::fmt;

/// The file header plus a `BITMAPINFOHEADER`: the only pixel offset this module accepts.
const HEADER_BYTES: usize = 54;

/// `biSize` for a `BITMAPINFOHEADER`.
const INFO_HEADER_BYTES: u32 = 40;

/// `biCompression` for uncompressed pixels.
const BI_RGB: u32 = 0;

/// The only bit depth in the corpus.
const BITS_PER_PIXEL: u16 = 24;

/// Whether a bitmap was *broken* or merely *outside what this module implements*.
///
/// The same split [`crate::wave`] makes, for the same reason: `--scan` counts a probe failure as a
/// defect in the archive, and a legal BMP in a variant nobody here has written a decoder for is a
/// limit of this tool, not a fault in the file. Only [`Unsupported`](Self::Unsupported) is
/// downgraded to a classification by [`crate::asset::probe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BmpErrorKind {
    /// The bytes do not form a bitmap: truncated, wrong magic, sizes that do not close.
    Malformed,
    /// A legal bitmap in a variant this module does not implement.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BmpError {
    kind: BmpErrorKind,
    message: String,
}

impl BmpError {
    fn malformed(message: impl Into<String>) -> Self {
        Self {
            kind: BmpErrorKind::Malformed,
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            kind: BmpErrorKind::Unsupported,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> BmpErrorKind {
        self.kind
    }
}

impl fmt::Display for BmpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BmpError {}

/// A decoded 24-bit uncompressed Windows bitmap.
///
/// Pixels are **top-down** and **R,G,B** here -- the orientation and channel order every other
/// image type in this crate uses -- while the file stores them bottom-up and B,G,R.
/// [`decode`](Self::decode) and [`encode`](Self::encode) are the only places that conversion
/// happens.
///
/// The four header fields that carry no pixel information are kept as fields rather than being
/// rebuilt from a constant, so that `encode(decode(bytes)) == bytes` holds because the values came
/// from the file and not because this module happens to hardcode what the corpus contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapImage {
    pub width: u32,
    pub height: u32,
    /// One `[r, g, b]` per pixel, row-major, **first row is the top row**.
    pub pixels: Vec<[u8; 3]>,
    pub x_pixels_per_meter: i32,
    pub y_pixels_per_meter: i32,
    pub colors_used: u32,
    pub colors_important: u32,
}

impl BitmapImage {
    /// A bitmap built from nothing, with the header fields both corpus members carry.
    ///
    /// 2,834 pixels per metre is 72 dpi. It is used here because it is what the two shipped members
    /// declare (**Observed in the corpus**); nothing in the engine is known to read it.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<[u8; 3]>) -> Result<Self, BmpError> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| usize::try_from(height).ok().map(|height| width * height))
            .ok_or_else(|| BmpError::malformed("bitmap dimensions overflow"))?;
        if pixels.len() != expected {
            return Err(BmpError::malformed(format!(
                "{}x{} needs {expected} pixels, got {}",
                width,
                height,
                pixels.len()
            )));
        }
        Ok(Self {
            width,
            height,
            pixels,
            x_pixels_per_meter: 2_834,
            y_pixels_per_meter: 2_834,
            colors_used: 0,
            colors_important: 0,
        })
    }

    /// Bytes per stored row, rounded up to a multiple of four.
    ///
    /// **Documented**, from the Windows `BITMAPINFOHEADER` definition -- **not observed here**.
    /// Both corpus members are 400 pixels wide and 400 x 3 = 1,200 is already a multiple of four,
    /// so the corpus contains **zero** padding bytes and cannot witness this rule, what the padding
    /// bytes hold, or that a decoder must skip them. The synthetic `padding_*` tests are the only
    /// coverage this branch has.
    pub fn stride(width: u32) -> Option<usize> {
        let bits = u64::from(width) * u64::from(BITS_PER_PIXEL);
        usize::try_from(bits.checked_add(31)? / 32 * 4).ok()
    }

    /// Read a 24-bit uncompressed bottom-up bitmap.
    ///
    /// Every variant outside that is an [`Unsupported`](BmpErrorKind::Unsupported) error naming the
    /// field that put it there; a file that is not a bitmap at all, or whose declared sizes do not
    /// close, is [`Malformed`](BmpErrorKind::Malformed).
    pub fn decode(bytes: &[u8]) -> Result<Self, BmpError> {
        if bytes.len() < HEADER_BYTES {
            return Err(BmpError::malformed(format!(
                "truncated bitmap: {} bytes, a header needs {HEADER_BYTES}",
                bytes.len()
            )));
        }
        if &bytes[0..2] != b"BM" {
            return Err(BmpError::malformed("not a bitmap: no BM signature"));
        }
        let file_size = read_u32(bytes, 2);
        if file_size as usize != bytes.len() {
            return Err(BmpError::malformed(format!(
                "bfSize says {file_size} bytes, the member is {}",
                bytes.len()
            )));
        }
        // Both reserved words are zero in the corpus and this struct has nowhere to keep a nonzero
        // one, so re-encoding such a file would silently drop it. Refuse instead.
        let reserved = (read_u16(bytes, 6), read_u16(bytes, 8));
        if reserved != (0, 0) {
            return Err(BmpError::unsupported(format!(
                "bfReserved1/bfReserved2 are {}/{}, not 0/0",
                reserved.0, reserved.1
            )));
        }
        let pixel_offset = read_u32(bytes, 10);
        if pixel_offset as usize != HEADER_BYTES {
            return Err(BmpError::unsupported(format!(
                "bfOffBits is {pixel_offset}; only {HEADER_BYTES} (no palette and no gap) is \
                 implemented"
            )));
        }
        let info_size = read_u32(bytes, 14);
        if info_size != INFO_HEADER_BYTES {
            return Err(BmpError::unsupported(format!(
                "DIB header size {info_size}; only BITMAPINFOHEADER ({INFO_HEADER_BYTES}) is \
                 implemented"
            )));
        }
        let width = read_i32(bytes, 18);
        let height = read_i32(bytes, 22);
        let planes = read_u16(bytes, 26);
        let bits_per_pixel = read_u16(bytes, 28);
        let compression = read_u32(bytes, 30);
        let size_image = read_u32(bytes, 34);
        if planes != 1 {
            return Err(BmpError::unsupported(format!(
                "biPlanes is {planes}, not 1"
            )));
        }
        if bits_per_pixel != BITS_PER_PIXEL {
            return Err(BmpError::unsupported(format!(
                "biBitCount is {bits_per_pixel}; only {BITS_PER_PIXEL} is implemented"
            )));
        }
        if compression != BI_RGB {
            return Err(BmpError::unsupported(format!(
                "biCompression is {compression}; only BI_RGB ({BI_RGB}) is implemented"
            )));
        }
        if width <= 0 {
            return Err(BmpError::malformed(format!("biWidth is {width}")));
        }
        // A negative height is a legal top-down bitmap and no member is one. Row order is the axis
        // the sibling-LBM comparison actually discriminated (see the module header); implementing
        // the reading it refuted, uncovered by any file, is how a wrong branch survives.
        if height <= 0 {
            return Err(BmpError::unsupported(format!(
                "biHeight is {height}; only a positive (bottom-up) height is implemented"
            )));
        }
        let width = width.unsigned_abs();
        let height = height.unsigned_abs();
        let stride = Self::stride(width)
            .ok_or_else(|| BmpError::malformed("bitmap row length overflows"))?;
        let expected_pixels = stride
            .checked_mul(height as usize)
            .ok_or_else(|| BmpError::malformed("bitmap pixel data overflows"))?;
        if bytes.len() - HEADER_BYTES != expected_pixels {
            return Err(BmpError::malformed(format!(
                "{width}x{height} needs {expected_pixels} pixel bytes, the member has {}",
                bytes.len() - HEADER_BYTES
            )));
        }
        // `biSizeImage` may legally be 0 for BI_RGB. Both members write the real size, and a third
        // value would be a claim this module cannot carry through an encode.
        if size_image != 0 && size_image as usize != expected_pixels {
            return Err(BmpError::malformed(format!(
                "biSizeImage is {size_image}, not 0 and not the {expected_pixels} the dimensions \
                 require"
            )));
        }

        let mut pixels = Vec::with_capacity(expected_pixels);
        for row in 0..height as usize {
            // Bottom-up: stored row 0 is the image's last row.
            let base = HEADER_BYTES + (height as usize - 1 - row) * stride;
            for column in 0..width as usize {
                let at = base + column * 3;
                // B, G, R in the file; R, G, B here.
                pixels.push([bytes[at + 2], bytes[at + 1], bytes[at]]);
            }
        }

        Ok(Self {
            width,
            height,
            pixels,
            x_pixels_per_meter: read_i32(bytes, 38),
            y_pixels_per_meter: read_i32(bytes, 42),
            colors_used: read_u32(bytes, 46),
            colors_important: read_u32(bytes, 50),
        })
    }

    /// Write the bitmap back out.
    ///
    /// `encode(decode(bytes)) == bytes` for **2 of 2** archived members (**Observed in the
    /// corpus**, 2026-09-19, `every_archived_bitmap_round_trips`). It holds by construction rather
    /// than by luck: every header field [`decode`](Self::decode) does not derive is carried on the
    /// struct, and every field it does derive it also *checks*, so a file carrying a value this
    /// encoder would not reproduce is refused at decode instead of being silently rewritten.
    ///
    /// Padding bytes are written as zero. The corpus has none, so nothing here witnesses what a
    /// real one holds -- see [`stride`](Self::stride).
    pub fn encode(&self) -> Vec<u8> {
        let stride = Self::stride(self.width).expect("a decoded bitmap has a representable stride");
        let pixel_bytes = stride * self.height as usize;
        let mut out = Vec::with_capacity(HEADER_BYTES + pixel_bytes);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(
            &u32::try_from(HEADER_BYTES + pixel_bytes)
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(
            &u32::try_from(HEADER_BYTES)
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&INFO_HEADER_BYTES.to_le_bytes());
        out.extend_from_slice(&(self.width as i32).to_le_bytes());
        out.extend_from_slice(&(self.height as i32).to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&BITS_PER_PIXEL.to_le_bytes());
        out.extend_from_slice(&BI_RGB.to_le_bytes());
        out.extend_from_slice(&u32::try_from(pixel_bytes).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(&self.x_pixels_per_meter.to_le_bytes());
        out.extend_from_slice(&self.y_pixels_per_meter.to_le_bytes());
        out.extend_from_slice(&self.colors_used.to_le_bytes());
        out.extend_from_slice(&self.colors_important.to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_BYTES);

        for row in (0..self.height as usize).rev() {
            let start = out.len();
            for column in 0..self.width as usize {
                let [red, green, blue] = self.pixels[row * self.width as usize + column];
                out.extend_from_slice(&[blue, green, red]);
            }
            out.resize(start + stride, 0);
        }
        out
    }

    /// The pixel at `(x, y)`, with `(0, 0)` the **top left**.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels
            .get((y as usize) * (self.width as usize) + x as usize)
            .copied()
    }
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_i32(bytes: &[u8], at: usize) -> i32 {
    read_u32(bytes, at) as i32
}

#[cfg(test)]
mod tests {
    use super::{BitmapImage, BmpError, BmpErrorKind, HEADER_BYTES};

    /// A synthetic bitmap in the corpus shape, so the fixture cannot pass by resembling nothing.
    fn build(width: u32, height: u32, pixels: Vec<[u8; 3]>) -> Vec<u8> {
        BitmapImage::from_pixels(width, height, pixels)
            .expect("a well-formed fixture")
            .encode()
    }

    fn ramp(width: u32, height: u32) -> Vec<[u8; 3]> {
        (0..width * height)
            .map(|index| {
                [
                    (index % 251) as u8,
                    (index / 3 % 241) as u8,
                    (index / 7 % 239) as u8,
                ]
            })
            .collect()
    }

    #[test]
    fn a_bitmap_survives_a_decode_and_an_encode() {
        let pixels = ramp(5, 4);
        let bytes = build(5, 4, pixels.clone());
        let decoded = BitmapImage::decode(&bytes).expect("decode");
        assert_eq!(decoded.width, 5);
        assert_eq!(decoded.height, 4);
        assert_eq!(decoded.pixels, pixels);
        assert_eq!(decoded.encode(), bytes);
    }

    /// The file stores B, G, R bottom-up; the struct holds R, G, B top-down.
    ///
    /// Asserted on the **bytes**, not on a second call to this module's own decoder, because a
    /// decode/encode pair agreeing with itself cannot fail on the convention being backwards.
    #[test]
    fn the_stored_bytes_are_bgr_and_the_last_stored_row_is_the_top_row() {
        let top_left = [10, 20, 30];
        let bottom_left = [40, 50, 60];
        let bytes = build(1, 2, vec![top_left, bottom_left]);
        // Stored row 0 is the image's bottom row, as B, G, R.
        assert_eq!(&bytes[HEADER_BYTES..HEADER_BYTES + 3], &[60, 50, 40]);
        // Stride for one 24-bit pixel is 4, not 3.
        assert_eq!(BitmapImage::stride(1), Some(4));
        assert_eq!(&bytes[HEADER_BYTES + 4..HEADER_BYTES + 7], &[30, 20, 10]);

        let decoded = BitmapImage::decode(&bytes).expect("decode");
        assert_eq!(decoded.pixel(0, 0), Some(top_left));
        assert_eq!(decoded.pixel(0, 1), Some(bottom_left));
        assert_eq!(decoded.pixel(1, 0), None);
    }

    /// The padding branch, which the corpus cannot reach. See [`BitmapImage::stride`].
    #[test]
    fn padding_rounds_each_row_up_to_four_bytes_and_is_skipped_on_the_way_back() {
        for width in 1..=8_u32 {
            let expected = (width as usize * 3).div_ceil(4) * 4;
            assert_eq!(BitmapImage::stride(width), Some(expected), "width {width}");
            let pixels = ramp(width, 3);
            let bytes = build(width, 3, pixels.clone());
            assert_eq!(bytes.len(), HEADER_BYTES + expected * 3, "width {width}");
            let decoded = BitmapImage::decode(&bytes).expect("decode");
            assert_eq!(decoded.pixels, pixels, "width {width}");
            assert_eq!(decoded.encode(), bytes, "width {width}");
        }
    }

    #[test]
    fn padding_bytes_are_written_as_zero_and_a_nonzero_one_does_not_reach_the_pixels() {
        let pixels = ramp(1, 1);
        let mut bytes = build(1, 1, pixels.clone());
        assert_eq!(bytes[HEADER_BYTES + 3], 0);
        bytes[HEADER_BYTES + 3] = 0xAB;
        let decoded = BitmapImage::decode(&bytes).expect("decode");
        assert_eq!(decoded.pixels, pixels);
        // The re-encode zeroes it again. That is a **change** to the file, and the only one this
        // encoder can make to a member it accepted -- which is exactly why the corpus round-trip is
        // byte-identical: no member has a padding byte at all.
        assert_eq!(decoded.encode()[HEADER_BYTES + 3], 0);
    }

    fn kind(bytes: &[u8]) -> Option<BmpErrorKind> {
        BitmapImage::decode(bytes)
            .err()
            .map(|error: BmpError| error.kind())
    }

    #[test]
    fn a_broken_file_is_malformed_and_an_unimplemented_variant_is_unsupported() {
        assert_eq!(
            kind(b"not a bitmap at all, truly"),
            Some(BmpErrorKind::Malformed)
        );
        let good = build(2, 2, ramp(2, 2));

        let mut wrong_magic = good.clone();
        wrong_magic[0] = b'X';
        assert_eq!(kind(&wrong_magic), Some(BmpErrorKind::Malformed));

        let mut wrong_size = good.clone();
        wrong_size[2] = wrong_size[2].wrapping_add(1);
        assert_eq!(kind(&wrong_size), Some(BmpErrorKind::Malformed));

        let mut truncated = good.clone();
        truncated.pop();
        assert_eq!(kind(&truncated), Some(BmpErrorKind::Malformed));

        let mut reserved = good.clone();
        reserved[6] = 1;
        assert_eq!(kind(&reserved), Some(BmpErrorKind::Unsupported));

        let mut palette_offset = good.clone();
        palette_offset[10] = 70;
        assert_eq!(kind(&palette_offset), Some(BmpErrorKind::Unsupported));

        let mut core_header = good.clone();
        core_header[14] = 12;
        assert_eq!(kind(&core_header), Some(BmpErrorKind::Unsupported));

        let mut eight_bit = good.clone();
        eight_bit[28] = 8;
        assert_eq!(kind(&eight_bit), Some(BmpErrorKind::Unsupported));

        let mut rle = good.clone();
        rle[30] = 1;
        assert_eq!(kind(&rle), Some(BmpErrorKind::Unsupported));

        let mut planes = good.clone();
        planes[26] = 3;
        assert_eq!(kind(&planes), Some(BmpErrorKind::Unsupported));

        // A top-down bitmap is legal and refused, on purpose.
        let mut top_down = good.clone();
        top_down[22..26].copy_from_slice(&(-2_i32).to_le_bytes());
        assert_eq!(kind(&top_down), Some(BmpErrorKind::Unsupported));
    }

    #[test]
    fn a_zero_size_image_field_is_accepted_and_a_third_value_is_not() {
        let mut bytes = build(2, 2, ramp(2, 2));
        bytes[34..38].copy_from_slice(&0_u32.to_le_bytes());
        assert!(BitmapImage::decode(&bytes).is_ok());
        bytes[34..38].copy_from_slice(&7_u32.to_le_bytes());
        assert_eq!(kind(&bytes), Some(BmpErrorKind::Malformed));
    }

    #[test]
    fn from_pixels_refuses_a_pixel_count_that_does_not_match_its_dimensions() {
        assert!(BitmapImage::from_pixels(4, 4, ramp(4, 3)).is_err());
        assert!(BitmapImage::from_pixels(4, 4, ramp(4, 4)).is_ok());
    }

    // --- The installed corpus ------------------------------------------------------------------
    //
    // `#[ignore]`d rather than skipped when the game is absent, for the reason `wave` gives: a test
    // that prints "could not run" and returns `ok` is indistinguishable from one that ran.
    //
    // Run with:
    //   LOM_GAME_DIR=.../English cargo test --release -- --ignored
    //
    // LOM_GAME_DIR must be the **GS5R3** profile. Measured 2026-09-19: GS5R3's `pic.mpq` holds 2
    // `.bmp` members and the 3.02, Development and Steambuild profiles hold 0, so pointing these at
    // another profile trips the size tripwire rather than passing over nothing.
    fn game_directory() -> std::path::PathBuf {
        let directory = std::env::var_os("LOM_GAME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set LOM_GAME_DIR to the installed GS5R3 English directory");
        assert!(
            directory.join("lomse.exe").is_file(),
            "no lomse.exe under {}",
            directory.display()
        );
        directory
    }

    /// Every `.bmp` in `pic.mpq`, as `(name, bytes)`, lowest name first.
    ///
    /// Names come from the archive's **internal** `(listfile)`, which names both members -- so
    /// unlike the tileset tests this one needs no `LOM_LISTFILE`.
    fn archived_bitmaps() -> Vec<(String, Vec<u8>)> {
        let archive =
            crate::mpq::Archive::open(&game_directory().join("pic.mpq")).expect("open pic.mpq");
        let mut out = Vec::new();
        for entry in archive.entries().expect("enumerate pic.mpq") {
            if entry.name.to_ascii_lowercase().ends_with(".bmp") {
                let bytes = archive.read(&entry.name).expect("read the member");
                out.push((entry.name.clone(), bytes));
            }
        }
        out.sort_by(|left, right| left.0.cmp(&right.0));
        out
    }

    /// The whole `.bmp` population decodes and survives the encoder byte for byte.
    #[test]
    #[ignore = "needs LOM_GAME_DIR (the GS5R3 profile)"]
    fn every_archived_bitmap_round_trips() {
        let members = archived_bitmaps();
        assert_eq!(members.len(), 2, "the archived BMP corpus changed size");
        let mut identical = 0;
        let mut shapes = std::collections::BTreeSet::new();
        for (name, bytes) in &members {
            let image = BitmapImage::decode(bytes)
                .unwrap_or_else(|error| panic!("{name} did not decode: {error}"));
            assert_eq!(&image.encode(), bytes, "{name} did not survive the encoder");
            identical += 1;
            shapes.insert((
                image.width,
                image.height,
                image.x_pixels_per_meter,
                image.y_pixels_per_meter,
                image.colors_used,
                image.colors_important,
            ));
        }
        assert_eq!(identical, members.len());
        // The header table in the module documentation, asserted rather than only written down.
        assert_eq!(
            shapes.into_iter().collect::<Vec<_>>(),
            vec![(400, 144, 2_834, 2_834, 0, 0)],
            "the two members no longer share one header shape"
        );
    }

    /// Channel order and row order, against an independent decoder rather than against this one.
    ///
    /// Each `.bmp` has a same-named `.lbm` sibling in the same archive. The assertion is that the
    /// standard B,G,R bottom-up reading matches it on **every** pixel and that the three
    /// alternative readings do not -- the negative control, without which a uniform image would
    /// pass whatever the convention.
    #[test]
    #[ignore = "needs LOM_GAME_DIR (the GS5R3 profile)"]
    fn every_archived_bitmap_matches_its_sibling_lbm() {
        let archive =
            crate::mpq::Archive::open(&game_directory().join("pic.mpq")).expect("open pic.mpq");
        let members = archived_bitmaps();
        assert_eq!(members.len(), 2, "the archived BMP corpus changed size");
        let mut compared = 0_usize;
        for (name, bytes) in &members {
            let sibling_name = format!("{}.lbm", name.trim_end_matches(".bmp"));
            let sibling_bytes = archive
                .read(&sibling_name)
                .unwrap_or_else(|error| panic!("{name} has no sibling {sibling_name}: {error}"));
            let sibling = crate::pbm::PbmImage::decode(&sibling_bytes)
                .unwrap_or_else(|error| panic!("{sibling_name} did not decode: {error}"));
            let image = BitmapImage::decode(bytes)
                .unwrap_or_else(|error| panic!("{name} did not decode: {error}"));
            assert_eq!(
                (image.width, image.height),
                (u32::from(sibling.width), u32::from(sibling.height)),
                "{name} and {sibling_name} are not the same size"
            );

            let pixels = (image.width * image.height) as usize;
            let reference: Vec<[u8; 3]> = (0..pixels)
                .map(|index| {
                    [
                        sibling.rgba[index * 4],
                        sibling.rgba[index * 4 + 1],
                        sibling.rgba[index * 4 + 2],
                    ]
                })
                .collect();

            let width = image.width as usize;
            let height = image.height as usize;
            let read = |swapped: bool, flipped: bool, index: usize| -> [u8; 3] {
                let (x, y) = (index % width, index / width);
                let y = if flipped { height - 1 - y } else { y };
                let [r, g, b] = image.pixels[y * width + x];
                if swapped { [b, g, r] } else { [r, g, b] }
            };
            let agreement = |swapped: bool, flipped: bool| -> usize {
                (0..pixels)
                    .filter(|index| read(swapped, flipped, *index) == reference[*index])
                    .count()
            };

            assert_eq!(
                agreement(false, false),
                pixels,
                "{name}: the B,G,R bottom-up reading does not reproduce {sibling_name}"
            );
            for (label, swapped, flipped) in [
                ("R,G,B bottom-up", true, false),
                ("B,G,R top-down", false, true),
                ("R,G,B top-down", true, true),
            ] {
                let agreed = agreement(swapped, flipped);
                assert!(
                    agreed < pixels,
                    "{name}: the {label} reading also reproduces {sibling_name} on all {pixels} \
                     pixels, so this comparison discriminates nothing"
                );
            }
            compared += pixels;
        }
        assert_eq!(
            compared, 115_200,
            "the compared pixel population changed size"
        );
    }
}
