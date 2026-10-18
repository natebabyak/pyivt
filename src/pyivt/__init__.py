"""Beyond 20/20 IVT reader: metadata + Apache Arrow export."""

from .metadata import (
    Dimension,
    Footnote,
    IvtMetadata,
    LocalizedText,
    MissingValue,
    read_metadata,
)
from .reader import Ivt, from_bytes, open

__all__ = [
    "Dimension",
    "Footnote",
    "Ivt",
    "IvtMetadata",
    "LocalizedText",
    "MissingValue",
    "from_bytes",
    "open",
    "read_metadata",
]

__version__ = "0.1.0"
