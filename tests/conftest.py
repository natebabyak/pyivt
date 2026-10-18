"""Shared pytest fixtures for StatCan IVT integration tests."""

from __future__ import annotations

import json
from pathlib import Path
from types import SimpleNamespace

import pytest

FIXTURES_DIR = Path(__file__).resolve().parent / "fixtures" / "statcan"
MANIFEST_PATH = FIXTURES_DIR / "manifest.json"


def _load_manifest() -> list[dict]:
    with MANIFEST_PATH.open(encoding="utf-8") as f:
        return json.load(f)


MANIFEST = _load_manifest()


@pytest.fixture(params=MANIFEST, ids=[e["id"] for e in MANIFEST])
def statcan_ivt(request) -> SimpleNamespace:
    """One StatCan IVT fixture entry; skips if the local file has not been downloaded."""
    entry = request.param
    path = FIXTURES_DIR / entry["filename"]
    if not path.is_file():
        pytest.skip(f"missing {path.name}; run: python tests/scripts/download.py")
    return SimpleNamespace(id=entry["id"], path=path, entry=entry)
