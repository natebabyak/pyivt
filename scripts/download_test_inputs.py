"""Download Statistics Canada Census IVT files into data/."""

import argparse
import hashlib
import json
from io import BytesIO
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from zipfile import ZipFile, is_zipfile

DATA_DIR = Path(__file__).resolve().parent.parent / "data"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--number", "-n", type=int, default=10)
    parser.add_argument(
        "--clear",
        "-c",
        action="store_true",
        help="Delete existing files in data/ before downloading (keep subdirectories).",
    )
    args = parser.parse_args()
    if args.number < 1:
        parser.error("--number must be positive")

    DATA_DIR.mkdir(exist_ok=True)
    if args.clear:
        for path in DATA_DIR.iterdir():
            if path.is_file() or path.is_symlink():
                path.unlink()

    headers = {"User-Agent": "Mozilla/5.0"}
    index_url = "https://www150.statcan.gc.ca/t1/wds/rest/getAllCubesListLite"
    with urlopen(Request(index_url, headers=headers), timeout=60) as response:
        tables = json.load(response)

    downloaded = set()
    for table in tables:
        product_id = str(table["productId"])
        if not product_id.startswith("98"):
            continue
        url = f"https://www150.statcan.gc.ca/n1/en/tbl/b2020/{product_id}.zip"
        try:
            with urlopen(Request(url, headers=headers), timeout=60) as response:
                payload = BytesIO(response.read())
        except HTTPError as error:
            if error.code == 404:
                continue
            raise
        if not is_zipfile(payload):
            continue
        with ZipFile(payload) as archive:
            for name in archive.namelist():
                if not name.lower().endswith(".ivt"):
                    continue
                content = archive.read(name)
                digest = hashlib.sha256(content).hexdigest()
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
