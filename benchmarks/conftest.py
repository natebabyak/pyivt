from pathlib import Path

import pytest

PROJECT_DIR = Path(__file__).resolve().parent.parent
DATA_DIR = PROJECT_DIR / "data"


def discover_benchmark_files() -> list[Path]:
    if not DATA_DIR.exists():
        return []

    return list(DATA_DIR.glob("*.ivt"))


BENCHMARK_FILES = discover_benchmark_files()


@pytest.fixture(
    params=BENCHMARK_FILES,
    ids=lambda path: path.stem,
)
def benchmark_file(request: pytest.FixtureRequest) -> Path:
    return request.param
