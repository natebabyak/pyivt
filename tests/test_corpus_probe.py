"""The corpus gate validates input identity and the subprocess's real result."""

import hashlib
import json
from pathlib import Path

import pytest

from scripts.ivt_synthetic import cube
from scripts.probe_corpus import probe


def test_training_and_heldout_hashes_are_disjoint():
    root = Path(__file__).parent / "corpus"
    sets = [
        {entry["sha256"] for entry in json.loads((root / name).read_text())["files"]}
        for name in ["training.json", "heldout.json"]
    ]
    assert all(sets)
    assert sets[0].isdisjoint(sets[1])


def test_probe_converts_both_paths_and_checks_official_counts(tmp_path):
    content = cube((2, 3, 4))
    sha = hashlib.sha256(content).hexdigest()
    (tmp_path / f"{sha}.ivt").write_bytes(content)
    results = probe(
        {"files": [{"sha256": sha, "official_dimension_counts": [2, 3, 4]}]},
        tmp_path,
        memory=1024**3,
        cap=2 * 1024**3,
    )
    assert [row["mode"] for row in results] == ["stream", "table"]
    assert all(row["status"] == "converted" and row["rows"] == 24 for row in results)
    wrong = probe(
        {"files": [{"sha256": sha, "official_dimension_counts": [2, 4, 3]}]},
        tmp_path,
        memory=1024**3,
        cap=2 * 1024**3,
    )
    assert all(row["status"] == "dimension_mismatch" for row in wrong)


def test_probe_fails_before_launch_when_hash_is_wrong(tmp_path):
    sha = "a" * 64
    (tmp_path / f"{sha}.ivt").write_bytes(b"wrong file")
    with pytest.raises(ValueError, match="pinned hash differs"):
        probe({"files": [{"sha256": sha}]}, tmp_path, memory=1024**3, cap=2 * 1024**3)
