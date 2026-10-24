"""Read Beyond 20/20 IVT metadata and export complete tables to Apache Arrow.

Call open(source) with a local path, IVT bytes, or a binary file object.
Inspect the returned Ivt.metadata and Ivt.layout, then call Ivt.to_arrow()
to decode the complete member grid. Detailed metadata fields load on demand.
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

__all__ = [
    "Dimension",
    "Footnote",
    "Geographies",
    "Ivt",
    "IvtMetadata",
    "Layout",
    "LocalizedLabels",
    "LocalizedText",
    "MissingValue",
    "open",
]

__version__: str
