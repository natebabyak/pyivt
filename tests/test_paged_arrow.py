"""Mandatory offline data-directory regression against official StatCan CSV.

The IVT retains 98-10-0018's native metadata/cells and relocates its data
directory into indexed pages. The paired official wide CSV supplies all
105 x 11 coordinate/value/symbol cells, geography labels and DGUIDs, and
measure labels. It does not independently validate other native member
attributes, French fields, or raw annotations. The separate schema test
states the complete Arrow type/name contract without claiming such coverage.
See the fixture provenance document for download and transformation details.
"""

import csv
import hashlib
import re
import struct
from pathlib import Path

import pyarrow as pa
import pyarrow.compute as pc
import pyivt
import pytest
from arrow_helpers import plain_table

FIXTURES = Path(__file__).resolve().parent / "fixtures"
IVT_PATH = FIXTURES / "98100018-paged.ivt"


def test_offline_fixture_identities() -> None:
    assert hashlib.sha256(IVT_PATH.read_bytes()).hexdigest() == (
        "83b1655af6af255f1f4611907892d81caed70a30eda0fc14294dbfb1ab2de56f"
    )
    assert hashlib.sha256((FIXTURES / "98100018.csv").read_bytes()).hexdigest() == (
        "402f935ce08bde6d4ae1aac9a52770d538eb9edf5b5065ae27b8dd3d6cf23045"
    )


def _official_csv_table() -> pa.Table:
    with (FIXTURES / "98100018.csv").open(encoding="utf-8-sig", newline="") as source:
        reader = csv.reader(source)
        header = next(reader)
        assert header[:4] == ["REF_DATE", "GEO", "DGUID", "Coordinate"]
        assert len(header) == 26
        measures = []
        for ordinal in range(1, 12):
            value_index = 4 + (ordinal - 1) * 2
            match = re.fullmatch(
                r"Population and dwelling counts \(11\): (.*) \[(\d+)\]",
                header[value_index],
            )
            assert match is not None, header[value_index]
            assert int(match[2]) == ordinal
            assert header[value_index + 1] == "Symbols"
            measures.append(match[1])
        rows = [row for row in reader if row]  # The export has blank trailing lines.
    assert len(rows) == 105
    assert all(len(row) == len(header) for row in rows)
    assert [int(row[3]) for row in rows] == list(range(1, 106))
    assert {row[0] for row in rows} == {"2021"}
    coordinates, members, geographies, dguids, labels, values, symbols = (
        [],
        [],
        [],
        [],
        [],
        [],
        [],
    )
    # CSV Coordinates identify geography ordinals; bracketed header indices
    # identify measure ordinals. Expand their Cartesian product in file order.
    for row in rows:
        for ordinal, label in enumerate(measures, start=1):
            value_index = 4 + (ordinal - 1) * 2
            coordinates.append(int(row[3]))
            members.append(ordinal)
            geographies.append(row[1])
            dguids.append(row[2] or None)
            labels.append(label)
            values.append(float(row[value_index]) if row[value_index] else None)
            symbols.append(row[value_index + 1] or None)
    return pa.table(
        {
            "geo": pa.array(coordinates, pa.int64()),
            "population_and": pa.array(members, pa.int64()),
            "geo_english_desc": pa.array(geographies, pa.string()),
            "geo_dguid_en": pa.array(dguids, pa.string()),
            "population_and_english_desc": pa.array(labels, pa.string()),
            "value": pa.array(values, pa.float64()),
            "symbol": pa.array(symbols, pa.string()),
        }
    )


def test_paged_arrow_matches_complete_official_csv() -> None:
    ivt = pyivt.open(IVT_PATH)
    assert ivt.layout.slugs == ["geo", "population_and"]
    assert ivt.layout.counts == [105, 11]
    assert ivt.metadata.number_of_datapoints == 1155
    actual = plain_table(ivt.to_arrow())
    expected = _official_csv_table()
    assert actual.num_rows == expected.num_rows == 1155
    for name in expected.column_names:
        column = actual[name]
        if name in ("geo_english_desc", "population_and_english_desc"):
            # Beyond stores hierarchy indentation; the public CSV omits it.
            column = pc.utf8_trim_whitespace(column)
        assert column.null_count == expected[name].null_count, name
        # All 1,155 CSV numbers match source float64 values exactly; there is
        # no rounding tolerance that could hide a bad page or cell address.
        assert column.equals(expected[name]), name


def test_paged_arrow_schema_contract() -> None:
    fields = [
        ("geo", pa.int64()),
        ("geo_code", pa.string()),
        ("geo_english_desc", pa.string()),
        ("geo_desc_fran_ais", pa.string()),
        ("geo_geo_name_en", pa.string()),
        ("geo_geo_name_fr", pa.string()),
        ("geo_geo_type_desc_en", pa.string()),
        ("geo_geo_type_desc_fr", pa.string()),
        ("geo_geo_type_abbr_en", pa.string()),
        ("geo_geo_type_abbr_fr", pa.string()),
        ("geo_geo_level_des_en", pa.string()),
        ("geo_geo_level_des_fr", pa.string()),
        ("geo_prov_abbr_en", pa.string()),
        ("geo_prov_abbr_fr", pa.string()),
        ("geo_dguid_en", pa.string()),
        ("geo_dguid_fr", pa.string()),
        ("geo_alt_geo_code_en", pa.string()),
        ("geo_alt_geo_code_fr", pa.string()),
        ("geo_pr_code_en", pa.string()),
        ("geo_pr_code_fr", pa.string()),
        ("geo_description", pa.string()),
        ("geo_itemnotes2", pa.string()),
        ("geo_decimals", pa.int64()),
        ("population_and", pa.int64()),
        ("population_and_code", pa.string()),
        ("population_and_english_desc", pa.string()),
        ("population_and_desc_fran_ais", pa.string()),
        ("value", pa.float64()),
        ("status_code", pa.int64()),
        ("symbol", pa.string()),
        ("status", pa.string()),
        ("footnote_raw", pa.list_(pa.uint16())),
    ]
    table = plain_table(pyivt.open(IVT_PATH).to_arrow())
    assert table.num_columns == len(fields) == 32
    assert table.schema.equals(pa.schema(fields), check_metadata=False)


@pytest.mark.parametrize(
    ("mutation", "reason"),
    [
        ("page_size", "unsupported directory page size 2048"),
        ("count", "entry count exceeds capacity"),
        ("zero_count", "metadata entry index"),
        ("null_pointer", "metadata page pointer"),
        ("outside_pointer", "metadata directory range"),
        ("truncated_index", "metadata directory range"),
    ],
)
def test_paged_directory_corruption_is_rejected(mutation: str, reason: str) -> None:
    data = bytearray(IVT_PATH.read_bytes())
    index_start = struct.unpack_from("<I", data, 558)[0]
    if mutation == "page_size":
        struct.pack_into("<H", data, 20, 2048)
    elif mutation == "count":
        struct.pack_into("<I", data, 566, 4097)
    elif mutation == "zero_count":
        struct.pack_into("<I", data, 566, 0)
    elif mutation == "null_pointer":
        struct.pack_into("<I", data, index_start, 0)
    elif mutation == "outside_pointer":
        struct.pack_into("<I", data, index_start, len(data) - 7)
    elif mutation == "truncated_index":
        del data[-1]
    ivt = pyivt.open(bytes(data))
    with pytest.raises(ValueError, match=reason):
        ivt.to_arrow()
