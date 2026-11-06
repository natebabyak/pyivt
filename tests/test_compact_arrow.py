"""Compact exports checked against independent native fixtures and CSV oracles."""

import hashlib
import struct
from concurrent.futures import ThreadPoolExecutor
from datetime import date
from itertools import product
from math import prod
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pyivt
import pytest
from arrow_helpers import expanded_table
from test_streaming import _populated_cube
from test_typed_arrow import _expected_table, _typed_fixture

from scripts.ivt_synthetic import cube


def test_typed_result_members_and_source_identity(tmp_path):
    source = _typed_fixture()
    ivt = pyivt.open(source)
    table = ivt.to_arrow()
    members = ivt.members()
    assert table.column_names == ["geography", "measure", "time", "value"]
    assert list(members) == table.column_names[:-1]
    assert [dim.slug for dim in ivt.metadata.dimensions] == list(members)
    assert table.num_rows == 12
    assert table["value"].equals(_expected_table()["value"])
    assert (
        table.schema.metadata[b"pyivt.table_id"]
        == hashlib.sha256(source).hexdigest().encode()
    )
    assert table.schema.metadata[b"pyivt.dimension_encoding"] == b"one_based_index"
    for slug, lookup in members.items():
        codes = table[slug].chunk(0)
        assert lookup["code"].type == codes.type == pa.uint8()
        assert lookup["code"].to_pylist() == sorted(codes.unique().to_pylist())
        assert (
            lookup.schema.metadata[b"pyivt.table_id"]
            == table.schema.metadata[b"pyivt.table_id"]
        )
        assert lookup.schema.metadata[b"pyivt.dimension_encoding"] == b"one_based_index"
    assert members["geography"]["native_Code"][0].as_py() == "001"
    assert members["geography"]["Rate"][0].as_py() == 1.25
    assert members["time"]["Timeseries"].type == pa.date32()
    assert members["time"]["Timeseries"][1].as_py() == date(2000, 2, 29)
    assert ivt.members() == members
    path = tmp_path / "geography.parquet"
    pq.write_table(members["geography"], path)
    lookup = pq.read_table(path)
    assert lookup.equals(members["geography"], check_metadata=True)
    joined = table.join(lookup.select(["code", "native_Code"]), "geography", "code")
    assert sorted(joined["native_Code"].to_pylist()) == ["001"] * 6 + ["09"] * 6


def test_selection_matches_native_grid_and_preserves_order():
    ivt = pyivt.open(_typed_fixture())
    full = ivt.to_arrow()
    all_members = ivt.members()
    table = ivt.to_arrow(geography=[2], measure=[2, 1], time=[3, 1])
    assert table["value"].to_pylist() == [3.5, -1000000000000.0, 1099511627776.5, 99.0]
    assert table["time"].to_pylist() == [1, 3, 1, 3]
    assert table["geography"].to_pylist() == [2] * 4
    assert ivt.members(geography=[2], time=[3, 1])["time"]["code"].to_pylist() == [1, 3]

    complete = {
        slug: lookup["code"].to_pylist()[::-1] for slug, lookup in all_members.items()
    }
    assert ivt.to_arrow(**complete).equals(full, check_metadata=True)
    assert ivt.members(**complete) == all_members

    projected = ivt.to_arrow(measure=2)
    assert projected.column_names == ["geography", "time", "value"]
    assert projected["value"].to_pylist() == [
        0.0,
        100.125,
        -2.5,
        1099511627776.5,
        None,
        99.0,
    ]
    assert list(ivt.members(measure=2)) == ["geography", "time"]

    assert ivt.to_arrow(geography=2, measure=1, time=3).to_pydict() == {
        "value": [-1000000000000.0]
    }
    assert ivt.members(geography=2, measure=1, time=3) == {}

    empty = ivt.to_arrow(geography=[], measure=1)
    members = ivt.members(geography=[], measure=1)
    assert empty.num_rows == 0
    assert empty.column_names == ["geography", "time", "value"]
    assert members["geography"].num_rows == 0
    assert members["time"].equals(all_members["time"])
    assert members["geography"].schema == all_members["geography"].schema


@pytest.mark.parametrize(
    "select, reason",
    [
        ({"bad": [1]}, "bad"),
        ({"geo": [1]}, "unknown dimension 'geo'"),
        ({"geography": [0]}, "geography"),
        ({"geography": [3]}, "geography"),
        ({"geography": [-1]}, "geography"),
        ({"geography": [1, 1]}, "geography"),
        ({"geography": [True]}, "geography"),
        ({"geography": [1.0]}, "geography"),
        ({"geography": ["001"]}, "geography"),
        ({"geography": "1"}, "geography"),
        ({"geography": (1,)}, "geography"),
        ({"geography": True}, "geography"),
    ],
)
def test_selection_errors_identify_dimension(select, reason):
    ivt = pyivt.open(_typed_fixture())
    for method in [ivt.to_arrow, ivt.members]:
        with pytest.raises(ValueError, match=reason):
            method(**select)


@pytest.mark.parametrize(
    "count, kind",
    [(255, pa.uint8()), (256, pa.uint16()), (65535, pa.uint16()), (65536, pa.uint32())],
)
def test_smallest_unsigned_code_type(count, kind):
    ivt = pyivt.open(cube((count, 1), label_fields=0))
    table = ivt.to_arrow()
    assert table["axis_0"].type == kind
    assert table["axis_0"][-1].as_py() == count
    for codes in ([count], [1], []):
        selected = ivt.to_arrow(axis_0=codes)["axis_0"]
        lookup = ivt.members(axis_0=codes)["axis_0"]["code"]
        assert selected.type == lookup.type == kind
        assert selected.to_pylist() == lookup.to_pylist() == codes


def test_selected_page_windows_and_projection_match_independent_values():
    ivt = pyivt.open(_populated_cube())
    select = {"axis_0": [3, 1], "axis_1": [1, 512, 513, 2049, 2051], "axis_2": [3, 1]}
    table = ivt.to_arrow(**select)
    coords = product(
        sorted(select["axis_0"]), sorted(select["axis_1"]), sorted(select["axis_2"])
    )
    assert table["value"].to_pylist() == [
        float(((a - 1) * 2051 + b - 1) * 3 + c - 1) for a, b, c in coords
    ]


def test_selection_does_not_decode_unselected_corrupt_pages():
    source = bytearray(_populated_cube())
    directory = struct.unpack_from("<I", source, 558)[0]
    struct.pack_into("<I", source, directory + 8, len(source) + 100)
    ivt = pyivt.open(bytes(source))
    selection = {"axis_0": [1], "axis_1": [1], "axis_2": [1]}
    table = ivt.to_arrow(**selection)
    members = ivt.members(**selection)
    assert table["value"].to_pylist() == [0.0]
    assert all(lookup["code"].to_pylist() == [1] for lookup in members.values())
    with pytest.raises(ValueError, match="entry 1.*outside file"):
        ivt.to_arrow()


def test_selection_precedes_row_allocation_and_members_do_not_decode():
    data = cube((1000, 1000, 250), label_fields=0)
    ivt = pyivt.open(data)
    with pytest.raises(MemoryError):
        ivt.to_arrow()
    select = {"axis_0": [1], "axis_1": [1], "axis_2": [1]}
    table = ivt.to_arrow(**select)
    members = ivt.members(**select)
    assert table.num_rows == 1
    assert members == ivt.members(**select)
    broken = bytearray(_typed_fixture())
    broken[558:562] = (len(broken) + 100).to_bytes(4, "little")
    ivt = pyivt.open(bytes(broken))
    assert len(ivt.members()) == 3
    with pytest.raises(ValueError, match="unknown member code"):
        ivt.to_arrow(geography=999)
    with pytest.raises(ValueError, match="directory"):
        ivt.to_arrow()


def test_cached_buffers_threading_and_lifetime():
    ivt = pyivt.open(_typed_fixture())
    first = ivt.to_arrow()
    with ThreadPoolExecutor(max_workers=4) as pool:
        tables = list(pool.map(lambda _: ivt.to_arrow(), range(8)))
    assert all(t.equals(first) for t in tables)
    assert (
        first["value"].chunk(0).buffers()[1].address
        == tables[0]["value"].chunk(0).buffers()[1].address
    )
    ivt = None
    assert first["value"].equals(_expected_table()["value"])


@pytest.mark.parametrize(
    "path",
    sorted(Path("data").glob("*.ivt")) + sorted(Path("tests/fixtures").rglob("*.ivt")),
)
def test_corpus_values_labels_and_metadata_match_detailed_reader(path):
    ivt = pyivt.open(path)
    table = ivt.to_arrow()
    members = ivt.members()
    detailed = expanded_table(ivt)
    counts = [dim.items for dim in ivt.metadata.dimensions]
    assert table.num_rows == prod(counts)
    assert table["value"].equals(detailed["value"])
    for axis, (slug, lookup) in enumerate(members.items()):
        array = table[slug].chunk(0)
        stride = prod(counts[axis + 1 :])
        assert array.to_pylist() == [
            r // stride % lookup.num_rows + 1 for r in range(table.num_rows)
        ]
        labels = ivt.metadata.dimensions[axis].members.en
        if labels is not None:
            assert [label or "" for label in lookup["label"].to_pylist()] == labels


def test_polars_retains_unsigned_codes():
    pl = pytest.importorskip("polars")
    table = pyivt.open(_typed_fixture()).to_arrow()
    frame = pl.from_arrow(table)
    for name in table.column_names[:-1]:
        assert frame.schema[name] == pl.UInt8
        assert frame[name].to_list() == table[name].to_pylist()


def test_equivalent_selections_share_buffers():
    ivt = pyivt.open(_typed_fixture())
    table = ivt.to_arrow(time=[3, 1], geography=[2])
    equivalent = ivt.to_arrow(geography=[2], time=[1, 3])
    assert (
        table["geography"].chunk(0).buffers()[1].address
        == equivalent["geography"].chunk(0).buffers()[1].address
    )
    fixed = ivt.to_arrow(geography=2, time=[3, 1])
    assert fixed.column_names == ["measure", "time", "value"]
    assert fixed["value"].equals(table["value"])


def test_bilingual_geography_and_gender_are_joined_to_same_codes():
    ivt = pyivt.open(Path("tests/fixtures/heldout/98100036.ivt"))
    selected = {
        slug: [1, 2]
        for slug in [dim.slug for dim in ivt.metadata.dimensions]
        if slug == "geography" or slug.startswith("gender")
    }
    table = ivt.to_arrow(**selected)
    dims = {dim.slug: dim for dim in ivt.metadata.dimensions}
    for language in ("en", "fr"):
        lookups = ivt.members(language=language, **selected)
        for slug in selected:
            lookup = lookups[slug]
            assert lookup["code"].to_pylist() == sorted(
                table[slug].unique().to_pylist()
            )
            assert lookup["label"].to_pylist() == dims[slug].members[language][:2]


def test_missing_translation_is_nullable():
    ivt = pyivt.open(_typed_fixture())
    measure = ivt.members(language="fr")["measure"]
    assert measure["label"].type == pa.string()
    assert measure["label"].null_count == measure.num_rows


@pytest.mark.parametrize(
    "rate, frequency",
    [
        (1, "annual"),
        (2, "semi_annual"),
        (4, "quarterly"),
        (12, "monthly"),
    ],
)
def test_time_series_member_labels_follow_dates_in_both_languages(rate, frequency):
    ivt = pyivt.open(_typed_fixture(rate=rate, time_name="Reference Period"))
    assert ivt.metadata.frequency == frequency
    assert ivt.metadata.dimensions[-1].slug == "reference_period"
    table = ivt.to_arrow(reference_period=[1, 2, 3])
    assert table["reference_period"].to_pylist() == [1, 2, 3] * 4
    for language in ("en", "fr"):
        time = ivt.members(language=language, reference_period=[1, 2, 3])[
            "reference_period"
        ]
        assert time["Timeseries"].to_pylist() == [
            date(1970, 1, 1),
            date(2000, 2, 29),
            None,
        ]
        assert time["label"].to_pylist() == ["1970-01-01", "2000-02-29", None]


@pytest.mark.parametrize(
    "stored_codes, expected_text",
    [
        (("00070000", "0"), ["00070000", "0"]),
        (("00", "000"), ["00", "000"]),
        (("INNC", "INNC"), ["INNC", "INNC"]),
        (("INNC", ""), ["INNC", None]),
    ],
)
def test_native_codes_are_text_metadata(stored_codes, expected_text):
    ivt = pyivt.open(_typed_fixture(geography_codes=stored_codes))
    table = ivt.to_arrow(geography=[2, 1])
    lookup = ivt.members(geography=[2, 1])["geography"]
    assert table["geography"].type == lookup["code"].type == pa.uint8()
    assert table["geography"].to_pylist() == [1] * 6 + [2] * 6
    assert lookup["code"].to_pylist() == [1, 2]
    assert lookup["native_Code"].type == pa.string()
    assert lookup["native_Code"].to_pylist() == expected_text
    assert (
        ivt.members(geography=[2])["geography"]["native_Code"].to_pylist()
        == expected_text[1:]
    )
    assert ivt.to_arrow(geography=2)["value"].equals(table["value"].slice(6))


def test_missing_native_code_field():
    source = bytearray(cube((2, 3), label_fields=0))
    source[824:838] = bytes(14)
    ivt = pyivt.open(source)
    lookup = ivt.members()["axis_0"]
    assert lookup.column_names == ["code", "label"]
    assert lookup["code"].to_pylist() == [1, 2]
    assert ivt.to_arrow(axis_0=[2])["axis_0"].to_pylist() == [2, 2, 2]


def test_native_sort_attribute_uses_smallest_unsigned_type():
    ivt = pyivt.open(_typed_fixture(measure_sort=(300, 1)))
    sort = ivt.members()["measure"]["Sort"]
    assert sort.to_pylist() == [300, 1]
    assert sort.type == pa.uint16()
    assert ivt.members(measure=[2])["measure"]["Sort"].type == pa.uint8()
