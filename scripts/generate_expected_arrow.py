import os
from pathlib import Path
from time import sleep
from typing import Self

import polars as pl
import pyperclip
from pywinauto import Application, ElementNotFoundError
from pywinauto.keyboard import send_keys

BEYOND_PATH = Path(r"C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe")
DATA_DIR = Path(__file__).resolve().parent.parent / "data"
EXPECTED_DIR = Path(__file__).resolve().parent.parent / "tests" / "expected"
TITLE_RE = r"^Beyond 20/20 Professional Browser.*$"


class Beyond:
    def __init__(
        self,
        executable: Path = BEYOND_PATH,
    ) -> None:
        self.executable = executable
        self.app: Application | None = None

    def __enter__(self) -> Self:
        try:
            Application("uia").connect(
                title_re=TITLE_RE,
            ).kill()
        except ElementNotFoundError:
            pass

        self.app = Application("uia").start(
            f'"{self.executable}"',
        )
        sleep(0.1)
        send_keys("{ESC}")
        self.app.top_window().wait("ready")
        return self

    def __exit__(self, *_: object) -> None:
        if self.app is not None:
            self.app.kill()
            self.app = None

    def to_parquet(
        self,
        ivt_path: Path,
        parquet_path: Path,
    ) -> None:
        csv_name = ivt_path.name.replace(".ivt", ".csv")
        csv_path = DATA_DIR / csv_name

        if csv_path.exists():
            return

        os.startfile(ivt_path)

        self.app.top_window().wait("ready").type_keys("%fa")

        pyperclip.copy(str(csv_path))

        send_keys("^v{TAB}{RIGHT}{DOWN}{DOWN}{ENTER}{ENTER}")

        pl.read_csv(csv_path).write_parquet(parquet_path)


def main() -> int:
    paths = DATA_DIR.glob("*.ivt")

    with Beyond() as beyond:
        for ivt_path in paths:
            parquet_name = ivt_path.name.replace(".ivt", ".parquet")
            parquet_path = EXPECTED_DIR / parquet_name

            if parquet_path.exists():
                continue

            beyond.to_parquet(ivt_path, parquet_path)


if __name__ == "__main__":
    raise SystemExit(main())
