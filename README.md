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

To see `pyarrow.Table` as the return type of `ivt.to_arrow()` and
`ivt.to_member_table()` in your editor, install `pyarrow-stubs` in the same
Python environment as pyivt and select that environment in your editor.

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

Pass an IVT file or a StatCan ZIP download as a path or complete bytes.
A ZIP containing exactly one `.ivt` member opens automatically. If it contains
several, choose one with `pyivt.open("tables.zip", member="folder/table.ivt")`;
the member name is exact and case-sensitive. ZIP tables are streamed to a
private temporary file, which is removed when the reader and its metadata
are released. Opening parses metadata; exports decode the cells.

Container versions 2 and 4 are supported. Version 2 includes historical survey,
agriculture, and Business Patterns tables with sparse member slots and older
bilingual display fields. Version 3 remains explicitly unsupported because no
real sample was found in the [historical archive search](tests/corpus/legacy-version-search.json).
The [legacy verification report](docs/investigations/older-ivt-support.md)
records the tested files and comparison limits.

### Inspect dimensions

```python
for dimension in ivt.metadata.dimensions:
    print(dimension.slug, dimension.name.en, dimension.items)
```

Use the slugs as Arrow column names, selection keywords, and values in the
member table's `dimension` column.
The geography axis uses `geography`. Other axes follow their IVT dimension names,
so a time dimension may be named `census_year`, `reference_period`, or `time`.

### Export to Parquet

```python
import pyarrow.parquet as pq

table = ivt.to_arrow()
members = ivt.to_member_table()
pq.write_table(table, "data.parquet")
pq.write_table(members, "data.members.parquet")
```

To attach geography labels, filter the member table by `dimension` and cast
`code` to the fact column's type before joining.

```python
import pyarrow.compute as pc

lookup = members.filter(pc.equal(members["dimension"], "geography"))
lookup = lookup.select(["code", "label"])
lookup = lookup.set_column(0, "code", lookup["code"].cast(table["geography"].type))
labeled = table.join(lookup, keys="geography", right_keys="code")
```

### Select a subset

On a table with `geography` and `census_year`, fix geography and keep two years.

```python
table = ivt.to_arrow(geography=2, census_year=[1, 2])
members = ivt.to_member_table(geography=2, census_year=[1, 2])
```

Use `Dimension.slug` when the dimensions depend on the input file.

```python
slug = ivt.metadata.dimensions[0].slug
selection = {slug: [1]}
subset = ivt.to_arrow(**selection)
subset_members = ivt.to_member_table(**selection)
```

Both methods select by the members' 1-based positions in the file. An integer
fixes a dimension at one member and omits it from both outputs. A list retains
the dimension and limits its members to the given codes. Omitted dimensions
retain all members. An empty list gives zero fact rows and zero member rows
for that dimension.

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
Use enum members as codes when calling `to_arrow()` or `to_member_table()`.
`NATIVE_CODES[slug][member]` gives the original code as text, or `None` if it
is missing. Add `--language fr` to use French labels for enum names.

## API Reference

### Opening and exporting

- `open(source, *, member=None)` returns an `Ivt`. It accepts a path, IVT or ZIP bytes, a `bytearray`, an object with `__bytes__`, or a binary file object. `member` selects an IVT inside a ZIP with several tables.
- `to_arrow()` returns rows in file order with unsigned member codes and a nullable `float64` column named `value`.
- `to_member_table()` returns one PyArrow table of member attributes, without decoding cells.
- `pyivt.__version__` gives the package version.

### Member lookup table

- `dimension` is the first column and contains dictionary-encoded dimension slugs. Rows follow dimension order and then member order in the IVT file.
- `code` contains 1-based member positions. The member table uses one unsigned integer type wide enough for the source table's largest dimension. Each fact column still uses the width of its own dimension.
- `label` is a nullable string in English by default. Use `to_member_table(language="fr")` for French labels. Time-axis labels use ISO dates.
- Native attributes use Arrow strings, integers, floats, or `date32` dates. The native `Code` field, when present, is a separate text column (typically `native_Code`) and preserves leading zeros and duplicate values. Attributes are null where they are absent. Names that collide with `dimension`, `code`, or `label` receive a `native_` prefix. Incompatible attributes with the same name receive numeric suffixes.
- Export schemas use `pyivt.dimension_encoding=one_based_index`. The fact schema and member exports with retained dimensions also carry `pyivt.table_id` to identify the source table.

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
