//! CI's explicit data contract; optional local inputs never masquerade as coverage.

use std::path::{Path, PathBuf};

pub fn required() -> bool {
    std::env::var("PYIVT_REQUIRE_DATA").is_ok_and(|s| s == "1")
}

pub fn files() -> Vec<PathBuf> {
    let root = std::env::var_os("PYIVT_DATA_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("data"),
        PathBuf::from,
    );
    let files = std::fs::read_dir(&root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|s| s == "ivt"))
        .collect::<Vec<_>>();
    assert!(
        !required() || !files.is_empty(),
        "PYIVT_REQUIRE_DATA=1: no IVT fixtures in {}",
        root.display()
    );
    files
}

pub fn find(prefix: &str) -> Option<PathBuf> {
    let result = files()
        .into_iter()
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix));
    assert!(
        !required() || result.is_some(),
        "PYIVT_REQUIRE_DATA=1: missing expected IVT hash prefix {prefix}"
    );
    result
}
