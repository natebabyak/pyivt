"""Integration tests for StatCan IVT metadata (requires downloaded fixtures)."""

from __future__ import annotations

from math import prod

from pyivt import from_bytes, open as open_ivt, read_metadata

# Hardcoded expectations keyed by manifest id. Easy to scan / review.
EXPECTED = {
    "98100033": {
        "product_id": "98100033",
        "title_en_contains": "Broad age groups, gender and census year",
        "title_fr_contains": "Grands groupes d’âge, genre et année de recensement",
        "dimension_counts": [14, 3, 25, 5],
        "number_of_datapoints": 5250,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100034": {
        "product_id": "98100034",
        "title_en_contains": "Broad age groups and sex: Canada, provinces and territories",
        "title_fr_contains": "Grands groupes d’âge et sexe",
        "dimension_counts": [14, 5, 3],
        "number_of_datapoints": 210,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100036": {
        "product_id": "98100036",
        "title_en_contains": "Broad age groups and gender: Canada, provinces and territories",
        "title_fr_contains": "Grands groupes d’âge et genre",
        "dimension_counts": [14, 3, 8],
        "number_of_datapoints": 336,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100039": {
        "product_id": "98100039",
        "title_en_contains": "Population in private dwellings",
        "title_fr_contains": "Population dans les logements privés",
        "dimension_counts": [14, 3, 9],
        "number_of_datapoints": 378,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100044": {
        "product_id": "98100044",
        "title_en_contains": "Type of collective dwelling",
        "title_fr_contains": "Type de logement collectif",
        "dimension_counts": [14, 16, 2],
        "number_of_datapoints": 448,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100047": {
        "product_id": "98100047",
        "title_en_contains": "Occupied private dwellings by structural type",
        "title_fr_contains": "Logements privés occupés selon le type de construction",
        "dimension_counts": [14, 9, 4],
        "number_of_datapoints": 504,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100123": {
        "product_id": "98100123",
        "title_en_contains": "Census family structure, presence of children",
        "title_fr_contains": "Structure de la famille de recensement",
        "dimension_counts": [166, 3, 7, 4],
        "number_of_datapoints": 13944,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100125": {
        "product_id": "98100125",
        "title_en_contains": "Marital status, age group and gender",
        "title_fr_contains": "État matrimonial, groupe d’âge et genre",
        "dimension_counts": [55, 3, 4, 13, 16],
        "number_of_datapoints": 137280,
        "time_series": "Geography",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100035": {
        "product_id": "98100035",
        "title_en_contains": "Broad age groups and sex: Canada and census metropolitan areas",
        "title_fr_contains": "Grands groupes d’âge et sexe : Canada et régions",
        "dimension_counts": [44, 5, 3],
        "number_of_datapoints": 660,
        "time_series": "Geography",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
    "98100030": {
        "product_id": "98100030",
        "title_en_contains": "census metropolitan areas and census agglomerations with parts",
        "title_fr_contains": "agglomérations de recensement y compris les parties",
        "dimension_counts": [174, 3, 5, 4],
        "number_of_datapoints": 10440,
        "time_series": "No",
        "read_only": True,
        "has_geography": True,
        "missing_symbols": ["..", "X", "...", "F"],
    },
}


def test_metadata_matches_expected(statcan_ivt):
    expect = EXPECTED[statcan_ivt.id]
    meta = read_metadata(statcan_ivt.path)

    assert meta.product_id == expect["product_id"]
    assert meta.title.en and expect["title_en_contains"] in meta.title.en
    assert meta.title.fr and expect["title_fr_contains"] in meta.title.fr
    assert [d.count for d in meta.dimensions] == expect["dimension_counts"]
    assert meta.number_of_datapoints == expect["number_of_datapoints"]
    assert prod(d.count for d in meta.dimensions) == expect["number_of_datapoints"]
    assert meta.time_series == expect["time_series"]
    assert meta.read_only is expect["read_only"]
    assert any(d.is_geography for d in meta.dimensions) is expect["has_geography"]
    assert [m.symbol for m in meta.missing_values] == expect["missing_symbols"]


def test_metadata_lazy_fields(statcan_ivt):
    meta = read_metadata(statcan_ivt.path)
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


def test_open_and_read_metadata_agree(statcan_ivt):
    path = statcan_ivt.path
    meta = read_metadata(path)
    ivt = open_ivt(path)
    assert ivt.metadata.product_id == meta.product_id
    assert ivt.metadata.title == meta.title
    assert ivt.metadata.number_of_datapoints == meta.number_of_datapoints
    assert ivt.metadata.time_series == meta.time_series
    assert ivt.metadata.read_only == meta.read_only
    assert ivt.metadata.version == meta.version
    assert ivt.metadata.signature == meta.signature


def test_from_bytes_opens(statcan_ivt):
    ivt = from_bytes(statcan_ivt.path.read_bytes())
    assert ivt.metadata.product_id == EXPECTED[statcan_ivt.id]["product_id"]
    assert ivt._cells is None
    assert ivt.complete is None
    assert len(ivt.layout.counts) == len(EXPECTED[statcan_ivt.id]["dimension_counts"])
