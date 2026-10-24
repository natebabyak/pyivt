"""Type stubs for the pyivt._native extension module."""

from os import PathLike
from typing import BinaryIO

import pyarrow as pa

class LocalizedText:
    """English and French versions of a metadata text field.

    Parameters
    ----------
    en : str or None, optional
        English text. Defaults to None.
    fr : str or None, optional
        French text. Defaults to None.

    Notes
    -----
    Attributes are read-only. A missing translation is None; accessing one
    language does not fall back to the other language.
    Translations support .en / .fr and ["en"] / ["fr"]. Other keys raise KeyError.

    Examples
    --------
    >>> text = LocalizedText(en="Population", fr="Population")
    >>> text.en
    'Population'
    """

    en: str | None
    """English text, or None when unavailable. No automatic translation fallback."""
    fr: str | None
    """French text, or None when unavailable. No automatic translation fallback."""

    def __init__(self, en: str | None = None, fr: str | None = None) -> None:
        """English and French versions of a metadata text field.

        Parameters
        ----------
        en : str or None, optional
            English text. Defaults to None.
        fr : str or None, optional
            French text. Defaults to None.

        Notes
        -----
        Attributes are read-only. A missing translation is None; accessing one
        language does not fall back to the other language.

        Examples
        --------
        >>> text = LocalizedText(en="Population", fr="Population")
        >>> text.en
        'Population'
        """

    def __getitem__(self, language: str) -> str | None:
        """Return the requested translation; unsupported keys raise KeyError."""

class LocalizedLabels:
    """English and French metadata label lists in member ID order.

    Read-only translations support .en / .fr and ["en"] / ["fr"]. Missing
    translations are None; unsupported keys raise KeyError. Lists are copies.
    """

    en: list[str] | None
    """English labels, or None when unavailable."""
    fr: list[str] | None
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
    """

    name: LocalizedText
    """Localized short name; English falls back to the descriptor name."""
    count: int
    """Declared number of members on this axis; member IDs run from 1 to count."""
    type: int
    """Raw IVT dimension type code. See type_label for a readable classification."""
    description: LocalizedText
    """Localized description; English falls back to name.en when unavailable."""
    type_label: str
    """Readable classification: 'Normal', 'Geographic', or 'Time Series'."""
    members: LocalizedLabels
    """Localized member labels in member ID order."""
    notes: LocalizedText
    """Localized dimension notes with normalized whitespace."""
    ordinal: list[int] | None
    """Optional member ordering information. Currently always None."""

class Footnote:
    """One language of an entry in the table's annotation legend.

    Obtain entries from IvtMetadata.footnotes. English and French descriptions
    are separate objects sharing a symbol. Numbered notes embedded in table or
    dimension notes are not extracted into this legend. Attributes are read-only;
    instances cannot be constructed directly.
    """

    symbol: str
    """Display symbol identifying this annotation in the table legend."""
    language: str
    """Description language: 'en' or 'fr'."""
    number: int
    """Numeric legend entry code, or 0 if the code is unavailable."""
    text: str
    """Annotation description in the language given by language."""
    scope: str | None
    """Optional scope of the annotation. Currently always None."""
    dimension: str | None
    """Optional dimension association. Currently always None."""
    member_id: int | None
    """Optional member association. Currently always None."""

class MissingValue:
    """A missing-value or status legend entry from IvtMetadata.missing_values.

    The optional numeric code links this entry to Arrow's status_code column.
    The symbol and localized label explain the file-defined status. Attributes
    are read-only; instances cannot be constructed directly.
    """

    symbol: str
    """File-defined display symbol for this status."""
    label: LocalizedText
    """English and French descriptions; either translation may be None."""
    code: int | None
    """Numeric status code used by Arrow output, or None when unavailable."""

class Geographies:
    """Parallel geography member lists from IvtMetadata.geographies.

    Position i in each available list describes member_id[i]. A list may be None
    when the field is unavailable; optional entries within a list may also be
    None. Names, UIDs and quality fields are parsed from English member labels,
    rather than looked up in an external geography database. Attributes are
    read-only; instances cannot be constructed directly.
    """

    member_id: list[int]
    """One-based geography member IDs in file order."""
    geo_label: LocalizedLabels
    """Original geography labels in English and French."""
    geo_name: list[str | None] | None
    """Names parsed from English labels, or None when labels are unavailable."""
    geo_uid: list[str | None] | None
    """Geographic identifiers parsed from labels; strings preserve leading zeros."""
    dqf_code: list[str | None] | None
    """Data quality flag codes parsed from labels; unavailable fields are None."""
    tnr_short_form: list[str | None] | None
    """Short-form total non-response rates as strings with decimal points, or None."""

class IvtMetadata:
    """Read-only table metadata, obtained from Ivt.metadata.

    Summary properties are available after open(). Dimensions, geographies,
    footnotes and missing_values are loaded on first access and cached. Accessing
    these lazy properties may raise ValueError if metadata cannot be read or
    parsed. Returned lists and nested objects are snapshots; modifying a Python
    list does not update the reader or source file. Optional fields use None
    when unavailable. Instances cannot be constructed directly.

    Examples
    --------
    >>> meta = open("table.ivt").metadata
    >>> title = meta.title.en or meta.title.fr
    >>> axes = [(dim.name.en, dim.count) for dim in meta.dimensions]
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
        """Table notes in both languages, with line endings and whitespace normalized."""
    @property
    def version(self) -> str:
        """IVT format version from the file header, not the pyivt package version."""
    @property
    def signature(self) -> str:
        """Four-byte file signature as eight lowercase hexadecimal digits."""
    @property
    def author(self) -> LocalizedText:
        """Author or producing organization in each available language."""
    @property
    def category(self) -> LocalizedText:
        """File-defined category text in each available language."""
    @property
    def keywords(self) -> LocalizedText:
        """Keyword text in each language, returned as strings rather than split lists."""
    @property
    def number_of_datapoints(self) -> int | None:
        """Product of declared dimension counts, or None when unavailable.

        Includes all member combinations, rather than counting only numeric or
        non-missing observations. The summary product saturates at 2**63 - 1.
        """
    @property
    def frequency(self) -> str | None:
        """'No', 'Annual', 'Semi-annual', 'Quarterly', or 'Monthly'; None if unknown.

        'No' is the file's explicit non-time-series frequency, distinct from None.
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

        Returns separate Footnote entries for each available language, or an
        empty list when no entries are found. Raises ValueError on loading errors.
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

    counts: list[int]
    """Member counts in file dimension order; their product is the Arrow row count."""
    slugs: list[str]
    """Unique Arrow member-ID column names; the selected geography axis is 'geo'."""
    n_dim: int
    """Number of dimensions; equals len(counts) and len(slugs)."""
    geo_dim: int
    """One-based index of the selected geography axis; may be a fallback axis."""
    straddle: int
    """One-based index of the axis split across IVT page windows."""
    window_count: int
    """Number of page windows covering the straddle axis, not the total page count."""

class Ivt:
    """An opened Beyond 20/20 table with metadata and deferred cell decoding.

    Obtain an instance with pyivt.open(source); direct construction is not
    supported. Inspect metadata and layout without exporting the cell grid.
    to_arrow() reads and decodes the complete table on first use, then caches
    decoded columns for subsequent exports. No context manager or close() call
    is required. This reader does not modify the source file.

    Examples
    --------
    >>> import pyivt
    >>> ivt = pyivt.open("table.ivt")
    >>> title = ivt.metadata.title.en
    >>> table = ivt.to_arrow()
    """
    @property
    def metadata(self) -> IvtMetadata:
        """Shared metadata object; detailed member and legend fields load on demand."""
    @property
    def layout(self) -> Layout:
        """Snapshot of dimension counts, Arrow column slugs and IVT paging details."""
    def to_arrow(self) -> pa.Table:
        """Export the complete table as a pyarrow.Table in long form.

        Returns
        -------
        pyarrow.Table
            One row per combination of dimension members, including combinations
            absent from storage. Row count is math.prod(self.layout.counts).
            Rows follow file dimension order, with the last dimension varying
            fastest. Each dimension has an int64 column named by layout.slugs
            containing one-based member IDs. File-defined member fields use
            '<slug>_<field>' names with collision suffixes where needed; types
            are string, int64, float64 or date32 according to the file.
            Additional columns are:

        * value (float64): numeric value when status_code is 0, otherwise null.
        * status_code (int64): decoded status; 0 denotes a numeric cell.
        * symbol (string): matching status legend symbol, or null.
        * status (string): legend description, preferring English then French,
          or null if no description is available.
        * footnote_raw (list<uint16>): raw cell annotation markers; an empty
          list means no markers. These are not resolved footnote texts.

        Raises
        ------
        OSError
            The path cannot be read when the full file is first needed.
        ValueError
            Decoding, member metadata or dimension size validation fails.
        ImportError
            pyarrow is unavailable.

        Notes
        -----
        This materializes the entire grid in memory; there is no streaming or
        filtering option. Combinations absent from storage have value 0 and
        status_code 0. Nonzero statuses produce null values even if a numeric
        payload exists. Source precision and label indentation are preserved.
        First export caches the file bytes and decoded columns. Later calls
        construct another Arrow table from cached columns without re-decoding.

        Examples
        --------
        >>> import pyarrow.parquet as pq
        >>> table = open("table.ivt").to_arrow()
        >>> pq.write_table(table, "table.parquet")
        """

def open(
    source: str | PathLike[str] | bytes | bytearray | memoryview | BinaryIO,
) -> Ivt:
    """Open a Beyond 20/20 IVT source without decoding its cell values.

    Parameters
    ----------
    source : str, os.PathLike[str], bytes, bytearray, memoryview or binary file object
        A local file path, complete IVT contents, or an object with read().
        Bytes are interpreted as file contents, never as a path. Path-like
        objects must return str from __fspath__(). File objects are read once
        with read() from their current position; they are not rewound or closed.
        The result of read() must be bytes or support conversion via __bytes__().
        Other objects implementing __bytes__() are also accepted.

    Returns
    -------
    Ivt
        Reader exposing metadata, layout and to_arrow(). Summary metadata and
        layout are parsed immediately; cell decoding waits until to_arrow().

    Raises
    ------
    TypeError
        The source type, path representation or read() result is unsupported.
    OSError
        The source path cannot be opened or read.
    ValueError
        The IVT header, descriptor or layout is invalid or unsupported.

    Notes
    -----
    Paths use selective metadata reads. Keep the file available and unchanged:
    the first to_arrow() reads its full contents, and lazy metadata properties
    may read additional ranges. In-memory sources are copied when opened, so
    later changes to a bytearray do not affect the reader. This function does
    not fetch URLs or decompress archives. Exceptions raised by a supplied
    read(), __fspath__() or __bytes__() method may also propagate.

    Examples
    --------
    >>> from pathlib import Path
    >>> import pyivt
    >>> ivt = pyivt.open(Path("table.ivt"))
    >>> title = ivt.metadata.title.en or ivt.metadata.title.fr
    >>> table = ivt.to_arrow()
    >>> from io import BytesIO
    >>> in_memory = pyivt.open(BytesIO(Path("table.ivt").read_bytes()))
    """
