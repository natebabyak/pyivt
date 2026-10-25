# pyivt

`pyivt` is a library for Statistics Canada Beyond 20/20 `.ivt` files, written in Rust. It is designed to be minimal and easily adaptable for existing codebases. Key features are:

> [!NOTE]
> `pyivt` is under active development and is not considered stable until v0.1.0

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
pq.write_table(ivt.to_arrow(), "data.parquet")
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

Each translation is `None` when unavailable. Unsupported language keys raise
`KeyError`. Dimension `name`, `description`, `members`, and `notes` replace the
previous `_en` / `_fr` fields; `geographies.geo_label` now holds both languages.

### Exporting Data

Use `Ivt.to_arrow()` to export to Apache Arrow.

Apache Arrow is ready to use with your current data stack.

- `pandas`:

  ```python
  import pandas as pd
  import pyivt

  ivt = pyivt.open("example.ivt")

  tab = ivt.to_arrow()

  df = pd.DataFrame.from_arrow(tab)
  print(df)
  ```

- `polars`:

  ```python
  import polars as pl
  import pyivt

  ivt = pyivt.open("example.ivt")

  tab = ivt.to_arrow()

  df = pl.from_arrow(tab)
  print(df)
  ```

- `pyarrow`:

  ```python
  import pyarrow.parquet as pq
  import pyivt

  ivt = pyivt.open("example.ivt")

  tab = ivt.to_arrow()

  pq.write_table(tab, "example.parquet")
  ```

## Benchmarks

Compared with `canivt` across 20 IVT files, `pyivt` achieved a median per-file speedup of **69.2× for metadata extraction** and **29.1× for Arrow conversion**.

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

````bash
uv sync --group benchmark
uv run maturin develop --release --skip-install
uv run --group benchmark pytest benchmarks --ivt-report .cache/benchmarks/results.json


## Tests

CI downloads three Census samples with committed JSON metadata references.
Parquet references are generated locally and are not committed; comparisons
against them are skipped when they are unavailable. Arrow export and round-trip
tests still run without these references.

1. Populate `data` with test inputs

   Put any number of `.ivt` files in `data`, or use the default dataset using:

   ```bash
   uv run --no-project scripts/copy_test_inputs.py
````

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
