"""Streaming contracts, bounded allocation and page/range boundary coverage."""

import gc
import struct
from concurrent.futures import ThreadPoolExecutor
from math import prod
from pathlib import Path

import pyarrow as pa
import pyarrow.compute as pc
import pyivt
import pytest
from arrow_helpers import expanded_table, plain_table, stream_reader
from test_typed_arrow import _expected_table, _typed_fixture

from scripts.ivt_synthetic import cube


def test_stream_matches_independently_declared_typed_result():
    ivt = pyivt.open(_typed_fixture())
    table = expanded_table(ivt)
    assert plain_table(table).equals(_expected_table())
    assert table.schema.field("geo").type == pa.int32()
    assert table.schema.field("geo_code").type == pa.dictionary(pa.int32(), pa.string())
    assert table["measure_english_desc"].null_count == 6
    assert table["symbol"].null_count == 9
    assert ivt.to_arrow()[0]["value"].equals(_expected_table()["value"])


def test_cached_exports_share_buffers():
    ivt = pyivt.open(_typed_fixture())
    first, second = ivt.to_arrow()[0], ivt.to_arrow()[0]
    assert (
        first["value"].chunk(0).buffers()[1].address
        == second["value"].chunk(0).buffers()[1].address
    )


def test_dictionary_categories_dedupe_members_and_exclude_null_values():
    data = cube((2, 3), label_fields=1).replace(b"member 0001", b"member 0000")
    batch = next(stream_reader(pyivt.open(data)))
    assert len(batch.column("axis_0_label_0").dictionary) == 1
    assert len(batch.column("axis_1_label_0").dictionary) == 2
    for source in [data, _typed_fixture()]:
        for batch in stream_reader(pyivt.open(source)):
            for array in batch.columns:
                if pa.types.is_dictionary(array.type):
                    assert array.dictionary.null_count == 0
                    assert len(pc.unique(array.dictionary)) == len(array.dictionary)


def test_shared_ivt_can_export_from_multiple_threads():
    ivt = pyivt.open(_typed_fixture())
    with ThreadPoolExecutor(max_workers=4) as pool:
        tables = list(pool.map(lambda _: ivt.to_arrow()[0], range(8)))
    assert all(table["value"].equals(_expected_table()["value"]) for table in tables)


def test_c_stream_owns_source_after_ivt_is_released():
    ivt = pyivt.open(_typed_fixture())
    reader = stream_reader(ivt)
    del ivt
    gc.collect()
    assert plain_table(reader.read_all()).equals(_expected_table())
    reader.close()


def test_early_release_and_independent_readers():
    ivt = pyivt.open(_typed_fixture())
    for _ in range(20):
        reader = stream_reader(ivt)
        assert reader.read_next_batch().num_rows == 12
        reader.close()
    with stream_reader(ivt) as first, stream_reader(ivt) as second:
        assert next(first).num_rows == 12
        assert next(second).num_rows == 12
    with pytest.raises(ValueError, match="requested_schema"):
        ivt.__arrow_c_stream__(pa.schema([]).__arrow_c_schema__())


def test_public_arrow_table_imports_c_stream_protocol():
    assert plain_table(pa.table(pyivt.open(_typed_fixture()))).equals(_expected_table())


def test_preflight_is_catchable_and_reader_recovers():
    ivt = pyivt.open(_typed_fixture())
    with pytest.raises(MemoryError, match="max_memory_bytes=1"):
        ivt.to_arrow(max_memory_bytes=1)
    assert ivt.to_arrow()[0].num_rows == 12


def test_materialization_budget_does_not_prevent_streaming():
    ivt = pyivt.open(cube((2, 1000, 1000), label_fields=1))
    with pytest.raises(MemoryError, match="2000000 rows"):
        ivt.to_arrow(max_memory_bytes=1_000_000)
    with stream_reader(ivt) as reader:
        left, right = reader
    assert left.num_rows == right.num_rows == 1_000_000
    left_dictionary = left.column("axis_1_label_0").dictionary
    right_dictionary = right.column("axis_1_label_0").dictionary
    assert left_dictionary.buffers()[2].address == right_dictionary.buffers()[2].address


def test_hostile_member_count_is_rejected_before_allocating_maps():
    data = bytearray(cube((2, 3)))
    master = struct.unpack_from("<I", data, 544)[0]
    matrix = struct.unpack_from("<I", data, master + 8)[0]
    struct.pack_into("<I", data, matrix + 6, 100_000_000)
    ivt = pyivt.open(bytes(data))
    with pytest.raises(MemoryError, match="member map estimate"):
        stream_reader(ivt)


def _populated_cube() -> bytes:
    """3 x 2051 x 3, five windows per outer member and power-of-two holes."""
    data = bytearray(cube((3, 2051, 3), label_fields=0))
    directory = struct.unpack_from("<I", data, 558)[0]
    for outer in range(3):
        for window in range(5):
            payload = [-999.0] * 2048
            for local in range(512):
                for inner in range(3):
                    payload[local * 4 + inner] = float(
                        (outer * 2051 + window * 512 + local) * 3 + inner
                    )
            page = struct.pack("<BBH2048d", 8, 1, 2048, *payload)
            offset = len(data)
            data.extend(page)
            struct.pack_into(
                "<IHH",
                data,
                directory + (window + 8 * outer) * 8,
                offset,
                len(page),
                len(page),
            )
    return bytes(data)


def test_stream_preserves_values_across_windows_padding_and_outer_axis():
    ivt = pyivt.open(_populated_cube())
    table = expanded_table(ivt)
    assert table["value"].to_pylist() == list(map(float, range(3 * 2051 * 3)))
    for axis, slug in enumerate(ivt.layout.slugs):
        stride = prod(ivt.layout.counts[axis + 1 :])
        assert table[slug].to_pylist() == [
            row // stride % ivt.layout.counts[axis] + 1 for row in range(table.num_rows)
        ]


def test_later_corrupt_page_raises_and_terminates_stream():
    data = bytearray(cube((2, 1000, 1000), label_fields=0))
    directory = struct.unpack_from("<I", data, 558)[0]
    # Each outer member occupies 512 directory slots, including padding.
    struct.pack_into("<IHH", data, directory + 512 * 8, len(data) + 100, 8, 8)
    with stream_reader(pyivt.open(bytes(data))) as reader:
        assert next(reader).num_rows == 1_000_000
        with pytest.raises(pa.ArrowException, match="entry 512.*outside file"):
            next(reader)
        with pytest.raises(StopIteration):
            next(reader)


def test_stream_crosses_noncontiguous_paged_directory_boundary():
    data = bytearray(cube((4097, 2049), label_fields=0))
    flat, capacity, count = struct.unpack_from("<III", data, 558)
    entries = bytearray(data[flat : flat + count * 8])
    for index, value in [(4095, 17), (4096, 23)]:
        page = struct.pack("<BBH", 2, 1, 2048) + struct.pack("<h", value) + bytes(4094)
        offset = len(data)
        data.extend(page)
        struct.pack_into("<IHH", entries, index * 8, offset, len(page), len(page))
    # Store directory pages backwards, forcing ordinal resolution through the
    # pointer index rather than accidental contiguous addressing.
    pointers = []
    for start in reversed(range(0, len(entries), 4096 * 8)):
        pointers.insert(0, len(data))
        data.extend(entries[start : start + 4096 * 8])
    index_offset = len(data)
    data.extend(struct.pack(f"<{len(pointers)}I", *pointers))
    struct.pack_into("<IIIH", data, 558, index_offset, capacity, count, 1)
    ivt = pyivt.open(bytes(data))
    expected = {2047 * 2049 + 2048: 17.0, 2048 * 2049: 23.0}
    position = 0
    seen = {}
    for batch in stream_reader(ivt):
        for row in expected:
            if position <= row < position + batch.num_rows:
                seen[row] = batch.column("value")[row - position].as_py()
        position += batch.num_rows
    assert position == 4097 * 2049
    assert seen == expected


@pytest.mark.parametrize(
    "path", sorted((Path(__file__).resolve().parents[1] / "data").glob("*.ivt"))
)
def test_local_files_stream_to_same_logical_table(path):
    ivt = pyivt.open(path)
    streamed = expanded_table(ivt)
    assert streamed.num_rows == prod(ivt.layout.counts)
    assert streamed["value"].equals(ivt.to_arrow()[0]["value"])
