"""File summary, dimension members, geographies, footnotes."""

from __future__ import annotations

import math
import re
from dataclasses import dataclass, field
from typing import Any

from ._bytes import latin1, rd_pascal, u16, u32
from ._io import Source, read_source
from .descriptor import Descriptor, DimDesc, geo_dim_index, parse_descriptor
from .header import (
    HDR_MASTER_PTR,
    HDR_STATUS_LEGEND,
    HeaderLayout,
    dim_slots,
    read_block_dir,
    read_header,
)

# FACET04 glued category suffixes (Beyond File Summary "Category")
_FACET_CATEGORIES = (
    "Data Tables",
    "Tableaux de données",
)
# Author glued immediately before category on FACET04 runs
_FACET_AUTHOR_RE = re.compile(
    r"(?:"
    r"\d{4} Census - Statistics Canada|"
    r"Recensement de \d{4}-Statistique Canada"
    r")$"
)
_TIME_SERIES_NAME_RE = re.compile(
    r"timeseries|time\s*series|time\s*period", re.IGNORECASE
)
# File-flags trailer ``81 01 20 00``; bit 0x10 of the first payload byte tracks
# Beyond's Summary "Read-Only" on published StatCan tables vs demo IVTs.
_READONLY_FLAG_BIT = 0x10


@dataclass
class Dimension:
    name: str
    name_fr: str | None
    count: int
    type: int
    is_geography: bool
    members: list[str] | None = None
    members_fr: list[str] | None = None
    ordinal: list[int] | None = None


@dataclass
class Footnote:
    language: str
    number: int
    text: str
    scope: str | None = None
    dimension: str | None = None
    member_id: int | None = None


@dataclass
class LocalizedText:
    en: str | None = None
    fr: str | None = None


@dataclass
class MissingValue:
    symbol: str
    label: LocalizedText
    code: int | None = None


@dataclass
class IvtMetadata:
    """
    IVT file summary metadata.

    Summary fields are eager. ``dimensions``, ``geographies``, ``footnotes``,
    and ``missing_values`` load on first access and are cached.
    """

    product_id: str | None
    title: LocalizedText
    universe: str | None
    notes: LocalizedText
    version: str
    signature: str
    _data: bytes = field(repr=False, compare=False)
    _desc: Descriptor = field(repr=False, compare=False)
    author: LocalizedText = field(default_factory=LocalizedText)
    category: LocalizedText = field(default_factory=LocalizedText)
    keywords: LocalizedText = field(default_factory=LocalizedText)
    number_of_datapoints: int | None = None
    time_series: str | None = None
    read_only: bool | None = None
    _dimensions: list[Dimension] | None = field(
        default=None, repr=False, compare=False
    )
    _geographies: dict[str, Any] | None = field(
        default=None, repr=False, compare=False
    )
    _footnotes: list[Footnote] | None = field(default=None, repr=False, compare=False)
    _missing_values: list[MissingValue] | None = field(
        default=None, repr=False, compare=False
    )

    @property
    def dimensions(self) -> list[Dimension]:
        self._ensure_dims_geo()
        assert self._dimensions is not None
        return self._dimensions

    @property
    def geographies(self) -> dict[str, Any]:
        self._ensure_dims_geo()
        assert self._geographies is not None
        return self._geographies

    @property
    def footnotes(self) -> list[Footnote]:
        if self._footnotes is None:
            self._footnotes = _footnotes(self._data)
        return self._footnotes

    @property
    def missing_values(self) -> list[MissingValue]:
        if self._missing_values is None:
            self._missing_values = _missing_values(self._data)
        return self._missing_values

    def _ensure_dims_geo(self) -> None:
        if self._dimensions is not None and self._geographies is not None:
            return
        dims = build_dimensions(self._data, self._desc)
        gd = geo_dim_index(self._desc.dims)
        geos = _geo_from_dir(self._data, self._desc.dims[gd - 1], gd - 1)
        self._dimensions = dims
        self._geographies = geos

    def _loaded(self, name: str) -> bool:
        """Whether a lazy field has been materialized (for tests)."""
        return getattr(self, f"_{name}") is not None


def _identity_from_blob(text: str) -> tuple[str | None, str | None]:
    text = text.lstrip()
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n", 1)
    pid = lines[0].strip() if lines else None
    title = lines[1].strip() if len(lines) > 1 else None
    if pid and not re.match(r"^[\w.-]+$", pid.split()[0] if pid else ""):
        # whole blob may be title-only
        return None, text.strip()
    return pid, title


def _frscore(s: str) -> int:
    if not s:
        return 0
    hits = len(re.findall(r"[àâäéèêëïîôùûüçœæÀÂÄÉÈÊËÏÎÔÙÛÜÇ]", s))
    hits += len(
        re.findall(
            r"\b(de|des|les|une|pour|dans|recensement|année|âge)\b", s, re.IGNORECASE
        )
    )
    return hits


def _master_dir_entries(data: bytes, max_entries: int = 32) -> list[tuple[int, int]]:
    """Return ``(offset, length)`` pairs from the master directory."""
    master = u32(data, HDR_MASTER_PTR)
    out: list[tuple[int, int]] = []
    if not (0 < master < len(data) - 8):
        return out
    o = master
    for _ in range(max_entries):
        if o + 8 > len(data):
            break
        off, s1 = u32(data, o), u16(data, o + 4)
        o += 8
        if off == 0 and s1 == 0:
            continue
        if s1 <= 0 or off + s1 > len(data):
            continue
        out.append((off, s1))
    return out


def _split_facet_run(text: str) -> tuple[str | None, str | None, str | None]:
    """Split a FACET04 printable run into title / author / category."""
    text = text.strip()
    if not text:
        return None, None, None
    category = None
    rest = text
    for cat in _FACET_CATEGORIES:
        if rest.endswith(cat):
            category = cat
            rest = rest[: -len(cat)]
            break
    author = None
    m = _FACET_AUTHOR_RE.search(rest)
    if m:
        author = m.group(0)
        rest = rest[: m.start()]
    title = rest.strip() or None
    return title, author, category


def parse_facet_summary(data: bytes) -> dict:
    """
    Beyond File Summary title / author / category from ``FACET04`` runs.

    Each run concatenates short title + author + category without separators.
    """
    runs: list[str] = []
    start = 0
    while True:
        i = data.find(b"FACET04", start)
        if i < 0:
            break
        run = data[i + 7 :]
        end = 0
        while end < len(run) and run[end] >= 32:
            end += 1
        text = latin1(run[:end]).strip()
        if text:
            runs.append(text)
        start = i + 7
    title_en = title_fr = author_en = author_fr = category_en = category_fr = None
    parsed = [_split_facet_run(r) for r in runs]
    scored = sorted(
        ((t, a, c) for t, a, c in parsed if t),
        key=lambda x: _frscore(x[0] or ""),
    )
    if scored:
        en = scored[0]
        fr = None
        if len(scored) > 1 and _frscore(scored[-1][0] or "") > _frscore(
            scored[0][0] or ""
        ):
            fr = scored[-1]
        title_en, author_en, category_en = en
        if fr:
            title_fr, author_fr, category_fr = fr
        elif _frscore(en[0] or "") >= 2:
            title_fr, author_fr, category_fr = en
            title_en = author_en = category_en = None
    return {
        "title_en": title_en,
        "title_fr": title_fr,
        "author_en": author_en,
        "author_fr": author_fr,
        "category_en": category_en,
        "category_fr": category_fr,
    }


def parse_identity_notes(data: bytes) -> dict:
    """Full bilingual Notes text from master-directory identity blobs."""
    notes_en = notes_fr = None
    product_id = None
    long_blobs: list[str] = []
    for off, length in _master_dir_entries(data):
        blk = data[off : off + length]
        if blk[:2] != b"\x01\x01" or length < 40:
            continue
        payload = blk[4:]
        i = 0
        while i < len(payload) and payload[i] < 0x20:
            i += 1
        text = latin1(payload[i:]).rstrip("\x00").strip()
        if not text or ("\n" not in text and "\r" not in text):
            continue
        long_blobs.append(text)
        pid, _ = _identity_from_blob(text)
        if pid and re.match(r"^[\dA-Za-z][\w./-]*$", pid):
            product_id = product_id or pid
    scored = sorted(long_blobs, key=_frscore)
    if scored:
        en = scored[0]
        fr = None
        if len(scored) > 1 and _frscore(scored[-1]) > _frscore(scored[0]):
            fr = scored[-1]
        notes_en = en
        notes_fr = fr
        if fr is None and _frscore(en) >= 2:
            notes_fr = en
            notes_en = None
    return {"notes_en": notes_en, "notes_fr": notes_fr, "product_id": product_id}


def parse_keywords(data: bytes, product_id: str | None = None) -> LocalizedText:
    """Keywords from short master-directory ``01 01`` product-id entries."""
    candidates: list[str] = []
    for off, length in _master_dir_entries(data):
        if length > 40 or length < 8:
            continue
        blk = data[off : off + length]
        if blk[:2] != b"\x01\x01":
            continue
        payload = blk[4:]
        i = 0
        while i < len(payload) and payload[i] < 0x20:
            i += 1
        if i < len(payload) and 0 < payload[i] < 64:
            n = payload[i]
            if i + 1 + n <= len(payload):
                kw = latin1(payload[i + 1 : i + 1 + n]).rstrip("\x00").strip()
                if kw:
                    candidates.append(kw)
                    continue
        text = latin1(payload[i:]).rstrip("\x00").strip()
        if text and "\n" not in text and len(text) <= 40:
            candidates.append(text)
    if not candidates:
        return LocalizedText()
    # Prefer product-id match as English; bilingual pair via frscore when distinct.
    unique: list[str] = []
    for c in candidates:
        if c not in unique:
            unique.append(c)
    if product_id:
        for c in unique:
            if c == product_id:
                others = [x for x in unique if x != c]
                if others:
                    scored = sorted(others, key=_frscore)
                    fr = scored[-1] if _frscore(scored[-1]) > _frscore(c) else None
                    return LocalizedText(en=c, fr=fr)
                return LocalizedText(en=c)
    if len(unique) == 1:
        return LocalizedText(en=unique[0])
    scored = sorted(unique, key=_frscore)
    en, fr = scored[0], None
    if _frscore(scored[-1]) > _frscore(scored[0]):
        fr = scored[-1]
    return LocalizedText(en=en, fr=fr)


_FREQ_NAME_RE = re.compile(
    r"\b(annual|yearly|quarterly|monthly|weekly|daily|"
    r"annuel(?:le)?|trimestriel(?:le)?|mensuel(?:le)?|"
    r"hebdomadaire|quotidien(?:ne)?)\b",
    re.IGNORECASE,
)


def _time_dim_desc(dims: list[DimDesc]) -> DimDesc | None:
    for d in dims:
        if d.type == 6:
            return d
        if d.name and _TIME_SERIES_NAME_RE.search(d.name):
            return d
    return None


def _has_time_series_marker(data: bytes) -> bool:
    o = 0
    while True:
        i = data.find(b"\x81\x02", o)
        if i < 0 or i + 6 > len(data):
            break
        if data[i + 4 : i + 6] == b"\x08\x00":
            return True
        o = i + 2
    return False


def _frequency_hint_name(name: str | None) -> str | None:
    if not name:
        return None
    m = _FREQ_NAME_RE.search(name)
    return m.group(1).capitalize() if m else None


def _time_series_label(data: bytes, dims: list[DimDesc]) -> str:
    """
    Beyond Summary Time Series value (eager; descriptor names/types only).

    Non-series tables show ``No``. Series tables usually show a frequency or
    dimension name; member ranges require loading dimension members.
    """
    dim = _time_dim_desc(dims)
    if dim is None and not _has_time_series_marker(data):
        return "No"
    if dim is None:
        return "Yes"
    freq = _frequency_hint_name(dim.name)
    if freq:
        return freq
    if dim.name:
        return dim.name
    return "Yes"


def _read_only_flag(data: bytes) -> bool:
    """
    Beyond Summary Read-Only from the file-flags trailer ``81 01 20 00``.

    Bit ``0x10`` of the first payload byte is set on published StatCan tables
    (Summary Yes) and clear on Beyond demo IVTs (Summary No).
    """
    for off, length in _master_dir_entries(data):
        if length < 5:
            continue
        blk = data[off : off + length]
        if blk[:4] == b"\x81\x01\x20\x00":
            return bool(blk[4] & _READONLY_FLAG_BIT)
    return False


def parse_table_identity(data: bytes) -> dict:
    """Read product id / bilingual titles from header text or master directory."""
    head = latin1(data[:8000])
    grab = lambda re_s: m.group(1).strip() if (m := re.search(re_s, head)) else None
    info = {
        "product_id": grab(r"Product ID:\s*([^\r\n]+)"),
        "title_en": grab(r"Title:\s*([^\r\n]+)"),
        "title_fr": grab(r"Titre\s*:\s*([^\r\n]+)"),
        "universe": grab(r"Universe:\s*([^\r\n]+)"),
        "notes": None,
    }

    blobs: list[str] = []
    for off, s1 in _master_dir_entries(data):
        blk = data[off : off + s1]
        if blk[:2] == b"\x01\x01" and s1 > 8:
            payload = blk[4:]
            i = 0
            while i < len(payload) and payload[i] < 0x20:
                i += 1
            text = latin1(payload[i:]).strip()
            if text:
                blobs.append(text)
        elif blk[:1] == b" " or (blk[:1].isalpha() if blk else False):
            text = latin1(blk).strip()
            if "98-" in text or "Title" in text:
                blobs.append(text)

    facet = parse_facet_summary(data)

    pid = info["product_id"]
    title_en = info["title_en"]
    title_fr = info["title_fr"]

    for blob in blobs:
        p, t = _identity_from_blob(blob)
        if (
            p
            and (pid is None or len(p) > len(pid or ""))
            and re.match(r"^[\dA-Za-z][\w./-]*$", p)
        ):
            pid = p
        # Identity-blob titles are the long Notes line; keep only as fallback
        if t and title_en is None and title_fr is None:
            if _frscore(t) >= 2:
                title_fr = t
            else:
                title_en = t

    if facet.get("title_en"):
        title_en = facet["title_en"]
    if facet.get("title_fr"):
        title_fr = facet["title_fr"]

    if pid is None:
        m = re.search(r"\b(\d{2}-\d{3}-[A-Z]\d{7,}|\d{8})\b", head)
        if m:
            pid = m.group(1)

    info["product_id"] = pid
    info["title_en"] = title_en
    info["title_fr"] = title_fr
    info["author_en"] = facet.get("author_en")
    info["author_fr"] = facet.get("author_fr")
    info["category_en"] = facet.get("category_en")
    info["category_fr"] = facet.get("category_fr")
    return info


def _read_member_array(block: bytes, expect: int) -> list[str]:
    """
    Parse a member-label array block.

    Standard framing: ``01 01`` / ``81 01`` + u16 length + u16 field + Pascal strings.
    Definition/note blobs share the ``01 01`` tag but have no second u16 and are rejected.
    """
    if expect < 1 or len(block) < 8:
        return []
    if block[:2] not in (b"\x01\x01", b"\x81\x01"):
        return []
    starts = (6, 7, 8, 4, 10, 12)
    for start in starts:
        labels: list[str] = []
        p = start
        while p < len(block) and len(labels) < expect:
            if block[p] == 0 and p + 1 < len(block):
                p += 1
                continue
            rec = rd_pascal(block, p)
            if rec is None:
                break
            labels.append(rec["text"])
            p = rec["end"]
        if len(labels) == expect and _labels_plausible(labels):
            return labels
    if block[:2] == b"\x81\x01":
        for start in range(4, min(64, len(block) - expect)):
            labels = []
            p = start
            while p < len(block) and len(labels) < expect:
                rec = rd_pascal(block, p)
                if rec is None:
                    break
                labels.append(rec["text"])
                p = rec["end"]
            if len(labels) == expect and _labels_plausible(labels):
                return labels
    return []


def _labels_plausible(labels: list[str]) -> bool:
    if not labels:
        return False
    for lab in labels:
        u = lab.upper()
        if "DEFINITION" in u or "DÉFINITION" in u or "DEFINITION" in lab:
            return False
        if "\n" in lab or "\r" in lab:
            return False
    joined = " ".join(labels[:5])
    if "Digit description" in joined or "Flag description" in joined:
        return False
    if "Incomplete enumeration" in joined:
        return False
    # Reject arrays that are mostly empty/single-char slots
    return sum(1 for x in labels if len(x.strip()) <= 1) <= max(1, len(labels) // 3)


def _dim_dir_labels(
    data: bytes, dim: DimDesc, slot_index: int, *, geography: bool = False
) -> tuple[list[str] | None, list[str] | None, str | None]:
    slots = dim_slots(data)
    if slot_index >= len(slots):
        return None, None, None
    slot = slots[slot_index]
    entries = read_block_dir(data, slot.dir_ptr, slot.n_entries)
    arrays: list[list[str]] = []
    for off, length in entries:
        if length < dim.count + 4:
            continue
        block = bytes(data[off : off + length])
        if block[:2] == b"\x81\x02":
            continue
        labels = _read_member_array(block, dim.count)
        if len(labels) != dim.count:
            continue
        if geography:
            sample = " ".join(labels[:3])
            if "Canada" not in sample and not any(
                re.search(r"\(\d", x) or re.search(r"\[\d", x) for x in labels[:5]
            ):
                continue
        arrays.append(labels)
    if not arrays:
        return None, None, None
    en = arrays[0]
    fr = arrays[1] if len(arrays) > 1 else None
    name_fr = None
    if fr and en and _frscore(" ".join(fr[:3])) < _frscore(" ".join(en[:3])):
        en, fr = fr, en
    return en, fr, name_fr


def _parse_inline_geo(label: str) -> dict:
    """Parse 'Name (code) … flag ( tnr%)' style geography labels."""
    out = {
        "geo_label": label,
        "geo_name": None,
        "geo_uid": None,
        "dqf_code": None,
        "tnr_short_form": None,
    }
    m = re.match(
        r"^(.*)\s+\(([0-9A-Za-z.]+)\)(?:,\s*|\s+)(?:([^0-9\s]\S*)\s+)?([0-9]+)"
        r"(?:\s*\(([^)]*)\))?\s*$",
        label,
    )
    if m:
        out["geo_name"] = m.group(1).rstrip()
        out["geo_uid"] = m.group(2)
        out["dqf_code"] = m.group(4)
        if m.group(5):
            tm = re.match(r"\s*([0-9]+(?:[.,][0-9]+)?)\s*%\s*$", m.group(5))
            if tm:
                out["tnr_short_form"] = tm.group(1).replace(",", ".")
        return out
    m2 = re.match(r"^(.*?)\s*\(([0-9A-Za-z.]+)\)\s*$", label)
    if m2:
        out["geo_name"] = m2.group(1).rstrip()
        out["geo_uid"] = m2.group(2)
    else:
        out["geo_name"] = label.strip()
    return out


def _geo_from_dir(data: bytes, dim: DimDesc, slot_index: int = 0) -> dict:
    en, fr, _ = _dim_dir_labels(data, dim, slot_index, geography=True)
    n = dim.count
    geo: dict = {"member_id": list(range(1, n + 1))}
    if en and len(en) >= n:
        en = en[:n]
        parsed = [_parse_inline_geo(x) for x in en]
        geo["geo_label"] = en
        geo["geo_name"] = [p["geo_name"] for p in parsed]
        geo["geo_uid"] = [p["geo_uid"] for p in parsed]
        if any(p["dqf_code"] for p in parsed):
            geo["dqf_code"] = [p["dqf_code"] for p in parsed]
        if any(p["tnr_short_form"] for p in parsed):
            geo["tnr_short_form"] = [p["tnr_short_form"] for p in parsed]
    if fr and len(fr) >= n:
        geo["geo_label_fr"] = fr[:n]
    return geo


def _footnotes(data: bytes) -> list[Footnote]:
    start = max(0, len(data) - 200_000)
    text_region = data[start:]
    regions = [data[:12000], text_region]
    out: list[Footnote] = []
    counts = {"en": 0, "fr": 0}
    for region in regions:
        s = latin1(region)
        for m in re.finditer(
            r"(Footnote|Renvoi)\s*(\d+)\s*(.*?)(?=(?:Footnote|Renvoi)\s*\d+|\Z)",
            s,
            flags=re.IGNORECASE | re.DOTALL,
        ):
            lang = "en" if m.group(1).lower().startswith("foot") else "fr"
            body = re.sub(r"\s+", " ", m.group(3)).strip()
            if len(body) < 8:
                continue
            counts[lang] += 1
            out.append(Footnote(language=lang, number=counts[lang], text=body[:4000]))
    seen: set[str] = set()
    uniq: list[Footnote] = []
    for f in out:
        key = f.text[:80]
        if key in seen:
            continue
        seen.add(key)
        uniq.append(f)
    return uniq[:200]


def _parse_status_legend_record(blob: bytes) -> tuple[str, str] | None:
    """Parse one ``82 01`` / ``02 01`` status-legend record into ``(symbol, text)``."""
    if len(blob) < 8 or blob[:2] not in (b"\x82\x01", b"\x02\x01"):
        return None
    body = blob[4:]
    for i in range(0, min(16, len(body) - 4)):
        n = body[i]
        if not (1 <= n <= 8) or i + 1 + n + 2 > len(body):
            continue
        sym = body[i + 1 : i + 1 + n]
        if sym[-1] != 0:
            continue
        symbol = latin1(sym[:-1])
        if not symbol or not all(32 < ord(c) < 127 for c in symbol):
            continue
        text_len = u16(body, i + 1 + n)
        start = i + 1 + n + 2
        take = min(text_len, len(body) - start)
        if take < 1:
            continue
        raw = body[start : start + take]
        out = bytearray()
        for b in raw:
            if b == 0 or b < 9 or b == 127:
                break
            out.append(b)
        text = latin1(out).strip()
        if len(text) < 3:
            continue
        return symbol, text
    return None


def _status_legend_dir(data: bytes, ptr: int, n_entries: int) -> list[tuple[int, int]]:
    """
    Read the ``@698`` entry array, keeping blank slots as ``(0, 0)``.

    Unlike ``read_block_dir``, blanks mid-directory (entry 1) must be preserved
    so code-index entry numbers stay aligned.
    """
    entries: list[tuple[int, int]] = []
    o = ptr
    for _ in range(n_entries):
        if o + 8 > len(data):
            break
        off = u32(data, o)
        s1 = u16(data, o + 4)
        s2 = u16(data, o + 6)
        o += 8
        if off == 0 and s1 == 0:
            entries.append((0, 0))
            continue
        ok = s1 == s2 or s2 == 4 * ((s1 + 3) // 4) or s2 >= s1
        if not ok or s1 <= 0 or off + s1 > len(data):
            break
        entries.append((off, s1))
    return entries


def _missing_values(data: bytes) -> list[MissingValue]:
    """Cell-status legend from header slot ``@698`` (Beyond Missing Values)."""
    if len(data) < HDR_STATUS_LEGEND + 14:
        return []
    dir_ptr = u32(data, HDR_STATUS_LEGEND)
    n_entries = u32(data, HDR_STATUS_LEGEND + 8)
    if dir_ptr == 0 or n_entries < 1:
        return []
    entries = _status_legend_dir(data, dir_ptr, n_entries)
    if not entries:
        return []
    # Entry 0 is the code index: ``04 02 | u16 n_codes | u16 per_code | …``
    idx_off, idx_len = entries[0]
    if idx_off == 0 or idx_len < 6:
        return []
    index = data[idx_off : idx_off + idx_len]
    if index[:2] != b"\x04\x02":
        return []
    n_codes = u16(index, 2)
    per_code = u16(index, 4)
    if n_codes < 1 or per_code < 1:
        return []
    need = 6 + n_codes * per_code * 4
    if len(index) < need:
        return []

    out: list[MissingValue] = []
    pos = 6
    for code in range(1, n_codes + 1):
        eidxs = [u32(index, pos + 4 * j) for j in range(per_code)]
        pos += 4 * per_code
        labels: list[str | None] = []
        symbol: str | None = None
        for ei in eidxs:
            if ei < 0 or ei >= len(entries):
                labels.append(None)
                continue
            off, length = entries[ei]
            if off == 0 or length < 1:
                labels.append(None)
                continue
            rec = _parse_status_legend_record(data[off : off + length])
            if rec is None:
                labels.append(None)
                continue
            sym, text = rec
            if symbol is None:
                symbol = sym
            labels.append(text)
        if symbol is None or not any(labels):
            continue
        en = labels[0] if labels else None
        fr = labels[1] if len(labels) > 1 else None
        out.append(
            MissingValue(
                symbol=symbol,
                label=LocalizedText(en=en, fr=fr),
                code=code,
            )
        )
    return out


def build_dimensions(data: bytes, desc: Descriptor) -> list[Dimension]:
    gd = geo_dim_index(desc.dims)
    dims: list[Dimension] = []
    for i, d in enumerate(desc.dims):
        is_geo = (i + 1) == gd
        members = members_fr = name_fr = None
        if not is_geo:
            members, members_fr, name_fr = _dim_dir_labels(data, d, i, geography=False)
        dims.append(
            Dimension(
                name="Geography" if is_geo else d.name,
                name_fr=name_fr if not is_geo else "Géographie",
                count=d.count,
                type=d.type,
                is_geography=is_geo,
                members=members,
                members_fr=members_fr,
            )
        )
    return dims


def build_metadata(
    data: bytes,
    *,
    desc: Descriptor | None = None,
    hdr: HeaderLayout | None = None,
) -> IvtMetadata:
    """
    Build lazy metadata from IVT bytes.

    Parses the header and dimension descriptor when not provided. Heavy blocks
    (dimensions, geographies, footnotes, missing values) load on first access.
    """
    if hdr is None:
        hdr = read_header(data)
    if hdr.signature not in (b"\x04\x00\x20\x00", b"\x02\x00\x20\x00"):
        raise ValueError(f"unrecognised IVT signature: {hdr.signature.hex()}")
    if desc is None:
        desc = parse_descriptor(data)
    if desc is None:
        raise ValueError("could not parse IVT dimension descriptor")

    info = parse_table_identity(data)
    notes_info = parse_identity_notes(data)
    product_id = info.get("product_id") or notes_info.get("product_id")
    keywords = parse_keywords(data, product_id)
    number_of_datapoints = (
        int(math.prod(d.count for d in desc.dims)) if desc.dims else 0
    )

    return IvtMetadata(
        product_id=product_id,
        title=LocalizedText(en=info.get("title_en"), fr=info.get("title_fr")),
        universe=info.get("universe"),
        notes=LocalizedText(
            en=notes_info.get("notes_en"), fr=notes_info.get("notes_fr")
        ),
        version=hdr.version,
        signature=hdr.signature.hex(),
        author=LocalizedText(en=info.get("author_en"), fr=info.get("author_fr")),
        category=LocalizedText(en=info.get("category_en"), fr=info.get("category_fr")),
        keywords=keywords,
        number_of_datapoints=number_of_datapoints,
        time_series=_time_series_label(data, desc.dims),
        read_only=_read_only_flag(data),
        _data=data,
        _desc=desc,
    )


def read_metadata(source: Source) -> IvtMetadata:
    """
    Extract file summary metadata without decoding cell values.

    Accepts a path, bytes-like buffer, or binary file object. Heavy fields
    (``dimensions``, ``geographies``, ``footnotes``, ``missing_values``) are
    loaded lazily on first access.
    """
    return build_metadata(read_source(source))
