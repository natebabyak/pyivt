# pyivt

Python library for Beyond 20/20 `.ivt` files.

- Up to 100,000% faster than GUI automation for metadata
- Export data to Apache Arrow, pandas, Parquet, or Polars

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
import pyivt

ivt = pyivt.open("data.ivt")

print(ivt.metadata.title.en)

ivt.to_parquet("data.parquet")
```

#### Opening IVT Files

##### Local files

```python
from pathlib import Path

import pyivt

ivt = pyivt.open("data.ivt")
ivt = pyivt.open(Path("data.ivt"))
```

##### Remote URLs

```python
import pyivt

ivt = pyivt.open("https://example.com/data.ivt")
```

##### Bytes and file-like objects

```python
from io import BytesIO

import pyivt

ivt = pyivt.open(data)
ivt = pyivt.open(BytesIO(data))

with open("data.ivt", "rb") as file:
    ivt = pyivt.open(file)
```

#### Exporting Data

```python
# Apache Arrow
table = ivt.to_arrow()

# pandas
df = ivt.to_pandas()

# Parquet
ivt.to_parquet("data.parquet")

# Polars
df = ivt.to_polars()
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
