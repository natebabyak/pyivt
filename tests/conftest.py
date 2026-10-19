"""Shared pytest fixtures for StatCan and Beyond demo IVT integration tests."""

from __future__ import annotations

import json
import os
from pathlib import Path
from types import SimpleNamespace

import pytest

_TESTS = Path(__file__).resolve().parent
STATCAN_DIR = _TESTS / "fixtures" / "statcan"
STATCAN_MANIFEST_PATH = STATCAN_DIR / "manifest.json"
DEMO_DIR = _TESTS / "fixtures" / "demo"
DEMO_MANIFEST_PATH = DEMO_DIR / "manifest.json"

_DEFAULT_BEYOND_DATA = Path(r"C:\Program Files\Beyond 2020\Professional Browser\Data")


def _load_manifest(path: Path) -> list[dict]:
    with path.open(encoding="utf-8") as f:
        return json.load(f)


STATCAN_MANIFEST = _load_manifest(STATCAN_MANIFEST_PATH)
DEMO_MANIFEST = _load_manifest(DEMO_MANIFEST_PATH)


def resolve_demo_ivt(filename: str) -> Path | None:
    """Return a readable path for a demo IVT, or None if unavailable."""
    local = DEMO_DIR / filename
    if local.is_file():
        return local
    env = os.environ.get("PYIVT_DEMO_DIR")
    candidates = []
    if env:
        candidates.append(Path(env) / filename)
    candidates.append(_DEFAULT_BEYOND_DATA / filename)
    for path in candidates:
        if path.is_file():
            return path
    return None


@pytest.fixture(params=STATCAN_MANIFEST, ids=[e["id"] for e in STATCAN_MANIFEST])
def statcan_ivt(request) -> SimpleNamespace:
    """One StatCan IVT fixture entry; skips if the local file has not been downloaded."""
    entry = request.param
    path = STATCAN_DIR / entry["filename"]
    if not path.is_file():
        pytest.skip(f"missing {path.name}; run: python tests/scripts/download.py")
    return SimpleNamespace(id=entry["id"], path=path, entry=entry)


@pytest.fixture(params=DEMO_MANIFEST, ids=[e["id"] for e in DEMO_MANIFEST])
def demo_ivt(request) -> SimpleNamespace:
    """One Beyond demo IVT; skips if not linked/copied and not in the install Data dir."""
    entry = request.param
    path = resolve_demo_ivt(entry["filename"])
    if path is None:
        pytest.skip(
            f"missing {entry['filename']}; run: python tests/scripts/link_demo_ivts.py "
            "or set PYIVT_DEMO_DIR"
        )
    return SimpleNamespace(id=entry["id"], path=path, entry=entry)
