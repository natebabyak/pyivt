"""Parquet / metadata CSV writers."""

from __future__ import annotations

import csv
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

from .metadata import IvtMetadata, read_metadata
from .reader import IvtTable, read_table
from .tidy import members_table

PathLike = str | Path


def _columns_to_table(cols: dict) -> pa.Table:
    arrays = {}
    for k, v in cols.items():
        if hasattr(v, "dtype"):
            arrays[k] = pa.array(v)
        else:
            arrays[k] = pa.array(v)
    return pa.table(arrays)


def write_parquet(
    source: IvtTable | PathLike,
    path: PathLike,
    *,
    labels: bool = True,
    members: bool = True,
    language: str = "en",
    complete: bool = True,
) -> Path:
    """
    Write the published (or sparse) cell table to Parquet.

    When ``source`` is a path, the file is decoded first. A ``_members.parquet``
    sidecar is written next to ``path`` when ``members`` is True.
    """
    out = Path(path)
    out.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(source, IvtTable):
        tab = source
    else:
        tab = read_table(source, complete=complete, language=language)
    cols = tab.to_columns(labels=labels, language=language)
    pq.write_table(_columns_to_table(cols), out)
    if members:
        mem_path = out.with_name(out.stem + "_members.parquet")
        rows = members_table(tab.metadata)
        if rows:
            mem_cols = {k: [r[k] for r in rows] for k in rows[0]}
            pq.write_table(_columns_to_table(mem_cols), mem_path)
    return out


def write_metadata(source: IvtMetadata | IvtTable | PathLike, dir: PathLike) -> Path:
    """
    Write Beyond 20/20-style file-summary sidecars as CSV:

    - table_info.csv
    - dimension_members.csv
    - geographies.csv
    - footnotes.csv
    """
    if isinstance(source, IvtMetadata):
        meta = source
    elif isinstance(source, IvtTable):
        meta = source.metadata
    else:
        meta = read_metadata(source)

    out_dir = Path(dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    with (out_dir / "table_info.csv").open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["field", "value"])
        for field in (
            "product_id",
            "title_en",
            "title_fr",
            "universe",
            "version",
            "signature",
        ):
            w.writerow([field, getattr(meta, field, None)])
        if meta.notes:
            w.writerow(["notes", meta.notes])

    rows = members_table(meta)
    if rows:
        with (out_dir / "dimension_members.csv").open(
            "w", newline="", encoding="utf-8"
        ) as f:
            w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
            w.writeheader()
            w.writerows(rows)

    geos = meta.geographies
    if geos and geos.get("member_id"):
        keys = list(geos.keys())
        n = len(geos["member_id"])
        with (out_dir / "geographies.csv").open("w", newline="", encoding="utf-8") as f:
            w = csv.writer(f)
            w.writerow(keys)
            for i in range(n):
                w.writerow(
                    [
                        geos[k][i] if geos[k] is not None and i < len(geos[k]) else None
                        for k in keys
                    ]
                )

    if meta.footnotes:
        with (out_dir / "footnotes.csv").open("w", newline="", encoding="utf-8") as f:
            w = csv.writer(f)
            w.writerow(["language", "number", "text"])
            for fn in meta.footnotes:
                w.writerow([fn.language, fn.number, fn.text])

    return out_dir
