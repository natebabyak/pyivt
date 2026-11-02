# pyivt

`pyivt` is a library for Statistics Canada Beyond 20/20 `.ivt` files, written in Rust. It is designed to be minimal and easily adaptable for existing codebases. Key features are:

> [!NOTE]
> `pyivt` is under active development. Version 0.1.0 introduces a breaking Arrow API change; see the migration notes below.

- **Fast**: Orders of magnitude faster than GUI automation for metadata
- **Python**: Easily integrates into modern data workflows with a minimal API
- **Typed**: Produces fully type-safe and predictable metadata
- **Interoperable**: Exports `.ivt` file data to Apache Arrow

## Installation

- Using `uv` (Recommended):

  ```bash
  uv add pyivt
  ```

- Using `pip`

  ```bash
  pip install pyivt
  ```

## Usage

### Quick Start

Open an IVT file, read its metadata, and write its data to a Parquet file:

```python
import pyarrow.parquet as pq
import pyivt

ivt = pyivt.open("data.ivt")
print(ivt.metadata.title.en)
table, members = ivt.to_arrow()
pq.write_table(table, "data.parquet")
members.to_json("data.members.json")
```

### Opening IVT Files

#### From Path

```python
import pyivt

ivt = pyivt.open("data.ivt")
```

#### From URL

```python
import requests

import pyivt

url = "https://example.com/data.ivt"
res = requests.get(url)
res.raise_for_status()

ivt = pyivt.open(res.content)
```

### Reading Metadata

`ivt.metadata` is a typed `IvtMetadata` object. Summary fields are eager; heavy fields load on first access.

| Beyond 20/20 Summary | `pyivt` Metadata                                                             |
| -------------------- | ---------------------------------------------------------------------------- |
| Title                | `title.en`, `title.fr`                                                       |
| Author               | `author.en`, `author.fr`                                                     |
| Category             | `category.en`, `category.fr`                                                 |
| Keywords             | `keywords.en`, `keywords.fr`                                                 |
| Notes                | `notes.en`, `notes.fr`                                                       |
| Number of Datapoints | `number_of_datapoints`                                                       |
| Time Series          | `frequency`, `start_date`, `end_date` (each `None` if absent)                |
| Read-Only            | `read_only`                                                                  |
| Dimensions           | `dimensions` (`name.en` / `name.fr`, `description.en` / `description.fr`, …) |
| Geographies          | `geographies`                                                                |
| Footnotes            | `footnotes`                                                                  |
| Missing Values       | `missing_values`                                                             |

Localized fields support both attribute and language-key access:

```python
meta = ivt.metadata
print(meta.title.en, meta.title["fr"])
for dimension in meta.dimensions:
    print(dimension.name.en, dimension.description["fr"])
    print(dimension.members.en, dimension.notes["fr"])
print(meta.geographies.geo_label.en, meta.geographies.geo_label["fr"])
```

Each translation is `None` when unavailable. Blank authors, categories,
keywords, and table or dimension notes also return `None`. Unsupported language
keys raise `KeyError`.
Keyword strings stored separately in the file are joined with `", "` in file
order; `meta.keywords.en` and `.fr` remain optional strings.

`dimension.type` is a typed string with three possible values: `"geographic"`,
`"normal"`, and `"time_series"`. The `DimensionType` alias is available for your
own annotations:

```python
from pyivt import DimensionType

kind: DimensionType = "time_series"
time_axes = [dimension for dimension in meta.dimensions if dimension.type == kind]
```

`meta.frequency` uses the `Frequency` literal enum: `"monthly"`, `"quarterly"`,
`"semi_annual"`, or `"annual"`. It returns `None` when Time Series is No or the
stored rate is unknown. `dimension.items` gives the declared number of members
on each axis; `dimension.count` remains an alias for the same value.

Each footnote contains both translations in `description`. Missing-value entries
use the same field name:

```python
for footnote in meta.footnotes:
    print(footnote.symbol, footnote.description.en, footnote.description["fr"])

for missing in meta.missing_values:
    print(missing.code, missing.symbol, missing.description.en)
```

Footnotes follow legend order and appear once per entry. A missing translation
is `None`. Numbered footnotes embedded in table or dimension notes remain in
those notes.

### Editor Support

The package includes type stubs, read-only property definitions, and Markdown
hover documentation with Python snippets. Select the Python environment where
`pyivt` is installed to get completions for metadata fields and the allowed
`dimension.type` and `frequency` values. The examples above show localized-field
access and optional-value handling; see [the changelog](CHANGELOG.md) for migration notes.

### Exporting Data

`metadata.descriptor_path` reports `"axis_matrix"`, the native format reader.
Files without a valid master axis matrix raise `ValueError`; the former
unverified descriptor scanner is disabled instead of returning guessed counts.
Axis kinds and stored language references determine types and translations.
Geography comes from an explicit native kind or UID member field; tables without
either have `layout.geo_dim is None`. `metadata.geographies.geo_uid` preserves
native DGUID values. Product identity is read within native summary/notes records,
without searching unrelated header bytes for plausible identifiers.

`Ivt.to_arrow()` returns `(table, members)`:

```python
table, members = ivt.to_arrow()
```

The full table has one row per combination of dimension members, in file
order with the last dimension varying fastest. Each dimension has exactly one
`dictionary<uint8|uint16|uint32|uint64, utf8>` column containing its display
labels, plus a nullable `float64` `value` column. The dictionary uses native
member order; its index type is the smallest unsigned type that holds all
zero-based indices (256 members still fit `uint8`). Labels can repeat or be
missing; the original member identity is retained by dictionary position.
Missing-value records determine value nulls independently of numeric payloads.
Published zero values remain zero. Absent storage cells retain the native
zero/published default. Flat and paged directories use the same strict reader.

Member codes in this API are **native numeric ordinals**, currently 1-based.
Their full map is checked for non-negative, unique, sorted, dense codes and
records `first_code`. Stored text codes such as `"001"`, both languages, dates,
notes and numeric attributes live in member metadata without row repetition.

#### Select members and columns before materializing

```python
# Keys are dimension slugs from ivt.layout.slugs, values are numeric ordinals.
select = {ivt.layout.slugs[0]: [1, 3]}
table, members = ivt.to_arrow(select)
```

Codes are validated and sorted into native order. Duplicate, non-integer or
unknown codes raise `ValueError` naming the dimension. Each specified axis is
restricted to its selected codes. An omitted dimension keeps all its rows,
but contributes no output column or members entry. There is no aggregation.
`None` selects every member and column; `{}` returns only `value` for the full
grid. An empty code list produces a zero-row table with an empty dictionary.
Selected dictionaries contain only selected members. Dictionary index `i`
corresponds to `members.dimensions[axis].members[i].code`, including subsets
with gaps. Selection runs before allocating row buffers or reading data pages.

#### Typed member maps and JSON

```python
# Same selection, without decoding cells:
assert ivt.members(select) == members

for dimension in members.dimensions:
    print(dimension.name, dimension.label, dimension.first_code)
    for member in dimension.members:
        print(member.code, member.label, member.metadata)

members.to_json("data.members.json")
restored = pyivt.Members.read_json("data.members.json")
restored.validate_table(table)
# Members.from_json(text) also reads JSON strings.
```

`Members`, `DimensionMembers` and `Member` are immutable typed objects.
Metadata maps retain native field names and JSON scalar types; dates are ISO
strings and unavailable values are `null`. JSON includes `schema_version: 1`,
a `table_id` (SHA-256 of the complete source bytes), `selection_id` and the
dimension/member maps. The Arrow schema stores matching `pyivt.table_id`,
`pyivt.selection_id` and `pyivt.members_schema_version` keys. Loading altered
JSON or validating a different source, selection or dictionary raises
`ValueError`. Keep the JSON alongside the Arrow IPC or Parquet artifact;
format conversions may change Arrow index types.

#### pandas and Polars

```python
import polars as pl

frame = pl.from_arrow(table)  # dimension labels become categoricals
```

```python
# Requires pandas. Handles duplicate and missing labels by remapping keys,
# without constructing one Python string per row.
frame = members.to_pandas(table)
```

For unique, non-null labels, `table.to_pandas()` also yields categoricals.
pandas merges equal labels into one category; retain `members` and the original
Arrow dictionary indices when distinct native member codes matter.

```python
import pyarrow.parquet as pq

pq.write_table(table, "data.parquet")
members.to_json("data.members.json")
```

The most recent equivalent export shares cached Arrow buffers. A different
selection releases the reader's previous cached table. `to_arrow()` raises
`MemoryError` when its allocation estimate exceeds `max_memory_bytes` (default
2 GiB). Estimates account for final row buffers, member maps, source bytes and
decoder scratch; they do not enforce a process memory limit or include other
consumer-held tables. The GIL is released for native cell decoding and column
construction, which run concurrently.

#### Detailed streaming exports

The existing `to_batches()` and Arrow C stream APIs keep their **detailed
schema**: native member IDs, every member field, `value`, `status_code`,
`symbol`, `status` and compact `footnote_raw` lists. This preserves access to
per-cell statuses and annotations, which are absent from the compact table.
These APIs return batches directly, without a members tuple; compact streaming
and enum code generation are outside this change's scope.

```python
batches = ivt.to_batches(batch_size=1_000_000)
first = next(batches)
with pq.ParquetWriter("detailed.parquet", first.schema) as writer:
    writer.write_batch(first)
    for batch in batches:
        writer.write_batch(batch)
```

Streaming text fields use shared `dictionary<int32, utf8>` values and member
IDs use `int32`. Batches default to a 1 GiB estimate limit. `pa.table(ivt)`
consumes this detailed stream, as does `pa.RecordBatchReader.from_stream(ivt)`
on recent PyArrow versions. For the compact table, unpack `ivt.to_arrow()`.

See [the compact export measurements](docs/benchmarks/compact-arrow.md) for the
80M-row synthetic comparison and a command to profile private files locally.

## Benchmarks

The following historical measurements predate the compact 0.1.0 schema. Compared with `canivt` across 20 IVT files, `pyivt` achieved a median per-file speedup of **69.2× for metadata extraction** and **29.1× for Arrow conversion**.

| Operation        | Library | Wall time (ms) | CPU time (ms) | Peak memory (MiB) | Memory growth (MiB) |
| ---------------- | ------- | -------------: | ------------: | ----------------: | ------------------: |
| Metadata         | canivt  |          810.0 |         800.0 |             179.5 |               107.8 |
| Metadata         | pyivt   |            9.1 |          15.6 |              22.7 |                 2.7 |
| Arrow conversion | canivt  |          565.0 |         565.0 |             223.6 |               120.0 |
| Arrow conversion | pyivt   |           24.5 |          23.4 |              38.0 |                 7.6 |

Table values are medians across the 20 files, using each file’s median trial result. Speedups are calculated per file before taking the median.

`pyivt` was faster on all 20 metadata cases and 19 of 20 Arrow cases. Peak memory was lower on all metadata cases and 17 of 20 Arrow cases. In the remaining Arrow case, `pyivt` took 710 ms versus 360 ms for `canivt`.

### Methodology

Each case runs three times in fresh worker processes. Timings exclude runtime startup and imports, but include the first operation’s initialization costs. Metadata extraction loads all lazy metadata fields; Arrow conversion materializes the complete grid, including missing cells, without writing to disk. The libraries produce different schemas: `pyivt` preserves additional member fields and raw annotations.

Memory is sampled resident memory (RSS), including native allocations. Peak memory includes the runtime; growth measures the increase above the baseline after imports. Sampling can miss brief allocation peaks. CPU time includes user and system time across threads; Windows timer resolution limits accuracy for short operations.

### Running the benchmarks

Tests automatically discover IVT files under `data`, including subdirectories.

```bash
uv sync --group benchmark
uv run maturin develop --release --skip-install
uv run --group benchmark pytest benchmarks --ivt-report .cache/benchmarks/results.json
```

The `canivt` backend embeds R through `rpy2` in each Python worker. Install R
and the R packages `canivt` and `arrow`; set `R_HOME` if R is not discovered.
Use `--ivt-r-library PATH` to add an R package library (the default also searches
the cache created by `scripts/generate_expected_parquet.py`). To benchmark only
Python, pass `--ivt-backends pyivt`.

The published results above used the previous standalone R worker. Rerun the
benchmarks to compare memory usage with the embedded R runtime.

## Tests

CI downloads three Census samples with committed JSON metadata references.
Parquet references are generated locally and are not committed; comparisons
against them are skipped when they are unavailable. Arrow export and round-trip
tests still run without these references.

Mandatory offline Arrow regressions additionally compare a committed paged IVT
against all 1,155 cells of an official CSV and check a hand-authored fixture's
complete typed output, including dates, missing statuses and raw annotations.
See [fixture provenance](tests/fixtures/README.md) and
[native format evidence](docs/investigations/ivt-native-directory.md).

To validate the original NAICS234 report with a complete independent Parquet
export, set `PYIVT_REGRESSION_NAICS234_PATH` and
`PYIVT_REGRESSION_NAICS234_EXPECTED_PARQUET`, then run
`pytest tests/test_arrow_regression.py`. The oracle must be produced independently
and use matching column names, types and row order. A requested regression with
a missing file or oracle fails; otherwise this optional case is skipped.

1. Populate `data` with test inputs

   Put any number of `.ivt` files in `data`, or use the default dataset using:

   ```bash
   uv run --no-project scripts/copy_test_inputs.py
   ```

2. Generate expected test outputs

   ```bash
   uv run --no-project scripts/generate_expected_metadata.py
   ```

   ```bash
   uv run --no-project scripts/generate_expected_parquet.py
   ```

3. Run tests

   ```bash
   pytest
   ```

To clear all files and subdirectories in `data` and `tests/expected`, including
the committed JSON references, run:

```bash
uv run --no-project scripts/cleanup.py
```

The folders themselves are preserved. Committed references can be restored with
`git restore tests/expected`.

## License

`pyivt` is licensed under the [MIT License](LICENSE).

## Acknowledgements

- [canivt](https://github.com/mountainMath/canivt)
- [REA: Reverse Engineer Anything](https://github.com/morluto/rea)
