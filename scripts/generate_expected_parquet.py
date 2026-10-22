from multiprocessing import get_context
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"
EXPECTED_DIR = Path(__file__).resolve().parent.parent / "tests" / "expected"


def _ivt_write_parquet(ivt_path: str, parquet_path: str) -> None:
    from rpy2.robjects.packages import importr

    canivt = importr("canivt")
    tab = canivt.read_ivt(ivt_path)
    canivt.ivt_write_parquet(tab, parquet_path)


def main() -> int:
    for ivt_path in DATA_DIR.glob("*.ivt"):
        parquet_path = EXPECTED_DIR / ivt_path.with_suffix(".parquet").name

        process = get_context("spawn").Process(
            target=_ivt_write_parquet,
            args=(
                str(ivt_path),
                str(parquet_path),
            ),
        )

        process.start()
        process.join()

        if process.exitcode != 0:
            raise RuntimeError(f"IVT parsing failed: {ivt_path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
