//! Bounded native reader for the detailed Arrow export.

use crate::decode::{decode_range, row_count, Layout};
use crate::error::IvtError;
use crate::io::SharedBytes;
use crate::tidy::TidySchema;
use arrow::datatypes::SchemaRef;
use arrow::error::ArrowError;
use arrow::record_batch::{RecordBatch, RecordBatchReader};
use std::sync::Arc;

pub struct BatchReader {
    pub data: SharedBytes,
    pub layout: Layout,
    pub maps: Arc<TidySchema>,
    pub batch_size: usize,
    pub position: usize,
    pub total: usize,
}

/// Include input, member maps, output and transient buffers, with headroom
/// for Arrow/Python and consumers retaining the preceding batch.
pub fn preflight(
    rows: usize,
    bytes_per_row: usize,
    fixed: usize,
    limit: usize,
) -> Result<(), IvtError> {
    let estimated = rows
        .checked_mul(bytes_per_row)
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(fixed));
    if estimated.is_none_or(|n| n > limit) {
        return Err(IvtError::resource_limit(
            format!(
                "memory estimate for {rows} rows exceeds max_memory_bytes={limit} (estimated {} bytes)",
                estimated.map_or_else(|| "more than addressable memory".into(), |n| n.to_string())
            ),
            estimated.map(|n| n as u64),
            Some(limit as u64),
        ));
    }
    Ok(())
}

impl BatchReader {
    pub fn new(
        data: SharedBytes,
        layout: Layout,
        maps: Arc<TidySchema>,
        limit: usize,
    ) -> Result<Self, IvtError> {
        const TARGET_ROWS: usize = 1_000_000;
        let total = row_count(&layout)?;
        // Prefer boundaries along the outermost dimension. Split an oversized
        // slab within that dimension so the target remains a real bound.
        let slab = total / layout.counts[0] as usize;
        let batch_size = if slab <= TARGET_ROWS {
            TARGET_ROWS / slab * slab
        } else {
            TARGET_ROWS
        };
        let fixed = data
            .len()
            .saturating_mul(3)
            .saturating_add(maps.dictionary_bytes);
        preflight(batch_size.min(total), maps.bytes_per_row, fixed, limit)?;
        Ok(Self {
            data,
            layout,
            maps,
            batch_size,
            position: 0,
            total,
        })
    }
}

impl BatchReader {
    /// Next batch with its typed error; the Arrow iterator wraps this.
    pub fn next_batch(&mut self) -> Option<Result<RecordBatch, IvtError>> {
        if self.position >= self.total {
            return None;
        }
        let end = self
            .position
            .saturating_add(self.batch_size)
            .min(self.total);
        let result = decode_range(&self.data, &self.layout, self.position, end)
            .and_then(|cells| self.maps.record_batch(cells));
        self.position = if result.is_ok() { end } else { self.total };
        Some(result)
    }
}

impl Iterator for BatchReader {
    type Item = Result<RecordBatch, ArrowError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_batch()
            .map(|result| result.map_err(|e| ArrowError::ParseError(e.to_string())))
    }
}

impl RecordBatchReader for BatchReader {
    fn schema(&self) -> SchemaRef {
        Arc::clone(&self.maps.schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn estimates_reject_overflow_and_small_budgets() {
        assert!(preflight(usize::MAX, 8, 1, usize::MAX).is_err());
        assert!(preflight(1_000_000, 200, 4096, 1024).is_err());
        assert!(preflight(100, 200, 4096, 100_000).is_ok());
    }
}
