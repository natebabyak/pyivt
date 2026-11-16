//! Positional I/O, range planning, file-backed IVT sources, and whole-file loads.

mod mapped;
mod planner;
mod read_at;
mod shared;
mod source;

pub use mapped::load_file;
pub(crate) use planner::{ByteRange, PlannedReader, METADATA_MERGE_GAP};
pub use read_at::IoMetrics;
pub(crate) use read_at::ReadAt;
pub use shared::{SharedBytes, WeakSharedBytes};
pub use source::IvtSource;
