"""Private line protocol used by the benchmark controller."""

import gc
import importlib.metadata
import json
import os
import platform
import sys
from pathlib import Path
from time import perf_counter, process_time

from benchmarks.adapters import (
    convert_to_arrow_with_pyivt,
    extract_metadata_with_pyivt,
)


def emit(message: dict) -> None:
    print(json.dumps(message), flush=True)


def main() -> None:
    operation, source = sys.argv[1:]
    path = Path(source)
    function = {
        "metadata": extract_metadata_with_pyivt,
        "arrow": convert_to_arrow_with_pyivt,
    }[operation]
    arrow_version = None
    if operation == "arrow":
        import pyarrow as pa

        arrow_version = pa.__version__
    gc.collect()
    emit(
        {
            "ready": True,
            "pid": os.getpid(),
            "backend_version": importlib.metadata.version("pyivt"),
            "runtime": platform.python_version(),
            "arrow_version": arrow_version,
        }
    )
    if not sys.stdin.readline():
        return
    cpu_start = process_time()
    start = perf_counter()
    result = function(path)
    wall_seconds = perf_counter() - start
    cpu_seconds = process_time() - cpu_start
    # Validation and describing the output are outside the measured interval.
    if operation == "arrow":
        result.validate()
        output = {
            "rows": result.num_rows,
            "columns": result.num_columns,
            "arrow_bytes": result.nbytes,
        }
    else:
        output = {"dimensions": len(result.dimensions)}
    emit({"wall_seconds": wall_seconds, "cpu_seconds": cpu_seconds, **output})
    # Keep the result alive until the controller has taken its final RSS sample.
    sys.stdin.readline()


if __name__ == "__main__":
    main()
