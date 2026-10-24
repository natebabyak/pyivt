"""Clear local test inputs and expected outputs, keeping their directories."""

import argparse
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIRECTORIES = (ROOT / "data", ROOT / "tests" / "expected")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)

    # Validate both targets before deleting anything. Do not follow redirected
    # directories outside the repository (including Windows junctions).
    for directory in DIRECTORIES:
        if directory.is_symlink() or directory.resolve() != directory:
            raise ValueError(f"Refusing to clear redirected directory: {directory}")
        if directory.exists() and not directory.is_dir():
            raise NotADirectoryError(directory)

    for directory in DIRECTORIES:
        if not directory.exists():
            continue
        for entry in directory.iterdir():
            if entry.is_symlink() or not entry.is_dir():
                entry.unlink()
            else:
                shutil.rmtree(entry)
        print(f"Cleared {directory.relative_to(ROOT)}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
