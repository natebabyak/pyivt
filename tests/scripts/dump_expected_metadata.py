"""Write golden metadata JSON for each downloaded StatCan IVT fixture."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

_TESTS = Path(__file__).resolve().parents[1]
_ROOT = _TESTS.parent
_SRC = _ROOT / "src"
for path in (_SRC, _TESTS):
    if path.is_dir() and str(path) not in sys.path:
        sys.path.insert(0, str(path))

from metadata_expected import EXPECTED_DIR, expected_path, snapshot_metadata

from pyivt import read_metadata

FIXTURES_DIR = _TESTS / "fixtures" / "statcan"
MANIFEST_PATH = FIXTURES_DIR / "manifest.json"


def load_manifest() -> list[dict]:
    with MANIFEST_PATH.open(encoding="utf-8") as f:
        return json.load(f)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)

    EXPECTED_DIR.mkdir(parents=True, exist_ok=True)
    wrote = 0
    skipped = 0
    for entry in load_manifest():
        product_id = entry["id"]
        ivt_path = FIXTURES_DIR / entry["filename"]
        if not ivt_path.is_file():
            print(f"skip {product_id}: missing {ivt_path.name}")
            skipped += 1
            continue
        meta = read_metadata(ivt_path)
        payload = snapshot_metadata(meta)
        out = expected_path(product_id)
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
