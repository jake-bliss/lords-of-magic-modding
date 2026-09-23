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
//! from "malformed", and why the order the two are decided in is load-bearing.
//!
//! # Channel order and row order, measured against a sibling -- and what that rests on
//!
//! A 24-bit BMP could be read B,G,R or R,G,B and bottom-up or top-down, and judging that by eye is
//! how this repository previously got a whole probe wrong. It did not have to be judged: both `.bmp`
//! members have a **same-named sibling** in the same archive -- `LBM\ARTIFACT5R3A.lbm` and
//! `LBM\ARTIFACT5R3B.lbm`, IFF PBM images of the same 400x144 dimensions -- decoded by
//! [`crate::pbm`], which shares no helper, constant or code path with this module.
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
//! discriminating measurement rather than a flat image agreeing with itself.
//! `every_archived_bitmap_matches_its_sibling_lbm` asserts all four rows -- the three wrong ones as
//! a negative control, each with a **ceiling**, because "not a perfect match" would still be
//! satisfied by 57,599 and would discriminate nothing.
//!
//! ## The chain the conclusion actually hangs on
//!
//! **This measurement is relative, not absolute, and the distinction is the whole point.** What is
//! measured is that these bitmaps store B,G,R **relative to** [`crate::pbm`]'s reading of an IFF
//! `CMAP`, and that reading -- `pbm.rs` slices each palette entry positionally as R, G, B -- is
//! **Documented** from the IFF/ILBM specification and has **never itself been measured against the
//! engine**. Nothing in this repository pins the PBM palette channel order to what `lomse.exe`
//! draws.
//!
//! So the honest statement is a chain:
//!
//! * **Observed in the corpus**: these two bitmaps and their `.lbm` siblings encode the same image,
//!   and the BMP's byte triples are the reverse of the PBM's palette triples.
//! * **Documented**: the PBM palette triples are R, G, B, per the IFF specification.
//! * **Inferred** from the two together: the bitmaps are B, G, R -- the Windows standard.
//!
//! Had `pbm` been R/B-swapped, the true reading would have *failed* the comparison and the obvious
//! response would have been to flip this decoder, producing a confident and exactly wrong result.
//! `docs/research-log.md` records that hazard as the one anticipated for a palette-channel
//! measurement; this module is that measurement, so the dependency is named here rather than left
//! for a reader to find. What would settle it absolutely: a `screencapture` of a frame drawn from a
//! known `.lbm`, or an operator body traced to the palette load.
//!
//! **This is not the same claim as the `screencapture` one.** `docs/research-log.md` records that
//! the engine's own `screencapture` operator writes G,R,B into a file whose header says otherwise
//! (corrected 2026-09-23 from R,G,B; see the research log entry of that date).
//! That is a property of *that operator's output*, measured on captures. These two members are
//! authored art shipped inside an archive. The two findings do not conflict and neither one may be
//! used to predict the other.

use std::fmt;

/// The file header plus a `BITMAPINFOHEADER`: the only pixel offset this module accepts.
const HEADER_BYTES: usize = 54;

/// The Windows `BITMAPFILEHEADER`, which is where `biSize` lives and so the minimum readable prefix.
const FILE_HEADER_BYTES: usize = 14;

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
/// downgraded to a classification by [`crate::asset::probe`]; [`Malformed`](Self::Malformed) is a
/// scan failure.
///
/// # The order these are decided in is load-bearing
///
/// **A review found the first version of this module getting it backwards**, and the consequence is
/// worse than it sounds: every variant refusal ran before the check that the file can hold the
/// pixels it declares, so a *truncated* member reported `Unsupported: biBitCount is 8` and scanned
/// as zero failures. A bit-rotted archive would have looked clean, with the blame pinned on a
/// format variant that was not the problem.
///
/// The rule this module now follows:
///
/// > **A variant check may run before the structural check only when the variant makes the
/// > structural check undecidable.**
///
/// Exactly two qualify. `biSize` decides *where* the width, height and depth are, so a DIB header
/// this module cannot locate fields in cannot be measured at all. `biCompression` decides *whether*
/// the pixel length is derivable from the dimensions, which it is not for any RLE encoding. Every
/// other refusal -- the offset, the reserved words, the plane count, the bit depth, the row order --
/// runs **after** the file has been shown to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BmpErrorKind {
    /// The bytes do not form a bitmap: truncated, wrong magic, a body too short for its own header.
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
/// # Every field is either carried from the file or derived *and* checked
///
/// That is what makes [`encode`](Self::encode) reproduce a decoded member, and the first version of
/// this module claimed it while being **false in two places** -- a review compiled the module and
/// executed the counterexamples rather than reasoning about them:
///
/// * `biSizeImage` was *accepted* as 0 and *written* as the derived byte count, so a file declaring
///   0 decoded fine and came back changed. It is now **carried**.
/// * `bfSize` was *required* to equal the member's length, which is not the same as carrying it:
///   a wrong or zero `bfSize` is common in real BMP writers, prevents no decode, and was being
///   turned into a `Malformed` probe failure across all five archives. It is now **carried**.
/// * A nonzero **padding** byte was accepted and rewritten as 0. There is nowhere to carry it and
///   no corpus member has one, so it is now **refused** as
///   [`Unsupported`](BmpErrorKind::Unsupported) rather than silently normalised.
///
/// The claim is no longer asserted in prose either: `every_header_mutation_that_decodes_also_re_encodes`
/// sweeps every byte of the header and the body of a fixture *that has padding*, and asserts that
/// every mutation which decodes re-encodes to itself. That is a measurement of the property, not a
/// statement about the code's intent.
///
/// The fields are **private**. They were public, and a review measured `encode` panicking with
/// `index out of bounds` after a caller set `width` on a decoded image -- a state neither
/// constructor can produce. Read them through the accessors; build one with
/// [`from_pixels`](Self::from_pixels) or [`decode`](Self::decode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapImage {
    width: u32,
    height: u32,
    /// One `[r, g, b]` per pixel, row-major, **first row is the top row**.
    pixels: Vec<[u8; 3]>,
    /// `bfSize`, carried. See the type documentation.
    file_size: u32,
    /// `biSizeImage`, carried. Legally 0 for `BI_RGB`, and one corpus member's worth of files say
    /// nothing about which a writer should emit.
    size_image: u32,
    x_pixels_per_meter: i32,
    y_pixels_per_meter: i32,
    colors_used: u32,
    colors_important: u32,
}

impl BitmapImage {
    /// A bitmap built from nothing, with the header fields both corpus members carry.
    ///
    /// 2,834 pixels per metre is 72 dpi. It is used here because it is what the two shipped members
    /// declare (**Observed in the corpus**); nothing in the engine is known to read it.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<[u8; 3]>) -> Result<Self, BmpError> {
        let expected = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(width, height)| width.checked_mul(height))
            .ok_or_else(|| BmpError::malformed("bitmap dimensions overflow"))?;
        if pixels.len() != expected {
            return Err(BmpError::malformed(format!(
                "{width}x{height} needs {expected} pixels, got {}",
                pixels.len()
            )));
        }
        let (file_size, size_image) = Self::derived_sizes(width, height)?;
        Ok(Self {
            width,
            height,
            pixels,
            file_size,
            size_image,
            x_pixels_per_meter: 2_834,
            y_pixels_per_meter: 2_834,
            colors_used: 0,
            colors_important: 0,
        })
    }

    /// `(bfSize, biSizeImage)` as the dimensions require them, refusing a file too big to describe.
    ///
    /// Computed once at construction and stored, so [`encode`](Self::encode) has no arithmetic that
    /// could fail and no `unwrap` that could write a false length. The previous version wrote
    /// `u32::try_from(..).unwrap_or(u32::MAX)`, which would have emitted a header claiming a size
    /// the file does not have rather than refusing.
    fn derived_sizes(width: u32, height: u32) -> Result<(u32, u32), BmpError> {
        let stride = Self::stride(width)
            .ok_or_else(|| BmpError::malformed("bitmap row length overflows"))?;
        let pixel_bytes = stride
            .checked_mul(height as usize)
            .ok_or_else(|| BmpError::malformed("bitmap pixel data overflows"))?;
        let file_size = pixel_bytes
            .checked_add(HEADER_BYTES)
            .and_then(|total| u32::try_from(total).ok())
            .ok_or_else(|| BmpError::malformed("bitmap is too large to describe in a header"))?;
        let size_image = u32::try_from(pixel_bytes)
            .map_err(|_| BmpError::malformed("bitmap pixel data is too large for biSizeImage"))?;
        Ok((file_size, size_image))
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// One `[r, g, b]` per pixel, row-major, first row is the top row.
    pub fn pixels(&self) -> &[[u8; 3]] {
        &self.pixels
    }

    pub fn x_pixels_per_meter(&self) -> i32 {
        self.x_pixels_per_meter
    }

    pub fn y_pixels_per_meter(&self) -> i32 {
        self.y_pixels_per_meter
    }

    pub fn colors_used(&self) -> u32 {
        self.colors_used
    }

    pub fn colors_important(&self) -> u32 {
        self.colors_important
    }

    /// `bfSize` as the file declares it, which is **not** guaranteed to be the member's length.
    pub fn declared_file_size(&self) -> u32 {
        self.file_size
    }

    /// `biSizeImage` as the file declares it, which is legally 0 for `BI_RGB`.
    pub fn declared_size_image(&self) -> u32 {
        self.size_image
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
    /// field that put it there; a file that is not a bitmap at all, or that cannot hold the pixels
    /// its own header declares, is [`Malformed`](BmpErrorKind::Malformed). See [`BmpErrorKind`] for
    /// why the structural check comes first and which two variant checks are allowed to precede it.
    pub fn decode(bytes: &[u8]) -> Result<Self, BmpError> {
        // --- Is this a bitmap at all, and can its declaration be located? ---------------------
        if bytes.len() < FILE_HEADER_BYTES + 4 {
            return Err(BmpError::malformed(format!(
                "truncated bitmap: {} bytes, too few to hold a file header and a DIB size",
                bytes.len()
            )));
        }
        if &bytes[0..2] != b"BM" {
            return Err(BmpError::malformed("not a bitmap: no BM signature"));
        }
        let info_size = read_u32(bytes, 14);
        // Undecidable-first, exception 1 of 2: `biSize` says where width, height and depth live.
        // Without it there is no declaration to measure the file against.
        if info_size != INFO_HEADER_BYTES {
            return Err(BmpError::unsupported(format!(
                "DIB header size {info_size}; only BITMAPINFOHEADER ({INFO_HEADER_BYTES}) is \
                 implemented"
            )));
        }
        if bytes.len() < HEADER_BYTES {
            return Err(BmpError::malformed(format!(
                "truncated bitmap: {} bytes, a BITMAPINFOHEADER needs {HEADER_BYTES}",
                bytes.len()
            )));
        }

        let pixel_offset = read_u32(bytes, 10);
        let width = read_i32(bytes, 18);
        let height = read_i32(bytes, 22);
        let bits_per_pixel = read_u16(bytes, 28);
        let compression = read_u32(bytes, 30);

        // Undecidable-first, exception 2 of 2: for any RLE or bitfield encoding the stored length
        // is not a function of the dimensions, so the structural check below does not apply.
        if compression != BI_RGB {
            return Err(BmpError::unsupported(format!(
                "biCompression is {compression}; only BI_RGB ({BI_RGB}) is implemented"
            )));
        }

        // --- Can the file hold what it declares? ----------------------------------------------
        //
        // This runs before every remaining variant check. A 54-byte member declaring 400x144 is
        // broken whatever its bit depth or row order says, and reporting the depth would blame the
        // wrong thing *and* downgrade a failure to a classification.
        if width <= 0 {
            return Err(BmpError::malformed(format!("biWidth is {width}")));
        }
        if height == 0 {
            return Err(BmpError::malformed("biHeight is 0"));
        }
        if bits_per_pixel == 0 {
            return Err(BmpError::malformed("biBitCount is 0"));
        }
        let rows = height.unsigned_abs();
        let declared_stride = stride_for(width.unsigned_abs(), bits_per_pixel)
            .ok_or_else(|| BmpError::malformed("bitmap row length overflows"))?;
        let declared_pixels = declared_stride
            .checked_mul(rows as usize)
            .ok_or_else(|| BmpError::malformed("bitmap pixel data overflows"))?;
        let offset = usize::try_from(pixel_offset)
            .map_err(|_| BmpError::malformed(format!("bfOffBits {pixel_offset} is not a size")))?;
        if offset > bytes.len() {
            return Err(BmpError::malformed(format!(
                "bfOffBits is {pixel_offset}, past the end of the {}-byte member",
                bytes.len()
            )));
        }
        let available = bytes.len() - offset;
        if available < declared_pixels {
            return Err(BmpError::malformed(format!(
                "{}x{} at {bits_per_pixel} bpp needs {declared_pixels} pixel bytes, the member has \
                 {available} after bfOffBits",
                width.unsigned_abs(),
                rows,
            )));
        }
        if available > declared_pixels {
            // Legal -- a BMP may carry trailing data -- and there is nowhere on this struct to keep
            // it, so re-encoding would drop it. Classified, not failed.
            return Err(BmpError::unsupported(format!(
                "{} byte(s) follow the pixel data; this module carries no trailing data",
                available - declared_pixels
            )));
        }

        // --- The variants this module does not implement ---------------------------------------
        if offset != HEADER_BYTES {
            return Err(BmpError::unsupported(format!(
                "bfOffBits is {pixel_offset}; only {HEADER_BYTES} (no palette and no gap) is \
                 implemented"
            )));
        }
        // Both reserved words are zero in the corpus and this struct has nowhere to keep a nonzero
        // one, so re-encoding such a file would silently drop it.
        let reserved = (read_u16(bytes, 6), read_u16(bytes, 8));
        if reserved != (0, 0) {
            return Err(BmpError::unsupported(format!(
                "bfReserved1/bfReserved2 are {}/{}, not 0/0",
                reserved.0, reserved.1
            )));
        }
        let planes = read_u16(bytes, 26);
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
        // A negative height is a legal top-down bitmap and no member is one. Row order is the axis
        // the sibling-LBM comparison actually discriminated (see the module header); implementing
        // the reading it refuted, uncovered by any file, is how a wrong branch survives.
        if height < 0 {
            return Err(BmpError::unsupported(format!(
                "biHeight is {height}; only a positive (bottom-up) height is implemented"
            )));
        }

        let width = width.unsigned_abs();
        let stride = declared_stride;
        // A padding byte has nowhere to live on this struct, so accepting a nonzero one would mean
        // rewriting it as 0 -- which is exactly the silent normalisation the round-trip claim is
        // supposed to exclude. The corpus has no padding byte at all, so this refuses nothing that
        // ships.
        let row_pixels = width as usize * 3;
        for row in 0..rows as usize {
            let base = HEADER_BYTES + row * stride;
            if let Some(at) = (row_pixels..stride).find(|at| bytes[base + at] != 0) {
                return Err(BmpError::unsupported(format!(
                    "row {row} has a nonzero padding byte ({:#04x} at offset {}); this module \
                     writes zero padding and cannot carry another value",
                    bytes[base + at],
                    base + at
                )));
            }
        }

        let mut pixels = Vec::with_capacity(width as usize * rows as usize);
        for row in 0..rows as usize {
            // Bottom-up: stored row 0 is the image's last row.
            let base = HEADER_BYTES + (rows as usize - 1 - row) * stride;
            for column in 0..width as usize {
                let at = base + column * 3;
                // B, G, R in the file; R, G, B here.
                pixels.push([bytes[at + 2], bytes[at + 1], bytes[at]]);
            }
        }

        Ok(Self {
            width,
            height: rows,
            pixels,
            file_size: read_u32(bytes, 2),
            size_image: read_u32(bytes, 34),
            x_pixels_per_meter: read_i32(bytes, 38),
            y_pixels_per_meter: read_i32(bytes, 42),
            colors_used: read_u32(bytes, 46),
            colors_important: read_u32(bytes, 50),
        })
    }

    /// Write the bitmap back out.
    ///
    /// `encode(decode(bytes)) == bytes` for **2 of 2** archived members (**Observed in the
    /// corpus**, 2026-09-19, `every_archived_bitmap_round_trips`) and for **every** mutation of a
    /// padded synthetic fixture that decodes at all
    /// (`every_header_mutation_that_decodes_also_re_encodes`). The second test is there because the
    /// first version of this module asserted the property in prose and was wrong twice; see the
    /// type documentation for what those two were.
    ///
    /// Padding bytes are written as zero, and a member whose padding is not zero is refused at
    /// decode rather than normalised here. The corpus has no padding byte at all, so nothing here
    /// witnesses what a real one holds -- see [`stride`](Self::stride).
    pub fn encode(&self) -> Vec<u8> {
        // Infallible by construction: `width`, `height` and `pixels` are private and every
        // constructor establishes `pixels.len() == width * height` and a representable
        // `file_size`. A review measured the previous version panicking here after a caller set
        // `width` on a decoded image, which is why the fields are no longer public.
        let stride = Self::stride(self.width).expect("a constructed bitmap has a stride");
        let pixel_bytes = stride * self.height as usize;
        let mut out = Vec::with_capacity(HEADER_BYTES + pixel_bytes);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&self.file_size.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
        out.extend_from_slice(&INFO_HEADER_BYTES.to_le_bytes());
        out.extend_from_slice(&(self.width as i32).to_le_bytes());
        out.extend_from_slice(&(self.height as i32).to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&BITS_PER_PIXEL.to_le_bytes());
        out.extend_from_slice(&BI_RGB.to_le_bytes());
        out.extend_from_slice(&self.size_image.to_le_bytes());
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

/// Bytes per stored row at an arbitrary depth, for the structural check only.
///
/// [`BitmapImage::stride`] is the 24-bit case this module actually encodes. This one exists so that
/// a member declaring a depth the module does not implement can still be *measured* against its own
/// length before being refused for its depth -- which is the whole of the ordering rule in
/// [`BmpErrorKind`].
fn stride_for(width: u32, bits_per_pixel: u16) -> Option<usize> {
    let bits = u64::from(width).checked_mul(u64::from(bits_per_pixel))?;
    usize::try_from(bits.checked_add(31)? / 32 * 4).ok()
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
        assert_eq!(decoded.width(), 5);
        assert_eq!(decoded.height(), 4);
        assert_eq!(decoded.pixels(), pixels.as_slice());
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
            assert_eq!(decoded.pixels(), pixels.as_slice(), "width {width}");
            assert_eq!(decoded.encode(), bytes, "width {width}");
        }
    }

    /// A nonzero padding byte is **refused**, not normalised.
    ///
    /// It used to decode and come back as zero, which is the silent rewrite the round-trip claim
    /// exists to exclude. There is nowhere on the struct to carry it and no corpus member has one.
    #[test]
    fn a_nonzero_padding_byte_is_refused_rather_than_rewritten_as_zero() {
        let mut bytes = build(1, 1, ramp(1, 1));
        assert_eq!(bytes[HEADER_BYTES + 3], 0, "the fixture has a padding byte");
        assert!(BitmapImage::decode(&bytes).is_ok());

        bytes[HEADER_BYTES + 3] = 0xAB;
        let error = BitmapImage::decode(&bytes).expect_err("a nonzero padding byte is refused");
        assert_eq!(error.kind(), BmpErrorKind::Unsupported);
        assert!(error.to_string().contains("padding byte"), "{error}");
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

        let mut truncated = good.clone();
        truncated.pop();
        assert_eq!(kind(&truncated), Some(BmpErrorKind::Malformed));

        let mut reserved = good.clone();
        reserved[6] = 1;
        assert_eq!(kind(&reserved), Some(BmpErrorKind::Unsupported));

        let mut palette_offset = good.clone();
        palette_offset[10] = 70;
        assert_eq!(kind(&palette_offset), Some(BmpErrorKind::Malformed));

        let mut core_header = good.clone();
        core_header[14] = 12;
        assert_eq!(kind(&core_header), Some(BmpErrorKind::Unsupported));

        let mut rle = good.clone();
        rle[30] = 1;
        assert_eq!(kind(&rle), Some(BmpErrorKind::Unsupported));

        let mut planes = good.clone();
        planes[26] = 3;
        assert_eq!(kind(&planes), Some(BmpErrorKind::Unsupported));

        // A top-down bitmap is legal and refused, on purpose. The fixture's body is the right
        // length for it, so this reaches the variant check rather than the structural one.
        let mut top_down = good.clone();
        top_down[22..26].copy_from_slice(&(-2_i32).to_le_bytes());
        assert_eq!(kind(&top_down), Some(BmpErrorKind::Unsupported));

        // Trailing data is legal and is not carried.
        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(kind(&trailing), Some(BmpErrorKind::Unsupported));
    }

    /// A file that cannot hold its own pixels is **Malformed**, whatever else its header says.
    ///
    /// This is the review finding the ordering rule in [`BmpErrorKind`] exists for. Each of these
    /// previously returned `Unsupported`, which `asset::probe` turns into a classification with no
    /// scan failure -- so a truncated or bit-rotted member reported `--scan: 0 failures` with the
    /// blame pinned on a format variant that was not the problem.
    #[test]
    fn a_truncated_member_is_malformed_even_when_its_header_also_names_a_variant() {
        let mut header = vec![0_u8; HEADER_BYTES];
        header[0..2].copy_from_slice(b"BM");
        header[2..6].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
        header[10..14].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
        header[14..18].copy_from_slice(&40_u32.to_le_bytes());
        header[18..22].copy_from_slice(&400_i32.to_le_bytes());
        header[26..28].copy_from_slice(&1_u16.to_le_bytes());
        header[30..34].copy_from_slice(&0_u32.to_le_bytes());

        // 400x144 at 24 bpp needs 172,800 pixel bytes; the file carries none.
        for (label, height, bits) in [
            ("top-down", -144_i32, 24_u16),
            ("bottom-up", 144, 24),
            ("eight-bit", 144, 8),
            ("eight-bit top-down", -144, 8),
            ("thirty-two-bit", 144, 32),
        ] {
            let mut bytes = header.clone();
            bytes[22..26].copy_from_slice(&height.to_le_bytes());
            bytes[28..30].copy_from_slice(&bits.to_le_bytes());
            let error = BitmapImage::decode(&bytes).expect_err(label);
            assert_eq!(error.kind(), BmpErrorKind::Malformed, "{label}: {error}");
            assert!(
                error.to_string().contains("pixel bytes"),
                "{label}: the message must name the truncation, not a variant -- {error}"
            );
        }

        // ...and a bfOffBits past the end is structural too, not a palette variant.
        let mut past_end = header.clone();
        past_end[22..26].copy_from_slice(&144_i32.to_le_bytes());
        past_end[28..30].copy_from_slice(&24_u16.to_le_bytes());
        past_end[10..14].copy_from_slice(&100_000_u32.to_le_bytes());
        let error = BitmapImage::decode(&past_end).expect_err("bfOffBits past the end");
        assert_eq!(error.kind(), BmpErrorKind::Malformed);
        assert!(error.to_string().contains("past the end"), "{error}");
    }

    /// Degenerate dimensions are structural, not variant.
    #[test]
    fn a_zero_or_negative_dimension_is_malformed() {
        let good = build(2, 2, ramp(2, 2));
        for (label, at, value) in [
            ("zero width", 18_usize, 0_i32),
            ("negative width", 18, -2),
            ("zero height", 22, 0),
        ] {
            let mut bytes = good.clone();
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert_eq!(kind(&bytes), Some(BmpErrorKind::Malformed), "{label}");
        }
        let mut zero_depth = good.clone();
        zero_depth[28..30].copy_from_slice(&0_u16.to_le_bytes());
        assert_eq!(kind(&zero_depth), Some(BmpErrorKind::Malformed));
    }

    /// `bfSize` and `biSizeImage` are **carried**, so a nonstandard value re-encodes as itself.
    ///
    /// Both were previously wrong in opposite directions: `biSizeImage` of 0 was accepted and
    /// rewritten as the derived count, and `bfSize` was *required* to equal the member's length,
    /// turning a common real-world nonconformance into a probe failure across all five archives.
    #[test]
    fn bf_size_and_bi_size_image_are_carried_not_derived() {
        for (label, at, value) in [
            ("biSizeImage 0", 34_usize, 0_u32),
            ("biSizeImage nonstandard", 34, 7),
            ("bfSize 0", 2, 0),
            ("bfSize nonstandard", 2, 1),
            ("bfSize huge", 2, u32::MAX),
        ] {
            let mut bytes = build(4, 3, ramp(4, 3));
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            let decoded = BitmapImage::decode(&bytes)
                .unwrap_or_else(|error| panic!("{label} should decode: {error}"));
            assert_eq!(decoded.encode(), bytes, "{label} did not round-trip");
        }
    }

    /// The round-trip property, **measured** rather than argued.
    ///
    /// The first version of this module asserted "byte-identity by construction" in prose and was
    /// false in two places that a review found by executing them. A claim about every input is a
    /// claim a sweep can test: mutate every byte of the header and the body of a fixture that
    /// **has padding**, and require that whatever decodes re-encodes to itself.
    ///
    /// The counters are asserted so the sweep cannot quietly stop reaching the accepting path --
    /// a version that refused everything would satisfy the implication vacuously.
    #[test]
    fn every_header_mutation_that_decodes_also_re_encodes() {
        // 3 pixels x 3 bytes = 9, padded to a 12-byte stride: this fixture has padding, which the
        // corpus does not, and which is where one of the two false claims lived.
        let base = build(3, 2, ramp(3, 2));
        assert_eq!(base.len(), HEADER_BYTES + 12 * 2);

        const PROBES: [u8; 6] = [0x00, 0x01, 0x7F, 0x80, 0xAB, 0xFF];
        let mut decoded = 0_usize;
        let mut refused = 0_usize;
        let mut unchanged = 0_usize;
        for at in 0..base.len() {
            for value in PROBES {
                let mut bytes = base.clone();
                if bytes[at] == value {
                    // The byte already holds this probe value, so there is no mutation to make.
                    // Counted rather than skipped silently: the sweep-size assertion below is what
                    // catches a loop that stopped iterating.
                    unchanged += 1;
                    continue;
                }
                bytes[at] = value;
                match BitmapImage::decode(&bytes) {
                    Ok(image) => {
                        decoded += 1;
                        assert_eq!(
                            image.encode(),
                            bytes,
                            "byte {at} set to {value:#04x} decoded but did not re-encode"
                        );
                    }
                    Err(_) => refused += 1,
                }
            }
        }
        // Both arms have to be reachable for the sweep to mean anything, and the sweep has to
        // have actually run: an implication over an empty set is vacuously true.
        assert!(decoded > 0, "no mutation decoded; the sweep proves nothing");
        assert!(refused > 0, "no mutation was refused; the guards are inert");
        assert_eq!(
            decoded + refused + unchanged,
            PROBES.len() * base.len(),
            "the sweep did not visit every byte at every probe value"
        );
        // **Measured**, 2026-09-19, not chosen: 220 of the 407 real mutations still decode, and
        // every one of them re-encoded to itself. Pinned so that a future change narrowing what
        // decodes is visible here rather than silently shrinking the population the round-trip
        // claim rests on -- 220 accepted is what stops the implication above being vacuous.
        assert_eq!(
            (decoded, refused, unchanged),
            (220, 187, 61),
            "the accepted/refused split moved; the round-trip claim now rests on a different set"
        );
    }

    #[test]
    fn from_pixels_refuses_a_pixel_count_that_does_not_match_its_dimensions() {
        assert!(BitmapImage::from_pixels(4, 4, ramp(4, 3)).is_err());
        assert!(BitmapImage::from_pixels(4, 4, ramp(4, 4)).is_ok());
    }

    /// A bitmap too large to describe in a 32-bit header is refused, not silently mis-sized.
    ///
    /// The previous `encode` wrote `u32::try_from(..).unwrap_or(u32::MAX)`, which would have
    /// emitted a header claiming a length the file does not have.
    #[test]
    fn dimensions_too_large_for_the_header_are_refused_at_construction() {
        // 100,000 x 100,000 x 3 overflows a u32 byte count long before it overflows a usize.
        let error = BitmapImage::from_pixels(100_000, 100_000, Vec::new())
            .expect_err("an unrepresentable bitmap is refused");
        assert_eq!(error.kind(), BmpErrorKind::Malformed);
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
    // another profile trips the size tripwire rather than passing over nothing. That was verified
    // by running them against the 3.02 profile, where both fail on the tripwire.
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
    /// unlike the tileset work this needs no `LOM_LISTFILE`.
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

    /// The `.lbm` beside a `.bmp`, matched **case-insensitively** on the suffix.
    ///
    /// The member filter above is case-insensitive, so deriving the sibling with a case-sensitive
    /// `trim_end_matches(".bmp")` would, on a member listed as `...A.BMP`, build `...A.BMP.lbm` and
    /// panic -- an instrument failure that would read as a corpus finding.
    fn sibling_name(bitmap: &str) -> String {
        let stem = &bitmap[..bitmap.len() - ".bmp".len()];
        format!("{stem}.lbm")
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
                image.width(),
                image.height(),
                image.declared_file_size(),
                image.declared_size_image(),
                image.x_pixels_per_meter(),
                image.y_pixels_per_meter(),
                image.colors_used(),
                image.colors_important(),
            ));
        }
        assert_eq!(identical, members.len());
        // The header table in the module documentation, asserted rather than only written down.
        // `bfSize` and `biSizeImage` are in here because they are now carried rather than derived,
        // so nothing else would notice if the corpus stopped agreeing with the table.
        assert_eq!(
            shapes.into_iter().collect::<Vec<_>>(),
            vec![(400, 144, 172_854, 172_800, 2_834, 2_834, 0, 0)],
            "the two members no longer share one header shape"
        );
    }

    /// Channel order and row order, against an independent decoder rather than against this one.
    ///
    /// Each `.bmp` has a same-named `.lbm` sibling in the same archive. The assertion is that the
    /// standard B,G,R bottom-up reading matches it on **every** pixel and that the three
    /// alternative readings come nowhere close -- the negative control. Each wrong reading is held
    /// under **half** the population rather than merely under all of it: the documented margins are
    /// 12-20%, and an assertion of `< 57_600` would be satisfied by 57,599, which would discriminate
    /// nothing while the doc still claimed it did.
    ///
    /// **What this does and does not establish** is set out in the module header: the result is
    /// B,G,R *relative to* `crate::pbm`'s ILBM `CMAP` reading, which is Documented rather than
    /// measured against the engine.
    #[test]
    #[ignore = "needs LOM_GAME_DIR (the GS5R3 profile)"]
    fn every_archived_bitmap_matches_its_sibling_lbm() {
        let archive =
            crate::mpq::Archive::open(&game_directory().join("pic.mpq")).expect("open pic.mpq");
        let members = archived_bitmaps();
        assert_eq!(members.len(), 2, "the archived BMP corpus changed size");
        let mut compared = 0_usize;
        for (name, bytes) in &members {
            let sibling = sibling_name(name);
            let sibling_bytes = archive
                .read(&sibling)
                .unwrap_or_else(|error| panic!("{name} has no sibling {sibling}: {error}"));
            let reference_image = crate::pbm::PbmImage::decode(&sibling_bytes)
                .unwrap_or_else(|error| panic!("{sibling} did not decode: {error}"));
            let image = BitmapImage::decode(bytes)
                .unwrap_or_else(|error| panic!("{name} did not decode: {error}"));
            assert_eq!(
                (image.width(), image.height()),
                (
                    u32::from(reference_image.width),
                    u32::from(reference_image.height)
                ),
                "{name} and {sibling} are not the same size"
            );

            let pixels = (image.width() * image.height()) as usize;
            let reference: Vec<[u8; 3]> = (0..pixels)
                .map(|index| {
                    [
                        reference_image.rgba[index * 4],
                        reference_image.rgba[index * 4 + 1],
                        reference_image.rgba[index * 4 + 2],
                    ]
                })
                .collect();

            let width = image.width() as usize;
            let height = image.height() as usize;
            let read = |swapped: bool, flipped: bool, index: usize| -> [u8; 3] {
                let (x, y) = (index % width, index / width);
                let y = if flipped { height - 1 - y } else { y };
                let [r, g, b] = image.pixels()[y * width + x];
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
                "{name}: the B,G,R bottom-up reading does not reproduce {sibling}"
            );
            for (label, swapped, flipped) in [
                ("R,G,B bottom-up", true, false),
                ("B,G,R top-down", false, true),
                ("R,G,B top-down", true, true),
            ] {
                let agreed = agreement(swapped, flipped);
                assert!(
                    agreed * 2 < pixels,
                    "{name}: the {label} reading agrees on {agreed} of {pixels} pixels. The \
                     documented margins are 12-20%; at half or more this comparison no longer \
                     discriminates and the conclusion drawn from it does not hold"
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
