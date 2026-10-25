//! Unified page-directory layout and cell decode.

use crate::bytes::{nextpow2, u16};
use crate::descriptor::{dim_slugs, geo_dim_index, DimDesc};
use crate::header::{dir_anchor_header, dir_entry, HDR_MASTER_PTR, MARKER_WIDTHS};
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
    let mut eb = 1.0f64;
    for &ep in &ent_pad {
        estride.push(eb as i64);
        eb = nextpow2((ep as f64 * eb).max(1.0) as i64) as f64;
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
    tuples: Vec<Vec<i32>>,
    vals: Vec<f64>,
    statuses: Vec<f64>,
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
    lay: &Layout,
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
    let mut vals = Vec::with_capacity(lay.bit.len());
    let mut statuses = Vec::with_capacity(lay.bit.len());
    let mut footnotes = Vec::with_capacity(lay.bit.len());
    for &bit in &lay.bit {
        let cell = usize::try_from(bit).map_err(|_| "negative page coordinate")?;
        if cell >= codes.len() {
            return Err("coordinate outside page".into());
        }
        vals.push(scalar(cell));
        statuses.push(codes[cell] as f64);
        footnotes.push(notes[cell].clone());
    }
    Ok(PageCells {
        tuples: lay.tuples.clone(),
        vals,
        statuses,
        footnotes,
    })
}

fn entry_index(coords: &[i64], estride: &[i64]) -> i64 {
    coords.iter().zip(estride.iter()).map(|(c, s)| c * s).sum()
}

pub fn decode_store(
    data: &[u8],
    dims: &[DimDesc],
    lay: &Layout,
) -> Result<Vec<(String, Vec<f64>)>, String> {
    // Returns columns as (name, values) — member ids as f64 for uniformity, plus "value"
    let base = dir_anchor_header(data).ok_or("could not locate data directory")?;
    let mode = missing_mode(data)?;
    let n = data.len();

    let mut cols: Vec<(String, Vec<i32>)> =
        lay.slugs.iter().map(|s| (s.clone(), Vec::new())).collect();
    let mut values: Vec<f64> = Vec::new();
    let mut statuses = Vec::new();
    let mut footnotes = Vec::new();

    // Recursive traversal carries the shared output buffers through each level.
    #[allow(clippy::too_many_arguments)]
    fn walk(
        level: usize,
        coord: &mut Vec<i64>,
        member_ids: &mut Vec<Option<i32>>,
        data: &[u8],
        lay: &Layout,
        base: usize,
        n: usize,
        cols: &mut [(String, Vec<i32>)],
        values: &mut Vec<f64>,
        statuses: &mut Vec<f64>,
        footnotes: &mut Vec<Vec<u16>>,
        mode: u16,
    ) -> Result<(), String> {
        if level == lay.ent_counts.len() {
            let k = entry_index(coord, &lay.estride);
            let o = base + 8 * k as usize;
            if o.checked_add(8).is_none_or(|end| end > n) {
                return Err("truncated data directory".into());
            }
            let pg = if data[o..o + 8].iter().all(|&v| v == 0) {
                decode_page(&[2, 1, 0, 0], 0, lay, 4, mode)?
            } else {
                let e = dir_entry(data, o)
                    .ok_or_else(|| format!("invalid data directory entry at {o}"))?;
                if (e.off as usize) + (e.size as usize) > n {
                    return Err("page outside file".into());
                }
                decode_page(data, e.off as usize, lay, e.size as usize, mode)
                    .map_err(|err| format!("page at {}: {err}", e.off))?
            };
            for row in 0..pg.vals.len() {
                let mut mids = member_ids.clone();
                let mut skip = false;
                for (col_i, &dim_i) in lay.inpage_idx.iter().enumerate() {
                    let mut mid = pg.tuples[row][col_i];
                    if dim_i == lay.straddle {
                        let win_i = coord[0];
                        mid += (win_i * lay.ipc[0]) as i32;
                        if mid as i64 > lay.counts[dim_i - 1] {
                            skip = true;
                            break;
                        }
                    }
                    mids[dim_i - 1] = Some(mid);
                }
                if skip {
                    continue;
                }
                if let Some(Some(mid)) = mids.get(lay.straddle - 1) {
                    if *mid as i64 > lay.counts[lay.straddle - 1] {
                        continue;
                    }
                }
                for (dim_i, (_slug, col)) in cols.iter_mut().enumerate() {
                    col.push(mids[dim_i].unwrap_or(0));
                }
                values.push(pg.vals[row]);
                statuses.push(pg.statuses[row]);
                footnotes.push(pg.footnotes[row].clone());
            }
            return Ok(());
        }

        for i in 0..lay.ent_counts[level] {
            let dim_i = lay.ent_idx[level];
            coord.push(i);
            if dim_i == lay.straddle {
                walk(
                    level + 1,
                    coord,
                    member_ids,
                    data,
                    lay,
                    base,
                    n,
                    cols,
                    values,
                    statuses,
                    footnotes,
                    mode,
                )?;
            } else {
                let mid = (i + 1) as i32;
                let prev = member_ids[dim_i - 1];
                member_ids[dim_i - 1] = Some(mid);
                walk(
                    level + 1,
                    coord,
                    member_ids,
                    data,
                    lay,
                    base,
                    n,
                    cols,
                    values,
                    statuses,
                    footnotes,
                    mode,
                )?;
                member_ids[dim_i - 1] = prev;
            }
            coord.pop();
        }
        Ok(())
    }

    let mut coord = Vec::new();
    let mut member_ids = vec![None; lay.n_dim];
    walk(
        0,
        &mut coord,
        &mut member_ids,
        data,
        lay,
        base,
        n,
        &mut cols,
        &mut values,
        &mut statuses,
        &mut footnotes,
        mode,
    )?;

    let mut out: Vec<(String, Vec<f64>)> = cols
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().map(|x| x as f64).collect()))
        .collect();
    out.push(("value".into(), values));
    out.push(("status_code".into(), statuses));
    let note_columns = footnotes.iter().map(Vec::len).max().unwrap_or(0).max(1);
    for index in 0..note_columns {
        out.push((
            format!("footnote_raw_{}", index + 1),
            footnotes
                .iter()
                .map(|notes| notes.get(index).map(|&note| note as f64).unwrap_or(-1.0))
                .collect(),
        ));
    }
    let _ = dims;
    Ok(out)
}

pub fn complete_grid(store: &[(String, Vec<f64>)], lay: &Layout) -> Vec<(String, Vec<f64>)> {
    let counts = &lay.counts;
    let n: usize = counts
        .iter()
        .fold(1usize, |a, &c| a.saturating_mul(c as usize));

    // Build dense member-id columns without meshgrid temporaries
    let mut out: Vec<(String, Vec<f64>)> = Vec::with_capacity(lay.slugs.len() + 1);
    for (dim_i, slug) in lay.slugs.iter().enumerate() {
        let mut col = Vec::with_capacity(n);
        let mut stride_after = 1usize;
        for c in counts.iter().skip(dim_i + 1) {
            stride_after *= *c as usize;
        }
        let ci = counts[dim_i] as usize;
        let stride_before = n / (ci * stride_after);
        for _ in 0..stride_before {
            for mid in 1..=ci {
                for _ in 0..stride_after {
                    col.push(mid as f64);
                }
            }
        }
        out.push((slug.clone(), col));
    }

    let value_col = store
        .iter()
        .find(|(k, _)| k == "value")
        .map(|(_, v)| v.as_slice())
        .unwrap_or(&[]);
    let mut flat = vec![0i64; value_col.len()];
    if !value_col.is_empty() {
        let mut stride = 1i64;
        for dim_i in (0..lay.n_dim).rev() {
            let slug = &lay.slugs[dim_i];
            if let Some((_, ids)) = store.iter().find(|(k, _)| k == slug) {
                for (i, &id) in ids.iter().enumerate() {
                    flat[i] += (id as i64 - 1) * stride;
                }
            }
            stride *= counts[dim_i];
        }
    }
    let columns: Vec<&str> = store
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|&name| {
            name == "value" || name == "status_code" || name.starts_with("footnote_raw_")
        })
        .collect();
    for name in columns {
        let mut values = vec![
            if name.starts_with("footnote_raw_") {
                -1.0
            } else {
                0.0
            };
            n
        ];
        if let Some((_, source)) = store.iter().find(|(key, _)| key == name) {
            for (i, &f) in flat.iter().enumerate() {
                if f >= 0 && (f as usize) < n {
                    values[f as usize] = source[i];
                }
            }
        }
        out.push((name.into(), values));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(cells.statuses[..2], [0.0, 1.0]);
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
        assert_eq!(cells.statuses[..2], [0.0, -1.0]);
        let cells = decode_page(&page, 0, &layout, page.len(), 1).unwrap();
        assert_eq!(cells.statuses[..2], [-1.0, 0.0]);
        // A page without a mask publishes zeros regardless of the table's
        // default policy. This also covers the zero-length short encoding.
        page.truncate(4100);
        page[2..4].copy_from_slice(&(PRES_BITS as u16).to_le_bytes());
        let cells = decode_page(&page, 0, &layout, page.len(), 3).unwrap();
        assert_eq!(cells.vals[..2], [0.0, 0.0]);
        assert_eq!(cells.statuses[..2], [0.0, 0.0]);
        let cells = decode_page(&[2, 1, 0, 0], 0, &layout, 4, 3).unwrap();
        assert_eq!(cells.statuses[..2], [0.0, 0.0]);
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
            let columns = decode_store(&data, &dims, &layout)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let count: usize = dims.iter().map(|dim| dim.count as usize).product();
            assert!(columns.len() >= dims.len() + 3);
            for (_, values) in &columns {
                assert_eq!(values.len(), count, "{}", path.display());
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
