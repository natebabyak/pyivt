# pyivt

`pyivt` is a library for Statistics Canada Beyond 20/20 `.ivt` files, written in Rust. It is designed to be minimal and easily adaptable for existing codebases. Key features are:

> [!NOTE]
> `pyivt` is under active development and is not considered stable until v0.1.0

- **Fast**: Orders of magnitude faster than GUI automation for metadata
- **Python**: Easily integrates into modern data workflows with a minimal API
- **Typed**: Produces fully type-safe and predictable metadata
- **Interoperable**: Exports `.ivt` file data to Apache Arrow

## Installation

```bash
uv add pyivt
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

| Beyond 20/20 Summary | `pyivt` Metadata                                         |
| -------------------- | -------------------------------------------------------- |
| Title                | `title.en`, `title.fr`                                   |
| Author               | `author.en`, `author.fr`                                 |
| Category             | `category.en`, `category.fr`                             |
| Keywords             | `keywords.en`, `keywords.fr`                             |
| Notes                | `notes.en`, `notes.fr`                                   |
| Number of Datapoints | `number_of_datapoints`                                   |
| Time Series          | `frequency`, `start_date`, `end_date` (each `None` if absent) |
| Read-Only            | `read_only`                                              |
| Dimensions           | `dimensions` (`name_en` / `name_fr`, `description_en` / `description_fr`, …) |
| Geographies          | `geographies`                                            |
| Footnotes            | `footnotes`                                              |
| Missing Values       | `missing_values`                                         |

`geographies` is a `Geographies` object (`member_id`, `geo_label`, `geo_uid`, …), not a dict.

```python
import pyivt

ivt = pyivt.open("data.ivt")
print(ivt.metadata.product_id, ivt.metadata.title.en)
```

### Exporting Data

`Ivt.to_arrow()` returns an Apache Arrow table. Keyword arguments:

| Argument    | Default       | Meaning                                                              |
| ----------- | ------------- | -------------------------------------------------------------------- |
| `complete`  | `True`        | Expand the published grid; set `False` for sparse decoded cells only |
| `max_cells` | `100_000_000` | Raise if the complete grid would exceed this many cells              |
| `labels`    | `True`        | Include dimension label columns                                      |
| `language`  | `"en"`        | Label language (`"en"` or `"fr"`)                                    |

```python
import polars as pl
import pyarrow.parquet as pq

table = ivt.to_arrow()

# pandas
df = table.to_pandas()

# Parquet
pq.write_table(table, "data.parquet")

# Polars
df = pl.from_arrow(table)
```

## Development

Local setup uses [uv](https://docs.astral.sh/uv/). From the repo root:

```bash
uv sync
uv run maturin develop --uv
uv run pytest
```

`uv sync` installs the package editable plus the default `dev` group (`pytest`, `ruff`, `maturin`). Use `maturin develop --uv` so the Rust extension rebuilds against the uv-managed env. For the Beyond File Summary expected-value script:

```bash
uv sync --group beyond
uv run python tests/scripts/beyond_summary_expected.py --suite demo
```

## License

`pyivt` is licensed under the [MIT License](LICENSE).

## Acknowledgements

- [canivt](https://github.com/mountainMath/canivt)
- [REA: Reverse Engineer Anything](https://github.com/morluto/rea)
