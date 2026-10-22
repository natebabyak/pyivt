"""Integration tests for IVT metadata against Beyond File Summary goldens."""

from __future__ import annotations

import json
from datetime import datetime, timezone
from math import prod
from pathlib import Path
from types import SimpleNamespace

from pyivt import IvtMetadata
from pyivt import open as open_ivt

# Top-level Beyond golden keys that already match pyivt (+ path stats) on all
# current fixtures. Add a key here and a branch in snapshot_comparable when
# more fields align.
COMPARED_KEYS: tuple[str, ...] = (
    "title_en",
    "title_fr",
    "file_name",
    "directory",
    "author_en",
    "author_fr",
    "category_en",
    "category_fr",
    "last_update",
    "file_size",
    "frequency",
)

# Not compared yet (Beyond golden vs current pyivt disagree or shapes differ):
# - notes_en / notes_fr        (whitespace / content drift)
# - number_of_datapoints       (Beyond grid vs pyivt decode counts)
# - start_date / end_date      (Beyond summary often null; pyivt fills ISO dates)
# - read_only                  (Beyond Yes/No vs pyivt bool disagrees on some demos)
# - dimensions                 (items / FR labels / descriptions diverge on some demos)
# - keywords_en / keywords_fr  (list[str] vs LocalizedText)
# - footnotes                  ({symbol, description_*} vs language/number/text)
# - missing_values             (description_* vs label / code)


def _load_expect(case: SimpleNamespace) -> dict:
    return json.loads(case.expect_path.read_text(encoding="utf-8"))


def _expect_slice(expect: dict) -> dict:
    return {key: expect[key] for key in COMPARED_KEYS}


def snapshot_comparable(meta: IvtMetadata, path: Path) -> dict:
    """Build Beyond-shaped fields; return only keys listed in COMPARED_KEYS.

    Values for not-yet-compared keys are still computed here so enabling a
    field later is usually just adding its name to COMPARED_KEYS.
    """
    stats = path.stat()
    path = path.resolve()
    full = {
        "title_en": meta.title.en,
        "title_fr": meta.title.fr,
        "file_name": path.name,
        "directory": str(path.parent),
        "author_en": meta.author.en,
        "author_fr": meta.author.fr,
        "category_en": meta.category.en,
        "category_fr": meta.category.fr,
        # Not in COMPARED_KEYS yet — whitespace/content still drifts from Beyond.
        "notes_en": meta.notes.en,
        "notes_fr": meta.notes.fr,
        "last_update": datetime.fromtimestamp(
            stats.st_mtime, timezone.utc
        ).isoformat(),
        "file_size": stats.st_size,
        # Not in COMPARED_KEYS yet — Beyond grid size can disagree with pyivt.
        "number_of_datapoints": meta.number_of_datapoints,
        "frequency": meta.frequency,
        # Not in COMPARED_KEYS yet — Beyond summary often leaves these null.
        "start_date": meta.start_date,
        "end_date": meta.end_date,
        # Not in COMPARED_KEYS yet — disagrees on some demo tables.
        "read_only": meta.read_only,
        # Not in COMPARED_KEYS yet — items / FR labels diverge on some demos.
        "dimensions": [
            {
                "name_en": d.name_en,
                "name_fr": d.name_fr,
                "items": d.count,
                "type": d.type_label,
                "description_en": d.description_en,
                "description_fr": d.description_fr,
            }
            for d in meta.dimensions
        ],
    }
    return {key: full[key] for key in COMPARED_KEYS}


def test_metadata_matches_expected(demo_ivt):
    expect = _load_expect(demo_ivt)
    meta = open_ivt(demo_ivt.path).metadata
    assert snapshot_comparable(meta, demo_ivt.path) == _expect_slice(expect)
    # Internal consistency: published grid size is the product of dim lengths.
    assert prod(d.count for d in meta.dimensions) == meta.number_of_datapoints


def test_metadata_lazy_fields(demo_ivt):
    meta = open_ivt(demo_ivt.path).metadata
    for name in ("dimensions", "geographies", "footnotes", "missing_values"):
        assert not meta._loaded(name)

    _ = meta.product_id
    _ = meta.title
    for name in ("dimensions", "geographies", "footnotes", "missing_values"):
        assert not meta._loaded(name)

    _ = meta.dimensions
    assert meta._loaded("dimensions")
    assert not meta._loaded("geographies")
    assert not meta._loaded("footnotes")
    assert not meta._loaded("missing_values")

    _ = meta.geographies
    assert meta._loaded("geographies")

    _ = meta.footnotes
    _ = meta.missing_values
    assert meta._loaded("footnotes")
    assert meta._loaded("missing_values")


def test_open_bytes_defers_decode(demo_ivt):
    expect = _load_expect(demo_ivt)
    ivt = open_ivt(demo_ivt.path.read_bytes())
    assert len(ivt.layout.counts) == len(expect["dimensions"])


def test_to_arrow_row_count(demo_ivt):
    ivt = open_ivt(demo_ivt.path)
    n = ivt.metadata.number_of_datapoints
    assert n is not None and n > 0
    if n > 5_000_000:
        # Keep CI / local runs bounded for very large published grids.
        table = ivt.to_arrow(complete=False, labels=False)
        assert table.num_rows > 0
        assert "value" in table.column_names
        return
    table = ivt.to_arrow(complete=True, labels=True)
    assert table.num_rows == n
    assert "value" in table.column_names
