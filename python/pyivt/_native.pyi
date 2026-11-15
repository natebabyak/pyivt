"""Typed IVT reader with Markdown hover documentation and Python examples.

```python
import pyivt

ivt = pyivt.open("table.ivt")
print(ivt.metadata.title.en)
table = ivt.to_arrow()
members = ivt.to_member_table()
```
"""

from collections.abc import Mapping
from os import PathLike
from typing import BinaryIO, Literal, Protocol, overload

import pyarrow as pa
from typing_extensions import deprecated

from pyivt._types import DimensionType, Frequency

class LocalizedText:
    """English and French versions of a metadata text field.

    Both constructor arguments default to `None`. Translations are read-only:
    use `.en` / `.fr` or `["en"]` / `["fr"]`. A missing translation is `None`,
    with no automatic fallback. Unsupported language keys raise `KeyError`.

    ```python
    text = LocalizedText(en="Population", fr="Population")
    english = text.en
    french = text["fr"]
    preferred = text.fr or text.en
    ```
    """

    @property
    def en(self) -> str | None:
        """English text, or None when unavailable. No automatic translation fallback."""
    @property
    def fr(self) -> str | None:
        """French text, or None when unavailable. No automatic translation fallback."""

    def __init__(self, en: str | None = None, fr: str | None = None) -> None:
        """English and French versions of a metadata text field.

        - `en`: English text, default `None`.
        - `fr`: French text, default `None`.

        ```python
        text = LocalizedText(en="Population", fr="Population")
        print(text.en, text["fr"])
        ```
        """

    def __getitem__(self, language: str) -> str | None:
        """Return the requested translation; unsupported keys raise KeyError."""

class LocalizedLabels:
    """English and French metadata label lists in member ID order.

    Read-only translations support .en / .fr and ["en"] / ["fr"]. Missing
    translations are None; unsupported keys raise KeyError. Lists are copies.

    ```python
    labels = ivt.metadata.dimensions[0].members
    for member_id, label in enumerate(labels.en or [], start=1):
        print(member_id, label)
    ```
    """

    @property
    def en(self) -> list[str] | None:
        """English labels, or None when unavailable."""
    @property
    def fr(self) -> list[str] | None:
        """French labels, or None when unavailable."""

    def __init__(
        self, en: list[str] | None = None, fr: list[str] | None = None
    ) -> None: ...
    def __getitem__(self, language: str) -> list[str] | None:
        """Return the requested labels; unsupported keys raise KeyError."""

class Dimension:
    """One table axis, obtained from IvtMetadata.dimensions in file order.

    All attributes are read-only. Member label lists, when available, follow
    file order. Labels may retain leading
    spaces used by Beyond 20/20 to express hierarchy. None means unavailable.
    Use the slug as the Arrow column name, selection keyword, and member
    lookup key. The geography axis uses `geography`. Other axes use slugs
    derived from their full English dimension titles. `name` and
    `description` hold the full titles from the table notes' variable list
    when the file stores one, and the stored 15-character axis name otherwise.

    ```python
    for dim in ivt.metadata.dimensions:
        print(dim.name.en, dim.type, dim.items)
        if dim.type == "time_series":
            print(dim.members.en)
    ```
    """

    @property
    def name(self) -> LocalizedText:
        """Localized short name; English falls back to the descriptor name."""
    @property
    def slug(self) -> str:
        """Dimension key for Arrow columns and selection."""
    @property
    def items(self) -> int:
        """Declared number of members on this axis; items is the axis size."""
    @property
    def type(self) -> DimensionType:
        """Axis classification: `"geographic"`, `"normal"`, or `"time_series"`.

        ```python
        time_axes = [dim for dim in ivt.metadata.dimensions if dim.type == "time_series"]
        ```
        """
    @property
    def description(self) -> LocalizedText:
        """Localized description; English falls back to name.en when unavailable."""
    @property
    def members(self) -> LocalizedLabels:
        """Localized member labels in member ID order."""
    @property
    def notes(self) -> LocalizedText:
        """Localized dimension notes with normalized whitespace; blank notes are None."""

class Footnote:
    """One annotation legend entry with English and French descriptions.

    Obtain entries from `IvtMetadata.footnotes`, in legend order. Each entry
    contains both translations in `description`; a missing translation is `None`.
    Numbered notes embedded in table or dimension notes are separate from this
    legend. Attributes are read-only; instances cannot be constructed directly.

    ```python
    for note in ivt.metadata.footnotes:
        print(note.symbol, note.description.en, note.description["fr"])
    ```
    """

    @property
    def symbol(self) -> str:
        """Display symbol identifying this annotation in the table legend."""
    @property
    def number(self) -> int:
        """Numeric legend entry code, or 0 if the code is unavailable."""
    @property
    def description(self) -> LocalizedText:
        """Localized annotation description; either translation may be `None`."""

class MissingValue:
    """A missing-value or status legend entry from IvtMetadata.missing_values.

    The optional numeric code is the status code stored in the file's cells.
    The symbol and localized description explain the file-defined status. Attributes
    are read-only; instances cannot be constructed directly.

    ```python
    for missing in ivt.metadata.missing_values:
        print(missing.code, missing.symbol, missing.description.en)
    ```
    """

    @property
    def symbol(self) -> str:
        """File-defined display symbol for this status."""
    @property
    def description(self) -> LocalizedText:
        """English and French descriptions; either translation may be None."""
    @property
    def code(self) -> int | None:
        """Numeric status code used by Arrow output, or None when unavailable."""

class Geographies:
    """Parallel geography member lists from IvtMetadata.geographies.

    Position i in each available list describes member_id[i]. A list may be None
    when the field is unavailable; optional entries within a list may also be
    None. Names, UIDs and quality fields are parsed from English member labels,
    rather than looked up in an external geography database. Attributes are
    read-only; instances cannot be constructed directly.
    """

    @property
    def member_id(self) -> list[int]:
        """One-based geography member IDs in file order."""
    @property
    def geo_label(self) -> LocalizedLabels:
        """Original geography labels in English and French."""
    @property
    def geo_name(self) -> list[str | None] | None:
        """Names parsed from English labels, or None when labels are unavailable."""
    @property
    def geo_uid(self) -> list[str | None] | None:
        """Geographic identifiers parsed from labels; strings preserve leading zeros."""
    @property
    def dqf_code(self) -> list[str | None] | None:
        """Data quality flag codes parsed from labels; unavailable fields are None."""
    @property
    def tnr_short_form(self) -> list[str | None] | None:
        """Short-form total non-response rates as strings with decimal points, or None."""

class IvtMetadata:
    """Read-only table metadata, obtained from Ivt.metadata.

    Summary properties are available after open(). Dimensions, geographies,
    footnotes and missing_values are loaded on first access and cached. Accessing
    these lazy properties may raise pyivt.IvtError if metadata cannot be read or
    parsed. Returned lists and nested objects are snapshots; modifying a Python
    list does not update the reader or source file. Optional fields use None
    when unavailable. Instances cannot be constructed directly.

    ```python
    import pyivt

    meta = pyivt.open("table.ivt").metadata
    title = meta.title.en or meta.title.fr
    axes = [(dim.name.en, dim.items) for dim in meta.dimensions]
    ```
    """
    @property
    def product_id(self) -> str | None:
        """Product identifier extracted from table identity or notes, or None."""
    @property
    def title(self) -> LocalizedText:
        """Table title in English and French; either translation may be None."""
    @property
    def universe(self) -> str | None:
        """Population or universe described by the table, or None when unavailable."""
    @property
    def notes(self) -> LocalizedText:
        """Table notes with normalized line endings and whitespace; blank notes are None."""
    @property
    def version(self) -> str:
        """IVT format version from the file header, not the pyivt package version."""
    @property
    def signature(self) -> str:
        """Four-byte file signature as eight lowercase hexadecimal digits."""
    @property
    def author(self) -> LocalizedText:
        """Author or producing organization; blank or missing translations are `None`."""
    @property
    def category(self) -> LocalizedText:
        """File-defined category; blank or missing translations are `None`."""
    @property
    def keywords(self) -> LocalizedText:
        """Keyword text in each language, or `None` when absent or blank.

        Multiple stored keyword strings are joined with `", "` in file order.
        No translation fallback is applied.

        ```python
        keywords = ivt.metadata.keywords.en
        terms = [term.strip() for term in keywords.split(",")] if keywords else []
        ```
        """
    @property
    def number_of_datapoints(self) -> int | None:
        """Product of declared dimension counts, or None when unavailable.

        Includes all member combinations, rather than counting only numeric or
        non-missing observations. The summary product saturates at 2**63 - 1.
        """
    @property
    def frequency(self) -> Frequency | None:
        """Time-series frequency: monthly, quarterly, semi_annual, or annual.

        None when the table is not a time series or its rate is unknown.
        """
    @property
    def start_date(self) -> str | None:
        """Time-series start date as an ISO date string, or None when unavailable."""
    @property
    def end_date(self) -> str | None:
        """Time-series end date as an ISO date string, or None when unavailable."""
    @property
    def read_only(self) -> bool | None:
        """File's stored read-only flag, or None; does not report filesystem permissions."""
    @property
    def dimensions(self) -> list[Dimension]:
        """Dimensions in file order, loaded on first access and cached.

        Returns a list of Dimension snapshots with member labels and notes.
        Raises pyivt.IvtError if the required metadata cannot be loaded or parsed.
        """
    @property
    def geographies(self) -> Geographies:
        """Geography member lists, loaded on first access and cached.

        Returns Geographies for the axis selected by the geographic dimension.
        Unavailable fields are None. Raises pyivt.IvtError on metadata loading errors.
        """
    @property
    def footnotes(self) -> list[Footnote]:
        """Annotation legend entries, loaded on first access and cached.

        Returns one `Footnote` per legend entry, with a localized `description`,
        or an empty list when no entries are found. Raises `pyivt.IvtError` on errors.

        ```python
        for note in ivt.metadata.footnotes:
            print(note.symbol, note.description.en, note.description.fr)
        ```
        """
    @property
    def missing_values(self) -> list[MissingValue]:
        """Status legend entries, loaded on first access and cached.

        Returns MissingValue entries linking numeric codes to symbols and
        localized descriptions, or an empty list if none are found. Raises
        pyivt.IvtError on loading errors. Per-cell statuses are not exported yet.
        """

class _Layout:
    """Private read-only decoding layout, obtained from Ivt._layout.

    counts and slugs follow file dimension order and describe the exported grid.
    geo_dim and straddle are one-based dimension indices; subtract 1 before
    indexing a Python list. Paging fields describe IVT storage, not Arrow record
    batches. List attributes are copies. Instances cannot be constructed directly.
    """

    @property
    def counts(self) -> list[int]:
        """Member counts in file dimension order; their product is the Arrow row count."""
    @property
    def slugs(self) -> list[str]:
        """Unique dimension slugs for Arrow columns and member selection."""
    @property
    def n_dim(self) -> int:
        """Number of dimensions; equals len(counts) and len(slugs)."""
    @property
    def geo_dim(self) -> int | None:
        """One-based geography axis from the native kind or UID field, or None when absent."""
    @property
    def straddle(self) -> int:
        """One-based index of the axis split across IVT page windows."""
    @property
    def window_count(self) -> int:
        """Number of page windows covering the straddle axis, not the total page count."""

class Ivt:
    """An opened Beyond 20/20 table with metadata and deferred cell decoding.

    Open a reader with pyivt.open(source). Metadata and member lookups are
    available without decoding cells. Each to_arrow() call builds a new
    table that the reader does not retain, so dropping the table releases
    its buffers. iter_batches() streams the same table in bounded memory.
    The reader does not modify the source file.

    ```python
    import pyivt

    ivt = pyivt.open("table.ivt")
    title = ivt.metadata.title.en
    table = ivt.to_arrow()
    members = ivt.to_member_table()
    ```
    """
    @property
    def metadata(self) -> IvtMetadata:
        """Shared metadata object; detailed member and legend fields load on demand."""
    @overload
    def to_arrow(
        self,
        /,
        *,
        select: Mapping[str, object] | None = None,
        max_memory_bytes: int = 2_147_483_648,
    ) -> pa.Table:
        """Return 1-based member positions and nullable float64 values as a table.

        `select` maps dimension slugs to member codes, which are 1-based
        positions in file order. An integer fixes a dimension and drops its
        column. A list of integers keeps the column and limits its rows.
        Omitted dimensions include all members. An empty list gives zero rows.
        Invalid slugs and codes raise ValueError before decoding.

        Each call decodes the selection into new buffers. The reader keeps no
        reference to the result, so dropping it releases its memory; repeated
        identical calls return equal but independent tables. A reader opened
        from a path reads the file for each export and does not keep it.

        The result plus one copy of the file must fit `max_memory_bytes`
        (default 2 GiB). Larger selections raise pyivt.ResourceLimitError:
        raise the limit, or stream them with iter_batches(). A limit that is
        not positive raises ValueError.

        ```python
        table = ivt.to_arrow(select={"geography": 1, "census_year": [1, 2]})
        ```
        """
    @overload
    @deprecated("Pass dimensions as select={...} instead of keyword arguments.")
    def to_arrow(
        self, /, *, max_memory_bytes: int = 2_147_483_648, **dims: object
    ) -> pa.Table:
        """Deprecated: pass dimensions as `select={...}`."""
    @overload
    def iter_batches(
        self,
        /,
        *,
        select: Mapping[str, object] | None = None,
        batch_size: int = 1_048_576,
        max_memory_bytes: int = 2_147_483_648,
    ) -> pa.RecordBatchReader:
        """Stream the to_arrow() table as record batches of at most batch_size rows.

        Takes the same `select` as to_arrow(). Batches arrive in file order
        and share to_arrow()'s schema and metadata, so concatenating them
        equals to_arrow(select=...). Each batch is decoded when the consumer
        asks for it, without holding the GIL. Memory stays near one copy of
        the file plus about two batches, whatever the total row count.

        The reader implements the Arrow C stream protocol, so pyarrow,
        polars.DataFrame(reader), DuckDB and other consumers read it without
        copying. A stream can be read once. It keeps its own reference to
        the file contents, so it outlives the Ivt and later changes to the
        path. A corrupt page raises when its batch is read and ends the
        stream. batch_size must be positive. One copy of the file plus two
        batches must fit `max_memory_bytes` (default 2 GiB), or
        pyivt.ResourceLimitError is raised.

        ```python
        with ivt.iter_batches(select={"geography": [1, 2]}) as reader:
            for batch in reader:
                ...
        frame = polars.DataFrame(ivt.iter_batches())
        ```
        """
    @overload
    @deprecated("Pass dimensions as select={...} instead of keyword arguments.")
    def iter_batches(
        self,
        /,
        *,
        batch_size: int = 1_048_576,
        max_memory_bytes: int = 2_147_483_648,
        **dims: object,
    ) -> pa.RecordBatchReader:
        """Deprecated: pass dimensions as `select={...}`."""
    @overload
    def to_member_table(
        self,
        /,
        *,
        select: Mapping[str, object] | None = None,
        language: Literal["en", "fr"] = "en",
    ) -> pa.Table:
        """Return one Arrow table with a dictionary-encoded dimension column first.

        The same `select` picks the same codes as to_arrow(). Each row
        identifies a retained dimension by slug and a member by its unsigned
        1-based code. The label is nullable and uses the requested language.
        Native attributes are null where absent. When present, the native
        Code is a separate text column. No cells are decoded.
        """
    @overload
    @deprecated("Pass dimensions as select={...} instead of keyword arguments.")
    def to_member_table(
        self, /, *, language: Literal["en", "fr"] = "en", **dims: object
    ) -> pa.Table:
        """Deprecated: pass dimensions as `select={...}`."""

class _IvtInternals(Protocol):  # noqa: PYI046 (used by pyivt._export)
    """Private hooks on Ivt for pyivt's own modules and tests.

    They exist at runtime but are not part of the public Ivt type. Reach them
    with ``typing.cast("_IvtInternals", ivt)``.
    """

    @property
    def _layout(self) -> _Layout:
        """Snapshot of dimension counts, Arrow column slugs and IVT paging details."""
    def _member_fields(
        self,
    ) -> tuple[
        str,
        list[list[tuple[str, str, list[str | int | float | None]]]],
        list[tuple[bool, tuple[str | None, str | None], tuple[int | None, int | None]]],
    ]: ...

__version__: str
"""Package version the extension was built with (from Cargo.toml)."""

def _open_owned(
    source: str | PathLike[str] | bytes | bytearray | memoryview,
    owner: object,
    /,
) -> Ivt:
    """Open source and keep owner alive as long as the reader's metadata."""

def open(
    source: str | PathLike[str] | bytes | bytearray | memoryview | BinaryIO,
    /,
) -> Ivt:
    """Open a Beyond 20/20 IVT source without decoding its cell values.

    ### Parameters

    `source`: a local path (`str` or `PathLike[str]`), complete IVT contents
    (`bytes`, `bytearray`, `memoryview`), or a binary file object. Bytes are
    interpreted as file contents. File objects are read once from their current
    position and are neither rewound nor closed. Objects implementing
    `__bytes__()` are also accepted.

    ### Returns

    An `Ivt` reader exposing `metadata`, `to_member_table()`, `to_arrow()` and
    `iter_batches()`. Summary
    metadata is parsed immediately. Cell decoding happens during
    export.

    ### Raises

    - `TypeError`: unsupported source, path representation or `read()` result.
    - `OSError`: the source path cannot be opened or read.
    - `pyivt.CorruptFileError`: bad signature or header, truncation, or
      offsets outside the file. Messages start with `Corrupt IVT file:`.
    - `pyivt.UnsupportedFormatError`: a valid file in a container version,
      descriptor or layout pyivt cannot read. Messages start with
      `Unsupported IVT file:`.
    - `pyivt.ResourceLimitError`: a memory estimate exceeds a safety limit.
      Also a `MemoryError`.

    All three subclass `pyivt.IvtError`, itself a `ValueError`, and carry
    `offset`, `length`, `file_len`, `slot`, `entry` and `dimension` attributes
    (`None` when unknown). Low-level details follow `Details:`.

    ### Notes

    Paths use selective metadata reads. Keep the file available and unchanged:
    the first export reads its full contents. Lazy metadata properties
    may read additional ranges. Immutable `bytes` are shared with the reader,
    not copied. Mutable contents such as a bytearray are copied once when
    opened, so later changes do not affect the reader. This function does
    not fetch URLs or decompress archives. Exceptions raised by a supplied
    read(), __fspath__() or __bytes__() method may also propagate.

    ### Examples

    ```python
    from io import BytesIO
    from pathlib import Path
    import pyivt

    ivt = pyivt.open(Path("table.ivt"))
    title = ivt.metadata.title.en or ivt.metadata.title.fr
    table = ivt.to_arrow()
    members = ivt.to_member_table()
    in_memory = pyivt.open(BytesIO(Path("table.ivt").read_bytes()))
    ```
    """
