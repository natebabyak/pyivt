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
for dimension in ivt.metadata.dimensions:
    print(dimension.slug, dimension.name.en, dimension.items)
```

Use the slugs as Arrow column names and selection keys.

### Export to Parquet

```python
import pyarrow.parquet as pq

table = ivt.to_arrow()
pq.write_table(table, "data.parquet")
for slug, lookup in ivt.members().items():
    pq.write_table(lookup, f"data.{slug}.parquet")
```

Each lookup has one row per member code. Join the fact table's dimension column
to the lookup's `code` column to attach labels or other attributes.

### Select a subset

```python
slug = ivt.metadata.dimensions[0].slug
code = ivt.members()[slug]["code"][0].as_py()
selection = {slug: [code]}
subset = ivt.to_arrow(**selection)
subset_members = ivt.members(**selection)
```

Pass the same dimension keywords to both methods so their code types and subsets match.
An integer fixes a dimension at one member and drops its column. A list retains
the column and filters its rows. Omitted dimensions retain all members and
their columns. An empty list selects zero rows.

### Generate Python types for a table

```sh
pyivt codegen data.ivt --output dimensions.py
```

The generated module contains one `IntEnum` per dimension and a `Dimensions`
typed dictionary for selection keywords. Use enum members as codes when calling
`to_arrow()` or `members()`. Add `--language fr` to use French labels for enum names.

## API Reference

### Opening and exporting

- `open()` returns an `Ivt`. It accepts a path, IVT bytes, a `bytearray`, an object with `__bytes__`, or a binary file object.
- `to_arrow()` returns rows in file order with unsigned member codes and a nullable `float64` column named `value`.
- `members()` returns a dictionary of PyArrow tables keyed by dimension slug, without decoding cells.
- `pyivt.__version__` gives the package version.

### Member lookup tables

- `code` contains native integer codes in file order. Axes without a native `Code` field use one-based positions. Invalid or duplicate native codes raise `ValueError`.
- `label` is a nullable string in English by default. Use `members(language="fr")` for French labels. Time-axis labels use ISO dates.
- Native attributes use Arrow strings, integers, floats, or `date32` dates. Text fields retain their original values. Native fields that collide with lookup column names, such as `Code`, receive a `native_` prefix. Remaining name collisions receive numeric suffixes.
- Both the fact table and lookup schemas carry `pyivt.table_id` to identify the source table.

### Metadata

```python
meta = ivt.metadata
print(meta.title.en, meta.title["fr"])
print([(dimension.slug, dimension.items) for dimension in meta.dimensions])
```

Read-only `IvtMetadata` is available without decoding cells.

- `title`, `author`, `category`, `keywords`, and `notes` use `LocalizedText`.
- Table fields include `product_id`, `universe`, `number_of_datapoints`, `version`, `signature`, and `read_only`.
- Time fields are `frequency`, `start_date`, and `end_date`. Known frequencies are `"monthly"`, `"quarterly"`, `"semi_annual"`, and `"annual"`.
- Both `LocalizedText` and `LocalizedLabels` accept `en` and `fr`. Missing translations are `None`.

#### Dimensions and legends

- Each `Dimension` in `meta.dimensions` has `slug`, `name`, `description`, `notes`, `items`, and `members`. Its `type` is `"geographic"`, `"normal"`, or `"time_series"`.
- `meta.geographies` returns `Geographies` with `member_id`, `geo_label`, `geo_name`, `geo_uid`, `dqf_code`, and `tnr_short_form`.
- Each `Footnote` in `meta.footnotes` has `symbol`, `number`, and `description`.
- Each `MissingValue` in `meta.missing_values` has `symbol`, `description`, and `code`.

## License

[MIT](LICENSE)

## Acknowledgments

- [canivt](https://github.com/mountainMath/canivt)
- [REA](https://github.com/morluto/rea)
