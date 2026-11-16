"""Validate dimension selections for compact and member-table exports."""

from __future__ import annotations

from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from pyivt._native import Ivt, _IvtInternals

_Axis = tuple[bool, list[int] | None]


def internals(ivt: Ivt) -> _IvtInternals:
    """Type the private hooks that the public Ivt stub leaves out."""
    return cast("_IvtInternals", ivt)


def prepare(ivt: Ivt, dims: dict[str, int | list[int]]) -> list[_Axis]:
    layout = internals(ivt)._layout
    for slug in dims.keys() - set(layout.slugs):
        raise ValueError(
            f"unknown dimension {slug!r}; expected one of {layout.slugs!r}"
        )
    axes = []
    for slug, count in zip(layout.slugs, layout.counts, strict=True):
        if slug not in dims:
            axes.append((True, None))
            continue
        choice = dims[slug]
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
