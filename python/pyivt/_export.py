"""Prepare compact projections and localized member maps."""

from __future__ import annotations

from typing import TYPE_CHECKING, cast

from pyivt._members import DimensionMembers, Language, Member, Members
from pyivt._selection import SelectionInput, _projection_id, _selected_axes

if TYPE_CHECKING:
    from pyivt._native import Ivt, Layout

_DISPLAY_NAMES = (("english desc",), ("desc français", "french desc"))
_LOCALIZED_NAMES = frozenset(name for group in _DISPLAY_NAMES for name in group)


def prepare(
    layout: Layout, selection: SelectionInput
) -> tuple[list[tuple[bool, list[int] | None]], str]:
    axes = _selected_axes(layout, selection)
    projection_id = _projection_id(
        [(slug, codes) for slug, codes in axes if codes is not None]
    )
    projection = [
        (
            codes is not None,
            None if codes is None or len(codes) == count else [c - 1 for c in codes],
        )
        for (_, codes), count in zip(axes, layout.counts, strict=True)
    ]
    return projection, projection_id


def prepare_members(ivt: Ivt, selection: SelectionInput, language: Language) -> Members:
    if language not in ("en", "fr"):
        raise ValueError(
            f"unsupported member language {language!r}; expected 'en' or 'fr'"
        )
    axes = _selected_axes(ivt.layout, selection)
    table_id, fields, display_fields = ivt._member_fields()
    language_index = 0 if language == "en" else 1
    dimensions = []
    for axis, (slug, codes) in enumerate(axes):
        if codes is None:
            continue
        axis_fields = fields[axis]
        is_time, names, references = display_fields[axis]
        labels = None
        if is_time:
            labels = next(
                (values for _, kind, values in axis_fields if kind == "date"), None
            )
        else:
            reference = references[language_index]
            if reference is not None:
                name, kind, values = axis_fields[reference]
                if kind == "text" and name.casefold() not in {
                    "code",
                    *_DISPLAY_NAMES[1 - language_index],
                }:
                    labels = values
            if labels is None:
                lookup = {
                    name.casefold(): values
                    for name, kind, values in axis_fields
                    if kind == "text" and name.casefold() in _LOCALIZED_NAMES
                }
                labels = next(
                    (
                        lookup[name]
                        for name in _DISPLAY_NAMES[language_index]
                        if name in lookup
                    ),
                    None,
                )
                if (
                    labels is None
                    and codes
                    and (lookup or any(i is not None for i in references))
                ):
                    raise ValueError(
                        f"dimension {slug}: member labels unavailable for language {language!r}"
                    )
        members = tuple(
            Member(
                code,
                cast("str | None", labels[code - 1]) if labels is not None else None,
                {name: values[code - 1] for name, _, values in axis_fields},
            )
            for code in codes
        )
        dimensions.append(
            DimensionMembers(slug, names[language_index] or slug, 1, members)
        )
    return Members(table_id, tuple(dimensions), language=language)
