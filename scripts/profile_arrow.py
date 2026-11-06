"""Profile compact vs pre-0.1 export in isolated, memory-capped workers.

Examples:
    python -m scripts.profile_arrow --mode compact --cells 80000000
    python -m scripts.profile_arrow --mode compact --cells 80000000 --fraction .05
    python -m scripts.profile_arrow --mode legacy --cells 80000000
    python -m scripts.profile_arrow --path private.ivt --select selection.json

The RSS watchdog is sampled, not an OS limit. Stage RSS is total process RSS
during each stage, not exclusive allocations. Native column construction and
value decoding overlap. Synthetic cubes have zero pages and eight text fields
per dimension; they measure row construction, not realistic decompression.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from math import ceil
from pathlib import Path
from queue import Empty, Queue
from threading import Thread

import psutil


def worker(args):
    os.environ["PYIVT_PROFILE"] = "1"
    import pyarrow as pa
    import pyivt

    from scripts.ivt_synthetic import cube

    source = args.path or cube((1000, 1000, args.cells // 1_000_000))
    start = time.perf_counter()
    ivt = pyivt.open(source)
    select = json.loads(args.select.read_text()) if args.select else None
    if args.fraction < 1:
        first = ivt.metadata.dimensions[0]
        count = ceil(first.items * args.fraction)
        select = {first.slug: list(range(1, count + 1))}
    print(
        json.dumps(
            {
                "ready_rss_bytes": psutil.Process().memory_info().rss,
                "pyarrow_version": pa.__version__,
                "pyivt_version": pyivt.__version__,
                "python_version": sys.version.split()[0],
                "platform": sys.platform,
            }
        ),
        flush=True,
    )
    try:
        if args.mode == "legacy":
            if select is not None:
                raise ValueError("legacy baseline has no selection API")
            table = ivt._legacy_to_arrow(max_memory_bytes=args.max_memory_bytes)
        else:
            table = ivt.to_arrow(**(select or {}))
        result = {
            "rows": table.num_rows,
            "columns": table.num_columns,
            "arrow_bytes": table.nbytes,
        }
    except (ValueError, MemoryError) as error:
        result = {"error": type(error).__name__, "message": str(error)}
    print(json.dumps({**result, "seconds": time.perf_counter() - start}), flush=True)


def profile(args):
    command = [
        sys.executable,
        "-m",
        "scripts.profile_arrow",
        "--worker",
        "--mode",
        args.mode,
        "--cells",
        str(args.cells),
        "--fraction",
        str(args.fraction),
        "--max-memory-bytes",
        str(args.max_memory_bytes),
    ]
    if args.path:
        command.extend(["--path", str(args.path)])
    if args.select:
        command.extend(["--select", str(args.select)])
    stages, active = {}, {}
    peak = 0
    termination = None
    started = time.monotonic()
    environment = dict(os.environ, PYTHONPATH=os.pathsep.join(map(str, sys.path)))
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=environment,
    )
    events = Queue()
    errors = []

    def read_events():
        for line in process.stderr:
            try:
                event = json.loads(line)
                if "stage" in event:
                    events.put(event)
                else:
                    errors.append(line)
            except json.JSONDecodeError:
                errors.append(line)

    receiver = Thread(target=read_events, daemon=True)
    receiver.start()
    previous_rss = 0
    while True:
        try:
            root = psutil.Process(process.pid)
            tree = [root, *root.children(recursive=True)]
            rss = sum(child.memory_info().rss for child in tree)
        except psutil.NoSuchProcess:
            rss = 0
        peak = max(peak, rss)
        finished = process.poll() is not None
        if finished:
            receiver.join(timeout=5)
        while True:
            try:
                event = events.get_nowait()
            except Empty:
                break
            name = event["stage"]
            stage = stages.setdefault(
                name, {"seconds": 0, "peak_rss_bytes": 0, "completed_calls": 0}
            )
            stage["peak_rss_bytes"] = max(stage["peak_rss_bytes"], rss, previous_rss)
            if event["event"] == "start":
                active[name] = True
            else:
                active.pop(name, None)
                stage["seconds"] += event["seconds"]
                stage["completed_calls"] += 1
        for name in active:
            stages[name]["peak_rss_bytes"] = max(stages[name]["peak_rss_bytes"], rss)
        previous_rss = rss
        if finished:
            break
        if rss > args.rss_cap_bytes:
            termination = "rss_cap_exceeded"
        elif time.monotonic() - started > args.timeout:
            termination = "timeout"
        if termination:
            for child in reversed(tree):
                try:
                    child.kill()
                except psutil.NoSuchProcess:
                    pass
            process.wait()
        time.sleep(0.005)
    output = {}
    for line in process.stdout:
        output.update(json.loads(line))
    if errors:
        output["stderr"] = "".join(errors)[-4000:]
    return {
        "mode": args.mode,
        "path": str(args.path) if args.path else None,
        "synthetic_cells": args.cells if not args.path else None,
        "fraction": args.fraction,
        "peak_rss_bytes": peak,
        "termination": termination,
        "exit_code": process.returncode,
        "stages": stages,
        **output,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", action="store_true")
    parser.add_argument("--mode", choices=["compact", "legacy"], default="compact")
    parser.add_argument("--path", type=Path)
    parser.add_argument(
        "--select", type=Path, help="JSON mapping of dimension slugs to integer codes"
    )
    parser.add_argument("--cells", type=int, default=80_000_000)
    parser.add_argument("--fraction", type=float, default=1)
    parser.add_argument(
        "--max-memory-bytes",
        type=int,
        default=2_147_483_648,
        help="estimate budget for legacy mode",
    )
    parser.add_argument("--rss-cap-bytes", type=int, default=2_000_000_000)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 0 < args.fraction <= 1:
        parser.error("fraction must be in (0, 1]")
    if not args.path and (args.cells < 1_000_000 or args.cells % 1_000_000):
        parser.error("synthetic cells must be a positive multiple of 1000000")
    if args.worker:
        worker(args)
    else:
        result = json.dumps(profile(args))
        if args.output:
            args.output.write_text(result + "\n", encoding="utf-8")
        print(result)


if __name__ == "__main__":
    main()
