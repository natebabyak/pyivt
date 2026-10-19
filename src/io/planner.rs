//! Range planning and coalescing for selective IVT reads.

use crate::error::IvtError;
use crate::io::read_at::ReadAt;

/// Default gap (bytes) below which adjacent ranges are merged.
pub const DEFAULT_MERGE_GAP: u64 = 64 * 1024;

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
pub fn coalesce_ranges(ranges: &[ByteRange], merge_gap: u64, file_len: u64) -> Result<Vec<ByteRange>, IvtError> {
    let mut sorted: Vec<ByteRange> = Vec::new();
    for r in ranges {
        if r.length == 0 {
            continue;
        }
        let end = r
            .end()
            .ok_or(IvtError::InvalidPointer {
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
pub fn slice_loaded<'a>(
    loaded: &'a [LoadedRange],
    file_offset: u64,
    length: usize,
) -> Option<&'a [u8]> {
    for lr in loaded {
        if let Some(s) = lr.slice_at(file_offset, length) {
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::read_at::MemoryReadAt;

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
}
