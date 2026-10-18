"""Smoke tests against a local IVT (env PYIVT_TEST_IVT or default Downloads path)."""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from pyivt import from_bytes, read_metadata
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
    data = SAMPLE.read_bytes()
    meta = read_metadata(data)
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
    assert meta.number_of_datapoints == 197_316
    assert meta.time_series == "No"
    assert meta.read_only is True
    assert len(meta.dimensions) == 5
    age = meta.dimensions[1]
    assert age.members is not None and age.members[0].startswith("Total")
    assert meta.geographies["geo_name"][0] == "Canada"
    assert len(meta.geographies["member_id"]) == 174
    assert len(meta.missing_values) == 5
    assert [m.symbol for m in meta.missing_values] == ["-", "..", "x", "...", "F"]
    assert [m.label.en for m in meta.missing_values] == [
        "default Missing Value",
        "not available for a specific reference period",
        "suppressed to meet the confidentiality requirements of the Statistics Act",
        "not applicable",
        "too unreliable to be published",
    ]
    assert [m.label.fr for m in meta.missing_values] == [
        "valeur manquante par défaut",
        "indisponible pour une période de référence précise",
        "confidentiel en vertu des dispositions de la Loi sur la statistique",
        "n'ayant pas lieu de figurer",
        "trop peu fiable pour être publié",
    ]
    assert [m.code for m in meta.missing_values] == [1, 2, 3, 4, 5]


def test_from_bytes_lazy_and_to_arrow():
    data = SAMPLE.read_bytes()
    ivt = from_bytes(data)
    assert ivt.metadata.product_id == "98-400-X2016112"
    assert ivt._cells is None
    assert ivt.complete is None
    assert ivt.layout.window_count == 2

    table = ivt.to_arrow()
    assert ivt._cells is not None
    assert ivt.complete is True
    assert len(ivt.cells["value"]) == 197_316
    assert table.num_rows == 197_316
    assert table.column("value")[0].as_py() == pytest.approx(28_642_980.0)
    assert table.column("geo")[0].as_py() == 1
