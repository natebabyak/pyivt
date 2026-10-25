"""Private line protocol used by the benchmark controller."""

from __future__ import annotations

import gc
import importlib.metadata
import json
import os
import platform
import sys
from collections.abc import Callable
from pathlib import Path
from time import perf_counter, process_time
from typing import TypeVar

from benchmarks.harness import BackendUnavailable

T = TypeVar("T")


def emit(message: dict) -> None:
    print(json.dumps(message), flush=True)


def measure(
    run: Callable[[], T],
    describe: Callable[[T], dict],
    *,
    backend_version: str,
    runtime: str,
    arrow_version: str | None = None,
) -> None:
    gc.collect()
    emit(
        {
            "ready": True,
            "pid": os.getpid(),
            "backend_version": backend_version,
            "runtime": runtime,
            "arrow_version": arrow_version,
        }
    )
    if not sys.stdin.readline():
        return
    cpu_start = process_time()
    start = perf_counter()
    result = run()
    wall_seconds = perf_counter() - start
    cpu_seconds = process_time() - cpu_start
    # Validation and describing the output are outside the measured interval.
    output = describe(result)
    emit({"wall_seconds": wall_seconds, "cpu_seconds": cpu_seconds, **output})
    # Keep the result alive until the controller has taken its final RSS sample.
    sys.stdin.readline()


def main() -> None:
    backend, operation, source, *options = sys.argv[1:]
    path = Path(source)
    if backend == "canivt":
        from benchmarks.r_backend import CanivtBackend

        try:
            adapter = CanivtBackend(
                operation, path, Path(options[0]) if options else None
            )
        except BackendUnavailable as error:
            emit({"unavailable": str(error)})
            raise SystemExit(2) from error
        measure(
            adapter.run,
            adapter.describe,
            backend_version=adapter.version,
            runtime=adapter.runtime,
            arrow_version=adapter.arrow_version,
        )
    elif backend == "pyivt":
        from benchmarks.adapters import (
            convert_to_arrow_with_pyivt,
            extract_metadata_with_pyivt,
        )

        version = importlib.metadata.version("pyivt")
        runtime = platform.python_version()
        if operation == "metadata":
            measure(
                lambda: extract_metadata_with_pyivt(path),
                lambda result: {"dimensions": len(result.dimensions)},
                backend_version=version,
                runtime=runtime,
            )
        elif operation == "arrow":
            import pyarrow as pa

            def describe_arrow(result: pa.Table) -> dict:
                result.validate()
                return {
                    "rows": result.num_rows,
                    "columns": result.num_columns,
                    "arrow_bytes": result.nbytes,
                }

            measure(
                lambda: convert_to_arrow_with_pyivt(path),
                describe_arrow,
                backend_version=version,
                runtime=runtime,
                arrow_version=pa.__version__,
            )
        else:
            raise ValueError(f"Unknown operation: {operation}")
    else:
        raise ValueError(f"Unknown backend: {backend}")


if __name__ == "__main__":
    main()
