# pyivt

Python library for Beyond 20/20 `.ivt` files.

- Up to 100,000% faster than GUI automation for metadata
- Export data to Apache Arrow (then pandas, Parquet, Polars, or anything Arrow supports)

## Getting Started

### Installation

Install `pyivt` using pip:

```bash
pip install pyivt
```

### Usage

#### Quick Start

Open an IVT file, inspect its metadata, and export its data to Parquet:

```python
from pathlib import Path

import pyarrow.parquet as pq
import pyivt

path = Path("data.ivt")
ivt = pyivt.open(path.read_bytes())

print(ivt.metadata.title.en)

pq.write_table(ivt.to_arrow(), "data.parquet")
```

#### Opening IVT Files

`pyivt.open` accepts IVT bytes only. Convert paths, URLs, or file objects first:

```python
import os
from pathlib import Path
from urllib.request import urlopen

import pyivt

# Local file
path = Path("data.ivt")
ivt = pyivt.open(path.read_bytes())

# Optional Beyond 20/20 summary fields from the filesystem
ivt = pyivt.open(
    path.read_bytes(),
    file_name=path.name,
    directory=str(path.resolve().parent) + os.sep,
)

# Remote URL
with urlopen("https://example.com/data.ivt") as resp:
    ivt = pyivt.open(resp.read())

# File-like object
with open("data.ivt", "rb") as file:
    ivt = pyivt.open(file.read())
```

#### Exporting Data

`IvtTable.to_arrow()` returns an Apache Arrow table. Convert from there:

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

## Beyond 20/20 Summary vs. `pyivt` Metadata

| Beyond 20/20 Summary | `pyivt` Metadata             |
| -------------------- | ---------------------------- |
| Title[^1]            | `title.en`, `title.fr`       |
| File Name            | `file_name`                  |
| Directory            | `directory`                  |
| Author               | `author.en`, `author.fr`     |
| Category             | `category.en`, `category.fr` |
| Keywords             | `keywords.en`, `keywords.fr` |
| Notes                | `notes.en`, `notes.fr`       |
| Last Update          | `last_update`                |
| File Size            | `file_size`                  |
| Number of Datapoints | `number_of_datapoints`       |
| Time Series          | `time_series`                |
| Read-Only            | `read_only`                  |
| Dimensions           | `dimensions`                 |
| Footnotes            | `footnotes`                  |
| Missing Values       | `missing_values`             |

## License

MIT

## Acknowledgements

- [canivt](https://github.com/mountainMath/canivt)
- [REA: Reverse Engineer Anything](https://github.com/morluto/rea)
