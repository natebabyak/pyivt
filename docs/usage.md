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

`dimension.items` is the declared member count. `DimensionType` covers `"geographic"`, `"normal"`, and `"time_series"`. `Frequency` covers `"monthly"`, `"quarterly"`, `"semi_annual"`, and `"annual"`.

## Export a compact table

```python
table = ivt.to_arrow()
members = ivt.members()
```

The table follows file dimension order, with the last dimension varying fastest. Each included dimension column stores native member codes directly, using the smallest unsigned type that fits the largest selected code. The `value` column is nullable `float64`. Missing-value statuses produce nulls, and published zeros remain zero.

### Select rows and dimension columns

Build a reusable `Selection` from dimension slugs and sequences of numeric member codes. Both methods also accept the mapping directly. Use the 1-based `Member.code`, since stored text codes such as `"001"` belong to `Member.metadata`.

```python
axis = ivt.layout.slugs[0]
selection = pyivt.Selection({axis: [1]})
selection.validate(ivt)
table = ivt.to_arrow(selection)
members = ivt.members(selection)
```

| Selection | Result |
| --- | --- |
| `None` | All rows and dimension columns |
| `{}` | All rows with only the `value` column |
| A mapping with selected axes | Listed axes use the selected codes and appear as columns. Omitted axes retain all their rows and have no column. |
| An empty code sequence for any axis | Zero rows |

Selection preserves file order and does not aggregate rows. Unknown slugs and invalid or duplicate codes raise `ValueError`. Use `Selection.to_json()` and `Selection.from_json()` to save and reuse the mapping.

Compact export and member preparation apply an internal 2 GiB allocation estimate. A request over the limit raises `MemoryError`. A smaller selection reduces the size of the compact export.

### Work with member maps

Call `ivt.members(selection, language="en")` to read labels and native attributes without decoding cells. Use `language="fr"` for French. If a dimension has localized labels but lacks the requested translation, `members()` raises `ValueError` naming the axis. Unlabelled axes and missing individual labels use `None`. Time-series labels are ISO 8601 dates in either language.

```python
for dimension in members.dimensions:
    for member in dimension.members:
        print(dimension.name, member.code, member.label, member.metadata)

members.to_json("data.members.json")
restored = pyivt.Members.read_json("data.members.json")
restored.validate_table(table)
```

`Members.from_json(text)` reads a JSON string. The JSON records the requested language, source SHA-256, selection fingerprint, and content checksum. `validate_table()` checks the source, projection, and member codes. Either language can be joined to the same numeric table.

Join each dimension column with `Member.code` to attach labels. Labels may repeat or be null. Member metadata preserves native field names, ISO date strings, and JSON scalars.

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


### Generate types for one table

```python
from pathlib import Path

Path("table_types.py").write_text(pyivt.codegen(ivt), encoding="utf-8")
```

The generated module contains an `IntEnum` per axis, a `DimensionSlug` literal, `COLUMN_NAMES`, the source SHA-256 as `TABLE_ID`, and a typed `selection()` helper. `Ivt.to_arrow()` returns a standard `pyarrow.Table`. For a table with a `geo` axis and a member labelled Canada, the generated module can be used like this.

```python
from table_types import Geo, selection

table = ivt.to_arrow(selection({"geo": [Geo.CANADA]}))
```

`codegen()` accepts an opened reader, any supported IVT source, a `Members` object, or members JSON as text or a `.json` path. It returns module text for the caller to save. The requested `language` determines the enum names when reading IVT. JSON uses its saved labels. Names are normalized to ASCII identifiers, with numeric suffixes for collisions and code-based names when a label is missing.
