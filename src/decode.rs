//! Unified page-directory layout and cell decode.

use crate::bytes::{nextpow2, u16, u32};
use crate::descriptor::{dim_slugs, DimDesc};
use crate::error::{corrupt_record, limit_exceeded, unsupported_record, Context, IvtError};
use crate::header::{HDR_DIR_PTR, HDR_MASTER_PTR, MARKER_WIDTHS};
use crate::metadata_directory::MetadataDirectory;
use std::collections::HashSet;

pub const PRES_BITS: i64 = 2048;

#[derive(Debug, Clone)]
pub struct Layout {
    pub counts: Vec<i64>,
    pub slugs: Vec<String>,
    pub n_dim: usize,
    pub geo_dim: Option<usize>,
    pub straddle: usize,
    pub ipc: Vec<i64>,
    pub window_count: i64,
    pub inpage_idx: Vec<usize>,
    pub bit: Vec<i32>,
    pub rec_bytes: usize,
    pub ent_idx: Vec<usize>,
    pub ent_counts: Vec<i64>,
    pub estride: Vec<i64>,
    pub geo_in_page: bool,
    pub member_slots: Vec<Option<Vec<usize>>>,
    page_strides: Vec<i64>,
}

pub fn build_descriptor_layout(desc: &crate::descriptor::Descriptor) -> Result<Layout, IvtError> {
    let mut physical = desc.dims.clone();
    // Slugs come from the full English title; the axis name is truncated.
    for (dim, [title, _]) in physical.iter_mut().zip(&desc.titles) {
        if let Some(title) = title {
            dim.name = title.clone();
        }
    }
    for (dim, slots) in physical.iter_mut().zip(&desc.member_slots) {
        if let Some(slots) = slots {
            if slots.len() != dim.count as usize || slots.windows(2).any(|s| s[0] >= s[1]) {
                return Err(corrupt_record("member slots do not match the axis matrix"));
            }
            dim.count = slots
                .last()
                .ok_or_else(|| corrupt_record("empty member slot map"))?
                .checked_add(1)
                .ok_or_else(|| corrupt_record("member slot overflow"))?
                as i64;
        }
    }
    let allocations = desc
        .allocation_bits
        .as_ref()
        .map(|bits| bits.iter().map(|&bits| 1i64 << bits).collect::<Vec<_>>());
    let mut layout =
        build_layout_with_allocations(&physical, desc.geo_dim, allocations.as_deref())?;
    layout.counts = desc.dims.iter().map(|d| d.count).collect();
    layout.member_slots = desc.member_slots.clone();
    Ok(layout)
}

#[cfg(test)]
fn value_trailer(b0: u8, b2: u8, b3: u8) -> Result<usize, IvtError> {
    let w = b0 & 0x0F;
    let hi = b0 & 0xF0;
    if !MARKER_WIDTHS.contains(&w) || (hi != 0x80 && hi != 0xA0) || !(0x08..0x0F).contains(&b3) {
        return Err(unsupported_record(format!(
            "unrecognised page marker 0x{b0:02x} .. 0x{b3:02x}"
        )));
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

/// In-page bit index of every cell of the in-page grid, in row-major order.
fn cell_bits(counts: &[i64], stride: &[i64]) -> Vec<i32> {
    let n: i64 = if counts.is_empty() {
        0
    } else {
        counts.iter().product()
    };
    if n == 0 {
        return Vec::new();
    }
    let mut bit = vec![0i32];
    // Expand axis by axis: each existing prefix is followed by every member
    // of the next axis, which yields row-major order.
    for (&count, &stride) in counts.iter().zip(stride) {
        let stride = stride as i32;
        bit = bit
            .iter()
            .flat_map(|&base| (0..count as i32).map(move |idx| base + idx * stride))
            .collect();
    }
    bit
}

#[cfg(test)]
pub fn build_layout(dims: &[DimDesc]) -> Result<Layout, IvtError> {
    build_layout_with_geo(dims, crate::descriptor::geo_dim_index(dims))
}

#[cfg(any(test, feature = "fuzzing"))]
pub fn build_layout_with_geo(dims: &[DimDesc], gd: Option<usize>) -> Result<Layout, IvtError> {
    build_layout_with_allocations(dims, gd, None)
}

fn build_layout_with_allocations(
    dims: &[DimDesc],
    gd: Option<usize>,
    allocations: Option<&[i64]>,
) -> Result<Layout, IvtError> {
    if dims.is_empty() || dims.iter().any(|dim| dim.count <= 0) {
        return Err(corrupt_record(
            "descriptor must have dimensions with positive item counts",
        ));
    }
    let cnt: Vec<i64> = dims.iter().map(|d| d.count).collect();
    if cnt.iter().any(|&c| c > i32::MAX as i64) {
        return Err(unsupported_record(
            "dimension member count exceeds int32 member IDs",
        ));
    }
    cnt.iter()
        .try_fold(1usize, |n, &c| n.checked_mul(c as usize))
        .ok_or_else(|| limit_exceeded("dimension product exceeds addressable memory"))?;
    let m = cnt.len();
    let didx: Vec<usize> = (1..=m).filter(|&i| Some(i) != gd).collect();
    let names: Vec<String> = didx.iter().map(|&i| dims[i - 1].name.clone()).collect();
    let mut slugs = vec![String::new(); m];
    for (i, slug) in didx.iter().zip(dim_slugs(&names)) {
        slugs[i - 1] = slug;
    }
    let mut used: HashSet<String> = [
        "value",
        "status_code",
        "symbol",
        "status",
        "footnote_raw",
        "language",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if let Some(gd) = gd {
        let slug = String::from("geography");
        used.insert(slug.clone());
        slugs[gd - 1] = slug;
    }
    for (index, slug) in slugs.iter_mut().enumerate() {
        if Some(index + 1) == gd {
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

    let pad: Vec<i64> = allocations.map_or_else(
        || cnt.iter().map(|&c| nextpow2(c)).collect(),
        |allocations| allocations.to_vec(),
    );
    if pad.len() != m
        || pad
            .iter()
            .zip(&cnt)
            .any(|(&capacity, &count)| capacity < count || capacity & (capacity - 1) != 0)
    {
        return Err(corrupt_record(
            "axis allocation does not contain its declared members",
        ));
    }
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
    let bit = cell_bits(&ipc, &stride);

    let ws = if allocations.is_some() {
        (pad[straddle - 1] / ipc_straddle).max(1)
    } else {
        nextpow2(win)
    };
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
            .ok_or_else(|| corrupt_record("padded data directory size overflow"))?;
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
        bit,
        rec_bytes,
        ent_idx,
        ent_counts,
        estride,
        geo_in_page: Some(straddle) == gd,
        member_slots: vec![None; m],
        page_strides: stride,
    })
}

#[cfg(test)]
impl Layout {
    /// One-based member IDs of every in-page cell, row-major, matching `bit`.
    fn tuples(&self) -> Vec<Vec<i32>> {
        let mut rows = vec![Vec::new()];
        for &count in &self.ipc {
            rows = rows
                .iter()
                .flat_map(|row: &Vec<i32>| {
                    (1..=count as i32).map(move |id| {
                        let mut next = row.clone();
                        next.push(id);
                        next
                    })
                })
                .collect();
        }
        rows
    }
}

fn mapped_cell(lay: &Layout, coordinate: impl Fn(usize) -> usize) -> (usize, usize) {
    let physical = |axis: usize| {
        let member = coordinate(axis);
        lay.member_slots[axis]
            .as_ref()
            .map_or(member, |slots| slots[member])
    };
    let axis = lay.straddle - 1;
    let member = physical(axis);
    let mut entry = member / lay.ipc[0] as usize * lay.estride[0] as usize;
    for (i, &dim) in lay.ent_idx.iter().enumerate().skip(1) {
        entry += physical(dim - 1) * lay.estride[i] as usize;
    }
    let mut bit = member % lay.ipc[0] as usize * lay.page_strides[0] as usize;
    for (i, &dim) in lay.inpage_idx.iter().enumerate().skip(1) {
        bit += physical(dim - 1) * lay.page_strides[i] as usize;
    }
    (entry, bit)
}

struct PageCells {
    vals: Vec<f64>,
    statuses: Vec<i16>,
    footnotes: Vec<Vec<u16>>,
}

/// Decode the native component's zero suppression. Bitmap bits describe stored
/// bytes, not data validity. Brwpro.exe 0x1402b5740 consumes little-endian u16
/// words most-significant-bit first and supplies zero for an unset bit.
pub fn decompress_component(data: &[u8]) -> Result<Vec<u8>, IvtError> {
    if data.len() < 2 {
        return Err(corrupt_record("truncated component header"));
    }
    let width = (data[0] & 15) as usize;
    let rank = data[1] as usize;
    if ![1, 2, 4, 8].contains(&width) || rank > 2 || data[0] & 0x50 != 0 {
        return Err(unsupported_record(format!(
            "unsupported component header {:02x?}",
            &data[..2]
        )));
    }
    let header = 2 + rank * 2;
    if data.len() < header {
        return Err(corrupt_record("truncated component extents"));
    }
    let mut count = 1usize;
    for dim in 0..rank {
        count = count
            .checked_mul(u16(data, 2 + dim * 2) as usize)
            .ok_or_else(|| corrupt_record("component extent overflow"))?;
    }
    let bytes = count
        .checked_mul(width)
        .and_then(|n| n.checked_add(header))
        .ok_or_else(|| corrupt_record("component size overflow"))?;
    // A native directory record has a u16 encoded length. Refuse adversarial
    // standalone extents before expansion, including all-zero compressed data.
    if bytes > 64 * 1024 * 1024 {
        return Err(limit_exceeded(
            "component expansion exceeds the 64 MiB safety limit",
        ));
    }
    // Rank-two byte components can declare huge extents. Validate encoded
    // input before allocating its expanded destination.
    if data[0] & 0x80 == 0 {
        return data
            .get(..bytes)
            .map(|b| b.to_vec())
            .ok_or_else(|| corrupt_record("truncated uncompressed component"));
    }
    let bitmap_len = count.div_ceil(16) * 2;
    let bitmap_end = header
        .checked_add(bitmap_len)
        .ok_or_else(|| corrupt_record("component size overflow"))?;
    if bitmap_end > data.len() {
        return Err(corrupt_record("truncated component bitmap"));
    }
    // Bit `i` is the `i % 16`-th most significant bit of word `i / 16`. Bits
    // past `count` in the final word are padding and must be ignored.
    let word = |w: usize| {
        let bits = u16(data, header + w * 2);
        let tail = count - w * 16;
        if tail >= 16 {
            bits
        } else {
            bits & !(u16::MAX >> tail)
        }
    };
    let words = count.div_ceil(16);
    let stored: usize = (0..words).map(|w| word(w).count_ones() as usize).sum();
    let encoded_end = bitmap_end
        .checked_add(stored * width)
        .ok_or_else(|| corrupt_record("component size overflow"))?;
    if encoded_end > data.len() {
        return Err(corrupt_record("truncated component values"));
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(bytes)
        .map_err(|_| limit_exceeded("component allocation failed"))?;
    result.resize(bytes, 0);
    result[..header].copy_from_slice(&data[..header]);
    result[0] &= 0x7f;
    let mut pos = bitmap_end;
    for w in 0..words {
        let mut bits = word(w);
        while bits != 0 {
            let i = w * 16 + bits.leading_zeros() as usize;
            bits &= !(0x8000 >> (i % 16));
            let dest = header + i * width;
            result[dest..dest + width].copy_from_slice(&data[pos..pos + width]);
            pos += width;
        }
    }
    Ok(result)
}

fn missing_mode(data: &[u8]) -> Result<u16, IvtError> {
    let master = crate::header::slot_offset(data, HDR_MASTER_PTR)
        .ok_or_else(|| corrupt_record("missing or truncated table settings slot"))?;
    let raw = crate::metadata_directory::record(data, master, 2)
        .ok_or_else(|| corrupt_record("missing or truncated table settings"))?;
    let decoded = decompress_component(raw)?;
    if decoded.len() < 8 || decoded[..4] != [1, 1, 32, 0] {
        return Err(unsupported_record("unsupported table settings component"));
    }
    let mode = u16(&decoded, 6);
    if ![0, 1, 3].contains(&mode) {
        return Err(corrupt_record(format!("invalid missing-value mode {mode}")));
    }
    Ok(mode)
}

#[cfg(any(test, feature = "fuzzing"))]
fn decode_page(
    data: &[u8],
    off: usize,
    _lay: &Layout,
    size: usize,
    mode: u16,
) -> Result<PageCells, IvtError> {
    decode_page_impl(data, off, _lay, size, mode, true)
}

fn decode_page_impl(
    data: &[u8],
    off: usize,
    _lay: &Layout,
    size: usize,
    mode: u16,
    keep_notes: bool,
) -> Result<PageCells, IvtError> {
    let raw = data
        .get(
            off..off
                .checked_add(size)
                .ok_or_else(|| corrupt_record("page size overflow"))?,
        )
        .ok_or_else(|| corrupt_record("truncated page"))?;
    let page = decompress_component(raw)?;
    let width = (page[0] & 15) as usize;
    if page[1] != 1 || !MARKER_WIDTHS.contains(&(width as u8)) {
        return Err(unsupported_record(
            "data component must be a rank-one signed integer or double array",
        ));
    }
    let count = u16(&page, 2) as usize;
    // The browser represents an entirely zero page with a zero-length short.
    if count != 0 && count < PRES_BITS as usize {
        return Err(corrupt_record(format!(
            "data component contains only {count} cells"
        )));
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
    let mut notes = if keep_notes {
        vec![Vec::new(); PRES_BITS as usize]
    } else {
        Vec::new()
    };
    let trailer = page.get(4 + PRES_BITS as usize * width..).unwrap_or(&[]);
    if page[0] & 0x20 != 0 {
        if trailer.is_empty() {
            return Err(corrupt_record("missing extended page records"));
        }
        let n = trailer[0] as usize;
        if !(1..=2).contains(&n) || trailer.len() < 1 + 2 * n {
            return Err(corrupt_record("invalid extended page record directory"));
        }
        let mut start = 1 + 2 * n;
        for index in 0..n {
            let size = u16(trailer, 1 + 2 * index) as usize;
            let record = trailer
                .get(
                    start
                        ..start
                            .checked_add(size)
                            .ok_or_else(|| corrupt_record("record size overflow"))?,
                )
                .ok_or_else(|| corrupt_record("truncated extended page record"))?;
            start += size;
            if record.len() < 2 {
                return Err(corrupt_record("truncated extended page record header"));
            }
            match record[0] {
                1 => {
                    let bits = record[1] as usize;
                    if ![1, 2, 4, 8].contains(&bits) || record.len() < 2 + 256 * bits {
                        return Err(corrupt_record("invalid packed missing-value record"));
                    }
                    for (cell, code) in codes.iter_mut().enumerate() {
                        let shift = 8 - bits - (cell * bits % 8);
                        *code = ((record[2 + cell * bits / 8] >> shift)
                            & ((1u16 << bits) - 1) as u8) as i16;
                    }
                }
                2 => {
                    if record.len() < 4 {
                        return Err(corrupt_record("truncated footnote record"));
                    }
                    let n = u16(record, 2) as usize;
                    if record.len() < 4 + 2 * n {
                        return Err(corrupt_record("truncated footnote indices"));
                    }
                    for i in 0..n {
                        let packed = u16(record, 4 + i * 2);
                        let cell = (packed >> 5) as usize;
                        if cell >= codes.len() {
                            return Err(corrupt_record("footnote cell outside page"));
                        }
                        // Keep the recorded packed annotation reference intact.
                        if keep_notes {
                            notes[cell].push(packed);
                        }
                    }
                }
                value => {
                    return Err(unsupported_record(format!(
                        "unknown extended page record {value}"
                    )))
                }
            }
        }
    } else {
        if !trailer.is_empty() && trailer.len() < 256 {
            return Err(corrupt_record("truncated legacy missing-value bitmap"));
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

#[cfg(feature = "fuzzing")]
pub(crate) fn fuzz_page(data: &[u8]) {
    let dims = [DimDesc {
        name: "Items".into(),
        count: 2048,
        type_code: 0,
        double01: false,
    }];
    let layout = build_layout_with_geo(&dims, None).unwrap();
    for mode in [0, 1, 3] {
        let _ = decode_page(data, 0, &layout, data.len(), mode);
    }
}

fn entry_index(coords: &[i64], estride: &[i64]) -> Result<usize, IvtError> {
    coords
        .iter()
        .zip(estride)
        .try_fold(0usize, |sum, (&c, &s)| {
            usize::try_from(c)
                .ok()
                .zip(usize::try_from(s).ok())
                .and_then(|(c, s)| c.checked_mul(s))
                .and_then(|index| sum.checked_add(index))
                .ok_or_else(|| corrupt_record("data directory entry index overflow"))
        })
}

#[cfg(test)]
fn directory_page(
    data: &[u8],
    directory: &MetadataDirectory,
    index: usize,
    lay: &Layout,
    mode: u16,
) -> Result<PageCells, IvtError> {
    directory_page_impl(data, directory, index, lay, mode, true)
}

fn directory_page_impl(
    data: &[u8],
    directory: &MetadataDirectory,
    index: usize,
    lay: &Layout,
    mode: u16,
    keep_notes: bool,
) -> Result<PageCells, IvtError> {
    let context = |position: Option<usize>, error: IvtError| {
        let position_text = position.map_or_else(|| "unresolved".into(), |p| p.to_string());
        let raw = position.and_then(|o| o.checked_add(8).and_then(|end| data.get(o..end)));
        let details = raw.map_or_else(String::new, |entry| {
            format!(
                ", raw={entry:02x?}, raw_offset=0x{:08x}, calculated_target_offset={}, content_length={}, allocated_length={}",
                u32(entry, 0), u32(entry, 0), u16(entry, 4), u16(entry, 6),
            )
        });
        let page = raw.map(|entry| (u32(entry, 0), u16(entry, 4)));
        error
            .reword(|reason| format!(
                "invalid data directory entry {index} at byte {position_text}: directory_start={}, expected_entry_count={}, file_size={}{}; reason={reason}",
                directory.offset, directory.count, data.len(), details,
            ))
            .with(Context {
                offset: page.map(|(offset, _)| u64::from(offset)),
                length: page.map(|(_, length)| u64::from(length)),
                file_len: Some(data.len() as u64),
                slot: Some(HDR_DIR_PTR),
                entry: Some(index),
                dimension: None,
            })
    };
    let position = directory
        .entry_offset(data, index)
        .map_err(|err| context(None, err))?;
    let entry = &data[position..position + 8];
    if entry.iter().all(|&v| v == 0) {
        return decode_page_impl(&[2, 1, 0, 0], 0, lay, 4, mode, keep_notes);
    }
    let target = usize::try_from(u32(entry, 0)).map_err(|_| {
        context(
            Some(position),
            corrupt_record("page offset conversion overflow"),
        )
    })?;
    let size = usize::from(u16(entry, 4));
    let allocated = usize::from(u16(entry, 6));
    // Brwpro.exe 0x1402b6e2a reads the first length as content size; +6 is
    // allocation capacity (release path 0x1402b5cb3), not a second copy.
    if target == 0 || size == 0 || size > allocated {
        return Err(context(
            Some(position),
            corrupt_record(
                "nonblank page requires a nonzero offset and 0 < content_length <= allocated_length",
            ),
        ));
    }
    if target.checked_add(size).is_none_or(|end| end > data.len()) {
        return Err(context(
            Some(position),
            corrupt_record("page content outside file"),
        ));
    }
    decode_page_impl(data, target, lay, size, mode, keep_notes)
        .map_err(|err| context(Some(position), err))
}

/// Data directory and missing-value mode, parsed once per export and shared
/// by every batch and decode worker.
pub struct DataPages {
    directory: MetadataDirectory,
    mode: u16,
}

impl DataPages {
    pub fn open(data: &[u8]) -> Result<Self, IvtError> {
        let slot = crate::header::slot_offset(data, HDR_DIR_PTR)
            .ok_or_else(|| corrupt_record("missing data directory slot"))?;
        let directory = MetadataDirectory::new(data, slot).map_err(|e| {
            e.reword(|e| format!("invalid data directory: {e}"))
                .with(Context::slot(HDR_DIR_PTR).and_file_len(data.len()))
        })?;
        if u16(data, slot + 12) & 1 != 0 && u16(data, 20) != 4096 {
            return Err(unsupported_record(format!(
                "invalid data directory: unsupported directory page size {}",
                u16(data, 20)
            )));
        }
        let capacity = u32(data, slot + 4) as usize;
        if directory.count > capacity {
            return Err(corrupt_record(format!("invalid data directory: expected_entry_count={}, allocated_capacity={capacity}; reason=entry count exceeds capacity", directory.count)));
        }
        let mode = missing_mode(data)?;
        Ok(Self { directory, mode })
    }

    fn page(
        &self,
        data: &[u8],
        index: usize,
        lay: &Layout,
        notes: bool,
    ) -> Result<PageCells, IvtError> {
        directory_page_impl(data, &self.directory, index, lay, self.mode, notes)
    }
}

/// Selected rows factored into page addresses. Axes before the straddling
/// axis only choose the directory entry, the straddling axis contributes to
/// both, and later axes only choose the in-page cell. A block is one value
/// of the outer axes (through the straddling one); its rows share one page
/// and their cells are `block_bit + inner_bits[k]`.
struct SelectionWalk {
    /// Selected member count per outer axis.
    outer_counts: Vec<usize>,
    /// Directory entry contribution per outer axis and selected position.
    entry: Vec<Vec<usize>>,
    /// In-page cell contribution of each straddling-axis selected position.
    block_bit: Vec<usize>,
    /// In-page cell offsets of the rows within one block.
    inner_bits: Vec<usize>,
}

impl SelectionWalk {
    fn new(lay: &Layout, choices: &[Option<Vec<usize>>], selected_counts: &[usize]) -> Self {
        let axis = lay.straddle - 1;
        let physical = |dim: usize, index: usize| {
            let member = choices[dim].as_ref().map_or(index, |codes| codes[index]);
            lay.member_slots[dim]
                .as_ref()
                .map_or(member, |slots| slots[member])
        };
        let ipc = lay.ipc[0] as usize;
        let mut entry_stride = vec![0usize; axis];
        for (position, &dim) in lay.ent_idx.iter().enumerate().skip(1) {
            entry_stride[dim - 1] = lay.estride[position] as usize;
        }
        let mut entry: Vec<Vec<usize>> = (0..axis)
            .map(|dim| {
                (0..selected_counts[dim])
                    .map(|index| physical(dim, index) * entry_stride[dim])
                    .collect()
            })
            .collect();
        entry.push(
            (0..selected_counts[axis])
                .map(|index| physical(axis, index) / ipc * lay.estride[0] as usize)
                .collect(),
        );
        let block_bit = (0..selected_counts[axis])
            .map(|index| physical(axis, index) % ipc * lay.page_strides[0] as usize)
            .collect();
        let mut inner_bits = vec![0usize];
        for (dim, &count) in selected_counts.iter().enumerate().skip(axis + 1) {
            let stride = lay.page_strides[dim - axis] as usize;
            let offsets: Vec<usize> = (0..count)
                .map(|index| physical(dim, index) * stride)
                .collect();
            inner_bits = inner_bits
                .iter()
                .flat_map(|&base| offsets.iter().map(move |&offset| base + offset))
                .collect();
        }
        Self {
            outer_counts: selected_counts[..=axis].to_vec(),
            entry,
            block_bit,
            inner_bits,
        }
    }

    fn inner_rows(&self) -> usize {
        self.inner_bits.len()
    }

    /// Selected positions of the outer axes for block `block`.
    fn positions(&self, mut block: usize) -> Vec<usize> {
        let mut positions = vec![0usize; self.outer_counts.len()];
        for (position, &count) in positions.iter_mut().zip(&self.outer_counts).rev() {
            *position = block % count;
            block /= count;
        }
        positions
    }

    fn prefix_entry(&self, positions: &[usize]) -> usize {
        positions
            .iter()
            .zip(&self.entry)
            .map(|(&position, entry)| entry[position])
            .sum()
    }

    fn block_entry(&self, block: usize) -> usize {
        self.prefix_entry(&self.positions(block))
    }

    /// Split `[start, end)` into about `target` chunks whose interior bounds
    /// fall where the page changes, so no page is decoded by two workers.
    fn chunks(&self, start: usize, end: usize, target: usize, min_rows: usize) -> Vec<usize> {
        let inner = self.inner_rows();
        let step = (end - start).div_ceil(target.max(1)).max(min_rows).max(1);
        let mut bounds = vec![start];
        let mut candidate = start.saturating_add(step);
        while candidate < end {
            let mut block = candidate.div_ceil(inner);
            let mut previous = self.block_entry(block - 1);
            while block * inner < end {
                let current = self.block_entry(block);
                if current != previous {
                    break;
                }
                previous = current;
                block += 1;
            }
            let bound = block * inner;
            if bound >= end {
                break;
            }
            bounds.push(bound);
            candidate = bound.saturating_add(step);
        }
        bounds.push(end);
        bounds
    }

    /// Decode rows `[start, start + values.len())` in contiguous page runs.
    fn decode(
        &self,
        data: &[u8],
        lay: &Layout,
        pages: &DataPages,
        start: usize,
        values: &mut [std::mem::MaybeUninit<f64>],
        valid: &mut arrow::array::BooleanBufferBuilder,
    ) -> Result<(), IvtError> {
        let inner = self.inner_rows();
        let last = self.outer_counts.len() - 1;
        let mut positions = self.positions(start / inner);
        let mut offset = start % inner;
        let mut prefix = self.prefix_entry(&positions[..last]);
        let mut cached: Option<(usize, PageCells)> = None;
        let mut out = 0usize;
        while out < values.len() {
            let member = positions[last];
            let index = prefix + self.entry[last][member];
            if cached
                .as_ref()
                .is_none_or(|(previous, _)| *previous != index)
            {
                cached = Some((index, pages.page(data, index, lay, false)?));
            }
            let page = &cached.as_ref().unwrap().1;
            let length = (inner - offset).min(values.len() - out);
            let base = self.block_bit[member];
            for (slot, &bit) in values[out..out + length]
                .iter_mut()
                .zip(&self.inner_bits[offset..offset + length])
            {
                let bit = base + bit;
                let published = page.statuses[bit] == 0;
                valid.append(published);
                slot.write(if published { page.vals[bit] } else { 0.0 });
            }
            out += length;
            if out == values.len() {
                break;
            }
            offset = 0;
            // Advance the outer odometer; the entry prefix changes only when
            // an axis before the straddling one rolls over.
            let mut dim = last;
            loop {
                positions[dim] += 1;
                if positions[dim] < self.outer_counts[dim] || dim == 0 {
                    break;
                }
                positions[dim] = 0;
                dim -= 1;
            }
            if dim < last {
                prefix = self.prefix_entry(&positions[..last]);
            }
        }
        Ok(())
    }
}

/// Rows per decode chunk below which splitting costs more than it saves.
const MIN_CHUNK_ROWS: usize = 32_768;

/// Materialize only selected cells into the final value and validity buffers.
/// Rows are split at page boundaries and decoded on the rayon pool (sized by
/// `RAYON_NUM_THREADS`); each chunk owns a disjoint slice of the output.
/// Output is identical for any thread count and preserves native row order.
pub fn decode_selected(
    data: &[u8],
    lay: &Layout,
    pages: &DataPages,
    choices: &[Option<Vec<usize>>],
    selected_counts: &[usize],
    start: usize,
    end: usize,
) -> Result<arrow::array::Float64Array, IvtError> {
    let _profile = crate::profile::Stage::new("decode_values");
    let target_chunks = rayon::current_num_threads().saturating_mul(4);
    decode_selected_chunked(
        data,
        lay,
        pages,
        choices,
        selected_counts,
        start..end,
        target_chunks,
        MIN_CHUNK_ROWS,
    )
}

#[allow(clippy::too_many_arguments)]
fn decode_selected_chunked(
    data: &[u8],
    lay: &Layout,
    pages: &DataPages,
    choices: &[Option<Vec<usize>>],
    selected_counts: &[usize],
    range: std::ops::Range<usize>,
    target_chunks: usize,
    min_chunk_rows: usize,
) -> Result<arrow::array::Float64Array, IvtError> {
    use arrow::array::BooleanBufferBuilder;
    use arrow::buffer::{NullBuffer, ScalarBuffer};
    use rayon::prelude::*;
    let rows = range.len();
    let mut values: Vec<f64> = reserved(rows)?;
    let mut parts = Vec::new();
    if rows != 0 {
        let walk = SelectionWalk::new(lay, choices, selected_counts);
        let bounds = walk.chunks(range.start, range.end, target_chunks, min_chunk_rows);
        let mut slices = Vec::with_capacity(bounds.len() - 1);
        let mut rest = &mut values.spare_capacity_mut()[..rows];
        for pair in bounds.windows(2) {
            let (head, tail) = rest.split_at_mut(pair[1] - pair[0]);
            slices.push((pair[0], head));
            rest = tail;
        }
        let decode = |(first, slice): (usize, &mut [std::mem::MaybeUninit<f64>])| {
            let mut valid = BooleanBufferBuilder::new(slice.len());
            walk.decode(data, lay, pages, first, slice, &mut valid)
                .map(|()| valid)
        };
        // One chunk runs on the calling thread without touching the pool.
        let results: Vec<Result<BooleanBufferBuilder, IvtError>> = if slices.len() == 1 {
            slices.into_iter().map(decode).collect()
        } else {
            slices.into_par_iter().map(decode).collect()
        };
        // Report the error a serial decode would reach first.
        for result in results {
            parts.push(result?);
        }
        // SAFETY: every chunk returned Ok, so each wrote its whole disjoint
        // slice and together they initialize all `rows` elements.
        unsafe { values.set_len(rows) };
    }
    let valid = if parts.len() == 1 {
        parts.pop().unwrap().finish()
    } else {
        let mut valid = BooleanBufferBuilder::new(rows);
        for part in &parts {
            valid.append_packed_range(0..part.len(), part.as_slice());
        }
        valid.finish()
    };
    Ok(arrow::array::Float64Array::new(
        ScalarBuffer::from(values),
        Some(NullBuffer::new(valid)),
    ))
}

/// The pre-parallel per-row decoder, kept as the byte-identity oracle.
#[cfg(test)]
fn decode_selected_reference(
    data: &[u8],
    lay: &Layout,
    choices: &[Option<Vec<usize>>],
    selected_counts: &[usize],
    selected_strides: &[usize],
    start: usize,
    end: usize,
) -> Result<arrow::array::Float64Array, IvtError> {
    use arrow::array::BooleanBufferBuilder;
    use arrow::buffer::{NullBuffer, ScalarBuffer};
    let slot = crate::header::slot_offset(data, HDR_DIR_PTR)
        .ok_or_else(|| corrupt_record("missing data directory slot"))?;
    let directory = MetadataDirectory::new(data, slot).map_err(|e| {
        e.reword(|e| format!("invalid data directory: {e}"))
            .with(Context::slot(HDR_DIR_PTR).and_file_len(data.len()))
    })?;
    if u16(data, slot + 12) & 1 != 0 && u16(data, 20) != 4096 {
        return Err(unsupported_record(format!(
            "invalid data directory: unsupported directory page size {}",
            u16(data, 20)
        )));
    }
    let capacity = u32(data, slot + 4) as usize;
    if directory.count > capacity {
        return Err(corrupt_record(format!("invalid data directory: expected_entry_count={}, allocated_capacity={capacity}; reason=entry count exceeds capacity", directory.count)));
    }
    let mode = missing_mode(data)?;
    let mut native_strides = vec![1usize; lay.n_dim];
    for axis in (0..lay.n_dim - 1).rev() {
        native_strides[axis] = native_strides[axis + 1] * lay.counts[axis + 1] as usize;
    }
    let rows = end.saturating_sub(start);
    let mut values = reserved(rows)?;
    let mut valid = BooleanBufferBuilder::new(rows);
    let mut cached: Option<(usize, PageCells)> = None;
    let axis = lay.straddle - 1;
    let inner = native_strides[axis];
    let full_grid = choices.iter().all(Option::is_none);
    let mapped = lay.member_slots.iter().any(Option::is_some);
    let mut coords = vec![0usize; lay.n_dim];
    for row in start..end {
        let native_row = if full_grid {
            row
        } else {
            let mut native_row = 0usize;
            for dim in 0..lay.n_dim {
                let index = row / selected_strides[dim] % selected_counts[dim];
                coords[dim] = choices[dim].as_ref().map_or(index, |codes| codes[index]);
                native_row += coords[dim] * native_strides[dim];
            }
            native_row
        };
        let coordinate = |dim: usize| {
            if full_grid {
                row / native_strides[dim] % selected_counts[dim]
            } else {
                coords[dim]
            }
        };
        let axis_coord = coordinate(axis);
        let window = axis_coord / lay.ipc[0] as usize;
        let mut index = window * lay.estride[0] as usize;
        for (entry_axis, &dim) in lay.ent_idx.iter().enumerate().skip(1) {
            index += coordinate(dim - 1) * lay.estride[entry_axis] as usize;
        }
        let mapped_address = mapped.then(|| mapped_cell(lay, coordinate));
        if let Some((entry, _)) = mapped_address {
            index = entry;
        }
        if cached
            .as_ref()
            .is_none_or(|(previous, _)| *previous != index)
        {
            cached = Some((
                index,
                directory_page_impl(data, &directory, index, lay, mode, false)?,
            ));
        }
        let page = &cached.as_ref().unwrap().1;
        let local = (axis_coord % lay.ipc[0] as usize) * inner + native_row % inner;
        let bit = mapped_address.map_or_else(|| lay.bit[local] as usize, |(_, bit)| bit);
        let published = page.statuses[bit] == 0;
        valid.append(published);
        values.push(if published { page.vals[bit] } else { 0.0 });
    }
    Ok(arrow::array::Float64Array::new(
        ScalarBuffer::from(values),
        Some(NullBuffer::new(valid.finish())),
    ))
}

/// Dense row range in file dimension order. Only the value column uses f64.
pub struct CellBatch {
    pub ids: Vec<Vec<i32>>,
    pub values: Vec<f64>,
    pub statuses: Vec<i16>,
    pub note_offsets: Vec<i32>,
    pub notes: Vec<u16>,
}

pub fn row_count(lay: &Layout) -> Result<usize, IvtError> {
    lay.counts
        .iter()
        .try_fold(1usize, |n, &count| {
            usize::try_from(count).ok().and_then(|c| n.checked_mul(c))
        })
        .ok_or_else(|| limit_exceeded("dimension product exceeds addressable memory"))
}

fn reserved<T>(n: usize) -> Result<Vec<T>, IvtError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(n)
        .map_err(|_| limit_exceeded("memory allocation failed for cell batch"))?;
    Ok(result)
}

/// Decode just [start, end), walking consecutive page windows and writing
/// directly in Cartesian order. No full store, sort, or per-cell ID copies.
pub fn decode_range(
    data: &[u8],
    lay: &Layout,
    pages: &DataPages,
    start: usize,
    end: usize,
) -> Result<CellBatch, IvtError> {
    let _profile = crate::profile::Stage::new("decode_rows");
    let total = row_count(lay)?;
    if start > end || end > total {
        return Err(IvtError::Parse("cell batch range outside grid".into()));
    }
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
    if lay.member_slots.iter().any(Option::is_some) {
        let mut cached: Option<(usize, PageCells)> = None;
        for row in start..end {
            let (entry, bit) =
                mapped_cell(lay, |axis| row / strides[axis] % lay.counts[axis] as usize);
            if cached
                .as_ref()
                .is_none_or(|(previous, _)| *previous != entry)
            {
                cached = Some((entry, pages.page(data, entry, lay, true)?));
            }
            let page = &cached.as_ref().unwrap().1;
            batch.values.push(page.vals[bit]);
            batch.statuses.push(page.statuses[bit]);
            batch.notes.extend_from_slice(&page.footnotes[bit]);
            batch.note_offsets.push(
                i32::try_from(batch.notes.len())
                    .map_err(|_| limit_exceeded("too many annotations in one batch"))?,
            );
        }
        return Ok(batch);
    }
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
        let page = pages.page(data, index, lay, true)?;
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
                .map_err(|_| limit_exceeded("memory allocation failed for annotations"))?;
            batch.notes.extend_from_slice(&page.footnotes[bit]);
            batch.note_offsets.push(
                i32::try_from(batch.notes.len())
                    .map_err(|_| limit_exceeded("too many annotations in one batch"))?,
            );
        }
        row += length;
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::Array;

    /// Selected positions per axis: everything, every other member, or a
    /// sparse prefix/suffix mix that crosses page windows.
    fn selections(counts: &[i64]) -> Vec<Vec<Option<Vec<usize>>>> {
        let thin = |count: usize, step: usize| -> Option<Vec<usize>> {
            Some((0..count).step_by(step).collect())
        };
        let ends = |count: usize| -> Option<Vec<usize>> {
            let mut codes: Vec<usize> = (0..count.min(3)).collect();
            codes.extend(count.saturating_sub(2).max(codes.len())..count);
            Some(codes)
        };
        let counts: Vec<usize> = counts.iter().map(|&c| c as usize).collect();
        vec![
            vec![None; counts.len()],
            counts.iter().map(|&c| thin(c, 2)).collect(),
            counts
                .iter()
                .enumerate()
                .map(|(axis, &c)| if axis % 2 == 0 { ends(c) } else { None })
                .collect(),
            counts
                .iter()
                .enumerate()
                .map(|(axis, &c)| if axis % 2 == 1 { thin(c, 3) } else { None })
                .collect(),
        ]
    }

    fn assert_parallel_matches_serial(name: &str, data: &[u8], lay: &Layout) {
        let pages = DataPages::open(data).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();
        for choices in selections(&lay.counts) {
            let counts: Vec<usize> = choices
                .iter()
                .zip(&lay.counts)
                .map(|(choice, &count)| choice.as_ref().map_or(count as usize, Vec::len))
                .collect();
            let mut strides = vec![1usize; counts.len()];
            for axis in (0..counts.len() - 1).rev() {
                strides[axis] = strides[axis + 1] * counts[axis + 1];
            }
            let rows: usize = counts.iter().product();
            // Bounded windows keep the per-row debug-build oracle fast; odd
            // offsets start and end mid-page.
            const WINDOW: usize = 150_001;
            let mut ranges = Vec::new();
            ranges.push(0..rows.min(WINDOW));
            if rows > WINDOW {
                let middle = rows / 2 - 7;
                ranges.push(middle..(middle + WINDOW).min(rows));
                ranges.push(rows - WINDOW + 3..rows);
            }
            for range in ranges {
                let expected = decode_selected_reference(
                    data,
                    lay,
                    &choices,
                    &counts,
                    &strides,
                    range.start,
                    range.end,
                )
                .unwrap();
                for (target, min_rows) in [(1, usize::MAX), (3, 1), (64, 1), (1024, 1)] {
                    let actual = pool.install(|| {
                        decode_selected_chunked(
                            data,
                            lay,
                            &pages,
                            &choices,
                            &counts,
                            range.clone(),
                            target,
                            min_rows,
                        )
                        .unwrap()
                    });
                    let context = format!("{name}: {choices:?} rows {range:?} chunks {target}");
                    assert_eq!(actual.len(), expected.len(), "{context}");
                    assert_eq!(actual.nulls(), expected.nulls(), "{context}");
                    assert!(
                        actual
                            .values()
                            .iter()
                            .zip(expected.values().iter())
                            .all(|(a, e)| a.to_bits() == e.to_bits()),
                        "{context}"
                    );
                }
            }
        }
    }

    #[test]
    fn parallel_selected_decode_matches_serial_on_training_corpus() {
        for path in crate::test_data::files() {
            let data = std::fs::read(&path).unwrap();
            let descriptor = crate::descriptor::parse_descriptor(&data).unwrap();
            let layout = build_descriptor_layout(&descriptor).unwrap();
            assert_parallel_matches_serial(&path.display().to_string(), &data, &layout);
        }
    }

    #[test]
    fn page_aligned_chunks_never_split_a_page() {
        for path in crate::test_data::files() {
            let data = std::fs::read(&path).unwrap();
            let descriptor = crate::descriptor::parse_descriptor(&data).unwrap();
            let lay = build_descriptor_layout(&descriptor).unwrap();
            let counts: Vec<usize> = lay.counts.iter().map(|&c| c as usize).collect();
            let choices = vec![None; counts.len()];
            let walk = SelectionWalk::new(&lay, &choices, &counts);
            let rows: usize = counts.iter().product();
            let bounds = walk.chunks(5, rows, 97, 1);
            assert_eq!((bounds[0], *bounds.last().unwrap()), (5, rows));
            let inner = walk.inner_rows();
            for &bound in &bounds[1..bounds.len() - 1] {
                assert_eq!(bound % inner, 0, "{}", path.display());
                let block = bound / inner;
                assert_ne!(
                    walk.block_entry(block - 1),
                    walk.block_entry(block),
                    "{}",
                    path.display()
                );
            }
            assert!(bounds.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn empty_selection_decodes_to_an_empty_array() {
        if let Some(path) = crate::test_data::files().first() {
            let data = std::fs::read(path).unwrap();
            let descriptor = crate::descriptor::parse_descriptor(&data).unwrap();
            let lay = build_descriptor_layout(&descriptor).unwrap();
            let counts: Vec<usize> = lay.counts.iter().map(|&c| c as usize).collect();
            let pages = DataPages::open(&data).unwrap();
            let choices = vec![None; counts.len()];
            let array = decode_selected(&data, &lay, &pages, &choices, &counts, 3, 3).unwrap();
            assert_eq!(array.len(), 0);
        }
    }

    fn mapped_layout(counts: &[i64], slots: Vec<Option<Vec<usize>>>) -> Layout {
        let descriptor = crate::descriptor::Descriptor {
            offset: 0,
            dims: counts
                .iter()
                .enumerate()
                .map(|(axis, &count)| DimDesc {
                    name: format!("Axis {axis}"),
                    count,
                    type_code: 0,
                    double01: false,
                })
                .collect(),
            titles: Vec::new(),
            geo_dim: None,
            member_slots: slots,
            allocation_bits: None,
        };
        build_descriptor_layout(&descriptor).unwrap()
    }

    #[test]
    fn deleted_slots_keep_inner_outer_and_straddling_coordinates() {
        // Independent physical coordinates: padded inner axes use 8 and 1
        // bits per step, while the first axis advances 16 bits in one page.
        let inner = mapped_layout(&[2, 2, 3], vec![None, None, Some(vec![0, 2, 5])]);
        assert_eq!(inner.counts, [2, 2, 3]);
        assert_eq!(mapped_cell(&inner, |axis| [1, 1, 2][axis]), (0, 29));

        // The middle axis spans nine 256-member windows; directory allocation
        // rounds that to sixteen. Physical outer member 5 begins at entry 80.
        let outer = mapped_layout(
            &[3, 2051, 3],
            vec![Some(vec![0, 2, 5]), None, Some(vec![1, 3, 6])],
        );
        assert_eq!(mapped_cell(&outer, |axis| [2, 2050, 2][axis]), (88, 22));

        // Physical member 600 crosses a 512-member window on the middle axis.
        let straddle = mapped_layout(&[2, 3, 3], vec![None, Some(vec![0, 255, 600]), None]);
        assert_eq!(mapped_cell(&straddle, |axis| [1, 2, 2][axis]), (3, 354));

        let single = mapped_layout(&[3], vec![Some(vec![0, 2048, 4096])]);
        assert_eq!(mapped_cell(&single, |_| 0), (0, 0));
        assert_eq!(mapped_cell(&single, |_| 1), (1, 0));
        assert_eq!(mapped_cell(&single, |_| 2), (2, 0));
    }

    #[test]
    fn native_axis_allocation_reserves_unused_directory_windows() {
        let dims: Vec<_> = [13, 19, 9]
            .into_iter()
            .map(|count| DimDesc {
                name: format!("Items {count}"),
                count,
                type_code: 0,
                double01: false,
            })
            .collect();
        let layout = build_layout_with_allocations(&dims, None, Some(&[16, 2048, 16])).unwrap();
        assert_eq!(layout.straddle, 2);
        assert_eq!(layout.estride, [1, 16]);
        assert_eq!(mapped_cell(&layout, |axis| [1, 0, 0][axis]), (16, 0));
        assert_eq!(mapped_cell(&layout, |axis| [12, 18, 8][axis]), (192, 296));
    }

    fn paged_data_directory() -> (Vec<u8>, Layout) {
        // Three directory pages in deliberately nonmonotonic physical order.
        // A partial last page must keep its holes and declared ordinal bound.
        let pages = [1024u32, 66_560, 33_792];
        let mut data = vec![0; 99_328];
        data[12..16].copy_from_slice(&544u32.to_le_bytes());
        data[16..18].copy_from_slice(&32u16.to_le_bytes());
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
            assert_eq!(lay.tuples()[..2], [vec![1], vec![2]]);
        }
        let error = directory_page(&data, &directory, 8194, &lay, 3)
            .err()
            .unwrap();
        assert_eq!(error.kind(), crate::error::ErrorKind::Corrupt);
        assert_eq!(error.context().entry, Some(8194));
        let error = error.to_string();
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
            assert_eq!(error.kind(), crate::error::ErrorKind::Corrupt);
            let context = error.context();
            assert_eq!(context.entry, Some(4096));
            assert_eq!(context.offset, Some(u64::from(offset)));
            assert_eq!(context.file_len, Some(data.len() as u64));
            let error = error.to_string();
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
            dim("Language"),
        ])
        .unwrap();
        assert_eq!(
            layout.slugs.iter().collect::<HashSet<_>>().len(),
            layout.slugs.len()
        );
        for reserved in ["value", "status_code", "symbol", "status", "footnote_raw"] {
            assert!(!layout.slugs.iter().any(|slug| slug == reserved));
        }
        assert_eq!(layout.slugs.last().unwrap(), "language_2");
        let dims = [dim("Geography"), dim("Region"), dim("Reference period")];
        assert_eq!(
            build_layout(&dims).unwrap().slugs,
            ["geography", "region", "reference_period"]
        );
        assert_eq!(
            build_layout_with_geo(&dims, Some(2)).unwrap().slugs,
            ["geography_2", "geography", "reference_period"]
        );
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

    /// The previous per-bit expansion, kept as the oracle for the word scan.
    fn decompress_reference(data: &[u8]) -> Option<Vec<u8>> {
        let width = (data[0] & 15) as usize;
        let rank = data[1] as usize;
        let header = 2 + rank * 2;
        let count: usize = (0..rank).map(|d| u16(data, 2 + d * 2) as usize).product();
        let bitmap_end = header + count.div_ceil(16) * 2;
        let set = |i: usize| u16(data, header + (i / 16) * 2) & (0x8000 >> (i % 16)) != 0;
        let stored = (0..count).filter(|&i| set(i)).count();
        if bitmap_end + stored * width > data.len() {
            return None;
        }
        let mut result = vec![0u8; header + count * width];
        result[..header].copy_from_slice(&data[..header]);
        result[0] &= 0x7f;
        let mut pos = bitmap_end;
        for i in (0..count).filter(|&i| set(i)) {
            let dest = header + i * width;
            result[dest..dest + width].copy_from_slice(&data[pos..pos + width]);
            pos += width;
        }
        Some(result)
    }

    #[test]
    fn component_word_scan_matches_per_bit_reference() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..2000 {
            let width = [1u8, 2, 4, 8][case % 4];
            // Cover partial final words (with padding bits set) and exact multiples.
            let count = (next() % 70) as u16;
            let mut data = vec![0x80 | width, 1];
            data.extend_from_slice(&count.to_le_bytes());
            for _ in 0..count.div_ceil(16) {
                let word = match next() % 4 {
                    0 => 0,
                    1 => u16::MAX,
                    _ => next() as u16,
                };
                data.extend_from_slice(&word.to_le_bytes());
            }
            let payload = (next() % (count as u64 * width as u64 + 2)) as usize;
            data.extend((0..payload).map(|_| next() as u8));
            let actual = decompress_component(&data).ok();
            assert_eq!(
                actual,
                decompress_reference(&data),
                "case {case}: {data:02x?}"
            );
        }
    }

    #[test]
    fn cell_bits_follow_row_major_page_strides() {
        let counts = [3i64, 5, 2];
        let (stride, _) = bit_layout(&[4, 8, 2]);
        let bits = cell_bits(&counts, &stride);
        assert_eq!(bits.len(), 30);
        let mut row = 0;
        for a in 0..3 {
            for b in 0..5 {
                for c in 0..2 {
                    let expected = a * stride[0] + b * stride[1] + c * stride[2];
                    assert_eq!(i64::from(bits[row]), expected);
                    row += 1;
                }
            }
        }
        assert!(cell_bits(&[], &[]).is_empty());
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
            .to_string()
            .contains("extended page record directory"));

        let mut short = vec![0u8; 4 + 2047 * 2];
        short[..4].copy_from_slice(&[2, 1, 255, 7]);
        assert!(decode_page(&short, 0, &lay, short.len(), 3)
            .err()
            .unwrap()
            .to_string()
            .contains("only 2047 cells"));
        let limit = decompress_component(&[0x88, 2, 255, 255, 255, 255])
            .err()
            .unwrap();
        assert_eq!(limit.kind(), crate::error::ErrorKind::ResourceLimit);
        assert!(limit.to_string().contains("safety limit"));
    }

    #[test]
    fn supplied_pages_decode_all_dimensions_and_cells() {
        for path in crate::test_data::files() {
            let data = std::fs::read(&path).unwrap();
            let descriptor = crate::descriptor::parse_descriptor(&data).unwrap();
            let dims = &descriptor.dims;
            let layout = build_descriptor_layout(&descriptor).unwrap();
            let count = row_count(&layout).unwrap();
            let columns = decode_range(&data, &layout, &DataPages::open(&data).unwrap(), 0, count)
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
