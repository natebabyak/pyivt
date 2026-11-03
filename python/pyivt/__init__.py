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
table, members = ivt.to_arrow()
```

Metadata objects expose read-only attributes. Missing optional values and
translations are represented by `None`. Use `to_arrow()` for a compact
table and member maps, or the Arrow C stream for detailed rows with statuses
and footnotes.
"""

from pyivt._members import DimensionMembers, Member, Members, MemberValue
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
from pyivt._types import DimensionType, Frequency

__all__ = [
    "Dimension",
    "DimensionMembers",
    "DimensionType",
    "Footnote",
    "Frequency",
    "Geographies",
    "Ivt",
    "IvtMetadata",
    "Layout",
    "LocalizedLabels",
    "LocalizedText",
    "Member",
    "MemberValue",
    "Members",
    "MissingValue",
    "open",
]

__version__ = "0.0.6"
