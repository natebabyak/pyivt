"""Fixed header offsets and page-directory anchoring."""

from __future__ import annotations

from dataclasses import dataclass

from ._bytes import u16, u32

HDR_DESCRIPTOR_PTR = 32
HDR_TITLE_FR_PTR = 40
HDR_TITLE_EN_PTR = 48
HDR_MASTER_PTR = 544
HDR_GEO_FIELDS = 552
HDR_DIR_PTR = 558
HDR_CODEBOOK_PTR = 572
HDR_STATUS_LEGEND = 698
HDR_DQF_LEGEND = 712
HDR_DIM_SLOT0 = 824
DIM_SLOT_STRIDE = 14

MARKER_WIDTHS = (2, 4, 8)
MARKER_B0 = tuple(w + hi for hi in (0x80, 0xA0) for w in MARKER_WIDTHS)
MARKER_B0_DENSE = MARKER_WIDTHS
MARKER_B3 = (0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E)

DIR_LEAD_BLANK_MAX = 1024


@dataclass(frozen=True)
class HeaderLayout:
    version: str
    descriptor: int
    title_en: int
    title_fr: int
    field_count: int
    directory_lo: int
    codebook: int
    eof: int
    signature: bytes


@dataclass(frozen=True)
class DirEntry:
    off: int
    size: int
    marker: bool


def is_marker(data: bytes, off: int) -> bool:
    if off + 4 > len(data) or data[off + 1] != 0x01:
        return False
    b0 = data[off]
    if b0 in MARKER_B0_DENSE:
        return u16(data, off + 2) > 0
    return b0 in MARKER_B0 and data[off + 3] in MARKER_B3


def dir_entry(data: bytes, o: int) -> DirEntry | None:
    if o + 8 > len(data):
        return None
    off = u32(data, o)
    s1 = u16(data, o + 4)
    s2 = u16(data, o + 6)
    if off < 1 or s1 <= 0 or s2 <= 0 or off + 4 > len(data):
        return None
    is02 = data[0] == 0x02
    if s1 != s2 and not is02 and abs(s1 - s2) > 256:
        return None
    size = min(s1, s2)
    return DirEntry(off=off, size=size, marker=is_marker(data, off))


def entry_valid(data: bytes, o: int) -> bool:
    e = dir_entry(data, o)
    return e is not None and e.marker and e.off >= 1024


def entry_blank(data: bytes, o: int) -> bool:
    if o + 8 > len(data):
        return False
    return u32(data, o) == 0 and u16(data, o + 4) == 0 and u16(data, o + 6) == 0


def dir_first_entry(data: bytes, off: int) -> int | None:
    for j in range(DIR_LEAD_BLANK_MAX + 1):
        o = off + 8 * j
        if o + 8 > len(data):
            return None
        if entry_valid(data, o):
            return o
        if not entry_blank(data, o):
            return None
    return None


def dir_anchor_header(data: bytes) -> int | None:
    if len(data) < HDR_DIR_PTR + 2:
        return None
    lo = u16(data, HDR_DIR_PTR)
    if lo <= 0:
        return None
    max_k = max(0, (len(data) - lo) // 65536)
    for k in range(max_k + 1):
        off = lo + k * 65536
        if entry_valid(data, off):
            return off
    for k in range(max_k + 1):
        off = lo + k * 65536
        if dir_first_entry(data, off) is not None:
            return off
    return None


def idx0(data: bytes) -> int:
    anchor = dir_anchor_header(data)
    return anchor if anchor is not None else 37167


def read_header(data: bytes) -> HeaderLayout:
    if len(data) < 576:
        raise ValueError("IVT file too small to contain a header")
    fr = u32(data, HDR_TITLE_FR_PTR)
    en = u32(data, HDR_TITLE_EN_PTR)
    legacy = en != 0 or fr != 0
    return HeaderLayout(
        version="legacy" if legacy else "modern",
        descriptor=u32(data, HDR_DESCRIPTOR_PTR),
        title_en=en,
        title_fr=fr,
        field_count=u32(data, HDR_GEO_FIELDS),
        directory_lo=u16(data, HDR_DIR_PTR),
        codebook=u32(data, HDR_CODEBOOK_PTR),
        eof=len(data),
        signature=bytes(data[:4]),
    )


@dataclass(frozen=True)
class DimSlot:
    index: int
    dir_ptr: int
    alloc: int
    n_entries: int
    flag: int


def dim_slots(data: bytes, max_dims: int = 32) -> list[DimSlot]:
    out: list[DimSlot] = []
    for k in range(max_dims):
        base = HDR_DIM_SLOT0 + DIM_SLOT_STRIDE * k
        if base + DIM_SLOT_STRIDE > len(data):
            break
        dir_ptr = u32(data, base)
        alloc = u32(data, base + 4)
        n_entries = u32(data, base + 8)
        flag = u16(data, base + 12)
        if dir_ptr == 0 and alloc == 0 and n_entries == 0:
            break
        out.append(DimSlot(k, dir_ptr, alloc, n_entries, flag))
    return out


def read_block_dir(data: bytes, ptr: int, n_entries: int) -> list[tuple[int, int]]:
    """Read a codebook block directory: list of (offset, content_len)."""
    entries: list[tuple[int, int]] = []
    o = ptr
    for _ in range(n_entries + 8):
        if o + 8 > len(data):
            break
        off = u32(data, o)
        s1 = u16(data, o + 4)
        s2 = u16(data, o + 6)
        if off == 0 and s1 == 0:
            break
        ok = s1 == s2 or s2 == 4 * ((s1 + 3) // 4) or s2 >= s1
        if not ok or s1 <= 0 or off + s1 > len(data):
            break
        entries.append((off, s1))
        o += 8
        if len(entries) >= n_entries:
            break
    return entries
