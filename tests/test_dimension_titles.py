"""Full dimension titles replace the 15-character stored axis names (NAT-260)."""

from __future__ import annotations

import csv
import os
from pathlib import Path

import pyivt
import pytest

TESTS = Path(__file__).parent
DATA_DIR = Path(os.environ.get("PYIVT_DATA_DIR", TESTS.parent / "data"))
FIXTURES = TESTS / "fixtures"

# Development inputs from tests/corpus/training.json.
AGE_98100020 = "07d7cf7da252ea1396ba7b7d070301ad165330bd6865a6ccfeb723de810ec1fc"
SAC_98100016 = "3f8d271072f3916cbc5b35f185c1abf5330c0fdd0fda4da3375003e2acd6f8b2"
# A Health Statistics input whose notes have no Variable List.
LEGACY_NO_LIST = "135f68215afc04d96caa1d0c77546f2115262dc1fb0d6ea75219ac6eebdbcf6d"


def _data(sha256: str) -> Path:
    path = DATA_DIR / f"{sha256}.ivt"
    if not path.is_file():
        if os.environ.get("PYIVT_REQUIRE_DATA") == "1":
            pytest.fail(f"PYIVT_REQUIRE_DATA=1: missing {path}")
        pytest.skip(f"missing development input {path.name}")
    return path


def _csv_dimension(path: Path) -> str:
    """The dimension title prefix of StatCan's first measure column header."""
    with path.open(encoding="utf-8-sig", newline="") as handle:
        header = next(csv.reader(handle))
    return header[4].split(": ", 1)[0]


def test_truncated_names_resolve_to_full_titles():
    ivt = pyivt.open(_data(AGE_98100020))
    dims = ivt.metadata.dimensions
    # The stored axis names are "Age (in single" and "Âge (en années)".
    age = dims[2]
    assert age.name.en == "Age (in single years), average age and median age (128)"
    assert age.name.fr == "Âge (en années), âge moyen et âge médian (128)"
    assert age.description.en == age.name.en
    assert age.description.fr == age.name.fr
    assert dims[1].name.fr == "Année de recensement (2)"
    assert [dim.slug for dim in dims] == [
        "geography",
        "census_year_2",
        "age_in_single_years_average_age_and_medi",
        "gender_3a",
    ]
    assert ivt._layout.slugs == [dim.slug for dim in dims]
    assert ivt.to_arrow().column_names[:4] == [dim.slug for dim in dims]


@pytest.mark.parametrize(
    ("source", "csv_path"),
    [
        (FIXTURES / "zip" / "98100001.zip", FIXTURES / "zip" / "98100001.csv"),
        (FIXTURES / "98100018-paged.ivt", FIXTURES / "98100018.csv"),
    ],
    ids=["98100001", "98100018"],
)
def test_names_match_official_csv_headers(source: Path, csv_path: Path):
    dims = pyivt.open(source).metadata.dimensions
    expected = _csv_dimension(csv_path)
    assert expected == "Population and dwelling counts (11)"
    assert dims[1].name.en == expected
    assert dims[1].slug == "population_and_dwelling_counts_11"


def test_axis_without_full_title_keeps_descriptor_name():
    # 98100016's Variable List omits the Statistical Area Classification axis.
    dims = pyivt.open(_data(SAC_98100016)).metadata.dimensions
    assert dims[1].name.en == "Statistical Are"
    assert dims[1].slug == "statistical_are"
    assert dims[2].name.en == "Population and dwelling counts (3)"


def test_file_without_variable_list_keeps_descriptor_names():
    dims = pyivt.open(_data(LEGACY_NO_LIST)).metadata.dimensions
    assert [(dim.name.en, dim.slug) for dim in dims] == [
        ("Quantifier", "quantifier"),
        ("AGE", "age"),
        ("REGION", "region"),
        ("Periods", "periods"),
    ]
