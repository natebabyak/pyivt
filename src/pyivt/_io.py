"""Load IVT bytes from paths, URLs, buffers, or file-like objects."""

from __future__ import annotations

import os
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import BinaryIO
from urllib.parse import urlparse
from urllib.request import urlopen

PathLike = str | Path
Source = PathLike | bytes | bytearray | memoryview | BinaryIO


@dataclass(frozen=True)
class SourceOrigin:
    file_name: str | None
    directory: str | None
    last_update: datetime | None
    file_size: int


def _is_url(value: str) -> bool:
    return value.startswith(("http://", "https://"))


def _from_path(path: Path) -> tuple[bytes, SourceOrigin]:
    data = path.read_bytes()
    st = path.stat()
    return data, SourceOrigin(
        file_name=path.name,
        directory=str(path.resolve().parent) + os.sep,
        last_update=datetime.fromtimestamp(st.st_mtime, tz=timezone.utc),
        file_size=st.st_size,
    )


def _from_url(url: str) -> tuple[bytes, SourceOrigin]:
    with urlopen(url) as resp:
        data = resp.read()
    name = Path(urlparse(url).path).name or None
    return data, SourceOrigin(
        file_name=name,
        directory=None,
        last_update=None,
        file_size=len(data),
    )


def _from_buffer(data: bytes) -> tuple[bytes, SourceOrigin]:
    return data, SourceOrigin(
        file_name=None,
        directory=None,
        last_update=None,
        file_size=len(data),
    )


def load_source(source: Source) -> tuple[bytes, SourceOrigin]:
    """
    Normalize an IVT source to ``(bytes, origin)``.

    Accepts a local path (``str`` / ``Path``), ``http(s)`` URL, ``bytes``-like
    buffer, or binary file-like object with a ``.read()`` method.
    """
    if isinstance(source, (bytes, bytearray, memoryview)):
        return _from_buffer(bytes(source))

    if isinstance(source, Path):
        return _from_path(source)

    if isinstance(source, str):
        if _is_url(source):
            return _from_url(source)
        return _from_path(Path(source))

    read = getattr(source, "read", None)
    if callable(read):
        raw = read()
        if isinstance(raw, str):
            raise TypeError("file-like IVT source must be opened in binary mode")
        if not isinstance(raw, (bytes, bytearray, memoryview)):
            raise TypeError(
                f"file-like .read() returned {type(raw).__name__}, expected bytes"
            )
        return _from_buffer(bytes(raw))

    raise TypeError(
        "IVT source must be a path, URL, bytes, or binary file-like object; "
        f"got {type(source).__name__}"
    )
