//! Positional I/O, range planning, and file-backed IVT sources.

mod planner;
mod read_at;
mod source;

pub use planner::{ByteRange, LoadedRange, DEFAULT_MERGE_GAP};
#[allow(unused_imports)]
pub use planner::coalesce_ranges;
#[allow(unused_imports)]
pub use read_at::{FileReadAt, InstrumentedReadAt, MemoryReadAt, ReadAt};
pub use read_at::IoMetrics;
pub use source::IvtSource;
