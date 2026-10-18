"""Write golden metadata JSON for StatCan or Beyond demo IVT fixtures."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

_TESTS = Path(__file__).resolve().parents[1]
_ROOT = _TESTS.parent
if str(_TESTS) not in sys.path:
    sys.path.insert(0, str(_TESTS))

from conftest import DEMO_MANIFEST, STATCAN_DIR, STATCAN_MANIFEST, resolve_demo_ivt
from metadata_expected import (
    DEMO_EXPECTED_DIR,
    STATCAN_EXPECTED_DIR,
    expected_path,
    snapshot_metadata,
)
from pyivt import open as open_ivt


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--suite",
        choices=("statcan", "demo"),
        default="statcan",
        help="Which fixture suite to dump (default: statcan)",
    )
    args = parser.parse_args(argv)

    if args.suite == "demo":
        expected_dir = DEMO_EXPECTED_DIR
        manifest = DEMO_MANIFEST
    else:
        expected_dir = STATCAN_EXPECTED_DIR
        manifest = STATCAN_MANIFEST

    expected_dir.mkdir(parents=True, exist_ok=True)
    wrote = 0
    skipped = 0
    for entry in manifest:
        product_id = entry["id"]
        if args.suite == "demo":
            ivt_path = resolve_demo_ivt(entry["filename"])
            if ivt_path is None:
                print(f"skip {product_id}: missing {entry['filename']}")
                skipped += 1
                continue
        else:
            ivt_path = STATCAN_DIR / entry["filename"]
            if not ivt_path.is_file():
                print(f"skip {product_id}: missing {ivt_path.name}")
                skipped += 1
                continue
        meta = open_ivt(ivt_path).metadata
        payload = snapshot_metadata(meta)
        out = expected_path(product_id, suite=args.suite)
        out.write_text(
            json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(f"wrote {out.relative_to(_ROOT)}")
        wrote += 1
    print(f"done: {wrote} written, {skipped} skipped")
    return 0 if wrote else 1


if __name__ == "__main__":
    raise SystemExit(main())
