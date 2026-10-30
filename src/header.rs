//! Fixed header offsets and page-directory anchoring.

use crate::bytes::{u16, u32};

pub const HDR_DESCRIPTOR_PTR: usize = 32;
pub const HDR_TITLE_FR_PTR: usize = 40;
pub const HDR_TITLE_EN_PTR: usize = 48;
pub const HDR_MASTER_PTR: usize = 544;
pub const HDR_GEO_FIELDS: usize = 552;
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
        field_count: u32(data, HDR_GEO_FIELDS),
        directory_lo: u16(data, HDR_DIR_PTR),
        codebook: u32(data, HDR_CODEBOOK_PTR),
        eof: file_len,
        signature,
    })
}

pub fn dim_slots(data: &[u8], max_dims: usize) -> Vec<DimSlot> {
    let mut out = Vec::new();
    for k in 0..max_dims {
        let base = HDR_DIM_SLOT0 + DIM_SLOT_STRIDE * k;
        if base + DIM_SLOT_STRIDE > data.len() {
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
