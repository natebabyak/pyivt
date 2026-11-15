"""Rejected files say whether they are corrupt or unsupported, and why.

Each case reproduces one failure type from the NAT-245 share scan with a
synthetic table. The original files are private, so the leading bytes and
offset/size ratios below are copied from the scan report, not the files.
"""

import struct

import pyivt
import pytest

from scripts.ivt_synthetic import cube

CORRUPT = "Corrupt IVT file: "
UNSUPPORTED = "Unsupported IVT file: "


FIELDS = ("offset", "length", "file_len", "slot", "entry", "dimension")


def _raised(data: bytes, tmp_path) -> pyivt.IvtError:
    """Return the path-route error after checking the bytes route agrees."""
    path = tmp_path / "table.ivt"
    path.write_bytes(data)
    errors = []
    for source in (path, data):
        with pytest.raises(pyivt.IvtError) as error:
            pyivt.open(source)
        errors.append(error.value)
    first, second = errors
    assert type(first) is type(second)
    assert str(first) == str(second)
    assert [getattr(first, f) for f in FIELDS] == [getattr(second, f) for f in FIELDS]
    return first


def _error(data: bytes, tmp_path) -> str:
    """Return the shared message after checking both input routes agree."""
    return str(_raised(data, tmp_path))


def _matrix(data: bytes) -> int:
    master = struct.unpack_from("<I", data, 544)[0]
    return struct.unpack_from("<I", data, master + 8)[0]


# Type 1: 14 files. A valid envelope whose axis matrix is not recognised.


def test_unrecognised_matrix_row_width_names_the_variant(tmp_path):
    data = bytearray(cube((2, 3, 5)))
    struct.pack_into("<H", data, _matrix(data) + 4, 80)
    message = _error(bytes(data), tmp_path)
    assert message.startswith(
        UNSUPPORTED + "descriptor layout not recognised (container version 4): "
        "the axis matrix has component header 01 02 and 80-byte rows; supported "
        "matrices have header 01 02 (byte rows) and 82-byte rows. Details: "
    )
    assert "unverified descriptor fallback is disabled" in message


def test_undecodable_matrix_record_shows_its_leading_bytes(tmp_path):
    data = bytearray(cube((2, 3, 5)))
    data[_matrix(data) + 1] = 3
    message = _error(bytes(data), tmp_path)
    assert message.startswith(
        UNSUPPORTED + "descriptor layout not recognised (container version 4): "
        "the axis matrix record could not be decoded (unsupported component "
        "header [01, 03]); its first bytes are 01 03 03 00 52 00"
    )


def test_missing_axis_matrix_is_unsupported_not_corrupt(tmp_path):
    data = bytearray(cube((2, 3)))
    master = struct.unpack_from("<I", data, 544)[0]
    data[master + 8 : master + 16] = bytes(8)
    message = _error(bytes(data), tmp_path)
    assert message.startswith(
        UNSUPPORTED + "descriptor layout not recognised (container version 4): "
        "master directory entry 1 is empty (offset=0 length=0)"
    )


def test_invalid_axis_row_identifies_the_axis(tmp_path):
    data = bytearray(cube((2, 3)))
    struct.pack_into("<H", data, _matrix(data) + 6 + 82 + 6, 3)
    message = _error(bytes(data), tmp_path)
    assert message.startswith(UNSUPPORTED)
    assert "axis 2 (" in message
    assert "has an unrecognised axis kind: items=3 kind=3" in message


# Type 2: 6 files. Leading bytes from the scan differ in every file.


@pytest.mark.parametrize(
    "leading",
    ["1c342034", "30002c00", "036a05ff", "4b000000", "f50cf6f7", "21000000"],
)
def test_unrecognised_signature_is_reported_as_corrupt(leading, tmp_path):
    data = bytearray(cube((2, 3)))
    data[:4] = bytes.fromhex(leading)
    message = _error(bytes(data), tmp_path)
    spaced = " ".join(leading[i : i + 2] for i in range(0, 8, 2))
    assert message.startswith(
        CORRUPT + "not an IVT file or its header is corrupted: "
        f"the first bytes are {spaced}, but IVT files start with "
        "02 00 20 00 or 04 00 20 00. Details: unrecognised IVT signature"
    )


@pytest.mark.parametrize(
    "prefix, hint",
    [
        (bytes(64), "starts with zero bytes"),
        (b"  <!DOCTYPE html><html>", "looks like an HTML or XML document"),
    ],
)
def test_signature_errors_describe_common_non_ivt_content(prefix, hint, tmp_path):
    data = bytearray(cube((2, 3)))
    data[: len(prefix)] = prefix
    assert hint in _error(bytes(data), tmp_path)


def test_file_shorter_than_header_is_reported_as_truncated(tmp_path):
    message = _error(cube((2, 3))[:100], tmp_path)
    assert message.startswith(
        CORRUPT + "the file is truncated or not an IVT file: an IVT header "
        "needs at least 576 bytes but the file is 100 bytes"
    )


# Type 3: 4 files. A record offset beyond EOF, as in 279,845,979 vs 202,680,320.


def test_record_far_past_eof_is_reported_as_truncated(tmp_path):
    data = bytearray(cube((2, 3), label_fields=1))
    master = struct.unpack_from("<I", data, 544)[0]
    offset = int(len(data) * 279_845_979 / 202_680_320)
    struct.pack_into("<I", data, master + 2 * 8, offset)
    length = struct.unpack_from("<H", data, master + 2 * 8 + 4)[0]
    message = _error(bytes(data), tmp_path)
    assert message.startswith(
        CORRUPT + "the file appears truncated: the master (axis matrix) record "
        f"at entry 2 needs at least {offset + length} bytes but the file is "
        f"{len(data)} bytes (likely an incomplete copy). Details: "
        "IVT metadata record extends past EOF: slot=544 ordinal=2"
    )


def test_cut_file_is_reported_as_truncated(tmp_path):
    data = cube((2, 3), label_fields=1)
    master = struct.unpack_from("<I", data, 544)[0]
    offset, length = struct.unpack_from("<IH", data, master + 2 * 8)
    message = _error(data[: offset + length - 1], tmp_path)
    assert message.startswith(CORRUPT + "the file appears truncated: ")


# Type 4: 1 file in 2 copies. A directory declared outside the file.


@pytest.mark.parametrize(
    "slot, name", [(544, "master (axis matrix)"), (824, "dimension 1 member")]
)
def test_directory_outside_file_names_its_slot(slot, name, tmp_path):
    data = bytearray(cube((2, 3)))
    count = struct.unpack_from("<I", data, slot + 8)[0]
    struct.pack_into("<I", data, slot, len(data) + 4096)
    message = _error(bytes(data), tmp_path)
    assert message.startswith(
        CORRUPT + f"the {name} directory points outside the file: it declares "
        f"{count} entries (flat) at byte {len(data) + 4096}, but the file is "
        f"{len(data)} bytes. Details: invalid directory: metadata directory range"
    )
    assert f"slot={slot}" in message


# Exception classes and structured fields (NAT-257).


def test_truncated_file_raises_corrupt_file_error(tmp_path):
    error = _raised(cube((2, 3))[:100], tmp_path)
    assert type(error) is pyivt.CorruptFileError
    assert (error.offset, error.length, error.file_len) == (0, 576, 100)


def test_record_past_eof_raises_corrupt_file_error_with_location(tmp_path):
    data = bytearray(cube((2, 3), label_fields=1))
    master = struct.unpack_from("<I", data, 544)[0]
    offset = len(data) * 2
    struct.pack_into("<I", data, master + 2 * 8, offset)
    length = struct.unpack_from("<H", data, master + 2 * 8 + 4)[0]
    error = _raised(bytes(data), tmp_path)
    assert type(error) is pyivt.CorruptFileError
    assert error.slot == 544
    assert error.entry == 2
    assert (error.offset, error.length, error.file_len) == (offset, length, len(data))


def test_bad_signature_raises_corrupt_file_error(tmp_path):
    data = bytearray(cube((2, 3)))
    data[:4] = bytes.fromhex("1c342034")
    error = _raised(bytes(data), tmp_path)
    assert type(error) is pyivt.CorruptFileError
    assert (error.offset, error.file_len) == (0, len(data))


def test_directory_outside_file_raises_corrupt_file_error(tmp_path):
    data = bytearray(cube((2, 3)))
    struct.pack_into("<I", data, 824, len(data) + 4096)
    error = _raised(bytes(data), tmp_path)
    assert type(error) is pyivt.CorruptFileError
    assert (error.slot, error.offset, error.file_len) == (
        824,
        len(data) + 4096,
        len(data),
    )


def test_unsupported_descriptor_raises_unsupported_format_error(tmp_path):
    data = bytearray(cube((2, 3, 5)))
    struct.pack_into("<H", data, _matrix(data) + 4, 80)
    error = _raised(bytes(data), tmp_path)
    assert type(error) is pyivt.UnsupportedFormatError
    assert str(error).startswith(UNSUPPORTED)


def test_unsupported_container_version_raises_unsupported_format_error(tmp_path):
    data = bytearray(cube((2, 3)))
    struct.pack_into("<H", data, 0, 3)
    error = _raised(bytes(data), tmp_path)
    assert type(error) is pyivt.UnsupportedFormatError
    assert error.offset == 0


def test_memory_limit_raises_resource_limit_error():
    ivt = pyivt.open(cube((1000, 1000, 250), label_fields=0))
    with pytest.raises(pyivt.ResourceLimitError) as raised:
        ivt.to_arrow()
    error = raised.value
    assert isinstance(error, MemoryError)
    assert isinstance(error, ValueError)
    assert error.limit == 2_147_483_648
    assert error.estimate is not None and error.estimate > error.limit
    assert "max_memory_bytes limit of 2 GiB" in str(error)


def test_corrupt_value_page_raises_corrupt_file_error_on_decode():
    data = bytearray(cube((2, 3), label_fields=0))
    directory = struct.unpack_from("<I", data, 558)[0]
    page = struct.pack("<BBH2048h", 2, 1, 2048, *range(2048))
    struct.pack_into("<IHH", data, directory, len(data) + 100, len(page), len(page))
    ivt = pyivt.open(bytes(data))
    with pytest.raises(pyivt.CorruptFileError) as raised:
        ivt.to_arrow()
    error = raised.value
    assert "page content outside file" in str(error)
    assert (error.slot, error.entry, error.offset, error.file_len) == (
        558,
        0,
        len(data) + 100,
        len(data),
    )


def _corruptions():
    data = cube((2, 3), label_fields=1)
    master = struct.unpack_from("<I", data, 544)[0]
    signature = bytearray(data)
    signature[:4] = bytes(4)
    version = bytearray(data)
    struct.pack_into("<H", version, 0, 3)
    matrix = bytearray(data)
    struct.pack_into("<H", matrix, _matrix(matrix) + 4, 80)
    past_eof = bytearray(data)
    struct.pack_into("<I", past_eof, master + 16, len(data) * 2)
    offset, length = struct.unpack_from("<IH", data, master + 16)
    return {
        "truncated": data[:100],
        "cut": data[: offset + length - 1],
        "signature": bytes(signature),
        "version": bytes(version),
        "matrix": bytes(matrix),
        "past_eof": bytes(past_eof),
    }


@pytest.mark.parametrize("name", sorted(_corruptions()))
def test_except_value_error_still_catches_parse_errors(name):
    data = _corruptions()[name]
    with pytest.raises(ValueError) as raised:
        pyivt.open(data)
    assert isinstance(raised.value, pyivt.IvtError)
    assert isinstance(
        raised.value, (pyivt.CorruptFileError, pyivt.UnsupportedFormatError)
    )


def test_hierarchy_is_public_and_picklable():
    import pickle

    assert issubclass(pyivt.IvtError, ValueError)
    for cls in (
        pyivt.CorruptFileError,
        pyivt.UnsupportedFormatError,
        pyivt.ResourceLimitError,
    ):
        assert issubclass(cls, pyivt.IvtError)
        assert cls.__module__ == "pyivt"
    assert issubclass(pyivt.ResourceLimitError, MemoryError)
    error = pyivt.ResourceLimitError("too big", estimate=3, limit=2, offset=1)
    copy = pickle.loads(pickle.dumps(error))
    assert (type(copy), str(copy), copy.estimate, copy.limit, copy.offset) == (
        pyivt.ResourceLimitError,
        "too big",
        3,
        2,
        1,
    )
