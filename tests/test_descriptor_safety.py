"""Malformed descriptors cannot fall back to plausible strings in table notes."""

import struct

import pyivt
import pytest

from scripts.ivt_synthetic import cube


def _matrix(data: bytes) -> int:
    master = struct.unpack_from("<I", data, 544)[0]
    return struct.unpack_from("<I", data, master + 8)[0]


@pytest.mark.parametrize("source", ["path", "bytes"])
@pytest.mark.parametrize(
    "mutation",
    [
        "missing",
        "stride",
        "rank",
        "kind",
        "zero_count",
        "huge_count",
        "ndim",
        "overflow",
    ],
)
def test_malformed_matrix_is_rejected_without_scanning_notes(
    mutation, source, tmp_path
):
    data = bytearray(cube((2, 3, 5)))
    matrix = _matrix(data)
    if mutation == "missing":
        master = struct.unpack_from("<I", data, 544)[0]
        data[master + 8 : master + 16] = bytes(8)
    elif mutation == "stride":
        struct.pack_into("<H", data, matrix + 4, 81)
    elif mutation == "rank":
        data[matrix + 1] = 1
    elif mutation == "kind":
        struct.pack_into("<H", data, matrix + 6 + 6, 3)
    elif mutation == "zero_count":
        struct.pack_into("<I", data, matrix + 6, 0)
    elif mutation == "huge_count":
        struct.pack_into("<I", data, matrix + 6, 2**31)
    elif mutation == "ndim":
        struct.pack_into("<H", data, matrix + 2, 33)
    elif mutation == "overflow":
        for axis in range(3):
            struct.pack_into("<I", data, matrix + 6 + 82 * axis, 2**31 - 1)
    # Bait for the deleted scanner: a plausible legacy dimension chain followed
    # by prose. Neither the pointer nor arbitrary marker scans may rescue it.
    struct.pack_into("<I", data, 32, len(data))
    bait = bytearray(42)
    bait[:4] = b"\x81\x01\x20\x00"
    struct.pack_into("<H", bait, 16, 2)
    bait.extend(b"\x02\x01\x01Census yearCensus year\x79\x07\x01PlacesPlaces\x81\x01")
    data.extend(bait)
    data.extend(b" Census year (2) diskettes secretariat more axes in a note")
    path = tmp_path / "invalid.ivt"
    path.write_bytes(data)
    with pytest.raises(
        ValueError, match="native axis matrix.*unverified descriptor fallback"
    ):
        pyivt.open(path if source == "path" else bytes(data))


def test_descriptor_path_is_exposed_and_names_do_not_affect_counts():
    data = bytearray(cube((2, 121)))
    matrix = _matrix(data)
    data[matrix + 18 : matrix + 36] = b"Census year (2)".ljust(18, b"\0")
    ivt = pyivt.open(bytes(data))
    assert ivt.metadata.descriptor_path == "axis_matrix"
    assert ivt.layout.counts == [2, 121]
    assert ivt.metadata.number_of_datapoints == 242
