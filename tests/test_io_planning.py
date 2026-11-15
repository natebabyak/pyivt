"""Path opens plan and coalesce metadata reads (NAT-253).

Each path open reads metadata in a few coalesced ranges instead of one
positional read per record, and must produce the same metadata as an
in-memory open of the same bytes, which reads nothing through the planner.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import pyivt
import pytest

_FIXTURES = Path(__file__).parent / "fixtures"
_HELDOUT = Path(os.environ.get("PYIVT_HELDOUT_DIR", ".cache/heldout"))

# Measured on Windows 11, release build: 98100532 issued 206 read ops before
# planning and 4 after; 98100291 issued 206 and 6.
_MAX_OPEN_READ_OPS = 20


def _snapshot(value, depth=0):
    """Every public, non-callable attribute, recursively, for exact comparison."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, (list, tuple)):
        return [_snapshot(item, depth + 1) for item in value]
    if isinstance(value, dict):
        return {str(k): _snapshot(v, depth + 1) for k, v in value.items()}
    assert depth < 10, f"unexpected nesting at {type(value).__name__}"
    out = {"type": type(value).__name__}
    for name in dir(value):
        if name.startswith("_"):
            continue
        attribute = getattr(value, name)
        if not callable(attribute):
            out[name] = _snapshot(attribute, depth + 1)
    return out


def _assert_planned_open(path: Path) -> tuple[int, int]:
    ivt = pyivt.open(path)
    _, read_ops, _, _ = ivt.metadata._io_metrics()
    assert read_ops <= _MAX_OPEN_READ_OPS
    snapshot = _snapshot(ivt.metadata)
    # Lazy members, geographies and legends come from the catalog read at open.
    assert ivt.metadata._io_metrics()[1] == read_ops
    assert snapshot == _snapshot(pyivt.open(path.read_bytes()).metadata)
    assert ivt._member_fields()[0] == pyivt.open(path.read_bytes())._member_fields()[0]
    return read_ops, ivt.metadata._io_metrics()[1] - read_ops


@pytest.mark.parametrize(
    "name", ["heldout/98100036.ivt", "heldout/98100042.ivt", "98100018-paged.ivt"]
)
def test_fixture_path_open_coalesces_metadata_reads(name):
    read_ops, identity_ops = _assert_planned_open(_FIXTURES / name)
    # Header, directories and records each need one coalesced read here.
    assert read_ops <= 6
    # pyivt.table_id reads the header and the value page directory.
    assert identity_ops <= 4


@pytest.mark.parametrize("product_id", ["98100532", "98100291"])
def test_heldout_path_open_issues_few_read_ops(product_id):
    manifest = json.loads(
        (Path(__file__).parent / "corpus" / "heldout.json").read_text(encoding="utf-8")
    )
    sha = next(
        f["sha256"] for f in manifest["files"] if f.get("product_id") == product_id
    )
    path = _HELDOUT / f"{sha}.ivt"
    if not path.is_file():
        pytest.skip(f"held-out input {path} is not downloaded")
    _assert_planned_open(path)


def _export(path: Path):
    ivt = pyivt.open(path)
    return ivt.to_arrow(), ivt.to_arrow(**{ivt._layout.slugs[0]: [1]})


@pytest.mark.parametrize("name", ["heldout/98100036.ivt", "98100018-paged.ivt"])
def test_mapped_and_read_exports_match(name, monkeypatch):
    path = _FIXTURES / name
    monkeypatch.delenv("PYIVT_MMAP", raising=False)
    mapped = _export(path)
    monkeypatch.setenv("PYIVT_MMAP", "0")
    read = _export(path)
    for left, right in zip(mapped, read, strict=True):
        assert left.equals(right)
        assert left.schema.metadata == right.schema.metadata


def test_file_truncated_after_open_is_read_not_mapped(tmp_path):
    source = (_FIXTURES / "heldout" / "98100036.ivt").read_bytes()
    path = tmp_path / "table.ivt"
    path.write_bytes(source)
    ivt = pyivt.open(path)
    # Truncating before the first export must not map a shorter file; the full
    # read reports the damage as an IvtError instead of crashing the process.
    with path.open("r+b") as stream:
        stream.truncate(len(source) // 2)
    with pytest.raises(pyivt.IvtError):
        ivt.to_arrow()
