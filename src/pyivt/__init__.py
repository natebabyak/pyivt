"""Minimal Beyond 20/20 IVT reader: metadata + tidy Parquet export."""

from .metadata import read_metadata
from .parquet import write_metadata, write_parquet
from .reader import IvtTable, read_table

__all__ = [
    "IvtTable",
    "read_metadata",
    "read_table",
    "write_metadata",
    "write_parquet",
]

__version__ = "0.1.0"
