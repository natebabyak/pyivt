"""Compare each IVT's Arrow export with its reference Parquet table."""

import hashlib
import math
import os
from collections.abc import Callable
from math import prod
from pathlib import Path
from typing import cast

import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq
import pyivt
import pytest

ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = ROOT / "data"
EXPECTED_DIR = ROOT / "tests" / "expected"


@pytest.mark.parametrize(
    "ivt_path", sorted(DATA_DIR.glob("*.ivt")), ids=lambda path: path.name
)
def test_to_arrow_matches_expected(ivt_path: Path) -> None:
    expected_path = EXPECTED_DIR / ivt_path.with_suffix(".parquet").name
    assert expected_path.is_file(), (
        f"Missing {expected_path.name}; "
        "run: python scripts/generate_expected_parquet.py"
    )

    actual = pyivt.open(ivt_path).to_arrow()
    expected = pq.read_table(expected_path)

    assert actual.num_rows == expected.num_rows, "Row counts differ"
    assert actual["value"].type == pa.float64()
    # The native export preserves all declared member fields, including both
    # languages and codes. canivt projects/renames a subset of those fields.
    for name in ("value", "symbol", "status"):
        reference = expected[name].cast(actual[name].type)
        assert actual[name].equals(reference), f"Column {name!r} differs"
    for name in set(expected.column_names) - {"value", "symbol", "status"}:
        reference = expected[name].cast(pa.string())
        candidates = [
            actual[field.name] for field in actual.schema if field.type == pa.string()
        ]
        # canivt trims indentation used by Beyond's hierarchical member labels.
        reference = pc.call_function("utf8_trim_whitespace", [reference])
        assert any(
            pc.call_function("utf8_trim_whitespace", [column]).equals(reference)
            for column in candidates
        ), f"Reference dimension field {name!r} is absent from {actual.column_names}"


@pytest.mark.parametrize(
    "ivt_path", sorted(DATA_DIR.glob("*.ivt")), ids=lambda p: p.name
)
def test_full_grid_and_repeatable_export(ivt_path: Path, tmp_path: Path) -> None:
    ivt = pyivt.open(ivt_path)
    table = ivt.to_arrow()
    assert table.num_rows == prod(ivt.layout.counts)
    assert len(ivt.layout.counts) == ivt.layout.n_dim
    for slug, count in zip(ivt.layout.slugs, ivt.layout.counts, strict=True):
        assert table[slug].type == pa.int64()
        assert len(table[slug].unique()) == count
        assert any(name.startswith(f"{slug}_") for name in table.column_names)
    assert table.equals(ivt.to_arrow())
    assert table.equals(pyivt.open(ivt_path.read_bytes()).to_arrow())
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
    table = ivt.to_arrow()
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
