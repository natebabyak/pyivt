"""Serialize IvtMetadata into golden JSON shapes for integration tests."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from pyivt import IvtMetadata, LocalizedText, TimeSeries

_TESTS = Path(__file__).resolve().parent
STATCAN_EXPECTED_DIR = _TESTS / "fixtures" / "statcan" / "expected"
DEMO_EXPECTED_DIR = _TESTS / "fixtures" / "demo" / "expected"

# Back-compat alias used by StatCan helpers/scripts.
EXPECTED_DIR = STATCAN_EXPECTED_DIR


def first_last_words(text: str | None) -> tuple[str | None, str | None]:
    """Return the first and last whitespace-split words of *text* (None-safe)."""
    if text is None:
        return None, None
    normalized = re.sub(
        r"\s+", " ", text.replace("\r\n", "\n").replace("\r", "\n")
    ).strip()
    if not normalized:
        return None, None
    parts = normalized.split()
    if not parts:
        return None, None
    return parts[0], parts[-1]


def _localized(text: LocalizedText) -> dict[str, str | None]:
    return {"en": text.en, "fr": text.fr}


def _notes_endpoints(text: LocalizedText) -> dict[str, dict[str, str | None]]:
    en_first, en_last = first_last_words(text.en)
    fr_first, fr_last = first_last_words(text.fr)
    return {
        "en": {"first": en_first, "last": en_last},
        "fr": {"first": fr_first, "last": fr_last},
    }


def _dim_name(d) -> dict[str, str | None]:
    return {"en": d.name, "fr": d.name_fr}


def _dim_description(d) -> dict[str, str | None]:
    # Prefer explicit bilingual description when present; else EN description + FR name.
    en = d.description
    fr = getattr(d, "description_fr", None)
    if fr is None:
        fr = d.name_fr
    return {"en": en, "fr": fr}


def _time_series(ts: TimeSeries | None) -> dict[str, str | None] | None:
    if ts is None:
        return None
    return {"frequency": ts.frequency, "start": ts.start, "end": ts.end}


def snapshot_metadata(meta: IvtMetadata) -> dict[str, Any]:
    """Build the golden-file payload for one IVT's metadata."""
    return {
        "title": _localized(meta.title),
        "author": _localized(meta.author),
        "category": _localized(meta.category),
        "keywords": _localized(meta.keywords),
        "notes": _notes_endpoints(meta.notes),
        "number_of_datapoints": meta.number_of_datapoints,
        "time_series": _time_series(meta.time_series),
        "read_only": meta.read_only,
        "dimensions": [
            {
                "name": _dim_name(d),
                "items": d.count,
                "type": d.type_label,
                "description": _dim_description(d),
            }
            for d in meta.dimensions
        ],
        "footnotes": [
            {
                "language": f.language,
                "number": f.number,
                "text": f.text,
                "scope": f.scope,
                "dimension": f.dimension,
                "member_id": f.member_id,
            }
            for f in meta.footnotes
        ],
        "missing_values": [
            {
                "symbol": m.symbol,
                "code": m.code,
                "label": _localized(m.label),
            }
            for m in meta.missing_values
        ],
    }


def expected_path(product_id: str, suite: str = "statcan") -> Path:
    if suite == "demo":
        return DEMO_EXPECTED_DIR / f"{product_id}.json"
    return STATCAN_EXPECTED_DIR / f"{product_id}.json"
