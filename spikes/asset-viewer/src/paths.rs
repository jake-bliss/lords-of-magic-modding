//! Filesystem identity checks the writers share.
//!
//! Lives in the library rather than in `main.rs` because the loose `map/` directory has no backup
//! and there is now more than one writer: the CLI's one-file-per-process verbs and the editor
//! server's Save As. A guard that only one of them can reach is a guard the other one will be
//! written without.

use std::fs;
use std::path::Path;

/// Whether two paths name the same file on disk.
///
/// Compares the **device and inode**, not canonical path strings. String comparison already caught
/// `map/URAK.scn` versus `./map/../map/URAK.scn` and the macOS case-only variant, but it answers
/// `false` for two hardlinks to one inode -- which is the same file by every meaning that matters
/// to a writer. No overwrite is reachable through that gap, because `create_new` refuses an
/// existing output whatever it is linked to; the function simply did not do what its name said,
/// and a guard whose contract is wider than its implementation is how the next caller gets
/// surprised.
///
/// An output that does not exist yet has no metadata to read, and that is the normal case -- it is
/// also, by definition, not the input.
pub fn paths_are_same_file(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}
