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
members = ivt.members(language="en")

pq.write_table(table, "data.parquet")
members.to_json("data.members.json")
```

Keep the member JSON alongside the Parquet file for labels and native attributes. Join on `Member.code`.

### Select a subset

```python
selection = pyivt.Selection({ivt.layout.slugs[0]: [1]})
subset = ivt.to_arrow(selection)
subset_members = ivt.members(selection)
subset_members.validate_table(subset)
```

Pass the same selection to both methods.

### Load member labels

```python
table = pq.read_table("data.parquet")
members = pyivt.Members.read_json("data.members.json")
members.validate_table(table)
```

With pandas installed, use `members.to_pandas(table)` for categorical labels. With Polars installed, `polars.from_arrow(table)` keeps the numeric codes.

### Generate typed selections

```python
pyivt.generate_member_types("data.members.json", "table_types.py")
```

Use the generated enums and `selection()` helper to select members by name.

## API Reference

### Opening and exporting

```python
ivt = pyivt.open("data.ivt")
table = ivt.to_arrow()
members = ivt.members(language="en")
```

- `open()` returns an `Ivt`. It accepts a path, IVT bytes, a `bytearray`, an object with `__bytes__`, or a binary file object.
- `to_arrow()` returns rows in file order with unsigned member codes and a nullable `float64` column named `value`.
- `members()` reads labels and attributes without decoding cells. `Language` is `"en"` (default) or `"fr"`.
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

### Member maps

```python
text = members.to_json()
restored = pyivt.Members.from_json(text)
restored.validate_table(table)
```

Pass a path to `to_json()` to write a file. Load it with `Members.read_json()`. Use `members.to_pandas(table)` for categorical labels.

- `Members` has `table_id`, `selection_id`, `schema_version`, `language`, and `dimensions`.
- `DimensionMembers` has `name` (slug), `label`, `first_code`, and `members`.
- `Member` has `code`, `label`, and `metadata`. A `MemberValue` is a string, integer, float, or `None`.

### Type generation

```python
pyivt.generate_member_types(text, "table_types.py")
```

Pass JSON text or a JSON file path. The module provides an enum per axis, `TABLE_ID`, `COLUMN_NAMES`, `DimensionSlug`, `SelectionCodes`, and `selection()`.

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
