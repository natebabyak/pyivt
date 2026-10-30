"""Measure peak process RSS in isolated workers; kill workers exceeding a cap.

Run with the same release build/interpreter for before/after comparisons.
Synthetic cubes have eight 47-byte label fields per dimension and zero pages.
The cap is a watchdog, not an OS address-space limit; nightly CI adds a cgroup.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import psutil


def worker(args):
    import pyarrow as pa
    import pyivt

    if args.path:
        source = args.path
    else:
        from scripts.ivt_synthetic import cube

        source = cube((1000, 1000, args.cells // 1_000_000))
    start = time.perf_counter()
    ivt = pyivt.open(source)
    rows = 0
    digest = hashlib.sha256()
    try:
        batches = (
            ivt.to_batches()
            if args.mode == "stream"
            else ivt.to_arrow().to_batches(max_chunksize=65536)
        )
        for batch in batches:
            rows += batch.num_rows
            if args.hash:
                # Normalize physical encodings so baseline and dictionary outputs
                # can be compared without changing any expected values.
                arrays = []
                for field, array in zip(batch.schema, batch.columns, strict=True):
                    if pa.types.is_dictionary(field.type):
                        array = array.dictionary_decode()
                    if pa.types.is_int32(array.type):
                        array = array.cast(pa.int64())
                    arrays.append(pa.concat_arrays([array]))
                normalized = pa.RecordBatch.from_arrays(
                    arrays, names=batch.schema.names
                )
                digest.update(normalized.serialize())
        print(
            json.dumps(
                {
                    "rows": rows,
                    "seconds": time.perf_counter() - start,
                    "sha256": digest.hexdigest() if args.hash else None,
                }
            )
        )
    except (ValueError, MemoryError) as error:
        print(
            json.dumps(
                {
                    "error": type(error).__name__,
                    "message": str(error),
                    "seconds": time.perf_counter() - start,
                }
            )
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", action="store_true")
    parser.add_argument("--mode", choices=["stream", "table"], default="stream")
    parser.add_argument("--cells", type=int, default=1_000_000)
    parser.add_argument("--path", type=Path)
    parser.add_argument("--hash", action="store_true")
    parser.add_argument("--cap-gb", type=float, default=8)
    args = parser.parse_args()
    if args.worker:
        worker(args)
        return
    command = [
        sys.executable,
        "-m",
        "scripts.benchmark_memory",
        "--worker",
        "--mode",
        args.mode,
        "--cells",
        str(args.cells),
    ]
    if args.path:
        command.extend(["--path", str(args.path)])
    if args.hash:
        command.append("--hash")
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=os.environ.copy(),
    )
    peak = 0
    exceeded = False
    while process.poll() is None:
        try:
            root = psutil.Process(process.pid)
            tree = [root, *root.children(recursive=True)]
            peak = max(peak, sum(child.memory_info().rss for child in tree))
            if peak > args.cap_gb * 1_000_000_000:
                for child in reversed(tree):
                    child.kill()
                exceeded = True
        except psutil.NoSuchProcess:
            pass
        time.sleep(0.01)
    stdout, stderr = process.communicate()
    result = {
        "mode": args.mode,
        "cells": args.cells,
        "path": str(args.path) if args.path else None,
        "peak_rss_bytes": peak,
        "exit_code": process.returncode,
        "cap_exceeded": exceeded,
    }
    if stdout.strip():
        result.update(json.loads(stdout))
    if stderr:
        result["stderr"] = stderr.strip()
    if result.get("rows"):
        result["rows_per_gb"] = result["rows"] * 1_000_000_000 / peak
    print(json.dumps(result))


if __name__ == "__main__":
    main()
