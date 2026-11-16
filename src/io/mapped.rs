//! Whole-file loads for value decoding: a read-only memory map, or a full read.
//!
//! Mapping avoids committing a private copy of the file. Exports that select a
//! subset touch only the value pages they decode, and a full export's pages
//! stay reclaimable page cache rather than process memory. The measurements
//! behind this choice are in `docs/benchmarks/mmap-export.md`.
//!
//! # Change-while-open risk
//!
//! A mapping reflects the file as it is now, not as it was when it was mapped.
//! The decoder bounds-checks every offset against the mapped length, but:
//!
//! * On Unix, truncating the file while it is mapped makes later page accesses
//!   past the new end raise `SIGBUS`, which terminates the process.
//! * On Windows, the OS refuses to truncate a mapped file, but in-place writes
//!   by another process become visible through the mapping.
//!
//! Callers must not modify or truncate an IVT file while an export or batch
//! stream that uses it is alive. Set `PYIVT_MMAP=0` to read files fully into
//! memory instead, for example on shares where other clients rewrite files.

use std::fs::File;
use std::path::Path;

use crate::error::IvtError;
use crate::io::SharedBytes;

/// Environment variable that disables memory mapping when set to `0`.
pub const MMAP_ENV: &str = "PYIVT_MMAP";

fn mmap_enabled() -> bool {
    std::env::var_os(MMAP_ENV).is_none_or(|value| value != "0")
}

/// Load the whole file at `path` for decoding.
///
/// The file is mapped only when its current length is still `expected_len`,
/// the length its metadata was parsed against. A file that has already been
/// truncated or replaced, an empty file, or a mapping failure falls back to
/// [`std::fs::read`], which reports the file as it is now.
pub fn load_file(path: &Path, expected_len: u64) -> Result<SharedBytes, IvtError> {
    if mmap_enabled() {
        if let Some(map) = map_file(path, expected_len) {
            return Ok(map);
        }
    }
    Ok(SharedBytes::from(
        std::fs::read(path).map_err(IvtError::Io)?,
    ))
}

fn map_file(path: &Path, expected_len: u64) -> Option<SharedBytes> {
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 || len != expected_len {
        return None;
    }
    // SAFETY: the mapping is read-only and every access is bounds-checked
    // against its length. The remaining hazard is another process changing or
    // truncating the file while it is mapped; see the module documentation.
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    // A truncation between the check above and the mapping shows up here.
    (map.len() as u64 == expected_len).then(|| SharedBytes::from_owner(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("pyivt-mapped-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn maps_file_with_expected_length() {
        let path = temp_file("same", b"ivt bytes");
        assert!(map_file(&path, 9).is_some());
        assert_eq!(&*load_file(&path, 9).unwrap(), b"ivt bytes");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn changed_length_falls_back_to_reading_current_contents() {
        let path = temp_file("shorter", b"short");
        assert!(map_file(&path, 9).is_none());
        assert_eq!(&*load_file(&path, 9).unwrap(), b"short");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn empty_file_is_read_not_mapped() {
        let path = temp_file("empty", b"");
        assert!(map_file(&path, 0).is_none());
        assert!(load_file(&path, 0).unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_file_reports_io_error() {
        let path = std::env::temp_dir().join("pyivt-mapped-does-not-exist.ivt");
        assert!(matches!(load_file(&path, 1), Err(IvtError::Io(_))));
    }
}
