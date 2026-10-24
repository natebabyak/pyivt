"""Read Beyond 20/20 IVT metadata and export complete tables to Apache Arrow.

Use ``open(source)`` with a local path, IVT bytes, or a binary file object.
The returned ``Ivt`` exposes ``metadata`` and ``layout`` immediately and defers
cell decoding until ``to_arrow()``. Detailed metadata fields load on demand.

Examples
--------
>>> import pyivt
>>> ivt = pyivt.open("table.ivt")
>>> title = ivt.metadata.title.en or ivt.metadata.title.fr
>>> table = ivt.to_arrow()

Metadata objects expose read-only attributes. Missing optional values and
translations are represented by ``None``. Arrow exports contain the full
member grid, typed member fields, numeric values, statuses and raw annotations.
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

__version__ = "0.0.1"
