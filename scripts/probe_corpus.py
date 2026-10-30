"""Isolated corpus conversions with hash, Cartesian-row and memory checks."""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from math import prod
from pathlib import Path

import psutil


def worker(path: Path, mode: str, memory: int) -> dict:
    import pyivt

    result = {"mode": mode}
    start = time.perf_counter()
    try:
        ivt = pyivt.open(path)
        result.update(
            counts=ivt.layout.counts, descriptor_path=ivt.metadata.descriptor_path
        )
        batches = (
            ivt.to_batches(max_memory_bytes=memory)
            if mode == "stream"
            else ivt.to_arrow(max_memory_bytes=memory).to_batches()
        )
        rows = sum(batch.num_rows for batch in batches)
        if rows != prod(ivt.layout.counts):
            raise AssertionError(
                f"row count {rows} differs from dimension product {prod(ivt.layout.counts)}"
            )
        result.update(status="converted", rows=rows)
    except (ValueError, MemoryError) as error:
        result.update(
            status="rejected", exception=type(error).__name__, message=str(error)
        )
    result["seconds"] = time.perf_counter() - start
    return result


def isolated(path: Path, mode: str, memory: int, cap: int, timeout: float) -> dict:
    command = [
        sys.executable,
        "-m",
        "scripts.probe_corpus",
        "--worker",
        str(path),
        "--mode",
        mode,
        "--max-memory-bytes",
        str(memory),
    ]
    start = time.monotonic()
    peak = 0
    termination = None
    with (
        tempfile.TemporaryFile(mode="w+", encoding="utf-8") as stdout,
        tempfile.TemporaryFile(mode="w+", encoding="utf-8") as stderr,
    ):
        environment = dict(os.environ, PYTHONPATH=os.pathsep.join(map(str, sys.path)))
        process = subprocess.Popen(
            command, stdout=stdout, stderr=stderr, env=environment
        )
        while process.poll() is None:
            try:
                root = psutil.Process(process.pid)
                tree = [root, *root.children(recursive=True)]
                peak = max(peak, sum(child.memory_info().rss for child in tree))
                if peak > cap:
                    termination = "memory_cap_exceeded"
                elif time.monotonic() - start > timeout:
                    termination = "timeout"
                if termination:
                    for child in reversed(tree):
                        child.kill()
            except psutil.NoSuchProcess:
                pass
            time.sleep(0.01)
        stdout.seek(0)
        stderr.seek(0)
        output, errors = stdout.read(), stderr.read()
    if process.returncode or termination:
        result = {
            "mode": mode,
            "status": termination or "crashed",
            "exit_code": process.returncode,
            "stderr": errors[-4000:],
        }
    else:
        try:
            result = json.loads(output)
        except json.JSONDecodeError:
            result = {
                "mode": mode,
                "status": "invalid_worker_output",
                "stdout": output[-4000:],
                "stderr": errors[-4000:],
            }
    result["peak_rss_bytes"] = peak
    return result


def probe(
    manifest: dict, directory: Path, *, memory: int, cap: int, timeout: float = 300
) -> list[dict]:
    results = []
    for entry in manifest["files"]:
        path = directory / f"{entry['sha256']}.ivt"
        digest = hashlib.sha256()
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
        actual = digest.hexdigest()
        if actual != entry["sha256"]:
            raise ValueError(f"pinned hash differs for {path}")
        for mode in ["stream", "table"]:
            result = isolated(path, mode, memory, cap, timeout)
            result.update(sha256=entry["sha256"], bytes=path.stat().st_size)
            expected = entry.get("official_dimension_counts")
            if (
                expected
                and result.get("counts") is not None
                and result["counts"] != expected
            ):
                result.update(status="dimension_mismatch", expected_counts=expected)
            results.append(result)
            print(json.dumps(result), flush=True)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("--mode", choices=["stream", "table"], default="stream")
    parser.add_argument(
        "--manifest", type=Path, default=Path("tests/corpus/heldout.json")
    )
    parser.add_argument("--data-dir", type=Path, default=Path(".cache/heldout"))
    parser.add_argument("--max-memory-bytes", type=int, default=6 * 1024**3)
    parser.add_argument("--rss-cap-bytes", type=int, default=8 * 1024**3)
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument(
        "--output", type=Path, default=Path(".cache/heldout-results.jsonl")
    )
    args = parser.parse_args()
    if args.worker:
        print(json.dumps(worker(args.worker, args.mode, args.max_memory_bytes)))
        return
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    results = probe(
        manifest,
        args.data_dir,
        memory=args.max_memory_bytes,
        cap=args.rss_cap_bytes,
        timeout=args.timeout,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        "".join(json.dumps(row) + "\n" for row in results), encoding="utf-8"
    )
    # Catchable refusals are reported explicitly; aborts, caps, wrong rows and
    # disagreement with official dimension counts always fail the run.
    if any(row["status"] not in {"converted", "rejected"} for row in results):
        raise SystemExit(1)
    if not any(
        row["status"] == "converted" and row["mode"] == "stream" for row in results
    ):
        raise SystemExit("no held-out file completed a stream conversion")


if __name__ == "__main__":
    main()
