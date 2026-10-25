"""Opt-in dynamic benchmarks: pytest benchmarks --ivt-report results.json."""

import json
import platform
from datetime import datetime, timezone
from pathlib import Path

import pytest

from benchmarks.harness import (
    BACKENDS,
    ROOT,
    BackendUnavailable,
    discover_files,
    measure,
    summarize,
    worker_command,
)


def pytest_addoption(parser):
    group = parser.getgroup("ivt benchmarks")
    group.addoption("--ivt-data-dir", default=str(ROOT / "data"))
    group.addoption("--ivt-backends", default=",".join(BACKENDS))
    group.addoption("--ivt-rounds", type=int, default=3)
    group.addoption("--ivt-timeout", type=float, default=300)
    group.addoption("--ivt-sample-interval", type=float, default=0.001)
    group.addoption("--ivt-r-library", type=Path, default=None)
    group.addoption("--ivt-report", type=Path, default=None)


def pytest_configure(config):
    config._ivt_results = []
    backends = config.getoption("--ivt-backends").split(",")
    if not backends or any(backend not in BACKENDS for backend in backends):
        raise pytest.UsageError("--ivt-backends must be canivt,pyivt or either backend")
    for name in ("rounds", "timeout", "sample-interval"):
        if config.getoption(f"--ivt-{name}") <= 0:
            raise pytest.UsageError(f"--ivt-{name} must be positive")


def pytest_generate_tests(metafunc):
    if "benchmark_file" in metafunc.fixturenames:
        directory = Path(metafunc.config.getoption("--ivt-data-dir")).resolve()
        paths = discover_files(directory)
        metafunc.parametrize(
            "benchmark_file",
            paths,
            ids=[p.relative_to(directory).as_posix() for p in paths],
        )
    if "backend" in metafunc.fixturenames:
        metafunc.parametrize(
            "backend", metafunc.config.getoption("--ivt-backends").split(",")
        )


@pytest.fixture
def run_benchmark(request, benchmark_file, backend):
    pytest.importorskip(
        "psutil", reason="Install benchmark dependencies: uv sync --group benchmark"
    )
    config = request.config

    def run(operation):
        record = {
            "test": request.node.nodeid,
            "status": "running",
            "file": str(benchmark_file),
            "input_bytes": benchmark_file.stat().st_size,
            "backend": backend,
            "operation": operation,
            "samples": [],
        }
        config._ivt_results.append(record)
        try:
            command = worker_command(
                backend,
                operation,
                benchmark_file,
                r_library=config.getoption("--ivt-r-library"),
            )
            for _ in range(config.getoption("--ivt-rounds")):
                record["samples"].append(
                    measure(
                        command,
                        timeout=config.getoption("--ivt-timeout"),
                        sample_interval=config.getoption("--ivt-sample-interval"),
                    )
                )
        except BackendUnavailable as error:
            record.update(status="skipped", reason=str(error))
            pytest.skip(str(error))
        except Exception as error:
            record.update(status="failed", reason=str(error))
            raise
        record.update(status="passed", median=summarize(record["samples"]))
        return record

    return run


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item, call):
    outcome = yield
    report = outcome.get_result()
    if report.failed:
        for record in reversed(item.config._ivt_results):
            if record["test"] == item.nodeid:
                record.update(status="failed", reason=report.longreprtext)
                break


def pytest_sessionfinish(session):
    config = session.config
    path = config.getoption("--ivt-report")
    if path is not None:
        path.parent.mkdir(parents=True, exist_ok=True)
        report = {
            "schema_version": 1,
            "created_at": datetime.now(timezone.utc).isoformat(),
            "platform": platform.platform(),
            "rounds": config.getoption("--ivt-rounds"),
            "sample_interval_seconds": config.getoption("--ivt-sample-interval"),
            "results": config._ivt_results,
        }
        path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


def pytest_terminal_summary(terminalreporter):
    records = terminalreporter.config._ivt_results
    if not records:
        return
    terminalreporter.section("IVT benchmarks (medians; RSS includes runtime)")
    terminalreporter.write_line(
        "operation backend file                  wall ms    CPU ms   CPU %  peak MiB  growth MiB"
    )
    for record in records:
        if record["status"] != "passed":
            continue
        metrics = record["median"]
        cpu = metrics["cpu_percent"]
        terminalreporter.write_line(
            f"{record['operation']:9} {record['backend']:7} "
            f"{Path(record['file']).stem[:20]:20} "
            f"{metrics['wall_seconds'] * 1000:9.3f} "
            f"{metrics['cpu_seconds'] * 1000:9.3f} "
            f"{cpu if cpu is not None else 0:7.1f} "
            f"{metrics['sampled_peak_rss_bytes'] / 2**20:9.2f} "
            f"{metrics['sampled_rss_growth_bytes'] / 2**20:11.2f}"
        )
