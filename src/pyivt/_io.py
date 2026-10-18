"""Normalize IVT byte buffers and source inputs."""

from __future__ import annotations

from pathlib import Path
from typing import BinaryIO

Source = str | Path | bytes | bytearray | memoryview | BinaryIO


def as_bytes(data: bytes | bytearray | memoryview) -> bytes:
    """
    Coerce a bytes-like buffer to ``bytes``.

    Already-``bytes`` inputs are returned as-is (no copy).
    """
    if isinstance(data, bytes):
        return data
    if isinstance(data, (bytearray, memoryview)):
        return bytes(data)
    raise TypeError(
        "IVT data must be bytes, bytearray, or memoryview; "
        f"got {type(data).__name__}"
    )


def read_source(source: Source) -> bytes:
    """
    Load IVT bytes from a path, buffer, or binary file object.
    """
    if isinstance(source, (bytes, bytearray, memoryview)):
        return as_bytes(source)
    if isinstance(source, (str, Path)):
        return Path(source).read_bytes()
    read = getattr(source, "read", None)
    if callable(read):
        data = read()
        if not isinstance(data, (bytes, bytearray, memoryview)):
            raise TypeError(
                "file-like source must return bytes; "
                f"got {type(data).__name__}"
            )
        return as_bytes(data)
    raise TypeError(
        "IVT source must be a path, bytes-like, or binary file object; "
        f"got {type(source).__name__}"
    )
