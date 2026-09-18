//! The two content checksums the engine computes over installed files, read out of the binary.
//!
//! Multiplayer in this engine is a lockstep simulation that compares six values between peers and
//! reports `Divergence` when any of them disagrees (see `docs/multiplayer.md`). Three of the six
//! are named after installed content -- `'EXE' version`, `'GS' files`, `'IMP' files` -- and if
//! those can be reproduced offline then the commonest cause of a desync becomes a pre-flight
//! check instead of a mid-game mystery.
//!
//! Two of the three are reproducible. Both are plain byte sums, and **they do not use the same
//! byte sum**, which is the whole reason this module exists rather than a one-line closure at the
//! call site.
//!
//! # `'EXE' version` -- [`exe_checksum`]
//!
//! Computed once at `0x004b4a95`-`0x004b4b2a` and cached in the global `0x00584424`. The engine
//! calls `GetModuleFileNameA(NULL, ...)`, opens that path `"rb"`, reads the whole file, and sums it:
//!
//! ```text
//! 004b4b0c  mov [584424h],ebx        ; ebx is zero: the accumulator starts at 0
//! 004b4b14  mov ecx,[584424h]
//! 004b4b1a  xor edx,edx              ; <-- zero-extend
//! 004b4b1c  mov dl,[eax+edi]
//! 004b4b1f  add ecx,edx
//! 004b4b21  inc eax
//! 004b4b22  cmp eax,esi              ; esi is the file length
//! 004b4b24  mov [584424h],ecx
//! 004b4b2a  jb  004B4B14h            ; <-- jb: unsigned compare
//! ```
//!
//! `xor edx,edx` before `mov dl` is a **zero**-extension, so every byte contributes `0..=255`.
//! There is no truncation other than the natural 32-bit wrap of `add`.
//!
//! # `'GS' files` -- [`script_checksum`]
//!
//! Accumulated as GameScript sources are loaded, at `0x004d497b`-`0x004d4990`, into the global
//! `0x00584604` -- the same global the state dump prints as `GS Checksum=%d`:
//!
//! ```text
//! 004d496c  mov eax,[584600h]        ; the gschecksumon/gschecksumoff flag
//! 004d4971  test eax,eax
//! 004d4973  je  004D4992h            ; flag clear: accumulate nothing
//! 004d497b  movsx edx,byte [eax+edi] ; <-- SIGN-extend
//! 004d497f  mov ebp,[584604h]
//! 004d4985  add ebp,edx
//! 004d4987  inc eax
//! 004d4988  cmp eax,ecx
//! 004d498a  mov [584604h],ebp
//! 004d4990  jl  004D497Bh
//! ```
//!
//! `movsx` is a **sign**-extension, so a byte of `0x80..=0xFF` contributes `-128..=-1`. Guessing
//! "a byte sum" and writing the obvious unsigned loop produces a different number for any file
//! containing a byte above `0x7F`, which every real script member does. That difference is what
//! [`tests::the_two_checksums_disagree_on_a_high_byte`] pins down.
//!
//! # What this module deliberately does not claim
//!
//! `script_checksum` is the engine's *algorithm*, applied to whatever byte string the caller
//! hands it. It is **not** a reproduction of the engine's *value*, because the engine's value
//! covers exactly the script members loaded while the flag is set, and the engine brackets one
//! top-level script run: at `0x004ff59c` it zeroes `0x00584604`, sets `0x00584600`, calls the
//! loader at `0x004d48a0`, and clears the flag again at `0x004ff5b8`. Which members that run
//! reaches is a property of the script graph, not of the archive, and this module does not
//! determine it. A `gschecksumon` call from a mod widens the bracket further.
//!
//! The consequence for a compatibility check is the useful part: **the comparison is sound even
//! though the absolute value is not.** Two installs whose loaded script bytes are identical must
//! produce the same engine value; two installs that differ in a loaded member must differ unless
//! the sums collide. So callers should report the sum *and* an exact content comparison, and treat
//! the exact comparison as authoritative. [`ContentVerdict`] exists to keep those two separate.
//!
//! `'IMP' files` is not here because no code that computes it was found; see `docs/multiplayer.md`.

/// The `'EXE' version` checksum: a 32-bit wrapping sum of the zero-extended bytes.
///
/// Reproduces `0x004b4a95`-`0x004b4b2a` exactly for the whole contents of `lomse.exe`.
pub fn exe_checksum(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0_u32, |total, byte| total.wrapping_add(u32::from(*byte)))
}

/// The `'GS' files` accumulator: a 32-bit wrapping sum of the **sign**-extended bytes.
///
/// Reproduces `0x004d497b`-`0x004d4990` for one buffer. The engine's global is the running total
/// over every buffer the script loader is handed while `gschecksumon` is in effect, so summing
/// several members is the caller's job -- and because addition is associative and commutative,
/// load order does not matter.
pub fn script_checksum(bytes: &[u8]) -> i32 {
    bytes.iter().fold(0_i32, |total, byte| {
        total.wrapping_add(i32::from(*byte as i8))
    })
}

/// Continue a [`script_checksum`] across another buffer, as the engine's global does.
pub fn script_checksum_continued(total: i32, bytes: &[u8]) -> i32 {
    total.wrapping_add(script_checksum(bytes))
}

/// The tag the engine builds from the two reproducible checksums and attaches to a session.
///
/// **This is the one place a player can see these values without patching anything.** The engine
/// formats `cksum=%d,%d` (`0x005736ec`) at `0x00505f30` from `[0x00584604]` and `[0x00584424]`,
/// and the argument order is worth getting right: the caller at `0x00505f10` pushes the executable
/// sum then the script checksum, and the callee re-pushes them so that the **script checksum comes
/// first** and the executable sum second.
///
/// Both concrete transports call it while setting the session name — the Storm class at
/// `0x0046fbf8` and `CDPlay` at `0x0044a84e`. `CDPlay`'s vtable slot 11 (`0x0044a840`) compares the
/// local tag against a session's byte by byte and, when they differ, formats the session's display
/// name through `"*%s"` (`0x00556b24`) instead of copying it plainly. So a game in the multiplayer
/// list whose host build does not match yours is shown with a **leading asterisk**, and one that
/// matches is not.
///
/// **Only half of this tag is comparable to what the game shows.** The executable sum is exact, so
/// that half must match the game's display. The script half is the accumulator over a member set
/// the engine may not load (see the module notes), so the tool's number will very likely differ
/// from the game's and that is expected rather than a fault in either. What transfers across the
/// two is the *comparison*: two installs showing the same tag in the game list agree on both
/// halves, and two showing different tags disagree on at least one.
pub fn session_tag(script_checksum: i32, exe_checksum: u32) -> String {
    format!("cksum={script_checksum},{exe_checksum}")
}

/// Whether two byte strings are the same, and if not, how they were told apart.
///
/// A checksum comparison can only ever say "these sums differ". An exact comparison can say
/// "these bytes differ", which is the claim a person acting on a pre-flight check needs. Keeping
/// the two verdicts in one type stops a caller reporting a sum match as a content match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentVerdict {
    /// Same length and same bytes.
    Identical,
    /// Different lengths, so necessarily different content.
    DifferentLength { left: usize, right: usize },
    /// Same length, first difference at this byte offset.
    DifferentAt { offset: usize },
}

impl ContentVerdict {
    pub fn compare(left: &[u8], right: &[u8]) -> Self {
        if left.len() != right.len() {
            return Self::DifferentLength {
                left: left.len(),
                right: right.len(),
            };
        }
        match left.iter().zip(right).position(|(a, b)| a != b) {
            Some(offset) => Self::DifferentAt { offset },
            None => Self::Identical,
        }
    }

    pub fn is_identical(&self) -> bool {
        matches!(self, Self::Identical)
    }

    /// A one-line description, so a report and a reader never phrase the same verdict differently.
    pub fn describe(&self) -> String {
        match self {
            Self::Identical => String::from("identical"),
            Self::DifferentLength { left, right } => {
                format!("different length ({left} vs {right} bytes)")
            }
            Self::DifferentAt { offset } => format!("same length, first difference at {offset:#x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_checksums_agree_on_bytes_below_0x80_and_this_is_why_a_guess_survives_a_weak_test() {
        // Any test vector made of ASCII cannot tell the two algorithms apart. That is the trap
        // this test exists to name, not a property worth relying on.
        let ascii = b"MOVE_ARMY";
        assert_eq!(exe_checksum(ascii) as i32, script_checksum(ascii));
    }

    #[test]
    fn the_two_checksums_disagree_on_a_high_byte() {
        // 0x80 contributes +128 zero-extended and -128 sign-extended: a 256 gap per high byte.
        assert_eq!(exe_checksum(&[0x80]), 128);
        assert_eq!(script_checksum(&[0x80]), -128);
        assert_eq!(exe_checksum(&[0xff]), 255);
        assert_eq!(script_checksum(&[0xff]), -1);
        // And the gap accumulates, so it is not a sign-of-the-result artefact.
        let three_high = [0xff_u8, 0xfe, 0x80];
        assert_eq!(exe_checksum(&three_high), 255 + 254 + 128);
        assert_eq!(script_checksum(&three_high), -1 + -2 + -128);
    }

    #[test]
    fn both_checksums_wrap_through_the_public_function_rather_than_saturating() {
        // This test used to fold `wrapping_add` in the test body and then call the public function
        // on four bytes. That exercised std, not the implementation: four bytes cannot overflow, so
        // a saturating or a checked implementation passed it. Overflowing for real needs an input
        // big enough to pass the bound, which is ~16 MB, and that is the price of the test meaning
        // anything. The buffers are built and dropped one at a time to keep the peak modest.
        //
        // 255 * 16_843_010 == 2^32 + 254, so a wrapping sum is 254 and a saturating one is
        // u32::MAX. A debug build of a plain `+` would panic here instead.
        {
            let high = vec![0xff_u8; 16_843_010];
            assert_eq!(exe_checksum(&high), 254);
            assert_ne!(exe_checksum(&high), u32::MAX);
        }
        // 127 * 16_909_321 == i32::MAX + 120, so a wrapping sum lands 120 past the top, at
        // i32::MIN + 119. Sign-extension is irrelevant here -- 0x7f is positive either way -- so
        // this isolates the wrap from the extension that the high-byte test covers.
        {
            let positive = vec![0x7f_u8; 16_909_321];
            assert_eq!(script_checksum(&positive), -2_147_483_529);
            assert!(
                script_checksum(&positive) < 0,
                "a saturating sum would stay at i32::MAX"
            );
        }
    }

    #[test]
    fn the_script_checksum_matches_an_independent_formulation() {
        // A cross-check that does not share the fold: accumulate the unsigned sum and correct it
        // by 256 for every byte with the high bit set, which is what sign extension costs.
        let sample: Vec<u8> = (0..=255_u8).chain(0..=255_u8).rev().collect();
        let high_bytes = sample.iter().filter(|byte| **byte & 0x80 != 0).count() as i64;
        let unsigned: i64 = sample.iter().map(|byte| i64::from(*byte)).sum();
        let independent = (unsigned - 256 * high_bytes) as i32;
        assert_eq!(script_checksum(&sample), independent);
        // The independent formulation must also *disagree* with the unsigned sum, or it would be
        // testing nothing.
        assert_ne!(independent, unsigned as i32);
    }

    #[test]
    fn continuing_a_script_checksum_equals_summing_the_concatenation() {
        // The engine accumulates into one global across many buffers; a caller that sums members
        // separately must get the same answer or the whole comparison is invalid.
        let first = [0x00_u8, 0x7f, 0x80, 0xff];
        let second = [0x41_u8, 0xc3, 0x01];
        let concatenated: Vec<u8> = first.iter().chain(&second).copied().collect();
        let stepwise = script_checksum_continued(script_checksum(&first), &second);
        assert_eq!(stepwise, script_checksum(&concatenated));
    }

    #[test]
    fn an_empty_buffer_contributes_nothing() {
        assert_eq!(exe_checksum(&[]), 0);
        assert_eq!(script_checksum(&[]), 0);
    }

    #[test]
    fn content_comparison_reports_how_it_told_the_two_apart() {
        assert_eq!(
            ContentVerdict::compare(b"abc", b"abc"),
            ContentVerdict::Identical
        );
        assert_eq!(
            ContentVerdict::compare(b"abc", b"abcd"),
            ContentVerdict::DifferentLength { left: 3, right: 4 }
        );
        assert_eq!(
            ContentVerdict::compare(b"abc", b"abd"),
            ContentVerdict::DifferentAt { offset: 2 }
        );
    }

    /// The three installs on the development machine, if they are present.
    ///
    /// Not a fixture: three real installs of the same game whose archives are known to differ in
    /// exactly one file. A fixture shaped to look like this could not fail on the thing that
    /// matters -- that the reproduction distinguishes installs a human would call incompatible.
    const INSTALLS: [&str; 3] = [
        "/Users/jakebliss/Applications/Steambuild 32 64bit DXVK.app",
        "/Users/jakebliss/Applications/Lords of Magic 3.02.app",
        "/Users/jakebliss/Applications/Lords of Magic GS5R3.app",
    ];

    const GAME_SUBPATH: &str = "Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/\
steamapps/common/Lords of Magic Special Edition/English";

    /// Validate the reproduction against three real installs.
    ///
    /// **This check is environment-dependent and skips when the installs are absent**, which means
    /// it cannot fail on a machine without the game. That is a real weakness by this repo's
    /// standards and it is why the algorithm itself is covered by synthetic tests above that always
    /// run. What this adds is the one thing synthetic tests cannot: that the reproduction separates
    /// installs a person would call incompatible, and joins ones they would call compatible.
    ///
    /// Predictions, from the algorithms recovered at the addresses in the module docs:
    ///
    /// - `'EXE' version` must be **equal** for all three pairs, because the executables are
    ///   byte-identical.
    /// - `imp.mpq` must be **identical** for all three pairs.
    /// - the script archives must **differ** for all three pairs, and the reproduced accumulator
    ///   must differ too -- a sum that collides would make the check useless on exactly the case
    ///   it exists for.
    #[test]
    fn the_reproduction_separates_the_three_real_installs() {
        let present: Vec<std::path::PathBuf> = INSTALLS
            .iter()
            .map(|app| std::path::Path::new(app).join(GAME_SUBPATH))
            .filter(|directory| directory.join("lomse.exe").is_file())
            .collect();
        if present.len() < 2 {
            eprintln!(
                "skipping: needs at least two of the three installs under ~/Applications, found {}",
                present.len()
            );
            return;
        }

        let mut executables = Vec::new();
        let mut imp_archives = Vec::new();
        let mut script_sums = Vec::new();
        let mut script_bytes = Vec::new();
        for directory in &present {
            let exe = std::fs::read(directory.join("lomse.exe")).expect("read lomse.exe");
            executables.push(exe_checksum(&exe));
            imp_archives.push(std::fs::read(directory.join("imp.mpq")).expect("read imp.mpq"));

            let archive =
                crate::mpq::Archive::open(&directory.join("gs.mpq")).expect("open gs.mpq");
            let mut total = 0_i32;
            let mut all = Vec::new();
            for entry in archive.entries().expect("enumerate gs.mpq") {
                if entry.name == "(listfile)" {
                    continue;
                }
                let bytes = archive.read(&entry.name).expect("read archive member");
                total = script_checksum_continued(total, &bytes);
                all.push((entry.name, bytes));
            }
            script_sums.push(total);
            script_bytes.push(all);
        }

        for left in 0..present.len() {
            for right in (left + 1)..present.len() {
                let pair = format!(
                    "{} vs {}",
                    present[left].display(),
                    present[right].display()
                );
                assert_eq!(
                    executables[left], executables[right],
                    "'EXE' version must match: the executables are byte-identical ({pair})"
                );
                assert!(
                    ContentVerdict::compare(&imp_archives[left], &imp_archives[right])
                        .is_identical(),
                    "imp.mpq must be identical ({pair})"
                );
                assert_ne!(
                    script_bytes[left], script_bytes[right],
                    "the script archives must differ ({pair})"
                );
                assert_ne!(
                    script_sums[left], script_sums[right],
                    "the reproduced accumulator must differ for archives that differ, or the \
                     check is useless on the case it exists for ({pair})"
                );
            }
        }
    }

    #[test]
    fn the_session_tag_puts_the_script_checksum_first() {
        // The argument order is the whole content of this test. Getting it backwards would make
        // every comparison against the game's own display silently wrong, and both values are
        // plain decimal integers so nothing else would give it away.
        assert_eq!(session_tag(-5, 7), "cksum=-5,7");
        assert_ne!(session_tag(-5, 7), session_tag(7, 5));
    }

    #[test]
    fn a_checksum_match_is_not_a_content_match() {
        // Two different byte strings with the same sum. If a caller ever reports a sum match as a
        // content match, this is the case that makes it wrong.
        let left = [0x01_u8, 0x02];
        let right = [0x02_u8, 0x01];
        assert_eq!(exe_checksum(&left), exe_checksum(&right));
        assert_eq!(script_checksum(&left), script_checksum(&right));
        assert!(!ContentVerdict::compare(&left, &right).is_identical());
    }
}
