"""Measure native workloads without including process startup in timings."""

from __future__ import annotations

import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from contextlib import suppress
from pathlib import Path
from statistics import median

ROOT = Path(__file__).resolve().parent.parent
BACKENDS = ("canivt", "pyivt")
OPERATIONS = ("metadata", "arrow")


class BackendUnavailable(RuntimeError):
    """An optional runtime or R package is missing."""


def discover_files(directory: Path) -> list[Path]:
    """Discover recursively, including upper-case extensions, in stable order."""
    return sorted(
        (p for p in directory.rglob("*") if p.is_file() and p.suffix.lower() == ".ivt"),
        key=lambda p: p.relative_to(directory).as_posix(),
    )


def find_rscript() -> str:
    if executable := shutil.which("Rscript"):
        return executable
    home = os.environ.get("R_HOME")
    if not home and os.name == "nt":
        import winreg

        for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
            try:
                with winreg.OpenKey(hive, r"Software\R-core\R") as key:
                    home = winreg.QueryValueEx(key, "InstallPath")[0]
                break
            except OSError:
                continue
    if home:
        path = Path(home) / "bin" / ("Rscript.exe" if os.name == "nt" else "Rscript")
        if path.is_file():
            return str(path)
    raise BackendUnavailable("Install R and put Rscript on PATH, or set R_HOME.")


def worker_command(
    backend: str,
    operation: str,
    path: Path,
    *,
    rscript: str | None = None,
    r_library: Path | None = None,
) -> list[str]:
    if backend not in BACKENDS or operation not in OPERATIONS:
        raise ValueError(f"Unknown benchmark: {backend}/{operation}")
    if backend == "pyivt":
        return [sys.executable, "-m", "benchmarks.worker", operation, str(path)]
    return [
        rscript or find_rscript(),
        "--vanilla",
        str(ROOT / "benchmarks" / "worker.R"),
        operation,
        str(path),
        str((r_library or ROOT / ".cache" / "generate_expected_parquet").resolve()),
    ]


def measure(
    command: list[str], *, timeout: float = 300, sample_interval: float = 0.001
) -> dict:
    """One fresh worker; imports precede READY, output stays alive through ACK.

    RSS is sampled externally, so native Rust/R/Arrow allocations are included.
    The sampled peak is a lower bound, not an allocation profiler's exact peak.
    """
    import psutil

    messages: queue.Queue[str | None] = queue.Queue()
    environment = os.environ.copy()
    environment["PYTHONPATH"] = os.pathsep.join(
        [str(ROOT / "python"), str(ROOT), environment.get("PYTHONPATH", "")]
    )
    with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as errors:
        process = subprocess.Popen(
            command,
            cwd=ROOT,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=errors,
            text=True,
            encoding="utf-8",
            errors="replace",
        )

        def read_stdout() -> None:
            for line in process.stdout:
                messages.put(line)
            messages.put(None)

        reader = threading.Thread(target=read_stdout, daemon=True)
        reader.start()
        deadline = time.monotonic() + timeout

        def receive(poll=None) -> dict:
            while time.monotonic() < deadline:
                if poll is not None:
                    poll()
                try:
                    line = messages.get(timeout=sample_interval)
                except queue.Empty:
                    continue
                if line is None:
                    errors.seek(0)
                    raise RuntimeError(f"Benchmark worker exited:\n{errors.read()}")
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(message, dict):
                    if "unavailable" in message:
                        raise BackendUnavailable(message["unavailable"])
                    return message
            raise TimeoutError(f"Benchmark worker exceeded {timeout:g}s")

        try:
            ready = receive()
            if ready.get("ready") is not True:
                raise RuntimeError(f"Unexpected worker readiness: {ready}")
            # Windows venv Python and Rscript can be launchers. The worker
            # reports the PID that actually owns Rust/R/Arrow allocations.
            monitored = psutil.Process(ready["pid"])
            baseline = monitored.memory_info().rss
            peak = baseline
            samples = 0

            def sample() -> None:
                nonlocal peak, samples
                # If decoding failed, let receive() collect stderr and report
                # the actual exception instead of a secondary missing PID.
                with suppress(psutil.NoSuchProcess):
                    peak = max(peak, monitored.memory_info().rss)
                    samples += 1

            process.stdin.write("GO\n")
            process.stdin.flush()
            result = receive(sample)
            sample()
            for key in ("wall_seconds", "cpu_seconds"):
                if not isinstance(result.get(key), (float, int)) or result[key] < 0:
                    raise RuntimeError(f"Invalid worker measurement: {result}")
            process.stdin.write("ACK\n")
            process.stdin.flush()
            process.wait(timeout=max(0.01, deadline - time.monotonic()))
            if process.returncode:
                errors.seek(0)
                raise RuntimeError(f"Benchmark worker failed:\n{errors.read()}")
            return {
                **{
                    key: value
                    for key, value in ready.items()
                    if key not in ("ready", "pid")
                },
                **result,
                "cpu_percent": (
                    100 * result["cpu_seconds"] / result["wall_seconds"]
                    if result["wall_seconds"] > 0
                    else None
                ),
                "baseline_rss_bytes": baseline,
                "sampled_peak_rss_bytes": peak,
                "sampled_rss_growth_bytes": peak - baseline,
                "rss_samples": samples,
            }
        finally:
            if process.poll() is None:
                # Killing just Rscript or a venv launcher can orphan the worker.
                with suppress(psutil.NoSuchProcess):
                    for child in psutil.Process(process.pid).children(recursive=True):
                        with suppress(psutil.NoSuchProcess):
                            child.kill()
                process.kill()
            process.wait()
            reader.join(timeout=1)
            process.stdin.close()
            process.stdout.close()


def summarize(samples: list[dict]) -> dict:
    keys = (
        "wall_seconds",
        "cpu_seconds",
        "cpu_percent",
        "baseline_rss_bytes",
        "sampled_peak_rss_bytes",
        "sampled_rss_growth_bytes",
    )
    summary = {}
    for key in keys:
        values = [sample[key] for sample in samples if sample[key] is not None]
        summary[key] = median(values) if values else None
    return summary
