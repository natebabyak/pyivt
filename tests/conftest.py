"""Shared pytest fixtures for Beyond demo IVT integration tests."""

from __future__ import annotations

import json
import os
from pathlib import Path
from types import SimpleNamespace

import pytest

_TESTS = Path(__file__).resolve().parent
EXPECTED_DIR = _TESTS / "expected"
DATA_DIR = Path(os.environ.get("PYIVT_DATA_DIR", _TESTS.parent / "data"))


def pytest_sessionstart(session):
    if os.environ.get("PYIVT_REQUIRE_DATA") == "1":
        manifest = json.loads(
            (_TESTS / "corpus" / "training.json").read_text(encoding="utf-8")
        )
        missing = [
            entry["sha256"]
            for entry in manifest["files"]
            if not (DATA_DIR / f"{entry['sha256']}.ivt").is_file()
        ]
        if missing or not list(DATA_DIR.glob("*.ivt")):
            raise pytest.UsageError(
                f"PYIVT_REQUIRE_DATA=1: missing expected IVT data in {DATA_DIR}: {missing}"
            )


def _data_cases() -> list[SimpleNamespace]:
    cases: list[SimpleNamespace] = []
    for path in sorted(DATA_DIR.glob("*.ivt")):
        sha = path.stem
        if not (EXPECTED_DIR / f"{sha}.json").is_file():
            continue
        cases.append(
            SimpleNamespace(
                id=sha,
                path=path,
                expect_path=EXPECTED_DIR / f"{sha}.json",
            )
        )
    return cases


@pytest.fixture(params=_data_cases(), ids=lambda c: c.id)
def demo_ivt(request) -> SimpleNamespace:
    """One hashed demo IVT paired with its Beyond summary golden."""
    case = request.param
    assert case.expect_path.is_file(), (
        f"missing golden {case.expect_path.name}; "
        "run: python scripts/generate_expected_metadata.py"
    )
    return case
