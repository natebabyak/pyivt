"""Compact exports checked against independent native fixtures and CSV oracles."""

import hashlib
import struct
from base64 import b64decode
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


def test_typed_result_members_and_identity(tmp_path):
    source = _typed_fixture()
    ivt = pyivt.open(source)
    table = ivt.to_arrow()
    members = ivt.to_member_tables()
    assert table.column_names == ["geo", "measure", "time", "value"]
    assert list(members) == table.column_names[:-1]
    assert table.num_rows == 12
    assert table["value"].equals(_expected_table()["value"])
    assert (
        table.schema.metadata[b"pyivt.table_id"]
        == hashlib.sha256(source).hexdigest().encode()
    )
    for slug, lookup in members.items():
        codes = table[slug].chunk(0)
        assert lookup["code"].type == codes.type == pa.uint8()
        assert lookup["code"].to_pylist() == sorted(codes.unique().to_pylist())
        for key in (b"pyivt.table_id", b"pyivt.selection_id"):
            assert lookup.schema.metadata[key] == table.schema.metadata[key]
    assert members["geo"]["native_Code"][0].as_py() == "001"
    assert members["geo"]["Rate"][0].as_py() == 1.25
    assert members["time"]["Timeseries"].type == pa.date32()
    assert members["time"]["Timeseries"][1].as_py() == date(2000, 2, 29)
    assert ivt.to_member_tables() == members
    path = tmp_path / "geo.parquet"
    pq.write_table(members["geo"], path)
    lookup = pq.read_table(path)
    assert lookup.equals(members["geo"], check_metadata=True)
    joined = table.join(lookup.select(["code", "native_Code"]), "geo", "code")
    assert sorted(joined["native_Code"].to_pylist()) == ["001"] * 6 + ["09"] * 6


def test_selection_matches_native_grid_and_preserves_order():
    ivt = pyivt.open(_typed_fixture())
    full = ivt.to_arrow()
    all_members = ivt.to_member_tables()
    select = {"geo": [2], "measure": [2, 1], "time": [3, 1]}
    table = ivt.to_arrow(select)
    members = ivt.to_member_tables(select)
    assert table["value"].to_pylist() == [3.5, -1000000000000.0, 1099511627776.5, 99.0]
    assert table["time"].to_pylist() == [1, 3, 1, 3]
    assert members["time"]["code"].to_pylist() == [1, 3]
    assert members == ivt.to_member_tables(select)
    complete = {
        slug: list(range(1, count + 1))
        for slug, count in zip(ivt.layout.slugs, ivt.layout.counts, strict=True)
    }
    table = ivt.to_arrow(complete)
    members = ivt.to_member_tables(complete)
    assert table.equals(full, check_metadata=True)
    assert members == all_members
    selection = {"measure": [2]}
    projected = ivt.to_arrow(selection)
    members = ivt.to_member_tables(selection)
    assert projected.column_names == ["measure", "value"]
    assert projected.num_rows == 6  # Omitted geo/time axes still vary.
    assert projected["value"].to_pylist() == [
        0.0,
        100.125,
        -2.5,
        1099511627776.5,
        None,
        99.0,
    ]
    assert list(members) == ["measure"]
    selection = {}
    projected = ivt.to_arrow(selection)
    members = ivt.to_member_tables(selection)
    assert projected.column_names == ["value"]
    assert projected["value"].equals(full["value"])
    assert members == {}
    selection = {"geo": []}
    empty = ivt.to_arrow(selection)
    members = ivt.to_member_tables(selection)
    assert empty.num_rows == 0
    assert empty.column_names == ["geo", "value"]
    assert members["geo"].num_rows == 0
    assert members["geo"].schema == all_members["geo"].schema


@pytest.mark.parametrize(
    "select, reason",
    [
        ({"bad": [1]}, "bad"),
        ({"geo": [0]}, "geo"),
        ({"geo": [3]}, "geo"),
        ({"geo": [-1]}, "geo"),
        ({"geo": [1, 1]}, "geo"),
        ({"geo": [True]}, "geo"),
        ({"geo": [1.0]}, "geo"),
        ({"geo": ["001"]}, "geo"),
        ({"geo": "1"}, "geo"),
        ([], "select"),
    ],
)
def test_selection_errors_identify_dimension(select, reason):
    ivt = pyivt.open(_typed_fixture())
    for method in [ivt.to_arrow, ivt.to_member_tables]:
        with pytest.raises(ValueError, match=reason):
            method(select)


@pytest.mark.parametrize(
    "count, kind",
    [(255, pa.uint8()), (256, pa.uint16()), (65535, pa.uint16()), (65536, pa.uint32())],
)
def test_smallest_unsigned_code_type(count, kind):
    ivt = pyivt.open(cube((count, 1), label_fields=0))
    table = ivt.to_arrow()
    assert table["axis_0"].type == kind
    assert table["axis_0"][-1].as_py() == count
    selected = ivt.to_arrow({"axis_0": [count]})
    assert selected["axis_0"].type == kind
    assert selected["axis_0"].to_pylist() == [count]
    members = ivt.to_member_tables({"axis_0": [count]})
    assert members["axis_0"]["code"].type == kind
    assert ivt.to_arrow({"axis_0": [1]})["axis_0"].type == pa.uint8()


def test_selected_page_windows_and_projection_match_independent_values():
    ivt = pyivt.open(_populated_cube())
    select = {"axis_0": [3, 1], "axis_1": [1, 512, 513, 2049, 2051], "axis_2": [3, 1]}
    table = ivt.to_arrow(select)
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
    table = ivt.to_arrow(selection)
    members = ivt.to_member_tables(selection)
    assert table["value"].to_pylist() == [0.0]
    assert all(lookup["code"].to_pylist() == [1] for lookup in members.values())
    with pytest.raises(ValueError, match="entry 1.*outside file"):
        ivt.to_arrow()


def test_embedded_member_tables_round_trip_without_extra_columns(tmp_path):
    ivt = pyivt.open(_typed_fixture())
    selection = pyivt.Selection({"geo": [2], "time": [1, 3]})
    compact = ivt.to_arrow(selection)
    table = ivt.to_arrow(selection, embed_members=True)
    members = ivt.to_member_tables(selection)
    assert table.equals(compact)
    assert (
        table["value"].chunk(0).buffers()[1].address
        == compact["value"].chunk(0).buffers()[1].address
    )
    keys = {f"pyivt.members.v1.{slug}".encode() for slug in members}
    assert table.schema.metadata.keys() - compact.schema.metadata.keys() == keys
    assert ivt.to_arrow(selection).equals(compact, check_metadata=True)

    parquet = tmp_path / "selected.parquet"
    pq.write_table(table, parquet)
    assert pq.read_metadata(parquet).num_columns == len(compact.column_names)
    parquet_table = pq.read_table(parquet)

    arrow = tmp_path / "selected.arrow"
    with (
        pa.OSFile(str(arrow), "wb") as sink,
        pa.ipc.new_file(sink, table.schema) as writer,
    ):
        writer.write_table(table)
    with pa.ipc.open_file(arrow) as reader:
        ipc_table = reader.read_all()

    for restored in (parquet_table, ipc_table):
        assert restored.equals(table, check_metadata=True)
        for slug, lookup in members.items():
            encoded = restored.schema.metadata[f"pyivt.members.v1.{slug}".encode()]
            with pa.ipc.open_file(b64decode(encoded)) as reader:
                assert reader.read_all().equals(lookup, check_metadata=True)


def test_selection_precedes_row_allocation_and_members_do_not_decode():
    data = cube((1000, 1000, 250), label_fields=0)
    ivt = pyivt.open(data)
    with pytest.raises(MemoryError):
        ivt.to_arrow()
    select = {"axis_0": [1], "axis_1": [1], "axis_2": [1]}
    table = ivt.to_arrow(select)
    members = ivt.to_member_tables(select)
    assert table.num_rows == 1
    assert members == ivt.to_member_tables(select)
    broken = bytearray(_typed_fixture())
    broken[558:562] = (len(broken) + 100).to_bytes(4, "little")
    ivt = pyivt.open(bytes(broken))
    assert len(ivt.to_member_tables()) == 3
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
    members = ivt.to_member_tables()
    detailed = expanded_table(ivt)
    assert table.num_rows == prod(ivt.layout.counts)
    assert table["value"].equals(detailed["value"])
    for axis, (slug, lookup) in enumerate(members.items()):
        array = table[slug].chunk(0)
        stride = prod(ivt.layout.counts[axis + 1 :])
        assert array.to_pylist() == [
            r // stride % lookup.num_rows + 1 for r in range(table.num_rows)
        ]
        labels = ivt.metadata.dimensions[axis].members.en
        if labels is not None:
            assert [label or "" for label in lookup["label_en"].to_pylist()] == labels


def test_polars_retains_unsigned_codes():
    pl = pytest.importorskip("polars")
    table = pyivt.open(_typed_fixture()).to_arrow()
    frame = pl.from_arrow(table)
    for name in table.column_names[:-1]:
        assert frame.schema[name] == pl.UInt8
        assert frame[name].to_list() == table[name].to_pylist()


def test_selection_round_trips_and_shares_equivalent_buffers():
    ivt = pyivt.open(_typed_fixture())
    mapping = {"time": [3, 1], "geo": [2]}
    selection = pyivt.Selection(mapping)
    mapping["time"].append(2)
    equivalent = pyivt.Selection.from_json(selection.to_json())
    assert (
        equivalent
        == selection
        == pyivt.Selection.from_dict({"geo": [2], "time": [1, 3]})
    )
    assert hash(equivalent) == hash(selection)
    assert equivalent.selection_id == selection.selection_id
    selection.validate(ivt)
    with pytest.raises(ValueError, match="geo"):
        pyivt.Selection({"geo": [3]}).validate(ivt)

    table = ivt.to_arrow(selection)
    rebuilt = ivt.to_arrow(equivalent.to_dict())
    assert (
        table["geo"].chunk(0).buffers()[1].address
        == rebuilt["geo"].chunk(0).buffers()[1].address
    )
    members = ivt.to_member_tables(equivalent)
    for lookup in members.values():
        assert (
            lookup.schema.metadata[b"pyivt.selection_id"]
            == selection.selection_id.encode()
        )


def test_bilingual_geography_and_gender_are_joined_to_same_codes():
    ivt = pyivt.open(Path("tests/fixtures/heldout/98100036.ivt"))
    selected = {
        slug: [1, 2]
        for slug in ivt.layout.slugs
        if slug == "geo" or slug.startswith("gender")
    }
    table = ivt.to_arrow(selected)
    for slug, lookup in ivt.to_member_tables(selected).items():
        assert lookup["code"].to_pylist() == sorted(table[slug].unique().to_pylist())
        metadata = ivt.metadata.dimensions[ivt.layout.slugs.index(slug)]
        for language in ("en", "fr"):
            assert (
                lookup[f"label_{language}"].to_pylist()
                == metadata.members[language][:2]
            )


def test_missing_translation_is_nullable():
    ivt = pyivt.open(_typed_fixture())
    measure = ivt.to_member_tables()["measure"]
    assert measure["label_fr"].type == pa.string()
    assert measure["label_fr"].null_count == measure.num_rows


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
    ivt = pyivt.open(_typed_fixture(rate=rate))
    assert ivt.metadata.frequency == frequency
    table = ivt.to_arrow({"time": [1, 2, 3]})
    assert table["time"].to_pylist() == [1, 2, 3] * 4
    time = ivt.to_member_tables({"time": [1, 2, 3]})["time"]
    assert time["Timeseries"].to_pylist() == [date(1970, 1, 1), date(2000, 2, 29), None]
    for language in ("en", "fr"):
        assert time[f"label_{language}"].to_pylist() == [
            "1970-01-01",
            "2000-02-29",
            None,
        ]
