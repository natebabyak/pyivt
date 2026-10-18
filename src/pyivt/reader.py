"""High-level IVT table reader."""

from __future__ import annotations

from dataclasses import dataclass, field

import pyarrow as pa

from ._io import as_bytes
from .decode import Layout, build_layout, complete_grid, decode_store
from .descriptor import DimDesc, parse_descriptor
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
    layout: Layout
    _data: bytes = field(repr=False)
    _dims: list[DimDesc] = field(repr=False)
    _cells: dict | None = field(default=None, repr=False)
    _complete: bool | None = field(default=None, repr=False)

    def _ensure_cells(
        self,
        *,
        complete: bool = True,
        max_cells: int = 100_000_000,
    ) -> dict:
        if self._cells is not None and self._complete is complete:
            return self._cells
        n = 1
        for c in self.layout.counts:
            n *= c
        if complete and n > max_cells:
            raise ValueError(
                f"published grid has {n:,} cells; raise max_cells or use complete=False"
            )
        store = decode_store(self._data, self._dims, self.layout)
        self._cells = complete_grid(store, self.layout) if complete else store
        self._complete = complete
        return self._cells

    @property
    def cells(self) -> dict:
        """Decoded cell columns; triggers lazy decode on first access."""
        return self._ensure_cells()

    @property
    def complete(self) -> bool | None:
        """Whether the cached cells are the published grid, or None if undecoded."""
        return self._complete

    def _to_columns(
        self,
        *,
        complete: bool = True,
        max_cells: int = 100_000_000,
        labels: bool = True,
        language: str = "en",
    ) -> dict:
        cells = self._ensure_cells(complete=complete, max_cells=max_cells)
        return tidy_arrays(
            cells,
            self.metadata,
            self.layout,
            labels=labels,
            language=language,
        )

    def to_arrow(
        self,
        *,
        complete: bool = True,
        max_cells: int = 100_000_000,
        labels: bool = True,
        language: str = "en",
    ) -> pa.Table:
        return _columns_to_arrow(
            self._to_columns(
                complete=complete,
                max_cells=max_cells,
                labels=labels,
                language=language,
            )
        )


def from_bytes(data: bytes | bytearray | memoryview) -> IvtTable:
    """
    Open an IVT file from a bytes buffer without decoding cell values.

    Parses metadata and dimension layout only. Call ``to_arrow()`` (or access
    ``.cells``) to decode the value grid.
    """
    data = as_bytes(data)
    meta = read_metadata_bytes(data)
    desc = parse_descriptor(data)
    if desc is None:
        raise ValueError("could not parse IVT dimension descriptor")
    lay = build_layout(desc.dims)
    return IvtTable(metadata=meta, layout=lay, _data=data, _dims=desc.dims)
