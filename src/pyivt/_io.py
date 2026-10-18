"""Normalize IVT byte buffers."""

from __future__ import annotations


def as_bytes(data: bytes | bytearray | memoryview) -> bytes:
    """
    Coerce a bytes-like buffer to ``bytes``.

    Paths, URLs, and file-like objects are not accepted; callers should read
    them into a buffer first.
    """
    if isinstance(data, (bytes, bytearray, memoryview)):
        return bytes(data)
    raise TypeError(
        "IVT data must be bytes, bytearray, or memoryview; "
        f"got {type(data).__name__}"
    )
