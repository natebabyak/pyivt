"""Declared slot boundaries and identical validation for file/bytes sources."""

import hashlib
import struct

import pyivt
import pytest
from arrow_helpers import expanded_table
from conftest import DATA_DIR
from test_typed_arrow import _typed_fixture

from scripts.ivt_synthetic import cube


def _open_error(source):
    with pytest.raises(ValueError) as error:
        pyivt.open(source)
    return str(error.value)


def test_twelve_dimensions_do_not_read_the_next_file_region(tmp_path):
    data = bytearray(cube((2,) * 12, label_fields=1))
    # Slot 31 ends at byte 992. A native file may place its master directory
    # immediately here; these bytes deliberately resemble an invalid slot 32.
    struct.pack_into("<IIIH", data, 992, len(data) + 100, 1, 1, 0)
    directory = struct.unpack_from("<I", data, 558)[0]
    for index in range(2):
        page = struct.pack(
            "<BBH2048h", 2, 1, 2048, *range(index * 2048, (index + 1) * 2048)
        )
        offset = len(data)
        data.extend(page)
        struct.pack_into(
            "<IHH", data, directory + index * 8, offset, len(page), len(page)
        )
    path = tmp_path / "twelve.ivt"
    path.write_bytes(data)
    tables = []
    for source in (path, bytes(data)):
        ivt = pyivt.open(source)
        assert len(ivt.metadata.dimensions) == 12
        assert [dim.items for dim in ivt.metadata.dimensions] == [2] * 12
        table = ivt.to_arrow()
        assert table.num_rows == 4096
        assert table["value"].to_pylist() == list(range(4096))
        tables.append(table)
    assert tables[0].equals(tables[1], check_metadata=True)


def _metadata_snapshot(metadata):
    return {
        "summary": [
            metadata.product_id,
            metadata.universe,
            metadata.signature,
            metadata.number_of_datapoints,
            metadata.frequency,
            metadata.start_date,
            metadata.end_date,
            metadata.read_only,
        ],
        "text": [
            (getattr(metadata, field).en, getattr(metadata, field).fr)
            for field in ("title", "notes", "author", "category", "keywords")
        ],
        "dimensions": [
            (
                dim.slug,
                dim.items,
                dim.type,
                dim.name.en,
                dim.name.fr,
                dim.description.en,
                dim.description.fr,
                dim.members.en,
                dim.members.fr,
                dim.notes.en,
                dim.notes.fr,
            )
            for dim in metadata.dimensions
        ],
        "footnotes": [
            (note.symbol, note.number, note.description.en, note.description.fr)
            for note in metadata.footnotes
        ],
        "missing": [
            (value.code, value.symbol, value.description.en, value.description.fr)
            for value in metadata.missing_values
        ],
    }


@pytest.mark.parametrize("fixture", ["typed", "legacy"])
def test_relocated_slot_table_preserves_metadata_members_and_cells(fixture, tmp_path):
    if fixture == "typed":
        original = _typed_fixture()
    else:
        sha256 = "b162a3aac1784293dde48ad0fe9ade3cc98bfa87c7beb24f7f95b9ff50976797"
        source = DATA_DIR / f"{sha256}.ivt"
        if not source.is_file():
            pytest.skip("download the legacy corpus to test relocated v2 slots")
        original = source.read_bytes()
        assert hashlib.sha256(original).hexdigest() == sha256
    data = bytearray(original)
    start = struct.unpack_from("<I", data, 12)[0]
    count = struct.unpack_from("<H", data, 16)[0]
    slots = data[start : start + count * 14]
    data[start : start + len(slots)] = bytes(len(slots))
    # A large gap proves the selective path reads the declared table itself,
    # rather than materializing a prefix through its new location at EOF.
    data.extend(bytes(1024 * 1024))
    struct.pack_into("<I", data, 12, len(data))
    data.extend(slots)
    path = tmp_path / "relocated.ivt"
    path.write_bytes(data)
    expected = pyivt.open(original)
    expected_metadata = _metadata_snapshot(expected.metadata)
    expected_members = expected.to_member_table()
    expected_values = expanded_table(expected)
    for source in (path, bytes(data)):
        ivt = pyivt.open(source)
        assert _metadata_snapshot(ivt.metadata) == expected_metadata
        if source == path:
            assert ivt.metadata._io_metrics()[0] < len(data) // 2
        assert ivt.to_member_table().equals(expected_members)
        assert expanded_table(ivt).equals(expected_values)
        assert ivt.to_arrow().equals(expected.to_arrow())


def test_truncated_metadata_record_has_identical_detailed_error(tmp_path):
    data = cube((2, 3), label_fields=1)
    master = struct.unpack_from("<I", data, 544)[0]
    offset, length = struct.unpack_from("<IH", data, master + 2 * 8)
    truncated = data[: offset + length - 1]
    path = tmp_path / "truncated.ivt"
    path.write_bytes(truncated)
    path_error = _open_error(path)
    assert path_error == _open_error(truncated)
    assert "IVT metadata record extends past EOF: slot=544 ordinal=2" in path_error
    assert f"offset={offset}" in path_error
    assert f"file_len={len(truncated)}" in path_error


def test_invalid_optional_record_is_not_silently_dropped_for_bytes(tmp_path):
    data = bytearray(cube((2, 3), label_fields=1))
    master = struct.unpack_from("<I", data, 544)[0]
    # No evidence supports masking on-disk bit 31. The optional master record
    # must fail validation equally, rather than disappearing on the bytes route.
    struct.pack_into("<IHH", data, master, (1 << 31) | 1024, 4, 4)
    path = tmp_path / "invalid_record.ivt"
    path.write_bytes(data)
    message = _open_error(path)
    assert message == _open_error(bytes(data))
    assert "IVT metadata record extends past EOF: slot=544 ordinal=0" in message


@pytest.mark.parametrize("fraction", [0.99, 0.90, 0.50])
def test_verified_cuts_of_real_training_file_have_identical_errors(fraction, tmp_path):
    # StatCan 98-10-0012, already pinned in the development manifest. These are
    # deliberate cuts of verified complete bytes, not diagnoses from pointers.
    sha256 = "0559dc74a44cb9675d249975c0b4ec26a689595d85e0a32a7bbbf8ef0d13f011"
    original = DATA_DIR / f"{sha256}.ivt"
    if not original.is_file():
        pytest.skip("download the training corpus to test real-file truncation")
    complete = original.read_bytes()
    assert hashlib.sha256(complete).hexdigest() == sha256
    truncated = complete[: int(len(complete) * fraction)]
    path = tmp_path / "truncated_training.ivt"
    path.write_bytes(truncated)
    message = _open_error(path)
    assert message == _open_error(truncated)
    assert "IVT metadata record extends past EOF:" in message


@pytest.mark.parametrize(
    "offset, value", [(8, 63), (10, 4), (16, 31), (18, 8), (20, 2048)]
)
def test_native_header_invariants_are_checked_on_both_routes(offset, value, tmp_path):
    data = bytearray(cube((2, 3), label_fields=0))
    struct.pack_into("<H", data, offset, value)
    path = tmp_path / "invalid_header.ivt"
    path.write_bytes(data)
    message = _open_error(path)
    assert message == _open_error(bytes(data))
    assert f"invalid IVT header at byte {offset}:" in message


def test_truncated_slot_table_has_identical_error(tmp_path):
    data = cube((2, 3))[:991]
    path = tmp_path / "truncated_slots.ivt"
    path.write_bytes(data)
    message = _open_error(path)
    assert message == _open_error(data)
    assert (
        message
        == "IVT slot table extends past EOF: expected bytes 544..992, file_len=991"
    )
