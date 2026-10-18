"""Beyond 20/20 IVT reader: metadata + Apache Arrow export."""

from .metadata import (
    Dimension,
    Footnote,
    IvtMetadata,
    LocalizedText,
    MissingValue,
)
from .reader import Ivt, open

__all__ = [
    "Dimension",
    "Footnote",
    "Ivt",
    "IvtMetadata",
    "LocalizedText",
    "MissingValue",
    "open",
]

__version__ = "0.1.0"
