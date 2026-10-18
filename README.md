# pyivt

`pyivt` is a library for Statistics Canada Beyond 20/20 `.ivt` files, written in Rust. It is design to be minimal and easily adaptable for existing codebases. Key features are:

- **Fast**: Up to 100,000% faster than GUI automation for metadata
- **Python**: Easily integrates into modern data workflows with a minimal API
- **Typed**: Produces fully type-safe and predictable metadata
- **Interoperable**: Exports `.ivt` file data to Apache Arrow

## Installation

Install the latest pyivt version with:

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

`pyivt.open` accepts a path, bytes-like buffer, or binary file object. Summary metadata and layout are parsed immediately; cell values decode only when you call `to_arrow()`.

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

`geographies` is a `Geographies` object (`member_id`, `geo_label`, `geo_uid`, …), not a dict.

```python
import pyivt

ivt = pyivt.open("data.ivt")
print(ivt.metadata.product_id, ivt.metadata.title.en)
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

`pyivt` is licensed under the [MIT License](LICENSE).

## Acknowledgements

- [canivt](https://github.com/mountainMath/canivt)
- [REA: Reverse Engineer Anything](https://github.com/morluto/rea)
