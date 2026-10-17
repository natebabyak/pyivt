"""High-level IVT table reader."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import pyarrow as pa

from ._io import Source, load_source
from .decode import Layout, build_layout, complete_grid, decode_store
from .descriptor import parse_descriptor
from .metadata import IvtMetadata, read_metadata_bytes
from .tidy import tidy_arrays

PathLike = str | Path


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

    def to_arrow(
        self,
        *,
        labels: bool = True,
        language: str = "en",
    ) -> pa.Table:
        return _columns_to_arrow(self.to_columns(labels=labels, language=language))

    def to_pandas(
        self,
        *,
        labels: bool = True,
        language: str = "en",
    ):
        try:
            return self.to_arrow(labels=labels, language=language).to_pandas()
        except ImportError as exc:  # pragma: no cover - optional dep
            raise ImportError(
                "to_pandas() requires pandas; install it with: pip install pandas"
            ) from exc

    def to_polars(
        self,
        *,
        labels: bool = True,
        language: str = "en",
    ):
        try:
            import polars as pl
        except ImportError as exc:  # pragma: no cover - optional dep
            raise ImportError(
                "to_polars() requires polars; install it with: pip install polars"
            ) from exc
        return pl.from_arrow(self.to_arrow(labels=labels, language=language))

    def to_parquet(
        self,
        path: PathLike,
        *,
        labels: bool = True,
        members: bool = True,
        language: str = "en",
    ) -> Path:
        from .parquet import write_parquet

        return write_parquet(
            self,
            path,
            labels=labels,
            members=members,
            language=language,
        )


def open(
    source: Source,
    *,
    complete: bool = True,
    language: str = "en",
    max_cells: int = 100_000_000,
) -> IvtTable:
    """
    Open an IVT file from a path, URL, bytes, or binary file-like object.

    Parameters
    ----------
    complete
        If True (default), expand to the published grid (absent cells → 0).
        If False, return only stored non-zero cells.
    """
    data, origin = load_source(source)
    meta = read_metadata_bytes(
        data,
        file_name=origin.file_name,
        directory=origin.directory,
        last_update=origin.last_update,
        file_size=origin.file_size,
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
