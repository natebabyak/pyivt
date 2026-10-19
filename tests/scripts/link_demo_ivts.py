"""Copy Beyond sample IVTs into tests/fixtures/demo/ (gitignored *.ivt)."""

from __future__ import annotations

import argparse
import os
import shutil
import sys
from pathlib import Path

_TESTS = Path(__file__).resolve().parents[1]
DEMO_DIR = _TESTS / "fixtures" / "demo"
MANIFEST_PATH = DEMO_DIR / "manifest.json"

_DEFAULT_BEYOND_DATA = Path(r"C:\Program Files\Beyond 2020\Professional Browser\Data")


def demo_source_dir() -> Path:
    env = os.environ.get("PYIVT_DEMO_DIR")
    if env:
        return Path(env)
    return _DEFAULT_BEYOND_DATA


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--src",
        type=Path,
        default=None,
        help=f"Source directory (default: PYIVT_DEMO_DIR or {_DEFAULT_BEYOND_DATA})",
    )
    args = parser.parse_args(argv)
    src_dir = args.src or demo_source_dir()
    if not src_dir.is_dir():
        print(f"source dir missing: {src_dir}", file=sys.stderr)
        return 1

    import json

    with MANIFEST_PATH.open(encoding="utf-8") as f:
        manifest = json.load(f)

    DEMO_DIR.mkdir(parents=True, exist_ok=True)
    copied = 0
    for entry in manifest:
        name = entry["filename"]
        src = src_dir / name
        dst = DEMO_DIR / name
        if not src.is_file():
            print(f"skip {name}: not found in {src_dir}")
            continue
        shutil.copy2(src, dst)
        print(f"copied {name}")
        copied += 1
    print(f"done: {copied} copied")
    return 0 if copied else 1


if __name__ == "__main__":
    raise SystemExit(main())
