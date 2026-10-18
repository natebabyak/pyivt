//! Unified page-directory layout and cell decode.

use crate::bytes::{f64_run, i16_run, i32_run, nextpow2};
use crate::descriptor::{dim_slugs, geo_dim_index, DimDesc};
use crate::header::{dir_entry, idx0, is_marker, MARKER_WIDTHS};

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

pub fn value_trailer(b0: u8, b2: u8, b3: u8) -> Result<usize, String> {
    let w = b0 & 0x0F;
    let hi = b0 & 0xF0;
    if !MARKER_WIDTHS.contains(&w) || (hi != 0x80 && hi != 0xA0) || !(0x08..0x0F).contains(&b3)
    {
        return Err(format!(
            "unrecognised page marker 0x{b0:02x} .. 0x{b3:02x}"
        ));
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
    for i in 0..k - 1 {
        stride[i] = block[i + 1];
    }
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

fn bit_addr(bit: &[i32]) -> (Vec<i32>, Vec<i32>, Vec<i32>) {
    let mut idx = Vec::with_capacity(bit.len());
    let mut bsw = Vec::with_capacity(bit.len());
    let mut bsh = Vec::with_capacity(bit.len());
    for &b in bit {
        let i = b / 8;
        idx.push(i);
        bsw.push(i ^ 1);
        bsh.push(7 - (b % 8));
    }
    (idx, bsw, bsh)
}

fn record_present(page: &[u8], rec_bytes: usize, bit: &[i32]) -> Vec<bool> {
    let rec = &page[..rec_bytes.min(page.len())];
    let (_, bsw, bsh) = bit_addr(bit);
    let mut out = vec![false; bit.len()];
    for i in 0..bit.len() {
        let s = bsw[i];
        if s >= 0 && (s as usize) < rec_bytes && (s as usize) < rec.len() {
            out[i] = ((rec[s as usize] >> bsh[i]) & 1) == 1;
        }
    }
    out
}

fn record_popcount(page: &[u8], rec_bytes: usize) -> usize {
    let take = rec_bytes.min(page.len());
    page[..take].iter().map(|b| b.count_ones() as usize).sum()
}

pub fn build_layout(dims: &[DimDesc]) -> Result<Layout, String> {
    if dims.len() < 2 {
        return Err("descriptor has too few dimensions".into());
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
}

fn decode_page(data: &[u8], off: usize, lay: &Layout, size: Option<usize>) -> Option<PageCells> {
    if off + 4 > data.len() || !is_marker(data, off) {
        return None;
    }
    let b0 = data[off];
    let b2 = data[off + 2];
    let b3 = data[off + 3];
    let w = b0 & 0x0F;
    if !MARKER_WIDTHS.contains(&w) {
        return None;
    }
    let width = w as usize;
    let is_float = w == 8;
    if b0 < 0x80 {
        return None;
    }

    let page = &data[off..];
    let vstart = 4 + lay.rec_bytes + value_trailer(b0, b2, b3).ok()?;
    let nvf = record_popcount(&page[4..], lay.rec_bytes);
    if nvf == 0 {
        return None;
    }
    let pres = record_present(&page[4..], lay.rec_bytes, &lay.bit);
    let nv = pres.iter().filter(|&&p| p).count();
    let vend = vstart + nvf * width;
    if let Some(sz) = size {
        if vend > sz {
            return None;
        }
    }
    if off + vend > data.len() {
        return None;
    }
    if nv == 0 && nvf == nv {
        return None;
    }

    let raw_vals = &page[vstart..vend];
    let vals: Vec<f64> = if is_float {
        f64_run(raw_vals, 0, nvf)
    } else if width == 4 {
        i32_run(raw_vals, 0, nvf)
            .into_iter()
            .map(|v| v as f64)
            .collect()
    } else {
        i16_run(raw_vals, 0, nvf)
            .into_iter()
            .map(|v| v as f64)
            .collect()
    };

    let present_rows: Vec<usize> = pres
        .iter()
        .enumerate()
        .filter_map(|(i, &p)| if p { Some(i) } else { None })
        .collect();
    let present_tuples: Vec<Vec<i32>> = present_rows
        .iter()
        .map(|&i| lay.tuples[i].clone())
        .collect();

    if nvf == nv {
        return Some(PageCells {
            tuples: present_tuples,
            vals,
        });
    }

    let all_bits: Vec<i32> = (0..(lay.rec_bytes * 8) as i32).collect();
    let full = record_present(&page[4..], lay.rec_bytes, &all_bits);
    let mut ranks = vec![0i32; full.len()];
    let mut acc = -1i32;
    for i in 0..full.len() {
        if full[i] {
            acc += 1;
        }
        ranks[i] = acc;
    }
    let mut selected = Vec::with_capacity(present_rows.len());
    for &row in &present_rows {
        let vi = ranks[lay.bit[row] as usize] as usize;
        selected.push(vals[vi]);
    }
    Some(PageCells {
        tuples: present_tuples,
        vals: selected,
    })
}

fn entry_index(coords: &[i64], estride: &[i64]) -> i64 {
    coords
        .iter()
        .zip(estride.iter())
        .map(|(c, s)| c * s)
        .sum()
}

pub fn decode_store(data: &[u8], dims: &[DimDesc], lay: &Layout) -> Result<Vec<(String, Vec<f64>)>, String> {
    // Returns columns as (name, values) — member ids as f64 for uniformity, plus "value"
    let base = idx0(data);
    let n = data.len();

    let mut cols: Vec<(String, Vec<i32>)> = lay
        .slugs
        .iter()
        .map(|s| (s.clone(), Vec::new()))
        .collect();
    let mut values: Vec<f64> = Vec::new();

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
    ) {
        if level == lay.ent_counts.len() {
            let k = entry_index(coord, &lay.estride);
            let o = base + 8 * k as usize;
            let Some(e) = dir_entry(data, o) else {
                return;
            };
            if e.off == 0 || e.size == 0 {
                return;
            }
            if (e.off as usize) + (e.size as usize) > n {
                return;
            }
            let Some(pg) = decode_page(data, e.off as usize, lay, Some(e.size as usize)) else {
                return;
            };
            for row in 0..pg.vals.len() {
                let mut mids = member_ids.clone();
                let mut skip = false;
                for (col_i, &dim_i) in lay.inpage_idx.iter().enumerate() {
                    let mut mid = pg.tuples[row][col_i];
                    if dim_i == lay.straddle {
                        let win_i = coord[0];
                        mid = (win_i * lay.ipc[0]) as i32 + mid;
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
            }
            return;
        }

        for i in 0..lay.ent_counts[level] {
            let dim_i = lay.ent_idx[level];
            coord.push(i);
            if dim_i == lay.straddle {
                walk(level + 1, coord, member_ids, data, lay, base, n, cols, values);
            } else {
                let mid = (i + 1) as i32;
                let prev = member_ids[dim_i - 1];
                member_ids[dim_i - 1] = Some(mid);
                walk(level + 1, coord, member_ids, data, lay, base, n, cols, values);
                member_ids[dim_i - 1] = prev;
            }
            coord.pop();
        }
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
    );

    let mut out: Vec<(String, Vec<f64>)> = cols
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().map(|x| x as f64).collect()))
        .collect();
    out.push(("value".into(), values));
    let _ = dims;
    Ok(out)
}

pub fn complete_grid(store: &[(String, Vec<f64>)], lay: &Layout) -> Vec<(String, Vec<f64>)> {
    let counts = &lay.counts;
    let n: usize = counts.iter().fold(1usize, |a, &c| a.saturating_mul(c as usize));

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

    let mut values = vec![0.0f64; n];
    let value_col = store
        .iter()
        .find(|(k, _)| k == "value")
        .map(|(_, v)| v.as_slice())
        .unwrap_or(&[]);
    if !value_col.is_empty() {
        let mut flat = vec![0i64; value_col.len()];
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
        for (i, &f) in flat.iter().enumerate() {
            if f >= 0 && (f as usize) < n {
                values[f as usize] = value_col[i];
            }
        }
    }
    out.push(("value".into(), values));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
