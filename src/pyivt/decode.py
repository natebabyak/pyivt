"""Unified page-directory layout and cell decode."""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from ._bytes import f64_run, i16_run, i32_run, nextpow2
from .descriptor import DimDesc, dim_slugs, geo_dim_index
from .header import MARKER_WIDTHS, dir_entry, idx0, is_marker

PRES_BITS = 2048
PRES_BYTES = PRES_BITS // 8


def value_trailer(b0: int, b2: int, b3: int = 0x08) -> int:
    w = b0 & 0x0F
    hi = b0 & 0xF0
    if w not in MARKER_WIDTHS or hi not in (0x80, 0xA0) or b3 not in range(0x08, 0x0F):
        raise ValueError(f"unrecognised page marker 0x{b0:02x} .. 0x{b3:02x}")
    head = 32 * (b3 - 8)
    if b2 == 0:
        return head
    return 2 * (b2 // 16) + 2 * int((b2 & 0x0F) > 0) + head


def bit_layout(counts: list[int]) -> tuple[list[int], int]:
    k = len(counts)
    if k == 0:
        return [], 0
    block = [0] * k
    block[k - 1] = nextpow2(counts[k - 1])
    for i in range(k - 2, -1, -1):
        block[i] = nextpow2(counts[i] * block[i + 1])
    stride = [0] * k
    stride[k - 1] = 1
    for i in range(k - 1):
        stride[i] = block[i + 1]
    return stride, block[0] // 8


def cell_grid(counts: list[int], stride: list[int]) -> tuple[np.ndarray, np.ndarray]:
    """Return (tuples NxK 1-based member ids, bit positions)."""
    k = len(counts)
    n = int(np.prod(counts)) if counts else 0
    if n == 0:
        return np.zeros((0, k), dtype=np.int32), np.zeros(0, dtype=np.int32)
    tuples = np.zeros((n, k), dtype=np.int32)
    bits = np.zeros((n, k), dtype=np.int32)
    rin = 1
    for i in range(k - 1, -1, -1):
        idx = np.tile(np.repeat(np.arange(counts[i]), rin), n // (counts[i] * rin))
        tuples[:, i] = idx
        bits[:, i] = idx
        rin *= counts[i]
    bit = (bits * np.array(stride, dtype=np.int32)).sum(axis=1).astype(np.int32)
    return tuples + 1, bit


def bit_addr(bit: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    idx = bit // 8  # 0-based byte index
    bsw = idx ^ 1
    bsh = 7 - (bit % 8)
    return idx, bsw, bsh


def record_present(
    page: bytes | memoryview, rec_bytes: int, bit: np.ndarray
) -> np.ndarray:
    """Presence flags for precomputed bit positions (pair-swap, MSB-first)."""
    rec = np.frombuffer(page[:rec_bytes], dtype=np.uint8)
    _, bsw, bsh = bit_addr(bit)
    # clamp invalid addresses
    ok = (bsw >= 0) & (bsw < rec_bytes)
    out = np.zeros(len(bit), dtype=bool)
    if not np.any(ok):
        return out
    vals = rec[bsw[ok]]
    out[ok] = ((vals >> bsh[ok]) & 1) == 1
    return out


def record_popcount(page: bytes | memoryview, rec_bytes: int) -> int:
    rec = np.frombuffer(page[:rec_bytes], dtype=np.uint8)
    return int(np.unpackbits(rec).sum())


@dataclass
class Layout:
    counts: list[int]
    slugs: list[str]
    n_dim: int
    geo_dim: int  # 1-based
    straddle: int  # 1-based dim index
    ipc: list[int]
    window_count: int
    inpage_idx: list[int]  # 1-based
    tuples: np.ndarray
    bit: np.ndarray
    rec_bytes: int
    ent_idx: list[int]
    ent_counts: list[int]
    estride: list[int]
    geo_in_page: bool


def build_layout(dims: list[DimDesc]) -> Layout:
    if len(dims) < 2:
        raise ValueError("descriptor has too few dimensions")
    cnt = [d.count for d in dims]
    m = len(cnt)
    gd = geo_dim_index(dims)
    didx = [i for i in range(1, m + 1) if i != gd]
    names = [dims[i - 1].name for i in didx]
    slugs = [""] * m
    slugs[gd - 1] = "geo"
    for i, slug in zip(didx, dim_slugs(names)):
        slugs[i - 1] = slug

    pad = [nextpow2(c) for c in cnt]
    blk = 1
    straddle = 1
    inner_block = 1
    for j in range(m, 0, -1):
        need = nextpow2(pad[j - 1] * blk)
        if need > PRES_BITS or j == 1:
            straddle = j
            inner_block = blk
            break
        blk = need

    ipc_straddle = PRES_BITS // inner_block
    win = int(np.ceil(cnt[straddle - 1] / ipc_straddle))
    inpage_idx = list(range(straddle, m + 1))
    ipc = [cnt[i - 1] for i in inpage_idx]
    ipc[0] = ipc_straddle
    ipc_ext = [pad[i - 1] for i in inpage_idx]
    ipc_ext[0] = ipc_straddle
    stride, rec_bytes = bit_layout(ipc_ext)
    # grid uses member counts (straddle capped to ipc)
    grid_counts = list(ipc)
    # actual members on straddle dim may be less than ipc on last window — decode handles windows
    tuples, bit = cell_grid(grid_counts, stride)

    ws = nextpow2(win)
    ent_idx = [straddle]
    ent_counts = [win]
    ent_pad = [ws]
    if straddle > 1:
        for j in range(straddle - 1, 0, -1):
            ent_idx.append(j)
            ent_counts.append(cnt[j - 1])
            ent_pad.append(pad[j - 1])

    estride: list[int] = []
    eb = 1.0
    for t, ep in enumerate(ent_pad):
        estride.append(int(eb))
        eb = float(nextpow2(max(int(ep * eb), 1)))

    return Layout(
        counts=cnt,
        slugs=slugs,
        n_dim=m,
        geo_dim=gd,
        straddle=straddle,
        ipc=ipc,
        window_count=win,
        inpage_idx=inpage_idx,
        tuples=tuples,
        bit=bit,
        rec_bytes=rec_bytes,
        ent_idx=ent_idx,
        ent_counts=ent_counts,
        estride=estride,
        geo_in_page=straddle == gd,
    )


@dataclass
class PageCells:
    tuples: np.ndarray  # present in-page member ids
    vals: np.ndarray


def decode_page(
    data: bytes, off: int, lay: Layout, size: int | None = None
) -> PageCells | None:
    if off + 4 > len(data) or not is_marker(data, off):
        return None
    b0, b2, b3 = data[off], data[off + 2], data[off + 3]
    w = b0 & 0x0F
    if w not in MARKER_WIDTHS:
        raise ValueError(f"unknown value-width in marker 0x{b0:02x}")
    width = w
    is_float = w == 8
    if b0 < 0x80:
        # dense pages deferred
        return None

    page = memoryview(data)[off:]
    vstart = 4 + lay.rec_bytes + value_trailer(b0, b2, b3)
    nvf = record_popcount(page[4:], lay.rec_bytes)
    if nvf == 0:
        return None
    pres = record_present(page[4:], lay.rec_bytes, lay.bit)
    nv = int(pres.sum())
    vend = vstart + nvf * width
    if size is not None and vend > size:
        raise ValueError(f"page overrun at {off}: need {vend}, size {size}")
    if off + vend > len(data):
        raise ValueError(f"page overrun past EOF at {off}")
    if nv == 0 and nvf == nv:
        return None

    raw_vals = page[vstart:vend]
    if is_float:
        vals = np.array(f64_run(raw_vals, 0, nvf), dtype=np.float64)
    elif width == 4:
        vals = np.array(i32_run(raw_vals, 0, nvf), dtype=np.float64)
    else:
        vals = np.array(i16_run(raw_vals, 0, nvf), dtype=np.float64)

    if nvf == nv:
        return PageCells(tuples=lay.tuples[pres], vals=vals)

    # Extra presence bits beyond the modelled grid: rank among all present bits
    all_bits = np.arange(lay.rec_bytes * 8, dtype=np.int32)
    full = record_present(page[4:], lay.rec_bytes, all_bits)
    ranks = np.cumsum(full) - 1
    vi = ranks[lay.bit[pres]]
    return PageCells(tuples=lay.tuples[pres], vals=vals[vi])


def entry_index(coords: list[int], estride: list[int]) -> int:
    return sum(c * s for c, s in zip(coords, estride))


def decode_store(data: bytes, dims: list[DimDesc], lay: Layout | None = None) -> dict:
    """
    Decode all non-zero cells into a dict of parallel arrays:
    member-id columns (1-based) keyed by layout slugs, plus 'value'.
    """
    if lay is None:
        lay = build_layout(dims)
    base = idx0(data)
    n = len(data)

    # Cartesian of paged levels (innermost first in ent_*)
    # Enumerate real member cartesian, map to directory entry index
    ent_counts = lay.ent_counts
    estride = lay.estride
    ent_idx = lay.ent_idx  # 1-based dim indices

    # Build output columns
    cols: dict[str, list] = {s: [] for s in lay.slugs}
    cols["value"] = []

    # Nested enumeration of paged coordinates
    def walk(level: int, coord: list[int], member_ids: dict[int, int]) -> None:
        if level == len(ent_counts):
            k = entry_index(coord, estride)
            o = base + 8 * k
            e = dir_entry(data, o)
            if e is None or e.off == 0 or e.size == 0:
                return
            if e.off + e.size > n:
                return
            try:
                pg = decode_page(data, e.off, lay, e.size)
            except ValueError:
                return
            if pg is None:
                return
            # Map in-page tuples to full member ids
            # tuples columns align with inpage_idx
            for row in range(len(pg.vals)):
                mids = dict(member_ids)
                for col_i, dim_i in enumerate(lay.inpage_idx):
                    mid = int(pg.tuples[row, col_i])
                    if dim_i == lay.straddle:
                        # window offset
                        win_i = coord[0]  # straddle window is innermost paged (index 0)
                        mid = win_i * lay.ipc[0] + mid
                        if mid > lay.counts[dim_i - 1]:
                            continue  # padding slot past real members
                    mids[dim_i] = mid
                if (
                    lay.straddle in mids
                    and mids[lay.straddle] > lay.counts[lay.straddle - 1]
                ):
                    continue
                for dim_i, slug in enumerate(lay.slugs, start=1):
                    cols[slug].append(mids.get(dim_i, 0))
                cols["value"].append(float(pg.vals[row]))
            return

        for i in range(ent_counts[level]):
            dim_i = ent_idx[level]
            if dim_i == lay.straddle:
                # window index; members filled in page
                walk(level + 1, coord + [i], member_ids)
            else:
                mid = i + 1
                m2 = dict(member_ids)
                m2[dim_i] = mid
                walk(level + 1, coord + [i], m2)

    walk(0, [], {})
    return {k: np.asarray(v) for k, v in cols.items()}


def complete_grid(store: dict, lay: Layout) -> dict:
    """Expand sparse store to the published table (absent → 0)."""
    counts = lay.counts
    n = int(np.prod(counts))
    # Build dense multi-index
    grids = np.meshgrid(
        *[np.arange(1, c + 1, dtype=np.int32) for c in counts],
        indexing="ij",
    )
    out = {slug: g.ravel() for slug, g in zip(lay.slugs, grids)}
    values = np.zeros(n, dtype=np.float64)

    if store["value"].size:
        # Map member-id tuples to flat index (row-major, last dim fastest)
        flat = np.zeros(store["value"].shape[0], dtype=np.int64)
        stride = 1
        for dim_i in range(lay.n_dim, 0, -1):
            slug = lay.slugs[dim_i - 1]
            flat += (store[slug].astype(np.int64) - 1) * stride
            stride *= counts[dim_i - 1]
        # last write wins on duplicates
        values[flat] = store["value"]

    out["value"] = values
    return out
