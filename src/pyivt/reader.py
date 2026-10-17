"""High-level IVT table reader."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from .decode import Layout, build_layout, complete_grid, decode_store
from .descriptor import parse_descriptor
from .metadata import IvtMetadata, read_metadata
from .tidy import tidy_arrays

PathLike = str | Path


@dataclass
class IvtTable:
    metadata: IvtMetadata
    cells: dict
    layout: Layout
    complete: bool = True

    def to_columns(
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


def read_table(
    path: PathLike,
    *,
    complete: bool = True,
    language: str = "en",
    max_cells: int = 100_000_000,
) -> IvtTable:
    """
    Decode an IVT file to a tidy cell table.

    Parameters
    ----------
    complete
        If True (default), expand to the published grid (absent cells → 0).
        If False, return only stored non-zero cells.
    """
    p = Path(path)
    data = p.read_bytes()
    meta = read_metadata(p)
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
