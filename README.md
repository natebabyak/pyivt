# pyivt

Python library for Beyond 20/20 `.ivt` files.

- Up to 100,000% faster than GUI automation for metadata
- Export data to Apache Arrow (then pandas, Parquet, Polars, or anything Arrow supports)

## Getting Started

### Prerequisites

- Python 3.11+

### Installation

1. Install `pyivt` using pip

   ```bash
   pip install pyivt
   ```

## Usage

`pyivt` is minimal by design: typed metadata for indexes, Arrow for data export.

### Quick Start

Open an IVT file, read its metadata, and write its data to Parquet:

```python
import pyarrow.parquet as pq
import pyivt

meta = pyivt.read_metadata("data.ivt")
print(meta.title.en)

ivt = pyivt.open("data.ivt")
pq.write_table(ivt.to_arrow(), "data.parquet")
```

### Opening IVT Files

Use `pyivt.open` for paths (or file objects). Use `pyivt.from_bytes` for in-memory buffers.

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

ivt = pyivt.from_bytes(res.content)
```

### Reading Metadata

`read_metadata` returns a typed `IvtMetadata` object. Summary fields are eager; heavy fields load on first access.

| Beyond 20/20 Summary | `pyivt` Metadata             |
| -------------------- | ---------------------------- |
| Title                | `title.en`, `title.fr`       |
| Author               | `author.en`, `author.fr`     |
| Category             | `category.en`, `category.fr` |
| Keywords             | `keywords.en`, `keywords.fr` |
| Notes                | `notes.en`, `notes.fr`       |
| Number of Datapoints | `number_of_datapoints`       |
| Time Series          | `time_series`                |
| Read-Only            | `read_only`                  |
| Dimensions           | `dimensions`                 |
| Geographies          | `geographies`                |
| Footnotes            | `footnotes`                  |
| Missing Values       | `missing_values`             |

```python
import pyivt

meta = pyivt.read_metadata("data.ivt")
print(meta.product_id, meta.title.en)
```

### Exporting Data

`Ivt.to_arrow()` returns an Apache Arrow table. Convert from there:

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

## License

MIT

## Acknowledgements

- [canivt](https://github.com/mountainMath/canivt)
- [REA: Reverse Engineer Anything](https://github.com/morluto/rea)
