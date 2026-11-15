//! Positional I/O, range planning, and file-backed IVT sources.

mod planner;
mod read_at;
mod shared;
mod source;

pub use read_at::IoMetrics;
pub use shared::SharedBytes;
pub use source::IvtSource;
