"""Type stubs for the pyivt._native extension module."""

from os import PathLike
from typing import BinaryIO

import pyarrow as pa

class LocalizedText:
    en: str | None
    fr: str | None

    def __init__(self, en: str | None = None, fr: str | None = None) -> None: ...

class TimeSeries:
    frequency: str | None
    start: str | None
    end: str | None

    def __init__(
        self,
        frequency: str | None = None,
        start: str | None = None,
        end: str | None = None,
    ) -> None: ...

class Dimension:
    name: str
    name_fr: str | None
    count: int
    type: int
    is_geography: bool
    description: str | None
    description_fr: str | None
    type_label: str
    members: list[str] | None
    members_fr: list[str] | None
    ordinal: list[int] | None

class Footnote:
    language: str
    number: int
    text: str
    scope: str | None
    dimension: str | None
    member_id: int | None

class MissingValue:
    symbol: str
    label: LocalizedText
    code: int | None

class Geographies:
    member_id: list[int]
    geo_label: list[str] | None
    geo_name: list[str | None] | None
    geo_uid: list[str | None] | None
    geo_label_fr: list[str] | None
    dqf_code: list[str | None] | None
    tnr_short_form: list[str | None] | None

class IvtMetadata:
    @property
    def product_id(self) -> str | None: ...
    @property
    def title(self) -> LocalizedText: ...
    @property
    def universe(self) -> str | None: ...
    @property
    def notes(self) -> LocalizedText: ...
    @property
    def version(self) -> str: ...
    @property
    def signature(self) -> str: ...
    @property
    def author(self) -> LocalizedText: ...
    @property
    def category(self) -> LocalizedText: ...
    @property
    def keywords(self) -> LocalizedText: ...
    @property
    def number_of_datapoints(self) -> int | None: ...
    @property
    def time_series(self) -> TimeSeries | None: ...
    @property
    def read_only(self) -> bool | None: ...
    @property
    def dimensions(self) -> list[Dimension]: ...
    @property
    def geographies(self) -> Geographies: ...
    @property
    def footnotes(self) -> list[Footnote]: ...
    @property
    def missing_values(self) -> list[MissingValue]: ...
    def _loaded(self, name: str) -> bool: ...

class Layout:
    counts: list[int]
    slugs: list[str]
    n_dim: int
    geo_dim: int
    straddle: int
    window_count: int

class Ivt:
    @property
    def metadata(self) -> IvtMetadata: ...
    @property
    def layout(self) -> Layout: ...
    def to_arrow(
        self,
        *,
        complete: bool = True,
        max_cells: int = 100_000_000,
        labels: bool = True,
        language: str = "en",
    ) -> pa.Table: ...

def open(
    source: str | PathLike[str] | bytes | bytearray | memoryview | BinaryIO,
) -> Ivt: ...
