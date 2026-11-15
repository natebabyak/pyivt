"""Record, per IVT or ZIP file, whether it opens, is corrupt, or is unsupported.

Rerun this over a share scan's failures to update their status. Each JSONL row
keeps the evidence needed to document a verdict: size, SHA-256 (equal hashes
identify copies of the same file), the leading header bytes, trailing zero
bytes, and pyivt's message. Opening reads metadata only. Pass ``--values`` to
also convert every cell in an isolated, memory-capped subprocess.

    python -m scripts.triage_files PATH [PATH ...] --output triage.jsonl
"""

import argparse
import hashlib
import json
import sys
from collections import Counter
from pathlib import Path

SUFFIXES = {".ivt", ".zip"}
TAIL = 1024 * 1024


def candidates(roots: list[Path]) -> list[Path]:
    found = []
    for root in roots:
        if root.is_dir():
            found.extend(
                sorted(
                    path
                    for path in root.rglob("*")
                    if path.is_file() and path.suffix.lower() in SUFFIXES
                )
            )
        else:
            found.append(root)
    return found


def evidence(path: Path, *, digest: bool) -> dict:
    size = path.stat().st_size
    sha = hashlib.sha256()
    with path.open("rb") as source:
        head = source.read(32)
        if digest:
            sha.update(head)
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                sha.update(chunk)
        source.seek(max(0, size - TAIL))
        tail = source.read()
    row = {
        "bytes": size,
        "head": head.hex(" "),
        "trailing_zero_bytes": len(tail) - len(tail.rstrip(b"\0")),
    }
    if digest:
        row["sha256"] = sha.hexdigest()
    return row


def classify(error: ValueError) -> str:
    import pyivt

    if isinstance(error, pyivt.CorruptFileError):
        return "corrupt"
    if isinstance(error, pyivt.UnsupportedFormatError):
        return "unsupported"
    return "failed"


def triage(path: Path, *, digest: bool = True) -> dict:
    import pyivt

    row = {"path": str(path)}
    try:
        row.update(evidence(path, digest=digest))
    except OSError as error:
        return row | {"status": "unreadable", "message": str(error)}
    try:
        ivt = pyivt.open(path)
        dimensions = ivt.metadata.dimensions
    except ValueError as error:
        return row | {"status": classify(error), "message": str(error)}
    except OSError as error:
        return row | {"status": "unreadable", "message": str(error)}
    return row | {
        "status": "opened",
        "version": ivt.metadata.signature,
        "dimensions": [[dim.name.en or dim.name.fr, dim.items] for dim in dimensions],
    }


def convert(row: dict, *, cap: int, timeout: float) -> dict:
    from scripts.probe_corpus import isolated

    result = isolated(Path(row["path"]), "stream", cap, timeout)
    conversion = {
        key: result[key]
        for key in ("status", "rows", "message", "exit_code", "peak_rss_bytes")
        if key in result
    }
    return row | {"conversion": conversion}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("paths", nargs="+", type=Path, help="files or directories")
    parser.add_argument("--output", type=Path, help="JSONL path (default: stdout)")
    parser.add_argument("--no-hash", action="store_true", help="skip SHA-256")
    parser.add_argument("--values", action="store_true", help="convert all cells")
    parser.add_argument("--rss-cap-bytes", type=int, default=8 * 1024**3)
    parser.add_argument("--timeout", type=float, default=600)
    args = parser.parse_args(argv)
    output = args.output.open("w", encoding="utf-8") if args.output else sys.stdout
    statuses = Counter()
    try:
        for path in candidates(args.paths):
            row = triage(path, digest=not args.no_hash)
            if args.values and row["status"] == "opened":
                row = convert(row, cap=args.rss_cap_bytes, timeout=args.timeout)
            statuses[row["status"]] += 1
            output.write(json.dumps(row, ensure_ascii=False) + "\n")
            output.flush()
    finally:
        if args.output:
            output.close()
    summary = ", ".join(f"{status}={n}" for status, n in sorted(statuses.items()))
    print(f"{sum(statuses.values())} files: {summary}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
