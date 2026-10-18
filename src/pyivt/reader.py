"""High-level IVT table reader."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime

import pyarrow as pa

from ._io import as_bytes
from .decode import Layout, build_layout, complete_grid, decode_store
from .descriptor import parse_descriptor
from .metadata import IvtMetadata, read_metadata_bytes
from .tidy import tidy_arrays


def _columns_to_arrow(cols: dict) -> pa.Table:
    arrays = {}
    for k, v in cols.items():
        arrays[k] = pa.array(v)
    return pa.table(arrays)


@dataclass
class IvtTable:
    metadata: IvtMetadata
    cells: dict
    layout: Layout
    complete: bool = True

    def _to_columns(
        self,
        *,
        labels: bool = True,
        language: str = "en",
    ) -> dict:
        return tidy_arrays(
            self.cells,
            self.metadata,
            self.layout,
            labels=labels,
            language=language,
        )

    def to_arrow(
        self,
        *,
        labels: bool = True,
        language: str = "en",
    ) -> pa.Table:
        return _columns_to_arrow(self._to_columns(labels=labels, language=language))


def open(
    data: bytes | bytearray | memoryview,
    *,
    complete: bool = True,
    language: str = "en",
    max_cells: int = 100_000_000,
    file_name: str | None = None,
    directory: str | None = None,
    last_update: datetime | None = None,
) -> IvtTable:
    """
    Open an IVT file from a bytes buffer.

    Parameters
    ----------
    complete
        If True (default), expand to the published grid (absent cells → 0).
        If False, return only stored non-zero cells.
    file_name, directory, last_update
        Optional Beyond 20/20 summary fields; omit when not known.
    """
    data = as_bytes(data)
    meta = read_metadata_bytes(
        data,
        file_name=file_name,
        directory=directory,
        last_update=last_update,
        file_size=len(data),
    )
    desc = parse_descriptor(data)
    if desc is None:
        raise ValueError("could not parse IVT dimension descriptor")
    lay = build_layout(desc.dims)
    n = 1
    for c in lay.counts:
        n *= c
    if complete and n > max_cells:
        raise ValueError(
            f"published grid has {n:,} cells; raise max_cells or use complete=False"
        )
    store = decode_store(data, desc.dims, lay)
    cells = complete_grid(store, lay) if complete else store
    # language is applied at tidy/export time; keep raw ids here
    _ = language
    return IvtTable(metadata=meta, cells=cells, layout=lay, complete=complete)
