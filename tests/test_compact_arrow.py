"""Compact exports checked against independent native fixtures and CSV oracles."""

import hashlib
import json
import struct
from concurrent.futures import ThreadPoolExecutor
from itertools import product
from math import prod
from pathlib import Path

import pyarrow as pa
import pyarrow.compute as pc
import pyivt
import pytest
from arrow_helpers import expanded_table
from pyivt._members import validate_codes
from test_streaming import _populated_cube
from test_typed_arrow import _expected_table, _typed_fixture

from scripts.ivt_synthetic import cube


def test_typed_result_members_and_identity(tmp_path):
    source = _typed_fixture()
    ivt = pyivt.open(source)
    table, members = ivt.to_arrow()
    assert table.column_names == ["geo", "measure", "time", "value"]
    assert table.num_rows == 12
    assert table["value"].equals(_expected_table()["value"])
    assert members.table_id == hashlib.sha256(source).hexdigest()
    for dim in members.dimensions:
        array = table[dim.name].chunk(0)
        assert array.type == pa.dictionary(pa.uint8(), pa.string())
        assert array.dictionary.to_pylist() == [m.label for m in dim.members]
        assert [m.code for m in dim.members] == list(range(1, len(dim.members) + 1))
        assert dim.first_code == 1
    assert members.dimensions[0].members[0].metadata["Code"] == "001"
    assert members.dimensions[0].members[0].metadata["Rate"] == 1.25
    assert members.dimensions[2].members[1].metadata["Timeseries"] == "2000-02-29"
    assert ivt.members() == members
    members.validate_table(table)
    path = tmp_path / "members.json"
    assert pyivt.Members.from_json(members.to_json(path)) == members
    assert pyivt.Members.read_json(path) == members
    with pytest.raises(TypeError):
        members.dimensions[0].members[0].metadata["Code"] = "changed"
    other, _ = pyivt.open(source.replace(b"West", b"Wast")).to_arrow()
    with pytest.raises(ValueError, match="identifiers"):
        members.validate_table(other)
    subset, _ = ivt.to_arrow({"geo": [1]})
    with pytest.raises(ValueError, match="selections"):
        members.validate_table(subset)
    altered = json.loads(members.to_json())
    altered["dimensions"][0]["members"][0]["label"] = "changed"
    with pytest.raises(ValueError, match="selection_id"):
        pyivt.Members.from_json(json.dumps(altered))
    altered["schema_version"] = 2
    with pytest.raises(ValueError, match="version"):
        pyivt.Members.from_json(json.dumps(altered))


def test_selection_matches_native_grid_and_preserves_order():
    ivt = pyivt.open(_typed_fixture())
    full, all_members = ivt.to_arrow()
    select = {"geo": [2], "measure": [2, 1], "time": [3, 1]}
    table, members = ivt.to_arrow(select)
    assert table["value"].to_pylist() == [3.5, -1000000000000.0, 1099511627776.5, 99.0]
    assert table["time"].chunk(0).indices.to_pylist() == [0, 1, 0, 1]
    assert [m.code for m in members.dimensions[2].members] == [1, 3]
    assert members == ivt.members(select)
    members.validate_table(table)
    complete = {
        slug: list(range(1, count + 1))
        for slug, count in zip(ivt.layout.slugs, ivt.layout.counts, strict=True)
    }
    table, members = ivt.to_arrow(complete)
    assert table.equals(full, check_metadata=True)
    assert members == all_members
    projected, members = ivt.to_arrow({"measure": [2]})
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
    assert [d.name for d in members.dimensions] == ["measure"]
    projected, members = ivt.to_arrow({})
    assert projected.column_names == ["value"]
    assert projected["value"].equals(full["value"])
    assert members.dimensions == ()
    empty, members = ivt.to_arrow({"geo": []})
    assert empty.num_rows == 0
    assert empty.column_names == ["geo", "value"]
    assert members.dimensions[0].members == ()
    members.validate_table(empty)


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
    for method in [ivt.to_arrow, ivt.members]:
        with pytest.raises(ValueError, match=reason):
            method(select)


@pytest.mark.parametrize("codes, offset", [([0, 1, 2], 0), ([1, 2, 3], 1)])
def test_dense_codes(codes, offset):
    assert validate_codes(codes, "table", "axis") == offset


@pytest.mark.parametrize(
    "codes", [[1, 3], [1, 1], [-1, 0], [2, 1], [2, 3], [0, 1.0], [False, 1]]
)
def test_invalid_full_codes_name_table_dimension_and_codes(codes):
    with pytest.raises(ValueError, match="table T, dimension D:.*codes"):
        validate_codes(codes, "T", "D")


@pytest.mark.parametrize(
    "count, kind",
    [(256, pa.uint8()), (257, pa.uint16()), (65536, pa.uint16()), (65537, pa.uint32())],
)
def test_smallest_unsigned_dictionary_type(count, kind):
    # No member fields: ordinal labels still give a complete dictionary.
    table, _ = pyivt.open(cube((count, 1), label_fields=0)).to_arrow()
    assert table["axis_0"].type.index_type == kind
    assert table["axis_0"].chunk(0).indices[-1].as_py() == count - 1


def test_selected_page_windows_and_projection_match_independent_values():
    ivt = pyivt.open(_populated_cube())
    select = {"axis_0": [3, 1], "axis_1": [1, 512, 513, 2049, 2051], "axis_2": [3, 1]}
    table, members = ivt.to_arrow(select)
    coords = product(
        sorted(select["axis_0"]), sorted(select["axis_1"]), sorted(select["axis_2"])
    )
    assert table["value"].to_pylist() == [
        float(((a - 1) * 2051 + b - 1) * 3 + c - 1) for a, b, c in coords
    ]
    members.validate_table(table)


def test_selection_does_not_decode_unselected_corrupt_pages():
    source = bytearray(_populated_cube())
    directory = struct.unpack_from("<I", source, 558)[0]
    struct.pack_into("<I", source, directory + 8, len(source) + 100)
    ivt = pyivt.open(bytes(source))
    table, members = ivt.to_arrow({"axis_0": [1], "axis_1": [1], "axis_2": [1]})
    assert table["value"].to_pylist() == [0.0]
    members.validate_table(table)
    with pytest.raises(ValueError, match="entry 1.*outside file"):
        ivt.to_arrow()


def test_ipc_round_trip_keeps_member_association(tmp_path):
    table, members = pyivt.open(_typed_fixture()).to_arrow({"geo": [2], "time": [1, 3]})
    path = tmp_path / "selected.arrow"
    with (
        pa.OSFile(str(path), "wb") as sink,
        pa.ipc.new_file(sink, table.schema) as writer,
    ):
        writer.write_table(table)
    with pa.ipc.open_file(path) as reader:
        restored = reader.read_all()
    assert restored.equals(table, check_metadata=True)
    pyivt.Members.from_json(members.to_json()).validate_table(restored)


def test_selection_precedes_row_allocation_and_members_do_not_decode():
    data = cube((1000, 1000, 80))
    ivt = pyivt.open(data)
    with pytest.raises(MemoryError):
        ivt.to_arrow(max_memory_bytes=100_000_000)
    select = {"axis_0": [1], "axis_1": [1], "axis_2": [1]}
    table, members = ivt.to_arrow(select, max_memory_bytes=20_000_000)
    assert table.num_rows == 1
    assert members == ivt.members(select)
    broken = bytearray(_typed_fixture())
    broken[558:562] = (len(broken) + 100).to_bytes(4, "little")
    ivt = pyivt.open(bytes(broken))
    assert len(ivt.members().dimensions) == 3
    with pytest.raises(ValueError, match="directory"):
        ivt.to_arrow()


def test_cached_buffers_threading_lifetime_and_budget():
    ivt = pyivt.open(_typed_fixture())
    first, _ = ivt.to_arrow()
    with ThreadPoolExecutor(max_workers=4) as pool:
        tables = list(pool.map(lambda _: ivt.to_arrow()[0], range(8)))
    assert all(t.equals(first) for t in tables)
    assert (
        first["value"].chunk(0).buffers()[1].address
        == tables[0]["value"].chunk(0).buffers()[1].address
    )
    with pytest.raises(MemoryError):
        ivt.to_arrow(max_memory_bytes=1)
    ivt = None
    assert first["value"].equals(_expected_table()["value"])


@pytest.mark.parametrize(
    "path",
    sorted(Path("data").glob("*.ivt")) + sorted(Path("tests/fixtures").rglob("*.ivt")),
)
def test_corpus_values_labels_and_metadata_match_detailed_reader(path):
    ivt = pyivt.open(path)
    table, members = ivt.to_arrow()
    detailed = expanded_table(ivt)
    assert table.num_rows == prod(ivt.layout.counts)
    assert table["value"].equals(detailed["value"])
    for axis, dimension in enumerate(members.dimensions):
        array = table[dimension.name].chunk(0)
        stride = prod(ivt.layout.counts[axis + 1 :])
        assert array.indices.to_pylist() == [
            r // stride % len(dimension.members) for r in range(table.num_rows)
        ]
        labels = ivt.metadata.dimensions[axis].members.en
        if labels is not None:
            assert [m.label for m in dimension.members] == labels
    members.validate_table(table)


def test_polars_categoricals_and_duplicate_missing_labels():
    pl = pytest.importorskip("polars")
    for source in [
        _typed_fixture(),
        cube((2, 3), 1).replace(b"member 0001", b"member 0000"),
    ]:
        table, _ = pyivt.open(source).to_arrow()
        frame = pl.from_arrow(table)
        for name in table.column_names[:-1]:
            assert frame.schema[name] == pl.Categorical
            assert (
                frame[name].cast(pl.String).to_list()
                == pc.dictionary_decode(table[name]).to_pylist()
            )


def test_pandas_categoricals():
    pd = pytest.importorskip("pandas")
    for source in [
        cube((2, 3), 1),
        _typed_fixture(),
        cube((2, 3), 1).replace(b"member 0001", b"member 0000"),
    ]:
        table, members = pyivt.open(source).to_arrow()
        frame = members.to_pandas(table)
        for name in table.column_names[:-1]:
            assert isinstance(frame[name].dtype, pd.CategoricalDtype)
            assert (
                frame[name].astype(object).where(frame[name].notna(), None).tolist()
                == table[name].to_pylist()
            )
