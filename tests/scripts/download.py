#!/usr/bin/env python3
"""Download StatCan IVT fixtures listed in tests/fixtures/statcan/manifest.json."""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

FIXTURES_DIR = Path(__file__).resolve().parents[1] / "fixtures" / "statcan"
MANIFEST_PATH = FIXTURES_DIR / "manifest.json"
ZIP_MAGIC = b"PK\x03\x04"
USER_AGENT = "pyivt-tests/0.1 (+https://github.com/)"


def load_manifest() -> list[dict]:
    with MANIFEST_PATH.open(encoding="utf-8") as f:
        entries = json.load(f)
    if not isinstance(entries, list) or not entries:
        raise SystemExit(f"manifest must be a non-empty list: {MANIFEST_PATH}")
    return entries


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=120) as resp:
        return resp.read()


def extract_ivt_from_zip(payload: bytes, dest: Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        zip_path = tmp_path / "download.zip"
        zip_path.write_bytes(payload)
        with zipfile.ZipFile(zip_path) as zf:
            names = [n for n in zf.namelist() if n.lower().endswith(".ivt")]
            if not names:
                raise RuntimeError("archive contains no .ivt file")
            extracted = Path(zf.extract(names[0], path=tmp_path))
            dest.write_bytes(extracted.read_bytes())


def write_ivt(payload: bytes, dest: Path) -> None:
    if payload.startswith(ZIP_MAGIC):
        extract_ivt_from_zip(payload, dest)
    else:
        dest.write_bytes(payload)


def download_entry(entry: dict, *, force: bool) -> None:
    entry_id = entry["id"]
    url = entry["url"]
    dest = FIXTURES_DIR / entry["filename"]
    if dest.is_file() and not force:
        print(f"skip {entry_id}: already exists ({dest.name})")
        return
    print(f"download {entry_id}: {url}")
    try:
        payload = fetch(url)
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"{entry_id}: HTTP {exc.code} for {url}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"{entry_id}: failed to fetch {url}: {exc.reason}") from exc
    if not payload:
        raise RuntimeError(f"{entry_id}: empty response from {url}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    write_ivt(payload, dest)
    print(f"  wrote {dest} ({dest.stat().st_size:,} bytes)")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--force",
        action="store_true",
        help="re-download even when the .ivt already exists",
    )
    args = parser.parse_args(argv)

    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)
    errors: list[str] = []
    for entry in load_manifest():
        try:
            download_entry(entry, force=args.force)
        except Exception as exc:  # noqa: BLE001 - collect all failures for CI
            print(f"error {entry.get('id', '?')}: {exc}", file=sys.stderr)
            errors.append(str(exc))
    if errors:
        print(f"{len(errors)} download(s) failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
