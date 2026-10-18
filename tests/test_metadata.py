"""Integration tests for StatCan IVT metadata (requires downloaded fixtures)."""

from __future__ import annotations

import json
from math import prod

from metadata_expected import expected_path, first_last_sentences, snapshot_metadata

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
    en_first, en_last = first_last_sentences(meta.notes.en)
    fr_first, fr_last = first_last_sentences(meta.notes.fr)
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
    assert meta._loaded("geographies")
    assert not meta._loaded("footnotes")
    assert not meta._loaded("missing_values")

    _ = meta.footnotes
    _ = meta.missing_values
    assert meta._loaded("footnotes")
    assert meta._loaded("missing_values")


def test_open_bytes_defers_cells(statcan_ivt):
    expect = json.loads(expected_path(statcan_ivt.id).read_text(encoding="utf-8"))
    ivt = open_ivt(statcan_ivt.path.read_bytes())
    assert ivt.metadata.product_id == statcan_ivt.id
    assert ivt._cells is None
    assert ivt.complete is None
    assert len(ivt.layout.counts) == len(expect["dimensions"])
