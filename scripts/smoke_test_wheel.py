"""Smoke-test an installed pyivt wheel.

The release workflow installs each built wheel into a clean environment on its
own OS and architecture, then runs this script against the committed fixture:

    python scripts/smoke_test_wheel.py tests/fixtures/98100018-paged.ivt

It opens the fixture, exports it with ``to_arrow()`` and ``to_member_table()``,
and checks the results against the independently published StatCan CSV for
the same table (98-10-0018: 105 geographies x 11 measures). It exits non-zero
on any failure. Only the standard library and pyarrow are needed, so the
script runs without the dev dependency group.

It refuses to run against the source tree: ``pyivt`` must be imported from the
installed wheel, not from ``python/`` in a checkout.
"""

from __future__ import annotations

import argparse
import math
import platform
import sys
from pathlib import Path

GEOGRAPHIES = 105
MEASURES = 11
# First CSV row: Quebec, "Population, 2021".
QUEBEC_POPULATION_2021 = 8501833.0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("fixture", type=Path)
    parser.add_argument(
        "--expect-version",
        help="fail unless pyivt.__version__ equals this version",
    )
    args = parser.parse_args()

    import pyarrow as pa
    import pyivt

    source_tree = Path(__file__).resolve().parent.parent / "python"
    module_path = Path(pyivt.__file__).resolve()
    if source_tree in module_path.parents:
        raise SystemExit(
            f"pyivt was imported from the source tree ({module_path}), "
            "not from an installed wheel"
        )

    print(
        f"pyivt {pyivt.__version__} from {module_path.parent}\n"
        f"Python {platform.python_version()} ({platform.python_implementation()}) "
        f"on {platform.system()} {platform.machine()}, pyarrow {pa.__version__}"
    )
    if args.expect_version and pyivt.__version__ != args.expect_version:
        raise SystemExit(
            f"pyivt.__version__ is {pyivt.__version__}, expected {args.expect_version}"
        )

    ivt = pyivt.open(args.fixture)
    dimensions = ivt.metadata.dimensions
    items = [dimension.items for dimension in dimensions]
    check(sorted(items) == sorted([GEOGRAPHIES, MEASURES]), f"dimension sizes {items}")

    table = ivt.to_arrow()
    check(isinstance(table, pa.Table), f"to_arrow() returned {type(table)}")
    check(
        table.num_rows == GEOGRAPHIES * MEASURES,
        f"to_arrow() has {table.num_rows} rows, expected {GEOGRAPHIES * MEASURES}",
    )
    expected_columns = [dimension.slug for dimension in dimensions] + ["value"]
    check(
        table.column_names == expected_columns,
        f"to_arrow() columns {table.column_names}, expected {expected_columns}",
    )
    check(
        table.schema.field("value").type == pa.float64(),
        f"value column is {table.schema.field('value').type}",
    )
    first = table.slice(0, 1).to_pylist()[0]
    check(
        all(first[slug] == 1 for slug in expected_columns[:-1]),
        f"first row codes {first}",
    )
    check(
        math.isclose(first["value"], QUEBEC_POPULATION_2021),
        f"first value {first['value']}, expected {QUEBEC_POPULATION_2021}",
    )
    check(table.column("value").null_count < table.num_rows, "every value is null")

    members = ivt.to_member_table()
    check(isinstance(members, pa.Table), f"to_member_table() returned {type(members)}")
    check(
        members.num_rows == GEOGRAPHIES + MEASURES,
        f"to_member_table() has {members.num_rows} rows, "
        f"expected {GEOGRAPHIES + MEASURES}",
    )
    check(
        members.column_names[:3] == ["dimension", "code", "label"],
        f"to_member_table() columns {members.column_names}",
    )
    check(members.column("label").null_count < members.num_rows, "every label is null")

    selected = ivt.to_arrow(select={expected_columns[0]: [1]})
    check(
        selected.num_rows == table.num_rows // items[0],
        f"select returned {selected.num_rows} rows",
    )

    print(
        f"OK: to_arrow() {table.num_rows} rows, "
        f"to_member_table() {members.num_rows} rows"
    )
    return 0


def check(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"smoke test failed: {message}")


if __name__ == "__main__":
    sys.exit(main())
