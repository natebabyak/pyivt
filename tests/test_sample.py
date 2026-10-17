"""Smoke tests against a local IVT (env PYIVT_TEST_IVT or default Downloads path)."""

from __future__ import annotations

import os
from datetime import datetime, timezone
from io import BytesIO
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

import pyivt
from pyivt import open as open_ivt
from pyivt import read_metadata, write_metadata, write_parquet
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


def test_open_path_and_exports(tmp_path: Path):
    ivt = open_ivt(SAMPLE, complete=True)
    assert len(ivt.cells["value"]) == 197_316
    cols = ivt.to_columns(labels=True)
    assert cols["geo"][0] == 1
    assert cols["value"][0] == pytest.approx(28_642_980.0)

    table = ivt.to_arrow()
    assert table.num_rows == 197_316
    assert table.column("value")[0].as_py() == pytest.approx(28_642_980.0)

    out = tmp_path / "t.parquet"
    path = ivt.to_parquet(out)
    assert path == out
    assert out.is_file()
    assert out.with_name("t_members.parquet").is_file()

    write_parquet(SAMPLE, tmp_path / "from_path.parquet")
    assert (tmp_path / "from_path.parquet").is_file()

    meta_dir = tmp_path / "meta"
    write_metadata(ivt.metadata, meta_dir)
    assert (meta_dir / "table_info.csv").is_file()
    assert (meta_dir / "dimension_members.csv").is_file()
    assert (meta_dir / "geographies.csv").is_file()
    info = (meta_dir / "table_info.csv").read_text(encoding="utf-8")
    assert "title.en" in info
    assert "author.en" in info
    assert "number_of_datapoints" in info
    assert "read_only" in info


def test_open_bytes_and_file_like():
    data = SAMPLE.read_bytes()
    from_bytes = open_ivt(data)
    from_bio = open_ivt(BytesIO(data))
    from_path = open_ivt(SAMPLE)

    assert len(from_bytes.cells["value"]) == len(from_path.cells["value"])
    assert from_bytes.cells["value"][0] == pytest.approx(from_path.cells["value"][0])
    assert from_bio.cells["value"][0] == pytest.approx(from_path.cells["value"][0])

    for ivt in (from_bytes, from_bio):
        assert ivt.metadata.file_name is None
        assert ivt.metadata.directory is None
        assert ivt.metadata.last_update is None
        assert ivt.metadata.file_size == len(data)


def test_open_path_string():
    ivt = pyivt.open(str(SAMPLE))
    assert ivt.metadata.file_name == SAMPLE.name
    assert ivt.metadata.file_size == SAMPLE.stat().st_size


def test_open_url_mocked():
    data = SAMPLE.read_bytes()
    resp = MagicMock()
    resp.read.return_value = data
    resp.__enter__.return_value = resp
    resp.__exit__.return_value = None
    url = "https://example.com/tables/98-400-X2016112.IVT"
    with patch("pyivt._io.urlopen", return_value=resp) as mocked:
        ivt = open_ivt(url)
    mocked.assert_called_once_with(url)
    assert ivt.metadata.file_name == "98-400-X2016112.IVT"
    assert ivt.metadata.directory is None
    assert ivt.metadata.last_update is None
    assert ivt.metadata.file_size == len(data)
    assert len(ivt.cells["value"]) == 197_316
