"""Benchmark metadata I/O: bytes read, ops, timings for path opens.

Usage:
  python tests/bench_metadata_io.py
  set PYIVT_BENCH_IVT=\\\\server\\share\\big.ivt && python tests/bench_metadata_io.py
"""

from __future__ import annotations

import os
import sys
import time
from pathlib import Path

_TESTS = Path(__file__).resolve().parent
sys.path.insert(0, str(_TESTS))

import pyivt
from conftest import DEMO_MANIFEST, STATCAN_DIR, STATCAN_MANIFEST, resolve_demo_ivt


def _paths() -> list[Path]:
    out: list[Path] = []
    for e in STATCAN_MANIFEST:
        p = STATCAN_DIR / e["filename"]
        if p.is_file():
            out.append(p)
    for e in DEMO_MANIFEST:
        p = resolve_demo_ivt(e["filename"])
        if p is not None:
            out.append(p)
    env = os.environ.get("PYIVT_BENCH_IVT")
    if env:
        p = Path(env)
        if p.is_file():
            out.append(p)
    return out


def _bench_one(path: Path, warm: bool) -> dict:
    t0 = time.perf_counter()
    ivt = pyivt.open(path)
    t_open = time.perf_counter() - t0

    meta = ivt.metadata
    t1 = time.perf_counter()
    _ = (
        meta.product_id,
        meta.title.en,
        meta.title.fr,
        meta.author.en,
        meta.category.en,
        meta.keywords.en,
        meta.notes.en,
        meta.number_of_datapoints,
        meta.time_series,
        meta.read_only,
    )
    t_summary = time.perf_counter() - t1

    lazy_times = {}
    for name, getter in (
        ("dimensions", lambda: meta.dimensions),
        ("geographies", lambda: meta.geographies),
        ("footnotes", lambda: meta.footnotes),
        ("missing_values", lambda: meta.missing_values),
    ):
        t = time.perf_counter()
        _ = getter()
        lazy_times[name] = time.perf_counter() - t

    bytes_read, read_ops, largest, fallback = meta._io_metrics()
    total = time.perf_counter() - t0
    return {
        "path": str(path),
        "file_size": path.stat().st_size,
        "cold_warm": "warm" if warm else "cold",
        "bytes_requested": bytes_read,
        "read_operations": read_ops,
        "largest_range_read": largest,
        "time_open_s": t_open,
        "time_summary_s": t_summary,
        "time_lazy_s": lazy_times,
        "total_s": total,
        "fallback_used": fallback,
        "pct_bytes": 100.0 * bytes_read / max(path.stat().st_size, 1),
    }


def main() -> int:
    paths = _paths()
    if not paths:
        print("no IVT fixtures found; run tests/scripts/download.py", file=sys.stderr)
        return 1
    print(
        "path\tsize\tcold/warm\tbytes\top\tlargest\tpct\topen_s\tsummary_s\ttotal_s\tfallback"
    )
    for path in paths:
        # First pass approximates cold (OS cache may still help).
        r0 = _bench_one(path, warm=False)
        r1 = _bench_one(path, warm=True)
        for r in (r0, r1):
            print(
                f"{Path(r['path']).name}\t{r['file_size']}\t{r['cold_warm']}\t"
                f"{r['bytes_requested']}\t{r['read_operations']}\t{r['largest_range_read']}\t"
                f"{r['pct_bytes']:.2f}\t{r['time_open_s']:.4f}\t{r['time_summary_s']:.4f}\t"
                f"{r['total_s']:.4f}\t{r['fallback_used']}"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
