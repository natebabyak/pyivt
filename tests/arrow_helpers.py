"""Compare logical values independently of Arrow's storage encodings."""

import pyarrow as pa


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
