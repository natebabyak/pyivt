//! Native slot directories: flat entries or an index of entry pages.
//! Data and metadata slots use the same addressing representation.

use crate::bytes::{u16, u32};
use crate::error::IvtError;
use crate::io::IvtSource;

const HEADER_PAGE_ENTRIES: usize = 20;
const ENTRY_SIZE: usize = 8;

pub(crate) struct MetadataDirectory {
    pub(crate) offset: usize,
    pub(crate) count: usize,
    page_entries: Option<usize>,
}

impl MetadataDirectory {
    pub(crate) fn new(header: &[u8], slot: usize) -> Result<Self, IvtError> {
        let raw = header
            .get(
                slot..slot
                    .checked_add(14)
                    .ok_or(IvtError::InvalidDirectory("metadata slot"))?,
            )
            .ok_or(IvtError::InvalidDirectory("metadata slot"))?;
        let page_entries = if u16(raw, 12) & 1 != 0 {
            let raw_size = header
                .get(HEADER_PAGE_ENTRIES..HEADER_PAGE_ENTRIES + 2)
                .ok_or(IvtError::InvalidDirectory("metadata page size"))?;
            let size = usize::from(u16(raw_size, 0));
            if size == 0 {
                return Err(IvtError::InvalidDirectory("metadata page size"));
            }
            Some(size)
        } else {
            None
        };
        Ok(Self {
            offset: usize::try_from(u32(raw, 0))
                .map_err(|_| IvtError::InvalidDirectory("directory offset overflow"))?,
            count: usize::try_from(u32(raw, 8))
                .map_err(|_| IvtError::InvalidDirectory("directory count overflow"))?,
            page_entries,
        })
    }

    fn range(
        offset: usize,
        length: usize,
        file_len: usize,
    ) -> Result<std::ops::Range<usize>, IvtError> {
        let end = offset
            .checked_add(length)
            .filter(|&end| offset != 0 && end <= file_len)
            .ok_or(IvtError::InvalidDirectory("metadata directory range"))?;
        Ok(offset..end)
    }

    fn byte_len(&self) -> Result<usize, IvtError> {
        self.count
            .checked_mul(ENTRY_SIZE)
            .ok_or(IvtError::InvalidDirectory("metadata directory size"))
    }

    /// Resolve an ordinal without compacting holes or reading unused capacity.
    pub(crate) fn entry_offset(&self, data: &[u8], index: usize) -> Result<usize, IvtError> {
        if self.offset == 0 || index >= self.count {
            return Err(IvtError::InvalidDirectory("metadata entry index"));
        }
        let (base, within) = if let Some(size) = self.page_entries {
            let pointer = (index / size)
                .checked_mul(4)
                .and_then(|n| self.offset.checked_add(n))
                .ok_or(IvtError::InvalidDirectory("metadata page pointer"))?;
            let raw = &data[Self::range(pointer, 4, data.len())?];
            (
                usize::try_from(u32(raw, 0))
                    .map_err(|_| IvtError::InvalidDirectory("directory page offset overflow"))?,
                index % size,
            )
        } else {
            (self.offset, index)
        };
        if base == 0 {
            return Err(IvtError::InvalidDirectory("metadata page pointer"));
        }
        let offset = within
            .checked_mul(ENTRY_SIZE)
            .and_then(|n| base.checked_add(n))
            .ok_or(IvtError::InvalidDirectory("metadata entry offset"))?;
        Self::range(offset, ENTRY_SIZE, data.len())?;
        Ok(offset)
    }

    pub(crate) fn entry<'a>(&self, data: &'a [u8], index: usize) -> Result<&'a [u8], IvtError> {
        let offset = self.entry_offset(data, index)?;
        Ok(&data[offset..offset + ENTRY_SIZE])
    }

    /// Flatten only the declared entries for the selective metadata catalog.
    pub(crate) fn read(&self, source: &IvtSource) -> Result<Vec<u8>, IvtError> {
        let file_len = usize::try_from(source.len()?)
            .map_err(|_| IvtError::InvalidDirectory("metadata file size"))?;
        let length = self.byte_len()?;
        if self.count == 0 || self.offset == 0 {
            return Ok(Vec::new());
        }
        let Some(size) = self.page_entries else {
            Self::range(self.offset, length, file_len)?;
            return source.read_range(self.offset as u64, length);
        };
        // Bound the output before allocating, even for malformed counts.
        if length > file_len {
            return Err(IvtError::InvalidDirectory("metadata directory size"));
        }
        let pointer_length = self
            .count
            .div_ceil(size)
            .checked_mul(4)
            .ok_or(IvtError::InvalidDirectory("metadata page index size"))?;
        Self::range(self.offset, pointer_length, file_len)?;
        let pointers = source.read_range(self.offset as u64, pointer_length)?;
        let mut entries = Vec::with_capacity(length);
        for (page, raw) in pointers.as_chunks::<4>().0.iter().enumerate() {
            let offset = u32(raw, 0) as usize;
            let count = size.min(self.count - page * size);
            let length = count * ENTRY_SIZE;
            Self::range(offset, length, file_len)?;
            entries.extend_from_slice(&source.read_range(offset as u64, length)?);
        }
        Ok(entries)
    }
}

pub(crate) fn record(data: &[u8], slot: usize, index: usize) -> Option<&[u8]> {
    let directory = MetadataDirectory::new(data, slot).ok()?;
    let raw = directory.entry(data, index).ok()?;
    let offset = u32(raw, 0) as usize;
    let length = u16(raw, 4) as usize;
    if offset == 0 || length == 0 {
        return None;
    }
    data.get(offset..offset.checked_add(length)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOT: usize = 32;
    const PAGE_SIZE: usize = 4096;

    fn paged_fixture() -> Vec<u8> {
        // A hole and a partial last page. The final page
        // ends at EOF so reading allocation capacity instead of count fails.
        let mut data = vec![0; 256 + PAGE_SIZE * 8 + 16];
        data[20..22].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
        data[SLOT..SLOT + 4].copy_from_slice(&64u32.to_le_bytes());
        data[SLOT + 4..SLOT + 8].copy_from_slice(&(2 * PAGE_SIZE as u32).to_le_bytes());
        data[SLOT + 8..SLOT + 12].copy_from_slice(&(PAGE_SIZE as u32 + 2).to_le_bytes());
        data[SLOT + 12..SLOT + 14].copy_from_slice(&1u16.to_le_bytes());
        data[64..68].copy_from_slice(&256u32.to_le_bytes());
        data[68..72].copy_from_slice(&(256 + PAGE_SIZE as u32 * 8).to_le_bytes());
        data[80..85].copy_from_slice(&[1, 1, 1, 0, b'A']);
        for i in [0, PAGE_SIZE - 1, PAGE_SIZE, PAGE_SIZE + 1] {
            let at = 256 + i * 8;
            data[at..at + 4].copy_from_slice(&80u32.to_le_bytes());
            data[at + 4..at + 8].copy_from_slice(&[5, 0, 8, 0]);
        }
        data
    }

    #[test]
    fn paged_ordinals_match_flat_directory_across_page_boundary() {
        let data = paged_fixture();
        let directory = MetadataDirectory::new(&data, SLOT).unwrap();
        let entries = directory
            .read(&IvtSource::from_bytes(data.clone()))
            .unwrap();
        let mut flat = data.clone();
        let offset = flat.len() as u32;
        flat.extend_from_slice(&entries);
        flat[SLOT..SLOT + 4].copy_from_slice(&offset.to_le_bytes());
        flat[SLOT + 12..SLOT + 14].fill(0);
        let flat_directory = MetadataDirectory::new(&flat, SLOT).unwrap();
        for i in 0..directory.count {
            assert_eq!(
                directory.entry(&data, i).unwrap(),
                flat_directory.entry(&flat, i).unwrap()
            );
            assert_eq!(record(&data, SLOT, i), record(&flat, SLOT, i));
        }
        assert_eq!(record(&data, SLOT, 1), None);
        assert_eq!(record(&data, SLOT, PAGE_SIZE), Some(&data[80..85]));
        assert!(directory.entry(&data, directory.count).is_err());
        assert_eq!(
            flat_directory.read(&IvtSource::from_bytes(flat)).unwrap(),
            entries
        );
    }

    #[test]
    fn malformed_page_indexes_are_rejected() {
        for pointer in [0, u32::MAX, 256 + PAGE_SIZE as u32 * 8 + 9] {
            let mut data = paged_fixture();
            data[68..72].copy_from_slice(&pointer.to_le_bytes());
            let directory = MetadataDirectory::new(&data, SLOT).unwrap();
            assert!(directory.entry(&data, PAGE_SIZE).is_err());
            assert!(directory.read(&IvtSource::from_bytes(data)).is_err());
        }
        let mut data = paged_fixture();
        data[20..22].fill(0);
        assert!(MetadataDirectory::new(&data, SLOT).is_err());
        data[20..22].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
        let truncated_index = data.len() as u32 - 4;
        data[SLOT..SLOT + 4].copy_from_slice(&truncated_index.to_le_bytes());
        let directory = MetadataDirectory::new(&data, SLOT).unwrap();
        assert!(directory.entry(&data, PAGE_SIZE).is_err());
        assert!(directory.read(&IvtSource::from_bytes(data)).is_err());
    }

    #[test]
    fn entry_ranges_reject_truncation_and_arithmetic_overflow() {
        assert!(MetadataDirectory::new(&[0; 40], SLOT).is_err());
        assert!(MetadataDirectory::range(usize::MAX - 3, 8, usize::MAX).is_err());
        assert!(MetadataDirectory::range(1, usize::MAX, usize::MAX).is_err());
        let mut data = paged_fixture();
        data.truncate(256 + PAGE_SIZE * 8 + 7);
        let directory = MetadataDirectory::new(&data, SLOT).unwrap();
        assert!(directory.entry(&data, PAGE_SIZE).is_err());
    }
}
