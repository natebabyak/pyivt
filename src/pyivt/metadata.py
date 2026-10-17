"""File summary, dimension members, geographies, footnotes."""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

from ._bytes import latin1, rd_pascal, u16, u32
from .descriptor import Descriptor, DimDesc, geo_dim_index, parse_descriptor
from .header import (
    HDR_MASTER_PTR,
    dim_slots,
    read_block_dir,
    read_header,
)

PathLike = str | Path


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
class IvtMetadata:
    product_id: str | None
    title_en: str | None
    title_fr: str | None
    universe: str | None
    notes: str | None
    version: str
    signature: str
    dimensions: list[Dimension] = field(default_factory=list)
    geographies: dict = field(default_factory=dict)
    footnotes: list[Footnote] = field(default_factory=list)
    path: str | None = None

    def to_dict(self) -> dict:
        return {
            "product_id": self.product_id,
            "title_en": self.title_en,
            "title_fr": self.title_fr,
            "universe": self.universe,
            "notes": self.notes,
            "version": self.version,
            "signature": self.signature,
            "dimensions": [
                {
                    "name": d.name,
                    "name_fr": d.name_fr,
                    "count": d.count,
                    "type": d.type,
                    "is_geography": d.is_geography,
                    "members": d.members,
                    "members_fr": d.members_fr,
                }
                for d in self.dimensions
            ],
            "geographies": self.geographies,
            "footnotes": [
                {"language": f.language, "number": f.number, "text": f.text}
                for f in self.footnotes
            ],
        }


def _load(path: PathLike) -> bytes:
    return Path(path).read_bytes()


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


def parse_table_identity(data: bytes) -> dict:
    """Read product id / bilingual titles from header text or master directory."""
    # Classic labelled header (some vintages)
    head = latin1(data[:8000])
    grab = lambda re_s: m.group(1).strip() if (m := re.search(re_s, head)) else None
    info = {
        "product_id": grab(r"Product ID:\s*([^\r\n]+)"),
        "title_en": grab(r"Title:\s*([^\r\n]+)"),
        "title_fr": grab(r"Titre\s*:\s*([^\r\n]+)"),
        "universe": grab(r"Universe:\s*([^\r\n]+)"),
        "notes": None,
    }

    # Master-directory identity blobs (modern 98-400 / FACET04 lineage)
    master = u32(data, HDR_MASTER_PTR)
    blobs: list[str] = []
    if 0 < master < len(data) - 8:
        o = master
        for _ in range(32):
            if o + 8 > len(data):
                break
            off, s1 = u32(data, o), u16(data, o + 4), u16(data, o + 6)
            o += 8
            if off == 0 and s1 == 0:
                continue
            if s1 <= 4 or off + s1 > len(data):
                continue
            blk = data[off : off + s1]
            if blk[:2] == b"\x01\x01" and s1 > 8:
                # [01 01][u16 len][…] payload
                payload = blk[4:]
                # skip optional leading control bytes
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

    # FACET04 title lines in descriptor region
    facet = data.find(b"FACET04")
    facet_titles: list[str] = []
    if facet >= 0:
        run = data[facet + 7 : facet + 7 + 800]
        end = 0
        while end < len(run) and run[end] >= 32:
            end += 1
        facet_titles.append(latin1(run[:end]).strip())

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
        if t and _frscore(t) > _frscore(title_fr or ""):
            if title_en is None and title_fr is None:
                # assign later by score among candidates
                pass
            if _frscore(t) >= 2:
                title_fr = t
            elif title_en is None or (
                _frscore(title_en) < 2 and len(t) > len(title_en or "")
            ):
                title_en = t

    # Prefer scoring two largest identity blobs as EN/FR titles
    long_blobs = sorted(
        [b for b in blobs if "\n" in b or len(b) > 40],
        key=len,
        reverse=True,
    )[:4]
    scored = sorted(long_blobs, key=_frscore)
    if len(scored) >= 2:
        en_cand, fr_cand = scored[0], scored[-1]
        if _frscore(fr_cand) > _frscore(en_cand):
            pe, te = _identity_from_blob(en_cand)
            pf, tf = _identity_from_blob(fr_cand)
            if pe:
                pid = pid or pe
            if pf and not pid:
                pid = pf
            if te:
                title_en = te
            if tf:
                title_fr = tf

    if facet_titles and title_en is None:
        title_en = facet_titles[0]

    # Product id fallback from filename-like tokens
    if pid is None:
        m = re.search(r"\b(\d{2}-\d{3}-[A-Z]\d{7,}|\d{8})\b", head)
        if m:
            pid = m.group(1)

    info["product_id"] = pid
    info["title_en"] = title_en
    info["title_fr"] = title_fr
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
    # 01 01 arrays: tag + two u16s then Pascals. 81 01 may insert a short mask.
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
    # 81 01 / masked blocks: scan for the first run of `expect` Pascal records
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
    return sum(1 for x in labels if len(x.strip()) <= 1) > max(1, len(labels) // 3)


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
        # Member arrays are almost always 01 01 / 81 01 framed
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
    # Schema order: English Desc then Desc Francais
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
    # Search near codebook / last 200 KB
    start = max(0, len(data) - 200_000)
    text_region = data[start:]
    # Also scan early header notes
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
    # Deduplicate by text prefix
    seen: set[str] = set()
    uniq: list[Footnote] = []
    for f in out:
        key = f.text[:80]
        if key in seen:
            continue
        seen.add(key)
        uniq.append(f)
    return uniq[:200]


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


def read_metadata(path: PathLike) -> IvtMetadata:
    """Extract file summary + dimension codebook without decoding cell values."""
    p = Path(path)
    data = _load(p)
    hdr = read_header(data)
    if hdr.signature not in (b"\x04\x00\x20\x00", b"\x02\x00\x20\x00"):
        raise ValueError(f"unrecognised IVT signature: {hdr.signature.hex()}")
    info = parse_table_identity(data)
    desc = parse_descriptor(data)
    if desc is None:
        raise ValueError("could not parse IVT dimension descriptor")
    dimensions = build_dimensions(data, desc)
    gd = geo_dim_index(desc.dims)
    geographies = _geo_from_dir(data, desc.dims[gd - 1], gd - 1)
    footnotes = _footnotes(data)
    # Attach notes from French/English identity blobs if present
    notes = None
    head = latin1(data[:6000])
    for marker in ("Note on data quality", "Note sur la qualité"):
        i = head.find(marker)
        if i >= 0:
            notes = re.sub(r"\s+", " ", head[i : i + 800]).strip()
            break
    return IvtMetadata(
        product_id=info.get("product_id"),
        title_en=info.get("title_en"),
        title_fr=info.get("title_fr"),
        universe=info.get("universe"),
        notes=notes,
        version=hdr.version,
        signature=hdr.signature.hex(),
        dimensions=dimensions,
        geographies=geographies,
        footnotes=footnotes,
        path=str(p),
    )
