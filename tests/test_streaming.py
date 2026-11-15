"""Streaming contracts, bounded allocation and page/range boundary coverage."""

import gc
import os
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
    assert table.schema.field("geography").type == pa.int32()
    assert table.schema.field("geography_code").type == pa.dictionary(
        pa.int32(), pa.string()
    )
    assert table["measure_english_desc"].null_count == 6
    assert table["symbol"].null_count == 9
    assert ivt.to_arrow()["value"].equals(_expected_table()["value"])


def test_repeated_exports_are_equal_and_independent():
    ivt = pyivt.open(_typed_fixture())
    first, second = ivt.to_arrow(), ivt.to_arrow()
    assert first.equals(second)
    assert (
        first["value"].chunk(0).buffers()[1].address
        != second["value"].chunk(0).buffers()[1].address
    )


def test_path_open_exports_do_not_depend_on_retained_file_bytes(tmp_path):
    path = tmp_path / "typed.ivt"
    path.write_bytes(_typed_fixture())
    ivt = pyivt.open(path)
    expected = _expected_table()["value"]
    reader = stream_reader(ivt)
    with ThreadPoolExecutor(max_workers=4) as pool:
        tables = list(pool.map(lambda _: ivt.to_arrow(), range(8)))
    assert all(table["value"].equals(expected) for table in tables)
    del tables
    gc.collect()
    # A live stream keeps its own copy; later exports read the file again.
    assert ivt.to_arrow()["value"].equals(expected)
    assert plain_table(reader.read_all()).equals(_expected_table())
    reader.close()
    assert ivt.to_arrow()["value"].equals(expected)


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
        tables = list(pool.map(lambda _: ivt.to_arrow(), range(8)))
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
        ivt._arrow_c_stream(pa.schema([]).__arrow_c_schema__())


def test_reader_does_not_expose_the_arrow_stream_protocol():
    assert not hasattr(pyivt.open(_typed_fixture()), "__arrow_c_stream__")


def test_max_memory_bytes_bounds_exports():
    ivt = pyivt.open(_typed_fixture())
    for export in (ivt.to_arrow, ivt.iter_batches):
        with pytest.raises(pyivt.ResourceLimitError, match="1000 bytes") as raised:
            export(max_memory_bytes=1000)
        assert raised.value.limit == 1000
        for bad in (0, -1):
            with pytest.raises(ValueError, match="must be positive"):
                export(max_memory_bytes=bad)
    roomy = 64 * 1024 * 1024
    assert ivt.to_arrow(max_memory_bytes=roomy).equals(ivt.to_arrow())
    assert ivt.iter_batches(max_memory_bytes=roomy).read_all().equals(ivt.to_arrow())


def test_raising_max_memory_bytes_admits_larger_exports():
    ivt = pyivt.open(cube((1000, 1000, 250), label_fields=0))
    select = {"axis_0": list(range(1, 21))}
    with pytest.raises(pyivt.ResourceLimitError) as raised:
        ivt.to_arrow(select=select, max_memory_bytes=16 * 1024 * 1024)
    needed = raised.value.estimate
    assert needed is not None
    assert ivt.to_arrow(select=select, max_memory_bytes=needed).num_rows == 5_000_000


def test_streaming_large_table_keeps_dictionary_buffers_shared():
    ivt = pyivt.open(cube((2, 1000, 1000), label_fields=1))
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
    for axis, slug in enumerate(ivt._layout.slugs):
        stride = prod(ivt._layout.counts[axis + 1 :])
        assert table[slug].to_pylist() == [
            row // stride % ivt._layout.counts[axis] + 1
            for row in range(table.num_rows)
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
    assert streamed.num_rows == prod(ivt._layout.counts)
    assert streamed["value"].equals(ivt.to_arrow()["value"])


def _assert_stream_matches(ivt, batch_size, **dims):
    expected = ivt.to_arrow(select=dims)
    reader = ivt.iter_batches(batch_size=batch_size, select=dims)
    assert isinstance(reader, pa.RecordBatchReader)
    assert reader.schema.equals(expected.schema, check_metadata=True)
    batches = list(reader)
    assert all(batch.num_rows <= batch_size for batch in batches)
    assert all(
        batch.schema.equals(expected.schema, check_metadata=True) for batch in batches
    )
    streamed = pa.Table.from_batches(batches, schema=reader.schema)
    assert streamed.combine_chunks().equals(expected.combine_chunks())
    assert streamed.schema.metadata == expected.schema.metadata


@pytest.mark.parametrize("batch_size", [1, 5, 7, 12, 1_000])
def test_iter_batches_matches_to_arrow_across_batch_sizes(batch_size):
    _assert_stream_matches(pyivt.open(_typed_fixture()), batch_size)


@pytest.mark.parametrize("batch_size", [1, 511, 3 * 2051 * 3 - 1, 4096, 1_048_576])
def test_iter_batches_crosses_windows_and_selections(batch_size):
    ivt = pyivt.open(_populated_cube())
    _assert_stream_matches(ivt, batch_size)
    slugs = ivt._layout.slugs
    _assert_stream_matches(
        ivt, batch_size, **{slugs[0]: [1, 3], slugs[1]: list(range(500, 1600, 3))}
    )
    _assert_stream_matches(ivt, batch_size, **{slugs[0]: 2, slugs[2]: [3]})


def test_iter_batches_default_size_and_consumers():
    ivt = pyivt.open(_populated_cube())
    expected = ivt.to_arrow()
    with ivt.iter_batches() as reader:
        assert reader.read_all().combine_chunks().equals(expected.combine_chunks())
    pl = pytest.importorskip("polars")
    frame = pl.DataFrame(ivt.iter_batches(batch_size=4096))
    assert frame.height == expected.num_rows
    assert frame.equals(pl.from_arrow(expected))
    batches = list(pl.DataFrame(ivt.iter_batches(batch_size=4096)).iter_slices())
    assert sum(len(batch) for batch in batches) == expected.num_rows


def test_iter_batches_rejects_bad_arguments():
    ivt = pyivt.open(_typed_fixture())
    with pytest.raises(ValueError, match="batch_size"):
        ivt.iter_batches(batch_size=0)
    with pytest.raises(ValueError, match="unknown dimension"):
        ivt.iter_batches(select={"nope": 1})
    with pytest.raises(ValueError, match="max_memory_bytes must be positive"):
        ivt.iter_batches(max_memory_bytes=0)


def test_iter_batches_streams_selections_too_large_to_materialize():
    ivt = pyivt.open(cube((1000, 1000, 250), label_fields=0))
    with pytest.raises(pyivt.ResourceLimitError, match="iter_batches"):
        ivt.to_arrow()
    with ivt.iter_batches() as reader:
        first = reader.read_next_batch()
    assert first.num_rows == 1_048_576
    assert first.schema.equals(reader.schema)
    with pytest.raises(pyivt.ResourceLimitError):
        ivt.iter_batches(batch_size=250_000_000)


def test_iter_batches_owns_source_and_releases_path_bytes(tmp_path):
    path = tmp_path / "cube.ivt"
    path.write_bytes(_populated_cube())
    ivt = pyivt.open(path)
    expected = ivt.to_arrow()
    reader = ivt.iter_batches(batch_size=1000)
    del ivt
    gc.collect()
    path.unlink()
    assert reader.read_all().combine_chunks().equals(expected.combine_chunks())


def test_iter_batches_corrupt_page_ends_stream():
    data = bytearray(cube((2, 1000, 1000), label_fields=0))
    directory = struct.unpack_from("<I", data, 558)[0]
    struct.pack_into("<IHH", data, directory + 512 * 8, len(data) + 100, 8, 8)
    with pyivt.open(bytes(data)).iter_batches(batch_size=1_000_000) as reader:
        assert reader.read_next_batch().num_rows == 1_000_000
        with pytest.raises(pa.ArrowException, match="outside file"):
            reader.read_next_batch()
        with pytest.raises(StopIteration):
            reader.read_next_batch()


@pytest.mark.parametrize(
    "path", sorted((Path(__file__).resolve().parents[1] / "data").glob("*.ivt"))
)
def test_local_files_iter_batches_concatenate_to_to_arrow(path):
    ivt = pyivt.open(path)
    try:
        expected = ivt.to_arrow()
    except pyivt.ResourceLimitError:
        pytest.skip("too large to materialize")
    batch_size = max(1, expected.num_rows // 7)
    with ivt.iter_batches(batch_size=batch_size) as reader:
        streamed = reader.read_all()
    assert streamed.schema.equals(expected.schema, check_metadata=True)
    assert streamed.combine_chunks().equals(expected.combine_chunks())


@pytest.mark.skipif(
    "PYIVT_LARGE_IVT" not in os.environ,
    reason="set PYIVT_LARGE_IVT to the held-out 98100532.ivt path",
)
def test_held_out_large_table_streams_in_full():
    ivt = pyivt.open(os.environ["PYIVT_LARGE_IVT"])
    rows = 0
    with ivt.iter_batches() as reader:
        for batch in reader:
            rows += batch.num_rows
    assert rows == prod(ivt._layout.counts) == 126_198_000
