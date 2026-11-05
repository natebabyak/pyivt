"""Prepare compact projections and typed member lookup tables."""

from __future__ import annotations

import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import pyarrow as pa

    from pyivt._native import Ivt

_Axis = tuple[bool, list[int] | None, list[int] | None]
_DISPLAY_NAMES = (("english desc",), ("desc français", "french desc"))


def prepare(ivt: Ivt, dims: dict[str, int | list[int]]) -> list[_Axis]:
    layout = ivt._layout
    for slug in dims.keys() - set(layout.slugs):
        raise ValueError(
            f"unknown dimension {slug!r}; expected one of {layout.slugs!r}"
        )
    _, fields, _ = ivt._member_fields(code_only=True)
    axes = []
    for slug, count, axis_fields in zip(
        layout.slugs, layout.counts, fields, strict=True
    ):
        native = next(
            (values for name, _, values in axis_fields if name.casefold() == "code"),
            None,
        )
        codes = None
        positions = None
        if native is not None:
            codes = []
            positions = {}
            for index, value in enumerate(native):
                if isinstance(value, str) and re.fullmatch(r"[0-9]+", value):
                    value = int(value)
                if not isinstance(value, int) or not 0 <= value < 2**64:
                    raise ValueError(
                        f"dimension {slug}: invalid native Code {value!r} at position {index + 1}"
                    )
                if value in positions:
                    raise ValueError(f"dimension {slug}: duplicate native Code {value}")
                positions[value] = index
                codes.append(value)

        if slug not in dims:
            axes.append((True, None, codes))
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
            index = positions.get(code) if positions is not None else code - 1
            if index is None or not 0 <= index < count:
                raise ValueError(f"dimension {slug}: unknown member code {code}")
            if index in indices:
                raise ValueError(f"dimension {slug}: duplicate member code {code}")
            indices.add(index)
        ordered = sorted(indices)
        if len(ordered) == count:
            axes.append((included, None, codes if included else None))
        else:
            selected_codes = (
                [codes[i] for i in ordered] if included and codes is not None else None
            )
            axes.append((included, ordered, selected_codes))
    return axes


def build_member_tables(
    ivt: Ivt, projection: list[_Axis], language: str
) -> dict[str, pa.Table]:
    import pyarrow as pa

    if not any(included for included, _, _ in projection):
        return {}
    index = ("en", "fr").index(language)
    table_id, fields, display_fields = ivt._member_fields()
    layout = ivt._layout
    types = {
        "text": pa.string(),
        "integer": pa.int64(),
        "float": pa.float64(),
        "date": pa.date32(),
    }
    tables = {}
    axes = zip(
        projection, layout.slugs, layout.counts, fields, display_fields, strict=True
    )
    for (included, indices, native_codes), slug, count, axis_fields, display in axes:
        if not included:
            continue
        if native_codes is not None:
            codes = native_codes
        elif indices is None:
            codes = range(1, count + 1)
        else:
            codes = [position + 1 for position in indices]
        max_code = (
            max(native_codes, default=0)
            if native_codes is not None
            else (codes[-1] if codes else 0)
        )
        columns = {"code": pa.array(codes, type=_unsigned_type(max_code))}
        is_time, names, references = display
        arrays = []
        for name, kind, values in axis_fields:
            selected = values if indices is None else [values[i] for i in indices]
            dtype = types[kind]
            if name.casefold() == "sort" and kind == "integer":
                integers = [value for value in selected if isinstance(value, int)]
                if min(integers, default=0) >= 0:
                    dtype = _unsigned_type(max(integers, default=0))
            arrays.append(pa.array(selected, type=dtype))
        text_fields = {
            name.casefold(): array
            for (name, kind, _), array in zip(axis_fields, arrays, strict=True)
            if kind == "text"
        }
        date_label = None
        if is_time:
            date_label = next(
                (
                    array.cast(pa.string())
                    for array in arrays
                    if array.type == pa.date32()
                ),
                None,
            )
        metadata = {
            b"pyivt.table_id": table_id.encode(),
            b"pyivt.dimension": slug.encode(),
        }
        label = date_label
        if not is_time:
            reference = references[index]
            if reference is not None:
                name, kind, _ = axis_fields[reference]
                if kind == "text" and name.casefold() not in {
                    "code",
                    *_DISPLAY_NAMES[1 - index],
                }:
                    label = arrays[reference]
            if label is None:
                label = next(
                    (
                        text_fields[name]
                        for name in _DISPLAY_NAMES[index]
                        if name in text_fields
                    ),
                    None,
                )
        columns["label"] = (
            label if label is not None else pa.nulls(len(codes), type=pa.string())
        )
        if (title := names[index]) is not None:
            metadata[b"pyivt.label"] = title.encode()

        reserved = set(columns)
        used = reserved.copy()
        for (name, _, _), array in zip(axis_fields, arrays, strict=True):
            base = f"native_{name}" if name.casefold() in reserved else name
            column = base
            suffix = 2
            while column.casefold() in used:
                column = f"{base}_{suffix}"
                suffix += 1
            columns[column] = array
            used.add(column.casefold())
        tables[slug] = pa.table(columns, metadata=metadata)
    return tables


def _unsigned_type(max_value: int) -> pa.DataType:
    import pyarrow as pa

    return next(
        dtype
        for dtype in (pa.uint8(), pa.uint16(), pa.uint32(), pa.uint64())
        if max_value.bit_length() <= dtype.bit_width
    )
