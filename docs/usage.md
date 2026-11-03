# Using pyivt

## Open an IVT source

Pass a path, complete IVT bytes, or a binary file object to `pyivt.open()`. The reader supports IVT container version 4.

```python
from pathlib import Path

import pyivt

path = Path("data.ivt")
ivt = pyivt.open(path)
in_memory = pyivt.open(path.read_bytes())
```

A binary file object is read from its current position and stays open. For an online file, download it first and pass its contents. Extract archived IVTs before opening them.

When opening by path, keep the file available and unchanged while using the reader.

## Inspect metadata

Read-only metadata is available through `ivt.metadata`. The package includes type hints for editor completion.

| Field | What it contains |
| --- | --- |
| `title`, `author`, `category`, `keywords`, `notes` | Localized text |
| `product_id`, `universe` | Product identifier and population description, if available |
| `number_of_datapoints` | Size of the full dimension grid, including missing cells |
| `frequency`, `start_date`, `end_date` | Time-series frequency and ISO dates, if available |
| `read_only` | The file's read-only flag, if available |
| `dimensions` | Dimensions in file order, with localized names, descriptions, member labels, and notes |
| `geographies` | Geography labels and native DGUIDs when available |
| `footnotes`, `missing_values` | Footnote and missing-value legends |

Localized fields support both `.en` / `.fr` and `["en"]` / `["fr"]`. Unavailable translations and blank optional text are `None`. Multiple keyword strings are joined with `", "`.

```python
meta = ivt.metadata
print(meta.title.en, meta.title["fr"])

for slug, dimension in zip(ivt.layout.slugs, meta.dimensions):
    print(slug, dimension.name.en, dimension.type, dimension.items)

for note in meta.footnotes:
    print(note.symbol, note.description.en)

for missing in meta.missing_values:
    print(missing.code, missing.symbol, missing.description.fr)
```

`dimension.items` is the member count and `dimension.count` is an alias. `DimensionType` covers `"geographic"`, `"normal"`, and `"time_series"`. `Frequency` covers `"monthly"`, `"quarterly"`, `"semi_annual"`, and `"annual"`.

## Export a compact table

```python
table, members = ivt.to_arrow()
```

The table follows file dimension order, with the last dimension varying fastest. Each dimension column is a dictionary of display labels with unsigned indices. The `value` column is nullable `float64`. Missing-value statuses produce nulls, and published zeros remain zero.

### Select rows and dimension columns

Pass a mapping from `ivt.layout.slugs` to sequences of numeric member codes. Use the 1-based `Member.code`, since stored text codes such as `"001"` belong to `Member.metadata`.

```python
axis = ivt.members().dimensions[0]
select = {axis.name: [member.code for member in axis.members[:2]]}
table, members = ivt.to_arrow(select)
```

| Selection | Result |
| --- | --- |
| `None` | All rows and dimension columns |
| `{}` | All rows with only the `value` column |
| A mapping with selected axes | Listed axes use the selected codes and appear as columns. Omitted axes retain all their rows and have no column. |
| An empty code sequence for any axis | Zero rows |

Selection preserves file order and does not aggregate rows. Unknown slugs and invalid or duplicate codes raise `ValueError`.

`to_arrow()` raises `MemoryError` when its estimated allocation exceeds `max_memory_bytes` (2 GiB by default). Use a smaller selection to reduce the size of the compact export.

### Work with member maps

Call `ivt.members(select)` to retrieve the same member maps without decoding cells.

```python
for dimension in members.dimensions:
    for member in dimension.members:
        print(dimension.name, member.code, member.label, member.metadata)

members.to_json("data.members.json")
restored = pyivt.Members.read_json("data.members.json")
restored.validate_table(table)
```

`Members.from_json(text)` reads a JSON string. `validate_table()` checks that the table and member maps match in source, selection, and label dictionaries.

Labels may repeat or be null. Dictionary index `i` in a dimension column refers to `dimension.members[i]`, even when the selected member codes have gaps. Member metadata uses native field names, ISO date strings, and JSON scalars.

Use Arrow IPC to preserve the dictionary layout when saving the table for use with `validate_table()`. Parquet readers may reconstruct the dictionaries.

```python
import pyarrow as pa

with pa.ipc.new_file("data.arrow", table.schema) as writer:
    writer.write_table(table)
```

### Convert to pandas or Polars

Install the dataframe library you want to use.

```python
pandas_frame = members.to_pandas(table)
```

`to_pandas()` handles duplicate and missing labels. Keep the Arrow table and member maps if you need to distinguish members that share a label.

```python
import polars as pl

polars_frame = pl.from_arrow(table)
```

## Stream detailed batches

Use `ivt.to_batches()` for native member IDs and fields, `value`, `status_code`, `symbol`, `status`, and `footnote_raw`. The iterator yields Arrow record batches for the full grid in file order.

```python
import pyarrow.parquet as pq

batches = ivt.to_batches(batch_size=100_000)
first = next(batches)
with pq.ParquetWriter("detailed.parquet", first.schema) as writer:
    writer.write_batch(first)
    del first
    for batch in batches:
        writer.write_batch(batch)
```

`batch_size` limits rows per batch and defaults to 1,000,000. Consume batches as they arrive to keep memory bounded. `max_memory_bytes` controls the allocation estimate and defaults to 1 GiB.

The reader also exposes the detailed schema through the Arrow C stream protocol. PyArrow versions with `RecordBatchReader.from_stream()` can open it with `pa.RecordBatchReader.from_stream(ivt)`.
