"""Compare logical values independently of Arrow's storage encodings."""

import pyarrow as pa


def expanded_table(ivt) -> pa.Table:
    """Detailed streaming schema for pre-0.1 member/status export oracles."""
    return pa.Table.from_batches(ivt.to_batches())


def plain_table(table: pa.Table) -> pa.Table:
    schema = pa.schema(
        [
            pa.field(
                field.name,
                pa.string()
                if pa.types.is_dictionary(field.type)
                else pa.int64()
                if field.type == pa.int32()
                else field.type,
            )
            for field in table.schema
        ]
    )
    return table.cast(schema)
