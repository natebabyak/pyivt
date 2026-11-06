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

### Editor type hints

To see `pyarrow.Table` as the return type of `ivt.to_arrow()` in your editor,
install `pyarrow-stubs` in the same Python environment as pyivt and select that
environment in your editor.

```sh
pip install pyarrow-stubs
```

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

Use the slugs as Arrow column names, selection keywords, and member lookup keys.
The geography axis uses `geography`. Other axes follow their IVT dimension names,
so a time dimension may be named `census_year`, `reference_period`, or `time`.

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

On a table with `geography` and `census_year`, fix geography and keep two years.

```python
table = ivt.to_arrow(geography=2, census_year=[1, 2])
lookups = ivt.members(geography=2, census_year=[1, 2])
```

Use `Dimension.slug` when the dimensions depend on the input file.

```python
slug = ivt.metadata.dimensions[0].slug
code = ivt.members()[slug]["code"][0].as_py()
selection = {slug: [code]}
subset = ivt.to_arrow(**selection)
subset_members = ivt.members(**selection)
```

Pass the same dimension keywords to both methods so their code types and subsets match.
Codes are the members' 1-based positions in the file.
An integer fixes a dimension at one member and drops its column. A list retains
the column and filters its rows. Omitted dimensions retain all members and
their columns. An empty list selects zero rows.

### Pivot industry by occupation with Polars

On a table with `geography`, `labour_force`, `industry`, and `occupation`
dimensions, fix geography and labour force to get one value for each industry
and occupation pair. With Polars installed, pivot the Arrow result into one row
per industry and one column per occupation code.

```python
import polars as pl

table = ivt.to_arrow(geography=5, labour_force=1)
pivoted = pl.from_arrow(table).pivot(
    on="occupation",
    index="industry",
    values="value",
)
```

### Generate Python types for a table

```sh
pyivt codegen data.ivt --output dimensions.py
```

The generated module contains one `IntEnum` per dimension and a `Dimensions`
typed dictionary for selection keywords. Enum values are 1-based member positions.
Use enum members as codes when calling `to_arrow()` or `members()`.
`NATIVE_CODES[slug][member]` gives the original code as text, or `None` if it
is missing. Add `--language fr` to use French labels for enum names.

## API Reference

### Opening and exporting

- `open()` returns an `Ivt`. It accepts a path, IVT bytes, a `bytearray`, an object with `__bytes__`, or a binary file object.
- `to_arrow()` returns rows in file order with unsigned member codes and a nullable `float64` column named `value`.
- `members()` returns a dictionary of PyArrow tables keyed by dimension slug, without decoding cells.
- `pyivt.__version__` gives the package version.

### Member lookup tables

- `code` contains 1-based member positions in file order. Each dimension uses the smallest unsigned integer type that can hold its full member count, including in slices.
- `label` is a nullable string in English by default. Use `members(language="fr")` for French labels. Time-axis labels use ISO dates.
- Native attributes use Arrow strings, integers, floats, or `date32` dates. The native `Code` field, when present, is a separate text column (typically `native_Code`) and preserves leading zeros and duplicate values. Missing values are null. Native fields that collide with lookup column names receive a `native_` prefix. Remaining name collisions receive numeric suffixes.
- Both the fact table and lookup schemas carry `pyivt.table_id` to identify the source table and `pyivt.dimension_encoding` set to `one_based_index`.

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
