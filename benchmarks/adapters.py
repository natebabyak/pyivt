"""Operations timed by the Python worker (imports happen before timing)."""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

import pyivt

if TYPE_CHECKING:
    import pyarrow as pa


def extract_metadata_with_pyivt(path: Path) -> pyivt.IvtMetadata:
    metadata = pyivt.open(path).metadata
    # Returning .metadata alone only measures opening the summary.
    _ = metadata.dimensions
    _ = metadata.geographies
    _ = metadata.footnotes
    _ = metadata.missing_values
    return metadata


def convert_to_arrow_with_pyivt(path: Path) -> pa.Table:
    return pyivt.open(path).to_arrow()
