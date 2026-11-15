//! Range planning and coalescing for selective IVT reads.
//!
//! Metadata parsing follows pointers (header → slot table → directories →
//! records), so a naive reader issues one positional read per record. The
//! planner instead collects every range one pointer level needs, merges
//! nearby ranges, and loads them in a few large reads. On network storage
//! this turns hundreds of round trips into a handful.

use crate::error::IvtError;
use crate::io::read_at::ReadAt;

/// Ranges closer than this are read together. Over-reading a gap costs far
/// less than another round trip on network storage, while the bound keeps a
/// planned read from pulling in value pages that lie between metadata blocks.
pub const METADATA_MERGE_GAP: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub offset: u64,
    pub length: usize,
}

impl ByteRange {
    pub fn new(offset: u64, length: usize) -> Self {
        Self { offset, length }
    }

    pub fn end(&self) -> Option<u64> {
        self.offset.checked_add(self.length as u64)
    }
}

#[derive(Clone, Debug)]
pub struct LoadedRange {
    pub offset: u64,
    pub data: Vec<u8>,
}

impl LoadedRange {
    pub fn end(&self) -> u64 {
        self.offset + self.data.len() as u64
    }

    pub fn contains(&self, offset: u64, length: usize) -> bool {
        let Some(end) = offset.checked_add(length as u64) else {
            return false;
        };
        offset >= self.offset && end <= self.end()
    }

    pub fn slice_at(&self, file_offset: u64, length: usize) -> Option<&[u8]> {
        if !self.contains(file_offset, length) {
            return None;
        }
        let start = (file_offset - self.offset) as usize;
        Some(&self.data[start..start + length])
    }
}

/// Sort, merge overlapping ranges, and merge gaps below `merge_gap`.
pub fn coalesce_ranges(
    ranges: &[ByteRange],
    merge_gap: u64,
    file_len: u64,
) -> Result<Vec<ByteRange>, IvtError> {
    let mut sorted: Vec<ByteRange> = Vec::new();
    for r in ranges {
        if r.length == 0 {
            continue;
        }
        let end = r.end().ok_or(IvtError::InvalidPointer {
            offset: r.offset,
            length: r.length,
            file_len,
        })?;
        if end > file_len {
            return Err(IvtError::InvalidPointer {
                offset: r.offset,
                length: r.length,
                file_len,
            });
        }
        sorted.push(*r);
    }
    if sorted.is_empty() {
        return Ok(sorted);
    }
    sorted.sort_by_key(|r| r.offset);

    let mut out: Vec<ByteRange> = Vec::new();
    let mut cur = sorted[0];
    for next in sorted.into_iter().skip(1) {
        let cur_end = cur.end().unwrap();
        let merge_limit = cur_end.saturating_add(merge_gap);
        if next.offset <= merge_limit {
            let next_end = next.end().unwrap();
            let new_end = cur_end.max(next_end);
            cur.length = (new_end - cur.offset) as usize;
        } else {
            out.push(cur);
            cur = next;
        }
    }
    out.push(cur);
    Ok(out)
}

/// Execute coalesced reads and return loaded buffers.
pub fn load_ranges<R: ReadAt + ?Sized>(
    reader: &R,
    ranges: &[ByteRange],
    merge_gap: u64,
) -> Result<Vec<LoadedRange>, IvtError> {
    let file_len = reader.len()?;
    let merged = coalesce_ranges(ranges, merge_gap, file_len)?;
    let mut loaded = Vec::with_capacity(merged.len());
    for r in merged {
        let data = reader.read_range(r.offset, r.length)?;
        loaded.push(LoadedRange {
            offset: r.offset,
            data,
        });
    }
    Ok(loaded)
}

/// Look up `length` bytes at `file_offset` across loaded ranges.
#[cfg(test)]
pub fn slice_loaded(loaded: &[LoadedRange], file_offset: u64, length: usize) -> Option<&[u8]> {
    for lr in loaded {
        if let Some(s) = lr.slice_at(file_offset, length) {
            return Some(s);
        }
    }
    None
}

/// A reader that serves planned ranges from memory and everything else from
/// the underlying source.
///
/// Planning is an optimization only: ranges that fall outside the file are
/// left out of the plan, and a failed planned read is dropped, so the
/// unplanned read that follows reports the same error the parser would have
/// seen without planning. Loaded ranges live only as long as this value; no
/// bytes are cached on the source.
pub struct PlannedReader<'a, R: ReadAt + ?Sized> {
    inner: &'a R,
    /// A few large ranges per plan, so lookups scan linearly.
    loaded: Vec<LoadedRange>,
    enabled: bool,
}

impl<'a, R: ReadAt + ?Sized> PlannedReader<'a, R> {
    /// `enabled = false` passes every read through, for sources that are
    /// already in memory where batching would only add copies.
    pub fn new(inner: &'a R, enabled: bool) -> Self {
        Self {
            inner,
            loaded: Vec::new(),
            enabled,
        }
    }

    fn find(&self, offset: u64, length: usize) -> Option<&[u8]> {
        self.loaded
            .iter()
            .find_map(|range| range.slice_at(offset, length))
    }

    /// Load `ranges` (coalesced with `merge_gap`) for later reads. Ranges that
    /// are already loaded, empty, or outside the file are skipped.
    pub fn prefetch(&mut self, ranges: &[ByteRange], merge_gap: u64) {
        if !self.enabled {
            return;
        }
        let Ok(file_len) = self.inner.len() else {
            return;
        };
        let wanted: Vec<ByteRange> = ranges
            .iter()
            .copied()
            .filter(|r| {
                r.length > 0
                    && r.end().is_some_and(|end| end <= file_len)
                    && self.find(r.offset, r.length).is_none()
            })
            .collect();
        // On failure the ranges stay unplanned; the parser's own read reports it.
        let Ok(loaded) = load_ranges(self.inner, &wanted, merge_gap) else {
            return;
        };
        for loaded in loaded {
            // Drop loaded ranges the new one covers.
            self.loaded
                .retain(|r| !(r.offset >= loaded.offset && r.end() <= loaded.end()));
            self.loaded.push(loaded);
        }
    }
}

impl<R: ReadAt + ?Sized> ReadAt for PlannedReader<'_, R> {
    fn len(&self) -> Result<u64, IvtError> {
        self.inner.len()
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> Result<(), IvtError> {
        if let Some(bytes) = self.find(offset, buffer.len()) {
            buffer.copy_from_slice(bytes);
            return Ok(());
        }
        self.inner.read_exact_at(offset, buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::read_at::{InstrumentedReadAt, MemoryReadAt};

    #[test]
    fn merge_overlap_and_gap() {
        let ranges = [
            ByteRange::new(100, 50),
            ByteRange::new(120, 40), // overlap
            ByteRange::new(200, 10), // gap 30 < 64
            ByteRange::new(1000, 5), // far
        ];
        let out = coalesce_ranges(&ranges, 64, 10_000).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], ByteRange::new(100, 110)); // 100..210
        assert_eq!(out[1], ByteRange::new(1000, 5));
    }

    #[test]
    fn reject_past_eof() {
        let ranges = [ByteRange::new(90, 20)];
        assert!(matches!(
            coalesce_ranges(&ranges, 0, 100),
            Err(IvtError::InvalidPointer { .. })
        ));
    }

    #[test]
    fn load_and_slice() {
        let data: Vec<u8> = (0u8..200).collect();
        let reader = MemoryReadAt::new(data);
        let loaded = load_ranges(
            &reader,
            &[ByteRange::new(10, 20), ByteRange::new(50, 10)],
            0,
        )
        .unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(slice_loaded(&loaded, 15, 3), Some(&[15u8, 16, 17][..]));
        assert_eq!(slice_loaded(&loaded, 50, 2), Some(&[50u8, 51][..]));
        assert!(slice_loaded(&loaded, 40, 5).is_none());
    }

    #[test]
    fn planned_reads_hit_prefetched_ranges() {
        let reader = InstrumentedReadAt::new(MemoryReadAt::new((0u8..=255).collect()));
        let mut planned = PlannedReader::new(&reader, true);
        planned.prefetch(
            &[
                ByteRange::new(10, 4),
                ByteRange::new(20, 4),
                ByteRange::new(200, 4),
            ],
            16,
        );
        assert_eq!(reader.metrics().read_ops, 2);
        assert_eq!(
            planned.read_range(12, 10).unwrap(),
            (12u8..22).collect::<Vec<_>>()
        );
        assert_eq!(
            planned.read_range(200, 4).unwrap(),
            vec![200, 201, 202, 203]
        );
        assert_eq!(reader.metrics().read_ops, 2);
        // Unplanned reads fall through to the source.
        assert_eq!(planned.read_range(100, 2).unwrap(), vec![100, 101]);
        assert_eq!(reader.metrics().read_ops, 3);
    }

    #[test]
    fn planning_skips_out_of_bounds_and_loaded_ranges() {
        let reader = InstrumentedReadAt::new(MemoryReadAt::new(vec![7u8; 64]));
        let mut planned = PlannedReader::new(&reader, true);
        planned.prefetch(&[ByteRange::new(60, 10), ByteRange::new(u64::MAX, 2)], 0);
        assert_eq!(reader.metrics().read_ops, 0);
        planned.prefetch(&[ByteRange::new(0, 32)], 0);
        planned.prefetch(&[ByteRange::new(4, 8)], 0);
        assert_eq!(reader.metrics().read_ops, 1);
        // A wider range replaces the one it covers.
        planned.prefetch(&[ByteRange::new(0, 48)], 0);
        assert_eq!(planned.loaded.len(), 1);
        // The parser's own read still reports out-of-bounds pointers.
        assert!(matches!(
            planned.read_range(60, 10),
            Err(IvtError::InvalidPointer { .. })
        ));
    }

    #[test]
    fn disabled_planning_passes_reads_through() {
        let reader = InstrumentedReadAt::new(MemoryReadAt::new(vec![0u8; 64]));
        let mut planned = PlannedReader::new(&reader, false);
        planned.prefetch(&[ByteRange::new(0, 32)], 0);
        assert_eq!(reader.metrics().read_ops, 0);
        planned.read_range(0, 4).unwrap();
        assert_eq!(reader.metrics().read_ops, 1);
    }
}
