"""Integration tests for StatCan IVT metadata (requires downloaded fixtures)."""

from __future__ import annotations

import json
from math import prod

from metadata_expected import expected_path, first_last_words, snapshot_metadata
from pyivt import open as open_ivt


def test_metadata_matches_expected(statcan_ivt):
    path = expected_path(statcan_ivt.id)
    assert path.is_file(), (
        f"missing golden {path.name}; run: python tests/scripts/dump_expected_metadata.py"
    )
    expect = json.loads(path.read_text(encoding="utf-8"))
    meta = open_ivt(statcan_ivt.path).metadata
    assert snapshot_metadata(meta) == expect

    # Cross-check notes endpoints and datapoint product without relying on golden alone.
    en_first, en_last = first_last_words(meta.notes.en)
    fr_first, fr_last = first_last_words(meta.notes.fr)
    assert {"first": en_first, "last": en_last} == expect["notes"]["en"]
    assert {"first": fr_first, "last": fr_last} == expect["notes"]["fr"]
    assert prod(d.count for d in meta.dimensions) == expect["number_of_datapoints"]


def test_metadata_lazy_fields(statcan_ivt):
    meta = open_ivt(statcan_ivt.path).metadata
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


def test_open_bytes_defers_decode(statcan_ivt):
    expect = json.loads(expected_path(statcan_ivt.id).read_text(encoding="utf-8"))
    ivt = open_ivt(statcan_ivt.path.read_bytes())
    assert ivt.metadata.product_id == statcan_ivt.id
    assert len(ivt.layout.counts) == len(expect["dimensions"])


def test_to_arrow_row_count(statcan_ivt):
    ivt = open_ivt(statcan_ivt.path)
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
