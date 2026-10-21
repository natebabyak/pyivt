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
| Dimensions           | `dimensions` (`name_en` / `name_fr`, `description_en` / `description_fr`, …) |
| Geographies          | `geographies`                                                                |
| Footnotes            | `footnotes`                                                                  |
| Missing Values       | `missing_values`                                                             |

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

## License

`pyivt` is licensed under the [MIT License](LICENSE).

## Acknowledgements

- [canivt](https://github.com/mountainMath/canivt)
- [REA: Reverse Engineer Anything](https://github.com/morluto/rea)
