from pathlib import Path
from typing import Any

import pyarrow.parquet as pq
import pyivt
from rpy2.robjects.packages import importr

canivt = importr("canivt")


def extract_metadata_with_canivt(path: Path) -> Any:
    return canivt.ivt_metadata(str(path))


def extract_metadata_with_pyivt(path: Path) -> Any:
    ivt = pyivt.open(path)
    return ivt.metadata


def convert_to_arrow_with_canivt(path: Path) -> None:
    return


def export_to_parquet_with_pyivt(path: Path) -> None:
    ivt = pyivt.open(path)
    tab = ivt.to_arrow()
    pq.write_table(tab, "data.parquet")
