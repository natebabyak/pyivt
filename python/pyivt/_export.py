"""Prepare compact projections and typed member lookup tables."""

from __future__ import annotations

from base64 import b64encode
from typing import TYPE_CHECKING

from pyivt._selection import SelectionInput, _projection_id, _selected_axes

if TYPE_CHECKING:
    import pyarrow as pa

    from pyivt._native import Ivt, Layout

_Prepared = tuple[list[tuple[bool, list[int] | None]], str]
_DISPLAY_NAMES = (("english desc",), ("desc français", "french desc"))


def prepare(layout: Layout, selection: SelectionInput) -> _Prepared:
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


def build_member_tables(ivt: Ivt, prepared: _Prepared) -> dict[str, pa.Table]:
    import pyarrow as pa

    projection, selection_id = prepared
    if not any(included for included, _ in projection):
        return {}

    table_id, fields, display_fields = ivt._member_fields()
    layout = ivt.layout
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
    for (included, indices), slug, count, axis_fields, display in axes:
        if not included:
            continue
        codes = (
            range(1, count + 1) if indices is None else [index + 1 for index in indices]
        )
        max_code = codes[-1] if codes else 0
        code_type = next(
            dtype
            for dtype in (pa.uint8(), pa.uint16(), pa.uint32(), pa.uint64())
            if max_code.bit_length() <= dtype.bit_width
        )
        columns = {"code": pa.array(codes, type=code_type)}
        is_time, names, references = display
        arrays = [
            pa.array(
                values if indices is None else [values[index] for index in indices],
                type=types[kind],
            )
            for _, kind, values in axis_fields
        ]
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
            b"pyivt.selection_id": selection_id.encode(),
            b"pyivt.dimension": slug.encode(),
        }
        for index, language in enumerate(("en", "fr")):
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
            columns[f"label_{language}"] = (
                label if label is not None else pa.nulls(len(codes), type=pa.string())
            )
            title = names[index]
            if title is not None:
                metadata[f"pyivt.label_{language}".encode()] = title.encode()

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


def embed_member_tables(ivt: Ivt, table: pa.Table, prepared: _Prepared) -> pa.Table:
    import pyarrow as pa

    metadata = dict(table.schema.metadata or {})
    for slug, members in build_member_tables(ivt, prepared).items():
        sink = pa.BufferOutputStream()
        with pa.ipc.new_file(sink, members.schema) as writer:
            writer.write_table(members)
        metadata[f"pyivt.members.v1.{slug}".encode()] = b64encode(sink.getvalue())
    return table.replace_schema_metadata(metadata)
