"""Compact exports checked against independent native fixtures and CSV oracles."""

import hashlib
import json
import struct
from concurrent.futures import ThreadPoolExecutor
from itertools import product
from math import prod
from pathlib import Path

import pyarrow as pa
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
    members = ivt.members()
    assert table.column_names == ["geo", "measure", "time", "value"]
    assert table.num_rows == 12
    assert table["value"].equals(_expected_table()["value"])
    assert members.table_id == hashlib.sha256(source).hexdigest()
    for dim in members.dimensions:
        array = table[dim.name].chunk(0)
        assert array.type == pa.uint8()
        assert sorted(array.unique().to_pylist()) == [m.code for m in dim.members]
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
    other = pyivt.open(source.replace(b"West", b"Wast")).to_arrow()
    with pytest.raises(ValueError, match="identifiers"):
        members.validate_table(other)
    subset = ivt.to_arrow({"geo": [1]})
    with pytest.raises(ValueError, match="selections"):
        members.validate_table(subset)
    altered = json.loads(members.to_json())
    altered["dimensions"][0]["members"][0]["label"] = "changed"
    with pytest.raises(ValueError, match="content_id"):
        pyivt.Members.from_json(json.dumps(altered))
    altered["schema_version"] = 2
    with pytest.raises(ValueError, match="version"):
        pyivt.Members.from_json(json.dumps(altered))


def test_selection_matches_native_grid_and_preserves_order():
    ivt = pyivt.open(_typed_fixture())
    full = ivt.to_arrow()
    all_members = ivt.members()
    select = {"geo": [2], "measure": [2, 1], "time": [3, 1]}
    table = ivt.to_arrow(select)
    members = ivt.members(select)
    assert table["value"].to_pylist() == [3.5, -1000000000000.0, 1099511627776.5, 99.0]
    assert table["time"].to_pylist() == [1, 3, 1, 3]
    assert [m.code for m in members.dimensions[2].members] == [1, 3]
    assert members == ivt.members(select)
    members.validate_table(table)
    complete = {
        slug: list(range(1, count + 1))
        for slug, count in zip(ivt.layout.slugs, ivt.layout.counts, strict=True)
    }
    table = ivt.to_arrow(complete)
    members = ivt.members(complete)
    assert table.equals(full, check_metadata=True)
    assert members == all_members
    selection = {"measure": [2]}
    projected = ivt.to_arrow(selection)
    members = ivt.members(selection)
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
    selection = {}
    projected = ivt.to_arrow(selection)
    members = ivt.members(selection)
    assert projected.column_names == ["value"]
    assert projected["value"].equals(full["value"])
    assert members.dimensions == ()
    selection = {"geo": []}
    empty = ivt.to_arrow(selection)
    members = ivt.members(selection)
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


def test_legacy_members_json_and_dictionary_table_still_match():
    dimension = pyivt.DimensionMembers(
        "axis", "Axis", 1, (pyivt.Member(3, "Label", {}),)
    )
    legacy = pyivt.Members("0" * 64, (dimension,))
    table = pa.table(
        {
            "axis": pa.DictionaryArray.from_arrays(
                pa.array([0], type=pa.uint8()), ["Label"]
            ),
            "value": [4.0],
        }
    ).replace_schema_metadata(
        {
            b"pyivt.table_id": legacy.table_id.encode(),
            b"pyivt.selection_id": legacy.selection_id.encode(),
            b"pyivt.members_schema_version": b"1",
        }
    )
    restored = pyivt.Members.from_json(legacy.to_json())
    assert restored.language is None
    restored.validate_table(table)


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
    assert ivt.to_arrow({"axis_0": [1]})["axis_0"].type == pa.uint8()


def test_selected_page_windows_and_projection_match_independent_values():
    ivt = pyivt.open(_populated_cube())
    select = {"axis_0": [3, 1], "axis_1": [1, 512, 513, 2049, 2051], "axis_2": [3, 1]}
    table = ivt.to_arrow(select)
    members = ivt.members(select)
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
    selection = {"axis_0": [1], "axis_1": [1], "axis_2": [1]}
    table = ivt.to_arrow(selection)
    members = ivt.members(selection)
    assert table["value"].to_pylist() == [0.0]
    members.validate_table(table)
    with pytest.raises(ValueError, match="entry 1.*outside file"):
        ivt.to_arrow()


def test_ipc_round_trip_keeps_member_association(tmp_path):
    ivt = pyivt.open(_typed_fixture())
    selection = pyivt.Selection({"geo": [2], "time": [1, 3]})
    table = ivt.to_arrow(selection)
    members = ivt.members(selection)
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
    data = cube((1000, 1000, 250), label_fields=0)
    ivt = pyivt.open(data)
    with pytest.raises(MemoryError):
        ivt.to_arrow()
    select = {"axis_0": [1], "axis_1": [1], "axis_2": [1]}
    table = ivt.to_arrow(select)
    members = ivt.members(select)
    assert table.num_rows == 1
    assert members == ivt.members(select)
    broken = bytearray(_typed_fixture())
    broken[558:562] = (len(broken) + 100).to_bytes(4, "little")
    ivt = pyivt.open(bytes(broken))
    assert len(ivt.members().dimensions) == 3
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
    assert table.num_rows == prod(ivt.layout.counts)
    assert table["value"].equals(detailed["value"])
    for axis, dimension in enumerate(members.dimensions):
        array = table[dimension.name].chunk(0)
        stride = prod(ivt.layout.counts[axis + 1 :])
        assert array.to_pylist() == [
            r // stride % len(dimension.members) + 1 for r in range(table.num_rows)
        ]
        labels = ivt.metadata.dimensions[axis].members.en
        if labels is not None:
            assert [m.label or "" for m in dimension.members] == labels
    members.validate_table(table)


def test_polars_retains_unsigned_codes():
    pl = pytest.importorskip("polars")
    table = pyivt.open(_typed_fixture()).to_arrow()
    frame = pl.from_arrow(table)
    for name in table.column_names[:-1]:
        assert frame.schema[name] == pl.UInt8
        assert frame[name].to_list() == table[name].to_pylist()


def test_pandas_categoricals():
    pd = pytest.importorskip("pandas")
    for source in [
        cube((2, 3), 1),
        _typed_fixture(),
        _typed_fixture(geography_labels=("West", "West")),
    ]:
        ivt = pyivt.open(source)
        table = ivt.to_arrow()
        members = ivt.members()
        frame = members.to_pandas(table)
        for dim in members.dimensions:
            labels = {m.code: m.label for m in dim.members}
            column = frame[dim.name]
            assert isinstance(column.dtype, pd.CategoricalDtype)
            assert column.astype(object).where(column.notna(), None).tolist() == [
                labels[code] for code in table[dim.name].to_pylist()
            ]


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
    members = ivt.members(equivalent)
    assert selection.selection_id == members.selection_id
    members.validate_table(table)
    modified = table.set_column(
        0, "geo", pa.array([255] * table.num_rows, type=pa.uint8())
    )
    with pytest.raises(ValueError, match="geo"):
        members.validate_table(modified)


def test_bilingual_geography_and_gender_are_joined_to_same_codes():
    ivt = pyivt.open(Path("tests/fixtures/heldout/98100036.ivt"))
    selected = {
        slug: [1, 2]
        for slug in ivt.layout.slugs
        if slug == "geo" or slug.startswith("gender")
    }
    table = ivt.to_arrow(selected)
    for language in ("en", "fr"):
        members = ivt.members(selected, language=language)
        members.validate_table(table)
        assert members.language == language
        for dim in members.dimensions:
            metadata = ivt.metadata.dimensions[ivt.layout.slugs.index(dim.name)]
            assert [member.label for member in dim.members] == metadata.members[
                language
            ][:2]
    assert ivt.members(selected) == ivt.members(selected, language="en")
    assert (
        ivt.members(selected, language="en").dimensions
        != ivt.members(selected, language="fr").dimensions
    )


def test_missing_translation_is_explicit():
    ivt = pyivt.open(_typed_fixture())
    with pytest.raises(ValueError, match="measure.*fr"):
        ivt.members(language="fr")
    with pytest.raises(ValueError, match="unsupported member language"):
        ivt.members(language="es")
    assert ivt.members({"measure": []}, language="fr").dimensions[0].members == ()


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
    for language in ("en", "fr"):
        members = ivt.members({"time": [1, 2, 3]}, language=language)
        assert [m.label for m in members.dimensions[0].members] == [
            "1970-01-01",
            "2000-02-29",
            None,
        ]
        members.validate_table(table)
