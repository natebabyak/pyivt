"""Download Statistics Canada Census IVT files into data/."""

import argparse
import hashlib
import json
import sys
import time
from io import BytesIO
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
from zipfile import ZipFile, is_zipfile

DATA_DIR = Path(__file__).resolve().parent.parent / "data"


def download(url: str, *, attempts: int = 3) -> bytes:
    """Retry transient HTTP and transport failures, including response read timeouts."""
    for attempt in range(1, attempts + 1):
        try:
            request = Request(url, headers={"User-Agent": "Mozilla/5.0"})
            with urlopen(request, timeout=60) as response:
                return response.read()
        except (HTTPError, URLError, TimeoutError, ConnectionError) as error:
            if isinstance(error, HTTPError) and error.code not in {
                408,
                429,
                500,
                502,
                503,
                504,
            }:
                raise
            if attempt == attempts:
                raise
            delay = 2 ** (attempt - 1)
            print(
                f"Download failed ({error}); retrying {url} "
                f"in {delay}s (attempt {attempt + 1}/{attempts})",
                file=sys.stderr,
                flush=True,
            )
            time.sleep(delay)
    raise ValueError("attempts must be positive")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--number", "-n", type=int, default=10)
    parser.add_argument(
        "--expected-dir",
        type=Path,
        help="Only download inputs whose SHA-256 has a JSON reference in this directory.",
    )
    parser.add_argument(
        "--clear",
        "-c",
        action="store_true",
        help="Delete existing files in data/ before downloading (keep subdirectories).",
    )
    args = parser.parse_args()
    if args.number < 1:
        parser.error("--number must be positive")
    expected_hashes = None
    if args.expected_dir is not None:
        expected_hashes = {path.stem for path in args.expected_dir.glob("*.json")}
        if len(expected_hashes) < args.number:
            parser.error("--expected-dir contains fewer JSON references than --number")

    DATA_DIR.mkdir(exist_ok=True)
    if args.clear:
        for path in DATA_DIR.iterdir():
            if path.is_file() or path.is_symlink():
                path.unlink()

    index_url = "https://www150.statcan.gc.ca/t1/wds/rest/getAllCubesListLite"
    tables = json.loads(download(index_url))

    downloaded = set()
    for table in tables:
        product_id = str(table["productId"])
        if not product_id.startswith("98"):
            continue
        url = f"https://www150.statcan.gc.ca/n1/en/tbl/b2020/{product_id}.zip"
        try:
            payload = BytesIO(download(url))
        except HTTPError as error:
            if error.code == 404:
                continue
            if error.code not in {408, 429, 500, 502, 503, 504}:
                raise
            print(f"Skipping {url} after retries: {error}", file=sys.stderr)
            continue
        except (URLError, TimeoutError, ConnectionError) as error:
            print(f"Skipping {url} after retries: {error}", file=sys.stderr)
            continue
        if not is_zipfile(payload):
            continue
        with ZipFile(payload) as archive:
            for name in archive.namelist():
                if not name.lower().endswith(".ivt"):
                    continue
                content = archive.read(name)
                digest = hashlib.sha256(content).hexdigest()
                if expected_hashes is not None and digest not in expected_hashes:
                    continue
                if digest in downloaded:
                    continue
                destination = DATA_DIR / f"{digest}.ivt"
                if not destination.exists():
                    destination.write_bytes(content)
                downloaded.add(digest)
                print(f"{len(downloaded)}/{args.number}: {destination.name}")
                if len(downloaded) == args.number:
                    break
        if len(downloaded) == args.number:
            break

    if len(downloaded) < args.number:
        parser.exit(1, f"Only {len(downloaded)} distinct IVT files were available.\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
