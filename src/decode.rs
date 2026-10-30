//! Unified page-directory layout and cell decode.

use crate::bytes::{nextpow2, u16, u32};
use crate::descriptor::{dim_slugs, geo_dim_index, DimDesc};
use crate::header::{HDR_DIR_PTR, HDR_MASTER_PTR, MARKER_WIDTHS};
use crate::metadata_directory::MetadataDirectory;
use std::collections::HashSet;

pub const PRES_BITS: i64 = 2048;

#[derive(Debug, Clone)]
pub struct Layout {
    pub counts: Vec<i64>,
    pub slugs: Vec<String>,
    pub n_dim: usize,
    pub geo_dim: usize,
    pub straddle: usize,
    pub ipc: Vec<i64>,
    pub window_count: i64,
    pub inpage_idx: Vec<usize>,
    pub tuples: Vec<Vec<i32>>,
    pub bit: Vec<i32>,
    pub rec_bytes: usize,
    pub ent_idx: Vec<usize>,
    pub ent_counts: Vec<i64>,
    pub estride: Vec<i64>,
    pub geo_in_page: bool,
}

#[cfg(test)]
fn value_trailer(b0: u8, b2: u8, b3: u8) -> Result<usize, String> {
    let w = b0 & 0x0F;
    let hi = b0 & 0xF0;
    if !MARKER_WIDTHS.contains(&w) || (hi != 0x80 && hi != 0xA0) || !(0x08..0x0F).contains(&b3) {
        return Err(format!("unrecognised page marker 0x{b0:02x} .. 0x{b3:02x}"));
    }
    let head = 32 * (b3 as usize - 8);
    if b2 == 0 {
        return Ok(head);
    }
    Ok(2 * (b2 as usize / 16) + 2 * usize::from((b2 & 0x0F) > 0) + head)
}

pub fn bit_layout(counts: &[i64]) -> (Vec<i64>, usize) {
    let k = counts.len();
    if k == 0 {
        return (Vec::new(), 0);
    }
    let mut block = vec![0i64; k];
    block[k - 1] = nextpow2(counts[k - 1]);
    for i in (0..k - 1).rev() {
        block[i] = nextpow2(counts[i] * block[i + 1]);
    }
    let mut stride = vec![0i64; k];
    stride[k - 1] = 1;
    stride[..k - 1].copy_from_slice(&block[1..k]);
    (stride, (block[0] / 8) as usize)
}

fn cell_grid(counts: &[i64], stride: &[i64]) -> (Vec<Vec<i32>>, Vec<i32>) {
    let k = counts.len();
    let n: i64 = if counts.is_empty() {
        0
    } else {
        counts.iter().product()
    };
    if n == 0 {
        return (vec![Vec::new(); k], Vec::new());
    }
    let n = n as usize;
    let mut tuples = vec![vec![0i32; n]; k];
    let mut bits = vec![vec![0i32; n]; k];
    let mut rin: i64 = 1;
    for i in (0..k).rev() {
        let ci = counts[i] as usize;
        let rin_u = rin as usize;
        let tile = n / (ci * rin_u);
        let mut col = Vec::with_capacity(n);
        for _ in 0..tile {
            for idx in 0..ci {
                for _ in 0..rin_u {
                    col.push(idx as i32);
                }
            }
        }
        tuples[i] = col.clone();
        bits[i] = col;
        rin *= counts[i];
    }
    let mut bit = vec![0i32; n];
    for row in 0..n {
        let mut s = 0i32;
        for i in 0..k {
            s += bits[i][row] * stride[i] as i32;
        }
        bit[row] = s;
    }
    for col in &mut tuples {
        for v in col.iter_mut() {
            *v += 1;
        }
    }
    // transpose to row-major list of member-id rows for easier page use
    let mut rows = vec![vec![0i32; k]; n];
    for i in 0..k {
        for row in 0..n {
            rows[row][i] = tuples[i][row];
        }
    }
    (rows, bit)
}

pub fn build_layout(dims: &[DimDesc]) -> Result<Layout, String> {
    if dims.is_empty() || dims.iter().any(|dim| dim.count <= 0) {
        return Err("descriptor must have dimensions with positive item counts".into());
    }
    let cnt: Vec<i64> = dims.iter().map(|d| d.count).collect();
    if cnt.iter().any(|&c| c > i32::MAX as i64) {
        return Err("dimension member count exceeds int32 member IDs".into());
    }
    cnt.iter()
        .try_fold(1usize, |n, &c| n.checked_mul(c as usize))
        .ok_or("dimension product exceeds addressable memory")?;
    let m = cnt.len();
    let gd = geo_dim_index(dims);
    let didx: Vec<usize> = (1..=m).filter(|&i| i != gd).collect();
    let names: Vec<String> = didx.iter().map(|&i| dims[i - 1].name.clone()).collect();
    let mut slugs = vec![String::new(); m];
    slugs[gd - 1] = "geo".into();
    for (i, slug) in didx.iter().zip(dim_slugs(&names)) {
        slugs[i - 1] = slug;
    }
    let mut used: HashSet<String> = [
        "value",
        "status_code",
        "symbol",
        "status",
        "footnote_raw",
        "geo",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (index, slug) in slugs.iter_mut().enumerate() {
        if index == gd - 1 {
            continue;
        }
        if slug.starts_with("footnote_raw_") {
            *slug = format!("dim_{slug}");
        }
        let base = slug.clone();
        let mut suffix = 2;
        while !used.insert(slug.clone()) {
            *slug = format!("{base}_{suffix}");
            suffix += 1;
        }
    }

    let pad: Vec<i64> = cnt.iter().map(|&c| nextpow2(c)).collect();
    let mut blk = 1i64;
    let mut straddle = 1usize;
    let mut inner_block = 1i64;
    for j in (1..=m).rev() {
        let need = nextpow2(pad[j - 1] * blk);
        if need > PRES_BITS || j == 1 {
            straddle = j;
            inner_block = blk;
            break;
        }
        blk = need;
    }

    let ipc_straddle = PRES_BITS / inner_block;
    let win = ((cnt[straddle - 1] as f64) / (ipc_straddle as f64)).ceil() as i64;
    let inpage_idx: Vec<usize> = (straddle..=m).collect();
    let mut ipc: Vec<i64> = inpage_idx.iter().map(|&i| cnt[i - 1]).collect();
    ipc[0] = ipc_straddle;
    let mut ipc_ext: Vec<i64> = inpage_idx.iter().map(|&i| pad[i - 1]).collect();
    ipc_ext[0] = ipc_straddle;
    let (stride, rec_bytes) = bit_layout(&ipc_ext);
    let grid_counts = ipc.clone();
    let (tuples, bit) = cell_grid(&grid_counts, &stride);

    let ws = nextpow2(win);
    let mut ent_idx = vec![straddle];
    let mut ent_counts = vec![win];
    let mut ent_pad = vec![ws];
    if straddle > 1 {
        for j in (1..straddle).rev() {
            ent_idx.push(j);
            ent_counts.push(cnt[j - 1]);
            ent_pad.push(pad[j - 1]);
        }
    }

    let mut estride = Vec::new();
    let mut eb = 1i64;
    for &ep in &ent_pad {
        estride.push(eb);
        eb = ep
            .checked_mul(eb)
            .ok_or("padded data directory size overflow")?;
    }

    Ok(Layout {
        counts: cnt,
        slugs,
        n_dim: m,
        geo_dim: gd,
        straddle,
        ipc,
        window_count: win,
        inpage_idx,
        tuples,
        bit,
        rec_bytes,
        ent_idx,
        ent_counts,
        estride,
        geo_in_page: straddle == gd,
    })
}

struct PageCells {
    vals: Vec<f64>,
    statuses: Vec<i16>,
    footnotes: Vec<Vec<u16>>,
}

/// Decode the native component's zero suppression. Bitmap bits describe stored
/// bytes, not data validity. Brwpro.exe 0x1402b5740 consumes little-endian u16
/// words most-significant-bit first and supplies zero for an unset bit.
pub fn decompress_component(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err("truncated component header".into());
    }
    let width = (data[0] & 15) as usize;
    let rank = data[1] as usize;
    if ![1, 2, 4, 8].contains(&width) || rank > 2 || data[0] & 0x50 != 0 {
        return Err(format!("unsupported component header {:02x?}", &data[..2]));
    }
    let header = 2 + rank * 2;
    if data.len() < header {
        return Err("truncated component extents".into());
    }
    let mut count = 1usize;
    for dim in 0..rank {
        count = count
            .checked_mul(u16(data, 2 + dim * 2) as usize)
            .ok_or("component extent overflow")?;
    }
    let bytes = count
        .checked_mul(width)
        .and_then(|n| n.checked_add(header))
        .ok_or("component size overflow")?;
    // A native directory record has a u16 encoded length. Refuse adversarial
    // standalone extents before expansion, including all-zero compressed data.
    if bytes > 64 * 1024 * 1024 {
        return Err("component expansion exceeds the 64 MiB safety limit".into());
    }
    // Rank-two byte components can declare huge extents. Validate encoded
    // input before allocating its expanded destination.
    if data[0] & 0x80 == 0 {
        return data
            .get(..bytes)
            .map(|b| b.to_vec())
            .ok_or_else(|| "truncated uncompressed component".into());
    }
    let bitmap_len = count.div_ceil(16) * 2;
    let bitmap_end = header
        .checked_add(bitmap_len)
        .ok_or("component size overflow")?;
    if bitmap_end > data.len() {
        return Err("truncated component bitmap".into());
    }
    let stored: usize = (0..count)
        .filter(|&i| u16(data, header + (i / 16) * 2) & (0x8000 >> (i % 16)) != 0)
        .count();
    let encoded_end = bitmap_end
        .checked_add(stored * width)
        .ok_or("component size overflow")?;
    if encoded_end > data.len() {
        return Err("truncated component values".into());
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(bytes)
        .map_err(|_| "component allocation failed")?;
    result.resize(bytes, 0);
    result[..header].copy_from_slice(&data[..header]);
    result[0] &= 0x7f;
    let mut pos = bitmap_end;
    for i in 0..count {
        if u16(data, header + (i / 16) * 2) & (0x8000 >> (i % 16)) != 0 {
            let dest = header + i * width;
            result[dest..dest + width].copy_from_slice(&data[pos..pos + width]);
            pos += width;
        }
    }
    Ok(result)
}

fn missing_mode(data: &[u8]) -> Result<u16, String> {
    let raw = crate::metadata_directory::record(data, HDR_MASTER_PTR, 2)
        .ok_or("missing or truncated table settings")?;
    let decoded = decompress_component(raw)?;
    if decoded.len() < 8 || decoded[..4] != [1, 1, 32, 0] {
        return Err("unsupported table settings component".into());
    }
    let mode = u16(&decoded, 6);
    if ![0, 1, 3].contains(&mode) {
        return Err(format!("invalid missing-value mode {mode}"));
    }
    Ok(mode)
}

fn decode_page(
    data: &[u8],
    off: usize,
    _lay: &Layout,
    size: usize,
    mode: u16,
) -> Result<PageCells, String> {
    let raw = data
        .get(off..off.checked_add(size).ok_or("page size overflow")?)
        .ok_or("truncated page")?;
    let page = decompress_component(raw)?;
    let width = (page[0] & 15) as usize;
    if page[1] != 1 || !MARKER_WIDTHS.contains(&(width as u8)) {
        return Err("data component must be a rank-one signed integer or double array".into());
    }
    let count = u16(&page, 2) as usize;
    // The browser represents an entirely zero page with a zero-length short.
    if count != 0 && count < PRES_BITS as usize {
        return Err(format!("data component contains only {count} cells"));
    }
    let scalar = |cell: usize| -> f64 {
        if count == 0 {
            return 0.0;
        }
        let pos = 4 + cell * width;
        match width {
            2 => i16::from_le_bytes(page[pos..pos + 2].try_into().unwrap()) as f64,
            4 => i32::from_le_bytes(page[pos..pos + 4].try_into().unwrap()) as f64,
            8 => f64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()),
            _ => unreachable!(),
        }
    };
    // Legacy masks record missingness without a legend reason (-1). Positive
    // codes are reserved for explicit reasons in extended page records.
    let mut codes = vec![0i16; PRES_BITS as usize];
    let mut notes = vec![Vec::new(); PRES_BITS as usize];
    let trailer = page.get(4 + PRES_BITS as usize * width..).unwrap_or(&[]);
    if page[0] & 0x20 != 0 {
        if trailer.is_empty() {
            return Err("missing extended page records".into());
        }
        let n = trailer[0] as usize;
        if !(1..=2).contains(&n) || trailer.len() < 1 + 2 * n {
            return Err("invalid extended page record directory".into());
        }
        let mut start = 1 + 2 * n;
        for index in 0..n {
            let size = u16(trailer, 1 + 2 * index) as usize;
            let record = trailer
                .get(start..start.checked_add(size).ok_or("record size overflow")?)
                .ok_or("truncated extended page record")?;
            start += size;
            if record.len() < 2 {
                return Err("truncated extended page record header".into());
            }
            match record[0] {
                1 => {
                    let bits = record[1] as usize;
                    if ![1, 2, 4, 8].contains(&bits) || record.len() < 2 + 256 * bits {
                        return Err("invalid packed missing-value record".into());
                    }
                    for (cell, code) in codes.iter_mut().enumerate() {
                        let shift = 8 - bits - (cell * bits % 8);
                        *code = ((record[2 + cell * bits / 8] >> shift)
                            & ((1u16 << bits) - 1) as u8) as i16;
                    }
                }
                2 => {
                    if record.len() < 4 {
                        return Err("truncated footnote record".into());
                    }
                    let n = u16(record, 2) as usize;
                    if record.len() < 4 + 2 * n {
                        return Err("truncated footnote indices".into());
                    }
                    for i in 0..n {
                        let packed = u16(record, 4 + i * 2);
                        let cell = (packed >> 5) as usize;
                        if cell >= codes.len() {
                            return Err("footnote cell outside page".into());
                        }
                        // Keep the recorded packed annotation reference intact.
                        notes[cell].push(packed);
                    }
                }
                value => return Err(format!("unknown extended page record {value}")),
            }
        }
    } else {
        if !trailer.is_empty() && trailer.len() < 256 {
            return Err("truncated legacy missing-value bitmap".into());
        }
        // Without a mask every stored value is published, including zero.
        if !trailer.is_empty() {
            for (cell, code) in codes.iter_mut().enumerate() {
                let bit = trailer[cell / 8] & (0x80 >> (cell % 8)) != 0;
                let missing = if mode == 3 {
                    scalar(cell) == 0.0 && !bit
                } else {
                    bit
                };
                *code = if missing { -1 } else { 0 };
            }
        }
    }
    let vals = (0..PRES_BITS as usize).map(scalar).collect();
    Ok(PageCells {
        vals,
        statuses: codes,
        footnotes: notes,
    })
}

fn entry_index(coords: &[i64], estride: &[i64]) -> Result<usize, String> {
    coords
        .iter()
        .zip(estride)
        .try_fold(0usize, |sum, (&c, &s)| {
            usize::try_from(c)
                .ok()
                .zip(usize::try_from(s).ok())
                .and_then(|(c, s)| c.checked_mul(s))
                .and_then(|index| sum.checked_add(index))
                .ok_or_else(|| "data directory entry index overflow".into())
        })
}

fn directory_page(
    data: &[u8],
    directory: &MetadataDirectory,
    index: usize,
    lay: &Layout,
    mode: u16,
) -> Result<PageCells, String> {
    let context = |position: Option<usize>, reason: &str| {
        let position_text = position.map_or_else(|| "unresolved".into(), |p| p.to_string());
        let raw = position.and_then(|o| o.checked_add(8).and_then(|end| data.get(o..end)));
        let details = raw.map_or_else(String::new, |entry| {
            format!(
                ", raw={entry:02x?}, raw_offset=0x{:08x}, calculated_target_offset={}, content_length={}, allocated_length={}",
                u32(entry, 0), u32(entry, 0), u16(entry, 4), u16(entry, 6),
            )
        });
        format!(
            "invalid data directory entry {index} at byte {position_text}: directory_start={}, expected_entry_count={}, file_size={}{}; reason={reason}",
            directory.offset, directory.count, data.len(), details,
        )
    };
    let position = directory
        .entry_offset(data, index)
        .map_err(|err| context(None, &err.to_string()))?;
    let entry = &data[position..position + 8];
    if entry.iter().all(|&v| v == 0) {
        return decode_page(&[2, 1, 0, 0], 0, lay, 4, mode);
    }
    let target = usize::try_from(u32(entry, 0))
        .map_err(|_| context(Some(position), "page offset conversion overflow"))?;
    let size = usize::from(u16(entry, 4));
    let allocated = usize::from(u16(entry, 6));
    // Brwpro.exe 0x1402b6e2a reads the first length as content size; +6 is
    // allocation capacity (release path 0x1402b5cb3), not a second copy.
    if target == 0 || size == 0 || size > allocated {
        return Err(context(
            Some(position),
            "nonblank page requires a nonzero offset and 0 < content_length <= allocated_length",
        ));
    }
    if target.checked_add(size).is_none_or(|end| end > data.len()) {
        return Err(context(Some(position), "page content outside file"));
    }
    decode_page(data, target, lay, size, mode).map_err(|err| context(Some(position), &err))
}

/// Dense row range in file dimension order. Only the value column uses f64.
pub struct CellBatch {
    pub ids: Vec<Vec<i32>>,
    pub values: Vec<f64>,
    pub statuses: Vec<i16>,
    pub note_offsets: Vec<i32>,
    pub notes: Vec<u16>,
}

pub fn row_count(lay: &Layout) -> Result<usize, String> {
    lay.counts
        .iter()
        .try_fold(1usize, |n, &count| {
            usize::try_from(count).ok().and_then(|c| n.checked_mul(c))
        })
        .ok_or_else(|| "dimension product exceeds addressable memory".into())
}

fn reserved<T>(n: usize) -> Result<Vec<T>, String> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(n)
        .map_err(|_| "memory allocation failed for cell batch")?;
    Ok(result)
}

/// Decode just [start, end), walking consecutive page windows and writing
/// directly in Cartesian order. No full store, sort, or per-cell ID copies.
pub fn decode_range(
    data: &[u8],
    lay: &Layout,
    start: usize,
    end: usize,
) -> Result<CellBatch, String> {
    let total = row_count(lay)?;
    if start > end || end > total {
        return Err("cell batch range outside grid".into());
    }
    let directory = MetadataDirectory::new(data, HDR_DIR_PTR)
        .map_err(|err| format!("invalid data directory: {err}"))?;
    if u16(data, HDR_DIR_PTR + 12) & 1 != 0 && u16(data, 20) != 4096 {
        return Err(format!(
            "invalid data directory: unsupported directory page size {}",
            u16(data, 20)
        ));
    }
    let capacity = u32(data, HDR_DIR_PTR + 4) as usize;
    if directory.count > capacity {
        return Err(format!("invalid data directory: expected_entry_count={}, allocated_capacity={capacity}; reason=entry count exceeds capacity", directory.count));
    }
    let mode = missing_mode(data)?;
    let n = end - start;
    let mut strides = vec![1usize; lay.n_dim];
    for axis in (0..lay.n_dim - 1).rev() {
        strides[axis] = strides[axis + 1] * lay.counts[axis + 1] as usize;
    }
    let mut ids = Vec::with_capacity(lay.n_dim);
    for (axis, &stride) in strides.iter().enumerate() {
        let mut column = reserved(n)?;
        column
            .extend((start..end).map(|row| (row / stride % lay.counts[axis] as usize + 1) as i32));
        ids.push(column);
    }
    let mut batch = CellBatch {
        ids,
        values: reserved(n)?,
        statuses: reserved(n)?,
        note_offsets: reserved(n + 1)?,
        notes: Vec::new(),
    };
    batch.note_offsets.push(0);
    let axis = lay.straddle - 1;
    let inner = strides[axis];
    let mut row = start;
    while row < end {
        let member = row / inner % lay.counts[axis] as usize;
        let window = member / lay.ipc[0] as usize;
        let mut coords = vec![window as i64];
        for &dim in &lay.ent_idx[1..] {
            coords.push((row / strides[dim - 1] % lay.counts[dim - 1] as usize) as i64);
        }
        let index = entry_index(&coords, &lay.estride)?;
        let page = directory_page(data, &directory, index, lay, mode)?;
        let first = (member % lay.ipc[0] as usize) * inner + row % inner;
        let remaining = (lay.ipc[0] as usize - member % lay.ipc[0] as usize)
            .min(lay.counts[axis] as usize - member)
            * inner
            - row % inner;
        let length = remaining.min(end - row);
        for &bit in &lay.bit[first..first + length] {
            let bit = bit as usize;
            batch.values.push(page.vals[bit]);
            batch.statuses.push(page.statuses[bit]);
            batch
                .notes
                .try_reserve(page.footnotes[bit].len())
                .map_err(|_| "memory allocation failed for annotations")?;
            batch.notes.extend_from_slice(&page.footnotes[bit]);
            batch.note_offsets.push(
                i32::try_from(batch.notes.len())
                    .map_err(|_| "too many annotations in one batch")?,
            );
        }
        row += length;
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paged_data_directory() -> (Vec<u8>, Layout) {
        // Three directory pages in deliberately nonmonotonic physical order.
        // A partial last page must keep its holes and declared ordinal bound.
        let pages = [1024u32, 66_560, 33_792];
        let mut data = vec![0; 99_328];
        data[20..22].copy_from_slice(&4096u16.to_le_bytes());
        data[HDR_DIR_PTR..HDR_DIR_PTR + 4].copy_from_slice(&1000u32.to_le_bytes());
        data[HDR_DIR_PTR + 4..HDR_DIR_PTR + 8].copy_from_slice(&16_384u32.to_le_bytes());
        data[HDR_DIR_PTR + 8..HDR_DIR_PTR + 12].copy_from_slice(&8194u32.to_le_bytes());
        data[HDR_DIR_PTR + 12..HDR_DIR_PTR + 14].copy_from_slice(&1u16.to_le_bytes());
        for (i, page) in pages.iter().enumerate() {
            data[1000 + 4 * i..1004 + 4 * i].copy_from_slice(&page.to_le_bytes());
        }
        for (index, value) in [
            (1, 11i16),
            (4095, 12),
            (4096, 13),
            (8191, 14),
            (8192, 15),
            (8193, 16),
        ] {
            let offset = data.len() as u32;
            let mut page = vec![0; 4100];
            page[..4].copy_from_slice(&[2, 1, 0, 8]);
            page[4..6].copy_from_slice(&value.to_le_bytes());
            page[6..8].copy_from_slice(&(-value).to_le_bytes());
            data.extend_from_slice(&page);
            let at = pages[index / 4096] as usize + 8 * (index % 4096);
            data[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            data[at + 4..at + 6].copy_from_slice(&4100u16.to_le_bytes());
            data[at + 6..at + 8].copy_from_slice(&4100u16.to_le_bytes());
        }
        let lay = build_layout(&[DimDesc {
            name: "Items".into(),
            count: 2,
            type_code: 0,
            double01: false,
        }])
        .unwrap();
        (data, lay)
    }

    #[test]
    fn data_directory_pages_preserve_coordinates_and_blank_entries() {
        let (data, lay) = paged_data_directory();
        let directory = MetadataDirectory::new(&data, HDR_DIR_PTR).unwrap();
        for (index, value) in [
            (0, 0.0),
            (1, 11.0),
            (4095, 12.0),
            (4096, 13.0),
            (8191, 14.0),
            (8192, 15.0),
            (8193, 16.0),
        ] {
            let page = directory_page(&data, &directory, index, &lay, 3).unwrap();
            assert_eq!(page.vals[..2], [value, -value]);
            assert_eq!(page.statuses[..2], [0, 0]);
            assert_eq!(lay.tuples[..2], [vec![1], vec![2]]);
        }
        let error = directory_page(&data, &directory, 8194, &lay, 3)
            .err()
            .unwrap();
        assert!(error.contains("entry 8194"), "{error}");
        assert!(error.contains("expected_entry_count=8194"), "{error}");
    }

    #[test]
    fn data_directory_rejects_malformed_entries_with_context() {
        let (original, lay) = paged_data_directory();
        for (offset, content, allocated, reason) in [
            (0, 4, 4, "nonblank page"),
            (original.len() as u32 - 3, 4, 4, "page content outside file"),
            (original.len() as u32, 4, 4, "page content outside file"),
            (u32::MAX, 4, 4, "page content outside file"),
            (99_328, 0, 4, "nonblank page"),
            (99_328, 4, 3, "content_length <= allocated_length"),
        ] {
            let mut data = original.clone();
            let at = 66_560;
            data[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            data[at + 4..at + 6].copy_from_slice(&(content as u16).to_le_bytes());
            data[at + 6..at + 8].copy_from_slice(&(allocated as u16).to_le_bytes());
            let directory = MetadataDirectory::new(&data, HDR_DIR_PTR).unwrap();
            let error = directory_page(&data, &directory, 4096, &lay, 3)
                .err()
                .unwrap();
            for expected in [
                "entry 4096 at byte 66560",
                "raw_offset=0x",
                "calculated_target_offset=",
                "directory_start=1000",
                reason,
            ] {
                assert!(error.contains(expected), "{expected}: {error}");
            }
        }
        // Declared content, rather than allocation or a size-difference
        // heuristic, controls the component read.
        let mut data = original;
        data[1038..1040].copy_from_slice(&10_000u16.to_le_bytes());
        let directory = MetadataDirectory::new(&data, HDR_DIR_PTR).unwrap();
        assert_eq!(
            directory_page(&data, &directory, 1, &lay, 3).unwrap().vals[0],
            11.0
        );
    }

    #[test]
    fn directory_index_arithmetic_is_checked() {
        assert_eq!(entry_index(&[1, 2], &[1, 4096]).unwrap(), 8193);
        assert!(entry_index(&[-1], &[1]).is_err());
        assert!(entry_index(&[i64::MAX], &[i64::MAX]).is_err());
        assert!(entry_index(&[1, 1, 1], &[i64::MAX; 3]).is_err());
    }

    #[test]
    fn layout_accepts_single_axes_and_disambiguates_reserved_names() {
        let dim = |name: &str| DimDesc {
            name: name.into(),
            count: 2,
            type_code: 0,
            double01: false,
        };
        assert!(build_layout(&[dim("Items")]).is_ok());
        let layout = build_layout(&[
            dim("Region"),
            dim("Geo"),
            dim("Value"),
            dim("Status code"),
            dim("Geo 2"),
        ])
        .unwrap();
        assert_eq!(
            layout.slugs.iter().collect::<HashSet<_>>().len(),
            layout.slugs.len()
        );
        for reserved in ["value", "status_code", "symbol", "status", "footnote_raw"] {
            assert!(!layout.slugs.iter().any(|slug| slug == reserved));
        }
        let mut invalid = dim("Items");
        invalid.count = 0;
        assert!(build_layout(&[invalid]).is_err());
    }

    #[test]
    fn component_bitmap_is_word_swapped_and_zero_is_valid() {
        let bytes = [0x82, 1, 17, 0, 1, 0x80, 0, 0x80, 0, 0x80, 7, 0, 9, 0];
        let decoded = decompress_component(&bytes).unwrap();
        assert_eq!(decoded.len(), 38);
        assert_eq!(
            i16::from_le_bytes(decoded[4..6].try_into().unwrap()),
            i16::MIN
        );
        assert_eq!(u16(&decoded, 6), 0);
        assert_eq!(u16(&decoded, 34), 7);
        assert_eq!(u16(&decoded, 36), 9);
    }

    #[test]
    fn component_rank_two_and_truncation() {
        let bytes = [0x81, 2, 2, 0, 2, 0, 0, 0xa0, 3, 5];
        assert_eq!(
            decompress_component(&bytes).unwrap(),
            [1, 2, 2, 0, 2, 0, 3, 0, 5, 0]
        );
        assert!(decompress_component(&bytes[..9]).is_err());
        assert!(decompress_component(&[2, 1, 1, 0, 1]).is_err());
    }

    #[test]
    fn missing_status_is_independent_of_numeric_value() {
        let dims = [DimDesc {
            name: "Items".into(),
            count: 2,
            type_code: 0,
            double01: false,
        }];
        let layout = build_layout(&dims).unwrap();
        let mut page = vec![0u8; 4 + 4096 + 262];
        page[..4].copy_from_slice(&[0x22, 1, 0x83, 8]);
        page[4..6].copy_from_slice(&i16::MIN.to_le_bytes());
        let trailer = 4 + 4096;
        page[trailer..trailer + 5].copy_from_slice(&[1, 2, 1, 1, 1]);
        page[trailer + 5] = 0x40; // Cell zero is valid; cell one is missing.
        let cells = decode_page(&page, 0, &layout, page.len(), 3).unwrap();
        assert_eq!(cells.vals[0], i16::MIN as f64);
        assert_eq!(cells.statuses[..2], [0, 1]);
        assert_eq!(cells.vals[1], 0.0);
    }

    #[test]
    fn legacy_zero_policy_uses_table_settings() {
        let dims = [DimDesc {
            name: "Items".into(),
            count: 2,
            type_code: 0,
            double01: false,
        }];
        let layout = build_layout(&dims).unwrap();
        let mut page = vec![0u8; 4 + 4096 + 256];
        page[..4].copy_from_slice(&[2, 1, 0x80, 8]);
        page[4100] = 0x80;
        let cells = decode_page(&page, 0, &layout, page.len(), 3).unwrap();
        assert_eq!(cells.statuses[..2], [0, -1]);
        let cells = decode_page(&page, 0, &layout, page.len(), 1).unwrap();
        assert_eq!(cells.statuses[..2], [-1, 0]);
        // A page without a mask publishes zeros regardless of the table's
        // default policy. This also covers the zero-length short encoding.
        page.truncate(4100);
        page[2..4].copy_from_slice(&(PRES_BITS as u16).to_le_bytes());
        let cells = decode_page(&page, 0, &layout, page.len(), 3).unwrap();
        assert_eq!(cells.vals[..2], [0.0, 0.0]);
        assert_eq!(cells.statuses[..2], [0, 0]);
        let cells = decode_page(&[2, 1, 0, 0], 0, &layout, 4, 3).unwrap();
        assert_eq!(cells.statuses[..2], [0, 0]);
    }

    #[test]
    fn extended_footnotes_preserve_multiple_raw_entries_in_order() {
        let dims = [DimDesc {
            name: "Items".into(),
            count: 2,
            type_code: 0,
            double01: false,
        }];
        let layout = build_layout(&dims).unwrap();
        let mut page = vec![0u8; 4 + 4096 + 12];
        page[..4].copy_from_slice(&[0x22, 1, 6, 8]);
        page[4100..4111].copy_from_slice(&[1, 8, 0, 2, 1, 2, 0, 0, 0, 3, 0]);
        let cells = decode_page(&page, 0, &layout, page.len(), 3).unwrap();
        assert_eq!(cells.footnotes[0], [0, 3]);
        assert!(cells.footnotes[1].is_empty());
    }

    #[test]
    fn page_capacity_two_extended_records_and_five_bit_references() {
        let lay = build_layout(&[DimDesc {
            name: "Items".into(),
            count: 2048,
            type_code: 0,
            double01: false,
        }])
        .unwrap();
        assert_eq!(lay.bit.len(), 2048);
        assert_eq!(lay.bit[2047], 2047);
        let mut page = vec![0u8; 4100];
        page[..4].copy_from_slice(&[0x22, 1, 0, 8]);
        page[4098..4100].copy_from_slice(&123i16.to_le_bytes());
        let mut missing = vec![1, 1];
        missing.extend_from_slice(&[0; 256]);
        missing[257] = 1; // The final cell is missing with explicit code 1.
        let notes = [2, 1, 2, 0, 0, 0, 255, 255]; // Cell 0/ref 0; cell 2047/ref 31.
        page.extend_from_slice(&[2, 2, 1, 8, 0]);
        page.extend_from_slice(&missing);
        page.extend_from_slice(&notes);
        if page.len() % 2 != 0 {
            page.push(0);
        }
        let count = ((page.len() - 4) / 2) as u16;
        page[2..4].copy_from_slice(&count.to_le_bytes());
        let cells = decode_page(&page, 0, &lay, page.len(), 3).unwrap();
        assert_eq!(cells.vals[2047], 123.0);
        assert_eq!(cells.statuses[2047], 1);
        assert_eq!(cells.footnotes[0], [0]);
        assert_eq!(cells.footnotes[2047], [u16::MAX]);
        assert_eq!(u16::MAX >> 5, 2047);
        assert_eq!(u16::MAX & 31, 31);
        page[4100] = 3;
        assert!(decode_page(&page, 0, &lay, page.len(), 3)
            .err()
            .unwrap()
            .contains("extended page record directory"));

        let mut short = vec![0u8; 4 + 2047 * 2];
        short[..4].copy_from_slice(&[2, 1, 255, 7]);
        assert!(decode_page(&short, 0, &lay, short.len(), 3)
            .err()
            .unwrap()
            .contains("only 2047 cells"));
        assert!(decompress_component(&[0x88, 2, 255, 255, 255, 255])
            .err()
            .unwrap()
            .contains("safety limit"));
    }

    #[test]
    fn supplied_pages_decode_all_dimensions_and_cells() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let Ok(files) = std::fs::read_dir(directory) else {
            return;
        };
        for file in files {
            let path = file.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "ivt") {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let dims = crate::descriptor::parse_descriptor(&data).unwrap().dims;
            let layout = build_layout(&dims).unwrap();
            let count = row_count(&layout).unwrap();
            let columns = decode_range(&data, &layout, 0, count)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let count: usize = dims.iter().map(|dim| dim.count as usize).product();
            assert_eq!(columns.values.len(), count);
            assert_eq!(columns.statuses.len(), count);
            for ids in &columns.ids {
                assert_eq!(ids.len(), count, "{}", path.display());
            }
        }
    }

    #[test]
    fn test_value_trailer() {
        assert_eq!(value_trailer(0x88, 0x00, 0x08).unwrap(), 0);
        assert_eq!(value_trailer(0xA8, 0x41, 0x08).unwrap(), 10);
    }

    #[test]
    fn test_bit_layout_age_table() {
        let (stride, rec) = bit_layout(&[8, 4, 32, 2]);
        assert_eq!(rec, 256);
        assert_eq!(*stride.last().unwrap(), 1);
    }
}
