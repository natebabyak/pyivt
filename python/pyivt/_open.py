"""Open IVT sources and ZIP downloads through the same native reader."""

from __future__ import annotations

import builtins
from io import BytesIO
from os import PathLike, fspath
from pathlib import Path
from shutil import copyfileobj
from tempfile import TemporaryDirectory
from typing import BinaryIO, cast
from zipfile import BadZipFile, ZipFile

from pyivt import _native


def _archive(archive: ZipFile, member: str | None) -> _native.Ivt:
    candidates = [
        info
        for info in archive.infolist()
        if not info.is_dir() and info.filename.lower().endswith(".ivt")
    ]
    if member is not None:
        candidates = [info for info in candidates if info.filename == member]
        if len(candidates) != 1:
            raise ValueError(
                f"ZIP must contain exactly one IVT member named {member!r}"
            )
    elif len(candidates) != 1:
        raise ValueError(
            f"ZIP contains {len(candidates)} IVT members; "
            "use member= with an exact member name when there are several"
        )
    owner = TemporaryDirectory(prefix="pyivt-")
    try:
        path = Path(owner.name) / "table.ivt"
        with archive.open(candidates[0]) as source, path.open("wb") as target:
            copyfileobj(source, target, length=1024 * 1024)
        return _native.open(path, _source_owner=owner)
    except BaseException:
        owner.cleanup()
        raise


def open(
    source: str | PathLike[str] | bytes | bytearray | memoryview | BinaryIO,
    *,
    member: str | None = None,
) -> _native.Ivt:
    """Open IVT contents or a ZIP containing an IVT table.

    Sources may be paths, bytes, bytes-like objects or binary file objects.
    File objects are read once from their current position and stay open.
    A ZIP with one IVT member selects it automatically. For several members,
    pass the exact, case-sensitive archive name as ``member=``.

    ZIP members are streamed to a private temporary file, retained until the
    reader and its metadata are released. Opening then uses selective metadata
    reads; cell values are decoded only when exported. No archive member paths
    are extracted. A corrupt ZIP or a failed CRC check raises ``ValueError``.
    """
    if member is not None and not isinstance(member, str):
        raise TypeError("member must be a string or None")
    if not isinstance(source, (bytes, bytearray, memoryview)) and hasattr(
        source, "read"
    ):
        source = cast(BinaryIO, source).read()
        if not isinstance(source, (bytes, bytearray, memoryview)):
            if hasattr(source, "__bytes__"):
                source = bytes(source)
            else:
                raise TypeError("file-like source must return bytes")
    if isinstance(source, (str, PathLike)):
        path = fspath(source)
        if not isinstance(path, str):
            raise TypeError("source path must resolve to a string")
        with builtins.open(path, "rb") as stream:
            signature = stream.read(4)
        contents = path
    elif isinstance(source, (bytes, bytearray, memoryview)) or hasattr(
        source, "__bytes__"
    ):
        contents = bytes(source)
        signature = contents[:4]
    else:
        return _native.open(source)
    if signature in {b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"}:
        try:
            with ZipFile(
                contents if isinstance(contents, str) else BytesIO(contents)
            ) as archive:
                return _archive(archive, member)
        except BadZipFile as error:
            raise ValueError(f"invalid ZIP archive: {error}") from error
    if member is not None:
        raise ValueError("member= applies only to ZIP sources")
    return _native.open(contents)
