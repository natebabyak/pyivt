"""Capture or verify Arrow output fingerprints for a pinned IVT corpus.

Run --capture on the reference checkout, then run without --capture on the
changed checkout, using the same PyArrow version. Schema metadata is omitted
because it contains filesystem identity. These hashes establish unchanged
output; they are not independent evidence that the original values are right.
"""

import argparse
import hashlib
import json
from pathlib import Path

import pyarrow as pa
import pyivt


def fingerprint(path: Path) -> dict:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    if digest.hexdigest() != path.stem:
        raise ValueError(f"input SHA-256 does not match pinned filename: {path}")
    table = pyivt.open(path).to_arrow().replace_schema_metadata(None)
    sink = pa.BufferOutputStream()
    with pa.ipc.new_stream(sink, table.schema) as writer:
        writer.write_table(table)
    return {
        "file": path.name,
        "rows": table.num_rows,
        "hash": hashlib.sha256(sink.getvalue()).hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--data-dir", type=Path, default=Path("data"))
    parser.add_argument(
        "--manifest", type=Path, default=Path("tests/corpus/training.json")
    )
    parser.add_argument("--capture", action="store_true")
    args = parser.parse_args()
    if args.capture:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        reference = [{"file": f"{entry['sha256']}.ivt"} for entry in manifest["files"]]
    else:
        reference = json.loads(args.baseline.read_text(encoding="utf-8"))
    if not reference:
        raise SystemExit("baseline corpus is empty")
    actual = []
    failed = False
    for expected in reference:
        result = fingerprint(args.data_dir / expected["file"])
        actual.append(result)
        matches = args.capture or result == expected
        failed |= not matches
        print(
            json.dumps({**result, "status": "ok" if matches else "changed"}), flush=True
        )
    if args.capture:
        args.baseline.parent.mkdir(parents=True, exist_ok=True)
        args.baseline.write_text(json.dumps(actual, indent=2) + "\n", encoding="utf-8")
    if failed:
        raise SystemExit("Arrow output differs from baseline")


if __name__ == "__main__":
    main()
