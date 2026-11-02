"""Selection and member preparation; row construction stays in Rust."""

from collections.abc import Mapping, Sequence
from datetime import date, timedelta

from pyivt._members import DimensionMembers, Member, Members, validate_codes


def prepare(ivt, select, max_memory_bytes):
    slugs, counts = ivt.layout.slugs, ivt.layout.counts
    if select is not None:
        if not isinstance(select, Mapping):
            raise ValueError(
                "select must map dimension slugs to lists of numeric member codes"
            )
        for name, codes in select.items():
            if name not in slugs:
                raise ValueError(
                    f"unknown dimension {name!r}; expected one of {slugs!r}"
                )
            axis = slugs.index(name)
            if not isinstance(codes, Sequence) or isinstance(codes, (str, bytes)):
                raise ValueError(  # noqa: TRY004 -- selection errors use ValueError
                    f"dimension {name}: member codes must be a sequence of integers"
                )
            if any(
                type(c) is not int or not 1 <= c <= counts[axis] for c in codes
            ) or len(set(codes)) != len(codes):
                raise ValueError(
                    f"dimension {name}: invalid or duplicate codes {codes!r}; expected 1..{counts[axis]}"
                )
    table_id, fields, dimension_labels = ivt._member_fields(max_memory_bytes)
    dimensions, selection, labels = [], [], []
    for axis, (slug, count) in enumerate(zip(slugs, counts, strict=True)):
        if select is not None and slug not in select:
            selection.append(None)
            labels.append([])
            continue
        full_codes = list(range(1, count + 1))
        first_code = validate_codes(full_codes, table_id, slug)
        codes = full_codes if select is None else sorted(select[slug])
        axis_fields = fields[axis]
        # Native display fields have stable names; preserve every field under
        # its recorded name. A table without display fields uses its ordinal.
        text_fields = [
            name
            for name, kind, _ in axis_fields
            if kind == "text" and name.lower() != "code"
        ]
        display = next(
            (name for name in ("English Desc", "French Desc") if name in text_fields),
            text_fields[0] if text_fields else None,
        )
        lookup = {name: values for name, _, values in axis_fields}
        members = []
        for code in codes:
            index = code - first_code
            metadata = {}
            for name, kind, values in axis_fields:
                value = values[index]
                metadata[name] = (
                    (date(1970, 1, 1) + timedelta(days=value)).isoformat()
                    if kind == "date" and value is not None
                    else value
                )
            label = lookup[display][index] if display else str(code)
            members.append(Member(code, label, metadata))
        dimensions.append(
            DimensionMembers(slug, dimension_labels[axis], first_code, tuple(members))
        )
        selection.append([c - first_code for c in codes])
        labels.append([m.label for m in members])
    result = Members(table_id, tuple(dimensions))
    return selection, labels, result
