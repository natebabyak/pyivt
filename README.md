# pyivt

> [!NOTE]
> Before v0.1.0, the API is unstable and may change at any time. Production use is discouraged. If you use pyivt in production, pin it to an exact version.

## About the Project

Read Statistics Canada Beyond 20/20 (`.ivt`) files, inspect bilingual metadata, and export to Apache Arrow or Parquet.

## Getting Started

### Prerequisites

Python 3.10+ and PyArrow 14+. Installation brings in PyArrow automatically.

### Installation

```sh
pip install pyivt
```

With uv, run `uv add pyivt`.

## Usage

### Open a table

```python
import pyivt

ivt = pyivt.open("data.ivt")
print(ivt.metadata.title.en or ivt.metadata.title.fr)
```

Use an extracted IVT version 4 file. For a download, pass the complete IVT bytes to `pyivt.open()`.

### Inspect dimensions

```python
for slug, dimension in zip(ivt.layout.slugs, ivt.metadata.dimensions):
    print(slug, dimension.name.en, dimension.items)
```

Use the slugs as Arrow column names and selection keys.

### Export to Parquet

```python
import pyarrow.parquet as pq

table = ivt.to_arrow()
pq.write_table(table, "data.parquet")
for slug, lookup in ivt.to_member_tables().items():
    pq.write_table(lookup, f"data.{slug}.parquet")
```

Each lookup has one row per native member code. Join the fact table's
dimension column to the lookup's `code` column and select the attributes you need.

### Select a subset

```python
selection = pyivt.Selection({ivt.layout.slugs[0]: [1]})
subset = ivt.to_arrow(selection)
subset_members = ivt.to_member_tables(selection)
```

Pass the same selection to both methods so their code types and subsets match.

### Embed member tables in a single file

```python
from base64 import b64decode

import pyarrow as pa
import pyarrow.parquet as pq

pq.write_table(ivt.to_arrow(embed_members=True), "data.parquet")

slug = ivt.layout.slugs[0]
key = f"pyivt.members.v1.{slug}".encode()
metadata = pq.read_schema("data.parquet").metadata
with pa.ipc.open_file(b64decode(metadata[key])) as reader:
    lookup = reader.read_all()
```

Embedding leaves the fact columns unchanged. The lookup tables are stored as
base64-encoded Arrow IPC files in schema metadata. Use separate Parquet
lookups if your tools rewrite files without preserving custom metadata.

## API Reference

### Opening and exporting

```python
ivt = pyivt.open("data.ivt")
table = ivt.to_arrow()
members = ivt.to_member_tables()
```

- `open()` returns an `Ivt`. It accepts a path, IVT bytes, a `bytearray`, an object with `__bytes__`, or a binary file object.
- `to_arrow()` returns rows in file order with unsigned member codes and a nullable `float64` column named `value`.
- `to_member_tables()` returns a dictionary of PyArrow tables keyed by dimension slug, without decoding cells.
- `to_arrow(embed_members=True)` attaches the corresponding lookup tables as schema metadata.
- `ivt.__arrow_c_stream__()` returns an Arrow stream capsule. `pyivt.__version__` gives the package version.

### Selections

```python
selection.validate(ivt)
pyivt.Selection.from_dict(selection.to_dict())
pyivt.Selection.from_json(selection.to_json())
```

Both exports accept a `Selection` or a mapping from dimension slugs to 1-based codes. `selection_id` identifies the selection.

- `None` includes all rows and columns. `{}` includes all rows with only `value`.
- Listed axes filter rows and appear as columns. Omitted axes keep all their members. An empty code list gives zero rows.

### Member lookup tables

- `code` contains 1-based native ordinals in file order. `label_en` and `label_fr` are nullable strings. Time-axis labels use ISO dates.
- Native attributes use Arrow strings, integers, floats, or `date32` dates. Text codes retain leading zeros. Native fields that collide with lookup column names, such as `Code`, receive a `native_` prefix. Remaining name collisions receive numeric suffixes.
- Both the fact table and lookup schemas carry `pyivt.table_id` and `pyivt.selection_id` for matching exports.

### Metadata and layout

```python
meta = ivt.metadata
print(meta.title.en, meta.title["fr"])
print(ivt.layout.slugs, ivt.layout.counts)
```

Read-only `IvtMetadata` is available without decoding cells.

- `title`, `author`, `category`, `keywords`, and `notes` use `LocalizedText`.
- Table fields include `product_id`, `universe`, `number_of_datapoints`, `version`, `signature`, `descriptor_path`, and `read_only`.
- Time fields are `frequency`, `start_date`, and `end_date`. `Frequency` is `"monthly"`, `"quarterly"`, `"semi_annual"`, or `"annual"`.
- Both `LocalizedText` and `LocalizedLabels` accept `en` and `fr`. Missing translations are `None`.

#### Dimensions and legends

- `ivt.layout` returns a `Layout` with `slugs`, `counts`, `n_dim`, `geo_dim`, `straddle`, and `window_count`.
- Each `Dimension` in `meta.dimensions` has `name`, `description`, `notes`, `items`, `members`, and `ordinal`. Its `type` is a `DimensionType` of `"geographic"`, `"normal"`, or `"time_series"`.
- `meta.geographies` returns `Geographies` with `member_id`, `geo_label`, `geo_name`, `geo_uid`, `dqf_code`, and `tnr_short_form`.
- Each `Footnote` in `meta.footnotes` has `symbol`, `number`, `description`, `scope`, `dimension`, and `member_id`.
- Each `MissingValue` in `meta.missing_values` has `symbol`, `description`, and `code`.

## License

[MIT](LICENSE)

## Acknowledgments

- [canivt](https://github.com/mountainMath/canivt)
- [REA](https://github.com/morluto/rea)
