"""Compare each IVT's Arrow export with its reference Parquet table."""

import csv
import hashlib
import math
import os
import re
from collections.abc import Callable
from math import prod
from pathlib import Path
from typing import cast

import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq
import pyivt
import pytest
from arrow_helpers import plain_table

ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = ROOT / "data"
EXPECTED_DIR = ROOT / "tests" / "expected"
_CANIVT_MISSING_DGUID_FIXTURE = (
    "ac8356156f994c45bf464eed0e0cf114e48ccdadf23b00bb65bea8c546ffa99b"
)


def _official_98100018_dguids() -> pa.ChunkedArray:
    """Use StatCan's coordinate-keyed CSV where canivt loses the DGUID field."""
    with (ROOT / "tests" / "fixtures" / "98100018.csv").open(
        encoding="utf-8-sig", newline=""
    ) as source:
        reader = csv.reader(source)
        header = next(reader)
        assert header[:4] == ["REF_DATE", "GEO", "DGUID", "Coordinate"]
        rows = [row for row in reader if row]
    assert all(len(row) == len(header) == 26 for row in rows)
    assert [int(row[3]) for row in rows] == list(range(1, 106))
    return pa.chunked_array(
        [pa.array([row[2] or None for row in rows for _ in range(11)], pa.string())]
    )


def _canivt_projection(ivt: pyivt.Ivt) -> dict[str, str]:
    """Map the Census oracle's declared roles, without searching column values.

    canivt 0.5.0 (219486b22413f42971ff6d8b3e26b1498af3a01e) uses
    ivt_dim_slug/ivt_dim_slugs for data axes and ivt_f2_tidy for geography.
    These Census files declare English Desc, GEO_NAME_EN, DGUID_EN and
    GEO_LEVEL_DES_EN. A different source schema needs an explicit mapping.
    """
    projection = {
        "geo": "geo",
        "geo_label": "geo_english_desc",
        "geo_name": "geo_geo_name_en",
        "geo_uid": "geo_dguid_en",
        "geo_level": "geo_geo_level_des_en",
    }
    axes = [
        (slug, dimension)
        for slug, dimension in zip(
            ivt.layout.slugs, ivt.metadata.dimensions, strict=True
        )
        if slug != "geo"
    ]
    names = []
    for index, (_, dimension) in enumerate(axes, start=1):
        match = re.match(r"^[^A-Za-z]*([A-Za-z]+)", dimension.name.en or "")
        names.append(match[1].lower() if match else f"dim{index}")
    # R's make.unique reserves original names before assigning numeric suffixes.
    reserved = {"geo", "value", *names}
    used = {"geo", "value"}
    for name, (slug, _) in zip(names, axes, strict=True):
        unique = name
        if unique in used:
            suffix = 1
            while f"{name}{suffix}" in reserved | used:
                suffix += 1
            unique = f"{name}{suffix}"
        used.add(unique)
        projection[unique] = f"{slug}_english_desc"
    return projection


def _assert_grid_and_member_alignment(ivt: pyivt.Ivt, table: pa.Table) -> None:
    """Every coordinate occurs once, with all fields attached to its own axis."""
    slugs, counts = ivt.layout.slugs, ivt.layout.counts
    assert len(slugs) == len(counts) == ivt.layout.n_dim
    assert table.num_rows == prod(counts)
    assert len(table.column_names) == len(set(table.column_names))
    assert table.column_names[-5:] == [
        "value",
        "status_code",
        "symbol",
        "status",
        "footnote_raw",
    ]
    for name, expected_type in (
        ("value", pa.float64()),
        ("status_code", pa.int64()),
        ("symbol", pa.string()),
        ("status", pa.string()),
        ("footnote_raw", pa.list_(pa.uint16())),
    ):
        assert table[name].type == expected_type, name
    assert table["status_code"].null_count == 0
    assert table["footnote_raw"].null_count == 0
    assert pc.is_null(table["value"]).equals(
        pc.not_equal(table["status_code"], pa.scalar(0, pa.int64()))
    )

    # The public tidy order is lexicographic in file dimension order (last axis
    # varies fastest). Exact coordinates also establish uniqueness and full
    # Cartesian coverage; cardinalities alone cannot establish either property.
    position = 0
    for axis, (slug, count) in enumerate(zip(slugs, counts, strict=True)):
        assert table.column_names[position] == slug
        assert table[slug].type == pa.int64()
        stride = prod(counts[axis + 1 :])
        expected_ids = pa.chunked_array(
            [pa.array(row // stride % count + 1 for row in range(table.num_rows))]
        )
        assert table[slug].equals(expected_ids), slug
        position += 1
        stop = (
            table.column_names.index(slugs[axis + 1])
            if axis + 1 < len(slugs)
            else table.column_names.index("value")
        )
        assert stop > position, f"dimension {slug} has no member fields"
        member_positions = pa.array(range(0, count * stride, stride), pa.int64())
        ordinal_indices = pc.subtract(table[slug], pa.scalar(1, pa.int64()))
        for field in list(table.schema)[position:stop]:
            assert field.name.startswith(f"{slug}_"), field.name
            assert field.type in (
                pa.string(),
                pa.int64(),
                pa.float64(),
                pa.date32(),
            ), field
            column = table[field.name]
            members = pc.take(column, member_positions)
            assert column.equals(pc.take(members, ordinal_indices)), field.name
        position = stop
    assert position == table.column_names.index("value")


@pytest.mark.parametrize(
    "ivt_path", sorted(DATA_DIR.glob("*.ivt")), ids=lambda path: path.name
)
def test_to_arrow_matches_expected(ivt_path: Path) -> None:
    expected_path = EXPECTED_DIR / ivt_path.with_suffix(".parquet").name
    if not expected_path.is_file():
        pytest.skip(
            f"Missing optional {expected_path.name}; "
            "run: python scripts/generate_expected_parquet.py"
        )

    ivt = pyivt.open(ivt_path)
    actual = plain_table(ivt.to_arrow())
    expected = pq.read_table(expected_path)

    assert actual.num_rows == expected.num_rows, "Row counts differ"
    assert actual["value"].type == pa.float64()
    # The native export preserves all declared member fields, including both
    # languages and codes. canivt projects/renames a subset of those fields.
    for name in ("value", "symbol", "status"):
        reference = expected[name].cast(actual[name].type)
        assert actual[name].equals(reference), f"Column {name!r} differs"
    projection = _canivt_projection(ivt)
    for name in set(expected.column_names) - {"value", "symbol", "status"}:
        assert name in projection, f"No explicit oracle mapping for {name!r}"
        target = projection[name]
        assert target in actual.column_names, f"Missing oracle field {name!r}: {target}"
        reference = expected[name].cast(actual[target].type)
        if name == "geo_uid" and ivt_path.stem == _CANIVT_MISSING_DGUID_FIXTURE:
            # canivt 0.5.0 emits heuristic-geography warnings for this file and
            # loses its DGUIDs. The old any-string comparison hid that loss by
            # matching an unrelated null column. Keep the pinned deficiency
            # explicit and validate this field against StatCan's official CSV.
            assert reference.null_count == expected.num_rows
            assert ivt.layout.counts == [105, 11]
            reference = _official_98100018_dguids()
        column = actual[target]
        if name == "geo_name":
            # canivt's ivt_f2_geo_fill_label uses the display label for synthetic
            # aggregate geographies whose native GEO_NAME_EN is absent.
            column = pc.coalesce(column, actual["geo_english_desc"])
        # canivt trims indentation used by Beyond's hierarchical member labels.
        if column.type == pa.string():
            reference = pc.utf8_trim_whitespace(reference)
            column = pc.utf8_trim_whitespace(column)
        assert column.equals(reference), (
            f"Oracle field {name!r} differs from {target!r}"
        )


@pytest.mark.parametrize(
    "ivt_path", sorted(DATA_DIR.glob("*.ivt")), ids=lambda p: p.name
)
def test_full_grid_and_repeatable_export(ivt_path: Path, tmp_path: Path) -> None:
    ivt = pyivt.open(ivt_path)
    table = plain_table(ivt.to_arrow())
    _assert_grid_and_member_alignment(ivt, table)
    assert table.equals(plain_table(ivt.to_arrow()))
    assert table.equals(plain_table(pyivt.open(ivt_path.read_bytes()).to_arrow()))
    path = tmp_path / "full.parquet"
    pq.write_table(table, path)
    assert table.equals(pq.read_table(path))
    arrow_path = tmp_path / "full.arrow"
    with (
        pa.OSFile(str(arrow_path), "wb") as sink,
        pa.ipc.new_file(sink, table.schema) as writer,
    ):
        writer.write_table(table)
    with pa.ipc.open_file(arrow_path) as reader:
        assert table.equals(reader.read_all())
    with pytest.raises(TypeError):
        # Broaden this call only: the test deliberately violates the public signature.
        cast("Callable[..., pa.Table]", ivt.to_arrow)(complete=False)


@pytest.mark.parametrize(
    "filename", ["Oecd.ivt", "People.ivt", "Products.ivt", "Sample.ivt"]
)
def test_installed_demo_matches_beyond_csv_slice(filename: str) -> None:
    path = (
        Path(os.environ.get("ProgramFiles", "C:/Program Files"))
        / ("Beyond 2020/Professional Browser/Data")
        / filename
    )
    if not path.is_file():
        pytest.skip("Beyond demonstration file is not installed")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    reference_path = EXPECTED_DIR / f"{digest}.parquet"
    if not reference_path.is_file():
        pytest.skip("Beyond CSV reference slice is not available")
    reference = pq.read_table(reference_path)
    ivt = pyivt.open(path)
    table = plain_table(ivt.to_arrow())
    assert table.num_rows == prod(ivt.layout.counts)
    row_axis = next(
        slug
        for slug, count in zip(ivt.layout.slugs, ivt.layout.counts, strict=True)
        if count == reference.num_rows
    )
    date_field = next(
        field.name for field in table.schema if pa.types.is_date(field.type)
    )
    time_axis = next(
        slug for slug in ivt.layout.slugs if date_field.startswith(slug + "_")
    )
    mask = pa.array([True] * table.num_rows)
    for slug in ivt.layout.slugs:
        if slug not in (row_axis, time_axis):
            matches = pc.call_function("equal", [table[slug], pa.scalar(1)])
            mask = pc.call_function("and", [mask, matches])
    selected = table.filter(mask)
    assert selected.num_rows == reference.num_rows * (reference.num_columns - 1)
    # The saved CSV is a displayed slice: other axes are at their first member.
    # Its integer cells are rounded by Beyond; Arrow keeps the source precision.
    for row in range(reference.num_rows):
        for time, column in enumerate(reference.column_names[1:]):
            position = row * (reference.num_columns - 1) + time
            expected = reference[column][row].as_py()
            actual = selected["value"][position].as_py()
            try:
                numeric = float(expected)
            except (ValueError, TypeError):
                assert actual is None
                assert selected["symbol"][position].as_py() == expected
            else:
                assert actual is not None
                assert math.isclose(actual, numeric, rel_tol=0, abs_tol=0.500001)
