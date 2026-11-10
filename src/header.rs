//! Fixed header offsets and page-directory anchoring.

use crate::bytes::{u16, u32};

pub const HDR_DESCRIPTOR_PTR: usize = 32;
pub const HDR_TITLE_FR_PTR: usize = 40;
pub const HDR_TITLE_EN_PTR: usize = 48;
pub const HDR_MASTER_PTR: usize = 544;
pub const HDR_DIR_PTR: usize = 558;
pub const HDR_CODEBOOK_PTR: usize = 572;
pub const HDR_STATUS_LEGEND: usize = 698;
pub const HDR_DIM_SLOT0: usize = 824;
pub const DIM_SLOT_STRIDE: usize = 14;

pub const MARKER_WIDTHS: [u8; 3] = [2, 4, 8];
#[derive(Debug, Clone)]
pub struct HeaderLayout {
    pub version: String,
    #[allow(dead_code)]
    pub descriptor: u32,
    #[allow(dead_code)]
    pub title_en: u32,
    #[allow(dead_code)]
    pub title_fr: u32,
    #[allow(dead_code)]
    pub field_count: u32,
    #[allow(dead_code)]
    pub directory_lo: u16,
    #[allow(dead_code)]
    pub codebook: u32,
    #[allow(dead_code)]
    pub eof: usize,
    pub signature: [u8; 4],
}

#[derive(Debug, Clone, Copy)]
pub struct DimSlot {
    #[allow(dead_code)]
    pub index: usize,
    #[allow(dead_code)]
    pub alloc: u32,
    pub n_entries: u32,
    #[allow(dead_code)]
    pub flag: u16,
}

pub fn read_header(data: &[u8]) -> Result<HeaderLayout, String> {
    read_header_ex(data, data.len())
}

/// Like [`read_header`], but `file_len` may exceed `data.len()` when only a prefix is loaded.
pub fn read_header_ex(data: &[u8], file_len: usize) -> Result<HeaderLayout, String> {
    if data.len() < 576 {
        return Err("IVT file too small to contain a header".into());
    }
    let container_version = u16(data, 0);
    if data[2..4] == [0x20, 0] && ![2, 4].contains(&container_version) {
        return Err(format!(
            "unsupported IVT container version {}; supported versions are 2 and 4",
            container_version
        ));
    }
    if data[2..4] != [0x20, 0] {
        return Err(format!("unrecognised IVT signature: {:02x?}", &data[..4]));
    }
    for (offset, expected) in [(8, 0x40), (10, 8), (16, 0x20), (18, 14), (20, 0x1000)] {
        let actual = u16(data, offset);
        if actual != expected {
            return Err(format!(
                "invalid IVT header at byte {offset}: expected {expected}, got {actual}"
            ));
        }
    }
    let slots = slot_table_range(data).ok_or("invalid IVT slot table range")?;
    if slots.start < 32 {
        return Err(format!("invalid IVT slot table offset: {}", slots.start));
    }
    if slots.end > file_len {
        return Err(format!(
            "IVT slot table extends past EOF: expected bytes {}..{}, file_len={file_len}",
            slots.start, slots.end
        ));
    }
    let fr = u32(data, HDR_TITLE_FR_PTR);
    let en = u32(data, HDR_TITLE_EN_PTR);
    let legacy = en != 0 || fr != 0;
    let mut signature = [0u8; 4];
    signature.copy_from_slice(&data[..4]);
    Ok(HeaderLayout {
        version: if legacy {
            "legacy".into()
        } else {
            "modern".into()
        },
        descriptor: u32(data, HDR_DESCRIPTOR_PTR),
        title_en: en,
        title_fr: fr,
        field_count: slot_offset(data, HDR_MASTER_PTR).map_or(0, |slot| u32(data, slot + 8)),
        directory_lo: slot_offset(data, HDR_DIR_PTR).map_or(0, |slot| u16(data, slot)),
        codebook: slot_offset(data, HDR_CODEBOOK_PTR).map_or(0, |slot| u32(data, slot)),
        eof: file_len,
        signature,
    })
}

pub(crate) fn slot_table_range(data: &[u8]) -> Option<std::ops::Range<usize>> {
    if data.len() < 20 {
        return None;
    }
    let start = u32(data, 12) as usize;
    let length = usize::from(u16(data, 16)).checked_mul(DIM_SLOT_STRIDE)?;
    Some(start..start.checked_add(length)?)
}

/// Translate a slot's conventional header position to its declared file position.
/// Directory readers receive physical offsets; translation happens at callers.
pub(crate) fn slot_offset(data: &[u8], conventional: usize) -> Option<usize> {
    let relative = conventional.checked_sub(HDR_MASTER_PTR)?;
    if relative % DIM_SLOT_STRIDE != 0 {
        return None;
    }
    let table = slot_table_range(data)?;
    let offset = table.start.checked_add(relative)?;
    let end = offset.checked_add(DIM_SLOT_STRIDE)?;
    (end <= table.end && end <= data.len()).then_some(offset)
}

pub fn dim_slots(data: &[u8], max_dims: usize) -> Vec<DimSlot> {
    let mut out = Vec::new();
    let Some(slots) = slot_table_range(data) else {
        return out;
    };
    let dimension_start = slots.start + 20 * DIM_SLOT_STRIDE;
    for k in 0..max_dims.min(usize::from(u16(data, 16)).saturating_sub(20)) {
        let base = dimension_start + DIM_SLOT_STRIDE * k;
        if base + DIM_SLOT_STRIDE > slots.end.min(data.len()) {
            break;
        }
        let dir_ptr = u32(data, base);
        let alloc = u32(data, base + 4);
        let n_entries = u32(data, base + 8);
        let flag = u16(data, base + 12);
        if dir_ptr == 0 && alloc == 0 && n_entries == 0 {
            break;
        }
        out.push(DimSlot {
            index: k,
            alloc,
            n_entries,
            flag,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(table_offset: usize, slot_count: u16) -> Vec<u8> {
        let mut data = vec![0; table_offset + usize::from(slot_count) * DIM_SLOT_STRIDE + 64];
        data[12..16].copy_from_slice(&(table_offset as u32).to_le_bytes());
        data[16..18].copy_from_slice(&slot_count.to_le_bytes());
        data
    }

    #[test]
    fn dimensions_stop_at_declared_slot_boundary() {
        // Bytes immediately following the table look like an occupied slot.
        // They belong to another file region and must never become an axis.
        for offset in [544, 1200] {
            let mut data = header(offset, 32);
            for index in 20..=32 {
                let at = offset + index * DIM_SLOT_STRIDE;
                data[at..at + 4].copy_from_slice(&100u32.to_le_bytes());
                data[at + 8..at + 12].copy_from_slice(&1u32.to_le_bytes());
            }
            let slots = dim_slots(&data, 32);
            assert_eq!(slots.len(), 12);
            assert_eq!(slots.last().unwrap().index, 11);
        }
    }

    #[test]
    fn dimensions_obey_declared_count_even_if_more_bytes_exist() {
        let mut data = header(544, 21);
        data[824..828].copy_from_slice(&100u32.to_le_bytes());
        data[838..842].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(dim_slots(&data, 32).len(), 1);
        assert!(dim_slots(&data[..830], 32).is_empty());
    }

    #[test]
    fn slot_positions_use_the_declared_table_and_bound() {
        let data = header(1200, 32);
        assert_eq!(slot_offset(&data, HDR_MASTER_PTR), Some(1200));
        assert_eq!(slot_offset(&data, HDR_DIR_PTR), Some(1214));
        assert_eq!(slot_offset(&data, HDR_DIM_SLOT0 + 11 * 14), Some(1634));
        assert_eq!(slot_offset(&data, HDR_DIM_SLOT0 + 12 * 14), None);
        assert_eq!(slot_offset(&data, HDR_MASTER_PTR + 1), None);
        assert_eq!(slot_offset(&data[..1205], HDR_MASTER_PTR), None);
    }
}
