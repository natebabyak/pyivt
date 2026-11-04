"""Read Beyond 20/20 IVT metadata and export complete tables to Apache Arrow.

Use `open(source)` with a local path, IVT bytes, or a binary file object.
Inspect `metadata` and `layout` without decoding cells. Detailed metadata
fields load on demand.

```python
import pyivt

ivt = pyivt.open("table.ivt")
title = ivt.metadata.title.en or ivt.metadata.title.fr
for note in ivt.metadata.footnotes:
    print(note.symbol, note.description.en)
table = ivt.to_arrow()
members = ivt.to_member_tables()
```

Metadata objects expose read-only attributes. Missing optional values and
translations are represented by `None`. Use `to_arrow()` for a compact
table, `to_member_tables()` for typed dimension lookups, or the Arrow C stream
for rows with statuses and footnotes.
"""

from pyivt._native import (
    Dimension,
    Footnote,
    Geographies,
    Ivt,
    IvtMetadata,
    Layout,
    LocalizedLabels,
    LocalizedText,
    MissingValue,
    open,
)
from pyivt._selection import Selection
from pyivt._types import DimensionType, Frequency

__all__ = [
    "Dimension",
    "DimensionType",
    "Footnote",
    "Frequency",
    "Geographies",
    "Ivt",
    "IvtMetadata",
    "Layout",
    "LocalizedLabels",
    "LocalizedText",
    "MissingValue",
    "Selection",
    "open",
]

__version__ = "0.0.7"
