"""Beyond 20/20 IVT reader: metadata + Apache Arrow export."""

from .metadata import read_metadata
from .reader import IvtTable, from_bytes

__all__ = [
    "IvtTable",
    "from_bytes",
    "read_metadata",
]

__version__ = "0.1.0"
