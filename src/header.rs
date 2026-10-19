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
pub const MARKER_B3: [u8; 7] = [0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E];
pub const DIR_LEAD_BLANK_MAX: usize = 1024;

const MARKER_B0: [u8; 6] = [0x82, 0x84, 0x88, 0xA2, 0xA4, 0xA8];

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
pub struct DirEntry {
    pub off: u32,
    pub size: u16,
    pub marker: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DimSlot {
    #[allow(dead_code)]
    pub index: usize,
    pub dir_ptr: u32,
    #[allow(dead_code)]
    pub alloc: u32,
    pub n_entries: u32,
    #[allow(dead_code)]
    pub flag: u16,
}

pub fn is_marker(data: &[u8], off: usize) -> bool {
    if off + 4 > data.len() || data[off + 1] != 0x01 {
        return false;
    }
    let b0 = data[off];
    if MARKER_WIDTHS.contains(&b0) {
        return u16(data, off + 2) > 0;
    }
    MARKER_B0.contains(&b0) && MARKER_B3.contains(&data[off + 3])
}

pub fn dir_entry(data: &[u8], o: usize) -> Option<DirEntry> {
    if o + 8 > data.len() {
        return None;
    }
    let off = u32(data, o);
    let s1 = u16(data, o + 4);
    let s2 = u16(data, o + 6);
    if off < 1 || s1 == 0 || s2 == 0 || (off as usize) + 4 > data.len() {
        return None;
    }
    let is02 = data[0] == 0x02;
    if s1 != s2 && !is02 && (s1 as i32 - s2 as i32).abs() > 256 {
        return None;
    }
    let size = s1.min(s2);
    Some(DirEntry {
        off,
        size,
        marker: is_marker(data, off as usize),
    })
}

pub fn entry_valid(data: &[u8], o: usize) -> bool {
    match dir_entry(data, o) {
        Some(e) => e.marker && e.off >= 1024,
        None => false,
    }
}

pub fn entry_blank(data: &[u8], o: usize) -> bool {
    if o + 8 > data.len() {
        return false;
    }
    u32(data, o) == 0 && u16(data, o + 4) == 0 && u16(data, o + 6) == 0
}

pub fn dir_first_entry(data: &[u8], off: usize) -> Option<usize> {
    for j in 0..=DIR_LEAD_BLANK_MAX {
        let o = off + 8 * j;
        if o + 8 > data.len() {
            return None;
        }
        if entry_valid(data, o) {
            return Some(o);
        }
        if !entry_blank(data, o) {
            return None;
        }
    }
    None
}

pub fn dir_anchor_header(data: &[u8]) -> Option<usize> {
    if data.len() < HDR_DIR_PTR + 2 {
        return None;
    }
    let lo = u16(data, HDR_DIR_PTR) as usize;
    if lo == 0 {
        return None;
    }
    let max_k = data.len().saturating_sub(lo) / 65536;
    for k in 0..=max_k {
        let off = lo + k * 65536;
        if entry_valid(data, off) {
            return Some(off);
        }
    }
    for k in 0..=max_k {
        let off = lo + k * 65536;
        if dir_first_entry(data, off).is_some() {
            return Some(off);
        }
    }
    None
}

pub fn idx0(data: &[u8]) -> usize {
    dir_anchor_header(data).unwrap_or(37167)
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

/// Read a block directory from an isolated directory buffer.
/// `file_len` bounds pointed-to blocks; `dir` is raw bytes starting at `ptr`.
pub fn read_block_dir_bytes(dir: &[u8], n_entries: u32, file_len: usize) -> Vec<(u32, u16)> {
    let mut entries = Vec::new();
    let mut o = 0usize;
    for _ in 0..(n_entries as usize + 8) {
        if o + 8 > dir.len() {
            break;
        }
        let off = u32(dir, o);
        let s1 = u16(dir, o + 4);
        let s2 = u16(dir, o + 6);
        if off == 0 && s1 == 0 {
            break;
        }
        let ok = s1 == s2 || s2 == 4 * ((s1 + 3) / 4) || s2 >= s1;
        if !ok || s1 == 0 || (off as usize) + (s1 as usize) > file_len {
            break;
        }
        entries.push((off, s1));
        o += 8;
        if entries.len() >= n_entries as usize {
            break;
        }
    }
    entries
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
            dir_ptr,
            alloc,
            n_entries,
            flag,
        });
    }
    out
}

pub fn read_block_dir(data: &[u8], ptr: u32, n_entries: u32) -> Vec<(u32, u16)> {
    let mut entries = Vec::new();
    let mut o = ptr as usize;
    for _ in 0..(n_entries as usize + 8) {
        if o + 8 > data.len() {
            break;
        }
        let off = u32(data, o);
        let s1 = u16(data, o + 4);
        let s2 = u16(data, o + 6);
        if off == 0 && s1 == 0 {
            break;
        }
        let ok = s1 == s2 || s2 == 4 * ((s1 + 3) / 4) || s2 >= s1;
        if !ok || s1 == 0 || (off as usize) + (s1 as usize) > data.len() {
            break;
        }
        entries.push((off, s1));
        o += 8;
        if entries.len() >= n_entries as usize {
            break;
        }
    }
    entries
}
