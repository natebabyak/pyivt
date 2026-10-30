"""Streaming contracts, bounded allocation and page/range boundary coverage."""

import gc
import struct
from concurrent.futures import ThreadPoolExecutor
from math import prod
from pathlib import Path

import pyarrow as pa
import pyivt
import pytest
from arrow_helpers import plain_table
from test_typed_arrow import _expected_table, _typed_fixture

from scripts.ivt_synthetic import cube


@pytest.mark.parametrize("batch_size", [1, 5, 6, 7, 100])
def test_stream_matches_independently_declared_typed_result(batch_size):
    ivt = pyivt.open(_typed_fixture())
    batches = list(ivt.to_batches(batch_size=batch_size))
    assert all(isinstance(batch, pa.RecordBatch) for batch in batches)
    assert all(0 < batch.num_rows <= batch_size for batch in batches)
    table = pa.Table.from_batches(batches)
    assert plain_table(table).equals(_expected_table())
    assert table.schema.field("geo").type == pa.int32()
    assert table.schema.field("geo_code").type == pa.dictionary(pa.int32(), pa.string())
    assert table["measure_english_desc"].null_count == 6
    assert table["symbol"].null_count == 9
    assert plain_table(ivt.to_arrow()).equals(_expected_table())


def test_dictionary_buffers_and_cached_exports_are_shared():
    ivt = pyivt.open(_typed_fixture())
    batches = list(ivt.to_batches(batch_size=6))
    for name in ("geo_code", "measure_english_desc"):
        left, right = [batch.column(name).dictionary for batch in batches]
        assert left.buffers()[2].address == right.buffers()[2].address
    first, second = ivt.to_arrow(), ivt.to_arrow()
    assert (
        first["value"].chunk(0).buffers()[1].address
        == second["value"].chunk(0).buffers()[1].address
    )


def test_shared_ivt_can_export_from_multiple_threads():
    ivt = pyivt.open(_typed_fixture())
    with ThreadPoolExecutor(max_workers=4) as pool:
        tables = list(pool.map(lambda _: ivt.to_arrow(), range(8)))
    assert all(plain_table(table).equals(_expected_table()) for table in tables)


def test_iterator_and_c_stream_own_source_after_ivt_is_released():
    ivt = pyivt.open(_typed_fixture())
    iterator = ivt.to_batches(batch_size=5)
    reader = pa.RecordBatchReader.from_stream(ivt)
    del ivt
    gc.collect()
    assert plain_table(pa.Table.from_batches(iterator)).equals(_expected_table())
    assert plain_table(reader.read_all()).equals(_expected_table())
    reader.close()


def test_early_release_and_independent_readers():
    ivt = pyivt.open(_typed_fixture())
    for _ in range(20):
        reader = pa.RecordBatchReader.from_stream(ivt)
        assert reader.read_next_batch().num_rows == 12
        reader.close()
    first, second = ivt.to_batches(batch_size=5), ivt.to_batches(batch_size=7)
    assert next(first).num_rows == 5
    assert next(second).num_rows == 6
    assert sum(batch.num_rows for batch in first) == 7
    assert sum(batch.num_rows for batch in second) == 6
    with pytest.raises(ValueError, match="requested_schema"):
        ivt.__arrow_c_stream__(pa.schema([]).__arrow_c_schema__())


@pytest.mark.parametrize("method", ["to_arrow", "to_batches"])
def test_preflight_is_catchable_and_reader_recovers(method):
    ivt = pyivt.open(_typed_fixture())
    with pytest.raises(MemoryError, match="max_memory_bytes=1"):
        getattr(ivt, method)(max_memory_bytes=1)
    assert ivt.to_arrow().num_rows == 12


def test_materialization_budget_does_not_prevent_streaming():
    ivt = pyivt.open(cube((1000, 1000, 10)))
    with pytest.raises(MemoryError, match="10000000 rows"):
        ivt.to_arrow()
    batches = ivt.to_batches(batch_size=100_000)
    assert sum(batch.num_rows for batch in batches) == prod(ivt.layout.counts)


@pytest.mark.parametrize("batch_size", [0, 2**31])
def test_invalid_batch_size(batch_size):
    with pytest.raises(ValueError, match="batch_size"):
        pyivt.open(_typed_fixture()).to_batches(batch_size=batch_size)


def test_hostile_member_count_is_rejected_before_allocating_maps():
    data = bytearray(cube((2, 3)))
    master = struct.unpack_from("<I", data, 544)[0]
    matrix = struct.unpack_from("<I", data, master + 8)[0]
    struct.pack_into("<I", data, matrix + 6, 100_000_000)
    ivt = pyivt.open(bytes(data))
    with pytest.raises(MemoryError, match="member map estimate"):
        ivt.to_batches(batch_size=1)


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


@pytest.mark.parametrize("batch_size", [1, 7, 2047, 2050, 4097, 1_000_000])
def test_ranges_preserve_values_across_windows_padding_and_outer_axis(batch_size):
    ivt = pyivt.open(_populated_cube())
    batches = list(ivt.to_batches(batch_size=batch_size))
    table = pa.Table.from_batches(batches)
    assert table["value"].to_pylist() == list(map(float, range(3 * 2051 * 3)))
    for axis, slug in enumerate(ivt.layout.slugs):
        stride = prod(ivt.layout.counts[axis + 1 :])
        assert table[slug].to_pylist() == [
            row // stride % ivt.layout.counts[axis] + 1 for row in range(table.num_rows)
        ]


def test_later_corrupt_page_raises_and_terminates_iterator():
    data = bytearray(_populated_cube())
    directory = struct.unpack_from("<I", data, 558)[0]
    struct.pack_into("<I", data, directory + 8, len(data) + 100)
    iterator = pyivt.open(bytes(data)).to_batches(batch_size=1536)
    assert next(iterator).num_rows == 1536
    with pytest.raises(ValueError, match="entry 1.*outside file"):
        next(iterator)
    with pytest.raises(StopIteration):
        next(iterator)


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
    for batch in ivt.to_batches(batch_size=100_000):
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
    streamed = pa.Table.from_batches(ivt.to_batches(batch_size=8191))
    assert streamed.num_rows == prod(ivt.layout.counts)
    assert plain_table(streamed).equals(plain_table(ivt.to_arrow()))
