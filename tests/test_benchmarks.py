"""Regression checks for discovery and native-process measurement."""

import json
import subprocess
import sys
import textwrap

import pytest

from benchmarks.harness import ROOT, BackendUnavailable, discover_files, measure


def test_benchmark_discovery(tmp_path):
    (tmp_path / "nested").mkdir()
    for name in ("b.ivt", "a.IVT", "nested/a.ivt", "ignore.parquet"):
        (tmp_path / name).touch()
    (tmp_path / "directory.ivt").mkdir()
    assert [p.relative_to(tmp_path).as_posix() for p in discover_files(tmp_path)] == [
        "a.IVT",
        "b.ivt",
        "nested/a.ivt",
    ]
    assert discover_files(tmp_path / "absent") == []


def run_worker(code, **kwargs):
    pytest.importorskip("psutil")
    return measure([sys.executable, "-u", "-c", textwrap.dedent(code)], **kwargs)


def test_benchmark_measures_worker_memory_and_excludes_startup():
    result = run_worker(
        """
        import json, os, sys, time
        time.sleep(0.2)
        print('startup noise', flush=True)
        print(json.dumps({'ready': True, 'pid': os.getpid()}), flush=True)
        sys.stdin.readline()
        start = time.perf_counter()
        cpu = time.process_time()
        data = bytearray(b'x' * (16 * 1024 * 1024))
        time.sleep(0.03)
        print(json.dumps({'wall_seconds': time.perf_counter() - start,
                          'cpu_seconds': time.process_time() - cpu}), flush=True)
        sys.stdin.readline()
        """
    )
    assert 0 < result["wall_seconds"] < 0.2
    assert result["cpu_seconds"] >= 0
    assert result["sampled_rss_growth_bytes"] >= 8 * 1024 * 1024
    assert result["sampled_peak_rss_bytes"] >= result["baseline_rss_bytes"]
    assert result["rss_samples"] >= 2


def test_benchmark_missing_dependency_is_distinct_from_failure():
    with pytest.raises(BackendUnavailable, match="canivt"):
        run_worker('print(\'{"unavailable":"canivt"}\', flush=True)')
    with pytest.raises(RuntimeError, match="broken decoder"):
        run_worker("raise RuntimeError('broken decoder')")
    with pytest.raises(RuntimeError, match="decode failed after startup"):
        run_worker(
            """
            import json, os, sys
            print(json.dumps({'ready': True, 'pid': os.getpid()}), flush=True)
            sys.stdin.readline()
            raise RuntimeError('decode failed after startup')
            """
        )


def test_benchmark_timeout_kills_worker():
    with pytest.raises(TimeoutError, match="exceeded"):
        run_worker(
            """
            import json, os, sys, time
            print(json.dumps({'ready': True, 'pid': os.getpid()}), flush=True)
            sys.stdin.readline()
            time.sleep(60)
            """,
            timeout=0.5,
        )


def test_benchmark_report_records_each_discovered_failure(tmp_path):
    pytest.importorskip("psutil")
    source = tmp_path / "inputs" / "nested" / "invalid.IVT"
    source.parent.mkdir(parents=True)
    source.write_bytes(b"invalid IVT")
    report = tmp_path / "report.json"
    process = subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "benchmarks",
            "--ivt-data-dir",
            str(source.parent.parent),
            "--ivt-backends",
            "pyivt",
            "--ivt-rounds",
            "1",
            "--ivt-report",
            str(report),
            "-q",
            "-p",
            "no:cacheprovider",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert process.returncode == 1, process.stdout + process.stderr
    results = json.loads(report.read_text(encoding="utf-8"))["results"]
    assert len(results) == 2
    assert {result["operation"] for result in results} == {"metadata", "arrow"}
    assert all(result["status"] == "failed" for result in results)
    assert all(result["file"] == str(source) for result in results)
    assert all("ValueError" in result["reason"] for result in results)
