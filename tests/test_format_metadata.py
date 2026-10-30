"""Non-Census names must not substitute for native axis roles."""

import struct
from pathlib import Path

import pyivt
from test_typed_arrow import _typed_fixture

from scripts.ivt_synthetic import cube


def test_date_name_does_not_make_a_normal_axis_time_series():
    data = bytearray(cube((2, 3)))
    master = struct.unpack_from("<I", data, 544)[0]
    matrix = struct.unpack_from("<I", data, master + 8)[0]
    data[matrix + 18 : matrix + 36] = b"Time Period".ljust(18, b"\0")
    ivt = pyivt.open(bytes(data))
    assert ivt.metadata.dimensions[0].type == "normal"
    assert ivt.layout.geo_dim is None
    assert ivt.layout.slugs == ["time_period", "axis_1"]
    assert ivt.metadata.geographies.member_id == []


def test_native_time_kind_is_used_even_without_a_time_name():
    data = bytearray(_typed_fixture())
    master = struct.unpack_from("<I", data, 544)[0]
    matrix = struct.unpack_from("<I", data, master + 8)[0]
    data[matrix + 6 + 2 * 82 + 12 : matrix + 6 + 2 * 82 + 30] = b"Accounting".ljust(
        18, b"\0"
    )
    assert pyivt.open(bytes(data)).metadata.dimensions[2].type == "time_series"


def test_arbitrary_header_text_cannot_supply_product_or_language():
    data = bytearray(cube((2, 3)))
    bait = b"Product ID: 12345678\nTitle: les donnees de la population\nTitre: English title\n"
    data[60 : 60 + len(bait)] = bait
    meta = pyivt.open(bytes(data)).metadata
    assert meta.product_id is None
    assert meta.title.en is None
    assert meta.title.fr is None


def test_census_geography_uid_comes_from_native_dguid_member_column():
    ivt = pyivt.open(Path(__file__).parent / "fixtures" / "98100018-paged.ivt")
    assert ivt.layout.geo_dim == 1
    actual = ivt.metadata.geographies.geo_uid
    native = next(ivt.to_batches()).column("geo_dguid_en").to_pylist()
    assert actual == native[::11]
    assert actual[0] == "2021A000224"
    assert any(value is None for value in actual)
