"""Dimension descriptor: names, counts, types from the header block."""

from __future__ import annotations

import re
from dataclasses import dataclass

from ._bytes import nextpow2, u16, u32
from .header import HDR_DESCRIPTOR_PTR, dir_anchor_header

GEOTYPES = {0x10, 0x0D, 0x0A, 0x0C, 0x09, 0x0F, 0x0B}


@dataclass
class DimDesc:
    name: str
    count: int
    type: int
    double01: bool = False
    slots: list[int] | None = None


@dataclass
class Descriptor:
    offset: int
    dims: list[DimDesc]


def _descriptor_name(run: bytes, first_record: bool = False) -> str | None:
    """Recover a display name from a doubled (or truncated-first) name run."""
    if not run:
        return None
    s = run.decode("ascii", errors="ignore")
    n = len(s)
    if n < 2:
        return None
    # Exact doubling: NameName
    for half in range(n // 2, 0, -1):
        if s[:half] == s[half : 2 * half] and 2 * half == n:
            return s[:half]
        if (
            s[:half] == s[half : 2 * half]
            and s[:half]
            and s[:half] == s[half : 2 * half]
        ):
            rest = s[2 * half :]
            if not rest or not rest[0].isalnum():
                return s[:half]
    # Truncated first copy + full second (Employment inco + Employment income...)
    for first_len in range(min(n, 40), 3, -1):
        first = s[:first_len]
        if s[first_len:].startswith(first):
            # extend second copy to a plausible end
            rest = s[first_len:]
            # prefer longest printable word-ish run
            m = re.match(r"^(.+?)(?:\s{2,}|\Z)", rest)
            cand = m.group(1) if m else rest
            if len(cand) >= first_len:
                return cand
    # Geography single-name (sometimes not doubled cleanly) — first record
    if first_record and s.startswith("Geography"):
        return "Geography"
    # Short+display: take the longer half if both halves look like names
    mid = n // 2
    a, b = s[:mid], s[mid:]
    if a and b and a[0].isupper() and b[0].isupper():
        return b if len(b) >= len(a) else a
    return s[: min(48, n)].strip() or None


def _walk_records(
    v: list[int], lend: int, cap: int, lenient: bool = False
) -> list[DimDesc]:
    dims: list[DimDesc] = []
    k = 3  # 0-based within v; need lookbehind
    while k < lend - 1 and len(dims) < cap:
        c1 = v[k + 1] if k + 1 < len(v) else 0
        namestart = (
            (65 <= c1 <= 90) or (48 <= c1 <= 57) or (lenient and 97 <= c1 <= 122)
        )
        bare02 = (
            v[k] == 0x02
            and v[k - 1] != 0x01
            and 1 <= v[k - 1] <= 0x10
            and v[k - 2] >= 1
        )
        anchor_a = (
            v[k] == 0x01
            or (v[k] == 0x02 and v[k - 1] == 0x01 and 1 <= v[k - 2] < 0x20)
            or bare02
        ) and namestart
        anchor_b = (
            lenient
            and not anchor_a
            and namestart
            and k >= 3
            and v[k] in GEOTYPES
            and v[k - 1] > 0
            and (v[k - 2] + v[k - 1] * 256) <= 200_000
        )
        if anchor_a or anchor_b:
            e = k + 1
            while e < len(v) and 32 <= v[e] <= 126:
                e += 1
            run = bytes(v[k + 1 : e])
            if not lenient or len(run) >= 4:
                double01 = anchor_a and v[k - 1] == 0x01
                if anchor_b:
                    typ = v[k]
                    count = v[k - 2] + v[k - 1] * 256
                elif double01:
                    typ = v[k - 3]
                    count = v[k - 2]
                else:
                    typ = v[k - 1]
                    if typ in GEOTYPES:
                        count = v[k - 3] + v[k - 2] * 256
                    else:
                        count = v[k - 2]
                nm = _descriptor_name(run, first_record=len(dims) == 0)
                if nm is None and lenient:
                    nm = run[:24].decode("ascii", errors="ignore").strip() or None
                if nm is not None and count >= 1:
                    dims.append(
                        DimDesc(
                            name=nm, count=int(count), type=int(typ), double01=double01
                        )
                    )
                    k = e
                    continue
        k += 1
    return dims


def parse_descriptor(data: bytes) -> Descriptor | None:
    if data and data[0] == 0x02:
        # 02-generation not fully supported in v1
        return None
    d_off = u32(data, HDR_DESCRIPTOR_PTR)
    n = len(data)
    if d_off < 1 or d_off + 18 > n:
        return None
    # ndim field is often unreliable; walk for doubled names instead
    win = min(d_off + 4000, n)
    v = list(data[d_off:win])
    text = "".join(chr(b) if 32 <= b <= 126 else "." for b in v)
    facet = text.find("FACET04")
    if facet > 0:
        lend = facet
    else:
        anchor = dir_anchor_header(data)
        if anchor is not None:
            vb = u32(data, anchor)
            if vb > d_off + 32:
                lend = min(vb - d_off, len(v))
            else:
                lend = len(v)
        else:
            lend = len(v)
    # authoritative ndim when plausible
    ndim_field = u16(data, d_off + 16)
    cap = ndim_field if 1 <= ndim_field <= 32 else 16
    dims = _walk_records(v, lend, cap)
    if len(dims) < 2:
        dims = _walk_records(v, lend, cap, lenient=True)
    if len(dims) < 2:
        return None
    return Descriptor(offset=d_off, dims=dims)


def geo_dim_index(dims: list[DimDesc]) -> int:
    """1-based geography dimension index (profile lineage not handled in v1)."""
    if not dims:
        return 1
    # Profile: first dim is 1-member "Values" placeholder → geography last
    if dims[0].count == 1 and dims[0].name.lower() in {"values", "value"}:
        return len(dims)
    return 1


def dim_slugs(names: list[str]) -> list[str]:
    """Short structural slugs for data dimension columns."""
    used: dict[str, int] = {}
    out: list[str] = []
    for name in names:
        base = re.sub(r"[^A-Za-z0-9]+", "_", name.strip()).strip("_").lower()
        base = base[:40] or "dim"
        if base in used:
            used[base] += 1
            slug = f"{base}_{used[base]}"
        else:
            used[base] = 1
            slug = base
        out.append(slug)
    return out


def layout_pad(count: int) -> int:
    return nextpow2(count)
