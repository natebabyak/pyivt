# Using pyivt in an editor

`pyivt` ships PEP 561 type information (`py.typed` and `.pyi` files). Select the
Python interpreter from the environment where you installed `pyivt`. A language
server that reads Python stubs can then show metadata completions, return types,
and Markdown hover documentation with Python examples. Runtime documentation is
also available through `help(pyivt.open)` and `help(pyivt.Ivt.to_arrow)`.

For a source checkout, build the extension before running examples:

```bash
uv sync --group dev
uv run maturin develop --release --skip-install
```

## Open a table and inspect localized metadata

```python
from pathlib import Path
import pyivt

ivt = pyivt.open(Path("table.ivt"))
meta = ivt.metadata
title = meta.title.en or meta.title.fr or "Untitled"
print(title)
```

Localized text uses `.en` / `.fr` or `["en"]` / `["fr"]`. Each translation is
`str | None`, with no automatic fallback. Blank author, category, and keyword
translations are `None`. Unsupported language keys raise `KeyError`.

```python
author = meta.author["fr"] or meta.author.en
category = meta.category.en
keyword_text = meta.keywords.en
keywords = [word.strip() for word in keyword_text.split(",")] if keyword_text else []
```

Keyword text stays a string; separate strings stored in the file are joined
with `", "`. The API does not return a list of keywords.

## Filter dimensions with typed values

`DimensionType` is the string literal enum
`Literal["geographic", "normal", "time_series"]`. At runtime `dimension.type`
is a string. Editors and type checkers can narrow it and reject misspelled
values in your annotations.

```python
from pyivt import DimensionType

kind: DimensionType = "time_series"
for dim in meta.dimensions:
    if dim.type == kind:
        print(dim.name.en, dim.count, dim.description.fr)
        for member_id, label in enumerate(dim.members.en or [], start=1):
            print(member_id, label)
```

Member lists use `LocalizedLabels`, so each translation is `list[str] | None`.
Labels follow member ID order; the first label describes member ID 1. Leading
spaces may encode hierarchy. `ivt.layout.slugs` gives the corresponding Arrow
member-ID column names in dimension order. `layout.geo_dim` is a one-based
index and may select a fallback axis even if its declared type is `"normal"`.

## Read annotation and missing-value descriptions

```python
for footnote in meta.footnotes:
    description = footnote.description.fr or footnote.description.en
    print(footnote.number, footnote.symbol, description)

status_descriptions = {
    entry.code: entry.description.en or entry.description.fr
    for entry in meta.missing_values
    if entry.code is not None
}
```

Each footnote has one localized `description` and appears once in legend order.
Missing-value entries use the same `description: LocalizedText` shape. Their
`code` corresponds to the Arrow `status_code` column. Arrow `footnote_raw`
contains raw markers; it does not resolve them to these descriptions. Numbered
notes embedded in table or dimension notes stay in `meta.notes` or `dim.notes`.

## Export to Arrow and Parquet

```python
import pyarrow.parquet as pq

table = ivt.to_arrow()
pq.write_table(table, "table.parquet")
```

Opening a table parses summary metadata and its layout. Dimensions,
geographies, footnotes, and missing values load on first access. `to_arrow()`
materializes the full grid, including missing cells, and caches decoded columns.
Keep path-based sources available and unchanged until reading is finished.

Metadata properties are read-only. Returned lists are copies; editing a list
does not modify the IVT file or the reader's cached metadata.

## Update existing code

| Previous API | Current API |
| --- | --- |
| One footnote per language, with `.language` and `.text` | One footnote with `.description.en` / `.description.fr` |
| `missing.label.en` | `missing.description.en` |
| Numeric `dim.type` and `dim.type_label` | `dim.type`: `"geographic"`, `"normal"`, or `"time_series"` |
| Empty author/category translation `""` | `None` |

The public type contracts and examples are checked with:

```bash
uv run ty check python tests/typing
```
