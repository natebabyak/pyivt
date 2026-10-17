"""Long/tidy table construction from decoded cells + metadata."""

from __future__ import annotations

import numpy as np

from .decode import Layout
from .metadata import IvtMetadata


def label_depth(labels: list[str], unit: int = 2) -> list[int]:
    out = []
    for lab in labels:
        lead = len(lab) - len(lab.lstrip(" "))
        out.append(0 if unit < 1 else lead // unit)
    return out


def label_parent(labels: list[str], unit: int = 2) -> list[int | None]:
    depths = label_depth(labels, unit)
    parent: list[int | None] = [None] * len(depths)
    last_at: list[int | None] = []
    for i, di in enumerate(depths):
        if di > 0 and last_at:
            for k in range(min(di, len(last_at)) - 1, -1, -1):
                if last_at[k] is not None:
                    parent[i] = last_at[k]
                    break
        if len(last_at) > di + 1:
            for j in range(di + 1, len(last_at)):
                last_at[j] = None
        while len(last_at) <= di:
            last_at.append(None)
        last_at[di] = i + 1  # 1-based member id
    return parent


def tidy_arrays(
    cells: dict,
    meta: IvtMetadata,
    lay: Layout,
    *,
    labels: bool = True,
    language: str = "en",
) -> dict[str, np.ndarray | list]:
    """Build column dict suitable for a DataFrame / Arrow table."""
    lang = language.lower()
    out: dict[str, np.ndarray | list] = {}
    n = len(cells["value"])

    for dim_i, slug in enumerate(lay.slugs, start=1):
        ids = cells[slug]
        out[slug] = ids
        if not labels:
            continue
        dim = meta.dimensions[dim_i - 1]
        if dim.is_geography:
            if lang.startswith("fr") and "geo_label_fr" in meta.geographies:
                labs = meta.geographies["geo_label_fr"]
            else:
                labs = meta.geographies.get("geo_label") or meta.geographies.get(
                    "geo_name"
                )
            if labs:
                # member ids are 1-based
                mapped = [labs[i - 1] if 1 <= i <= len(labs) else None for i in ids]
                out[f"{slug}_label"] = mapped
            uids = meta.geographies.get("geo_uid")
            if uids:
                out["geo_uid"] = [
                    uids[i - 1] if 1 <= i <= len(uids) else None for i in ids
                ]
        else:
            mem = (
                dim.members_fr
                if lang.startswith("fr") and dim.members_fr
                else dim.members
            )
            if mem:
                out[f"{slug}_label"] = [
                    mem[i - 1] if 1 <= i <= len(mem) else None for i in ids
                ]

    out["value"] = cells["value"]
    # ensure length
    for k, v in list(out.items()):
        if isinstance(v, list) and len(v) != n:
            out[k] = (v + [None] * n)[:n]
    return out


def members_table(meta: IvtMetadata) -> list[dict]:
    """Flat dimension_members rows."""
    rows: list[dict] = []
    for d in meta.dimensions:
        if d.is_geography:
            geos = meta.geographies
            n = d.count
            labels = geos.get("geo_label") or geos.get("geo_name") or [None] * n
            labels_fr = geos.get("geo_label_fr")
            depths = label_depth([x or "" for x in labels]) if labels else [0] * n
            parents = label_parent([x or "" for x in labels]) if labels else [None] * n
            for i in range(n):
                rows.append(
                    {
                        "dimension": d.name,
                        "dimension_fr": d.name_fr,
                        "member_id": i + 1,
                        "ordinal": i + 1,
                        "label": labels[i] if labels and i < len(labels) else None,
                        "label_fr": labels_fr[i]
                        if labels_fr and i < len(labels_fr)
                        else None,
                        "depth": depths[i] if i < len(depths) else 0,
                        "parent_id": parents[i] if i < len(parents) else None,
                        "geo_uid": (geos.get("geo_uid") or [None] * n)[i]
                        if geos.get("geo_uid")
                        else None,
                    }
                )
            continue
        mem = d.members or []
        mem_fr = d.members_fr
        depths = label_depth(mem) if mem else []
        parents = label_parent(mem) if mem else []
        for i in range(d.count):
            rows.append(
                {
                    "dimension": d.name,
                    "dimension_fr": d.name_fr,
                    "member_id": i + 1,
                    "ordinal": i + 1,
                    "label": mem[i] if i < len(mem) else None,
                    "label_fr": mem_fr[i] if mem_fr and i < len(mem_fr) else None,
                    "depth": depths[i] if i < len(depths) else 0,
                    "parent_id": parents[i] if i < len(parents) else None,
                    "geo_uid": None,
                }
            )
    return rows
