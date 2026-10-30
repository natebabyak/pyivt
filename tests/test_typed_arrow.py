"""Hand-authored native records with an independently declared full Arrow result.

This fixture uses the component and field encodings observed in the shipped
browser (see docs/investigations/ivt-native-directory.md and src/members.rs).
It is synthetic format coverage, not an export oracle for an external IVT.
Expected values below are declared by hand; pyivt never generates the oracle.
"""

import struct
from datetime import date
from pathlib import Path

import pyarrow as pa
import pyivt
import pytest
from arrow_helpers import plain_table


def _vector(width: int, payload: bytes) -> bytes:
    assert len(payload) % width == 0
    return struct.pack("<BBH", width, 1, len(payload) // width) + payload


def _text_members(*values: str) -> bytes:
    payload = bytearray(struct.pack("<H", len(values)))
    for value in values:
        encoded = value.encode("cp1252")
        payload.extend(bytes([len(encoded)]) + encoded + b"\0")
    return _vector(1, bytes(payload))


def _codes(*values: str) -> bytes:
    rows = [value.encode("cp1252") for value in values]
    # Native code-field display at Brwpro.exe 0x1401f3fb3 uses 22-byte rows.
    stride = 22
    assert max(map(len, rows)) < stride
    return struct.pack("<BBHH", 1, 2, len(rows), stride) + b"".join(
        bytes([len(row)]) + row + bytes(stride - len(row) - 1) for row in rows
    )


def _typed_fixture() -> bytes:
    """Three axes (2 x 2 x 3), all fields, one extended double data component."""
    data = bytearray(1024)
    data[:24] = bytes.fromhex(
        "04 00 20 00 20 00 00 00 40 00 08 00 20 02 00 00 20 00 0e 00 00 10 00 00"
    )

    def append(record: bytes) -> int:
        offset = len(data)
        data.extend(record)
        return offset

    def directory(slot: int, records: list[bytes]) -> None:
        count = len(records)
        capacity = 1 << (count - 1).bit_length()
        start = append(bytes(8 * capacity))
        struct.pack_into("<IIIH", data, slot, start, capacity, count, 0)
        for index, record in enumerate(records):
            if not record:
                continue
            offset = append(record)
            struct.pack_into(
                "<IHH", data, start + index * 8, offset, len(record), len(record)
            )

    def dimension(
        axis: int, fields: list[tuple[str, int, bytes]], extra: tuple[bytes, ...] = ()
    ) -> None:
        definitions = bytearray(struct.pack("<BBHH", 1, 2, len(fields), 34))
        records: list[bytes] = [b""]
        for name, kind, values in fields:
            row = bytearray(34)
            encoded = name.encode("cp1252")
            assert len(encoded) <= 17
            row[0] = len(encoded)
            row[1 : 1 + len(encoded)] = encoded
            struct.pack_into("<H", row, 18, kind)
            struct.pack_into("<I", row, 26, len(records))
            definitions.extend(row)
            records.extend([_vector(4, struct.pack("<I", len(records) + 1)), values])
        records[0] = bytes(definitions)
        records.extend(extra)
        directory(824 + 14 * axis, records)

    axis_matrix = bytearray(struct.pack("<BBHH", 1, 2, 3, 82))
    for name, count, kind in [("Region", 2, 1), ("Measure", 2, 0), ("Time", 3, 2)]:
        row = bytearray(82)
        struct.pack_into("<I", row, 0, count)
        struct.pack_into("<H", row, 6, kind)
        row[12 : 12 + len(name)] = name.encode("ascii")
        axis_matrix.extend(row)
    settings = bytearray(32)
    struct.pack_into("<H", settings, 2, 3)  # Native missing-value policy.
    directory(544, [b"", bytes(axis_matrix), _vector(1, bytes(settings))])

    # A text-reference field can point at a note or retain a null member ordinal.
    # Seven fields occupy definition 0 plus fourteen chunk/index records, so the
    # individual note is directory record 15. Its first byte is the text format.
    dimension(
        0,
        [
            ("Code", 0x0140, _codes("001", "09")),
            ("English Desc", 0x0001, _text_members("  East – Coast", "West")),
            ("French Desc", 0x0001, _text_members("  Québec – Est", "Ouest")),
            ("Small", 0x0302, _vector(2, struct.pack("<hh", -32768, 42))),
            ("Large", 0x0302, _vector(4, struct.pack("<ii", -2000000000, 2000000000))),
            ("Rate", 0x0302, _vector(8, struct.pack("<dd", 1.25, -2.5))),
            ("Notes", 0x0304, _vector(4, struct.pack("<II", 15, 0))),
        ],
        (_vector(1, b"\x01Caf\xe9"),),
    )
    dimension(
        1,
        [
            ("Code", 0x0140, _codes("01", "02")),
            ("English Desc", 0x0001, _text_members("Level", "")),
            ("ByteAttr", 0x0302, _vector(1, struct.pack("<bb", -3, 4))),
            ("LongAttr", 0x0302, _vector(4, struct.pack("<ii", -123456789, 987654321))),
        ],
    )
    dimension(
        2,
        [
            ("Code", 0x0140, _codes("000", "001", "002")),
            ("English Desc", 0x0001, _text_members("Epoch", "Leap day", "Undated")),
            (
                "Timeseries",
                0x0180,
                struct.pack("<BBHHIIIIII", 1, 2, 3, 8, 719469, 12, 730485, 999, 0, 7),
            ),
        ],
    )

    def legend_record(symbol: str, text: str) -> bytes:
        sym = symbol.encode("cp1252") + b"\0"
        desc = text.encode("cp1252") + b"\0"
        body = b"\0" + bytes([len(sym)]) + sym + struct.pack("<H", len(desc)) + desc
        return _vector(2, body + bytes(len(body) % 2))

    directory(
        698,
        [
            struct.pack("<BBHHIIII", 4, 2, 2, 2, 1, 2, 3, 4),
            legend_record("x", "Suppressed"),
            legend_record("x", "Confidentiel"),
            legend_record("..", "Not available"),
            legend_record("..", "Non disponible"),
        ],
    )

    # Power-of-two member padding leaves holes at cells 3, 7, 11, and 15.
    # Deliberately distinct data on both sides of holes detects misalignment.
    payload = bytearray(2048 * 8)
    for cell, value in [
        (0, 1.25),
        (1, -32768.0),
        (2, 987.5),
        (4, 0.0),
        (5, 100.125),
        (6, -2.5),
        (8, 3.5),
        (9, 77.0),
        (10, -1000000000000.0),
        (12, 1099511627776.5),
        (13, -0.125),
        (14, 99.0),
    ]:
        struct.pack_into("<d", payload, cell * 8, value)
    missing = bytearray(512)
    missing[0] = 0x18  # Cell 1: code 1; cell 2: code 2; cell 0 remains numeric.
    missing[3] = 0x10  # Cell 13: code 1, despite its nonzero numeric payload.
    missing_record = b"\x01\x02" + missing
    # Cell zero has two annotations, including raw reference zero.
    notes = struct.pack("<BBHHHHH", 2, 1, 4, 0, 3, 197, 449)
    trailer = struct.pack("<BHH", 2, len(missing_record), len(notes))
    trailer += missing_record + notes
    trailer += bytes(-len(trailer) % 8)
    page = bytearray(_vector(8, bytes(payload) + trailer))
    page[0] |= 0x20
    directory(558, [bytes(page)])
    return bytes(data)


def _expected_table() -> pa.Table:
    """Literal twelve-row result, including every declared member field."""
    i64 = pa.int64()
    f64 = pa.float64()
    text = pa.string()
    columns = [
        ("geo", i64, [1] * 6 + [2] * 6),
        ("geo_code", text, ["001"] * 6 + ["09"] * 6),
        ("geo_english_desc", text, ["  East – Coast"] * 6 + ["West"] * 6),
        ("geo_french_desc", text, ["  Québec – Est"] * 6 + ["Ouest"] * 6),
        ("geo_small", i64, [-32768] * 6 + [42] * 6),
        ("geo_large", i64, [-2000000000] * 6 + [2000000000] * 6),
        ("geo_rate", f64, [1.25] * 6 + [-2.5] * 6),
        ("geo_notes", text, ["Café"] * 6 + [None] * 6),
        ("measure", i64, [1, 1, 1, 2, 2, 2] * 2),
        ("measure_code", text, ["01"] * 3 + ["02"] * 3 + ["01"] * 3 + ["02"] * 3),
        (
            "measure_english_desc",
            text,
            ["Level"] * 3 + [None] * 3 + ["Level"] * 3 + [None] * 3,
        ),
        ("measure_byteattr", i64, [-3] * 3 + [4] * 3 + [-3] * 3 + [4] * 3),
        (
            "measure_longattr",
            i64,
            [-123456789] * 3 + [987654321] * 3 + [-123456789] * 3 + [987654321] * 3,
        ),
        ("time", i64, [1, 2, 3] * 4),
        ("time_code", text, ["000", "001", "002"] * 4),
        ("time_english_desc", text, ["Epoch", "Leap day", "Undated"] * 4),
        ("time_timeseries_serial", i64, [719469, 730485, 0] * 4),
        ("time_timeseries_aux", i64, [12, 999, 7] * 4),
        (
            "time_timeseries",
            pa.date32(),
            [date(1970, 1, 1), date(2000, 2, 29), None] * 4,
        ),
        (
            "value",
            f64,
            [
                1.25,
                None,
                None,
                0.0,
                100.125,
                -2.5,
                3.5,
                77.0,
                -1000000000000.0,
                1099511627776.5,
                None,
                99.0,
            ],
        ),
        ("status_code", i64, [0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0]),
        (
            "symbol",
            text,
            [None, "x", "..", None, None, None, None, None, None, None, "x", None],
        ),
        (
            "status",
            text,
            [
                None,
                "Suppressed",
                "Not available",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                "Suppressed",
                None,
            ],
        ),
        (
            "footnote_raw",
            pa.list_(pa.uint16()),
            [[0, 3], [], [], [], [], [197], [], [], [], [], [], [449]],
        ),
    ]
    return pa.table(
        [pa.array(values, type=kind) for _, kind, values in columns],
        names=[name for name, _, _ in columns],
    )


@pytest.mark.parametrize("source_kind", ["bytes", "path"])
def test_all_dimensions_and_native_types_match_declared_result(
    source_kind: str, tmp_path: Path
) -> None:
    data = _typed_fixture()
    path = tmp_path / "typed.ivt"
    path.write_bytes(data)
    ivt = pyivt.open(data if source_kind == "bytes" else path)
    assert ivt.layout.counts == [2, 2, 3]
    assert ivt.layout.slugs == ["geo", "measure", "time"]
    assert ivt.metadata.number_of_datapoints == 12
    encoded = ivt.to_arrow()
    assert encoded["geo"].type == pa.int32()
    assert encoded["geo_code"].type == pa.dictionary(pa.int32(), pa.string())
    actual = plain_table(encoded)
    expected = _expected_table()
    assert actual.shape == expected.shape == (12, 24)
    assert actual.schema.equals(expected.schema, check_metadata=True)
    for name in expected.column_names:
        assert actual[name].null_count == expected[name].null_count, name
        assert actual[name].equals(expected[name]), name
    assert actual.equals(plain_table(ivt.to_arrow()))
