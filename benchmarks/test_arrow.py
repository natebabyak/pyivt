from pathlib import Path

import pyarrow as pa
import pyivt
from pytest_benchmark.fixture import BenchmarkFixture


def test_arrow(
    benchmark: BenchmarkFixture,
    benchmark_file: Path,
) -> None:
    table = benchmark(pyivt.read_arrow, benchmark_file)

    assert isinstance(table, pa.Table)

    benchmark.extra_info["input_bytes"] = benchmark_file.stat().st_size
    benchmark.extra_info["rows"] = table.num_rows
    benchmark.extra_info["columns"] = table.num_columns
    benchmark.extra_info["arrow_bytes"] = table.nbytes
