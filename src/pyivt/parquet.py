"""Metadata CSV writers."""

from __future__ import annotations

import csv
from pathlib import Path

from .metadata import IvtMetadata
from .reader import IvtTable
from .tidy import members_table

PathLike = str | Path


def write_metadata(source: IvtMetadata | IvtTable, dir: PathLike) -> Path:
    """
    Write Beyond 20/20-style file-summary sidecars as CSV:

    - table_info.csv
    - dimension_members.csv
    - geographies.csv
    - footnotes.csv
    """
    if isinstance(source, IvtMetadata):
        meta = source
    else:
        meta = source.metadata

    out_dir = Path(dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    with (out_dir / "table_info.csv").open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["field", "value"])
        rows: list[tuple[str, object]] = [
            ("product_id", meta.product_id),
            ("title.en", meta.title.en),
            ("title.fr", meta.title.fr),
            ("file_name", meta.file_name),
            ("directory", meta.directory),
            ("author.en", meta.author.en),
            ("author.fr", meta.author.fr),
            ("category.en", meta.category.en),
            ("category.fr", meta.category.fr),
            ("keywords.en", meta.keywords.en),
            ("keywords.fr", meta.keywords.fr),
            ("universe", meta.universe),
            ("notes.en", meta.notes.en),
            ("notes.fr", meta.notes.fr),
            (
                "last_update",
                meta.last_update.isoformat() if meta.last_update else None,
            ),
            ("file_size", meta.file_size),
            ("number_of_datapoints", meta.number_of_datapoints),
            ("time_series", meta.time_series),
            ("read_only", meta.read_only),
            ("version", meta.version),
            ("signature", meta.signature),
        ]
        for field, value in rows:
            w.writerow([field, value])

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
