"""Integration tests for Beyond demo IVT metadata (Products/People/Sample/Oecd)."""

from __future__ import annotations

import json
from math import prod

from metadata_expected import expected_path, first_last_sentences, snapshot_metadata
from pyivt import open as open_ivt


def test_demo_metadata_matches_expected(demo_ivt):
    path = expected_path(demo_ivt.id, suite="demo")
    assert path.is_file(), (
        f"missing golden {path.name}; run: "
        "python tests/scripts/dump_expected_metadata.py --suite demo"
    )
    expect = json.loads(path.read_text(encoding="utf-8"))
    meta = open_ivt(demo_ivt.path).metadata
    assert snapshot_metadata(meta) == expect

    en_first, en_last = first_last_sentences(meta.notes.en)
    fr_first, fr_last = first_last_sentences(meta.notes.fr)
    assert {"first": en_first, "last": en_last} == expect["notes"]["en"]
    assert {"first": fr_first, "last": fr_last} == expect["notes"]["fr"]
    assert prod(d.count for d in meta.dimensions) == expect["number_of_datapoints"]


def test_demo_metadata_lazy_fields(demo_ivt):
    meta = open_ivt(demo_ivt.path).metadata
    for name in ("dimensions", "geographies", "footnotes", "missing_values"):
        assert not meta._loaded(name)

    _ = meta.title
    _ = meta.author
    for name in ("dimensions", "geographies", "footnotes", "missing_values"):
        assert not meta._loaded(name)

    _ = meta.dimensions
    assert meta._loaded("dimensions")
    assert meta._loaded("geographies")
    assert not meta._loaded("footnotes")
    assert not meta._loaded("missing_values")

    _ = meta.footnotes
    _ = meta.missing_values
    assert meta._loaded("footnotes")
    assert meta._loaded("missing_values")


def test_demo_to_arrow_row_count(demo_ivt):
    ivt = open_ivt(demo_ivt.path)
    n = ivt.metadata.number_of_datapoints
    assert n is not None and n > 0
    if n > 100_000:
        table = ivt.to_arrow(complete=False, labels=False)
        assert table.num_rows > 0
        assert "value" in table.column_names
        return
    table = ivt.to_arrow(complete=True, labels=True)
    assert table.num_rows == n
    assert "value" in table.column_names
