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
members = ivt.members()
```

Metadata objects expose read-only attributes. Missing optional values and
translations are represented by `None`. Use `to_arrow()` for a compact
table, members() for localized labels, or the Arrow C stream for rows with statuses
and footnotes.
"""

from pyivt._codegen import codegen as codegen
from pyivt._members import DimensionMembers as DimensionMembers
from pyivt._members import Language as Language
from pyivt._members import Member as Member
from pyivt._members import Members as Members
from pyivt._members import MemberValue as MemberValue
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
from pyivt._selection import Selection as Selection
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
    "Language",
    "Layout",
    "LocalizedLabels",
    "LocalizedText",
    "Member",
    "MemberValue",
    "Members",
    "MissingValue",
    "Selection",
    "codegen",
    "open",
]

__version__: str
