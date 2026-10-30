"""Typed IVT reader with Markdown hover documentation and Python examples.

```python
import pyivt

ivt = pyivt.open("table.ivt")
print(ivt.metadata.title.en)
table = ivt.to_arrow()
```
"""

from collections.abc import Iterator
from os import PathLike
from typing import Any, BinaryIO

import pyarrow as pa

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
    member ID order: position 0 describes member ID 1. Labels may retain leading
    spaces used by Beyond 20/20 to express hierarchy. None means unavailable.
    Use Ivt.layout.slugs for the corresponding Arrow column names; display names
    in this object need not match those column names. Instances come from the
    reader and cannot be constructed directly.

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
    def count(self) -> int:
        """Alias for items, the declared number of members on this axis."""
    @property
    def items(self) -> int:
        """Declared number of members on this axis; member IDs run from 1 to items."""
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
    @property
    def ordinal(self) -> list[int] | None:
        """Optional member ordering information. Currently always None."""

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
    @property
    def scope(self) -> str | None:
        """Optional scope of the annotation. Currently always None."""
    @property
    def dimension(self) -> str | None:
        """Optional dimension association. Currently always None."""
    @property
    def member_id(self) -> int | None:
        """Optional member association. Currently always None."""

class MissingValue:
    """A missing-value or status legend entry from IvtMetadata.missing_values.

    The optional numeric code links this entry to Arrow's status_code column.
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
    these lazy properties may raise ValueError if metadata cannot be read or
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
        Raises ValueError if the required metadata cannot be loaded or parsed.
        Position i corresponds to Ivt.layout.counts[i] and Ivt.layout.slugs[i].
        """
    @property
    def geographies(self) -> Geographies:
        """Geography member lists, loaded on first access and cached.

        Returns Geographies for the axis selected by Ivt.layout.geo_dim.
        Unavailable fields are None. Raises ValueError on metadata loading errors.
        """
    @property
    def footnotes(self) -> list[Footnote]:
        """Annotation legend entries, loaded on first access and cached.

        Returns one `Footnote` per legend entry, with a localized `description`,
        or an empty list when no entries are found. Raises `ValueError` on errors.

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
        ValueError on loading errors. See Ivt.to_arrow for status column behavior.
        """
    def _loaded(self, name: str) -> bool:
        """Internal diagnostic: whether a named lazy metadata field is cached."""
    def _io_metrics(self) -> tuple[int, int, int, bool]:
        """Internal: bytes read / read ops during metadata I/O (for benchmarks)."""

class Layout:
    """Read-only decoding layout, obtained from Ivt.layout.

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
        """Unique Arrow member-ID column names; the selected geography axis is 'geo'."""
    @property
    def n_dim(self) -> int:
        """Number of dimensions; equals len(counts) and len(slugs)."""
    @property
    def geo_dim(self) -> int:
        """One-based index of the selected geography axis; may be a fallback axis."""
    @property
    def straddle(self) -> int:
        """One-based index of the axis split across IVT page windows."""
    @property
    def window_count(self) -> int:
        """Number of page windows covering the straddle axis, not the total page count."""

class Ivt:
    """An opened Beyond 20/20 table with metadata and deferred cell decoding.

    Obtain an instance with pyivt.open(source); direct construction is not
    supported. Inspect metadata and layout without exporting the cell grid.
    to_batches() decodes bounded row ranges; to_arrow() caches Arrow batches
    for subsequent exports. No context manager or close() call
    is required. This reader does not modify the source file.

    ```python
    import pyivt

    ivt = pyivt.open("table.ivt")
    title = ivt.metadata.title.en
    table = ivt.to_arrow()
    ```
    """
    @property
    def metadata(self) -> IvtMetadata:
        """Shared metadata object; detailed member and legend fields load on demand."""
    @property
    def layout(self) -> Layout:
        """Snapshot of dimension counts, Arrow column slugs and IVT paging details."""
    def to_batches(
        self, *, batch_size: int = 1_000_000, max_memory_bytes: int = 1_073_741_824
    ) -> Iterator[pa.RecordBatch]:
        """Iterate over bounded batches in file dimension order.

        Member IDs are int32. Text fields use dictionary<int32, utf8>, with
        member dictionaries shared across batches. Batches align to outer-axis
        slabs where possible and never exceed batch_size. The iterator retains
        the source and member maps, without caching decoded batches.
        MemoryError reports an estimate exceeding max_memory_bytes; ValueError
        reports malformed data. Release each batch to keep memory bounded.
        """
    def __arrow_c_stream__(self, requested_schema: Any = None) -> Any:
        """Export an independent Arrow C stream capsule; schema requests raise ValueError."""
    def to_arrow(self, *, max_memory_bytes: int = 2_147_483_648) -> pa.Table:
        """Concatenate bounded batches into a chunked pyarrow.Table.

        One row per dimension-member combination, last dimension fastest.
        Member IDs are int32; text fields, symbol and status use dictionaries.
        Native numeric and date attributes keep int64, float64 and date32.
        value is float64, null whenever status_code (int64) is nonzero.
        footnote_raw is list<uint16> with the original packed references.
        Combinations absent from storage retain numeric zero and status zero.

        Arrow batches are cached and their buffers shared by subsequent calls.
        MemoryError reports an unsafe estimate; use to_batches() to process a
        large cube incrementally. max_memory_bytes bounds the conservative
        allocation estimate, not the consumer's total process memory.
        """

def open(
    source: str | PathLike[str] | bytes | bytearray | memoryview | BinaryIO,
) -> Ivt:
    """Open a Beyond 20/20 IVT source without decoding its cell values.

    ### Parameters

    `source`: a local path (`str` or `PathLike[str]`), complete IVT contents
    (`bytes`, `bytearray`, `memoryview`), or a binary file object. Bytes are
    interpreted as file contents. File objects are read once from their current
    position and are neither rewound nor closed. Objects implementing
    `__bytes__()` are also accepted.

    ### Returns

    An `Ivt` reader exposing `metadata`, `layout`, and `to_arrow()`. Summary
    metadata and layout are parsed immediately; cell decoding waits until
    `to_arrow()`.

    ### Raises

    - `TypeError`: unsupported source, path representation or `read()` result.
    - `OSError`: the source path cannot be opened or read.
    - `ValueError`: invalid or unsupported IVT header, descriptor or layout.

    ### Notes

    Paths use selective metadata reads. Keep the file available and unchanged:
    the first to_arrow() reads its full contents, and lazy metadata properties
    may read additional ranges. In-memory sources are copied when opened, so
    later changes to a bytearray do not affect the reader. This function does
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
    in_memory = pyivt.open(BytesIO(Path("table.ivt").read_bytes()))
    ```
    """
