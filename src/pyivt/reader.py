"""High-level IVT table reader."""

from __future__ import annotations

from dataclasses import dataclass, field

import pyarrow as pa

from ._io import Source, read_source
from .decode import Layout, build_layout, complete_grid, decode_store
from .descriptor import DimDesc, parse_descriptor
from .header import read_header
from .metadata import IvtMetadata, build_metadata
from .tidy import tidy_arrays


def _columns_to_arrow(cols: dict) -> pa.Table:
    arrays = {}
    for k, v in cols.items():
        arrays[k] = pa.array(v)
    return pa.table(arrays)


@dataclass
class Ivt:
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


def open(source: Source) -> Ivt:
    """
    Open an IVT file from a path, bytes-like buffer, or binary file object.

    Parses summary metadata and dimension layout only (descriptor once).
    Access ``.metadata`` for typed metadata (heavy fields load on first use).
    Call ``to_arrow()`` (or access ``.cells``) to decode the value grid.
    """
    data = read_source(source)
    hdr = read_header(data)
    desc = parse_descriptor(data)
    if desc is None:
        raise ValueError("could not parse IVT dimension descriptor")
    meta = build_metadata(data, desc=desc, hdr=hdr)
    lay = build_layout(desc.dims)
    return Ivt(metadata=meta, layout=lay, _data=data, _dims=desc.dims)
