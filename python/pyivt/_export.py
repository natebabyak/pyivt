"""Validate dimension selections for compact and member-table exports."""

from __future__ import annotations

import warnings
from collections.abc import Mapping
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from pyivt._native import Ivt, _IvtInternals

_Axis = tuple[bool, list[int] | None]


def internals(ivt: Ivt) -> _IvtInternals:
    """Type the private hooks that the public Ivt stub leaves out."""
    return cast("_IvtInternals", ivt)


def _selection(
    select: object, dims: dict[str, int | list[int]]
) -> Mapping[str, int | list[int]]:
    if select is None:
        if dims:
            # Skip _selection and prepare; the Rust method adds no Python frame.
            warnings.warn(
                "Passing dimensions as keywords is deprecated and will be removed "
                "in a future minor release; pass them as select={...} instead, "
                f"for example select={dict(dims)!r}",
                DeprecationWarning,
                stacklevel=3,
            )
        return dims
    if dims:
        raise TypeError(
            f"unexpected keyword arguments {sorted(dims)!r}; "
            "pass every dimension in select={...}"
        )
    if not isinstance(select, Mapping):
        raise TypeError(
            "select must be a mapping of dimension slugs to member codes, "
            f"got {type(select).__name__}"
        )
    return cast("Mapping[str, int | list[int]]", select)


def prepare(ivt: Ivt, select: object, dims: dict[str, int | list[int]]) -> list[_Axis]:
    chosen = _selection(select, dims)
    layout = internals(ivt)._layout
    for slug in sorted(chosen.keys() - set(layout.slugs), key=str):
        raise ValueError(
            f"unknown dimension {slug!r}; expected one of {layout.slugs!r}"
        )
    axes = []
    for slug, count in zip(layout.slugs, layout.counts, strict=True):
        if slug not in chosen:
            axes.append((True, None))
            continue
        choice = chosen[slug]
        included = isinstance(choice, list)
        selected = choice if included else [choice]
        indices = set()
        for code in selected:
            if not isinstance(code, int) or isinstance(code, bool):
                raise ValueError(  # noqa: TRY004
                    f"dimension {slug}: expected an integer code, got {code!r}"
                )
            index = code - 1
            if not 0 <= index < count:
                raise ValueError(f"dimension {slug}: unknown member code {code}")
            if index in indices:
                raise ValueError(f"dimension {slug}: duplicate member code {code}")
            indices.add(index)
        axes.append((included, None if len(indices) == count else sorted(indices)))
    return axes
