"""Read Beyond 20/20 IVT metadata, member lookups, and Arrow tables."""

from pyivt._errors import (
    CorruptFileError,
    IvtError,
    ResourceLimitError,
    UnsupportedFormatError,
)
from pyivt._native import (
    Dimension,
    Footnote,
    Geographies,
    Ivt,
    IvtMetadata,
    LocalizedLabels,
    LocalizedText,
    MissingValue,
)
from pyivt._open import open

__all__ = [
    "CorruptFileError",
    "Dimension",
    "Footnote",
    "Geographies",
    "Ivt",
    "IvtError",
    "IvtMetadata",
    "LocalizedLabels",
    "LocalizedText",
    "MissingValue",
    "ResourceLimitError",
    "UnsupportedFormatError",
    "open",
]

__version__ = "0.0.10"
