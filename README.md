# pyivt

Minimal Python library for Statistics Canada **Beyond 20/20** `.ivt` tables:

- extract **file summary** and **dimension** metadata (members, geographies, footnotes)
- decode cell values into a tidy grid
- write **Parquet** (plus a `_members` sidecar) and metadata CSVs

Byte layout and decode geometry follow the reverse-engineered Beyond 20/20 format documented by [canivt](https://github.com/mountainMath/canivt). pyivt is an independent Python implementation, not an R binding.

Beyond 20/20 Professional Browser (`ProBrowser.exe` installer → `Brwpro.exe`) is useful for visual ground truth (File Summary / dimension item lists). This library reads the `.ivt` bytes directly and does not automate the GUI.

## Getting Started

### Installation

```bash
pip install pyivt
```

### Usage

#### Quick Start

```python
import pyivt

ivt = pyivt.open("data.ivt")

ivt.to_parquet("data.parquet")
```

#### Opening `.ivt` Files

##### By Path

```python
import pyivt

ivt = pyivt.open("data.ivt")
```

or

```python
from pathlib import Path

import pyivt

ivt = pyivt.open(Path("data.ivt"))
```

##### By URL

```python
import pyivt

ivt = pyivt.open("https://example.com/data.ivt")
```

##### By Bytes

```python
import pyivt

ivt = pyivt.open(bytes)
```

or

```python
from io import BytesIO

import pyivt

ivt = pyivt.open(BytesIO(bytes))
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
