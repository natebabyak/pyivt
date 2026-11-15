"""Exception hierarchy raised when an IVT file cannot be read.

Every class derives from ``IvtError``, which subclasses ``ValueError``, so
existing ``except ValueError`` handlers keep working. Structured fields are
``None`` when the reader does not know them.
"""

from __future__ import annotations

__all__ = [
    "CorruptFileError",
    "IvtError",
    "ResourceLimitError",
    "UnsupportedFormatError",
]


class IvtError(ValueError):
    """Base class for pyivt read errors.

    ``str(error)`` is the human-readable message. Location fields:

    - ``offset``: byte offset of the failing read, record, or header word.
    - ``length``: byte length of that read or record.
    - ``file_len``: size of the source file in bytes.
    - ``slot``: header slot offset of the directory involved (e.g. 544).
    - ``entry``: directory entry (record ordinal) involved.
    - ``dimension``: zero-based axis index involved.
    """

    offset: int | None
    length: int | None
    file_len: int | None
    slot: int | None
    entry: int | None
    dimension: int | None

    def __init__(
        self,
        message: str,
        *,
        offset: int | None = None,
        length: int | None = None,
        file_len: int | None = None,
        slot: int | None = None,
        entry: int | None = None,
        dimension: int | None = None,
    ) -> None:
        super().__init__(message)
        self.offset = offset
        self.length = length
        self.file_len = file_len
        self.slot = slot
        self.entry = entry
        self.dimension = dimension


class CorruptFileError(IvtError):
    """The file is damaged: truncated, a bad header, or a record or directory
    that points outside the file or cannot be decoded."""


class UnsupportedFormatError(IvtError):
    """The file looks like an IVT file but uses a container version,
    descriptor, or record layout this reader does not recognise."""


class ResourceLimitError(IvtError, MemoryError):
    """A memory estimate or allocation exceeded a safety limit.

    Also a ``MemoryError``. ``estimate`` and ``limit`` are in bytes when known.
    """

    estimate: int | None
    limit: int | None

    def __init__(
        self,
        message: str,
        *,
        estimate: int | None = None,
        limit: int | None = None,
        **location: int | None,
    ) -> None:
        super().__init__(message, **location)
        self.estimate = estimate
        self.limit = limit


for _cls in (IvtError, CorruptFileError, UnsupportedFormatError, ResourceLimitError):
    _cls.__module__ = "pyivt"
del _cls
