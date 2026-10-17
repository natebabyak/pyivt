"""Smoke tests against a local IVT (env PYIVT_TEST_IVT or default Downloads path)."""

from __future__ import annotations

import os
from datetime import datetime, timezone
from pathlib import Path

import pytest

from pyivt import read_metadata, read_table, write_metadata, write_parquet
from pyivt.decode import build_layout
from pyivt.descriptor import parse_descriptor
from pyivt.header import read_header

DEFAULT = Path(r"C:\Users\nateb\Downloads\98-400-X2016112.IVT")
SAMPLE = Path(os.environ.get("PYIVT_TEST_IVT", DEFAULT))


pytestmark = pytest.mark.skipif(
    not SAMPLE.is_file(), reason=f"sample IVT not found: {SAMPLE}"
)


def test_header_signature():
    data = SAMPLE.read_bytes()
    hdr = read_header(data)
    assert hdr.signature == b"\x04\x00\x20\x00"
    assert hdr.version == "modern"


def test_descriptor_dimensions():
    data = SAMPLE.read_bytes()
    desc = parse_descriptor(data)
    assert desc is not None
    names = [d.name for d in desc.dims]
    assert names[0] == "Geography"
    assert desc.dims[0].count == 174
    assert desc.dims[1].count == 9
    assert desc.dims[2].count == 3
    assert desc.dims[3].count == 21
    assert desc.dims[4].count == 2
    lay = build_layout(desc.dims)
    assert lay.window_count == 2
    assert lay.rec_bytes == 256
    assert int(__import__("numpy").prod(lay.counts)) == 197_316


def test_read_metadata():
    meta = read_metadata(SAMPLE)
    assert meta.product_id == "98-400-X2016112"
    assert meta.title.en and "Employment Income" in meta.title.en
    assert meta.title.en.endswith("2006 Census")
    assert meta.title.fr and "Tranches de revenu" in meta.title.fr
    assert meta.author.en == "2016 Census - Statistics Canada"
    assert meta.author.fr == "Recensement de 2016-Statistique Canada"
    assert meta.category.en == "Data Tables"
    assert meta.category.fr == "Tableaux de données"
    assert meta.keywords.en == "98-400-X2016112"
    assert meta.notes.en and "Historical comparison" in meta.notes.en
    assert meta.notes.fr and "Comparaison historique" in meta.notes.fr
    assert meta.file_name == SAMPLE.name
    assert meta.directory and meta.directory.endswith(os.sep)
    assert meta.file_size == SAMPLE.stat().st_size
    assert meta.number_of_datapoints == 197_316
    assert meta.time_series == "No"
    assert meta.read_only is True
    expected_mtime = datetime.fromtimestamp(SAMPLE.stat().st_mtime, tz=timezone.utc)
    assert meta.last_update == expected_mtime
    assert len(meta.dimensions) == 5
    age = meta.dimensions[1]
    assert age.members is not None and age.members[0].startswith("Total")
    assert meta.geographies["geo_name"][0] == "Canada"
    assert len(meta.geographies["member_id"]) == 174


def test_read_table_and_parquet(tmp_path: Path):
    tab = read_table(SAMPLE, complete=True)
    assert len(tab.cells["value"]) == 197_316
    # Canada / Total Age / Total Sex / Total Emp / 2015
    cols = tab.to_columns(labels=True)
    # first completed row in meshgrid order is all-1s coordinates
    assert cols["geo"][0] == 1
    assert cols["value"][0] == pytest.approx(28_642_980.0)

    out = tmp_path / "t.parquet"
    write_parquet(tab, out)
    assert out.is_file()
    assert out.with_name("t_members.parquet").is_file()

    meta_dir = tmp_path / "meta"
    write_metadata(tab.metadata, meta_dir)
    assert (meta_dir / "table_info.csv").is_file()
    assert (meta_dir / "dimension_members.csv").is_file()
    assert (meta_dir / "geographies.csv").is_file()
    info = (meta_dir / "table_info.csv").read_text(encoding="utf-8")
    assert "title.en" in info
    assert "author.en" in info
    assert "number_of_datapoints" in info
    assert "read_only" in info
