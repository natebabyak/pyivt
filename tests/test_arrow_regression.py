"""Opt-in comparison of NAICS234 with a complete independent Arrow oracle.

Set PYIVT_REGRESSION_NAICS234_PATH to the IVT and
PYIVT_REGRESSION_NAICS234_EXPECTED_PARQUET to its trusted expected table.
Create that reference from Beyond 20/20's all-dimensions export or another
independent reader, retain its source/precision settings, and normalize its
column names, native types and dimension order to the documented pyivt schema.
Do not generate the expected table with pyivt. The reference must cover every
column and cell, including member fields, nulls, status and raw annotations.

Only absence of the IVT environment variable skips this test. An explicitly
requested case with a missing file or oracle fails, so a successful parse
cannot be mistaken for independently verified correctness.
"""

import os
from math import prod
from pathlib import Path

import pyarrow.parquet as pq
import pyivt
import pytest


def test_naics234_matches_complete_independent_export() -> None:
    ivt_name = os.environ.get("PYIVT_REGRESSION_NAICS234_PATH")
    if not ivt_name:
        pytest.skip("Set PYIVT_REGRESSION_NAICS234_PATH for the full-file regression")
    ivt_path = Path(ivt_name)
    assert ivt_path.is_file(), f"Regression IVT is unavailable: {ivt_path}"
    reference_name = os.environ.get("PYIVT_REGRESSION_NAICS234_EXPECTED_PARQUET")
    assert reference_name, (
        "Set PYIVT_REGRESSION_NAICS234_EXPECTED_PARQUET to a complete independent "
        "export; parsing without an oracle does not validate data correctness"
    )
    reference_path = Path(reference_name)
    assert reference_path.is_file(), (
        f"Independent export is unavailable: {reference_path}"
    )
    ivt = pyivt.open(ivt_path)
    actual = ivt.to_arrow()
    expected = pq.read_table(reference_path)
    assert actual.num_rows == expected.num_rows == prod(ivt.layout.counts)
    assert actual.num_columns == expected.num_columns
    assert actual.schema.equals(expected.schema, check_metadata=False), (
        f"Schema differs:\nactual: {actual.schema}\nexpected: {expected.schema}"
    )
    for name in expected.column_names:
        assert actual[name].null_count == expected[name].null_count, name
        assert actual[name].equals(expected[name]), f"Column {name!r} differs"
