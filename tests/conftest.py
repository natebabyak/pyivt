"""Shared pytest fixtures for Beyond demo IVT integration tests."""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

import pytest

_TESTS = Path(__file__).resolve().parent
EXPECTED_DIR = _TESTS / "expected"
DATA_DIR = _TESTS.parent / "data"


def _data_cases() -> list[SimpleNamespace]:
    cases: list[SimpleNamespace] = []
    for path in sorted(DATA_DIR.glob("*.ivt")):
        sha = path.stem
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
