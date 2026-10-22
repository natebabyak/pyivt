"""Shared pytest fixtures for Beyond demo IVT integration tests."""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

import pytest

_TESTS = Path(__file__).resolve().parent
EXPECTED_DIR = _TESTS / "expected"
DATA_DIR = _TESTS.parent / "data"


def _expected_cases() -> list[SimpleNamespace]:
    cases: list[SimpleNamespace] = []
    for expect_path in sorted(EXPECTED_DIR.glob("*.json")):
        sha = expect_path.stem
        cases.append(
            SimpleNamespace(
                id=sha,
                path=DATA_DIR / f"{sha}.ivt",
                expect_path=expect_path,
            )
        )
    return cases


@pytest.fixture(params=_expected_cases(), ids=lambda c: c.id)
def demo_ivt(request) -> SimpleNamespace:
    """One hashed demo IVT paired with its Beyond summary golden."""
    case = request.param
    if not case.path.is_file():
        pytest.skip(f"missing {case.path.name}; run: python scripts/copy_test_inputs.py")
    if not case.expect_path.is_file():
        pytest.skip(
            f"missing golden {case.expect_path.name}; "
            "run: python scripts/generate_expected_test_outputs.py"
        )
    return case
