"""I/O-volume assertions for selective path metadata extraction."""

from __future__ import annotations

from pathlib import Path

import pytest

from conftest import STATCAN_DIR

import pyivt


def _largest_statcan() -> Path:
    files = list(STATCAN_DIR.glob("*.ivt"))
    if not files:
        pytest.skip("no StatCan fixtures")
    return max(files, key=lambda p: p.stat().st_size)


def test_summary_reads_less_than_full_file_when_large(tmp_path):
    """For files > 2 MiB, summary open must not read the entire IVT.

    StatCan fixtures in-repo are < 2 MiB and use the small-file full-read path.
    This test synthesizes a large sparse file only when a big fixture exists, or
    skips when the corpus is entirely small.
    """
    src = _largest_statcan()
    size = src.stat().st_size
    if size <= 2 * 1024 * 1024:
        # Pad a copy past the small-file threshold while preserving the IVT head.
        big = tmp_path / "padded.ivt"
        data = src.read_bytes()
        # Append zeros so the file is large; selective path must not read the pad
        # for summary metadata (descriptor/master live in the original prefix).
        pad = (2 * 1024 * 1024 + 1) - len(data)
        big.write_bytes(data + bytes(pad))
        path = big
        original_size = len(data)
    else:
        path = src
        original_size = size

    ivt = pyivt.open(path)
    meta = ivt.metadata
    # Touch summary fields that must not require the padded tail.
    _ = meta.product_id, meta.title.en, meta.author.en, meta.number_of_datapoints
    bytes_read, ops, largest, fallback = meta._io_metrics()
    file_size = path.stat().st_size
    assert bytes_read < file_size
    # Should stay near the original IVT payload, not the padded tail.
    assert bytes_read <= max(original_size + 256 * 1024, 5 * 1024 * 1024)
    assert ops < 50
    assert largest < file_size
    assert not fallback


def test_lazy_fields_independent_io():
    path = _largest_statcan()
    meta = pyivt.open(path).metadata
    b0, o0, _, _ = meta._io_metrics()
    assert not meta._loaded("dimensions")
    assert not meta._loaded("geographies")

    _ = meta.dimensions
    b1, o1, _, _ = meta._io_metrics()
    assert meta._loaded("dimensions")
    assert not meta._loaded("geographies")
    assert b1 >= b0
    assert o1 >= o0

    _ = meta.geographies
    assert meta._loaded("geographies")
    b2, o2, _, _ = meta._io_metrics()
    assert b2 >= b1

    # Repeat access should not issue new reads (cache / already parsed).
    _ = meta.dimensions
    _ = meta.geographies
    b3, o3, _, _ = meta._io_metrics()
    assert b3 == b2
    assert o3 == o2


def test_footnotes_do_not_require_prior_dims():
    path = _largest_statcan()
    meta = pyivt.open(path).metadata
    _ = meta.footnotes
    assert meta._loaded("footnotes")
    assert not meta._loaded("dimensions")
