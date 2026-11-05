"""Compare logical values independently of Arrow's storage encodings."""

import pyarrow as pa


def stream_reader(ivt) -> pa.RecordBatchReader:
    return pa.RecordBatchReader._import_from_c_capsule(ivt._arrow_c_stream())


def expanded_table(ivt) -> pa.Table:
    """Detailed streaming schema for pre-0.1 member/status export oracles."""
    with stream_reader(ivt) as reader:
        return reader.read_all()


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
