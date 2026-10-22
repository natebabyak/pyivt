import argparse
import hashlib
import os
import shutil
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"
DEFAULT_DIRECTORIES = [
    Path(r"C:\Program Files\Beyond 2020\Professional Browser\Data"),
    Path(r"C:\Program Files (x86)\Beyond 2020\Professional Browser\Data"),
]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "directories",
        nargs="*",
        type=Path,
        default=DEFAULT_DIRECTORIES,
    )
    args = parser.parse_args(argv)

    DATA_DIR.mkdir(parents=True, exist_ok=True)

    for directory in args.directories:
        for root, _, files in os.walk(directory):
            for file in files:
                if not file.lower().endswith(".ivt"):
                    continue

                source = Path(root) / file

                digest = hashlib.file_digest(
                    source.open("rb"),
                    "sha256",
                ).hexdigest()

                destination = DATA_DIR / f"{digest}.ivt"

                if destination.exists():
                    continue

                shutil.copyfile(source, destination)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
