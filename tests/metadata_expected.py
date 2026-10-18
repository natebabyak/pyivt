"""Serialize IvtMetadata into golden JSON shapes for StatCan integration tests."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from pyivt.metadata import IvtMetadata, LocalizedText

EXPECTED_DIR = Path(__file__).resolve().parent / "fixtures" / "statcan" / "expected"

_SENTENCE_SPLIT = re.compile(r"(?<=[.!?])\s+")


def first_last_sentences(text: str | None) -> tuple[str | None, str | None]:
    """Return the first and last sentences of *text* (None-safe)."""
    if text is None:
        return None, None
    normalized = re.sub(
        r"\s+", " ", text.replace("\r\n", "\n").replace("\r", "\n")
    ).strip()
    if not normalized:
        return None, None
    parts = [p.strip() for p in _SENTENCE_SPLIT.split(normalized) if p.strip()]
    if not parts:
        return None, None
    if len(parts) == 1:
        return parts[0], parts[0]
    return parts[0], parts[-1]


def _localized(text: LocalizedText) -> dict[str, str | None]:
    return {"en": text.en, "fr": text.fr}


def _notes_endpoints(text: LocalizedText) -> dict[str, dict[str, str | None]]:
    en_first, en_last = first_last_sentences(text.en)
    fr_first, fr_last = first_last_sentences(text.fr)
    return {
        "en": {"first": en_first, "last": en_last},
        "fr": {"first": fr_first, "last": fr_last},
    }


def snapshot_metadata(meta: IvtMetadata) -> dict[str, Any]:
    """Build the golden-file payload for one IVT's metadata."""
    return {
        "title": _localized(meta.title),
        "author": _localized(meta.author),
        "category": _localized(meta.category),
        "keywords": _localized(meta.keywords),
        "notes": _notes_endpoints(meta.notes),
        "number_of_datapoints": meta.number_of_datapoints,
        "time_series": meta.time_series,
        "read_only": meta.read_only,
        "dimensions": [
            {
                "name": d.name,
                "items": d.count,
                "type": d.type_label,
                "description": d.description,
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


def expected_path(product_id: str) -> Path:
    return EXPECTED_DIR / f"{product_id}.json"
